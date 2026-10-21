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
  d.nearM = 0.5;
  d.charactersNearM = 1.2;
  const rs = lightsRust(validateLights(d));
  assert.match(rs, /max_lights: 4,/);
  assert.match(rs, /cull_m: 32\.0,/);
  assert.match(rs, /near_m: 0\.5,/);
  assert.match(rs, /characters_near_m: 1\.2,/);
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.maxLights = 17), /maxLights must be a number in \[0, 16\]/],
    [(d) => (d.maxLights = 2.5), /maxLights must be a whole number/],
    [(d) => (d.cullM = 0), /cullM must be a number in \[1, 512\]/],
    [(d) => delete d.nearM, /missing entry nearM/],
    [(d) => (d.extra = 1), /unknown entry extra/],
  ];
  for (const [edit, re] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateLights(d), re);
  }
});
