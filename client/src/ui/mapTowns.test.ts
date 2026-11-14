import { describe, expect, it } from 'vitest';
import { ShrineSites, filterLayers, visibleShrines } from './mapShrines';
import { TOWN_LAYER, TOWN_MAP, TOWN_SPACING, parseTowns, townColor } from './mapTowns';
import { MARKER_ICONS } from './markers.gen';

describe('town sites on the maps (docs/BLUEPRINTS.md §6)', () => {
  it('reads the crate’s [x, z, state]* and colours a town being built apart', () => {
    const towns = parseTowns([100.5, -20.5, 0, 1300.5, 40.5, 1]);
    expect(towns).toEqual([
      { x: 100.5, z: -20.5, building: false },
      { x: 1300.5, z: 40.5, building: true },
    ]);
    expect(townColor(towns[0] ?? { building: false })).toBe(TOWN_MAP.color);
    expect(townColor(towns[1] ?? { building: false })).toBe(TOWN_MAP.buildingColor);
    // A vector icon of the markers' set, and a filter layer of its own.
    expect(MARKER_ICONS.some((i) => i.id === TOWN_MAP.icon)).toBe(true);
    expect(filterLayers([]).map((l) => l.key)).toContain(TOWN_LAYER);
  });

  it.each([0, 1])(
    'shows the sites of explored town cells on face %i, each cell sited once',
    (face) => {
      const sites = new ShrineSites(TOWN_SPACING);
      sites.noteExplored(face, 10, 10);
      expect(sites.hasExplored(face, 0, 0)).toBe(true);
      expect(sites.hasExplored(1 - face, 0, 0)).toBe(false);
      const asked: [number, number][] = [];
      const site = (x: number, z: number) => {
        asked.push([x, z]);
        return [x - 30, z + 12, x > 0 ? 1 : 0];
      };
      const cells: [number, number][] = [
        [0, 0],
        [-1, 0],
      ];
      const { shrines: towns } = sites.gather(face, cells, site, () => false, parseTowns);
      const h = TOWN_SPACING / 2;
      expect(towns).toEqual([
        { x: h - 30, z: h + 12, building: true },
        { x: -h - 30, z: h + 12, building: false },
      ]);
      expect(asked).toEqual([
        [h, h],
        [-h, h],
      ]);
      // The reveal rule: explored ones only, unless revealed; none with the filter off.
      const explored = (x: number) => x > 0;
      expect(visibleShrines(towns, true, explored, false, TOWN_MAP)).toEqual(
        TOWN_MAP.reveal === 'all' ? towns : [towns[0]],
      );
      expect(visibleShrines(towns, true, explored, true, TOWN_MAP)).toEqual(towns);
      expect(visibleShrines(towns, false, explored, true, TOWN_MAP)).toEqual([]);
    },
  );
});
