// The cannons table (docs/CANNONS.md): validation and output.
import assert from 'node:assert/strict';
import test from 'node:test';
import { cannonsRust, loadCannons, loadNames, validateCannons } from './gen.mjs';

test("the build's cannon and its two balls validate and reach the Rust table", () => {
  const d = validateCannons(loadCannons(), loadNames());
  const rs = cannonsRust(d);
  assert.match(rs, /CannonDef \{ block: "dwell:cannon", ball: "dwell:iron_ball", muzzle_speed_ms: 45\.0/);
  assert.match(rs, /BallDef \{ name: "dwell:iron_ball", radius_m: 0\.24, density_kg_m3: 7850\.0, roll_damping: 1\.5, impact: Impact \{ destroy_speed_ms: 20\.0,/);
});

test('a fixture cannon and ball by data alone; bad entries fail naming them', () => {
  const names = loadNames();
  names.blocks.set('test:mortar', { use: 'fire' });
  const data = loadCannons();
  names.blocks.set('test:stone_ball', { look: 'ball' });
  data.balls.push({ name: 'test:stone_ball', radiusM: 0.2, densityKgM3: 2400, rollDamping: 2, impact: { destroySpeedMs: 30, energyPerKgJ: 10, dragRadiusM: 0.5, dragForcePerSpeedN: 100, maxLooseCells: 4, minSpeedMs: 2 } });
  names.blocks.set('test:tiller', { use: 'helm' });
  data.helms.push({ block: 'test:tiller', thrustAccelMs2: 0.5, turnRadS2PerMs: 0.05, camera: { backM: 3, upM: 1, easeS: 0.2 } });
  assert.match(cannonsRust(validateCannons(data, names)), /HelmDef \{ block: "test:tiller", thrust_accel_ms2: 0\.5/);
  data.cannons.push({ block: 'test:mortar', ball: 'test:stone_ball', muzzleSpeedMs: 20, muzzleUpM: 1, muzzleForwardM: 0.5, recoilShare: 0.5, reloadS: 6, fire: structuredClone(data.cannons[0].fire) });
  assert.match(cannonsRust(validateCannons(data, names)), /CannonDef \{ block: "test:mortar", ball: "test:stone_ball"/);
  const bad = (edit, re) => {
    const d = loadCannons();
    edit(d);
    assert.throws(() => validateCannons(d, loadNames()), re);
  };
  bad((d) => (d.cannons[0].ball = 'dwell:nope'), /cannons\[0\]\.ball: no ball "dwell:nope"/);
  bad((d) => (d.cannons[0].block = 'dwell:stone'), /must have use: fire/);
  bad((d) => (d.cannons[0].reloadS = -1), /cannons\[0\]\.reloadS must be a number/);
  bad((d) => (d.balls[0].impact.kind = 'splash'), /impact: unknown field kind/);
  bad((d) => (d.balls[0].impact.minSpeedMs = 50), /minSpeedMs must not exceed destroySpeedMs/);
  bad((d) => (d.balls[0].name = 'dwell:stone'), /must be a block of look ball/);
  bad((d) => (d.balls[0].densityKgM3 = 5), /densityKgM3 must be a number/);
  bad((d) => (d.cannons[0].speed = 1), /unknown field speed/);
  bad((d) => (d.cannons[0].fire.flash.color = 'red'), /fire\.flash\.color must be #rrggbb/);
  bad((d) => (d.helms[0].block = 'dwell:cannon'), /must have use: helm/);
  bad((d) => (d.helms[0].camera.easeS = 9), /camera\.easeS must be a number/);
  bad((d) => (d.helms[0].thrustAccelMs2 = -1), /helms\[0\]\.thrustAccelMs2 must be a number/);
});
