import { describe, expect, it } from 'vitest';
import { FpsMeter } from './fps';

describe('FPS meter', () => {
  it('reports frames per second over each full second', () => {
    const m = new FpsMeter();
    expect(m.fps).toBeNull();
    for (let t = 0; t <= 1020; t += 1000 / 60) m.frame(t);
    expect(m.fps).toBeCloseTo(60, 0);
    // Slowing to 10 fps shows after the next full second, not frame by frame.
    for (let t = 1100; t <= 2100; t += 100) m.frame(t);
    expect(m.fps).toBeCloseTo(10, 0);
  });
});
