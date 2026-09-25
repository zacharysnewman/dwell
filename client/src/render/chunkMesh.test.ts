import { describe, expect, it } from 'vitest';
import { buildChunkMeshes } from './chunkMesh';

function face(x: number, y: number, z: number, f: number, material: number): number[] {
  return [x, y, z, f, material & 0xff, material >> 8, 0, 0];
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

describe('chunk meshes', () => {
  it('winds every face counter-clockwise, facing out of the cell', () => {
    const faces = Uint8Array.from([0, 1, 2, 3, 4, 5].flatMap((f) => face(1, 1, 1, f, 2)));
    const { opaque } = buildChunkMeshes(faces);
    expect(opaque.indices.length).toBe(36);
    const expected = [
      [1, 0, 0],
      [-1, 0, 0],
      [0, 1, 0],
      [0, -1, 0],
      [0, 0, 1],
      [0, 0, -1],
    ];
    for (let f = 0; f < 6; f++) {
      const n = normalOf(
        opaque.positions,
        opaque.indices[f * 6] ?? 0,
        opaque.indices[f * 6 + 1] ?? 0,
        opaque.indices[f * 6 + 2] ?? 0,
      );
      expect(n.map(Math.sign)).toEqual(expected[f]);
    }
  });

  it('draws slabs half height and water in the transparent pass', () => {
    const faces = Uint8Array.from([...face(0, 0, 0, 2, 5), ...face(3, 0, 0, 2, 10)]);
    const { opaque, transparent } = buildChunkMeshes(faces);
    expect(opaque.positions[1]).toBe(0.5); // slab top at y + 0.5
    expect(transparent.indices.length).toBe(6);
    expect(transparent.positions[1]).toBe(0.875);
  });

  it('draws a ladder as a plate near the back of its cell', () => {
    // ladder_n faces −Z (face 5): the plate is at z = 1 − 0.05.
    const { opaque } = buildChunkMeshes(Uint8Array.from(face(0, 0, 0, 5, 6)));
    expect(opaque.positions[2]).toBeCloseTo(0.95);
  });
});
