// Messages between the main thread and a meshing worker (worker.ts).
import type { MeshSectionOptions, SectionMeshes } from './lodMesher';
import type { ChunkMeshes } from './mesher';

export type ToMesher =
  | {
      t: 'mesh';
      id: number;
      /** Padded voxels (mesher.ts), transferred. */
      voxels: Uint16Array<ArrayBuffer>;
      /** The chunk column's biome tint grid (mesher.ts TintField), copied; null: untinted. */
      tint: Uint8Array | null;
      /** A chunk of face B: meshed as its mirror image (mesher.ts `meshChunk`). */
      mirror: boolean;
    }
  /** A LOD section's 34³ cells (lodMesher.ts), transferred. */
  | {
      t: 'lod';
      id: number;
      cells: Uint16Array<ArrayBuffer>;
      /** lodMesher.ts `meshSection` options; `surface` (if any) is transferred. */
      options: MeshSectionOptions;
    };

/** `meshes` buffers are transferred. */
export type FromMesher =
  { t: 'mesh'; id: number; meshes: ChunkMeshes } | { t: 'lod'; id: number; meshes: SectionMeshes };
