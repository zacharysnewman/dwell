// node --test shared/models/decor.test.mjs: a model's decor (shared/models/decor.mjs) over a fixture hat
// made in code: a crown (a ring of quads round the y axis) on a brim (a flat ring further out).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { bandSector, decorPlan, facets, validateDecor } from "./decor.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const palette = JSON.parse(readFileSync(join(here, "../textures/palette.json"), "utf8"));

/** A hat: `n` crown quads (radius 0.15, y 1.55..1.65) and `n` brim quads (radius 0.2..0.3, y 1.55). */
function hat(n = 24) {
  const pos = [];
  const idx = [];
  const quad = (a, b, c, d) => {
    const k = pos.length;
    pos.push(a, b, c, d);
    idx.push(k, k + 1, k + 2, k, k + 2, k + 3);
  };
  const at = (r, y, i) => {
    const t = (i / n) * 2 * Math.PI;
    return [r * Math.sin(t), y, r * Math.cos(t)];
  };
  for (let i = 0; i < n; i++) quad(at(0.15, 1.55, i), at(0.15, 1.55, i + 1), at(0.15, 1.65, i + 1), at(0.15, 1.65, i));
  for (let i = 0; i < n; i++) quad(at(0.2, 1.55, i), at(0.3, 1.55, i), at(0.3, 1.55, i + 1), at(0.2, 1.55, i + 1));
  return { pos, idx, crown: n * 4 };
}

const band = { kind: "band", part: "hat", y: [1.5, 1.7], r: [0, 0.18], colours: ["bark", "brass"], segments: 12 };

test("a band paints only its ring (the crown, no brim facet) and alternates its colours round it", () => {
  const h = hat();
  const groups = facets(h.idx, h.pos.length);
  assert.equal(groups.length, 48, "one facet a quad");
  const plan = decorPlan([band], groups, (v) => h.pos[v], () => "hat");
  assert.equal(plan.length, 24, "every crown facet");
  for (const [vs] of plan) assert.ok(vs.every((v) => v < h.crown), "no brim vertex painted");
  // Round the crown in order: the colours alternate every 30° (2 quads of 15° each).
  const seq = plan
    .map(([vs, c]) => [Math.atan2(...[0, 2].map((k) => vs.reduce((a, v) => a + h.pos[v][k], 0))), c])
    .map(([a, c]) => [((a * 180) / Math.PI + 360) % 360, c])
    .sort((a, b) => a[0] - b[0])
    .map(([, c]) => c);
  assert.deepEqual(seq, Array.from({ length: 24 }, (_, i) => (Math.floor(i / 2) % 2 ? "brass" : "bark")));
});

test("a band with a part paints only that part's facets", () => {
  const h = hat();
  const plan = decorPlan([band], facets(h.idx, h.pos.length), (v) => h.pos[v], () => "hair");
  assert.equal(plan.length, 0);
});

test("a band's sectors turn with its phase", () => {
  assert.equal(bandSector(band, 0.01, 1), 0);
  assert.equal(bandSector(band, 1, 1), 1); // 45°: the second sector
  assert.equal(bandSector({ ...band, phase: 30 }, 1, 1), 0);
});

test("a mark paints the facets near its point and its mirror (the nearest if none is near)", () => {
  const h = hat();
  const groups = facets(h.idx, h.pos.length);
  const mark = { kind: "mark", colour: "hair_black", at: [0.04, 1.6, 0.14], radius: 0.001, mirror: true };
  const plan = decorPlan([mark], groups, (v) => h.pos[v], () => undefined);
  assert.equal(plan.length, 2, "one facet each side");
  const xs = plan.map(([vs]) => vs.reduce((a, v) => a + h.pos[v][0], 0));
  assert.ok(xs[0] > 0 && xs[1] < 0, `both sides: ${xs}`);
});

test("bad decor fails with a message", () => {
  const bad = (d, re, opts = {}) =>
    assert.throws(() => validateDecor([d], palette, "decor", opts.parts ?? ["hat"], opts.repaint), re);
  bad({ ...band, kind: "stripe" }, /kind is one of/);
  bad({ ...band, segments: 7 }, /whole multiple/);
  bad({ ...band, colours: ["bark"] }, /two or more/);
  bad({ ...band, colours: ["bark", "nope"] }, /colours\[1\]/);
  bad({ ...band, part: "cape" }, /no part cape/);
  bad({ ...band, y: [2, 1] }, /min < max/);
  bad({ ...band, wobble: 1 }, /unknown key wobble/);
  bad({ ...band, colours: ["hair_brown", "bark"] }, /repaint source/, {
    repaint: [{ from: "hair_brown", to: "wool_yellow", part: "hat" }],
  });
  bad({ kind: "mark", colour: "hair_black", at: [0, 1], radius: 0.01 }, /at is \[x, y, z\]/);
  assert.doesNotThrow(() => validateDecor([band], palette, "decor", ["hat"]));
});
