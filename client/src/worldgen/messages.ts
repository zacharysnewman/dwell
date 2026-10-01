// Messages between the main thread and a worldgen worker (worker.ts).
import type { LodCoord, LodKind } from '../lod/grid';
import type { ChunkCoord } from '../protocol/messages';

export type ToWorldgen =
  | { t: 'init'; generatorVersion: number; worldSeed: bigint }
  | { t: 'generate'; id: number; coord: ChunkCoord }
  | { t: 'map'; id: number; x0: number; z0: number; step: number; n: number }
  /** GenerateLod of a section, and a column of sections' bounds (§6.6). */
  | { t: 'lod'; id: number; coord: LodCoord }
  | { t: 'bounds'; id: number; level: number; i: number; k: number };

export type FromWorldgen =
  | { t: 'ready' }
  | { t: 'error'; message: string }
  /** `voxels` is transferred; `hash` is its ChunkHash. */
  | { t: 'chunk'; id: number; voxels: Uint16Array<ArrayBuffer>; hash: bigint }
  /** `bytes` (transferred) is the terrain map, or null for generators without one. */
  | { t: 'map'; id: number; bytes: Uint8Array<ArrayBuffer> | null }
  /** `cells` (transferred): the section's 34³ cells. */
  | {
      t: 'lod';
      id: number;
      kind: LodKind;
      cells: Uint16Array<ArrayBuffer>;
      surface: Float32Array<ArrayBuffer> | null;
    }
  | { t: 'bounds'; id: number; lo: number; hi: number; anyInside: boolean }
  /** The worker's WebAssembly memory changed size (bytes); not an answer to a job. */
  | { t: 'memory'; bytes: number };
