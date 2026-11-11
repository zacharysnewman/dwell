// The items' validation and output (shared/items/gen.mjs).
import assert from 'node:assert/strict';
import test from 'node:test';
import { buildModesTs, itemsRust, loadItems, registryBlocks, toolNames, validateItems } from './gen.mjs';

const blocks = registryBlocks();

test("the build's items validate", () => {
  const rs = itemsRust(validateItems(loadItems(), blocks));
  assert.match(rs, /name: "dwell:stone", label: "Stone", block: "dwell:stone", tool: None, rows: 1, drag: ItemDrag::Modes/);
  assert.match(rs, /name: "dwell:ladder", .*drag: ItemDrag::Vertical/);
  assert.match(rs, /name: "dwell:torch", .*drag: ItemDrag::None/);
  // The stone axe is on the creative hotbar (ADR 0067; owner, 2026-11-08): a tool, placing nothing.
  assert.match(rs, /name: "dwell:stone_axe", label: "Stone axe", block: "", tool: Some\("dwell:stone_axe"\), rows: 1, drag: ItemDrag::None/);
  assert.match(rs, /BUILD_MODE_KEY: &str = "KeyL"/);
  assert.match(rs, /name: "dwell:line", label: "Line", kind: DragKind::Line/);
  assert.match(buildModesTs(loadItems()), /key: 'KeyL'/);
  assert.match(rs, /HOTBAR_PREV_KEY: &str = "ArrowLeft"/);
  assert.match(buildModesTs(loadItems()), /HOTBAR_KEYS = \{ prev: 'ArrowLeft', next: 'ArrowRight' \}/);
});

test('an item added by data alone is emitted', () => {
  const d = loadItems();
  d.items.push({ name: 'test:leaves_tall', label: 'Leaves, 1 m', category: 'plants', block: 'dwell:leaves', rows: 2 });
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
  // By name, not position: the hotbar's order changes.
  const at = (d, name) => d.items.findIndex((it) => it.name === name);
  const item = (d, name) => d.items[at(d, name)];
  const ladder = at(loadItems(), 'dwell:ladder');
  const torch = at(loadItems(), 'dwell:torch');
  const slope = at(loadItems(), 'dwell:stone_slope');
  const steep = at(loadItems(), 'dwell:stone_steep_slope');
  const cases = [
    [(d) => (d.items[0].block = 'dwell:nope'), /items\[0\]\.block "dwell:nope" is not in the registry/],
    [(d) => (d.items[0].block = 'dwell:bedrock'), /items\[0\]\.block dwell:bedrock is not placeable/],
    [(d) => (d.items[0].rows = 3), /items\[0\]\.rows must be 1\.\.2/],
    [(d) => (item(d, 'dwell:torch').rows = 2), new RegExp(`items\\[${torch}\\]: dwell:torch brings its own other cells`)],
    [(d) => (item(d, 'dwell:ladder').drag = 'modes'), new RegExp(`items\\[${ladder}\\]: dwell:ladder has place rules \\(palette, needsSupport\\)`)],
    [(d) => (d.items[0].drag = 'vertical'), /items\[0\]: drag vertical is for a block hung on a wall/],
    [(d) => (d.items[0].drag = 'sideways'), /items\[0\]\.drag "sideways" must be one of/],
    [(d) => (d.items[1].name = d.items[0].name), /items\[1\]\.name dwell:stone is listed twice/],
    [(d) => (d.items[0].colour = 'red'), /items\[0\]: unknown colour/],
    // The slopes' state, facing and upper cell (owner, 2026-10-30).
    [(d) => (item(d, 'dwell:stone_slope').state.shape = 'curl'), new RegExp(`items\\[${slope}\\]: dwell:stone_ramp\\[facing=north,flooded=false,half=bottom,shape=curl\\] is not a state of the registry`)],
    [(d) => (item(d, 'dwell:stone_slope').facing = 'up'), new RegExp(`items\\[${slope}\\]\\.facing must be one of toward, away`)],
    [(d) => (item(d, 'dwell:stone_steep_slope').rows = 1), new RegExp(`items\\[${steep}\\]\\.upper is a 2-row piece's upper cell`)],
    [(d) => (item(d, 'dwell:stone_steep_slope').upper = { shape: 7 }), new RegExp(`items\\[${steep}\\]\\.upper must be property values`)],
    [(d) => (item(d, 'dwell:stone_slope').block = 'dwell:dirt_ramp'), new RegExp(`items\\[${slope}\\]\\.block dwell:dirt_ramp is not placeable`)],
    [(d) => (d.buildModes.modes[1].kind = 'wall'), /buildModes\.modes\[1\]\.kind "wall" must be one of line, level/],
    [(d) => (d.buildModes.modes[1].name = 'dwell:line'), /buildModes\.modes\[1\]\.name dwell:line is listed twice/],
    [(d) => (d.hotbar.prevKey = 'left'), /hotbar\.prevKey must be a KeyboardEvent\.code/],
    [(d) => (d.hotbar.nextKey = d.hotbar.prevKey), /hotbar\.prevKey and hotbar\.nextKey must differ/],
    [(d) => (d.hotbar.wheelNotchPx = 0), /hotbar\.wheelNotchPx must be 1\.\.1000/],
    [(d) => (d.hotbar.wheelGapMs = 1.5), /hotbar\.wheelGapMs must be 0\.\.2000/],
    [(d) => (d.hotbar.extra = 1), /hotbar: unknown extra/],
    [(d) => (d.buildModes.key = 'l'), /buildModes\.key must be a KeyboardEvent\.code/],
    [(d) => (d.buildModes.modes = []), /buildModes\.modes must be a non-empty list/],
    [(d) => (d.buildModes.clearance = 2), /buildModes\.clearance must be 0\.\.0\.5/],
    [(d) => (d.buildModes.holdMs = -1), /buildModes\.holdMs must be 0\.\.2000/],
    [(d) => d.items.push({ name: 'test:saw', label: 'Saw', category: 'tools', tool: 'test:saw' }), /items\[\d+\]\.tool "test:saw" is not a tool of shared\/tools\/tools\.json/],
    [(d) => d.items.push({ name: 'test:axe2', label: 'Axe', category: 'tools', tool: 'dwell:stone_axe', block: 'dwell:stone' }), /items\[\d+\]: give block or tool \(not both\)/],
    [(d) => d.items.push({ name: 'test:axe3', label: 'Axe', category: 'tools', tool: 'dwell:stone_axe', rows: 1 }), /items\[\d+\]: a tool places nothing \(rows\)/],
  ];
  for (const [change, message] of cases) {
    const d = loadItems();
    change(d);
    assert.throws(() => validateItems(d, blocks), message);
  }
});

test('a slope item added by data alone is generated with its state, facing and upper cell', () => {
  const d = loadItems();
  d.items.push({
    name: 'dwell:stone_ramp_away',
    label: 'Stone ramp (away)', category: 'shapes',
    block: 'dwell:stone_ramp',
    rows: 2,
    state: { flooded: 'false', half: 'top', shape: 'steep_lower' },
    upper: { shape: 'steep_upper' },
    facing: 'away',
  });
  const rs = itemsRust(validateItems(d, blocks));
  assert.match(
    rs,
    /name: "dwell:stone_ramp_away", .*state: &\[\("flooded", "false"\), \("half", "top"\), \("shape", "steep_lower"\)\], facing: ItemFacing::Away, upper: &\[\("shape", "steep_upper"\)\]/,
  );
});

test('a tool item added by data alone is emitted (a fixture tool of the tools data)', () => {
  const d = loadItems();
  d.items.push({ name: 'test:flint_axe', label: 'Flint axe', category: 'tools', tool: 'test:flint_axe' });
  const tools = new Set([...toolNames(), 'test:flint_axe']);
  const rs = itemsRust(validateItems(d, blocks, undefined, tools));
  assert.match(rs, /ITEMS: \[ItemDef; 27\]/);
  assert.match(rs, /name: "test:flint_axe", label: "Flint axe", block: "", tool: Some\("test:flint_axe"\)/);
});

test('the creative inventory view: every item in a group, a fixture item and group by data alone', () => {
  const d = loadItems();
  d.inventory.categories.push({ name: 'test_group', label: 'Test group' });
  d.items.push({ name: 'test:brick', label: 'Brick', category: 'test_group', block: 'dwell:stone', rows: 1 });
  const ts = buildModesTs(validateItems(d, blocks));
  assert.match(ts, /\{ name: 'test_group', label: 'Test group' \}/);
  assert.match(ts, /\{ name: 'test:brick', label: 'Brick', category: 'test_group' \}/);
  for (const it of loadItems().items) assert.ok(ts.includes(`name: '${it.name}'`), it.name);
  assert.match(
    itemsRust(validateItems(loadItems(), blocks)),
    /CREATIVE_HOTBAR: \[&str; 6\] = \["dwell:stone", "dwell:dirt", "dwell:grass", "dwell:log", "dwell:stone_axe", "dwell:stone_pickaxe"\]/,
  );
  const bad = [
    [(x) => delete x.items[0].category, /items\[0\]\.category undefined must be one of inventory\.categories/],
    [(x) => x.inventory.hotbar.pop(), /inventory\.hotbar must list 6 items/],
    [(x) => (x.inventory.hotbar[0] = 'test:nothing'), /inventory\.hotbar\[0\] "test:nothing" is not an item above/],
  ];
  for (const [edit, message] of bad) {
    const x = loadItems();
    edit(x);
    assert.throws(() => validateItems(x, blocks), message);
  }
});
