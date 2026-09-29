import { describe, expect, it } from 'vitest';
import { World } from '../protocol/constants.gen';
import type { ChunkCoord } from '../protocol/messages';
import {
  chunkOfLod,
  kindFromBounds,
  lodAncestor,
  lodCell,
  lodChild,
  lodInWorld,
  LodKind,
  lodOfChunk,
  lodParent,
  lodRows,
  MAX_LEVEL,
  sectionAt,
  sectionOrigin,
  sectionSize,
} from './grid';

describe('LOD grid (mirrors lod.h)', () => {
  it('level 0 is the chunk grid; the root holds the whole disc', () => {
    const chunks: ChunkCoord[] = [
      [0, 0, 0],
      [-1, -64, 5],
      [255999, 191, -256000],
    ];
    for (const c of chunks) {
      const l = lodOfChunk(c);
      expect(chunkOfLod(l)).toEqual(c);
      expect(sectionOrigin(l)).toEqual([c[0] * 32, c[1] * 32, c[2] * 32]);
    }
    expect(lodInWorld([MAX_LEVEL, 0, 0, 0])).toBe(true);
    expect(lodInWorld([MAX_LEVEL, 1, 0, 0])).toBe(false);
    expect(sectionSize(MAX_LEVEL)).toBe(2 ** 24);
    expect(lodAncestor(lodOfChunk([-256000, -64, 255999]), MAX_LEVEL)).toEqual([
      MAX_LEVEL,
      0,
      0,
      0,
    ]);
  });

  it('has one row from level 8, and children nest in their parent', () => {
    expect([lodRows(0), lodRows(7), lodRows(8), lodRows(19)]).toEqual([256, 2, 1, 1]);
    expect(lodInWorld([8, 1024, 1, 1024])).toBe(false);
    const p = [5, 1000, 3, 1001] as const;
    for (let o = 0; o < 8; o++) {
      const c = lodChild(p, o);
      expect(lodParent(c)).toEqual(p);
      const [x, y, z] = sectionOrigin(c);
      const [px, py, pz] = sectionOrigin(p);
      const h = sectionSize(4);
      expect([x - px, y - py, z - pz]).toEqual([(o & 1) * h, ((o >> 1) & 1) * h, (o >> 2) * h]);
    }
  });

  it('knows the rim, cells and bounds as the C++ does', () => {
    expect(lodInWorld(sectionAt(10, [8_000_000, 0, 0]))).toBe(true);
    expect(lodInWorld(sectionAt(10, [8_230_000, 0, 0]))).toBe(false);
    expect(lodInWorld(sectionAt(10, [5_840_000, 0, 5_840_000]))).toBe(false);
    expect(lodCell(-1, -1, -1)).toBe(0);
    expect(lodCell(0, 0, 0)).toBe(1 + 34 + 34 * 34);
    const c = sectionAt(2, [0, 0, 0]);
    expect(kindFromBounds(c, { lo: -40, hi: 60, anyInside: true })).toBe(LodKind.Content);
    expect(kindFromBounds(sectionAt(2, [0, 300, 0]), { lo: -40, hi: 60, anyInside: true })).toBe(
      LodKind.Empty,
    );
    expect(kindFromBounds(sectionAt(2, [0, -500, 0]), { lo: -40, hi: 60, anyInside: true })).toBe(
      LodKind.Buried,
    );
    expect(kindFromBounds(c, { lo: 0, hi: 0, anyInside: false })).toBe(LodKind.Empty);
    expect(World.worldMinY).toBe(-2048);
  });
});
