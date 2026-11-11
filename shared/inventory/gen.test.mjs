// Inventories, items and block drops: validation and outputs (shared/inventory/gen.mjs, ADR 0066).
import assert from 'node:assert/strict';
import test from 'node:test';
import { inventoryRust, inventoryTs, loadBlocks, loadInventory, validateInventory } from './gen.mjs';

const fresh = () => [loadInventory(), loadBlocks()];
const dropsOf = (d, id) => d.drops.find((b) => b.block === id).drops;

test("the build's data validates with the owner's rules", () => {
  const d = validateInventory(...fresh());
  assert.equal(d.hotbarSlots, 6);
  assert.equal(d.backpackSlots, 30);
  assert.equal(d.stackSize, 64);
  assert.deepEqual(
    dropsOf(d, 'dwell:grass').map((r) => [r.choices[0].item, r.chance]),
    [
      ['dwell:dirt_clump', 1],
      ['dwell:rock', 0.5],
    ],
  );
  // Owner, 2026-11-11: a ½ m cell breaks into 8 pieces of its material.
  assert.deepEqual(dropsOf(d, 'dwell:dirt')[0].choices[0], { item: 'dwell:dirt_clump', min: 8, max: 8, weight: 1 });
  assert.deepEqual(dropsOf(d, 'dwell:stone')[0].choices[0], { item: 'dwell:rock_shard', min: 8, max: 8, weight: 1 });
  assert.deepEqual(dropsOf(d, 'dwell:log')[0].choices[0], { item: 'dwell:wood', min: 8, max: 8, weight: 1 });
  assert.equal(dropsOf(d, 'dwell:tall_grass')[0].choices[0].item, 'dwell:grass_fiber');
  assert.deepEqual(
    d.recipes.find((r) => r.name === 'dwell:stone_axe').inputs.map((i) => [i.item, i.count]),
    [
      ['dwell:stick', 2],
      ['dwell:rock_shard', 2],
      ['dwell:grass_fiber', 2],
    ],
  );
  assert.deepEqual(dropsOf(d, 'dwell:leaves')[0].choices[0].item, 'dwell:stick');
  assert.deepEqual(dropsOf(d, 'dwell:water'), []);
  assert.equal(d.drops.find((b) => b.block === 'dwell:stone_ramp').cost, 'dwell:stone');
  assert.match(inventoryRust(d), /pub const HOTBAR_SLOTS: usize = 6;/);
  assert.match(inventoryTs(d), /export const ITEMS/);
});

test('an item and a block with a custom drop added by data alone are emitted', () => {
  const [inv, blocks] = fresh();
  inv.items.push({ name: 'dwell:crystal', label: 'Crystal', color: '#aaccff', size: 0.1, stack: 16 });
  blocks.push({
    namespace: 'test',
    blocks: [{ name: 'ore', shape: 'full', drops: [{ item: 'dwell:crystal', count: [1, 3], chance: 0.25 }] }],
  });
  const d = validateInventory(inv, blocks);
  const rs = inventoryRust(d);
  assert.match(rs, /ItemDef \{ name: "dwell:crystal", label: "Crystal", color: \[170, 204, 255\], size: 0.1, stack: 16, value: 0, look: None \}/);
  assert.match(
    rs,
    /BlockDrops \{ block: "test:ore", cost: "test:ore", drops: &\[DropRule \{ chance: 0.25, choices: &\[DropChoice \{ item: "dwell:crystal", min: 1, max: 3, weight: 1.0 \}\] \}\] \}/,
  );
  assert.match(inventoryTs(d), /name: "dwell:crystal"/);
});

test('an item with a look added by data alone carries it (one look for hand, hotbar, loose, ground)', () => {
  const [inv, blocks] = fresh();
  inv.items.push({ name: 'test:twig', label: 'Twig', color: '#aaccff', size: 0.1, look: { shape: 'rod', size: [0.3, 0.02, 0.02], texture: 'dwell:log', color: '#ffffff' } });
  const rs = inventoryRust(validateInventory(inv, blocks));
  assert.match(rs, /name: "test:twig".*look: Some\(ItemLook \{ shape: LookShape::Rod, size: \[0.3, 0.02, 0.02\], texture: "dwell:log", color: \[255, 255, 255\], model: "" \}\)/);
  assert.match(rs, /name: "dwell:stick".*look: Some\(ItemLook \{ shape: LookShape::Rod/);
});

test('a model look added by data alone names a prop (ADR 0070 §8); the stone pickaxe is one', () => {
  const [inv, blocks] = fresh();
  inv.items.push({ name: 'test:mug', label: 'Mug', color: '#aaccff', size: 0.2, look: { model: 'fixed_simple_mug' } });
  const rs = inventoryRust(validateInventory(inv, blocks));
  assert.match(rs, /name: "test:mug".*look: Some\(ItemLook \{ shape: LookShape::Model, size: \[0.25, 0.25, 0.25\], texture: "", color: \[255, 255, 255\], model: "fixed_simple_mug" \}\)/);
  assert.match(rs, /name: "dwell:stone_pickaxe".*shape: LookShape::Model.*model: "stone_pickaxes"/);
  const bad = fresh()[0];
  bad.items.push({ name: 'test:ghost', label: 'Ghost', color: '#aaccff', size: 0.2, look: { model: 'nothing' } });
  assert.throws(() => validateInventory(bad, blocks), /items\.test:ghost\.look: model "nothing" is not a prop/);
  const mixed = fresh()[0];
  mixed.items.push({ name: 'test:odd', label: 'Odd', color: '#aaccff', size: 0.2, look: { model: 'chest', shape: 'rod' } });
  assert.throws(() => validateInventory(mixed, blocks), /items\.test:odd\.look: a model look has only `model`/);
});

test('a cluster look added by data alone carries it; the grass seed is one', () => {
  const [inv, blocks] = fresh();
  inv.items.push({ name: 'test:barley', label: 'Barley', color: '#aaccff', size: 0.1, look: { shape: 'cluster', size: [0.1, 0.04, 0.1], texture: 'dwell:sand', color: '#ffffff' } });
  const rs = inventoryRust(validateInventory(inv, blocks));
  assert.match(rs, /name: "test:barley".*look: Some\(ItemLook \{ shape: LookShape::Cluster, size: \[0.1, 0.04, 0.1\]/);
  assert.match(rs, /name: "dwell:grass_seed".*look: Some\(ItemLook \{ shape: LookShape::Cluster/);
});

test('bad entries fail naming the entry and the problem', () => {
  const cases = [
    [(i) => (i.stackSize = 0), /stackSize must be a whole number/],
    [(i) => (i.hotbarSlots = 11), /hotbarSlots must be a whole number in \[1, 10\]/],
    [(i) => (i.items[0].colour = '#000000'), /items\.dwell:rock: unknown field colour/],
    [(i) => (i.items[0].name = 'rock'), /items\.rock: name must be namespace:name/],
    [(i) => (i.items[0].name = 'dwell:dirt'), /a block of that name is already an item/],
    [(i) => (i.items[1].stack = 65), /items\.dwell:stick: stack must be/],
    [(i) => (i.items[3].value = 0), /items\.dwell:gem: value must be/],
    [(i) => (i.items[0].look.shape = 'cube'), /items\.dwell:rock\.look: shape must be one of rod, lump, cluster/],
    [(i) => (i.items[0].look.size = [0.1, 0, 0.1]), /items\.dwell:rock\.look: size must be/],
    [(i) => (i.items[1].look.texture = 'dwell:marble'), /items\.dwell:stick\.look: texture "dwell:marble" is not a block/],
    [(i) => (i.items[1].look.colour = '#000000'), /items\.dwell:stick\.look: unknown field colour/],
    [(i) => (i.startingWallet = 'dwell:bag'), /startingWallet must name one of the wallets/],
    [(i) => (i.wallets[0].capacity = 0), /wallets\.dwell:pouch: capacity must be/],
    [(i, b) => (b[0].blocks.find((x) => x.name === 'dirt').drops = [{ item: 'dwell:diamond' }]), /dwell:dirt: drops\[0\]: item dwell:diamond is neither/],
    [(i, b) => (b[0].blocks.find((x) => x.name === 'dirt').drops = [{ item: 'dwell:dirt', chance: 2 }]), /dwell:dirt: drops\[0\]: chance must be in \(0, 1\]/],
    [(i, b) => (b[0].blocks.find((x) => x.name === 'dirt').drops = [{ item: 'dwell:dirt', count: [3, 1] }]), /dwell:dirt: drops\[0\]: count must be/],
    [(i, b) => (b[0].blocks.find((x) => x.name === 'dirt').drops = [{ pick: [{ item: 'dwell:dirt', weight: 0 }] }]), /dwell:dirt: drops\[0\]\.pick\[0\]: weight must be above 0/],
    [(i, b) => (b[0].blocks.find((x) => x.name === 'dirt').drops = [{ item: 'dwell:dirt', pick: [] }]), /give item or pick/],
    [(i, b) => (b[0].blocks.find((x) => x.name === 'dirt').drops = 'dirt'), /dwell:dirt: drops must be a list/],
  ];
  for (const [change, message] of cases) {
    const [inv, blocks] = fresh();
    change(inv, blocks);
    assert.throws(() => validateInventory(inv, blocks), message);
  }
});

test('new players start with the data\'s items: an axe (owner, 2026-11-08)', () => {
  const d = validateInventory(...fresh());
  assert.deepEqual(d.startingItems, [{ item: 'dwell:stone_axe', count: 1 }]);
  assert.match(inventoryRust(d), /STARTING_ITEMS: &\[\(&str, u16\)\] = &\[\("dwell:stone_axe", 1\)\]/);
  const [inv, blocks] = fresh();
  inv.startingItems = [{ item: 'dwell:nothing', count: 1 }];
  assert.throws(() => validateInventory(inv, blocks), /startingItems\[0\]: item must name a block or an item/);
});

test('a recipe added by data alone is emitted, and a bad one fails with a message', () => {
  const [inv, blocks] = fresh();
  inv.recipes.push({ name: 'test:cairn', inputs: [{ item: 'dwell:rock', count: 4 }], output: { item: 'dwell:stone', count: 1 } });
  const d = validateInventory(inv, blocks);
  assert.match(inventoryRust(d), /Recipe \{ name: "test:cairn", inputs: &\[\("dwell:rock", 4\)\], output: \("dwell:stone", 1\) \}/);
  assert.match(inventoryTs(d), /"name":"test:cairn"/);
  const bad = (r, re) => {
    const [i, b] = fresh();
    i.recipes.push(r);
    assert.throws(() => validateInventory(i, b), re);
  };
  bad({ name: 'test:x', inputs: [{ item: 'dwell:nothing', count: 1 }], output: { item: 'dwell:stone', count: 1 } }, /neither|must name/);
  bad({ name: 'test:x', inputs: [], output: { item: 'dwell:stone', count: 1 } }, /inputs must be a list/);
  bad({ name: 'test:x', inputs: [{ item: 'dwell:rock', count: 0 }], output: { item: 'dwell:stone', count: 1 } }, /count must be/);
  bad({ name: 'test:x', inputs: [{ item: 'dwell:gem', count: 1 }], output: { item: 'dwell:stone', count: 1 } }, /currency/);
  bad({ name: 'dwell:stone_axe', inputs: [{ item: 'dwell:rock', count: 1 }], output: { item: 'dwell:stone', count: 1 } }, /listed twice/);
});
