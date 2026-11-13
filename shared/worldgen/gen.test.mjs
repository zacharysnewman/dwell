// The terrain data's validation and generation (decorations, the height field, placements and the hydrology profiles) (node --test shared/worldgen/gen.test.mjs).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  decorationsRust,
  hydrologyRust,
  midplaneRust,
  platesRust,
  climateRust,
  validateClimate,
  placementsRust,
  rocksRust,
  validateRocks,
  vegetationRust,
  validateVegetation,
  registryBlocks,
  registryStructures,
  terrainRust,
  loadBiomes,
  validateDecorations,
  validateHydrology,
  validateMidplane,
  validatePlates,
  validatePlacements,
  validateTerrainAndBiomes,
  validateBiome,
  validateFixtureWorlds,
  validateFixtureBiomes,
  validatePresetBiomes,
  validateVolume,
  volumeRust,
  validateGrass,
  grassRust,
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
  assert.ok(rs.includes(`DECORATIONS: [Decoration; ${d.decorations.length}]`));
  assert.match(rs, /name: "dirt_tufts", block: "dwell:tall_grass", on: &\["dwell:dirt"\], density: 0\.1, patch_wavelength: 0, patch_coverage: 1\.0, max_slope: 2, min_height: 0, max_height: 50, on_ramps: false/);
});

test('a rule on ramps needs its ground to have ramps its block may stand on', () => {
  const d = load();
  d.decorations[0].onRamps = true;
  validateDecorations(d, blocks);
  const strict = new Map(blocks);
  const grass = strict.get('dwell:tall_grass');
  strict.set('dwell:tall_grass', { ...grass, placeOn: grass.placeOn.filter((b) => b !== 'dwell:grass_ramp') });
  assert.throws(() => validateDecorations(d, strict), /onRamps: dwell:tall_grass may not stand on dwell:grass_ramp/);
  const noRamps = new Map(blocks);
  noRamps.delete('dwell:grass_ramp');
  assert.throws(() => validateDecorations(d, noRamps), /onRamps: dwell:grass has no ramps/);
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
    [(d) => (d.decorations[0].onRamps = 1), /onRamps must be true or false/],
    [(d) => d.decorations.push({ ...d.decorations[0] }), /duplicate name/],
  ];
  for (const [edit, message] of cases) {
    const d = load();
    edit(d);
    assert.throws(() => validateDecorations(d, blocks), message);
  }
});

const loadTerrain = () => JSON.parse(readFileSync(join(here, 'terrain.json'), 'utf8'));
const loadDefault = () => JSON.parse(readFileSync(join(here, 'biomes/default.json'), 'utf8'));
const terrainOf = (world, biomes) => validateTerrainAndBiomes(world, biomes, blocks);

test('the checked-in terrain and biomes are valid and generate the checked-in Rust', () => {
  const fixtures = validateFixtureBiomes(loadBiomes(join(here, 'biomes/fixtures')), blocks);
  const worlds = validateFixtureWorlds(loadBiomes(join(here, 'worlds')), loadTerrain());
  const presets = validatePresetBiomes(loadBiomes(join(here, 'biomes/presets')), blocks);
  const rs = terrainRust({ ...terrainOf(loadTerrain(), loadBiomes()), fixtures, presets, worlds });
  const checkedIn = readFileSync(join(here, '../../crates/dwell-worldgen/src/terrain.gen.rs'), 'utf8');
  assert.equal(rs, checkedIn, 'run `node shared/worldgen/gen.mjs`');
});

test("the default biome is today's terrain: HEIGHT and SURFACE are its fields", () => {
  const rs = terrainRust(terrainOf(loadTerrain(), loadBiomes()));
  assert.match(rs, /BIOMES: &\[Biome\] = &\[\n {4}Biome \{\n {8}name: "dwell:default",/);
  assert.match(rs, /pub const HEIGHT: HeightField = HeightField \{\n {4}inland_rise: 320\.0,/);
  assert.match(rs, /pub const SURFACE: Surface = BIOMES\[0\]\.surface;/);
  assert.match(rs, /hill_height: 225\.0,/);
  assert.match(rs, /detail_mountain_amplitude: 14\.0,/);
});

test('a height-field value changed by data alone is emitted', () => {
  const w = loadTerrain();
  w.hills.amplitude = 75;
  w.bumps.wavelength = 256;
  w.seaFloor.depth = 12.5;
  const rs = terrainRust(terrainOf(w, [loadDefault()]));
  assert.match(rs, /hill_height: 75\.0,/);
  assert.match(rs, /bump_wavelength: 256,/);
  assert.match(rs, /sea_floor: -12\.5,/);
});

test('a fixture biome added by data alone is emitted after the default, which stays first', () => {
  const fixture = { ...loadDefault(), name: 'dwell:fixture_dunes', placements: [], water: 'dwell:water' };
  fixture.bands = fixture.bands.map((b, i) => (i === 0 ? { ...b, ground: 'dwell:sand' } : b));
  fixture.detail = { ...fixture.detail, amplitude: 4 };
  const w = loadTerrain();
  const t = terrainOf(w, [fixture, loadDefault()]);
  assert.deepEqual(
    t.biomes.map((b) => b.name),
    ['dwell:default', 'dwell:fixture_dunes'],
  );
  const rs = terrainRust(t);
  assert.match(rs, /name: "dwell:fixture_dunes",[\s\S]*detail_amplitude: 4\.0,[\s\S]*ground: "dwell:sand",[\s\S]*placements: &\[\],/);
  // The elevation and the water's shaping are the world's, in every biome.
  assert.equal(rs.match(/inland_rise: 320\.0,/g).length, 1);
  assert.equal(rs.match(/river_bank: 0\.5,/g).length, 2);
  // It can be the default by data alone too.
  const rs2 = terrainRust(terrainOf({ ...w, defaultBiome: 'dwell:fixture_dunes' }, [loadDefault(), fixture]));
  assert.match(rs2, /&\[\n {4}Biome \{\n {8}name: "dwell:fixture_dunes",/);
});

test('a bad biome or world terrain fails with the key and the problem', () => {
  const cases = [
    [(b) => (b.detail.amplitude = -1), /biome dwell:default: detail\.amplitude must be in \[0, 100\]/],
    [(b) => (b.detail.octaves = 2.5), /detail\.octaves must be a whole number in \[1, 8\]/],
    [(b) => (b.shaping.roughness = 1), /shaping: unknown roughness/],
    [(b) => (b.bands[0].from = 10), /bands\[0\] \(the lowest\) has no from/],
    [(b) => (b.bands[1].from = -5), /bands\[1\]\.from must be in \[0, 100000\]/],
    [(b) => b.bands.push({ ...b.bands[1], from: 100 }), /bands: snow is listed twice/],
    [(b) => b.bands.push({ ...b.bands[1], name: 'peak', from: 100 }), /bands\[2\]\.from must be higher/],
    [(b) => (b.bands[0].roughness = 1), /bands\[0\]: unknown roughness/],
    [(b) => (b.bands = []), /bands must be a list of elevation bands/],
    [(b) => (b.where.weight = 0), /where\.weight must be in \(0, 1000\]/],
    [(b) => (b.where.snowfall = { min: 0, blend: 0 }), /where: unknown snowfall/],
    [(b) => (b.where.celsius = { min: 20, max: 10, blend: 1 }), /where\.celsius\.max must be greater/],
    [(b) => (b.where.rain = { max: 500 }), /where\.rain\.blend must be/],
    [(b) => (b.where.wetness = { min: 0.5, max: 0.2, blend: 0 }), /where\.wetness\.max must be greater/],
    [(b) => (b.where.coast = { blend: 1 }), /where\.coast needs a min or a max/],
    [(b) => (b.where.elevation = { min: 0 }), /where\.elevation\.blend must be/],
    [(b) => (b.mountains = {}), /unknown mountains/],
    [(b) => delete b.shaping, /shaping must be an object/],
    [(b) => (b.hills = {}), /biome dwell:default: unknown hills/],
    [(b) => (b.detail.mountainFull = 10), /detail\.mountainFull must be greater than detail\.mountainFrom/],
    [(b) => (b.bands[1].ground = 'dwell:moss'), /bands\[1\]\.ground "dwell:moss" is not in the registry/],
    [(b) => (b.name = 'default'), /name must be "namespace:name"/],
    [(b) => (b.decorations = ['palms']), /decorations: "palms" is not a rule of decorations\.json/],
    [(b) => (b.placements = ['pine_groves', 'pine_groves']), /placements: a rule is listed twice/],
    [(b) => (b.trees = { boulders: 1 }), /trees: "boulders" is not one of its placements/],
    [(b) => (b.trees = { pine_groves: 4 }), /trees\.pine_groves must be in \[0, 3\]/],
    [(b) => (b.trees = ['pine_groves']), /trees must map placement names to densities/],
    [(b) => (b.ground = { 'dwell:ground_shell': 1 }), /ground: "dwell:ground_shell" is not a kind of ground\.json/],
    [(b) => (b.ground = { 'dwell:ground_rock': 3.5 }), /ground\.dwell:ground_rock must be in \[0, 3\]/],
    [(b) => (b.ground = ['dwell:ground_rock']), /ground must map ground item kinds to densities/],
    [(b) => (b.water = 'dwell:stone'), /water dwell:stone must be a fluid block/],
    [(b) => (b.inland = { rise: 1, halfDistance: 1 }), /unknown inland/],
  ];
  for (const [edit, message] of cases) {
    const b = loadDefault();
    edit(b);
    assert.throws(() => terrainOf(loadTerrain(), [b]), message);
  }
  const worldCases = [
    [(t) => (t.inland.halfDistance = 0), /terrain\.json: inland\.halfDistance must be in \(0, 100000\]/],
    [(t) => (t.defaultBiome = 'dwell:moor'), /defaultBiome "dwell:moor" is not a biome/],
    [(t) => (t.hills.amplitude = -1), /terrain\.json: hills\.amplitude must be in \[0, 1000\]/],
    [(t) => (t.bumps.wavelength = 2), /bumps\.wavelength/],
    [(t) => (t.hills.fadeTo = 100), /hills\.fadeTo must be greater than hills\.fadeFrom/],
    [(t) => delete t.seaFloor, /seaFloor must be an object/],
    [(t) => (t.rivers.leveeFall = 0), /rivers\.leveeFall must be in \(0, 10\]/],
    [(t) => (t.bands = {}), /terrain\.json: unknown bands/],
    [(t) => (t.biomeMap.wetReach = 0), /terrain\.json: biomeMap\.wetReach must be in \(0, 100000\]/],
    [(t) => (t.biomeMap.rainShare = 2), /terrain\.json: biomeMap\.rainShare must be in \[0, 1\]/],
    [(t) => (t.biomeMap.windDirection = 400), /terrain\.json: biomeMap\.windDirection must be in \[0, 360\]/],
    [(t) => delete t.biomeMap.windReach, /terrain\.json: biomeMap\.windReach must be in \(0, 1000000\]/],
    [(t) => (t.spawn.maxHeight = 1), /spawn\.maxHeight must be greater than spawn\.minHeight/],
  ];
  for (const [edit, message] of worldCases) {
    const t = loadTerrain();
    edit(t);
    assert.throws(() => terrainOf(t, [loadDefault()]), message);
  }
  assert.throws(() => terrainOf(loadTerrain(), [loadDefault(), loadDefault()]), /biome dwell:default is defined twice/);
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
  const n = d.placements.length;
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
  assert.ok(rs.includes(`PLACEMENTS: [Placement; ${String(n + 1)}]`));
  assert.match(
    rs,
    /name: "rock_scatter",\n\s+structures: &\[\("dwell:proto_rock", 1\)\],\n\s+on: &\["dwell:grass", "dwell:dirt"\],\n\s+footprint_on: false,\n\s+pad_trunk: false,\n\s+grid: 24,/,
  );
  // No groves: everywhere; no rise: flat; 30° as a squared gradient (tan² 30° = 1/3); quarter turns.
  assert.match(rs, /grove_wavelength: 0,\n\s+grove_coverage: 1\.0,\n\s+grove_outside: 1\.0,/);
  assert.match(rs, /rise_full: 0,\n\s+rise_low: 1\.0,\n\s+max_gradient_sq: 0\.33333334,/);
  assert.match(rs, /rotations: &\[0, 2\],/);
});

test('a placement pads its trunks by its `pad`, by default when it lists a tree species', () => {
  // WORK_QUEUE item 44: the ground under the trunk's columns is levelled to its base.
  const d = loadPlacements();
  const base = { on: ['dwell:grass'], footprint: 'anchor', grid: 24, density: 0.2, height: [0, 600], maxSlopeDeg: 30, spacing: 0, rotate: [0] };
  d.placements = [
    { ...base, name: 'species_default', structures: { 'dwell:oak': 1 } },
    { ...base, name: 'structure_default', structures: { 'dwell:proto_rock': 1 } },
    { ...base, name: 'species_none', structures: { 'dwell:oak': 1 }, pad: 'none' },
    { ...base, name: 'structure_trunk', structures: { 'dwell:proto_tree': 1 }, pad: 'trunk' },
  ];
  const rs = placementsRust(validatePlacements(d, blocks, structures));
  const pads = [...rs.matchAll(/pad_trunk: (true|false),/g)].map((m) => m[1]);
  assert.deepEqual(pads, ['true', 'false', 'false', 'true']);
  // The checked-in tree placements all pad; the boulders don't.
  const built = placementsRust(validatePlacements(loadPlacements(), blocks, structures));
  const names = [...built.matchAll(/name: "([a-z_]+)",\n(?:.*\n){3}\s+pad_trunk: (true|false)/g)].map((m) => [m[1], m[2]]);
  assert.deepEqual(
    names.filter(([, p]) => p === 'false').map(([n]) => n),
    ['boulders'],
  );
});

test('a bad placement fails with its name and the problem', () => {
  const cases = [
    [(p) => (p.structures = { 'dwell:birch': 1 }), /pine_groves: structure dwell:birch is not in shared\/structures or shared\/trees/],
    [(p) => (p.structures = { 'dwell:oak': 0 }), /structures.dwell:oak weight must be a whole number/],
    [(p) => (p.structures = {}), /structures must map structure names to weights/],
    [(p) => (p.structures['dwell:pine_tree'] = 0.5), /weight must be a whole number/],
    [(p) => (p.on = ['dwell:moss']), /on names unknown block dwell:moss/],
    [(p) => (p.on = ['dwell:water']), /on block dwell:water is not a full block/],
    [(p) => (p.footprint = 'crown'), /footprint must be one of on, anchor/],
    [(p) => (p.pad = 'ring'), /pine_groves: pad must be one of trunk, none/],
    [(p) => (p.grid = 1), /grid must be a whole number of metres in \[2, 256\]/],
    [(p) => (p.density = 1.5), /density must be in \(0, 1\]/],
    [(p) => (p.groves.wavelength = 4), /groves\.wavelength/],
    [(p) => (p.groves.outside = -0.1), /groves\.outside must be in \[0, 1\]/],
    [(p) => (p.height = [500, 400]), /height must be \[min, max\]/],
    [(p) => (p.rise.full = 1900), /rise\.full must be a whole number of metres in \(height\[0\], height\[1\]\]/],
    [(p) => (p.maxSlopeDeg = 90), /maxSlopeDeg must be in \(0, 90\) degrees/],
    [(p) => (p.spacing = -5), /spacing must be a whole number of blocks/],
    [(p) => (p.rotate = [0, 45]), /rotate must be distinct angles from 0, 90, 180, 270/],
    [(p) => (p.mirror = ['x']), /pine_groves: unknown mirror/],
    [(p) => (p.variants[0].replace = { 'dwell:leaves': 'dwell:moss' }), /variant snowy: replace names unknown block dwell:moss/],
    [(p) => (p.variants[0].height = [900, 800]), /variant snowy: height must be \[from, full\]/],
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

const loadMidplane = () => JSON.parse(readFileSync(join(here, 'midplane.json'), 'utf8'));

test('the checked-in bedrock and portals are valid and generate the checked-in Rust', () => {
  const rs = midplaneRust(validateMidplane(loadMidplane(), blocks));
  const checkedIn = readFileSync(join(here, '../../crates/dwell-worldgen/src/midplane.gen.rs'), 'utf8');
  assert.equal(rs, checkedIn, 'run `node shared/worldgen/gen.mjs`');
});

test('portal rules changed by data alone are emitted', () => {
  const m = loadMidplane();
  m.portals.count = [2, 2];
  m.portals.radius = 4;
  m.portals.ladder.wall = 'east';
  m.portals.rim = { block: 'dwell:sand', width: 2, height: 1 };
  m.bedrock.thickness = 2.5;
  m.portals.exit = { push: 12.5, depth: 3, minRise: 9 };
  const rs = midplaneRust(validateMidplane(m, blocks));
  assert.match(rs, /exit: PortalExit \{ push: 12\.5, depth: 3\.0, min_rise: 9\.0 \},/);
  assert.match(rs, /count_min: 2,/);
  assert.match(rs, /radius: 4\.0,/);
  assert.match(rs, /ladder: "dwell:ladder\[facing=west\]",/);
  assert.match(rs, /wall: \[1, 0\],/);
  assert.match(rs, /rim_block: "dwell:sand",/);
  assert.match(rs, /rim_rows: 2,/);
  assert.match(rs, /bedrock_rows: 5,/);
});

test('bad bedrock or portal entries fail with the key and the problem', () => {
  const cases = [
    [(m) => (m.bedrock.block = 'dwell:stone'), /bedrock\.block dwell:stone must be indestructible/],
    [(m) => (m.bedrock.block = 'dwell:nope'), /bedrock\.block dwell:nope is not in the registry/],
    [(m) => (m.bedrock.thickness = 0.3), /bedrock\.thickness must be a whole number of cells/],
    [(m) => (m.bedrock.thickness = 10.5), /bedrock\.thickness must be in \[0, 10\]/],
    [(m) => (m.portals.count = [5, 3]), /portals\.count must be \[min, max\]/],
    [(m) => (m.portals.radius = 0.5), /portals\.radius must be in \[1, 16\]/],
    [(m) => (m.portals.ladder.block = 'dwell:stone'), /portals\.ladder\.block dwell:stone must be climbable/],
    [(m) => (m.portals.ladder.wall = 'up'), /portals\.ladder\.wall must be one of north, east, south, west/],
    [(m) => (m.portals.rim.block = 'dwell:tall_grass'), /portals\.rim\.block dwell:tall_grass must be a full block/],
    [(m) => (m.portals.rim.height = 0.3), /portals\.rim\.height must be a whole number of cells/],
    [(m) => (m.portals.extra = 1), /portals: unknown extra/],
    [(m) => (m.portals.motes = 'dwell:nope'), /portals\.motes "dwell:nope" is not a look in shared\/look\/motes\.json/],
    [(m) => (m.other = 1), /unknown other/],
    [(m) => (m.portals.monument.structure = 'dwell:nope'), /portals\.monument\.structure "dwell:nope" is not in shared\/structures/],
    [(m) => (m.portals.clear = 1), /portals\.monument\.structure dwell:portal_arch reaches 7\.1 m from the axis, past the clear ground \(4\.5 m\)/],
    [(m) => (m.portals.monument.lodCellM = 3), /portals\.monument\.lodCellM must be a power of two in \[1, 64\]/],
    [(m) => (m.portals.monument.extra = 1), /portals\.monument: unknown extra/],
    [(m) => delete m.portals.exit, /portals\.exit must be an object/],
    [(m) => (m.portals.exit.push = -1), /portals\.exit\.push must be in \[0, 1000\]/],
    [(m) => (m.portals.exit.depth = 100), /portals\.exit\.depth must be in \[0, 64\]/],
    [(m) => (m.portals.exit.speed = 3), /portals\.exit: unknown speed/],
    [(m) => (m.portals.exit.minRise = 5), /portals\.exit\.minRise must be at least 8 m\/s/],
  ];
  for (const [change, message] of cases) {
    const m = loadMidplane();
    change(m);
    assert.throws(() => validateMidplane(m, blocks), message);
  }
});

test('a portal monument is named by data: another structure, or none', () => {
  const m = loadMidplane();
  m.portals.monument = { structure: 'dwell:proto_rock', lodCellM: 4 };
  const rs = midplaneRust(validateMidplane(m, blocks));
  assert.match(rs, /monument: Some\("dwell:proto_rock"\),/);
  assert.match(rs, /monument_lod_cell: 4,/);
  m.portals.monument.structure = null;
  assert.match(midplaneRust(validateMidplane(m, blocks)), /monument: None,/);
});

const loadPlates = () => JSON.parse(readFileSync(join(here, 'plates.json'), 'utf8'));

test('the checked-in plate layout rules are valid and generate the checked-in Rust', () => {
  const rs = platesRust(validatePlates(loadPlates()));
  const checkedIn = readFileSync(join(here, '../../crates/dwell-worldgen/src/plates.gen.rs'), 'utf8');
  assert.equal(rs, checkedIn, 'run `node shared/worldgen/gen.mjs`');
});

test('a plate fixture and new ranges added by data alone are emitted', () => {
  const p = loadPlates();
  p.count = [6, 9];
  p.kinds.oceanic.min = 2;
  p.fixtures.push({
    name: 'young_ocean_dives_last',
    plates: [
      { kind: 'oceanic', at: [0, 0], motion: [2, 0], age: 300 },
      { kind: 'oceanic', at: [4000, 0], motion: [-2, 0], age: 10 },
    ],
    expect: { kind: 'convergent', subducts: 0 },
  });
  const rs = platesRust(validatePlates(p));
  assert.match(rs, /count_min: 6,/);
  assert.match(rs, /count_max: 9,/);
  assert.match(rs, /oceanic: KindRule \{ weight: 0\.45, min: 2 \},/);
  assert.match(rs, /name: "young_ocean_dives_last",/);
  assert.match(rs, /age: 300\.0 \},\n.*age: 10\.0 \},\n {8}\],\n {8}kind: BoundaryKind::Convergent,\n {8}subducts: Some\(0\),/);
});

test('bad plate entries fail with the key and the problem', () => {
  const cases = [
    [(p) => (p.count = [5, 3]), /plates\.json: count must be \[min, max\]/],
    [(p) => (p.count = [0, 3]), /count must be \[min, max\], whole numbers 1–32/],
    [(p) => delete p.reach, /plates\.json: reach is missing/],
    [(p) => (p.seaMargin = 30000), /seaMargin must be in \[0, 25000\]/],
    [(p) => (p.kinds.basaltic = { weight: 1, min: 0 }), /kinds: unknown basaltic/],
    [(p) => delete p.kinds.oceanic, /kinds\.oceanic is missing/],
    [(p) => (p.kinds.oceanic.weight = 2), /kinds\.oceanic\.weight must be in \[0, 1\]/],
    [(p) => ((p.kinds.oceanic.weight = 0), (p.kinds.continental.weight = 0)), /kinds: the weights must not all be 0/],
    [(p) => (p.kinds.oceanic.min = 3), /kinds: the kinds' mins \(4\) exceed the least plate count \(3\)/],
    [(p) => (p.warp.wavelength = 1.5), /warp\.wavelength must be a whole number/],
    [(p) => (p.transformBelow = 1.5), /transformBelow must be in \[0, 1\]/],
    [(p) => (p.hotspots.extra = 1), /hotspots: unknown extra/],
    [(p) => (p.other = 1), /the file: unknown other/],
    [(p) => (p.uplift.cell = 100), /uplift\.cell must be in \[500, 10000\]/],
    [(p) => delete p.uplift.rift, /uplift\.rift is missing/],
    [(p) => (p.uplift.range.width = 0), /uplift\.range\.width must be in \[1, 50000\]/],
    [(p) => (p.uplift.arc.maxCones = 2.5), /uplift\.arc\.maxCones must be a whole number/],
    [(p) => delete p.uplift.hotspot.decay, /uplift\.hotspot\.decay is missing/],
    [(p) => (p.uplift.wobble = 1), /uplift: unknown wobble/],
    [(p) => (p.fixtures[0].plates[1].kind = 'mantle'), /fixtures\[0\] \(ocean_under_continent\)\.plates\[1\]\.kind must be one of oceanic, continental/],
    [(p) => p.fixtures[0].plates.pop(), /fixtures\[0\] \(ocean_under_continent\)\.plates must be two plates/],
    [(p) => (p.fixtures[0].plates[0].at = [5000, 0]), /plates must not share a centre/],
    [(p) => (p.fixtures[0].expect.kind = 'sideways'), /expect\.kind must be one of convergent, divergent, transform/],
    [(p) => (p.fixtures[3].expect.subducts = 0), /fixtures\[3\] \(rift_opens\)\.expect\.subducts must be null for a divergent boundary/],
    [(p) => (p.fixtures[1].name = p.fixtures[0].name), /fixtures\[1\] \(ocean_under_continent\): duplicate name/],
  ];
  for (const [change, message] of cases) {
    const p = loadPlates();
    change(p);
    assert.throws(() => validatePlates(p), message);
  }
});

test('the checked-in volumes are valid and generate the checked-in Rust', () => {
  const data = validateVolume(JSON.parse(readFileSync(join(here, 'volume.json'), 'utf8')));
  const rust = readFileSync(join(here, '../../crates/dwell-worldgen/src/volume.gen.rs'), 'utf8');
  assert.equal(volumeRust(data), rust);
});

test('a terrace over the surface is emitted by data alone', () => {
  const data = {
    terrain: 'steps',
    volumes: [
      {
        name: 'steps',
        reach: 8,
        material: 'dwell:stone',
        soilDepth: 3,
        floorGrass: 3,
        strength: 1,
        density: {
          add: ['depth', { terrace: { of: 'surface', step: 6, jitter: 0.5, flat: 1, riser: 0.05, stream: 9 } }],
        },
      },
    ],
  };
  const rust = volumeRust(validateVolume(data));
  assert.match(rust, /Node::Surface/);
  assert.match(rust, /Node::Terrace \{ of: 1, step: 6\.0, jitter: 0\.5, flat: 1\.0, riser: 0\.05, stream: 9 \}/);
});

test('a bad terrace fails with its path and the problem', () => {
  const vol = (terrace) => ({
    terrain: null,
    volumes: [
      { name: 'v', reach: 8, material: 'dwell:stone', soilDepth: 3, floorGrass: 3, strength: 1, density: { terrace } },
    ],
  });
  const ok = { of: 'surface', step: 6, jitter: 0.5, flat: 1, riser: 0.05, stream: 1 };
  assert.throws(() => validateVolume(vol({ ...ok, jitter: 1 })), /density: terrace.jitter must be in \[0, 0.9\]/);
  assert.throws(() => validateVolume(vol({ ...ok, riser: 0 })), /terrace.riser must be in/);
  const { stream: _, ...missing } = ok;
  assert.throws(() => validateVolume(vol(missing)), /terrace: missing stream/);
});

test('an identical subtree is one node', () => {
  const warp = { add: ['surface', { mul: [3, { noise: { wavelength: 90, octaves: 2, stretchY: 0, stream: 4 } }] }] };
  const data = {
    terrain: null,
    volumes: [
      {
        name: 'v',
        reach: 8,
        material: 'dwell:stone',
        soilDepth: 3,
        floorGrass: 3,
        strength: 1,
        density: { add: ['depth', { min: [warp, { abs: warp }] }] },
      },
    ],
  };
  const rust = volumeRust(validateVolume(data));
  assert.equal(rust.match(/Node::Noise/g)?.length, 1);
  assert.equal(rust.match(/Node::Surface/g)?.length, 1);
});

test('a biome water look and the bank fields are checked; the archipelago fixtures load', async () => {
  const { biomeRefs } = await import('./gen.mjs');
  const { validateBiome } = await import('./biomes.mjs');
  const { readFileSync } = await import('node:fs');
  const here = new URL('.', import.meta.url).pathname;
  const world = JSON.parse(readFileSync(`${here}terrain.json`, 'utf8'));
  const [w] = validateFixtureWorlds(loadBiomes(`${here}worlds`), world);
  assert.equal(w.name, 'test:archipelago');
  assert.equal(w.world.islands.amplitude, 1400);
  assert.equal(world.islands.amplitude, 0, "the default world's islands knob is off");
  assert.throws(
    () => validateFixtureWorlds([{ name: 'test:x', overrides: { islands: { amplitude: -1 } } }], world),
    /islands\.amplitude must be in/,
  );
  const blocks = registryBlocks();
  const refs = biomeRefs();
  const isles = JSON.parse(readFileSync(`${here}biomes/fixtures/archipelago.json`, 'utf8'));
  validateBiome(isles, blocks, refs);
  for (const [mut, msg] of [
    [(b) => (b.waterLook = {}), /waterLook needs a scatter or an absorption/],
    [(b) => (b.waterLook.absorption = [1, 2]), /waterLook\.absorption must be three numbers/],
    [(b) => (b.waterLook.tint = 1), /waterLook: unknown tint/],
    [(b) => (b.banks.beach = -1), /banks\.beach must be in/],
    [(b) => delete b.banks, /banks must be an object/],
  ]) {
    const b = structuredClone(isles);
    mut(b);
    assert.throws(() => validateBiome(b, blocks, refs), msg);
  }
});

test('the preset biomes validate, each labelled and described, and stay off the default world', () => {
  const presets = validatePresetBiomes(loadBiomes(join(here, 'biomes/presets')), blocks);
  assert.ok(presets.length >= 12, `${presets.length} presets`);
  for (const p of presets) {
    assert.match(p.name, /^preset:/);
    assert.ok(p.label.length <= 40 && p.description.length <= 200, p.name);
  }
  // Not one of the world's biomes: the default world is unchanged.
  const { biomes } = terrainOf(loadTerrain(), loadBiomes());
  assert.deepEqual(
    biomes.map((b) => b.name),
    ['dwell:default'],
  );
  const one = presets[0];
  const bad = [
    [(b) => delete b.label, /a preset needs a label and a description/],
    [(b) => (b.label = ''), /label must be text of 1 to 40 characters/],
    [(b) => (b.description = 'x'.repeat(201)), /description must be text of 1 to 200 characters/],
    [(b) => (b.label = presets[1].label), /preset label .* is used twice/],
  ];
  for (const [edit, want] of bad) {
    const b = structuredClone(one);
    edit(b);
    assert.throws(() => validatePresetBiomes([b, ...presets.slice(1)], blocks), want);
  }
  assert.throws(
    () => validatePresetBiomes(presets, blocks, undefined, [one.name]),
    /preset name .* is used twice/,
  );
  // A fixture preset by data alone validates.
  const extra = { ...structuredClone(one), name: 'preset:test_extra', label: 'Test extra' };
  assert.equal(validatePresetBiomes([...presets, extra], blocks).length, presets.length + 1);
});

test('the checked-in rocks are valid, generate the checked-in Rust, and take a rock by data alone', () => {
  const rocks = () => JSON.parse(readFileSync(join(here, 'rocks.json'), 'utf8'));
  const rs = readFileSync(join(here, '../../crates/dwell-worldgen/src/rocks.gen.rs'), 'utf8');
  assert.equal(rocksRust(validateRocks(rocks(), blocks)), rs);
  assert.equal(rocks().shaping.strength, 0, 'the default world is unshaped');
  const d = rocks();
  d.rocks.push({ name: 'test:cap', hardness: 1, jointing: 'vertical', bed: 5, solubility: 0, face: 'dwell:stone', cap: 'dwell:stone', scree: 'dwell:sand', color: '#ff0000' });
  d.stacks.push({ name: 'test:capped', weight: 0, layers: [{ rock: 'test:cap', thickness: 10 }] });
  const out = rocksRust(validateRocks(d, blocks));
  assert.match(out, /name: "test:cap",\n {8}hardness: 1\.0,\n {8}jointing: Jointing::Vertical/);
  assert.match(out, /Layer \{ rock: "test:cap", thickness: 10\.0 \}/);
  const bad = (f, msg) => {
    const x = rocks();
    f(x);
    assert.throws(() => validateRocks(x, blocks), msg);
  };
  bad((x) => (x.rocks[0].jointing = 'wobbly'), /rock dwell:sandstone: jointing must be one of/);
  bad((x) => (x.rocks[0].hardness = 2), /rock dwell:sandstone: hardness must be in \[0, 1\]/);
  bad((x) => (x.rocks[1].face = 'dwell:nothing'), /rock dwell:shale: face "dwell:nothing" is not in the registry/);
  bad((x) => (x.stacks[0].layers[0].rock = 'dwell:nothing'), /stack dwell:red_beds: layer rock "dwell:nothing"/);
  bad((x) => (x.shaping.strength = 3), /shaping.strength must be in \[0, 1\]/);
  bad((x) => (x.extra = 1), /unknown extra/);
  // The erosion (T4): its knobs by data alone; bad ones named.
  const e = rocks();
  e.erosion.reach = 1234;
  assert.match(rocksRust(validateRocks(e, blocks)), /erosion: Erosion \{\n {8}strength: 1\.0,\n {8}reach: 1234\.0,/);
  bad((x) => (x.erosion.threshold = 0.5), /erosion.threshold must be/);
  bad((x) => delete x.erosion.wall, /erosion.wall must be/);
  bad((x) => (x.erosion.flow = 1), /erosion: unknown flow/);
});

test('R5: plateProvinces map plate settings to stacks by name, by data alone; bad entries fail naming them', () => {
  const rocks = () => JSON.parse(readFileSync(join(here, 'rocks.json'), 'utf8'));
  const d = rocks();
  // Every rule names a stack; the new stacks take no share of the noise's provinces.
  const stacks = new Map(d.stacks.map((s) => [s.name, s]));
  for (const r of d.plateProvinces.rules) assert.ok(stacks.has(r.stack), r.setting);
  for (const n of ['dwell:arc_volcanics', 'dwell:hotspot_shield', 'dwell:collision_core', 'dwell:rift_fill'])
    assert.equal(stacks.get(n).weight, 0, n);
  // A rule swapped and one added by data alone.
  d.plateProvinces.rules.find((r) => r.setting === 'core').stack = 'dwell:granite_core';
  d.plateProvinces.rules.push({ setting: 'default', stack: 'dwell:badland_beds' });
  const out = rocksRust(validateRocks(d, blocks));
  assert.match(out, /PlateRule \{ setting: PlateSetting::Core, stack: "dwell:granite_core" \}/);
  assert.match(out, /PlateRule \{ setting: PlateSetting::Default, stack: "dwell:badland_beds" \}/);
  const bad = (f, msg) => {
    const x = rocks();
    f(x);
    assert.throws(() => validateRocks(x, blocks), msg);
  };
  bad((x) => (x.plateProvinces.rules[0].setting = 'mantle'), /plateProvinces: rule setting "mantle" must be one of/);
  bad((x) => (x.plateProvinces.rules[0].stack = 'dwell:nothing'), /plateProvinces: rule arc: stack "dwell:nothing" is not a stack/);
  bad((x) => x.plateProvinces.rules.push({ ...x.plateProvinces.rules[0] }), /plateProvinces: rule arc is listed twice/);
  bad((x) => (x.plateProvinces.rangeCore = [9000, 1000]), /plateProvinces: rangeCore must be/);
  bad((x) => (x.plateProvinces.riftWidth = -1), /plateProvinces: riftWidth must be/);
  bad((x) => (x.plateProvinces.depth = 1), /plateProvinces: unknown depth/);
  bad((x) => (x.plateProvinces.edgeFade = -5), /plateProvinces: edgeFade must be/);
});

test('item 15: the Realistic world steps by its rock, not the global cliffs volume (data)', () => {
  const r = JSON.parse(readFileSync(join(here, 'worlds/realistic.json'), 'utf8'));
  assert.equal(r.cliffs, 0, 'no global terrace volume in Realistic worlds');
  assert.ok(r.geology.shaping.strength > 0, 'the rock shaping is on');
  assert.ok(r.geology.erosion.strength > 0, 'the erosion is on');
  const rocks = JSON.parse(readFileSync(join(here, 'rocks.json'), 'utf8'));
  assert.ok(rocks.plateProvinces.edgeFade > 0, 'the provinces fade at their edges');
});

test('R5: a rock\'s soils by climate, in order; bad climates fail naming the rock', () => {
  const veg = () => JSON.parse(readFileSync(join(here, 'vegetation.json'), 'utf8'));
  const andesite = veg().rockSoils.filter((r) => r.rock === 'dwell:andesite');
  assert.ok(new Set(andesite.map((r) => r.type)).size >= 2, 'andesite weathers by climate');
  assert.ok(!andesite.at(-1).celsius && !andesite.at(-1).moisture, 'its last entry names no climate');
  const d = veg();
  d.rockSoils.unshift({ rock: 'dwell:granite', celsius: [10, 40], moisture: [0.5, 1], depth: 1.5, type: 'dwell:loamy' });
  assert.match(
    vegetationRust(validateVegetation(d)),
    /RockSoil \{ rock: "dwell:granite", depth: 1\.5, soil_type: 1, celsius: \[10\.0, 40\.0\], moisture: \[0\.5, 1\.0\] \}/,
  );
  const bad = (f, msg) => {
    const x = veg();
    f(x);
    assert.throws(() => validateVegetation(x), msg);
  };
  bad((x) => x.rockSoils.push({ rock: 'dwell:shale', depth: 1, type: 'dwell:loamy' }), /rockSoils: dwell:shale is listed after its entry without a climate/);
  bad((x) => (x.rockSoils[0].celsius = [5, 1]), /rockSoils: dwell:sandstone: celsius must be \[least, most\]/);
  bad((x) => (x.rockSoils[0].moisture = [0, 2]), /rockSoils: dwell:sandstone: moisture must be/);
  bad((x) => (x.rockSoils[0].rain = [0, 1]), /rockSoils: dwell:sandstone: unknown rain/);
});

test('a biome opts in to erosion and gullies in [0, 1]; the default biome does not', async () => {
  const { biomeRefs } = await import('./gen.mjs');
  const { validateBiome } = await import('./biomes.mjs');
  const blocks = registryBlocks();
  const def = JSON.parse(readFileSync(join(here, 'biomes/default.json'), 'utf8'));
  assert.equal(def.erosion, undefined);
  assert.equal(def.gullies, undefined);
  const mesa = JSON.parse(readFileSync(join(here, 'biomes/presets/red_rock_mesa.json'), 'utf8'));
  assert.equal(mesa.erosion, 1);
  const refs = biomeRefs();
  validateBiome(mesa, blocks, refs);
  assert.throws(() => validateBiome({ ...mesa, gullies: 2 }, blocks, refs), /gullies must be in \[0, 1\]/);
});

test("a biome's arches.fins and arches.rock (0.52.66) pass when valid and fail naming the field", async () => {
  const { biomeRefs } = await import('./gen.mjs');
  const { validateBiome } = await import('./biomes.mjs');
  const blocks = registryBlocks();
  const refs = biomeRefs();
  const fixture = () => JSON.parse(readFileSync(join(here, 'biomes/fixtures/arches.json'), 'utf8'));
  // Valid: the fin rule with and without its optional overrun, at its ranges' ends, and a rock
  // of the fixture's stack (dwell:red_beds: sandstone over shale).
  for (const [fins, rock] of [
    [{ approach: 150, width: 12 }, 'dwell:sandstone'],
    [{ approach: 0, width: 2, overrun: 0 }, 'dwell:shale'],
    [{ $comment: 'the ends', approach: 1000, width: 64, overrun: 2000 }, undefined],
  ]) {
    const b = fixture();
    b.arches.fins = fins;
    if (rock !== undefined) b.arches.rock = rock;
    assert.doesNotThrow(() => validateBiome(b, blocks, refs), JSON.stringify({ fins, rock }));
  }
  // A biome without its own rockStack takes the province's: any rock of rocks.json may be named.
  const noStack = fixture();
  delete noStack.rockStack;
  noStack.arches.rock = 'dwell:granite';
  assert.doesNotThrow(() => validateBiome(noStack, blocks, refs));
  const at = /^biome test:arches: /;
  const cases = [
    [(a) => (a.fins = { approach: -1, width: 12 }), /arches\.fins\.approach must be in \[0, 1000\]/],
    [(a) => (a.fins = { approach: 1001, width: 12 }), /arches\.fins\.approach must be in \[0, 1000\]/],
    [(a) => (a.fins = { width: 12 }), /arches\.fins\.approach must be in \[0, 1000\]/],
    [(a) => (a.fins = { approach: '150', width: 12 }), /arches\.fins\.approach must be in \[0, 1000\]/],
    [(a) => (a.fins = { approach: 150, width: 1 }), /arches\.fins\.width must be in \[2, 64\]/],
    [(a) => (a.fins = { approach: 150, width: 65 }), /arches\.fins\.width must be in \[2, 64\]/],
    [(a) => (a.fins = { approach: 150, width: -12 }), /arches\.fins\.width must be in \[2, 64\]/],
    [(a) => (a.fins = { approach: 150 }), /arches\.fins\.width must be in \[2, 64\]/],
    [(a) => (a.fins = { approach: 150, width: 12, overrun: -1 }), /arches\.fins\.overrun must be in \[0, 2000\]/],
    [(a) => (a.fins = { approach: 150, width: 12, overrun: 2001 }), /arches\.fins\.overrun must be in \[0, 2000\]/],
    [(a) => (a.fins = { approach: 150, width: 12, overrun: true }), /arches\.fins\.overrun must be in \[0, 2000\]/],
    [(a) => (a.fins = { approach: 150, width: 12, reach: 3 }), /arches\.fins: unknown reach/],
    [(a) => (a.fins = [150, 12]), /arches\.fins must be an object/],
    [(a) => (a.fins = 150), /arches\.fins must be an object/],
    [(a) => (a.fins = null), /arches\.fins must be an object/],
    [(a) => (a.rock = 'dwell:moonrock'), /arches\.rock: "dwell:moonrock" is not a rock of rocks\.json/],
    [(a) => (a.rock = 'dwell:granite'), /arches\.rock: "dwell:granite" is not a layer of the biome's rockStack/],
    [(a) => (a.rock = 3), /arches\.rock must be a rock name/],
    [(a) => (a.rock = ['dwell:sandstone']), /arches\.rock must be a rock name/],
  ];
  for (const [edit, message] of cases) {
    const b = fixture();
    edit(b.arches);
    assert.throws(
      () => validateBiome(b, blocks, refs),
      (e) => at.test(e.message) && message.test(e.message),
      `${JSON.stringify(b.arches.fins ?? null)} / ${JSON.stringify(b.arches.rock ?? null)}: ${message}`,
    );
  }
});

test('the checked-in vegetation is valid, generates the checked-in Rust, and takes a kind by data alone', () => {
  const veg = () => JSON.parse(readFileSync(join(here, 'vegetation.json'), 'utf8'));
  const rs = readFileSync(join(here, '../../crates/dwell-worldgen/src/vegetation.gen.rs'), 'utf8');
  assert.equal(vegetationRust(validateVegetation(veg())), rs);
  // The default biome doesn't opt in: the default world is unchanged.
  const def = JSON.parse(readFileSync(join(here, 'biomes/default.json'), 'utf8'));
  assert.equal(def.vegetation, undefined);
  const d = veg();
  d.soilTypes.push({ name: 'test:clay', color: '#aa5500' });
  d.kinds[0] = { ...d.kinds[0], name: 'test:clay_pines', temperature: [0, 9], soils: ['test:clay'] };
  const out = vegetationRust(validateVegetation(d));
  assert.match(out, /name: "test:clay_pines",\n {12}placement: "pine_groves",\n {12}decoration: "",\n {12}ground: "",\n {12}temperature: \[0\.0, 9\.0\]/);
  assert.match(out, /soils: &\[3\]/);
  const bad = (f, msg) => {
    const x = veg();
    f(x);
    assert.throws(() => validateVegetation(x), msg);
  };
  bad((x) => (x.kinds[0].placement = 'nothing'), /kind dwell:conifers: placement "nothing" is not a rule/);
  bad((x) => (x.kinds[0].temperature = [10, 5]), /temperature must be \[least, most\]/);
  bad((x) => (x.kinds[0].soils = ['test:none']), /soils must be a list of soil types/);
  bad((x) => (x.rockSoils[0].rock = 'dwell:nothing'), /rockSoils: "dwell:nothing" is not a rock/);
  bad((x) => (x.rockSoils[0].type = 'dwell:nothing'), /rock dwell:sandstone: type "dwell:nothing" is not a soil type/);
  bad((x) => (x.climate.perUnitC = -1), /climate.perUnitC must be in/);
  bad((x) => x.kinds.push({ ...x.kinds[0], name: 'test:twin' }), /placement pine_groves already has a kind/);
  bad((x) => (x.extra = 1), /unknown extra/);
});

test('T3b: soil types name their blocks, kinds govern decorations and ground items, biomes opt in to snow lines, all checked', () => {
  const veg = () => JSON.parse(readFileSync(join(here, 'vegetation.json'), 'utf8'));
  // A kind over a decoration and one over a ground item, by data alone.
  const d = veg();
  d.soilTypes[1].under = 'dwell:sand';
  d.kinds = d.kinds.filter((k) => !k.decoration);
  d.kinds.push({ ...d.kinds[0], name: 'test:tufts', decoration: 'meadow_grass' });
  delete d.kinds.at(-1).placement;
  const out = vegetationRust(validateVegetation(d));
  assert.match(out, /name: "dwell:loamy", color: 0x5f4426, ground: "", under: "dwell:sand"/);
  assert.match(out, /name: "test:tufts",\n {12}placement: "",\n {12}decoration: "meadow_grass",/);
  const bad = (f, msg) => {
    const x = veg();
    f(x);
    assert.throws(() => validateVegetation(x), msg);
  };
  bad((x) => (x.soilTypes[0].under = 'dwell:nothing'), /soil type dwell:sandy: under "dwell:nothing" is not a full block/);
  bad((x) => (x.soilTypes[0].colour = '#000000'), /soil type dwell:sandy: unknown colour/);
  bad((x) => (x.kinds[0].decoration = 'meadow_grass'), /must govern exactly one of placement, decoration or ground/);
  bad((x) => {
    const k = { ...x.kinds[0], name: 'test:g', ground: 'dwell:nothing' };
    delete k.placement;
    x.kinds.push(k);
  }, /ground "dwell:nothing" is not a kind of ground.json/);
  bad((x) => {
    const k = { ...x.kinds.find((k) => k.decoration), name: 'test:twin' };
    x.kinds.push(k);
  }, /decoration meadow_grass already has a kind/);
  // A biome's snow line, soil blocks and undergrowth.
  const biome = () => JSON.parse(readFileSync(join(here, 'biomes/default.json'), 'utf8'));
  const snowy = { ...biome(), snow: { belowC: 0, maxSlope: 1, block: 'dwell:snow', iceBelowC: -8, ice: 'dwell:glacier_ice' }, soilBlocks: true, undergrowth: 1 };
  validateBiome(snowy);
  const rs = terrainRust(terrainOf(loadTerrain(), [snowy]));
  assert.match(rs, /snow: Some\(crate::terrain::BiomeSnow \{ below_c: 0\.0, max_slope: 1\.0, block: "dwell:snow", ice: Some\(\("dwell:glacier_ice", -8\.0\)\) \}\),\n {8}soil_blocks: true,\n {8}undergrowth: 1\.0,/);
  const badBiome = (extra, msg) => assert.throws(() => validateBiome({ ...biome(), ...extra }), msg);
  badBiome({ snow: { belowC: 0, maxSlope: 1, block: 'dwell:nothing' } }, /snow\.block "dwell:nothing" is not a full block/);
  badBiome({ snow: { belowC: 0, maxSlope: 1, block: 'dwell:snow', ice: 'dwell:glacier_ice' } }, /ice and iceBelowC go together/);
  badBiome({ snow: { belowC: 0, maxSlope: 1, block: 'dwell:snow', iceBelowC: 3, ice: 'dwell:glacier_ice' } }, /iceBelowC must be colder/);
  badBiome({ snow: { belowC: 0, maxSlope: 0, block: 'dwell:snow' } }, /snow\.maxSlope must be in/);
  badBiome({ snow: { belowC: 0, maxSlope: 1, block: 'dwell:snow', deep: 2 } }, /snow: unknown deep/);
  badBiome({ soilBlocks: 1 }, /soilBlocks must be true or false/);
  badBiome({ undergrowth: 2 }, /undergrowth must be in \[0, 1\]/);
  // The default biome opts in to none of them.
  const def = biome();
  assert.equal(def.snow ?? def.soilBlocks ?? def.undergrowth, undefined);
});

test('the tectonics (T5) are off by default, take a boundary and a chain by data alone, and name bad keys', async () => {
  const { validateWorldKnobs } = await import('./biomes.mjs');
  const world = JSON.parse(readFileSync(join(here, 'terrain.json'), 'utf8'));
  assert.equal(world.tectonics.kind, 0, 'the default world has no boundary');
  assert.equal(world.tectonics.hotspotCount, 0, 'and no hotspot chain');
  const on = { name: 'test:plates', overrides: { tectonics: { kind: 1, arcCount: 3, hotspotCount: 4 } } };
  const [w] = validateFixtureWorlds([on], world);
  assert.equal(w.world.tectonics.arcCount, 3);
  assert.throws(
    () => validateFixtureWorlds([{ name: 'test:x', overrides: { tectonics: { kind: 4 } } }], world),
    /tectonics\.kind must be a whole number in \[0, 3\]/,
  );
  validateWorldKnobs({ tectonics: { kind: 2, riftDepth: 120 } });
  assert.throws(() => validateWorldKnobs({ tectonics: { coneRadius: 0 } }), /tectonics\.coneRadius/);
  const rocks = JSON.parse(readFileSync(join(here, 'rocks.json'), 'utf8'));
  // The Rainier detail: off by default, validated by range.
  for (const k of ['cleaverDepth', 'rimNotchDepth', 'cragHeight', 'cirqueDepth'])
    assert.equal(world.tectonics[k], 0, `${k} off by default`);
  validateWorldKnobs({ tectonics: { cleaverDepth: 160, rimNotches: 5, cragCount: 4, cirqueCount: 4 } });
  assert.throws(() => validateWorldKnobs({ tectonics: { rimNotches: 2.5 } }), /tectonics\.rimNotches/);
  assert.throws(() => validateWorldKnobs({ tectonics: { cirqueRadius: 0 } }), /tectonics\.cirqueRadius/);
  assert.throws(() => validateWorldKnobs({ tectonics: { cragHeight: -1 } }), /tectonics\.cragHeight/);
  assert.throws(
    () => validateFixtureWorlds([{ name: 'test:x', overrides: { tectonics: { cirqueCount: 17 } } }], world),
    /tectonics\.cirqueCount must be a whole number in \[0, 16\]/,
  );
  rocks.tectonic.volcanic = 'dwell:nothing';
  assert.throws(() => validateRocks(rocks, blocks), /tectonic\.volcanic must name a stack/);
});

test('a world fixture may be offered in the New world form (newWorld), checked', () => {
  const world = loadTerrain();
  const base = { name: 'test:offer', overrides: {} };
  const ok = { ...base, newWorld: { label: 'Offer', description: 'A world to pick.' } };
  assert.equal(validateFixtureWorlds([ok], world)[0].name, 'test:offer');
  assert.throws(() => validateFixtureWorlds([{ ...base, newWorld: {} }], world), /newWorld\.label/);
  assert.throws(
    () => validateFixtureWorlds([{ ...base, newWorld: { label: 'x', size: 3 } }], world),
    /newWorld: unknown size/,
  );
});

test('a biome may set spacing per placement it lists (0.52.65), checked', async () => {
  const { biomeRefs } = await import('./gen.mjs');
  const { validateBiome } = await import('./biomes.mjs');
  const blocks = registryBlocks();
  const refs = biomeRefs();
  const boreal = JSON.parse(readFileSync(join(here, 'biomes/presets/boreal.json'), 'utf8'));
  validateBiome({ ...boreal, spacing: { pine_groves: 6 } }, blocks, refs);
  assert.throws(
    () => validateBiome({ ...boreal, spacing: { boulders: 6 } }, blocks, refs),
    /spacing: "boulders" is not one of its placements/,
  );
  assert.throws(
    () => validateBiome({ ...boreal, spacing: { pine_groves: 2.5 } }, blocks, refs),
    /spacing\.pine_groves must be a whole number/,
  );
});

test('lava vents on a cone are fixture-world data: checked, and a new one added by data alone', () => {
  const world = loadTerrain();
  const vent = { cone: 'hotspot', index: 0, kind: 'lake', fluid: 'dwell:lava', rateM3s: 2, depthM: 1, radius: 8 };
  const of = (v, t = {}) => [
    { name: 'test:x', overrides: { tectonics: { hotspotCount: 2, ...t, lavaVents: [v] } } },
  ];
  const [w] = validateFixtureWorlds(of({ ...vent, kind: 'river', index: 1, distance: 300, angle: 90 }), world);
  assert.equal(w.world.tectonics.lavaVents[0].kind, 'river');
  assert.equal(world.tectonics.lavaVents, undefined, 'the default world lists none');
  const starter = validateFixtureWorlds(loadBiomes(join(here, 'worlds')), world).find((w) => w.name === 'test:starter');
  assert.equal(starter.world.tectonics.lavaVents.length, 2, 'the starter world has its lake and its river');
  const [w2] = validateFixtureWorlds(of({ ...vent, faces: ['b'] }), world);
  assert.deepEqual(w2.world.tectonics.lavaVents[0].faces, ['b'], 'a vent on one face');
  for (const [v, msg] of [
    [{ ...vent, index: 2 }, /no hotspot cone 2/],
    [{ ...vent, cone: 'arc' }, /no arc cone 0/],
    [{ ...vent, kind: 'geyser' }, /kind must be "lake" or "river"/],
    [{ ...vent, fluid: 'dwell:honey' }, /dwell:honey is not a hot fluid/],
    [{ ...vent, radius: 0 }, /radius must be a whole number/],
    [{ ...vent, rateM3s: 0 }, /rateM3s must be in/],
    [{ ...vent, heat: 1 }, /unknown heat/],
    [{ ...vent, faces: ['c'] }, /faces must list "a" and\/or "b"/],
    [{ ...vent, faces: [] }, /faces must list "a" and\/or "b"/],
  ])
    assert.throws(() => validateFixtureWorlds(of(v), world), msg);
});

test('grass.json validates, its output is current, and bad entries fail naming the field', () => {
  const blocks = registryBlocks();
  const decorations = validateDecorations(load(), blocks).decorations;
  const grass = () => JSON.parse(readFileSync(join(here, 'grass.json'), 'utf8'));
  const g = validateGrass(grass(), blocks, decorations);
  const checkedIn = readFileSync(join(here, '../../crates/dwell-worldgen/src/grass.gen.rs'), 'utf8');
  assert.equal(grassRust(g), checkedIn, 'grass.gen.rs is stale: run node shared/worldgen/gen.mjs');
  const bad = (patch, re) =>
    assert.throws(() => validateGrass({ ...grass(), ...patch }, blocks, decorations), re);
  bad({ block: 'dwell:nope' }, /block "dwell:nope" is not a block/);
  bad({ on: ['dwell:tall_grass'] }, /on "dwell:tall_grass" is not a full block/);
  bad({ decoration: 'nope' }, /decoration "nope" is not a decoration/);
  bad({ maxDensity: 1.5 }, /maxDensity must be in/);
  bad({ maxDensity: 0.1 }, /maxDensity 0.1 is under decoration meadow_grass/);
  bad({ reach: 1.5 }, /reach must be a whole number/);
  bad({ extra: 1 }, /unknown extra/);
  bad({ onRamps: 1 }, /onRamps must be true or false/);
  const strict = new Map(blocks);
  const tall = strict.get('dwell:tall_grass');
  strict.set('dwell:tall_grass', { ...tall, placeOn: tall.placeOn.filter((b) => b !== 'dwell:grass_ramp') });
  assert.throws(() => validateGrass(grass(), strict, decorations), /onRamps: dwell:tall_grass may not stand on dwell:grass_ramp/);
  const noRamps = new Map(blocks);
  noRamps.delete('dwell:grass_ramp');
  assert.throws(() => validateGrass(grass(), noRamps, decorations), /onRamps: dwell:grass has no ramps/);
  assert.match(grassRust(g), /on_ramps: true,/);
  assert.match(grassRust({ ...g, onRamps: undefined }), /on_ramps: false,/);
  // A rule by data alone: another ground and rates.
  const fixture = validateGrass({ ...grass(), on: ['dwell:dirt'], spreadPerDay: 1, reach: 2 }, blocks, decorations);
  assert.match(grassRust(fixture), /on: &\["dwell:dirt"\]/);
  // Growing in height (docs/ECOLOGY.md §10): the stages, by data.
  const stages = (patch) => ({ ...grass(), stages: { ...grass().stages, ...patch } });
  const badStages = (patch, re) => assert.throws(() => validateGrass(stages(patch), blocks, decorations), re);
  badStages({ property: 'nope' }, /stages.property "nope" is not a property of dwell:tall_grass/);
  badStages({ order: ['0', '1', '3'], days: [3, 4], share: [0, 0.5, 1] }, /at least 4 values/);
  badStages({ order: ['3', '2', '1', '0'] }, /must end with the property's default 3/);
  badStages({ order: ['0', '1', '1', '3'] }, /each of dwell:tall_grass's stage values once/);
  badStages({ days: [1, 1, 1] }, /add up to at least 7/);
  badStages({ days: [3, 4] }, /stages.days must give each stage but the last/);
  badStages({ share: [0.1, 0.2, 0.4, 0.6] }, /must start at 0/);
  badStages({ share: [0, 0.5, 0.4, 0.6] }, /must not fall/);
  badStages({ extra: 1 }, /stages: unknown extra/);
  assert.throws(() => validateGrass({ ...grass(), stages: undefined }, blocks, decorations), /stages must be an object/);
  // A fixture's slower stages by data alone.
  const slow = validateGrass(stages({ days: [10, 10, 10], share: [0, 0, 0, 0.9] }), blocks, decorations);
  assert.match(grassRust(slow), /GrassStage \{ value: "1", days: 10\.0, share: 0\.0 \}/);
  assert.match(grassRust(slow), /GrassStage \{ value: "3", days: 0\.0, share: 0\.9 \}/);
});

const loadClimate = () => JSON.parse(readFileSync(join(here, 'climate.json'), 'utf8'));

test('the checked-in climate is valid and generates the checked-in Rust', () => {
  const rs = climateRust(validateClimate(loadClimate()));
  const checkedIn = readFileSync(join(here, '../../crates/dwell-worldgen/src/climate.gen.rs'), 'utf8');
  assert.equal(rs, checkedIn, 'run `node shared/worldgen/gen.mjs`');
});

test('a latitude band and a wind cell added by data alone are emitted', () => {
  const c = loadClimate();
  c.bands.push({ name: 'polar', weight: 0.5, latitude: [76, 86], rainMm: 200 });
  c.winds.push({ name: 'doldrums', latitude: [0, 5], from: 90 });
  const rs = climateRust(validateClimate(c));
  assert.match(rs, /Band \{ name: "polar", weight: 0\.5, latitude: \[76\.0, 86\.0\], rain_mm: 200\.0 \}/);
  assert.match(rs, /WindCell \{ name: "doldrums", latitude: \[0\.0, 5\.0\], from: 90\.0 \}/);
});

test('a bad climate entry fails naming it', () => {
  for (const [edit, message] of [
    [(c) => (c.bands[0].latitude = [40, 30]), /bands\[0\]\.latitude must be \[south, north\]/],
    [(c) => (c.bands[1].name = c.bands[0].name), /bands\[1\]: subtropical is named twice/],
    [(c) => (c.bands[0].colour = 'red'), /bands\[0\]: unknown colour/],
    [(c) => c.seaLevel.reverse(), /seaLevel\[1\]: latitudes must ascend/],
    [(c) => (c.winds = [{ name: 'westerlies', latitude: [30, 60], from: 240 }]), /bands\[0\] \(subtropical\): no wind cell covers its middle latitude 21/],
    [(c) => (c.rain.maxCells = 1024), /rain\.maxCells 1024 a side needs more than maxGridBytes/],
    [(c) => (c.rain.cell = 50), /rain\.cell must be in \[100, 10000\]/],
    [(c) => (c.discharge = { min: 2, max: 4 }), /discharge must be \[min ≤ 1, max ≥ 1\]/],
    [(c) => delete c.lapseRate, /the file: missing lapseRate/],
    [(c) => delete c.bands[0].rainMm, /bands\[0\]: missing rainMm/],
    [(c) => (c.biomes.wetMm = 0), /biomes\.wetMm must be in \[1, 50000\]/],
    [(c) => (c.biomes.moistureShare = 2), /biomes\.moistureShare must be in \[0, 1\]/],
  ]) {
    const c = loadClimate();
    edit(c);
    assert.throws(() => validateClimate(c), message);
  }
});

// ---- Resurrection shrines (shrines.json, docs/SHRINES.md) -----------------------------------------

import { shrineStructure, shrinesRust, validateShrines } from './gen.mjs';

const loadShrines = () =>
  JSON.parse(readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'shrines.json'), 'utf8'));

test('the shrine design builds the owner\'s shrine: 8 × 8 m, a half circle of stepping pillars, the tallest half joined over a quarter circle', () => {
  // Owner, 2026-11-12: "a 8x8m terraced and filled stone circle with stepping pillars forming a
  // half circle. Only the tallest pillars (half of them) are connected at the top. Forming a
  // quarter circle."
  const d = loadShrines();
  const { structure: s, pillars, lintel } = shrineStructure(d.structure, d.design, registryBlocks());
  assert.equal(s.size[0], 8);
  assert.equal(s.size[2], 8);
  // Filled from the bottom row: every column of the circle has stone from row 0 up (no hollow).
  const at = (x, y, z) => s.layers[y][z][x];
  let floor = 0;
  for (let z = 0; z < 8; z++)
    for (let x = 0; x < 8; x++) {
      if (at(x, 0, z) === '.') continue;
      floor++;
      for (let y = 0; y < d.design.base / 0.5; y++) assert.notEqual(at(x, y, z), '.', `${x},${y},${z}`);
    }
  assert.ok(floor >= 48, `${floor} floor columns`);
  // Terraced: the centre stands higher than the rim by a row a step.
  const top = (x, z) => {
    let t = 0;
    while (t < s.size[1] && at(x, t, z) === 's') t++;
    return t;
  };
  assert.equal(top(4, 4) - top(4, 7), d.design.steps.length);
  // The pillars: a half circle (180° to 360°, the −z half), all on one side of the axis.
  const ids = [...new Set(pillars.map((p) => p.i))];
  assert.equal(ids.length, d.design.pillars.count);
  const degs = ids.map((i) => pillars.find((p) => p.i === i).deg);
  assert.equal(Math.min(...degs), 180);
  assert.equal(Math.max(...degs), 360);
  for (const p of pillars) assert.ok(p.z <= 4, `${p.x},${p.z} on the back half`);
  // Heights step up toward the arc's middle and back down: symmetric, strictly rising to it.
  const h = ids.map((i) => pillars.find((p) => p.i === i).height);
  const mid = (h.length - 1) / 2;
  for (let i = 0; i < mid; i++) {
    assert.ok(h[i + 1] > h[i], `rising ${h}`);
    assert.equal(h[i], h[h.length - 1 - i], `symmetric ${h}`);
  }
  // The joined ones are the tallest half, spanning a quarter circle (90°).
  const joined = ids.filter((i) => pillars.find((p) => p.i === i).joined);
  assert.equal(joined.length, (d.design.pillars.count + 1) / 2);
  const jh = joined.map((i) => h[i]);
  const rest = ids.filter((i) => !joined.includes(i)).map((i) => h[i]);
  assert.ok(Math.min(...jh) > Math.max(...rest), 'the joined are the tallest');
  const jd = joined.map((i) => degs[i]);
  assert.equal(Math.max(...jd) - Math.min(...jd), 90);
  // The lintel lies only between joined pillars, at their tops, and every lintel cell is filled.
  assert.ok(lintel.length > 0);
  for (const p of pillars.filter((q) => !q.joined)) {
    assert.ok(!lintel.includes(`${p.x},${p.z}`), 'no lintel over a lower pillar');
    for (let y = p.top; y < s.size[1]; y++) assert.equal(at(p.x, y, p.z), '.', 'open over it');
  }
  const lintelCells = s.layers.flatMap((l) => l.join('')).join('').split('').filter((c) => c === 'l');
  assert.ok(lintelCells.length > 0);
  // The light stands at the centre of the dais.
  assert.equal(at(4, top(4, 4), 4), 't');
});

test('the shrine rules validate, each world\'s overrides too, and the generated table is current', () => {
  const d = loadShrines();
  const v = validateShrines(d);
  assert.equal(v.rules.spacing, 2000);
  const rs = shrinesRust(v);
  assert.equal(
    rs,
    readFileSync(join(dirname(fileURLToPath(import.meta.url)), '../../crates/dwell-worldgen/src/shrines.gen.rs'), 'utf8'),
  );
  // A fixture world's overrides by data alone.
  const w = validateShrines({ ...d, worlds: { 'test:fixture': { spacing: 900 } } });
  assert.equal(w.worlds[0][1].spacing, 900);
  assert.match(shrinesRust(w), /"test:fixture"/);
  const bad = (patch, msg) => assert.throws(() => validateShrines({ ...d, ...patch }), msg);
  bad({ spacing: 10 }, /spacing must be/);
  bad({ jitter: 2 }, /jitter must be/);
  bad({ structure: 'dwell:nothing' }, /structure "dwell:nothing" is not in/);
  bad({ stand: [0, 9] }, /stand \[0,9\] lies outside/);
  // Under the lintel: not open over the floor.
  bad({ stand: [-2, -3] }, /stand \[-2,-3\] must stand on a floor/);
  bad({ map: { ...d.map, reveal: 'sometimes' } }, /map.reveal must be one of/);
  bad({ worlds: { x: { design: {} } } }, /worlds.x: unknown design/);
  bad({ lodCellM: 12 }, /power of two/);
  assert.throws(
    () => shrineStructure(d.structure, { ...d.design, pillars: { ...d.design.pillars, count: 8 } }, registryBlocks()),
    /pillars.count must be 1 more than a multiple of 4/,
  );
});
