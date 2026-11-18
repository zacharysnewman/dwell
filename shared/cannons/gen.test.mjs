// The cannons table (docs/CANNONS.md): validation and output.
import assert from 'node:assert/strict';
import test from 'node:test';
import { cannonsRust, loadCannons, loadNames, validateCannons } from './gen.mjs';

test("the build's cannon and its two balls validate and reach the Rust table", () => {
  const d = validateCannons(loadCannons(), loadNames());
  const rs = cannonsRust(d);
  assert.match(rs, /CannonDef \{ block: "dwell:cannon", ball: "dwell:iron_ball", muzzle_speed_ms: 45\.0/);
  assert.match(rs, /BallDef \{ name: "dwell:iron_ball", radius_m: 0\.12, density_kg_m3: 7850\.0, roll_damping: 1\.5, impact: Impact::Knock \{/);
  assert.match(rs, /impact: Impact::Explode \{ explosive: "dwell:dynamite" \}/);
});

test('a fixture cannon and ball by data alone; bad entries fail naming them', () => {
  const names = loadNames();
  names.blocks.set('test:mortar', { use: 'fire' });
  const data = loadCannons();
  data.balls.push({ name: 'test:stone_ball', radiusM: 0.2, densityKgM3: 2400, rollDamping: 2, impact: { kind: 'knock', knockRadiusM: 1.5, maxCells: 12, minSpeedMs: 4 } });
  data.cannons.push({ block: 'test:mortar', ball: 'test:stone_ball', muzzleSpeedMs: 20, muzzleUpM: 1, muzzleForwardM: 0.5, reloadS: 6 });
  assert.match(cannonsRust(validateCannons(data, names)), /CannonDef \{ block: "test:mortar", ball: "test:stone_ball"/);
  const bad = (edit, re) => {
    const d = loadCannons();
    edit(d);
    assert.throws(() => validateCannons(d, loadNames()), re);
  };
  bad((d) => (d.cannons[0].ball = 'dwell:nope'), /cannons\[0\]\.ball: no ball "dwell:nope"/);
  bad((d) => (d.cannons[0].block = 'dwell:stone'), /must have use: fire/);
  bad((d) => (d.cannons[0].reloadS = 0), /cannons\[0\]\.reloadS must be a number/);
  bad((d) => (d.balls[0].impact.kind = 'splash'), /impact\.kind must be one of knock, explode/);
  bad((d) => (d.balls[1].impact.explosive = 'dwell:tnt'), /no explosive "dwell:tnt"/);
  bad((d) => (d.balls[0].densityKgM3 = 5), /densityKgM3 must be a number/);
  bad((d) => (d.cannons[0].speed = 1), /unknown field speed/);
});
