// node --test shared/look/ssao.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { ssaoRust, validateSsao } from './ssao.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'ssao.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in looks validate and generate the checked-in Rust', () => {
  assert.equal(ssaoRust(validateSsao(data())), checkedIn('crates/dwell-look/src/ssao.gen.rs'));
});

test('a retuned tier and look by data alone reach the output', () => {
  const d = data();
  d.radius = 1.5;
  d.levels.weak.radius = 1.5; // weak is the file's look
  d.blurRadius = 5;
  d.tiers.high = { scale: 1, samples: 24 };
  const rs = ssaoRust(validateSsao(d));
  assert.match(rs, /radius: 1\.5,/);
  assert.match(rs, /blur_radius: 5,/);
  assert.match(rs, /HIGH: SsaoTier = SsaoTier \{\s+scale: 1\.0,\s+samples: 24,/);
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.tiers.low.samples = 2), /tiers\.low\.samples must be a number in \[4, 32\]/],
    [(d) => (d.tiers.high.samples = 9.5), /tiers\.high\.samples must be a whole number/],
    [(d) => (d.tiers.high.scale = 0), /tiers\.high\.scale must be a number in \(0, 1\]/],
    [(d) => (d.strength = 2), /strength must be a number in \[0, 1\]/],
    [(d) => (d.radius = 0), /radius must be a number in \(0, 8\]/],
    [(d) => (d.glow = 1), /unknown entry the file\.glow/],
    [(d) => (d.levels.weak.intensity = 3), /levels\.weak\.intensity must be the file's intensity/],
    [(d) => delete d.levels.strong, /missing entry levels\.strong/],
    [(d) => (d.levels.normal.radius = 0), /levels\.normal\.radius must be a number in \(0, 8\]/],
    [(d) => delete d.bias, /missing entry the file\.bias/],
    [(d) => (d.farTo = d.farFrom), /farTo must be past farFrom/],
    [(d) => (d.nearTo = 0.1), /nearTo must be at least nearFrom/],
    [(d) => (d.tiers.low = { scale: 1, samples: 32 }), /tiers\.low must cost no more than tiers\.high/],
  ];
  for (const [edit, re] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateSsao(d), re);
  }
});
