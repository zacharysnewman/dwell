// The items' validation and output (shared/items/gen.mjs).
import assert from 'node:assert/strict';
import test from 'node:test';
import { itemsRust, loadItems, registryBlocks, validateItems } from './gen.mjs';

const blocks = registryBlocks();

test("the build's items validate", () => {
  const rs = itemsRust(validateItems(loadItems(), blocks));
  assert.match(rs, /name: "dwell:stone", label: "Stone", block: "dwell:stone", rows: 2/);
  assert.match(rs, /name: "dwell:stone_half", label: "Stone, ½ m", block: "dwell:stone", rows: 1/);
});

test('an item added by data alone is emitted', () => {
  const d = loadItems();
  d.items.push({ name: 'test:leaves_half', label: 'Leaves, ½ m', block: 'dwell:leaves', rows: 1 });
  assert.match(itemsRust(validateItems(d, blocks)), /name: "test:leaves_half"/);
});

test('bad items fail with the entry and the problem', () => {
  const cases = [
    [(d) => (d.items[0].block = 'dwell:nope'), /items\[0\]\.block "dwell:nope" is not in the registry/],
    [(d) => (d.items[0].block = 'dwell:bedrock'), /items\[0\]\.block dwell:bedrock is not placeable/],
    [(d) => (d.items[0].rows = 3), /items\[0\]\.rows must be 1\.\.2/],
    [(d) => (d.items[8].rows = 2), /items\[8\]: dwell:torch brings its own other cells/],
    [(d) => (d.items[1].name = d.items[0].name), /items\[1\]\.name dwell:stone is listed twice/],
    [(d) => (d.items[0].colour = 'red'), /items\[0\]: unknown colour/],
  ];
  for (const [change, message] of cases) {
    const d = loadItems();
    change(d);
    assert.throws(() => validateItems(d, blocks), message);
  }
});
