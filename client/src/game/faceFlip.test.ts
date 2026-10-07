import { describe, expect, it } from 'vitest';
import { FaceFlip, FLIP_SECONDS } from './faceFlip';

describe('FaceFlip.draw', () => {
  it('rests at 0 and starts at a half turn when the face changes, easing back over half a second', () => {
    const flip = new FaceFlip();
    expect(flip.draw(1, 0.016)).toBe(0);
    expect(flip.draw(1, 0.016)).toBe(0);
    // The first frame on the other face: nearly the old orientation (almost π).
    const first = flip.draw(-1, 0.016);
    expect(first).toBeGreaterThan(Math.PI * 0.99);
    // Monotone, reaching 0 after FLIP_SECONDS and staying there.
    let last = first;
    let t = 0.016;
    while (t < FLIP_SECONDS) {
      const r = flip.draw(-1, 0.016);
      expect(r).toBeLessThanOrEqual(last + 1e-12);
      last = r;
      t += 0.016;
    }
    expect(flip.draw(-1, 0.016)).toBe(0);
    expect(flip.draw(-1, 5)).toBe(0);
  });

  it('turns over once per crossing, either way, and midway is a quarter turn', () => {
    const flip = new FaceFlip();
    flip.draw(-1, 1);
    const half = flip.draw(1, FLIP_SECONDS / 2);
    expect(half).toBeCloseTo(Math.PI / 2, 6);
    expect(flip.draw(1, FLIP_SECONDS)).toBe(0);
    expect(flip.draw(-1, 0.01)).toBeGreaterThan(Math.PI * 0.99);
  });
});

describe('FaceFlip.tick', () => {
  it('turns the heading by 180° on each face change, either way, and not on the first call', () => {
    const flip = new FaceFlip();
    expect(flip.tick(-1)).toBe(0);
    expect(flip.tick(-1)).toBe(0);
    expect(flip.tick(1)).toBe(180);
    expect(flip.tick(1)).toBe(0);
    expect(flip.tick(-1)).toBe(180);
  });
});
