// The biomes' and the terrain's validation, without Node (ADR 0074): shared by gen.mjs (which emits
// the Rust) and Chisel, the biome editor page (client/chisel.html), so both check a biome by the
// same rules. Pure functions over parsed JSON; `blocks` is a Map of block name → { shape, fluid }
// and `refs` the decoration and placement rule names ({ decorations, placements }: Sets).

const isNum = (v) => typeof v === 'number' && Number.isFinite(v);

// The terrain's groups (terrain.json), each key's range: [min, max, whole number?]. A minimum given
// as `{ above: v }` is exclusive. The base height field (inland, seaFloor, hills, bumps) is the world
// map's input; the rest shapes each column from the map (ADR 0049).
export const TERRAIN = {
  inland: { rise: [0, 1000], halfDistance: [{ above: 0 }, 100_000] },
  seaFloor: { depth: [0, 1000], shelf: [{ above: 0 }, 100_000] },
  islands: { amplitude: [0, 10_000], wavelength: [64, 65_536, true], octaves: [1, 8, true], cap: [-10_000, 100_000] },
  hills: {
    amplitude: [0, 1000],
    wavelength: [8, 65_536, true],
    octaves: [1, 8, true],
    fadeFrom: [0, 100_000],
    fadeTo: [0, 100_000],
  },
  bumps: { amplitude: [0, 100], wavelength: [4, 4096, true], octaves: [1, 8, true] },
  detail: {
    amplitude: [0, 100],
    wavelength: [4, 4096, true],
    octaves: [1, 8, true],
    mountainAmplitude: [0, 100],
    mountainFrom: [0, 10_000],
    mountainFull: [0, 10_000],
    waterFade: [0, 1000],
  },
  rivers: {
    smooth: [0, 6, true],
    minDepth: [0.5, 50],
    bank: [0, 20],
    joinMargin: [0, 50],
    leveeFall: [{ above: 0 }, 10],
    joinSlope: [{ above: 0 }, 10],
    reach: [0, 1000],
    reachPerWidth: [0, 50],
    meander: [0, 100],
    meanderWavelength: [4, 4096, true],
    lodWidth: [0, 4],
    lodMinShare: [0, 1],
    lodMaxCell: [1, 4096, true],
  },
  mouths: {
    reach: [0, 20_000],
    estuaryWiden: [1, 20],
    estuaryDeepen: [1, 10],
    deltaWiden: [1, 20],
    deltaBranches: [0, 8, true],
    deltaSplit: [0, 20_000],
    deltaSpread: [0, 89],
    deltaBranchWidth: [{ above: 0 }, 1],
  },
  lakes: {
    levee: [0, 1000],
    shoreNoise: [0, 200],
    shoreWavelength: [4, 4096, true],
  },
  shaping: {
    edgeNoise: [0, 10_000],
    edgeWavelength: [4, 65_536, true],
    edgeSlopeLift: [0, 10_000],
    rockSlope: [{ above: 0 }, 100],
    rockSlopeHigh: [{ above: 0 }, 100],
    rockHigh: [{ above: 0 }, 100_000],
    rockWindow: [{ above: 0 }, 2000],
    shore: [0, 100],
    underDepth: [0, 100],
  },
  banks: { riverSlope: [{ above: 0 }, 10], lake: [0, 20], lakeSlope: [{ above: 0 }, 10], beach: [0, 100] },
  biomeMap: {
    climateWavelength: [64, 1_000_000, true],
    climateNoise: [0, 1],
    lapseRate: [0, 10],
    wetReach: [{ above: 0 }, 100_000],
    ditherWavelength: [4, 65_536, true],
    dither: [0, 1],
  },
  spawn: { minHeight: [0, 10_000], maxHeight: [0, 10_000], nearRiver: [0, 100_000] },
};
/** An elevation band's blocks, by name: each must be a full block of the registry. */
const BAND_BLOCKS = ['ground', 'under', 'rock', 'shoreBlock'];
/** The geography fields a biome's `where` ranges may name (B2's biome map; code, a fixed vocabulary). */
export const WHERE_FIELDS = ['temperature', 'wetness', 'elevation', 'coast'];
const BAND_NAME = /^[a-z0-9_]+$/;

/**
 * The world's elevation and water (terrain.json): one world-scale system, not per biome (ADR 0074;
 * owner, 2026-11-10). The rest of `TERRAIN` is each biome's.
 */
export const WORLD_GROUPS = ['inland', 'seaFloor', 'islands', 'hills', 'bumps', 'rivers', 'mouths', 'lakes', 'spawn', 'biomeMap'];
/** A biome's groups (shared/worldgen/biomes/*.json): layered on the elevation. */
export const BIOME_GROUPS = Object.keys(TERRAIN).filter((g) => !WORLD_GROUPS.includes(g));
/** A biome's reference fields beside its groups. */
const BIOME_REFS = ['where', 'bands', 'decorations', 'placements', 'water', 'waterLook'];
const BIOME_NAME = /^[a-z0-9_]+:[a-z0-9_]+$/;

/** Checks `groups` of `data` against `TERRAIN`'s ranges; `fail` throws with the group and key. */
export function checkGroups(data, groups, fail) {
  for (const group of groups) {
    const keys = TERRAIN[group];
    const g = data[group];
    if (typeof g !== 'object' || g === null) fail(`${group} must be an object`);
    const ge = Object.keys(g).filter((k) => k !== '$comment' && !(k in keys));
    if (ge.length) fail(`${group}: unknown ${ge.join(', ')}`);
    for (const [key, [min, max, whole]] of Object.entries(keys)) {
      const v = g[key];
      const low = typeof min === 'object' ? v > min.above : v >= min;
      const lo = typeof min === 'object' ? `(${min.above}` : `[${min}`;
      if (!(isNum(v) && low && v <= max && (!whole || Number.isInteger(v)))) {
        fail(`${group}.${key} must be ${whole ? 'a whole number ' : ''}in ${lo}, ${max}]`);
      }
    }
  }
}

/**
 * Checks a biome (shared/worldgen/biomes/<name>.json) against `blocks` and the rule names it may
 * reference (`refs.decorations`, `refs.placements`: sets of names); throws naming the biome, the
 * group and the key. Unknown fields fail.
 */
export function validateBiome(data, blocks, refs) {
  const at = typeof data?.name === 'string' ? data.name : '?';
  const fail = (m) => {
    throw new Error(`biome ${at}: ${m}`);
  };
  if (typeof data !== 'object' || data === null) fail('must be an object');
  if (typeof data.name !== 'string' || !BIOME_NAME.test(data.name)) fail('name must be "namespace:name"');
  const extra = Object.keys(data).filter((k) => !['$comment', 'name', ...BIOME_GROUPS, ...BIOME_REFS].includes(k));
  if (extra.length) fail(`unknown ${extra.join(', ')}`);
  checkGroups(data, BIOME_GROUPS, fail);
  if (!(data.detail.mountainFull > data.detail.mountainFrom)) {
    fail('detail.mountainFull must be greater than detail.mountainFrom');
  }
  checkWhere(data.where, fail);
  checkBands(data.bands, blocks, fail);
  for (const k of ['decorations', 'placements']) {
    const names = data[k];
    if (!Array.isArray(names)) fail(`${k} must be a list of rule names`);
    for (const n of names) {
      if (!refs[k].has(n)) fail(`${k}: ${JSON.stringify(n)} is not a rule of ${k}.json`);
    }
    if (new Set(names).size !== names.length) fail(`${k}: a rule is listed twice`);
  }
  const w = blocks.get(data.water);
  if (!w) fail(`water ${JSON.stringify(data.water)} is not in the registry`);
  if (!w.fluid) fail(`water ${data.water} must be a fluid block`);
  if (data.waterLook !== undefined) checkWaterLook(data.waterLook, fail);
  return data;
}

/** The fluid look's fields a biome's `waterLook` may set (its water's colour and clarity, ADR 0074 B4). */
export const WATER_LOOK_FIELDS = ['scatter', 'absorption'];
const HEX = /^#[0-9a-f]{6}$/i;

/**
 * Checks a biome's optional `waterLook`: its water's colour (`scatter`, sRGB "#rrggbb") and clarity
 * (`absorption`, per metre of red, green, blue in [0, 10]) over its fluid's look
 * (shared/blocks/dwell.json `fluids`); at least one, nothing else.
 */
function checkWaterLook(l, fail) {
  if (typeof l !== 'object' || l === null || Array.isArray(l)) fail('waterLook must be an object');
  const extra = Object.keys(l).filter((k) => !['$comment', ...WATER_LOOK_FIELDS].includes(k));
  if (extra.length) fail(`waterLook: unknown ${extra.join(', ')}`);
  if (!WATER_LOOK_FIELDS.some((k) => k in l)) fail('waterLook needs a scatter or an absorption');
  if (l.scatter !== undefined && !(typeof l.scatter === 'string' && HEX.test(l.scatter))) {
    fail('waterLook.scatter must be a colour "#rrggbb"');
  }
  if (
    l.absorption !== undefined &&
    !(Array.isArray(l.absorption) && l.absorption.length === 3 && l.absorption.every((v) => isNum(v) && v >= 0 && v <= 10))
  ) {
    fail('waterLook.absorption must be three numbers in [0, 10]');
  }
}

/** Checks a biome's `where` (its place in the geography): a weight and optional eased ranges. */
function checkWhere(w, fail) {
  if (typeof w !== 'object' || w === null || Array.isArray(w)) fail('where must be an object');
  const extra = Object.keys(w).filter((k) => !['$comment', 'weight', ...WHERE_FIELDS].includes(k));
  if (extra.length) fail(`where: unknown ${extra.join(', ')}`);
  if (!(isNum(w.weight) && w.weight > 0 && w.weight <= 1000)) fail('where.weight must be in (0, 1000]');
  for (const f of WHERE_FIELDS) {
    const r = w[f];
    if (r === undefined) continue;
    if (typeof r !== 'object' || r === null) fail(`where.${f} must be an object`);
    const re = Object.keys(r).filter((k) => !['min', 'max', 'blend'].includes(k));
    if (re.length) fail(`where.${f}: unknown ${re.join(', ')}`);
    for (const k of ['min', 'max']) {
      if (r[k] !== undefined && !(isNum(r[k]) && Math.abs(r[k]) <= 1_000_000)) fail(`where.${f}.${k} must be a number`);
    }
    if (r.min === undefined && r.max === undefined) fail(`where.${f} needs a min or a max`);
    if (r.min !== undefined && r.max !== undefined && !(r.max > r.min)) {
      fail(`where.${f}.max must be greater than its min`);
    }
    if (!(isNum(r.blend) && r.blend >= 0 && r.blend <= 1_000_000)) fail(`where.${f}.blend must be in [0, 1000000]`);
  }
}

/** Checks a biome's elevation bands: the lowest without `from`, each next one higher. */
function checkBands(bands, blocks, fail) {
  if (!Array.isArray(bands) || bands.length === 0) fail('bands must be a list of elevation bands');
  if (bands.length > 16) fail('bands: at most 16');
  let below = -Infinity;
  const names = new Set();
  bands.forEach((band, i) => {
    const at = `bands[${i}]`;
    if (typeof band !== 'object' || band === null) fail(`${at} must be an object`);
    const extra = Object.keys(band).filter((k) => !['$comment', 'name', 'from', ...BAND_BLOCKS].includes(k));
    if (extra.length) fail(`${at}: unknown ${extra.join(', ')}`);
    if (typeof band.name !== 'string' || !BAND_NAME.test(band.name)) fail(`${at}.name must be a lowercase name`);
    if (names.has(band.name)) fail(`bands: ${band.name} is listed twice`);
    names.add(band.name);
    if (i === 0) {
      if (band.from !== undefined) fail(`${at} (the lowest) has no from: it starts at the sea floor`);
    } else {
      if (!(isNum(band.from) && band.from >= 0 && band.from <= 100_000)) fail(`${at}.from must be in [0, 100000]`);
      if (!(band.from > below)) fail(`${at}.from must be higher than the band below's`);
      below = band.from;
    }
    for (const k of BAND_BLOCKS) {
      const id = band[k];
      const b = blocks.get(id);
      if (!b) fail(`${at}.${k} ${JSON.stringify(id)} is not in the registry`);
      if (b.shape !== 'full') fail(`${at}.${k} ${id} must be a full block`);
    }
  });
}

/**
 * Checks a `biomeMap` group (terrain.json's, or Chisel's edit of it) against `TERRAIN`'s ranges;
 * throws naming the key.
 */
export function validateBiomeMap(bm) {
  checkGroups({ biomeMap: bm }, ['biomeMap'], (m) => {
    throw new Error(`terrain.json: ${m}`);
  });
  return bm;
}

/**
 * The world knobs Chisel may change (ADR 0074 B4, the archipelago's: the coast's islands, the sea
 * floor's shelf, the inland rise, the hills' height): terrain.json's groups and keys.
 */
export const WORLD_KNOBS = {
  islands: ['amplitude', 'wavelength', 'octaves', 'cap'],
  seaFloor: ['depth', 'shelf'],
  inland: ['rise', 'halfDistance'],
  hills: ['amplitude'],
};

/** Checks Chisel's world knobs (`{ group: { key: v } }`, `WORLD_KNOBS`'s) by `TERRAIN`'s ranges. */
export function validateWorldKnobs(w) {
  const fail = (m) => {
    throw new Error(`world: ${m}`);
  };
  if (typeof w !== 'object' || w === null || Array.isArray(w)) fail('must be an object');
  for (const [g, keys] of Object.entries(w)) {
    if (!(g in WORLD_KNOBS)) fail(`${g} is not a world knob`);
    for (const [k, v] of Object.entries(keys ?? {})) {
      if (!WORLD_KNOBS[g].includes(k)) fail(`${g}.${k} is not a world knob`);
      const [min, max, whole] = TERRAIN[g][k];
      const low = typeof min === 'object' ? v > min.above : v >= min;
      if (!(isNum(v) && low && v <= max && (!whole || Number.isInteger(v)))) fail(`${g}.${k} is out of range`);
    }
  }
  return w;
}
