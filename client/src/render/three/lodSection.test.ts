import { BufferAttribute, Vector3 } from 'three';
import { describe, expect, it } from 'vitest';
import type { FlatMesh, SectionMeshes } from '../../mesh/lodMesher';
import { LodSectionGeometry, releaseOnUpload } from './lodSection';

/** `quads` quads whose vertices all sit at x = `tag` (to tell the parts apart). */
function quads(n: number, tag: number): FlatMesh {
  const positions = new Float32Array(n * 4 * 3);
  for (let i = 0; i < n * 4; i++) positions[i * 3] = tag;
  const indices = new Uint32Array(n * 6);
  for (let q = 0; q < n; q++)
    indices.set(
      [0, 1, 2, 0, 2, 3].map((i) => q * 4 + i),
      q * 6,
    );
  return {
    positions,
    normals: new Float32Array(n * 12),
    colors: new Float32Array(n * 12),
    indices,
  };
}

/** A section with 2 surface quads and `face + 1` skirt quads on each side. */
function section(): SectionMeshes {
  return {
    opaque: quads(2, 100),
    water: quads(0, 0),
    skirts: [0, 1, 2, 3, 4, 5].map((f) => quads(f + 1, f)),
  };
}

/** The parts (100 = surface, else the face) the drawn triangles belong to, with their counts. */
function drawnParts(s: LodSectionGeometry): Map<number, number> {
  const x = s.geometry.getAttribute('position');
  const out = new Map<number, number>();
  for (const i of s.drawn) out.set(x.getX(i), (out.get(x.getX(i)) ?? 0) + 1);
  return out;
}

describe('LOD section geometry (§6.6)', () => {
  it('draws the surface and the chosen sides’ skirts from one geometry', () => {
    const s = LodSectionGeometry.from(section());
    expect(s).not.toBeNull();
    if (!s) return;
    // Hidden skirts to start with: the surface alone.
    expect(drawnParts(s)).toEqual(new Map([[100, 12]]));
    // +X (face 0, 1 quad) and −Y (face 3, 4 quads).
    s.setSkirts(0b001001);
    expect(drawnParts(s)).toEqual(
      new Map([
        [100, 12],
        [0, 6],
        [3, 24],
      ]),
    );
    s.setSkirts(0b111111);
    expect(s.drawn.length).toBe(12 + 6 * (1 + 2 + 3 + 4 + 5 + 6));
    s.setSkirts(0);
    expect(drawnParts(s)).toEqual(new Map([[100, 12]]));
  });

  it('rewrites the index only when the skirts shown change', () => {
    const s = LodSectionGeometry.from(section());
    if (!s) throw new Error('no geometry');
    const index = s.geometry.getIndex();
    if (!index) throw new Error('no index');
    const version = index.version;
    s.setSkirts(0);
    expect(index.version).toBe(version);
    s.setSkirts(4);
    expect(index.version).toBe(version + 1);
  });

  it('bounds the skirts too, and has no geometry without any opaque faces', () => {
    const m = section();
    m.skirts[5] = { ...quads(1, 0), positions: new Float32Array(12).fill(-500) };
    const s = LodSectionGeometry.from(m);
    const sphere = s?.geometry.boundingSphere;
    expect(sphere?.containsPoint(new Vector3(-500, -500, -500))).toBe(true);
    expect(
      LodSectionGeometry.from({ opaque: quads(0, 0), water: quads(3, 0), skirts: [] }),
    ).toBeNull();
  });

  it('drops static vertex data from the page once it is on the GPU', () => {
    const a = releaseOnUpload(new BufferAttribute(new Float32Array(9), 3));
    a.onUploadCallback();
    expect(a.array).toBeNull();
    const s = LodSectionGeometry.from(section());
    if (!s) throw new Error('no geometry');
    for (const name of ['position', 'normal', 'color']) {
      const attribute = s.geometry.getAttribute(name) as BufferAttribute;
      attribute.onUploadCallback();
      expect(attribute.array).toBeNull();
    }
    // The index stays: it is rewritten when the skirts shown change.
    s.geometry.getIndex()?.onUploadCallback();
    s.setSkirts(1);
    expect(s.drawn.length).toBe(18);
  });
});
