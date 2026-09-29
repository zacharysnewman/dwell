// Messages between the main thread and a worldgen worker (worker.ts).
import type { ChunkCoord } from '../protocol/messages';

export type ToWorldgen =
  | { t: 'init'; generatorVersion: number; worldSeed: bigint }
  | { t: 'generate'; id: number; coord: ChunkCoord }
  | { t: 'map'; id: number; x0: number; z0: number; step: number; n: number };

export type FromWorldgen =
  | { t: 'ready' }
  | { t: 'error'; message: string }
  /** `voxels` is transferred; `hash` is its ChunkHash. */
  | { t: 'chunk'; id: number; voxels: Uint16Array<ArrayBuffer>; hash: bigint }
  /** `bytes` (transferred) is the terrain map, or null for generators without one. */
  | { t: 'map'; id: number; bytes: Uint8Array<ArrayBuffer> | null };
