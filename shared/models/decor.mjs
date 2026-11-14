// A model's decor (models.json `decor`, docs/CHARACTERS.md §11.14, WORK_QUEUE item 43): colours painted
// over the imported mesh by position, for what the pack's texture had and the per-face import lost
// (the Peasant's eyes: painted in the texture, smaller than a face of the mesh) and for patterns the
// pack never had (a band of alternating colours round the hat's crown or the waist). The importer
// (shared/models/import.mjs --coat) runs it after `repaint`; gen.mjs checks the GLB is painted.
//
// It paints facets: the flat pieces of the mesh, triangles joined by shared vertex indices (the
// exporter splits a vertex where the colour or the flat normal changes, so a facet is one colour on
// one plane; painting by facet never puts two colours on one vertex). A facet is tested at its
// centre (bind pose, model units, y up), on every detail level, so each level gets the same decor.
//
//   { "kind": "mark",                 a feature: the facets whose centre is within `radius` of `at`
//     "colour": "hair_black",         (the nearest one if none is), and of [-x, y, z] with `mirror`
//     "at": [x, y, z], "radius": r, "mirror": true }
//
//   { "kind": "band",                 a ring of alternating colours round the vertical axis: the
//     "part": "hat",                  facets (of part `part`, by the part map; any part if omitted)
//     "y": [min, max],                whose centre's height is in `y` and distance from the axis in
//     "r": [min, max],                `r`, coloured by their angle round it (0 at +z, towards +x):
//     "colours": ["bark", "brass"],   `segments` equal sectors, sector k taking colours[k mod n]
//     "segments": 12, "phase": 0 }    (a multiple of the colours' count), turned by `phase` degrees
//
//   A band with "order": true colours its facets by their place round the ring instead (sorted by
//   angle, the k-th taking colours[k mod n]: strictly alternating, whatever the facets' widths; no
//   `segments`). "span": s keeps only the facets no taller than s (no long spikes); "outer": true
//   only the outermost (no other triangle of the mesh further out along the facet's direction from
//   the axis: the ring a viewer sees, not what is under it).
//
//   A band's "parts": [p0, p1, ...] (one a colour, its place in the sequence) puts each painted facet
//   in that part of the model's part map (gen.mjs, every detail level: `decorParts`), so an NPC's
//   pattern recolours each place of the ring as its own (shared/creatures/patterns.json) and the
//   shirt's or hat's recolour never repaints it. An ordered band holds a whole number of sequences
//   round the ring: when its facet count is not a multiple of the colours' count, the narrowest
//   facets left over join the segment before them (a wider segment, never two of one colour apart);
//   a ring of fewer facets than one sequence (a low detail level's) is left unpainted.
//
//   { "kind": "fill",                 one colour over a region: the facets (of part `part`) whose
//     "part": "hat", "colour": "wool_yellow",   centre's height is in `y` and distance from the axis
//     "y": [min, max], "r": [min, max] }        in `r` (the hat's crown above its band, one colour)
//
//   { "kind": "piece",                small geometry added to every detail level (the Peasant's eyes:
//     "joint": "spine.006",           smaller than a face of the mesh, which paint can't show): flat
//     "at": [x, y, z], "facing": "+z",  quads laid on the level's surface where a ray from `at`
//     "lift": 0.003, "mirror": true,  (moved out along `facing`) back along -`facing` first hits it,
//     "quads": [                      `lift` off it along that triangle's normal, each `size` [w, h]
//       { "colour": "hair_black",     (metres: across, up) at `at` [u, v] in the surface's frame (u
//         "at": [0, 0], "size": [w, h], "lift": 0 } ] }   across, v up; mirrored, u flips), skinned
//                                     wholly to `joint`. A quad's own `lift` adds to the piece's.
//     In place of `at` and `mirror`: "faces": [[808, 809], [1518, 1519]], one anchor per list, each
//     the area-weighted centre of those triangles of the full mesh (`detail_0`, primitive 0, the
//     triangle indices the Model Part Painter copies; joined by shared edges), its ray along `facing`
//     laid on every level as `at`'s is; an anchor at -x flips u as `mirror` does.
//   The added vertices sit after the mesh's own (the part map lists them, no part), recorded in the
//   primitive's `extras.decor` so a second run replaces them rather than adding more.
//
// Colours are palette references (shared/textures/palette.json), written as the GLB's linear
// COLOR_0 as `repaint` does. A decor colour may not be a `repaint` source ramp of a repaint that
// reaches its part (the repaint would take it on the next run). Applying it again changes nothing.
import { resolveColor } from "./gen.mjs";

const KINDS = {
  mark: new Set(["kind", "colour", "at", "radius", "mirror", "$comment"]),
  band: new Set(["kind", "part", "y", "r", "colours", "segments", "phase", "order", "span", "outer", "parts", "$comment"]),
  fill: new Set(["kind", "part", "y", "r", "colour", "$comment"]),
  piece: new Set(["kind", "joint", "at", "faces", "facing", "lift", "mirror", "quads", "$comment"]),
};
const QUAD_KEYS = new Set(["colour", "at", "size", "lift", "$comment"]);

const hexRgb = (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
const toLinear = (c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);

/**
 * Checks a model's `decor` (`at` names it); `parts`: the model's part names; `repaint`: its repaint
 * list (a decor colour may not be one of its `from` ramps). Throws naming the bad entry.
 */
export function validateDecor(list, palette, at = "decor", parts = undefined, repaint = [], joints = undefined) {
  const fail = (msg) => {
    throw new Error(`${at}: ${msg}`);
  };
  if (!Array.isArray(list) || !list.length) fail("is a non-empty list");
  const colour = (ref, w, part = undefined) => {
    try {
      resolveColor(palette, ref);
    } catch (e) {
      fail(`${w}: ${e.message}`);
    }
    const ramp = String(ref).split(".")[0];
    const hit = (repaint ?? []).some(
      (r) => r.from === ramp && (r.part === undefined || part === undefined || r.part === part),
    );
    if (hit) fail(`${w}: ${ref} is a repaint source ramp (the repaint would take it)`);
  };
  const range = (v, w) => {
    if (!Array.isArray(v) || v.length !== 2 || !v.every(Number.isFinite) || !(v[0] < v[1]))
      fail(`${w} is [min, max] with min < max`);
  };
  const point = (v, w) => {
    if (!Array.isArray(v) || v.length !== 3 || !v.every(Number.isFinite)) fail(`${w} is [x, y, z]`);
  };
  list.forEach((d, i) => {
    const w = `[${i}]`;
    const keys = KINDS[d?.kind];
    if (!keys) fail(`${w}.kind is one of ${Object.keys(KINDS).join(", ")}, not ${d?.kind}`);
    for (const k of Object.keys(d)) if (!keys.has(k)) fail(`${w}: unknown key ${k}`);
    if (d.kind === "mark") {
      colour(d.colour, `${w}.colour`);
      point(d.at, `${w}.at`);
      if (!(d.radius > 0)) fail(`${w}.radius is a positive distance`);
      if (d.mirror !== undefined && typeof d.mirror !== "boolean") fail(`${w}.mirror is true or false`);
    } else if (d.kind === "piece") {
      if (typeof d.joint !== "string" || (joints && !joints.includes(d.joint)))
        fail(`${w}.joint: the model's skin has no joint ${d.joint}`);
      if ((d.at === undefined) === (d.faces === undefined)) fail(`${w}: either at or faces, not both`);
      if (d.at !== undefined) point(d.at, `${w}.at`);
      else {
        if (!Array.isArray(d.faces) || !d.faces.length) fail(`${w}.faces is a non-empty list of face lists`);
        d.faces.forEach((f, k) => {
          if (!Array.isArray(f) || !f.length || !f.every((t) => Number.isInteger(t) && t >= 0))
            fail(`${w}.faces[${k}] is a non-empty list of face ids (triangle indices of detail_0)`);
        });
        if (d.mirror !== undefined) fail(`${w}.mirror: a piece by faces lists each side's faces`);
      }
      if (!/^[+-][xyz]$/.test(d.facing ?? "")) fail(`${w}.facing is an axis (+z, -x, ...)`);
      if (d.lift !== undefined && !(d.lift >= 0)) fail(`${w}.lift is a distance (0 or more)`);
      if (d.mirror !== undefined && typeof d.mirror !== "boolean") fail(`${w}.mirror is true or false`);
      if (!Array.isArray(d.quads) || !d.quads.length) fail(`${w}.quads is a non-empty list`);
      d.quads.forEach((q, k) => {
        const qw = `${w}.quads[${k}]`;
        for (const key of Object.keys(q ?? {})) if (!QUAD_KEYS.has(key)) fail(`${qw}: unknown key ${key}`);
        colour(q.colour, `${qw}.colour`);
        if (!Array.isArray(q.at) || q.at.length !== 2 || !q.at.every(Number.isFinite)) fail(`${qw}.at is [u, v]`);
        if (!Array.isArray(q.size) || q.size.length !== 2 || !q.size.every((x) => x > 0))
          fail(`${qw}.size is [w, h], both positive`);
        if (q.lift !== undefined && !(q.lift >= 0)) fail(`${qw}.lift is a distance (0 or more)`);
      });
    } else if (d.kind === "fill") {
      if (d.part !== undefined && parts && !parts.includes(d.part))
        fail(`${w}.part: the model's part map has no part ${d.part}`);
      if (d.part !== undefined && !parts) fail(`${w}.part needs a part map (models.json parts)`);
      range(d.y, `${w}.y`);
      if (d.r !== undefined) range(d.r, `${w}.r`);
      colour(d.colour, `${w}.colour`, d.part);
    } else {
      if (d.part !== undefined && parts && !parts.includes(d.part))
        fail(`${w}.part: the model's part map has no part ${d.part}`);
      if (d.part !== undefined && !parts) fail(`${w}.part needs a part map (models.json parts)`);
      range(d.y, `${w}.y`);
      if (d.r !== undefined) {
        range(d.r, `${w}.r`);
        if (d.r[0] < 0) fail(`${w}.r starts at 0 or more`);
      }
      if (!Array.isArray(d.colours) || d.colours.length < 2) fail(`${w}.colours is a list of two or more`);
      d.colours.forEach((c, k) => colour(c, `${w}.colours[${k}]`, d.part));
      if (d.parts !== undefined) {
        if (!Array.isArray(d.parts) || d.parts.length !== d.colours.length)
          fail(`${w}.parts is a list of part names, one a colour (${d.colours.length})`);
        if (!parts) fail(`${w}.parts needs a part map (models.json parts)`);
        d.parts.forEach((p, k) => {
          if (!parts.includes(p)) fail(`${w}.parts[${k}]: the model's part map has no part ${p}`);
        });
      }
      if (d.order !== undefined && d.order !== true) fail(`${w}.order is true (or left out)`);
      if (d.order && d.segments !== undefined) fail(`${w}: an ordered band has no segments`);
      if (!d.order && (!Number.isInteger(d.segments) || d.segments < d.colours.length || d.segments % d.colours.length))
        fail(`${w}.segments is a whole multiple of the colours' count (${d.colours.length})`);
      if (d.span !== undefined && !(d.span > 0)) fail(`${w}.span is a positive height`);
      if (d.outer !== undefined && typeof d.outer !== "boolean") fail(`${w}.outer is true or false`);
      if (d.phase !== undefined && !Number.isFinite(d.phase)) fail(`${w}.phase is degrees`);
    }
  });
  return list;
}

/** A band's colour index for a point (x, z) round the vertical axis. */
export function bandSector(d, x, z) {
  const deg = (((Math.atan2(x, z) * 180) / Math.PI - (d.phase ?? 0)) % 360 + 360) % 360;
  const k = Math.min(d.segments - 1, Math.floor((deg / 360) * d.segments));
  return k % d.colours.length;
}

/**
 * The facets of a mesh (`indices`, `count` vertices): each vertex's facet id (triangles joined by
 * shared vertex indices) and each facet's vertices.
 */
export function facets(indices, count) {
  const parent = Int32Array.from({ length: count }, (_, i) => i);
  const find = (a) => {
    while (parent[a] !== a) a = parent[a] = parent[parent[a]];
    return a;
  };
  for (let t = 0; t < indices.length; t += 3) {
    const a = find(indices[t]);
    for (const v of [indices[t + 1], indices[t + 2]]) {
      const b = find(v);
      if (b !== a) parent[b] = a;
    }
  }
  const used = new Uint8Array(count);
  for (const v of indices) used[v] = 1;
  const groups = new Map();
  for (let v = 0; v < count; v++) {
    if (!used[v]) continue;
    const r = find(v);
    if (!groups.has(r)) groups.set(r, []);
    groups.get(r).push(v);
  }
  return [...groups.values()];
}

/**
 * Which facets of one mesh each decor entry paints, and with what: a list of [vertices, colourRef].
 * `pos(i)` a vertex's position; `partOf(p)` the part name at a point (for bands with `part`).
 */
export function decorPlan(list, groups, pos, partOf, indices = undefined) {
  const centre = groups.map((vs) => {
    const c = [0, 0, 0];
    for (const v of vs) pos(v).forEach((x, k) => (c[k] += x / vs.length));
    return c;
  });
  const out = [];
  for (const d of list) {
    if (d.kind === "mark") {
      const ats = d.mirror ? [d.at, [-d.at[0], d.at[1], d.at[2]]] : [d.at];
      for (const a of ats) {
        const dist = centre.map((c) => Math.hypot(c[0] - a[0], c[1] - a[1], c[2] - a[2]));
        let hit = dist.flatMap((x, g) => (x <= d.radius ? [g] : []));
        if (!hit.length) hit = [dist.indexOf(Math.min(...dist))];
        for (const g of hit) out.push([groups[g], d.colour]);
      }
    } else if (d.kind === "fill") {
      groups.forEach((vs, g) => {
        const [x, y, z] = centre[g];
        if (y < d.y[0] || y > d.y[1]) return;
        const r = Math.hypot(x, z);
        if (d.r && (r < d.r[0] || r > d.r[1])) return;
        if (d.part !== undefined && !vs.some((v) => partOf(pos(v)) === d.part)) return;
        out.push([vs, d.colour]);
      });
    } else if (d.kind === "band") {
      const ring = [];
      groups.forEach((vs, g) => {
        const [x, y, z] = centre[g];
        if (y < d.y[0] || y > d.y[1]) return;
        const r = Math.hypot(x, z);
        if (d.r && (r < d.r[0] || r > d.r[1])) return;
        if (d.span !== undefined) {
          const ys = vs.map((v) => pos(v)[1]);
          if (Math.max(...ys) - Math.min(...ys) > d.span) return;
        }
        if (d.part !== undefined && !vs.some((v) => partOf(pos(v)) === d.part)) return;
        if (d.outer && indices && covered(centre[g], vs, indices, pos)) return;
        ring.push([vs, x, z]);
      });
      if (d.order) {
        const deg = ([, x, z]) => ((((Math.atan2(x, z) * 180) / Math.PI - (d.phase ?? 0)) % 360) + 360) % 360;
        ring.sort((a, b) => deg(a) - deg(b));
        // A whole number of sequences: the narrowest facets left over join the segment before them.
        const n = d.colours.length;
        if (ring.length < n) continue;
        const extra = ring.length % n;
        const width = (vs) => {
          const as = vs.map((v) => Math.atan2(pos(v)[0], pos(v)[2]));
          const lo = Math.min(...as);
          const hi = Math.max(...as);
          return hi - lo > Math.PI ? 2 * Math.PI - (hi - lo) : hi - lo;
        };
        const joined = new Set(
          ring
            .map(([vs], k) => [width(vs), k])
            .filter(([, k]) => k > 0)
            .sort((a, b) => a[0] - b[0] || a[1] - b[1])
            .slice(0, extra)
            .map(([, k]) => k),
        );
        let k = -1;
        ring.forEach(([vs], j) => {
          if (!joined.has(j) || k < 0) k++;
          out.push([vs, d.colours[((k % n) + n) % n], d.parts?.[k % n]]);
        });
      } else
        for (const [vs, x, z] of ring) {
          const s = bandSector(d, x, z);
          out.push([vs, d.colours[s], d.parts?.[s]]);
        }
    }
  }
  return out;
}

/** Whether a triangle not of facet `vs` lies further out from the vertical axis, along the
 * horizontal ray from the facet's centre `c` away from it (the facet is under something). */
function covered(c, vs, indices, pos) {
  const r = Math.hypot(c[0], c[2]) || 1;
  const d = [c[0] / r, 0, c[2] / r];
  const own = new Set(vs);
  for (let t = 0; t < indices.length; t += 3) {
    if (own.has(indices[t])) continue;
    if (rayHit(c, d, pos(indices[t]), pos(indices[t + 1]), pos(indices[t + 2])) > 1e-4) return true;
  }
  return false;
}

/** The distance along the ray (o, d) to triangle (a, b, c), or -1 (Moller-Trumbore). */
export function rayHit(o, d, a, b, c) {
  const e1 = b.map((v, k) => v - a[k]);
  const e2 = c.map((v, k) => v - a[k]);
  const p = cross(d, e2);
  const det = dot(e1, p);
  if (Math.abs(det) < 1e-12) return -1;
  const s = o.map((v, k) => v - a[k]);
  const u = dot(s, p) / det;
  if (u < 0 || u > 1) return -1;
  const q = cross(s, e1);
  const v = dot(d, q) / det;
  if (v < 0 || u + v > 1) return -1;
  return dot(e2, q) / det;
}
const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const norm = (a) => {
  const l = Math.hypot(...a) || 1;
  return a.map((x) => x / l);
};
const AXES = { x: 0, y: 1, z: 2 };

/**
 * A piece's anchors: [{ at: [x, y, z], side: 1 | -1 }], from `at` (and `mirror`), or from `faces` on
 * the full mesh (`indices`, `pos(i)`: detail_0's own triangles, before any piece): each list's
 * area-weighted centre, side the sign of its x. Throws on a face id past the mesh or faces not joined.
 */
export function pieceAnchors(d, indices = undefined, pos = undefined) {
  if (d.faces === undefined)
    return (d.mirror ? [1, -1] : [1]).map((side) => ({
      at: [d.at[0] * side, d.at[1], d.at[2]],
      side,
    }));
  if (!indices) throw new Error("a piece by faces needs the full mesh (detail_0)");
  const tris = indices.length / 3;
  return d.faces.map((list) => {
    for (const t of list) if (t >= tris) throw new Error(`a piece's face ${t} is past detail_0's ${tris} triangles`);
    // Joined: every face reaches the first through faces sharing an edge.
    const edges = (t) =>
      [0, 1, 2].map((k) => [indices[3 * t + k], indices[3 * t + ((k + 1) % 3)]].sort((a, b) => a - b).join());
    const reached = new Set([list[0]]);
    for (let grew = true; grew; ) {
      grew = false;
      for (const t of list) {
        if (reached.has(t)) continue;
        const e = new Set(edges(t));
        if ([...reached].some((r) => edges(r).some((x) => e.has(x)))) {
          reached.add(t);
          grew = true;
        }
      }
    }
    if (reached.size !== new Set(list).size)
      throw new Error(`a piece's faces [${list}] are not joined by shared edges`);
    let area = 0;
    const sum = [0, 0, 0];
    for (const t of list) {
      const [a, b, c] = [0, 1, 2].map((k) => pos(indices[3 * t + k]));
      const w =
        Math.hypot(
          ...cross(
            b.map((v, k) => v - a[k]),
            c.map((v, k) => v - a[k]),
          ),
        ) / 2;
      area += w;
      for (let k = 0; k < 3; k++) sum[k] += (w * (a[k] + b[k] + c[k])) / 3;
    }
    const at = sum.map((x) => x / (area || 1));
    return { at, side: at[0] < 0 ? -1 : 1 };
  });
}

/**
 * A piece's quads on one mesh (`indices`, `pos(i)`): a list of { corners: [4 x [x,y,z]] (counter-
 * clockwise seen from outside), normal, colour }; throws when its ray misses the mesh. `anchors`:
 * `pieceAnchors` (needed for a piece by `faces`).
 */
export function pieceQuads(d, indices, pos, anchors = pieceAnchors(d)) {
  const axis = [0, 0, 0];
  axis[AXES[d.facing[1]]] = d.facing[0] === "-" ? -1 : 1;
  const out = [];
  for (const { at, side } of anchors) {
    const o = at.map((v, k) => v + axis[k]);
    const dir = axis.map((v) => -v);
    let best = Infinity;
    let tri = -1;
    for (let t = 0; t < indices.length; t += 3) {
      const h = rayHit(o, dir, pos(indices[t]), pos(indices[t + 1]), pos(indices[t + 2]));
      if (h > 0 && h < best) [best, tri] = [h, t];
    }
    if (tri < 0) throw new Error(`a piece at [${at}] facing ${d.facing} misses the mesh`);
    const [a, b, c] = [0, 1, 2].map((k) => pos(indices[tri + k]));
    let n = norm(cross(b.map((v, k) => v - a[k]), c.map((v, k) => v - a[k])));
    if (dot(n, axis) < 0) n = n.map((x) => -x);
    const hit = o.map((v, k) => v + dir[k] * best);
    // The surface's frame: v up (the world's +y off the normal), u across (u, v, n right-handed).
    const up = norm([0, 1, 0].map((x, k) => x - n[1] * n[k]));
    const u = cross(up, n);
    for (const q of d.quads) {
      const lift = (d.lift ?? 0) + (q.lift ?? 0);
      const centre = hit.map((x, k) => x + n[k] * lift + u[k] * q.at[0] * side + up[k] * q.at[1]);
      const [hw, hh] = [q.size[0] / 2, q.size[1] / 2];
      const corner = (su, sv) => centre.map((x, k) => x + u[k] * su * hw + up[k] * sv * hh);
      out.push({ corners: [corner(-1, -1), corner(1, -1), corner(1, 1), corner(-1, 1)], normal: n, colour: q.colour });
    }
  }
  return out;
}

/** Reads a GLB's JSON chunk and where its binary chunk starts. */
function chunks(bytes) {
  const b = Buffer.from(bytes);
  const jsonLen = b.readUInt32LE(12);
  return { b, json: JSON.parse(b.subarray(20, 20 + jsonLen).toString("utf8")), binAt: 20 + jsonLen + 8 };
}

/** An accessor as [get(i, k), set(i, k, x)] over the buffer (floats, or normalised u8/u16). */
function view(b, json, binAt, index) {
  const a = json.accessors[index];
  const v = json.bufferViews[a.bufferView];
  const [size, fn, max] = { 5126: [4, "FloatLE", 1], 5123: [2, "UInt16LE", 65535], 5121: [1, "UInt8", 255], 5125: [4, "UInt32LE", 1] }[a.componentType];
  const width = { SCALAR: 1, VEC3: 3, VEC4: 4 }[a.type];
  const stride = v.byteStride ?? size * width;
  const at = binAt + (v.byteOffset ?? 0) + (a.byteOffset ?? 0);
  const off = (i, k) => at + i * stride + k * size;
  const scale = a.normalized ? max : 1;
  return {
    count: a.count,
    width,
    get: (i, k = 0) => b[`read${fn}`](off(i, k)) / scale,
    set: (i, k, x) =>
      max === 1 || !a.normalized
        ? b[`write${fn}`](x, off(i, k))
        : b[`write${fn}`](Math.round(Math.min(1, Math.max(0, x)) * max), off(i, k)),
  };
}

/**
 * The GLB with its decor painted on every mesh's COLOR_0 (a new buffer). `base`: the full mesh's
 * vertex positions and parts (`{ positions: [[x,y,z]…], parts: Uint8Array, names }`), which a
 * band's `part` is read from (the part of the full mesh's vertex nearest the facet: by position
 * only, so it is the same before and after painting).
 */
export function applyDecor(bytes, list, palette, base = undefined) {
  const { b: src, json, binAt } = chunks(bytes);
  const b = Buffer.from(src);
  const partOf = (p) => {
    if (!base) return undefined;
    let best = 0;
    let bd = Infinity;
    base.positions.forEach((q, i) => {
      const d = (q[0] - p[0]) ** 2 + (q[1] - p[1]) ** 2 + (q[2] - p[2]) ** 2;
      if (d < bd) [bd, best] = [d, i];
    });
    return base.names[base.parts[best] - 1];
  };
  const linear = (ref) => hexRgb(resolveColor(palette, ref)).map(toLinear);
  for (const m of json.meshes ?? [])
    for (const p of m.primitives) {
      if (p.attributes.COLOR_0 === undefined || p.indices === undefined) continue;
      const pos = view(b, json, binAt, p.attributes.POSITION);
      const col = view(b, json, binAt, p.attributes.COLOR_0);
      const ix = view(b, json, binAt, p.indices);
      const indices = Array.from({ length: ix.count }, (_, i) => ix.get(i));
      const at = (i) => [0, 1, 2].map((k) => pos.get(i, k));
      for (const [vs, ref] of decorPlan(list, facets(indices, pos.count), at, partOf, indices)) {
        const c = linear(ref);
        for (const v of vs) {
          for (let k = 0; k < 3; k++) col.set(v, k, c[k]);
          if (col.width === 4) col.set(v, 3, 1);
        }
      }
    }
  return b;
}

/**
 * The GLB with its decor's pieces added to every mesh (a new buffer): the vertices any earlier run
 * added (the primitive's `extras.decor`) are removed first, so running it again changes nothing.
 */
export function addPieces(bytes, list, palette) {
  const pieces = list.filter((d) => d.kind === "piece");
  const { b, json, binAt } = chunks(bytes);
  const jointNames = (json.skins?.[0]?.joints ?? []).map((j) => json.nodes[j].name);
  // Each buffer view's bytes, laid out again at the end (every attribute has a view of its own).
  const views = json.bufferViews.map((v) =>
    Buffer.from(b.subarray(binAt + (v.byteOffset ?? 0), binAt + (v.byteOffset ?? 0) + v.byteLength)),
  );
  const linear = (ref) => hexRgb(resolveColor(palette, ref)).map(toLinear);
  const SIZE = { 5126: 4, 5123: 2, 5121: 1, 5125: 4 };
  const WIDTH = { SCALAR: 1, VEC2: 2, VEC3: 3, VEC4: 4, MAT4: 16 };
  const stride = (a) => SIZE[a.componentType] * WIDTH[a.type];
  const prims = (json.meshes ?? []).flatMap((m) =>
    m.primitives.filter((p) => p.indices !== undefined && p.attributes.POSITION !== undefined).map((p) => ({ m, p })),
  );
  // Every primitive's own triangles (what an earlier run added removed).
  const own = new Map();
  for (const { m, p } of prims) {
    const names = Object.keys(p.attributes);
    for (const i of [...names.map((k) => p.attributes[k]), p.indices]) {
      const a = json.accessors[i];
      if (a.byteOffset || json.bufferViews[a.bufferView].byteStride)
        throw new Error(`mesh ${m.name}: an accessor shares or strides its buffer view`);
    }
    // Remove what an earlier run added.
    const had = p.extras?.decor ?? { vertices: 0, indices: 0 };
    const keep = json.accessors[p.attributes.POSITION].count - had.vertices;
    for (const k of names) {
      const a = json.accessors[p.attributes[k]];
      a.count = keep;
      views[a.bufferView] = views[a.bufferView].subarray(0, keep * stride(a));
    }
    const ia = json.accessors[p.indices];
    ia.count -= had.indices;
    views[ia.bufferView] = views[ia.bufferView].subarray(0, ia.count * stride(ia));
    if (p.extras) delete p.extras.decor;
    if (p.extras && !Object.keys(p.extras).length) delete p.extras;
    const posView = views[json.accessors[p.attributes.POSITION].bufferView];
    const pos = (i) => [0, 1, 2].map((k) => posView.readFloatLE(i * 12 + k * 4));
    const idxSize = SIZE[ia.componentType];
    const idxView = views[ia.bufferView];
    const indices = Array.from({ length: ia.count }, (_, i) =>
      idxSize === 2 ? idxView.readUInt16LE(i * 2) : idxView.readUInt32LE(i * 4),
    );
    own.set(p, { indices, pos, keep, idxSize });
  }
  // The anchors, once, from the full mesh's first primitive (a piece by `faces` names its triangles).
  const full = prims.find(({ m }) => m.name === "detail_0") ?? prims[0];
  const anchors = pieces.map((d) => {
    const base = d.faces && full ? own.get(full.p) : undefined;
    return pieceAnchors(d, base?.indices, base?.pos);
  });
  for (const { m, p } of prims) {
    const names = Object.keys(p.attributes);
    const ia = json.accessors[p.indices];
    const posAcc = json.accessors[p.attributes.POSITION];
    const { indices, pos, keep, idxSize } = own.get(p);
    const quads = pieces.flatMap((d, n) => {
      const joint = jointNames.indexOf(d.joint);
      if (joint < 0) throw new Error(`a piece's joint ${d.joint} is not in the skin`);
      return pieceQuads(d, indices, pos, anchors[n]).map((q) => ({
        ...q,
        joint,
      }));
    });
    const add = quads.length * 4;
    if (idxSize === 2 && keep + add > 65535) throw new Error(`mesh ${m.name}: too many vertices for 16-bit indices`);
    for (const k of names) {
      const a = json.accessors[p.attributes[k]];
      const size = SIZE[a.componentType];
      const w = WIDTH[a.type];
      const extra = Buffer.alloc(add * w * size);
      const put = (i, c, x) => {
        const off = (i * w + c) * size;
        const unit = (max) => (a.normalized ? Math.round(Math.min(1, Math.max(0, x)) * max) : x);
        if (a.componentType === 5126) extra.writeFloatLE(x, off);
        else if (a.componentType === 5123) extra.writeUInt16LE(unit(65535), off);
        else if (a.componentType === 5121) extra.writeUInt8(unit(255), off);
        else extra.writeUInt32LE(x, off);
      };
      quads.forEach((q, n) => {
        const c = linear(q.colour);
        for (let v = 0; v < 4; v++) {
          const vals =
            {
              POSITION: q.corners[v],
              NORMAL: q.normal,
              COLOR_0: [...c, 1],
              JOINTS_0: [q.joint, 0, 0, 0],
              WEIGHTS_0: [1, 0, 0, 0],
            }[k] ?? [];
          for (let cc = 0; cc < w; cc++) put(n * 4 + v, cc, vals[cc] ?? 0);
        }
      });
      views[a.bufferView] = Buffer.concat([views[a.bufferView], extra]);
      a.count = keep + add;
    }
    // POSITION's bounds (glTF requires them).
    const pv = views[posAcc.bufferView];
    posAcc.min = [Infinity, Infinity, Infinity];
    posAcc.max = [-Infinity, -Infinity, -Infinity];
    for (let i = 0; i < posAcc.count; i++)
      for (let c = 0; c < 3; c++) {
        const x = pv.readFloatLE((i * 3 + c) * 4);
        posAcc.min[c] = Math.min(posAcc.min[c], x);
        posAcc.max[c] = Math.max(posAcc.max[c], x);
      }
    if (!quads.length) continue;
    const extraIdx = Buffer.alloc(quads.length * 6 * idxSize);
    quads.forEach((_, n) =>
      [0, 1, 2, 0, 2, 3].forEach((v, j) => {
        const x = keep + n * 4 + v;
        if (idxSize === 2) extraIdx.writeUInt16LE(x, (n * 6 + j) * 2);
        else extraIdx.writeUInt32LE(x, (n * 6 + j) * 4);
      }),
    );
    views[ia.bufferView] = Buffer.concat([views[ia.bufferView], extraIdx]);
    ia.count += quads.length * 6;
    p.extras = {
      ...(p.extras ?? {}),
      decor: { vertices: add, indices: quads.length * 6 },
    };
  }
  // The buffer laid out again (views 4-byte aligned) and the GLB written.
  let off = 0;
  const bins = [];
  json.bufferViews.forEach((v, i) => {
    const pad = (4 - (off % 4)) % 4;
    if (pad) bins.push(Buffer.alloc(pad));
    off += pad;
    v.byteOffset = off;
    v.byteLength = views[i].length;
    bins.push(views[i]);
    off += views[i].length;
  });
  if (off % 4) bins.push(Buffer.alloc(4 - (off % 4)));
  const bin = Buffer.concat(bins);
  json.buffers[0].byteLength = bin.length;
  let text = Buffer.from(JSON.stringify(json), "utf8");
  if (text.length % 4) text = Buffer.concat([text, Buffer.alloc(4 - (text.length % 4), 0x20)]);
  const u32 = (...xs) => {
    const h = Buffer.alloc(xs.length * 4);
    xs.forEach((x, i) => h.writeUInt32LE(x, i * 4));
    return h;
  };
  return Buffer.concat([
    u32(0x46546c67, 2, 28 + text.length + bin.length),
    u32(text.length, 0x4e4f534a),
    text,
    u32(bin.length, 0x004e4942),
    bin,
  ]);
}

/**
 * Every detail level's parts (`levels`, from parts.mjs `allLevelParts`, in mesh order detail_0,
 * detail_1, …) with each band's `parts` put on the facets it paints: a new list. `names`: the part
 * map's part names; `base`: `decorBase` (a band's `part` filter).
 */
export function decorParts(bytes, list, names, levels, base) {
  const withParts = list.filter((d) => d.kind === "band" && d.parts);
  if (!withParts.length) return levels;
  const { b, json, binAt } = chunks(bytes);
  const partOf = (p) => {
    let best = 0;
    let bd = Infinity;
    base.positions.forEach((q, i) => {
      const d = (q[0] - p[0]) ** 2 + (q[1] - p[1]) ** 2 + (q[2] - p[2]) ** 2;
      if (d < bd) [bd, best] = [d, i];
    });
    return base.names[base.parts[best] - 1];
  };
  const meshes = (json.meshes ?? []).filter((m) => /^detail_\d+$/.test(m.name));
  meshes.sort((x, y) => Number(x.name.slice(7)) - Number(y.name.slice(7)));
  return levels.map((lv, l) => {
    const m = meshes[l];
    if (!m) return lv;
    const p = m.primitives[0];
    const pos = view(b, json, binAt, p.attributes.POSITION);
    const ix = view(b, json, binAt, p.indices);
    const indices = Array.from({ length: ix.count }, (_, i) => ix.get(i));
    const at = (i) => [0, 1, 2].map((k) => pos.get(i, k));
    const out = Uint8Array.from(lv);
    for (const [vs, , part] of decorPlan(withParts, facets(indices, pos.count), at, partOf, indices))
      if (part !== undefined) for (const v of vs) out[v] = names.indexOf(part) + 1;
    return out;
  });
}

/** The full mesh's positions and parts (a part map file), which a band's `part` is read from. */
export function decorBase(bytes, partsFile) {
  const { json, b, binAt } = chunks(bytes);
  const m = json.meshes.find((x) => x.name === "detail_0") ?? json.meshes[0];
  const pos = view(b, json, binAt, m.primitives[0].attributes.POSITION);
  return {
    positions: Array.from({ length: pos.count }, (_, i) => [0, 1, 2].map((k) => pos.get(i, k))),
    parts: Uint8Array.from(partsFile.vertices.join(""), Number),
    names: partsFile.parts,
  };
}

/** How many vertices the pieces added to a GLB's mesh `name` (its primitive's `extras.decor`). */
export function addedVertices(bytes, name) {
  const { json } = chunks(bytes);
  return json.meshes?.find((m) => m.name === name)?.primitives?.[0]?.extras?.decor?.vertices ?? 0;
}
