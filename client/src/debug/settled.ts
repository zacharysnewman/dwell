// Whether the view has finished loading (debug tooling: scripts/shot.ts waits for it rather than a
// fixed time): no chunk is generating, waiting to mesh or meshing, and the LOD has no generation,
// meshing or server request in flight. The LOD keeps refining as coarse levels finish, so this can
// take minutes in software rendering; it does come true once the view holds still.

export interface TerrainLoad {
  generating: number;
  dirty: number;
  meshing: number;
}

export interface LodLoad {
  generating: number;
  meshing: number;
  asking: number;
}

/** True when nothing is loading. Without an LOD (`?lod=0`), the terrain alone decides. */
export function isSettled(terrain: TerrainLoad, lod: LodLoad | null): boolean {
  if (terrain.generating > 0 || terrain.dirty > 0 || terrain.meshing > 0) return false;
  return !lod || (lod.generating === 0 && lod.meshing === 0 && lod.asking === 0);
}
