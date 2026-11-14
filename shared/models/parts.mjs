#!/usr/bin/env node
// A skinned model's part map (docs/CHARACTERS.md §11.11, BUGS.md #96, WORK_QUEUE item 43): which part
// (skin, hair, hat, shirt, …) each vertex of the model's full mesh (`detail_0`) belongs to, so a
// creature's colour variants (creatures.json `colours`, per `part`) recolour by part, not by colour.
// The file (models.json `parts`, e.g. shared/models/parts/peasant.json) holds:
//
//   { "parts": ["skin", "hair", ...],     the part names (at most MAX_PARTS)
//     "seed": [ rule, ... ],              how `--seed` first painted it (below)
//     "vertices": ["0012…", ...] }        one character a detail_0 vertex, in order, in rows of
//                                          ROW characters: 0 no part, k the k-th part (1-based)
//
// The vertices are the source of truth (a prototype page paints them by hand, WORK_QUEUE item 43);
// `seed` only makes a first painting from the mesh. A seed rule paints the vertices it matches with
// its `part`, the first matching rule winning; a rule's tests (all must hold, at least one given):
//
//   near:   [x, y, z]  the vertex is on the mesh island (welded, connected triangles) nearest that
//                      point (model units, bind pose: the hat, the hair)
//   ramps:  [ramp]     the vertex's colour is nearest a stop of one of these palette ramps
//   joints: [joint] with weight w (default 0.5): the vertex has at least w of its skin weight on them
//   y:      [min, max] its height (model units)
//
// The other detail levels take their parts from the full mesh: each triangle the part of the full
// mesh's nearest triangle of the same colour (by centre; any colour if none is near), every vertex the
// part of its first triangle (`levelParts`). gen.mjs checks the file and writes the levels' parts
// into the game's table.
//
//   node shared/models/parts.mjs --seed <model>     paint the part map from its `seed` rules
//   node shared/models/parts.mjs --stats <model>    vertices per part, each level (and by colour:
//                                                   what the colour remap matched before parts)
//   node shared/models/parts.mjs --render <model> [--out <dir>] [--level k]
//                                                   front, side and back PNGs, a colour a part
import { readFileSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { deflateSync } from "node:zlib";
import { glbAccessor, glbJson, paletteStops } from "./gen.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..", "..");

/** The most parts a model may have (the skinned shader's remap rows: dwell_models::MAX_REMAP). */
export const MAX_PARTS = 9;
/** Characters a row of `vertices`. */
export const ROW = 100;
const RULE_KEYS = new Set(["part", "near", "ramps", "joints", "weight", "y", "$comment"]);

/** A GLB mesh by name: positions, linear colours, joints, weights, triangle indices. */
export function readMesh(bytes, name) {
  const g = glbJson(bytes);
  const m = (g.meshes ?? []).find((x) => x.name === name);
  if (!m) throw new Error(`no mesh ${name}`);
  const p = m.primitives[0];
  const get = (k) => glbAccessor(bytes, p.attributes[k]);
  const pos = get("POSITION").values;
  const n = pos.length / 3;
  const col = p.attributes.COLOR_0 === undefined ? null : get("COLOR_0");
  const joints = p.attributes.JOINTS_0 === undefined ? null : get("JOINTS_0").values;
  const weights = p.attributes.WEIGHTS_0 === undefined ? null : get("WEIGHTS_0").values;
  const indices =
    p.indices === undefined
      ? Array.from({ length: n }, (_, i) => i)
      : glbAccessor(bytes, p.indices).values;
  const at = (a, w) => (i) => a.slice(i * w, i * w + w);
  return {
    count: n,
    position: at(pos, 3),
    colour: col ? (i) => col.values.slice(i * col.width, i * col.width + 3) : () => [1, 1, 1],
    joints: joints ? at(joints, 4) : () => [0, 0, 0, 0],
    weights: weights ? at(weights, 4) : () => [1, 0, 0, 0],
    indices,
    jointNames: (g.skins?.[0]?.joints ?? []).map((j) => g.nodes?.[j]?.name),
  };
}

const toSrgb = (c) => (c <= 0.0031308 ? c * 12.92 : 1.055 * c ** (1 / 2.4) - 0.055);
const hexRgb = (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);

/** The palette ramp a linear colour is nearest a stop of (`exclude`d ramps left out). */
export function nearestRamp(palette, linear, exclude = []) {
  const s = linear.map(toSrgb);
  let best = null;
  let bd = Infinity;
  for (const { name, hex } of paletteStops(palette, exclude)) {
    const c = hexRgb(hex);
    const d = c.reduce((a, v, k) => a + (v - s[k]) ** 2, 0);
    if (d < bd) [bd, best] = [d, name.split(".")[0]];
  }
  return best;
}

/** Each vertex's island: vertices welded by position and joined by triangles share one. */
export function islands(mesh) {
  const parent = Array.from({ length: mesh.count }, (_, i) => i);
  const find = (a) => {
    while (parent[a] !== a) a = parent[a] = parent[parent[a]];
    return a;
  };
  const join = (a, b) => {
    parent[find(a)] = find(b);
  };
  const weld = new Map();
  for (let i = 0; i < mesh.count; i++) {
    const k = mesh.position(i).map((v) => v.toFixed(4)).join(",");
    if (weld.has(k)) join(i, weld.get(k));
    else weld.set(k, i);
  }
  for (let t = 0; t < mesh.indices.length; t += 3) {
    join(mesh.indices[t + 1], mesh.indices[t]);
    join(mesh.indices[t + 2], mesh.indices[t]);
  }
  return Array.from({ length: mesh.count }, (_, i) => find(i));
}

/** Checks a part map file (`at` names it); `count` the full mesh's vertices, if known. */
export function validateParts(file, at = "parts", count = undefined) {
  const fail = (msg) => {
    throw new Error(`${at}: ${msg}`);
  };
  if (!file || typeof file !== "object") fail("is an object");
  for (const k of Object.keys(file))
    if (!["parts", "seed", "vertices", "$comment"].includes(k)) fail(`unknown key ${k}`);
  const { parts } = file;
  if (!Array.isArray(parts) || !parts.length) fail("parts is a non-empty list of names");
  if (parts.length > MAX_PARTS) fail(`${parts.length} parts, at most ${MAX_PARTS}`);
  const seen = new Set();
  for (const p of parts) {
    if (typeof p !== "string" || !/^[a-z][a-z0-9_]*$/.test(p))
      fail(`part ${JSON.stringify(p)} is not a lower_snake_case name`);
    if (seen.has(p)) fail(`part ${p} twice`);
    seen.add(p);
  }
  (file.seed ?? []).forEach((r, i) => {
    for (const k of Object.keys(r)) if (!RULE_KEYS.has(k)) fail(`seed[${i}]: unknown key ${k}`);
    if (!parts.includes(r.part)) fail(`seed[${i}]: no part ${r.part}`);
    if (!["near", "ramps", "joints", "y"].some((k) => k in r))
      fail(`seed[${i}]: no test (near, ramps, joints or y)`);
  });
  if (!Array.isArray(file.vertices) || file.vertices.some((r) => typeof r !== "string"))
    fail("vertices is a list of strings (run node shared/models/parts.mjs --seed)");
  const v = file.vertices.join("");
  const bad = v.search(new RegExp(`[^0-${parts.length}]`));
  if (bad >= 0) fail(`vertex ${bad} is ${JSON.stringify(v[bad])}, not 0..${parts.length}`);
  if (count !== undefined && v.length !== count)
    fail(`${v.length} vertices, but the full mesh has ${count}: re-seed or repaint it`);
  return v;
}

/** The parts of a full mesh from `seed` rules: a Uint8Array, 0 no part, k the k-th part. */
export function seedParts(mesh, file, palette, exclude = []) {
  const isl = islands(mesh);
  const islandNear = (p) => {
    let best = 0;
    let bd = Infinity;
    for (let i = 0; i < mesh.count; i++) {
      const q = mesh.position(i);
      const d = (q[0] - p[0]) ** 2 + (q[1] - p[1]) ** 2 + (q[2] - p[2]) ** 2;
      if (d < bd) [bd, best] = [d, i];
    }
    return isl[best];
  };
  const rules = (file.seed ?? []).map((r) => ({
    ...r,
    index: file.parts.indexOf(r.part) + 1,
    island: r.near ? islandNear(r.near) : undefined,
    joints: r.joints?.map((j) => {
      const k = mesh.jointNames.indexOf(j);
      if (k < 0) throw new Error(`seed: no joint ${j}`);
      return k;
    }),
  }));
  const out = new Uint8Array(mesh.count);
  for (let i = 0; i < mesh.count; i++) {
    const ramp = nearestRamp(palette, mesh.colour(i), exclude);
    const y = mesh.position(i)[1];
    const js = mesh.joints(i);
    const ws = mesh.weights(i);
    for (const r of rules) {
      if (r.island !== undefined && isl[i] !== r.island) continue;
      if (r.ramps && !r.ramps.includes(ramp)) continue;
      if (r.y && (y < r.y[0] || y > r.y[1])) continue;
      if (r.joints) {
        const w = js.reduce((a, j, k) => a + (r.joints.includes(j) ? ws[k] : 0), 0);
        if (w < (r.weight ?? 0.5)) continue;
      }
      out[i] = r.index;
      break;
    }
  }
  return out;
}

/** The part map's `vertices` rows for a Uint8Array of parts. */
export function rows(parts) {
  const s = Array.from(parts, (p) => String(p)).join("");
  const out = [];
  for (let i = 0; i < s.length; i += ROW) out.push(s.slice(i, i + ROW));
  return out;
}

/**
 * A detail level's parts from the full mesh's (`baseParts`, a vertex each): each of `mesh`'s
 * triangles votes with its three corners and its centre, each taking the part of the full mesh's
 * nearest vertex of the triangle's colour (within two bytes; of any colour if none is within 5 cm),
 * the most votes winning (a tie: the centre's); each vertex takes the part of its first triangle.
 */
export function levelParts(base, baseParts, mesh) {
  const bp = Array.from({ length: base.count }, (_, i) => base.position(i));
  const bc = Array.from({ length: base.count }, (_, i) => base.colour(i));
  const nearest = (p, colour) => {
    let best = -1;
    let bd = Infinity;
    let any = -1;
    let ad = Infinity;
    for (let i = 0; i < base.count; i++) {
      const q = bp[i];
      const d = (q[0] - p[0]) ** 2 + (q[1] - p[1]) ** 2 + (q[2] - p[2]) ** 2;
      if (d < ad) [ad, any] = [d, i];
      if (d < bd && bc[i].every((v, k) => Math.abs(v - colour[k]) < 2 / 255)) [bd, best] = [d, i];
    }
    return baseParts[best >= 0 && bd < 0.05 ** 2 ? best : any];
  };
  const out = new Uint8Array(mesh.count);
  const done = new Uint8Array(mesh.count);
  for (let t = 0; t < mesh.indices.length; t += 3) {
    const vs = [0, 1, 2].map((k) => mesh.indices[t + k]);
    const ps = vs.map((v) => mesh.position(v));
    const colour = mesh.colour(vs[0]);
    const centre = [0, 1, 2].map((a) => (ps[0][a] + ps[1][a] + ps[2][a]) / 3);
    const votes = [nearest(centre, colour), ...ps.map((p) => nearest(p, colour))];
    const count = (x) => votes.filter((v) => v === x).length;
    const pick = votes.reduce((a, v) => (count(v) > count(a) ? v : a), votes[0]);
    for (const v of vs)
      if (!done[v]) {
        out[v] = pick;
        done[v] = 1;
      }
  }
  return out;
}

/** Every detail level's parts (`detail_0`, `detail_1`, …) of a GLB from its full mesh's. */
export function allLevelParts(bytes, baseParts) {
  const g = glbJson(bytes);
  const names = (g.meshes ?? []).map((m) => m.name).filter((n) => /^detail_\d+$/.test(n));
  names.sort((a, b) => Number(a.slice(7)) - Number(b.slice(7)));
  if (!names.length) return [Uint8Array.from(baseParts)];
  const base = readMesh(bytes, names[0]);
  return names.map((n, k) =>
    k === 0 ? Uint8Array.from(baseParts) : levelParts(base, baseParts, readMesh(bytes, n)),
  );
}

// ---- The CLI ----

/** A part's colour in the renders (sRGB bytes): 0 grey, then distinct hues. */
export const PART_COLOURS = [
  [90, 90, 96],
  [240, 190, 150],
  [120, 70, 30],
  [240, 200, 40],
  [60, 170, 70],
  [70, 110, 220],
  [210, 60, 160],
  [240, 130, 30],
  [40, 190, 190],
  [150, 80, 210],
  [230, 230, 60],
];

/** A PNG of `w`×`h` RGB bytes. */
export function png(w, h, rgb) {
  const crcTable = Array.from({ length: 256 }, (_, n) => {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    return c >>> 0;
  });
  const crc = (buf) => {
    let c = 0xffffffff;
    for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
    return (c ^ 0xffffffff) >>> 0;
  };
  const chunk = (type, data) => {
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length);
    const td = Buffer.concat([Buffer.from(type), data]);
    const c = Buffer.alloc(4);
    c.writeUInt32BE(crc(td));
    return Buffer.concat([len, td, c]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr[8] = 8;
  ihdr[9] = 2;
  const raw = Buffer.alloc((w * 3 + 1) * h);
  for (let y = 0; y < h; y++) rgb.copy(raw, y * (w * 3 + 1) + 1, y * w * 3, (y + 1) * w * 3);
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw)),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

/** The mesh drawn from `view` (front: looking at +z's face, side: from +x, back), a colour a part. */
export function renderParts(mesh, parts, view, size = 512) {
  const lo = [Infinity, Infinity, Infinity];
  const hi = [-Infinity, -Infinity, -Infinity];
  for (let i = 0; i < mesh.count; i++)
    mesh.position(i).forEach((v, k) => {
      lo[k] = Math.min(lo[k], v);
      hi[k] = Math.max(hi[k], v);
    });
  const span = Math.max(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]) * 1.05;
  const mid = lo.map((v, k) => (v + hi[k]) / 2);
  const proj = (p) => {
    const [x, y, z] = p.map((v, k) => v - mid[k]);
    const [u, d] = view === "front" ? [x, z] : view === "back" ? [-x, -z] : [-z, x];
    return [(u / span + 0.5) * size, (0.5 - y / span) * size, d];
  };
  const rgb = Buffer.alloc(size * size * 3, 32);
  const depth = new Float32Array(size * size).fill(-Infinity);
  const light = [0.4, 0.6, 0.7];
  for (let t = 0; t < mesh.indices.length; t += 3) {
    const vs = [0, 1, 2].map((k) => mesh.indices[t + k]);
    const ps = vs.map((v) => mesh.position(v));
    const e1 = ps[1].map((v, k) => v - ps[0][k]);
    const e2 = ps[2].map((v, k) => v - ps[0][k]);
    const n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
    const nl = Math.hypot(...n) || 1;
    const shade = 0.55 + 0.45 * Math.abs((n[0] * light[0] + n[1] * light[1] + n[2] * light[2]) / nl);
    const col = PART_COLOURS[parts[vs[0]] % PART_COLOURS.length].map((c) => Math.round(c * shade));
    const q = ps.map(proj);
    const x0 = Math.max(0, Math.floor(Math.min(...q.map((p) => p[0]))));
    const x1 = Math.min(size - 1, Math.ceil(Math.max(...q.map((p) => p[0]))));
    const y0 = Math.max(0, Math.floor(Math.min(...q.map((p) => p[1]))));
    const y1 = Math.min(size - 1, Math.ceil(Math.max(...q.map((p) => p[1]))));
    const area = (q[1][0] - q[0][0]) * (q[2][1] - q[0][1]) - (q[2][0] - q[0][0]) * (q[1][1] - q[0][1]);
    if (Math.abs(area) < 1e-9) continue;
    for (let y = y0; y <= y1; y++)
      for (let x = x0; x <= x1; x++) {
        const px = x + 0.5;
        const py = y + 0.5;
        const w0 = ((q[1][0] - px) * (q[2][1] - py) - (q[2][0] - px) * (q[1][1] - py)) / area;
        const w1 = ((q[2][0] - px) * (q[0][1] - py) - (q[0][0] - px) * (q[2][1] - py)) / area;
        const w2 = 1 - w0 - w1;
        if (w0 < 0 || w1 < 0 || w2 < 0) continue;
        const d = w0 * q[0][2] + w1 * q[1][2] + w2 * q[2][2];
        const at = y * size + x;
        if (d <= depth[at]) continue;
        depth[at] = d;
        rgb[at * 3] = col[0];
        rgb[at * 3 + 1] = col[1];
        rgb[at * 3 + 2] = col[2];
      }
  }
  return png(size, size, rgb);
}

/** A model's GLB bytes and part map file, by name (models.json `parts`). */
export function loadModel(name, dir = here) {
  const data = JSON.parse(readFileSync(join(dir, "models.json"), "utf8"));
  const m = data.models[name];
  if (!m) throw new Error(`no model ${name}`);
  if (!m.parts) throw new Error(`model ${name} has no parts file (models.json \`parts\`)`);
  return {
    data,
    model: m,
    bytes: readFileSync(join(dir, m.glb)),
    path: join(dir, m.parts),
    file: JSON.parse(readFileSync(join(dir, m.parts), "utf8")),
  };
}

function main(args) {
  const opt = (k, d) => {
    const i = args.indexOf(k);
    return i < 0 ? d : args[i + 1];
  };
  const palette = JSON.parse(readFileSync(join(root, "shared/textures/palette.json"), "utf8"));
  const name = opt("--seed") ?? opt("--stats") ?? opt("--render");
  if (!name) {
    console.error(
      "usage: node shared/models/parts.mjs --seed <model> | --stats <model> | --render <model> [--out <dir>] [--level k]",
    );
    process.exit(2);
  }
  const { data, bytes, path, file } = loadModel(name);
  const exclude = data.palette?.exclude ?? [];
  const base = readMesh(bytes, "detail_0");
  if (args.includes("--seed")) {
    const parts = seedParts(base, file, palette, exclude);
    writeFileSync(path, `${JSON.stringify({ ...file, vertices: rows(parts) }, null, 2)}\n`);
    console.log(`wrote ${path}`);
    return;
  }
  const baseParts = Uint8Array.from(validateParts(file, path, base.count), (c) => Number(c));
  const levels = allLevelParts(bytes, baseParts);
  if (args.includes("--stats")) {
    const names = ["none", ...file.parts];
    levels.forEach((ps, k) => {
      const n = new Array(names.length).fill(0);
      for (const p of ps) n[p]++;
      console.log(`detail_${k}: ${names.map((s, i) => `${s} ${n[i]}`).join(", ")}`);
    });
    return;
  }
  const level = Number(opt("--level", 0));
  const mesh = readMesh(bytes, `detail_${level}`);
  const out = opt("--out", join(tmpdir(), `dwell-parts-${name}`));
  mkdirSync(out, { recursive: true });
  for (const view of ["front", "side", "back"]) {
    const file = join(out, `${name}-${view}-detail_${level}.png`);
    writeFileSync(file, renderParts(mesh, levels[level], view));
    console.log(file);
  }
  console.log(
    `colours: ${["none", ...file.parts].map((p, i) => `${p} rgb(${PART_COLOURS[i].join(",")})`).join(", ")}`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href)
  main(process.argv.slice(2));
