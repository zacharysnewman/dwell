// node --test shared/look/fog.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { fogRust, validateFog } from './fog.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'fog.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in fog validates and generates the checked-in Rust', () => {
  assert.equal(fogRust(validateFog(data())), checkedIn('crates/dwell-look/src/fog.gen.rs'));
});

test('a fog changed by data alone reaches the output', () => {
  const d = data();
  d.distanceM = 4000;
  d.density = 0.5;
  d.heightM = 900.5;
  const rs = fogRust(validateFog(d));
  assert.match(rs, /distance_m: 4000\.0,/);
  assert.match(rs, /density: 0\.5,/);
  assert.match(rs, /height_m: 900\.5,/);
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.distanceM = 999), /distanceM must be a number in \[1000, 100000\]/],
    [(d) => (d.density = 1.1), /density must be a number in \[0, 1\]/],
    [(d) => (d.heightM = 'tall'), /heightM must be a number/],
    [(d) => delete d.density, /missing entry \(root\)\.density/],
    [(d) => (d.colour = '#fff'), /unknown entry \(root\)\.colour/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateFog(d), message);
  }
});

test('a preset added by data alone is generated over the fog', () => {
  const d = data();
  d.presets = { misty: { density: 0.5, air: { sunTint: 0.25 }, veil: { contrast: 0.1 } } };
  const rs = fogRust(validateFog(d));
  assert.match(rs, /"misty",\n\s+FogSettings \{\n\s+distance_m: 16000\.0,\n\s+density: 0\.5,/);
  assert.match(rs, /sun_tint: 0\.25,/);
  assert.match(rs, /veil_contrast: 0\.1,/);
});

test('bad aerial entries and presets fail with their path named', () => {
  const cases = [
    [(d) => (d.air.elevationDeg = 45), /\(root\)\.air\.elevationDeg must be a number in \[0, 30\]/],
    [(d) => (d.air.warm = [1, 1]), /\(root\)\.air\.warm must be \[r, g, b\]/],
    [(d) => (d.veil.saturation = -0.1), /\(root\)\.veil\.saturation must be a number in \[0, 1\]/],
    [(d) => delete d.veil, /missing entry \(root\)\.veil/],
    [(d) => (d.presets.thicker.heightM = 10), /presets\.thicker\.heightM must be a number/],
    [(d) => (d.presets.thicker.glow = 1), /unknown entry presets\.thicker\.glow/],
    [(d) => (d.presets.Bad = {}), /presets\.Bad: a name is lower-case/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateFog(d), message);
  }
});
