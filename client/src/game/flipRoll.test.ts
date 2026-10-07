import { describe, expect, it } from 'vitest';
import { FlipRoll, ROLL_SECONDS } from './flipRoll';

describe('FlipRoll', () => {
  it('rests at 0 and starts at a half turn when the face changes, easing back over half a second', () => {
    const roll = new FlipRoll();
    expect(roll.update(1, 0.016)).toBe(0);
    expect(roll.update(1, 0.016)).toBe(0);
    // The first frame on the other face: nearly the old orientation (a roll of almost π).
    const first = roll.update(-1, 0.016);
    expect(first).toBeGreaterThan(Math.PI * 0.99);
    // Monotone, reaching 0 after ROLL_SECONDS and staying there.
    let last = first;
    let t = 0.016;
    while (t < ROLL_SECONDS) {
      const r = roll.update(-1, 0.016);
      expect(r).toBeLessThanOrEqual(last + 1e-12);
      last = r;
      t += 0.016;
    }
    expect(roll.update(-1, 0.016)).toBe(0);
    expect(roll.update(-1, 5)).toBe(0);
  });

  it('turns over once per crossing, either way, and midway is a quarter turn', () => {
    const roll = new FlipRoll();
    roll.update(-1, 1);
    const half = roll.update(1, ROLL_SECONDS / 2);
    expect(half).toBeCloseTo(Math.PI / 2, 6);
    expect(roll.update(1, ROLL_SECONDS)).toBe(0);
    expect(roll.update(-1, 0.01)).toBeGreaterThan(Math.PI * 0.99);
  });
});
