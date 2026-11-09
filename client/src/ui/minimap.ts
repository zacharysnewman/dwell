// The minimap and the explored map (docs/adr/0068-minimap-and-explored-map.md; owner,
// 2026-11-08). The client crate draws the round minimap (`minimapFrame`: the heading up, a texel a
// metre, drawn again only on a move or a turn) and reports the explored cells it covers
// (`takeExplored`); this shell shows the disc with the player's and the others' dots, keeps the
// explored cells per world in this browser (IndexedDB, append-only), and draws them on the big map
// (M, or a tap on the minimap): zoomable and pannable, unexplored ground fogged, the player's place
// and heading on it. Its looks and sizes are data (shared/ui/map.json → map.gen.ts). In a mode
// with the `mapRevealed` ability (creative) nothing is fogged: ground not yet explored takes its
// colour from the terrain generator, a tile at a time in the job workers (`RevealCache`).

import { GAMEPAD } from '../input/gamepad.gen';
import { MAP } from './map.gen';

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
  pxPerM: number;
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

/** The zoom's range in pixels a metre (the data's pixels a cell over the cell's size). */
export function zoomRange(): [number, number] {
  const c = MAP.explored.cellM;
  return [MAP.bigMap.minPxPerCell / c, MAP.bigMap.maxPxPerCell / c];
}

/** Zoomed by `factor` about pixels (sx, sy) from the centre: the point under them stays put. */
export function zoomAt(v: BigView, factor: number, sx: number, sy: number): BigView {
  const [lo, hi] = zoomRange();
  const pxPerM = Math.min(hi, Math.max(lo, v.pxPerM * factor));
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

/** What the revealed map needs from the client (`crates/dwell-client/src/map_reveal.rs`). */
export interface RevealClient {
  requestReveal(face: number, ti: number, tk: number): void;
  takeReveal(): Uint8Array | undefined;
}

/**
 * A finished tile: face, tile indices, its RGBA (null: no terrain to reveal from) and its cells'
 * height codes (null: none sent).
 */
export interface RevealTile {
  face: number;
  ti: number;
  tk: number;
  rgba: Uint8Array | null;
  heights: Int16Array | null;
}

/**
 * `takeReveal`'s bytes: face, ti and tk (i32 LE), then the tile's RGBA rows of x from its least z,
 * then (when there) its `n`² i16 LE height codes in the same order.
 */
export function parseRevealTile(b: Uint8Array, n: number = MAP.revealed.tileCells): RevealTile {
  const d = new DataView(b.buffer, b.byteOffset, b.byteLength);
  const colours = n * n * 4;
  let heights: Int16Array | null = null;
  if (b.length >= 9 + colours + n * n * 2) {
    heights = new Int16Array(n * n);
    for (let j = 0; j < n * n; j++) heights[j] = d.getInt16(9 + colours + j * 2, true);
  }
  return {
    face: b[0] ?? 0,
    ti: d.getInt32(1, true),
    tk: d.getInt32(5, true),
    rgba: b.length > 9 ? b.subarray(9, Math.min(b.length, 9 + colours)) : null,
    heights,
  };
}

/**
 * The revealed big map's tiles (ADR 0068; owner, 2026-11-08): `revealed.tileCells`² explored cells
 * each, generated in the job workers from the seed and kept for the session. Each frame
 * `pump` takes finished tiles for at most `budgetMs` and asks for the missing tiles of the view,
 * nearest its centre first, at most `jobsInFlight` at once: opening or zooming the map never
 * waits for generation.
 */
export class RevealCache {
  readonly tiles = new Map<string, RevealTile>();
  private readonly pending = new Set<string>();
  /** Tiles still to ask for, the nearest last (the last view's). */
  private wanted: [number, number, number][] = [];
  private wantedFor = '';

  constructor(
    readonly tileCells: number = MAP.revealed.tileCells,
    readonly jobsInFlight: number = MAP.revealed.jobsInFlight,
    readonly budgetMs: number = MAP.revealed.budgetMs,
  ) {}

  /** A tile's key. */
  static key(face: number, ti: number, tk: number): string {
    return cellKey(face, ti, tk);
  }

  /** The tiles a `w` × `h` view covers, [face, ti, tk] each, nearest its centre first. */
  tilesFor(v: BigView, w: number, h: number): [number, number, number][] {
    const span = this.tileCells * MAP.explored.cellM;
    const [ax, az] = toWorld(v, -w / 2, -h / 2);
    const [bx, bz] = toWorld(v, w / 2, h / 2);
    const [i0, i1] = [Math.floor(Math.min(ax, bx) / span), Math.floor(Math.max(ax, bx) / span)];
    const [k0, k1] = [Math.floor(Math.min(az, bz) / span), Math.floor(Math.max(az, bz) / span)];
    const out: [number, number, number][] = [];
    for (let k = k0; k <= k1; k++) for (let i = i0; i <= i1; i++) out.push([v.face, i, k]);
    const d = ([, i, k]: [number, number, number]) =>
      ((i + 0.5) * span - v.cx) ** 2 + ((k + 0.5) * span - v.cz) ** 2;
    return out.sort((a, b) => d(a) - d(b));
  }

  /**
   * One frame's fill: takes finished tiles (handing each to `onTile`) while under `budgetMs` by
   * `now`, then asks for the view's missing tiles up to `jobsInFlight` pending. The tiles taken.
   */
  pump(
    client: RevealClient,
    v: BigView,
    w: number,
    h: number,
    now: () => number,
    onTile?: (t: RevealTile) => void,
  ): number {
    const t0 = now();
    let taken = 0;
    while (now() - t0 < this.budgetMs) {
      const bytes = client.takeReveal();
      if (!bytes) break;
      const t = parseRevealTile(bytes);
      const key = RevealCache.key(t.face, t.ti, t.tk);
      this.pending.delete(key);
      this.tiles.set(key, t);
      onTile?.(t);
      taken++;
    }
    const at = [v.face, v.cx, v.cz, v.pxPerM, w, h].join(',');
    if (at !== this.wantedFor) {
      this.wantedFor = at;
      this.wanted = this.tilesFor(v, w, h).reverse();
    }
    while (this.pending.size < this.jobsInFlight && this.wanted.length > 0) {
      const [face, ti, tk] = this.wanted.pop() ?? [0, 0, 0];
      const key = RevealCache.key(face, ti, tk);
      if (this.tiles.has(key) || this.pending.has(key)) continue;
      this.pending.add(key);
      client.requestReveal(face, ti, tk);
    }
    return taken;
  }

  /** Tiles asked for and not yet back. */
  get inFlight(): number {
    return this.pending.size;
  }

  /**
   * Explored cell (i, k)'s revealed colour: 0xRRGGBB, null beyond the world's edge (or in a world
   * the client does not generate), undefined while its tile is still to come.
   */
  colour(face: number, i: number, k: number): number | null | undefined {
    const n = this.tileCells;
    const [ti, tk] = [Math.floor(i / n), Math.floor(k / n)];
    const t = this.tiles.get(RevealCache.key(face, ti, tk));
    if (!t) return undefined;
    if (!t.rgba) return null;
    const o = ((k - tk * n) * n + (i - ti * n)) * 4;
    if ((t.rgba[o + 3] ?? 0) === 0) return null;
    return ((t.rgba[o] ?? 0) << 16) | ((t.rgba[o + 1] ?? 0) << 8) | (t.rgba[o + 2] ?? 0);
  }

  /** Explored cell (i, k)'s revealed height code, NO_HEIGHT where its tile has none (yet). */
  height(face: number, i: number, k: number): number {
    const n = this.tileCells;
    const [ti, tk] = [Math.floor(i / n), Math.floor(k / n)];
    const h = this.tiles.get(RevealCache.key(face, ti, tk))?.heights;
    const v = h?.[(k - tk * n) * n + (i - ti * n)];
    return v === undefined || v === -32768 ? NO_HEIGHT : v;
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

/**
 * What the big map draws at explored cell (i, k): its explored colour, else (the map revealed)
 * the generator's, or null for nothing there (beyond the world's edge); undefined is fog.
 */
export function cellColour(
  cells: ExploredCells,
  reveal: RevealCache | null,
  face: number,
  i: number,
  k: number,
): number | null | undefined {
  const c = cells.get(cellKey(face, i, k));
  if (c !== undefined) return c;
  return reveal?.colour(face, i, k);
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
export interface MapClient extends RevealClient {
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
   * The big map's cells shaded by height (`dwell_game::minimap::shade_grid`, the minimap's own
   * shading): a `w` × `h` screen-order grid inside a one-cell border; RGBA out.
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
  /** The zoom factor this frame (1: none). */
  zoom: number;
  /** The pan this frame, screen pixels the view moves (right, down). */
  panX: number;
  panY: number;
}

/**
 * The controller on the open map (docs/GAMEPAD.md; buttons as data, `bigMap.pad*`): the left
 * stick and the D-pad pan, the zoom buttons zoom while held, B (and View) close, Y centres. A
 * button held when the map opened is not a press until it lets go.
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
    let zoom = 1;
    if (held(b.padZoomIn) || held(b.padZoomInAlt)) zoom *= b.padZoomPerS ** dt;
    if (held(b.padZoomOut) || held(b.padZoomOutAlt)) zoom /= b.padZoomPerS ** dt;
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
 * + and − zoom, "Center on me", a scale bar, drag to pan): open or closed, the view, and what each
 * control does to it.
 */
export class BigMapState {
  view: BigView | null = null;
  readonly pad = new PadMap();

  constructor(private readonly pose: () => ArrayLike<number>) {}

  get open(): boolean {
    return this.view !== null;
  }

  /** Opens it centred on the player at the starting zoom. */
  openAt(): void {
    const [face = 0, x = 0, z = 0] = Array.from(this.pose());
    this.view = { face, cx: x, cz: z, pxPerM: MAP.bigMap.startPxPerCell / MAP.explored.cellM };
    this.pad.reset();
  }

  close(): void {
    this.view = null;
  }

  /** One zoom step about the centre (+1 in, −1 out), within the zoom's range. */
  zoomStep(dir: number): void {
    if (!this.view) return;
    const f = dir > 0 ? MAP.bigMap.zoomStep : 1 / MAP.bigMap.zoomStep;
    this.view = zoomAt(this.view, f, 0, 0);
  }

  /** Whether zooming in (+1) or out (−1) can go further. */
  canZoom(dir: number): boolean {
    if (!this.view) return false;
    const [lo, hi] = zoomRange();
    return dir > 0 ? this.view.pxPerM < hi - 1e-9 : this.view.pxPerM > lo + 1e-9;
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
    if (s.zoom !== 1) {
      this.view = zoomAt(this.view, s.zoom, 0, 0);
      moved = true;
    }
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

/** The tiles whose images show explored cell (i, k) (its own, and those it borders). */
export function tilesShowing(face: number, i: number, k: number, n: number): string[] {
  const out = new Set<string>();
  for (const di of [-1, 0, 1]) {
    for (const dk of [-1, 0, 1]) {
      out.add(RevealCache.key(face, Math.floor((i + di) / n), Math.floor((k + dk) / n)));
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
  private cells: ExploredCells = new Map();
  private heights: ExploredHeights = new Map();
  private dirty = false;
  private lastSave = 0;
  private dots: ArrayLike<number> = [];
  private lastUpdate = -Infinity;
  private readonly cpu: boolean;
  /** The revealed map's tiles (a mode with `mapRevealed`). */
  private readonly reveal = new RevealCache();
  /** The big map's shaded tile images, and those to make again. */
  private readonly tileImages = new Map<string, HTMLCanvasElement | null>();
  private readonly staleTiles = new Set<string>();
  /** The tiles with explored cells in them. */
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
  ) {
    this.cpu = client.cpuAdapter();
    this.state = new BigMapState(() => client.mapPose());
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
    hint.textContent = 'Drag to move · scroll, pinch or + − to zoom · M or Esc to close';
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
      this.state.zoomStep(1);
      this.onZoom?.();
      this.drawBig();
    });
    this.zoomOut.addEventListener('click', () => {
      this.state.zoomStep(-1);
      this.onZoom?.();
      this.drawBig();
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
      this.panel.classList.toggle('phone', p.phone);
      if (this.bigOpen) this.drawBig();
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
    if (code === 'Equal' || code === 'NumpadAdd') this.state.zoomStep(1);
    else if (code === 'Minus' || code === 'NumpadSubtract') this.state.zoomStep(-1);
    if (code === 'Equal' || code === 'NumpadAdd' || code === 'Minus' || code === 'NumpadSubtract') {
      this.onZoom?.();
    } else if (code === 'KeyC') this.state.center();
    else return false;
    this.drawBig();
    return true;
  }

  /** The controller while the big map is open (docs/GAMEPAD.md). */
  padFrame(values: ArrayLike<number>, axes: ArrayLike<number>, now: number): void {
    if (!this.bigOpen) return;
    const r = this.state.padFrame(values, axes, now);
    if (r.closed) {
      this.big.hidden = true;
      this.onOpenChange?.(false);
    } else if (r.moved) this.drawBig();
  }

  /** A cell's tile has explored cells (drawn without the reveal) and its images are stale. */
  private noteCell(key: string): void {
    const [face = 0, i = 0, k = 0] = key.split(',').map(Number);
    const n = this.reveal.tileCells;
    this.exploredTiles.add(RevealCache.key(face, Math.floor(i / n), Math.floor(k / n)));
    for (const t of tilesShowing(face, i, k, n)) this.staleTiles.add(t);
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

  /** The open revealed map's tiles: one frame's budget of them (drawn at most every `redrawMs`). */
  private fillReveal(): void {
    const v = this.state.view;
    if (this.big.hidden || !v || !this.client.mapRevealed()) return;
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
            this.staleTiles.add(RevealCache.key(t.face, t.ti + di, t.tk + dk));
          }
        }
      },
    );
    if (taken > 0) this.bigStale = true;
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

  /** Tile (ti, tk)'s shaded image (null: nothing in it to draw), made again when stale. */
  private tileImage(face: number, ti: number, tk: number): HTMLCanvasElement | null {
    const key = RevealCache.key(face, ti, tk);
    const revealed = this.client.mapRevealed();
    if (!revealed && !this.exploredTiles.has(key)) return null;
    const had = this.tileImages.get(key);
    if (had !== undefined && !this.staleTiles.has(key)) return had;
    const n = this.reveal.tileCells;
    const reveal = revealed ? this.reveal : null;
    const g = tileGrid(
      face,
      ti,
      tk,
      n,
      (i, k) => cellColour(this.cells, reveal, face, i, k),
      (i, k) => {
        const key = cellKey(face, i, k);
        return this.heights.get(key) ?? (reveal ? reveal.height(face, i, k) : NO_HEIGHT);
      },
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
    const cell = MAP.explored.cellM;
    const n = this.reveal.tileCells;
    // The shaded tiles, a texel a cell, unsmoothed; made again within a budget a draw (the rest
    // the next frames), whole pixels on both edges (a scaled draw at a fractional corner left
    // seams).
    ctx.imageSmoothingEnabled = false;
    const t0 = performance.now();
    for (const [face, ti, tk] of this.reveal.tilesFor(v, w, h)) {
      const key = RevealCache.key(face, ti, tk);
      const fresh = !this.tileImages.has(key) || this.staleTiles.has(key);
      if (fresh && performance.now() - t0 > 8) this.bigStale = true;
      else this.tileImage(face, ti, tk);
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
      const factor = e.deltaY < 0 ? MAP.bigMap.zoomStep : 1 / MAP.bigMap.zoomStep;
      this.state.view = zoomAt(
        v,
        factor,
        e.clientX - r.left - r.width / 2,
        e.clientY - r.top - r.height / 2,
      );
      this.drawBig();
    });
    c.addEventListener('pointerdown', (e) => {
      const [x, y] = local(e);
      down.set(e.pointerId, { x, y });
      c.setPointerCapture(e.pointerId);
      c.classList.add('dragging');
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
          let next = pan(v, (x - was.x) / 2, (y - was.y) / 2);
          if (before > 4) next = zoomAt(next, after / before, mx, my);
          this.state.view = next;
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
