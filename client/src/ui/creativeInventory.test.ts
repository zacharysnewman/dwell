import { afterEach, describe, expect, it, vi } from 'vitest';
import { GamepadInput } from '../input/gamepad';
import { CREATIVE } from './buildModes.gen';
import { catalogue, loadPicks, PICKS_KEY, savePicks } from './creativeInventory';
import { HOTBAR_SLOTS } from './inventory.gen';

const palette = CREATIVE.items.map((i) => i.name);

/** A Storage stand-in. */
function memory(init: Record<string, string> = {}): Pick<Storage, 'getItem' | 'setItem'> & {
  data: Record<string, string>;
} {
  const data = { ...init };
  return {
    data,
    getItem: (k) => data[k] ?? null,
    setItem: (k, v) => {
      data[k] = v;
    },
  };
}

describe('creative inventory view', () => {
  it('lists every items.json entry once, by its category, in the data’s group order', () => {
    const groups = catalogue(palette);
    const listed = groups.flatMap((g) => g.items.map((i) => i.name));
    expect([...listed].sort()).toEqual([...palette].sort());
    expect(groups.map((g) => g.name)).toEqual(
      CREATIVE.categories.map((c) => c.name).filter((c) => groups.some((g) => g.name === c)),
    );
    for (const g of groups) {
      for (const it of g.items) {
        expect(CREATIVE.items.find((x) => x.name === it.name)?.category).toBe(g.name);
        expect(palette[it.index]).toBe(it.name);
      }
    }
    expect(groups.find((g) => g.name === 'tools')?.items.map((i) => i.name)).toContain(
      'dwell:stone_pickaxe',
    );
  });

  it('shows a fixture item and group added by data alone', () => {
    const data = {
      categories: [...CREATIVE.categories, { name: 'test_group', label: 'Test group' }],
      items: [...CREATIVE.items, { name: 'test:brick', label: 'Brick', category: 'test_group' }],
    };
    const groups = catalogue([...palette, 'test:brick'], '', data);
    expect(groups.at(-1)).toEqual({
      name: 'test_group',
      label: 'Test group',
      items: [{ name: 'test:brick', label: 'Brick', index: palette.length }],
    });
  });

  it('the search keeps the items whose label holds it', () => {
    const names = catalogue(palette, 'AXE').flatMap((g) => g.items.map((i) => i.name));
    expect(names).toEqual([
      'dwell:stone_axe',
      'dwell:metal_axe',
      'dwell:stone_pickaxe',
      'dwell:metal_pickaxe',
    ]);
    expect(catalogue(palette, 'nothing like it')).toEqual([]);
  });

  it('a new world starts with the data’s six picks', () => {
    expect(CREATIVE.hotbar).toHaveLength(HOTBAR_SLOTS);
    expect(HOTBAR_SLOTS).toBe(6);
    expect(CREATIVE.hotbar).toEqual([
      'dwell:stone',
      'dwell:dirt',
      'dwell:grass',
      'dwell:log',
      'dwell:stone_axe',
      'dwell:stone_pickaxe',
    ]);
  });

  it('picks persist per world by name, other worlds and unknown fields kept (append-only)', () => {
    const store = memory({ [PICKS_KEY]: JSON.stringify({ other: ['x:y'], note: 7 }) });
    expect(loadPicks(store, 'w1', palette)).toBeNull();
    const picks = [3, 0, 5, 7, 8, 10];
    savePicks(store, 'w1', picks, palette);
    expect(loadPicks(store, 'w1', palette)).toEqual(picks);
    expect(loadPicks(store, 'w2', palette)).toBeNull();
    const all = JSON.parse(store.data[PICKS_KEY] ?? '') as Record<string, unknown>;
    expect(all.other).toEqual(['x:y']);
    expect(all.note).toBe(7);
    expect(all.w1).toEqual(picks.map((i) => palette[i]));
    // An unreadable store is left alone; a pick this build lacks loads nothing.
    const bad = memory({ [PICKS_KEY]: '{broken' });
    savePicks(bad, 'w1', picks, palette);
    expect(bad.data[PICKS_KEY]).toBe('{broken');
    expect(
      loadPicks(
        memory({ [PICKS_KEY]: '{"w":["a:b","a:b","a:b","a:b","a:b","a:b"]}' }),
        'w',
        palette,
      ),
    ).toBeNull();
  });
});

describe('the inventory by controller', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('the inventory action opens it; B (back) closes it as a menu', () => {
    let open = false;
    const back = vi.fn(() => {
      open = false;
    });
    const pad = {
      connected: true,
      mapping: 'standard',
      buttons: [] as { pressed: boolean; value: number }[],
      axes: [0, 0, 0, 0],
    };
    vi.stubGlobal('navigator', { getGamepads: () => [pad] });
    const shell: string[][] = [['inventory'], []];
    const input = new GamepadInput(
      {
        client: {
          gamepad: () => shell.shift() ?? [],
          gamepadDisconnected: () => undefined,
        },
        menuOpen: () => open,
        openMenu: () => undefined,
        closeMenu: () => undefined,
        back,
        inventory: () => {
          open = !open;
        },
      },
      { querySelectorAll: () => [], body: { querySelectorAll: () => [] } } as unknown as Document,
    );
    input.poll(0);
    expect(open).toBe(true);
    input.poll(16);
    pad.buttons = [
      { pressed: false, value: 0 },
      { pressed: true, value: 1 },
    ];
    input.poll(32);
    expect(back).toHaveBeenCalledOnce();
    expect(open).toBe(false);
  });
});
