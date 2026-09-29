// Messages between the main thread and a meshing worker (worker.ts).
import type { ChunkMeshes } from './mesher';

export type ToMesher = {
  t: 'mesh';
  id: number;
  /** Padded voxels (mesher.ts), transferred. */
  voxels: Uint16Array<ArrayBuffer>;
};

/** `meshes` buffers are transferred. */
export type FromMesher = { t: 'mesh'; id: number; meshes: ChunkMeshes };
