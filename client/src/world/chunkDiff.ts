// In-game "regenerate chunk and diff" (Phase 3e debug tooling): how many voxels of a loaded chunk
// differ from the chunk as the generator makes it — the edits the world holds there.
import { PAD, paddedIndex } from '../mesh/mesher';
import { CHUNK_SIZE } from '../protocol/constants.gen';

/** Voxels where the chunk inside `padded` (mesh/mesher.ts layout) differs from `generated`. */
export function countChanged(padded: Uint16Array, generated: Uint16Array): number {
  if (padded.length !== PAD * PAD * PAD) throw new RangeError('padded voxels expected');
  let n = 0;
  for (let z = 0; z < CHUNK_SIZE; z++)
    for (let y = 0; y < CHUNK_SIZE; y++)
      for (let x = 0; x < CHUNK_SIZE; x++)
        if (padded[paddedIndex(x, y, z)] !== generated[x | (y << 5) | (z << 10)]) n++;
  return n;
}
