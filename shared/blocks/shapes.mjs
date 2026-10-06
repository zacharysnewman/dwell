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

  // Side profiles for culling: heights (halves) at the lower and higher coordinate of the face's
  // running axis (z for ±X, x for ±Z), in face order +X, −X, +Z, −Z.
  const sides = [
    [h[1], h[2]], // +X: NE (z = 0), SE (z = 1)
    [h[0], h[3]], // −X: NW, SW
    [h[3], h[2]], // +Z: SW (x = 0), SE (x = 1)
    [h[0], h[1]], // −Z: NW, NE
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
