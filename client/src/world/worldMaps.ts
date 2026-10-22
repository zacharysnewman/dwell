// The world maps of a terrain world (ADR 0049, docs/WORLD_GENERATION.md §3): one per face, a pure
// function of the seed and the world's map settings (WORLD_GENERATION.md §9), ~1.3 s each to compute
// in WASM. This page gets them once per world: from IndexedDB if an earlier visit kept them (keyed by
// the build's map fingerprint, the settings' hash and the face's seed, so a different build or other
// settings never read them), else computed in two workers at once (never on the page's thread),
// then kept. Every WASM instance of the world (the page's, each job worker's, the local
// world's) installs the same bytes rather than computing its own.

/** The terrain generator's world version (constants: `GENERATOR_TERRAIN`). */
export const GENERATOR_TERRAIN = 8;

/** How many maps the cache keeps (two per world): the most recently used. */
export const KEEP_MAPS = 8;

/** A map store (IndexedDB in the browser; a memory one in tests). */
export interface MapStore {
  get(key: string): Promise<Uint8Array | null>;
  put(key: string, bytes: Uint8Array): Promise<void>;
}

/** What the maps need from the build: each face's cache key and a builder (a worker). */
export interface MapDeps {
  /** The cache keys of the world's faces A and B under its settings (`worldMapKeys`). */
  keys(seed: bigint, worldgen: string): string[];
  /** Computes face `face`'s map off the page's thread; resolves with its bytes. */
  build(seed: bigint, face: number, worldgen: string): Promise<Uint8Array>;
  store: MapStore | null;
  /** Each face's map as it becomes ready (the loading screen's progress). */
  onProgress?: (ready: number, needed: number) => void;
}

const pending = new Map<string, Promise<Uint8Array[]>>();

/**
 * The maps of world `seed` under generation settings `worldgen` (encoded; faces A and B), or null
 * for a world without terrain. The same promise for every caller of a seed and settings on this page
 * (the local world's start and the client's join share it).
 */
export function worldMaps(
  seed: bigint,
  generatorVersion: number,
  worldgen: string,
  deps: MapDeps,
): Promise<Uint8Array[]> | null {
  if (generatorVersion !== GENERATOR_TERRAIN) return null;
  const id = `${seed.toString()}|${worldgen}`;
  let p = pending.get(id);
  if (!p) {
    p = fetchMaps(seed, worldgen, deps);
    pending.set(id, p);
    // A failure is not kept: the next join tries again.
    p.catch(() => pending.delete(id));
  }
  return p;
}

/** Forgets the maps this page holds (tests). */
export function forgetWorldMaps(): void {
  pending.clear();
}

async function fetchMaps(seed: bigint, worldgen: string, deps: MapDeps): Promise<Uint8Array[]> {
  const keys = deps.keys(seed, worldgen);
  let ready = 0;
  deps.onProgress?.(0, keys.length);
  return Promise.all(
    keys.map(async (key, face) => {
      let bytes: Uint8Array | null;
      try {
        bytes = (await deps.store?.get(key)) ?? null;
      } catch {
        bytes = null; // storage blocked or damaged: build it
      }
      if (!bytes) {
        bytes = await deps.build(seed, face, worldgen);
        try {
          await deps.store?.put(key, bytes);
        } catch {
          // Not kept (quota, private mode): built again next time.
        }
      }
      ready += 1;
      deps.onProgress?.(ready, keys.length);
      return bytes;
    }),
  );
}

/** The maps kept in IndexedDB (`dwell-world-maps`), the least recently used dropped past KEEP_MAPS. */
export class IndexedDbMapStore implements MapStore {
  private static readonly DB = 'dwell-world-maps';
  private static readonly STORE = 'maps';

  private open(): Promise<IDBDatabase> {
    return new Promise((resolve, reject) => {
      const req = indexedDB.open(IndexedDbMapStore.DB, 1);
      req.onupgradeneeded = () => {
        req.result.createObjectStore(IndexedDbMapStore.STORE);
      };
      req.onsuccess = () => {
        resolve(req.result);
      };
      req.onerror = () => {
        reject(req.error ?? new Error('IndexedDB open failed'));
      };
    });
  }

  private async run<T>(
    mode: IDBTransactionMode,
    op: (s: IDBObjectStore) => Promise<T>,
  ): Promise<T> {
    const db = await this.open();
    try {
      return await op(
        db.transaction(IndexedDbMapStore.STORE, mode).objectStore(IndexedDbMapStore.STORE),
      );
    } finally {
      db.close();
    }
  }

  async get(key: string): Promise<Uint8Array | null> {
    const entry = await this.run('readonly', (s) =>
      request<{ bytes: Uint8Array; used: number } | undefined>(s.get(key)),
    );
    if (!entry) return null;
    // Mark it used (best effort).
    void this.run('readwrite', (s) => request(s.put({ ...entry, used: Date.now() }, key))).catch(
      () => undefined,
    );
    return entry.bytes;
  }

  async put(key: string, bytes: Uint8Array): Promise<void> {
    await this.run('readwrite', async (s) => {
      await request(s.put({ bytes, used: Date.now() }, key));
      const keys = await request<IDBValidKey[]>(s.getAllKeys());
      if (keys.length <= KEEP_MAPS) return;
      const entries = await Promise.all(
        keys.map(async (k) => ({
          k,
          used: (await request<{ used: number } | undefined>(s.get(k)))?.used ?? 0,
        })),
      );
      for (const e of oldest(entries, keys.length - KEEP_MAPS)) await request(s.delete(e.k));
    });
  }
}

/** The `count` least recently used entries. */
export function oldest<T extends { used: number }>(entries: T[], count: number): T[] {
  return [...entries].sort((a, b) => a.used - b.used).slice(0, Math.max(0, count));
}

function request<T>(req: IDBRequest): Promise<T> {
  return new Promise((resolve, reject) => {
    req.onsuccess = () => {
      resolve(req.result as T);
    };
    req.onerror = () => {
      reject(req.error ?? new Error('IndexedDB request failed'));
    };
  });
}

/** A builder that computes a map in a fresh worker (the job worker script), then ends it. */
export function workerBuilder(makeWorker: () => Worker): MapDeps['build'] {
  return (seed, face, worldgen) =>
    new Promise((resolve, reject) => {
      const worker = makeWorker();
      worker.onmessage = (e: MessageEvent<{ map?: Uint8Array; error?: string }>) => {
        worker.terminate();
        if (e.data.map) resolve(e.data.map);
        else reject(new Error(e.data.error ?? 'the world map worker failed'));
      };
      worker.onerror = (e) => {
        worker.terminate();
        reject(new Error(e.message || 'the world map worker failed'));
      };
      worker.postMessage({ t: 'buildMap', seed, face, worldgen });
    });
}
