// Tests of the cel texture style and its data (docs/PAINTERLY_TEXTURES.md §10):
// `node --test shared/textures/cel.test.mjs`.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { loadPixelArt } from '../look/pixelart.mjs';
import { CEL_KINDS, STYLES, celRecipe, celTones, paintCel, upscaleCel, validateCel } from './cel.mjs';
import { bakeCel, celRustSource } from './gen.mjs';
import { TILE, paintMaterial, resolveColor } from './paint.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const readJson = (p) => JSON.parse(readFileSync(p, 'utf8'));
const palette = readJson(join(here, 'palette.json'));
const materials = readJson(join(here, 'materials.json'));
const cel = readJson(join(here, 'cel.json'));
const pixel = loadPixelArt().palettes;
const clone = (v) => structuredClone(v);
const N = cel.tileTexels;
const at = (px, x, y, n = N) => [...px.subarray((y * n + x) * 4, (y * n + x) * 4 + 4)];
const rgb = (ref, ramp) => resolveColor(palette, ref, ramp);

test('the checked-in cel data is valid and its baked tiles are current', () => {
  const { names, n, texels } = bakeCel(cel, materials, palette, pixel);
  assert.deepEqual(names, Object.keys(materials.tiles), 'the painterly tiles, in order');
  const baked = readFileSync(join(here, '..', '..', 'crates', 'dwell-mesh', 'src', 'cel.rgba'));
  assert.ok(Buffer.from(texels).equals(baked), 'cel.rgba is stale: run node shared/textures/gen.mjs');
  const src = readFileSync(join(here, '..', '..', 'crates', 'dwell-mesh', 'src', 'cel.gen.rs'), 'utf8');
  assert.equal(src, celRustSource(names, n, cel.mips));
  assert.match(src, new RegExp(`CEL_MODE_MIPS: bool = ${cel.mips === 'mode'};`));
  assert.deepEqual(STYLES, ['painterly', 'cel']);
});

test('the cel tiles are low resolution: tileTexels divides the atlas tile', () => {
  assert.ok(N <= 16 && TILE % N === 0, `${N}`);
  for (const bad of [0, 3, 12, 64, 7.5]) {
    const c = clone(cel);
    c.tileTexels = bad;
    assert.throws(() => validateCel(c, materials, palette), /tileTexels must divide the atlas's tile/);
  }
});

test('every cel tile uses at most maxTones colours (data-driven)', () => {
  for (const [tile, mat] of Object.entries(materials.tiles)) {
    const tones = celTones(paintCel(mat, cel, materials, palette));
    assert.ok(tones.length >= 1 && tones.length <= cel.maxTones, `${tile}: ${tones.length} tones`);
  }
  // Every material (characters too) stays under it.
  for (const name of Object.keys(materials.materials)) {
    assert.ok(celTones(paintCel(name, cel, materials, palette)).length <= cel.maxTones, name);
  }
  // Lowering the limit by data fails the bake, naming the tile.
  const c = clone(cel);
  c.maxTones = 2;
  assert.throws(() => validateCel(c, materials, palette), /tile \w+ \(\w+\) has \d tones, more than maxTones 2/);
});

test("the cel tiles' tones are colours of the pixel-art palette, so quantizing changes nothing", () => {
  const allowed = new Set(pixel[cel.palette].flat().map((c) => c.join(',')));
  for (const [tile, mat] of Object.entries(materials.tiles)) {
    if (cel.paletteExempt.includes(tile)) continue;
    for (const t of celTones(paintCel(mat, cel, materials, palette))) {
      assert.ok(allowed.has(t.join(',')), `${tile}: ${t} is not a "${cel.palette}" colour`);
    }
  }
  // A tone off the palette fails the bake.
  const c = clone(cel);
  c.recipes.stone.layers[0].tone = 'wool_red.base';
  assert.throws(() => bakeCel(c, materials, palette, pixel), /tile stone \(stone\): tone #b9473f is not a colour of the pixel-art palette "dwell"/);
  c.palette = 'nope';
  assert.throws(() => bakeCel(c, materials, palette, pixel), /palette "nope" is not a pixel-art palette/);
});

test("the cel tiles average near the painterly tiles' (the LOD's flat colours stay right)", () => {
  // The LOD and the hotbar read the painterly averages in both styles (textures.rs): each cel tile
  // keeps its material's look from afar, within 16 of 255 per channel.
  const lin = (b) => ((b /= 255) <= 0.04045 ? b / 12.92 : ((b + 0.055) / 1.055) ** 2.4);
  const avg = (px) => {
    const s = [0, 0, 0];
    let n = 0;
    for (let i = 0; i < px.length; i += 4) {
      const a = px[i + 3] / 255;
      for (let k = 0; k < 3; k++) s[k] += lin(px[i + k]) * a;
      n += a;
    }
    return s.map((v) => v / n).map((v) => 255 * (v <= 0.0031308 ? v * 12.92 : 1.055 * v ** (1 / 2.4) - 0.055));
  };
  for (const [tile, mat] of Object.entries(materials.tiles)) {
    const a = avg(paintMaterial(mat, materials, palette));
    const b = avg(paintCel(mat, cel, materials, palette));
    const d = Math.max(...a.map((v, i) => Math.abs(v - b[i])));
    assert.ok(d <= 16, `${tile}: ${a.map(Math.round)} vs ${b.map(Math.round)}`);
  }
});

test('cutouts stay cutouts (hard-edged), opaque tiles stay opaque, and gaps keep the base colour', () => {
  for (const [tile, mat] of Object.entries(materials.tiles)) {
    const px = paintCel(mat, cel, materials, palette);
    const cutout = materials.materials[mat]?.alpha === 'strokes';
    const alphas = new Set();
    for (let i = 3; i < px.length; i += 4) alphas.add(px[i]);
    assert.ok([...alphas].every((a) => a === 0 || a === 255), `${tile}: hard edges`);
    assert.equal(alphas.has(0), cutout, `${tile}: cutout ${cutout}`);
  }
  const leaves = paintCel('leaves', cel, materials, palette);
  const gap = [...Array(N * N).keys()].find((k) => leaves[k * 4 + 3] === 0);
  assert.deepEqual(at(leaves, gap % N, Math.floor(gap / N)).slice(0, 3), rgb('leaf.base'));
  // An opaque recipe that leaves a gap fails.
  const c = clone(cel);
  c.recipes.stone.layers[0].tone = 'clear';
  assert.throws(() => validateCel(c, materials, palette), /tile stone \(stone\): an opaque material must not paint "clear"/);
});

test('a material added by data alone gets a cel tile: its own recipe, or the default for its use', () => {
  const m = clone(materials);
  m.materials.test_moss = { use: 'block', ramp: 'leaf', layers: [{ kind: 'fill', color: '@base' }] };
  m.tiles.testMoss = 'test_moss';
  // No recipe: the block default, from its ramp.
  assert.equal(celRecipe(cel, m, 'test_moss').from, 'defaults.block');
  const plain = paintCel('test_moss', cel, m, palette);
  assert.ok(celTones(plain).length >= 2 && celTones(plain).length <= cel.maxTones);
  assert.deepEqual(at(plain, 0, N - 1).slice(0, 3), rgb('leaf.light'), 'the lit top row');
  // Its own recipe, by data: bricks of shadow mortar on the base.
  const c = clone(cel);
  c.recipes.test_moss = {
    layers: [
      { kind: 'fill', tone: '@base' },
      { kind: 'bricks', mortar: '@shadow', width: [4, 4], height: [4, 4] },
    ],
  };
  const { names, n, texels } = bakeCel(c, m, palette, pixel);
  const i = names.indexOf('testMoss');
  const tile = texels.subarray(i * n * n * 4, (i + 1) * n * n * 4);
  const tones = celTones(tile).map(String);
  assert.deepEqual(tones.sort(), [rgb('leaf.base'), rgb('leaf.shadow')].map(String).sort());
  // Each row of bricks has a mortar row along its bottom.
  for (let x = 0; x < n; x++) assert.deepEqual(at(tile, x, 0, n).slice(0, 3), rgb('leaf.shadow'));
  // A template takes its recipe from the material it is like (dirt_skirt from grass_side).
  assert.equal(celRecipe(cel, materials, 'dirt_skirt').from, 'recipes.grass_side');
  assert.equal(celRecipe(cel, materials, 'cloth_linen').from, 'defaults.character');
});

test('every pattern kind paints, deterministically and seamlessly when the tile wraps', () => {
  const m = clone(materials);
  const fixture = {
    fill: { kind: 'fill', tone: '@base' },
    edge: { kind: 'edge', tone: '@light', edge: 'top', width: 2, ragged: 1 },
    clumps: { kind: 'clumps', tone: '@shadow', count: 3, width: [3, 4], height: [3, 3], shape: 'blob', light: '@light' },
    blades: { kind: 'blades', tone: '@shadow', tip: '@light', count: 3, length: [3, 5] },
    bricks: { kind: 'bricks', mortar: '@shadow', light: '@light', width: [5, 7], height: [4, 5] },
    planks: { kind: 'planks', seam: '@shadow', light: '@light', direction: 'horizontal', width: 4 },
    bands: { kind: 'bands', tones: ['@base', '@shadow'], direction: 'vertical', width: [2, 3] },
    ripples: { kind: 'ripples', tone: '@light', under: '@shadow', count: 3, length: [3, 4] },
    rings: { kind: 'rings', tones: ['@light', '@shadow'], center: [8, 8], step: 2 },
    rects: { kind: 'rects', tone: '@shadow', at: [[0, 0, 2, 2]] },
    checker: { kind: 'checker', tone: '@shadow', size: 4 },
  };
  assert.deepEqual(Object.keys(fixture), CEL_KINDS);
  for (const [kind, layer] of Object.entries(fixture)) {
    m.materials.test_kind = { use: 'block', ramp: 'stone', layers: [{ kind: 'fill', color: '@base' }] };
    const c = clone(cel);
    c.recipes.test_kind = { layers: kind === 'fill' ? [layer] : [fixture.fill, layer] };
    validateCel(c, m, palette);
    const a = paintCel('test_kind', c, m, palette);
    assert.deepEqual(a, paintCel('test_kind', c, m, palette), `${kind}: deterministic`);
    if (kind !== 'fill') assert.ok(celTones(a).length >= 2, `${kind} paints something`);
  }
  // A shape over the edge continues on the far side (wrap), or stops there (a sprite): a blade
  // three texels wide rooted in the last column.
  m.materials.test_kind = { use: 'block', ramp: 'stone', layers: [{ kind: 'fill', color: '@base' }] };
  const c = clone(cel);
  c.recipes.test_kind = { layers: [fixture.fill, { kind: 'blades', tone: 'ink', at: [N - 1], width: 3, length: [4, 4] }] };
  const ink = [...rgb('ink'), 255];
  let px = paintCel('test_kind', c, m, palette);
  for (const x of [N - 1, 0, 1]) assert.deepEqual(at(px, x, 2), ink, `wraps to column ${x}`);
  m.materials.test_kind.wrap = false;
  px = paintCel('test_kind', c, m, palette);
  assert.deepEqual(at(px, N - 1, 2), ink);
  assert.deepEqual(at(px, 0, 2), [...rgb('stone.base'), 255], 'a sprite stops at its edge');
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(c) => (c.bloom = 1), /cel\.json: unknown entry bloom/],
    [(c) => (c.maxTones = 0), /maxTones must be a whole number/],
    [(c) => (c.mips = 'linear'), /mips must be one of average, mode/],
    [(c) => (c.paletteExempt = ['nope']), /paletteExempt: "nope" is not an atlas tile/],
    [(c) => (c.recipes.nope = { layers: [] }), /recipes\.nope: no material "nope"/],
    [(c) => (c.recipes.stone.layers[1].kind = 'swirl'), /recipes\.stone\.layers\[1\] \(painting stone\)\.kind must be one of fill, edge/],
    [(c) => (c.recipes.stone.layers[1].angle = 3), /recipes\.stone\.layers\[1\] \(painting stone\): unknown entry "angle" for a bricks layer/],
    [(c) => (c.recipes.stone.layers[1].mortar = 'nope'), /\.mortar: unknown palette colour "nope"/],
    [(c) => (c.recipes.stone.layers[1].width = [9, 4]), /\.width must be \[min, max\], whole texels/],
    [(c) => (c.recipes.stone.layers[1].height = 1.5), /\.height must be a whole number/],
    [(c) => (c.recipes.sand.layers[1].band = [0.9, 0.1]), /\.band must be \[bottom, top\]/],
    [(c) => (c.recipes.ladder_wood.layers[1].at[0] = [15, 0, 3, 16]), /\.at\[0\] must be \[x, y, width, height\] in whole texels inside the 16 × 16 tile/],
    [(c) => (c.recipes.tall_grass.layers[1].count = 3), /give exactly one of "count" \(spread\) or "at" \(columns\)/],
    [(c) => (c.recipes.grass_top = { like: 'grass_top' }), /"like" forms a cycle/],
    [(c) => (c.recipes.grass_top = { like: 'nope' }), /unknown recipe "nope"/],
    [(c) => delete c.defaults.character, /material "cloth_canvas" has no recipe and there is no default for its use "character"/],
    [(c) => (c.defaults.water = { layers: [] }), /defaults\.water: a default is per use/],
  ];
  for (const [edit, message] of cases) {
    const c = clone(cel);
    edit(c);
    assert.throws(() => validateCel(c, materials, palette), message);
  }
});

test('the atlas tile is the cel tile repeated, nearest', () => {
  const px = paintCel('stone', cel, materials, palette);
  const big = upscaleCel(px, N);
  const k = TILE / N;
  for (const [x, y] of [[0, 0], [5, 9], [TILE - 1, TILE - 1]]) {
    assert.deepEqual(at(big, x, y, TILE), at(px, Math.floor(x / k), Math.floor(y / k)));
  }
});
