#!/usr/bin/env node
// Generates the far ecology's data (docs/ECOLOGY.md) for Rust from shared/ecology/ecology.json:
// `crates/dwell-ecology/src/ecology.gen.rs` (the herds' and packs' step), and from
// shared/ecology/fixtures/ecology.json `ecology_fixture.gen.rs` (tests only: species added by data
// alone). Species refer to each other by name (`flee.from`, `diet.prey.species`); the output holds
// their indices. The output is checked in; CI regenerates it and fails on any diff (`--check` fails
// here without writing). A bad entry fails with a message naming it.
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..', '..');

const TOP_FIELDS = ['formatVersion', 'island', 'grazing', 'near', 'species'];
const NEAR_FIELDS = ['spawnM', 'foldM', 'thinkS'];
const NEAR_SPECIES_FIELDS = ['creature', 'hungerPerS', 'energyPerS', 'sleep', 'graze', 'hunt', 'flee', 'mate', 'companyM', 'wanderM'];
const NEAR_SLEEP_FIELDS = ['belowEnergy', 'nightChancePerS', 'nightBelowHunger', 'gainPerS', 'wakeAboveEnergy', 'wakeAboveHunger', 'minS'];
const NEAR_GRAZE_FIELDS = ['aboveHunger', 'fullBelow', 'eatPerS', 'foragePerS', 'block', 'blockChancePerS'];
const NEAR_HUNT_FIELDS = ['aboveHunger', 'senseM', 'sprint', 'reachM', 'feedsPackM', 'feedsPack'];
const NEAR_FLEE_FIELDS = ['radiusM', 'pace'];
const NEAR_MATE_FIELDS = ['belowHunger', 'aboveEnergy', 'radiusM', 'reachM', 'chance', 'cost', 'tired'];
const ISLAND_FIELDS = ['cellM', 'stepDays'];
const GRAZING_FIELDS = ['regrowPerDay', 'grounds'];
const GROUND_FIELDS = ['name', 'maxHeightM', 'maxSlopeDeg', 'max', 'refuge'];
const SPECIES_FIELDS = [
  'name', 'group', 'start', 'breedPerDay', 'breedBelowHunger', 'hungerDays', 'starveAboveHunger',
  'starvePerDay', 'speedMPerDay', 'senseRadiusM', 'diet', 'flee', 'disperse', 'near',
];
const GROUP_FIELDS = ['dieBelow', 'cap', 'mergeBelow', 'mergeRadiusM', 'maxGroups'];
const START_FIELDS = ['groups', 'size'];
const DIETS = ['graze', 'prey'];
const GRAZE_FIELDS = ['perHeadPerDay'];
const PREY_FIELDS = ['species', 'killsPerHeadPerDay', 'halfSaturationHead', 'needPerHeadPerDay', 'preyPerHead', 'reachM'];
const FLEE_FIELDS = ['from', 'radiusM'];
const DISPERSE_FIELDS = ['aboveHunger', 'speedFactor', 'leavePerDay'];
const NAMESPACED = /^[a-z][a-z0-9_]*:[a-z][a-z0-9_]*$/;

const fail = (m) => {
  throw new Error(`shared/ecology: ${m}`);
};
const num = (v) => typeof v === 'number' && Number.isFinite(v);
const isObject = (v) => typeof v === 'object' && v !== null && !Array.isArray(v);
const object = (o, fields, at) => {
  if (!isObject(o)) fail(`${at} must be an object`);
  for (const k of Object.keys(o)) if (!fields.includes(k) && k[0] !== '$') fail(`${at}: unknown field ${k} (${fields.join(', ')})`);
  return o;
};
/** `v` a number in [lo, hi] (`open`: lo excluded), or a message naming `at`. */
const range = (v, lo, hi, at, open = false) => {
  if (!num(v) || (open ? v <= lo : v < lo) || v > hi) fail(`${at} must be a number in ${open ? '(' : '['}${lo}, ${hi}]`);
  return v;
};
const whole = (v, lo, hi, at) => {
  if (!Number.isInteger(v) || v < lo || v > hi) fail(`${at} must be a whole number in [${lo}, ${hi}]`);
  return v;
};

/** Checks an ecology file; returns it with the species' cross references as indices. */
export function validateEcology(data) {
  object(data, TOP_FIELDS, 'the file');
  if (data.formatVersion !== 1) fail(`formatVersion must be 1, not ${data.formatVersion}`);
  const is = object(data.island, ISLAND_FIELDS, 'island');
  range(is.cellM, 10, 2000, 'island.cellM');
  range(is.stepDays, 0, 30, 'island.stepDays', true);
  const g = object(data.grazing, GRAZING_FIELDS, 'grazing');
  range(g.regrowPerDay, 0, 1, 'grazing.regrowPerDay', true);
  if (!Array.isArray(g.grounds) || g.grounds.length === 0) fail('grazing.grounds must be a non-empty list');
  const groundNames = new Set();
  g.grounds.forEach((gr, i) => {
    const at = `grazing.grounds.${gr?.name ?? i}`;
    object(gr, GROUND_FIELDS, at);
    if (typeof gr.name !== 'string' || gr.name === '') fail(`${at}: name must be a string`);
    if (groundNames.has(gr.name)) fail(`${at}: listed twice`);
    groundNames.add(gr.name);
    range(gr.maxHeightM, -100, 100000, `${at}.maxHeightM`);
    range(gr.maxSlopeDeg, 0, 90, `${at}.maxSlopeDeg`);
    range(gr.max, 0, 100000, `${at}.max`);
    if (gr.refuge !== undefined) range(gr.refuge, 0, 1, `${at}.refuge`);
  });
  const nr = object(data.near, NEAR_FIELDS, 'near');
  range(nr.spawnM, 1, 1000, 'near.spawnM');
  range(nr.foldM, nr.spawnM, 2000, 'near.foldM');
  range(nr.thinkS, 0, 10, 'near.thinkS', true);
  if (nr.foldM <= nr.spawnM) fail('near.foldM must be past near.spawnM (the hysteresis)');
  if (!Array.isArray(data.species) || data.species.length === 0) fail('species must be a non-empty list');
  const index = new Map();
  data.species.forEach((s, i) => {
    const name = s?.name;
    if (typeof name !== 'string' || !NAMESPACED.test(name)) fail(`species[${i}]: name must be namespace:name`);
    if (index.has(name)) fail(`species.${name}: listed twice`);
    index.set(name, i);
  });
  const ref = (n, at) => {
    if (!index.has(n)) fail(`${at}: ${JSON.stringify(n)} is not a species`);
    return index.get(n);
  };
  // A hunter's prey must graze (one level of hunting for now: the island's bound for a hunter is
  // its prey's grazing capacity).
  const grazes = (n) => isObject(data.species[index.get(n)]?.diet?.graze);
  const species = data.species.map((s) => {
    const at = `species.${s.name}`;
    object(s, SPECIES_FIELDS, at);
    const gp = object(s.group, GROUP_FIELDS, `${at}.group`);
    range(gp.dieBelow, 0, 1000, `${at}.group.dieBelow`, true);
    whole(gp.cap, 2, 1000, `${at}.group.cap`);
    if (gp.cap < 2 * gp.dieBelow) fail(`${at}.group: cap must be at least twice dieBelow (a split's halves live)`);
    range(gp.mergeBelow, 0, gp.cap, `${at}.group.mergeBelow`);
    range(gp.mergeRadiusM, 0, 100000, `${at}.group.mergeRadiusM`);
    whole(gp.maxGroups, 1, 10000, `${at}.group.maxGroups`);
    const st = object(s.start, START_FIELDS, `${at}.start`);
    whole(st.groups, 1, gp.maxGroups, `${at}.start.groups`);
    range(st.size, gp.dieBelow, gp.cap, `${at}.start.size`);
    range(s.breedPerDay, 0, 1, `${at}.breedPerDay`);
    range(s.breedBelowHunger, 0, 1, `${at}.breedBelowHunger`);
    range(s.hungerDays, 0, 365, `${at}.hungerDays`, true);
    range(s.starveAboveHunger, 0, 1, `${at}.starveAboveHunger`);
    range(s.starvePerDay, 0, 0.2, `${at}.starvePerDay`);
    range(s.speedMPerDay, 0, 100000, `${at}.speedMPerDay`);
    range(s.senseRadiusM, 0, 100000, `${at}.senseRadiusM`);
    object(s.diet, DIETS, `${at}.diet`);
    const kinds = Object.keys(s.diet).filter((k) => k[0] !== '$');
    if (kinds.length !== 1) fail(`${at}.diet must name exactly one of ${DIETS.join(', ')}`);
    let diet;
    if (kinds[0] === 'graze') {
      const d = object(s.diet.graze, GRAZE_FIELDS, `${at}.diet.graze`);
      diet = { graze: { perHeadPerDay: range(d.perHeadPerDay, 0, 1000, `${at}.diet.graze.perHeadPerDay`, true) } };
    } else {
      const d = object(s.diet.prey, PREY_FIELDS, `${at}.diet.prey`);
      if (!Array.isArray(d.species) || d.species.length === 0) fail(`${at}.diet.prey.species must list the species it hunts`);
      const prey = d.species.map((n, j) => ref(n, `${at}.diet.prey.species[${j}]`));
      if (prey.includes(index.get(s.name))) fail(`${at}.diet.prey: a species cannot hunt itself`);
      d.species.forEach((n, j) => {
        if (!grazes(n)) fail(`${at}.diet.prey.species[${j}]: ${JSON.stringify(n)} must graze (hunters hunt grazers only)`);
      });
      diet = {
        prey: {
          species: prey,
          killsPerHeadPerDay: range(d.killsPerHeadPerDay, 0, 100, `${at}.diet.prey.killsPerHeadPerDay`),
          halfSaturationHead: range(d.halfSaturationHead, 0, 100000, `${at}.diet.prey.halfSaturationHead`),
          needPerHeadPerDay: range(d.needPerHeadPerDay, 0, 100, `${at}.diet.prey.needPerHeadPerDay`, true),
          preyPerHead: range(d.preyPerHead, 0, 100000, `${at}.diet.prey.preyPerHead`, true),
          reachM: range(d.reachM, 0, 100000, `${at}.diet.prey.reachM`),
        },
      };
    }
    let flee = null;
    if (s.flee !== undefined) {
      object(s.flee, FLEE_FIELDS, `${at}.flee`);
      if (!Array.isArray(s.flee.from) || s.flee.from.length === 0) fail(`${at}.flee.from must list the species it flees`);
      flee = {
        from: s.flee.from.map((n, j) => ref(n, `${at}.flee.from[${j}]`)),
        radiusM: range(s.flee.radiusM, 0, 100000, `${at}.flee.radiusM`),
      };
    }
    let disperse = null;
    if (s.disperse !== undefined) {
      const d = object(s.disperse, DISPERSE_FIELDS, `${at}.disperse`);
      if (!diet.prey) fail(`${at}.disperse: only a hunter disperses (to its prey)`);
      disperse = {
        aboveHunger: range(d.aboveHunger, 0, 1, `${at}.disperse.aboveHunger`),
        speedFactor: range(d.speedFactor, 1, 10, `${at}.disperse.speedFactor`),
        leavePerDay: range(d.leavePerDay, 0, 0.2, `${at}.disperse.leavePerDay`),
      };
    }
    const near = s.near === undefined ? null : nearOf(s, `${at}.near`, diet, flee);
    return { ...s, diet, flee, disperse, near };
  });
  return { island: is, grazing: g, near: nr, species };
}

/** A species' individuals near players (docs/ECOLOGY.md §8): the mockup's needs as data. */
function nearOf(s, at, diet, flee) {
  const n = object(s.near, NEAR_SPECIES_FIELDS, at);
  if (typeof n.creature !== 'string' || !NAMESPACED.test(n.creature)) fail(`${at}.creature must name a creature (namespace:name)`);
  range(n.hungerPerS, 0, 1, `${at}.hungerPerS`);
  range(n.energyPerS, 0, 1, `${at}.energyPerS`);
  range(n.companyM, 0, 1000, `${at}.companyM`);
  range(n.wanderM, 0, 1000, `${at}.wanderM`);
  const sl = object(n.sleep, NEAR_SLEEP_FIELDS, `${at}.sleep`);
  for (const k of ['belowEnergy', 'nightBelowHunger', 'wakeAboveEnergy', 'wakeAboveHunger']) range(sl[k], 0, 1, `${at}.sleep.${k}`);
  range(sl.nightChancePerS, 0, 10, `${at}.sleep.nightChancePerS`);
  range(sl.gainPerS, 0, 10, `${at}.sleep.gainPerS`, true);
  range(sl.minS, 0, 3600, `${at}.sleep.minS`);
  let graze = null;
  let hunt = null;
  if (diet.graze) {
    if (n.hunt !== undefined) fail(`${at}.hunt: a grazer doesn't hunt`);
    const g = object(n.graze, NEAR_GRAZE_FIELDS, `${at}.graze`);
    range(g.aboveHunger, 0, 1, `${at}.graze.aboveHunger`);
    range(g.fullBelow, 0, g.aboveHunger, `${at}.graze.fullBelow`);
    range(g.eatPerS, 0, 10, `${at}.graze.eatPerS`, true);
    range(g.foragePerS, 0, 100, `${at}.graze.foragePerS`);
    if (g.block !== undefined && (typeof g.block !== 'string' || !NAMESPACED.test(g.block))) fail(`${at}.graze.block must name a block (namespace:name)`);
    range(g.blockChancePerS ?? 0, 0, 10, `${at}.graze.blockChancePerS`);
    graze = { ...g, block: g.block ?? null, blockChancePerS: g.blockChancePerS ?? 0 };
  } else {
    if (n.graze !== undefined) fail(`${at}.graze: a hunter doesn't graze`);
    const h = object(n.hunt, NEAR_HUNT_FIELDS, `${at}.hunt`);
    range(h.aboveHunger, 0, 1, `${at}.hunt.aboveHunger`);
    range(h.senseM, 0, 1000, `${at}.hunt.senseM`);
    range(h.sprint, 1, 10, `${at}.hunt.sprint`);
    range(h.reachM, 0, 100, `${at}.hunt.reachM`, true);
    range(h.feedsPackM, 0, 1000, `${at}.hunt.feedsPackM`);
    range(h.feedsPack, 0, 1, `${at}.hunt.feedsPack`);
    hunt = h;
  }
  let nflee = null;
  if (n.flee !== undefined) {
    if (!flee) fail(`${at}.flee: the species flees nothing (its flee.from)`);
    const fl = object(n.flee, NEAR_FLEE_FIELDS, `${at}.flee`);
    range(fl.radiusM, 0, 1000, `${at}.flee.radiusM`);
    range(fl.pace, 1, 10, `${at}.flee.pace`);
    nflee = fl;
  }
  const m = object(n.mate, NEAR_MATE_FIELDS, `${at}.mate`);
  for (const k of ['belowHunger', 'aboveEnergy', 'chance', 'cost', 'tired']) range(m[k], 0, 1, `${at}.mate.${k}`);
  range(m.radiusM, 0, 1000, `${at}.mate.radiusM`);
  range(m.reachM, 0, 100, `${at}.mate.reachM`, true);
  return { ...n, sleep: sl, graze, hunt, flee: nflee, mate: m };
}

function nearRust(n) {
  if (!n) return 'None';
  const sl = n.sleep;
  const g = n.graze;
  const h = n.hunt;
  const m = n.mate;
  const graze = g
    ? `Some(NearGraze { above_hunger: ${f(g.aboveHunger)}, full_below: ${f(g.fullBelow)}, eat_per_s: ${f(g.eatPerS)}, forage_per_s: ${f(g.foragePerS)}, block: ${g.block ? `Some(${JSON.stringify(g.block)})` : 'None'}, block_chance_per_s: ${f(g.blockChancePerS)} })`
    : 'None';
  const hunt = h
    ? `Some(NearHunt { above_hunger: ${f(h.aboveHunger)}, sense_m: ${f(h.senseM)}, sprint: ${f(h.sprint)}, reach_m: ${f(h.reachM)}, feeds_pack_m: ${f(h.feedsPackM)}, feeds_pack: ${f(h.feedsPack)} })`
    : 'None';
  const flee = n.flee ? `Some(NearFlee { radius_m: ${f(n.flee.radiusM)}, pace: ${f(n.flee.pace)} })` : 'None';
  return [
    'Some(NearDef {',
    `                creature: ${JSON.stringify(n.creature)},`,
    `                hunger_per_s: ${f(n.hungerPerS)},`,
    `                energy_per_s: ${f(n.energyPerS)},`,
    `                sleep: NearSleep { below_energy: ${f(sl.belowEnergy)}, night_chance_per_s: ${f(sl.nightChancePerS)}, night_below_hunger: ${f(sl.nightBelowHunger)}, gain_per_s: ${f(sl.gainPerS)}, wake_above_energy: ${f(sl.wakeAboveEnergy)}, wake_above_hunger: ${f(sl.wakeAboveHunger)}, min_s: ${f(sl.minS)} },`,
    `                graze: ${graze},`,
    `                hunt: ${hunt},`,
    `                flee: ${flee},`,
    `                mate: NearMate { below_hunger: ${f(m.belowHunger)}, above_energy: ${f(m.aboveEnergy)}, radius_m: ${f(m.radiusM)}, reach_m: ${f(m.reachM)}, chance: ${f(m.chance)}, cost: ${f(m.cost)}, tired: ${f(m.tired)} },`,
    `                company_m: ${f(n.companyM)},`,
    `                wander_m: ${f(n.wanderM)},`,
    '            })',
  ].join('\n');
}

const f = (v) => (Number.isInteger(v) ? `${v}.0` : `${v}`);
const idx = (a) => `&[${a.join(', ')}]`;

export function ecologyRust(d, source) {
  const rs = [
    `// Generated by shared/ecology/gen.mjs from ${source}. Do not edit.`,
    '',
    '#[allow(unused_imports)]',
    'use crate::data::{Data, Diet, Disperse, Flee, GroundDef, GroupRules, IslandParams, NearDef, NearFlee, NearGraze, NearHunt, NearMate, NearParams, NearSleep, SpeciesDef};',
    '',
    'pub const DATA: Data = Data {',
    `    island: IslandParams { cell_m: ${f(d.island.cellM)}, step_days: ${f(d.island.stepDays)} },`,
    `    regrow_per_day: ${f(d.grazing.regrowPerDay)},`,
    `    near: NearParams { spawn_m: ${f(d.near.spawnM)}, fold_m: ${f(d.near.foldM)}, think_s: ${f(d.near.thinkS)} },`,
    '    grounds: &[',
  ];
  for (const g of d.grazing.grounds) {
    rs.push(`        GroundDef { name: ${JSON.stringify(g.name)}, max_height_m: ${f(g.maxHeightM)}, max_slope_deg: ${f(g.maxSlopeDeg)}, max: ${f(g.max)}, refuge: ${f(g.refuge ?? 0)} },`);
  }
  rs.push('    ],', '    species: &[');
  for (const s of d.species) {
    const gp = s.group;
    const diet = s.diet.graze
      ? `Diet::Graze { per_head_per_day: ${f(s.diet.graze.perHeadPerDay)} }`
      : `Diet::Prey { species: ${idx(s.diet.prey.species)}, kills_per_head_per_day: ${f(s.diet.prey.killsPerHeadPerDay)}, half_saturation_head: ${f(s.diet.prey.halfSaturationHead)}, need_per_head_per_day: ${f(s.diet.prey.needPerHeadPerDay)}, prey_per_head: ${f(s.diet.prey.preyPerHead)}, reach_m: ${f(s.diet.prey.reachM)} }`;
    const flee = s.flee ? `Some(Flee { from: ${idx(s.flee.from)}, radius_m: ${f(s.flee.radiusM)} })` : 'None';
    const disperse = s.disperse
      ? `Some(Disperse { above_hunger: ${f(s.disperse.aboveHunger)}, speed_factor: ${f(s.disperse.speedFactor)}, leave_per_day: ${f(s.disperse.leavePerDay)} })`
      : 'None';
    rs.push(
      '        SpeciesDef {',
      `            name: ${JSON.stringify(s.name)},`,
      `            group: GroupRules { die_below: ${f(gp.dieBelow)}, cap: ${f(gp.cap)}, merge_below: ${f(gp.mergeBelow)}, merge_radius_m: ${f(gp.mergeRadiusM)}, max_groups: ${gp.maxGroups} },`,
      `            start_groups: ${s.start.groups},`,
      `            start_size: ${f(s.start.size)},`,
      `            breed_per_day: ${f(s.breedPerDay)},`,
      `            breed_below_hunger: ${f(s.breedBelowHunger)},`,
      `            hunger_days: ${f(s.hungerDays)},`,
      `            starve_above_hunger: ${f(s.starveAboveHunger)},`,
      `            starve_per_day: ${f(s.starvePerDay)},`,
      `            speed_m_per_day: ${f(s.speedMPerDay)},`,
      `            sense_radius_m: ${f(s.senseRadiusM)},`,
      `            diet: ${diet},`,
      `            flee: ${flee},`,
      `            disperse: ${disperse},`,
      `            near: ${nearRust(s.near)},`,
      '        },',
    );
  }
  rs.push('    ],', '};', '');
  return rs.join('\n');
}

/** Every generated file: [path, text]. */
export function generate() {
  const read = (p) => JSON.parse(readFileSync(join(here, p), 'utf8'));
  return [
    ['ecology.json', 'ecology.gen.rs'],
    ['fixtures/ecology.json', 'ecology_fixture.gen.rs'],
  ].map(([src, out]) => [join(root, 'crates/dwell-ecology/src', out), ecologyRust(validateEcology(read(src)), `shared/ecology/${src}`)]);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  const check = process.argv.includes('--check');
  let stale = 0;
  for (const [path, text] of generate()) {
    if (!check) writeFileSync(path, text);
    else if ((() => { try { return readFileSync(path, 'utf8'); } catch { return null; } })() !== text) {
      console.error(`${path} is stale: run node shared/ecology/gen.mjs`);
      stale++;
    }
  }
  if (stale) process.exit(1);
  console.log(`shared/ecology: ${check ? 'checked' : 'wrote'} ecology.gen.rs, ecology_fixture.gen.rs`);
}
