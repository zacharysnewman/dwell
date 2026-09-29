import { describe, expect, it } from 'vitest';
import { fogRange } from './fog';

describe('distance fog', () => {
  it('is off for now: nothing fades, from the ground or from orbit', () => {
    for (const height of [0, 3000, 50_000, 24_000_000]) expect(fogRange(height)).toBeNull();
  });
});
