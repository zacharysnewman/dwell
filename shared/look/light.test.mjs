// node --test shared/look/light.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { lightRust, validateLight } from './light.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'light.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in data validates and generates the checked-in Rust', () => {
  assert.equal(lightRust(validateLight(data())), checkedIn('crates/dwell-world/src/light.gen.rs'));
});

test('values changed by data alone are in the output', () => {
  const d = data();
  d.falloff.vertical = 2;
  d.sky.curve = 1;
  d.sky.direct.fromLevel = 12;
  d.block.color = '#ff0000';
  d.block.strength = 0.5;
  d.characters.easeS = 0.4;
  const rs = lightRust(validateLight(d));
  assert.match(rs, /falloff_vertical: 2,/);
  assert.match(rs, /sky_curve: 1\.0,/);
  assert.match(rs, /direct_from: 12,/);
  assert.match(rs, /block_color: \[1\.0, 0\.0, 0\.0\],/);
  assert.match(rs, /block_strength: 0\.5,/);
  assert.match(rs, /character_ease_s: 0\.4,/);
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.falloff.across = 0), /falloff\.across must be a number in \[1, 15\]/],
    [(d) => (d.falloff.vertical = 1.5), /falloff\.vertical must be a whole number/],
    [(d) => (d.sky.curve = 0.5), /sky\.curve must be a number in \[1, 16\]/],
    [(d) => (d.sky.direct.fromLevel = 15), /fromLevel \(15\) must be below fullLevel/],
    [(d) => (d.block.color = 'orange'), /block\.color must be "#rrggbb"/],
    [(d) => delete d.block.strength, /missing entry block\.strength/],
    [(d) => (d.sky.extra = 1), /unknown entry sky\.extra/],
    [(d) => (d.characters.easeS = -1), /characters\.easeS must be a number in \[0, 5\]/],
  ];
  for (const [edit, re] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateLight(d), re);
  }
});
