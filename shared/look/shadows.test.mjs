// node --test shared/look/shadows.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { shadowsRust, validateShadows } from './shadows.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'shadows.json'), 'utf8'));

test('the checked-in tunables validate and generate the checked-in Rust', () => {
  const rs = shadowsRust(validateShadows(data()));
  const checkedIn = readFileSync(join(here, '../../crates/dwell-look/src/shadows.gen.rs'), 'utf8');
  assert.equal(rs, checkedIn);
});

test('a tunable changed by data changes the output', () => {
  const d = data();
  d.tiers.high.cascades = 3;
  d.tiers.low.resolution = 512;
  d.lights.moon.strength = 0.5;
  const rs = shadowsRust(validateShadows(d));
  assert.match(rs, /cascades: 3,/);
  assert.match(rs, /resolution: 512,/);
  assert.match(rs, /strength: 0\.5,/);
  assert.notEqual(rs, shadowsRust(validateShadows(data())));
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.tiers.high.cascades = 5), /tiers\.high\.cascades must be a number in \[1, 4\]/],
    [(d) => (d.tiers.high.cascades = 2.5), /tiers\.high\.cascades must be a whole number/],
    [(d) => (d.tiers.low.resolution = 1000), /tiers\.low\.resolution must be a power of two/],
    [(d) => delete d.bias.depthBiasM, /missing entry bias\.depthBiasM/],
    [(d) => (d.tiers.ultra = d.tiers.high), /unknown entry tiers\.ultra/],
    [(d) => (d.lights.sun.fadeEndDeg = -1), /lights\.sun\.fadeEndDeg must be over fadeStartDeg/],
    [(d) => (d.tiers.low.filterTaps = 0), /tiers\.low\.filterTaps must be a number in \[1, 4\]/],
    [(d) => ((d.cascades.nearM = 50), (d.tiers.low.distanceM = 40)), /tiers\.low\.distanceM must be over cascades\.nearM/],
    [(d) => (d.cascades.spyglassMaxM = 10), /cascades\.spyglassMaxM must be at least/],
    [(d) => (d.tiers.high.plantCascades = d.tiers.high.cascades + 1), /tiers\.high\.plantCascades must be at most cascades/],
    [(d) => (d.tiers.low.farPerFrame = 0), /tiers\.low\.farPerFrame must be a number in \[1, 4\]/],
    [(d) => (d.cascades.slack = 2), /cascades\.slack must be a number in \[0, 1\]/],
    [(d) => delete d.cascades.lightTurnDeg, /missing entry cascades\.lightTurnDeg/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateShadows(d), message);
  }
});
