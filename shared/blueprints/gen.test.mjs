// node --test shared/blueprints/gen.test.mjs: the blueprints' data (docs/BLUEPRINTS.md).
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { generate, loadCreatureNames, loadStructureNames, rust, validateBlueprints } from './gen.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'blueprints.json'), 'utf8'));
const structures = loadStructureNames();
const creatures = loadCreatureNames();

test('the data validates and the generated table is current', () => {
  validateBlueprints(data(), structures, creatures);
  for (const [path, text] of generate()) assert.equal(readFileSync(path, 'utf8'), text, `${path} is stale: run node shared/blueprints/gen.mjs`);
});

test('the catalog holds the four test blueprints', () => {
  assert.deepEqual(data().catalog, ['dwell:bp_hut', 'dwell:bp_wall', 'dwell:bp_well', 'dwell:bp_watchtower']);
});

test('a fixture catalog entry, builder pace and town plan are added by data alone', () => {
  const d = JSON.parse(readFileSync(join(here, 'fixtures/fixture.json'), 'utf8'));
  const v = validateBlueprints(d, structures, creatures);
  assert.deepEqual(v.catalog, ['test:ladder_post']);
  assert.match(rust(v), /pace_s: 0\.5/);
  assert.match(rust(v), /catalog: &\["test:ladder_post"\]/);
  assert.match(rust(v), /TownPlan \{ name: "test:camp", weight: 2, blueprints: &\[PlanBlueprint \{ structure: "test:ladder_post", at: \[1, -1\], turn: 2 \}\] \}/);
});

test('bad entries fail naming them', () => {
  const bad = (edit, re) => {
    const d = data();
    edit(d);
    assert.throws(() => validateBlueprints(d, structures, creatures), re);
  };
  bad((d) => (d.catalog = ['dwell:nope']), /catalog\[0\]: unknown structure "dwell:nope"/);
  bad((d) => d.catalog.push(d.catalog[0]), /catalog\[4\]: dwell:bp_hut twice/);
  bad((d) => (d.builder.creature = 'dwell:ghost'), /builder.creature: unknown creature "dwell:ghost"/);
  bad((d) => (d.player.paceMs = 0), /player.paceMs must be a number in 16..5000/);
  bad((d) => (d.limits.perWorld = 1.5), /limits.perWorld must be an integer/);
  bad((d) => (d.builder.speed = 1), /builder: unknown field "speed"/);
  bad((d) => (d.extra = 1), /unknown field "extra"/);
  bad((d) => (d.towns.plans[0].blueprints[0].structure = 'dwell:nope'), /towns.plans\[0\].blueprints\[0\]: unknown structure/);
  bad((d) => (d.towns.plans[0].blueprints[1].turn = 4), /blueprints\[1\].turn must be 0..3/);
  bad((d) => (d.towns.plans[0].blueprints[0].at = [999, 0]), /within footprintM/);
  bad((d) => (d.towns.spacingM = 10), /towns.spacingM must be an integer in 64/);
  bad((d) => (d.towns.heightM = [5, 1]), /towns.heightM/);
  bad((d) => (d.towns.plans = []), /towns.plans must be a non-empty list/);
});
