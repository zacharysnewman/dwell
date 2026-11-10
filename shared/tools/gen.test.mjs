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
  assert.match(toolsRust(d), /fell_wedge: Some\("dwell:log_ramp"\)/);
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
  assert.match(rs, /block: "dwell:log", .*chips: Chips \{ count: 7, color: \[217, 179, 122\]/);
  assert.match(rs, /\("dwell:log", ToolKind::Axe\),/);
});

test("a tool's swing, its two holds and a wood's leaves, sway and sounds are data", () => {
  const d = validateTools(...fresh());
  const rs = toolsRust(d);
  assert.match(rs, /held_vertical: Some\("stone_axe_vertical"\), swing: SwingDef \{ windup_s: 0\.12, active_s: 0\.14, hit_stop_s: 0\.08/);
  assert.match(rs, /stamina_cost: 8\.0, swish: Some\("axe_swish"\), bounce: Some\("axe_clank"\)/);
  assert.match(rs, /leaves: Leaves \{ block: Some\("dwell:leaves"\), count: 6/);
  assert.match(rs, /sway: Sway \{ from_fraction: 0\.4, max_deg: 3\.5 \}, sounds: &\[\(0\.0, "chop_bark"\), \(0\.2, "chop_wood"\), \(0\.4, "chop_creak"\)\]/);
});

test('a fixture tool with its own swing by data alone; a tool without one takes the defaults', () => {
  const [data, items, blocks] = fresh();
  items.add('test:maul');
  data.tools.push({ name: 'test:maul', kind: 'axe', swingSeconds: 0.9, biteM: 0.3, bindM: 0.1, across: 1, along: 0.2, sharpness: 2, swing: { windupS: 0.3, activeS: 0.2, recoverMissS: 0.6, arcDeg: 70, staminaCost: 20 } });
  const rs = toolsRust(validateTools(data, items, blocks));
  assert.match(rs, /name: "test:maul".*swing: SwingDef \{ windup_s: 0\.3, active_s: 0\.2, hit_stop_s: 0\.07, .*arc_deg: 70\.0, .*stamina_cost: 20\.0/);
});

test('a bad swing, leaves, sway or sound fails naming it', () => {
  const bad = (edit, re) => {
    const [data, items, blocks] = fresh();
    edit(data);
    assert.throws(() => validateTools(data, items, blocks), re);
  };
  bad((d) => (d.tools[0].swing.arcDeg = 400), /tools\.dwell:stone_axe\.swing\.arcDeg must be a number/);
  bad((d) => (d.tools[0].swing.recoverMissS = 0.1), /recoverMissS \(0\.1\) must not be below recoverHitS/);
  bad((d) => ((d.tools[0].swing.windupS = 0), (d.tools[0].swing.recoverMissS = 0.25)), /a miss \(0\.390 s\) must last at least swingSeconds/);
  bad((d) => (d.tools[0].swing.swish = 'nope'), /swing\.swish: no sound event "nope"/);
  bad((d) => (d.tools[0].swing.spin = 1), /swing: unknown field spin/);
  bad((d) => (d.tools[0].heldVertical = 'nope'), /heldVertical: no item "nope"/);
  bad((d) => (d.woods[0].leaves.block = 'dwell:nope'), /leaves\.block: no block named dwell:nope/);
  bad((d) => (d.woods[0].sway.fromFraction = 0.9), /sway\.fromFraction must be below fellFraction/);
  bad((d) => (d.woods[0].sounds[1].from = 0), /sounds: from must start at 0 and rise/);
  bad((d) => (d.woods[0].sounds[0].event = 'nope'), /sounds\[0\]\.event: no sound event "nope"/);
});

test('a block needing a tool kind by data alone (breakWith) is emitted, an unknown kind fails', () => {
  const [data, items, blocks] = fresh();
  blocks.set('test:hard_wood', { breakWith: 'axe' });
  assert.match(toolsRust(validateTools(data, items, blocks)), /\("test:hard_wood", ToolKind::Axe\),/);
  blocks.set('test:hard_wood', { breakWith: 'pick' });
  assert.throws(() => validateTools(data, items, blocks), /test:hard_wood\.breakWith must be one of axe/);
});

test("a wood's cut look (its carved faces, ADR 0067 §11) is validated and reaches the Rust table", () => {
  const rs = toolsRust(validateTools(...fresh()));
  assert.match(rs, /block: "dwell:log", .*cut: CutLook \{ tile: Some\("logCut"\), tint: \[255, 226, 184\], open: 2\.5, kerf_width_m: 0\.035 \}/);
  const bad = (edit, re) => {
    const [data, items, blocks] = fresh();
    edit(data);
    assert.throws(() => validateTools(data, items, blocks), re);
  };
  bad((d) => (d.woods[0].cut.tile = 'noTile'), /woods\.dwell:log\.cut\.tile: no atlas tile named noTile/);
  bad((d) => (d.woods[0].cut.tint = 'tan'), /woods\.dwell:log\.cut\.tint must be #rrggbb/);
  bad((d) => (d.woods[0].cut.open = 20), /cut.*open must be a number/);
  bad((d) => (d.woods[0].cut.depth = 1), /cut: unknown field depth/);
  // Without a cut look: the defaults (the end-grain tile, untinted).
  const [data, items, blocks] = fresh();
  delete data.woods[0].cut;
  assert.match(toolsRust(validateTools(data, items, blocks)), /cut: CutLook \{ tile: None, tint: \[255, 255, 255\], open: 2\.5, kerf_width_m: 0\.03 \}/);
});

test("the axe clears leaves; a fixture tool clearing another block by data alone is emitted", () => {
  const d = validateTools(...fresh());
  const axe = d.tools.find((t) => t.name === 'dwell:stone_axe');
  assert.deepEqual(axe.clears, ['dwell:leaves', 'dwell:snowy_leaves']);
  assert.match(toolsRust(d), /rustle: Some\("axe_rustle"\), rustle_leaves: 5 \}, clears: &\["dwell:leaves", "dwell:snowy_leaves"\] \}/);
  const [data, items, blocks] = fresh();
  items.add('test:sickle');
  data.tools.push({ name: 'test:sickle', kind: 'axe', swingSeconds: 0.3, biteM: 0.1, bindM: 0.1, across: 1, along: 0.3, sharpness: 1, clears: ['dwell:tall_grass'] });
  const rs = toolsRust(validateTools(data, items, blocks));
  assert.match(rs, /name: "test:sickle".*rustle: None, rustle_leaves: 4 \}, clears: &\["dwell:tall_grass"\] \}/);
  const bad = (edit, re) => {
    const [data, items, blocks] = fresh();
    edit(data);
    assert.throws(() => validateTools(data, items, blocks), re);
  };
  bad((d) => (d.tools[0].clears = ['dwell:nope']), /tools\.dwell:stone_axe\.clears\[0\]: no block named "dwell:nope"/);
  bad((d) => (d.tools[0].clears = 'dwell:leaves'), /clears must be a list of block names/);
  bad((d) => (d.tools[0].clears = ['dwell:leaves', 'dwell:leaves']), /clears: dwell:leaves listed twice/);
  bad((d) => (d.tools[0].clears = ['dwell:log']), /clears: dwell:log is a wood/);
  bad((d) => (d.tools[0].swing.rustle = 'nope'), /swing\.rustle: no sound event "nope"/);
  bad((d) => (d.tools[0].swing.rustleLeaves = 99), /rustleLeaves must be a whole number/);
});

test('a fixture wood with its own fell wedge by data alone; a bad wedge fails naming it', () => {
  const { items, blocks } = loadNames(true);
  const data = loadTools();
  data.woods.push({ ...data.woods[0], block: 'test:post', fellWedge: 'test:post_ramp', leaves: { block: null } });
  const d = validateTools(data, items, blocks);
  assert.match(toolsRust(d), /block: "test:post", .*fell_wedge: Some\("test:post_ramp"\)/);
  const bad = (wedge, re) => {
    const data = loadTools();
    data.woods[0].fellWedge = wedge;
    assert.throws(() => validateTools(data, items, blocks), re);
  };
  bad('dwell:nope', /woods\.dwell:log\.fellWedge: no block named "dwell:nope"/);
  bad('dwell:stone', /fellWedge: dwell:stone is not a ramp block/);
  bad('dwell:stone_ramp', /fellWedge: dwell:stone_ramp must be a carrier/);
});
