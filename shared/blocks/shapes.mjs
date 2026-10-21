// Voxel shapes for the block generator (docs/SLOPE_BLOCKS.md §1.2): every shaped piece fits one
// 1 m cell and is described by the heights of its top surface at the cell's four corners, in
// halves of a cell (0, 1 or 2), listed (NW, NE, SE, SW) with north = −Z and east = +X. The surface
// is two planar triangles split along one diagonal (NW–SE, or NE–SW once rotated an odd number of
// quarter turns), which is the hip or valley line of a corner piece.
//
// `faces` is the closed outward-wound surface of the solid, as convex polygons in cell
// coordinates, each tagged with the cell face it lies on (the mesher's axis·2 + sign: 0 +X, 1 −X,
// 2 +Y, 3 −Y, 4 +Z, 5 −Z) or SURFACE for the sloped (or flat, partial-height) top. C++ and
// TypeScript both read the table this module bakes, so the two sides agree exactly.

export const SURFACE = 6;
export const FACINGS = ["north", "east", "south", "west"];

// The nine shapes in canonical orientation (descending toward +X, corners toward +X and +Z).
export const SLOPE_SHAPES = {
  wedge: [2, 0, 0, 2],
  outer: [2, 0, 0, 0],
  inner: [2, 2, 0, 2],
  gentle_low: [1, 0, 0, 1],
  gentle_high: [2, 1, 1, 2],
  gentle_outer_low: [1, 0, 0, 0],
  gentle_outer_high: [2, 1, 1, 1],
  gentle_inner_low: [1, 1, 0, 1],
  gentle_inner_high: [2, 2, 1, 2],
};

// Quarter turns clockwise (seen from above) from the canonical east-facing orientation.
export const FACING_TURNS = { east: 0, south: 1, west: 2, north: 3 };

/** Rotates corner heights `turns` quarter turns clockwise: NW → NE → SE → SW → NW. */
export function rotateCorners(h, turns) {
  let out = h.slice();
  for (let t = 0; t < ((turns % 4) + 4) % 4; ++t)
    out = [out[3], out[0], out[1], out[2]];
  return out;
}

// Outward normals of the six cell faces (tag order +X −X +Y −Y +Z −Z).
const AXES = [
  [1, 0, 0],
  [-1, 0, 0],
  [0, 1, 0],
  [0, -1, 0],
  [0, 0, 1],
  [0, 0, -1],
];

const cross = (a, b) => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];
const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];

// Corner positions on the floor plane, in (NW, NE, SE, SW) order: [x, z].
const CORNER_XZ = [
  [0, 0],
  [1, 0],
  [1, 1],
  [0, 1],
];

/**
 * Describes a shape: `corners` (halves, NW NE SE SW), whether it is `inverted` (hangs from the
 * ceiling) and its top-surface `diagonal` (0: NW–SE, 1: NE–SW). Returns the baked geometry.
 */
export function buildShape(corners, inverted, diagonal) {
  const h = corners;
  const y = (i) => h[i] / 2;
  const vert = (i) => [CORNER_XZ[i][0], y(i), CORNER_XZ[i][1]];
  const faces = [];
  const push = (tag, pts) => {
    // Drop repeated consecutive points (a triangle given as a degenerate quad).
    const out = [];
    for (const p of pts) {
      const q = out[out.length - 1];
      if (!q || q[0] !== p[0] || q[1] !== p[1] || q[2] !== p[2]) out.push(p);
    }
    while (out.length > 1) {
      const a = out[0];
      const b = out[out.length - 1];
      if (a[0] === b[0] && a[1] === b[1] && a[2] === b[2]) out.pop();
      else break;
    }
    if (out.length < 3) return;
    // A polygon of zero area (collinear points) is not a face.
    let area2 = [0, 0, 0];
    for (let i = 1; i + 1 < out.length; ++i) {
      const c = cross(sub(out[i], out[0]), sub(out[i + 1], out[0]));
      area2 = [area2[0] + c[0], area2[1] + c[1], area2[2] + c[2]];
    }
    if (area2[0] === 0 && area2[1] === 0 && area2[2] === 0) return;
    // Wind outward: the sloped surface faces up, a cell face along its own axis.
    const want = tag === SURFACE ? [0, 1, 0] : AXES[tag];
    const dot = area2[0] * want[0] + area2[1] * want[1] + area2[2] * want[2];
    faces.push({ tag, pts: dot < 0 ? out.reverse() : out });
  };

  // Faces are listed in the order the original terrain-collision builder produced them (per axis,
  // the negative then the positive face), with each cell-face quad's corners in its (u, v) order
  // (u = axis + 1, v = axis + 2, cyclic), so the physics of cubes and slabs does not change.
  const profile = [
    [h[1], h[2]], // +X, running along z: NE at z = 0, SE at z = 1
    [h[0], h[3]], // −X: NW, SW
    [h[3], h[2]], // +Z, running along x: SW at x = 0, SE at x = 1
    [h[0], h[1]], // −Z: NW, NE
  ];
  const sideQuad = (tag) => {
    const [lo, hi] = profile[{ 0: 0, 1: 1, 4: 2, 5: 3 }[tag]];
    const a = lo / 2;
    const b = hi / 2;
    let pts;
    if (tag < 2) {
      const x = tag === 0 ? 1 : 0; // u = y, v = z
      pts = [
        [x, 0, 0],
        [x, a, 0],
        [x, b, 1],
        [x, 0, 1],
      ];
    } else {
      const z = tag === 4 ? 1 : 0; // u = x, v = y
      pts = [
        [0, 0, z],
        [1, 0, z],
        [1, b, z],
        [0, a, z],
      ];
    }
    // Positive faces keep the corner order; negative ones are wound the other way.
    return tag % 2 === 0 ? pts : [pts[0], pts[3], pts[2], pts[1]];
  };
  // The +Y quad's corners in (u = z, v = x) order, at height y.
  const topQuad = (y) => [
    [0, y, 0],
    [0, y, 1],
    [1, y, 1],
    [1, y, 0],
  ];
  push(1, sideQuad(1));
  push(0, sideQuad(0));
  // −Y: the floor, (z0, x0), (z1, x0), (z1, x1), (z0, x1) reversed for the negative face.
  push(3, [
    [0, 0, 0],
    [1, 0, 0],
    [1, 0, 1],
    [0, 0, 1],
  ]);
  // +Y: a full-height flat top is the cell's +Y face; any other top is the sloped surface.
  const flat = h.every((v) => v === h[0]);
  if (flat && h[0] === 2) {
    push(2, topQuad(1));
  } else if (flat) {
    push(SURFACE, topQuad(h[0] / 2));
  } else if (diagonal === 0) {
    push(SURFACE, [vert(0), vert(2), vert(1)]);
    push(SURFACE, [vert(0), vert(3), vert(2)]);
  } else {
    push(SURFACE, [vert(1), vert(0), vert(3)]);
    push(SURFACE, [vert(1), vert(3), vert(2)]);
  }
  push(5, sideQuad(5));
  push(4, sideQuad(4));

  // Inverted: mirror in y (and the winding), swapping the +Y and −Y cell faces.
  const outFaces = faces.map((f) => {
    if (!inverted) return f;
    const pts = f.pts.map((p) => [p[0], 1 - p[1], p[2]]).reverse();
    const tag = f.tag === 2 ? 3 : f.tag === 3 ? 2 : f.tag;
    return { tag, pts };
  });

  // Side profiles for culling: heights (halves) at the lower end, the middle and the higher end
  // of the face's running axis (z for ±X, x for ±Z), in face order +X, −X, +Z, −Z. A corner shape's
  // side is straight, so its middle is the mean of its ends.
  const mid = (a, b) => (h[a] + h[b]) / 2;
  const sides = [
    [h[1], mid(1, 2), h[2]], // +X: NE (z = 0), SE (z = 1)
    [h[0], mid(0, 3), h[3]], // −X: NW, SW
    [h[3], mid(3, 2), h[2]], // +Z: SW (x = 0), SE (x = 1)
    [h[0], mid(0, 1), h[1]], // −Z: NW, NE
  ];
  // The top as 3 × 3 surface points (halves; rows north to south, columns west to east): corners,
  // edge midpoints and the centre, which lies on the split diagonal.
  const centre = diagonal === 0 ? (h[0] + h[2]) / 2 : (h[1] + h[3]) / 2;
  const points = [
    [h[0], mid(0, 1), h[1]],
    [mid(0, 3), centre, mid(1, 2)],
    [h[3], mid(3, 2), h[2]],
  ];
  // Volume: each triangle's area (½) × its mean height (halves → cells).
  const tri = (a, b, c) => (h[a] + h[b] + h[c]) / 3 / 2;
  const volume =
    (diagonal === 0 ? tri(0, 1, 2) + tri(0, 2, 3) : tri(0, 1, 3) + tri(1, 2, 3)) * 0.5;
  // Convex solid iff the height function is concave: the diagonal's ends sit at least as high on
  // average as the other diagonal's.
  const [a, c] = diagonal === 0 ? [0, 2] : [1, 3];
  const [b, d] = diagonal === 0 ? [1, 3] : [0, 2];
  const convex = h[a] + h[c] >= h[b] + h[d];
  const maxH = Math.max(...h) / 2;
  const minH = Math.min(...h) / 2;
  return {
    corners: h.slice(),
    points,
    fan: false,
    inverted,
    diagonal,
    faces: outFaces,
    sides,
    volume,
    convex,
    // The solid's extent in y over the cell: upright [0, maxH]; inverted [1 − maxH, 1].
    minY: inverted ? 1 - maxH : 0,
    maxY: inverted ? 1 : maxH,
    // Whether the cell's top (bottom) face is entirely solid, for culling the neighbour above
    // (below): an upright piece fills its floor, an inverted one its ceiling; a cube both.
    fullTop: inverted || h.every((v) => v === 2),
    fullBottom: !inverted || h.every((v) => v === 2),
    flatMin: minH,
  };
}

/** Corner heights, inversion and diagonal of a slope state's property values. */
export function slopeShape(values) {
  const turns = FACING_TURNS[values.facing];
  return buildShape(
    rotateCorners(SLOPE_SHAPES[values.shape], turns),
    values.half === "top",
    turns % 2,
  );
}

/** A slab of the given half. */
export function slabShape(values) {
  return buildShape([1, 1, 1, 1], values.half === "top", 0);
}

export const cubeShape = () => buildShape([2, 2, 2, 2], false, 0);

/** Rotates a 3 × 3 grid `turns` quarter turns clockwise seen from above. */
export function rotateGrid(g, turns) {
  let o = g.map((r) => r.slice());
  for (let t = 0; t < ((turns % 4) + 4) % 4; ++t)
    o = o.map((_, r) => o.map((row) => row[r]).reverse());
  return o;
}

const key3 = (p) => `${p[0]},${p[1]},${p[2]}`;
function gcd(a, b) {
  a = Math.abs(a);
  b = Math.abs(b);
  while (b) [a, b] = [b, a % b];
  return a;
}
const turn = (a, b, c) => cross(sub(b, a), sub(c, b));

/**
 * A ridge shape from its 3 × 3 surface points (halves): the top is the fan of 8 triangles around
 * the centre, each quadrant split along its centre–corner diagonal (the prototypes' surface),
 * with coplanar neighbours merged into convex polygons of at most 4 points; the sides follow the
 * edge profiles (three points each). Same fields as `buildShape`, with `fan` set.
 */
export function buildRidgeShape(points, inverted) {
  // Integer working frame: x and z in halves of a cell (0..2), y in halves (0..2).
  const P = (r, c) => [c, points[r][c], r];
  const C = P(1, 1);
  const tris = [];
  for (const [kr, kc] of [
    [0, 0],
    [0, 2],
    [2, 2],
    [2, 0],
  ]) {
    const K = P(kr, kc);
    tris.push([C, P(kr, 1), K], [C, K, P(1, kc)]);
  }
  const up = (t) => (cross(sub(t[1], t[0]), sub(t[2], t[0]))[1] < 0 ? [t[0], t[2], t[1]] : t);
  const plane = (t) => {
    const n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
    const g = gcd(gcd(n[0], n[1]), n[2]) || 1;
    const m = n.map((v) => v / g);
    return `${m.join(",")}|${m[0] * t[0][0] + m[1] * t[0][1] + m[2] * t[0][2]}`;
  };
  const groups = new Map();
  for (const t0 of tris) {
    const t = up(t0);
    const k = plane(t);
    if (!groups.has(k)) groups.set(k, []);
    groups.get(k).push(t);
  }
  const surface = [];
  for (const group of groups.values()) {
    // The group's boundary: its directed edges whose reverse is not also in it.
    const edges = new Map();
    for (const t of group) {
      for (let i = 0; i < 3; ++i) {
        const a = t[i];
        const b = t[(i + 1) % 3];
        const back = `${key3(b)}>${key3(a)}`;
        if (edges.has(back)) edges.delete(back);
        else edges.set(`${key3(a)}>${key3(b)}`, [a, b]);
      }
    }
    const next = new Map([...edges.values()].map(([a, b]) => [key3(a), b]));
    const seen = new Set();
    const loops = [];
    let ok = next.size === edges.size; // a point with two outgoing edges: keep the triangles
    for (const [a] of edges.values()) {
      if (!ok || seen.has(key3(a))) continue;
      const loop = [];
      let p = a;
      while (p && !seen.has(key3(p))) {
        seen.add(key3(p));
        loop.push(p);
        p = next.get(key3(p));
      }
      const pts = loop.filter((q, i) => {
        const c = turn(loop[(i + loop.length - 1) % loop.length], q, loop[(i + 1) % loop.length]);
        return c[0] !== 0 || c[1] !== 0 || c[2] !== 0;
      });
      const convex = pts.every(
        (q, i) => turn(pts[(i + pts.length - 1) % pts.length], q, pts[(i + 1) % pts.length])[1] >= 0,
      );
      if (!convex || pts.length > 4 || pts.length < 3) ok = false;
      loops.push(pts);
    }
    if (ok) surface.push(...loops);
    else surface.push(...group);
  }
  const faces = [];
  const push = (tag, pts) => {
    const out = [];
    for (const p of pts) {
      const q = out[out.length - 1];
      if (!q || key3(q) !== key3(p)) out.push(p);
    }
    while (out.length > 1 && key3(out[0]) === key3(out[out.length - 1])) out.pop();
    if (out.length < 3) return;
    let area2 = [0, 0, 0];
    for (let i = 1; i + 1 < out.length; ++i) {
      const c = cross(sub(out[i], out[0]), sub(out[i + 1], out[0]));
      area2 = [area2[0] + c[0], area2[1] + c[1], area2[2] + c[2]];
    }
    if (area2[0] === 0 && area2[1] === 0 && area2[2] === 0) return;
    const want = tag === SURFACE ? [0, 1, 0] : AXES[tag];
    const dot = area2[0] * want[0] + area2[1] * want[1] + area2[2] * want[2];
    faces.push({ tag, pts: dot < 0 ? out.reverse() : out });
  };
  // Side profiles (halves) at the low end, middle and high end of the running axis.
  const sides = [
    [points[0][2], points[1][2], points[2][2]], // +X, along z
    [points[0][0], points[1][0], points[2][0]], // −X
    [points[2][0], points[2][1], points[2][2]], // +Z, along x
    [points[0][0], points[0][1], points[0][2]], // −Z
  ];
  const side = (tag, k) => {
    const pr = sides[k];
    const at = (s, y) => (tag < 2 ? [tag === 0 ? 1 : 0, y, s] : [s, y, tag === 4 ? 1 : 0]);
    return [at(0, 0), at(1, 0), at(1, pr[2] / 2), at(0.5, pr[1] / 2), at(0, pr[0] / 2)];
  };
  push(1, side(1, 1));
  push(0, side(0, 0));
  push(3, [
    [0, 0, 0],
    [1, 0, 0],
    [1, 0, 1],
    [0, 0, 1],
  ]);
  for (const pts of surface) push(SURFACE, pts.map((p) => [p[0] / 2, p[1] / 2, p[2] / 2]));
  push(5, side(5, 3));
  push(4, side(4, 2));
  // A side with a raised middle is a pentagon: a fan of triangles (convex, the middle is never
  // below its ends in a ridge shape).
  const split = [];
  for (const f of faces) {
    if (f.pts.length <= 4) split.push(f);
    else
      for (let i = 1; i + 1 < f.pts.length; ++i)
        split.push({ tag: f.tag, pts: [f.pts[0], f.pts[i], f.pts[i + 1]] });
  }
  const outFaces = split.map((f) => {
    if (!inverted) return f;
    const pts = f.pts.map((p) => [p[0], 1 - p[1], p[2]]).reverse();
    const tag = f.tag === 2 ? 3 : f.tag === 3 ? 2 : f.tag;
    return { tag, pts };
  });
  // Volume: each fan triangle covers ⅛ of the floor, at its mean height (halves → cells).
  let volume = 0;
  for (const t of tris) volume += ((t[0][1] + t[1][1] + t[2][1]) / 3 / 2) * 0.125;
  // Convex iff all nine points lie on or below every fan triangle's plane.
  const all = [0, 1, 2].flatMap((r) => [0, 1, 2].map((c) => P(r, c)));
  const convex = tris.every((t0) => {
    const t = up(t0);
    const n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
    return all.every(
      (p) => n[0] * (p[0] - t[0][0]) + n[1] * (p[1] - t[0][1]) + n[2] * (p[2] - t[0][2]) <= 0,
    );
  });
  const flat = points.flat();
  const maxH = Math.max(...flat) / 2;
  return {
    corners: [points[0][0], points[0][2], points[2][2], points[2][0]],
    points: points.map((r) => r.slice()),
    fan: true,
    inverted,
    diagonal: 0,
    faces: outFaces,
    sides,
    volume,
    convex,
    minY: inverted ? 1 - maxH : 0,
    maxY: inverted ? 1 : maxH,
    fullTop: inverted,
    fullBottom: !inverted,
    flatMin: Math.min(...flat) / 2,
  };
}

// ---- Stored cardinal ramps (docs/HALF_HEIGHT_GRID.md §3, Phase 41) ------------------------------
//
// On the half-height grid natural ground is shaped once, at generation, from three pieces facing
// north, east, south or west (canonical: descending toward +X): the ½ m ramp (a wedge in one cell)
// and the two halves of a 1 m (45°) ramp that spans two cells. The halves are given as 3 × 3
// surface points (halves of a cell; rows north to south, columns west to east), a height field
// whose zero part is open.

export const RAMP_SHAPES = ['ramp', 'steep_lower', 'steep_upper'];
const STEP_POINTS = {
  // Flat to the middle, then down to the floor at the east edge.
  steep_lower: [
    [2, 2, 0],
    [2, 2, 0],
    [2, 2, 0],
  ],
  // From the top at the west edge down to the floor at the middle; open beyond.
  steep_upper: [
    [2, 0, 0],
    [2, 0, 0],
    [2, 0, 0],
  ],
};

/**
 * A piece given by 3 × 3 surface points whose zero part is open (the 45° halves): the fan shape
 * of `buildRidgeShape`, its faces rebuilt so that only the solid part has any. Surface polygons at
 * zero height go, the floor covers only the solid part's footprint, side polygons lose the points
 * where they double back along the floor, and the floor counts as full only when it is.
 */
export function buildStepShape(points, inverted) {
  const upright = buildRidgeShape(points, false);
  const zero = (pts) => pts.every((p) => p[1] === 0);
  const kept = upright.faces.filter((f) => f.tag !== SURFACE || !zero(f.pts));
  const surfacePts = kept.filter((f) => f.tag === SURFACE).flatMap((f) => f.pts);
  const [x0, x1] = [Math.min(...surfacePts.map((p) => p[0])), Math.max(...surfacePts.map((p) => p[0]))];
  const [z0, z1] = [Math.min(...surfacePts.map((p) => p[2])), Math.max(...surfacePts.map((p) => p[2]))];
  const fullFloor = x0 === 0 && x1 === 1 && z0 === 0 && z1 === 1;
  // A side point where the outline doubles back (it goes on along the floor, then returns).
  const backtracks = (a, q, b) => {
    const u = sub(q, a);
    const v = sub(b, q);
    const c = cross(u, v);
    return c[0] === 0 && c[1] === 0 && c[2] === 0 && u[0] * v[0] + u[1] * v[1] + u[2] * v[2] < 0;
  };
  const faces = [];
  for (const f of kept) {
    if (f.tag === 3) {
      faces.push({
        tag: 3,
        pts: [
          [x0, 0, z0],
          [x1, 0, z0],
          [x1, 0, z1],
          [x0, 0, z1],
        ],
      });
      continue;
    }
    let pts = f.pts;
    for (let changed = true; changed && pts.length > 3; ) {
      changed = false;
      const i = pts.findIndex((q, k) =>
        backtracks(pts[(k + pts.length - 1) % pts.length], q, pts[(k + 1) % pts.length]),
      );
      if (i >= 0) {
        pts = pts.filter((_, k) => k !== i);
        changed = true;
      }
    }
    // A full-height wall's fan has a sliver of zero area (its top points are collinear): the
    // other triangles already cover the wall.
    let area = [0, 0, 0];
    for (let i = 1; i + 1 < pts.length; ++i) {
      const c = cross(sub(pts[i], pts[0]), sub(pts[i + 1], pts[0]));
      area = [area[0] + c[0], area[1] + c[1], area[2] + c[2]];
    }
    if (area[0] !== 0 || area[1] !== 0 || area[2] !== 0) faces.push({ tag: f.tag, pts });
  }
  const outFaces = faces.map((f) => {
    if (!inverted) return f;
    const pts = f.pts.map((p) => [p[0], 1 - p[1], p[2]]).reverse();
    const tag = f.tag === 2 ? 3 : f.tag === 3 ? 2 : f.tag;
    return { tag, pts };
  });
  return {
    ...upright,
    inverted,
    faces: outFaces,
    // The solid prism is convex.
    convex: true,
    minY: inverted ? 1 - upright.maxY : 0,
    maxY: inverted ? 1 : upright.maxY,
    fullTop: inverted && fullFloor,
    fullBottom: !inverted && fullFloor,
  };
}

/** A ramp state's geometry from its property values (`shape`, `facing`, `half`). */
export function rampShape(values) {
  const turns = FACING_TURNS[values.facing];
  const inverted = values.half === 'top';
  if (values.shape === 'ramp') return buildShape(rotateCorners([2, 0, 0, 2], turns), inverted, turns % 2);
  const points = STEP_POINTS[values.shape];
  if (!points) throw new Error(`unknown ramp shape ${values.shape}`);
  return buildStepShape(rotateGrid(points, turns), inverted);
}
