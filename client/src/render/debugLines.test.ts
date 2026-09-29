import { describe, expect, it } from 'vitest';
import { debugLineArrays } from './debugLines';

describe('debugLineArrays', () => {
  it('keeps millimetres ~8,000 km from the origin (vertices relative to the first point)', () => {
    // A 3 cm ground probe under a player far east: absolute float32 vertices would round both
    // ends to the same 0.5 m step.
    const x = 7999501.1;
    const { origin, positions } = debugLineArrays([
      { from: [x, 10.03, 0.5], to: [x, 10.0, 0.5], color: 0xff0000 },
      { from: [x + 0.004, 10, 0.5], to: [x + 0.1, 10, 0.5], color: 0x00ff00 },
    ]);
    expect(origin).toEqual([x, 10.03, 0.5]);
    const at = (i: number) => positions[i] ?? NaN;
    expect(at(4) - at(1)).toBeCloseTo(-0.03, 6);
    expect(origin[0] + at(6)).toBeCloseTo(x + 0.004, 6);
    expect(origin[0] + at(9)).toBeCloseTo(x + 0.1, 6);
    // What absolute float32 vertices would have drawn.
    expect(Math.fround(x + 0.004)).toBe(Math.fround(x + 0.1));
  });
});
