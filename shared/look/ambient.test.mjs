// node --test shared/look/ambient.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { ambientRust, validateAmbient } from './ambient.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'ambient.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in data validates and generates the checked-in Rust', () => {
  assert.equal(ambientRust(validateAmbient(data())), checkedIn('crates/dwell-look/src/ambient.gen.rs'));
});

test('values changed by data alone are in the output', () => {
  const d = data();
  d.sky.rays = 24;
  d.sky.lengthM = 2.5;
  d.ao.strength = 0;
  d.floor = 0.2;
  const rs = ambientRust(validateAmbient(d));
  assert.match(rs, /rays: 24,/);
  assert.match(rs, /length_m: 2\.5,/);
  assert.match(rs, /ao_strength: 0\.0,/);
  assert.match(rs, /floor: 0\.2,/);
  // No rays: the sky visibility is off (every vertex open).
  d.sky.rays = 0;
  assert.match(ambientRust(validateAmbient(d)), /rays: 0,/);
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.ao.strength = 1.5), /ao\.strength must be a number in \[0, 1\]/],
    [(d) => (d.sky.rays = 64), /sky\.rays must be a number in \[0, 63\]/],
    [(d) => (d.sky.rays = 2.5), /sky\.rays must be a whole number/],
    [(d) => (d.sky.lengthM = 0), /sky\.lengthM must be a number/],
    [(d) => (d.sky.probeM = -1), /sky\.probeM must be a number/],
    [(d) => delete d.floor, /missing entry \(root\)\.floor/],
    [(d) => (d.sky.colour = 1), /unknown entry sky\.colour/],
    [(d) => (d.extra = 1), /unknown entry \(root\)\.extra/],
  ];
  for (const [edit, re] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateAmbient(d), re);
  }
});
