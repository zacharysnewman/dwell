// node --test shared/look/lights.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { lightsRust, validateLights } from './lights.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'lights.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in data validates and generates the checked-in Rust', () => {
  assert.equal(lightsRust(validateLights(data())), checkedIn('crates/dwell-look/src/lights.gen.rs'));
});

test('values changed by data alone are in the output', () => {
  const d = data();
  d.maxLights = 4;
  d.cullM = 32;
  d.curve = 1;
  d.wrap = 0.25;
  d.glowPeak = 0.8;
  d.shadows.tiers[1].lights = 3;
  d.shadows.placedM = 12;
  const rs = lightsRust(validateLights(d));
  assert.match(rs, /max_lights: 4,/);
  assert.match(rs, /cull_m: 32\.0,/);
  assert.match(rs, /curve: 1\.0,/);
  assert.match(rs, /wrap: 0\.25,/);
  assert.match(rs, /glow_peak: 0\.8,/);
  assert.match(rs, /TorchShadowTier \{ lights: 3, resolution: 384 \}/);
  assert.match(rs, /placed_m: 12\.0,/);
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.maxLights = 17), /maxLights must be a number in \[0, 16\]/],
    [(d) => (d.maxLights = 2.5), /maxLights must be a whole number/],
    [(d) => (d.cullM = 0), /cullM must be a number in \[1, 512\]/],
    [(d) => delete d.curve, /missing entry curve/],
    [(d) => (d.wrap = 2), /wrap must be a number in \[0, 1\]/],
    [(d) => (d.extra = 1), /unknown entry extra/],
    [(d) => (d.shadows.tiers[0].resolution = 300), /resolution must be one of/],
    [(d) => (d.shadows.tiers[1].lights = 9), /shadows\.tiers\[1\]\.lights must be a number in \[1, 4\]/],
    [(d) => delete d.shadows.biasM, /missing entry shadows\.biasM/],
    [(d) => (d.shadows.faceMargin = 2), /faceMargin must be a number in \[1, 1\.5\]/],
  ];
  for (const [edit, re] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateLights(d), re);
  }
});
