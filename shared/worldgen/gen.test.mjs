// The terrain data's validation and generation (decorations and the height field) (node --test shared/worldgen/gen.test.mjs).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { decorationsRust, registryBlocks, terrainRust, validateDecorations, validateTerrain } from './gen.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const load = () => JSON.parse(readFileSync(join(here, 'decorations.json'), 'utf8'));
const blocks = registryBlocks();

test('the checked-in decorations are valid and generate the checked-in Rust', () => {
  const rs = decorationsRust(validateDecorations(load(), blocks));
  const checkedIn = readFileSync(join(here, '../../crates/dwell-worldgen/src/decorations.gen.rs'), 'utf8');
  assert.equal(rs, checkedIn, 'run `node shared/worldgen/gen.mjs`');
});

test('an entry added by data alone is emitted', () => {
  const d = load();
  d.decorations.push({
    name: 'dirt_tufts',
    block: 'dwell:tall_grass',
    on: ['dwell:dirt'],
    density: 0.1,
    maxSlope: 2,
    height: [0, 50],
  });
  const rs = decorationsRust(validateDecorations(d, blocks));
  assert.match(rs, /DECORATIONS: \[Decoration; 2\]/);
  assert.match(rs, /name: "dirt_tufts", block: "dwell:tall_grass", on: &\["dwell:dirt"\], density: 0\.1, patch_wavelength: 0, patch_coverage: 1\.0, max_slope: 2, min_height: 0, max_height: 50/);
});

test('a bad entry fails with its name and the problem', () => {
  const cases = [
    [(d) => (d.decorations[0].block = 'dwell:moss'), /meadow_grass: block dwell:moss is not in the registry/],
    [(d) => (d.decorations[0].block = 'dwell:stone'), /must have shape empty/],
    [(d) => (d.decorations[0].on = ['dwell:sand']), /may not stand on dwell:sand/],
    [(d) => (d.decorations[0].on = ['dwell:water']), /is not a full block/],
    [(d) => (d.decorations[0].density = 0), /density must be in \(0, 1\]/],
    [(d) => (d.decorations[0].patches.wavelength = 2), /patches\.wavelength/],
    [(d) => (d.decorations[0].maxSlope = 1.5), /maxSlope must be a whole number/],
    [(d) => (d.decorations[0].height = [10, 5]), /height must be \[min, max\]/],
    [(d) => (d.decorations[0].rarity = 1), /unknown rarity/],
    [(d) => d.decorations.push({ ...d.decorations[0] }), /duplicate name/],
  ];
  for (const [edit, message] of cases) {
    const d = load();
    edit(d);
    assert.throws(() => validateDecorations(d, blocks), message);
  }
});

const loadTerrain = () => JSON.parse(readFileSync(join(here, 'terrain.json'), 'utf8'));

test('the checked-in height field is valid and generates the checked-in Rust', () => {
  const rs = terrainRust(validateTerrain(loadTerrain()));
  const checkedIn = readFileSync(join(here, '../../crates/dwell-worldgen/src/terrain.gen.rs'), 'utf8');
  assert.equal(rs, checkedIn, 'run `node shared/worldgen/gen.mjs`');
});

test('a height-field value changed by data alone is emitted', () => {
  const t = loadTerrain();
  t.hills.amplitude = 75;
  t.bumps.wavelength = 256;
  t.seaFloor.depth = 12.5;
  const rs = terrainRust(validateTerrain(t));
  assert.match(rs, /hill_height: 75\.0,/);
  assert.match(rs, /bump_wavelength: 256,/);
  assert.match(rs, /sea_floor: -12\.5,/);
});

test('a bad height field fails with the key and the problem', () => {
  const cases = [
    [(t) => (t.hills.amplitude = -1), /hills\.amplitude must be in \[0, 1000\]/],
    [(t) => (t.hills.octaves = 2.5), /hills\.octaves must be a whole number in \[1, 8\]/],
    [(t) => (t.inland.halfDistance = 0), /inland\.halfDistance must be in \(0, 100000\]/],
    [(t) => (t.bumps.wavelength = 2), /bumps\.wavelength/],
    [(t) => (t.hills.fadeTo = 100), /hills\.fadeTo must be greater than hills\.fadeFrom/],
    [(t) => (t.hills.roughness = 1), /hills: unknown roughness/],
    [(t) => (t.mountains = {}), /unknown mountains/],
    [(t) => delete t.seaFloor, /seaFloor must be an object/],
    [(t) => (t.steps.height = 0.75), /steps\.height must be a whole number of cells/],
  ];
  for (const [edit, message] of cases) {
    const t = loadTerrain();
    edit(t);
    assert.throws(() => validateTerrain(t), message);
  }
});
