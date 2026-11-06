// node --test shared/look/waterflow.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { liquidNames, validateWaterFlow, waterFlowRust } from './waterflow.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'waterflow.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in data validates and generates the checked-in Rust', () => {
  assert.equal(waterFlowRust(validateWaterFlow(data())), checkedIn('crates/dwell-look/src/waterflow.gen.rs'));
  assert.deepEqual(liquidNames(data()), ['dwell:water']);
});

test('a liquid added by data alone reaches the Rust table (a fixture oil, slow and dark)', () => {
  const d = data();
  d.liquids['test:oil'] = {
    ripples: { advect: 0.5, cycleS: 4 },
    foam: { fromMs: 1, toMs: 3, color: '#403010', strength: 0.2, lengthM: 6, widthM: 0.5 },
    particles: { count: 0, cpuCount: 0, fromMs: 1, sizeM: 0.1, color: '#201808', lifeS: 10, radiusM: 10 },
  };
  const rust = waterFlowRust(validateWaterFlow(d));
  assert.match(rust, /fluid: "test:oil",\n {8}advect: 0\.5,/);
  assert.match(rust, /LIQUID_FLOWS: \[LiquidFlowLook; 2\]/);
});

test('bad entries fail with their path', () => {
  const bad = (edit, re) => {
    const d = data();
    edit(d);
    assert.throws(() => validateWaterFlow(d), re);
  };
  bad((d) => (d.windowColumns = 100), /power of two/);
  bad((d) => (d.fadeToM = 90), /within half the window/);
  bad((d) => (d.liquids['dwell:water'].foam.toMs = 0.1), /foam\.toMs must be above/);
  bad((d) => (d.liquids['dwell:water'].ripples.speed = 1), /unknown entry liquids\.dwell:water\.ripples\.speed/);
  bad((d) => delete d.liquids['dwell:water'].particles.color, /missing entry liquids\.dwell:water\.particles\.color/);
  bad((d) => (d.liquids.Water = d.liquids['dwell:water']), /namespace:name/);
  bad((d) => (d.liquids['dwell:water'].particles.cpuCount = 999), /cpuCount must be at most count/);
});
