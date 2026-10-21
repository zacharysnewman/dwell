import { describe, expect, it } from 'vitest';
import {
  DEFAULT_EXPOSURE,
  EXPOSURE_LIMITS,
  DEFAULT_FOV,
  FOV_LIMITS,
  sanitizeExposure,
  CLOUD_QUALITIES,
  defaultClouds,
  sanitizeClouds,
  sanitizeFovSetting,
  sanitizeShowArms,
  viewRecord,
} from './viewSettings';

describe('exposure', () => {
  it('clamps into range and defaults when missing', () => {
    expect(sanitizeExposure(undefined)).toBe(DEFAULT_EXPOSURE);
    expect(sanitizeExposure(Number.NaN)).toBe(DEFAULT_EXPOSURE);
    expect(sanitizeExposure(100)).toBe(EXPOSURE_LIMITS.max);
    expect(sanitizeExposure(0)).toBe(EXPOSURE_LIMITS.min);
    expect(sanitizeExposure(1.3)).toBe(1.3);
  });
});

describe('clouds setting', () => {
  it('defaults to High on desktops and Low on phones', () => {
    expect(defaultClouds(false)).toBe('high');
    expect(defaultClouds(true)).toBe('low');
  });

  it('reads a stored choice and defaults anything else by device', () => {
    for (const q of CLOUD_QUALITIES) {
      expect(sanitizeClouds(q, false)).toBe(q);
      expect(sanitizeClouds(q, true)).toBe(q);
    }
    for (const bad of [null, undefined, 'ultra', 2, { q: 'low' }]) {
      expect(sanitizeClouds(bad, false)).toBe('high');
      expect(sanitizeClouds(bad, true)).toBe('low');
    }
    expect(CLOUD_QUALITIES).toEqual(['off', 'low', 'high']);
  });
});

describe('field of view', () => {
  it('defaults to 75°, clamps to 60–100° in whole degrees', () => {
    expect(DEFAULT_FOV).toBe(75);
    expect(sanitizeFovSetting(null)).toBe(DEFAULT_FOV);
    expect(sanitizeFovSetting({ fovDeg: 'wide' })).toBe(75);
    expect(sanitizeFovSetting({ fovDeg: 150 })).toBe(FOV_LIMITS.max);
    expect(sanitizeFovSetting({ fovDeg: 10 })).toBe(FOV_LIMITS.min);
    expect(sanitizeFovSetting({ fovDeg: 82.4 })).toBe(82);
  });

  it('is stored append-only: fields this build does not know are kept', () => {
    expect(viewRecord({ fovDeg: 75, future: [1] }, 90)).toEqual({
      fovDeg: 90,
      future: [1],
      showArms: true,
    });
    expect(viewRecord('garbage', 80, false)).toEqual({ fovDeg: 80, showArms: false });
  });

  it('shows your arms unless the record turned them off', () => {
    expect(sanitizeShowArms(null)).toBe(true);
    expect(sanitizeShowArms({ fovDeg: 80 })).toBe(true);
    expect(sanitizeShowArms({ showArms: 'no' })).toBe(true);
    expect(sanitizeShowArms({ showArms: false })).toBe(false);
    expect(sanitizeShowArms(viewRecord({}, 75, false))).toBe(false);
  });
});
