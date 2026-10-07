// Worldgen worker pool (ARCHITECTURE.md §5.1, ADR 0007): chunk generation off the main thread.
// Jobs run in request order (the server streams nearest first); jobs not yet handed to a worker
// can be cancelled when their chunk leaves the view. Each worker holds a few jobs at once, so it
// keeps generating while the main thread is busy with a long frame.
import type { LodBounds, LodCoord } from '../lod/grid';
import type { ChunkCoord } from '../protocol/messages';
import type { GeneratedSection } from './generator';
import type { FromWorldgen, ToWorldgen } from './messages';

export interface GeneratedChunk {
  voxels: Uint16Array<ArrayBuffer>;
  /** ChunkHash (the WorldgenCheck value). */
  hash: bigint;
}

/** Anything that generates chunks asynchronously: the worker pool, or a test double. */
export interface ChunkSource {
  generate(coord: ChunkCoord): Promise<GeneratedChunk>;
  /**
   * The biome tint grid of the columns of chunk (cx, cz) for the mesher (mesh/mesher.ts
   * TintField), or null for generators without biomes. Optional: sources without it mesh untinted.
   */
  tint?(cx: number, cz: number): Promise<Uint8Array<ArrayBuffer> | null>;
  /** Drops a queued job; its promise never settles. False if it already started. */
  cancel(coord: ChunkCoord): boolean;
  /** Jobs queued or running. */
  readonly pending: number;
}

/** The minimal surface of a worker endpoint (a Web Worker, or a fake in tests). */
export interface WorkerLike {
  postMessage(message: unknown, transfer?: Transferable[]): void;
  onmessage: ((e: MessageEvent) => void) | null;
  terminate(): void;
}

/** Anything that generates LOD sections and their column bounds asynchronously (§6.6). */
export interface SectionSource {
  lod(coord: LodCoord): Promise<GeneratedSection>;
  lodBounds(level: number, i: number, k: number): Promise<LodBounds>;
}

/** Arguments of a terrain map job (the debug map overlay). */
export interface MapRequest {
  x0: number;
  z0: number;
  step: number;
  n: number;
}

interface Job {
  id: number;
  /** What the worker runs: a chunk to generate, or one of the other requests. */
  coord: ChunkCoord | null;
  message?: DistributiveOmit<ToWorldgen, 'id'>;
  resolve: (result: unknown) => void;
  reject: (e: Error) => void;
}

type DistributiveOmit<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;

/** Jobs handed to one worker at a time. */
const JOBS_PER_WORKER = 4;

const key = (c: ChunkCoord): string => `${String(c[0])},${String(c[1])},${String(c[2])}`;

/**
 * Workers to start: cores − 2 (the main thread and the local server keep theirs), 1..4; on phones
 * at most 2, as each worker's memory counts against the tab's (mobile Safari closes a tab that
 * uses too much).
 */
export function defaultWorkerCount(mobile = false): number {
  const cores = typeof navigator === 'undefined' ? 4 : navigator.hardwareConcurrency || 4;
  return Math.max(1, Math.min(mobile ? 2 : 4, cores - 2));
}

export class WorldgenPool implements ChunkSource, SectionSource {
  private readonly ready: WorkerLike[] = [];
  private readonly queue: Job[] = [];
  /** LOD jobs: handed out only when no chunk job waits (chunks are the player's terrain). */
  private readonly lodQueue: Job[] = [];
  /** Jobs handed to each worker, in order (workers answer in order). */
  private readonly running = new Map<WorkerLike, Job[]>();
  private runningCount = 0;
  private nextId = 1;
  private failure: Error | null = null;
  private readonly memory = new Map<WorkerLike, number>();

  constructor(
    private readonly workers: WorkerLike[],
    generatorVersion: number,
    worldSeed: bigint,
  ) {
    for (const w of workers) {
      w.onmessage = (e: MessageEvent) => {
        this.onMessage(w, e.data as FromWorldgen);
      };
      w.postMessage({ t: 'init', generatorVersion, worldSeed });
    }
  }

  /** Browser pool of module workers. */
  static create(generatorVersion: number, worldSeed: bigint, count = defaultWorkerCount()) {
    const workers: WorkerLike[] = [];
    for (let i = 0; i < count; i++) {
      workers.push(new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' }));
    }
    return new WorldgenPool(workers, generatorVersion, worldSeed);
  }

  get pending(): number {
    return this.queue.length + this.lodQueue.length + this.runningCount;
  }

  /** Jobs the pool runs at once. */
  get capacity(): number {
    return this.workers.length * JOBS_PER_WORKER;
  }

  generate(coord: ChunkCoord): Promise<GeneratedChunk> {
    return new Promise((resolve, reject) => {
      if (this.failure) {
        reject(this.failure);
        return;
      }
      this.queue.push({
        id: this.nextId++,
        coord,
        resolve: (r) => {
          resolve(r as GeneratedChunk);
        },
        reject,
      });
      this.dispatch();
    });
  }

  /**
   * Samples the terrain's biome/height map (worldgen/generator.ts `map`) in a worker; null for
   * generators without one.
   */
  map(request: MapRequest): Promise<Uint8Array<ArrayBuffer> | null> {
    return this.submit<Uint8Array<ArrayBuffer> | null>(this.queue, { t: 'map', ...request });
  }

  /** The tint grid of the chunk column (cx, cz): see ChunkSource.tint. */
  tint(cx: number, cz: number): Promise<Uint8Array<ArrayBuffer> | null> {
    return this.submit<Uint8Array<ArrayBuffer> | null>(this.queue, { t: 'tint', cx, cz });
  }

  /** GenerateLod of a section (§6.6). */
  lod(coord: LodCoord): Promise<GeneratedSection> {
    return this.submit<GeneratedSection>(this.lodQueue, { t: 'lod', coord });
  }

  /** Height bounds of the column of sections (level, i, ·, k). */
  lodBounds(level: number, i: number, k: number): Promise<LodBounds> {
    return this.submit<LodBounds>(this.lodQueue, { t: 'bounds', level, i, k });
  }

  private submit<T>(queue: Job[], message: DistributiveOmit<ToWorldgen, 'id'>): Promise<T> {
    return new Promise((resolve, reject) => {
      if (this.failure) {
        reject(this.failure);
        return;
      }
      queue.push({
        id: this.nextId++,
        coord: null,
        message,
        resolve: (r) => {
          resolve(r as T);
        },
        reject,
      });
      this.dispatch();
    });
  }

  cancel(coord: ChunkCoord): boolean {
    const k = key(coord);
    const i = this.queue.findIndex((j) => j.coord !== null && key(j.coord) === k);
    if (i < 0) return false;
    this.queue.splice(i, 1);
    return true;
  }

  terminate(): void {
    for (const w of this.workers) w.terminate();
  }

  /** Each worker's WebAssembly memory (bytes), as last reported. */
  heapBytes(): number[] {
    return this.workers.map((w) => this.memory.get(w) ?? 0);
  }

  private onMessage(w: WorkerLike, msg: FromWorldgen) {
    if (msg.t === 'memory') {
      this.memory.set(w, msg.bytes);
      return;
    }
    if (msg.t === 'ready') {
      this.ready.push(w);
      this.running.set(w, []);
    } else if (msg.t === 'error') {
      this.failure = new Error(msg.message);
      for (const j of [...this.queue.splice(0), ...this.lodQueue.splice(0)]) j.reject(this.failure);
      return;
    } else {
      // Workers answer their jobs in order.
      const job = this.running.get(w)?.shift();
      if (job) this.runningCount--;
      job?.resolve(
        msg.t === 'chunk'
          ? { voxels: msg.voxels, hash: msg.hash }
          : msg.t === 'map' || msg.t === 'tint'
            ? msg.bytes
            : msg.t === 'lod'
              ? { kind: msg.kind, cells: msg.cells, surface: msg.surface }
              : { lo: msg.lo, hi: msg.hi, anyInside: msg.anyInside },
      );
    }
    this.dispatch();
  }

  /** Hands queued jobs to the least busy workers, up to JOBS_PER_WORKER each. */
  private dispatch(): void {
    while (this.queue.length > 0 || this.lodQueue.length > 0) {
      let best: WorkerLike | null = null;
      let fewest = JOBS_PER_WORKER;
      for (const w of this.ready) {
        const n = this.running.get(w)?.length ?? JOBS_PER_WORKER;
        if (n < fewest) {
          best = w;
          fewest = n;
        }
      }
      const job = best ? (this.queue.shift() ?? this.lodQueue.shift()) : undefined;
      if (!best || !job) return;
      this.running.get(best)?.push(job);
      this.runningCount++;
      const message: ToWorldgen = job.coord
        ? { t: 'generate', id: job.id, coord: job.coord }
        : ({ ...job.message, id: job.id } as ToWorldgen);
      best.postMessage(message);
    }
  }
}
