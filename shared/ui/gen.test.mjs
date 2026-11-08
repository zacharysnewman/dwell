// node --test shared/ui/gen.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { hudCss, hudTs, layoutTs, mapRs, mapTs, validateHud, validateLayout, validateMap } from './gen.mjs';

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

test('bad map entries fail naming them', () => {
  for (const [change, message] of [
    [(d) => (d.minimap.radiusM = 3), /minimap\.radiusM must be within/],
    [(d) => (d.minimap.playerColor = 'red'), /minimap\.playerColor must be a #rrggbb colour/],
    [(d) => (d.heightShading.default = 1), /heightShading\.default must be true or false/],
    [(d) => (d.explored.cellM = 7), /cellM must divide/],
    [(d) => (d.bigMap.startPxPerCell = 100), /minPxPerCell ≤ startPxPerCell ≤ maxPxPerCell/],
    [(d) => (d.minimap.extra = 1), /unknown entry minimap\.extra/],
    [(d) => delete d.bigMap.fogColor, /missing entry bigMap\.fogColor/],
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
