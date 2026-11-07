// node --test shared/fluids/falls.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { fallsRust, validateFalls } from './falls.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'falls.json'), 'utf8'));

test('the checked-in falls data validates and generates the checked-in Rust', () => {
  assert.equal(
    fallsRust(validateFalls(data())),
    readFileSync(join(here, '../../crates/dwell-world/src/falls.gen.rs'), 'utf8'),
  );
});

test('a value changed by data alone is in the output', () => {
  const d = data();
  d.minDropM = 2.5;
  assert.match(fallsRust(validateFalls(d)), /min_drop_m: 2\.5,/);
});

test('bad entries fail with their path named', () => {
  for (const [change, message] of [
    [(d) => delete d.minDropM, /minDropM must be a number/],
    [(d) => (d.reachM = 0), /reachM must be a number in \[0\.1, 4\]/],
    [(d) => (d.splash = 1), /unknown entry \(root\)\.splash/],
    [(d) => (d.maxDropM = 0.5), /maxDropM must be above minDropM/],
    [(d) => (d.pushRate = 'fast'), /pushRate must be a number/],
  ]) {
    const d = data();
    change(d);
    assert.throws(() => validateFalls(d), message);
  }
});
