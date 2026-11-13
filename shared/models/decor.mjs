// A model's decor (models.json `decor`, docs/CHARACTERS.md §11.13, WORK_QUEUE item 43): colours painted
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
// Colours are palette references (shared/textures/palette.json), written as the GLB's linear
// COLOR_0 as `repaint` does. A decor colour may not be a `repaint` source ramp (the repaint would
// take it on the next run). Applying it again changes nothing.
import { resolveColor } from "./gen.mjs";

const KINDS = {
  mark: new Set(["kind", "colour", "at", "radius", "mirror", "$comment"]),
  band: new Set(["kind", "part", "y", "r", "colours", "segments", "phase", "$comment"]),
};

const hexRgb = (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
const toLinear = (c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);

/**
 * Checks a model's `decor` (`at` names it); `parts`: the model's part names; `repaint`: its repaint
 * list (a decor colour may not be one of its `from` ramps). Throws naming the bad entry.
 */
export function validateDecor(list, palette, at = "decor", parts = undefined, repaint = []) {
  const fail = (msg) => {
    throw new Error(`${at}: ${msg}`);
  };
  if (!Array.isArray(list) || !list.length) fail("is a non-empty list");
  const sources = new Set((repaint ?? []).map((r) => r.from));
  const colour = (ref, w) => {
    try {
      resolveColor(palette, ref);
    } catch (e) {
      fail(`${w}: ${e.message}`);
    }
    const ramp = String(ref).split(".")[0];
    if (sources.has(ramp)) fail(`${w}: ${ref} is a repaint source ramp (the repaint would take it)`);
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
      d.colours.forEach((c, k) => colour(c, `${w}.colours[${k}]`));
      if (!Number.isInteger(d.segments) || d.segments < d.colours.length || d.segments % d.colours.length)
        fail(`${w}.segments is a whole multiple of the colours' count (${d.colours.length})`);
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
export function decorPlan(list, groups, pos, partOf) {
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
    } else {
      groups.forEach((vs, g) => {
        const [x, y, z] = centre[g];
        if (y < d.y[0] || y > d.y[1]) return;
        const r = Math.hypot(x, z);
        if (d.r && (r < d.r[0] || r > d.r[1])) return;
        if (d.part !== undefined && partOf(pos(vs[0])) !== d.part) return;
        out.push([vs, d.colours[bandSector(d, x, z)]]);
      });
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
      for (const [vs, ref] of decorPlan(list, facets(indices, pos.count), at, partOf)) {
        const c = linear(ref);
        for (const v of vs) {
          for (let k = 0; k < 3; k++) col.set(v, k, c[k]);
          if (col.width === 4) col.set(v, 3, 1);
        }
      }
    }
  return b;
}
