// The backpack screen's moves and order, and item labels (ADR 0066).
import { describe, expect, it } from 'vitest';
import { backpackKey, combinable, moveFor, recipeInputs, recipeOutput, screenOrder } from './backpack';
import { itemLabel, wearLeft, type InventoryHud } from './hotbar';
import { BACKPACK_SLOTS, HOTBAR_SLOTS, ITEMS, STACK_SIZE } from './inventory.gen';

const inv = (): InventoryHud => ({
  slots: [{ item: 'dwell:dirt', count: 5 }, ...Array<null>(35).fill(null)],
  held: 0,
  money: 0,
  capacity: 99,
});

describe('backpack', () => {
  it("has the data's sizes: 6 hotbar slots under 30 backpack slots (5 rows of 6), stacks of 64", () => {
    expect([HOTBAR_SLOTS, BACKPACK_SLOTS, STACK_SIZE]).toEqual([6, 30, 64]);
    const order = screenOrder();
    expect(order).toHaveLength(36);
    expect(order.slice(-6)).toEqual([0, 1, 2, 3, 4, 5]);
  });

  it('moves a whole stack, or half of it', () => {
    expect(moveFor(inv(), 0, 12, false)).toEqual([0, 12, 0]);
    expect(moveFor(inv(), 0, 12, true)).toEqual([0, 12, 3]);
    expect(moveFor(inv(), 1, 12, false)).toBeNull();
    expect(moveFor(inv(), 0, 0, false)).toBeNull();
  });

  it('labels items: non-block items from the data, blocks from their name', () => {
    expect(ITEMS.find((i) => i.name === 'dwell:gem')?.value).toBe(1);
    expect(itemLabel('dwell:rock')).toBe('Rock');
    expect(itemLabel('dwell:snowy_grass')).toBe('Snowy grass');
  });
});

// E is the backpack's only (ADR 0071, amended 2026-11-09): ground items are knocked loose by the
// break action, so E no longer picks one up first.
describe('the backpack key', () => {
  it('E opens and closes the backpack where the world uses items', () => {
    expect(backpackKey('KeyE', false, false, true)).toBe(true);
    expect(backpackKey('KeyE', false, true, true)).toBe(false);
  });

  it('a held E, E in creative, and other keys leave it', () => {
    expect(backpackKey('KeyE', true, false, true)).toBeNull();
    expect(backpackKey('KeyE', false, false, false)).toBeNull();
    expect(backpackKey('KeyQ', false, false, true)).toBeNull();
  });

  it('Escape closes it, and only when open', () => {
    expect(backpackKey('Escape', false, true, true)).toBe(false);
    expect(backpackKey('Escape', false, false, true)).toBeNull();
  });
});

describe('combine and wear (owner, 2026-11-11)', () => {
  const holding = (...stacks: { item: string; count: number }[]): InventoryHud => ({
    slots: [...stacks, ...Array<null>(36 - stacks.length).fill(null)],
    held: 0,
    money: 0,
    capacity: 99,
    combines: true,
  });

  it('lists the recipes the inventory can make, with how many times', () => {
    expect(combinable(holding({ item: 'dwell:rock_shard', count: 7 }))).toEqual([]);
    const can = combinable(holding({ item: 'dwell:rock_shard', count: 17 }));
    expect(can.map((c) => [c.recipe.name, c.times])).toEqual([['dwell:stone_slab', 2]]);
    const tools = combinable(
      holding(
        { item: 'dwell:stick', count: 4 },
        { item: 'dwell:rock_shard', count: 2 },
        { item: 'dwell:grass_fiber', count: 2 },
      ),
    );
    expect(tools.map((c) => [c.recipe.name, c.times])).toEqual([
      ['dwell:stone_axe', 1],
      ['dwell:stone_pickaxe', 1],
    ]);
    expect(tools[0] && recipeInputs(tools[0].recipe)).toBe(
      '2 Stick + 2 Rock shard + 2 Grass fiber',
    );
    expect(itemLabel('dwell:wood')).toBe('Wood chunk');
  });

  it('names a slab recipe\'s output by its own label ("Stone slab"), a tool\'s by its item', () => {
    const can = combinable(
      holding(
        { item: 'dwell:rock_shard', count: 8 },
        { item: 'dwell:dirt_clump', count: 8 },
        { item: 'dwell:wood', count: 8 },
        { item: 'dwell:stick', count: 2 },
        { item: 'dwell:grass_fiber', count: 2 },
      ),
    );
    const names = can.map((c) => recipeOutput(c.recipe));
    expect(names.slice(0, 3)).toEqual(['Stone slab', 'Dirt slab', 'Log slab']);
    expect(names).toContain('Stone axe');
    expect(
      recipeOutput({
        name: 'test:pair',
        inputs: [{ item: 'dwell:rock', count: 1 }],
        output: { item: 'dwell:stick', count: 2 },
      }),
    ).toBe('2 Stick');
  });

  it("draws a tool's wear bar from its data (stone axe 32, metal 64); unlimited tools have none", () => {
    expect(wearLeft('dwell:stone_axe', 0)).toBeNull();
    expect(wearLeft('dwell:stone_axe', 8)).toBe(0.75);
    expect(wearLeft('dwell:stone_axe', 31)).toBeCloseTo(1 / 32);
    // Owner, 2026-11-12: metal tools "have double durability".
    expect(wearLeft('dwell:metal_axe', 8)).toBe(0.875);
    expect(wearLeft('dwell:rock', 8)).toBeNull();
  });
});
