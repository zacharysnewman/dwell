// node --test shared/look/atmosphere.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { atmosphereRust, validateAtmosphere } from './atmosphere.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'atmosphere.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in atmosphere validates and generates the checked-in Rust', () => {
  assert.equal(atmosphereRust(validateAtmosphere(data())), checkedIn('crates/dwell-look/src/atmosphere.gen.rs'));
});

test('an atmosphere changed by data alone reaches the output', () => {
  const d = data();
  d.topM = 120000.5;
  d.spaceColor = '#ff0000';
  const rs = atmosphereRust(validateAtmosphere(d));
  assert.match(rs, /top_m: 120000\.5,/);
  assert.match(rs, /space_color: \[1\.0, 0\.0, 0\.0\],/);
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.topM = d.baseM), /topM must be greater than baseM/],
    [(d) => (d.starsInSpace = 2), /starsInSpace must be a number in \[0, 1\]/],
    [(d) => (d.spaceColor = 'black'), /spaceColor must be a colour/],
    [(d) => delete d.limbWidthDeg, /missing entry \(root\)\.limbWidthDeg/],
    [(d) => (d.ozone = 1), /unknown entry \(root\)\.ozone/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateAtmosphere(d), message);
  }
});
