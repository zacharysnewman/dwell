import { describe, expect, it } from 'vitest';
import { buildChunkMeshes } from './chunkMesh';
import { tileRect } from './textures';

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

  it('maps each face into its texture tile: grass top, grass side, untextured plain', () => {
    const faces = Uint8Array.from([
      ...face(0, 0, 0, 2, 4),
      ...face(0, 0, 0, 4, 4),
      ...face(0, 0, 0, 2, 11),
    ]);
    const { opaque } = buildChunkMeshes(faces);
    expect(opaque.uvs.length).toBe((opaque.positions.length / 3) * 2);
    const uvRange = (quad: number) => {
      const us = [0, 1, 2, 3].map((k) => opaque.uvs[(quad * 4 + k) * 2] ?? 0);
      const vs = [0, 1, 2, 3].map((k) => opaque.uvs[(quad * 4 + k) * 2 + 1] ?? 0);
      return { u0: Math.min(...us), u1: Math.max(...us), v0: Math.min(...vs), v1: Math.max(...vs) };
    };
    const expectRect = (quad: number, name: Parameters<typeof tileRect>[0]) => {
      const r = tileRect(name);
      const got = uvRange(quad);
      expect(got.u0).toBeCloseTo(r.u0);
      expect(got.u1).toBeCloseTo(r.u1);
      expect(got.v0).toBeCloseTo(r.v0);
      expect(got.v1).toBeCloseTo(r.v1);
    };
    expectRect(0, 'grass');
    expectRect(1, 'grassSide');
    expectRect(2, 'plain');
    // Textured faces shade white (the texture carries the colour); the launch pad keeps its colour.
    expect(opaque.colors[0]).toBe(1);
    expect(opaque.colors[8 * 3]).not.toBe(1);
  });

  it('samples the bottom half of the tile on slab sides', () => {
    const { opaque } = buildChunkMeshes(Uint8Array.from(face(0, 0, 0, 0, 5)));
    const r = tileRect('stone');
    const vs = [0, 1, 2, 3].map((k) => opaque.uvs[k * 2 + 1] ?? 0);
    expect(Math.max(...vs)).toBeCloseTo(r.v0 + (r.v1 - r.v0) / 2);
  });
});
