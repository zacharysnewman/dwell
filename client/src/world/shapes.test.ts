import { describe, expect, it } from 'vitest';
import { SHAPES, STATE_DEFS, parseState, type ShapeDef, type ShapeFace } from './blocks';
import { faceCovered } from '../mesh/mesher';

type V3 = readonly [number, number, number];

const sub = (a: V3, b: V3): V3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const cross = (a: V3, b: V3): V3 => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];
const dot = (a: V3, b: V3): number => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const vertex = (f: ShapeFace, i: number): V3 => f.pts[i] ?? [0, 0, 0];

/** Volume by the divergence theorem over the fan-triangulated outward faces. */
function volumeOf(shape: ShapeDef): number {
  let six = 0;
  for (const f of shape.faces) {
    for (let k = 1; k + 1 < f.pts.length; k++) {
      six += dot(vertex(f, 0), cross(vertex(f, k), vertex(f, k + 1)));
    }
  }
  return six / 6;
}

/** Every directed edge of the surface has its reverse on it exactly as often. */
function isClosed(shape: ShapeDef): boolean {
  const edges = new Map<string, number>();
  for (const f of shape.faces) {
    for (let i = 0; i < f.pts.length; i++) {
      const a = vertex(f, i);
      const b = vertex(f, (i + 1) % f.pts.length);
      const key = `${a.join()}>${b.join()}`;
      edges.set(key, (edges.get(key) ?? 0) + 1);
    }
  }
  for (const [key, n] of edges) {
    const [a = '', b = ''] = key.split('>');
    if (edges.get(`${b}>${a}`) !== n) return false;
  }
  return true;
}

/** Convex iff every vertex lies on or inside every face's plane. */
function isConvex(shape: ShapeDef): boolean {
  for (const f of shape.faces) {
    const n = cross(sub(vertex(f, 1), vertex(f, 0)), sub(vertex(f, 2), vertex(f, 0)));
    for (const g of shape.faces)
      for (const p of g.pts) if (dot(n, sub(p, vertex(f, 0))) > 1e-9) return false;
  }
  return true;
}

// The §1.2 table in the canonical orientation (facing east, upright): heights NW NE SE SW in
// cells, volume and convexity.
const TABLE: [string, number[], number, boolean][] = [
  ['wedge', [1, 0, 0, 1], 1 / 2, true],
  ['outer', [1, 0, 0, 0], 1 / 3, true],
  ['inner', [1, 1, 0, 1], 2 / 3, false],
  ['gentle_low', [0.5, 0, 0, 0.5], 1 / 4, true],
  ['gentle_high', [1, 0.5, 0.5, 1], 3 / 4, true],
  ['gentle_outer_low', [0.5, 0, 0, 0], 1 / 6, true],
  ['gentle_outer_high', [1, 0.5, 0.5, 0.5], 2 / 3, true],
  ['gentle_inner_low', [0.5, 0.5, 0, 0.5], 1 / 3, false],
  ['gentle_inner_high', [1, 1, 0.5, 1], 5 / 6, false],
];

function shapeOf(text: string): ShapeDef {
  const parsed = parseState(text);
  if ('error' in parsed) throw new Error(parsed.error);
  const state = STATE_DEFS[parsed.id];
  const shape = state && SHAPES[state.shape];
  if (!shape) throw new Error(`no shape for ${text}`);
  return shape;
}

describe('shape table', () => {
  it('matches the design: corner heights, volumes and convexity of every shape', () => {
    for (const [name, corners, volume, convex] of TABLE) {
      const shape = shapeOf(`dwell:stone_slope[facing=east,half=bottom,shape=${name}]`);
      expect(
        shape.corners.map((h) => h / 2),
        name,
      ).toEqual(corners);
      expect(volumeOf(shape), name).toBeCloseTo(volume, 9);
      expect(shape.volume, name).toBeCloseTo(volume, 9);
      expect(isConvex(shape), name).toBe(convex);
      expect(shape.convex, name).toBe(convex);
      expect(isClosed(shape), name).toBe(true);
    }
  });

  it('is a closed outward-wound solid for every facing and half', () => {
    for (const [name, , volume, convex] of TABLE) {
      for (const facing of ['north', 'east', 'south', 'west']) {
        for (const half of ['bottom', 'top']) {
          const shape = shapeOf(`dwell:stone_slope[facing=${facing},half=${half},shape=${name}]`);
          const label = `${name} ${facing} ${half}`;
          expect(isClosed(shape), label).toBe(true);
          expect(volumeOf(shape), label).toBeCloseTo(volume, 9); // positive: wound outward
          expect(isConvex(shape), label).toBe(convex);
          expect(shape.inverted, label).toBe(half === 'top');
        }
      }
    }
  });

  it('describes cubes and slabs too', () => {
    expect(volumeOf(shapeOf('dwell:stone'))).toBeCloseTo(1, 9);
    expect(volumeOf(shapeOf('dwell:stone_slab'))).toBeCloseTo(0.5, 9);
    expect(shapeOf('dwell:stone_slab[half=top]').inverted).toBe(true);
    expect(SHAPES[0]?.faces).toEqual([]); // the empty shape
  });
});

// Whether (a, b) on the plane of `axis` lies inside a polygon (projected along the axis).
function inPolygon(f: ShapeFace, axis: number, a: number, b: number): boolean {
  const u = (axis + 1) % 3;
  const v = (axis + 2) % 3;
  let sign = 0;
  for (let i = 0; i < f.pts.length; i++) {
    const p = vertex(f, i);
    const q = vertex(f, (i + 1) % f.pts.length);
    const c = (q[u] ?? 0) - (p[u] ?? 0);
    const cross2 = c * (b - (p[v] ?? 0)) - ((q[v] ?? 0) - (p[v] ?? 0)) * (a - (p[u] ?? 0));
    if (Math.abs(cross2) < 1e-9) continue;
    const s = cross2 > 0 ? 1 : -1;
    if (sign === 0) sign = s;
    else if (s !== sign) return false;
  }
  return sign !== 0;
}

/** Height of the shape's top surface at (x, z) in the cell (cells; mirrors SurfaceHeight in block_shape.cpp). */
function surface(s: ShapeDef, x: number, z: number): number {
  const h = (i: number): number => (s.corners[i] ?? 0) / 2;
  if (s.diagonal === 0) {
    return x >= z
      ? h(0) + (h(1) - h(0)) * x + (h(2) - h(1)) * z
      : h(0) + (h(2) - h(3)) * x + (h(3) - h(0)) * z;
  }
  return x + z <= 1
    ? h(0) + (h(1) - h(0)) * x + (h(3) - h(0)) * z
    : h(2) + (h(3) - h(2)) * (1 - x) + (h(1) - h(2)) * (1 - z);
}

/** Is there solid on the plane of the cell face `face`, at in-plane point (a, b)? */
function solidOnFace(s: ShapeDef, face: number, a: number, b: number): boolean {
  const axis = face >> 1;
  if (axis === 1) return face === 2 ? s.fullTop : s.fullBottom;
  // a runs along u = axis + 1, b along v = axis + 2: for ±X (y, z), for ±Z (x, y).
  const x = axis === 0 ? (face === 0 ? 1 : 0) : a;
  const z = axis === 0 ? b : face === 4 ? 1 : 0;
  const y = axis === 0 ? a : b;
  const h = surface(s, x, z);
  const lo = s.inverted ? 1 - h : 0;
  const hi = s.inverted ? 1 : h;
  return hi - lo > 1e-6 && y > lo + 1e-6 && y < hi - 1e-6;
}

describe('shape adjacency', () => {
  it('leaves no holes between any two shapes on any side (the mesher culls exactly)', () => {
    const distinct = new Map<number, ShapeDef>();
    for (const st of STATE_DEFS) {
      if (st.shape !== 0 && !distinct.has(st.shape))
        distinct.set(st.shape, SHAPES[st.shape] as ShapeDef);
    }
    expect(distinct.size).toBeGreaterThanOrEqual(70);
    const N = 7;
    let checked = 0;
    const failures: string[] = [];
    for (const [ia, a] of [...distinct.entries()]) {
      for (const [ib, b] of [...distinct.entries()]) {
        for (let face = 0; face < 6; face++) {
          const axis = face >> 1;
          const opposite = face ^ 1;
          const coveredA = faceCovered(a, face, b);
          const coveredB = faceCovered(b, opposite, a);
          for (let i = 0; i < N; i++) {
            for (let j = 0; j < N; j++) {
              const pu = (i + 0.31) / N;
              const pv = (j + 0.57) / N;
              const drawn =
                (coveredA
                  ? 0
                  : a.faces.filter((f) => f.tag === face && inPolygon(f, axis, pu, pv)).length) +
                (coveredB
                  ? 0
                  : b.faces.filter((f) => f.tag === opposite && inPolygon(f, axis, pu, pv)).length);
              const inA = solidOnFace(a, face, pu, pv);
              const inB = solidOnFace(b, opposite, pu, pv);
              checked++;
              // Solid on exactly one side: that surface is drawn once. Neither: nothing is drawn.
              if ((inA !== inB && drawn !== 1) || (!inA && !inB && drawn !== 0)) {
                if (failures.length < 5)
                  failures.push(
                    `${String(ia)} | ${String(ib)} face ${String(face)} at ${String(pu)},${String(pv)}: drawn ${String(drawn)}`,
                  );
              }
            }
          }
        }
      }
    }
    expect(failures).toEqual([]);
    expect(checked).toBeGreaterThan(100000);
  }, 120_000);
});
