// node --test shared/look/underwater.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { underwaterRust, validateUnderwater } from './underwater.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'underwater.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in looks validate and generate the checked-in Rust', () => {
  assert.equal(underwaterRust(validateUnderwater(data())), checkedIn('crates/dwell-look/src/underwater.gen.rs'));
});

test('a murky fluid added by data alone reaches the output', () => {
  const d = data();
  d.fluids['test:mud'] = {
    lensM: 0.1,
    line: { amplitudeM: 0, wavelengthM: 1, speed: 0 },
    band: {
      depthM: 0, tint: '#806040', streaks: 0, glints: 0, rimPx: 0, rimShade: 1, linePx: 0, lineGlow: 1,
      breakup: 0, mirror: 0.6, mirrorWarpPx: 0,
    },
    underside: { texture: 0, breakup: 0, sunPatch: 0, sunPower: 8, cellScale: 1 },
    window: { cells: 0.5, bodies: 0 },
    fog: { clearM: 0, density: 4 },
  };
  const rs = underwaterRust(validateUnderwater(d));
  assert.match(rs, /fluid: "test:mud",[\s\S]*fog_density: 4\.0,/);
});

test('bad entries fail with their path named', () => {
  const w = 'dwell:water';
  const cases = [
    [(d) => (d.fluids[w].fog.density = 0), /fluids\.dwell:water\.fog\.density must be a number in \(0, 16\]/],
    [(d) => (d.fluids[w].fog.clearM = -1), /fluids\.dwell:water\.fog\.clearM must be a number/],
    [(d) => (d.fluids[w].band.streaks = 2), /band\.streaks must be a number in \[0, 1\]/],
    [(d) => (d.fluids[w].band.tint = 'white'), /band\.tint must be #rrggbb/],
    [(d) => (d.fluids[w].band.lineGlow = 0.5), /band\.lineGlow must be a number in \[1, 4\]/],
    [(d) => (d.fluids[w].underside.texture = -1), /underside\.texture must be a number/],
    [(d) => (d.fluids[w].underside.sunPower = 0), /underside\.sunPower must be a number in \[1, 256\]/],
    [(d) => (d.fluids[w].window.cells = 2), /window\.cells must be a number in \[0, 1\]/],
    [(d) => delete d.fluids[w].window, /window/],
    [(d) => (d.fluids[w].band.mirrorWarpPx = 40), /band\.mirrorWarpPx must be a number in \[0, 32\]/],
    [(d) => ((d.fluids[w].lensM = 0.05), (d.fluids[w].line.amplitudeM = 0.1)), /line\.amplitudeM must be under lensM/],
    [(d) => (d.fluids[w].lensM = 0), /lensM must be a number in \[0\.01, 2\]/],
    [(d) => (d.fluids.Water = d.fluids[w]), /fluids\.Water: the name must be/],
    [(d) => (d.default.fog.extra = 1), /unknown entry default\.fog\.extra/],
    [(d) => delete d.fluids[w].band, /missing entry fluids\.dwell:water\.band/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateUnderwater(d), message);
  }
});

test("water's waterline is a band of real thickness, a dark line over a light one (owner, 2026-11-07)", () => {
  const w = data().fluids['dwell:water'];
  // 2.5–4 % of the screen's height at the default FOV (the lens plane spans lensM × 2 × 0.745 m):
  // 5–8 % until 0.50.23, halved then (owner: "halve the width").
  const share = w.band.depthM / (w.lensM * 2 * 0.745);
  assert.ok(share >= 0.025 && share <= 0.04, `${share}`);
  assert.ok(w.band.rimShade < 1 && w.band.lineGlow > 1, 'darker over lighter');
  assert.ok(w.underside.texture > 0, 'the underside has a texture');
  // Owner's references (2026-11-07): broken bright cells, brightest toward the sun; the band's
  // ripples broken and its mirror of the land not literal.
  assert.ok(w.underside.breakup > 0.5 && w.underside.sunPatch > 0, 'broken cells, a sun patch');
  assert.ok(w.band.breakup > 0 && w.band.mirrorWarpPx > 0 && w.band.mirror < 0.6, 'a rippled mirror');
});

test("water's band is half as deep and smoother (owner, 2026-11-07: 'a little smoother … halve the width')", () => {
  const b = data().fluids['dwell:water'].band;
  assert.equal(b.depthM, 0.021);
  assert.ok(b.breakup <= 0.35 && b.glints <= 0.15 && b.mirrorWarpPx <= 10);
});
