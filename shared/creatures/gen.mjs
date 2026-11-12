#!/usr/bin/env node
// Generates the creatures (docs/adr/0073-creatures.md) for Rust from shared/creatures/creatures.json:
// `crates/dwell-sim/src/creatures.gen.rs` (the server's spawning and wandering), and from
// shared/creatures/fixtures/creatures.json `creatures_fixture.gen.rs` (tests only: a creature added
// by data alone; fixtures/npcs.json `npcs_fixture.gen.rs`: an NPC by data alone). It checks every block against the registry (shared/blocks/*.json) and every model
// against shared/models/models.json. The output is checked in; CI regenerates it and fails on any
// diff. A bad entry fails with a message naming it.
import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { loadBlocks, loadInventory } from '../inventory/gen.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..', '..');

const TOP_FIELDS = ['formatVersion', 'limits', 'creatures', 'rules'];
const LIMIT_FIELDS = ['spawnChunks', 'despawnM', 'max', 'gapM', 'pushMps'];
const CREATURE_FIELDS = ['name', 'model', 'walkSpeed', 'wanderM', 'idleSeconds', 'rows', 'climbRows', 'dropRows', 'npc', 'health', 'hitRadiusM', 'bodyRadiusM', 'drops'];
/** A death's drop (WORK_QUEUE item 35): an item and how many, [min, max] or n. */
const DROP_FIELDS = ['item', 'count'];
const RULE_FIELDS = ['creature', 'on', 'chance', 'group', 'mix', 'spacingM', 'biomes'];
const NPC_FIELDS = ['lookAtM', 'turnDegPerS', 'bumpM', 'sidestepM', 'idles', 'colours', 'held', 'disposition', 'combat'];
/** Dispositions (owner 2026-11-12, item 35: "neutral by default"): who an NPC treats as a foe. */
export const DISPOSITIONS = ['neutral'];
/** Combat hooks: what an NPC does when attacked (`fightBack`: it turns on its attacker; the
 * swords' stances and blocks come with the sword mechanic, WORK_QUEUE item 35). */
export const COMBATS = ['none', 'fightBack'];
const IDLE_FIELDS = ['pose', 'weight', 'seconds'];
const COLOUR_FIELDS = ['part', 'from', 'to'];
/** The idle poses (dwell-protocol `NPC_POSES`, in its order: the wire's pose byte). */
export const POSES = ['stand', 'look', 'sit'];
const MIX_FIELDS = ['creature', 'chance', 'max'];
const NAMESPACED = /^[a-z][a-z0-9_]*:[a-z][a-z0-9_]*$/;
/** The most rows a creature may be tall or step (the server's ground search). */
export const MAX_ROWS = 8;

const fail = (m) => {
  throw new Error(`shared/creatures: ${m}`);
};
const num = (v) => typeof v === 'number' && Number.isFinite(v);
const int = (v, lo, hi) => Number.isInteger(v) && v >= lo && v <= hi;
const isObject = (v) => typeof v === 'object' && v !== null && !Array.isArray(v);
const unknown = (o, fields, at) => {
  for (const k of Object.keys(o)) if (!fields.includes(k) && k[0] !== '$') fail(`${at}: unknown field ${k} (${fields.join(', ')})`);
};

/** An NPC's behaviour and colours (WORK_QUEUE item 38, CHARACTERS.md §11.10). */
function validateNpc(n, at, ramps, items) {
  if (!isObject(n)) fail(`${at} must be an object`);
  unknown(n, NPC_FIELDS, at);
  const range = (k, lo, hi) => {
    if (!num(n[k]) || n[k] < lo || n[k] > hi) fail(`${at}: ${k} must be in [${lo}, ${hi}]`);
  };
  range('lookAtM', 0, 32);
  range('turnDegPerS', 1, 720);
  range('bumpM', 0.1, 4);
  range('sidestepM', 0.25, 8);
  if (!Array.isArray(n.idles) || n.idles.length === 0) fail(`${at}: idles must be a non-empty list`);
  n.idles.forEach((d, i) => {
    const iat = `${at}.idles[${i}]`;
    if (!isObject(d)) fail(`${iat} must be an object`);
    unknown(d, IDLE_FIELDS, iat);
    if (!POSES.includes(d.pose)) fail(`${iat}: pose ${JSON.stringify(d.pose)} is not one of ${POSES.join(', ')}`);
    if (!num(d.weight) || d.weight <= 0) fail(`${iat}: weight must be over 0`);
    const s = d.seconds;
    if (!Array.isArray(s) || s.length !== 2 || !s.every((v) => num(v) && v > 0 && v <= 600) || s[0] > s[1]) fail(`${iat}: seconds must be [min, max] in (0, 600], min <= max`);
  });
  if (n.held !== undefined && !items.has(n.held)) fail(`${at}: held ${JSON.stringify(n.held)} is not an item (shared/items/items.json)`);
  if (n.disposition !== undefined && !DISPOSITIONS.includes(n.disposition)) fail(`${at}: disposition ${JSON.stringify(n.disposition)} is not one of ${DISPOSITIONS.join(', ')}`);
  if (n.combat !== undefined && !COMBATS.includes(n.combat)) fail(`${at}: combat ${JSON.stringify(n.combat)} is not one of ${COMBATS.join(', ')}`);
  if (!Array.isArray(n.colours)) fail(`${at}: colours must be a list`);
  const parts = new Set();
  const froms = new Set();
  n.colours.forEach((c, i) => {
    const cat = `${at}.colours[${i}]`;
    if (!isObject(c)) fail(`${cat} must be an object`);
    unknown(c, COLOUR_FIELDS, cat);
    if (typeof c.part !== 'string' || !/^[a-z][a-z0-9_]*$/.test(c.part)) fail(`${cat}: part must be a lowercase name`);
    if (parts.has(c.part)) fail(`${cat}: part ${c.part} listed twice`);
    parts.add(c.part);
    if (!ramps.has(c.from)) fail(`${cat}: from ${JSON.stringify(c.from)} is not a palette ramp`);
    if (froms.has(c.from)) fail(`${cat}: from ${c.from} is another part's`);
    froms.add(c.from);
    if (!Array.isArray(c.to) || c.to.length === 0 || c.to.length > 32) fail(`${cat}: to must list 1-32 palette ramps`);
    c.to.forEach((r, j) => {
      if (!ramps.has(r)) fail(`${cat}.to[${j}]: ${JSON.stringify(r)} is not a palette ramp`);
    });
  });
  return n;
}

/** Checks the creatures against the block files (`{ namespace, blocks }`) and the model names;
 * NPCs' colours against the palette's ramps and rules' biomes against the biome names. */
export function validateCreatures(data, namespaces, models, ramps = loadRamps(), biomes = loadBiomes(), items = loadItems(), dropItems = loadDropItems(namespaces)) {
  if (!isObject(data)) fail('the file must hold an object');
  unknown(data, TOP_FIELDS, 'the file');
  if (data.formatVersion !== 1) fail(`formatVersion must be 1, not ${data.formatVersion}`);
  const blocks = new Set(namespaces.flatMap((ns) => ns.blocks.map((b) => `${ns.namespace}:${b.name}`)));
  const l = data.limits;
  if (!isObject(l)) fail('limits must be an object');
  unknown(l, LIMIT_FIELDS, 'limits');
  if (!int(l.spawnChunks, 0, 8)) fail('limits.spawnChunks must be a whole number in [0, 8]');
  if (!num(l.despawnM) || l.despawnM < 16 || l.despawnM > 1024) fail('limits.despawnM must be in [16, 1024]');
  if (!int(l.max, 1, 4096)) fail('limits.max must be a whole number in [1, 4096]');
  // Spacing (BUGS.md #80): creatures' bodies never overlap, and they steer apart at `pushMps` to
  // keep `gapM` between them.
  l.gapM ??= 0.2;
  l.pushMps ??= 1;
  if (!num(l.gapM) || l.gapM < 0 || l.gapM > 8) fail('limits.gapM must be in [0, 8] m');
  if (!num(l.pushMps) || l.pushMps < 0 || l.pushMps > 20) fail('limits.pushMps must be in [0, 20] m/s');
  if (!Array.isArray(data.creatures) || data.creatures.length === 0) fail('creatures must be a non-empty list');
  const creatures = new Map();
  data.creatures.forEach((c, i) => {
    const at = `creatures.${c?.name ?? i}`;
    if (!isObject(c)) fail(`${at} must be an object`);
    unknown(c, CREATURE_FIELDS, at);
    if (typeof c.name !== 'string' || !NAMESPACED.test(c.name)) fail(`${at}: name must be namespace:name`);
    if (creatures.has(c.name)) fail(`${at}: listed twice`);
    if (!models.has(c.model)) fail(`${at}: model ${JSON.stringify(c.model)} is not in shared/models/models.json`);
    if (!num(c.walkSpeed) || c.walkSpeed <= 0 || c.walkSpeed > 10) fail(`${at}: walkSpeed must be in (0, 10] m/s`);
    if (!num(c.wanderM) || c.wanderM <= 0 || c.wanderM > 64) fail(`${at}: wanderM must be in (0, 64]`);
    const s = c.idleSeconds;
    if (!Array.isArray(s) || s.length !== 2 || !s.every((v) => num(v) && v >= 0 && v <= 600) || s[0] > s[1]) fail(`${at}: idleSeconds must be [min, max] in [0, 600], min <= max`);
    if (!int(c.rows, 1, MAX_ROWS)) fail(`${at}: rows must be a whole number in [1, ${MAX_ROWS}]`);
    if (!int(c.climbRows, 0, MAX_ROWS)) fail(`${at}: climbRows must be a whole number in [0, ${MAX_ROWS}]`);
    if (!int(c.dropRows, 0, MAX_ROWS)) fail(`${at}: dropRows must be a whole number in [0, ${MAX_ROWS}]`);
    // Its body to a weapon (WORK_QUEUE item 35): `health` (none: it can't be hurt), the radius a
    // swing's arc must come within to hit it (`hitRadiusM`), and what it drops when killed.
    const health = c.health ?? 0;
    if (!num(health) || health < 0 || health > 10000) fail(`${at}: health must be in [0, 10000] (0 or none: it can't be hurt)`);
    const hitRadiusM = c.hitRadiusM ?? 0.4;
    if (!num(hitRadiusM) || hitRadiusM <= 0 || hitRadiusM > 4) fail(`${at}: hitRadiusM must be in (0, 4] m`);
    // Its body's radius on the ground (BUGS.md #80, #81): kept out of walls and other bodies.
    const bodyRadiusM = c.bodyRadiusM ?? 0.3;
    if (!num(bodyRadiusM) || bodyRadiusM <= 0 || bodyRadiusM > 2) fail(`${at}: bodyRadiusM must be in (0, 2] m`);
    if (c.drops !== undefined && !Array.isArray(c.drops)) fail(`${at}: drops must be a list`);
    const drops = (c.drops ?? []).map((d, j) => {
      const dat = `${at}.drops[${j}]`;
      if (!isObject(d)) fail(`${dat} must be an object`);
      unknown(d, DROP_FIELDS, dat);
      if (!dropItems.has(d.item)) fail(`${dat}: no item ${JSON.stringify(d.item)} (shared/inventory or a block)`);
      const n = d.count ?? 1;
      const [min, max] = Array.isArray(n) ? n : [n, n];
      if (!int(min, 0, 64) || !int(max, 1, 64) || min > max) fail(`${dat}.count must be n or [min, max], whole numbers in 0..64, max at least 1`);
      return { item: d.item, min, max };
    });
    if (drops.length > 0 && health === 0) fail(`${at}: drops need health (a creature that can't be hurt never dies)`);
    if (c.npc !== undefined) validateNpc(c.npc, `${at}.npc`, ramps, items);
    creatures.set(c.name, { ...c, health, hitRadiusM, bodyRadiusM, drops, index: i });
  });
  if (!Array.isArray(data.rules) || data.rules.length === 0) fail('rules must be a non-empty list');
  const rules = data.rules.map((r, i) => {
    const at = `rules[${i}]`;
    if (!isObject(r)) fail(`${at} must be an object`);
    unknown(r, RULE_FIELDS, at);
    if (!creatures.has(r.creature)) fail(`${at}: creature ${JSON.stringify(r.creature)} is not in creatures`);
    if (!Array.isArray(r.on) || r.on.length === 0) fail(`${at}: on must list the blocks it spawns on`);
    r.on.forEach((b, j) => {
      if (!blocks.has(b)) fail(`${at}.on[${j}]: ${JSON.stringify(b)} is not a block`);
    });
    if (!num(r.chance) || r.chance <= 0 || r.chance > 1) fail(`${at}: chance must be in (0, 1]`);
    const g = r.group;
    if (!Array.isArray(g) || g.length !== 2 || !g.every((v) => int(v, 1, 16)) || g[0] > g[1]) fail(`${at}: group must be [min, max] whole numbers in [1, 16], min <= max`);
    // A group's mix (ADR 0073): each part, rolled once a group with its `chance`, puts up to `max`
    // of that creature in the group in place of the rule's own (a herd of does with a stag).
    if (r.mix !== undefined && !Array.isArray(r.mix)) fail(`${at}: mix must be a list`);
    const mix = (r.mix ?? []).map((m, j) => {
      const mat = `${at}.mix[${j}]`;
      if (!isObject(m)) fail(`${mat} must be an object`);
      unknown(m, MIX_FIELDS, mat);
      if (!creatures.has(m.creature)) fail(`${mat}: creature ${JSON.stringify(m.creature)} is not in creatures`);
      if (!num(m.chance) || m.chance <= 0 || m.chance > 1) fail(`${mat}: chance must be in (0, 1]`);
      if (!int(m.max, 1, 16)) fail(`${mat}: max must be a whole number in [1, 16]`);
      return { creature: creatures.get(m.creature).index, chance: m.chance, max: m.max };
    });
    // An NPC rule's spacing (no group within `spacingM` of another of its kind) and biomes (the
    // column's biome one of them; none listed: anywhere).
    const spacing = r.spacingM ?? 0;
    if (!num(spacing) || spacing < 0 || spacing > 512) fail(`${at}: spacingM must be in [0, 512]`);
    if (r.biomes !== undefined && (!Array.isArray(r.biomes) || r.biomes.length === 0)) fail(`${at}: biomes must be a non-empty list`);
    (r.biomes ?? []).forEach((b, j) => {
      if (!biomes.has(b)) fail(`${at}.biomes[${j}]: ${JSON.stringify(b)} is not a biome (shared/worldgen/biomes)`);
    });
    return { creature: creatures.get(r.creature).index, on: r.on, chance: r.chance, group: g, mix, spacing, biomes: r.biomes ?? [] };
  });
  return { limits: l, creatures: [...creatures.values()], rules };
}

/** Every item's name a creature may drop: the blocks and shared/inventory's items. */
function loadDropItems(namespaces) {
  const blocks = namespaces.flatMap((ns) => ns.blocks.map((b) => `${ns.namespace}:${b.name}`));
  return new Set([...blocks, ...loadInventory().items.map((i) => i.name)]);
}

/** The item names (shared/items/items.json): what an NPC may hold. */
export function loadItems() {
  const d = JSON.parse(readFileSync(join(root, 'shared/items/items.json'), 'utf8'));
  return new Set(d.items.map((i) => i.name));
}

/** The palette's ramp names (shared/textures/palette.json). */
export function loadRamps() {
  const p = JSON.parse(readFileSync(join(root, 'shared/textures/palette.json'), 'utf8'));
  return new Set(Object.keys(p.ramps));
}

/** The biome names (shared/worldgen/biomes: the default and the presets). */
export function loadBiomes() {
  const dir = join(root, 'shared/worldgen/biomes');
  const files = ['default.json', ...readdirSync(join(dir, 'presets')).filter((f) => f.endsWith('.json')).map((f) => `presets/${f}`)];
  return new Set(files.map((f) => JSON.parse(readFileSync(join(dir, f), 'utf8')).name));
}

export function loadModels() {
  const m = JSON.parse(readFileSync(join(root, 'shared/models/models.json'), 'utf8'));
  return new Set(Object.keys(m.models));
}

const f = (v) => (Number.isInteger(v) ? `${v}.0` : `${v}`);
const strs = (a) => `&[${a.map((s) => JSON.stringify(s)).join(', ')}]`;

const pose = (p) => p[0].toUpperCase() + p.slice(1);

function npcRust(n) {
  if (!n) return 'None';
  const idles = n.idles.map((d) => `Idle { pose: Pose::${pose(d.pose)}, weight: ${f(d.weight)}, seconds: [${d.seconds.map(f).join(', ')}] }`).join(', ');
  const colours = n.colours.map((c) => `ColourSlot { part: ${JSON.stringify(c.part)}, from: ${JSON.stringify(c.from)}, to: ${strs(c.to)} }`).join(', ');
  return `Some(NpcDef { look_at_m: ${f(n.lookAtM)}, turn_deg_per_s: ${f(n.turnDegPerS)}, bump_m: ${f(n.bumpM)}, sidestep_m: ${f(n.sidestepM)}, held: ${n.held ? `Some(${JSON.stringify(n.held)})` : 'None'}, disposition: Disposition::${pose(n.disposition ?? 'neutral')}, combat: Combat::${pose(n.combat ?? 'none')}, idles: &[${idles}], colours: &[${colours}] })`;
}

export function creaturesRust(d, source) {
  const rs = [`// Generated by shared/creatures/gen.mjs from ${source}. Do not edit.`, '', '#[allow(unused_imports)]', 'use crate::creatures::{ColourSlot, Combat, CreatureDef, Disposition, Idle, Limits, MixPart, NpcDef, Pose, SpawnRule};', ''];
  const l = d.limits;
  rs.push(`pub const LIMITS: Limits = Limits { spawn_chunks: ${l.spawnChunks}, despawn_m: ${f(l.despawnM)}, max: ${l.max}, gap_m: ${f(l.gapM)}, push_mps: ${f(l.pushMps)} };`, '');
  rs.push(`pub const CREATURES: [CreatureDef; ${d.creatures.length}] = [`);
  for (const c of d.creatures) {
    rs.push(
      `    CreatureDef { name: ${JSON.stringify(c.name)}, model: ${JSON.stringify(c.model)}, walk_speed: ${f(c.walkSpeed)}, wander_m: ${f(c.wanderM)}, idle_seconds: [${c.idleSeconds.map(f).join(', ')}], rows: ${c.rows}, climb_rows: ${c.climbRows}, drop_rows: ${c.dropRows}, npc: ${npcRust(c.npc)}, health: ${f(c.health)}, hit_radius_m: ${f(c.hitRadiusM)}, body_radius_m: ${f(c.bodyRadiusM)}, drops: &[${c.drops.map((d) => `(${JSON.stringify(d.item)}, ${d.min}, ${d.max})`).join(', ')}] },`,
    );
  }
  rs.push('];', '');
  rs.push(`pub const RULES: [SpawnRule; ${d.rules.length}] = [`);
  for (const r of d.rules) {
    const mix = r.mix.map((m) => `MixPart { creature: ${m.creature}, chance: ${f(m.chance)}, max: ${m.max} }`).join(', ');
    rs.push(`    SpawnRule { creature: ${r.creature}, on: ${strs(r.on)}, chance: ${f(r.chance)}, group: [${r.group.join(', ')}], mix: &[${mix}], spacing_m: ${f(r.spacing)}, biomes: ${strs(r.biomes)} },`);
  }
  rs.push('];', '');
  return rs.join('\n');
}

/** Every generated file: [path, text]. */
export function generate() {
  const blocks = loadBlocks();
  const models = loadModels();
  const read = (p) => JSON.parse(readFileSync(join(here, p), 'utf8'));
  return [
    ['creatures.json', 'creatures.gen.rs'],
    ['fixtures/creatures.json', 'creatures_fixture.gen.rs'],
    ['fixtures/npcs.json', 'npcs_fixture.gen.rs'],
  ].map(([src, out]) => [
    join(root, 'crates/dwell-sim/src', out),
    creaturesRust(validateCreatures(read(src), blocks, models), `shared/creatures/${src}`),
  ]);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  for (const [path, text] of generate()) writeFileSync(path, text);
  console.log('shared/creatures: wrote creatures.gen.rs, creatures_fixture.gen.rs, npcs_fixture.gen.rs');
}
