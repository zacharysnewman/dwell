#!/usr/bin/env node
// Validates shared/models/models.json (docs/CHARACTERS.md §11, ADR 0070) against its GLBs and the
// shared palette, and writes the game's table: crates/dwell-models/src/models.gen.rs (each model's
// GLB path, scale, yaw and role → clip map, and the player model's GLB bytes); and the same from
// shared/models/fixtures/models.json as fixture.gen.rs (tests only: a model added by data alone).
// CI fails on any diff. A bad entry fails with a message naming it.
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..', '..');
const CRATE = join(root, 'crates/dwell-models/src');

/** The most joints a model's skin may have (the shader's joint block: dwell_models::MAX_JOINTS). */
export const MAX_JOINTS = 64;
/** The roles a model may map (dwell_characters::animator::body_role's names). */
export const ROLES = [
  'idle', 'walk', 'sprint', 'jump', 'fall', 'land', 'crouch', 'crouch_walk', 'slide', 'swim', 'climb', 'fly',
];

/** Every palette stop a remap may snap to: `[{ name, hex }]`, ramps first, `exclude`d ramps left out. */
export function paletteStops(palette, exclude = []) {
  const out = [];
  for (const [ramp, stops] of Object.entries(palette.ramps)) {
    if (exclude.includes(ramp)) continue;
    for (const [stop, hex] of Object.entries(stops)) out.push({ name: `${ramp}.${stop}`, hex });
  }
  return out;
}

/** A palette reference's hex: `ramp.stop`, a bare ramp (its base), or a swatch. */
export function resolveColor(palette, ref) {
  const [ramp, stop = 'base'] = ref.split('.');
  const hex = palette.ramps[ramp]?.[stop] ?? (ref in (palette.swatches ?? {}) ? palette.swatches[ref] : undefined);
  if (!hex) throw new Error(`no palette colour ${ref}`);
  return hex;
}

/** A GLB's JSON chunk. */
export function glbJson(bytes) {
  const b = Buffer.from(bytes);
  if (b.length < 20 || b.readUInt32LE(0) !== 0x46546c67) throw new Error('not a GLB');
  const len = b.readUInt32LE(12);
  if (b.readUInt32LE(16) !== 0x4e4f534a) throw new Error('GLB: first chunk is not JSON');
  return JSON.parse(b.subarray(20, 20 + len).toString('utf8'));
}

/** What the game reads from a GLB: its clip names and its skin's joint count. */
export function glbSummary(bytes) {
  const g = glbJson(bytes);
  return {
    clips: (g.animations ?? []).map((a) => a.name),
    joints: g.skins?.[0]?.joints.length ?? 0,
    meshes: (g.meshes ?? []).length,
  };
}

/** Validates the data (`dir`: where its GLB paths are relative to). Throws naming the bad entry. */
export function validate(data, palette, dir, read = (p) => readFileSync(p)) {
  const fail = (msg) => {
    throw new Error(`models.json: ${msg}`);
  };
  if (!data.models || typeof data.models !== 'object') fail('no models');
  for (const r of data.palette?.exclude ?? []) if (!palette.ramps[r]) fail(`palette.exclude: no ramp ${r}`);
  if (data.player && !data.models[data.player]) fail(`player: no model ${data.player}`);
  const out = {};
  for (const [name, m] of Object.entries(data.models)) {
    const at = `model ${name}`;
    if (!/^[a-z][a-z0-9_]*$/.test(name)) fail(`${at}: names are lower_snake_case`);
    if (typeof m.glb !== 'string') fail(`${at}: no glb`);
    const path = join(dir, m.glb);
    if (!existsSync(path)) fail(`${at}: missing GLB ${m.glb}`);
    let s;
    try {
      s = glbSummary(read(path));
    } catch (e) {
      fail(`${at}: ${m.glb}: ${e.message}`);
    }
    if (s.meshes !== 1) fail(`${at}: ${m.glb} has ${s.meshes} meshes, not one`);
    if (s.joints < 1 || s.joints > MAX_JOINTS) fail(`${at}: ${s.joints} joints (1..${MAX_JOINTS})`);
    if (!(m.scale > 0)) fail(`${at}: scale must be > 0`);
    if (typeof (m.yaw ?? 0) !== 'number') fail(`${at}: yaw is degrees`);
    for (const [src, to] of Object.entries(m.remap ?? {})) {
      if (!/^#[0-9a-f]{6}$/i.test(src)) fail(`${at}: remap key ${src} is not #rrggbb`);
      try {
        resolveColor(palette, to);
      } catch (e) {
        fail(`${at}: remap ${src}: ${e.message}`);
      }
    }
    const roles = Object.entries(m.roles ?? {});
    if (!roles.length) fail(`${at}: no roles`);
    for (const [role, r] of roles) {
      if (!ROLES.includes(role)) fail(`${at}: unknown role ${role} (${ROLES.join(', ')})`);
      if (!s.clips.includes(r.clip)) fail(`${at}: role ${role}: no clip ${r.clip} in ${m.glb} (${s.clips.join(', ')})`);
      if (r.speed !== undefined && !(r.speed > 0)) fail(`${at}: role ${role}: speed must be > 0`);
    }
    if (!m.roles[m.fallback]) fail(`${at}: fallback ${m.fallback} is not one of its roles`);
    out[name] = { ...m, clips: s.clips, joints: s.joints };
  }
  return out;
}

const f32 = (v) => (Number.isInteger(v) ? `${v}.0` : `${v}`);

/** The Rust table (`glbRoot`: the GLB paths' directory, relative to the crate's src). */
export function rust(data, models, glbRoot) {
  const lines = [
    '// Generated by shared/models/gen.mjs from shared/models/models.json. Do not edit.',
    'use crate::{ModelData, RoleClip};',
    '',
    `pub const PLAYER: &str = ${JSON.stringify(data.player ?? '')};`,
  ];
  const player = data.player && models[data.player];
  lines.push(
    player
      ? `pub static PLAYER_GLB: &[u8] = include_bytes!(${JSON.stringify(`${glbRoot}/${player.glb}`)});`
      : 'pub static PLAYER_GLB: &[u8] = &[];',
  );
  lines.push('pub static MODELS: &[ModelData] = &[');
  for (const [name, m] of Object.entries(models)) {
    const roles = Object.entries(m.roles)
      .map(([role, r]) => `RoleClip { role: ${JSON.stringify(role)}, clip: ${JSON.stringify(r.clip)}, speed: ${f32(r.speed ?? 1)} }`)
      .join(', ');
    lines.push(
      `    ModelData { name: ${JSON.stringify(name)}, glb: ${JSON.stringify(m.glb)}, joints: ${m.joints}, scale: ${f32(m.scale)}, yaw: ${f32(m.yaw ?? 0)}, fallback: ${JSON.stringify(m.fallback)}, roles: &[${roles}] },`,
    );
  }
  lines.push('];', '');
  return lines.join('\n');
}

const readJson = (p) => JSON.parse(readFileSync(p, 'utf8'));

export function generate() {
  const palette = readJson(join(root, 'shared/textures/palette.json'));
  const outputs = [];
  for (const [src, out] of [
    [here, 'models.gen.rs'],
    [join(here, 'fixtures'), 'fixture.gen.rs'],
  ]) {
    const data = readJson(join(src, 'models.json'));
    const models = validate(data, palette, src);
    outputs.push([join(CRATE, out), rust(data, models, relative(CRATE, src))]);
  }
  return outputs;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  for (const [path, text] of generate()) writeFileSync(path, text);
  console.log('shared/models: wrote models.gen.rs, fixture.gen.rs');
}
