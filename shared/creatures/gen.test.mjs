// node --test shared/creatures/gen.test.mjs: the creatures' data (docs/adr/0073-creatures.md).
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { loadBlocks } from '../inventory/gen.mjs';
import { generate, loadModels, validateCreatures } from './gen.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'creatures.json'), 'utf8'));
const blocks = loadBlocks();
const models = loadModels();

test('the data validates and the generated tables are current', () => {
  validateCreatures(data(), blocks, models);
  for (const [path, text] of generate()) assert.equal(readFileSync(path, 'utf8'), text, `${path} is stale: run node shared/creatures/gen.mjs`);
});

test('a fixture creature is added by data alone', () => {
  const d = JSON.parse(readFileSync(join(here, 'fixtures/creatures.json'), 'utf8'));
  const v = validateCreatures(d, blocks, models);
  assert.equal(v.creatures[0].name, 'test:walker');
});

test('bad entries fail with a message naming them', () => {
  const bad = (edit, re) => {
    const d = data();
    edit(d);
    assert.throws(() => validateCreatures(d, blocks, models), re);
  };
  bad((d) => (d.creatures[0].model = 'unicorn'), /creatures\.dwell:deer: model "unicorn"/);
  bad((d) => (d.rules[0].on = ['dwell:nope']), /rules\[0\]\.on\[0\]: "dwell:nope" is not a block/);
  bad((d) => (d.rules[0].creature = 'dwell:nope'), /rules\[0\]: creature "dwell:nope"/);
  bad((d) => (d.rules[0].group = [4, 2]), /rules\[0\]: group/);
  bad((d) => (d.creatures[0].idleSeconds = [5, 1]), /idleSeconds/);
  bad((d) => (d.creatures[0].walkSpeed = 0), /walkSpeed/);
  bad((d) => (d.limits.max = 0), /limits\.max/);
  bad((d) => (d.creatures[0].wings = 2), /unknown field wings/);
});

// WORK_QUEUE item 35: a creature's body to a weapon (health, hit radius, drops) is data.
test('a creature’s health, hit radius and drops are checked and emitted', () => {
  const v = validateCreatures(data(), blocks, models);
  const doe = v.creatures.find((c) => c.name === 'dwell:doe');
  assert.ok(doe.health > 0 && doe.hitRadiusM > 0);
  assert.deepEqual(doe.drops, [{ item: 'dwell:raw_meat', min: 1, max: 2 }]);
  const bad = (edit, re) => {
    const d = data();
    edit(d.creatures.find((c) => c.name === 'dwell:doe'));
    assert.throws(() => validateCreatures(d, blocks, models), re);
  };
  bad((c) => (c.drops = [{ item: 'dwell:nothing' }]), /creatures\.dwell:doe\.drops\[0\]: no item "dwell:nothing"/);
  bad((c) => (c.health = -1), /health must be in \[0, 10000\]/);
  bad((c) => (c.hitRadiusM = 0), /hitRadiusM must be in \(0, 4\]/);
  bad((c) => delete c.health, /drops need health/);
});
