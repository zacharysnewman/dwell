// The capabilities table (shared/protocol/capabilities.json, ARCHITECTURE.md §8.3 "Capabilities"):
// optional features named by string, announced at join. A new optional feature adds an entry here,
// not a flag bit. gen.mjs emits the names into dwell-protocol (`capability_names`) and the TS
// constants.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));

/** Lowercase, dotted (`area.feature`); a mod prefixes its own (`mymod:thing`). */
export const CAP_NAME = /^[a-z0-9][a-z0-9_.:-]*$/;
/** The server announces every name in one WorldSettings field (`world_settings::MAX_FIELD_BYTES`). */
export const MAX_SETTINGS_FIELD_BYTES = 1024;

export function loadCapabilities() {
  return JSON.parse(readFileSync(join(here, 'capabilities.json'), 'utf8'));
}

/** The Rust constant a name becomes: `structures.place` → `STRUCTURES_PLACE`. */
export const capConst = (name) => name.replace(/[^a-z0-9]+/g, '_').toUpperCase();

export function validateCapabilities(t) {
  const fail = (m) => {
    throw new Error(`capabilities.json: ${m}`);
  };
  if (!t || t.formatVersion !== 1) fail('formatVersion must be 1');
  for (const k of ['maxNames', 'maxNameBytes']) {
    if (!Number.isInteger(t[k]) || t[k] < 1 || t[k] > 255) fail(`${k} must be a whole number 1..255`);
  }
  if (!Array.isArray(t.capabilities)) fail('needs a capabilities list');
  if (t.capabilities.length > t.maxNames) fail(`more than maxNames (${t.maxNames}) capabilities`);
  const names = new Set();
  const consts = new Set();
  // u8 count, then each name as a u16 length and its bytes.
  let bytes = 1;
  for (const e of t.capabilities) {
    if (typeof e.name !== 'string' || !CAP_NAME.test(e.name)) {
      fail(`bad name ${JSON.stringify(e?.name)} (lowercase letters, digits, _ . : -)`);
    }
    if (Buffer.byteLength(e.name) > t.maxNameBytes) fail(`"${e.name}" is longer than maxNameBytes (${t.maxNameBytes})`);
    if (names.has(e.name)) fail(`"${e.name}" twice`);
    names.add(e.name);
    const k = capConst(e.name);
    if (consts.has(k)) fail(`"${e.name}" makes the same constant as another (${k})`);
    consts.add(k);
    if (typeof e.description !== 'string' || e.description.length === 0) fail(`"${e.name}" needs a description`);
    for (const f of Object.keys(e)) if (!['name', 'description'].includes(f)) fail(`"${e.name}" has no field "${f}"`);
    bytes += 2 + Buffer.byteLength(e.name);
  }
  if (bytes > MAX_SETTINGS_FIELD_BYTES) {
    fail(`the names take ${bytes} bytes, more than one WorldSettings field (${MAX_SETTINGS_FIELD_BYTES})`);
  }
  return t;
}

/** Lines of `constants.gen.rs`: the `capability_names` module. */
export function capabilitiesRust(t) {
  const rs = [];
  rs.push('/// The capabilities (`shared/protocol/capabilities.json`, ARCHITECTURE.md §8.3): optional');
  rs.push('/// features named by string, announced at join (`dwell_protocol::capabilities`).');
  rs.push('pub mod capability_names {');
  rs.push(`    pub const MAX_NAMES: usize = ${t.maxNames};`);
  rs.push(`    pub const MAX_NAME_BYTES: usize = ${t.maxNameBytes};`);
  for (const e of t.capabilities) {
    rs.push(`    /// ${e.description}`);
    rs.push(`    pub const ${capConst(e.name)}: &str = "${e.name}";`);
  }
  rs.push("    /// Every capability this build knows, in the table's order.");
  rs.push(`    pub const ALL: &[&str] = &[${t.capabilities.map((e) => capConst(e.name)).join(', ')}];`);
  rs.push('}');
  return rs;
}

/** Lines of `constants.gen.ts`: the `Capabilities` constant. */
export function capabilitiesTs(t) {
  return [
    '/** The capabilities (shared/protocol/capabilities.json): optional features named by string. */',
    'export const Capabilities = {',
    `  maxNames: ${t.maxNames},`,
    `  maxNameBytes: ${t.maxNameBytes},`,
    `  names: [${t.capabilities.map((e) => `'${e.name}'`).join(', ')}],`,
    '} as const;',
  ];
}
