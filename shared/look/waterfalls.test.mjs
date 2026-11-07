// node --test shared/look/waterfalls.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { liquidNames, validateWaterfalls, waterfallsRust } from './waterfalls.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'waterfalls.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in data validates and generates the checked-in Rust', () => {
  assert.equal(waterfallsRust(validateWaterfalls(data())), checkedIn('crates/dwell-look/src/waterfalls.gen.rs'));
  assert.deepEqual(liquidNames(data()), ['dwell:water']);
});

test('a liquid added by data alone reaches the Rust table (a fixture oil, dark and thick)', () => {
  const d = data();
  d.liquids['test:oil'] = {
    sheet: { color: '#201808', opacityThin: 0.9, opacityFull: 1, streaksPerM: 1, scroll: 0.5, segments: 4 },
    foam: { color: '#403010', opacity: 0.2, radiusM: 0.2 },
    spray: { count: 0, cpuCount: 0, sizeM: 0.1, heightM: 0.2, lifeS: 1, color: '#201808', opacity: 0.5 },
  };
  const rust = waterfallsRust(validateWaterfalls(d));
  assert.match(rust, /fluid: "test:oil",\n {8}sheet_color: 0x201808,/);
  assert.match(rust, /FALL_LOOKS: \[FallLook; 2\]/);
});

test('bad entries fail with their path', () => {
  const bad = (edit, re) => {
    const d = data();
    edit(d);
    assert.throws(() => validateWaterfalls(d), re);
  };
  const w = (edit) => (d) => edit(d.liquids['dwell:water']);
  bad((d) => (d.maxFalls = 0.5), /maxFalls must be a number/);
  bad((d) => delete d.drawM, /missing entry the file\.drawM/);
  bad(w((l) => (l.sheet.segments = 2.5)), /sheet\.segments must be a whole number/);
  bad(w((l) => (l.sheet.opacityFull = 0.1)), /opacityFull must be at least sheet\.opacityThin/);
  bad(w((l) => (l.spray.cpuCount = 60)), /cpuCount must be at most spray\.count/);
  bad(w((l) => (l.foam.colour = '#ffffff')), /unknown entry liquids\.dwell:water\.foam\.colour/);
  bad(w((l) => (l.spray.color = 'white')), /spray\.color must be #rrggbb/);
  bad((d) => (d.liquids.Water = d.liquids['dwell:water']), /namespace:name/);
});

test('a CPU adapter draws no spray, and the colours are tinted, not pure white', () => {
  const l = validateWaterfalls(data()).liquids['dwell:water'];
  assert.equal(l.spray.cpuCount, 0);
  for (const c of [l.sheet.color, l.foam.color, l.spray.color]) {
    const [r, g, b] = [1, 3, 5].map((k) => parseInt(c.slice(k, k + 2), 16));
    assert.ok(Math.max(r, g, b) < 0xf0 && b >= r, c);
  }
});
