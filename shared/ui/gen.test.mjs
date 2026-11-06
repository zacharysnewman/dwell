// node --test shared/ui/gen.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { hudCss, hudTs, validateHud } from './gen.mjs';

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
