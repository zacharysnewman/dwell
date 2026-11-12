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
  assert.match(rs, /held_vertical: Some\("stone_axe_vertical"\), swing: SwingDef \{ windup_s: 0\.3, active_s: 0\.12, hit_stop_s: 0\.08/);
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
  bad((d) => ((d.tools[0].swing.windupS = 0), (d.tools[0].swing.recoverMissS = 0.25)), /a miss \(0\.370 s\) must last at least swingSeconds/);
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

test('breaking: the right tool class per material and a speed per tier reach the Rust table', () => {
  const d = validateTools(...fresh());
  const cls = (b) => d.breaking.classes.find((c) => c.block === b)?.class;
  assert.equal(cls('dwell:stone'), 'pickaxe');
  assert.equal(cls('dwell:dirt'), 'shovel');
  assert.equal(cls('dwell:sand'), 'shovel');
  assert.equal(cls('dwell:log'), 'axe');
  const speed = (i) => d.breaking.tools.find((t) => t.item === i).speed;
  assert.ok(speed('dwell:metal_pickaxe') > speed('dwell:stone_pickaxe') && speed('dwell:stone_pickaxe') > 1);
  const rs = toolsRust(d);
  assert.match(rs, /\("dwell:stone", "pickaxe"\)/);
  assert.match(rs, /BreakTool \{ item: "dwell:metal_shovel", class: "shovel", tier: "metal", speed: 2\.5 \}/);
});

test('breaking: a fixture tool class and tier by data alone, and bad entries fail naming them', () => {
  const [data, items, blocks] = fresh();
  items.add('test:trowel');
  blocks.set('test:clay', {});
  data.breaking.tiers.bronze = 2;
  data.breaking.classes.trowel = ['test:clay'];
  data.breaking.tools.push({ item: 'test:trowel', class: 'trowel', tier: 'bronze' });
  assert.match(toolsRust(validateTools(data, items, blocks)), /BreakTool \{ item: "test:trowel", class: "trowel", tier: "bronze", speed: 2\.0 \}/);
  const bad = (edit, re) => {
    const [d, i, b] = fresh();
    edit(d.breaking);
    assert.throws(() => validateTools(d, i, b), re);
  };
  bad((b) => b.tools.push({ item: 'test:nope', class: 'axe', tier: 'stone' }), /no item "test:nope"/);
  bad((b) => (b.tools[0].class = 'hammer'), /unknown class "hammer" \(pickaxe, shovel, axe\)/);
  bad((b) => (b.tools[0].tier = 'gold'), /unknown tier "gold" \(stone, metal\)/);
  bad((b) => b.classes.axe.push('dwell:stone'), /dwell:stone is in both pickaxe and axe/);
  bad((b) => b.classes.axe.push('dwell:nope'), /breaking\.classes\.axe: no block "dwell:nope"/);
  bad((b) => (b.tiers.stone = 0), /breaking\.tiers\.stone is a speed above 0/);
  bad((b) => b.tools.push({ ...b.tools[0] }), /dwell:stone_axe is listed twice/);
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

test("the axe clears leaves and the stump's wood ramp; a fixture tool clearing another block by data alone is emitted", () => {
  const d = validateTools(...fresh());
  const axe = d.tools.find((t) => t.name === 'dwell:stone_axe');
  assert.deepEqual(axe.clears, ['dwell:leaves', 'dwell:snowy_leaves', 'dwell:log_ramp']);
  assert.match(toolsRust(d), /rustle: Some\("axe_rustle"\), rustle_leaves: 5 \}, clears: &\["dwell:leaves", "dwell:snowy_leaves", "dwell:log_ramp"\], chop2_bite: [\d.]+ \}/);
  const [data, items, blocks] = fresh();
  items.add('test:sickle');
  data.tools.push({ name: 'test:sickle', kind: 'axe', swingSeconds: 0.3, biteM: 0.1, bindM: 0.1, across: 1, along: 0.3, sharpness: 1, clears: ['dwell:tall_grass'] });
  const rs = toolsRust(validateTools(data, items, blocks));
  assert.match(rs, /name: "test:sickle".*rustle: None, rustle_leaves: 4 \}, clears: &\["dwell:tall_grass"\], chop2_bite: [\d.]+ \}/);
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
  const rows = loadTools();
  rows.woods[0].fellWedgeTrunk = 'flat';
  assert.throws(() => validateTools(rows, items, blocks), /woods\.dwell:log\.fellWedgeTrunk must be one of open, matching/);
  // The trunk's tip (owner, 2026-11-09: "like we used to") is data: validated, 0 by default.
  rows.woods[0].fellWedgeTrunk = 'open';
  rows.woods[0].fellTip = -1;
  assert.throws(() => validateTools(rows, items, blocks), /woods\.dwell:log\.fellTip must be a number in \[0, 2\]/);
  const plain = loadTools();
  delete plain.woods[0].fellTip;
  delete plain.woods[0].fellWedgeTrunk;
  assert.match(toolsRust(validateTools(plain, items, blocks)), /fell_wedge_matching: true, fell_tip: 0\.0, chop2: Chop2Wood \{[^}]*\} \}/);
  assert.match(toolsRust(validateTools(loadTools(), items, blocks)), /block: "dwell:log", .*fell_tip: 1\.0, chop2: Chop2Wood \{[^}]*\} \}/);
});

test("block damage: the data's hits reach Rust and TS; a fixture block by data alone; bad entries fail naming them", () => {
  const [data, items, blocks] = fresh();
  const d = validateTools(data, items, blocks);
  assert.match(toolsRust(d), /BlockDamageDef \{ block: "dwell:stone", default: None, tools: &\[\("pickaxe", 7\)\], look: Some\(DamageLook \{ kind: DamageLookKind::Shards,/);
  assert.match(toolsTs(d), /\{ block: 'dwell:dirt', default: 11, tools: \{ shovel: 7 \} \},/);
  blocks.set('test:clay', { damage: { default: 9, shovel: 4, 'dwell:metal_shovel': 1 } });
  assert.match(
    toolsRust(validateTools(data, items, blocks)),
    /BlockDamageDef \{ block: "test:clay", default: Some\(9\), tools: &\[\("shovel", 4\), \("dwell:metal_shovel", 1\)\], look: None, chips: None \},/,
  );
  const bad = (damage, re) => {
    blocks.set('test:clay', { damage });
    assert.throws(() => validateTools(data, items, blocks), re);
  };
  bad({ shovel: 4 }, /test:clay\.damage needs a default/);
  bad({ default: 0 }, /test:clay\.damage\.default is a hit count from 1 to 255, not 0/);
  bad({ default: null, spoon: 3 }, /test:clay\.damage\.spoon: no tool class or item "spoon"/);
  bad({ default: 2, shovel: 1.5 }, /test:clay\.damage\.shovel is a hit count/);
  bad([], /test:clay\.damage must be an object/);
  // Its look (D2) by data alone, and a bad one fails naming it.
  blocks.set('test:clay', { damage: { default: 3, look: { kind: 'scoops', size: 0.3, color: '#806040', crack: '#302010' } } });
  assert.match(
    toolsRust(validateTools(data, items, blocks)),
    /block: "test:clay", .*look: Some\(DamageLook \{ kind: DamageLookKind::Scoops, size: 0\.3, color: \[128, 96, 64\], crack: \[48, 32, 16\], depth: 0\.1, rim: 0\.012, parallax: 6, darken: 0\.35, profile: CarveProfile::Bowl, facets: 5, grid: 8, triangles: 256, cracks: None \}\)/,
  );
  bad({ default: 3, look: { kind: 'dents', size: 0.3, color: '#806040', crack: '#302010' } }, /test:clay\.damage\.look\.kind must be one of shards, scoops, chips/);
  bad({ default: 3, look: { kind: 'scoops', size: 0.3, color: '#806040', crack: '#302010', depth: 0.9 } }, /test:clay\.damage\.look\.depth must be a number in \[0, 0\.5\]/);
  // A scoop's per-pixel relief is data (BLOCK_DAMAGE.md §6.1): a fixture's rim, march and damp floor.
  bad({ default: 3, look: { kind: 'scoops', size: 0.3, color: '#806040', crack: '#302010', rim: 0.2 } }, /test:clay\.damage\.look\.rim must be a number in \[0, 0\.03\]/);
  bad({ default: 3, look: { kind: 'scoops', size: 0.3, color: '#806040', crack: '#302010', parallax: 2.5 } }, /test:clay\.damage\.look\.parallax must be a whole number of steps from 0 to 8/);
  bad({ default: 3, look: { kind: 'scoops', size: 0.3, color: '#806040', crack: '#302010', darken: -1 } }, /test:clay\.damage\.look\.darken must be a number in \[0, 1\]/);
  // The carved mesh's data (§6.2): a fixture's facets by data alone, bad values failing by name.
  blocks.set('test:clay', { damage: { default: 3, look: { kind: 'chips', size: 0.3, color: '#806040', crack: '#302010', profile: 'facets', facets: 7, grid: 6, triangles: 160 } } });
  assert.match(toolsRust(validateTools(data, items, blocks)), /block: "test:clay", .*profile: CarveProfile::Facets, facets: 7, grid: 6, triangles: 160, cracks: None \}\)/);
  bad({ default: 3, look: { kind: 'scoops', size: 0.3, color: '#806040', crack: '#302010', profile: 'spiky' } }, /test:clay\.damage\.look\.profile must be one of bowl, facets/);
  bad({ default: 3, look: { kind: 'scoops', size: 0.3, color: '#806040', crack: '#302010', facets: 2 } }, /test:clay\.damage\.look\.facets must be a whole number from 3 to 8/);
  bad({ default: 3, look: { kind: 'scoops', size: 0.3, color: '#806040', crack: '#302010', grid: 40 } }, /test:clay\.damage\.look\.grid must be a whole number from 2 to 16/);
  bad({ default: 3, look: { kind: 'scoops', size: 0.3, color: '#806040', crack: '#302010', triangles: 64 } }, /test:clay\.damage\.look\.triangles must be a whole number from 128 to 2048/);
  bad({ default: 3, look: { kind: 'shards', size: 2, color: '#806040', crack: '#302010' } }, /test:clay\.damage\.look\.size must be a number/);
  bad({ default: 3, look: { kind: 'shards', size: 0.3, color: 'red', crack: '#302010' } }, /test:clay\.damage\.look\.color must be a #rrggbb colour/);
  // The crack overlay (BLOCK_DAMAGE.md §6.3): a fixture's set, stages and strength by data alone,
  // resolved to the set's tiles; bad ones fail naming them.
  const look = { kind: 'scoops', size: 0.3, color: '#806040', crack: '#302010' };
  blocks.set('test:clay', { damage: { default: 3, look: { ...look, cracks: { set: 'wood', stages: [0.1, 0.3, 0.5, 0.9], strength: 0.4 } } } });
  assert.match(
    toolsRust(validateTools(data, items, blocks)),
    /block: "test:clay", .*cracks: Some\(Cracks \{ tiles: &\["crackWood1", "crackWood2", "crackWood3", "crackWood4"\], stages: &\[0\.1, 0\.3, 0\.5, 0\.9\], strength: 0\.4, span: 1\.5 \}\) \}\)/,
  );
  assert.match(toolsRust(d), /block: "dwell:stone", .*cracks: Some\(Cracks \{ tiles: &\["crackStone1",/);
  bad({ default: 3, look: { ...look, cracks: { set: 'glass', stages: [0.5], strength: 0.4 } } }, /test:clay\.damage\.look\.cracks\.set must be a crack set of shared\/textures\/cracks\.json/);
  bad({ default: 3, look: { ...look, cracks: { set: 'wood', stages: [0.5], strength: 0.4 } } }, /test:clay\.damage\.look\.cracks\.stages must list 4 damage fractions/);
  bad({ default: 3, look: { ...look, cracks: { set: 'wood', stages: [0.1, 0.5, 0.4, 0.9], strength: 0.4 } } }, /test:clay\.damage\.look\.cracks\.stages must ascend/);
  bad({ default: 3, look: { ...look, cracks: { set: 'wood', stages: [0, 0.5, 0.6, 0.9], strength: 0.4 } } }, /test:clay\.damage\.look\.cracks\.stages\[0\] must be a damage fraction in \(0, 1\]/);
  bad({ default: 3, look: { ...look, cracks: { set: 'wood', stages: [0.1, 0.3, 0.5, 0.9], strength: 2 } } }, /test:clay\.damage\.look\.cracks\.strength must be a number in \[0, 1\]/);
  bad({ default: 3, look: { ...look, cracks: { set: 'wood', stages: [0.1, 0.3, 0.5, 0.9], strength: 0.4, glow: 1 } } }, /test:clay\.damage\.look\.cracks/);
  // Its hit's chips (sounds round 2b) by data alone, and a bad one fails naming it.
  blocks.set('test:clay', { damage: { default: 3, chips: { count: 5, color: '#806040', speed: 1.5, size: 0.05 } } });
  assert.match(
    toolsRust(validateTools(data, items, blocks)),
    /block: "test:clay", .*chips: Some\(Chips \{ count: 5, color: \[128, 96, 64\], speed: 1\.5, size: 0\.05 \}\)/,
  );
  bad({ default: 3, chips: { count: 5, color: 'brown' } }, /test:clay\.damage\.chips\.color must be #rrggbb/);
  bad({ default: 3, chips: { count: 99 } }, /test:clay\.damage\.chips\.count must be a whole number/);
});

test('durability: the stone axe, stone pickaxe and wooden shovel survive 32 hits, creative does not wear, a fixture tool by data alone', () => {
  const [tools, items, blocks] = fresh();
  const d = validateTools(tools, items, blocks);
  assert.deepEqual(d.durability.items, [
    { item: 'dwell:stone_axe', hits: 32 },
    { item: 'dwell:stone_pickaxe', hits: 32 },
    { item: 'dwell:wooden_shovel', hits: 32 },
  ]);
  assert.equal(d.durability.creativeWear, false);
  assert.match(toolsRust(d), /pub static DURABILITY: &\[\(&str, u16\)\] = &\[\("dwell:stone_axe", 32\), \("dwell:stone_pickaxe", 32\), \("dwell:wooden_shovel", 32\)\];/);
  assert.match(toolsRust(d), /pub const CREATIVE_WEAR: bool = false;/);
  assert.match(toolsTs(d), /'dwell:stone_axe': 32,/);
  tools.durability.items['dwell:metal_pickaxe'] = 200;
  assert.match(toolsRust(validateTools(tools, items, blocks)), /\("dwell:metal_pickaxe", 200\)/);
  const bad = (edit, re) => {
    const [t, i, b] = fresh();
    edit(t.durability);
    assert.throws(() => validateTools(t, i, b), re);
  };
  bad((x) => (x.items['dwell:nothing'] = 3), /no item/);
  bad((x) => (x.items['dwell:stone_axe'] = 0), /hits must be/);
  bad((x) => (x.creativeWear = 'no'), /creativeWear/);
  bad((x) => (x.breakSound = 'nope'), /no sound event/);
});

test("a swing's camera sway (CHARACTERS.md §5.2): the axe's per kind, a fixture tool's by data alone, bad curves fail naming them", () => {
  const d = validateTools(...fresh());
  const rs = toolsRust(d);
  // The stone axe's level sway leans right into the wind-up and turns left through the strike.
  assert.match(rs, /name: "dwell:stone_axe".*camera_level: SwingCamera \{ keys: &\[CameraKey \{ t: 0\.0, yaw: 0\.0, .*CameraKey \{ t: 1\.0, yaw: 1\.8, pitch: 0\.5, roll: 1\.4/);
  assert.match(rs, /camera_vertical: SwingCamera \{ keys: &\[.*CameraKey \{ t: 1\.0, yaw: 0\.3, pitch: 2\.2/);
  // A fixture tool with its own sway by data alone; one without has none (no keys, no kick).
  const [data, items, blocks] = fresh();
  items.add('test:maul');
  items.add('test:club');
  const keys = [{ t: 0 }, { t: 1.2, pitch: 3, bob: [0, 0.02, 0] }, { t: 3 }];
  data.tools.push({ name: 'test:maul', kind: 'axe', swingSeconds: 0.45, biteM: 0.3, bindM: 0.1, across: 1, along: 0.2, sharpness: 2, swing: { camera: { vertical: { keys, kick: { pitch: -2, decayS: 0.2 } } } } });
  data.tools.push({ name: 'test:club', kind: 'axe', swingSeconds: 0.45, biteM: 0.3, bindM: 0.1, across: 1, along: 0.2, sharpness: 2 });
  const out = toolsRust(validateTools(data, items, blocks));
  assert.match(out, /name: "test:maul".*camera_level: SwingCamera \{ keys: &\[\], .*camera_vertical: SwingCamera \{ keys: &\[CameraKey \{ t: 0\.0, .*CameraKey \{ t: 1\.2, yaw: 0\.0, pitch: 3\.0, roll: 0\.0, bob: \[0\.0, 0\.02, 0\.0\] \}.*kick: CameraKick \{ offset: CameraOffset \{ yaw: 0\.0, pitch: -2\.0, .*decay_s: 0\.2, miss: 0\.4, bounce: 1\.6/);
  assert.match(out, /name: "test:club".*camera_level: SwingCamera \{ keys: &\[\], .*camera_vertical: SwingCamera \{ keys: &\[\],/);
  const bad = (edit, re) => {
    const [x, i, b] = fresh();
    edit(x.tools[0].swing.camera);
    assert.throws(() => validateTools(x, i, b), re);
  };
  bad((c) => (c.sideways = {}), /swing\.camera: unknown field sideways/);
  bad((c) => (c.level.keys[0].yaw = 1), /camera\.level\.keys must start at rest at t 0/);
  bad((c) => c.level.keys.pop(), /camera\.level\.keys must end at rest at t 3/);
  bad((c) => (c.level.keys[2].t = 0.5), /camera\.level\.keys\[2\]\.t \(0\.5\) must be after the key before \(1\)/);
  bad((c) => (c.vertical.keys[1].pitch = 40), /camera\.vertical\.keys\[1\]\.pitch must be a number in \[-10, 10\]/);
  bad((c) => (c.level.keys[1].bob = [0, 1]), /camera\.level\.keys\[1\]\.bob must be \[right, up, back\] metres/);
  bad((c) => (c.level.kick.decayS = 0), /camera\.level\.kick\.decayS must be a number in \(0, 1\]/);
  bad((c) => (c.level.kick.spin = 1), /camera\.level\.kick: unknown field spin/);
  bad((c) => (c.level.keys = [{ t: 0 }]), /camera\.level\.keys must be a list of at least two keys/);
});
