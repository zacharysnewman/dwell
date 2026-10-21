// node --test shared/blocks/parts.test.mjs — multi-cell blocks as block data (BLOCK_REGISTRY.md).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkParts } from './parts.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const dwell = () => JSON.parse(readFileSync(join(here, 'dwell.json'), 'utf8'));
const blocksOf = (ns) => ns.blocks.filter((d) => !d.family).map((def) => ({ id: `dwell:${def.name}`, def }));

test('the torch is two cells: its upper part names it back', () => {
  const parts = checkParts(blocksOf(dwell()));
  assert.deepEqual(parts.get('dwell:torch'), { extendsUp: 'dwell:torch_upper', partOf: null });
  assert.deepEqual(parts.get('dwell:torch_upper'), { extendsUp: null, partOf: 'dwell:torch' });
  assert.deepEqual(parts.get('dwell:stone'), { extendsUp: null, partOf: null });
});

test('a multi-cell block added by data alone validates (a lantern on a post)', () => {
  const blocks = [
    { id: 'test:post', def: { extends: { up: 'test:lantern' } } },
    { id: 'test:lantern', def: { partOf: 'test:post' } },
  ];
  assert.equal(checkParts(blocks).get('test:post').extendsUp, 'test:lantern');
});

test('a dangling or mismatched partner fails with the block named', () => {
  const cases = [
    [[{ id: 'a:x', def: { extends: { up: 'a:gone' } } }], /a:x: extends.up names unknown block a:gone/],
    [[{ id: 'a:x', def: { extends: { down: 'a:y' } } }, { id: 'a:y', def: { partOf: 'a:x' } }], /extends.down: directions are up/],
    [[{ id: 'a:x', def: { extends: { up: 'a:y' } } }, { id: 'a:y', def: {} }], /whose partOf must be a:x/],
    [[{ id: 'a:y', def: { partOf: 'a:x' } }, { id: 'a:x', def: {} }], /a:y: partOf names a:x, whose extends.up must be a:y/],
    [[{ id: 'a:x', def: { extends: { up: 'a:y' } } }, { id: 'a:y', def: { partOf: 'a:x', placeable: true } }], /may not be placeable/],
    [[{ id: 'a:y', def: { partOf: 'a:nope' } }], /partOf names unknown block a:nope/],
  ];
  for (const [blocks, re] of cases) assert.throws(() => checkParts(blocks), re);
});
