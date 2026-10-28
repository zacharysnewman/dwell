// The items' validation and output (shared/items/gen.mjs).
import assert from 'node:assert/strict';
import test from 'node:test';
import { buildModesTs, itemsRust, loadItems, registryBlocks, validateItems } from './gen.mjs';

const blocks = registryBlocks();

test("the build's items validate", () => {
  const rs = itemsRust(validateItems(loadItems(), blocks));
  assert.match(rs, /name: "dwell:stone", label: "Stone", block: "dwell:stone", rows: 1, drag: ItemDrag::Modes/);
  assert.match(rs, /name: "dwell:ladder", .*drag: ItemDrag::Vertical/);
  assert.match(rs, /name: "dwell:torch", .*drag: ItemDrag::None/);
  assert.match(rs, /BUILD_MODE_KEY: &str = "KeyL"/);
  assert.match(rs, /name: "dwell:line", label: "Line", kind: DragKind::Line/);
  assert.match(buildModesTs(loadItems()), /key: 'KeyL'/);
});

test('an item added by data alone is emitted', () => {
  const d = loadItems();
  d.items.push({ name: 'test:leaves_tall', label: 'Leaves, 1 m', block: 'dwell:leaves', rows: 2 });
  assert.match(itemsRust(validateItems(d, blocks)), /name: "test:leaves_tall".*rows: 2/);
});

test('a build mode added by data alone is emitted', () => {
  const d = loadItems();
  d.buildModes.modes.push({ name: 'test:ceiling', label: 'Ceiling', kind: 'level' });
  assert.match(
    itemsRust(validateItems(d, blocks)),
    /name: "test:ceiling", label: "Ceiling", kind: DragKind::Level/,
  );
});

test('bad items fail with the entry and the problem', () => {
  const cases = [
    [(d) => (d.items[0].block = 'dwell:nope'), /items\[0\]\.block "dwell:nope" is not in the registry/],
    [(d) => (d.items[0].block = 'dwell:bedrock'), /items\[0\]\.block dwell:bedrock is not placeable/],
    [(d) => (d.items[0].rows = 3), /items\[0\]\.rows must be 1\.\.2/],
    [(d) => (d.items[8].rows = 2), /items\[8\]: dwell:torch brings its own other cells/],
    [(d) => (d.items[3].drag = 'modes'), /items\[3\]: dwell:ladder has place rules \(palette, needsSupport\)/],
    [(d) => (d.items[0].drag = 'vertical'), /items\[0\]: drag vertical is for a block hung on a wall/],
    [(d) => (d.items[0].drag = 'sideways'), /items\[0\]\.drag "sideways" must be one of/],
    [(d) => (d.items[1].name = d.items[0].name), /items\[1\]\.name dwell:stone is listed twice/],
    [(d) => (d.items[0].colour = 'red'), /items\[0\]: unknown colour/],
    [(d) => (d.buildModes.modes[1].kind = 'wall'), /buildModes\.modes\[1\]\.kind "wall" must be one of line, level/],
    [(d) => (d.buildModes.modes[1].name = 'dwell:line'), /buildModes\.modes\[1\]\.name dwell:line is listed twice/],
    [(d) => (d.buildModes.key = 'l'), /buildModes\.key must be a KeyboardEvent\.code/],
    [(d) => (d.buildModes.modes = []), /buildModes\.modes must be a non-empty list/],
    [(d) => (d.buildModes.clearance = 2), /buildModes\.clearance must be 0\.\.0\.5/],
    [(d) => (d.buildModes.holdMs = -1), /buildModes\.holdMs must be 0\.\.2000/],
  ];
  for (const [change, message] of cases) {
    const d = loadItems();
    change(d);
    assert.throws(() => validateItems(d, blocks), message);
  }
});
