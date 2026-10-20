import { describe, expect, it } from 'vitest';
import { displayScale } from './displayScale';

describe('the drawing-buffer scale', () => {
  it('reads ?scale=, clamped, and is 1 without it', () => {
    expect(displayScale('?scale=0.5')).toBe(0.5);
    expect(displayScale('')).toBe(1);
    expect(displayScale('?scale=9')).toBe(2);
    expect(displayScale('?scale=0.1')).toBe(0.25);
    expect(displayScale('?scale=wide')).toBe(1);
  });
});
