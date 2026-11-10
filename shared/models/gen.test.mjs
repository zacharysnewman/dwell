// node --test shared/models/gen.test.mjs: the skinned models' data (docs/CHARACTERS.md §11).
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { generate, glbSummary, paletteStops, resolveColor, rust, validate } from './gen.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..', '..');
const palette = JSON.parse(readFileSync(join(root, 'shared/textures/palette.json'), 'utf8'));
const data = () => JSON.parse(readFileSync(join(here, 'models.json'), 'utf8'));
const fixtures = join(here, 'fixtures');
const fixture = () => JSON.parse(readFileSync(join(fixtures, 'models.json'), 'utf8'));

test('the data validates and the generated tables are current', () => {
  validate(data(), palette, here);
  for (const [path, text] of generate()) {
    assert.equal(readFileSync(path, 'utf8'), text, `${path} is stale: run node shared/models/gen.mjs`);
  }
});

test('every GLB holds the clips its roles name', () => {
  for (const [name, m] of Object.entries(data().models)) {
    const s = glbSummary(readFileSync(join(here, m.glb)));
    for (const [role, r] of Object.entries(m.roles)) assert.ok(s.clips.includes(r.clip), `${name} ${role}`);
  }
});

test('a model is added by data alone: the fixture validates and lands in the table', () => {
  const f = fixture();
  const models = validate(f, palette, fixtures);
  assert.equal(models.two_bone.joints, 2);
  assert.deepEqual(models.two_bone.clips, ['rest', 'bend']);
  const out = rust(f, models, '../../../shared/models/fixtures');
  assert.match(out, /name: "two_bone"/);
  assert.match(out, /role: "sprint", clip: "bend", speed: 2\.0/);
  assert.match(out, /include_bytes!\("\.\.\/\.\.\/\.\.\/shared\/models\/fixtures\/two_bone\.glb"\)/);
});

test('a role naming a clip the GLB lacks fails naming it', () => {
  const f = fixture();
  f.models.two_bone.roles.walk.clip = 'cartwheel';
  assert.throws(() => validate(f, palette, fixtures), /model two_bone: role walk: no clip cartwheel/);
});

test('a missing GLB fails naming it', () => {
  const f = fixture();
  f.models.two_bone.glb = 'nope.glb';
  assert.throws(() => validate(f, palette, fixtures), /model two_bone: missing GLB nope\.glb/);
});

test('unknown roles, bad fallbacks, scales and remaps fail', () => {
  const bad = (edit, re) => {
    const f = fixture();
    edit(f.models.two_bone, f);
    assert.throws(() => validate(f, palette, fixtures), re);
  };
  bad((m) => (m.roles.dance = { clip: 'rest' }), /unknown role dance/);
  bad((m) => (m.fallback = 'swim'), /fallback swim/);
  bad((m) => (m.scale = 0), /scale must be > 0/);
  bad((m) => (m.remap = { '#123456': 'nowhere.base' }), /no palette colour nowhere\.base/);
  bad((m) => (m.remap = { red: 'soil' }), /not #rrggbb/);
  bad((_, f) => (f.player = 'ghost'), /player: no model ghost/);
  bad((m) => (m.roles.walk.speed = -1), /speed must be > 0/);
});

test('palette references resolve; excluded ramps are left out of the snap', () => {
  assert.equal(resolveColor(palette, 'soil'), palette.ramps.soil.base);
  assert.equal(resolveColor(palette, 'soil.light'), palette.ramps.soil.light);
  assert.equal(resolveColor(palette, 'ink'), palette.swatches.ink);
  const names = paletteStops(palette, ['chalk']).map((s) => s.name);
  assert.ok(names.includes('soil.shadow'));
  assert.ok(!names.some((n) => n.startsWith('chalk.')));
});
