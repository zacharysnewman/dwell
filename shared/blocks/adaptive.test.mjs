// Tests of the adaptive rule table's data and generator (`node --test shared/blocks/`; CI runs it
// with the generators). The Rust side reads the generated table and `adaptive_vectors.txt`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  HIGHER,
  LOWER,
  NEIGHBOURHOODS,
  OFFSETS,
  SAME,
  aroundOf,
  buildTable,
  holesIn,
  indexOf,
  parseAround,
  ruleShape,
} from "./adaptive.mjs";
import { FACING_TURNS, pointsOf } from "./shapes.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const data = JSON.parse(readFileSync(join(here, "adaptive.json"), "utf8"));
const table = buildTable(data);
const exported = JSON.parse(readFileSync(join(here, "playground-export.json"), "utf8"));

const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const turnsOfWay = (way) => ({ ...FACING_TURNS, "east-west": 0, "north-south": 1, "": 0 })[way];

/** The neighbourhood index of column (x, z) of the playground's grid. */
function playgroundIndex(x, z) {
  const height = (xx, zz) => parseInt(exported.columns[zz]?.[xx] ?? "0", 36);
  const t = height(x, z);
  return indexOf(
    OFFSETS.map(([dx, dz]) => {
      const h = height(x + dx, z + dz);
      return h > t ? HIGHER : h < t ? LOWER : SAME;
    }),
  );
}

test("the table reproduces every block of the owner's Shape Playground export", () => {
  assert.equal(exported.blocks.length, 151);
  const wrong = [];
  for (const [x, z, h, shape, way] of exported.blocks) {
    assert.equal(parseInt(exported.columns[z][x], 36), h, `(${x},${z}) height`);
    const e = table[playgroundIndex(x, z)];
    if (!same(pointsOf(e.shape, e.turns), pointsOf(shape, turnsOfWay(way))))
      wrong.push(`(${x},${z}) ${shape} ${way}: the table says ${e.shape} turned ${e.turns}`);
  }
  assert.deepEqual(wrong, []);
});

test("the situations the owner approved (Q–AB) produce the shapes they are named for", () => {
  const centre = { "25,2": "half_saddle", "29,2": "uneven_saddle", "25,6": "tilt", "29,6": "tall_half_and_half", "25,10": "diagonal_slope", "29,10": "saddle", "25,14": "tall_half_and_half", "29,14": "tilt", "25,18": "dimple", "29,18": "high_half_saddle", "25,22": "half_ridge_tee", "29,22": "half_ridge_cross" };
  for (const [at, name] of Object.entries(centre)) {
    const [x, z] = at.split(",").map(Number);
    assert.ok(table[playgroundIndex(x, z)].shape.startsWith(name), `${at}: ${table[playgroundIndex(x, z)].shape}, expected ${name}`);
  }
});

test("every entry follows the five rules when nothing is overridden", () => {
  assert.equal(table.length, NEIGHBOURHOODS);
  for (let i = 0; i < NEIGHBOURHOODS; ++i) {
    const r = ruleShape(i);
    assert.equal(table[i].shape, r.shape);
    assert.equal(table[i].turns, r.turns);
  }
  // A few by hand: a flat field rests at half height; a lone block is a half pyramid; a block
  // against one higher neighbour and lower on the other three sides is a ramp up to it.
  assert.equal(table[parseAround("===/=.=/===").index].shape, "slab");
  assert.equal(table[parseAround("---/-.-/---").index].shape, "half_pyramid");
  const ramp = table[parseAround("---/-.+/---").index];
  assert.deepEqual([ramp.shape, ramp.turns], ["wedge", 2]); // descends west, up to the east
});

test("an override is data: it replaces one neighbourhood, and only that one", () => {
  // A lone block as a slab instead of a half pyramid (it has no equal-height neighbour to meet).
  const around = "---/-.-/---";
  const t = buildTable({ ...data, overrides: [{ around, shape: "slab" }] });
  const i = parseAround(around).index;
  assert.equal(table[i].shape, "half_pyramid");
  assert.equal(t[i].shape, "slab");
  assert.equal(t[i].override, true);
  assert.equal(t.filter((e) => e.override).length, 1);
  assert.equal(t[i - 1].shape, table[i - 1].shape);
});

test("bad data fails with a message naming the entry", () => {
  const bad = (overrides, pattern) =>
    assert.throws(() => buildTable({ ...data, overrides }), pattern);
  bad([{ around: "===/=.=/==", shape: "slab" }], /overrides\[0\].*expected three rows/);
  bad([{ around: "===/=.=/===", shape: "spike" }], /overrides\[0\].*unknown shape "spike"/);
  bad([{ around: "+==/=.=/===", shape: "wedge", facing: "up" }], /wedge facing "up"/);
  bad([{ around: "===/=.=/===", shape: "slab", facing: "east" }], /slab takes no facing/);
  bad(
    [
      { around: "===/=.=/===", shape: "slab" },
      { around: "===/=.=/===", shape: "full" },
    ],
    /overrides\[1\].*repeats overrides\[0\]/,
  );
  assert.throws(() => buildTable({ ...data, rules: "other" }), /the only rule set is "five-rules"/);
});

test("the adjacency test finds a step between equal-height neighbours", () => {
  assert.deepEqual(holesIn(table), []);
  // A full cube in the middle of a flat field stands half a block above its neighbours' corners.
  assert.throws(
    () => buildTable({ ...data, overrides: [{ around: "===/=.=/===", shape: "full" }] }),
    /step at a shared corner/,
  );
});

test("neighbourhood strings round-trip", () => {
  for (const i of [0, 1, 2, 3, 100, 6560]) assert.equal(parseAround(aroundOf(i)).index, i);
});
