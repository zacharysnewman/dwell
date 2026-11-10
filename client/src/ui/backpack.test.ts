// The backpack screen's moves and order, and item labels (ADR 0066).
import { describe, expect, it } from 'vitest';
import { backpackKey, moveFor, screenOrder } from './backpack';
import { itemLabel, type InventoryHud } from './hotbar';
import { BACKPACK_SLOTS, HOTBAR_SLOTS, ITEMS, STACK_SIZE } from './inventory.gen';

const inv = (): InventoryHud => ({
  slots: [{ item: 'dwell:dirt', count: 5 }, ...Array<null>(35).fill(null)],
  held: 0,
  money: 0,
  capacity: 99,
});

describe('backpack', () => {
  it("has the data's sizes: 9 hotbar slots, 27 backpack slots, stacks of 64", () => {
    expect([HOTBAR_SLOTS, BACKPACK_SLOTS, STACK_SIZE]).toEqual([9, 27, 64]);
    const order = screenOrder();
    expect(order).toHaveLength(36);
    expect(order.slice(-9)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8]);
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
