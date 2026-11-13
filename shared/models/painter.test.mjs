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
  const ctx = { TextDecoder, Uint8Array, DataView, Float64Array, Int32Array, Map, Math, JSON, Array, Object, Number, String, RegExp, Error };
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
    { parts: ["a", "b", "c", "d", "e", "f", "g"], vertices: ["1"] },
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
