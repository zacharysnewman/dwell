// Tests for the tree species' validation (shared/trees/gen.mjs).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { loadSpecies, validateSpecies, registryBlocks } from "./gen.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const blocks = registryBlocks();
const fixture = () => JSON.parse(readFileSync(join(here, "fixtures/shrub.json"), "utf8"));

test("the species and the fixture validate", () => {
  const names = loadSpecies(here, blocks).map((s) => s.name);
  assert.deepEqual(names, ["dwell:oak", "dwell:old_oak", "dwell:old_pine", "dwell:palm", "dwell:pine", "dwell:tall_broadleaf"]);
  assert.equal(loadSpecies(join(here, "fixtures"), blocks)[0].name, "test:shrub");
});

test("a bad entry fails naming the species and the field", () => {
  const bad = (edit, msg) => {
    const s = fixture();
    edit(s);
    assert.throws(() => validateSpecies(s, blocks, "x.json"), msg);
  };
  bad((s) => (s.blocks.leaf = "dwell:nope"), /test:shrub: blocks.leaf: dwell:nope is not a block/);
  bad((s) => (s.crown.shape = "blob"), /crown.shape must be one of/);
  bad((s) => (s.trunk.height.mature = [3, 2]), /trunk.height.mature: min above max/);
  bad((s) => (s.growth.wild = [1]), /growth.wild must weigh each stage/);
  bad((s) => (s.trunk.heightStep = 0.75), /whole ½ m cells/);
  bad((s) => (s.extra = 1), /unknown extra/);
  bad((s) => (s.branches = { kind: "whorl", perWhorl: 4, reach: 0.5, emergeAt: 0.5 }), /lumpy crown takes heavy/);
  bad((s) => (s.sapling.item = "dwell:nope"), /sapling.item: dwell:nope is not a block/);
  bad((s) => (s.sapling.plantOn = []), /sapling.plantOn must list/);
  bad((s) => (s.sapling.plantOn = ["dwell:nope"]), /sapling.plantOn: dwell:nope is not a block/);
  bad((s) => (s.sapling.dropChance = 2), /sapling.dropChance must be a number in \[0, 1\]/);
  bad((s) => delete s.sapling.dropChance, /sapling.dropChance is missing/);
});

test("every species names its own sapling, a block of the registry (ADR 0079 S4)", () => {
  // The old-growth species (WORK_QUEUE item 32) and the rainforest's tall broadleaf are wild only: no sapling of their own yet.
  const species = loadSpecies(here, blocks).filter((s) => s.sapling);
  const items = species.map((s) => s.sapling.item);
  assert.deepEqual(items, ["dwell:oak_sapling", "dwell:palm_sapling", "dwell:pine_sapling"]);
  for (const s of species) for (const g of s.sapling.plantOn) assert.ok(blocks.has(g), g);
  const wild = loadSpecies(here, blocks).filter((s) => !s.sapling).map((s) => s.name);
  assert.deepEqual(wild, ["dwell:old_oak", "dwell:old_pine", "dwell:tall_broadleaf"]);
  // A species without a sapling is still valid (it is not planted).
  const s = fixture();
  delete s.sapling;
  validateSpecies(s, blocks, "x.json");
});

test("the old-growth species stand taller than the species they grow from (WORK_QUEUE item 32)", () => {
  const by = Object.fromEntries(loadSpecies(here, blocks).map((s) => [s.name, s]));
  for (const [old, young] of [["dwell:old_pine", "dwell:pine"], ["dwell:old_oak", "dwell:oak"]]) {
    const [lo, hi] = by[old].trunk.height.mature;
    assert.ok(lo > by[young].trunk.height.mature[1], `${old} mature from ${lo} m, above ${young}'s tallest`);
    assert.ok(hi <= 26, `${old} at most 26 m (a leaf's tree is looked for 32 m up, growth.rs LEAF_RISE)`);
    // An old stand: most generated trees at the last stage.
    const w = by[old].growth.wild;
    assert.ok(w[w.length - 1] * 2 > w.reduce((a, b) => a + b, 0), `${old}: mostly mature`);
  }
});
