import { describe, expect, it } from 'vitest';
import { FpsMeter } from './fps';

describe('FPS meter', () => {
  it('reports frames per second over each full second', () => {
    const m = new FpsMeter();
    expect(m.fps).toBeNull();
    for (let t = 0; t <= 1020; t += 1000 / 60) m.frame(t, true);
    expect(m.fps).toBeCloseTo(60, 0);
    // Slowing to 10 fps shows after the next full second, not frame by frame.
    for (let t = 1100; t <= 2100; t += 100) m.frame(t, true);
    expect(m.fps).toBeCloseTo(10, 0);
  });

  it('counts only the frames drawn, not those skipped while the GPU is behind', () => {
    // A phone at 60 animation frames a second drawing every other one (owner, 2026-11-06: the
    // status line read 60 while the game ran at 30).
    const m = new FpsMeter();
    let n = 0;
    for (let t = 0; t <= 1020; t += 1000 / 60) m.frame(t, n++ % 2 === 0);
    expect(m.fps).toBeCloseTo(30, 0);
    expect(m.animationFps).toBeCloseTo(60, 0);
  });
});
