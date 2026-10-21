// node --test shared/look/lodshadows.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { lodShadowsRust, validateLodShadows } from './lodshadows.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'lodshadows.json'), 'utf8'));

test('the checked-in tunables validate and generate the checked-in Rust', () => {
  const rs = lodShadowsRust(validateLodShadows(data()));
  const checkedIn = readFileSync(join(here, '../../crates/dwell-look/src/lodshadows.gen.rs'), 'utf8');
  assert.equal(rs, checkedIn);
});

test('a tunable changed by data changes the output', () => {
  const d = data();
  d.tiers.low.levels = 2;
  d.tiers.high.steps = 40;
  d.march.penumbraDeg = 3;
  const rs = lodShadowsRust(validateLodShadows(d));
  assert.match(rs, /levels: 2,\n {12}level_scale: 4,\n {12}steps: 16,/);
  assert.match(rs, /steps: 40,/);
  assert.match(rs, /penumbra_deg: 3\.0,/);
  assert.notEqual(rs, lodShadowsRust(validateLodShadows(data())));
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.tiers.high.levels = 3), /tiers\.high\.levels must be a number in \[1, 2\]/],
    [(d) => (d.tiers.high.steps = 2.5), /tiers\.high\.steps must be a whole number/],
    [(d) => (d.tiers.low.resolution = 1000), /tiers\.low\.resolution must be a power of two/],
    [(d) => delete d.march.biasM, /missing entry march\.biasM/],
    [(d) => (d.tiers.ultra = d.tiers.high), /unknown entry tiers\.ultra/],
    [(d) => ((d.heights.bottomM = 500), (d.heights.topM = 400)), /heights\.topM must be over heights\.bottomM/],
    [(d) => (d.tiers.low.reachM = 9000), /tiers\.low\.reachM must be at most the outermost map's half-width \(2048 m\)/],
    [(d) => (d.tiers.low.reachM = 10), /tiers\.low\.reachM must be over the first step/],
    [(d) => (d.march.penumbraDeg = 0), /march\.penumbraDeg must be a number in/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateLodShadows(d), message);
  }
});
