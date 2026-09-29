import { describe, expect, it } from 'vitest';
import { fogRange } from './fog';

describe('distance fog', () => {
  it('lets the ground see about 512 km, as far as the clearest air on Earth', () => {
    expect(fogRange(0).far).toBe(512_000);
    expect(fogRange(3000).far).toBe(512_000); // a mountain top too
    // Terrain is still clearly visible at 100 km, and faint (not gone) at 480 km.
    const { near, far } = fogRange(0);
    const fade = (d: number) => Math.min(1, Math.max(0, (d - near) / (far - near)));
    expect(fade(100_000)).toBeLessThan(0.2);
    expect(fade(480_000)).toBeLessThan(1);
  });

  it('scales with height far above the terrain, so the disc stays visible from orbit', () => {
    const { near, far } = fogRange(24_000_000);
    expect(near).toBe(48_000_000);
    expect(far).toBe(960_000_000);
  });
});
