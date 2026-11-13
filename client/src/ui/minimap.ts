// The minimap and the explored map (docs/adr/0068-minimap-and-explored-map.md; owner,
// 2026-11-08). The client crate draws the round minimap (`minimapFrame`: the heading up, a texel a
// metre, drawn again only on a move or a turn) and reports the explored cells it covers
// (`takeExplored`); this shell shows the disc with the player's and the others' dots, keeps the
// explored cells per world in this browser (IndexedDB, append-only), and draws them on the big map
// (M, or a tap on the minimap): five fixed zoom levels from the minimap's area to the whole world disc
// (owner, 2026-11-09), pannable, unexplored ground fogged, the player's place and heading on it.
// Each level's ground is its own tiles of its own cell size, generated from the terrain generator
// in the job workers once per world and build, in the background, and kept in this browser
// (`RevealCache`, IndexedDB `dwell-map-tiles`); explored cells (the loaded terrain's, edits too)
// draw over the levels of their size and coarser. In a mode with the `mapRevealed` ability
// (creative) nothing is fogged. Its looks, levels and sizes are data (shared/ui/map.json →
// map.gen.ts).

import {
  MAP_LAYERS,
  type MapLayerDef,
  layerPixels,
  layerSpan,
  nextLayer,
  parseLayerGrid,
} from './mapLayers';
import { GAMEPAD } from '../input/gamepad.gen';
import { MAP } from './map.gen';
import { compatibilityLine } from '../version/semver';
import { World } from '../protocol/constants.gen';
import { ExploredChanges, type ExploredStore } from './exploredStore';
import {
  MarkerList,
  type MapMarker,
  decodeMarkers,
  drawIcon,
  encodeMarkers,
  formatMarkerDistance,
  iconSvg,
  markersInRange,
  pickMarker,
  waypointAlpha,
  waypointOnScreen,
} from './markers';
import { MARKER_WORLD } from './markers.gen';
import { MarkerPicker } from './markerPicker';

/** An explored cell's key: its face (0 A, 1 B) and its index along face-local x and z. */
export function cellKey(face: number, i: number, k: number): string {
  return `${String(face)},${String(i)},${String(k)}`;
}

/** The explored cells of one world: key → 0xRRGGBB. */
export type ExploredCells = Map<string, number>;

/**
 * The explored cells' heights: key → height code (`dwell_game::minimap::height_code`: half-metres
 * times two, plus 1 for water), from the same source as the cell's colour; absent where none.
 */
export type ExploredHeights = Map<string, number>;

/** No height (`dwell_game::minimap::NO_HEIGHT`). */
export const NO_HEIGHT = -2147483648;

/** The fields of `takeExplored`'s flat list a cell: [face, i, k, rgb, height code]. */
export const EXPLORED_STRIDE = 5;

/**
 * Adds `takeExplored`'s flat [face, i, k, rgb, height code]* to the cells and their heights;
 * whether any changed. The keys of the changed cells go to `changed` when given.
 */
export function addExplored(
  cells: ExploredCells,
  flat: ArrayLike<number>,
  heights: ExploredHeights = new Map(),
  changed?: string[],
): boolean {
  let any = false;
  for (let n = 0; n + EXPLORED_STRIDE - 1 < flat.length; n += EXPLORED_STRIDE) {
    const key = cellKey(flat[n] ?? 0, flat[n + 1] ?? 0, flat[n + 2] ?? 0);
    const rgb = (flat[n + 3] ?? 0) & 0xffffff;
    const code = flat[n + 4] ?? NO_HEIGHT;
    const had = heights.get(key) ?? NO_HEIGHT;
    if (cells.get(key) !== rgb || had !== code) {
      cells.set(key, rgb);
      if (code === NO_HEIGHT) heights.delete(key);
      else heights.set(key, code);
      changed?.push(key);
      any = true;
    }
  }
  return any;
}

/** The stored record's heights (`heights`: key → height code), for this build's cell size. */
export function parseHeights(stored: unknown): ExploredHeights {
  const out: ExploredHeights = new Map();
  if (typeof stored !== 'object' || stored === null) return out;
  const r = stored as Record<string, unknown>;
  if (r.cellM !== MAP.explored.cellM) return out;
  const h = r.heights;
  if (typeof h !== 'object' || h === null || Array.isArray(h)) return out;
  for (const [k, v] of Object.entries(h as Record<string, unknown>)) {
    if (/^[01],-?\d+,-?\d+$/.test(k) && typeof v === 'number' && Number.isInteger(v)) {
      out.set(k, v);
    }
  }
  return out;
}

/** The stored record as cells: those of this build's cell size (another size's are kept, unread). */
export function parseExplored(stored: unknown): ExploredCells {
  const cells: ExploredCells = new Map();
  if (typeof stored !== 'object' || stored === null) return cells;
  const r = stored as Record<string, unknown>;
  if (r.cellM !== MAP.explored.cellM) return cells;
  const c = r.cells;
  if (typeof c !== 'object' || c === null || Array.isArray(c)) return cells;
  for (const [k, v] of Object.entries(c as Record<string, unknown>)) {
    if (/^[01],-?\d+,-?\d+$/.test(k) && typeof v === 'number' && Number.isInteger(v)) {
      cells.set(k, v & 0xffffff);
    }
  }
  return cells;
}

export {
  EXPLORED_FORMAT,
  ExploredChanges,
  exploredRecord,
  indexedDbExplored,
  type ExploredStore,
} from './exploredStore';

/**
 * The big map's view: the face-local point at its centre (m), its zoom (pixels a metre) and its
 * face. Screen x is the right of a player facing yaw 0 (+Z) on that face (face B's mirrored, as the
 * minimap's), screen y down.
 */
export interface BigView {
  face: number;
  cx: number;
  cz: number;
  /** The zoom level (`bigMap.levels`, 0 the closest). */
  level: number;
  pxPerM: number;
}

/** A zoom level: its cell side and span (m), and where its tiles are built. */
export interface Level {
  cellM: number;
  spanM: number;
  reach: Reach;
}

/**
 * The zoom levels, closest first (shared/ui/map.json): each its cell side and span (m). A bigger
 * world's (BIG_MAP.md M2b) are its size's (`setMapWorldSize`), its coarse cells and spans wider.
 */
export let LEVELS: readonly Level[] = MAP.bigMap.levels;

/** The world's size (BIG_MAP.md M2b): its radius scale, 1 today's. */
let worldScale = 1;

/**
 * The map follows the world's size (BIG_MAP.md M2b): its disc's and island's radii × `radiusScale`
 * and its levels' cells and spans `levels` ([cellM, spanM, …], the client's `bigMapLevels`,
 * `MapTunables::big_map_levels_sized`); each level's reach stays the data's. Today's size, or
 * levels that don't match the data's count, restore map.json's.
 */
export function setMapWorldSize(radiusScale: number, levels: ArrayLike<number>): void {
  const data = MAP.bigMap.levels;
  const ok =
    Number.isInteger(radiusScale) &&
    radiusScale > 1 &&
    levels.length === 2 * data.length &&
    Array.from(levels).every((v) => Number.isFinite(v) && v > 0);
  worldScale = ok ? radiusScale : 1;
  LEVELS = ok
    ? data.map((l, i) => ({
        ...l,
        cellM: levels[2 * i] ?? l.cellM,
        spanM: levels[2 * i + 1] ?? l.spanM,
      }))
    : data;
}

/**
 * Where the background builds a level's tiles (`bigMap.levels[].reach`; owner, 2026-11-10): round
 * the player, the whole island, or the whole world disc.
 */
export type Reach = 'player' | 'island' | 'disc';

/** `level` within the levels. */
export function clampLevel(level: number): number {
  return Math.max(0, Math.min(LEVELS.length - 1, Math.round(level)));
}

/** A zoom level's cell side (m). */
export function levelCellM(level: number): number {
  return LEVELS[clampLevel(level)]?.cellM ?? MAP.explored.cellM;
}

/** Level `level`'s zoom on a `w` × `h` map (pixels a metre): its span across the short side. */
export function levelPxPerM(level: number, w: number, h: number): number {
  return Math.max(1, Math.min(w, h)) / (LEVELS[clampLevel(level)]?.spanM ?? 1);
}

/**
 * The world disc's radius (m; shared/protocol/constants.json's worldRadius, the widest level's
 * half span): the map's tiles are built anywhere on it, the sea's included, and nowhere off it.
 */
export function worldRadiusM(): number {
  return World.worldRadius * worldScale;
}

/** The island's radius (m; constants.json's islandRadius × the world's size, BIG_MAP.md M2b). */
export function islandRadiusM(): number {
  return World.islandRadius * worldScale;
}

/**
 * Turns a stream of amounts (a wheel's pixels, a pinch's log spread) into whole steps of `step`
 * each, the remainder kept for the next.
 */
export class Steps {
  private acc = 0;
  constructor(readonly step: number) {}
  add(d: number): number {
    this.acc += d;
    const n = Math.trunc(this.acc / this.step) || 0;
    this.acc -= n * this.step;
    return n;
  }
  reset(): void {
    this.acc = 0;
  }
}

/** The handed sign of a face: its face-local −x is a yaw-0 player's right on face A, +x on B. */
export function rightSign(face: number): number {
  return face === 1 ? 1 : -1;
}

/** A face-local point (x, z) in the view's pixels from its centre. */
export function toScreen(v: BigView, x: number, z: number): [number, number] {
  return [rightSign(v.face) * (x - v.cx) * v.pxPerM, -(z - v.cz) * v.pxPerM];
}

/** The face-local point (x, z) at pixels (sx, sy) from the view's centre. */
export function toWorld(v: BigView, sx: number, sy: number): [number, number] {
  return [v.cx + (rightSign(v.face) * sx) / v.pxPerM, v.cz - sy / v.pxPerM];
}

/** Zoomed to `pxPerM` about pixels (sx, sy) from the centre: the point under them stays put. */
export function zoomAt(v: BigView, pxPerM: number, sx: number, sy: number): BigView {
  const [x, z] = toWorld(v, sx, sy);
  const next = { ...v, pxPerM };
  const [nx, nz] = toWorld(next, sx, sy);
  return { ...next, cx: v.cx + (x - nx), cz: v.cz + (z - nz) };
}

/** Panned by a drag of (dx, dy) pixels: the ground follows the pointer. */
export function pan(v: BigView, dx: number, dy: number): BigView {
  const [x, z] = toWorld(v, -dx, -dy);
  return { ...v, cx: x, cz: z };
}

/** A heading (yaw degrees, 0 = +Z) as a screen angle on the big map (radians, clockwise from up). */
export function headingOnScreen(face: number, yaw: number): number {
  const r = (yaw * Math.PI) / 180;
  const [sx, sy] = [rightSign(face) * Math.sin(r), -Math.cos(r)];
  return Math.atan2(sx, -sy);
}

/** What the map's tiles need from the client (`crates/dwell-client/src/map_reveal.rs`). */
export interface RevealClient {
  /** Asks for tile (ti, tk) of the level of `cellM` cells on `face`. */
  requestReveal(face: number, ti: number, tk: number, cellM: number): void;
  takeReveal(): Uint8Array | undefined;
}

/**
 * The closest levels' real blocks (owner, 2026-11-09: "big map closest zooms should use 1m"):
 * a tile read now from the loaded terrain (`crates/dwell-client/src/map_reveal.rs` `live_tile`),
 * `takeReveal`'s bytes, alpha 0 where nothing is loaded; undefined for nothing loaded there.
 */
export interface LiveClient {
  liveTile(face: number, ti: number, tk: number, cellM: number): Uint8Array | undefined;
}

/**
 * The minimap's fill (owner, 2026-11-10: it "should always show the full width and height of
 * what's around you" and "shouldn't ever be coarse"): the generated 1 m tiles under its disc
 * (`dwell_game::minimap::Generated`), the big map's closest level's, named by the client and
 * handed over from the tile cache.
 */
export interface MinimapFeed {
  /** The tiles wanted: [face, ti, tk] each, the nearest first. */
  minimapWants(): Int32Array;
  minimapTile(face: number, ti: number, tk: number, rgba: Uint8Array, heights: Int16Array): void;
}

/** Whether the level of `cellM` cells is drawn from the loaded terrain where loaded. */
export function isLive(cellM: number): boolean {
  return cellM >= 1 && cellM <= MAP.revealed.liveMaxCellM;
}

/**
 * A finished tile: face, tile indices, its level's cell side, its RGBA (null: no terrain to
 * reveal from) and its cells' height codes (null: none sent).
 */
export interface RevealTile {
  face: number;
  ti: number;
  tk: number;
  cellM: number;
  rgba: Uint8Array | null;
  heights: Int16Array | null;
}

/** `takeReveal`'s header: face (u8), then ti, tk and the cell side (i32 LE). */
const TILE_HEADER = 13;

/**
 * `takeReveal`'s bytes: face, ti, tk and cellM (i32 LE), then the tile's RGBA rows of x from its
 * least z, then (when there) its `n`² i16 LE height codes in the same order.
 */
export function parseRevealTile(b: Uint8Array, n: number = MAP.revealed.tileCells): RevealTile {
  const d = new DataView(b.buffer, b.byteOffset, b.byteLength);
  const colours = n * n * 4;
  const at = TILE_HEADER;
  let heights: Int16Array | null = null;
  if (b.length >= at + colours + n * n * 2) {
    heights = new Int16Array(n * n);
    for (let j = 0; j < n * n; j++) heights[j] = d.getInt16(at + colours + j * 2, true);
  }
  return {
    face: b[0] ?? 0,
    ti: d.getInt32(1, true),
    tk: d.getInt32(5, true),
    cellM: b.length >= at ? d.getInt32(9, true) : MAP.explored.cellM,
    rgba: b.length > at ? b.subarray(at, Math.min(b.length, at + colours)) : null,
    heights,
  };
}

/**
 * The built tiles kept in this browser (IndexedDB `dwell-map-tiles` in the browser, a memory one
 * in tests): each a tile's `takeReveal` bytes under its key, a world's and line's under one
 * prefix ([`tilePrefix`]). Append-only: a key is a tile, written once, never rewritten.
 */
export interface TileStore {
  /** Every tile kept under `prefix`: [the rest of its key, its bytes]. */
  list(prefix: string): Promise<[string, Uint8Array][]>;
  put(key: string, bytes: Uint8Array): Promise<void>;
}

/**
 * The prefix of a world's tiles for this build: the world, the build's compatibility line (its
 * generated terrain is the line's, so a new patch reuses the tiles and a new line builds its own
 * once; owner, 2026-11-09) and the tile size. Keys of the per-version prefix before 0.50.75
 * (`world|0.50.74|…`) are left where they are: the store is append-only.
 */
export function tilePrefix(world: string, version: string, n: number = MAP.revealed.tileCells) {
  return `${world}|${compatibilityLine(version) ?? version}|${String(n)}|`;
}

/** The browser's tile store: `dwell-map-tiles`. Null where there is none. */
export function indexedDbTiles(): TileStore | null {
  if (typeof indexedDB === 'undefined') return null;
  let db: Promise<IDBDatabase> | null = null;
  const open = (): Promise<IDBDatabase> =>
    (db ??= new Promise((resolve, reject) => {
      const req = indexedDB.open('dwell-map-tiles', 1);
      req.onupgradeneeded = () => req.result.createObjectStore('tiles');
      req.onsuccess = () => {
        resolve(req.result);
      };
      req.onerror = () => {
        reject(req.error ?? new Error('indexedDB'));
      };
    }));
  return {
    list: (prefix) =>
      open().then(
        (d) =>
          new Promise((resolve, reject) => {
            const out: [string, Uint8Array][] = [];
            const range = IDBKeyRange.bound(prefix, `${prefix}\uffff`);
            const req = d.transaction('tiles', 'readonly').objectStore('tiles').openCursor(range);
            req.onsuccess = () => {
              const c = req.result;
              if (!c) {
                resolve(out);
                return;
              }
              if (typeof c.key === 'string' && c.value instanceof Uint8Array) {
                out.push([c.key.slice(prefix.length), c.value]);
              }
              c.continue();
            };
            req.onerror = () => {
              reject(req.error ?? new Error('indexedDB'));
            };
          }),
      ),
    put: (key, bytes) =>
      open().then(
        (d) =>
          new Promise((resolve, reject) => {
            const req = d.transaction('tiles', 'readwrite').objectStore('tiles').put(bytes, key);
            req.onsuccess = () => {
              resolve();
            };
            req.onerror = () => {
              reject(req.error ?? new Error('indexedDB'));
            };
          }),
      ),
  };
}

/** A tile: [cellM, face, ti, tk]. */
export type TileAt = [number, number, number, number];

/**
 * The big map's tiles (ADR 0068; owner, 2026-11-09: "If the map loads I'd love if it only did it
 * once. And preferably invisibly"): `revealed.tileCells`² cells of a zoom level each, generated
 * in the job workers from the seed, kept in the store per world and build, so each is built once.
 * Each frame `pump` takes finished tiles for at most `budgetMs`, asks for the open view's missing
 * tiles (nearest its centre first, at most `jobsInFlight` at once) and, once the kept tiles are
 * read and the view has what it needs, builds the background's: the 1 m level round the player,
 * the coarse levels' whole island and whole disc (each level's `reach`). Opening or zooming the map never
 * waits for generation.
 */
export class RevealCache {
  readonly tiles = new Map<string, RevealTile>();
  /**
   * The closest levels' tiles read from the loaded terrain (never kept in the store): a cell they
   * hold (alpha 255) shows over the generator's; when each was read (ms).
   */
  readonly live = new Map<string, RevealTile>();
  private readonly liveAt = new Map<string, number>();
  private readonly pending = new Set<string>();
  /** Tiles still to ask for, the nearest last (the last view's). */
  private wanted: TileAt[] = [];
  private wantedFor = '';
  /**
   * The background's lanes, one a level in `backgroundOrder`: its tiles still to ask for (the
   * nearest last) and what they were listed round. An island or disc level's count left is its
   * reach's tiles less those built (`reachBuilt`), so it needs no scan.
   */
  private readonly lanes: { l: number; at: string; list: TileAt[] }[] =
    RevealCache.backgroundOrder().map((l) => ({ l, at: '', list: [] }));
  private readonly reachTotal = new Map<number, number>();
  private readonly reachBuilt = new Map<string, number>();
  /** The player levels' tiles listed round the player (for the count left). */
  private playerKeys = new Set<string>();
  /** The face the island and disc levels are being built on. */
  private laneFace = 0;
  /** The minimap's tiles handed over and asked for (`feedMinimap`). */
  minimapGiven = 0;
  minimapAsked = 0;
  /** Whether the kept tiles have been read (the background waits: nothing is built twice). */
  loaded: boolean;
  /** Tiles built this session (not read from the store). */
  built = 0;
  /** No terrain to build from (a world the client does not generate): no background. */
  noTerrain = false;

  constructor(
    readonly tileCells: number = MAP.revealed.tileCells,
    readonly jobsInFlight: number = MAP.revealed.jobsInFlight,
    readonly budgetMs: number = MAP.revealed.budgetMs,
    readonly backgroundJobs: number = MAP.revealed.backgroundJobs,
    private readonly store: TileStore | null = null,
    private readonly prefix = '',
  ) {
    this.loaded = store === null;
  }

  /** A tile's key. */
  static key(cellM: number, face: number, ti: number, tk: number): string {
    return `${String(cellM)}|${cellKey(face, ti, tk)}`;
  }

  /** Reads the kept tiles (once); the count read. */
  async load(): Promise<number> {
    let read = 0;
    try {
      for (const [rest, bytes] of (await this.store?.list(this.prefix)) ?? []) {
        const t = parseRevealTile(bytes, this.tileCells);
        const key = RevealCache.key(t.cellM, t.face, t.ti, t.tk);
        if (key !== rest || this.tiles.has(key) || !t.rgba) continue;
        this.tiles.set(key, t);
        this.noteKept(t);
        read++;
      }
    } catch {
      // Unreadable: built again.
    } finally {
      this.loaded = true;
    }
    return read;
  }

  /** A tile's side (m) at a level of `cellM` cells. */
  span(cellM: number): number {
    return this.tileCells * cellM;
  }

  /**
   * Whether tile (i, k) at the level of `cellM` cells touches the world disc (its sea too: the
   * owner, 2026-11-09, "Including all the water"; off the disc the panel shows `fogColor`).
   */
  onWorld(cellM: number, i: number, k: number): boolean {
    const span = this.span(cellM);
    const r = worldRadiusM();
    // The tile's nearest point to the world's centre.
    const nx = Math.max(i * span, Math.min(0, (i + 1) * span));
    const nz = Math.max(k * span, Math.min(0, (k + 1) * span));
    return nx * nx + nz * nz <= r * r;
  }

  /** The tiles a `w` × `h` view covers at its level that touch the world disc, nearest its centre first. */
  tilesFor(v: BigView, w: number, h: number): TileAt[] {
    const cellM = levelCellM(v.level);
    const span = this.span(cellM);
    const [ax, az] = toWorld(v, -w / 2, -h / 2);
    const [bx, bz] = toWorld(v, w / 2, h / 2);
    const [i0, i1] = [Math.floor(Math.min(ax, bx) / span), Math.floor(Math.max(ax, bx) / span)];
    const [k0, k1] = [Math.floor(Math.min(az, bz) / span), Math.floor(Math.max(az, bz) / span)];
    const out: TileAt[] = [];
    for (let k = k0; k <= k1; k++) {
      for (let i = i0; i <= i1; i++) if (this.onWorld(cellM, i, k)) out.push([cellM, v.face, i, k]);
    }
    const d = ([, , i, k]: TileAt) =>
      ((i + 0.5) * span - v.cx) ** 2 + ((k + 0.5) * span - v.cz) ** 2;
    return out.sort((a, b) => d(a) - d(b));
  }

  /** Whether tile (i, k) of level `l` is in its island or disc reach (a player level: the disc). */
  inReach(l: number, i: number, k: number): boolean {
    const { cellM, reach } = LEVELS[l] ?? { cellM: 1, reach: 'disc' };
    if (reach !== 'island') return this.onWorld(cellM, i, k);
    const span = this.span(cellM);
    const r = islandRadiusM();
    const nx = Math.max(i * span, Math.min(0, (i + 1) * span));
    const nz = Math.max(k * span, Math.min(0, (k + 1) * span));
    return nx * nx + nz * nz <= r * r;
  }

  /**
   * Level `l`'s background tiles on `face`, the nearest (x, z) first: a player level's within
   * `prebuildSpans` of its span round (x, z) (a square, on the world disc only: no holes in the
   * world), an island or disc level's every tile touching it.
   */
  levelTiles(l: number, face: number, x: number, z: number): TileAt[] {
    const { cellM, spanM, reach } = LEVELS[l] ?? { cellM: 1, spanM: 1, reach: 'player' };
    const span = this.span(cellM);
    const out: TileAt[] = [];
    if (reach === 'player') {
      const half = (spanM * MAP.revealed.prebuildSpans) / 2;
      const [i0, i1] = [Math.floor((x - half) / span), Math.floor((x + half) / span)];
      const [k0, k1] = [Math.floor((z - half) / span), Math.floor((z + half) / span)];
      for (let k = k0; k <= k1; k++) {
        for (let i = i0; i <= i1; i++) if (this.onWorld(cellM, i, k)) out.push([cellM, face, i, k]);
      }
    } else {
      const n = Math.ceil((reach === 'island' ? islandRadiusM() : worldRadiusM()) / span);
      for (let k = -n; k < n; k++) {
        for (let i = -n; i < n; i++) if (this.inReach(l, i, k)) out.push([cellM, face, i, k]);
      }
    }
    const d = ([, , i, k]: TileAt) => ((i + 0.5) * span - x) ** 2 + ((k + 0.5) * span - z) ** 2;
    return out.sort((a, b) => d(a) - d(b));
  }

  /**
   * The background's levels in the order built: the player levels first (the 1 m area round the
   * player, always streamed), then the island and disc levels, the coarsest first (the whole
   * world in a few tiles, then the island's detail).
   */
  static backgroundOrder(): number[] {
    const rank = (l: number) => (LEVELS[l]?.reach === 'player' ? 0 : 1);
    return LEVELS.map((_, l) => l).sort((a, b) => rank(a) - rank(b) || b - a);
  }

  /**
   * Every background tile in the order built, for the player at (x, z) on `face`; the island and
   * disc levels nearest `at` ([face, x, z], the open view's centre) first when given.
   */
  backgroundFor(
    face: number,
    x: number,
    z: number,
    at: readonly [number, number, number] | null = null,
  ): TileAt[] {
    return RevealCache.backgroundOrder().flatMap((l) =>
      LEVELS[l]?.reach === 'player' || !at
        ? this.levelTiles(l, face, x, z)
        : this.levelTiles(l, at[0], at[1], at[2]),
    );
  }

  /** Counts a tile kept (built or read) towards its level's reach. */
  private noteKept(t: RevealTile): void {
    const l = LEVELS.findIndex((lv) => lv.cellM === t.cellM);
    if (l < 0 || LEVELS[l]?.reach === 'player' || !this.inReach(l, t.ti, t.tk)) return;
    const key = `${String(l)}|${String(t.face)}`;
    this.reachBuilt.set(key, (this.reachBuilt.get(key) ?? 0) + 1);
  }

  /** How many tiles an island or disc level's reach holds (on either face). */
  private reachCount(l: number): number {
    let n = this.reachTotal.get(l);
    if (n === undefined) {
      n = this.levelTiles(l, 0, 0, 0).length;
      this.reachTotal.set(l, n);
    }
    return n;
  }

  /** Whether tile `t` is built (or read) or on its way. */
  private known([cellM, face, ti, tk]: TileAt): boolean {
    const key = RevealCache.key(cellM, face, ti, tk);
    return this.tiles.has(key) || this.pending.has(key);
  }

  private ask(client: RevealClient, [cellM, face, ti, tk]: TileAt): void {
    this.pending.add(RevealCache.key(cellM, face, ti, tk));
    client.requestReveal(face, ti, tk, cellM);
  }

  /**
   * The minimap's tiles (`MinimapFeed`): each it wants that is kept here is handed over; the rest
   * are asked for first of anything (up to `jobsInFlight` pending), the nearest first. Before
   * `pump`, each frame.
   */
  feedMinimap(client: RevealClient & MinimapFeed): void {
    const wants = client.minimapWants();
    const cellM = levelCellM(0);
    for (let n = 0; n + 2 < wants.length; n += 3) {
      const t: TileAt = [cellM, wants[n] ?? 0, wants[n + 1] ?? 0, wants[n + 2] ?? 0];
      const [, face, ti, tk] = t;
      const kept = this.tiles.get(RevealCache.key(cellM, face, ti, tk));
      if (kept?.rgba) {
        client.minimapTile(face, ti, tk, kept.rgba, kept.heights ?? new Int16Array(0));
        this.minimapGiven++;
      } else if (!kept && this.loaded && !this.noTerrain) {
        if (this.known(t) || this.pending.size >= this.jobsInFlight) continue;
        this.ask(client, t);
        this.minimapAsked++;
      }
    }
  }

  /**
   * One frame's fill: takes finished tiles (keeping each, handing it to `onTile`) while under
   * `budgetMs` by `now`; asks for the open view `v`'s missing tiles up to `jobsInFlight` pending;
   * then, with the kept tiles read and the view's all asked for, the background's, map open or
   * shut (owner, 2026-11-10: "The two coarse ones load along with the area around you in 1m. The
   * 1m is always streamed"): the player levels round the player `pose` ([face, x, z]), then the
   * island and disc levels whole, the coarsest first, nearest the open view's centre (else the
   * player) first; up to `jobsInFlight` pending while the map is open, `backgroundJobs` while it
   * is shut. The tiles taken.
   */
  pump(
    client: RevealClient,
    v: BigView | null,
    w: number,
    h: number,
    now: () => number,
    onTile?: (t: RevealTile) => void,
    pose: ArrayLike<number> | null = null,
  ): number {
    const t0 = now();
    let taken = 0;
    while (now() - t0 < this.budgetMs) {
      const bytes = client.takeReveal();
      if (!bytes) break;
      const t = parseRevealTile(bytes, this.tileCells);
      const key = RevealCache.key(t.cellM, t.face, t.ti, t.tk);
      this.pending.delete(key);
      if (!this.tiles.has(key)) this.noteKept(t);
      this.tiles.set(key, t);
      if (t.rgba) {
        this.built++;
        void this.store?.put(this.prefix + key, bytes.slice()).catch(() => undefined);
      } else this.noTerrain = true;
      onTile?.(t);
      taken++;
    }
    if (v) {
      const at = [v.face, v.cx, v.cz, v.pxPerM, v.level, w, h].join(',');
      if (at !== this.wantedFor) {
        this.wantedFor = at;
        this.wanted = this.tilesFor(v, w, h).reverse();
      }
      while (this.pending.size < this.jobsInFlight && this.wanted.length > 0) {
        const t = this.wanted.pop();
        if (t && !this.known(t)) this.ask(client, t);
      }
    }
    if (!this.loaded || this.noTerrain || (!pose && !v) || (v && this.wanted.length > 0)) {
      return taken;
    }
    const [face = 0, x = 0, z = 0] = pose ? Array.from(pose) : [];
    // The island and disc levels nearest the open view's centre, else the player.
    const [cf, cx, cz] = v ? [v.face, v.cx, v.cz] : [face, x, z];
    this.laneFace = cf;
    for (const lane of this.lanes) {
      const { cellM, reach } = LEVELS[lane.l] ?? { cellM: 1, reach: 'player' };
      const player = reach === 'player';
      if (player && !pose) continue;
      // Listed again once the player has moved one of the level's tiles, or the centre four.
      const step = this.span(cellM) * (player ? 1 : 4);
      const [f, px, pz] = player ? [face, x, z] : [cf, cx, cz];
      const at = [f, Math.floor(px / step), Math.floor(pz / step)].join(',');
      if (at === lane.at) continue;
      lane.at = at;
      lane.list = this.levelTiles(lane.l, f, px, pz).reverse();
      if (player) {
        this.playerKeys = new Set(lane.list.map(([c, tf, i, k]) => RevealCache.key(c, tf, i, k)));
      }
    }
    const cap = v ? this.jobsInFlight : this.backgroundJobs;
    // At most this many known tiles passed over a frame (a reopened world's kept ones).
    let skips = 4096;
    for (const lane of this.lanes) {
      while (this.pending.size < cap && lane.list.length > 0 && skips > 0) {
        const t = lane.list.pop();
        if (!t) break;
        if (this.known(t)) skips--;
        else this.ask(client, t);
      }
      if (this.pending.size >= cap || skips <= 0) break;
    }
    return taken;
  }

  /** Tiles asked for and not yet back. */
  get inFlight(): number {
    return this.pending.size;
  }

  /** The background's tiles still to build (after the last pump). */
  get backgroundLeft(): number {
    if (this.noTerrain) return 0;
    let left = 0;
    for (const key of this.playerKeys) if (!this.tiles.has(key) && !this.pending.has(key)) left++;
    for (const { l } of this.lanes) {
      if (LEVELS[l]?.reach === 'player') continue;
      const cellM = LEVELS[l]?.cellM ?? 1;
      let pending = 0;
      for (const key of this.pending) {
        const [c, f, i, k] = key.split(/[|,]/).map(Number);
        if (c === cellM && f === this.laneFace && this.inReach(l, i ?? 0, k ?? 0)) pending++;
      }
      const built = this.reachBuilt.get(`${String(l)}|${String(this.laneFace)}`) ?? 0;
      left += Math.max(0, this.reachCount(l) - built - pending);
    }
    return left;
  }

  private cell(cellM: number, face: number, i: number, k: number) {
    const n = this.tileCells;
    const [ti, tk] = [Math.floor(i / n), Math.floor(k / n)];
    const key = RevealCache.key(cellM, face, ti, tk);
    const j = (k - tk * n) * n + (i - ti * n);
    // The loaded terrain's block where it was read, else the generator's.
    const l = this.live.get(key);
    if (l?.rgba && (l.rgba[j * 4 + 3] ?? 0) === 255) return { t: l, j };
    return { t: this.tiles.get(key), j };
  }

  /**
   * One frame's read of the closest levels' tiles from the loaded terrain (the open view `v`'s,
   * a level with `isLive`): at most `perFrame` of them, the nearest the view's centre first,
   * each again once `refreshMs` old (for edits). The tiles whose cells changed.
   */
  pumpLive(
    client: LiveClient,
    v: BigView | null,
    w: number,
    h: number,
    nowMs: number,
    perFrame: number = MAP.revealed.liveTilesPerFrame,
    refreshMs: number = MAP.revealed.liveRefreshMs,
  ): TileAt[] {
    if (!v || !isLive(levelCellM(v.level))) return [];
    const changed: TileAt[] = [];
    let read = 0;
    for (const t of this.tilesFor(v, w, h)) {
      if (read >= perFrame) break;
      const [cellM, face, ti, tk] = t;
      const key = RevealCache.key(cellM, face, ti, tk);
      const at = this.liveAt.get(key);
      if (at !== undefined && nowMs - at < refreshMs) continue;
      read++;
      this.liveAt.set(key, nowMs);
      const bytes = client.liveTile(face, ti, tk, cellM);
      const had = this.live.get(key);
      if (!bytes) {
        if (had) {
          this.live.delete(key);
          changed.push(t);
        }
        continue;
      }
      const tile = parseRevealTile(bytes, this.tileCells);
      if (had && sameTile(had, tile)) continue;
      this.live.set(key, tile);
      changed.push(t);
    }
    return changed;
  }

  /**
   * Cell (i, k) of the level of `cellM` cells: 0xRRGGBB, null beyond the world's edge (or in a
   * world the client does not generate), undefined while its tile is still to come.
   */
  colour(cellM: number, face: number, i: number, k: number): number | null | undefined {
    const { t, j } = this.cell(cellM, face, i, k);
    if (!t) return undefined;
    if (!t.rgba) return null;
    const o = j * 4;
    if ((t.rgba[o + 3] ?? 0) === 0) return null;
    return ((t.rgba[o] ?? 0) << 16) | ((t.rgba[o + 1] ?? 0) << 8) | (t.rgba[o + 2] ?? 0);
  }

  /** Cell (i, k)'s height code at the level of `cellM` cells, NO_HEIGHT where none (yet). */
  height(cellM: number, face: number, i: number, k: number): number {
    const { t, j } = this.cell(cellM, face, i, k);
    const v = t?.heights?.[j];
    return v === undefined || v === -32768 ? NO_HEIGHT : v;
  }
}

/** Whether two tiles hold the same colours and heights. */
function sameTile(a: RevealTile, b: RevealTile): boolean {
  const eq = (x: ArrayLike<number> | null, y: ArrayLike<number> | null) => {
    if (x === null || y === null) return x === y;
    if (x.length !== y.length) return false;
    for (let j = 0; j < x.length; j++) if (x[j] !== y[j]) return false;
    return true;
  };
  return eq(a.rgba, b.rgba) && eq(a.heights, b.heights);
}

/**
 * The explored cells at every zoom level (fog per level, owner, 2026-11-09: "Adventure keeps the
 * fog at every level"): a level's cell is explored when an explored cell covers it (a level finer
 * than the explored cells) or lies in it (a coarser one, its latest explored cell standing for it).
 */
export class ExploredLevels {
  private readonly coarse = new Map<number, Map<string, string>>();

  constructor(
    readonly cells: ExploredCells,
    readonly heights: ExploredHeights,
  ) {}

  /** An explored cell's key came or changed. */
  note(key: string): void {
    const [face = 0, i = 0, k = 0] = key.split(',').map(Number);
    const e = MAP.explored.cellM;
    for (const { cellM } of LEVELS) {
      if (cellM <= e) continue;
      const r = cellM / e;
      let m = this.coarse.get(cellM);
      if (!m) this.coarse.set(cellM, (m = new Map<string, string>()));
      m.set(cellKey(face, Math.floor(i / r), Math.floor(k / r)), key);
    }
  }

  /** The explored cell standing for level cell (i, k) of `cellM` cells; undefined: fogged. */
  keyAt(cellM: number, face: number, i: number, k: number): string | undefined {
    const e = MAP.explored.cellM;
    if (cellM > e) return this.coarse.get(cellM)?.get(cellKey(face, i, k));
    const r = e / cellM;
    const key = cellKey(face, Math.floor(i / r), Math.floor(k / r));
    return this.cells.has(key) ? key : undefined;
  }

  /**
   * What level cell (i, k) of `cellM` cells shows: [0xRRGGBB, height code], null for nothing
   * there (beyond the world's edge), undefined for fog (or, revealed, still to come). Unexplored
   * is fog unless `revealed`; explored ground is the generator's at a level finer than the
   * explored cells (their detail), else the explored cell's own (the loaded terrain's: edits).
   */
  at(
    reveal: RevealCache,
    revealed: boolean,
    cellM: number,
    face: number,
    i: number,
    k: number,
  ): [number, number] | null | undefined {
    const ek = this.keyAt(cellM, face, i, k);
    if (!revealed && ek === undefined) return undefined;
    const g = reveal.colour(cellM, face, i, k);
    if (ek !== undefined && (cellM >= MAP.explored.cellM || g === undefined || g === null)) {
      return [this.cells.get(ek) ?? 0, this.heights.get(ek) ?? NO_HEIGHT];
    }
    if (g === undefined || g === null) return g;
    return [g, reveal.height(cellM, face, i, k)];
  }

  /** The tiles (at the level of `cellM` cells, `n` a side) whose images show explored cell `key`. */
  tilesOf(cellM: number, key: string, n: number): string[] {
    const [face = 0, i = 0, k = 0] = key.split(',').map(Number);
    const e = MAP.explored.cellM;
    if (cellM > e) {
      const r = cellM / e;
      return tilesShowing(cellM, face, Math.floor(i / r), Math.floor(k / r), n);
    }
    const r = e / cellM;
    return [
      ...new Set([
        ...tilesShowing(cellM, face, i * r, k * r, n),
        ...tilesShowing(cellM, face, i * r + r - 1, k * r + r - 1, n),
      ]),
    ];
  }
}

/**
 * A tile's RGBA (rows of face-local x from its least z) turned to how the big map shows it: rows
 * from the top of the screen (+z up), columns from the left (−x on face A, +x on face B).
 */
export function tileOnScreen(
  rgba: Uint8Array,
  n: number,
  face: number,
): Uint8ClampedArray<ArrayBuffer> {
  const out = new Uint8ClampedArray(n * n * 4);
  const flipX = rightSign(face) < 0;
  for (let r = 0; r < n; r++) {
    const b = n - 1 - r;
    for (let c = 0; c < n; c++) {
      const a = flipX ? n - 1 - c : c;
      out.set(rgba.subarray((b * n + a) * 4, (b * n + a) * 4 + 4), (r * n + c) * 4);
    }
  }
  return out;
}

/** Whether two dot lists ([right, up] m each) draw the same (within a tenth of a metre). */
export function sameDots(a: ArrayLike<number>, b: ArrayLike<number>): boolean {
  if (a.length !== b.length) return false;
  for (let n = 0; n < a.length; n++) if (Math.abs((a[n] ?? 0) - (b[n] ?? 0)) > 0.1) return false;
  return true;
}

/** The minimap's CSS size on a `width` × `height` screen: compact when its short side is under `compactBelowPx` (a phone either way up; shared/ui/map.json). */
export function minimapSize(width: number, height: number): number {
  return Math.min(width, height) < MAP.minimap.compactBelowPx
    ? MAP.minimap.compactSizePx
    : MAP.minimap.sizePx;
}

/** What the map needs from the client (the wasm `Client`). */
export interface MapClient extends RevealClient, LiveClient, MinimapFeed {
  minimapFrame(nowMs: number, shading: boolean): Uint8Array | undefined;
  minimapPixels(): number;
  minimapDots(): Float64Array;
  mapPose(): Float64Array;
  takeExplored(): Int32Array;
  mapAllowed(): boolean;
  /** The `mapRevealed` ability (creative): the big map shows everything. */
  mapRevealed(): boolean;
  cpuAdapter(): boolean;
  /** The joined world's size's radius scale (BIG_MAP.md M2b; 1 today's). */
  worldRadiusScale?(): number;
  /** The big map's levels for the world's size, [cellM, spanM, …] (BIG_MAP.md M2b). */
  bigMapLevels?(): ArrayLike<number>;
  /**
   * The big map's cells tinted by height (`dwell_game::minimap::shade_grid`, the minimap's look
   * when its toggle is on): a `w` × `h` screen-order grid inside a one-cell border; RGBA out.
   */
  shadeMap(colours: Uint32Array, codes: Int32Array, w: number, h: number): Uint8Array;
  /**
   * The debug layers (REALISTIC_WORLD.md R6, `mapLayers.ts`): asks for field `field` (an index
   * into `mapLayerFields`) sampled n × n from face-local (x0, z0), `step` m apart.
   */
  requestMapLayer?(
    field: number,
    x0: number,
    z0: number,
    step: number,
    n: number,
    face: number,
  ): void;
  /** A requested layer once: its field index and face, then its `LayerGrid::encode` bytes. */
  takeMapLayer?(): Uint8Array | undefined;
  /** The generator's layer fields by index (`map_layers.rs` `MapField`). */
  mapLayerFields?(): string[];
  /**
   * Map markers (docs/MAP_MARKERS.md): the player's feet, face-local [face, x, y, z, dead, then
   * while dead the death's face, x, y, z].
   */
  markerPose?(): ArrayLike<number>;
  /** The player's saved markers record the server sent, once. */
  takeMarkers?(): Uint8Array | undefined;
  /** Whether the server keeps the player's markers (`markers.private`). */
  markersSynced?(): boolean;
  /** The player's markers record now, to the server. */
  setMarkers?(record: Uint8Array): boolean;
  /** A face-local point in the view: [x, y (−1..1, +y up), depth (≤ 0 behind), distance (m)]. */
  markerScreen?(face: number, x: number, y: number, z: number): ArrayLike<number>;
}

/** A nice scale bar: the longest 1, 2 or 5 × 10ⁿ metres no wider than `maxPx` at `pxPerM`. */
export function scaleBar(pxPerM: number, maxPx: number = MAP.bigMap.scaleBarMaxPx) {
  let best = 1;
  for (let e = -1; e <= 6; e++) {
    for (const m of [1, 2, 5]) {
      const v = m * 10 ** e;
      if (v * pxPerM <= maxPx) best = v;
    }
  }
  const metres = Math.round(best * 10) / 10;
  const label = metres >= 1000 ? `${String(metres / 1000)} km` : `${String(metres)} m`;
  return { metres, px: metres * pxPerM, label };
}

/** The big map's panel on a `width` × `height` screen: its margin round it and its buttons' size. */
export function bigMapPanel(width: number, height: number) {
  const phone = Math.min(width, height) < MAP.minimap.compactBelowPx;
  const f = MAP.bigMap.panelFraction;
  return phone
    ? {
        phone,
        marginX: MAP.bigMap.phoneMarginPx,
        marginY: MAP.bigMap.phoneMarginPx,
        buttonPx: MAP.bigMap.phoneButtonPx,
      }
    : {
        phone,
        marginX: Math.round((width * (1 - f)) / 2),
        marginY: Math.round((height * (1 - f)) / 2),
        buttonPx: MAP.bigMap.buttonPx,
      };
}

/** A controller button's index in the standard mapping (shared/input/gamepad.json). */
function padIndex(name: string): number {
  return GAMEPAD.buttons.find((b) => b.name === name)?.index ?? -1;
}

/** One frame of the controller on the open map: what it asks for. */
export interface PadMapStep {
  close: boolean;
  center: boolean;
  /** The zoom levels stepped this frame: +1 in, −1 out, 0 none. */
  zoom: number;
  /** The pan this frame, screen pixels the view moves (right, down). */
  panX: number;
  panY: number;
}

/**
 * The controller on the open map (docs/GAMEPAD.md; buttons as data, `bigMap.pad*`): the left
 * stick and the D-pad pan, a press of a zoom button steps one zoom level, B (and View) close, Y
 * centres. A button held when the map opened is not a press until it lets go.
 */
export class PadMap {
  private was: boolean[] | null = null;
  private last: number | null = null;

  /** Forgets what was held (on opening). */
  reset(): void {
    this.was = null;
    this.last = null;
  }

  step(values: ArrayLike<number>, axes: ArrayLike<number>, now: number): PadMapStep {
    const th = GAMEPAD.tunables.triggerThreshold;
    const down = Array.from(values, (v) => v >= Math.min(0.5, th));
    const was = this.was ?? down;
    this.was = down;
    const dt = this.last === null ? 0 : Math.min(0.1, Math.max(0, (now - this.last) / 1000));
    this.last = now;
    const b = MAP.bigMap;
    const held = (name: string) => down[padIndex(name)] ?? false;
    const pressed = (name: string) => held(name) && !(was[padIndex(name)] ?? false);
    let zoom = 0;
    if (pressed(b.padZoomIn) || pressed(b.padZoomInAlt)) zoom += 1;
    if (pressed(b.padZoomOut) || pressed(b.padZoomOutAlt)) zoom -= 1;
    const dz = GAMEPAD.tunables.deadzone;
    const stick = (v: number) => (Math.abs(v) < dz ? 0 : (v - Math.sign(v) * dz) / (1 - dz));
    const sx =
      stick(axes[GAMEPAD.sticks.moveX] ?? 0) + (held('right') ? 1 : 0) - (held('left') ? 1 : 0);
    const sy =
      stick(axes[GAMEPAD.sticks.moveY] ?? 0) + (held('down') ? 1 : 0) - (held('up') ? 1 : 0);
    const clamp = (v: number) => Math.max(-1, Math.min(1, v));
    return {
      close: pressed(b.padClose) || pressed('view'),
      center: pressed(b.padCenter),
      zoom,
      panX: clamp(sx) * b.padPanPxPerS * dt,
      panY: clamp(sy) * b.padPanPxPerS * dt,
    };
  }
}

/**
 * The big map's state, apart from the page (owner, 2026-11-09: a framed panel, an obvious close,
 * + and − zoom, "Center on me", a scale bar, drag to pan; at most five fixed zoom levels, the
 * closest the minimap's area, the widest the whole world disc): open or closed, the view, and what
 * each control does to it. `size` is the map's size in pixels, each level's span across its short
 * side.
 */
export class BigMapState {
  view: BigView | null = null;
  readonly pad = new PadMap();

  constructor(
    private readonly pose: () => ArrayLike<number>,
    private readonly size: () => [number, number] = () => [800, 600],
  ) {}

  get open(): boolean {
    return this.view !== null;
  }

  /** Opens it centred on the player at the start level. */
  openAt(): void {
    const [face = 0, x = 0, z = 0] = Array.from(this.pose());
    const level = clampLevel(MAP.bigMap.startLevel);
    this.view = { face, cx: x, cz: z, level, pxPerM: levelPxPerM(level, ...this.size()) };
    this.pad.reset();
  }

  close(): void {
    this.view = null;
  }

  /**
   * `steps` zoom levels in (+) or out (−) about pixels (sx, sy) from the centre, clamped to the
   * levels; whether the level changed.
   */
  zoomStep(steps: number, sx = 0, sy = 0): boolean {
    if (!this.view || steps === 0) return false;
    const level = clampLevel(this.view.level - Math.sign(steps) * Math.min(Math.abs(steps), 4));
    if (level === this.view.level) return false;
    this.view = { ...zoomAt(this.view, levelPxPerM(level, ...this.size()), sx, sy), level };
    // The widest level shows the whole world disc, fitted to the short side: centred on it, not
    // on the player.
    if (level === LEVELS.length - 1) this.view = { ...this.view, cx: 0, cz: 0 };
    return true;
  }

  /** Whether zooming in (+1) or out (−1) can go further. */
  canZoom(dir: number): boolean {
    if (!this.view) return false;
    return dir > 0 ? this.view.level > 0 : this.view.level < LEVELS.length - 1;
  }

  /** The level's zoom again for the map's size (on a resize). */
  fit(): void {
    if (this.view)
      this.view = { ...this.view, pxPerM: levelPxPerM(this.view.level, ...this.size()) };
  }

  /** Back to the player's place and face, the zoom kept. */
  center(): void {
    if (!this.view) return;
    const [face = 0, x = 0, z = 0] = Array.from(this.pose());
    this.view = { ...this.view, face, cx: x, cz: z };
  }

  /** The scale bar for the view. */
  scale(): ReturnType<typeof scaleBar> | null {
    return this.view ? scaleBar(this.view.pxPerM) : null;
  }

  /** A controller frame; true when it closed the map. Whether the view changed is `moved`. */
  padFrame(values: ArrayLike<number>, axes: ArrayLike<number>, now: number) {
    const s = this.pad.step(values, axes, now);
    if (!this.view) return { closed: false, moved: false };
    if (s.close) {
      this.close();
      return { closed: true, moved: true };
    }
    let moved = false;
    if (s.center) {
      this.center();
      moved = true;
    }
    if (this.zoomStep(s.zoom)) moved = true;
    if (s.panX !== 0 || s.panY !== 0) {
      this.view = pan(this.view, -s.panX, -s.panY);
      moved = true;
    }
    return { closed: false, moved };
  }
}

/**
 * Explored cell (i, k) of tile (ti, tk) at screen column `c` and row `r` of the tile's image (−1
 * and n: its border), as the big map shows it: rows from the top (+z up), columns from the left
 * (−x on face A, +x on face B).
 */
export function tileCellAt(
  face: number,
  ti: number,
  tk: number,
  c: number,
  r: number,
  n: number,
): [number, number] {
  const i = rightSign(face) > 0 ? ti * n + c : ti * n + (n - 1 - c);
  return [i, tk * n + (n - 1 - r)];
}

/**
 * A tile's cells for `shadeMap`: (n + 2)² colours (top byte 0xff: nothing there) and height codes
 * in screen order, the tile's n² inside a border of its neighbours' cells.
 */
export function tileGrid(
  face: number,
  ti: number,
  tk: number,
  n: number,
  colour: (i: number, k: number) => number | null | undefined,
  height: (i: number, k: number) => number,
): { colours: Uint32Array; codes: Int32Array; any: boolean } {
  const s = n + 2;
  const colours = new Uint32Array(s * s);
  const codes = new Int32Array(s * s);
  let any = false;
  for (let r = -1; r <= n; r++) {
    for (let c = -1; c <= n; c++) {
      const [i, k] = tileCellAt(face, ti, tk, c, r, n);
      const col = colour(i, k);
      const o = (r + 1) * s + (c + 1);
      if (col === undefined || col === null) {
        colours[o] = 0xff000000;
        codes[o] = NO_HEIGHT;
      } else {
        colours[o] = col & 0xffffff;
        codes[o] = height(i, k);
        if (r >= 0 && r < n && c >= 0 && c < n) any = true;
      }
    }
  }
  return { colours, codes, any };
}

/** The tiles (of the level of `cellM` cells) whose images show its cell (i, k): its own, and those it borders. */
export function tilesShowing(
  cellM: number,
  face: number,
  i: number,
  k: number,
  n: number,
): string[] {
  const out = new Set<string>();
  for (const di of [-1, 0, 1]) {
    for (const dk of [-1, 0, 1]) {
      out.add(RevealCache.key(cellM, face, Math.floor((i + di) / n), Math.floor((k + dk) / n)));
    }
  }
  return [...out];
}

/** The minimap and the big map on the page. */
export class MapUi {
  readonly root: HTMLDivElement;
  private readonly disc: HTMLCanvasElement;
  private readonly image: HTMLCanvasElement;
  private readonly big: HTMLDivElement;
  private readonly panel: HTMLDivElement;
  private readonly bigCanvas: HTMLCanvasElement;
  private readonly zoomIn: HTMLButtonElement;
  private readonly zoomOut: HTMLButtonElement;
  private readonly scaleEl: HTMLDivElement;
  private readonly scaleLabel: HTMLSpanElement;
  /** The tiles still building while the map is open (a quiet count). */
  private readonly progress: HTMLDivElement;
  readonly state: BigMapState;
  private readonly cells: ExploredCells = new Map();
  private readonly heights: ExploredHeights = new Map();
  /** The explored cells at each zoom level (the fog). */
  private readonly levels = new ExploredLevels(this.cells, this.heights);
  /** The explored cells changed since the last save (only they are saved). */
  private readonly unsaved = new ExploredChanges();
  private lastSave = 0;
  private dots: ArrayLike<number> = [];
  private lastUpdate = -Infinity;
  private readonly cpu: boolean;
  /** Every zoom level's tiles, built once per world and build (`RevealCache`). */
  readonly reveal: RevealCache;
  /** The pinch's and the wheel's steps between zoom levels. */
  private readonly wheel = new Steps(MAP.bigMap.wheelStepPx);
  private readonly pinch = new Steps(Math.log(MAP.bigMap.pinchStep));
  /** The big map's shaded tile images, and those to make again. */
  private readonly tileImages = new Map<string, HTMLCanvasElement | null>();
  private readonly staleTiles = new Set<string>();
  /** The tiles (of every level) with explored cells in them. */
  private readonly exploredTiles = new Set<string>();
  private lastBigDraw = 0;
  private bigStale = false;
  /** The debug layer shown (an index into `MAP_LAYERS.layers`; −1 none) and its button. */
  private layer = -1;
  private readonly layerButton: HTMLButtonElement;
  /** Each face's layer images by `face:name` (null: the world has no such field), and those asked. */
  private readonly layerImages = new Map<string, HTMLCanvasElement | null>();
  private readonly layerAsked = new Set<string>();
  /** The disc's last draw (ms), for the bench's cost readout. */
  costMs = 0;
  shading: boolean = MAP.heightShading.default;
  /** The player's map markers (docs/MAP_MARKERS.md), their picker and their world waypoints. */
  readonly markers = new MarkerList();
  private readonly picker: MarkerPicker;
  private readonly waypoints: HTMLDivElement;
  private readonly waypointEls = new Map<number, HTMLDivElement>();
  /** The server's record arrived (or the server keeps none), so the auto markers may be noted. */
  private markersReady = false;
  private markersWaitFrom = -1;
  private wasDead = false;
  /** The big map opened or closed (the shell holds the game's input while it is open). */
  onOpenChange: ((open: boolean) => void) | null = null;
  /** A zoom button or key zoomed the big map (its sound). */
  onZoom: (() => void) | null = null;

  constructor(
    private readonly doc: Document,
    private readonly client: MapClient,
    private readonly world: string,
    private readonly store: ExploredStore | null,
    tiles: TileStore | null = null,
    version = '',
  ) {
    this.cpu = client.cpuAdapter();
    // The world's size (BIG_MAP.md M2b): its disc and its levels, before any tile is asked for.
    setMapWorldSize(client.worldRadiusScale?.() ?? 1, client.bigMapLevels?.() ?? []);
    this.reveal = new RevealCache(
      MAP.revealed.tileCells,
      MAP.revealed.jobsInFlight,
      MAP.revealed.budgetMs,
      // `?mapbuild=off` (DEV_TOOLS.md): no background build with the map shut, to measure its cost.
      typeof location !== 'undefined' &&
        new URLSearchParams(location.search).get('mapbuild') === 'off'
        ? 0
        : MAP.revealed.backgroundJobs,
      tiles,
      tilePrefix(world, version),
    );
    void this.reveal.load().then((read) => {
      if (read > 0 && this.bigOpen) this.bigStale = true;
    });
    this.state = new BigMapState(
      () => client.mapPose(),
      () => [
        Math.max(1, Math.round(this.bigCanvas.clientWidth || 800)),
        Math.max(1, Math.round(this.bigCanvas.clientHeight || 600)),
      ],
    );
    const px = client.minimapPixels();
    this.root = doc.createElement('div');
    this.root.id = 'minimap';
    this.root.title = 'Map (M)';
    this.disc = doc.createElement('canvas');
    this.disc.width = this.disc.height = px;
    this.root.append(this.disc);
    this.image = doc.createElement('canvas');
    this.image.width = this.image.height = px;
    // The big map: a framed panel over the game (the world showing round it), a title bar with
    // its close button, the map, its zoom and centre buttons and its scale bar.
    this.big = doc.createElement('div');
    this.big.id = 'world-map';
    this.big.hidden = true;
    this.panel = doc.createElement('div');
    this.panel.className = 'map-panel';
    this.panel.setAttribute('role', 'dialog');
    this.panel.setAttribute('aria-label', 'Map');
    const bar = doc.createElement('div');
    bar.className = 'map-titlebar';
    const title = doc.createElement('h2');
    title.textContent = 'Map';
    const hint = doc.createElement('span');
    hint.className = 'map-hint';
    hint.textContent =
      'Drag to move · scroll, pinch or + − to zoom a level · click to mark, drag a marker to move it · M or Esc to close';
    const close = this.button('map-close', '✕', 'Close map (M or Esc)');
    bar.append(title, hint, close);
    const body = doc.createElement('div');
    body.className = 'map-body';
    this.bigCanvas = doc.createElement('canvas');
    const controls = doc.createElement('div');
    controls.className = 'map-controls';
    this.zoomIn = this.button('map-zoom-in', '+', 'Zoom in');
    this.zoomOut = this.button('map-zoom-out', '−', 'Zoom out');
    const centre = this.button('map-center', '◎', 'Center on me');
    centre.append(doc.createElement('span'));
    (centre.lastChild as HTMLSpanElement).textContent = 'Center on me';
    this.layerButton = this.button('map-layer', '▦', `Map layer (${MAP_LAYERS.key.slice(3)})`);
    this.layerButton.append(doc.createElement('span'));
    this.layerButton.hidden = !client.requestMapLayer;
    controls.append(this.zoomIn, this.zoomOut, centre, this.layerButton);
    this.scaleEl = doc.createElement('div');
    this.scaleEl.className = 'map-scale';
    this.scaleLabel = doc.createElement('span');
    this.scaleEl.append(doc.createElement('i'), this.scaleLabel);
    this.progress = doc.createElement('div');
    this.progress.className = 'map-progress';
    this.progress.hidden = true;
    this.picker = new MarkerPicker(doc);
    body.append(this.bigCanvas, controls, this.scaleEl, this.progress, this.picker.root);
    // The waypoints over the world: a layer under the HUD (hud.json "waypoints").
    this.waypoints = doc.createElement('div');
    this.waypoints.id = 'map-waypoints';
    this.waypoints.setAttribute('aria-hidden', 'true');
    doc.body.append(this.waypoints);
    this.markers.onChange = (list) => {
      this.client.setMarkers?.(encodeMarkers(list));
      this.drawDisc();
      if (this.bigOpen) this.drawBig();
    };
    this.panel.append(bar, body);
    this.big.append(this.panel);
    doc.body.append(this.root, this.big);
    close.addEventListener('click', () => {
      this.setBig(false);
    });
    this.zoomIn.addEventListener('click', () => {
      this.zoomBy(1);
    });
    this.zoomOut.addEventListener('click', () => {
      this.zoomBy(-1);
    });
    centre.addEventListener('click', () => {
      this.state.center();
      this.drawBig();
    });
    this.layerButton.addEventListener('click', () => {
      this.stepLayer();
    });
    this.showLayerLabel();
    // A click on the game round the panel closes it too.
    this.big.addEventListener('pointerdown', (e) => {
      if (e.target === this.big) this.setBig(false);
    });
    const size = () => {
      const w = doc.defaultView?.innerWidth ?? 1000;
      const h = doc.defaultView?.innerHeight ?? 1000;
      const st = doc.documentElement.style;
      st.setProperty('--minimap-size', `${String(minimapSize(w, h))}px`);
      const p = bigMapPanel(w, h);
      st.setProperty('--map-margin-x', `${String(p.marginX)}px`);
      st.setProperty('--map-margin-y', `${String(p.marginY)}px`);
      st.setProperty('--map-button', `${String(p.buttonPx)}px`);
      st.setProperty('--map-frame', MAP.bigMap.frameColor);
      st.setProperty('--map-fog', MAP.bigMap.fogColor);
      st.setProperty('--map-levels', String(LEVELS.length));
      this.panel.classList.toggle('phone', p.phone);
      if (this.bigOpen) {
        this.state.fit();
        this.drawBig();
      }
    };
    size();
    doc.defaultView?.addEventListener('resize', size);
    this.root.addEventListener('pointerup', (e) => {
      e.stopPropagation();
      this.toggleBig();
    });
    this.bindBig();
    void store?.get(world).then((r) => {
      const hs = parseHeights(r);
      for (const [k, v] of parseExplored(r)) {
        if (this.cells.has(k)) continue;
        this.cells.set(k, v);
        const h = hs.get(k);
        if (h !== undefined) this.heights.set(k, h);
        this.noteCell(k);
      }
    });
  }

  private button(cls: string, text: string, label: string): HTMLButtonElement {
    const b = this.doc.createElement('button');
    b.type = 'button';
    b.className = `map-button ${cls}`;
    b.textContent = text;
    b.title = label;
    b.setAttribute('aria-label', label);
    return b;
  }

  /** Steps zoom levels about pixels (sx, sy) from the map's centre, with the zoom's sound. */
  private zoomBy(steps: number, sx = 0, sy = 0): void {
    if (this.state.zoomStep(steps, sx, sy)) {
      this.onZoom?.();
      this.drawBig();
    }
  }

  /** The open map's zoom level (0 the closest), or null. */
  get level(): number | null {
    return this.state.view?.level ?? null;
  }

  /** Whether the big map is open. */
  get bigOpen(): boolean {
    return !this.big.hidden;
  }

  toggleBig(): void {
    this.setBig(!this.bigOpen);
  }

  setBig(open: boolean): void {
    if (open && !this.client.mapAllowed()) return;
    if (open === this.bigOpen) return;
    this.big.hidden = !open;
    if (open) {
      this.wheel.reset();
      this.state.openAt();
      if (this.doc.pointerLockElement) this.doc.exitPointerLock();
      this.drawBig();
    } else {
      this.picker.close();
      this.state.close();
    }
    this.onOpenChange?.(open);
  }

  /** A key on the open big map: + and − zoom, C centres on the player. Whether it was the map's. */
  key(code: string): boolean {
    if (!this.bigOpen) return false;
    if (code === 'Equal' || code === 'NumpadAdd') this.zoomBy(1);
    else if (code === 'Minus' || code === 'NumpadSubtract') this.zoomBy(-1);
    else if (code === 'KeyC') {
      this.state.center();
      this.drawBig();
    } else if (code === MAP_LAYERS.key && this.client.requestMapLayer) this.stepLayer();
    else return false;
    return true;
  }

  /** The debug layer shown (its data name), or null. */
  get layerName(): string | null {
    return MAP_LAYERS.layers[this.layer]?.name ?? null;
  }

  /** Steps to the next debug layer (after the last, none). */
  stepLayer(): void {
    this.layer = nextLayer(this.layer);
    this.showLayerLabel();
    this.drawBig();
  }

  private showLayerLabel(): void {
    const def = MAP_LAYERS.layers[this.layer];
    const v = this.state.view;
    const none = def && v && this.layerImages.get(`${String(v.face)}:${def.name}`) === null;
    const label = def ? `${def.label}${none ? ': not in this world' : ''}` : 'Layers';
    (this.layerButton.lastChild as HTMLSpanElement).textContent = label;
    this.layerButton.classList.toggle('on', !!def);
  }

  /** The layer's image for the face, asked for once (null: none yet, or the world has none). */
  private layerImage(def: MapLayerDef, face: number): HTMLCanvasElement | null {
    const key = `${String(face)}:${def.name}`;
    const had = this.layerImages.get(key);
    if (had !== undefined) return had;
    const fields = this.client.mapLayerFields?.() ?? [];
    const field = fields.indexOf(def.field);
    if (!this.layerAsked.has(key) && field >= 0) {
      this.layerAsked.add(key);
      const n = MAP_LAYERS.samples;
      const { x0, step } = layerSpan(worldRadiusM(), n);
      this.client.requestMapLayer?.(field, x0, x0, step, n, face);
    }
    return null;
  }

  /** A debug layer arrived: the images of every layer of its field on its face. */
  private takeLayer(): void {
    const b = this.client.takeMapLayer?.();
    if (!b || b.length < 2) return;
    const fields = this.client.mapLayerFields?.() ?? [];
    const field = fields[b[0] ?? 0];
    const face = b[1] ?? 0;
    const grid = parseLayerGrid(b.subarray(2));
    const radiusM = worldRadiusM();
    const disc = grid ? { ...layerSpan(radiusM, grid.n), radiusM } : undefined;
    for (const def of MAP_LAYERS.layers) {
      if (def.field !== field) continue;
      const key = `${String(face)}:${def.name}`;
      if (!grid) {
        this.layerImages.set(key, null);
        continue;
      }
      const img = this.doc.createElement('canvas');
      img.width = img.height = grid.n;
      img
        .getContext('2d')
        ?.putImageData(new ImageData(layerPixels(def, grid, disc), grid.n, grid.n), 0, 0);
      this.layerImages.set(key, img);
    }
    this.showLayerLabel();
    if (this.bigOpen) this.bigStale = true;
  }

  /** The controller while the big map is open (docs/GAMEPAD.md). */
  padFrame(values: ArrayLike<number>, axes: ArrayLike<number>, now: number): void {
    if (!this.bigOpen) return;
    const level = this.state.view?.level;
    const r = this.state.padFrame(values, axes, now);
    if (this.state.view && this.state.view.level !== level) this.onZoom?.();
    if (r.closed) {
      this.picker.close();
      this.big.hidden = true;
      this.onOpenChange?.(false);
    } else if (r.moved) this.drawBig();
  }

  /** An explored cell came or changed: at every level its tiles have explored cells and are stale. */
  private noteCell(key: string): void {
    this.levels.note(key);
    const n = this.reveal.tileCells;
    for (const { cellM } of LEVELS) {
      const tiles = this.levels.tilesOf(cellM, key, n);
      for (const t of tiles) this.staleTiles.add(t);
      // Its own tiles (the middle of each 3 × 3).
      const [face = 0, i = 0, k = 0] = key.split(',').map(Number);
      const e = MAP.explored.cellM;
      const [li, lk] =
        cellM > e
          ? [Math.floor((i * e) / cellM), Math.floor((k * e) / cellM)]
          : [i * (e / cellM), k * (e / cellM)];
      const [hi, hk] = cellM > e ? [li, lk] : [li + e / cellM - 1, lk + e / cellM - 1];
      for (const [a, b] of [
        [li, lk],
        [hi, hk],
      ] as const) {
        this.exploredTiles.add(RevealCache.key(cellM, face, Math.floor(a / n), Math.floor(b / n)));
      }
    }
  }

  /** Each frame: the disc when the client drew it again, the dots, the explored cells. */
  frame(now: number): void {
    const allowed = this.client.mapAllowed();
    this.root.classList.toggle('map-off', !allowed);
    this.doc.body.classList.toggle('minimap-on', allowed);
    if (!allowed) {
      this.setBig(false);
      return;
    }
    this.fillReveal();
    this.takeLayer();
    this.markersFrame(now);
    if (this.bigStale && this.bigOpen && now - this.lastBigDraw >= MAP.revealed.redrawMs) {
      this.drawBig();
    }
    // The CPU adapter's tier (CLAUDE.md): a software renderer's page updates it at most every
    // `cpuUpdateMs`, the dots with it.
    if (this.cpu && now - this.lastUpdate < MAP.minimap.cpuUpdateMs) return;
    this.lastUpdate = now;
    const t0 = performance.now();
    const pixels = this.client.minimapFrame(now, this.shading);
    if (pixels) {
      const px = this.image.width;
      const ctx = this.image.getContext('2d');
      ctx?.putImageData(new ImageData(new Uint8ClampedArray(pixels), px, px), 0, 0);
      this.costMs = performance.now() - t0;
    }
    const dots = this.client.minimapDots();
    if (pixels || !sameDots(dots, this.dots)) {
      this.dots = dots;
      this.drawDisc();
    }
    const changed: string[] = [];
    if (addExplored(this.cells, this.client.takeExplored(), this.heights, changed)) {
      for (const k of changed) {
        this.noteCell(k);
        this.unsaved.note(k, this.cells.get(k) ?? 0, this.heights.get(k) ?? NO_HEIGHT);
      }
      if (this.bigOpen) this.bigStale = true;
    }
    if (this.unsaved.size > 0 && now - this.lastSave > 10_000) this.save(now);
  }

  /** Keeps the explored cells now; `leaving` (the page closing) merges them on the page. */
  save(now: number, leaving = false): void {
    if (!this.store || this.unsaved.size === 0) return;
    this.lastSave = now;
    const taken = this.unsaved.take();
    // Only the changes go: the store merges them into the record off the page (exploredStore.ts).
    this.store.add(this.world, taken.cells, taken.heights, leaving).catch(() => {
      this.unsaved.restore(taken);
    });
  }

  /**
   * The map's tiles: one frame's budget of them, the open view's first, else the background's
   * (the open map drawn again at most every `redrawMs`).
   */
  private fillReveal(): void {
    const v = this.big.hidden ? null : this.state.view;
    const c = this.bigCanvas;
    const w = Math.max(1, Math.round(c.clientWidth || 800));
    const h = Math.max(1, Math.round(c.clientHeight || 600));
    // The minimap's 1 m tiles first: handed over from the cache, else built before anything.
    this.reveal.feedMinimap(this.client);
    const taken = this.reveal.pump(
      this.client,
      v,
      w,
      h,
      () => performance.now(),
      (t) => {
        // Its own image and its neighbours' borders.
        for (let di = -1; di <= 1; di++) {
          for (let dk = -1; dk <= 1; dk++) {
            this.staleTiles.add(RevealCache.key(t.cellM, t.face, t.ti + di, t.tk + dk));
          }
        }
      },
      this.client.mapPose(),
    );
    if (taken > 0) this.bigStale = true;
    if (v) {
      const left = this.reveal.backgroundLeft + this.reveal.inFlight;
      this.progress.hidden = left === 0;
      if (left > 0) this.progress.textContent = `Mapping · ${String(left)} tiles`;
    }
    // The closest levels' real blocks where the terrain is loaded (`liveTilesPerFrame` a frame).
    for (const [cellM, face, ti, tk] of this.reveal.pumpLive(
      this.client,
      v,
      w,
      h,
      performance.now(),
    )) {
      for (let di = -1; di <= 1; di++) {
        for (let dk = -1; dk <= 1; dk++) {
          this.staleTiles.add(RevealCache.key(cellM, face, ti + di, tk + dk));
        }
      }
      this.bigStale = true;
    }
  }

  private drawDisc(): void {
    const ctx = this.disc.getContext('2d');
    if (!ctx) return;
    const px = this.disc.width;
    const half = px / 2;
    const perM = px / (2 * MAP.minimap.radiusM);
    ctx.clearRect(0, 0, px, px);
    ctx.drawImage(this.image, 0, 0);
    const dot = (x: number, y: number, c: string) => {
      ctx.beginPath();
      ctx.arc(x, y, (MAP.minimap.dotPx / MAP.minimap.sizePx) * px * 0.5, 0, Math.PI * 2);
      ctx.fillStyle = c;
      ctx.fill();
      ctx.lineWidth = 1;
      ctx.strokeStyle = 'rgb(0 0 0 / 60%)';
      ctx.stroke();
    };
    const d = this.dots;
    for (let n = 0; n + 1 < d.length; n += 2) {
      dot(half + (d[n] ?? 0) * perM, half - (d[n + 1] ?? 0) * perM, MAP.minimap.otherColor);
    }
    // The markers on the disc (heading up, as the dots), their icons over the ground.
    const [face = 0, x = 0, z = 0, yaw = 0] = this.client.mapPose();
    const iconPx = Math.max(8, Math.round(px * 0.09));
    for (const { marker: m, right, up } of markersInRange(
      this.markers.all,
      { face, x, z, yaw },
      MAP.minimap.radiusM,
    )) {
      drawIcon(ctx, m.icon, m.rgb, half + right * perM, half - up * perM, iconPx);
    }
    dot(half, half, MAP.minimap.playerColor);
  }

  /**
   * The markers each frame: the server's record once (the auto markers wait for it, or for a server
   * that keeps none), spawn the first time the player is placed, the last death when they die, and
   * the world waypoints.
   */
  private markersFrame(now: number): void {
    const got = this.client.takeMarkers?.();
    if (got) {
      const list = decodeMarkers(got);
      // The server's record replaces what this session had; markers placed before it arrived
      // are kept.
      if (list) {
        const before = this.markers.all.filter((m) => m.auto === '');
        this.markers.load(list);
        for (const m of before) this.markers.place(m, m);
      }
      this.markersReady = true;
      this.drawDisc();
    }
    const pose = Array.from(this.client.markerPose?.() ?? []);
    if (pose.length < 5) return;
    if (!this.markersReady) {
      if (this.client.markersSynced?.()) return;
      // A server without `markers.private`: wait a moment for the session's capabilities.
      if (this.markersWaitFrom < 0) this.markersWaitFrom = now;
      if (now - this.markersWaitFrom < 2000) return;
      this.markersReady = true;
    }
    const [face = 0, x = 0, y = 0, z = 0, dead = 0] = pose;
    if (!this.markers.hasAuto('spawn') && dead === 0)
      this.markers.noteAuto('spawn', { face, x, y, z });
    if (dead !== 0 && !this.wasDead) {
      const [, , , , , df = 0, dx = 0, dy = 0, dz = 0] = pose;
      this.markers.noteAuto('death', { face: df, x: dx, y: dy, z: dz });
    }
    this.wasDead = dead !== 0;
    this.drawWaypoints(face);
  }

  /** The waypoints over the markers' spots on the player's face (none while the big map is open). */
  private drawWaypoints(face: number): void {
    const screen = this.client.markerScreen?.bind(this.client);
    const shown = new Set<number>();
    const w = this.doc.defaultView?.innerWidth ?? 1000;
    const h = this.doc.defaultView?.innerHeight ?? 1000;
    if (screen && !this.bigOpen) {
      for (const m of this.markers.all) {
        if (m.face !== face) continue;
        // The marker's spot, a little over the ground it was placed on.
        const [nx = 0, ny = 0, depth = 0, dist = 0] = Array.from(
          screen(m.face, m.x, m.y + 1.5, m.z),
        );
        if (dist > MARKER_WORLD.showM) continue;
        shown.add(m.id);
        const el = this.waypoint(m);
        const at = waypointOnScreen(nx, ny, depth, w, h);
        el.style.transform = `translate(${at.x.toFixed(1)}px, ${at.y.toFixed(1)}px)`;
        el.style.opacity = waypointAlpha(dist).toFixed(2);
        el.classList.toggle('edge', !at.onScreen);
        const d = el.lastChild as HTMLSpanElement;
        d.textContent = formatMarkerDistance(dist);
      }
    }
    for (const [id, el] of this.waypointEls) {
      if (!shown.has(id)) {
        el.remove();
        this.waypointEls.delete(id);
      }
    }
  }

  /** A marker's waypoint element (made again when its look changed). */
  private waypoint(m: MapMarker): HTMLDivElement {
    const key = `${m.icon}|${m.rgb}|${m.label}`;
    const had = this.waypointEls.get(m.id);
    if (had?.dataset.key === key) return had;
    had?.remove();
    const el = this.doc.createElement('div');
    el.className = 'waypoint';
    el.dataset.key = key;
    const label = this.doc.createElement('span');
    label.className = 'waypoint-label';
    label.textContent = m.label;
    const dist = this.doc.createElement('span');
    dist.className = 'waypoint-distance';
    el.append(iconSvg(this.doc, m.icon, m.rgb, MARKER_WORLD.sizePx), label, dist);
    this.waypoints.append(el);
    this.waypointEls.set(m.id, el);
    return el;
  }

  /** A marker's spot on the open big map (pixels from its top left), or null off its face. */
  private markerPx(m: MapMarker): [number, number] | null {
    const v = this.state.view;
    if (!v || m.face !== v.face) return null;
    const c = this.bigCanvas;
    const [sx, sy] = toScreen(v, m.x, m.z);
    return [(c.width || 800) / 2 + sx, (c.height || 600) / 2 + sy];
  }

  /** The ground's height for a marker placed on the map: the player's when on their face. */
  private placeY(face: number): number {
    const [pf = 0, , py = 0] = Array.from(this.client.markerPose?.() ?? []);
    return pf === face ? py : 0;
  }

  /** A click on the open map: edit the marker under it, else place one there. */
  private mapClick(px: number, py: number): void {
    const v = this.state.view;
    if (!v) return;
    const hit = pickMarker(this.markers.all, (m) => this.markerPx(m), px, py);
    if (hit) {
      this.picker.openFor(hit, (r) => {
        if (r.kind === 'save') this.markers.edit(hit.id, r.look);
        else if (r.kind === 'delete') this.markers.remove(hit.id);
      });
      return;
    }
    if (this.markers.full) return;
    const c = this.bigCanvas;
    const [x, z] = toWorld(v, px - (c.width || 800) / 2, py - (c.height || 600) / 2);
    const face = v.face;
    this.picker.openFor(undefined, (r) => {
      if (r.kind === 'save') this.markers.place(r.look, { face, x, y: this.placeY(face), z });
    });
  }

  /** Tile (ti, tk) of a level's tinted image (null: nothing in it to draw), made again when stale. */
  private tileImage(cellM: number, face: number, ti: number, tk: number): HTMLCanvasElement | null {
    const key = RevealCache.key(cellM, face, ti, tk);
    const revealed = this.client.mapRevealed();
    if (!revealed && !this.exploredTiles.has(key)) return null;
    const had = this.tileImages.get(key);
    if (had !== undefined && !this.staleTiles.has(key)) return had;
    const n = this.reveal.tileCells;
    const g = tileGrid(
      face,
      ti,
      tk,
      n,
      (i, k) => {
        const c = this.levels.at(this.reveal, revealed, cellM, face, i, k);
        return c ? c[0] : c;
      },
      (i, k) => this.levels.at(this.reveal, revealed, cellM, face, i, k)?.[1] ?? NO_HEIGHT,
    );
    this.staleTiles.delete(key);
    if (!g.any) {
      this.tileImages.set(key, null);
      return null;
    }
    const rgba = this.client.shadeMap(g.colours, g.codes, n, n);
    const img = had ?? this.doc.createElement('canvas');
    img.width = img.height = n;
    img.getContext('2d')?.putImageData(new ImageData(new Uint8ClampedArray(rgba), n, n), 0, 0);
    this.tileImages.set(key, img);
    return img;
  }

  private drawBig(): void {
    const v = this.state.view;
    if (!v) return;
    this.lastBigDraw = performance.now();
    this.bigStale = false;
    const canvas = this.bigCanvas;
    const w = (canvas.width = Math.max(1, Math.round(canvas.clientWidth || 800)));
    const h = (canvas.height = Math.max(1, Math.round(canvas.clientHeight || 600)));
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.fillStyle = MAP.bigMap.fogColor;
    ctx.fillRect(0, 0, w, h);
    const cell = levelCellM(v.level);
    const n = this.reveal.tileCells;
    // The shaded tiles, a texel a cell, unsmoothed; made again within a budget a draw (the rest
    // the next frames), whole pixels on both edges (a scaled draw at a fractional corner left
    // seams).
    ctx.imageSmoothingEnabled = false;
    const t0 = performance.now();
    for (const [cellM, face, ti, tk] of this.reveal.tilesFor(v, w, h)) {
      const key = RevealCache.key(cellM, face, ti, tk);
      const fresh = !this.tileImages.has(key) || this.staleTiles.has(key);
      if (fresh && performance.now() - t0 > 8) this.bigStale = true;
      else this.tileImage(cellM, face, ti, tk);
      const img = this.tileImages.get(key);
      if (!img) continue;
      const [ax, ay] = toScreen(v, ti * n * cell, tk * n * cell);
      const [bx, by] = toScreen(v, (ti + 1) * n * cell, (tk + 1) * n * cell);
      const [x0, x1] = [Math.round(w / 2 + Math.min(ax, bx)), Math.round(w / 2 + Math.max(ax, bx))];
      const [y0, y1] = [Math.round(h / 2 + Math.min(ay, by)), Math.round(h / 2 + Math.max(ay, by))];
      ctx.drawImage(img, x0, y0, x1 - x0, y1 - y0);
    }
    // The debug layer over the ground (REALISTIC_WORLD.md R6): its grid over the face's disc,
    // mirrored as the face's view is (`toScreen`).
    const def = MAP_LAYERS.layers[this.layer];
    const layerImg = def ? this.layerImage(def, v.face) : null;
    if (layerImg) {
      const { x0, step } = layerSpan(worldRadiusM(), layerImg.width);
      const x1 = x0 + step * layerImg.width;
      const [ax, ay] = toScreen(v, x0, x0);
      const [bx, by] = toScreen(v, x1, x1);
      ctx.save();
      ctx.imageSmoothingEnabled = false;
      ctx.translate(w / 2 + ax, h / 2 + ay);
      ctx.scale(bx > ax ? 1 : -1, by > ay ? 1 : -1);
      ctx.drawImage(layerImg, 0, 0, Math.abs(bx - ax), Math.abs(by - ay));
      ctx.restore();
    }
    this.updateControls();
    // The markers on this face.
    for (const m of this.markers.all) {
      const p = this.markerPx(m);
      if (p) drawIcon(ctx, m.icon, m.rgb, p[0], p[1], 22);
    }
    const [face = 0, x = 0, z = 0, yaw = 0] = this.client.mapPose();
    if (face !== v.face) return;
    const [px, py] = toScreen(v, x, z);
    ctx.save();
    ctx.translate(w / 2 + px, h / 2 + py);
    ctx.rotate(headingOnScreen(face, yaw));
    ctx.beginPath();
    ctx.moveTo(0, -11);
    ctx.lineTo(7, 8);
    ctx.lineTo(0, 4);
    ctx.lineTo(-7, 8);
    ctx.closePath();
    ctx.fillStyle = MAP.minimap.playerColor;
    ctx.fill();
    ctx.strokeStyle = 'white';
    ctx.lineWidth = 1.5;
    ctx.stroke();
    ctx.restore();
  }

  /** The zoom buttons' limits and the scale bar for the view. */
  private updateControls(): void {
    this.zoomIn.disabled = !this.state.canZoom(1);
    this.zoomOut.disabled = !this.state.canZoom(-1);
    const s = this.state.scale();
    if (!s) return;
    (this.scaleEl.firstChild as HTMLElement).style.width = `${String(Math.round(s.px))}px`;
    this.scaleLabel.textContent = s.label;
  }

  private bindBig(): void {
    const c = this.bigCanvas;
    // Pointers down on the map: one drags, two pinch (the point between them kept under them).
    const down = new Map<number, { x: number; y: number }>();
    const local = (e: PointerEvent): [number, number] => {
      const r = c.getBoundingClientRect();
      return [e.clientX - r.left - r.width / 2, e.clientY - r.top - r.height / 2];
    };
    c.addEventListener('wheel', (e) => {
      e.preventDefault();
      const v = this.state.view;
      if (!v) return;
      const r = c.getBoundingClientRect();
      // A notch (or a line) of scroll is a step's worth; a trackpad's pixels add up to one.
      const px = e.deltaMode === 0 ? e.deltaY : Math.sign(e.deltaY) * MAP.bigMap.wheelStepPx;
      const steps = this.wheel.add(-px);
      this.zoomBy(steps, e.clientX - r.left - r.width / 2, e.clientY - r.top - r.height / 2);
    });
    // A press on a marker drags it; a press that doesn't move is a click (place or edit).
    let press: { id: number; x: number; y: number; moved: boolean; marker: number | null } | null =
      null;
    const px = (e: PointerEvent): [number, number] => {
      const r = c.getBoundingClientRect();
      return [e.clientX - r.left, e.clientY - r.top];
    };
    c.addEventListener('pointerdown', (e) => {
      const [x, y] = local(e);
      const [ax, ay] = px(e);
      press =
        down.size === 0
          ? {
              id: e.pointerId,
              x: ax,
              y: ay,
              moved: false,
              marker: pickMarker(this.markers.all, (m) => this.markerPx(m), ax, ay)?.id ?? null,
            }
          : null;
      down.set(e.pointerId, { x, y });
      c.setPointerCapture(e.pointerId);
      c.classList.add('dragging');
      this.pinch.reset();
    });
    c.addEventListener('pointermove', (e) => {
      const was = down.get(e.pointerId);
      const v = this.state.view;
      if (!was || !v) return;
      const [x, y] = local(e);
      if (press?.id === e.pointerId) {
        const [ax, ay] = px(e);
        if (Math.hypot(ax - press.x, ay - press.y) > 4) press.moved = true;
        if (press.moved && press.marker !== null) {
          // Dragging a marker moves it (drawn as it goes, saved once on release).
          const m = this.markers.get(press.marker);
          const r = c.getBoundingClientRect();
          if (m) {
            const [wx, wz] = toWorld(v, ax - r.width / 2, ay - r.height / 2);
            Object.assign(m, { x: wx, z: wz });
          }
          down.set(e.pointerId, { x, y });
          this.drawBig();
          return;
        }
      }
      if (down.size >= 2) {
        const other = [...down.entries()].find(([id]) => id !== e.pointerId)?.[1];
        if (other) {
          const before = Math.hypot(was.x - other.x, was.y - other.y);
          const after = Math.hypot(x - other.x, y - other.y);
          const [mx, my] = [(x + other.x) / 2, (y + other.y) / 2];
          this.state.view = pan(v, (x - was.x) / 2, (y - was.y) / 2);
          // A pinch steps a level each `pinchStep` of spread or squeeze.
          if (before > 4) this.zoomBy(this.pinch.add(Math.log(after / before)), mx, my);
        }
      } else {
        this.state.view = pan(v, x - was.x, y - was.y);
      }
      down.set(e.pointerId, { x, y });
      this.drawBig();
    });
    const up = (e: PointerEvent) => {
      down.delete(e.pointerId);
      if (down.size === 0) c.classList.remove('dragging');
    };
    c.addEventListener('pointerup', (e) => {
      const p = press;
      press = null;
      up(e);
      if (p?.id !== e.pointerId) return;
      const m = p.marker === null ? undefined : this.markers.get(p.marker);
      if (p.moved && m) this.markers.move(m.id, { ...m, y: this.placeY(m.face) });
      else if (!p.moved) this.mapClick(p.x, p.y);
    });
    c.addEventListener('pointercancel', (e) => {
      press = null;
      up(e);
    });
  }
}
