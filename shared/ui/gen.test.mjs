// node --test shared/ui/gen.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  hudCss,
  hudTs,
  layoutTs,
  mapRs,
  mapTs,
  levelSpans,
  menuBackgroundsTs,
  validateHud,
  validateLayout,
  validateMap,
  validateMenuBackgrounds,
} from './gen.mjs';

const backgroundsData = () => JSON.parse(readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'menu-backgrounds.json'), 'utf8'));
const hasImage = (file) => existsSync(join(dirname(fileURLToPath(import.meta.url)), 'menu-backgrounds', file));

test('the checked-in menu backgrounds validate, their images are there, and they generate the checked-in TypeScript', () => {
  const ts = menuBackgroundsTs(validateMenuBackgrounds(backgroundsData(), hasImage));
  const out = join(dirname(fileURLToPath(import.meta.url)), '../../client/src/ui/menuBackgrounds.gen.ts');
  assert.equal(ts, readFileSync(out, 'utf8'));
});

test('a menu background added by data alone reaches the output', () => {
  const d = backgroundsData();
  d.backgrounds.push({ name: 'sky-island', label: 'Sky island', about: 'A fixture.', seed: 7, position: [1, 2, 3], yaw: 90, timeOfDay: 0.75, image: 'sky-island.webp' });
  const ts = menuBackgroundsTs(validateMenuBackgrounds(d));
  assert.match(ts, /import IMAGE_SKY_ISLAND from '\.\.\/\.\.\/\.\.\/shared\/ui\/menu-backgrounds\/sky-island\.webp';/);
  assert.match(ts, /\{ name: "sky-island", label: "Sky island", url: IMAGE_SKY_ISLAND \}/);
});

test('bad menu backgrounds fail with their path named', () => {
  const cases = [
    [(d) => (d.default = 'nowhere'), /default must name a background/],
    [(d) => (d.backgrounds[0].name = 'Beach Peaks'), /backgrounds\[0\]\.name must be lowercase words/],
    [(d) => delete d.backgrounds[1].seed, /missing entry backgrounds\[1\]\.seed/],
    [(d) => (d.backgrounds[0].position = [1, 2]), /backgrounds\[0\]\.position must be \[x, y, z\]/],
    [(d) => (d.backgrounds[0].timeOfDay = 1.5), /backgrounds\[0\]\.timeOfDay must be a number within 0–1/],
    [(d) => (d.backgrounds[0].image = 'beach.png'), /backgrounds\[0\]\.image must be a file name/],
    [(d) => (d.backgrounds[0].colour = 'red'), /unknown entry backgrounds\[0\]\.colour/],
    [(d) => d.backgrounds.push({ ...d.backgrounds[0] }), /is listed twice/],
    [(d) => (d.capture.steps = 3), /capture\.steps must be a whole number within 8–64/],
    [(d) => (d.turnS = 5), /turnS must be a number within 30–1200/],
  ];
  for (const [change, message] of cases) {
    const d = backgroundsData();
    change(d);
    assert.throws(() => validateMenuBackgrounds(d), message);
  }
  assert.throws(() => validateMenuBackgrounds(backgroundsData(), () => false), /menu-backgrounds\/beach-peaks\.webp is missing/);
});

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'hud.json'), 'utf8'));

test('the checked-in parts validate and generate the checked-in TypeScript', () => {
  const ts = hudTs(validateHud(data()));
  assert.equal(ts, readFileSync(join(here, '../../client/src/ui/hud.gen.ts'), 'utf8'));
  const css = hudCss(validateHud(data()));
  assert.equal(css, readFileSync(join(here, '../../client/src/ui/hud.gen.css'), 'utf8'));
});

test('a part added by data alone reaches the output', () => {
  const d = data();
  d.elements.push({ id: 'compass', label: 'Compass', hint: 'North', selector: '#compass', shown: false, phone: true });
  assert.match(hudTs(validateHud(d)), /"id":"compass","label":"Compass"/);
  assert.match(hudCss(validateHud(d)), /body\.hud-hide-compass #compass \{\n  display: none !important;/);
});

test('the menu button is the one part a phone always shows', () => {
  const kept = data().elements.filter((e) => !e.phone).map((e) => e.id);
  assert.deepEqual(kept, ['menu-button']);
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.elements[0].id = 'Menu Button'), /elements\[0\]\.id must be lowercase words/],
    [(d) => delete d.elements[1].selector, /missing entry elements\[1\]\.selector/],
    [(d) => (d.elements[1].selector = 'body *'), /elements\[1\]\.selector must be one #id or \.class selector/],
    [(d) => (d.elements[2].shown = 'yes'), /elements\[2\]\.shown must be true or false/],
    [(d) => (d.elements[2].color = 'red'), /unknown entry elements\[2\]\.color/],
    [(d) => d.elements.push({ ...d.elements[0] }), /is listed twice/],
  ];
  for (const [change, message] of cases) {
    const d = data();
    change(d);
    assert.throws(() => validateHud(d), message);
  }
});

const mapData = () => JSON.parse(readFileSync(join(here, 'map.json'), 'utf8'));

test('the checked-in map data validates and generates the checked-in Rust and TypeScript', () => {
  const d = validateMap(mapData());
  assert.equal(mapRs(d), readFileSync(join(here, '../../crates/dwell-game/src/map.gen.rs'), 'utf8'));
  assert.equal(mapTs(d), readFileSync(join(here, '../../client/src/ui/map.gen.ts'), 'utf8'));
});

test('a changed look or size reaches both outputs by data alone', () => {
  const d = mapData();
  d.minimap.radiusM = 96;
  d.minimap.otherColor = '#00ff88';
  assert.match(mapRs(validateMap(d)), /minimap_radius_m: 96\.0,/);
  assert.match(mapRs(validateMap(d)), /minimap_other_color: 0x00ff88,/);
  assert.match(mapTs(validateMap(d)), /otherColor: '#00ff88',/);
});

test("the zoom levels, the height tint and the map's controller buttons are data", () => {
  const d = mapData();
  // A fixture level by data alone: a sixth would be too many, so a 3-level map with its own sizes.
  d.bigMap.levels = [{ cellM: 1 }, { cellM: 16 }, { cellM: 512 }];
  d.bigMap.startLevel = 2;
  d.heightTint.brightest = 1.3;
  d.bigMap.padCenter = 'x';
  d.bigMap.panelFraction = 0.9;
  const rs = mapRs(validateMap(d));
  // The spans: the minimap's diameter, the island's, the geometric mean between.
  const mid = Math.round(Math.sqrt(128 * 50000));
  assert.deepEqual(levelSpans(d), [128, mid, 50000]);
  assert.match(rs, new RegExp(`big_map_levels: &\\[\\(1\\.0, 128\\.0\\), \\(16\\.0, ${mid}\\.0\\), \\(512\\.0, 50000\\.0\\)\\],`));
  assert.match(rs, /big_map_start_level: 2\.0,/);
  assert.match(rs, /height_tint_brightest: 1\.3,/);
  const ts = mapTs(validateMap(d));
  assert.match(ts, /\{ cellM: 16, spanM: \d+ \},/);
  assert.match(rs, /big_map_pad_center: "x",/);
  assert.match(ts, /padCenter: 'x',/);
  assert.match(ts, /panelFraction: 0\.9,/);
});

test('bad map entries fail naming them', () => {
  for (const [change, message] of [
    [(d) => (d.minimap.radiusM = 3), /minimap\.radiusM must be within/],
    [(d) => (d.minimap.playerColor = 'red'), /minimap\.playerColor must be a #rrggbb colour/],
    [(d) => (d.heightShading.default = 1), /heightShading\.default must be true or false/],
    [(d) => (d.explored.cellM = 7), /cellM must divide/],
    [(d) => (d.bigMap.levels = [{ cellM: 1 }]), /bigMap\.levels must be a list of 2 to 5 levels/],
    [(d) => d.bigMap.levels.push({ cellM: 1024 }), /bigMap\.levels must be a list of 2 to 5 levels/],
    [(d) => (d.bigMap.levels[1].cellM = 3), /bigMap\.levels\[1\]\.cellM must be a whole power of two/],
    [(d) => (d.bigMap.levels[2].cellM = 2), /bigMap\.levels\[2\]\.cellM must be above the level before it/],
    [(d) => (d.bigMap.levels[0].spanM = 9), /unknown entry bigMap\.levels\[0\]\.spanM/],
    [(d) => (d.bigMap.startLevel = 4) && (d.bigMap.levels = [{ cellM: 1 }, { cellM: 2 }]), /bigMap\.startLevel must name a level/],
    [(d) => (d.heightTint.highM = d.heightTint.lowM), /heightTint: lowM must be below highM/],
    [(d) => (d.heightTint.darkest = 0.1), /heightTint\.darkest must be within/],
    [(d) => (d.minimap.extra = 1), /unknown entry minimap\.extra/],
    [(d) => delete d.bigMap.fogColor, /missing entry bigMap\.fogColor/],
    [(d) => (d.bigMap.padClose = 'z'), /bigMap\.padClose must be a controller button/],
    [(d) => (d.bigMap.padCenter = d.bigMap.padClose), /controller buttons must differ/],
    [(d) => (d.bigMap.padZoomIn = 'view'), /controller buttons must differ/],
  ]) {
    const d = mapData();
    change(d);
    assert.throws(() => validateMap(d), message);
  }
});

const layoutData = () => JSON.parse(readFileSync(join(here, 'layout.json'), 'utf8'));

test('the checked-in layout validates and generates the checked-in TypeScript', () => {
  assert.equal(layoutTs(validateLayout(layoutData())), readFileSync(join(here, '../../client/src/ui/layout.gen.ts'), 'utf8'));
});

test('a layout part added by data alone reaches the output', () => {
  const d = layoutData();
  d.parts.push({ id: 'compass', selector: '#compass', anchor: 'top-right', priority: 2, after: 'minimap', landscape: { flush: true } });
  assert.match(layoutTs(validateLayout(d)), /\{"id":"compass","selector":"#compass","anchor":"top-right","priority":2,"after":"minimap","landscape":\{"flush":true\}\}/);
});

test('bad layout entries fail with their path named', () => {
  const at = (id) => (d) => d.parts.find((p) => p.id === id);
  const cases = [
    [(d) => (at('minimap')(d).anchor = 'middle'), /parts\[\d+\]\.anchor must be one of top-left/],
    [(d) => (at('performance')(d).after = 'nothing'), /parts\[\d+\]\.after must name a part listed before it/],
    [(d) => (at('hotbar')(d).touch.size = 3), /unknown entry parts\[\d+\]\.touch\.size/],
    [(d) => (at('crosshair')(d).anchor = 'top'), /parts\[\d+\]\.anchor: a fixed part isn't placed/],
    [(d) => delete at('minimap')(d).priority, /parts\[\d+\]\.priority must be a whole number/],
    [(d) => (at('hotbar')(d).minWidthPx = -1), /minWidthPx must be a whole number within 0–2000/],
    [(d) => (d.keepClear[0].x = [0.8, 0.2]), /keepClear\[0\]\.x must be \[from, to\]/],
    [(d) => (d.keepClear[0].orientation = 'sideways'), /keepClear\[0\]\.orientation must be/],
    [(d) => d.parts.push({ ...d.parts[2] }), /is listed twice/],
  ];
  for (const [change, message] of cases) {
    const d = layoutData();
    change(d);
    assert.throws(() => validateLayout(d), message);
  }
});
