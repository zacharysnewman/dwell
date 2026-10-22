// node --test shared/look/lodmap.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { lodMapRust, validateLodMap } from './lodmap.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'lodmap.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in tunables validate and generate the checked-in Rust', () => {
  assert.equal(lodMapRust(validateLodMap(data())), checkedIn('crates/dwell-look/src/lodmap.gen.rs'));
});

test('a ring added by data alone reaches the output', () => {
  const d = data();
  d.rings = d.rings.slice(0, 3);
  d.rings.push({ texelM: 512, size: d.rings[0].size });
  assert.match(lodMapRust(validateLodMap(d)), /texel_m: 512,\n            size: 512,/);
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.rings[1].size = 300), /rings\[1\]\.size must be a power of two/],
    [(d) => (d.rings[1].size = 256), /rings\[1\]\.size must equal rings\[0\]\.size/],
    [(d) => (d.rings[2].texelM = 8), /rings\[2\]\.texelM must be coarser/],
    [(d) => (d.blend.toM = 1), /blend\.toM must be a number/],
    [(d) => (d.water.block = 'water'), /water\.block must be a block name/],
    [(d) => (d.extra = 1), /unknown entry \(root\)\.extra/],
    [(d) => delete d.valleyMarginM, /missing entry \(root\)\.valleyMarginM/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateLodMap(d), message);
  }
});
