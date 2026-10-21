// node --test shared/look/flames.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { flameNames, flamesRust, validateFlames } from './flames.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'flames.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in data validates and generates the checked-in Rust', () => {
  assert.equal(flamesRust(validateFlames(data())), checkedIn('crates/dwell-look/src/flames.gen.rs'));
  assert.deepEqual(flameNames(data()), ['dwell:torch']);
});

test('a flame look added by data alone reaches the Rust table (a fixture candle)', () => {
  const d = data();
  d.flames['test:candle'] = {
    ramp: ['#d0401c', '#ffe2a0'],
    bands: 0,
    heightM: 0.08,
    widthM: 0.04,
    baseM: 0.2,
    heldScale: 1,
    baseWidthM: 0.02,
    speed: 0.8,
    noiseScale: 1.5,
    distort: 0.2,
    tongues: 0,
    sway: 0.05,
    sizeFlicker: 0.2,
    glow: { color: '#ffc070', radiusM: 0.15, strength: 0.2 },
    embers: { count: 0, color: '#ffc070', sizeM: 0.01, riseM: 0, speed: 1 },
  };
  d.drawM = 32;
  const rs = flamesRust(validateFlames(d));
  assert.match(rs, /pub static FLAMES: \[FlameLook; 2\]/);
  assert.match(rs, /name: "test:candle",\n {8}ramp: &\[0xd0401c, 0xffe2a0\],\n {8}bands: 0,/);
  assert.match(rs, /embers: FlameEmbers \{ count: 0, color: 0xffc070/);
  assert.match(rs, /draw_m: 32\.0,/);
  assert.deepEqual(flameNames(d), ['dwell:torch', 'test:candle']);
});

test('bad entries fail with their path named', () => {
  const torch = (d) => d.flames['dwell:torch'];
  const cases = [
    [(d) => (torch(d).ramp = ['#ff0000']), /flames\.dwell:torch\.ramp must list 2 to 6 colours/],
    [(d) => (torch(d).ramp[1] = 'orange'), /flames\.dwell:torch\.ramp\[1\] must be #rrggbb/],
    [(d) => (torch(d).heightM = 0), /flames\.dwell:torch\.heightM must be a number in \(0, 2\]/],
    [(d) => (torch(d).bands = 2.5), /flames\.dwell:torch\.bands must be a whole number/],
    [(d) => delete torch(d).speed, /missing entry flames\.dwell:torch\.speed/],
    [(d) => (torch(d).baseWidthM = 0.5), /flames\.dwell:torch\.baseWidthM must be at most widthM/],
    [(d) => (torch(d).heldScale = 0), /flames\.dwell:torch\.heldScale must be a number in \(0, 4\]/],
    [(d) => (torch(d).glow.radiusM = 9), /flames\.dwell:torch\.glow\.radiusM must be a number/],
    [(d) => (torch(d).embers.count = 20), /flames\.dwell:torch\.embers\.count must be a number in \[0, 8\]/],
    [(d) => (torch(d).embers.wobble = 1), /unknown entry flames\.dwell:torch\.embers\.wobble/],
    [(d) => (torch(d).smoke = 1), /unknown entry flames\.dwell:torch\.smoke/],
    [(d) => (d.flames.Torch = torch(d)), /flames\.Torch: the name must be/],
    [(d) => (d.peak = 0), /peak must be a number in \[0\.1, 16\]/],
    [(d) => (d.maxFlames = 0), /maxFlames must be a number in \[1, 256\]/],
    [(d) => (d.flames = {}), /at least one look/],
  ];
  for (const [edit, re] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateFlames(d), re);
  }
});
