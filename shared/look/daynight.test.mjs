// The day/night data's validation and generation (node --test shared/look/daynight.test.mjs).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkMoonDistance, checkSunDistance, dayNightRust, loadDayNight, pathSkew, validateDayNight } from './daynight.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const load = () => JSON.parse(readFileSync(join(here, 'daynight.json'), 'utf8'));

test('the checked-in data is valid and generates the checked-in Rust', () => {
  const rs = dayNightRust(validateDayNight(load()));
  const checkedIn = readFileSync(join(here, '../../crates/dwell-look/src/daynight.gen.rs'), 'utf8');
  assert.equal(rs, checkedIn, 'run `node shared/look/daynight.mjs`');
});

test('a keyframe changed or added by data alone reaches the generated table', () => {
  const d = load();
  const noon = d.keyframes.find((k) => k.name === 'noon');
  noon.skyZenith = '#ff0000';
  d.keyframes.splice(d.keyframes.indexOf(noon) + 1, 0, { ...noon, name: 'early afternoon', sun: 120 });
  const rs = dayNightRust(validateDayNight(d));
  assert.match(rs, /name: "noon",\n\s+sun: 90\.0,\n\s+sky_zenith: \[1\.0, 0\.0, 0\.0\],/);
  assert.match(rs, /name: "early afternoon",\n\s+sun: 120\.0,/);
  assert.equal(rs.match(/Keyframe \{/g).length, load().keyframes.length + 1);
});

test('a bad entry fails with its path named', () => {
  const cases = [
    [(d) => (d.clock.dayS = -1), /clock\.dayS must be a number/],
    [(d) => (d.keyframes[0].skyZenith = 'blue'), /keyframes\[0\] \("sunrise"\)\.skyZenith must be a colour/],
    [(d) => (d.keyframes[0].sparkle = 1), /unknown entry keyframes\[0\]\.sparkle/],
    [(d) => delete d.keyframes[1].exposure, /missing entry keyframes\[1\]\.exposure/],
    [(d) => (d.keyframes[2].sun = 1), /must be greater than the keyframe before it/],
    [(d) => (d.keyframes[3].name = d.keyframes[0].name), /is used twice/],
    [(d) => (d.keyframes.find((k) => k.name === 'midnight').sunIntensity = 1), /sunIntensity must be 0: the sun is below the horizon/],
    [(d) => (d.keyframes.find((k) => k.name === 'noon').moonIntensity = 1), /moonIntensity must be 0/],
    [(d) => ((d.clock.dayS = 1700), (d.clock.nightS = 100)), /the sun's path would turn back/],
    [(d) => (d.keyframes = d.keyframes.slice(0, 1)), /at least 2/],
  ];
  for (const [edit, message] of cases) {
    const d = load();
    edit(d);
    assert.throws(() => validateDayNight(d), message);
  }
});

test("the sun's path skew puts sunset where the day's share says", () => {
  assert.equal(pathSkew(900, 900), 0);
  const k = pathSkew(1200, 600);
  // Half the daylight after noon, the hour angle is a quarter turn.
  const u = (Math.PI * 2 * 600) / 1800;
  assert.ok(Math.abs(u + k * Math.sin(u) - Math.PI / 2) < 1e-12);
});

test("the moon's orbit keeps the moon grid at the sky's size", () => {
  const grids = JSON.parse(readFileSync(join(here, '../worldgen/grids.json'), 'utf8'));
  assert.ok(loadDayNight(), 'the checked-in distance agrees with grids.json');
  const d = load();
  d.moonOrbit.distanceM = 400000;
  assert.throws(() => checkMoonDistance(validateDayNight(d), grids), /moonOrbit\.distanceM must keep the moon's 12500 m radius/);
  const far = load();
  far.moon.radiusDeg = 0.6;
  far.moonOrbit.distanceM = 12500 / Math.tan((0.6 * Math.PI) / 180);
  assert.ok(checkMoonDistance(validateDayNight(far), grids), 'a smaller moon in the sky by data alone');
  assert.throws(() => checkMoonDistance(validateDayNight(load()), { grids: [] }), /no "dwell:moon" grid/);
});

test("the sun world sits on the sun's path at the sky's sun size", () => {
  const grids = JSON.parse(readFileSync(join(here, '../worldgen/grids.json'), 'utf8'));
  const sun = grids.grids.find((g) => g.name === 'dwell:sun');
  const d = load();
  assert.ok(Math.abs(Math.atan(sun.radiusM / d.sun.distanceM) * (180 / Math.PI) - d.sun.radiusDeg) < 0.015, "today's disc size");
  d.sun.distanceM = 500000;
  assert.throws(() => checkSunDistance(validateDayNight(d), grids), /sun\.distanceM must keep the sun's 25000 m radius/);
  const big = load();
  big.sun.radiusDeg = 3;
  big.sun.distanceM = 25000 / Math.tan((3 * Math.PI) / 180);
  assert.ok(checkSunDistance(validateDayNight(big), grids), 'a bigger sun in the sky by data alone');
  assert.throws(() => checkSunDistance(validateDayNight(load()), { grids: [] }), /no "dwell:sun" grid/);
});
