// node --test shared/models/painter.test.mjs: the Model Part Painter page's core
// (docs/prototypes/model-part-painter.html, its <script id="core">, WORK_QUEUE item 43) against
// shared/models/parts.mjs on the repo's files: it reads a GLB as readMesh does, and its export is the
// part map file parts.mjs writes, byte for byte.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import vm from "node:vm";
import { allLevelParts, islands, nearestRamp, readMesh, rows, validateParts, MAX_PARTS, ROW, PART_COLOURS } from "./parts.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..", "..");
const page = readFileSync(join(root, "docs/prototypes/model-part-painter.html"), "utf8");

/** The page's core, run as the page runs it (a classic script defining PainterCore). */
function core() {
  const m = page.match(/<script id="core">([\s\S]*?)<\/script>/);
  assert.ok(m, "the page has a <script id=\"core\">");
  const ctx = { TextDecoder, Uint8Array, DataView, Float64Array, Int32Array, Map, Set, Math, JSON, Array, Object, Number, String, RegExp, Error };
  vm.createContext(ctx);
  return vm.runInContext(`${m[1]}\nPainterCore;`, ctx);
}

const C = core();
const glbBytes = readFileSync(join(here, "glb/peasant.glb"));
const mapPath = join(here, "parts/peasant.json");
const mapText = readFileSync(mapPath, "utf8");
const palette = JSON.parse(readFileSync(join(root, "shared/textures/palette.json"), "utf8"));

test("the page's constants are parts.mjs's", () => {
  assert.equal(C.MAX_PARTS, MAX_PARTS);
  assert.equal(C.ROW, ROW);
  assert.deepEqual(JSON.parse(JSON.stringify(C.PART_COLOURS)), PART_COLOURS);
});

test("the page reads the Peasant's detail_0 as readMesh does", () => {
  const ref = readMesh(glbBytes, "detail_0");
  const g = C.parseGlb(new Uint8Array(glbBytes));
  const m = g.mesh("detail_0");
  assert.equal(m.count, ref.count);
  assert.deepEqual(Array.from(m.indices), Array.from(ref.indices));
  assert.deepEqual(Array.from(g.jointNames), ref.jointNames);
  assert.deepEqual(Array.from(g.levels), ["detail_0", "detail_1", "detail_2", "detail_3"]);
  for (const i of [0, 1, 17, 1000, ref.count - 1]) {
    assert.deepEqual(Array.from(m.position.slice(i * 3, i * 3 + 3)), ref.position(i));
    assert.deepEqual(Array.from(m.colour.slice(i * 3, i * 3 + 3)), ref.colour(i));
    assert.deepEqual(Array.from(m.joints.slice(i * 4, i * 4 + 4)), ref.joints(i));
    assert.deepEqual(Array.from(m.weights.slice(i * 4, i * 4 + 4)), ref.weights(i));
  }
  // The same islands (as partitions: the roots may differ).
  const a = C.islands(m);
  const b = islands(ref);
  const pair = new Map();
  for (let i = 0; i < ref.count; i++) {
    if (!pair.has(a[i])) pair.set(a[i], b[i]);
    assert.equal(pair.get(a[i]), b[i], `vertex ${i}'s island`);
  }
  assert.equal(new Set(a).size, new Set(b).size);
  for (const i of [0, 500, 2000, 4000])
    assert.equal(C.nearestRamp(palette, Array.from(m.colour.slice(i * 3, i * 3 + 3)), []), nearestRamp(palette, ref.colour(i)));
});

test("loading the Peasant's part map and exporting it reproduces the file byte for byte", () => {
  const count = readMesh(glbBytes, "detail_0").count;
  const { file, names, parts } = C.parseParts(mapText, count, "peasant.json");
  assert.equal(C.exportParts(file, names, parts), mapText);
  // And it is parts.mjs's writer: `{ ...file, vertices: rows(parts) }`.
  const ref = JSON.parse(mapText);
  const v = Uint8Array.from(validateParts(ref, "p", count), Number);
  assert.equal(C.exportParts(file, names, parts), `${JSON.stringify({ ...ref, vertices: rows(v) }, null, 2)}\n`);
});

test("an edited map exports in the repo's format and passes parts.mjs's check", () => {
  const count = readMesh(glbBytes, "detail_0").count;
  const { file, names, parts } = C.parseParts(mapText, count, "peasant.json");
  parts[0] = 2;
  parts[count - 1] = 0;
  const text = C.exportParts(file, names, parts);
  const back = JSON.parse(text);
  assert.equal(validateParts(back, "export", count)[0], "2");
  assert.ok(back.vertices.every((r, i) => r.length === (i < back.vertices.length - 1 ? ROW : r.length)));
  // A new map (no file) is `{ parts, vertices }`.
  const fresh = C.exportParts(null, ["skin", "hair"], Uint8Array.from([1, 2, 0]));
  assert.equal(fresh, `${JSON.stringify({ parts: ["skin", "hair"], vertices: rows([1, 2, 0]) }, null, 2)}\n`);
  // A deleted part's seed rules go with it (else validateParts refuses the file).
  const dropped = JSON.parse(C.exportParts(file, names.filter((n) => n !== "hat"), new Uint8Array(count)));
  assert.ok(dropped.seed.every((r) => r.part !== "hat"));
  validateParts(dropped, "dropped", count);
});

test("a part map of the wrong vertex count is refused with a message", () => {
  assert.throws(() => C.parseParts(mapText, 12, "peasant.json"), /\d+ vertices, but the full mesh has 12/);
  assert.throws(() => validateParts(JSON.parse(mapText), "peasant.json", 12), /\d+ vertices, but the full mesh has 12/);
  for (const bad of [
    { parts: [], vertices: [] },
    { parts: ["Skin"], vertices: ["1"] },
    { parts: ["a", "a"], vertices: ["1"] },
    { parts: ["a"], vertices: ["2"] },
    { parts: ["a"], vertices: ["1"], extra: 1 },
    { parts: ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"], vertices: ["1"] },
  ]) {
    const ref = (() => { try { validateParts(bad, "x"); return "ok"; } catch (e) { return e.message; } })();
    const got = (() => { try { C.validateParts(bad, "x"); return "ok"; } catch (e) { return e.message; } })();
    assert.notEqual(ref, "ok");
    assert.equal(got, ref);
  }
});

test("the page maps detail_1's parts as parts.mjs does", () => {
  const count = readMesh(glbBytes, "detail_0").count;
  const { parts } = C.parseParts(mapText, count);
  const g = C.parseGlb(new Uint8Array(glbBytes));
  const ref = allLevelParts(glbBytes, parts)[1];
  assert.deepEqual(Array.from(C.levelParts(g.mesh("detail_0"), parts, g.mesh("detail_1"))), Array.from(ref));
});

test("the variant recolour is dwell_models::recolour's luminance-ratio rule", () => {
  const remap = C.partRemap(["skin", "hair"], [{ part: "hair", from: "hair_brown", to: ["hair_brown", "hair_black"] }], [1], palette);
  assert.equal(remap[0], null);
  const hex = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16) / 255);
  const from = hex(palette.ramps.hair_brown.base);
  const to = hex(palette.ramps.hair_black.base);
  assert.deepEqual(Array.from(remap[1][0]), from);
  assert.deepEqual(Array.from(remap[1][1]), to);
  const luma = (c) => 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
  // The base itself becomes the new base; a darker shade keeps its ratio; other parts unchanged.
  assert.deepEqual(Array.from(C.recolour(from, 2, remap)), to.map((t) => Math.min(t, 1)));
  const dark = from.map((v) => v / 2);
  const out = C.recolour(dark, 2, remap);
  out.forEach((v, k) => assert.ok(Math.abs(v - Math.min(to[k] * (luma(dark) / luma(from)), 1)) < 1e-12));
  assert.deepEqual(C.recolour(dark, 1, remap), dark);
  assert.deepEqual(C.recolour(dark, 0, remap), dark);
  // A part the model lacks or a ramp the palette lacks is left out.
  const none = C.partRemap(["skin"], [{ part: "hat", from: "hair_brown", to: ["x"] }, { part: "skin", from: "nope", to: ["skin_tan"] }], [0, 0], palette);
  assert.ok(none.every((r) => r === null));
});

// ---- Face select (owner, 2026-11-14: "select/deselect faces on the mesh … copy those Face IDs so I can paste
// them here and talk about them") ----

test("face ids compress to ranges of three or more and parse back", () => {
  assert.equal(C.compressIds([1215, 1203, 1204, 1210, 1211, 1212, 1213, 1214, 1204]), "1203,1204,1210\u20131215");
  assert.equal(C.compressIds([]), "");
  assert.equal(C.compressIds([5]), "5");
  assert.equal(C.compressIds([0, 1, 2, 4, 6, 7, 8, 9]), "0\u20132,4,6\u20139");
  assert.deepEqual(Array.from(C.parseIds("1203,1204,1210\u20131215")), [1203, 1204, 1210, 1211, 1212, 1213, 1214, 1215]);
  // Typed by hand: hyphens, "..", spaces, out of order, repeats.
  assert.deepEqual(Array.from(C.parseIds(" 9, 3-5 7..8 4 ")), [3, 4, 5, 7, 8, 9]);
  assert.throws(() => C.parseIds("3,x"), /not a face id/);
  for (let n = 0; n < 50; n++) {
    const ids = [...new Set(Array.from({ length: 40 }, (_, i) => (i * 7919 + n * 31) % 97))].sort((a, b) => a - b);
    assert.deepEqual(Array.from(C.parseIds(C.compressIds(ids))), ids);
  }
});

test("the copied selection text has the model, level, ranges, count, parts and colours, and reads back", () => {
  const text = C.formatSelection({
    model: "peasant", level: "60%", ids: [1203, 1204, 1210, 1211, 1212, 1213, 1214, 1215],
    parts: ["shirt", "shirt", "shirt", "belt", "shirt", "shirt", "shirt", "belt"],
    colours: ["wool_green", "wool_green", "wool_green", "leather", "wool_green", "wool_green", "wool_green", "leather"],
  });
  assert.equal(text, "peasant @60% faces: 1203,1204,1210\u20131215 (8) \u2014 parts: shirt 6, belt 2 \u2014 colours: wool_green 6, leather 2");
  const back = C.parseSelection(text);
  assert.equal(back.model, "peasant");
  assert.equal(back.level, 60);
  assert.deepEqual(Array.from(back.ids), [1203, 1204, 1210, 1211, 1212, 1213, 1214, 1215]);
  // A bare list, a hand-typed line, an empty selection.
  assert.deepEqual(Array.from(C.parseSelection("4, 6-8").ids), [4, 6, 7, 8]);
  assert.equal(C.parseSelection("4, 6-8").level, null);
  const typed = C.parseSelection("peasant @ 35 % faces: 10-12");
  assert.equal(typed.level, 35);
  assert.deepEqual(Array.from(typed.ids), [10, 11, 12]);
  const none = C.formatSelection({ model: "peasant", level: "100%", ids: [] });
  assert.equal(none, "peasant @100% faces: none (0)");
  assert.deepEqual(Array.from(C.parseSelection(none).ids), []);
});

/** The Peasant's level `k` seen from the front (+z), orthographic: screen x = x, y = -y (pixels, y down). */
function frontScreen(m, scale = 500) {
  const sx = new Float64Array(m.count), sy = new Float64Array(m.count);
  for (let i = 0; i < m.count; i++) { sx[i] = m.position[i * 3] * scale; sy[i] = -m.position[i * 3 + 1] * scale; }
  return { sx, sy };
}

test("box select takes the faces whose centre is in the box, front-facing ones by their winding", () => {
  const g = C.parseGlb(new Uint8Array(glbBytes));
  const m = g.mesh("detail_1");
  const { sx, sy } = frontScreen(m);
  const all = C.facesInRect(m.indices, sx, sy, [-1e4, -1e4, 1e4, 1e4], false);
  assert.equal(all.length, m.indices.length / 3);
  // The rectangle's corners in any order.
  const rect = [-60, -500, 60, -400]; // x ±0.12 m, y 0.8–1.0 m: the waist
  const a = C.facesInRect(m.indices, sx, sy, rect, false);
  assert.deepEqual(Array.from(C.facesInRect(m.indices, sx, sy, [60, -400, -60, -500], false)), Array.from(a));
  for (const t of a) {
    const c = C.faceCentre(m, t);
    assert.ok(Math.abs(c[0] * 500) <= 60 && c[1] >= 0.8 - 1e-9 && c[1] <= 1.0 + 1e-9, `face ${t}'s centre is in the box`);
  }
  // Front-facing from +z: the faces whose geometric normal (glTF's counter-clockwise front) points to +z.
  const front = C.facesInRect(m.indices, sx, sy, rect, true);
  assert.ok(front.length > 0 && front.length < a.length, `${front.length} of ${a.length} face the camera`);
  for (const t of a) {
    const [p, q, r] = [0, 1, 2].map((k) => m.indices[t * 3 + k]);
    const P = (i) => [m.position[i * 3], m.position[i * 3 + 1], m.position[i * 3 + 2]];
    const [A, B, D] = [P(p), P(q), P(r)];
    const nz = (B[0] - A[0]) * (D[1] - A[1]) - (B[1] - A[1]) * (D[0] - A[0]);
    if (Math.abs(nz) < 1e-9) continue;
    assert.equal(front.includes(t), nz > 0, `face ${t}: front iff its normal's z > 0`);
  }
});

test("a face behind another is hidden; one in front is not", () => {
  const g = C.parseGlb(new Uint8Array(glbBytes));
  const m = g.mesh("detail_1");
  const eye = [0, 0.9, 5];
  const { sx, sy } = frontScreen(m);
  const waist = C.facesInRect(m.indices, sx, sy, [-60, -500, 60, -400], false);
  const front = waist.filter((t) => C.faceCentre(m, t)[2] > 0.05);
  const back = waist.filter((t) => C.faceCentre(m, t)[2] < -0.05);
  assert.ok(front.length && back.length);
  assert.ok(back.every((t) => C.hidden(m, eye, C.faceCentre(m, t), t, 1e-3)), "the back of the waist is hidden from the front");
  assert.ok(front.some((t) => !C.hidden(m, eye, C.faceCentre(m, t), t, 1e-3)), "some of the front is seen");
});

test("connected same colour stays on one colour and grows across shared edges", () => {
  const g = C.parseGlb(new Uint8Array(glbBytes));
  const m = g.mesh("detail_0");
  const col = (t) => Array.from(m.colour.slice(m.indices[t * 3] * 3, m.indices[t * 3] * 3 + 3));
  let grew = false;
  for (const start of [0, 100, 500, 1000, 1500, 2000]) {
    const set = C.connectedSameColour(m, start);
    assert.ok(set.includes(start));
    const c0 = col(start);
    for (const t of set) col(t).forEach((x, k) => assert.ok(Math.abs(x - c0[k]) < 2 / 255, `face ${t} is face ${start}'s colour`));
    if (set.length > 1) grew = true;
  }
  assert.ok(grew, "some region is more than one face");
});

test("a band from a selection covers its faces' heights and radii", () => {
  const g = C.parseGlb(new Uint8Array(glbBytes));
  const m = g.mesh("detail_1");
  const { sx, sy } = frontScreen(m);
  const ids = C.facesInRect(m.indices, sx, sy, [-60, -470, 60, -440], true);
  const band = C.bandFromFaces(m, ids, () => "shirt");
  assert.equal(band.kind, "band");
  assert.equal(band.part, "shirt");
  assert.equal(band.order, true);
  for (const t of ids) {
    const c = C.faceCentre(m, t);
    assert.ok(c[1] >= band.y[0] && c[1] <= band.y[1]);
    assert.ok(Math.hypot(c[0], c[2]) >= band.r[0] && Math.hypot(c[0], c[2]) <= band.r[1]);
  }
  assert.equal(C.bandFromFaces(m, ids, (t) => (t % 2 ? "shirt" : "pattern_1")).part, undefined);
  assert.throws(() => C.bandFromFaces(m, [], null), /select some faces/);
});
