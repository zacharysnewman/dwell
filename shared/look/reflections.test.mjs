// node --test shared/look/reflections.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { reflectionsRust, validateReflections } from './reflections.mjs';

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
  d.tiers.high.cloudShadow = false;
  d.ssr.edgeFade = 0.2;
  const rs = reflectionsRust(validateReflections(d));
  assert.match(rs, /ssr_steps: 7,/);
  assert.match(rs, /edge_fade: 0\.2,/);
  assert.doesNotMatch(rs, /cloud_shadow: true/);
  assert.notEqual(rs, reflectionsRust(validateReflections(data())));
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.ssr.edgeFade = 2), /ssr\.edgeFade must be a number in \[0\.001, 0\.5\]/],
    [(d) => delete d.ssr.thicknessM, /missing entry ssr\.thicknessM/],
    [(d) => (d.tiers.ultra = d.tiers.high), /unknown entry tiers\.ultra/],
    [(d) => (d.tiers.high.ssrSteps = 2.5), /tiers\.high\.ssrSteps must be a whole number/],
    [(d) => (d.tiers.high.ssrSteps = 100), /tiers\.high\.ssrSteps must be a number in \[0, 64\]/],
    [(d) => (d.tiers.medium.cloudOctaves = 9), /tiers\.medium\.cloudOctaves must be a number in \[0, 8\]/],
    [(d) => (d.tiers.medium.cloudShadow = 1), /tiers\.medium\.cloudShadow must be true or false/],
    [(d) => (d.tiers.low.ssrSteps = 4), /tiers\.low\.ssrSteps must be 0/],
    [(d) => (d.tiers.medium.maxDistanceM = 0), /tiers\.medium\.maxDistanceM must be over 0/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateReflections(d), message);
  }
});
