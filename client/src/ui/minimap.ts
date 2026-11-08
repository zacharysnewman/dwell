// The minimap and the explored map (docs/adr/0068-minimap-and-explored-map.md; owner,
// 2026-11-08). The client crate draws the round minimap (`minimapFrame`: the heading up, a texel a
// metre, drawn again only on a move or a turn) and reports the explored cells it covers
// (`takeExplored`); this shell shows the disc with the player's and the others' dots, keeps the
// explored cells per world in this browser (IndexedDB, append-only), and draws them on the big map
// (M, or a tap on the minimap): zoomable and pannable, unexplored ground fogged, the player's place
// and heading on it. Its looks and sizes are data (shared/ui/map.json → map.gen.ts).

import { MAP } from './map.gen';

/** An explored cell's key: its face (0 A, 1 B) and its index along face-local x and z. */
export function cellKey(face: number, i: number, k: number): string {
  return `${String(face)},${String(i)},${String(k)}`;
}

/** The explored cells of one world: key → 0xRRGGBB. */
export type ExploredCells = Map<string, number>;

/** Adds `takeExplored`'s flat [face, i, k, rgb]* to the cells; whether any changed. */
export function addExplored(cells: ExploredCells, flat: ArrayLike<number>): boolean {
  let changed = false;
  for (let n = 0; n + 3 < flat.length; n += 4) {
    const key = cellKey(flat[n] ?? 0, flat[n + 1] ?? 0, flat[n + 2] ?? 0);
    const rgb = (flat[n + 3] ?? 0) & 0xffffff;
    if (cells.get(key) !== rgb) {
      cells.set(key, rgb);
      changed = true;
    }
  }
  return changed;
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
export function exploredRecord(stored: unknown, cells: ExploredCells): Record<string, unknown> {
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
  return {
    ...base,
    format: base.format ?? EXPLORED_FORMAT,
    cellM: MAP.explored.cellM,
    cells: { ...kept, ...ours },
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

const hex = (c: number) => `#${c.toString(16).padStart(6, '0')}`;

/** What the map needs from the client (the wasm `Client`). */
export interface MapClient {
  minimapFrame(nowMs: number, shading: boolean): Uint8Array | undefined;
  minimapPixels(): number;
  minimapDots(): Float64Array;
  mapPose(): Float64Array;
  takeExplored(): Int32Array;
  mapAllowed(): boolean;
}

/** The minimap and the big map on the page. */
export class MapUi {
  readonly root: HTMLDivElement;
  private readonly disc: HTMLCanvasElement;
  private readonly image: HTMLCanvasElement;
  private readonly big: HTMLDivElement;
  private readonly bigCanvas: HTMLCanvasElement;
  private view: BigView | null = null;
  private cells: ExploredCells = new Map();
  private dirty = false;
  private lastSave = 0;
  private lastDots = 0;
  /** The disc's last draw (ms), for the bench's cost readout. */
  costMs = 0;
  shading: boolean = MAP.heightShading.default;

  constructor(
    private readonly doc: Document,
    private readonly client: MapClient,
    private readonly world: string,
    private readonly store: ExploredStore | null,
  ) {
    const px = client.minimapPixels();
    this.root = doc.createElement('div');
    this.root.id = 'minimap';
    this.root.title = 'Map (M)';
    this.disc = doc.createElement('canvas');
    this.disc.width = this.disc.height = px;
    this.root.append(this.disc);
    this.image = doc.createElement('canvas');
    this.image.width = this.image.height = px;
    this.big = doc.createElement('div');
    this.big.id = 'world-map';
    this.big.hidden = true;
    this.bigCanvas = doc.createElement('canvas');
    this.big.append(this.bigCanvas);
    doc.body.append(this.root, this.big);
    this.root.addEventListener('pointerup', (e) => {
      e.stopPropagation();
      this.toggleBig();
    });
    this.bindBig();
    void store?.get(world).then((r) => {
      for (const [k, v] of parseExplored(r)) if (!this.cells.has(k)) this.cells.set(k, v);
    });
  }

  /** Whether the big map is open. */
  get bigOpen(): boolean {
    return !this.big.hidden;
  }

  toggleBig(): void {
    if (!this.client.mapAllowed()) return;
    this.big.hidden = !this.big.hidden;
    if (!this.big.hidden) {
      const [face = 0, x = 0, z = 0] = this.client.mapPose();
      const c = MAP.explored.cellM;
      this.view = { face, cx: x, cz: z, pxPerM: MAP.bigMap.startPxPerCell / c };
      if (this.doc.pointerLockElement) this.doc.exitPointerLock();
      this.drawBig();
    }
  }

  /** Each frame: the disc when the client drew it again, the dots, the explored cells. */
  frame(now: number): void {
    const allowed = this.client.mapAllowed();
    this.root.classList.toggle('map-off', !allowed);
    this.doc.body.classList.toggle('minimap-on', allowed);
    if (!allowed) {
      this.big.hidden = true;
      return;
    }
    const t0 = performance.now();
    const pixels = this.client.minimapFrame(now, this.shading);
    if (pixels) {
      const px = this.image.width;
      const ctx = this.image.getContext('2d');
      ctx?.putImageData(new ImageData(new Uint8ClampedArray(pixels), px, px), 0, 0);
      this.costMs = performance.now() - t0;
    }
    if (pixels || now - this.lastDots > 100) {
      this.lastDots = now;
      this.drawDisc();
    }
    if (addExplored(this.cells, this.client.takeExplored())) {
      this.dirty = true;
      if (!this.big.hidden) this.drawBig();
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
      .then((r) => store.put(this.world, exploredRecord(r, this.cells)))
      .catch(() => {
        this.dirty = true;
      });
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
    const d = this.client.minimapDots();
    for (let n = 0; n + 1 < d.length; n += 2) {
      dot(half + (d[n] ?? 0) * perM, half - (d[n + 1] ?? 0) * perM, MAP.minimap.otherColor);
    }
    dot(half, half, MAP.minimap.playerColor);
  }

  private drawBig(): void {
    const v = this.view;
    if (!v) return;
    const canvas = this.bigCanvas;
    const w = (canvas.width = Math.max(1, Math.round(canvas.clientWidth || 800)));
    const h = (canvas.height = Math.max(1, Math.round(canvas.clientHeight || 600)));
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.fillStyle = MAP.bigMap.fogColor;
    ctx.fillRect(0, 0, w, h);
    const cell = MAP.explored.cellM;
    const size = cell * v.pxPerM;
    const [ax, az] = toWorld(v, -w / 2, -h / 2);
    const [bx, bz] = toWorld(v, w / 2, h / 2);
    const [i0, i1] = [Math.floor(Math.min(ax, bx) / cell), Math.floor(Math.max(ax, bx) / cell)];
    const [k0, k1] = [Math.floor(Math.min(az, bz) / cell), Math.floor(Math.max(az, bz) / cell)];
    const s = rightSign(v.face);
    for (let k = k0; k <= k1; k++) {
      for (let i = i0; i <= i1; i++) {
        const c = this.cells.get(cellKey(v.face, i, k));
        if (c === undefined) continue;
        // The cell's corner nearest the screen's top left.
        const [sx, sy] = toScreen(v, (s > 0 ? i : i + 1) * cell, (k + 1) * cell);
        ctx.fillStyle = hex(c);
        ctx.fillRect(
          Math.floor(w / 2 + sx),
          Math.floor(h / 2 + sy),
          Math.ceil(size),
          Math.ceil(size),
        );
      }
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

  private bindBig(): void {
    let drag: { x: number; y: number } | null = null;
    const c = this.bigCanvas;
    c.addEventListener('wheel', (e) => {
      e.preventDefault();
      if (!this.view) return;
      const r = c.getBoundingClientRect();
      const factor = e.deltaY < 0 ? MAP.bigMap.zoomStep : 1 / MAP.bigMap.zoomStep;
      this.view = zoomAt(
        this.view,
        factor,
        e.clientX - r.left - r.width / 2,
        e.clientY - r.top - r.height / 2,
      );
      this.drawBig();
    });
    c.addEventListener('pointerdown', (e) => {
      drag = { x: e.clientX, y: e.clientY };
      c.setPointerCapture(e.pointerId);
    });
    c.addEventListener('pointermove', (e) => {
      if (!drag || !this.view) return;
      this.view = pan(this.view, e.clientX - drag.x, e.clientY - drag.y);
      drag = { x: e.clientX, y: e.clientY };
      this.drawBig();
    });
    c.addEventListener('pointerup', () => {
      drag = null;
    });
    this.big.addEventListener('dblclick', () => {
      this.toggleBig();
    });
  }
}
