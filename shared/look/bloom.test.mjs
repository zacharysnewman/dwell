// node --test shared/look/bloom.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { bloomRust, validateBloom } from './bloom.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'bloom.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in looks validate and generate the checked-in Rust', () => {
  assert.equal(bloomRust(validateBloom(data())), checkedIn('crates/dwell-look/src/bloom.gen.rs'));
});

test('a retuned tier and look by data alone reach the output', () => {
  const d = data();
  d.intensity = 2.5;
  d.tiers.high = { scale: 1, levels: 6 };
  const rs = bloomRust(validateBloom(d));
  assert.match(rs, /intensity: 2\.5,/);
  assert.match(rs, /HIGH: BloomTier = BloomTier \{\s+scale: 1\.0,\s+levels: 6,/);
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.tiers.low.levels = 1), /tiers\.low\.levels must be a number in \[2, 8\]/],
    [(d) => (d.tiers.high.levels = 2.5), /tiers\.high\.levels must be a whole number/],
    [(d) => (d.tiers.high.scale = 0), /tiers\.high\.scale must be a number in \(0, 1\]/],
    [(d) => (d.intensity = -1), /intensity must be a number/],
    [(d) => (d.glow = 1), /unknown entry the file\.glow/],
    [(d) => delete d.radius, /missing entry the file\.radius/],
    [(d) => (d.tiers.low = { scale: 1, levels: 8 }), /tiers\.low must cost no more than tiers\.high/],
  ];
  for (const [edit, re] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateBloom(d), re);
  }
});
