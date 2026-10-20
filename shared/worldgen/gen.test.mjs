// The terrain data's validation and generation (decorations and the height field) (node --test shared/worldgen/gen.test.mjs).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  decorationsRust,
  placementsRust,
  registryBlocks,
  registryStructures,
  terrainRust,
  validateDecorations,
  validatePlacements,
  validateTerrain,
} from './gen.mjs';

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
  ];
  for (const [edit, message] of cases) {
    const t = loadTerrain();
    edit(t);
    assert.throws(() => validateTerrain(t), message);
  }
});

const loadPlacements = () => JSON.parse(readFileSync(join(here, 'placements.json'), 'utf8'));
const structures = registryStructures();

test('the checked-in placements are valid and generate the checked-in Rust', () => {
  const rs = placementsRust(validatePlacements(loadPlacements(), blocks, structures));
  const checkedIn = readFileSync(join(here, '../../crates/dwell-worldgen/src/placements.gen.rs'), 'utf8');
  assert.equal(rs, checkedIn, 'run `node shared/worldgen/gen.mjs`');
});

test('a placement added by data alone is emitted', () => {
  const d = loadPlacements();
  d.placements.push({
    name: 'rock_scatter',
    structures: { 'dwell:proto_rock': 1 },
    on: ['dwell:grass', 'dwell:dirt'],
    footprint: 'anchor',
    grid: 24,
    density: 0.2,
    height: [0, 600],
    maxSlopeDeg: 30,
    spacing: 0,
    rotate: [0, 180],
  });
  const rs = placementsRust(validatePlacements(d, blocks, structures));
  assert.match(rs, /PLACEMENTS: \[Placement; 2\]/);
  assert.match(
    rs,
    /name: "rock_scatter",\n\s+structures: &\[\("dwell:proto_rock", 1\)\],\n\s+on: &\["dwell:grass", "dwell:dirt"\],\n\s+footprint_on: false,\n\s+grid: 24,/,
  );
  // No groves: everywhere; no rise: flat; 30° as a squared gradient (tan² 30° = 1/3); quarter turns.
  assert.match(rs, /grove_wavelength: 0,\n\s+grove_coverage: 1\.0,\n\s+grove_outside: 1\.0,/);
  assert.match(rs, /rise_full: 0,\n\s+rise_low: 1\.0,\n\s+max_gradient_sq: 0\.33333334,/);
  assert.match(rs, /rotations: &\[0, 2\],/);
});

test('a bad placement fails with its name and the problem', () => {
  const cases = [
    [(p) => (p.structures = { 'dwell:oak': 1 }), /pine_groves: structure dwell:oak is not in shared\/structures/],
    [(p) => (p.structures = {}), /structures must map structure names to weights/],
    [(p) => (p.structures['dwell:pine_tree'] = 0.5), /weight must be a whole number/],
    [(p) => (p.on = ['dwell:moss']), /on names unknown block dwell:moss/],
    [(p) => (p.on = ['dwell:water']), /on block dwell:water is not a full block/],
    [(p) => (p.footprint = 'crown'), /footprint must be one of on, anchor/],
    [(p) => (p.grid = 1), /grid must be a whole number of metres in \[2, 256\]/],
    [(p) => (p.density = 1.5), /density must be in \(0, 1\]/],
    [(p) => (p.groves.wavelength = 4), /groves\.wavelength/],
    [(p) => (p.groves.outside = -0.1), /groves\.outside must be in \[0, 1\]/],
    [(p) => (p.height = [500, 400]), /height must be \[min, max\]/],
    [(p) => (p.rise.full = 900), /rise\.full must be a whole number of metres in \(height\[0\], height\[1\]\]/],
    [(p) => (p.maxSlopeDeg = 90), /maxSlopeDeg must be in \(0, 90\) degrees/],
    [(p) => (p.spacing = -1), /spacing must be a whole number of blocks/],
    [(p) => (p.rotate = [0, 45]), /rotate must be distinct angles from 0, 90, 180, 270/],
    [(p) => (p.mirror = ['x']), /pine_groves: unknown mirror/],
  ];
  for (const [edit, message] of cases) {
    const d = loadPlacements();
    edit(d.placements[0]);
    assert.throws(() => validatePlacements(d, blocks, structures), message);
  }
  const d = loadPlacements();
  d.placements.push({ ...d.placements[0] });
  assert.throws(() => validatePlacements(d, blocks, structures), /duplicate name/);
});
