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

describe('flight speed levels (mirror C++ FlySpeedFactor)', () => {
  it('double every two levels', () => {
    expect(flySpeedFactor(0)).toBe(1);
    expect(flySpeedFactor(1)).toBe(Math.SQRT2);
    expect(flySpeedFactor(2)).toBe(2);
    expect(flySpeedFactor(20)).toBe(1024);
    expect(flySpeedFactor(MAX_FLY_SPEED_LEVEL)).toBe(Math.SQRT2 * 2 ** 19);
  });

  it('reach about the speed near the flight ceiling at the top', () => {
    // The height-based factor at 24,000 km is 1 + 24e6 / 32 ≈ 750,000.
    expect(flySpeedFactor(MAX_FLY_SPEED_LEVEL)).toBeLessThanOrEqual(1 + 24e6 / 32);
    expect(flySpeedFactor(MAX_FLY_SPEED_LEVEL)).toBeGreaterThan(0.95 * (1 + 24e6 / 32));
    expect(flySpeedFloor(MAX_FLY_SPEED_LEVEL)).toBeCloseTo(FLY_BASE_SPEED * 741455.2, -2);
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
    expect(flySpeedLabel(0)).toBe('Normal (faster with height)');
    expect(flySpeedLabel(8)).toBe('At least 176 m/s');
  });
});
