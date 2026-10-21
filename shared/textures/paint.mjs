// The painterly texture generator (docs/PAINTERLY_TEXTURES.md, ADR 0040): turns a material of
// shared/textures/materials.json into a TILE × TILE RGBA tile, with colours from
// shared/textures/palette.json. Pure and dependency-free, so the build (gen.mjs), the tests, the
// editor page (docs/prototypes/painterly-texture-lab.html) and the character prototypes run the
// same code. The vocabulary below (layer kinds, placement, the stroke shapes) is the engine and is
// fixed; every material is data.
//
// Coordinates are texels with y up: texel (x, y) has its centre at (x + 0.5, y + 0.5) and y = TILE − 1
// is the tile's top row (the atlas's convention, crates/dwell-mesh/src/textures.rs). Tiles come out
// in that order, row y = 0 first; `imageRows` flips them for an image whose first row is its top.

/** Texels per tile edge (the atlas's `TILE`). */
export const TILE = 32;

/** The stroke shapes. */
export const STROKE_KINDS = ['dab', 'streak', 'blade', 'slab', 'ring'];
/** Every layer kind: base layers, stroke layers and the painted highlight. */
export const LAYER_KINDS = ['fill', 'gradient', 'edgeLight', ...STROKE_KINDS];
/** What a material is for (blocks feed the atlas; characters and sky accents are read by name). */
export const USES = ['block', 'character', 'sky'];
export const RAMP_STOPS = ['shadow', 'base', 'light'];
const DIRECTIONS = ['up', 'down', 'left', 'right', 'radial'];
const EDGES = ['top', 'bottom', 'left', 'right', 'all'];

const MATERIAL_KEYS = ['$comment', 'use', 'ramp', 'overlay', 'wrap', 'alpha', 'emissive', 'like', 'over', 'layers'];
const COMMON_KEYS = ['$comment', 'kind', 'opacity', 'clip'];
const LAYER_KEYS = {
  fill: ['color'],
  gradient: ['from', 'to', 'direction', 'start', 'end', 'steps'],
  edgeLight: ['color', 'edge', 'width', 'ragged', 'seed'],
  stroke: [
    'colors', 'count', 'at', 'band', 'jitter', 'width', 'length', 'angle', 'angleJitter', 'bend',
    'softness', 'arc', 'seed',
  ],
};

/** Hash of (i, j, k, seed) → [0, 1), in int32 arithmetic (the atlas's hash). */
export function hash(i, j, k, seed) {
  let h =
    (Math.imul(i | 0, 374761393) +
      Math.imul(j | 0, 668265263) +
      Math.imul(k | 0, 2147483647) +
      Math.imul(seed | 0, 144269504)) |
    0;
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  h ^= h >>> 16;
  return (h >>> 0) / 4294967296;
}

const clamp01 = (v) => (v < 0 ? 0 : v > 1 ? 1 : v);
const isNum = (v) => typeof v === 'number' && Number.isFinite(v);
const HEX = /^#[0-9a-fA-F]{6}$/;

function hexRgb(hex) {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** Checks the palette; throws naming the bad entry. */
export function validatePalette(palette) {
  const fail = (m) => {
    throw new Error(`palette.json: ${m}`);
  };
  if (typeof palette !== 'object' || palette === null) fail('must be an object');
  for (const k of Object.keys(palette)) {
    if (!['$comment', 'ramps', 'swatches'].includes(k)) fail(`unknown entry ${k}`);
  }
  const names = new Set();
  for (const [name, ramp] of Object.entries(palette.ramps ?? {})) {
    if (!/^[a-z][a-z0-9_]*$/.test(name)) fail(`ramp name ${JSON.stringify(name)} must be lower_snake_case`);
    if (typeof ramp !== 'object' || ramp === null) fail(`ramps.${name} must be an object`);
    for (const k of Object.keys(ramp)) {
      if (!RAMP_STOPS.includes(k)) fail(`unknown stop ramps.${name}.${k} (stops: ${RAMP_STOPS.join(', ')})`);
    }
    for (const s of RAMP_STOPS) {
      if (!HEX.test(ramp[s] ?? '')) fail(`ramps.${name}.${s} must be a colour "#rrggbb", got ${JSON.stringify(ramp[s])}`);
    }
    names.add(name);
  }
  for (const [name, hex] of Object.entries(palette.swatches ?? {})) {
    if (!/^[a-z][a-z0-9_]*$/.test(name)) fail(`swatch name ${JSON.stringify(name)} must be lower_snake_case`);
    if (names.has(name)) fail(`swatch ${name} has the name of a ramp`);
    if (!HEX.test(hex)) fail(`swatches.${name} must be a colour "#rrggbb", got ${JSON.stringify(hex)}`);
  }
}

/**
 * The colour (sRGB bytes) of palette reference `ref`: "ramp.stop", "ramp" (its base), a swatch, or
 * "@stop" — a stop of `ramp`, the material's own ramp. Throws naming `where` if it is unknown.
 */
export function resolveColor(palette, ref, ramp, where = 'colour') {
  if (typeof ref !== 'string') throw new Error(`${where}: a palette reference must be a string, got ${JSON.stringify(ref)}`);
  let name = ref;
  let stop = 'base';
  if (ref.startsWith('@')) {
    if (!ramp) throw new Error(`${where}: "${ref}" needs the material's ramp`);
    name = ramp;
    stop = ref.slice(1);
  } else if (ref.includes('.')) {
    [name, stop] = ref.split('.', 2);
  } else if (palette.swatches?.[ref]) {
    return hexRgb(palette.swatches[ref]);
  }
  const r = palette.ramps?.[name];
  if (!r) throw new Error(`${where}: unknown palette colour "${ref}"`);
  if (!RAMP_STOPS.includes(stop)) throw new Error(`${where}: unknown stop "${stop}" in "${ref}" (stops: ${RAMP_STOPS.join(', ')})`);
  return hexRgb(r[stop]);
}

/**
 * A material with `like` (a template: its fields, overridden by this one's) resolved. Throws on an
 * unknown or cyclic template.
 */
export function resolveMaterial(materials, name, seen = []) {
  const m = materials.materials?.[name];
  if (!m) throw new Error(`materials.json: unknown material "${name}"${seen.length ? ` (from ${seen.at(-1)})` : ''}`);
  if (seen.includes(name)) throw new Error(`materials.json: materials.${name}: "like" forms a cycle (${[...seen, name].join(' → ')})`);
  if (m.like === undefined) return m;
  const base = resolveMaterial(materials, m.like, [...seen, name]);
  const { like: _like, ...own } = m;
  return { ...base, ...own };
}

function checkRange(where, v, lo, hi, { pair = false, int = false } = {}) {
  const ok = (n) => isNum(n) && n >= lo && n <= hi && (!int || Number.isInteger(n));
  if (pair && Array.isArray(v)) {
    if (v.length !== 2 || !ok(v[0]) || !ok(v[1]) || v[0] > v[1]) {
      throw new Error(`${where} must be [min, max] in [${lo}, ${hi}], got ${JSON.stringify(v)}`);
    }
    return;
  }
  if (!ok(v)) {
    throw new Error(`${where} must be ${int ? 'a whole number' : 'a number'}${pair ? ' (or [min, max])' : ''} in [${lo}, ${hi}], got ${JSON.stringify(v)}`);
  }
}

function validateLayer(where, layer, palette, ramp) {
  if (typeof layer !== 'object' || layer === null) throw new Error(`${where} must be an object`);
  if (!LAYER_KINDS.includes(layer.kind)) {
    throw new Error(`${where}.kind must be one of ${LAYER_KINDS.join(', ')}, got ${JSON.stringify(layer.kind)}`);
  }
  const own = STROKE_KINDS.includes(layer.kind) ? LAYER_KEYS.stroke : LAYER_KEYS[layer.kind];
  for (const k of Object.keys(layer)) {
    if (!COMMON_KEYS.includes(k) && !own.includes(k)) throw new Error(`${where}: unknown entry "${k}" for a ${layer.kind} layer`);
  }
  const color = (k, v) => resolveColor(palette, v, ramp, `${where}.${k}`);
  if (layer.opacity !== undefined) checkRange(`${where}.opacity`, layer.opacity, 0, 1, { pair: true });
  if (layer.clip !== undefined && typeof layer.clip !== 'boolean') throw new Error(`${where}.clip must be true or false`);
  switch (layer.kind) {
    case 'fill':
      color('color', layer.color);
      return;
    case 'gradient':
      color('from', layer.from);
      color('to', layer.to);
      if (!DIRECTIONS.includes(layer.direction)) throw new Error(`${where}.direction must be one of ${DIRECTIONS.join(', ')}`);
      if (layer.start !== undefined) checkRange(`${where}.start`, layer.start, 0, 1);
      if (layer.end !== undefined) checkRange(`${where}.end`, layer.end, 0, 1);
      if ((layer.start ?? 0) >= (layer.end ?? 1)) throw new Error(`${where}: start must be below end`);
      if (layer.steps !== undefined) checkRange(`${where}.steps`, layer.steps, 2, 16, { int: true });
      return;
    case 'edgeLight':
      color('color', layer.color);
      if (!EDGES.includes(layer.edge)) throw new Error(`${where}.edge must be one of ${EDGES.join(', ')}`);
      checkRange(`${where}.width`, layer.width, 0.25, TILE / 2);
      if (layer.ragged !== undefined) checkRange(`${where}.ragged`, layer.ragged, 0, TILE / 4);
      if (layer.seed !== undefined) checkRange(`${where}.seed`, layer.seed, 0, 1e6, { int: true });
      return;
    default: {
      if (!Array.isArray(layer.colors) || layer.colors.length === 0) throw new Error(`${where}.colors must be a non-empty list of palette colours`);
      layer.colors.forEach((c, i) => color(`colors[${i}]`, c));
      if ((layer.count === undefined) === (layer.at === undefined)) throw new Error(`${where}: give exactly one of "count" (scattered) or "at" (positions)`);
      if (layer.count !== undefined) checkRange(`${where}.count`, layer.count, 1, 256, { int: true });
      if (layer.at !== undefined) {
        if (!Array.isArray(layer.at) || layer.at.length === 0 || layer.at.length > 256) throw new Error(`${where}.at must be a list of 1–256 positions`);
        layer.at.forEach((p, i) => {
          if (!Array.isArray(p) || (p.length !== 2 && p.length !== 3) || !p.every(isNum)) {
            throw new Error(`${where}.at[${i}] must be [x, y] or [x, y, length] in texels, got ${JSON.stringify(p)}`);
          }
        });
      }
      if (layer.band !== undefined) {
        checkRange(`${where}.band`, layer.band, 0, 1, { pair: true });
        if (!Array.isArray(layer.band)) throw new Error(`${where}.band must be [bottom, top], shares of the tile's height`);
      }
      if (layer.jitter !== undefined) checkRange(`${where}.jitter`, layer.jitter, 0, 1);
      checkRange(`${where}.width`, layer.width, 0.25, TILE, { pair: true });
      checkRange(`${where}.length`, layer.length, 0.25, 2 * TILE, { pair: true });
      if (layer.angle !== undefined) checkRange(`${where}.angle`, layer.angle, -360, 360);
      if (layer.angleJitter !== undefined) checkRange(`${where}.angleJitter`, layer.angleJitter, 0, 180);
      if (layer.bend !== undefined) checkRange(`${where}.bend`, layer.bend, -TILE / 2, TILE / 2, { pair: true });
      if (layer.softness !== undefined) checkRange(`${where}.softness`, layer.softness, 0.25, 8);
      if (layer.arc !== undefined) {
        if (layer.kind !== 'ring') throw new Error(`${where}.arc is for ring layers only`);
        checkRange(`${where}.arc`, layer.arc, 1, 360);
      }
      if (layer.seed !== undefined) checkRange(`${where}.seed`, layer.seed, 0, 1e6, { int: true });
    }
  }
}

/**
 * Checks the materials against the palette; `atlasTiles` (optional) are the atlas's tiles (the
 * texture names the block registry draws with, and the engine's utility tiles), each of which
 * `tiles` must map, and nothing else. Throws naming the bad entry.
 */
export function validateMaterials(materials, palette, atlasTiles = null) {
  const fail = (m) => {
    throw new Error(`materials.json: ${m}`);
  };
  if (typeof materials !== 'object' || materials === null) fail('must be an object');
  for (const k of Object.keys(materials)) {
    if (!['$comment', 'tileSize', 'tiles', 'materials'].includes(k)) fail(`unknown entry ${k}`);
  }
  if (materials.tileSize !== TILE) fail(`tileSize must be ${TILE} (the atlas's TILE), got ${JSON.stringify(materials.tileSize)}`);
  const all = materials.materials;
  if (typeof all !== 'object' || all === null || Object.keys(all).length === 0) fail('materials must be a non-empty object');
  for (const name of Object.keys(all)) {
    const where = `materials.${name}`;
    if (!/^[a-z][a-z0-9_]*$/.test(name)) fail(`material name ${JSON.stringify(name)} must be lower_snake_case`);
    for (const k of Object.keys(all[name])) {
      if (!MATERIAL_KEYS.includes(k)) fail(`${where}: unknown entry "${k}"`);
    }
    const m = resolveMaterial(materials, name);
    if (!USES.includes(m.use)) fail(`${where}.use must be one of ${USES.join(', ')}, got ${JSON.stringify(m.use)}`);
    if (!palette.ramps?.[m.ramp]) fail(`${where}.ramp: unknown ramp ${JSON.stringify(m.ramp)}`);
    if (m.overlay !== undefined) checkRange(`materials.json: ${where}.overlay`, m.overlay, 0, 1);
    if (m.wrap !== undefined && typeof m.wrap !== 'boolean') fail(`${where}.wrap must be true or false`);
    // `emissive` (docs/LIGHTS.md): how much of its colour it gives off unlit, a flame's glow.
    if (m.emissive !== undefined) checkRange(`materials.json: ${where}.emissive`, m.emissive, 0, 4);
    if (m.alpha !== undefined && !['opaque', 'strokes'].includes(m.alpha)) fail(`${where}.alpha must be "opaque" or "strokes"`);
    if (m.over !== undefined) {
      const chain = [name];
      for (let o = m.over; o !== undefined; o = resolveMaterial(materials, o).over) {
        if (chain.includes(o)) fail(`${where}.over forms a cycle (${[...chain, o].join(' → ')})`);
        chain.push(o);
      }
    }
    if (!Array.isArray(m.layers) || m.layers.length === 0) fail(`${where}.layers must be a non-empty list`);
    m.layers.forEach((l, i) => validateLayer(`materials.json: ${where}.layers[${i}]`, l, palette, m.ramp));
  }
  const tiles = materials.tiles ?? {};
  for (const [tile, mat] of Object.entries(tiles)) {
    if (!all[mat]) fail(`tiles.${tile}: unknown material "${mat}"`);
    if (resolveMaterial(materials, mat).use !== 'block') fail(`tiles.${tile}: material "${mat}" is not a block material`);
  }
  if (atlasTiles) {
    for (const t of atlasTiles) {
      if (!(t in tiles)) fail(`the atlas has tile "${t}", which "tiles" doesn't paint`);
    }
    for (const t of Object.keys(tiles)) {
      if (!atlasTiles.includes(t)) fail(`tiles.${t}: the atlas has no tile "${t}" (no block in the registry draws with it)`);
    }
  }
}

const pick = (v, h) => (Array.isArray(v) ? v[0] + (v[1] - v[0]) * h : v);

/** The positions of a stroke layer's strokes: [x, y, length | undefined, index]. */
function placements(layer, seed) {
  if (layer.at) return layer.at.map((p, i) => [p[0], p[1], p[2], i]);
  const [b0, b1] = layer.band ?? [0, 1];
  const h = Math.max((b1 - b0) * TILE, 1e-3);
  const n = layer.count;
  // A jittered grid: one stroke per cell, so coverage is even and nothing clumps. A thin band asks
  // for more columns than strokes; there are never more columns than strokes, so the strokes of a
  // one-row band span the whole width (an edge fringe), not only its first `count` cells.
  const cols = Math.min(n, Math.max(1, Math.round(Math.sqrt((n * TILE) / h))));
  const rows = Math.ceil(n / cols);
  const jitter = layer.jitter ?? 1;
  const out = [];
  for (let i = 0; i < n; i++) {
    const cx = i % cols;
    const cy = Math.floor(i / cols);
    // Odd rows shift half a cell, so the grid reads as scattered, not as rows.
    const shift = cy % 2 ? 0.5 : 0;
    const jx = 0.5 + (hash(i, 0, 1, seed) - 0.5) * jitter;
    const jy = 0.5 + (hash(i, 0, 2, seed) - 0.5) * jitter;
    out.push([((cx + shift + jx) * TILE) / cols, b0 * TILE + ((cy + jy) * h) / rows, undefined, i]);
  }
  return out;
}

/** Signed distance (texels, < 0 inside) from local point (s along, t across) to a stroke's shape. */
function shapeDistance(kind, s, t, w, len, bend, q, arc, angle) {
  const hw = w / 2;
  switch (kind) {
    case 'dab': {
      const a = Math.max(len / 2, 1e-3);
      const b = Math.max(hw, 1e-3);
      return (Math.hypot(s / a, t / b) - 1) * Math.min(a, b);
    }
    case 'streak': {
      const half = len / 2;
      const u = s / half;
      if (Math.abs(u) >= 1) return Math.hypot(Math.abs(s) - half, t);
      const c = bend * (1 - u * u);
      return Math.abs(t - c) - hw * Math.pow(1 - u * u, 0.6);
    }
    case 'blade': {
      if (s < 0) return Math.hypot(s, Math.max(Math.abs(t) - hw, 0)) || -s;
      const u = s / len;
      if (u >= 1) return Math.hypot(s - len, t - bend);
      return Math.abs(t - bend * u * u) - hw * Math.pow(1 - u, 0.8);
    }
    case 'slab':
      return Math.max(Math.abs(s) - len / 2, Math.abs(t) - hw);
    case 'ring': {
      // q: [dx, dy] from the centre; len is the radius.
      const r = Math.hypot(q[0], q[1]);
      let d = Math.abs(r - len) - hw;
      if (arc < 360) {
        let da = Math.abs(((Math.atan2(q[1], q[0]) * 180) / Math.PI - angle) % 360);
        if (da > 180) da = 360 - da;
        d = Math.max(d, ((da - arc / 2) * Math.PI * len) / 180);
      }
      return d;
    }
    default:
      throw new Error(`unknown stroke kind ${kind}`);
  }
}

function paintStrokes(layer, li, buf, alpha, cover, wrap, palette, ramp) {
  const seed = (layer.seed ?? 0) * 7919 + li * 104729 + 17;
  const colors = layer.colors.map((c) => resolveColor(palette, c, ramp));
  const soft = layer.softness ?? 1;
  const arc = layer.arc ?? 360;
  for (const [px, py, fixedLen, i] of placements(layer, seed)) {
    const color = colors[Math.floor(hash(i, 1, 3, seed) * colors.length) % colors.length];
    const w = pick(layer.width, hash(i, 1, 4, seed));
    const len = fixedLen ?? pick(layer.length, hash(i, 1, 5, seed));
    const angle = (layer.angle ?? 0) + (layer.angleJitter ?? 0) * (2 * hash(i, 1, 6, seed) - 1);
    const bend = Array.isArray(layer.bend) ? pick(layer.bend, hash(i, 1, 7, seed)) : (layer.bend ?? 0);
    const op = pick(layer.opacity ?? 1, hash(i, 1, 8, seed));
    const rad = (angle * Math.PI) / 180;
    const ux = Math.cos(rad);
    const uy = Math.sin(rad);
    const reach = Math.ceil(len + w + Math.abs(bend) + 2 * soft + 1);
    const offsets = wrap ? [-TILE, 0, TILE] : [0];
    for (const oy of offsets) {
      for (const ox of offsets) {
        const cx = px + ox;
        const cy = py + oy;
        const x0 = Math.max(0, Math.floor(cx - reach));
        const x1 = Math.min(TILE - 1, Math.ceil(cx + reach));
        const y0 = Math.max(0, Math.floor(cy - reach));
        const y1 = Math.min(TILE - 1, Math.ceil(cy + reach));
        for (let y = y0; y <= y1; y++) {
          for (let x = x0; x <= x1; x++) {
            const dx = x + 0.5 - cx;
            const dy = y + 0.5 - cy;
            const s = dx * ux + dy * uy;
            const t = -dx * uy + dy * ux;
            const d = shapeDistance(layer.kind, s, t, w, len, bend, [dx, dy], arc, angle);
            const cov = clamp01(0.5 - d / soft);
            if (cov <= 0) continue;
            const k = y * TILE + x;
            const a = cov * op * (layer.clip ? alpha[k] : 1);
            const o = k * 3;
            buf[o] += (color[0] - buf[o]) * a;
            buf[o + 1] += (color[1] - buf[o + 1]) * a;
            buf[o + 2] += (color[2] - buf[o + 2]) * a;
            if (cover && !layer.clip) alpha[k] = Math.max(alpha[k], cov);
          }
        }
      }
    }
  }
}

/** Smooth periodic 1-D value noise over the tile (cells per tile), in [0, 1]. */
function edgeNoise(x, cells, seed) {
  const f = (x / TILE) * cells;
  const i = Math.floor(f);
  const t = f - i;
  const s = t * t * (3 - 2 * t);
  const a = hash(((i % cells) + cells) % cells, 0, 11, seed);
  const b = hash((((i + 1) % cells) + cells) % cells, 0, 11, seed);
  return a + (b - a) * s;
}

function paintLayers(materials, palette, name, buf, alpha, depth) {
  const m = resolveMaterial(materials, name);
  if (m.over !== undefined) {
    if (depth > 8) throw new Error(`materials.json: materials.${name}.over is nested too deep`);
    paintLayers(materials, palette, m.over, buf, alpha, depth + 1);
  }
  const ramp = m.ramp;
  const wrap = m.wrap ?? true;
  const cover = m.alpha === 'strokes';
  m.layers.forEach((layer, li) => {
    const op = layer.opacity ?? 1;
    const at = (k) => op * (layer.clip ? alpha[k] : 1);
    const mixAt = (k, c, a) => {
      const o = k * 3;
      buf[o] += (c[0] - buf[o]) * a;
      buf[o + 1] += (c[1] - buf[o + 1]) * a;
      buf[o + 2] += (c[2] - buf[o + 2]) * a;
    };
    if (layer.kind === 'fill') {
      const c = resolveColor(palette, layer.color, ramp);
      for (let k = 0; k < TILE * TILE; k++) mixAt(k, c, at(k));
    } else if (layer.kind === 'gradient') {
      const from = resolveColor(palette, layer.from, ramp);
      const to = resolveColor(palette, layer.to, ramp);
      const start = layer.start ?? 0;
      const end = layer.end ?? 1;
      for (let y = 0; y < TILE; y++) {
        for (let x = 0; x < TILE; x++) {
          const fx = (x + 0.5) / TILE;
          const fy = (y + 0.5) / TILE;
          let t = { up: fy, down: 1 - fy, right: fx, left: 1 - fx }[layer.direction];
          if (layer.direction === 'radial') t = Math.min(1, Math.hypot(fx - 0.5, fy - 0.5) / 0.5);
          t = clamp01((t - start) / (end - start));
          // Steps: flat painted bands instead of a smooth blend.
          if (layer.steps) t = Math.min(layer.steps - 1, Math.floor(t * layer.steps)) / (layer.steps - 1);
          const c = [0, 1, 2].map((i) => from[i] + (to[i] - from[i]) * t);
          mixAt(y * TILE + x, c, at(y * TILE + x));
        }
      }
    } else if (layer.kind === 'edgeLight') {
      const c = resolveColor(palette, layer.color, ramp);
      const seed = (layer.seed ?? 0) * 7919 + li * 104729 + 29;
      const ragged = layer.ragged ?? 0;
      for (let y = 0; y < TILE; y++) {
        for (let x = 0; x < TILE; x++) {
          const fx = x + 0.5;
          const fy = y + 0.5;
          // Distance from the edge, and the band's ragged inner boundary along it.
          const sides = {
            top: [TILE - fy, fx],
            bottom: [fy, fx],
            left: [fx, fy],
            right: [TILE - fx, fy],
          };
          const pickSides = layer.edge === 'all' ? Object.values(sides) : [sides[layer.edge]];
          let cov = 0;
          pickSides.forEach(([d, along], si) => {
            const w = layer.width + ragged * (2 * edgeNoise(along, 4, seed + si) - 1);
            cov = Math.max(cov, clamp01(w - d + 0.5));
          });
          if (cov > 0) mixAt(y * TILE + x, c, cov * at(y * TILE + x));
        }
      }
    } else {
      paintStrokes(layer, li, buf, alpha, cover, wrap, palette, ramp);
    }
  });
}

/**
 * Paints material `name`: TILE × TILE RGBA bytes, row y = 0 (the tile's bottom) first. An opaque
 * material is opaque everywhere; a `"alpha": "strokes"` one is covered only by its strokes (its
 * fills and gradients colour the gaps, so filtering at an edge adds no fringe).
 */
export function paintMaterial(name, materials, palette) {
  const m = resolveMaterial(materials, name);
  const buf = new Float64Array(TILE * TILE * 3);
  const alpha = new Float64Array(TILE * TILE).fill(m.alpha === 'strokes' ? 0 : 1);
  paintLayers(materials, palette, name, buf, alpha, 0);
  const out = new Uint8Array(TILE * TILE * 4);
  for (let k = 0; k < TILE * TILE; k++) {
    for (let i = 0; i < 3; i++) out[k * 4 + i] = Math.round(Math.min(255, Math.max(0, buf[k * 3 + i])));
    out[k * 4 + 3] = Math.round(clamp01(alpha[k]) * 255);
  }
  return out;
}

/** A tile's rows flipped, top row first (for an image or a canvas). */
export function imageRows(tile) {
  const out = new Uint8Array(tile.length);
  const row = TILE * 4;
  for (let y = 0; y < TILE; y++) out.set(tile.subarray(y * row, (y + 1) * row), (TILE - 1 - y) * row);
  return out;
}

/** The ramp of a material as sRGB hex strings (for flat-shaded parts, e.g. the characters). */
export function materialRamp(name, materials, palette) {
  const m = resolveMaterial(materials, name);
  return { ...palette.ramps[m.ramp], overlay: m.overlay ?? 0 };
}
