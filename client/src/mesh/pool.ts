// Meshing worker pool (ARCHITECTURE.md §5.1, ADR 0007): chunk render meshes are built off the main
// thread. Jobs run in request order; each worker holds a few at once.
import type { WorkerLike } from '../worldgen/pool';
import { defaultWorkerCount } from '../worldgen/pool';
import { meshSection, type LiquidMode, type SectionMeshes } from './lodMesher';
import { meshChunk, type ChunkMeshes } from './mesher';
import type { FromMesher } from './messages';

/** Anything that meshes chunks asynchronously: the worker pool, or a test double. */
export interface Mesher {
  /** Meshes padded voxels (transferred to the worker: the caller gives up `voxels`). */
  mesh(voxels: Uint16Array<ArrayBuffer>): Promise<ChunkMeshes>;
  /** Jobs queued or running. */
  readonly pending: number;
}

/** Anything that meshes LOD sections asynchronously (§6.6). */
export interface SectionMesher {
  /**
   * Meshes a section's 34³ cells (transferred: the caller gives up `cells` and `surface`), with
   * its liquids and optional column surfaces as in lodMesher.ts `meshSection`.
   */
  meshSection(
    cells: Uint16Array<ArrayBuffer>,
    liquids?: LiquidMode,
    surface?: Float32Array<ArrayBuffer> | null,
  ): Promise<SectionMeshes>;
}

interface Job {
  id: number;
  lod: boolean;
  liquids?: LiquidMode;
  surface?: Float32Array<ArrayBuffer> | null;
  voxels: Uint16Array<ArrayBuffer>;
  resolve: (m: unknown) => void;
}

const JOBS_PER_WORKER = 2;

export class MeshPool implements Mesher, SectionMesher {
  private readonly queue: Job[] = [];
  private readonly running = new Map<WorkerLike, Job[]>();
  private runningCount = 0;
  private nextId = 1;

  constructor(private readonly workers: WorkerLike[]) {
    for (const w of workers) {
      this.running.set(w, []);
      w.onmessage = (e: MessageEvent) => {
        this.onMessage(w, e.data as FromMesher);
      };
    }
  }

  /** Browser pool of module workers. */
  static create(count = defaultWorkerCount()): MeshPool {
    const workers: WorkerLike[] = [];
    for (let i = 0; i < count; i++) {
      workers.push(new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' }));
    }
    return new MeshPool(workers);
  }

  /** Jobs the pool runs at once; callers keep about this many in flight. */
  get capacity(): number {
    return this.workers.length * JOBS_PER_WORKER;
  }

  get pending(): number {
    return this.queue.length + this.runningCount;
  }

  mesh(voxels: Uint16Array<ArrayBuffer>): Promise<ChunkMeshes> {
    return new Promise((resolve) => {
      this.queue.push({
        id: this.nextId++,
        lod: false,
        voxels,
        resolve: (m) => {
          resolve(m as ChunkMeshes);
        },
      });
      this.dispatch();
    });
  }

  meshSection(
    cells: Uint16Array<ArrayBuffer>,
    liquids: LiquidMode = 'translucent',
    surface: Float32Array<ArrayBuffer> | null = null,
  ): Promise<SectionMeshes> {
    return new Promise((resolve) => {
      this.queue.push({
        id: this.nextId++,
        lod: true,
        liquids,
        surface,
        voxels: cells,
        resolve: (m) => {
          resolve(m as SectionMeshes);
        },
      });
      this.dispatch();
    });
  }

  terminate(): void {
    for (const w of this.workers) w.terminate();
  }

  private onMessage(w: WorkerLike, msg: FromMesher): void {
    const job = this.running.get(w)?.shift();
    if (job) {
      this.runningCount--;
      job.resolve(msg.meshes);
    }
    this.dispatch();
  }

  private dispatch(): void {
    while (this.queue.length > 0) {
      let best: WorkerLike | null = null;
      let fewest = JOBS_PER_WORKER;
      for (const [w, jobs] of this.running) {
        if (jobs.length < fewest) {
          best = w;
          fewest = jobs.length;
        }
      }
      const job = best ? this.queue.shift() : undefined;
      if (!best || !job) return;
      this.running.get(best)?.push(job);
      this.runningCount++;
      const surface = job.surface ?? null;
      best.postMessage(
        job.lod
          ? {
              t: 'lod',
              id: job.id,
              cells: job.voxels,
              liquids: job.liquids ?? 'translucent',
              surface,
            }
          : { t: 'mesh', id: job.id, voxels: job.voxels },
        surface ? [job.voxels.buffer, surface.buffer] : [job.voxels.buffer],
      );
    }
  }
}

/** Meshes on the calling thread (tests; a fallback when workers are unavailable). */
export class InlineMesher implements Mesher, SectionMesher {
  pending = 0;
  mesh(voxels: Uint16Array<ArrayBuffer>): Promise<ChunkMeshes> {
    return Promise.resolve(meshChunk(voxels));
  }
  meshSection(
    cells: Uint16Array<ArrayBuffer>,
    liquids: LiquidMode = 'translucent',
    surface: Float32Array<ArrayBuffer> | null = null,
  ): Promise<SectionMeshes> {
    return Promise.resolve(meshSection(cells, liquids, surface));
  }
}
