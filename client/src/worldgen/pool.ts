// Worldgen worker pool (ARCHITECTURE.md §5.1, ADR 0007): chunk generation off the main thread.
// Jobs run in request order (the server streams nearest first); jobs not yet handed to a worker
// can be cancelled when their chunk leaves the view. Each worker holds a few jobs at once, so it
// keeps generating while the main thread is busy with a long frame.
import type { ChunkCoord } from '../protocol/messages';

export interface GeneratedChunk {
  voxels: Uint16Array<ArrayBuffer>;
  /** ChunkHash (the WorldgenCheck value). */
  hash: bigint;
}

/** Anything that generates chunks asynchronously: the worker pool, or a test double. */
export interface ChunkSource {
  generate(coord: ChunkCoord): Promise<GeneratedChunk>;
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

interface Job {
  id: number;
  coord: ChunkCoord;
  resolve: (c: GeneratedChunk) => void;
  reject: (e: Error) => void;
}

/** Jobs handed to one worker at a time. */
const JOBS_PER_WORKER = 4;

const key = (c: ChunkCoord): string => `${String(c[0])},${String(c[1])},${String(c[2])}`;

/** Workers to start: cores − 2 (the main thread and the local server keep theirs), 1..4. */
export function defaultWorkerCount(): number {
  const cores = typeof navigator === 'undefined' ? 4 : navigator.hardwareConcurrency || 4;
  return Math.max(1, Math.min(4, cores - 2));
}

export class WorldgenPool implements ChunkSource {
  private readonly ready: WorkerLike[] = [];
  private readonly queue: Job[] = [];
  /** Jobs handed to each worker, in order (workers answer in order). */
  private readonly running = new Map<WorkerLike, Job[]>();
  private runningCount = 0;
  private nextId = 1;
  private failure: Error | null = null;

  constructor(
    private readonly workers: WorkerLike[],
    generatorVersion: number,
    worldSeed: bigint,
  ) {
    for (const w of workers) {
      w.onmessage = (e: MessageEvent) => {
        this.onMessage(w, e.data as { t: string; message?: string } & Partial<GeneratedChunk>);
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
    return this.queue.length + this.runningCount;
  }

  generate(coord: ChunkCoord): Promise<GeneratedChunk> {
    return new Promise((resolve, reject) => {
      if (this.failure) {
        reject(this.failure);
        return;
      }
      this.queue.push({ id: this.nextId++, coord, resolve, reject });
      this.dispatch();
    });
  }

  cancel(coord: ChunkCoord): boolean {
    const k = key(coord);
    const i = this.queue.findIndex((j) => key(j.coord) === k);
    if (i < 0) return false;
    this.queue.splice(i, 1);
    return true;
  }

  terminate(): void {
    for (const w of this.workers) w.terminate();
  }

  private onMessage(w: WorkerLike, msg: { t: string; message?: string } & Partial<GeneratedChunk>) {
    if (msg.t === 'ready') {
      this.ready.push(w);
      this.running.set(w, []);
    } else if (msg.t === 'error') {
      this.failure = new Error(msg.message ?? 'worldgen failed');
      for (const j of this.queue.splice(0)) j.reject(this.failure);
      return;
    } else if (msg.t === 'chunk' && msg.voxels && msg.hash !== undefined) {
      const job = this.running.get(w)?.shift();
      if (job) this.runningCount--;
      job?.resolve({ voxels: msg.voxels, hash: msg.hash });
    }
    this.dispatch();
  }

  /** Hands queued jobs to the least busy workers, up to JOBS_PER_WORKER each. */
  private dispatch(): void {
    while (this.queue.length > 0) {
      let best: WorkerLike | null = null;
      let fewest = JOBS_PER_WORKER;
      for (const w of this.ready) {
        const n = this.running.get(w)?.length ?? JOBS_PER_WORKER;
        if (n < fewest) {
          best = w;
          fewest = n;
        }
      }
      const job = best ? this.queue.shift() : undefined;
      if (!best || !job) return;
      this.running.get(best)?.push(job);
      this.runningCount++;
      best.postMessage({ t: 'generate', id: job.id, coord: job.coord });
    }
  }
}
