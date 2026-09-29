// Messages between the main thread and a meshing worker (worker.ts).
import type { LiquidMode, SectionMeshes } from './lodMesher';
import type { ChunkMeshes } from './mesher';

export type ToMesher =
  | {
      t: 'mesh';
      id: number;
      /** Padded voxels (mesher.ts), transferred. */
      voxels: Uint16Array<ArrayBuffer>;
    }
  /** A LOD section's 34³ cells (lodMesher.ts), transferred. */
  | { t: 'lod'; id: number; cells: Uint16Array<ArrayBuffer>; liquids: LiquidMode };

/** `meshes` buffers are transferred. */
export type FromMesher =
  { t: 'mesh'; id: number; meshes: ChunkMeshes } | { t: 'lod'; id: number; meshes: SectionMeshes };
