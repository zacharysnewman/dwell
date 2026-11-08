// node --test shared/look/reflections.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { phoneTier, reflectionsRust, validateReflections } from './reflections.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'reflections.json'), 'utf8'));

test('the checked-in tunables validate and generate the checked-in Rust', () => {
  const rs = reflectionsRust(validateReflections(data()));
  const checkedIn = readFileSync(join(here, '../../crates/dwell-look/src/reflections.gen.rs'), 'utf8');
  assert.equal(rs, checkedIn);
});

test('a tunable changed by data changes the output', () => {
  const d = data();
  d.tiers.medium.ssrSteps = 7;
  d.tiers.medium.cloudShadow = true;
  d.ssr.edgeFade = 0.2;
  const rs = reflectionsRust(validateReflections(d));
  assert.match(rs, /ssr_steps: 7,/);
  assert.match(rs, /edge_fade: 0\.2,/);
  assert.match(rs, /cloud_shadow: true/);
  assert.notEqual(rs, reflectionsRust(validateReflections(data())));
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.phone.opaque = { ssrSteps: 2 }), /phone\.opaque: it marches nothing/],
    [(d) => (d.phone.ultra = {}), /unknown entry phone\.ultra/],
    [(d) => (d.phone.medium.speed = 1), /unknown entry phone\.medium\.speed/],
    [(d) => (d.phone.medium.ssrSteps = 100), /phone\.medium\.ssrSteps must be a number in \[0, 64\]/],
    [(d) => (d.ssr.minReflectance = 0.9), /ssr\.minReflectance must be a number in \[0, 0\.5\]/],
    [(d) => (d.ssr.edgeFade = 2), /ssr\.edgeFade must be a number in \[0\.001, 0\.5\]/],
    [(d) => delete d.ssr.thicknessM, /missing entry ssr\.thicknessM/],
    [(d) => (d.tiers.ultra = d.tiers.medium), /unknown entry tiers\.ultra/],
    [(d) => (d.tiers.high = d.tiers.medium), /unknown entry tiers\.high/],
    [(d) => delete d.tiers.low, /missing entry tiers\.low/],
    [(d) => (d.tiers.medium.ssrSteps = 2.5), /tiers\.medium\.ssrSteps must be a whole number/],
    [(d) => (d.tiers.medium.ssrSteps = 100), /tiers\.medium\.ssrSteps must be a number in \[0, 64\]/],
    [(d) => (d.tiers.medium.cloudOctaves = 9), /tiers\.medium\.cloudOctaves must be a number in \[0, 8\]/],
    [(d) => (d.tiers.medium.cloudShadow = 1), /tiers\.medium\.cloudShadow must be true or false/],
    [(d) => (d.tiers.opaque.ssrSteps = 4), /tiers\.opaque\.ssrSteps must be 0/],
    [(d) => (d.tiers.medium.maxDistanceM = 0), /tiers\.medium\.maxDistanceM must be over 0/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateReflections(d), message);
  }
});

test("a phone draws a tier under the phone's overrides, the rest the tier's", () => {
  const d = validateReflections(data());
  const m = phoneTier(d, 'medium');
  assert.ok(m.ssrSteps < d.tiers.medium.ssrSteps, 'a phone marches fewer steps on Medium');
  assert.equal(m.maxDistanceM, d.tiers.medium.maxDistanceM);
  assert.deepEqual(phoneTier(d, 'low'), d.tiers.low);
  assert.deepEqual(phoneTier(d, 'opaque'), d.tiers.opaque);
  // A fixture override by data alone reaches the Rust.
  d.phone.low = { cloudShadow: false, ssrSteps: 9, maxDistanceM: 100 };
  const rs = reflectionsRust(validateReflections(d));
  assert.match(rs.split('phone_tiers')[1], /ssr_steps: 9,/);
});
