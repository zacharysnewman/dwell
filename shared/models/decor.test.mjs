// node --test shared/models/decor.test.mjs: a model's decor (shared/models/decor.mjs) over a fixture hat
// made in code: a crown (a ring of quads round the y axis) on a brim (a flat ring further out).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { addPieces, addedVertices, bandSector, decorBase, decorParts, decorPlan, facets, rayHit, validateDecor } from "./decor.mjs";
import { resolveColor } from "./gen.mjs";
import { allLevelParts, readMesh } from "./parts.mjs";

const hexRgb = (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
const toLinear = (c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);

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
    assert.throws(() => validateDecor([d], palette, "decor", opts.parts ?? ["hat"], opts.repaint, opts.joints), re);
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
  bad({ ...band, order: true }, /no segments/);
  bad({ ...band, span: -1 }, /span/);
  const piece = { kind: "piece", joint: "head", at: [0, 1.6, 0], facing: "+z", quads: [{ colour: "hair_black", at: [0, 0], size: [0.01, 0.01] }] };
  bad({ ...piece, joint: "tail" }, /no joint tail/, { joints: ["head"] });
  bad({ ...piece, facing: "up" }, /facing/);
  bad({ ...piece, quads: [{ colour: "hair_black", at: [0, 0], size: [0, 0.01] }] }, /size/);
  assert.doesNotThrow(() => validateDecor([band], palette, "decor", ["hat"]));
  assert.doesNotThrow(() => validateDecor([piece], palette, "decor", ["hat"], [], ["head"]));
});

// The Peasant as checked in (models.json `decor`, applied by `node shared/models/import.mjs --coat`).
const models = JSON.parse(readFileSync(join(here, "models.json"), "utf8"));
const peasant = models.models.peasant;
const glb = readFileSync(join(here, peasant.glb));
const partsFile = JSON.parse(readFileSync(join(here, peasant.parts), "utf8"));
const levels = models.detail.levels.map((_, k) => readMesh(glb, `detail_${k}`));
const levelParts = allLevelParts(glb, Uint8Array.from(partsFile.vertices.join(""), Number));
const entry = (kind, part) => peasant.decor.find((d) => d.kind === kind && d.part === part);
const centreOf = (m, vs) => [0, 1, 2].map((k) => vs.reduce((a, v) => a + m.position(v)[k], 0) / vs.length);
const near = (a, b) => a.every((x, k) => Math.abs(x - b[k]) < 0.004);

test("the Peasant has its eyes at every detail level: dark quads on the head joint, both sides of the face", () => {
  const eyes = entry("piece");
  const dark = hexRgb(resolveColor(palette, eyes.quads[0].colour)).map(toLinear);
  const head = levels[0].jointNames.indexOf(eyes.joint);
  levels.forEach((m, k) => {
    const added = addedVertices(glb, `detail_${k}`);
    assert.equal(added, 4 * eyes.quads.length * 2, `detail_${k}: a quad a side per entry`);
    const first = m.count - added;
    const sides = new Set();
    for (let v = first; v < m.count; v++) {
      assert.equal(m.joints(v)[0], head, `detail_${k}: on the head joint`);
      assert.equal(m.weights(v)[0], 1);
      if (near(m.colour(v), dark)) sides.add(Math.sign(m.position(v)[0]));
    }
    assert.deepEqual([...sides].sort(), [-1, 1], `detail_${k}: a dark eye each side`);
    // Drawn: the quads are in the index list.
    assert.ok(m.indices.some((i) => i >= first), `detail_${k}: the eyes' triangles are drawn`);
    // In front of the face: no triangle of the head is nearer the viewer at the eye's centre.
    const eye = Array.from({ length: 4 }, (_, i) => m.position(first + i));
    const c = [0, 1, 2].map((a) => eye.reduce((s, p) => s + p[a], 0) / 4);
    for (let t = 0; t < first * 0 + m.indices.length; t += 3) {
      if (m.indices[t] >= first) continue;
      const h = rayHit([c[0], c[1], c[2] + 1], [0, 0, -1], ...[0, 1, 2].map((j) => m.position(m.indices[t + j])));
      assert.ok(!(h > 0 && h < 1 - 1e-4), `detail_${k}: the face covers an eye`);
    }
  });
  // The part map lists the added vertices, no part.
  assert.equal(partsFile.vertices.join("").length, levels[0].count);
  assert.ok(partsFile.vertices.join("").endsWith("0".repeat(addedVertices(glb, "detail_0"))));
});

test("the Peasant's crown band is on the hat's crown only, never the brim, at every level", () => {
  const crown = entry("band", "hat");
  const base = decorBase(glb, partsFile);
  const hat = partsFile.parts.indexOf("hat") + 1;
  const brimR = crown.r[1];
  levels.forEach((m, k) => {
    const plan = decorPlan([crown], facets(m.indices, m.count), m.position, () => "hat", m.indices);
    // Every painted facet is the hat's, within the crown's radius.
    const partPlan = decorPlan([crown], facets(m.indices, m.count), m.position, (p) => {
      let best = 0;
      let bd = Infinity;
      base.positions.forEach((q, i) => {
        const d = (q[0] - p[0]) ** 2 + (q[1] - p[1]) ** 2 + (q[2] - p[2]) ** 2;
        if (d < bd) [bd, best] = [d, i];
      });
      return base.names[base.parts[best] - 1];
    });
    // The 20 % level keeps no crown facet short enough for the band (no spikes up the crown).
    if (k <= 2) assert.ok(partPlan.length >= 8, `detail_${k}: a ring (${partPlan.length} facets)`);
    assert.ok(plan.length >= partPlan.length);
    for (const [vs] of partPlan) {
      const c = centreOf(m, vs);
      assert.ok(Math.hypot(c[0], c[2]) <= brimR, `detail_${k}: a brim facet painted`);
    }
    // No brim facet (the hat's, beyond the crown) carries a band colour it didn't have.
    const red = hexRgb(resolveColor(palette, crown.colours[0])).map(toLinear);
    for (const vs of facets(m.indices, m.count)) {
      const c = centreOf(m, vs);
      if (Math.hypot(c[0], c[2]) <= brimR || levelParts[k][vs[0]] !== hat) continue;
      assert.ok(!near(m.colour(vs[0]), red), `detail_${k}: the brim at [${c.map((x) => x.toFixed(2))}] is banded`);
    }
  });
});

/** A band's plan as segments round the ring: runs of one colour, in angle order (one wrapped run joined). */
function segments(plan, m) {
  const round = plan
    .map(([vs, colour, part]) => {
      const c = centreOf(m, vs);
      return { deg: ((Math.atan2(c[0], c[2]) * 180) / Math.PI + 360) % 360, vs, colour, part };
    })
    .sort((x, y) => x.deg - y.deg);
  const runs = [];
  for (const f of round) {
    if (runs.length && runs.at(-1).colour === f.colour) runs.at(-1).facets.push(f);
    else runs.push({ colour: f.colour, part: f.part, facets: [f] });
  }
  if (runs.length > 1 && runs[0].colour === runs.at(-1).colour) runs[0].facets.push(...runs.pop().facets);
  return runs;
}

test("the Peasant's waist ring and crown band run their pattern's places strictly in sequence, no spikes, at every level", () => {
  // Owner, 2026-11-13: "losing the colorful waist of the shirt patterns. I want those back, but cleaner".
  for (const d of [entry("band", undefined), entry("band", "hat")]) {
    assert.ok(d.order && d.span && d.parts.length === d.colours.length);
    const cols = d.colours.map((c) => hexRgb(resolveColor(palette, c)).map(toLinear));
    const base = decorBase(glb, partsFile);
    const partOf = (p) => {
      let best = 0;
      let bd = Infinity;
      base.positions.forEach((q, i) => {
        const e = (q[0] - p[0]) ** 2 + (q[1] - p[1]) ** 2 + (q[2] - p[2]) ** 2;
        if (e < bd) [bd, best] = [e, i];
      });
      return base.names[base.parts[best] - 1];
    };
    levels.forEach((m, k) => {
      const plan = decorPlan([d], facets(m.indices, m.count), m.position, partOf, m.indices);
      // The default (60 %) and full levels show a ring; a ring shorter than one sequence is left out.
      if (k <= 1) assert.ok(plan.length >= 8, `detail_${k}: a ring (${plan.length} facets)`);
      if (!plan.length) return;
      const runs = segments(plan, m);
      assert.equal(runs.length % d.colours.length, 0, `detail_${k}: whole sequences round the ring`);
      runs.forEach((r, j) => {
        assert.equal(r.colour, d.colours[j % d.colours.length], `detail_${k}: segment ${j} in sequence`);
        assert.equal(r.part, d.parts[j % d.colours.length], `detail_${k}: segment ${j}'s part`);
      });
      for (const [vs, colour] of plan) {
        assert.ok(near(m.colour(vs[0]), cols[d.colours.indexOf(colour)]), `detail_${k}: painted ${colour}`);
        const ys = vs.map((v) => m.position(v)[1]);
        assert.ok(Math.max(...ys) - Math.min(...ys) <= d.span, `detail_${k}: a spike`);
      }
    });
  }
});

test("every painted ring facet is in its pattern part at every level (the shirt's and hat's recolour never reach it)", () => {
  const names = partsFile.parts;
  const parts = decorParts(glb, peasant.decor, names, levelParts, decorBase(glb, partsFile));
  const pattern = new Set(["pattern_1", "pattern_2", "pattern_3", "pattern_4"].map((p) => names.indexOf(p) + 1));
  const ringCols = entry("band", undefined).colours.map((c) => hexRgb(resolveColor(palette, c)).map(toLinear));
  levels.forEach((m, k) => {
    let n = 0;
    for (let v = 0; v < m.count; v++) {
      if (!pattern.has(parts[k][v])) continue;
      n++;
      const want = ringCols[parts[k][v] - names.indexOf("pattern_1") - 1];
      assert.ok(near(m.colour(v), want), `detail_${k}: vertex ${v} of ${names[parts[k][v] - 1]} is its place's colour`);
    }
    if (k <= 1) assert.ok(n > 0, `detail_${k}: pattern vertices`);
    // No pattern part on the map's own vertices: the bands put them on.
    assert.ok(![...levelParts[k]].some((p) => pattern.has(p)));
  });
});

test("the Peasant's crown above its band is one colour at every level (no stray triangles)", () => {
  // Owner, 2026-11-13: "the hat tops have some color artifacts, not just above the brim".
  const fill = entry("fill", "hat");
  const crown = entry("band", "hat");
  const want = hexRgb(resolveColor(palette, fill.colour)).map(toLinear);
  const hat = partsFile.parts.indexOf("hat") + 1;
  const parts = decorParts(glb, peasant.decor, partsFile.parts, levelParts, decorBase(glb, partsFile));
  levels.forEach((m, k) => {
    let n = 0;
    for (const vs of facets(m.indices, m.count)) {
      const c = centreOf(m, vs);
      if (c[1] <= crown.y[1] || Math.hypot(c[0], c[2]) > crown.r[1] || parts[k][vs[0]] !== hat) continue;
      n++;
      for (const v of vs) assert.ok(near(m.colour(v), want), `detail_${k}: a crown facet at [${c.map((x) => x.toFixed(2))}] off the hat's colour`);
    }
    assert.ok(n > 3, `detail_${k}: the crown (${n} facets)`);
  });
});

test("pieces are added once: a second run replaces them", () => {
  const again = addPieces(glb, peasant.decor, palette);
  assert.ok(Buffer.from(again).equals(Buffer.from(glb)));
  const none = addPieces(glb, [], palette);
  assert.equal(addedVertices(none, "detail_0"), 0);
  assert.equal(readMesh(none, "detail_0").count, levels[0].count - addedVertices(glb, "detail_0"));
});
