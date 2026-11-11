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
    [(d) => (d.steam.color = 'white'), /steam\.color must be a colour/],
    [(d) => (d.steam.sizeM = [2, 1]), /steam\.sizeM must be/],
    [(d) => (d.steam.glow = 1), /steam: unknown field glow/],
    [(d) => (d.burnEdge.opacity = 2), /burnEdge\.opacity must be a number in \[0, 1\]/],
  ]) {
    const d = data();
    change(d);
    assert.throws(() => validateLava(d), message);
  }
});

test("the clients' looks (steam, the burn edge) come from data, defaulted for a file without them", () => {
  const d = data();
  d.steam.color = '#ffffff';
  d.burnEdge.seconds = 2;
  const src = lavaRust(validateLava(d));
  assert.match(src, /steam: SteamLook \{ color: "#ffffff"/);
  assert.match(src, /burn_edge: BurnEdge \{ color: "#ff5a1e", opacity: 0\.6, seconds: 2\.0 \}/);
  delete d.steam;
  delete d.burnEdge;
  assert.match(lavaRust(validateLava(d)), /steam: SteamLook \{ color: "#f2f4f6"/);
});

test('heat stops: a stop added by data alone is generated, bad stops fail naming them', () => {
  const d = data();
  d.heat.unshift({ name: 'mymod_white', above: 0.95, color: '#fff8e0', intensity: 5, crust: 0 });
  assert.match(lavaRust(validateLava(d)), /HeatStop \{ name: "mymod_white", above: 0\.95, color: "#fff8e0", intensity: 5\.0, crust: 0\.0 \}/);
  for (const [change, message] of [
    [(d) => (d.heat[1].above = 0.9), /heat\.warm\.above must be below the hotter stop's/],
    [(d) => (d.heat[3].above = 0.05), /coolest stop's above must be 0/],
    [(d) => (d.heat[0].crust = 2), /heat\.hot\.crust must be a number in \[0, 1\]/],
    [(d) => (d.heat[0].color = 'red'), /heat\.hot\.color must be a colour/],
    [(d) => (d.heat[1].name = 'hot'), /heat\.hot: listed twice/],
    [(d) => (d.heat = []), /heat must be a non-empty list/],
  ]) {
    const bad = data();
    change(bad);
    assert.throws(() => validateLava(bad), message);
  }
});

test("the hottest stop's emissive is over 1, so it glows past white and blooms", () => {
  assert.ok(data().heat[0].intensity > 1);
});
