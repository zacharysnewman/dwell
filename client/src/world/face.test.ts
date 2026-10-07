import { describe, expect, it } from 'vitest';

import { CHUNK_SIZE, World } from '../protocol/constants.gen';
import {
  FACE_A,
  FACE_B,
  faceOfChunkY,
  faceOfY,
  MIDPLANE_CHUNK_Y,
  mirrorChunkY,
  mirrorY,
} from './face';

describe('the two faces', () => {
  it('splits the world at the midplane, a chunk boundary', () => {
    expect(World.midplaneY).toBe(-2048);
    expect(World.worldBottomY).toBe(-10240);
    expect(MIDPLANE_CHUNK_Y).toBe(-64);
    expect(faceOfY(-2048)).toBe(FACE_A);
    expect(faceOfY(-2049)).toBe(FACE_B);
    expect(faceOfChunkY(-64)).toBe(FACE_A);
    expect(faceOfChunkY(-65)).toBe(FACE_B);
  });

  it('mirrors voxels onto voxels and chunks onto chunks, each its own inverse', () => {
    expect(mirrorY(-2048)).toBe(-2049);
    expect(mirrorY(-4096)).toBe(-1);
    expect(mirrorY(-4097)).toBe(0);
    expect(mirrorY(World.worldBottomY)).toBe(World.worldMaxY - 1);
    expect(mirrorChunkY(-65)).toBe(-64);
    expect(mirrorChunkY(-129)).toBe(0);
    for (const y of [-10240, -4097, -2049, -2048, -1, 0, 6143]) {
      expect(mirrorY(mirrorY(y))).toBe(y);
      const cy = Math.floor(y / CHUNK_SIZE);
      // A chunk's rows land in the mirrored chunk.
      expect(Math.floor(mirrorY(y) / CHUNK_SIZE)).toBe(mirrorChunkY(cy));
    }
  });
});
