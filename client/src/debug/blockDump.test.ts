import { describe, expect, it } from 'vitest';
import { stateId } from '../world/blocks';
import { blockDump, formatBlockDump } from './blockDump';

const STONE = stateId('dwell:stone');
const WEDGE = stateId('dwell:grass_slope[facing=east,flooded=false,half=bottom,shape=wedge]');

// A world of stone below y = 10, with a wedge at (3, 10, -4).
const voxel = (x: number, y: number, z: number) =>
  x === 3 && y === 10 && z === -4 ? WEDGE : y < 10 ? STONE : 0;

const base = {
  voxel,
  feet: [3.5, 11.25, -2.123456],
  view: { yaw: 90, pitch: -30.55555 },
  world: { seed: 18446744073709551615n, generatorVersion: 6 },
  build: { version: '0.4.2', sha: 'abc1234' },
};

describe('block dump', () => {
  it('holds the cube around the target by palette index, bottom to top, north to south, west to east', () => {
    const d = blockDump({ ...base, target: { cell: [3, 10, -4], face: 2 } });
    expect(d.kind).toBe('dwell-block-dump');
    expect(d.origin).toEqual([1, 8, -6]);
    expect(d.size).toBe(5);
    expect(d.layers).toHaveLength(5);
    const at = (x: number, y: number, z: number) =>
      d.palette[d.layers[y - 8]?.[z + 6]?.[x - 1] ?? -1];
    expect(at(3, 10, -4)).toBe(
      'dwell:grass_slope[facing=east,flooded=false,half=bottom,shape=wedge]',
    );
    expect(at(1, 8, -6)).toBe('dwell:stone');
    expect(at(5, 12, -2)).toBe('dwell:air');
    expect(new Set(d.palette).size).toBe(d.palette.length); // each state named once
  });

  it('keeps the seed exact, rounds the view, and centres on the feet without a target', () => {
    const d = blockDump({ ...base, target: null, radius: 1 });
    expect(d.world).toEqual({ seed: '18446744073709551615', generatorVersion: 6 });
    expect(d.player).toEqual({ feet: [3.5, 11.25, -2.123], yaw: 90, pitch: -30.556 });
    expect(d.target).toBeNull();
    expect(d.origin).toEqual([2, 9, -4]); // the cell under the feet (3, 10, -3), one each side
    expect(d.size).toBe(3);
  });

  it('formats as JSON that parses back to the same dump', () => {
    const d = blockDump({ ...base, target: { cell: [3, 10, -4], face: 5 } });
    const text = formatBlockDump(d);
    expect(JSON.parse(text)).toEqual(d);
    expect(text).toContain('\n      [0,'); // one row per line
  });
});
