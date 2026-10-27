import { describe, expect, it } from 'vitest';
import {
  clampFlySpeedLevel,
  FLY_BASE_SPEED,
  flySpeedFactor,
  flySpeedFloor,
  flySpeedLabel,
  formatSpeed,
  MAX_FLY_SPEED_LEVEL,
} from './flightSpeed';

describe('flight speed levels (mirror dwell-physics fly_speed_factor)', () => {
  it('double every two levels', () => {
    expect(flySpeedFactor(0)).toBe(1);
    expect(flySpeedFactor(1)).toBe(Math.SQRT2);
    expect(flySpeedFactor(2)).toBe(2);
    expect(flySpeedFactor(11)).toBe(32 * Math.SQRT2);
    expect(flySpeedFactor(20)).toBe(flySpeedFactor(MAX_FLY_SPEED_LEVEL));
  });

  it('the top level is one terrain can stream for (owner, 2026-10-26)', () => {
    expect(MAX_FLY_SPEED_LEVEL).toBe(11);
    expect(flySpeedFloor(MAX_FLY_SPEED_LEVEL)).toBe(FLY_BASE_SPEED * 32 * Math.SQRT2);
    expect(flySpeedFloor(MAX_FLY_SPEED_LEVEL)).toBeLessThan(400);
  });

  it('clamp levels', () => {
    expect(clampFlySpeedLevel(-3)).toBe(0);
    expect(clampFlySpeedLevel(2.6)).toBe(3);
    expect(clampFlySpeedLevel(500)).toBe(MAX_FLY_SPEED_LEVEL);
    expect(clampFlySpeedLevel(Number.NaN)).toBe(0);
  });

  it('describe speeds', () => {
    expect(formatSpeed(44.6)).toBe('45 m/s');
    expect(formatSpeed(1234)).toBe('1.2 km/s');
    expect(formatSpeed(8_156_007)).toBe('8,156 km/s');
    expect(flySpeedLabel(0)).toBe('8 m/s');
    expect(flySpeedLabel(8)).toBe('128 m/s');
  });
});
