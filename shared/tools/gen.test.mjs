// Tools and woods: validation and outputs (shared/tools/gen.mjs, ADR 0067).
import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { hitsMd, loadNames, loadTools, toolsRust, toolsTs, validateTools } from './gen.mjs';

/** The build's tools with the chopping axes of fixtures/chop_axes.json put back first (the axes
 * are break tools in the build since 2026-11-14; the chopping vocabulary is tested on these), as
 * break tools without their own hold (a chopping tool's hold is its swing's). */
const chopData = () => {
  const data = loadTools();
  const fx = JSON.parse(readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'fixtures/chop_axes.json'), 'utf8'));
  data.tools = [...fx.tools, ...data.tools];
  data.hints = { ...fx.hints, ...data.hints };
  for (const b of data.breaking.tools) {
    if (fx.tools.some((t) => t.name === b.item)) {
      for (const k of ['contactS', 'repeatOnHold', 'repeatS', 'clip']) delete b[k];
    }
  }
  return data;
};

const fresh = () => {
  const { items, blocks } = loadNames();
  return [chopData(), items, blocks];
};

test("the build's axes are break tools as the pickaxes and shovels, not chopping tools", () => {
  const { items, blocks } = loadNames();
  const d = validateTools(loadTools(), items, blocks);
  const pick = d.breaking.tools.find((t) => t.item === 'dwell:stone_pickaxe');
  for (const axe of ['dwell:stone_axe', 'dwell:metal_axe']) {
    assert.ok(!d.tools.some((t) => t.name === axe), `${axe}: no chopping swing`);
    const b = d.breaking.tools.find((t) => t.item === axe);
    assert.equal(b.class, 'axe');
    assert.equal(b.contactS, pick.contactS, `${axe}: the pickaxe's contact`);
    assert.equal(b.repeatS, pick.repeatS, `${axe}: the pickaxe's repeat`);
  }
  assert.match(toolsRust(d), /item: "dwell:stone_axe", class: "axe", material: "stone", contact_s: 0\.26, repeat_s: Some\(0\.5\)/);
});

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

test("the axe's held item and the log's chips reach the Rust table; the log needs no tool (by hand: 48 hits, no drops)", () => {
  const d = validateTools(...fresh());
  const rs = toolsRust(d);
  assert.match(rs, /name: "dwell:stone_axe", kind: ToolKind::Axe, held: Some\("stone_axe"\)/);
  // Owner, 2026-11-17: hit particles half their size; wood's pale like the wood inside, not bark.
  assert.match(rs, /block: "dwell:log", .*chips: Chips \{ count: 7, shards: 3, piece: "dwell:wood", color: \[239, 216, 176\], speed: 2\.6, size: 0\.025 \}/);
  assert.doesNotMatch(rs, /\("dwell:log", "axe"\),/);
  assert.match(rs, /BlockDamageDef \{ block: "dwell:log", volume: 1\.0, default: Some\(48\), .*barren: &\["hand"\]/);
});

test("a tool's swing, its holds and a wood's leaves, sway and sounds are data", () => {
  const d = validateTools(...fresh());
  const rs = toolsRust(d);
  // Owner, 2026-11-14: the axe has one mode, like the pickaxe and shovel: no second hold. The
  // sword keeps its two stances.
  assert.match(rs, /name: "dwell:stone_axe", [^\n]*held_vertical: None, swing: SwingDef \{ windup_s: 0\.3, active_s: 0\.12, hit_stop_s: 0\.08/);
  assert.match(rs, /name: "dwell:sword", [^\n]*held_vertical: Some\("sword_vertical"\)/);
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

test('a block needing a break tool class by data alone (breakWith) is emitted, an unknown class fails', () => {
  const [data, items, blocks] = fresh();
  blocks.set('test:hard_rock', { breakWith: 'pickaxe' });
  assert.match(toolsRust(validateTools(data, items, blocks)), /\("test:hard_rock", "pickaxe"\),/);
  blocks.set('test:hard_rock', { breakWith: 'pick' });
  assert.throws(() => validateTools(data, items, blocks), /test:hard_rock\.breakWith must be one of shovel, axe, pickaxe/);
});

test("breaking: the owner's sheet (2026-11-17): hits = ceil(base × proficiency × material), each tool's wear, the table for review", () => {
  const d = validateTools(...fresh());
  const row = (b) => d.damage.find((x) => x.block === b);
  const hit = (b, item) => row(b).tools.find((t) => t.tool === item).hits;
  assert.equal(row('dwell:dirt').default, 16);
  assert.equal(hit('dwell:dirt', 'dwell:wooden_shovel'), 6);
  assert.equal(hit('dwell:dirt', 'dwell:metal_shovel'), 2);
  assert.equal(row('dwell:stone').default, 48);
  assert.equal(hit('dwell:stone', 'dwell:stone_pickaxe'), 6);
  assert.equal(hit('dwell:log', 'dwell:metal_axe'), 3);
  assert.equal(hit('dwell:thatch', 'dwell:stone_axe'), 3, '6 × 0.5 × 1 = 3');
  assert.deepEqual(row('dwell:stone').barren, ['hand']);
  // Fixed hits: door, ladder, dynamite 2 and shaking; torch and tall grass 1.
  assert.equal(row('dwell:door').default, 2);
  assert.ok(row('dwell:door').shake);
  assert.ok(row('dwell:door').tools.every((t) => t.hits === 2));
  assert.equal(row('dwell:torch').default, 1);
  // The planned wood tools are in the table, not the build.
  const md = hitsMd(d);
  assert.match(md, /\| dwell:dirt \| 8 \| 16 \| 6 \| 4 \| 2 \|/);
  assert.match(md, /wooden_pickaxe \(planned\)/);
  assert.match(md, /\| dwell:stone_shovel \| shovel \| stone \| 256 \|/);
  assert.doesNotMatch(toolsRust(d), /wooden_pickaxe/);
});

test('breaking: a fixture class, material and block by data alone, and bad entries fail naming them', () => {
  const [data, items, blocks] = fresh();
  items.add('test:trowel');
  blocks.set('test:clay', { damage: { baseHits: 10, proficiency: { trowel: 'proficient', hand: 'inefficient' } } });
  data.breaking.materials.bronze = { hits: 0.8, durability: 1.5 };
  data.breaking.classes.trowel = { durability: { baseBlocks: 10, block: 'test:clay', material: 'stone' } };
  data.breaking.tools.push({ item: 'test:trowel', class: 'trowel', material: 'bronze', contactS: 0.4 });
  const d = validateTools(data, items, blocks);
  const rs = toolsRust(d);
  assert.match(rs, /BreakTool \{ item: "test:trowel", class: "trowel", material: "bronze", contact_s: 0\.4, repeat_s: Some\(0\.5\), clip: "break" \}/);
  // clay: 10 × 0.5 × 0.8 = 4 with the trowel; by hand 10 × 2 × 2 = 40, nothing dropped.
  assert.match(rs, /block: "test:clay", volume: 1\.0, default: Some\(40\), tools: &\[.*\("test:trowel", 4\)\], barren: &\["hand"\], shake: false/);
  // Its wear: 10 blocks × clay's 5 stone-trowel hits × bronze 1.5 = 75.
  assert.match(rs, /\("test:trowel", 75\)/);
  const bad = (edit, re) => {
    const [d2, i2, b2] = fresh();
    edit(d2.breaking, b2);
    assert.throws(() => validateTools(d2, i2, b2), re);
  };
  bad((b) => b.tools.push({ item: 'test:nope', class: 'axe', material: 'stone' }), /no item "test:nope"/);
  bad((b) => (b.tools[0].class = 'hammer'), /unknown class "hammer" \(shovel, axe, pickaxe\)/);
  bad((b) => (b.tools[0].material = 'gold'), /unknown material "gold" \(body, wood, stone, metal\)/);
  bad((b) => (b.tools[0].material = 'body'), /material body has no durability multiplier/);
  bad((b) => (b.materials.stone.hits = 0), /breaking\.materials\.stone\.hits is a hits multiplier/);
  bad((b) => delete b.proficiency.inefficient, /breaking\.proficiency\.inefficient is required/);
  bad((b) => (b.classes.axe.durability.block = 'dwell:nope'), /breaking\.classes\.axe\.durability\.block: no block "dwell:nope"/);
  bad((b) => (b.hand.material = 'skin'), /breaking\.hand\.material: no material "skin"/);
  bad((b) => b.tools.push({ ...b.tools[0] }), /dwell:wooden_shovel is listed twice/);
  bad((b) => (b.tools[0].contactS = -1), /breaking\.tools\[0\]: contactS is seconds in \[0, 2\]/);
  bad((b) => (b.tools[0].contactS = 0.9), /contactS 0\.9 is after the swing pace/);
  bad((b) => b.planned.push({ item: 'dwell:stone_axe', class: 'axe', material: 'stone' }), /dwell:stone_axe is an item now/);
  bad((b, bl) => bl.set('test:clay', { damage: { baseHits: 5, proficiency: { spoon: 'proficient' } } }), /test:clay\.damage\.proficiency\.spoon: no class "spoon"/);
  bad((b, bl) => bl.set('test:clay', { damage: { baseHits: 5, proficiency: { shovel: 'great' } } }), /test:clay\.damage\.proficiency\.shovel: no level "great"/);
  bad((b, bl) => bl.set('test:clay', { damage: { baseHits: 0 } }), /test:clay\.damage\.baseHits is a number of hits/);
  bad((b, bl) => bl.set('test:clay', { damage: { fixedHits: 2, baseHits: 3 } }), /fixedHits takes no baseHits or proficiency/);
  bad((b, bl) => bl.set('test:clay', { damage: { fixedHits: 1.5 } }), /fixedHits is a whole number/);
  bad((b, bl) => bl.set('test:clay', { damage: { fixedHits: 2, hit: 'wobble' } }), /hit must be one of marks, shake/);
  bad((b, bl) => bl.set('test:clay', { damage: { default: 3 } }), /test:clay\.damage: unknown/);
});

test("a wood's cut look (its carved faces, ADR 0067 §11) is validated and reaches the Rust table", () => {
  const rs = toolsRust(validateTools(...fresh()));
  assert.match(rs, /block: "dwell:log", .*cut: CutLook \{ tile: Some\("logCut"\), tint: \[255, 226, 184\], open: 2\.5, kerf_width_m: 0\.035, bite: BiteLook \{ width_m: 0\.14, ends_m: 0\.03, step: 0\.6, band_m: 0\.25, depth_per_mouth: 2\.0, width_per_depth: 1\.0 \}, cracks: Some\(Cracks \{ tiles: &\["crackWood1", "crackWood2", "crackWood3", "crackWood4"\], stages: &\[0\.15, 0\.4, 0\.65, 0\.85\], strength: 0\.85, span: 1\.5 \}\) \}/);
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
  assert.match(toolsRust(validateTools(data, items, blocks)), /cut: CutLook \{ tile: None, tint: \[255, 255, 255\], open: 2\.5, kerf_width_m: 0\.03, bite: BiteLook \{ width_m: 0\.14, ends_m: 0\.03, step: 0\.6, band_m: 0\.25, depth_per_mouth: 2\.0, width_per_depth: 1\.0 \}, cracks: None \}/);
});

test("a wood's bites and their cracks (WORK_QUEUE item 26) are data: validated, a fixture wood's by data alone", () => {
  const { items, blocks } = loadNames(true);
  const data = chopData();
  data.woods.push({
    ...data.woods[0],
    block: 'test:post',
    fellWedge: 'test:post_ramp',
    leaves: { block: null },
    cut: { open: 3, bite: { widthM: 0.2, endsM: 0.05, step: 0.5, bandM: 0.3, depthPerMouth: 3 }, cracks: { set: 'stone', stages: [0.1, 0.3, 0.5, 0.7], strength: 0.4 } },
  });
  assert.match(
    toolsRust(validateTools(data, items, blocks)),
    /block: "test:post", .*bite: BiteLook \{ width_m: 0\.2, ends_m: 0\.05, step: 0\.5, band_m: 0\.3, depth_per_mouth: 3\.0, width_per_depth: 1\.0 \}, cracks: Some\(Cracks \{ tiles: &\["crackStone1", "crackStone2", "crackStone3", "crackStone4"\], stages: &\[0\.1, 0\.3, 0\.5, 0\.7\], strength: 0\.4, span: 1\.5 \}\) \}/,
  );
  const bad = (edit, re) => {
    const data = chopData();
    edit(data.woods[0].cut);
    assert.throws(() => validateTools(data, items, blocks), re);
  };
  bad((c) => (c.bite.widthM = 2), /woods\.dwell:log\.cut\.bite\.widthM must be a number in \[0\.02, 1\]/);
  bad((c) => (c.bite.endsM = 0.1), /woods\.dwell:log\.cut\.bite\.endsM must be a number in \[0, 0\.07\]/);
  bad((c) => (c.bite.step = 0), /woods\.dwell:log\.cut\.bite\.step must be a number in \[0\.1, 1\]/);
  bad((c) => (c.bite.depthPerMouth = 0.1), /woods\.dwell:log\.cut\.bite\.depthPerMouth must be a number in \[0\.5, 8\]/);
  bad((c) => (c.bite.bandM = 0), /woods\.dwell:log\.cut\.bite\.bandM must be a number in \[0\.02, 1\]/);
  bad((c) => (c.bite.depth = 1), /cut\.bite: unknown field depth/);
  bad((c) => (c.bite = 3), /woods\.dwell:log\.cut\.bite must be an object/);
  bad((c) => (c.cracks.set = 'bark'), /woods\.dwell:log\.cut\.cracks\.set must be a crack set/);
  bad((c) => (c.cracks.stages = [0.5, 0.2, 0.6, 0.9]), /cut\.cracks\.stages must ascend/);
});

test("the axe clears leaves, not the stump's wood ramp (chopped); a fixture tool clearing another block by data alone is emitted", () => {
  const d = validateTools(...fresh());
  const axe = d.tools.find((t) => t.name === 'dwell:stone_axe');
  assert.deepEqual(axe.clears, ['dwell:leaves', 'dwell:snowy_leaves']);
  assert.match(toolsRust(d), /rustle: Some\("axe_rustle"\), rustle_leaves: 5 \}, clears: &\["dwell:leaves", "dwell:snowy_leaves"\], chop2_bite: [\d.]+, weapon: None, repeat_on_hold: true, clip_level/);
  const [data, items, blocks] = fresh();
  items.add('test:sickle');
  data.tools.push({ name: 'test:sickle', kind: 'axe', swingSeconds: 0.3, biteM: 0.1, bindM: 0.1, across: 1, along: 0.3, sharpness: 1, clears: ['dwell:tall_grass'] });
  const rs = toolsRust(validateTools(data, items, blocks));
  assert.match(rs, /name: "test:sickle".*rustle: None, rustle_leaves: 4 \}, clears: &\["dwell:tall_grass"\], chop2_bite: [\d.]+, weapon: None, repeat_on_hold: false, clip_level/);
  const bad = (edit, re) => {
    const [data, items, blocks] = fresh();
    edit(data);
    assert.throws(() => validateTools(data, items, blocks), re);
  };
  bad((d) => (d.tools[0].clears = ['dwell:nope']), /tools\.dwell:stone_axe\.clears\[0\]: no block named "dwell:nope"/);
  bad((d) => (d.tools[0].clears = 'dwell:leaves'), /clears must be a list of block names/);
  bad((d) => (d.tools[0].clears = ['dwell:leaves', 'dwell:leaves']), /clears: dwell:leaves listed twice/);
  bad((d) => (d.tools[0].clears = ['dwell:log']), /clears: dwell:log is a wood/);
  // WORK_QUEUE item 26: the stump keeps its gash, a fell wedge is chopped, not cleared.
  bad((d) => (d.tools[0].clears = ['dwell:leaves', 'dwell:log_ramp']), /clears: dwell:log_ramp is a wood's fellWedge/);
  bad((d) => (d.tools[0].swing.rustle = 'nope'), /swing\.rustle: no sound event "nope"/);
  bad((d) => (d.tools[0].swing.rustleLeaves = 99), /rustleLeaves must be a whole number/);
});

test('a fixture wood with its own fell wedge by data alone; a bad wedge fails naming it', () => {
  const { items, blocks } = loadNames(true);
  const data = chopData();
  data.woods.push({ ...data.woods[0], block: 'test:post', fellWedge: 'test:post_ramp', leaves: { block: null } });
  const d = validateTools(data, items, blocks);
  assert.match(toolsRust(d), /block: "test:post", .*fell_wedge: Some\("test:post_ramp"\)/);
  const bad = (wedge, re) => {
    const data = chopData();
    data.woods[0].fellWedge = wedge;
    assert.throws(() => validateTools(data, items, blocks), re);
  };
  bad('dwell:nope', /woods\.dwell:log\.fellWedge: no block named "dwell:nope"/);
  bad('dwell:stone', /fellWedge: dwell:stone is not a ramp block/);
  bad('dwell:stone_ramp', /fellWedge: dwell:stone_ramp must be a carrier/);
  const rows = chopData();
  rows.woods[0].fellWedgeTrunk = 'flat';
  assert.throws(() => validateTools(rows, items, blocks), /woods\.dwell:log\.fellWedgeTrunk must be one of open, matching/);
  // The trunk's tip (owner, 2026-11-09: "like we used to") is data: validated, 0 by default.
  rows.woods[0].fellWedgeTrunk = 'open';
  rows.woods[0].fellTip = -1;
  assert.throws(() => validateTools(rows, items, blocks), /woods\.dwell:log\.fellTip must be a number in \[0, 2\]/);
  const plain = chopData();
  delete plain.woods[0].fellTip;
  delete plain.woods[0].fellWedgeTrunk;
  assert.match(toolsRust(validateTools(plain, items, blocks)), /fell_wedge_matching: true, fell_tip: 0\.0, chop2: Chop2Wood \{[^}]*\} \}/);
  assert.match(toolsRust(validateTools(chopData(), items, blocks)), /block: "dwell:log", .*fell_tip: 1\.0, chop2: Chop2Wood \{[^}]*\} \}/);
});

test("block damage: the data's hits reach Rust and TS; a fixture block by data alone; bad entries fail naming them", () => {
  const [data, items, blocks] = fresh();
  const d = validateTools(data, items, blocks);
  // The bite rule's volume from the shape (BLOCK_DAMAGE.md §10): a stored ramp is half a cell.
  assert.match(toolsRust(d), /BlockDamageDef \{ block: "dwell:stone_ramp", volume: 0\.5,/);
  assert.match(toolsRust(d), /BlockDamageDef \{ block: "dwell:stone", volume: 1\.0, default: Some\(48\), tools: &\[.*\("dwell:stone_pickaxe", 6\).*\], barren: &\["hand"\], shake: false, look: Some\(DamageLook \{ kind: DamageLookKind::Shards,/);
  assert.match(toolsTs(d), /\{ block: 'dwell:dirt', default: 16, tools: \{ .*'dwell:stone_shovel': 4,/);
  blocks.set('test:clay', { damage: { baseHits: 9, proficiency: { shovel: 'proficient' } } });
  assert.match(
    toolsRust(validateTools(data, items, blocks)),
    /BlockDamageDef \{ block: "test:clay", volume: 1\.0, default: Some\(18\), tools: &\[\("dwell:wooden_shovel", 7\), \("dwell:stone_shovel", 5\), \("dwell:metal_shovel", 3\), .*\], barren: &\[\], shake: false, look: None, chips: None \},/,
  );
  const bad = (damage, re) => {
    blocks.set('test:clay', { damage });
    assert.throws(() => validateTools(data, items, blocks), re);
  };
  bad({ shovel: 4 }, /test:clay\.damage: unknown field shovel/);
  bad({ proficiency: {} }, /test:clay\.damage\.baseHits is a number of hits in \(0, 1000\], not undefined/);
  bad([], /test:clay\.damage must be an object/);
  // Its look (D2) by data alone, and a bad one fails naming it.
  blocks.set('test:clay', { damage: { baseHits: 3, look: { kind: 'scoops', size: 0.3, color: '#806040', crack: '#302010' } } });
  assert.match(
    toolsRust(validateTools(data, items, blocks)),
    /block: "test:clay", .*look: Some\(DamageLook \{ kind: DamageLookKind::Scoops, size: 0\.3, color: \[128, 96, 64\], crack: \[48, 32, 16\], depth: 0\.1, rim: 0\.012, parallax: 6, darken: 0\.35, profile: CarveProfile::Bowl, facets: 5, flakes: 2, grid: 8, triangles: 256, cracks: None \}\)/,
  );
  bad({ baseHits: 3, look: { kind: 'dents', size: 0.3, color: '#806040', crack: '#302010' } }, /test:clay\.damage\.look\.kind must be one of shards, scoops, chips/);
  bad({ baseHits: 3, look: { kind: 'scoops', size: 0.3, color: '#806040', crack: '#302010', depth: 0.9 } }, /test:clay\.damage\.look\.depth must be a number in \[0, 0\.5\]/);
  // A scoop's per-pixel relief is data (BLOCK_DAMAGE.md §6.1): a fixture's rim, march and damp floor.
  bad({ baseHits: 3, look: { kind: 'scoops', size: 0.3, color: '#806040', crack: '#302010', rim: 0.2 } }, /test:clay\.damage\.look\.rim must be a number in \[0, 0\.03\]/);
  bad({ baseHits: 3, look: { kind: 'scoops', size: 0.3, color: '#806040', crack: '#302010', parallax: 2.5 } }, /test:clay\.damage\.look\.parallax must be a whole number of steps from 0 to 8/);
  bad({ baseHits: 3, look: { kind: 'scoops', size: 0.3, color: '#806040', crack: '#302010', darken: -1 } }, /test:clay\.damage\.look\.darken must be a number in \[0, 1\]/);
  // The carved mesh's data (§6.2): a fixture's facets by data alone, bad values failing by name.
  blocks.set('test:clay', { damage: { baseHits: 3, look: { kind: 'chips', size: 0.3, color: '#806040', crack: '#302010', profile: 'facets', facets: 7, grid: 6, triangles: 160 } } });
  assert.match(toolsRust(validateTools(data, items, blocks)), /block: "test:clay", .*profile: CarveProfile::Facets, facets: 7, flakes: 2, grid: 6, triangles: 160, cracks: None \}\)/);
  bad({ baseHits: 3, look: { kind: 'scoops', size: 0.3, color: '#806040', crack: '#302010', profile: 'spiky' } }, /test:clay\.damage\.look\.profile must be one of bowl, facets, spall/);
  // Stone's spall (WORK_QUEUE item 18): a fixture's flakes by data alone, a bad count failing by name.
  blocks.set('test:clay', { damage: { baseHits: 3, look: { kind: 'shards', size: 0.3, color: '#806040', crack: '#302010', profile: 'spall', flakes: 3 } } });
  assert.match(toolsRust(validateTools(data, items, blocks)), /block: "test:clay", .*profile: CarveProfile::Spall, facets: 5, flakes: 3, grid: 8/);
  bad({ baseHits: 3, look: { kind: 'shards', size: 0.3, color: '#806040', crack: '#302010', profile: 'spall', flakes: 5 } }, /test:clay\.damage\.look\.flakes must be a whole number from 1 to 4/);
  bad({ baseHits: 3, look: { kind: 'scoops', size: 0.3, color: '#806040', crack: '#302010', facets: 2 } }, /test:clay\.damage\.look\.facets must be a whole number from 3 to 8/);
  bad({ baseHits: 3, look: { kind: 'scoops', size: 0.3, color: '#806040', crack: '#302010', grid: 40 } }, /test:clay\.damage\.look\.grid must be a whole number from 2 to 16/);
  bad({ baseHits: 3, look: { kind: 'scoops', size: 0.3, color: '#806040', crack: '#302010', triangles: 64 } }, /test:clay\.damage\.look\.triangles must be a whole number from 128 to 2048/);
  bad({ baseHits: 3, look: { kind: 'shards', size: 2, color: '#806040', crack: '#302010' } }, /test:clay\.damage\.look\.size must be a number/);
  bad({ baseHits: 3, look: { kind: 'shards', size: 0.3, color: 'red', crack: '#302010' } }, /test:clay\.damage\.look\.color must be a #rrggbb colour/);
  // The crack overlay (BLOCK_DAMAGE.md §6.3): a fixture's set, stages and strength by data alone,
  // resolved to the set's tiles; bad ones fail naming them.
  const look = { kind: 'scoops', size: 0.3, color: '#806040', crack: '#302010' };
  blocks.set('test:clay', { damage: { baseHits: 3, look: { ...look, cracks: { set: 'wood', stages: [0.1, 0.3, 0.5, 0.9], strength: 0.4 } } } });
  assert.match(
    toolsRust(validateTools(data, items, blocks)),
    /block: "test:clay", .*cracks: Some\(Cracks \{ tiles: &\["crackWood1", "crackWood2", "crackWood3", "crackWood4"\], stages: &\[0\.1, 0\.3, 0\.5, 0\.9\], strength: 0\.4, span: 1\.5 \}\) \}\)/,
  );
  assert.match(toolsRust(d), /block: "dwell:stone", .*cracks: Some\(Cracks \{ tiles: &\["crackStone1",/);
  bad({ baseHits: 3, look: { ...look, cracks: { set: 'glass', stages: [0.5], strength: 0.4 } } }, /test:clay\.damage\.look\.cracks\.set must be a crack set of shared\/textures\/cracks\.json/);
  bad({ baseHits: 3, look: { ...look, cracks: { set: 'wood', stages: [0.5], strength: 0.4 } } }, /test:clay\.damage\.look\.cracks\.stages must list 4 damage fractions/);
  bad({ baseHits: 3, look: { ...look, cracks: { set: 'wood', stages: [0.1, 0.5, 0.4, 0.9], strength: 0.4 } } }, /test:clay\.damage\.look\.cracks\.stages must ascend/);
  bad({ baseHits: 3, look: { ...look, cracks: { set: 'wood', stages: [0, 0.5, 0.6, 0.9], strength: 0.4 } } }, /test:clay\.damage\.look\.cracks\.stages\[0\] must be a damage fraction in \(0, 1\]/);
  bad({ baseHits: 3, look: { ...look, cracks: { set: 'wood', stages: [0.1, 0.3, 0.5, 0.9], strength: 2 } } }, /test:clay\.damage\.look\.cracks\.strength must be a number in \[0, 1\]/);
  bad({ baseHits: 3, look: { ...look, cracks: { set: 'wood', stages: [0.1, 0.3, 0.5, 0.9], strength: 0.4, glow: 1 } } }, /test:clay\.damage\.look\.cracks/);
  // Its hit's chips (sounds round 2b) by data alone, and a bad one fails naming it.
  blocks.set('test:clay', { damage: { baseHits: 3, chips: { count: 5, color: '#806040', speed: 1.5, size: 0.05 } } });
  assert.match(
    toolsRust(validateTools(data, items, blocks)),
    /block: "test:clay", .*chips: Some\(Chips \{ count: 5, shards: 0, piece: "dwell:rock_shard", color: \[128, 96, 64\], speed: 1\.5, size: 0\.05 \}\)/,
  );
  bad({ baseHits: 3, chips: { count: 5, color: 'brown' } }, /test:clay\.damage\.chips\.color must be #rrggbb/);
  bad({ baseHits: 3, chips: { count: 99 } }, /test:clay\.damage\.chips\.count must be a whole number/);
  // A hit's 3D fragments by data alone (WORK_QUEUE item 18), a bad count failing by name.
  blocks.set('test:clay', { damage: { baseHits: 3, chips: { count: 5, shards: 3 } } });
  assert.match(toolsRust(validateTools(data, items, blocks)), /block: "test:clay", .*chips: Some\(Chips \{ count: 5, shards: 3,/);
  bad({ baseHits: 3, chips: { count: 5, shards: 9 } }, /test:clay\.damage\.chips\.shards must be a whole number in \[0, 8\]/);
});

test("durability: each break tool's wear computed from its class's rule (the owner's sheet), the sword's its own, creative does not wear", () => {
  const [tools, items, blocks] = fresh();
  const d = validateTools(tools, items, blocks);
  const wear = Object.fromEntries(d.durability.items.map((x) => [x.item, x.hits]));
  // Owner, 2026-11-17: 64 blocks × the reference block's stone-tool hits × the material's multiplier.
  assert.deepEqual(
    [wear['dwell:wooden_shovel'], wear['dwell:stone_shovel'], wear['dwell:metal_shovel'], wear['dwell:stone_axe'], wear['dwell:metal_axe'], wear['dwell:stone_pickaxe'], wear['dwell:metal_pickaxe']],
    [128, 256, 512, 384, 768, 384, 768],
  );
  // The sword's `weapon.durability` (WORK_QUEUE item 35).
  assert.equal(wear['dwell:sword'], 120);
  assert.equal(d.durability.creativeWear, false);
  assert.match(toolsRust(d), /pub const CREATIVE_WEAR: bool = false;/);
  assert.match(toolsTs(d), /'dwell:stone_axe': 384,/);
  // By data alone: a longer-lived class.
  tools.breaking.classes.pickaxe.durability.baseBlocks = 100;
  assert.match(toolsRust(validateTools(tools, items, blocks)), /\("dwell:metal_pickaxe", 1200\)/);
  const bad = (edit, re) => {
    const [t, i, b] = fresh();
    edit(t.durability);
    assert.throws(() => validateTools(t, i, b), re);
  };
  bad((x) => (x.items['dwell:nothing'] = 3), /no item/);
  bad((x) => (x.items['dwell:stone_pickaxe'] = 40), /a break tool's wear is computed/);
  bad((x) => (x.creativeWear = 'no'), /creativeWear/);
  bad((x) => (x.breakSound = 'nope'), /no sound event/);
});

test("a swing's camera sway (CHARACTERS.md §5.2): the axe's per kind, a fixture tool's by data alone, bad curves fail naming them", () => {
  const d = validateTools(...fresh());
  const rs = toolsRust(d);
  // The stone axe's level sway leans right into the wind-up and turns left through the strike.
  assert.match(rs, /name: "dwell:stone_axe".*camera_level: SwingCamera \{ keys: &\[CameraKey \{ t: 0\.0, yaw: 0\.0, .*CameraKey \{ t: 1\.0, yaw: 1\.8, pitch: 0\.5, roll: 1\.4/);
  // One mode: the axe has no vertical sway.
  assert.match(rs, /name: "dwell:stone_axe"[^\n]*camera_vertical: SwingCamera \{ keys: &\[\],/);
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
    const c = x.tools[0].swing.camera;
    c.vertical = structuredClone(c.level);
    edit(c);
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

// WORK_QUEUE item 35: a `weapon` tool cuts no wood and strikes by its data; the swordplay's
// thresholds (owner, 2026-11-12: perpendicular blocks, parallel connects) are checked.
test('the sword is a weapon: its strike, guard and wear are emitted', () => {
  const d = validateTools(...fresh());
  const sword = d.tools.find((t) => t.name === 'dwell:sword');
  assert.equal(sword.kind, 'weapon');
  assert.deepEqual(sword.clears, []);
  assert.ok(d.durability.items.some((x) => x.item === 'dwell:sword'), 'its durability wears as a tool');
  assert.match(toolsRust(d), /name: "dwell:sword", kind: ToolKind::Weapon/);
  assert.match(toolsRust(d), /weapon: Some\(WeaponDef \{ damage: 12\.0, knockback_m: 0\.8, knockback_mps: 4\.0, lift_mps: 2\.5, max_targets: 2, guard: GuardDef \{ level_roll_deg: 0\.0, vertical_roll_deg: 90\.0/);
  assert.match(toolsRust(d), /name: "dwell:stone_axe"[^\n]*weapon: None/);
});

test('a fixture weapon added by data alone is emitted with the default guard', () => {
  const [data, items, blocks] = fresh();
  items.add('test:club');
  data.tools.push({ name: 'test:club', kind: 'weapon', swingSeconds: 0.4, weapon: { damage: 3, knockbackM: 0.2 } });
  const d = validateTools(data, items, blocks);
  assert.match(toolsRust(d), /name: "test:club", kind: ToolKind::Weapon[^\n]*knockback_mps: 0\.0, lift_mps: 0\.0, max_targets: 1, guard: GuardDef \{ level_roll_deg: 0\.0, vertical_roll_deg: 90\.0, block_within_deg: 45\.0, connect_within_deg: 45\.0/);
  assert.ok(!d.durability.items.some((x) => x.item === 'test:club'), 'no durability: it never wears');
});

test('a bad weapon fails naming it and the problem', () => {
  const bad = (edit, re) => {
    const [data, items, blocks] = fresh();
    edit(data.tools.find((t) => t.name === 'dwell:sword'));
    assert.throws(() => validateTools(data, items, blocks), re);
  };
  bad((t) => (t.biteM = 0.1), /tools\.dwell:sword\.biteM: a weapon cuts no wood/);
  bad((t) => delete t.weapon, /tools\.dwell:sword\.weapon is required/);
  bad((t) => (t.weapon.damage = 0), /weapon\.damage must be a number in \(0, 1000\]/);
  // WORK_QUEUE item 38: a player's push from a connecting strike.
  bad((t) => (t.weapon.knockbackMps = -1), /weapon\.knockbackMps must be a number in \[0, 30\]/);
  bad((t) => (t.weapon.liftMps = 'up'), /weapon\.liftMps must be a number in \[0, 30\]/);
  bad((t) => (t.weapon.maxTargets = 0), /maxTargets must be a whole number/);
  bad((t) => (t.weapon.guard.blockWithinDeg = 60), /blockWithinDeg \+ connectWithinDeg \(95\) must not be above 90/);
  bad((t) => (t.weapon.guard.stances.level = { rollDeg: 400 }), /stances\.level\.rollDeg must be a number/);
  bad((t) => (t.weapon.wobble = 1), /unknown field wobble/);
  bad((t) => (t.weapon.guard.clash.sound = 'kazoo'), /guard\.clash\.sound: no sound event "kazoo"/);
  bad((t) => (t.weapon.guard.clash.sparks.color = 'gold'), /clash\.sparks\.color must be #rrggbb/);
  bad((t) => (t.weapon.guard.raiseAfterS = 2), /raiseAfterS must be a number/);
  bad((t) => (t.weapon.guard.stances.vertical.raisedPose = 'Up Guard'), /raisedPose must be a role name/);
  bad((t) => (t.weapon.guard.stances.vertical.blade = { grip: [0, 1.3, 0.4] }), /stances\.vertical\.blade\.tip must be \[right, up, ahead\]/);
  bad((t) => (t.weapon.guard.stances.level.blade = { grip: [0, 1.3, 0.4], tip: [0, 1.3, 0.45] }), /stances\.level\.blade: the tip must be at least 0\.1 m from the grip/);
  bad((t) => (t.weapon.guard.stances.level.blade.edge = 1), /blade: unknown field edge/);
});

test('each stance’s raised blade is emitted, a weapon naming none taking the defaults (BUGS.md #86)', () => {
  const rs = toolsRust(validateTools(...fresh()));
  assert.match(rs, /name: "dwell:sword"[^\n]*level_blade: RaisedBlade \{ grip: \[0\.06, 1\.3, 0\.44\], tip: \[-0\.72, 1\.38, 0\.53\] \}, vertical_blade: RaisedBlade \{ grip: \[0\.12, 1\.19, 0\.43\]/);
  const [data, items, blocks] = fresh();
  items.add('test:club');
  data.tools.push({ name: 'test:club', kind: 'weapon', swingSeconds: 0.4, weapon: { damage: 3, knockbackM: 0.2, guard: { stances: { vertical: { rollDeg: 90, blade: { grip: [0, 1, 0.5], tip: [0, 2, 0.5] } } } } } });
  const club = toolsRust(validateTools(data, items, blocks)).split('\n').find((l) => l.includes('"test:club"'));
  assert.match(club, /level_blade: RaisedBlade \{ grip: \[0\.06, 1\.3, 0\.44\]/, 'the default level blade');
  assert.match(club, /vertical_blade: RaisedBlade \{ grip: \[0\.0, 1\.0, 0\.5\], tip: \[0\.0, 2\.0, 0\.5\] \}/, 'its own');
});

test('the sword’s clash look, raise timing and raised poses are emitted (round 2)', () => {
  const rs = toolsRust(validateTools(...fresh()));
  assert.match(rs, /name: "dwell:sword"[^\n]*raise_after_s: 0\.15, level_pose: Some\("guard_level"\), vertical_pose: Some\("guard_vertical"\), level_blade: [^\n]*?, clash: ClashLook \{ sound: Some\("axe_clank"\), hear_m: 32\.0, sparks: Some\(Chips \{ count: 12/);
  // A weapon naming none: no sound, no sparks, no poses.
  const [data, items, blocks] = fresh();
  items.add('test:club');
  data.tools.push({ name: 'test:club', kind: 'weapon', swingSeconds: 0.4, weapon: { damage: 3, knockbackM: 0.2 } });
  assert.match(toolsRust(validateTools(data, items, blocks)), /name: "test:club"[^\n]*level_pose: None, vertical_pose: None, level_blade: [^\n]*?, clash: ClashLook \{ sound: None, hear_m: 32\.0, sparks: None, glance_share: 0\.5 \}/);
});

test('each tool kind has its own hint, as data; a kind without one fails', () => {
  const [data, items, blocks] = fresh();
  const rs = toolsRust(validateTools(data, items, blocks));
  assert.match(rs, /\(ToolKind::Weapon, "Sword: level stance/);
  // One mode (owner, 2026-11-14): the axe's level line stands for both.
  assert.match(rs, /\(ToolKind::Axe, "Axe: hold to keep chopping", "Axe: hold to keep chopping"\)/);
  // A kind with a second stance needs its vertical line.
  const [d2, i2, b2] = fresh();
  delete d2.hints.weapon.vertical;
  assert.throws(() => validateTools(d2, i2, b2), /hints\.weapon\.vertical is required/);
  delete data.hints.weapon;
  assert.throws(() => validateTools(data, items, blocks), /hints\.weapon is required/);
});

test('hold to keep swinging (WORK_QUEUE item 17): every break tool and the hand at one pace (owner, 2026-11-17: the axe\'s); a chopping tool by its swing; bad entries fail naming them', () => {
  const d = validateTools(...fresh());
  const rs = toolsRust(d);
  for (const n of ['dwell:stone_axe', 'dwell:metal_axe', 'dwell:sword']) assert.match(rs, new RegExp(`name: "${n}", kind: ToolKind::\\w+, .*repeat_on_hold: true, clip_level`));
  for (const n of ['dwell:stone_pickaxe', 'dwell:metal_pickaxe', 'dwell:wooden_shovel', 'dwell:metal_shovel']) assert.match(rs, new RegExp(`item: "${n}", .*contact_s: 0\\.26, repeat_s: Some\\(0\\.5\\), clip: "break_`));
  assert.match(rs, /pub const HAND_REPEAT_S: Option<f64> = Some\(0\.5\);/);
  // A chopping tool's break-table entry never repeats as a break (its swing does).
  assert.match(rs, /item: "dwell:stone_axe", .*repeat_s: None, clip: "break" \}/);
  // By data alone: one pace for all.
  const [data, items, blocks] = fresh();
  data.breaking.repeatS = 0.7;
  const fx = toolsRust(validateTools(data, items, blocks));
  assert.match(fx, /item: "dwell:metal_shovel", .*repeat_s: Some\(0\.7\)/);
  assert.match(fx, /HAND_REPEAT_S: Option<f64> = Some\(0\.7\);/);
  delete data.breaking.repeatS;
  assert.match(toolsRust(validateTools(data, items, blocks)), /HAND_REPEAT_S: Option<f64> = None;/);
  const bad = (edit, re) => {
    const [d2, i2, b2] = fresh();
    edit(d2);
    assert.throws(() => validateTools(d2, i2, b2), re);
  };
  bad((d2) => (d2.tools[0].repeatOnHold = 'yes'), /tools\.dwell:stone_axe\.repeatOnHold must be true or false/);
  bad((d2) => (d2.breaking.repeatS = 0.05), /breaking\.repeatS is seconds in \[0\.1, 5\]/);
  bad((d2) => (d2.breaking.tools[2].repeatS = 0.4), /breaking\.tools\[2\]: unknown field repeatS/);
  bad((d2) => (d2.breaking.tools[2].clip = 'Break Pick'), /breaking\.tools\[2\]: clip must be a role name/);
});

test("a tool's swing clips (WORK_QUEUE item 35): the sword's slashes, the axe's chops by default, a fixture's by data alone; bad entries fail naming them", () => {
  const rs = toolsRust(validateTools(...fresh()));
  assert.match(rs, /name: "dwell:sword", [^\n]*clip_level: "slash_level", clip_vertical: "slash_overhead"/);
  assert.match(rs, /name: "dwell:stone_axe", [^\n]*clip_level: "chop_level", clip_vertical: "chop_vertical"/);
  const [data, items, blocks] = fresh();
  items.add('test:club');
  data.tools.push({ name: 'test:club', kind: 'weapon', swingSeconds: 0.4, clips: { level: 'club_swing' }, weapon: { damage: 3, knockbackM: 0.2 } });
  assert.match(toolsRust(validateTools(data, items, blocks)), /name: "test:club", [^\n]*clip_level: "club_swing", clip_vertical: "chop_vertical"/);
  const bad = (edit, re) => {
    const [d2, i2, b2] = fresh();
    edit(d2.tools.find((t) => t.name === 'dwell:sword'));
    assert.throws(() => validateTools(d2, i2, b2), re);
  };
  bad((t) => (t.clips = 'slash'), /tools\.dwell:sword\.clips must be \{ level\?, vertical\? \} role names/);
  bad((t) => (t.clips = { level: 'Slash Left' }), /tools\.dwell:sword\.clips\.level must be a role name/);
  bad((t) => (t.clips = { thrust: 'thrust' }), /clips: unknown field thrust/);
});

