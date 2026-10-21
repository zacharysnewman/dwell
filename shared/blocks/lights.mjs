// Light sources of the block registry (docs/LIGHTS.md §2): each namespace's `lights` table names the
// dynamic point lights that blocks reference by name (`"light": "dwell:torch"`), held in a hand or
// placed in the world. Looks only: the flood light a placed block gives off is its `emits` (the
// block light, docs/DAY_NIGHT.md §11); a `light` is the smooth light around it drawn by the
// renderer (and, in Phase 47's T2, its shadows). gen.mjs validates them with these functions and
// emits the table; lights.test.mjs calls them on fixtures to see a bad entry fail with its message.

const NAME = /^[a-z0-9_]+:[a-z0-9_]+$/;
const COLOR = /^#[0-9a-fA-F]{6}$/;

// Every number, required, with its range (inclusive unless `open`).
const NUMBERS = {
  strength: { min: 0, max: 16, open: 'min' }, // the light's irradiance at 1 m (× colour, linear)
  radiusM: { min: 0, max: 32, open: 'min' }, // where its light ends (a smooth window to 0)
};
const FLICKER = {
  amount: { min: 0, max: 1, open: 'max' }, // the share of its strength a flicker takes at most
  speed: { min: 0, max: 30, open: 'min' }, // how fast it flickers (the noise's cycles a second)
};
const FIELDS = ['color', ...Object.keys(NUMBERS), 'flicker', 'shadows'];

const inRange = (v, { min, max, open }) =>
  typeof v === 'number' &&
  Number.isFinite(v) &&
  (open === 'min' ? v > min : v >= min) &&
  (open === 'max' ? v < max : v <= max);
const range = ({ min, max, open }) => `${open === 'min' ? '(' : '['}${min}, ${max}${open === 'max' ? ')' : ']'}`;

/** One light entry, checked: throws `light <name>: …` naming the bad field. */
export function checkLight(name, l, namespace) {
  const fail = (m) => {
    throw new Error(`shared/blocks: light ${name}: ${m}`);
  };
  if (!NAME.test(name)) fail('the name must be `namespace:name` (lowercase, digits, _)');
  if (namespace !== undefined && !name.startsWith(`${namespace}:`)) {
    fail(`declared in namespace ${namespace}, so it must be named ${namespace}:…`);
  }
  if (typeof l !== 'object' || l === null || Array.isArray(l)) fail('must be an object');
  for (const k of Object.keys(l)) if (k !== '$comment' && !FIELDS.includes(k)) fail(`unknown field ${k}`);
  for (const k of FIELDS) if (l[k] === undefined) fail(`missing ${k}`);
  if (typeof l.color !== 'string' || !COLOR.test(l.color)) fail('color must be #rrggbb');
  for (const [k, r] of Object.entries(NUMBERS)) {
    if (!inRange(l[k], r)) fail(`${k} ${JSON.stringify(l[k])} is outside ${range(r)}`);
  }
  const f = l.flicker;
  if (typeof f !== 'object' || f === null || Array.isArray(f)) fail('flicker must be { amount, speed }');
  for (const k of Object.keys(f)) if (!(k in FLICKER)) fail(`unknown field flicker.${k}`);
  for (const [k, r] of Object.entries(FLICKER)) {
    if (!inRange(f[k], r)) fail(`flicker.${k} ${JSON.stringify(f[k])} is outside ${range(r)}`);
  }
  if (typeof l.shadows !== 'boolean') fail('shadows must be true or false');
  return { name, color: l.color, strength: l.strength, radiusM: l.radiusM, flicker: { ...f }, shadows: l.shadows };
}

/** All namespaces' lights, in id order (namespaces sorted, entries in declared order). */
export function readLights(namespaces) {
  const out = [];
  for (const ns of namespaces) {
    for (const [name, l] of Object.entries(ns.lights ?? {})) {
      if (name === '$comment') continue;
      if (out.some((o) => o.name === name)) throw new Error(`shared/blocks: duplicate light ${name}`);
      out.push(checkLight(name, l, ns.namespace));
    }
  }
  if (out.length > 255) throw new Error('shared/blocks: too many lights');
  return out;
}

/** A block's light reference, checked against `lights`: its index, or null for a block without one. */
export function blockLight(id, def, lights) {
  if (def.light === undefined) return null;
  if (typeof def.light !== 'string') throw new Error(`shared/blocks: ${id}: light must be a light's name`);
  const i = lights.findIndex((l) => l.name === def.light);
  if (i < 0) throw new Error(`shared/blocks: ${id}: unknown light ${def.light}`);
  return i;
}

/**
 * A block's held item, checked: `held` (the item a hand holding it shows, a name in
 * shared/characters/characters.json's `items`; else the hand shows the block itself) must name one
 * of `items`. Its glow is its texture's (`emissive` in shared/textures/materials.json).
 */
export function blockHeld(id, def, items) {
  if (def.held === undefined) return null;
  if (typeof def.held !== 'string' || !items.includes(def.held)) {
    throw new Error(
      `shared/blocks: ${id}: held must name an item of shared/characters/characters.json (one of ${items.join(', ')}; got ${JSON.stringify(def.held)})`,
    );
  }
  return def.held;
}

const hexNum = (c) => `0x${c.replace('#', '').toLowerCase()}`;

/** The Rust `LIGHTS` table (crates/dwell-blocks `LightInfo`). */
export function lightsRust(lights, rf) {
  const rows = lights.map(
    (l) =>
      `    LightInfo { name: ${JSON.stringify(l.name)}, color: ${hexNum(l.color)}, strength: ${rf(l.strength)}, radius_m: ${rf(l.radiusM)}, flicker: ${rf(l.flicker.amount)}, flicker_speed: ${rf(l.flicker.speed)}, shadows: ${l.shadows} },`,
  );
  return [`pub static LIGHTS: [LightInfo; ${lights.length}] = [`, ...rows, '];', ''];
}
