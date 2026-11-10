#!/usr/bin/env node
// Generates the creatures (docs/adr/0073-creatures.md) for Rust from shared/creatures/creatures.json:
// `crates/dwell-sim/src/creatures.gen.rs` (the server's spawning and wandering), and from
// shared/creatures/fixtures/creatures.json `creatures_fixture.gen.rs` (tests only: a creature added
// by data alone). It checks every block against the registry (shared/blocks/*.json) and every model
// against shared/models/models.json. The output is checked in; CI regenerates it and fails on any
// diff. A bad entry fails with a message naming it.
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { loadBlocks } from '../inventory/gen.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..', '..');

const TOP_FIELDS = ['formatVersion', 'limits', 'creatures', 'rules'];
const LIMIT_FIELDS = ['spawnChunks', 'despawnM', 'max'];
const CREATURE_FIELDS = ['name', 'model', 'walkSpeed', 'wanderM', 'idleSeconds', 'rows', 'climbRows', 'dropRows'];
const RULE_FIELDS = ['creature', 'on', 'chance', 'group', 'mix'];
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

/** Checks the creatures against the block files (`{ namespace, blocks }`) and the model names. */
export function validateCreatures(data, namespaces, models) {
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
    creatures.set(c.name, { ...c, index: i });
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
    return { creature: creatures.get(r.creature).index, on: r.on, chance: r.chance, group: g, mix };
  });
  return { limits: l, creatures: [...creatures.values()], rules };
}

export function loadModels() {
  const m = JSON.parse(readFileSync(join(root, 'shared/models/models.json'), 'utf8'));
  return new Set(Object.keys(m.models));
}

const f = (v) => (Number.isInteger(v) ? `${v}.0` : `${v}`);
const strs = (a) => `&[${a.map((s) => JSON.stringify(s)).join(', ')}]`;

export function creaturesRust(d, source) {
  const rs = [`// Generated by shared/creatures/gen.mjs from ${source}. Do not edit.`, '', 'use crate::creatures::{CreatureDef, Limits, MixPart, SpawnRule};', ''];
  const l = d.limits;
  rs.push(`pub const LIMITS: Limits = Limits { spawn_chunks: ${l.spawnChunks}, despawn_m: ${f(l.despawnM)}, max: ${l.max} };`, '');
  rs.push(`pub const CREATURES: [CreatureDef; ${d.creatures.length}] = [`);
  for (const c of d.creatures) {
    rs.push(
      `    CreatureDef { name: ${JSON.stringify(c.name)}, model: ${JSON.stringify(c.model)}, walk_speed: ${f(c.walkSpeed)}, wander_m: ${f(c.wanderM)}, idle_seconds: [${c.idleSeconds.map(f).join(', ')}], rows: ${c.rows}, climb_rows: ${c.climbRows}, drop_rows: ${c.dropRows} },`,
    );
  }
  rs.push('];', '');
  rs.push(`pub const RULES: [SpawnRule; ${d.rules.length}] = [`);
  for (const r of d.rules) {
    const mix = r.mix.map((m) => `MixPart { creature: ${m.creature}, chance: ${f(m.chance)}, max: ${m.max} }`).join(', ');
    rs.push(`    SpawnRule { creature: ${r.creature}, on: ${strs(r.on)}, chance: ${f(r.chance)}, group: [${r.group.join(', ')}], mix: &[${mix}] },`);
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
  ].map(([src, out]) => [
    join(root, 'crates/dwell-sim/src', out),
    creaturesRust(validateCreatures(read(src), blocks, models), `shared/creatures/${src}`),
  ]);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  for (const [path, text] of generate()) writeFileSync(path, text);
  console.log('shared/creatures: wrote creatures.gen.rs, creatures_fixture.gen.rs');
}
