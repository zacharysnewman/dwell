// node --test shared/fluids/lava.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { lavaRust, outputs, validateLava } from './lava.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'lava.json'), 'utf8'));

test('the checked-in lava data validates and generates the checked-in Rust', () => {
  for (const [p, src] of outputs()) assert.equal(src, readFileSync(join(here, '../..', p), 'utf8'), p);
});

test('a fluid and a vent added by data alone are in the output', () => {
  const d = data();
  d.fluids.push({ ...d.fluids[0], name: 'mymod:pitch', viscosityM2s: 50 });
  d.vents = [{ name: 'mymod:seep', fluid: 'mymod:pitch', kind: 'river', x: 1, z: 2, rateM3s: 0.5 }];
  const rs = lavaRust(validateLava(d));
  assert.match(rs, /name: "mymod:pitch",\n\s+viscosity_m2s: 50\.0,/);
  assert.match(rs, /VentDef \{ name: "mymod:seep", fluid: "mymod:pitch", kind: VentKind::River, x: 1, z: 2, rate_m3s: 0\.5, level_m: 0\.0 \}/);
});

test('bad entries fail with their path named', () => {
  const lava = (d) => d.fluids[0];
  for (const [change, message] of [
    [(d) => (d.tickEvery = 0.5), /\(root\)\.tickEvery must be a whole number/],
    [(d) => (d.extra = 1), /unknown field extra/],
    [(d) => (lava(d).viscosityM2s = 0), /fluids\.dwell:lava\.viscosityM2s must be a number in \[0\.01, 1000\]/],
    [(d) => (lava(d).rock = 'dwell:nope'), /fluids\.dwell:lava\.rock: "dwell:nope" is not a block of the registry/],
    [(d) => (lava(d).quench.rock = 'dwell:nope'), /quench\.rock: "dwell:nope" is not a block/],
    [(d) => (lava(d).light = 16), /light must be a whole number in \[0, 15\]/],
    [(d) => (lava(d).name = 'lava'), /name must be namespaced/],
    [(d) => d.fluids.push({ ...lava(d) }), /fluids\.dwell:lava: listed twice/],
    [(d) => (d.vents = [{ name: 'a:v', fluid: 'a:none', kind: 'lake', x: 0, z: 0, rateM3s: 1, levelM: 1 }]), /vents\.a:v\.fluid: "a:none" is not a hot fluid/],
    [(d) => (d.vents = [{ name: 'a:v', fluid: 'dwell:lava', kind: 'geyser', x: 0, z: 0, rateM3s: 1 }]), /vents\.a:v\.kind: "geyser" is not one of lake, river/],
    [(d) => (d.vents = [{ name: 'a:v', fluid: 'dwell:lava', kind: 'lake', x: 0, z: 0, rateM3s: 1 }]), /vents\.a:v\.levelM must be a number/],
    [(d) => (d.vents = [{ name: 'a:v', fluid: 'dwell:lava', kind: 'river', x: 0, z: 0, rateM3s: 1, levelM: 3 }]), /only a lake vent holds a level/],
  ]) {
    const d = data();
    change(d);
    assert.throws(() => validateLava(d), message);
  }
});
