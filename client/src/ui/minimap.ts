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

import { GAMEPAD } from '../input/gamepad.gen';
import { MAP } from './map.gen';
import { compatibilityLine } from '../version/semver';
import { World } from '../protocol/constants.gen';

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

/** The record's format (append-only: unknown fields and cells kept on rewrite). */
export const EXPLORED_FORMAT = 1;

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

/**
 * The record to store: the stored one's other fields kept, its cells merged under ours (a newer
 * build's or another tab's cells survive), or, for a record of another cell size, left as it was
 * with ours kept beside it under `cellsByM`.
 */
export function exploredRecord(
  stored: unknown,
  cells: ExploredCells,
  heights: ExploredHeights = new Map(),
): Record<string, unknown> {
  const base =
    typeof stored === 'object' && stored !== null && !Array.isArray(stored)
      ? (stored as Record<string, unknown>)
      : {};
  const ours = Object.fromEntries(cells);
  if (base.cellM !== undefined && base.cellM !== MAP.explored.cellM) {
    const by =
      typeof base.cellsByM === 'object' && base.cellsByM !== null
        ? (base.cellsByM as Record<string, unknown>)
        : {};
    return { ...base, cellsByM: { ...by, [String(MAP.explored.cellM)]: ours } };
  }
  const kept =
    typeof base.cells === 'object' && base.cells !== null && !Array.isArray(base.cells)
      ? (base.cells as Record<string, unknown>)
      : {};
  const keptHeights =
    typeof base.heights === 'object' && base.heights !== null && !Array.isArray(base.heights)
      ? (base.heights as Record<string, unknown>)
      : {};
  return {
    ...base,
    format: base.format ?? EXPLORED_FORMAT,
    cellM: MAP.explored.cellM,
    cells: { ...kept, ...ours },
    heights: { ...keptHeights, ...Object.fromEntries(heights) },
  };
}

/** A store of records by world (IndexedDB in the browser, a memory one in tests). */
export interface ExploredStore {
  get(world: string): Promise<unknown>;
  put(world: string, record: unknown): Promise<void>;
}

/** The browser's store: `dwell-explored`, one record per world key. Null where there is none. */
export function indexedDbExplored(): ExploredStore | null {
  if (typeof indexedDB === 'undefined') return null;
  const open = (): Promise<IDBDatabase> =>
    new Promise((resolve, reject) => {
      const req = indexedDB.open('dwell-explored', 1);
      req.onupgradeneeded = () => req.result.createObjectStore('worlds');
      req.onsuccess = () => {
        resolve(req.result);
      };
      req.onerror = () => {
        reject(req.error ?? new Error('indexedDB'));
      };
    });
  const run = <T>(mode: IDBTransactionMode, f: (s: IDBObjectStore) => IDBRequest<T>) =>
    open().then(
      (db) =>
        new Promise<T>((resolve, reject) => {
          const req = f(db.transaction('worlds', mode).objectStore('worlds'));
          req.onsuccess = () => {
            resolve(req.result);
          };
          req.onerror = () => {
            reject(req.error ?? new Error('indexedDB'));
          };
        }),
    );
  return {
    get: (world) => run<unknown>('readonly', (s) => s.get(world)),
    put: (world, record) => run('readwrite', (s) => s.put(record, world)).then(() => undefined),
  };
}

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

/** The zoom levels, closest first (shared/ui/map.json): each its cell side and span (m). */
export const LEVELS: readonly { cellM: number; spanM: number }[] = MAP.bigMap.levels;

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
  return World.worldRadius;
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
 * read and the view has what it needs, builds the background's (`backgroundJobs` at once): each
 * level's round the player, the widest level's the whole world disc. Opening or zooming the map never
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
  /** The background's tiles still to ask for, the first last. */
  private background: TileAt[] = [];
  private backgroundAt = '';
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

  /**
   * The background's tiles round a player at (x, z) on `face`: each level's within
   * `prebuildSpans` of its span (a square, so the widest level's is the whole world disc), those
   * touching the disc only (no holes inside the world); the start level first, then outward, nearest first in each.
   */
  backgroundFor(face: number, x: number, z: number): TileAt[] {
    const start = MAP.bigMap.startLevel;
    const order = LEVELS.map((_, l) => l).sort(
      (a, b) => Math.abs(a - start) - Math.abs(b - start) || b - a,
    );
    const out: TileAt[] = [];
    for (const l of order) {
      const { cellM, spanM } = LEVELS[l] ?? { cellM: 1, spanM: 1 };
      const span = this.span(cellM);
      const half = (spanM * MAP.revealed.prebuildSpans) / 2;
      const [i0, i1] = [Math.floor((x - half) / span), Math.floor((x + half) / span)];
      const [k0, k1] = [Math.floor((z - half) / span), Math.floor((z + half) / span)];
      const level: TileAt[] = [];
      for (let k = k0; k <= k1; k++) {
        for (let i = i0; i <= i1; i++)
          if (this.onWorld(cellM, i, k)) level.push([cellM, face, i, k]);
      }
      const d = ([, , i, k]: TileAt) => ((i + 0.5) * span - x) ** 2 + ((k + 0.5) * span - z) ** 2;
      out.push(...level.sort((a, b) => d(a) - d(b)));
    }
    return out;
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
   * One frame's fill: takes finished tiles (keeping each, handing it to `onTile`) while under
   * `budgetMs` by `now`; asks for the open view `v`'s missing tiles up to `jobsInFlight` pending;
   * then, with the kept tiles read and the view whole, the background's round the player `pose`
   * ([face, x, z]) up to `backgroundJobs` pending. The tiles taken.
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
    if (!this.loaded || this.noTerrain || !pose || (v && this.wanted.length > 0)) return taken;
    const [face = 0, x = 0, z = 0] = Array.from(pose);
    // Again once the player has moved a closest-level tile.
    const step = this.span(levelCellM(0));
    const at = [face, Math.floor(x / step), Math.floor(z / step)].join(',');
    if (at !== this.backgroundAt) {
      this.backgroundAt = at;
      this.background = this.backgroundFor(face, x, z).reverse();
    }
    while (this.pending.size < this.backgroundJobs && this.background.length > 0) {
      const t = this.background.pop();
      if (t && !this.known(t)) this.ask(client, t);
    }
    return taken;
  }

  /** Tiles asked for and not yet back. */
  get inFlight(): number {
    return this.pending.size;
  }

  /** The background's tiles still to build (after the last pump). */
  get backgroundLeft(): number {
    return this.background.filter((t) => !this.known(t)).length;
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
export interface MapClient extends RevealClient, LiveClient {
  minimapFrame(nowMs: number, shading: boolean): Uint8Array | undefined;
  minimapPixels(): number;
  minimapDots(): Float64Array;
  mapPose(): Float64Array;
  takeExplored(): Int32Array;
  mapAllowed(): boolean;
  /** The `mapRevealed` ability (creative): the big map shows everything. */
  mapRevealed(): boolean;
  cpuAdapter(): boolean;
  /**
   * The big map's cells tinted by height (`dwell_game::minimap::shade_grid`, the minimap's look
   * when its toggle is on): a `w` × `h` screen-order grid inside a one-cell border; RGBA out.
   */
  shadeMap(colours: Uint32Array, codes: Int32Array, w: number, h: number): Uint8Array;
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
  readonly state: BigMapState;
  private readonly cells: ExploredCells = new Map();
  private readonly heights: ExploredHeights = new Map();
  /** The explored cells at each zoom level (the fog). */
  private readonly levels = new ExploredLevels(this.cells, this.heights);
  private dirty = false;
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
  /** The disc's last draw (ms), for the bench's cost readout. */
  costMs = 0;
  shading: boolean = MAP.heightShading.default;
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
    this.reveal = new RevealCache(
      MAP.revealed.tileCells,
      MAP.revealed.jobsInFlight,
      MAP.revealed.budgetMs,
      MAP.revealed.backgroundJobs,
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
    hint.textContent = 'Drag to move · scroll, pinch or + − to zoom a level · M or Esc to close';
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
    controls.append(this.zoomIn, this.zoomOut, centre);
    this.scaleEl = doc.createElement('div');
    this.scaleEl.className = 'map-scale';
    this.scaleLabel = doc.createElement('span');
    this.scaleEl.append(doc.createElement('i'), this.scaleLabel);
    body.append(this.bigCanvas, controls, this.scaleEl);
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
    } else return false;
    return true;
  }

  /** The controller while the big map is open (docs/GAMEPAD.md). */
  padFrame(values: ArrayLike<number>, axes: ArrayLike<number>, now: number): void {
    if (!this.bigOpen) return;
    const level = this.state.view?.level;
    const r = this.state.padFrame(values, axes, now);
    if (this.state.view && this.state.view.level !== level) this.onZoom?.();
    if (r.closed) {
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
      for (const k of changed) this.noteCell(k);
      this.dirty = true;
      if (this.bigOpen) this.bigStale = true;
    }
    if (this.dirty && now - this.lastSave > 10_000) this.save(now);
  }

  /** Keeps the explored cells now (also on leaving the page). */
  save(now: number): void {
    if (!this.store || !this.dirty) return;
    this.dirty = false;
    this.lastSave = now;
    const store = this.store;
    void store
      .get(this.world)
      .then((r) => store.put(this.world, exploredRecord(r, this.cells, this.heights)))
      .catch(() => {
        this.dirty = true;
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
    dot(half, half, MAP.minimap.playerColor);
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
    this.updateControls();
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
    c.addEventListener('pointerdown', (e) => {
      const [x, y] = local(e);
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
    c.addEventListener('pointerup', up);
    c.addEventListener('pointercancel', up);
  }
}
