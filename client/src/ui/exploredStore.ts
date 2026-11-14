// The explored map's store (ADR 0068): one record per world in IndexedDB `dwell-explored`, its
// cells and heights as objects keyed by `cellKey`, append-only across builds. A save hands the
// store only the cells changed since the last one; the store merges them into the stored record.
// The merge reads and writes the whole record, which grows with the world explored (100 000
// cells, a few hours' play, ~150 ms of structured clone), so the browser's store does it in its
// own worker (`exploredWorker.ts`), off the frame (0.52.3: a stall every ~10 s while moving).
import { MAP } from './map.gen';

/** The record's format (append-only: unknown fields and cells kept on rewrite). */
export const EXPLORED_FORMAT = 1;

/** Explored cells by key → 0xRRGGBB, and their heights by key → height code. */
export type CellMap = Map<string, number>;

/**
 * The record to store: the stored one's other fields kept, its cells merged under ours (a newer
 * build's or another tab's cells survive), or, for a record of another cell size, left as it was
 * with ours merged beside it under `cellsByM`. `cells` and `heights` may be only those changed
 * since the record was last written: what it holds already is kept.
 */
export function exploredRecord(
  stored: unknown,
  cells: CellMap,
  heights: CellMap = new Map(),
): Record<string, unknown> {
  const obj = (v: unknown): Record<string, unknown> =>
    typeof v === 'object' && v !== null && !Array.isArray(v) ? (v as Record<string, unknown>) : {};
  const base = obj(stored);
  const ours = Object.fromEntries(cells);
  if (base.cellM !== undefined && base.cellM !== MAP.explored.cellM) {
    const by = obj(base.cellsByM);
    const key = String(MAP.explored.cellM);
    return { ...base, cellsByM: { ...by, [key]: { ...obj(by[key]), ...ours } } };
  }
  return {
    ...base,
    format: base.format ?? EXPLORED_FORMAT,
    cellM: MAP.explored.cellM,
    cells: { ...obj(base.cells), ...ours },
    heights: { ...obj(base.heights), ...Object.fromEntries(heights) },
  };
}

/** A store of records by world (IndexedDB in the browser, a memory one in tests). */
export interface ExploredStore {
  get(world: string): Promise<unknown>;
  /**
   * Merges these changed cells (and heights) into the world's record; `onPage` (the page closing,
   * where a worker's save might not finish) on the page itself.
   */
  add(world: string, cells: CellMap, heights: CellMap, onPage?: boolean): Promise<void>;
}

/** No height (`dwell_game::minimap::NO_HEIGHT`; minimap.ts's `NO_HEIGHT`). */
const NO_HEIGHT = -2147483648;

/**
 * The explored cells changed since the last save, all a save hands the store: a save's cost is
 * the cells explored since the last one, not the world's. Cells read from the store at load are
 * not noted.
 */
export class ExploredChanges {
  private cells: CellMap = new Map();
  private heights: CellMap = new Map();

  /** A cell's new colour and height code (NO_HEIGHT: none, the stored height kept). */
  note(key: string, rgb: number, code: number): void {
    this.cells.set(key, rgb);
    if (code === NO_HEIGHT) this.heights.delete(key);
    else this.heights.set(key, code);
  }

  get size(): number {
    return this.cells.size;
  }

  /** The changes so far, forgotten here (handed to the store). */
  take(): { cells: CellMap; heights: CellMap } {
    const out = { cells: this.cells, heights: this.heights };
    this.cells = new Map();
    this.heights = new Map();
    return out;
  }

  /** Changes a failed save took back, under any noted since. */
  restore(taken: { cells: CellMap; heights: CellMap }): void {
    for (const [k, v] of taken.cells) {
      if (this.cells.has(k)) continue;
      this.cells.set(k, v);
      const h = taken.heights.get(k);
      if (h !== undefined) this.heights.set(k, h);
    }
  }
}

/** A store's `add` over its `get` and `put` (the worker's, and the fallback without workers). */
export async function mergeIntoStore(
  store: {
    get(world: string): Promise<unknown>;
    put(world: string, record: unknown): Promise<void>;
  },
  world: string,
  cells: CellMap,
  heights: CellMap,
): Promise<void> {
  const stored = await store.get(world);
  await store.put(world, exploredRecord(stored, cells, heights));
}

/** `dwell-explored` read and written where this runs (the page or a worker). */
export function idbExplored(): {
  get(world: string): Promise<unknown>;
  put(world: string, record: unknown): Promise<void>;
} {
  let db: Promise<IDBDatabase> | null = null;
  const open = (): Promise<IDBDatabase> =>
    (db ??= new Promise((resolve, reject) => {
      const req = indexedDB.open('dwell-explored', 1);
      req.onupgradeneeded = () => req.result.createObjectStore('worlds');
      req.onsuccess = () => {
        resolve(req.result);
      };
      req.onerror = () => {
        db = null;
        reject(req.error ?? new Error('indexedDB'));
      };
    }));
  const run = <T>(mode: IDBTransactionMode, f: (s: IDBObjectStore) => IDBRequest<T>) =>
    open().then(
      (d) =>
        new Promise<T>((resolve, reject) => {
          const req = f(d.transaction('worlds', mode).objectStore('worlds'));
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

/** A save handed to the worker, and its answer. */
export interface ExploredAdd {
  id: number;
  world: string;
  cells: CellMap;
  heights: CellMap;
}
export interface ExploredAdded {
  id: number;
  ok: boolean;
}

/**
 * The browser's store: `dwell-explored`, one record per world key; saves merged in a worker
 * (on the page where workers are missing). Null where there is no IndexedDB.
 */
export function indexedDbExplored(): ExploredStore | null {
  if (typeof indexedDB === 'undefined') return null;
  const here = idbExplored();
  let worker: Worker | null = null;
  try {
    worker = new Worker(new URL('./exploredWorker.ts', import.meta.url), { type: 'module' });
  } catch {
    worker = null;
  }
  const waiting = new Map<number, (ok: boolean) => void>();
  let next = 0;
  if (worker) {
    worker.onmessage = (e: MessageEvent<ExploredAdded>) => {
      waiting.get(e.data.id)?.(e.data.ok);
      waiting.delete(e.data.id);
    };
    worker.onerror = () => {
      // A worker that fails to start: saves from here on are merged on the page.
      worker = null;
      for (const done of waiting.values()) done(false);
      waiting.clear();
    };
  }
  return {
    get: (world) => here.get(world),
    add: (world, cells, heights, onPage = false) => {
      const w = worker;
      if (!w || onPage) return mergeIntoStore(here, world, cells, heights);
      const id = next++;
      return new Promise<void>((resolve, reject) => {
        waiting.set(id, (ok) => {
          if (ok) resolve();
          else reject(new Error('explored save failed'));
        });
        w.postMessage({ id, world, cells, heights } satisfies ExploredAdd);
      });
    },
  };
}
