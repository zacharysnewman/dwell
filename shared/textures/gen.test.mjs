// Tests of the painterly texture generator and its data (docs/PAINTERLY_TEXTURES.md, ADR 0040):
// `node --test shared/textures/gen.test.mjs`.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { ENGINE_TILES, atlasTiles, bake, labPage, rustSource, tileEmissive, tileHang } from './gen.mjs';
import { TILE, imageRows, materialRamp, paintMaterial, resolveColor, validateMaterials, validatePalette } from './paint.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const readJson = (p) => JSON.parse(readFileSync(p, 'utf8'));
const palette = readJson(join(here, 'palette.json'));
const materials = readJson(join(here, 'materials.json'));
const tiles = atlasTiles(readJson(join(here, '..', 'blocks', 'dwell.json')));
const clone = (v) => structuredClone(v);

const texel = (tile, x, y) => [...tile.subarray((y * TILE + x) * 4, (y * TILE + x) * 4 + 4)];

test('the checked-in data is valid and the baked tiles are current', () => {
  const { names, texels } = bake(materials, palette, tiles);
  assert.deepEqual(names, Object.keys(materials.tiles));
  const baked = readFileSync(join(here, '..', '..', 'crates', 'dwell-mesh', 'src', 'painterly.rgba'));
  assert.ok(Buffer.from(texels).equals(baked), 'painterly.rgba is stale: run node shared/textures/gen.mjs');
  const src = readFileSync(join(here, '..', '..', 'crates', 'dwell-mesh', 'src', 'painterly.gen.rs'), 'utf8');
  assert.equal(src, rustSource(names, tileEmissive(materials, names), tileHang(materials, names)));
});

test("a tile glows by its material's emissive, by data alone (docs/LIGHTS.md)", () => {
  const m = clone(materials);
  const names = Object.keys(m.tiles);
  assert.equal(tileEmissive(m, names)[names.indexOf('stone')], 0, 'stone gives off nothing');
  assert.ok(tileEmissive(m, names)[names.indexOf('torchTop')] >= 1, "the torch's flame is self-lit");
  m.materials.stone.emissive = 0.5;
  assert.equal(tileEmissive(m, names)[names.indexOf('stone')], 0.5);
  assert.match(rustSource(names, tileEmissive(m, names)), /PAINTERLY_EMISSIVE/);
  m.materials.stone.emissive = 9;
  assert.throws(() => validateMaterials(m, palette), /emissive must be/);
});

test("a side tile hangs from its block's top by its material's hang, by data alone", () => {
  const m = clone(materials);
  const names = Object.keys(m.tiles);
  assert.equal(tileHang(m, names)[names.indexOf('grassSide')], true, 'the turf skirt hangs');
  assert.equal(tileHang(m, names)[names.indexOf('dirtSkirt')], true, 'like grass_side');
  assert.equal(tileHang(m, names)[names.indexOf('stone')], false, 'stone runs in world metres');
  m.materials.stone.hang = true;
  assert.equal(tileHang(m, names)[names.indexOf('stone')], true);
  assert.match(rustSource(names, tileEmissive(m, names), tileHang(m, names)), /PAINTERLY_HANG: \[bool; \d+\] = \[[^\]]*true/);
  m.materials.stone.hang = 'yes';
  assert.throws(() => validateMaterials(m, palette), /hang must be true or false/);
});

test('the editor page embeds the current painter and data, and its painter paints the same', () => {
  const lab = join(here, '..', '..', 'docs', 'prototypes', 'painterly-texture-lab.html');
  const html = readFileSync(lab, 'utf8');
  const paintSource = readFileSync(join(here, 'paint.mjs'), 'utf8');
  assert.equal(html, labPage(html, paintSource, palette, materials), 'stale: run node shared/textures/gen.mjs');
  // The embedded block runs on its own and paints what the module paints.
  const block = html.slice(html.indexOf('const PAINT = '), html.indexOf('// END GENERATED'));
  const page = new Function(`${block}; return { PAINT, REPO_PALETTE, REPO_MATERIALS };`)();
  assert.deepEqual(
    page.PAINT.paintMaterial('grass_side', page.REPO_MATERIALS, page.REPO_PALETTE),
    paintMaterial('grass_side', materials, palette),
  );
});

test('painting is deterministic', () => {
  for (const name of Object.keys(materials.materials)) {
    assert.deepEqual(paintMaterial(name, materials, palette), paintMaterial(name, materials, palette), name);
  }
});

test('a material added by data alone is painted, mapped to a tile and read by name', () => {
  const m = clone(materials);
  m.materials.test_moss = {
    use: 'block',
    ramp: 'leaf',
    layers: [
      { kind: 'fill', color: '@shadow' },
      { kind: 'dab', at: [[16, 16]], colors: ['ink'], width: 6, length: 6 },
    ],
  };
  m.tiles.testMoss = 'test_moss';
  const { names, texels } = bake(m, palette, [...tiles, 'testMoss']);
  const i = names.indexOf('testMoss');
  assert.ok(i >= 0);
  const tile = texels.subarray(i * TILE * TILE * 4, (i + 1) * TILE * TILE * 4);
  assert.deepEqual(texel(tile, 0, 0), [...resolveColor(palette, 'leaf.shadow'), 255], 'the fill');
  assert.deepEqual(texel(tile, 16, 16), [...resolveColor(palette, 'ink'), 255], 'the dab at its centre');
  assert.deepEqual(materialRamp('test_moss', m, palette), { ...palette.ramps.leaf, overlay: 0 });
});

test('a template ("like") changes the ramp and keeps the strokes', () => {
  const red = paintMaterial('cloth_wool_red', materials, palette);
  const blue = paintMaterial('cloth_wool_blue', materials, palette);
  assert.notDeepEqual(red, blue);
  // Same strokes: where one is its base colour, so is the other.
  const base = (ramp) => [...resolveColor(palette, `${ramp}.base`), 255];
  let both = 0;
  for (let k = 0; k < TILE * TILE; k++) {
    const r = [...red.subarray(k * 4, k * 4 + 4)];
    const b = [...blue.subarray(k * 4, k * 4 + 4)];
    if (String(r) === String(base('wool_red'))) {
      assert.deepEqual(b, base('wool_blue'));
      both++;
    }
  }
  assert.ok(both > 0);
});

test('wrapping tiles are seamless: a stroke over an edge continues on the far side', () => {
  const m = clone(materials);
  m.materials.edge = {
    use: 'block',
    ramp: 'stone',
    layers: [
      { kind: 'fill', color: '@base' },
      { kind: 'slab', at: [[0, 16]], colors: ['ink'], width: 4, length: 4 },
    ],
  };
  const tile = paintMaterial('edge', m, palette);
  assert.deepEqual(texel(tile, 0, 16), texel(tile, TILE - 1, 16));
  assert.deepEqual(texel(tile, 0, 16).slice(0, 3), resolveColor(palette, 'ink'));
  m.materials.edge.wrap = false;
  assert.deepEqual(texel(paintMaterial('edge', m, palette), TILE - 1, 16).slice(0, 3), resolveColor(palette, 'stone.base'));
});

test('"strokes" alpha covers only the strokes; clip paints inside them', () => {
  const tile = paintMaterial('tall_grass', materials, palette);
  for (let y = 0; y < TILE; y++) {
    assert.equal(texel(tile, 0, y)[3], 0);
    assert.equal(texel(tile, TILE - 1, y)[3], 0);
  }
  for (let x = 0; x < TILE; x++) assert.equal(texel(tile, x, TILE - 1)[3], 0, 'nothing on the top row');
  assert.equal(texel(tile, 16, 1)[3], 255, "blades at the foot");
  // imageRows puts the top first.
  assert.deepEqual(texel(imageRows(tile), 16, TILE - 2), texel(tile, 16, 1));
});

test('bad entries fail with a message naming them', () => {
  const cases = [
    [(m) => (m.materials.stone.layers[1].kind = 'splat'), /materials\.stone\.layers\[1\]\.kind must be one of/],
    [(m) => (m.materials.stone.layers[0].color = 'meadow.glow'), /materials\.stone\.layers\[0\]\.color: unknown stop "glow"/],
    [(m) => (m.materials.stone.layers[0].color = 'mauve'), /unknown palette colour "mauve"/],
    [(m) => (m.materials.stone.layers[1].width = [9, 2]), /layers\[1\]\.width must be \[min, max\]/],
    [(m) => (m.materials.stone.layers[1].wobble = 1), /unknown entry "wobble" for a slab layer/],
    [(m) => (m.materials.stone.ramp = 'granite'), /materials\.stone\.ramp: unknown ramp "granite"/],
    [(m) => (m.materials.stone.use = 'prop'), /materials\.stone\.use must be one of/],
    [(m) => delete m.materials.stone.layers[1].count, /give exactly one of "count" \(scattered\) or "at"/],
    [(m) => (m.materials.cloth_wool_blue.like = 'cloth_wool_blue'), /"like" forms a cycle/],
    [(m) => (m.materials.soil.over = 'grass_side'), /over forms a cycle/],
    [(m) => (m.tiles.stone = 'nothing'), /tiles\.stone: unknown material "nothing"/],
    [(m) => (m.tiles.stone = 'leather'), /tiles\.stone: material "leather" is not a block material/],
    [(m) => delete m.tiles.sand, /the atlas has tile "sand", which "tiles" doesn't paint/],
    [(m) => delete m.tiles.missing, /the atlas has tile "missing", which "tiles" doesn't paint/],
    [(m) => (m.tiles.marble = 'stone'), /tiles\.marble: the atlas has no tile "marble"/],
    [(m) => (m.keepClassic = ['grid']), /unknown entry keepClassic/],
    [(m) => (m.tileSize = 64), /tileSize must be 32/],
  ];
  for (const [mutate, message] of cases) {
    const m = clone(materials);
    mutate(m);
    assert.throws(() => validateMaterials(m, palette, tiles), message, String(message));
  }
  const p = clone(palette);
  p.ramps.meadow.base = 'green';
  assert.throws(() => validatePalette(p), /ramps\.meadow\.base must be a colour/);
  const q = clone(palette);
  q.ramps.meadow.mid = '#000000';
  assert.throws(() => validatePalette(q), /unknown stop ramps\.meadow\.mid/);
});

test('the materials the characters reference exist with a ramp and an overlay', () => {
  const wanted = [
    'cloth_canvas', 'cloth_linen', 'cloth_wool_red', 'cloth_wool_blue', 'cloth_wool_green', 'leather',
    'metal_iron', 'metal_brass', 'fur_wolf', 'fur_wolf_belly', 'fur_deer', 'fur_deer_cream', 'antler',
    'skin_fair', 'skin_tan', 'skin_brown', 'skin_deep', 'hair_black', 'hair_brown', 'hair_ginger', 'hair_blonde',
  ];
  for (const name of wanted) {
    const r = materialRamp(name, materials, palette);
    assert.match(r.shadow, /^#/, name);
    assert.ok(r.overlay > 0 && r.overlay <= 1, name);
  }
  assert.ok(palette.swatches.ink);
});

/**
 * How far a material painted `over` another hangs down from its top edge, per column: the run of
 * texels from the top row down that differ from the material underneath.
 */
function hemLengths(name) {
  const top = paintMaterial(name, materials, palette);
  const under = paintMaterial(materials.materials[name].over, materials, palette);
  const covered = (x, y) => {
    const a = texel(top, x, y);
    const b = texel(under, x, y);
    return Math.abs(a[0] - b[0]) + Math.abs(a[1] - b[1]) + Math.abs(a[2] - b[2]) > 24;
  };
  return Array.from({ length: TILE }, (_, x) => {
    let n = 0;
    while (n < TILE && covered(x, TILE - 1 - n)) n++;
    return n;
  });
}

test('a thin band spreads its strokes over the whole width, not only its first cells', () => {
  const m = clone(materials);
  m.materials.fringe = {
    use: 'block',
    ramp: 'stone',
    layers: [
      { kind: 'fill', color: '@base' },
      { kind: 'slab', count: 4, band: [0.5, 0.52], jitter: 0, colors: ['ink'], width: 2, length: 2 },
    ],
  };
  const tile = paintMaterial('fringe', m, palette);
  const inked = (x0, x1) => {
    for (let x = x0; x < x1; x++) for (let y = 14; y < 18; y++) if (String(texel(tile, x, y).slice(0, 3)) === String(resolveColor(palette, 'ink'))) return true;
    return false;
  };
  for (let q = 0; q < 4; q++) assert.ok(inked((q * TILE) / 4, ((q + 1) * TILE) / 4), `a stroke in quarter ${q}`);
});

test("grass_side's turf skirt is one continuous, uneven hem that wraps across the tile", () => {
  const hem = hemLengths('grass_side');
  // The turf band (the edgeLight, 7 texels) plus at least two texels of overhang in every column:
  // no column of the skirt is bare, so tiles side by side join into one fringe.
  const bare = hem.flatMap((n, x) => (n < 9 ? [x] : []));
  assert.deepEqual(bare, [], `columns without an overhang (hem lengths ${hem.join(' ')})`);
  // It wraps: the first and last columns hang about as far, as neighbours across the seam do.
  assert.ok(Math.abs(hem[0] - hem[TILE - 1]) <= 3, `the seam: ${hem[0]} vs ${hem[TILE - 1]}`);
  // And it is a painted, uneven hem, not a ruler-straight one.
  assert.ok(Math.max(...hem) - Math.min(...hem) >= 3, `hem lengths ${hem.join(' ')}`);
});

test("the utility tiles: plain is white, grid is white with a faint edge line, missing is a magenta and black checker", () => {
  for (const t of ENGINE_TILES) assert.ok(t in materials.tiles, `${t} is painted`);
  const paint = (tile) => paintMaterial(materials.tiles[tile], materials, palette);
  const plain = paint('plain');
  for (let k = 0; k < TILE * TILE; k++) assert.deepEqual([...plain.subarray(k * 4, k * 4 + 4)], [255, 255, 255, 255]);
  // One texel of line along the bottom and left edges (each block edge once), white elsewhere.
  const grid = paint('grid');
  const line = [224, 224, 224, 255];
  for (let i = 0; i < TILE; i++) {
    assert.deepEqual(texel(grid, i, 0), line, `bottom ${i}`);
    assert.deepEqual(texel(grid, 0, i), line, `left ${i}`);
  }
  for (const [x, y] of [[1, 1], [TILE - 1, 5], [16, 30], [5, TILE - 1]]) assert.deepEqual(texel(grid, x, y), [255, 255, 255, 255]);
  // Squares of 8 texels, magenta where (x / 8 + y / 8) is even, with hard edges.
  const missing = paint('missing');
  for (let y = 0; y < TILE; y++) {
    for (let x = 0; x < TILE; x++) {
      const want = (Math.floor(x / 8) + Math.floor(y / 8)) % 2 === 0 ? [255, 0, 255, 255] : [0, 0, 0, 255];
      assert.deepEqual(texel(missing, x, y), want, `${x}, ${y}`);
    }
  }
});
