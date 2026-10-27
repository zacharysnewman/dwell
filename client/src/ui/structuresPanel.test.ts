import { describe, expect, it } from 'vitest';
import { stateId } from '../world/blocks';
import { checkStructure, type Vec3 } from '../local/structures';
import { parseCell, saveBox, saveFromWorld } from './structuresPanel';

const air = stateId('dwell:air');
const floor = stateId('dwell:sandbox_floor');
const stone = stateId('dwell:stone');

/** A sandbox: the floor below y = 0, a 2 × 3 × 1 stone wall at (10..11, 0..2, 5). */
function sandbox(built = true) {
  const at = (x: number, y: number, z: number) => {
    if (built && x >= 10 && x <= 11 && y >= 0 && y <= 2 && z === 5) return stone;
    return y < 0 ? floor : air;
  };
  return (min: Vec3, size: Vec3) => {
    const out: number[] = [];
    for (let y = 0; y < size[1]; y++)
      for (let z = 0; z < size[2]; z++)
        for (let x = 0; x < size[0]; x++) out.push(at(min[0] + x, min[1] + y, min[2] + z));
    return out;
  };
}

const deps = (read = sandbox(), selection: { min: Vec3; size: Vec3 } | null = null) => ({
  read,
  selection: () => selection,
  feet: (): Vec3 => [8.5, 0, 3.5],
});

describe('structures page', () => {
  it('reads cells typed as three whole numbers', () => {
    expect(parseCell('1 2 -3')).toEqual([1, 2, -3]);
    expect(parseCell(' 4, 5,6 ')).toEqual([4, 5, 6]);
    expect(parseCell('1 2')).toBeNull();
    expect(parseCell('1 2 x')).toBeNull();
    expect(parseCell('1.5 2 3')).toBeNull();
  });

  it('saves what was built near the player, the selection, or between corners', () => {
    expect(saveBox('built', deps(), ['', ''])).toEqual({ min: [10, 0, 5], size: [2, 3, 1] });
    expect(saveBox('built', deps(sandbox(false)), ['', ''])).toEqual({
      error: 'Nothing built near you yet.',
    });
    const sel = { min: [0, 0, 0] as Vec3, size: [4, 4, 4] as Vec3 };
    expect(saveBox('selection', deps(sandbox(), sel), ['', ''])).toEqual(sel);
    expect(saveBox('selection', deps(), ['', ''])).toHaveProperty('error');
    expect(saveBox('corners', deps(), ['12 2 5', '10 0 5'])).toEqual({
      min: [10, 0, 5],
      size: [3, 3, 1],
    });
    expect(saveBox('corners', deps(), ['0 0 0', '32 0 0'])).toEqual({
      error: 'That is 33 × 1 × 1: a structure is at most 32 across and 64 tall.',
    });
  });

  it('makes a valid structure with the anchor at the bottom centre unless one is given', () => {
    const read = sandbox();
    const box = { min: [10, 0, 5] as Vec3, size: [2, 3, 1] as Vec3 };
    const made = saveFromWorld(read, box, 'Stone Wall', '');
    if ('error' in made) throw new Error(made.error);
    expect(made.json).toMatchObject({ name: 'dwell:stone_wall', anchor: [1, 0, 0] });
    expect(checkStructure(made.json, 'stone_wall.json').cells).toBe('ssssss');
    const anchored = saveFromWorld(read, box, 'wall', '0 2 0');
    expect('json' in anchored && anchored.json.anchor).toEqual([0, 2, 0]);
    expect(saveFromWorld(read, box, 'wall', '2 0 0')).toHaveProperty('error');
    expect(saveFromWorld(read, box, '', '')).toEqual({ error: 'Give the structure a name.' });
    expect(saveFromWorld(() => [], box, 'wall', '')).toEqual({
      error: 'Part of that box is not loaded: go closer.',
    });
  });
});
