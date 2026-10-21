// node --test shared/look/lodblend.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { lodBlendRust, validateLodBlend } from './lodblend.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'lodblend.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in tunables validate and generate the checked-in Rust', () => {
  assert.equal(lodBlendRust(validateLodBlend(data())), checkedIn('crates/dwell-look/src/lodblend.gen.rs'));
});

test('a pattern, fade time and ratio changed by data alone reach the output', () => {
  const d = data();
  d.pattern = 'noise';
  d.fadeS = 0;
  d.coarsenRatio = 1;
  const rs = lodBlendRust(validateLodBlend(d));
  assert.match(rs, /pattern: FadePattern::Noise,/);
  assert.match(rs, /fade_s: 0\.0,/);
  assert.match(rs, /coarsen_ratio: 1\.0,/);
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.fadeS = -0.1), /fadeS must be a number in \[0, 5\]/],
    [(d) => (d.fadeS = 'slow'), /fadeS must be a number/],
    [(d) => (d.pattern = 'blue'), /pattern must be one of bayer4, bayer8, noise/],
    [(d) => (d.pattern = 'toString'), /pattern must be one of/],
    [(d) => (d.coarsenRatio = 0.9), /coarsenRatio must be a number in \[1, 4\]/],
    [(d) => delete d.pattern, /missing entry \(root\)\.pattern/],
    [(d) => (d.speed = 2), /unknown entry \(root\)\.speed/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateLodBlend(d), message);
  }
});
