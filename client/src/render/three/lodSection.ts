// One LOD section's opaque surface and its six skirts as a single geometry (ARCHITECTURE.md §6.6):
// one draw call per section instead of one per mesh. Which skirts show is chosen by rewriting the
// index — the surface's triangles, then those of each side whose skirt shows — and drawing only
// that much of it; the vertices are uploaded once and never change. (As separate meshes, the
// skirts were two thirds of the LOD's draw calls.)
import { BufferAttribute, BufferGeometry } from 'three';
import type { FlatMesh, SectionMeshes } from '../../mesh/lodMesher';

/** Drops an attribute's CPU copy once it is on the GPU (static geometry: never read back). */
export function releaseOnUpload(attribute: BufferAttribute): BufferAttribute {
  return attribute.onUpload(function (this: BufferAttribute) {
    // three.js types the array as always present; it is not read again after the upload.
    (this as unknown as { array: ArrayLike<number> | null }).array = null;
  });
}

export class LodSectionGeometry {
  private mask = -1;

  private constructor(
    readonly geometry: BufferGeometry,
    /** Triangle indices of the surface (0) and of each side's skirt (1 + face), into `geometry`. */
    private readonly parts: Uint32Array[],
    private readonly index: BufferAttribute,
  ) {}

  /** The section's surface and skirts in one geometry, or null if it has no opaque faces at all. */
  static from(meshes: SectionMeshes): LodSectionGeometry | null {
    const meshList: FlatMesh[] = [meshes.opaque, ...meshes.skirts];
    let vertices = 0;
    let indices = 0;
    for (const m of meshList) {
      vertices += m.positions.length / 3;
      indices += m.indices.length;
    }
    if (indices === 0) return null;
    const positions = new Float32Array(vertices * 3);
    const normals = new Float32Array(vertices * 3);
    const colors = new Float32Array(vertices * 3);
    const parts: Uint32Array[] = [];
    let base = 0;
    for (const m of meshList) {
      positions.set(m.positions, base * 3);
      normals.set(m.normals, base * 3);
      colors.set(m.colors, base * 3);
      const part = new Uint32Array(m.indices.length);
      for (let i = 0; i < part.length; i++) part[i] = (m.indices[i] ?? 0) + base;
      parts.push(part);
      base += m.positions.length / 3;
    }
    const g = new BufferGeometry();
    g.setAttribute('position', new BufferAttribute(positions, 3));
    g.setAttribute('normal', new BufferAttribute(normals, 3));
    g.setAttribute('color', new BufferAttribute(colors, 3));
    // The bounds cover the skirts too: computed before the upload drops the positions.
    g.computeBoundingSphere();
    for (const name of ['position', 'normal', 'color']) {
      releaseOnUpload(g.getAttribute(name) as BufferAttribute);
    }
    const index = new BufferAttribute(new Uint32Array(indices), 1);
    g.setIndex(index);
    const section = new LodSectionGeometry(g, parts, index);
    section.setSkirts(0);
    return section;
  }

  /** Shows the skirts of the sides in `mask` (bit per face index) and hides the rest. */
  setSkirts(mask: number): void {
    if (mask === this.mask) return;
    this.mask = mask;
    const out = this.index.array as Uint32Array;
    let n = 0;
    this.parts.forEach((part, i) => {
      if (i > 0 && (mask & (1 << (i - 1))) === 0) return;
      out.set(part, n);
      n += part.length;
    });
    this.geometry.setDrawRange(0, n);
    this.index.clearUpdateRanges();
    if (n > 0) this.index.addUpdateRange(0, n);
    this.index.needsUpdate = true;
  }

  /** Indices drawn (tests). */
  get drawn(): Uint32Array {
    return (this.index.array as Uint32Array).subarray(0, this.geometry.drawRange.count);
  }
}
