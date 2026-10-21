// node --test shared/look/lodcolour.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { lodColourRust, validateLodColour } from './lodcolour.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'lodcolour.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in makeup validates and generates the checked-in Rust', () => {
  assert.equal(lodColourRust(validateLodColour(data())), checkedIn('crates/dwell-look/src/lodcolour.gen.rs'));
});

test('a point and a step added by data alone reach the output', () => {
  const d = data();
  d.gradientStep = 0.1;
  d.rise.push({ gradient: 50, top: 0, fringe: 0.01 });
  const rs = lodColourRust(validateLodColour(d));
  assert.match(rs, /gradient_step: 0\.1,/);
  assert.match(rs, /gradient: 50\.0,\n {12}top: 0\.0,\n {12}fringe: 0\.01,/);
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.gradientStep = 0), /gradientStep must be a number in \[0\.001, 1\]/],
    [(d) => (d.rise = []), /rise must be a non-empty array/],
    [(d) => (d.rise[1].gradient = d.rise[0].gradient), /rise\[1\]\.gradient must be greater/],
    [(d) => (d.rise[0].top = 1.2), /rise\[0\]\.top must be a number in \[0, 1\]/],
    [(d) => (d.rise[0].fringe = 0.9), /rise\[0\]: top \+ fringe must be at most 1/],
    [(d) => delete d.rise[2].fringe, /missing entry rise\[2\]\.fringe/],
    [(d) => (d.rise[0].dirt = 0.2), /unknown entry rise\[0\]\.dirt/],
    [(d) => (d.tint = 2), /unknown entry \(root\)\.tint/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateLodColour(d), message);
  }
});
