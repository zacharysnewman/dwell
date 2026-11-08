// node --test shared/look/shafts.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { shaftsRust, validateShafts } from './shafts.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'shafts.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in looks validate and generate the checked-in Rust', () => {
  assert.equal(shaftsRust(validateShafts(data())), checkedIn('crates/dwell-look/src/shafts.gen.rs'));
});

test('a fluid added by data alone reaches the output', () => {
  const d = data();
  d.fluids['test:glow'] = {
    strength: 1.5,
    reachM: 6,
    fadeM: 3,
    cellM: 0.5,
    speed: 0,
    window: { color: '#ffcc88', glow: 1, edge: 0.5 },
  };
  const rs = shaftsRust(validateShafts(d));
  assert.match(rs, /fluid: "test:glow",\s+strength: 1\.5,[\s\S]*window_color: 0xffcc88,/);
});

test('bad entries fail with their path named', () => {
  const w = 'dwell:water';
  const cases = [
    [(d) => (d.tiers.high.steps = 2.5), /tiers\.high\.steps must be a whole number/],
    [(d) => (d.tiers.low.scale = 0), /tiers\.low\.scale must be a number in \(0, 1\]/],
    [(d) => (d.tiers.low.steps = 64), /tiers\.low must cost no more than tiers\.high/],
    [(d) => delete d.tiers.low, /missing entry tiers\.low/],
    [(d) => (d.air.sun.decay = 2), /air\.sun\.decay must be a number in \[0\.5, 1\]/],
    [(d) => (d.air.moon.extra = 1), /unknown entry air\.moon\.extra/],
    [(d) => (d.fluids[w].reachM = 0), /fluids\.dwell:water\.reachM must be a number in \(0, 256\]/],
    [(d) => (d.fluids[w].window.color = 'cyan'), /fluids\.dwell:water\.window\.color must be #rrggbb/],
    [(d) => (d.fluids.Water = d.fluids[w]), /fluids\.Water: the name must be/],
    [(d) => delete d.default.window, /missing entry default\.window/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateShafts(d), message);
  }
});

test('the shafts are soft and subtle, and Low is the cheaper tier (owner, 2026-11-07)', () => {
  const d = data();
  assert.ok(d.air.sun.strength > 0 && d.air.sun.strength <= 0.6, 'the sun shafts glow, softly');
  assert.ok(d.air.moon.strength < d.air.sun.strength, 'the moon faintly');
  assert.ok(d.fluids['dwell:water'].strength > 0, 'water has shafts');
  assert.equal(d.tiers.high.scale, 0.5, 'High at half resolution');
  assert.equal(d.tiers.low.scale, 0.25, 'Low at quarter resolution');
});
