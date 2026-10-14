import { describe, expect, it } from 'vitest';
import { SHAPES, STATE_DEFS, parseState, type ShapeDef, type ShapeFace } from './blocks';

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

function shapeOf(text: string): ShapeDef {
  const parsed = parseState(text);
  if ('error' in parsed) throw new Error(parsed.error);
  const state = STATE_DEFS[parsed.id];
  const shape = state && SHAPES[state.shape];
  if (!shape) throw new Error(`no shape for ${text}`);
  return shape;
}

describe('shape table', () => {
  // The shaped families were cut with the baseline (Phase 15): the table is the empty shape and
  // the cube. Shaped blocks return as adaptive natural blocks (Phase 18).
  it('describes the cube and the empty shape', () => {
    expect(volumeOf(shapeOf('dwell:stone'))).toBeCloseTo(1, 9);
    expect(isClosed(shapeOf('dwell:stone'))).toBe(true);
    expect(isConvex(shapeOf('dwell:stone'))).toBe(true);
    expect(SHAPES[0]?.faces).toEqual([]);
  });
});
