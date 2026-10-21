// node --test shared/look/loading.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { VIEW_RADIUS_CHUNKS, loadingRust, validateLoading } from './loading.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'loading.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in tunables validate and generate the checked-in Rust', () => {
  assert.equal(loadingRust(validateLoading(data())), checkedIn('crates/dwell-game/src/loading.gen.rs'));
});

test('a change by data alone is in the output', () => {
  const d = data();
  d.radiusChunks = 1;
  d.timeoutS = 7.5;
  const rs = loadingRust(validateLoading(d));
  assert.match(rs, /radius_chunks: 1,/);
  assert.match(rs, /timeout_s: 7\.5,/);
});

test('the near ring stays within the server-streamed view', () => {
  assert.equal(VIEW_RADIUS_CHUNKS, 3);
  const d = data();
  d.radiusChunks = 4;
  assert.throws(() => validateLoading(d, 4 - 1), /radiusChunks must be a whole number in \[0, 3\]/);
  assert.doesNotThrow(() => validateLoading(d, 5));
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.radiusChunks = 1.5), /radiusChunks must be a whole number/],
    [(d) => (d.radiusChunks = -1), /radiusChunks must be a whole number/],
    [(d) => (d.timeoutS = 0), /timeoutS must be a number in \[1, 600\]/],
    [(d) => (d.timeoutS = '20'), /timeoutS must be a number/],
    [(d) => (d.radius = 2), /unknown entry radius/],
    [(d) => delete d.timeoutS, /missing entry timeoutS/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateLoading(d), message);
  }
});
