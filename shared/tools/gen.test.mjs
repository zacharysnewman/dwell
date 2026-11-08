// Tools and woods: validation and outputs (shared/tools/gen.mjs, ADR 0067).
import assert from 'node:assert/strict';
import test from 'node:test';
import { loadNames, loadTools, toolsRust, toolsTs, validateTools } from './gen.mjs';

const fresh = () => {
  const { items, blocks } = loadNames();
  return [loadTools(), items, blocks];
};

test("the build's data validates: the stone axe and the log", () => {
  const d = validateTools(...fresh());
  const axe = d.tools.find((t) => t.name === 'dwell:stone_axe');
  assert.equal(axe.kind, 'axe');
  assert.ok(axe.along < axe.across, 'cutting along the grain is worse');
  const log = d.woods.find((w) => w.block === 'dwell:log');
  assert.deepEqual(log.buckDrops, [{ item: 'dwell:wood', min: 2, max: 3, chance: 1 }]);
  assert.match(toolsRust(d), /ToolDef \{ name: "dwell:stone_axe", kind: ToolKind::Axe/);
  assert.match(toolsTs(d), /dwell:stone_axe/);
});

test('a fixture tool added by data alone is emitted', () => {
  const [data, items, blocks] = fresh();
  items.add('test:flint_axe');
  data.tools.push({ name: 'test:flint_axe', kind: 'axe', swingSeconds: 0.3, biteM: 0.2, bindM: 0.1, across: 1, along: 0.3, sharpness: 1 });
  const d = validateTools(data, items, blocks);
  assert.match(toolsRust(d), /name: "test:flint_axe", kind: ToolKind::Axe, held: None, swing_seconds: 0.3, bite_m: 0.2/);
  assert.match(toolsTs(d), /test:flint_axe/);
});

test('a bad entry fails naming it and the problem', () => {
  const bad = (edit, re) => {
    const [data, items, blocks] = fresh();
    edit(data);
    assert.throws(() => validateTools(data, items, blocks), re);
  };
  bad((d) => (d.tools[0].kind = 'saw'), /tools\.dwell:stone_axe\.kind must be one of axe/);
  bad((d) => (d.tools[0].name = 'dwell:nothing'), /tools\.dwell:nothing: no item of that name/);
  bad((d) => (d.tools[0].along = 2), /along must be a number/);
  bad((d) => (d.tools[0].along = 1, (d.tools[0].across = 0.5)), /along \(1\) must not be above across/);
  bad((d) => (d.tools[0].edge = 1), /unknown field edge/);
  bad((d) => (d.woods[0].block = 'dwell:stone'), /woods\.dwell:stone: the block has no axis property/);
  bad((d) => (d.woods[0].block = 'dwell:nope'), /woods\.dwell:nope: no block of that name/);
  bad((d) => (d.woods[0].buckDrops[0].item = 'dwell:nope'), /buckDrops\[0\]: no item named dwell:nope/);
  bad((d) => (d.woods[0].buckDrops[0].count = [3, 2]), /count must be/);
  bad((d) => (d.woods[0].fellFraction = 0), /fellFraction must be a number in \(0, 1\]/);
  bad((d) => (d.tools[0].held = 'no_such_item'), /tools\.dwell:stone_axe\.held: no item "no_such_item" in shared\/characters/);
  bad((d) => (d.woods[0].chips.color = 'brown'), /woods\.dwell:log\.chips\.color must be #rrggbb/);
  bad((d) => (d.woods[0].chips.count = 99), /chips\.count must be a whole number/);
  bad((d) => (d.woods[0].chips.spin = 1), /chips: unknown field spin/);
});

test("the axe's held item, the log's chips and its breakWith reach the Rust table", () => {
  const d = validateTools(...fresh());
  const rs = toolsRust(d);
  assert.match(rs, /name: "dwell:stone_axe", kind: ToolKind::Axe, held: Some\("stone_axe"\)/);
  assert.match(rs, /block: "dwell:log", .*chips: Chips \{ count: 7, color: \[201, 154, 98\]/);
  assert.match(rs, /\("dwell:log", ToolKind::Axe\),/);
});

test('a block needing a tool kind by data alone (breakWith) is emitted, an unknown kind fails', () => {
  const [data, items, blocks] = fresh();
  blocks.set('test:hard_wood', { breakWith: 'axe' });
  assert.match(toolsRust(validateTools(data, items, blocks)), /\("test:hard_wood", ToolKind::Axe\),/);
  blocks.set('test:hard_wood', { breakWith: 'pick' });
  assert.throws(() => validateTools(data, items, blocks), /test:hard_wood\.breakWith must be one of axe/);
});
