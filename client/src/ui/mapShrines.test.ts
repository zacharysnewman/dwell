import { describe, expect, it } from 'vitest';
import {
  FILTERS_KEY,
  MapFilters,
  OWN_MARKERS,
  SHRINE_MAP,
  filterLayers,
  markerLayer,
  ShrineSites,
  clusterShrines,
  parseShrines,
  shrineCellsIn,
  shrinesShownAt,
  visibleShrines,
} from './mapShrines';

const sites = parseShrines([10.5, 20.5, 2000.5, -40.5, -3000.5, 900.5]);

describe('the shrines on the big map', () => {
  it('reads the flat list', () => {
    expect(sites).toEqual([
      { x: 10.5, z: 20.5 },
      { x: 2000.5, z: -40.5 },
      { x: -3000.5, z: 900.5 },
    ]);
  });

  // BUGS.md #115 (a 4× world zoomed out): the map asked for a square of at most 4 cells round the
  // view's centre, so shrines showed in an 18 km patch only.
  it('asks for every grid cell on the view, on the disc, not a square round its centre', () => {
    const cells = shrineCellsIn(
      { x0: -200_000, z0: -120_000, x1: 200_000, z1: 120_000 },
      200_000,
      2000,
    );
    const is = cells.map(([i]) => i);
    const ks = cells.map(([, k]) => k);
    expect([Math.min(...is), Math.max(...is)]).toEqual([-100, 99]);
    expect([Math.min(...ks), Math.max(...ks)]).toEqual([-60, 59]);
    // A corner cell of the view off the disc is not asked for.
    const r = 100_000;
    const small = shrineCellsIn({ x0: -r, z0: -r, x1: r, z1: r }, r, 2000);
    expect(small.some(([i, k]) => i === -50 && k === -50)).toBe(false);
    expect(small.some(([i, k]) => i === 0 && k === 0)).toBe(true);
    for (const [i, k] of small) {
      const nx = Math.max(i * 2000, Math.min(0, (i + 1) * 2000));
      const nz = Math.max(k * 2000, Math.min(0, (k + 1) * 2000));
      expect(Math.hypot(nx, nz)).toBeLessThanOrEqual(r);
    }
  });

  it('merges icons nearer than clusterPx into one with a count, so none overlap', () => {
    const pts = [
      { x: 0, y: 0 },
      { x: 5, y: 3 },
      { x: 100, y: 0 },
      { x: 26, y: 0 },
      { x: 128, y: 1 },
    ];
    const c = clusterShrines(pts, 28);
    expect(c.reduce((n, g) => n + g.count, 0)).toBe(pts.length);
    for (const a of c)
      for (const b of c)
        if (a !== b)
          expect(Math.max(Math.abs(a.x - b.x), Math.abs(a.y - b.y))).toBeGreaterThanOrEqual(28);
    expect(c.find((g) => g.x === 0)?.count).toBe(3);
    // A dense block (a grid 4 px apart) becomes a few counts, not hundreds of icons.
    const dense = [];
    for (let i = 0; i < 20; i++) for (let k = 0; k < 20; k++) dense.push({ x: i * 4, y: k * 4 });
    expect(clusterShrines(dense, SHRINE_MAP.clusterPx).length).toBeLessThanOrEqual(9);
  });

  it('hides them below the zoom where the grid is denser than minSpacingPx', () => {
    const at = (px: number) => px / 2000; // px a metre for the grid spacing to draw `px` wide
    expect(shrinesShownAt(at(SHRINE_MAP.minSpacingPx), SHRINE_MAP, 2000)).toBe(true);
    expect(shrinesShownAt(at(SHRINE_MAP.minSpacingPx - 1), SHRINE_MAP, 2000)).toBe(false);
  });

  it('sites new cells within a budget, the rest the next draws, and knows explored cells', () => {
    const s = new ShrineSites(2000);
    s.noteExplored(0, 10, 10);
    s.noteExplored(1, 4500, -10);
    expect([s.hasExplored(0, 0, 0), s.hasExplored(0, 2, -1), s.hasExplored(1, 2, -1)]).toEqual([
      true,
      false,
      true,
    ]);
    const asked: number[][] = [];
    const site = (x: number, z: number): number[] => {
      asked.push([x, z]);
      // Each cell's shrine, and the spawn's (returned by every nearby cell) once.
      return [x + 0.5, z + 0.5, 7.5, 7.5];
    };
    let budget = 2;
    const cells: [number, number][] = [
      [0, 0],
      [1, 0],
      [2, 0],
    ];
    const first = s.gather(0, cells, site, () => budget-- <= 0);
    expect(first.pending).toBe(1);
    expect(asked).toEqual([
      [1000, 1000],
      [3000, 1000],
    ]);
    expect(first.shrines).toEqual([
      { x: 1000.5, z: 1000.5 },
      { x: 7.5, z: 7.5 },
      { x: 3000.5, z: 1000.5 },
    ]);
    // Sited cells are asked again (cheap: cached in the crate) whatever the budget; the rest go on.
    const second = s.gather(0, cells, site, () => true);
    expect(second.pending).toBe(1);
    budget = 5;
    const third = s.gather(0, cells, site, () => budget-- <= 0);
    expect(third.pending).toBe(0);
    expect(third.shrines.length).toBe(4);
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
