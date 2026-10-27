// The structure format's rules (docs/STRUCTURES.md §3), shared by the generator (gen.mjs, in Node)
// and the client's structures store (client/src/local/structures.ts, in the browser), so a
// structure saved in the sandbox is checked exactly as CI checks it. No Node built-ins here.

/** The format version this generator reads (a file's `"format"`). */
export const FORMAT = 1;
/** The largest horizontal edge of a hand-built (`"voxels"`) structure, in voxels (owner, 2026-10-14). */
export const MAX_EDGE = 32;
/** Its largest height, in rows of ½ m cells: 32 m, the owner's 32 at 1 m cells (2026-10-26, the
 * 30 m portal arch). */
export const MAX_HEIGHT = 64;
/** The largest edge along axis `i` (0 x, 1 y, 2 z). */
export const maxEdge = (i) => (i === 1 ? MAX_HEIGHT : MAX_EDGE);
/** Structure kinds: an open list; each needs its validator here and its body in the Rust loader. */
export const KINDS = ['voxels'];
/** Variant operations, in the canonical order they apply: an open list, as for kinds. */
export const VARIANT_OPS = ['rotate', 'mirror', 'choices'];
const ROTATIONS = [0, 90, 180, 270];
const MIRROR_AXES = ['x', 'z'];
const NAME = /^[a-z0-9_]+:[a-z0-9_]+$/;
export const TOP_LEVEL = ['$comment', 'format', 'name', 'kind', 'size', 'anchor', 'palette', 'layers', 'variants'];

// ---- The block registry, from the generated vector (shared/blocks/vectors.txt) -------------------

/** Blocks (id → ordered properties with their values in declared order) from the registry vector. */
export function registryFromVector(text) {
  const blocks = new Map();
  for (const line of text.split('\n')) {
    const m = /^\d+ (.+)$/.exec(line);
    if (!m) continue;
    const s = m[1];
    const open = s.indexOf('[');
    const id = open < 0 ? s : s.slice(0, open);
    const pairs = open < 0 ? [] : s.slice(open + 1, -1).split(',').map((p) => p.split('='));
    if (!blocks.has(id)) blocks.set(id, new Map(pairs.map(([k]) => [k, []])));
    const props = blocks.get(id);
    // Mixed radix enumerates each property's values in declared order: first sight is that order.
    for (const [k, v] of pairs) if (!props.get(k).includes(v)) props.get(k).push(v);
  }
  return blocks;
}

/**
 * The same registry from the client's generated block table (`BLOCK_DEFS` in
 * client/src/world/blocks.gen.ts: `{ id, properties: [{ name, values }] }`, values in declared order).
 */
export function registryFromBlockDefs(defs) {
  return new Map(defs.map((b) => [b.id, new Map(b.properties.map((p) => [p.name, [...p.values]]))]));
}

/** The canonical string of a block state named in any form `parse_state` takes, or an error. */
export function canonicalState(registry, text) {
  if (typeof text !== 'string') throw new Error(`a block state must be a string, not ${JSON.stringify(text)}`);
  const open = text.indexOf('[');
  if (open >= 0 && !text.endsWith(']')) throw new Error(`missing ']' in "${text}"`);
  const id = open < 0 ? text : text.slice(0, open);
  const props = registry.get(id);
  if (!props) throw new Error(`unknown block "${id}"`);
  const chosen = new Map();
  const list = open < 0 ? '' : text.slice(open + 1, -1);
  if (list) {
    for (const pair of list.split(',')) {
      const [k, v, extra] = pair.split('=');
      if (v === undefined || extra !== undefined) throw new Error(`expected key=value: "${pair}" in "${text}"`);
      if (!props.has(k)) throw new Error(`${id} has no property "${k}"`);
      if (chosen.has(k)) throw new Error(`repeated property "${k}" in "${text}"`);
      if (!props.get(k).includes(v)) throw new Error(`${id}.${k} has no value "${v}"`);
      chosen.set(k, v);
    }
  }
  if (!props.size) return id;
  const keys = [...props.keys()].sort();
  return `${id}[${keys.map((k) => `${k}=${chosen.get(k) ?? props.get(k)[0]}`).join(',')}]`;
}

// ---- Validation ----------------------------------------------------------------------------------

const isInt = (v) => Number.isInteger(v);
const isTriple = (v) => Array.isArray(v) && v.length === 3 && v.every(isInt);
const sorted = (xs, order) => [...xs].sort((a, b) => order.indexOf(a) - order.indexOf(b));

/**
 * Validates one parsed structure file and returns its canonical form. Every error names the file
 * and the entry (`playground.json: palette."s": unknown block "dwell:nope"`).
 */
export function validate(json, file, registry) {
  const fail = (entry, why) => {
    throw new Error(`${file}: ${entry}: ${why}`);
  };
  if (json === null || typeof json !== 'object' || Array.isArray(json)) fail('(file)', 'not a JSON object');
  for (const k of Object.keys(json)) {
    if (!TOP_LEVEL.includes(k)) fail(k, `unknown field (expected one of ${TOP_LEVEL.join(', ')})`);
  }
  if (json.format !== FORMAT) fail('format', `must be ${FORMAT}, not ${JSON.stringify(json.format)}`);
  if (typeof json.name !== 'string' || !NAME.test(json.name)) {
    fail('name', `must be namespace:name in [a-z0-9_], not ${JSON.stringify(json.name)}`);
  }
  if (!KINDS.includes(json.kind)) fail('kind', `unknown kind ${JSON.stringify(json.kind)} (known: ${KINDS.join(', ')})`);

  // kind "voxels": size, anchor, palette, layers.
  const { size, anchor } = json;
  if (!isTriple(size) || size.some((n, i) => n < 1 || n > maxEdge(i))) {
    fail('size', `must be [x, y, z], x and z 1..${MAX_EDGE}, y 1..${MAX_HEIGHT}, not ${JSON.stringify(size)}`);
  }
  if (!isTriple(anchor) || anchor.some((n, i) => n < 0 || n >= size[i])) {
    fail('anchor', `must be [x, y, z] inside the size ${JSON.stringify(size)}, not ${JSON.stringify(anchor)}`);
  }
  const palette = json.palette;
  if (palette === null || typeof palette !== 'object' || Array.isArray(palette)) fail('palette', 'must be an object');
  const states = new Map(); // char → canonical state or null (keep)
  for (const [c, name] of Object.entries(palette)) {
    if (c.length !== 1 || c < '!' || c > '~') fail(`palette."${c}"`, 'keys are single printable ASCII characters, not space');
    if (name === null) {
      states.set(c, null);
      continue;
    }
    try {
      states.set(c, canonicalState(registry, name));
    } catch (e) {
      fail(`palette."${c}"`, e.message);
    }
  }
  const layers = json.layers;
  if (!Array.isArray(layers) || layers.length !== size[1]) {
    fail('layers', `must hold size[1] = ${size[1]} y-slices, not ${Array.isArray(layers) ? layers.length : typeof layers}`);
  }
  layers.forEach((layer, y) => {
    if (!Array.isArray(layer) || layer.length !== size[2]) {
      fail(`layers[${y}]`, `must hold size[2] = ${size[2]} z-rows, not ${Array.isArray(layer) ? layer.length : typeof layer}`);
    }
    layer.forEach((row, z) => {
      if (typeof row !== 'string' || row.length !== size[0]) {
        fail(`layers[${y}][${z}]`, `must be a string of size[0] = ${size[0]} characters, not ${JSON.stringify(row)}`);
      }
      for (let x = 0; x < row.length; ++x) {
        if (!states.has(row[x])) fail(`layers[${y}][${z}][${x}]`, `"${row[x]}" is not in the palette`);
      }
    });
  });

  // Variants: named operations, each validated by its own rule.
  const variants = [];
  const v = json.variants ?? {};
  if (v === null || typeof v !== 'object' || Array.isArray(v)) fail('variants', 'must be an object');
  for (const op of Object.keys(v)) {
    if (!VARIANT_OPS.includes(op)) fail(`variants.${op}`, `unknown variant (known: ${VARIANT_OPS.join(', ')})`);
  }
  for (const op of VARIANT_OPS) {
    if (!(op in v)) continue;
    const value = v[op];
    const entry = `variants.${op}`;
    if (op === 'rotate' || op === 'mirror') {
      const allowed = op === 'rotate' ? ROTATIONS : MIRROR_AXES;
      if (!Array.isArray(value) || !value.length) fail(entry, `must be a non-empty list of ${allowed.join(', ')}`);
      for (const x of value) if (!allowed.includes(x)) fail(entry, `${JSON.stringify(x)} is not one of ${allowed.join(', ')}`);
      if (new Set(value).size !== value.length) fail(entry, 'repeats a value');
      variants.push({ op, values: sorted(value, allowed) });
    } else {
      if (value === null || typeof value !== 'object' || Array.isArray(value)) fail(entry, 'must map palette characters to options');
      const choices = [];
      for (const c of Object.keys(value).sort()) {
        const e = `${entry}."${c}"`;
        if (!states.has(c)) fail(e, `"${c}" is not in the palette`);
        if (states.get(c) === null) fail(e, `"${c}" keeps what is there (null): it has no block to vary`);
        const options = value[c];
        if (!Array.isArray(options) || !options.length) fail(e, 'must be a non-empty list of [block state, weight]');
        let total = 0;
        const out = options.map((o, i) => {
          if (!Array.isArray(o) || o.length !== 2) fail(`${e}[${i}]`, 'must be [block state, weight]');
          if (!isInt(o[1]) || o[1] < 1) fail(`${e}[${i}]`, `weight must be a positive integer, not ${JSON.stringify(o[1])}`);
          total += o[1];
          try {
            return [canonicalState(registry, o[0]), o[1]];
          } catch (err) {
            return fail(`${e}[${i}]`, err.message);
          }
        });
        if (total > 0x7fffffff) fail(e, 'weights add up to more than 2^31 - 1');
        choices.push({ key: c, options: out });
      }
      variants.push({ op, choices });
    }
  }

  const cells = layers.map((layer) => layer.join('')).join('');
  return { name: json.name, kind: json.kind, size, anchor, palette: states, cells, variants };
}

// ---- Canonical form and hash (dwell-world structure.rs rebuilds the same text) --------------------

/** The canonical text of a validated structure: the hash is taken over it. */
export function canonicalText(s) {
  const out = [`structure ${FORMAT}`, `name ${s.name}`, `kind ${s.kind}`];
  out.push(`size ${s.size.join(' ')}`, `anchor ${s.anchor.join(' ')}`);
  for (const c of [...s.palette.keys()].sort()) out.push(`palette ${c} ${s.palette.get(c) ?? 'keep'}`);
  const [sx, sy, sz] = s.size;
  for (let y = 0; y < sy; ++y) {
    out.push(`layer ${y}`);
    for (let z = 0; z < sz; ++z) out.push(s.cells.slice((y * sz + z) * sx, (y * sz + z + 1) * sx));
  }
  for (const v of s.variants) {
    if (v.op === 'choices') {
      for (const c of v.choices) out.push(`choices ${c.key} ${c.options.map(([n, w]) => `${n} ${w}`).join(' ')}`);
    } else {
      out.push(`${v.op} ${v.values.join(' ')}`);
    }
  }
  return `${out.join('\n')}\n`;
}

/** FNV-1a 64 over the UTF-8 bytes of the canonical text, as 16 hex digits. */
export function structureHash(text) {
  let h = 0xcbf29ce484222325n;
  for (const byte of new TextEncoder().encode(text)) {
    h ^= BigInt(byte);
    h = (h * 0x100000001b3n) & 0xffffffffffffffffn;
  }
  return h.toString(16).padStart(16, '0');
}

