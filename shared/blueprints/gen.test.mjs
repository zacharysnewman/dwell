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

test('a fixture catalog entry and builder pace are added by data alone', () => {
  const d = JSON.parse(readFileSync(join(here, 'fixtures/fixture.json'), 'utf8'));
  const v = validateBlueprints(d, structures, creatures);
  assert.deepEqual(v.catalog, ['test:ladder_post']);
  assert.match(rust(v), /pace_s: 0\.5/);
  assert.match(rust(v), /catalog: &\["test:ladder_post"\]/);
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
});
