// node --test shared/look/outline.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { outlineRust, validateOutline } from './outline.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'outline.json'), 'utf8'));
const palette = () => JSON.parse(readFileSync(join(here, '../textures/palette.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in modes validate and generate the checked-in Rust', () => {
  assert.equal(outlineRust(validateOutline(data(), palette())), checkedIn('crates/dwell-look/src/outline.gen.rs'));
});

test('the ink is the palette colour it names', () => {
  const rs = outlineRust(validateOutline(data(), palette()));
  // ink #241c28
  assert.match(rs, /ink: \[0\.141176, 0\.109804, 0\.156863\],/);
  const d = data();
  d.ink = 'stone.shadow';
  const hex = palette().ramps.stone.shadow;
  const r = (parseInt(hex.slice(1, 3), 16) / 255).toFixed(6);
  assert.match(outlineRust(validateOutline(d, palette())), new RegExp(`ink: \\[${+r}`));
});

test('a mode added by data alone is in the output, its edges as bits', () => {
  const d = data();
  d.modes.push({
    name: 'thick-blocks',
    edges: ['block', 'depth'],
    widthPx: 3,
    fadeStartM: 8,
    fadeEndM: 20,
    strength: 1,
    depthThreshold: 0.2,
    creaseThreshold: 1,
  });
  const rs = outlineRust(validateOutline(d, palette()));
  assert.match(rs, /name: "thick-blocks",\n {12}edges: 5,\n {12}width_px: 3\.0,/);
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.ink = 'nope'), /ink: unknown palette colour "nope"/],
    [(d) => (d.modes = []), /modes must be a list of 1 to 16 modes/],
    [(d) => (d.modes[0].name = 'off'), /modes\[0\]\.name "off" is reserved/],
    [(d) => (d.modes[1].name = 'silhouette'), /modes\[1\]\.name "silhouette" is another mode's/],
    [(d) => (d.modes[0].name = 'Big'), /modes\[0\]\.name must be lower-case/],
    [(d) => (d.modes[0].edges = []), /modes\[0\]\.edges must list one or more/],
    [(d) => (d.modes[0].edges = ['ids']), /modes\[0\]\.edges\[0\] "ids" is not an edge kind/],
    [(d) => (d.modes[0].edges = ['depth', 'depth']), /modes\[0\]\.edges\[1\] "depth" is listed twice/],
    [(d) => (d.modes[0].widthPx = 0), /modes\[0\]\.widthPx must be a number in \[0\.5, 8\]/],
    [(d) => (d.modes[2].fadeEndM = 5), /modes\[2\]\.fadeEndM must be greater than modes\[2\]\.fadeStartM/],
    [(d) => (d.modes[0].strength = 2), /modes\[0\]\.strength must be a number in \[0, 1\]/],
    [(d) => delete d.modes[0].depthThreshold, /missing entry modes\[0\]\.depthThreshold/],
    [(d) => (d.modes[0].colour = '#000000'), /unknown entry modes\[0\]\.colour/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateOutline(d, palette()), message);
  }
});
