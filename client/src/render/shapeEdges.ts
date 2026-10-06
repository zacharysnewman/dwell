// The outline of a voxel shape (the placement preview, docs/SLOPE_BLOCKS.md §6): the creases and
// borders of its polygons as line segments in cell coordinates. An edge between two coplanar
// polygons (the diagonal of a flat quad) is not a crease and is left out.
import type { ShapeDef, ShapeFace } from '../world/blocks';

type V3 = readonly [number, number, number];

function normalOf(f: ShapeFace): V3 {
  const [a = [0, 0, 0], b = [0, 0, 0], c = [0, 0, 0]] = f.pts;
  const e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]] as const;
  const e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]] as const;
  const n: [number, number, number] = [
    e1[1] * e2[2] - e1[2] * e2[1],
    e1[2] * e2[0] - e1[0] * e2[2],
    e1[0] * e2[1] - e1[1] * e2[0],
  ];
  const len = Math.hypot(...n) || 1;
  return [n[0] / len, n[1] / len, n[2] / len];
}

const key = (a: V3, b: V3): string => {
  const p = a.join();
  const q = b.join();
  return p < q ? `${p}|${q}` : `${q}|${p}`;
};

/** Line segment endpoints (x, y, z pairs, six floats per edge) of the shape's creases and borders. */
export function shapeEdges(shape: ShapeDef): Float32Array {
  const edges = new Map<string, { a: V3; b: V3; normals: V3[] }>();
  for (const f of shape.faces) {
    const n = normalOf(f);
    for (let i = 0; i < f.pts.length; i++) {
      const a = f.pts[i] as V3;
      const b = f.pts[(i + 1) % f.pts.length] as V3;
      const k = key(a, b);
      const e = edges.get(k) ?? { a, b, normals: [] };
      e.normals.push(n);
      edges.set(k, e);
    }
  }
  const out: number[] = [];
  for (const { a, b, normals } of edges.values()) {
    const [n0, n1] = normals;
    const flat =
      n0 !== undefined &&
      n1 !== undefined &&
      Math.abs(n0[0] - n1[0]) + Math.abs(n0[1] - n1[1]) + Math.abs(n0[2] - n1[2]) < 1e-6;
    if (!flat) out.push(...a, ...b);
  }
  return Float32Array.from(out);
}
