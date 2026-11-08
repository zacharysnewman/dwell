// Inventories, items and block drops: validation and outputs (shared/inventory/gen.mjs, ADR 0066).
import assert from 'node:assert/strict';
import test from 'node:test';
import { inventoryRust, inventoryTs, loadBlocks, loadInventory, validateInventory } from './gen.mjs';

const fresh = () => [loadInventory(), loadBlocks()];
const dropsOf = (d, id) => d.drops.find((b) => b.block === id).drops;

test("the build's data validates with the owner's rules", () => {
  const d = validateInventory(...fresh());
  assert.equal(d.hotbarSlots, 9);
  assert.equal(d.backpackSlots, 27);
  assert.equal(d.stackSize, 64);
  assert.deepEqual(
    dropsOf(d, 'dwell:grass').map((r) => [r.choices[0].item, r.chance]),
    [
      ['dwell:dirt', 1],
      ['dwell:rock', 0.5],
    ],
  );
  assert.deepEqual(dropsOf(d, 'dwell:dirt')[0].choices[0].item, 'dwell:dirt');
  assert.deepEqual(dropsOf(d, 'dwell:leaves')[0].choices[0].item, 'dwell:stick');
  assert.deepEqual(dropsOf(d, 'dwell:water'), []);
  assert.equal(d.drops.find((b) => b.block === 'dwell:stone_ramp').cost, 'dwell:stone');
  assert.match(inventoryRust(d), /pub const HOTBAR_SLOTS: usize = 9;/);
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
  assert.match(rs, /ItemDef \{ name: "dwell:crystal", label: "Crystal", color: \[170, 204, 255\], size: 0.1, stack: 16, value: 0 \}/);
  assert.match(
    rs,
    /BlockDrops \{ block: "test:ore", cost: "test:ore", drops: &\[DropRule \{ chance: 0.25, choices: &\[DropChoice \{ item: "dwell:crystal", min: 1, max: 3, weight: 1.0 \}\] \}\] \}/,
  );
  assert.match(inventoryTs(d), /name: "dwell:crystal"/);
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
