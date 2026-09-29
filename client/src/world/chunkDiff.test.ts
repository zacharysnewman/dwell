import { describe, expect, it } from 'vitest';
import { PADDED_VOLUME, paddedIndex } from '../mesh/mesher';
import { CHUNK_VOLUME } from '../protocol/chunkVoxels';
import { countChanged } from './chunkDiff';

describe('regenerate and diff', () => {
  it('counts the voxels that differ from generation, ignoring the apron', () => {
    const generated = new Uint16Array(CHUNK_VOLUME).fill(2);
    const padded = new Uint16Array(PADDED_VOLUME);
    for (let z = 0; z < 32; z++)
      for (let y = 0; y < 32; y++) for (let x = 0; x < 32; x++) padded[paddedIndex(x, y, z)] = 2;
    padded[paddedIndex(-1, 0, 0)] = 9; // apron: not this chunk
    expect(countChanged(padded, generated)).toBe(0);
    padded[paddedIndex(3, 4, 5)] = 0;
    padded[paddedIndex(31, 31, 31)] = 16;
    expect(countChanged(padded, generated)).toBe(2);
  });
});
