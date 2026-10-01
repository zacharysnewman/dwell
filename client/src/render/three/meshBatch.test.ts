import { BufferAttribute, BufferGeometry, Matrix4, MeshBasicMaterial } from 'three';
import { describe, expect, it } from 'vitest';
import { MeshBatch } from './meshBatch';

/** A section's water: `quads` quads. */
function water(quads: number): BufferGeometry {
  const g = new BufferGeometry();
  const n = quads * 4;
  g.setAttribute('position', new BufferAttribute(new Float32Array(n * 3), 3));
  g.setAttribute('normal', new BufferAttribute(new Float32Array(n * 3), 3));
  g.setAttribute('color', new BufferAttribute(new Float32Array(n * 3), 3));
  const idx = new Uint32Array(quads * 6);
  for (let q = 0; q < quads; q++)
    idx.set(
      [0, 1, 2, 0, 2, 3].map((i) => q * 4 + i),
      q * 6,
    );
  g.setIndex(new BufferAttribute(idx, 1));
  return g;
}

describe('MeshBatch', () => {
  it('holds any number of sections as they come and go, in one mesh', () => {
    const batch = new MeshBatch(new MeshBasicMaterial());
    const live = [];
    // Far more sections and vertices than the initial space, with churn.
    for (let i = 0; i < 600; i++) {
      live.push(batch.add(water(1 + (i % 97) * 20), [i * 32, 0, 0], 2));
      const gone = i % 3 === 2 ? live.splice(i % live.length, 1)[0] : undefined;
      if (gone) batch.remove(gone);
    }
    for (const h of live) batch.setVisible(h, true);
    expect(batch.mesh.instanceCount).toBe(live.length);
    // Each live section still has its own placement.
    const e = new Matrix4();
    batch.mesh.getMatrixAt(live[5]?.instance ?? -1, e);
    expect(e.elements[0]).toBe(2);
  });
});
