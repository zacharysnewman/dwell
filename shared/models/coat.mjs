// A model's coat (models.json `coat`, ADR 0070 §11, docs/CHARACTERS.md §11): a few key colours
// placed by body region, blended softly into one another, instead of the pack texture's per-face
// colours. The importer (shared/models/import.mjs) runs it over the GLB Blender wrote: every vertex
// of every detail level takes the coat's colour at its bind-pose position, so faces keep their own
// normals (the flat, low-poly look) while the colour runs smoothly across the body.
//
// The coat:
//
//   { "head": "+z",              the GLB axis the head points along (glTF, y up)
//     "base": "canvas",          the colour everywhere no patch covers (a palette reference)
//     "keep": ["hair_black"],    ramps whose imported colours stay (the eyes and the nose)
//     "patches": [               laid over the base in order
//       { "colour": "wolf_pale", "height": [0, 0.42], "blend": 0.08, "amount": 1 }, ... ] }
//
// A patch covers a box in the body's normalised coordinates, each an optional [from, to] in 0..1:
// `along` (0 the nose, 1 the tail's tip), `height` (0 the feet, 1 the top) and `side` (0 the
// centre line, 1 the widest point). Its edges fade over `blend` (each edge's ramp is centred on
// it; a range reaching 0 or 1 has no edge there), and `amount` is how much it covers its middle.
// Colours are palette references (shared/textures/palette.json); the blends between them are
// mixed in sRGB, so a vertex may hold a colour between two palette stops.
import { resolveColor } from "./gen.mjs";

export const COAT_AXES = ["along", "height", "side"];
const PATCH_KEYS = new Set([
  ...COAT_AXES,
  "colour",
  "blend",
  "amount",
  "$comment",
]);
const COAT_KEYS = new Set(["head", "base", "keep", "patches", "$comment"]);

const hexRgb = (hex) =>
  [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
const toLinear = (c) =>
  c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
const toSrgb = (c) =>
  c <= 0.0031308 ? c * 12.92 : 1.055 * c ** (1 / 2.4) - 0.055;

/** Checks a coat (`at` names it in messages); throws naming the bad entry. */
export function validateCoat(coat, palette, at = "coat") {
  const fail = (msg) => {
    throw new Error(`${at}: ${msg}`);
  };
  if (!coat || typeof coat !== "object") fail("is an object");
  for (const k of Object.keys(coat))
    if (!COAT_KEYS.has(k)) fail(`unknown key ${k}`);
  if (!/^[+-][xyz]$/.test(coat.head ?? ""))
    fail(`head is an axis (+x, -z, ...), not ${coat.head}`);
  const colour = (ref, where) => {
    try {
      return resolveColor(palette, ref);
    } catch (e) {
      fail(`${where}: ${e.message}`);
    }
  };
  colour(coat.base, "base");
  const keep = coat.keep ?? [];
  if (!Array.isArray(keep)) fail("keep is a list of ramps");
  for (const r of keep) if (!palette.ramps[r]) fail(`keep: no ramp ${r}`);
  if (!Array.isArray(coat.patches)) fail("patches is a list");
  coat.patches.forEach((p, k) => {
    const pat = `patch ${k}`;
    for (const key of Object.keys(p))
      if (!PATCH_KEYS.has(key)) fail(`${pat}: unknown key ${key}`);
    colour(p.colour, pat);
    if (keep.includes(String(p.colour).split(".")[0]))
      fail(
        `${pat}: ${p.colour} is a kept ramp's colour (a coat never paints one)`,
      );
    const axes = COAT_AXES.filter((a) => p[a] !== undefined);
    if (!axes.length) fail(`${pat}: no range (${COAT_AXES.join(", ")})`);
    for (const a of axes) {
      const r = p[a];
      if (
        !Array.isArray(r) ||
        r.length !== 2 ||
        !r.every(Number.isFinite) ||
        !(r[0] >= 0 && r[0] < r[1] && r[1] <= 1)
      )
        fail(`${pat}: ${a} is [from, to] with 0 <= from < to <= 1`);
    }
    if (!(p.blend > 0 && p.blend <= 0.5))
      fail(`${pat}: blend must be in (0, 0.5]`);
    if (p.amount !== undefined && !(p.amount > 0 && p.amount <= 1))
      fail(`${pat}: amount must be in (0, 1]`);
  });
  return coat;
}

const smooth = (e0, e1, x) => {
  const t = Math.min(1, Math.max(0, (x - e0) / (e1 - e0)));
  return t * t * (3 - 2 * t);
};

/** How much a patch covers the body point `q` ({along, height, side}, each 0..1). */
export function patchWeight(p, q) {
  let w = p.amount ?? 1;
  const b = p.blend / 2;
  for (const a of COAT_AXES) {
    const r = p[a];
    if (!r) continue;
    if (r[0] > 0) w *= smooth(r[0] - b, r[0] + b, q[a]);
    if (r[1] < 1) w *= 1 - smooth(r[1] - b, r[1] + b, q[a]);
  }
  return w;
}

/** The coat's colour (sRGB 0..1) at the body point `q`. */
export function coatColour(coat, palette, q) {
  let c = hexRgb(resolveColor(palette, coat.base));
  for (const p of coat.patches) {
    const w = patchWeight(p, q);
    if (w <= 0) continue;
    const to = hexRgb(resolveColor(palette, p.colour));
    c = c.map((v, k) => v + (to[k] - v) * w);
  }
  return c;
}

/** Reads a GLB's JSON and binary chunks. */
function chunks(bytes) {
  const b = Buffer.from(bytes);
  const jsonLen = b.readUInt32LE(12);
  const json = JSON.parse(b.subarray(20, 20 + jsonLen).toString("utf8"));
  return { b, json, binAt: 20 + jsonLen + 8 };
}

const COMPONENTS = {
  5126: [4, "FloatLE", 1],
  5123: [2, "UInt16LE", 65535],
  5121: [1, "UInt8", 255],
};
const WIDTH = { VEC3: 3, VEC4: 4 };

/** An accessor's element `i`, component `k`: [read(), write(v)] over the buffer. */
function view(b, json, binAt, index) {
  const a = json.accessors[index];
  const v = json.bufferViews[a.bufferView];
  const [size, fn, max] = COMPONENTS[a.componentType];
  const width = WIDTH[a.type];
  const stride = v.byteStride ?? size * width;
  const at = binAt + (v.byteOffset ?? 0) + (a.byteOffset ?? 0);
  const scale = a.normalized || max === 1 ? max : 1;
  const off = (i, k) => at + i * stride + k * size;
  return {
    count: a.count,
    width,
    get: (i, k) => b[`read${fn}`](off(i, k)) / scale,
    set: (i, k, x) =>
      max === 1
        ? b[`write${fn}`](x, off(i, k))
        : b[`write${fn}`](
            Math.round(Math.min(1, Math.max(0, x)) * max),
            off(i, k),
          ),
  };
}

/** The kept colours' sRGB bytes ("r,g,b"), from the coat's `keep` ramps. */
function keptColours(coat, palette) {
  const out = [];
  for (const r of coat.keep ?? [])
    for (const hex of Object.values(palette.ramps[r])) out.push(hexRgb(hex));
  return out;
}

/**
 * The GLB with every mesh's COLOR_0 painted by the coat (a new buffer; the input is untouched).
 * Positions are read in the meshes' own space (the bind pose; the importer's meshes have no node
 * transform), normalised over the bounds of every mesh together so each detail level gets the same
 * coat. A vertex whose colour is one of the `keep` ramps' stops keeps it, so painting a painted
 * GLB again changes nothing.
 */
export function applyCoat(bytes, coat, palette) {
  const { b: src, json, binAt } = chunks(bytes);
  const b = Buffer.from(src);
  const axis = "xyz".indexOf(coat.head[1]);
  const sign = coat.head[0] === "+" ? 1 : -1;
  const up = 1; // glTF: y up
  const across = [0, 1, 2].find((k) => k !== axis && k !== up);
  const prims = (json.meshes ?? []).flatMap((m) => m.primitives);
  const lo = [Infinity, Infinity, Infinity];
  const hi = [-Infinity, -Infinity, -Infinity];
  const views = prims.map((p) => {
    if (p.attributes.COLOR_0 === undefined)
      throw new Error("coat: a mesh without COLOR_0");
    const pos = view(b, json, binAt, p.attributes.POSITION);
    for (let i = 0; i < pos.count; i++)
      for (let k = 0; k < 3; k++) {
        lo[k] = Math.min(lo[k], pos.get(i, k));
        hi[k] = Math.max(hi[k], pos.get(i, k));
      }
    return [pos, view(b, json, binAt, p.attributes.COLOR_0)];
  });
  const centre = (lo[across] + hi[across]) / 2;
  const half = Math.max((hi[across] - lo[across]) / 2, 1e-9);
  const kept = keptColours(coat, palette);
  for (const [pos, col] of views) {
    for (let i = 0; i < pos.count; i++) {
      const s = [0, 1, 2].map((k) => toSrgb(col.get(i, k)));
      if (kept.some((c) => c.every((v, k) => Math.abs(v - s[k]) < 1.5 / 255)))
        continue;
      const h = pos.get(i, axis);
      const q = {
        along:
          sign > 0
            ? (hi[axis] - h) / (hi[axis] - lo[axis])
            : (h - lo[axis]) / (hi[axis] - lo[axis]),
        height: (pos.get(i, up) - lo[up]) / (hi[up] - lo[up]),
        side: Math.abs(pos.get(i, across) - centre) / half,
      };
      const c = coatColour(coat, palette, q);
      for (let k = 0; k < 3; k++) col.set(i, k, toLinear(c[k]));
      if (col.width === 4) col.set(i, 3, 1);
    }
  }
  return b;
}
