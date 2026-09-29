import { describe, expect, it } from 'vitest';
import { tileRect } from '../render/textures';
import { meshChunk, PADDED_VOLUME, paddedIndex, type MeshArrays } from './mesher';

/** Padded voxels with the given cells set (chunk-local, −1..32). */
function voxels(cells: [number, number, number, number][]): Uint16Array {
  const out = new Uint16Array(PADDED_VOLUME);
  for (const [x, y, z, m] of cells) out[paddedIndex(x, y, z)] = m;
  return out;
}

function normalOf(p: Float32Array, i0: number, i1: number, i2: number): number[] {
  const a = [p[i0 * 3] ?? 0, p[i0 * 3 + 1] ?? 0, p[i0 * 3 + 2] ?? 0];
  const b = [p[i1 * 3] ?? 0, p[i1 * 3 + 1] ?? 0, p[i1 * 3 + 2] ?? 0];
  const c = [p[i2 * 3] ?? 0, p[i2 * 3 + 1] ?? 0, p[i2 * 3 + 2] ?? 0];
  const e1 = b.map((v, i) => v - (a[i] ?? 0));
  const e2 = c.map((v, i) => v - (a[i] ?? 0));
  return [
    (e1[1] ?? 0) * (e2[2] ?? 0) - (e1[2] ?? 0) * (e2[1] ?? 0),
    (e1[2] ?? 0) * (e2[0] ?? 0) - (e1[0] ?? 0) * (e2[2] ?? 0),
    (e1[0] ?? 0) * (e2[1] ?? 0) - (e1[1] ?? 0) * (e2[0] ?? 0),
  ];
}

const quads = (a: MeshArrays) => a.indices.length / 6;

/** Bounds of quad `q`'s vertices. */
function bounds(a: MeshArrays, q: number): { min: number[]; max: number[] } {
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (let k = 0; k < 4; k++)
    for (let i = 0; i < 3; i++) {
      const v = a.positions[(q * 4 + k) * 3 + i] ?? 0;
      min[i] = Math.min(min[i] ?? 0, v);
      max[i] = Math.max(max[i] ?? 0, v);
    }
  return { min, max };
}

describe('greedy chunk mesher', () => {
  it('draws a lone cube as six quads, wound counter-clockwise facing out', () => {
    const { opaque, transparent } = meshChunk(voxels([[1, 1, 1, 2]]));
    expect(quads(opaque)).toBe(6);
    expect(quads(transparent)).toBe(0);
    const normals = new Set<string>();
    for (let q = 0; q < 6; q++) {
      const n = normalOf(
        opaque.positions,
        opaque.indices[q * 6] ?? 0,
        opaque.indices[q * 6 + 1] ?? 0,
        opaque.indices[q * 6 + 2] ?? 0,
      ).map(Math.sign);
      // The winding agrees with the stored normal.
      expect(n).toEqual([0, 1, 2].map((i) => opaque.normals[q * 12 + i] ?? 0));
      normals.add(n.join());
    }
    expect(normals.size).toBe(6);
  });

  it('merges a floor of one material into one quad per side', () => {
    const cells: [number, number, number, number][] = [];
    for (let z = 0; z < 32; z++) for (let x = 0; x < 32; x++) cells.push([x, 0, z, 2]);
    const { opaque } = meshChunk(voxels(cells));
    // Top, bottom, and one strip per side.
    expect(quads(opaque)).toBe(6);
    const top = [...Array(quads(opaque)).keys()].find((q) => opaque.normals[q * 12 + 1] === 1);
    expect(top).toBeDefined();
    const b = bounds(opaque, top ?? 0);
    expect(b.min).toEqual([0, 1, 0]);
    expect(b.max).toEqual([32, 1, 32]);
    // The texture repeats per block: uvs span 32 blocks, the tile stays the stone tile.
    const r = tileRect('stone');
    expect(Math.max(...opaque.uvs)).toBe(32);
    expect(opaque.tiles[0]).toBeCloseTo(r.u0);
    expect(opaque.tiles[2]).toBeCloseTo(r.u1 - r.u0);
  });

  it('keeps materials apart and hides faces against full neighbours, including the apron', () => {
    // Two different blocks side by side: the shared faces are hidden, the tops do not merge.
    let { opaque } = meshChunk(
      voxels([
        [0, 0, 0, 2],
        [1, 0, 0, 3],
      ]),
    );
    expect(quads(opaque)).toBe(10);
    // A block at the chunk's edge next to a solid neighbour chunk (apron): that face is hidden.
    ({ opaque } = meshChunk(
      voxels([
        [31, 5, 5, 2],
        [32, 5, 5, 2],
      ]),
    ));
    expect(quads(opaque)).toBe(5);
  });

  it('draws slabs half height, one quad per face, and keeps their tops open', () => {
    const { opaque } = meshChunk(
      voxels([
        [0, 0, 0, 5],
        [1, 0, 0, 5],
        [0, 1, 0, 2], // a block on a slab: the slab's top stays open
      ]),
    );
    // Slabs: 2 tops + 2 bottoms + 2×3 open sides (the shared side is hidden) = 10; the block: 6.
    expect(quads(opaque)).toBe(16);
    const slabTops = [...Array(quads(opaque)).keys()].filter(
      (q) => opaque.normals[q * 12 + 1] === 1 && bounds(opaque, q).max[1] === 0.5,
    );
    expect(slabTops.length).toBe(2);
    // Slab sides sample the bottom half of the tile: uvs span half a block upward.
    const side = [...Array(quads(opaque)).keys()].find(
      (q) => opaque.normals[q * 12] === -1 && bounds(opaque, q).max[1] === 0.5,
    );
    const vs = [0, 1, 2, 3].map((k) => opaque.uvs[((side ?? 0) * 4 + k) * 2 + 1] ?? 0);
    expect(Math.max(...vs) - Math.min(...vs)).toBe(0.5);
  });

  it('draws water in the transparent pass with its surface lowered and hides it against water', () => {
    const { opaque, transparent } = meshChunk(
      voxels([
        [0, 0, 0, 10],
        [1, 0, 0, 10],
      ]),
    );
    expect(quads(opaque)).toBe(0);
    expect(quads(transparent)).toBe(6); // merged top, bottom, and sides of the 2×1 pool
    const top = [...Array(quads(transparent)).keys()].find(
      (q) => transparent.normals[q * 12 + 1] === 1,
    );
    expect(bounds(transparent, top ?? 0).max[1]).toBe(0.875);
  });

  it('draws a ladder as one plate near the back of its cell', () => {
    // ladder_n faces −Z (face 5): the plate is at z = 1 − 0.05.
    const { opaque } = meshChunk(voxels([[0, 0, 0, 6]]));
    expect(quads(opaque)).toBe(1);
    expect(opaque.positions[2]).toBeCloseTo(0.95);
    expect(opaque.normals[2]).toBe(-1);
  });

  it('textures faces by material and face: grass top and side, unknown ids plain magenta', () => {
    const { opaque } = meshChunk(
      voxels([
        [0, 0, 0, 4],
        [5, 0, 0, 999],
      ]),
    );
    const tileOf = (q: number) => [opaque.tiles[q * 16] ?? 0, opaque.tiles[q * 16 + 1] ?? 0];
    const find = (pred: (q: number) => boolean) =>
      [...Array(quads(opaque)).keys()].find(pred) ?? -1;
    const grassTop = find(
      (q) => opaque.normals[q * 12 + 1] === 1 && bounds(opaque, q).max[0] === 1,
    );
    const grassSide = find((q) => opaque.normals[q * 12] === 1 && bounds(opaque, q).max[0] === 1);
    const unknown = find((q) => (bounds(opaque, q).min[0] ?? 0) >= 5);
    expect(tileOf(grassTop)).toEqual([tileRect('grass').u0, tileRect('grass').v0].map(Math.fround));
    expect(tileOf(grassSide)).toEqual(
      [tileRect('grassSide').u0, tileRect('grassSide').v0].map(Math.fround),
    );
    expect(tileOf(unknown)).toEqual([tileRect('plain').u0, tileRect('plain').v0].map(Math.fround));
    // Textured faces shade white (the texture carries the colour); unknown ids keep a flat colour.
    expect(opaque.colors[grassTop * 12]).toBe(1);
    expect(opaque.colors[unknown * 12 + 1]).toBe(0); // magenta
  });
});
