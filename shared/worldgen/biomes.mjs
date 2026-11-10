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
    windDirection: [0, 360],
    windReach: [{ above: 0 }, 1_000_000],
    baseRain: [0, 1],
    upliftRain: [0, 10],
    leeDry: [0, 10],
    rainShare: [0, 1],
    windSmoothM: [0, 50_000],
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
const BIOME_REFS = ['where', 'bands', 'decorations', 'placements', 'water', 'waterLook', 'rockStack'];
/** A biome's optional words for people (Chisel's lists): a short label and a one-line description. */
export const BIOME_TEXT = { label: 40, description: 200 };
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
  const extra = Object.keys(data).filter(
    (k) => !['$comment', 'name', ...BIOME_GROUPS, ...BIOME_REFS, ...Object.keys(BIOME_TEXT)].includes(k),
  );
  if (extra.length) fail(`unknown ${extra.join(', ')}`);
  for (const [k, max] of Object.entries(BIOME_TEXT)) {
    const t = data[k];
    if (t !== undefined && !(typeof t === 'string' && t.trim().length > 0 && t.length <= max)) {
      fail(`${k} must be text of 1 to ${max} characters`);
    }
  }
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
  if (data.rockStack !== undefined) {
    if (typeof data.rockStack !== 'string') fail('rockStack must be a stack name');
    if (refs.stacks && !refs.stacks.has(data.rockStack)) {
      fail(`rockStack: ${JSON.stringify(data.rockStack)} is not a stack of rocks.json`);
    }
  }
  return data;
}

/** A rock's jointing (code, a fixed vocabulary; systemic terrain T2). */
export const JOINTING = ['bedded', 'vertical', 'massive'];
/** The geology's groups' ranges (rocks.json `provinces`, `shaping`), as `TERRAIN`'s. */
export const GEOLOGY = {
  provinces: { wavelength: [256, 1_000_000, true] },
  shaping: { strength: [0, 1], tread: [0, 10], sheer: [0, 10] },
};

/**
 * Checks the geology (shared/worldgen/rocks.json, systemic terrain T2): the rock types, the strata
 * stacks (layers top to bottom, each a known rock), the provinces and the shaping; throws naming the
 * rock, the stack or the key. `blocks` as `validateBiome`'s: a rock's blocks must be full blocks.
 */
export function validateRocks(data, blocks) {
  const fail = (m) => {
    throw new Error(`rocks.json: ${m}`);
  };
  if (typeof data !== 'object' || data === null) fail('must be an object');
  const extra = Object.keys(data).filter((k) => !['$comment', 'rocks', 'stacks', 'provinces', 'shaping'].includes(k));
  if (extra.length) fail(`unknown ${extra.join(', ')}`);
  const between = (v, lo, hi) => isNum(v) && v >= lo && v <= hi;
  if (!Array.isArray(data.rocks) || data.rocks.length === 0) fail('rocks must be a list of at least one rock');
  const rocks = new Set();
  for (const r of data.rocks) {
    const at = `rock ${typeof r?.name === 'string' ? r.name : '?'}`;
    const rf = (m) => fail(`${at}: ${m}`);
    if (typeof r.name !== 'string' || !BIOME_NAME.test(r.name)) rf('name must be "namespace:name"');
    if (rocks.has(r.name)) rf('is listed twice');
    rocks.add(r.name);
    const keys = ['name', 'hardness', 'jointing', 'bed', 'solubility', 'face', 'cap', 'scree', 'color', '$comment'];
    const ex = Object.keys(r).filter((k) => !keys.includes(k));
    if (ex.length) rf(`unknown ${ex.join(', ')}`);
    if (!between(r.hardness, 0, 1)) rf('hardness must be in [0, 1]');
    if (!between(r.solubility, 0, 1)) rf('solubility must be in [0, 1]');
    if (!JOINTING.includes(r.jointing)) rf(`jointing must be one of ${JOINTING.join(', ')}`);
    if (!(isNum(r.bed) && r.bed >= 1 && r.bed <= 1000)) rf('bed must be in [1, 1000] m');
    for (const k of ['face', 'cap', 'scree']) {
      const b = blocks.get(r[k]);
      if (!b) rf(`${k} ${JSON.stringify(r[k])} is not in the registry`);
      if (b.shape !== 'full') rf(`${k} ${r[k]} must be a full block`);
    }
    if (typeof r.color !== 'string' || !/^#[0-9a-fA-F]{6}$/.test(r.color)) rf('color must be "#rrggbb"');
  }
  if (!Array.isArray(data.stacks) || data.stacks.length === 0) fail('stacks must be a list of at least one stack');
  const stacks = new Set();
  for (const s of data.stacks) {
    const at = `stack ${typeof s?.name === 'string' ? s.name : '?'}`;
    const sf = (m) => fail(`${at}: ${m}`);
    if (typeof s.name !== 'string' || !BIOME_NAME.test(s.name)) sf('name must be "namespace:name"');
    if (stacks.has(s.name)) sf('is listed twice');
    stacks.add(s.name);
    const ex = Object.keys(s).filter((k) => !['name', 'weight', 'layers', '$comment'].includes(k));
    if (ex.length) sf(`unknown ${ex.join(', ')}`);
    if (!between(s.weight, 0, 1000)) sf('weight must be in [0, 1000]');
    if (!Array.isArray(s.layers) || s.layers.length === 0) sf('layers must be a list of at least one layer');
    for (const l of s.layers) {
      if (!rocks.has(l?.rock)) sf(`layer rock ${JSON.stringify(l?.rock)} is not a rock of rocks.json`);
      if (!(isNum(l.thickness) && l.thickness >= 1 && l.thickness <= 2000)) sf('a layer thickness must be in [1, 2000] m');
      const ex2 = Object.keys(l).filter((k) => !['rock', 'thickness'].includes(k));
      if (ex2.length) sf(`a layer: unknown ${ex2.join(', ')}`);
    }
  }
  if (!data.stacks.some((s) => s.weight > 0)) fail('a stack must have a weight above 0');
  const groups = (g) => {
    for (const [group, keys] of Object.entries(GEOLOGY)) {
      const v = g[group];
      if (typeof v !== 'object' || v === null) fail(`${group} must be an object`);
      const ex = Object.keys(v).filter((k) => k !== '$comment' && !(k in keys));
      if (ex.length) fail(`${group}: unknown ${ex.join(', ')}`);
      for (const [key, [min, max, whole]] of Object.entries(keys)) {
        const x = v[key];
        if (!(isNum(x) && x >= min && x <= max && (!whole || Number.isInteger(x)))) {
          fail(`${group}.${key} must be ${whole ? 'a whole number ' : ''}in [${min}, ${max}]`);
        }
      }
    }
  };
  groups(data);
  return data;
}

/**
 * Checks the preset biomes (shared/worldgen/biomes/presets/*.json, starting points for Chisel, never
 * on the default world's map): each a valid biome with a `label` and a `description`, the names and
 * labels distinct, none named as a world biome (`taken`). Throws naming the preset.
 */
export function validatePresets(presets, blocks, refs, taken = []) {
  for (const b of presets) {
    validateBiome(b, blocks, refs);
    if (b.label === undefined || b.description === undefined) {
      throw new Error(`biome ${b.name}: a preset needs a label and a description`);
    }
  }
  for (const k of ['name', 'label']) {
    const vs = presets.map((b) => b[k]);
    const dup = vs.find((v, i) => vs.indexOf(v) !== i || (k === 'name' && taken.includes(v)));
    if (dup !== undefined) throw new Error(`preset ${k} ${dup} is used twice`);
  }
  return presets;
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
