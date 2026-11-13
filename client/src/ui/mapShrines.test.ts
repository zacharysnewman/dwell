import { describe, expect, it } from 'vitest';
import {
  SHRINE_FILTER_KEY,
  SHRINE_MAP,
  ShrineFilter,
  cellsFor,
  parseShrines,
  visibleShrines,
} from './mapShrines';

const sites = parseShrines([10.5, 20.5, 2000.5, -40.5, -3000.5, 900.5]);

describe('the shrines on the big map', () => {
  it('reads the flat list and asks for enough grid cells to cover the view', () => {
    expect(sites).toEqual([
      { x: 10.5, z: 20.5 },
      { x: 2000.5, z: -40.5 },
      { x: -3000.5, z: 900.5 },
    ]);
    expect(cellsFor(1000)).toBe(1);
    expect(cellsFor(9000)).toBe(3);
    expect(cellsFor(1e6)).toBe(4);
  });

  it('shows the explored ones (the reveal rule), all when revealed, none with the filter off', () => {
    expect(SHRINE_MAP.reveal).toBe('explored');
    const explored = (x: number) => x > 0;
    expect(visibleShrines(sites, true, explored, false)).toEqual(sites.slice(0, 2));
    expect(visibleShrines(sites, true, explored, true)).toEqual(sites);
    expect(
      visibleShrines(sites, true, () => false, false, { ...SHRINE_MAP, reveal: 'all' }),
    ).toEqual(sites);
    // The filter hides them, explored or not.
    expect(visibleShrines(sites, false, explored, true)).toEqual([]);
  });

  it('remembers the filter in this browser, on by default, and survives failing storage', () => {
    const kept = new Map<string, string>();
    const storage = {
      getItem: (k: string) => kept.get(k) ?? null,
      setItem: (k: string, v: string) => void kept.set(k, v),
    };
    const f = new ShrineFilter(storage);
    expect(f.on).toBe(true);
    expect(f.toggle()).toBe(false);
    expect(kept.get(SHRINE_FILTER_KEY)).toBe('off');
    expect(new ShrineFilter(storage).on).toBe(false);
    const broken = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
    };
    const b = new ShrineFilter(broken);
    expect(b.on).toBe(true);
    expect(b.toggle()).toBe(false);
  });
});
