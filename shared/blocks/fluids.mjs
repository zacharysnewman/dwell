// Fluid looks of the block registry (docs/BLOCK_REGISTRY.md, "Fluid looks", docs/WATER_RENDERING.md §2): each
// namespace's `fluids` table names looks that blocks with `"look": "fluid"` reference by name
// (`"fluid": "dwell:water"`). Looks only: behaviour (flow, viscosity, damage) is not data here.
// gen.mjs validates them with these functions and emits the table; the client's tests call them on
// fixtures to see a bad entry fail with its message.

const NAME = /^[a-z0-9_]+:[a-z0-9_]+$/;
const COLOR = /^#[0-9a-fA-F]{6}$/;

// Every field, required, with its range (inclusive unless `open`). Units: absorption per metre
// (Beer–Lambert σ, linear RGB), lengths in metres, speed in metres per second.
const NUMBERS = {
  opacity: { min: 0, max: 1, open: 'min' }, // the fixed alpha of the Low tier (no scene copies)
  surfaceDrop: { min: 0, max: 1, open: 'max' }, // the drawn top below the cell's top, in cells
  roughness: { min: 0, max: 1 },
  ior: { min: 1, max: 3 },
  normalScale: { min: 0, max: 10 },
  waveScale: { min: 0, max: 1000, open: 'min' },
  flowSpeed: { min: 0, max: 10 },
  scattering: { min: 0, max: 100 }, // σ per metre of the light the fluid scatters toward the eye (its body)
  refraction: { min: 0, max: 4 }, // how far (m) the scene behind shifts per unit of ripple slope
  sparkle: { min: 0, max: 100 }, // the sun's glitter off the ripples' facets
};
const FIELDS = [...Object.keys(NUMBERS), 'absorption', 'scatter', 'foam', 'caustics'];
// The caustics: light focused by the ripples onto what is under the surface.
const CAUSTICS = {
  strength: { min: 0, max: 10 }, // brightness of the lines (0: none)
  scale: { min: 0, max: 100, open: 'min' }, // the pattern's cell size (m)
  depth: { min: 0, max: 1000, open: 'min' }, // the depth (m) over which they fade to 1/e
};

const inRange = (v, { min, max, open }) =>
  typeof v === 'number' &&
  Number.isFinite(v) &&
  (open === 'min' ? v > min : v >= min) &&
  (open === 'max' ? v < max : v <= max);
const range = ({ min, max, open }) => `${open === 'min' ? '(' : '['}${min}, ${max}${open === 'max' ? ')' : ']'}`;

/** One fluid entry, checked: throws `fluid <name>: …` naming the bad field. */
export function checkFluid(name, f, namespace) {
  const fail = (m) => {
    throw new Error(`shared/blocks: fluid ${name}: ${m}`);
  };
  if (!NAME.test(name)) fail('the name must be `namespace:name` (lowercase, digits, _)');
  if (namespace !== undefined && !name.startsWith(`${namespace}:`)) {
    fail(`declared in namespace ${namespace}, so it must be named ${namespace}:…`);
  }
  if (typeof f !== 'object' || f === null || Array.isArray(f)) fail('must be an object');
  for (const k of Object.keys(f)) if (!FIELDS.includes(k)) fail(`unknown field ${k}`);
  for (const k of FIELDS) if (f[k] === undefined) fail(`missing ${k}`);
  for (const [k, r] of Object.entries(NUMBERS)) {
    if (!inRange(f[k], r)) fail(`${k} ${JSON.stringify(f[k])} is outside ${range(r)}`);
  }
  const a = f.absorption;
  if (!Array.isArray(a) || a.length !== 3 || !a.every((v) => inRange(v, { min: 0, max: 100 }))) {
    fail('absorption must be three numbers in [0, 100] (per metre, red green blue)');
  }
  if (typeof f.scatter !== 'string' || !COLOR.test(f.scatter)) fail('scatter must be #rrggbb');
  const foam = f.foam;
  if (typeof foam !== 'object' || foam === null) fail('foam must be { depth, color }');
  for (const k of Object.keys(foam)) if (k !== 'depth' && k !== 'color') fail(`unknown field foam.${k}`);
  if (!inRange(foam.depth, { min: 0, max: 16 })) fail(`foam.depth ${JSON.stringify(foam.depth)} is outside [0, 16]`);
  if (typeof foam.color !== 'string' || !COLOR.test(foam.color)) fail('foam.color must be #rrggbb');
  const c = f.caustics;
  if (typeof c !== 'object' || c === null || Array.isArray(c)) fail('caustics must be { strength, scale, depth }');
  for (const k of Object.keys(c)) if (!(k in CAUSTICS)) fail(`unknown field caustics.${k}`);
  for (const [k, r] of Object.entries(CAUSTICS)) {
    if (!inRange(c[k], r)) fail(`caustics.${k} ${JSON.stringify(c[k])} is outside ${range(r)}`);
  }
  return { name, ...f };
}

/** All namespaces' fluids, in id order (namespaces sorted, entries in declared order). */
export function readFluids(namespaces) {
  const out = [];
  for (const ns of namespaces) {
    for (const [name, f] of Object.entries(ns.fluids ?? {})) {
      if (out.some((o) => o.name === name)) throw new Error(`shared/blocks: duplicate fluid ${name}`);
      out.push(checkFluid(name, f, ns.namespace));
    }
  }
  if (out.length > 255) throw new Error('shared/blocks: too many fluids (ids are a byte)');
  return out;
}

/**
 * A block's fluid reference, checked against `fluids`: its index, or null for blocks without the
 * fluid look. A fluid block takes its opacity from the fluid.
 */
export function blockFluid(id, def, fluids) {
  const fail = (m) => {
    throw new Error(`shared/blocks: ${id}: ${m}`);
  };
  if (def.look !== 'fluid') {
    if (def.fluid !== undefined) fail('`fluid` needs "look": "fluid"');
    return null;
  }
  if (typeof def.fluid !== 'string') fail('"look": "fluid" needs a `fluid` name');
  const i = fluids.findIndex((f) => f.name === def.fluid);
  if (i < 0) fail(`unknown fluid ${def.fluid}`);
  if (def.opacity !== undefined) fail(`opacity comes from the fluid ${def.fluid}`);
  return i;
}

const hexNum = (c) => `0x${c.replace('#', '').toLowerCase()}`;

/** The Rust `FLUIDS` table (crates/dwell-blocks `FluidInfo`). */
export function fluidsRust(fluids, rf) {
  const rows = fluids.map(
    (f) =>
      `    FluidInfo { name: ${JSON.stringify(f.name)}, opacity: ${rf(f.opacity)}, surface_drop: ${rf(f.surfaceDrop)}, absorption: [${f.absorption.map(rf).join(', ')}], scatter: ${hexNum(f.scatter)}, roughness: ${rf(f.roughness)}, ior: ${rf(f.ior)}, normal_scale: ${rf(f.normalScale)}, wave_scale: ${rf(f.waveScale)}, flow_speed: ${rf(f.flowSpeed)}, foam_depth: ${rf(f.foam.depth)}, foam_color: ${hexNum(f.foam.color)}, caustics: ${rf(f.caustics.strength)}, caustic_scale: ${rf(f.caustics.scale)}, caustic_depth: ${rf(f.caustics.depth)}, scattering: ${rf(f.scattering)}, refraction: ${rf(f.refraction)}, sparkle: ${rf(f.sparkle)} },`,
  );
  return [`pub static FLUIDS: [FluidInfo; ${fluids.length}] = [`, ...rows, '];', ''];
}
