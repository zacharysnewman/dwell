// The biomes' and the terrain's validation, without Node (ADR 0074): shared by gen.mjs (which emits
// the Rust) and Chisel, the biome editor page (client/chisel.html), so both check a biome by the
// same rules. Pure functions over parsed JSON; `blocks` is a Map of block name → { shape, fluid }
// and `refs` the decoration and placement rule names ({ decorations, placements }: Sets).

const isNum = (v) => typeof v === "number" && Number.isFinite(v);

// The terrain's groups (terrain.json), each key's range: [min, max, whole number?]. A minimum given
// as `{ above: v }` is exclusive. The base height field (inland, seaFloor, hills, bumps) is the world
// map's input; the rest shapes each column from the map (ADR 0049).
export const TERRAIN = {
  inland: { rise: [0, 1000], halfDistance: [{ above: 0 }, 100_000] },
  seaFloor: { depth: [0, 1000], shelf: [{ above: 0 }, 100_000] },
  islands: {
    amplitude: [0, 10_000],
    wavelength: [64, 65_536, true],
    octaves: [1, 8, true],
    cap: [-10_000, 100_000],
  },
  // Systemic terrain T5: one plate boundary (kind 0 none, 1 convergent, 2 divergent, 3 transform), its
  // volcanic arc and one hotspot chain (tectonics.rs).
  tectonics: {
    kind: [0, 3, true],
    angle: [-360, 360],
    offset: [-50_000, 50_000],
    width: [{ above: 0 }, 20_000],
    rangeHeight: [0, 3000],
    trenchDepth: [0, 1000],
    riftDepth: [0, 1000],
    shift: [0, 20_000],
    arcCount: [0, 32, true],
    arcDistance: [0, 30_000],
    arcSpacing: [{ above: 0 }, 50_000],
    coneHeight: [0, 8000],
    arcDecay: [0, 1],
    coneRadius: [{ above: 0 }, 20_000],
    craterRadius: [0, 5000],
    craterDepth: [0, 1000],
    coneHardness: [0, 1],
    ridgeCount: [0, 64, true],
    ridgeDepth: [0, 3000],
    ridgeWarp: [0, 2],
    summitOffset: [0, 10_000],
    summitAngle: [-360, 360],
    shoulderHeight: [0, 3000],
    shoulderDistance: [0, 20_000],
    shoulderAngle: [-360, 360],
    shoulderRadius: [{ above: 0 }, 20_000],
    ventCount: [0, 16, true],
    ventHeight: [0, 2000],
    ventRadius: [{ above: 0 }, 10_000],
    ventDistance: [0, 20_000],
    iceAbove: [0, 100_000],
    iceDome: [0, 1000],
    snowBelowC: [-100, 50],
    iceBelowC: [-100, 50],
    snowMaxSlope: [0, 10],
    hotspotCount: [0, 32, true],
    hotspotX: [-50_000, 50_000],
    hotspotZ: [-50_000, 50_000],
    hotspotHeading: [-360, 360],
    hotspotSpacing: [{ above: 0 }, 50_000],
    hotspotHeight: [0, 5000],
    hotspotRadius: [{ above: 0 }, 20_000],
    hotspotDecay: [0, 1],
    hotspotWiden: [0, 2],
    coreDistance: [0, 50_000],
  },
  hills: {
    amplitude: [0, 1000],
    wavelength: [8, 65_536, true],
    octaves: [1, 8, true],
    fadeFrom: [0, 100_000],
    fadeTo: [0, 100_000],
  },
  bumps: {
    amplitude: [0, 100],
    wavelength: [4, 4096, true],
    octaves: [1, 8, true],
  },
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
  banks: {
    riverSlope: [{ above: 0 }, 10],
    lake: [0, 20],
    lakeSlope: [{ above: 0 }, 10],
    beach: [0, 100],
  },
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
    sharpen: [1, 16],
  },
  spawn: {
    minHeight: [0, 10_000],
    maxHeight: [0, 10_000],
    nearRiver: [0, 100_000],
  },
};
/** An elevation band's blocks, by name: each must be a full block of the registry. */
const BAND_BLOCKS = ["ground", "under", "rock", "shoreBlock"];
/** The geography fields a biome's `where` ranges may name (B2's biome map; code, a fixed vocabulary). */
export const WHERE_FIELDS = ["temperature", "wetness", "elevation", "coast"];
const BAND_NAME = /^[a-z0-9_]+$/;

/**
 * The world's elevation and water (terrain.json): one world-scale system, not per biome (ADR 0074;
 * owner, 2026-11-10). The rest of `TERRAIN` is each biome's.
 */
export const WORLD_GROUPS = [
  "inland",
  "seaFloor",
  "islands",
  "tectonics",
  "hills",
  "bumps",
  "rivers",
  "mouths",
  "lakes",
  "spawn",
  "biomeMap",
];
/** A biome's groups (shared/worldgen/biomes/*.json): layered on the elevation. */
export const BIOME_GROUPS = Object.keys(TERRAIN).filter(
  (g) => !WORLD_GROUPS.includes(g),
);
/** A biome's reference fields beside its groups. */
const BIOME_REFS = [
  "where",
  "bands",
  "decorations",
  "placements",
  "trees",
  "ground",
  "water",
  "waterLook",
  "rockStack",
  "vegetation",
  "erosion",
  "gullies",
  "arches",
];
/** A biome's optional words for people (Chisel's lists): a short label and a one-line description. */
export const BIOME_TEXT = { label: 40, description: 200 };
const BIOME_NAME = /^[a-z0-9_]+:[a-z0-9_]+$/;

/** Checks `groups` of `data` against `TERRAIN`'s ranges; `fail` throws with the group and key. */
export function checkGroups(data, groups, fail) {
  for (const group of groups) {
    const keys = TERRAIN[group];
    const g = data[group];
    if (typeof g !== "object" || g === null) fail(`${group} must be an object`);
    const ge = Object.keys(g).filter((k) => k !== "$comment" && !(k in keys));
    if (ge.length) fail(`${group}: unknown ${ge.join(", ")}`);
    for (const [key, [min, max, whole]] of Object.entries(keys)) {
      const v = g[key];
      const low = typeof min === "object" ? v > min.above : v >= min;
      const lo = typeof min === "object" ? `(${min.above}` : `[${min}`;
      if (!(isNum(v) && low && v <= max && (!whole || Number.isInteger(v)))) {
        fail(
          `${group}.${key} must be ${whole ? "a whole number " : ""}in ${lo}, ${max}]`,
        );
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
  const at = typeof data?.name === "string" ? data.name : "?";
  const fail = (m) => {
    throw new Error(`biome ${at}: ${m}`);
  };
  if (typeof data !== "object" || data === null) fail("must be an object");
  if (typeof data.name !== "string" || !BIOME_NAME.test(data.name))
    fail('name must be "namespace:name"');
  const extra = Object.keys(data).filter(
    (k) =>
      ![
        "$comment",
        "name",
        ...BIOME_GROUPS,
        ...BIOME_REFS,
        ...Object.keys(BIOME_TEXT),
      ].includes(k),
  );
  if (extra.length) fail(`unknown ${extra.join(", ")}`);
  for (const [k, max] of Object.entries(BIOME_TEXT)) {
    const t = data[k];
    if (
      t !== undefined &&
      !(typeof t === "string" && t.trim().length > 0 && t.length <= max)
    ) {
      fail(`${k} must be text of 1 to ${max} characters`);
    }
  }
  checkGroups(data, BIOME_GROUPS, fail);
  if (!(data.detail.mountainFull > data.detail.mountainFrom)) {
    fail("detail.mountainFull must be greater than detail.mountainFrom");
  }
  checkWhere(data.where, fail);
  checkBands(data.bands, blocks, fail);
  for (const k of ["decorations", "placements"]) {
    const names = data[k];
    if (!Array.isArray(names)) fail(`${k} must be a list of rule names`);
    for (const n of names) {
      if (!refs[k].has(n))
        fail(`${k}: ${JSON.stringify(n)} is not a rule of ${k}.json`);
    }
    if (new Set(names).size !== names.length)
      fail(`${k}: a rule is listed twice`);
  }
  checkTrees(data.trees, data.placements, fail);
  checkGround(data.ground, refs.ground, fail);
  const w = blocks.get(data.water);
  if (!w) fail(`water ${JSON.stringify(data.water)} is not in the registry`);
  if (!w.fluid) fail(`water ${data.water} must be a fluid block`);
  if (data.waterLook !== undefined) checkWaterLook(data.waterLook, fail);
  if (data.rockStack !== undefined) {
    if (typeof data.rockStack !== "string")
      fail("rockStack must be a stack name");
    if (refs.stacks && !refs.stacks.has(data.rockStack)) {
      fail(
        `rockStack: ${JSON.stringify(data.rockStack)} is not a stack of rocks.json`,
      );
    }
  }
  if (
    data.vegetation !== undefined &&
    !(isNum(data.vegetation) && data.vegetation >= 0 && data.vegetation <= 1)
  ) {
    fail("vegetation must be in [0, 1]");
  }
  for (const k of ["erosion", "gullies"]) {
    if (
      data[k] !== undefined &&
      !(isNum(data[k]) && data[k] >= 0 && data[k] <= 1)
    )
      fail(`${k} must be in [0, 1]`);
  }
  checkArches(data.arches, fail);
  return data;
}

/**
 * A biome's optional `arches` (ADR 0074 B8): openings carved through narrow fins of rock. `density`
 * sites per km² (one chance per 32 m square, so at most `ARCHES.density[1]`), each range a `[min, max]`
 * pair in m: `span` the opening across the fin, `rise` its crown over the floor, `lintel` the rock left
 * above it; `fin` the thickest fin (m, along the way through) it cuts; `overhangs` (optional) also cuts
 * a scarp's site (low on one side) as an overhang from its low side. Absent or density 0: none.
 */
export const ARCHES = {
  density: [0, 976.5625],
  span: [2, 64],
  rise: [1, 48],
  lintel: [1, 32],
  fin: [2, 64],
};

/** Checks a biome's optional `arches` (`ARCHES`'s ranges; each range's min at most its max). */
export function checkArches(a, fail) {
  if (a === undefined) return;
  if (typeof a !== "object" || a === null || Array.isArray(a))
    fail("arches must be an object");
  const extra = Object.keys(a).filter(
    (k) => !["$comment", "overhangs"].includes(k) && !(k in ARCHES),
  );
  if (extra.length) fail(`arches: unknown ${extra.join(", ")}`);
  for (const [k, [lo, hi]] of Object.entries(ARCHES)) {
    const v = a[k];
    if (k === "density" || k === "fin") {
      if (!(isNum(v) && v >= lo && v <= hi))
        fail(`arches.${k} must be in [${lo}, ${hi}]`);
    } else if (!(
      Array.isArray(v) &&
      v.length === 2 &&
      v.every((x) => isNum(x) && x >= lo && x <= hi) &&
      v[0] <= v[1]
    )) {
      fail(`arches.${k} must be [min, max] in [${lo}, ${hi}], min ≤ max`);
    }
  }
  if (a.overhangs !== undefined && typeof a.overhangs !== "boolean")
    fail("arches.overhangs must be true or false");
}

/** The most a biome's `trees` may multiply a placement's chance by (ADR 0074 B5). */
export const TREE_DENSITY_MAX = 3;

/**
 * Checks a biome's optional `trees` (ADR 0074 B5): placement name → density, a multiplier on that
 * placement's chance where the biome has weight (1 when not given; 0 none). Each must be one of the
 * biome's `placements`.
 */
export function checkTrees(trees, placements, fail) {
  if (trees === undefined) return;
  if (typeof trees !== "object" || trees === null || Array.isArray(trees)) {
    fail("trees must map placement names to densities");
  }
  for (const [p, d] of Object.entries(trees)) {
    if (!placements.includes(p))
      fail(`trees: ${JSON.stringify(p)} is not one of its placements`);
    if (!(isNum(d) && d >= 0 && d <= TREE_DENSITY_MAX))
      fail(`trees.${p} must be in [0, ${TREE_DENSITY_MAX}]`);
  }
}

/** The most a biome's `ground` may multiply a ground item kind's density by (ADR 0074 B5). */
export const GROUND_DENSITY_MAX = 3;

/**
 * Checks a biome's optional `ground` (ADR 0074 B5): ground item kind (shared/ground/ground.json
 * `kinds`) → density, a multiplier on that kind's rules where the biome has weight. Without it
 * every kind takes 1 (the default biome: unchanged); with it a kind it does not name lies nowhere
 * it rules, as with `placements`. `kinds` (a set of names) is checked when given.
 */
export function checkGround(ground, kinds, fail) {
  if (ground === undefined) return;
  if (typeof ground !== "object" || ground === null || Array.isArray(ground)) {
    fail("ground must map ground item kinds to densities");
  }
  for (const [k, d] of Object.entries(ground)) {
    if (kinds && !kinds.has(k))
      fail(`ground: ${JSON.stringify(k)} is not a kind of ground.json`);
    if (!(isNum(d) && d >= 0 && d <= GROUND_DENSITY_MAX))
      fail(`ground.${k} must be in [0, ${GROUND_DENSITY_MAX}]`);
  }
}

/** The soil and vegetation's groups' ranges (vegetation.json `climate`, `soil`), as `TERRAIN`'s. */
export const VEGETATION = {
  climate: { zeroC: [-60, 60], perUnitC: [0, 200] },
  soil: { flatSlopeDeg: [1, 90], valley: [0, 10] },
};

/**
 * Checks the soil and vegetation (shared/worldgen/vegetation.json, systemic terrain T3): the climate's
 * °C scale, the soil rules, the soil types, each rock's soil (`rocks`: the rock names it may name; a
 * rock not listed takes `fallback`) and the vegetation kinds (each a placement's envelope;
 * `placements`: the names it may name). Throws naming the kind, the rock or the key.
 */
export function validateVegetation(data, rocks, placements) {
  const fail = (m) => {
    throw new Error(`vegetation.json: ${m}`);
  };
  if (typeof data !== "object" || data === null) fail("must be an object");
  const keys = [
    "$comment",
    "climate",
    "soil",
    "soilTypes",
    "fallback",
    "rockSoils",
    "kinds",
  ];
  const extra = Object.keys(data).filter((k) => !keys.includes(k));
  if (extra.length) fail(`unknown ${extra.join(", ")}`);
  for (const [group, ks] of Object.entries(VEGETATION)) {
    const v = data[group];
    if (typeof v !== "object" || v === null) fail(`${group} must be an object`);
    const ex = Object.keys(v).filter((k) => k !== "$comment" && !(k in ks));
    if (ex.length) fail(`${group}: unknown ${ex.join(", ")}`);
    for (const [key, [min, max]] of Object.entries(ks)) {
      if (!(isNum(v[key]) && v[key] >= min && v[key] <= max))
        fail(`${group}.${key} must be in [${min}, ${max}]`);
    }
  }
  if (
    !Array.isArray(data.soilTypes) ||
    data.soilTypes.length === 0 ||
    data.soilTypes.length > 32
  ) {
    fail("soilTypes must be a list of 1 to 32 soil types");
  }
  const types = data.soilTypes.map((t) => t?.name);
  for (const t of data.soilTypes) {
    if (typeof t.name !== "string" || !BIOME_NAME.test(t.name))
      fail('a soil type name must be "namespace:name"');
    if (typeof t.color !== "string" || !/^#[0-9a-fA-F]{6}$/.test(t.color))
      fail(`soil type ${t.name}: color must be "#rrggbb"`);
  }
  if (new Set(types).size !== types.length) fail("a soil type is listed twice");
  const soil = (s, at) => {
    if (!(isNum(s?.depth) && s.depth >= 0 && s.depth <= 50))
      fail(`${at}: depth must be in [0, 50] m`);
    if (!types.includes(s.type))
      fail(`${at}: type ${JSON.stringify(s.type)} is not a soil type`);
  };
  soil(data.fallback, "fallback");
  if (!Array.isArray(data.rockSoils)) fail("rockSoils must be a list");
  const seen = new Set();
  for (const r of data.rockSoils) {
    if (rocks && !rocks.has(r?.rock))
      fail(`rockSoils: ${JSON.stringify(r?.rock)} is not a rock of rocks.json`);
    if (seen.has(r.rock)) fail(`rockSoils: ${r.rock} is listed twice`);
    seen.add(r.rock);
    soil(r, `rock ${r.rock}`);
  }
  if (!Array.isArray(data.kinds)) fail("kinds must be a list");
  const names = new Set();
  const governed = new Set();
  for (const k of data.kinds) {
    const at = `kind ${typeof k?.name === "string" ? k.name : "?"}`;
    const kf = (m) => fail(`${at}: ${m}`);
    if (typeof k.name !== "string" || !BIOME_NAME.test(k.name))
      kf('name must be "namespace:name"');
    if (names.has(k.name)) kf("is listed twice");
    names.add(k.name);
    const ks = [
      "name",
      "placement",
      "temperature",
      "moisture",
      "minSoil",
      "maxSlopeDeg",
      "soils",
      "fade",
      "density",
      "$comment",
    ];
    const ex = Object.keys(k).filter((x) => !ks.includes(x));
    if (ex.length) kf(`unknown ${ex.join(", ")}`);
    if (placements && !placements.has(k.placement))
      kf(
        `placement ${JSON.stringify(k.placement)} is not a rule of placements.json`,
      );
    if (governed.has(k.placement))
      kf(`placement ${k.placement} already has a kind`);
    governed.add(k.placement);
    const range = (v, lo, hi, n) => {
      if (!(
        Array.isArray(v) &&
        v.length === 2 &&
        v.every(isNum) &&
        v[0] >= lo &&
        v[1] <= hi &&
        v[0] < v[1]
      )) {
        kf(
          `${n} must be [least, most] within [${lo}, ${hi}], least below most`,
        );
      }
    };
    range(k.temperature, -80, 80, "temperature");
    range(k.moisture, 0, 1, "moisture");
    if (!(isNum(k.minSoil) && k.minSoil >= 0 && k.minSoil <= 50))
      kf("minSoil must be in [0, 50] m");
    if (!(isNum(k.maxSlopeDeg) && k.maxSlopeDeg > 0 && k.maxSlopeDeg <= 90))
      kf("maxSlopeDeg must be in (0, 90]");
    if (!(isNum(k.density) && k.density >= 0 && k.density <= 1))
      kf("density must be in [0, 1]");
    if (!Array.isArray(k.soils) || !k.soils.every((s) => types.includes(s)))
      kf("soils must be a list of soil types");
    const f = k.fade;
    if (typeof f !== "object" || f === null) kf("fade must be an object");
    for (const [key, hi] of [
      ["temperature", 40],
      ["moisture", 1],
      ["soil", 10],
    ]) {
      if (!(isNum(f[key]) && f[key] >= 0 && f[key] <= hi))
        kf(`fade.${key} must be in [0, ${hi}]`);
    }
    const fx = Object.keys(f).filter(
      (x) => !["temperature", "moisture", "soil"].includes(x),
    );
    if (fx.length) kf(`fade: unknown ${fx.join(", ")}`);
  }
  return data;
}

/** A rock's jointing (code, a fixed vocabulary; systemic terrain T2). */
export const JOINTING = ["bedded", "vertical", "massive"];
/** The geology's groups' ranges (rocks.json `provinces`, `shaping`), as `TERRAIN`'s. */
export const GEOLOGY = {
  provinces: { wavelength: [256, 1_000_000, true] },
  shaping: { strength: [0, 1], tread: [0, 10], sheer: [0, 10] },
  erosion: {
    strength: [0, 1],
    reach: [0, 10_000],
    valley: [0, 4],
    resist: [0, 1],
    relief: [0, 100],
    threshold: [1, 100_000, true],
    snap: [0, 1],
    wall: [0.5, 50],
    talusWidth: [0, 500],
    talusShare: [0, 1],
    outline: [0, 1],
    outlineWavelength: [16, 100_000, true],
    gullyWavelength: [4, 10_000, true],
    gullyDepth: [0, 100],
  },
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
  if (typeof data !== "object" || data === null) fail("must be an object");
  const extra = Object.keys(data).filter(
    (k) =>
      ![
        "$comment",
        "rocks",
        "stacks",
        "provinces",
        "shaping",
        "erosion",
        "tectonic",
      ].includes(k),
  );
  if (extra.length) fail(`unknown ${extra.join(", ")}`);
  const between = (v, lo, hi) => isNum(v) && v >= lo && v <= hi;
  if (!Array.isArray(data.rocks) || data.rocks.length === 0)
    fail("rocks must be a list of at least one rock");
  const rocks = new Set();
  for (const r of data.rocks) {
    const at = `rock ${typeof r?.name === "string" ? r.name : "?"}`;
    const rf = (m) => fail(`${at}: ${m}`);
    if (typeof r.name !== "string" || !BIOME_NAME.test(r.name))
      rf('name must be "namespace:name"');
    if (rocks.has(r.name)) rf("is listed twice");
    rocks.add(r.name);
    const keys = [
      "name",
      "hardness",
      "jointing",
      "bed",
      "solubility",
      "face",
      "cap",
      "scree",
      "color",
      "$comment",
    ];
    const ex = Object.keys(r).filter((k) => !keys.includes(k));
    if (ex.length) rf(`unknown ${ex.join(", ")}`);
    if (!between(r.hardness, 0, 1)) rf("hardness must be in [0, 1]");
    if (!between(r.solubility, 0, 1)) rf("solubility must be in [0, 1]");
    if (!JOINTING.includes(r.jointing))
      rf(`jointing must be one of ${JOINTING.join(", ")}`);
    if (!(isNum(r.bed) && r.bed >= 1 && r.bed <= 1000))
      rf("bed must be in [1, 1000] m");
    for (const k of ["face", "cap", "scree"]) {
      const b = blocks.get(r[k]);
      if (!b) rf(`${k} ${JSON.stringify(r[k])} is not in the registry`);
      if (b.shape !== "full") rf(`${k} ${r[k]} must be a full block`);
    }
    if (typeof r.color !== "string" || !/^#[0-9a-fA-F]{6}$/.test(r.color))
      rf('color must be "#rrggbb"');
  }
  if (!Array.isArray(data.stacks) || data.stacks.length === 0)
    fail("stacks must be a list of at least one stack");
  const stacks = new Set();
  for (const s of data.stacks) {
    const at = `stack ${typeof s?.name === "string" ? s.name : "?"}`;
    const sf = (m) => fail(`${at}: ${m}`);
    if (typeof s.name !== "string" || !BIOME_NAME.test(s.name))
      sf('name must be "namespace:name"');
    if (stacks.has(s.name)) sf("is listed twice");
    stacks.add(s.name);
    const ex = Object.keys(s).filter(
      (k) => !["name", "weight", "layers", "$comment"].includes(k),
    );
    if (ex.length) sf(`unknown ${ex.join(", ")}`);
    if (!between(s.weight, 0, 1000)) sf("weight must be in [0, 1000]");
    if (!Array.isArray(s.layers) || s.layers.length === 0)
      sf("layers must be a list of at least one layer");
    for (const l of s.layers) {
      if (!rocks.has(l?.rock))
        sf(`layer rock ${JSON.stringify(l?.rock)} is not a rock of rocks.json`);
      if (!(isNum(l.thickness) && l.thickness >= 1 && l.thickness <= 2000))
        sf("a layer thickness must be in [1, 2000] m");
      const ex2 = Object.keys(l).filter(
        (k) => !["rock", "thickness"].includes(k),
      );
      if (ex2.length) sf(`a layer: unknown ${ex2.join(", ")}`);
    }
  }
  if (!data.stacks.some((s) => s.weight > 0))
    fail("a stack must have a weight above 0");
  const groups = (g) => {
    for (const [group, keys] of Object.entries(GEOLOGY)) {
      const v = g[group];
      if (typeof v !== "object" || v === null)
        fail(`${group} must be an object`);
      const ex = Object.keys(v).filter((k) => k !== "$comment" && !(k in keys));
      if (ex.length) fail(`${group}: unknown ${ex.join(", ")}`);
      for (const [key, [min, max, whole]] of Object.entries(keys)) {
        const x = v[key];
        if (!(
          isNum(x) &&
          x >= min &&
          x <= max &&
          (!whole || Number.isInteger(x))
        )) {
          fail(
            `${group}.${key} must be ${whole ? "a whole number " : ""}in [${min}, ${max}]`,
          );
        }
      }
    }
  };
  groups(data);
  // The tectonic provinces (systemic terrain T5): the stacks of the volcanoes and the old core.
  const t = data.tectonic;
  if (typeof t !== "object" || t === null) fail("tectonic must be an object");
  const te = Object.keys(t).filter(
    (k) => !["$comment", "volcanic", "core", "snow", "ice"].includes(k),
  );
  if (te.length) fail(`tectonic: unknown ${te.join(", ")}`);
  for (const k of ["volcanic", "core"]) {
    if (!stacks.has(t[k]))
      fail(`tectonic.${k} must name a stack (${String(t[k])} is none)`);
  }
  // The snow line's cover (terrain.json tectonics.snowBelowC): a full block.
  const sb = blocks.get(t.snow);
  if (!sb)
    fail(`tectonic.snow ${JSON.stringify(t.snow)} is not in the registry`);
  if (sb.shape !== "full") fail(`tectonic.snow ${t.snow} must be a full block`);
  // The glacier's cover (terrain.json tectonics.iceBelowC): a full block.
  const ib = blocks.get(t.ice);
  if (!ib)
    fail(`tectonic.ice ${JSON.stringify(t.ice)} is not in the registry`);
  if (ib.shape !== "full") fail(`tectonic.ice ${t.ice} must be a full block`);
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
      throw new Error(
        `biome ${b.name}: a preset needs a label and a description`,
      );
    }
  }
  for (const k of ["name", "label"]) {
    const vs = presets.map((b) => b[k]);
    const dup = vs.find(
      (v, i) => vs.indexOf(v) !== i || (k === "name" && taken.includes(v)),
    );
    if (dup !== undefined) throw new Error(`preset ${k} ${dup} is used twice`);
  }
  return presets;
}

/** The fluid look's fields a biome's `waterLook` may set (its water's colour and clarity, ADR 0074 B4). */
export const WATER_LOOK_FIELDS = ["scatter", "absorption"];
const HEX = /^#[0-9a-f]{6}$/i;

/**
 * Checks a biome's optional `waterLook`: its water's colour (`scatter`, sRGB "#rrggbb") and clarity
 * (`absorption`, per metre of red, green, blue in [0, 10]) over its fluid's look
 * (shared/blocks/dwell.json `fluids`); at least one, nothing else.
 */
function checkWaterLook(l, fail) {
  if (typeof l !== "object" || l === null || Array.isArray(l))
    fail("waterLook must be an object");
  const extra = Object.keys(l).filter(
    (k) => !["$comment", ...WATER_LOOK_FIELDS].includes(k),
  );
  if (extra.length) fail(`waterLook: unknown ${extra.join(", ")}`);
  if (!WATER_LOOK_FIELDS.some((k) => k in l))
    fail("waterLook needs a scatter or an absorption");
  if (
    l.scatter !== undefined &&
    !(typeof l.scatter === "string" && HEX.test(l.scatter))
  ) {
    fail('waterLook.scatter must be a colour "#rrggbb"');
  }
  if (
    l.absorption !== undefined &&
    !(
      Array.isArray(l.absorption) &&
      l.absorption.length === 3 &&
      l.absorption.every((v) => isNum(v) && v >= 0 && v <= 10)
    )
  ) {
    fail("waterLook.absorption must be three numbers in [0, 10]");
  }
}

/** Checks a biome's `where` (its place in the geography): a weight and optional eased ranges. */
function checkWhere(w, fail) {
  if (typeof w !== "object" || w === null || Array.isArray(w))
    fail("where must be an object");
  const extra = Object.keys(w).filter(
    (k) => !["$comment", "weight", ...WHERE_FIELDS].includes(k),
  );
  if (extra.length) fail(`where: unknown ${extra.join(", ")}`);
  if (!(isNum(w.weight) && w.weight > 0 && w.weight <= 1000))
    fail("where.weight must be in (0, 1000]");
  for (const f of WHERE_FIELDS) {
    const r = w[f];
    if (r === undefined) continue;
    if (typeof r !== "object" || r === null)
      fail(`where.${f} must be an object`);
    const re = Object.keys(r).filter(
      (k) => !["min", "max", "blend"].includes(k),
    );
    if (re.length) fail(`where.${f}: unknown ${re.join(", ")}`);
    for (const k of ["min", "max"]) {
      if (r[k] !== undefined && !(isNum(r[k]) && Math.abs(r[k]) <= 1_000_000))
        fail(`where.${f}.${k} must be a number`);
    }
    if (r.min === undefined && r.max === undefined)
      fail(`where.${f} needs a min or a max`);
    if (r.min !== undefined && r.max !== undefined && !(r.max > r.min)) {
      fail(`where.${f}.max must be greater than its min`);
    }
    if (!(isNum(r.blend) && r.blend >= 0 && r.blend <= 1_000_000))
      fail(`where.${f}.blend must be in [0, 1000000]`);
  }
}

/** Checks a biome's elevation bands: the lowest without `from`, each next one higher. */
function checkBands(bands, blocks, fail) {
  if (!Array.isArray(bands) || bands.length === 0)
    fail("bands must be a list of elevation bands");
  if (bands.length > 16) fail("bands: at most 16");
  let below = -Infinity;
  const names = new Set();
  bands.forEach((band, i) => {
    const at = `bands[${i}]`;
    if (typeof band !== "object" || band === null)
      fail(`${at} must be an object`);
    const extra = Object.keys(band).filter(
      (k) => !["$comment", "name", "from", ...BAND_BLOCKS].includes(k),
    );
    if (extra.length) fail(`${at}: unknown ${extra.join(", ")}`);
    if (typeof band.name !== "string" || !BAND_NAME.test(band.name))
      fail(`${at}.name must be a lowercase name`);
    if (names.has(band.name)) fail(`bands: ${band.name} is listed twice`);
    names.add(band.name);
    if (i === 0) {
      if (band.from !== undefined)
        fail(`${at} (the lowest) has no from: it starts at the sea floor`);
    } else {
      if (!(isNum(band.from) && band.from >= 0 && band.from <= 100_000))
        fail(`${at}.from must be in [0, 100000]`);
      if (!(band.from > below))
        fail(`${at}.from must be higher than the band below's`);
      below = band.from;
    }
    for (const k of BAND_BLOCKS) {
      const id = band[k];
      const b = blocks.get(id);
      if (!b) fail(`${at}.${k} ${JSON.stringify(id)} is not in the registry`);
      if (b.shape !== "full") fail(`${at}.${k} ${id} must be a full block`);
    }
  });
}

/**
 * Checks a `biomeMap` group (terrain.json's, or Chisel's edit of it) against `TERRAIN`'s ranges;
 * throws naming the key.
 */
export function validateBiomeMap(bm) {
  checkGroups({ biomeMap: bm }, ["biomeMap"], (m) => {
    throw new Error(`terrain.json: ${m}`);
  });
  return bm;
}

/**
 * The world knobs Chisel may change (ADR 0074 B4, the archipelago's: the coast's islands, the sea
 * floor's shelf, the inland rise, the hills' height): terrain.json's groups and keys.
 */
export const WORLD_KNOBS = {
  islands: ["amplitude", "wavelength", "octaves", "cap"],
  seaFloor: ["depth", "shelf"],
  inland: ["rise", "halfDistance"],
  hills: ["amplitude"],
  tectonics: Object.keys(TERRAIN.tectonics),
};

/** Checks Chisel's world knobs (`{ group: { key: v } }`, `WORLD_KNOBS`'s) by `TERRAIN`'s ranges. */
export function validateWorldKnobs(w) {
  const fail = (m) => {
    throw new Error(`world: ${m}`);
  };
  if (typeof w !== "object" || w === null || Array.isArray(w))
    fail("must be an object");
  for (const [g, keys] of Object.entries(w)) {
    if (!(g in WORLD_KNOBS)) fail(`${g} is not a world knob`);
    for (const [k, v] of Object.entries(keys ?? {})) {
      if (!WORLD_KNOBS[g].includes(k)) fail(`${g}.${k} is not a world knob`);
      const [min, max, whole] = TERRAIN[g][k];
      const low = typeof min === "object" ? v > min.above : v >= min;
      if (!(isNum(v) && low && v <= max && (!whole || Number.isInteger(v))))
        fail(`${g}.${k} is out of range`);
    }
  }
  return w;
}
