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
  assert.deepEqual(names, ["dwell:oak", "dwell:palm", "dwell:pine"]);
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
  const species = loadSpecies(here, blocks);
  const items = species.map((s) => s.sapling.item);
  assert.deepEqual(items, ["dwell:oak_sapling", "dwell:palm_sapling", "dwell:pine_sapling"]);
  for (const s of species) for (const g of s.sapling.plantOn) assert.ok(blocks.has(g), g);
  // A species without a sapling is still valid (it is not planted).
  const s = fixture();
  delete s.sapling;
  validateSpecies(s, blocks, "x.json");
});
