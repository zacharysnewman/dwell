import { describe, expect, it } from 'vitest';
import { World } from '../protocol/constants.gen';
import {
  DEFAULT_EXPOSURE,
  DEFAULT_FOG,
  EXPOSURE_LIMITS,
  FOG_LIMITS,
  sanitizeExposure,
  sanitizeFog,
} from './viewSettings';

describe('fog settings', () => {
  it('reach out to the whole world: the distance goes up to its diameter', () => {
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

describe('exposure', () => {
  it('clamps into range and defaults when missing', () => {
    expect(sanitizeExposure(undefined)).toBe(DEFAULT_EXPOSURE);
    expect(sanitizeExposure(Number.NaN)).toBe(DEFAULT_EXPOSURE);
    expect(sanitizeExposure(100)).toBe(EXPOSURE_LIMITS.max);
    expect(sanitizeExposure(0)).toBe(EXPOSURE_LIMITS.min);
    expect(sanitizeExposure(1.3)).toBe(1.3);
  });
});
