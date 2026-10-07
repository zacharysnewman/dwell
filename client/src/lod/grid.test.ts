import { describe, expect, it } from 'vitest';
import { World } from '../protocol/constants.gen';
import type { ChunkCoord } from '../protocol/messages';
import { FACE_A, FACE_B } from '../world/face';
import {
  chunkOfLod,
  kindFromBounds,
  lodAncestor,
  lodCell,
  lodChild,
  lodId,
  lodInWorld,
  LodKind,
  lodOfChunk,
  lodParent,
  lodFirstRow,
  lodLastRow,
  lodRowFace,
  MAX_LEVEL,
  ORIGIN,
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

  it('counts y from −2²³ too: rows cover both faces, one row from level 9', () => {
    expect(ORIGIN).toEqual([-(2 ** 23), -(2 ** 23), -(2 ** 23)]);
    const rows = (level: number): number => lodLastRow(level) - lodFirstRow(level) + 1;
    expect([rows(0), rows(6), rows(8), rows(19)]).toEqual([512, 8, 3, 1]);
    expect(lodFirstRow(19)).toBe(0);
    expect(lodOfChunk([0, World.worldBottomY / 32, 0])[2]).toBe(lodFirstRow(0));
    expect(lodInWorld(lodOfChunk([0, World.worldBottomY / 32, 0]))).toBe(true);
    expect(lodInWorld(lodOfChunk([0, World.worldBottomY / 32 - 1, 0]))).toBe(false);
    expect(lodInWorld(lodOfChunk([0, World.worldMaxY / 32, 0]))).toBe(false);
    const j8 = lodFirstRow(8);
    expect(lodInWorld([8, 1024, j8, 1024])).toBe(true);
    expect(lodInWorld([8, 1024, j8 + 2, 1024])).toBe(true);
    expect(lodInWorld([8, 1024, j8 - 1, 1024])).toBe(false);
    expect(lodInWorld([8, 1024, j8 + 3, 1024])).toBe(false);
    // Up to level 6 a section lies wholly on one face; a coarser one holds the midplane.
    for (let level = 0; level <= 6; level++) {
      const s = sectionAt(level, [0, World.midplaneY, 0]);
      expect(sectionOrigin(s)[1]).toBe(World.midplaneY);
      expect(lodRowFace(s, -1)).toBe(FACE_B); // only its apron is below
      expect(lodRowFace(s, 0)).toBe(FACE_A);
    }
    const straddle = sectionAt(8, [0, World.midplaneY, 0]);
    expect(lodRowFace(straddle, 0)).toBe(FACE_B);
    expect(lodRowFace(straddle, 31)).toBe(FACE_A);
  });

  it('numbers sections uniquely: ids differ across rows and fall outside the world', () => {
    const ids = new Set<number>();
    for (let j = lodFirstRow(0); j <= lodLastRow(0); j++) ids.add(lodId(0, 262144, j, 262144));
    expect(ids.size).toBe(512);
    expect(ids.has(-1)).toBe(false);
    expect(lodId(0, 262144, lodFirstRow(0) - 1, 262144)).toBe(-1);
    expect(lodId(0, 262144, lodLastRow(0) + 1, 262144)).toBe(-1);
    expect(Number.isSafeInteger(lodId(0, 2 ** 19 - 1, lodLastRow(0), 2 ** 19 - 1))).toBe(true);
  });

  it('has children nest in their parent', () => {
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
    const b = { lo: -40, hi: 60, loB: -40, hiB: 60, anyInside: true, bifacial: true };
    expect(kindFromBounds(c, b)).toBe(LodKind.Content);
    expect(kindFromBounds(sectionAt(2, [0, 300, 0]), b)).toBe(LodKind.Empty);
    expect(kindFromBounds(sectionAt(2, [0, -500, 0]), b)).toBe(LodKind.Buried);
    expect(kindFromBounds(c, { ...b, anyInside: false })).toBe(LodKind.Empty);
    // Face B: its terrain's face-local bounds, mirrored — rock toward the midplane, sky below.
    expect(kindFromBounds(sectionAt(2, [0, -3000, 0]), b)).toBe(LodKind.Buried);
    expect(kindFromBounds(sectionAt(2, [0, -9000, 0]), b)).toBe(LodKind.Empty);
    expect(kindFromBounds(sectionAt(2, [0, -4100, 0]), b)).toBe(LodKind.Content);
    expect(World.midplaneY).toBe(-2048);
  });
});
