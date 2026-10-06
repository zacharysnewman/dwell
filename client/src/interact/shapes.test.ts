import { describe, expect, it } from 'vitest';
import { STATE_DEFS, stateId } from '../world/blocks';
import {
  PIECES,
  facingToward,
  hasShapes,
  hitFractionY,
  pieceLabel,
  pieceState,
  placementHalf,
} from './shapes';

describe('slope placement', () => {
  it('lets a slope rise away from the player: it descends toward them', () => {
    expect(facingToward(0)).toBe('north'); // looking +Z: descends toward −Z
    expect(facingToward(90)).toBe('west'); // looking +X
    expect(facingToward(180)).toBe('south');
    expect(facingToward(-90)).toBe('east');
    expect(facingToward(30)).toBe('north');
    expect(facingToward(60)).toBe('west');
    expect(facingToward(359)).toBe('north');
  });

  it('hangs a piece from the ceiling against an underside or the upper half of a side', () => {
    expect(placementHalf(3, 0)).toBe('top'); // under a block
    expect(placementHalf(2, 1)).toBe('bottom'); // on top of a block
    expect(placementHalf(0, 0.8)).toBe('top');
    expect(placementHalf(5, 0.2)).toBe('bottom');
    expect(placementHalf(1, 0.5)).toBe('bottom'); // the exact middle stays upright
  });

  it('finds the height where the view ray meets a side face', () => {
    // Looking east and a little down from (0, 2, 0.5) at the −X face of the block at (3, 0, 0):
    // the ray meets x = 3 at y = 2 − 0.5 × 3.
    expect(hitFractionY([0, 2, 0.5], [1, -0.5, 0], [3, 0, 0], 1)).toBeCloseTo(0.5);
    // Along −Z at the +Z face of the block at (0, 0, 0).
    expect(hitFractionY([0.5, 0.9, 4], [0, 0, -1], [0, 0, 0], 4)).toBeCloseTo(0.9);
    expect(hitFractionY([0.5, 5, 4], [0, 0, -1], [0, 0, 0], 4)).toBe(1); // clamped
  });

  it('names the states of every piece for the shapeable materials', () => {
    expect(hasShapes('dwell:stone')).toBe(true);
    expect(hasShapes('dwell:grass')).toBe(true);
    expect(hasShapes('dwell:leaves')).toBe(false);
    expect(hasShapes('dwell:coal_ore')).toBe(false);
    expect(hasShapes('dwell:ladder')).toBe(false);
    expect(pieceState('dwell:leaves', 'wedge', 'north', 'bottom')).toBeNull();
    expect(pieceState('dwell:stone', 'cube', 'north', 'bottom')).toBeNull(); // the slot's own state
    for (const block of ['dwell:stone', 'dwell:dirt', 'dwell:grass', 'dwell:log']) {
      for (const piece of PIECES.slice(1)) {
        for (const facing of ['north', 'east', 'south', 'west'] as const) {
          for (const half of ['bottom', 'top'] as const) {
            const id = pieceState(block, piece, facing, half);
            expect(id, `${block} ${piece} ${facing} ${half}`).not.toBeNull();
            const def = STATE_DEFS[id ?? 0];
            expect(def?.placeable).toBe(true);
            expect(def?.flooded).toBe(false);
            expect(def?.values.half).toBe(half);
            if (piece !== 'slab') {
              expect(def?.values.shape).toBe(piece);
              expect(def?.values.facing).toBe(facing);
            }
          }
        }
      }
    }
    expect(pieceState('dwell:stone', 'slab', 'north', 'top')).toBe(
      stateId('dwell:stone_slab[flooded=false,half=top]'),
    );
  });

  it('labels pieces for the hotbar', () => {
    expect(pieceLabel('gentle_outer_high')).toBe('gentle outer high');
    expect(PIECES[0]).toBe('cube');
  });
});
