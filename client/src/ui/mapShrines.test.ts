import { describe, expect, it } from 'vitest';
import {
  FILTERS_KEY,
  MapFilters,
  OWN_MARKERS,
  SHRINE_MAP,
  filterLayers,
  markerLayer,
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

  it('filters the shrines and each marker kind, remembered in this browser, surviving failing storage', () => {
    const layers = filterLayers([
      { kind: 'spawn', label: 'Spawn' },
      { kind: 'death', label: 'Last death' },
    ]);
    expect(layers.map((l) => l.key)).toEqual(['shrine', 'spawn', 'death', OWN_MARKERS]);
    expect(markerLayer({ auto: '' })).toBe(OWN_MARKERS);
    expect(markerLayer({ auto: 'death' })).toBe('death');
    const kept = new Map<string, string>([[FILTERS_KEY, '{"future":false}']]);
    const storage = {
      getItem: (k: string) => kept.get(k) ?? null,
      setItem: (k: string, v: string) => void kept.set(k, v),
    };
    const f = new MapFilters(storage);
    expect(f.shows('shrine')).toBe(true);
    expect(f.toggle('shrine')).toBe(false);
    f.set('death', false);
    // Kept, with a layer this build doesn't know left as it was.
    expect(JSON.parse(kept.get(FILTERS_KEY) ?? '')).toEqual({
      future: false,
      shrine: false,
      death: false,
    });
    const again = new MapFilters(storage);
    expect([again.shows('shrine'), again.shows('death'), again.shows('spawn')]).toEqual([
      false,
      false,
      true,
    ]);
    // The filter hides the shrines whatever the reveal.
    expect(visibleShrines(sites, again.shows('shrine'), () => true, true)).toEqual([]);
    const broken = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
    };
    const b = new MapFilters(broken);
    expect(b.shows('shrine')).toBe(true);
    expect(b.toggle('shrine')).toBe(false);
    expect(new MapFilters(null).shows('own')).toBe(true);
  });
});
