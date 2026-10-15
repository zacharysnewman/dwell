// node --test shared/look/clouds.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { cloudsRust, validateClouds } from './clouds.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'clouds.json'), 'utf8'));

test('the checked-in tunables validate and generate the checked-in Rust', () => {
  const rs = cloudsRust(validateClouds(data()));
  const checkedIn = readFileSync(join(here, '../../crates/dwell-look/src/clouds.gen.rs'), 'utf8');
  assert.equal(rs, checkedIn);
});

test('a tunable changed by data changes the output', () => {
  const d = data();
  d.shape.coverage = 0.8;
  d.light.shade = '#102030';
  const rs = cloudsRust(validateClouds(d));
  assert.match(rs, /coverage: 0\.8,/);
  assert.match(rs, /shade: \[0\.062745, 0\.12549, 0\.188235\],/);
  assert.notEqual(rs, cloudsRust(validateClouds(data())));
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.shape.coverage = 1.5), /shape\.coverage must be a number in \[0, 1\]/],
    [(d) => delete d.deck.altitudeM, /missing entry deck\.altitudeM/],
    [(d) => (d.wind.gusts = 1), /unknown entry wind\.gusts/],
    [(d) => (d.light.sunlit = 'white'), /light\.sunlit must be a colour/],
    [(d) => (d.shape.octaves = 2.5), /shape\.octaves must be a whole number/],
    [(d) => (d.shape.octaves = 9), /shape\.octaves must be a number in \[1, 8\]/],
    [(d) => (d.shape.lowOctaves = 6), /lowOctaves must not exceed/],
    [(d) => (d.shape.wrapM = 100000), /whole multiple of shape\.scaleM/],
    [(d) => (d.shape.wrapM = 2 * d.shape.scaleM), /at least 4 × shape\.scaleM/],
    [(d) => (d.deck.fadeEndM = 10), /fadeEndM must be greater/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateClouds(d), message);
  }
});
