import { describe, expect, it } from 'vitest';
import { ByteReader, ByteWriter } from './bytes';
import { LOD_CELL_COUNT, readLodCells, writeLodCells } from './chunkVoxels';

describe('LOD section cells (palette + RLE, §6.6)', () => {
  it('round-trips 34³ cells canonically, and rejects the wrong count', () => {
    const cells = new Uint16Array(LOD_CELL_COUNT);
    for (let i = 0; i < cells.length; i++) cells[i] = i < 20000 ? 2 : i % 97 === 0 ? 17 : 0;
    const w = new ByteWriter();
    writeLodCells(w, cells);
    const bytes = w.finish();
    const back = readLodCells(new ByteReader(bytes));
    expect(back).toEqual(cells);
    const again = new ByteWriter();
    writeLodCells(again, back);
    expect(again.finish()).toEqual(bytes);
    expect(() => {
      writeLodCells(new ByteWriter(), new Uint16Array(10));
    }).toThrow(RangeError);
  });
});
