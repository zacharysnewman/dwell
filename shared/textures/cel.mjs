// The cel texture style (docs/PAINTERLY_TEXTURES.md §10, a prototype for the pixel-art preview,
// docs/proposals/PIXEL_ART_LOOK.md): the same materials as the painterly set
// (shared/textures/materials.json), painted from shared/textures/cel.json as a few flat tones on a
// low-resolution grid (`tileTexels` texels per tile edge) with hard edges — blocky clumps, bricks,
// plank and grain bands, ripples, blades, rings — and no gradients or soft strokes, so a face is a
// few large flat areas that hold still as the camera moves. Pure and dependency-free, like paint.mjs.
//
// Coordinates are cel texels with y up: texel (x, y), y = 0 the tile's bottom row; tiles come out in
// that order (row y = 0 first). A tone is a palette reference (paint.mjs `resolveColor`: "ramp.stop",
// "@stop" for the material's own ramp, a swatch) or "clear" (transparent: a cutout's gap). The layer
// kinds below are the engine and are fixed; every recipe is data.
import { TILE, hash, resolveColor, resolveMaterial } from './paint.mjs';

/** The texture styles a build bakes (an atlas each): the painterly set and this one. */
export const STYLES = ['painterly', 'cel'];

/** The cel layer kinds (the fixed vocabulary). */
export const CEL_KINDS = ['fill', 'edge', 'clumps', 'blades', 'bricks', 'planks', 'bands', 'ripples', 'rings', 'rects', 'checker'];

const ROOT_KEYS = ['$comment', 'tileTexels', 'maxTones', 'palette', 'paletteExempt', 'defaults', 'recipes'];
const RECIPE_KEYS = ['$comment', 'like', 'layers'];
const COMMON = ['$comment', 'kind', 'clip'];
const KEYS = {
  fill: ['tone'],
  edge: ['tone', 'edge', 'width', 'ragged', 'seed'],
  clumps: ['tone', 'count', 'width', 'height', 'band', 'shape', 'light', 'shadow', 'seed'],
  blades: ['tone', 'tip', 'count', 'at', 'length', 'width', 'root', 'dir', 'seed'],
  bricks: ['mortar', 'width', 'height', 'light', 'shadow', 'seed'],
  planks: ['seam', 'direction', 'width', 'light', 'joints', 'seed'],
  bands: ['tones', 'direction', 'width', 'seed'],
  ripples: ['tone', 'under', 'count', 'length', 'band', 'seed'],
  rings: ['tones', 'center', 'step', 'metric', 'radius'],
  rects: ['tone', 'at'],
  checker: ['tone', 'size'],
};
const EDGES = ['top', 'bottom', 'left', 'right', 'all'];
const DIRECTIONS = ['horizontal', 'vertical'];

const isInt = (v) => Number.isInteger(v);

/** The tone of `ref`: "clear" (null), or its sRGB bytes. */
function toneColor(palette, ref, ramp, where) {
  if (ref === 'clear') return null;
  return resolveColor(palette, ref, ramp, where);
}

/** A recipe's layers, with `like` (another recipe's layers) resolved. */
function recipeLayers(cel, name, seen = []) {
  const r = cel.recipes?.[name];
  if (!r) throw new Error(`cel.json: unknown recipe "${name}"${seen.length ? ` (from ${seen.at(-1)})` : ''}`);
  if (seen.includes(name)) throw new Error(`cel.json: recipes.${name}: "like" forms a cycle (${[...seen, name].join(' → ')})`);
  if (r.like !== undefined) return recipeLayers(cel, r.like, [...seen, name]);
  return r.layers;
}

/**
 * The layers that paint material `name` in the cel style, and where they come from: its own recipe
 * (`recipes.<name>`), else the recipe of the material it is `like` (its template), else the
 * default for its use (`defaults.<use>`), so a material added by data gets a cel tile at once.
 */
export function celRecipe(cel, materials, name) {
  for (let n = name; n !== undefined; n = materials.materials[n]?.like) {
    if (cel.recipes?.[n]) return { from: `recipes.${n}`, layers: recipeLayers(cel, n) };
  }
  const use = resolveMaterial(materials, name).use;
  const d = cel.defaults?.[use];
  if (!d) throw new Error(`cel.json: material "${name}" has no recipe and there is no default for its use "${use}"`);
  return { from: `defaults.${use}`, layers: d.layers };
}

function range(where, v, lo, hi, { pair = false } = {}) {
  const ok = (n) => isInt(n) && n >= lo && n <= hi;
  if (pair && Array.isArray(v)) {
    if (v.length !== 2 || !ok(v[0]) || !ok(v[1]) || v[0] > v[1]) {
      throw new Error(`${where} must be [min, max], whole texels in [${lo}, ${hi}], got ${JSON.stringify(v)}`);
    }
    return;
  }
  if (!ok(v)) throw new Error(`${where} must be a whole number${pair ? ' (or [min, max])' : ''} in [${lo}, ${hi}], got ${JSON.stringify(v)}`);
}

function share(where, v) {
  if (!Array.isArray(v) || v.length !== 2 || !v.every((n) => typeof n === 'number' && n >= 0 && n <= 1) || v[0] > v[1]) {
    throw new Error(`${where} must be [bottom, top], shares of the tile's height in [0, 1], got ${JSON.stringify(v)}`);
  }
}

function validateLayer(where, l, n, palette, ramp) {
  if (typeof l !== 'object' || l === null) throw new Error(`${where} must be an object`);
  if (!CEL_KINDS.includes(l.kind)) throw new Error(`${where}.kind must be one of ${CEL_KINDS.join(', ')}, got ${JSON.stringify(l.kind)}`);
  for (const k of Object.keys(l)) {
    if (!COMMON.includes(k) && !KEYS[l.kind].includes(k)) throw new Error(`${where}: unknown entry "${k}" for a ${l.kind} layer`);
  }
  const tone = (k, required = true) => {
    if (l[k] === undefined) {
      if (required) throw new Error(`${where}.${k} is missing (a palette colour or "clear")`);
      return;
    }
    toneColor(palette, l[k], ramp, `${where}.${k}`);
  };
  const tones = (k) => {
    if (!Array.isArray(l[k]) || l[k].length === 0) throw new Error(`${where}.${k} must be a non-empty list of palette colours`);
    l[k].forEach((t, i) => toneColor(palette, t, ramp, `${where}.${k}[${i}]`));
  };
  if (l.clip !== undefined && typeof l.clip !== 'boolean') throw new Error(`${where}.clip must be true or false`);
  if (l.seed !== undefined) range(`${where}.seed`, l.seed, 0, 1e6);
  switch (l.kind) {
    case 'fill':
      tone('tone');
      break;
    case 'edge':
      tone('tone');
      if (!EDGES.includes(l.edge)) throw new Error(`${where}.edge must be one of ${EDGES.join(', ')}`);
      range(`${where}.width`, l.width, 1, n);
      if (l.ragged !== undefined) range(`${where}.ragged`, l.ragged, 0, n);
      break;
    case 'clumps':
      tone('tone');
      tone('light', false);
      tone('shadow', false);
      range(`${where}.count`, l.count, 1, 256);
      range(`${where}.width`, l.width, 1, n, { pair: true });
      range(`${where}.height`, l.height, 1, n, { pair: true });
      if (l.band !== undefined) share(`${where}.band`, l.band);
      if (l.shape !== undefined && !['rect', 'blob'].includes(l.shape)) throw new Error(`${where}.shape must be "rect" or "blob"`);
      break;
    case 'blades':
      tone('tone');
      tone('tip', false);
      if ((l.count === undefined) === (l.at === undefined)) throw new Error(`${where}: give exactly one of "count" (spread) or "at" (columns)`);
      if (l.count !== undefined) range(`${where}.count`, l.count, 1, 256);
      if (l.at !== undefined) {
        if (!Array.isArray(l.at) || l.at.length === 0) throw new Error(`${where}.at must be a list of columns`);
        l.at.forEach((x, i) => range(`${where}.at[${i}]`, x, 0, n - 1));
      }
      range(`${where}.length`, l.length, 1, n, { pair: true });
      if (l.width !== undefined) range(`${where}.width`, l.width, 1, n);
      if (l.root !== undefined && !(typeof l.root === 'number' && l.root >= 0 && l.root <= 1)) {
        throw new Error(`${where}.root must be a share of the tile's height in [0, 1]`);
      }
      if (l.dir !== undefined && !['up', 'down'].includes(l.dir)) throw new Error(`${where}.dir must be "up" or "down"`);
      break;
    case 'bricks':
      tone('mortar');
      tone('light', false);
      tone('shadow', false);
      range(`${where}.width`, l.width, 2, n, { pair: true });
      range(`${where}.height`, l.height, 2, n, { pair: true });
      break;
    case 'planks':
      tone('seam');
      tone('light', false);
      if (!DIRECTIONS.includes(l.direction)) throw new Error(`${where}.direction must be one of ${DIRECTIONS.join(', ')}`);
      range(`${where}.width`, l.width, 2, n);
      if (l.joints !== undefined && typeof l.joints !== 'boolean') throw new Error(`${where}.joints must be true or false`);
      break;
    case 'bands':
      tones('tones');
      if (!DIRECTIONS.includes(l.direction)) throw new Error(`${where}.direction must be one of ${DIRECTIONS.join(', ')}`);
      range(`${where}.width`, l.width, 1, n, { pair: true });
      break;
    case 'ripples':
      tone('tone');
      tone('under', false);
      range(`${where}.count`, l.count, 1, 256);
      range(`${where}.length`, l.length, 1, n, { pair: true });
      if (l.band !== undefined) share(`${where}.band`, l.band);
      break;
    case 'rings':
      tones('tones');
      if (!Array.isArray(l.center) || l.center.length !== 2 || !l.center.every((v) => typeof v === 'number')) {
        throw new Error(`${where}.center must be [x, y] in texels`);
      }
      range(`${where}.step`, l.step, 1, n);
      if (l.metric !== undefined && !['round', 'square'].includes(l.metric)) throw new Error(`${where}.metric must be "round" or "square"`);
      if (l.radius !== undefined) range(`${where}.radius`, l.radius, 1, n);
      break;
    case 'rects':
      tone('tone');
      if (!Array.isArray(l.at) || l.at.length === 0) throw new Error(`${where}.at must be a list of [x, y, width, height]`);
      l.at.forEach((r, i) => {
        if (!Array.isArray(r) || r.length !== 4 || !r.every(isInt) || r[0] < 0 || r[1] < 0 || r[2] < 1 || r[3] < 1 || r[0] + r[2] > n || r[1] + r[3] > n) {
          throw new Error(`${where}.at[${i}] must be [x, y, width, height] in whole texels inside the ${n} × ${n} tile, got ${JSON.stringify(r)}`);
        }
      });
      break;
    case 'checker':
      tone('tone');
      range(`${where}.size`, l.size, 1, n);
      break;
  }
}

/**
 * Checks the cel style's data against the materials and the palette: the grid, every recipe and
 * default, and that every atlas tile's material (`materials.tiles`) has a recipe. With
 * `pixelPalette` (a list of sRGB byte triples: the pixel-art palette it names), the painted tiles'
 * tones must be colours of it. Throws naming the bad entry.
 */
export function validateCel(cel, materials, palette, pixelPalette = null) {
  const fail = (m) => {
    throw new Error(`cel.json: ${m}`);
  };
  if (typeof cel !== 'object' || cel === null) fail('must be an object');
  for (const k of Object.keys(cel)) if (!ROOT_KEYS.includes(k)) fail(`unknown entry ${k}`);
  const n = cel.tileTexels;
  if (!isInt(n) || n < 4 || n > TILE || TILE % n !== 0) fail(`tileTexels must divide the atlas's tile (${TILE}) and be at least 4, got ${JSON.stringify(n)}`);
  range('cel.json: maxTones', cel.maxTones, 1, 16);
  if (typeof cel.palette !== 'string') fail('palette must name a pixel-art palette (shared/look/pixelart.json)');
  if (!Array.isArray(cel.paletteExempt)) fail('paletteExempt must be a list of atlas tiles');
  for (const t of cel.paletteExempt) if (!(t in (materials.tiles ?? {}))) fail(`paletteExempt: "${t}" is not an atlas tile`);
  for (const [section, entries] of [['defaults', cel.defaults ?? {}], ['recipes', cel.recipes ?? {}]]) {
    for (const [name, r] of Object.entries(entries)) {
      const where = `${section}.${name}`;
      if (typeof r !== 'object' || r === null) fail(`${where} must be an object`);
      for (const k of Object.keys(r)) if (!RECIPE_KEYS.includes(k)) fail(`${where}: unknown entry "${k}"`);
      if (section === 'defaults' && !['block', 'character', 'sky'].includes(name)) fail(`${where}: a default is per use (block, character, sky)`);
      if (section === 'recipes' && !materials.materials[name]) fail(`${where}: no material "${name}" in materials.json`);
      if (r.like !== undefined) {
        if (section !== 'recipes') fail(`${where}: "like" is for recipes`);
        recipeLayers(cel, name);
        continue;
      }
      if (!Array.isArray(r.layers) || r.layers.length === 0) fail(`${where}.layers must be a non-empty list`);
    }
  }
  // Each layer is checked against the ramp of every material it paints.
  for (const mat of Object.keys(materials.materials)) {
    const { from, layers } = celRecipe(cel, materials, mat);
    const ramp = resolveMaterial(materials, mat).ramp;
    layers.forEach((l, i) => validateLayer(`cel.json: ${from}.layers[${i}] (painting ${mat})`, l, n, palette, ramp));
  }
  // The painted tiles: few tones, cutouts as their materials say, and on the palette.
  const allowed = pixelPalette ? new Set(pixelPalette.map((c) => c.join(','))) : null;
  for (const [tile, mat] of Object.entries(materials.tiles ?? {})) {
    const px = paintCel(mat, cel, materials, palette);
    const tones = celTones(px);
    if (tones.length > cel.maxTones) fail(`tile ${tile} (${mat}) has ${tones.length} tones, more than maxTones ${cel.maxTones}`);
    const cutout = resolveMaterial(materials, mat).alpha === 'strokes';
    const clear = px.some((_, i) => i % 4 === 3 && px[i] === 0);
    if (cutout !== clear) {
      fail(`tile ${tile} (${mat}): ${cutout ? 'a cutout material must leave some texels "clear"' : 'an opaque material must not paint "clear"'}`);
    }
    if (allowed && !cel.paletteExempt.includes(tile)) {
      for (const t of tones) {
        if (!allowed.has(t.join(','))) {
          fail(`tile ${tile} (${mat}): tone ${hex(t)} is not a colour of the pixel-art palette "${cel.palette}"`);
        }
      }
    }
  }
}

const hex = (c) => `#${c.map((v) => v.toString(16).padStart(2, '0')).join('')}`;

/** The distinct colours of a painted tile's covered texels (sRGB byte triples). */
export function celTones(px) {
  const seen = new Map();
  for (let i = 0; i < px.length; i += 4) {
    if (px[i + 3] === 0) continue;
    const c = [px[i], px[i + 1], px[i + 2]];
    seen.set(c.join(','), c);
  }
  return [...seen.values()];
}

/** Picks a whole number in [min, max] (or the number itself) by hash h in [0, 1). */
const pickInt = (v, h) => (Array.isArray(v) ? v[0] + Math.min(v[1] - v[0], Math.floor(h * (v[1] - v[0] + 1))) : v);

/** Whole-texel runs that sum to exactly n (so a wrapping tile repeats): lengths picked from `w`. */
function runs(n, w, seed, salt) {
  const out = [];
  let sum = 0;
  for (let i = 0; sum < n && i < 4 * n; i++) {
    const len = pickInt(w, hash(i, salt, 41, seed));
    out.push(len);
    sum += len;
  }
  // Trim the overshoot off the last run; one too short for the range joins the one before.
  out[out.length - 1] -= sum - n;
  const min = Array.isArray(w) ? w[0] : w;
  if (out.length > 1 && out[out.length - 1] < min) out[out.length - 2] += out.pop();
  return out;
}

/** Jittered grid placements of `count` items inside `band` (shares of the height): [x, y] texels. */
function spread(n, count, band, seed) {
  const [b0, b1] = band ?? [0, 1];
  const h = Math.max((b1 - b0) * n, 1);
  const cols = Math.min(count, Math.max(1, Math.round(Math.sqrt((count * n) / h))));
  const rows = Math.ceil(count / cols);
  const out = [];
  for (let i = 0; i < count; i++) {
    const cx = i % cols;
    const cy = Math.floor(i / cols);
    const shift = cy % 2 ? 0.5 : 0;
    const x = Math.floor(((cx + shift + hash(i, 0, 1, seed)) * n) / cols);
    const y = Math.floor(b0 * n + ((cy + hash(i, 0, 2, seed)) * h) / rows);
    out.push([x, Math.min(y, n - 1)]);
  }
  return out;
}

/**
 * Paints material `name` in the cel style: `tileTexels`² RGBA bytes, row y = 0 (the bottom)
 * first. Every covered texel is exactly one of its tones; a "clear" texel is transparent and keeps
 * the material's base colour (so filtering at a cutout's edge adds no fringe).
 */
export function paintCel(name, cel, materials, palette) {
  const n = cel.tileTexels;
  const m = resolveMaterial(materials, name);
  const ramp = m.ramp;
  const wrap = m.wrap ?? true;
  const { layers } = celRecipe(cel, materials, name);
  // The tone of each texel: an index into `colors` (−1: clear, `undefined`: never painted).
  const colors = [];
  const index = new Map();
  const toneOf = (ref) => {
    if (ref === 'clear') return -1;
    const c = resolveColor(palette, ref, ramp);
    const key = c.join(',');
    if (!index.has(key)) {
      index.set(key, colors.length);
      colors.push(c);
    }
    return index.get(key);
  };
  const grid = new Int16Array(n * n).fill(-1);
  layers.forEach((l, li) => {
    const seed = (l.seed ?? 0) * 7919 + li * 104729 + 23;
    const set = (x, y, t) => {
      if (wrap) {
        x = ((x % n) + n) % n;
        y = ((y % n) + n) % n;
      } else if (x < 0 || y < 0 || x >= n || y >= n) return;
      const k = y * n + x;
      if (l.clip && grid[k] < 0) return;
      grid[k] = t;
    };
    const tone = l.tone !== undefined ? toneOf(l.tone) : undefined;
    switch (l.kind) {
      case 'fill':
        for (let k = 0; k < n * n; k++) if (!l.clip || grid[k] >= 0) grid[k] = tone;
        break;
      case 'edge': {
        const ragged = l.ragged ?? 0;
        const depth = (i, side) => l.width + Math.floor(hash(i, side, 7, seed) * (ragged + 1));
        const sides = l.edge === 'all' ? ['top', 'bottom', 'left', 'right'] : [l.edge];
        sides.forEach((side, si) => {
          for (let i = 0; i < n; i++) {
            const d = depth(i, si);
            for (let j = 0; j < Math.min(d, n); j++) {
              if (side === 'top') set(i, n - 1 - j, tone);
              else if (side === 'bottom') set(i, j, tone);
              else if (side === 'left') set(j, i, tone);
              else set(n - 1 - j, i, tone);
            }
          }
        });
        break;
      }
      case 'clumps': {
        const light = l.light !== undefined ? toneOf(l.light) : undefined;
        const shadow = l.shadow !== undefined ? toneOf(l.shadow) : undefined;
        spread(n, l.count, l.band, seed).forEach(([cx, cy], i) => {
          const w = pickInt(l.width, hash(i, 1, 3, seed));
          const h = pickInt(l.height, hash(i, 1, 4, seed));
          const x0 = cx - Math.floor(w / 2);
          const y0 = cy - Math.floor(h / 2);
          const blob = l.shape === 'blob' && w >= 4 && h >= 3;
          for (let y = 0; y < h; y++) {
            for (let x = 0; x < w; x++) {
              const corner = (x === 0 || x === w - 1) && (y === 0 || y === h - 1);
              if (blob && corner) continue;
              let t = tone;
              if (light !== undefined && y === h - 1 && h >= 2) t = light;
              if (shadow !== undefined && y === 0 && h >= 2) t = shadow;
              set(x0 + x, y0 + y, t);
            }
          }
        });
        break;
      }
      case 'blades': {
        const tip = l.tip !== undefined ? toneOf(l.tip) : undefined;
        const width = l.width ?? 1;
        const up = (l.dir ?? 'up') === 'up';
        const root = Math.min(n - 1, Math.floor((l.root ?? (up ? 0 : 1)) * n - (up ? 0 : 1)));
        const cols = l.at ?? Array.from({ length: l.count }, (_, i) => Math.floor(((i + hash(i, 0, 5, seed)) * n) / l.count));
        cols.forEach((x0, i) => {
          const len = pickInt(l.length, hash(i, 1, 6, seed));
          for (let s = 0; s < len; s++) {
            const y = up ? root + s : root - s;
            for (let dx = 0; dx < width; dx++) set(x0 + dx, y, tip !== undefined && s === len - 1 ? tip : tone);
          }
        });
        break;
      }
      case 'bricks': {
        const mortar = toneOf(l.mortar);
        const light = l.light !== undefined ? toneOf(l.light) : undefined;
        const shadow = l.shadow !== undefined ? toneOf(l.shadow) : undefined;
        let y0 = 0;
        runs(n, l.height, seed, 0).forEach((h, row) => {
          const offset = Math.floor(hash(row, 2, 9, seed) * n);
          let x0 = offset;
          runs(n, l.width, seed, row + 1).forEach((w) => {
            for (let y = 0; y < h; y++) {
              for (let x = 0; x < w; x++) {
                let t;
                if (y === 0 || x === 0) t = mortar;
                else if (light !== undefined && y === h - 1) t = light;
                else if (shadow !== undefined && y === 1 && h >= 4) t = shadow;
                else continue;
                set(x0 + x, y0 + y, t);
              }
            }
            x0 += w;
          });
          y0 += h;
        });
        break;
      }
      case 'planks': {
        const seam = toneOf(l.seam);
        const light = l.light !== undefined ? toneOf(l.light) : undefined;
        const across = l.direction === 'horizontal';
        const at = (a, b, t) => (across ? set(b, a, t) : set(a, b, t));
        for (let p = 0; p * l.width < n; p++) {
          const a0 = p * l.width;
          for (let b = 0; b < n; b++) {
            at(a0, b, seam);
            if (light !== undefined && l.width > 2) at(a0 + 1, b, light);
          }
          if (l.joints ?? true) {
            const b = Math.floor(hash(p, 3, 11, seed) * n);
            for (let a = a0; a < Math.min(a0 + l.width, n); a++) at(a, b, seam);
          }
        }
        break;
      }
      case 'bands': {
        const tones = l.tones.map(toneOf);
        let a0 = 0;
        runs(n, l.width, seed, 7).forEach((w, i) => {
          const t = tones[i % tones.length];
          for (let a = a0; a < a0 + w; a++) {
            for (let b = 0; b < n; b++) (l.direction === 'vertical' ? set(a, b, t) : set(b, a, t));
          }
          a0 += w;
        });
        break;
      }
      case 'ripples': {
        const under = l.under !== undefined ? toneOf(l.under) : undefined;
        spread(n, l.count, l.band, seed).forEach(([cx, cy], i) => {
          const len = pickInt(l.length, hash(i, 1, 12, seed));
          const x0 = cx - Math.floor(len / 2);
          for (let x = 0; x < len; x++) {
            set(x0 + x, cy, tone);
            if (under !== undefined) set(x0 + x + 1, cy - 1, under);
          }
        });
        break;
      }
      case 'rings': {
        const tones = l.tones.map(toneOf);
        const [cx, cy] = l.center;
        for (let y = 0; y < n; y++) {
          for (let x = 0; x < n; x++) {
            const dx = x + 0.5 - cx;
            const dy = y + 0.5 - cy;
            const d = l.metric === 'square' ? Math.max(Math.abs(dx), Math.abs(dy)) : Math.hypot(dx, dy);
            if (l.radius !== undefined && d > l.radius) continue;
            set(x, y, tones[Math.floor(d / l.step) % tones.length]);
          }
        }
        break;
      }
      case 'rects':
        for (const [x0, y0, w, h] of l.at) for (let y = y0; y < y0 + h; y++) for (let x = x0; x < x0 + w; x++) set(x, y, tone);
        break;
      case 'checker':
        for (let y = 0; y < n; y++) for (let x = 0; x < n; x++) if ((Math.floor(x / l.size) + Math.floor(y / l.size)) % 2 === 0) set(x, y, tone);
        break;
    }
  });
  const base = resolveColor(palette, '@base', ramp);
  const out = new Uint8Array(n * n * 4);
  for (let k = 0; k < n * n; k++) {
    const t = grid[k];
    const c = t >= 0 ? colors[t] : base;
    out.set([c[0], c[1], c[2], t >= 0 ? 255 : 0], k * 4);
  }
  return out;
}

/** A cel tile scaled up to the atlas's tile (each texel repeated, nearest): TILE² RGBA bytes. */
export function upscaleCel(px, n) {
  const k = TILE / n;
  const out = new Uint8Array(TILE * TILE * 4);
  for (let y = 0; y < TILE; y++) {
    for (let x = 0; x < TILE; x++) {
      const s = (Math.floor(y / k) * n + Math.floor(x / k)) * 4;
      out.set(px.subarray(s, s + 4), (y * TILE + x) * 4);
    }
  }
  return out;
}
