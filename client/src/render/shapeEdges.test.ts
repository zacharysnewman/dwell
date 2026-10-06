import { describe, expect, it } from 'vitest';
import { SHAPES, parseState, STATE_DEFS } from '../world/blocks';
import { shapeEdges } from './shapeEdges';

function shapeOf(text: string) {
  const parsed = parseState(text);
  if ('error' in parsed) throw new Error(parsed.error);
  return SHAPES[STATE_DEFS[parsed.id]?.shape ?? 0];
}

describe('shapeEdges', () => {
  it('outlines a cube with its twelve edges', () => {
    const cube = shapeOf('dwell:stone');
    if (!cube) throw new Error('no cube');
    expect(shapeEdges(cube).length / 6).toBe(12);
  });

  it('outlines a wedge with nine edges: no diagonal across its sloped quad', () => {
    const wedge = shapeOf('dwell:stone_slope[facing=east,half=bottom,shape=wedge]');
    if (!wedge) throw new Error('no wedge');
    expect(shapeEdges(wedge).length / 6).toBe(9);
  });

  it('keeps the hip line of an outer corner, where the two faces meet at an angle', () => {
    const outer = shapeOf('dwell:stone_slope[facing=east,half=bottom,shape=outer]');
    if (!outer) throw new Error('no outer corner');
    const edges = shapeEdges(outer);
    // A pyramid-like piece: the hip from the high corner (0,1,0) to the low one (1,0,1).
    let hip = false;
    for (let i = 0; i < edges.length; i += 6) {
      const s = [...edges.subarray(i, i + 6)].join();
      if (s === '0,1,0,1,0,1' || s === '1,0,1,0,1,0') hip = true;
    }
    expect(hip).toBe(true);
  });
});
