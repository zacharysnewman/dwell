// Tests of the structure generator's validation, canonical form and hash
// (`node --test shared/structures/`). The registry is a fixed vector, so these don't move when
// the real block set changes.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { canonicalState, canonicalText, MAX_EDGE, registryFromVector, structureHash, validate } from './gen.mjs';

const registry = registryFromVector(
  [
    '# a fixed registry',
    'hash 0',
    '0 dwell:air',
    '1 dwell:stone',
    '2 dwell:ladder[facing=north,flooded=false]',
    '3 dwell:ladder[facing=north,flooded=true]',
    '4 dwell:ladder[facing=east,flooded=false]',
    '5 dwell:ladder[facing=east,flooded=true]',
    '6 dwell:sand',
  ].join('\n'),
);

const good = () => ({
  format: 1,
  name: 'test:post',
  kind: 'voxels',
  size: [2, 1, 2],
  anchor: [0, 0, 0],
  palette: { '.': null, s: 'dwell:stone', l: 'dwell:ladder[facing=east]', _: 'dwell:air' },
  layers: [['sl', '._']],
  variants: { rotate: [90, 0], mirror: ['x'], choices: { s: [['dwell:stone', 3], ['dwell:sand', 1]] } },
});

const fails = (mutate, ...expected) => {
  const json = good();
  mutate(json);
  assert.throws(
    () => validate(json, 'post.json', registry),
    (e) => {
      for (const want of ['post.json', ...expected]) assert.ok(e.message.includes(want), `"${e.message}" lacks "${want}"`);
      return true;
    },
  );
};

test('block states canonicalise like parse_state', () => {
  assert.equal(canonicalState(registry, 'dwell:ladder'), 'dwell:ladder[facing=north,flooded=false]');
  assert.equal(canonicalState(registry, 'dwell:ladder[flooded=true,facing=east]'), 'dwell:ladder[facing=east,flooded=true]');
  assert.equal(canonicalState(registry, 'dwell:stone'), 'dwell:stone');
  assert.throws(() => canonicalState(registry, 'dwell:nope'), /unknown block "dwell:nope"/);
  assert.throws(() => canonicalState(registry, 'dwell:ladder[facing=up]'), /no value "up"/);
  assert.throws(() => canonicalState(registry, 'dwell:ladder[facing=east,facing=east]'), /repeated/);
});

test('a good structure validates to its canonical form', () => {
  const s = validate(good(), 'post.json', registry);
  assert.equal(s.cells, 'sl._');
  assert.equal(s.palette.get('l'), 'dwell:ladder[facing=east,flooded=false]');
  assert.deepEqual(s.variants[0], { op: 'rotate', values: [0, 90] });
  assert.equal(
    canonicalText(s),
    [
      'structure 1',
      'name test:post',
      'kind voxels',
      'size 2 1 2',
      'anchor 0 0 0',
      'palette . keep',
      'palette _ dwell:air',
      'palette l dwell:ladder[facing=east,flooded=false]',
      'palette s dwell:stone',
      'layer 0',
      'sl',
      '._',
      'rotate 0 90',
      'mirror x',
      'choices s dwell:stone 3 dwell:sand 1',
      '',
    ].join('\n'),
  );
});

test('the hash is stable: pinned, and independent of key order, spelling and whitespace', () => {
  const s = validate(good(), 'post.json', registry);
  const hash = structureHash(canonicalText(s));
  assert.equal(hash, 'bd29aea8c2bcbaa9');
  const respelled = JSON.parse(
    JSON.stringify({
      variants: { choices: { s: [['dwell:stone', 3], ['dwell:sand', 1]] }, mirror: ['x'], rotate: [0, 90] },
      layers: [['sl', '._']],
      palette: { _: 'dwell:air', l: 'dwell:ladder[flooded=false,facing=east]', s: 'dwell:stone', '.': null },
      anchor: [0, 0, 0],
      size: [2, 1, 2],
      kind: 'voxels',
      name: 'test:post',
      format: 1,
    }),
  );
  assert.equal(structureHash(canonicalText(validate(respelled, 'post.json', registry))), hash);
  const changed = good();
  changed.layers[0][1] = '.s';
  assert.notEqual(structureHash(canonicalText(validate(changed, 'post.json', registry))), hash);
});

test('bad entries fail naming the file and the entry', () => {
  fails((j) => (j.palette.s = 'dwell:no_such_block'), 'palette."s"', 'unknown block "dwell:no_such_block"');
  fails((j) => (j.palette.ab = 'dwell:stone'), 'palette."ab"', 'single printable');
  fails((j) => (j.palette[' '] = 'dwell:stone'), 'palette." "');
  fails((j) => (j.layers[0][0] = 'sx'), 'layers[0][0][1]', '"x" is not in the palette');
  fails((j) => (j.layers[0][0] = 's'), 'layers[0][0]', 'size[0] = 2');
  fails((j) => j.layers[0].pop(), 'layers[0]', 'size[2] = 2');
  fails((j) => j.layers.push([]), 'layers', 'size[1] = 1');
  fails((j) => (j.anchor = [2, 0, 0]), 'anchor', 'inside the size');
  fails((j) => (j.kind = 'curves'), 'kind', 'unknown kind "curves"');
  fails((j) => (j.format = 2), 'format', 'must be 1');
  fails((j) => (j.name = 'post'), 'name', 'namespace:name');
  fails((j) => (j.extra = 1), 'extra', 'unknown field');
  fails((j) => (j.variants.scale = [2]), 'variants.scale', 'unknown variant');
  fails((j) => (j.variants.rotate = [45]), 'variants.rotate', '45 is not one of');
  fails((j) => (j.variants.rotate = [0, 0]), 'variants.rotate', 'repeats');
  fails((j) => (j.variants.mirror = ['y']), 'variants.mirror', '"y" is not one of');
  fails((j) => (j.variants.choices = { q: [['dwell:stone', 1]] }), 'variants.choices."q"', 'not in the palette');
  fails((j) => (j.variants.choices = { '.': [['dwell:stone', 1]] }), 'variants.choices."."', 'keeps what is there');
  fails((j) => (j.variants.choices.s[1][1] = 0), 'variants.choices."s"[1]', 'positive integer');
  fails((j) => (j.variants.choices.s[0][0] = 'dwell:nope'), 'variants.choices."s"[0]', 'unknown block');
});

test(`structures are capped at ${MAX_EDGE} voxels a side`, () => {
  const big = (n) => ({
    ...good(),
    size: [n, 1, 1],
    palette: { s: 'dwell:stone' },
    layers: [['s'.repeat(n)]],
    variants: {},
  });
  assert.equal(validate(big(MAX_EDGE), 'big.json', registry).cells.length, MAX_EDGE);
  assert.throws(() => validate(big(MAX_EDGE + 1), 'big.json', registry), /big\.json: size: .*1\.\.32/);
});
