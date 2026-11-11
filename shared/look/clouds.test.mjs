// node --test shared/look/clouds.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { cloudsRust, validateClouds } from './clouds.mjs';

const here = dirname(fileURLToPath(import.meta.url));
/** A fixture plume (a volcano's ash column), added by data alone. */
const ASH = {
  name: 'test:ash',
  xM: 500,
  zM: -300,
  radiusM: 200,
  spread: 1.5,
  baseM: 300,
  topM: 3500,
  density: 1,
  color: '#6e6a66',
};
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
    [(d) => (d.volume.topM = d.volume.baseM), /volume\.topM must be greater/],
    [(d) => (d.volume.detailCellM = 300), /whole multiple of volume\.detailCellM/],
    [(d) => (d.tiers.high.steps = 65), /tiers\.high\.steps must be a number in \[0, 64\]/],
    [(d) => delete d.tiers.low, /missing entry tiers\.low/],
    [(d) => (d.plumes = {}), /plumes must be a list/],
    [(d) => d.plumes.push({ ...ASH, topM: 100 }), /plumes\[0\]\.topM must be greater/],
    [(d) => d.plumes.push({ ...ASH, name: 'Bad Name' }), /plumes\[0\]\.name must be a name/],
    [(d) => d.plumes.push(ASH, ASH), /plumes\[1\]\.name test:ash is used twice/],
    [(d) => d.plumes.push({ ...ASH, heat: 1 }), /unknown entry plumes\[0\]\.heat/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateClouds(d), message);
  }
});

test('a plume and a tier added by data reach the Rust', () => {
  const d = data();
  d.plumes.push(ASH);
  d.tiers.high.steps = 32;
  const rs = cloudsRust(validateClouds(d));
  assert.match(rs, /name: "test:ash",/);
  assert.match(rs, /radius_m: 200\.0,/);
  assert.match(rs, /color: \[0\.431373, 0\.415686, 0\.4\],/);
  assert.match(rs, /steps: 32,/);
});
