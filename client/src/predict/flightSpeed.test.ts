import { describe, expect, it } from 'vitest';
import { World } from '../protocol/constants.gen';
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
    expect(flySpeedFactor(20)).toBe(1024);
    expect(flySpeedFactor(MAX_FLY_SPEED_LEVEL)).toBe(1024);
  });

  it('cross the 50 km world in seconds at the top (Phase 16)', () => {
    expect(MAX_FLY_SPEED_LEVEL).toBe(20);
    expect(flySpeedFloor(MAX_FLY_SPEED_LEVEL)).toBe(FLY_BASE_SPEED * 1024);
    expect((2 * World.worldRadius) / flySpeedFloor(MAX_FLY_SPEED_LEVEL)).toBeLessThan(10);
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
