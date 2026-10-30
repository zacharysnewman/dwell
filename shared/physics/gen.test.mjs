// node --test shared/physics/gen.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { fellingRust, validateFelling } from './gen.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'felling.json'), 'utf8'));

test('the checked-in tunables validate and generate the checked-in Rust', () => {
  assert.equal(
    fellingRust(validateFelling(data())),
    readFileSync(join(here, '../../crates/dwell-sim/src/felling.gen.rs'), 'utf8'),
  );
});

test('a value changed by data alone is in the output', () => {
  const d = data();
  d.toppleSpin = 1.25;
  assert.match(fellingRust(validateFelling(d)), /topple_spin: 1\.25,/);
});

test('bad entries fail with their path named', () => {
  for (const [change, message] of [
    [(d) => (d.riderReach = 1.5), /riderReach must be a whole number/],
    [(d) => (d.maxPieceCells = 5000), /maxPieceCells must be a whole number in \[1, 4096\]/],
    [(d) => delete d.hitSpeed, /missing entry \(root\)\.hitSpeed/],
    [(d) => (d.settleBounce = 1.5), /settleBounce must be a number in \[0, 1\]/],
    [(d) => (d.spin = 1), /unknown entry \(root\)\.spin/],
  ]) {
    const d = data();
    change(d);
    assert.throws(() => validateFelling(d), message);
  }
});
