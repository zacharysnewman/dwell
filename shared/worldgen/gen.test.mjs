// The terrain data's validation and generation (decorations, the height field, placements and the hydrology profiles) (node --test shared/worldgen/gen.test.mjs).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  decorationsRust,
  hydrologyRust,
  placementsRust,
  registryBlocks,
  registryStructures,
  terrainRust,
  validateDecorations,
  validateHydrology,
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
    [(t) => (t.steps.height = 0.75), /steps\.height must be a whole number of cells/],
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

// ---- The hydrology prototype's profiles ----------------------------------------------------------

const loadHydrology = () => JSON.parse(readFileSync(join(here, 'hydrology.json'), 'utf8'));

test('the checked-in hydrology profiles are valid and generate the checked-in Rust', () => {
  const rs = hydrologyRust(validateHydrology(loadHydrology()));
  const checkedIn = readFileSync(join(here, '../../crates/dwell-worldgen/src/hydrology/profiles.gen.rs'), 'utf8');
  assert.equal(rs, checkedIn, 'run `node shared/worldgen/gen.mjs`');
});

test('a hydrology profile added by data alone is emitted and can be the default', () => {
  const d = loadHydrology();
  const p = structuredClone(d.profiles[0]);
  p.name = 'coarse_wet';
  p.grid.cell = 250;
  p.rain.amount = 2.5;
  p.lakes.smallPits = 'fill';
  p.rivers.geometry.push({ discharge: 5000, width: 400, depth: 12 });
  d.profiles.push(p);
  d.default = 'coarse_wet';
  const rs = hydrologyRust(validateHydrology(d));
  assert.match(rs, /PROFILES: \[HydrologyParams; 7\]/);
  assert.match(rs, /DEFAULT: &str = "coarse_wet"/);
  assert.match(rs, /name: "coarse_wet",\n        cell: 250,/);
  assert.match(rs, /rainfall: 2\.5,/);
  assert.match(rs, /small_pits: SmallPits::Fill,/);
  assert.match(rs, /RiverPoint \{ discharge: 5000\.0, width: 400\.0, depth: 12\.0 \}/);
});

test('an uplift layer, rock bands and orographic rain added by data alone are emitted', () => {
  const d = loadHydrology();
  const p = structuredClone(d.profiles.find((q) => q.name === 'medium'));
  p.name = 'two_ranges';
  p.uplift = {
    rate: 0.001,
    baseHeight: 800,
    keepRelief: 0,
    plains: 0.2,
    shelf: { min: 500, max: 500, wavelength: 1000 },
    ranges: {
      count: 2,
      length: { min: 10000, max: 20000 },
      width: 3000,
      strength: 1.5,
      wander: 0.5,
      warp: { amplitude: 0, wavelength: 1000 },
      minCoast: 1000,
      maxCoast: 5000,
      align: 1,
    },
  };
  p.rockBands = { contrast: 0.4, wavelength: 2000, hardShare: 0.5, warp: { amplitude: 100, wavelength: 1000 } };
  p.rain = { model: 'orographic', amount: 2, direction: 45, spread: 0, upslope: 3, fetch: 9000, shadow: 0.5, shadowHeight: 300, min: 0.5, max: 2 };
  p.rivers.mouths.rule = 'coastRelief';
  d.profiles.push(p);
  const rs = hydrologyRust(validateHydrology(d));
  assert.match(rs, /name: "two_ranges",[\s\S]*tectonics: Some\(Tectonics \{\n            rate: 0\.001,\n            base_height: 800\.0,/);
  assert.match(rs, /ranges: Ranges \{\n                count: 2,\n                length_min: 10000\.0,/);
  assert.match(rs, /apron_strength: 0\.0,/);
  assert.match(rs, /plates: Plates \{\n                count: 0,/);
  assert.match(rs, /rock_bands: Some\(RockBands \{\n            contrast: 0\.4,/);
  assert.match(rs, /rain: Rain::Orographic\(Orographic \{\n            direction: 45\.0,/);
  assert.match(rs, /mouth_rule: MouthRule::CoastRelief,/);
});

test('a bad hydrology profile fails with its name, the key and the problem', () => {
  const cases = [
    [(p) => (p.grid.cell = 101), /medium: grid\.cell must be an even number/],
    [(p) => (p.grid.cell = 10), /medium: grid\.cell must be a whole number in \[25, 2000\]/],
    [(p) => (p.erosion.iterations = 1.5), /medium: erosion\.iterations must be a whole number/],
    [(p) => (p.erosion.slopeExponent = 3), /medium: erosion\.slopeExponent must be in \[0\.5, 2\]/],
    [(p) => (p.erosion.lakeFill = 2), /medium: erosion\.lakeFill must be in \[0, 1\]/],
    [(p) => (p.erosion.talus.passes = -1), /medium: erosion\.talus\.passes/],
    [(p) => (p.erosion.speed = 1), /medium: erosion: unknown speed/],
    [(p) => (p.rain.model = 'monsoon'), /medium: rain\.model must be one of uniform, orographic/],
    [(p) => (p.rain.model = 'orographic'), /medium: rain\.direction must be in/],
    [(p) => (p.rivers.mouths.rule = 'tides'), /medium: rivers\.mouths\.rule must be one of riverSlope, coastRelief/],
    [(p) => (p.lakes.smallPits = 'drain'), /medium: lakes\.smallPits must be one of fill, breach/],
    [(p) => (p.rivers.threshold = 0), /medium: rivers\.threshold must be in \(0/],
    [(p) => p.rivers.geometry.reverse(), /medium: rivers\.geometry discharges must increase/],
    [(p) => (p.rivers.geometry[1].width = 1), /widths and depths must not decrease/],
    [(p) => (p.rivers.mouths = {}), /medium: rivers\.mouths\.minDischarge/],
    [(p) => (p.biomes = []), /medium: unknown biomes/],
  ];
  for (const [edit, message] of cases) {
    const d = loadHydrology();
    edit(d.profiles.find((p) => p.name === 'medium'));
    assert.throws(() => validateHydrology(d), message);
  }
  const upliftCases = [
    [(p) => (p.uplift.rate = -1), /uplift: uplift\.rate must be in \[0, 0\.1\]/],
    [(p) => delete p.uplift.shelf, /uplift: uplift\.shelf must be an object/],
    [(p) => (p.uplift.shelf.min = 9000), /uplift: uplift\.shelf\.min must not exceed/],
    [(p) => (p.uplift.ranges.count = 2.5), /uplift: uplift\.ranges\.count must be a whole number/],
    [(p) => (p.uplift.ranges.length.min = 1e6), /uplift: uplift\.ranges\.length\.min must not exceed its max/],
    [(p) => (p.uplift.ranges.align = 2), /uplift: uplift\.ranges\.align must be in \[0, 1\]/],
    [(p) => (p.uplift.ranges.peaks = 3), /uplift: uplift\.ranges: unknown peaks/],
    [(p) => (p.uplift.apron.width = 0), /uplift: uplift\.apron\.width must be in \(0/],
    [(p) => (p.uplift.plates = { count: 1, width: 1, strength: 1, rift: 0, warp: { amplitude: 0, wavelength: 100 } }), /plates\.count must be 0 or at least 2/],
    [(p) => (p.rockBands = { contrast: 1, wavelength: 1, hardShare: 0, warp: { amplitude: 0, wavelength: 100 } }), /uplift: rockBands\.contrast must be in \[0, 0\.99\]/],
  ];
  for (const [edit, message] of upliftCases) {
    const d = loadHydrology();
    edit(d.profiles.find((p) => p.name === 'uplift'));
    assert.throws(() => validateHydrology(d), message);
  }
  const d = loadHydrology();
  d.default = 'nope';
  assert.throws(() => validateHydrology(d), /default names no profile \(nope\)/);
  const e = loadHydrology();
  e.profiles.push(structuredClone(e.profiles[0]));
  assert.throws(() => validateHydrology(e), /gentle: duplicate name/);
});
