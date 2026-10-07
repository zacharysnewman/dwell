import { describe, expect, it } from 'vitest';
import { MOON_DIRECTION_WORLD, SUN_DIRECTION, type Rgb } from './look';
import { SKY_AXIS, SKY_REST, skyAngle, skyFrame, skySwitches } from './skyFrame';

const dot = (a: Rgb, b: Rgb): number => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const near = (a: Rgb, b: Rgb): void => {
  for (let i = 0; i < 3; i++) expect(a[i] ?? 0).toBeCloseTo(b[i] ?? 0, 9);
};

describe('skyFrame', () => {
  it('is the static sky at angle 0', () => {
    near(SKY_REST.dayPole, [0, 1, 0]);
    near(SKY_REST.sun, SUN_DIRECTION);
    near(SKY_REST.moon, MOON_DIRECTION_WORLD);
  });

  it('turns day and night over at π: the pole flips and the sun is where the moon was', () => {
    const f = skyFrame(Math.PI);
    near(f.dayPole, [0, -1, 0]);
    near(f.sun, MOON_DIRECTION_WORLD);
    near(f.moon, SUN_DIRECTION);
  });

  it('keeps unit vectors and their mutual angles; the axis is horizontal and ⟂ the sun’s azimuth', () => {
    expect(SKY_AXIS[1]).toBe(0);
    expect(dot(SKY_AXIS, [SUN_DIRECTION[0], 0, SUN_DIRECTION[2]])).toBeCloseTo(0, 12);
    for (const angle of [0.3, 1, 2.5, 4, 6]) {
      const f = skyFrame(angle);
      for (const v of [f.dayPole, f.sun, f.moon]) expect(Math.hypot(...v)).toBeCloseTo(1, 12);
      expect(dot(f.dayPole, f.sun)).toBeCloseTo(dot(SKY_REST.dayPole, SKY_REST.sun), 12);
      expect(dot(f.sun, f.moon)).toBeCloseTo(-1, 12);
    }
  });
});

describe('the sky angle', () => {
  it('reads ?skyrot and ?skyspin, ignoring what is not a number', () => {
    expect(skySwitches('?skyrot=90&skyspin=-12.5')).toEqual({
      rotateDeg: 90,
      spinDegPerSecond: -12.5,
    });
    expect(skySwitches('?skyrot=abc')).toEqual({ rotateDeg: 0, spinDegPerSecond: 0 });
    expect(skySwitches('')).toEqual({ rotateDeg: 0, spinDegPerSecond: 0 });
  });

  it('adds the slider, the fixed angle and the spin, wrapped to a turn', () => {
    const none = { rotateDeg: 0, spinDegPerSecond: 0 };
    expect(skyAngle(0, none, 100)).toBe(0);
    expect(skyAngle(90, { rotateDeg: 90, spinDegPerSecond: 0 }, 0)).toBeCloseTo(Math.PI, 12);
    expect(skyAngle(0, { rotateDeg: 0, spinDegPerSecond: 90 }, 2)).toBeCloseTo(Math.PI, 12);
    expect(skyAngle(0, { rotateDeg: -90, spinDegPerSecond: 0 }, 0)).toBeCloseTo(1.5 * Math.PI, 12);
    expect(skyAngle(360, none, 0)).toBe(0);
  });
});
