// node --test shared/look/pixelart.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { pixelArtRust, validatePixelArt } from './pixelart.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'pixelart.json'), 'utf8'));
const palette = () => JSON.parse(readFileSync(join(here, '../textures/palette.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in presets validate and generate the checked-in Rust', () => {
  assert.equal(pixelArtRust(validatePixelArt(data(), palette())), checkedIn('crates/dwell-look/src/pixelart.gen.rs'));
});

test('palette references resolve to the palette colour they name', () => {
  const d = data();
  d.palettes.dwell[0][1] = 'wool_blue.shadow';
  const hex = palette().ramps.wool_blue.shadow;
  const r = +(parseInt(hex.slice(1, 3), 16) / 255).toFixed(6);
  assert.match(pixelArtRust(validatePixelArt(d, palette())), new RegExp(`\\[${r}, `));
});

test('a palette and a preset added by data alone are in the output', () => {
  const d = data();
  d.palettes.mono = [['#101010', '#505050', '#a0a0a0', '#f0f0f0']];
  d.presets.push({
    ...d.presets[0],
    name: 'gameboy',
    height: 144,
    fit: 'letterbox',
    palette: 'mono',
    edges: ['normal'],
    atlas: 'point',
  });
  const rs = pixelArtRust(validatePixelArt(d, palette()));
  assert.match(rs, /name: "mono",\n {12}ramps: &\[\n {16}&\[\n {20}\[0\.062745, 0\.062745, 0\.062745\],/);
  assert.match(
    rs,
    /name: "gameboy",\n {12}height: 144,\n {12}fit: Fit::Letterbox,\n {12}palette: 2,/,
  );
  assert.match(rs, /edges: 2,\n {12}edge_darken: [\d.]+,\n {12}edge_fade_m: [\d.]+,\n {12}atlas: AtlasFilter::Point,/);
});

test("a preset's textures pick the texture set, and the cel presets are their painterly twins", () => {
  const d = data();
  const byName = Object.fromEntries(d.presets.map((p) => [p.name, p]));
  for (const [cel, painterly] of [['270-cel', '270'], ['360-cel', '360']]) {
    assert.equal(byName[cel].textures, 'cel');
    assert.deepEqual({ ...byName[cel], name: painterly, textures: 'painterly' }, byName[painterly]);
  }
  const rs = pixelArtRust(validatePixelArt(d, palette()));
  assert.match(rs, /name: "270-cel",(\n.*){12}\n {12}textures: Textures::Cel,/);
  assert.match(rs, /name: "270",(\n.*){12}\n {12}textures: Textures::Painterly,/);
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.palettes.dwell[0][0] = 'nope'), /palettes\.dwell\[0\]\[0\]: unknown palette colour "nope"/],
    [(d) => (d.palettes.dwell[0][0] = '#12345'), /palettes\.dwell\[0\]\[0\] must be a colour/],
    [(d) => (d.palettes.dwell[0] = ['ink']), /palettes\.dwell\[0\] must be a ramp of 2 to 8 colours/],
    [(d) => (d.palettes.dwell[1][1] = 'ink'), /palettes\.dwell\[1\]\[1\] is the same colour as palettes\.dwell\[0\]\[1\]/],
    [(d) => (d.palettes.tiny = [['#000000', '#ffffff']]), /palettes\.tiny must hold 4 to 64 colours, has 2/],
    [(d) => (d.presets = []), /presets must be a list of 1 to 16 presets/],
    [(d) => (d.presets[0].name = 'off'), /presets\[0\]\.name "off" is reserved/],
    [(d) => (d.presets[1].name = '180'), /presets\[1\]\.name "180" is another preset's/],
    [(d) => (d.presets[0].height = 20), /presets\[0\]\.height must be a number in \[60, 1080\]/],
    [(d) => (d.presets[0].height = 200.5), /presets\[0\]\.height must be a whole number/],
    [(d) => (d.presets[0].fit = 'stretch'), /presets\[0\]\.fit must be one of crop, letterbox/],
    [(d) => (d.presets[0].palette = 'nes'), /presets\[0\]\.palette "nes" is not a palette/],
    [(d) => (d.presets[0].dither = 2), /presets\[0\]\.dither must be a number in \[0, 1\]/],
    [(d) => (d.presets[0].bands = 1), /presets\[0\]\.bands must be 0 \(smooth\) or at least 2/],
    [(d) => (d.presets[0].shadowTint = 'nope'), /presets\[0\]\.shadowTint: unknown palette colour/],
    [(d) => (d.presets[0].edges = ['block']), /presets\[0\]\.edges\[0\] "block" is not an edge kind/],
    [(d) => (d.presets[0].atlas = 'linear'), /presets\[0\]\.atlas must be one of point, point-mips/],
    [(d) => (d.presets[0].textures = 'classic'), /presets\[0\]\.textures must be one of painterly, cel/],
    [(d) => delete d.presets[0].textures, /missing entry presets\[0\]\.textures/],
    [(d) => (d.presets[0].snapCamera = 'yes'), /presets\[0\]\.snapCamera must be true or false/],
    [(d) => delete d.presets[0].snapDepthM, /missing entry presets\[0\]\.snapDepthM/],
    [(d) => (d.presets[0].bloom = 1), /unknown entry presets\[0\]\.bloom/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validatePixelArt(d, palette()), message);
  }
});
