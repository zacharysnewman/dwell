// node --test shared/look/waterflow.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { liquidNames, validateWaterFlow, waterFlowRust } from './waterflow.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'waterflow.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in data validates and generates the checked-in Rust', () => {
  assert.equal(waterFlowRust(validateWaterFlow(data())), checkedIn('crates/dwell-look/src/waterflow.gen.rs'));
  assert.deepEqual(liquidNames(data()), ['dwell:water']);
});

test('a liquid added by data alone reaches the Rust table (a fixture oil, slow and dark)', () => {
  const d = data();
  d.liquids['test:oil'] = {
    ripples: { advect: 0.5, cycleS: 4 },
    shore: { bands: 1, periodS: 9, lineM: 0, featherM: 0.1, widthM: 0.3, bandSoftness: 0.5, strength: 0.4, wobble: 0, cornerRadiusM: 0.3 },
    foam: { fromMs: 1, toMs: 3, color: '#403010', strength: 0.2, lengthM: 2, widthM: 0.5 },
    particles: {
      count: 0,
      cpuCount: 0,
      fromMs: 1,
      fullMs: 2,
      sizeM: 0.1,
      stretch: 1,
      color: '#201808',
      opacity: 0.8,
      lifeS: 10,
      fadeInS: 1,
      fadeOutS: 1,
      radiusM: 10,
      ragged: 0,
      lumps: 0,
      minPixels: 2,
    },
  };
  const rust = waterFlowRust(validateWaterFlow(d));
  assert.match(rust, /fluid: "test:oil",\n {8}advect: 0\.5,/);
  assert.match(rust, /LIQUID_FLOWS: \[LiquidFlowLook; 2\]/);
});

test('bad entries fail with their path', () => {
  const bad = (edit, re) => {
    const d = data();
    edit(d);
    assert.throws(() => validateWaterFlow(d), re);
  };
  bad((d) => (d.windowColumns = 100), /power of two/);
  bad((d) => (d.fadeToM = 90), /within half the window/);
  bad((d) => (d.liquids['dwell:water'].foam.toMs = 0.04), /foam\.toMs must be above/);
  bad((d) => (d.liquids['dwell:water'].foam.lengthM = 2), /foam\.lengthM must be at most 4\.5/);
  bad((d) => (d.liquids['dwell:water'].ripples.speed = 1), /unknown entry liquids\.dwell:water\.ripples\.speed/);
  bad((d) => delete d.liquids['dwell:water'].particles.color, /missing entry liquids\.dwell:water\.particles\.color/);
  bad((d) => (d.liquids.Water = d.liquids['dwell:water']), /namespace:name/);
  bad((d) => (d.liquids['dwell:water'].particles.cpuCount = 999), /cpuCount must be at most count/);
  const p = (edit) => (d) => edit(d.liquids['dwell:water'].particles);
  bad(p((q) => (q.fullMs = q.fromMs)), /particles\.fullMs must be above particles\.fromMs/);
  bad(p((q) => (q.fadeInS = q.lifeS)), /fadeInS \+ fadeOutS must be at most lifeS/);
  bad(p((q) => (q.opacity = 0)), /particles\.opacity must be a number in \(0, 1\]/);
  bad(p((q) => (q.opacity = 1.5)), /particles\.opacity must be a number/);
  bad(p((q) => (q.stretch = 0.5)), /particles\.stretch must be a number in \[1, 8\]/);
  bad(p((q) => (q.lumps = 2.5)), /particles\.lumps must be a whole number/);
  bad(p((q) => (q.ragged = 2)), /particles\.ragged must be a number in \[0, 1\]/);
  bad(p((q) => delete q.minPixels), /missing entry liquids\.dwell:water\.particles\.minPixels/);
  const s = (edit) => (d) => edit(d.liquids['dwell:water'].shore);
  bad(s((q) => (q.bands = 1.5)), /shore\.bands must be a whole number/);
  bad(s((q) => (q.bands = 9)), /shore\.bands must be/);
  bad(s((q) => delete q.periodS), /missing entry liquids\.dwell:water\.shore\.periodS/);
  bad(s((q) => (q.featherM = 2)), /shore\.featherM must be a number in \[0, 1\]/);
  bad(s((q) => delete q.bandSoftness), /missing entry liquids\.dwell:water\.shore\.bandSoftness/);
});

test('shore foam is pulsing bands with a soft, even contact edge (owner, 2026-11-10, 2026-11-13)', () => {
  const s = validateWaterFlow(data()).liquids['dwell:water'].shore;
  assert.ok(s.bands >= 2 && s.bands <= 3 && s.widthM > s.lineM, JSON.stringify(s));
  // the edge feathers into the water over more than its solid core, and the bands are soft-sided
  assert.ok(s.featherM >= 0.1 && s.featherM > s.lineM && s.bandSoftness >= 0.5, JSON.stringify(s));
  // slow: a band takes seconds to roll out, and the noise only varies it (no holes)
  assert.ok(s.periodS >= 3 && s.wobble < 1 && s.strength > 0, JSON.stringify(s));
});

test("the flecks read as foam, not white discs (owner, 2026-11-06)", () => {
  const p = validateWaterFlow(data()).liquids['dwell:water'].particles;
  // Smaller than the 0.1 m discs, translucent, drawn out along the flow, broken at the edge, and
  // not pure white: no channel at full, tinted toward the water.
  assert.ok(p.sizeM < 0.1 && p.opacity < 1 && p.stretch > 1 && p.ragged > 0, JSON.stringify(p));
  const [r, g, b] = [1, 3, 5].map((k) => parseInt(p.color.slice(k, k + 2), 16));
  assert.ok(Math.max(r, g, b) < 0xe0 && b >= r, p.color);
  // Faded in and out over seconds, not popped.
  assert.ok(p.fadeInS >= 1 && p.fadeOutS >= 1, JSON.stringify(p));
});

test('slow creeks show their flow (owner, 2026-11-06)', () => {
  // A creek of ~0.1 m/s (seed 0's dam site) showed no flecks (from 0.3 m/s) or foam (0.4 m/s);
  // both begin under 0.1 m/s, still above water that isn't moving.
  const w = validateWaterFlow(data()).liquids['dwell:water'];
  assert.ok(w.particles.fromMs < 0.1 && w.particles.fromMs > 0, JSON.stringify(w.particles));
  assert.ok(w.foam.fromMs < 0.1 && w.foam.fromMs > 0, JSON.stringify(w.foam));
  // The foam streaks were the largest discs: 2 m of taps over 0.3 m noise broke into round white
  // blobs. Now thin dashes along the flow (at least three times as long as wide, the taps joined),
  // and not pure white.
  const f = validateWaterFlow(data()).liquids['dwell:water'].foam;
  assert.ok(f.lengthM >= 3 * f.widthM && f.lengthM <= 4.5 * f.widthM, JSON.stringify(f));
  const [fr, fg, fb] = [1, 3, 5].map((k) => parseInt(f.color.slice(k, k + 2), 16));
  assert.ok(Math.max(fr, fg, fb) < 0xe8 && f.strength <= 0.4, JSON.stringify(f));
});
