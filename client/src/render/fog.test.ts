import { describe, expect, it } from 'vitest';
import { Players, World } from '../protocol/constants.gen';
import { DEFAULT_FOG, FOG_LIMITS, hazeAmount, sanitizeFog } from './fog';

const EYE = 1.6;

describe('height fog', () => {
  it('hazes the horizon from the ground, lightly: never past the density', () => {
    const near = hazeAmount(DEFAULT_FOG, EYE, 0, 1000);
    const far = hazeAmount(DEFAULT_FOG, EYE, 0, 400_000);
    expect(near).toBeLessThan(0.01);
    expect(far).toBeGreaterThan(0.5);
    expect(hazeAmount(DEFAULT_FOG, EYE, 0, 2 * World.worldRadius)).toBeLessThanOrEqual(
      DEFAULT_FOG.density,
    );
  });

  it('reaches half its density at the distance, at sea level', () => {
    const fog = { ...DEFAULT_FOG, distanceM: 250_000 };
    expect(hazeAmount(fog, 0, 0, 250_000)).toBeCloseTo(fog.density / 2, 6);
  });

  it('thins with altitude: the same view is clearer from higher up', () => {
    let previous = Infinity;
    for (const height of [0, 1000, 5000, 20_000, 200_000, 2_000_000]) {
      // A point on the ground a fixed 300 km sideways.
      const d = Math.hypot(300_000, height);
      const haze = hazeAmount(DEFAULT_FOG, height, 0, d);
      expect(haze).toBeLessThan(previous);
      previous = haze;
    }
  });

  it('leaves mountain tops clearer than valleys at the same distance', () => {
    expect(hazeAmount(DEFAULT_FOG, 4000, 4000, 100_000)).toBeLessThan(
      hazeAmount(DEFAULT_FOG, 0, 0, 100_000) / 10,
    );
  });

  it('shows the whole world clearly from the flight ceiling', () => {
    const top = Players.flightCeiling;
    for (const r of [0, World.worldRadius / 2, World.worldRadius]) {
      expect(hazeAmount(DEFAULT_FOG, top, 0, Math.hypot(r, top))).toBeLessThan(0.05);
    }
  });

  it('stays finite at the extremes of every setting', () => {
    for (const heightM of [FOG_LIMITS.heightM.min, FOG_LIMITS.heightM.max]) {
      for (const distanceM of [FOG_LIMITS.distanceM.min, FOG_LIMITS.distanceM.max]) {
        const fog = { distanceM, density: 1, heightM };
        for (const [cam, point] of [
          [Players.flightCeiling, World.worldMinY],
          [World.worldMinY, World.worldMinY],
          [World.worldMinY, Players.flightCeiling],
        ] as const) {
          const haze = hazeAmount(fog, cam, point, 2 * World.worldRadius);
          expect(Number.isFinite(haze)).toBe(true);
          expect(haze).toBeGreaterThanOrEqual(0);
          expect(haze).toBeLessThanOrEqual(1);
        }
      }
    }
  });

  it('is off at zero density', () => {
    expect(hazeAmount({ ...DEFAULT_FOG, density: 0 }, 0, 0, 1e7)).toBe(0);
  });

  it('reaches out to the whole world: the distance goes up to its diameter', () => {
    expect(FOG_LIMITS.distanceM.max).toBe(2 * World.worldRadius);
  });

  it('sanitizes stored settings into range, defaulting anything missing', () => {
    expect(sanitizeFog(null)).toEqual(DEFAULT_FOG);
    expect(sanitizeFog({ distanceM: 1e12, density: -1, heightM: 'x' })).toEqual({
      distanceM: FOG_LIMITS.distanceM.max,
      density: 0,
      heightM: DEFAULT_FOG.heightM,
    });
  });
});
