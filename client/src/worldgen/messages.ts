// Messages between the main thread and a worldgen worker (worker.ts).
import type { ChunkCoord } from '../protocol/messages';

export type ToWorldgen =
  | { t: 'init'; generatorVersion: number; worldSeed: bigint }
  | { t: 'generate'; id: number; coord: ChunkCoord };

export type FromWorldgen =
  | { t: 'ready' }
  | { t: 'error'; message: string }
  /** `voxels` is transferred; `hash` is its ChunkHash. */
  | { t: 'chunk'; id: number; voxels: Uint16Array<ArrayBuffer>; hash: bigint };
