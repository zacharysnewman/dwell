import { describe, expect, it } from 'vitest';
import { FOG_LIMITS, sanitizeFog } from '../render/fog';
import { DETAIL_LIMITS, defaultDetail, sanitizeDetail } from '../lod/detail';
import {
  formatMetres,
  settingsJson,
  SLIDER_STEPS,
  sliderToValue,
  valueToSlider,
} from './settingsMenu';

describe('settings menu sliders', () => {
  const { min, max } = FOG_LIMITS.distanceM;

  it('span the whole range, ends included', () => {
    expect(sliderToValue(0, min, max, true)).toBeCloseTo(min);
    expect(sliderToValue(SLIDER_STEPS, min, max, true)).toBeCloseTo(max);
    expect(sliderToValue(SLIDER_STEPS / 2, 0, 1, false)).toBeCloseTo(0.5);
  });

  it('spread a log range evenly by ratio: the middle is the geometric mean', () => {
    expect(sliderToValue(SLIDER_STEPS / 2, min, max, true)).toBeCloseTo(Math.sqrt(min * max));
  });

  it('round-trip a value to its slider position', () => {
    for (const v of [min, 100_000, 3_000_000, max]) {
      const back = sliderToValue(valueToSlider(v, min, max, true), min, max, true);
      expect(Math.abs(back - v) / v).toBeLessThan(0.01);
    }
    expect(valueToSlider(1e15, min, max, true)).toBe(SLIDER_STEPS);
  });

  it('label distances in metres or kilometres', () => {
    expect(formatMetres(850)).toBe('850 m');
    expect(formatMetres(1500)).toBe('1.5 km');
    expect(formatMetres(120_400)).toBe('120 km');
    expect(formatMetres(16_384_000)).toBe('16,384 km');
  });
});

describe('settings JSON', () => {
  it('rounds the settings for sharing, and reads back as the same settings', () => {
    const json = settingsJson({
      fog: { distanceM: 11313.708, density: 0.6049, heightM: 1499.6 },
      detail: { distanceM: 200.4, pixelError: 2.46, memoryMb: 511.7 },
    });
    const parsed = JSON.parse(json) as { fog: unknown; detail: unknown };
    expect(parsed).toEqual({
      fog: { distanceM: 11314, density: 0.6, heightM: 1500 },
      detail: { distanceM: 200, pixelError: 2.5, memoryMb: 512 },
    });
    expect(sanitizeFog(parsed.fog)).toEqual({ distanceM: 11314, density: 0.6, heightM: 1500 });
    expect(sanitizeDetail(parsed.detail, defaultDetail(false))).toEqual({
      distanceM: 200,
      pixelError: 2.5,
      memoryMb: 512,
    });
  });
});

describe('detail settings', () => {
  it('span their ranges, defaulting by device', () => {
    expect(DETAIL_LIMITS.distanceM).toEqual({ min: 96, max: 352 });
    expect(DETAIL_LIMITS.pixelError).toEqual({ min: 1, max: 16 });
    expect(DETAIL_LIMITS.memoryMb).toEqual({ min: 32, max: 1024 });
    // The LOD's defaults are the protocol constants (LOD_PIXEL_ERROR, LOD_CACHE_MB).
    expect(defaultDetail(false)).toEqual({ distanceM: 256, pixelError: 4, memoryMb: 256 });
    expect(defaultDetail(true)).toEqual({ distanceM: 128, pixelError: 8, memoryMb: 96 });
  });

  it('clamp stored values, and take the default for anything missing or not a number', () => {
    const mobile = defaultDetail(true);
    expect(sanitizeDetail({ distanceM: 5000, pixelError: 0.1, memoryMb: 1e6 }, mobile)).toEqual({
      distanceM: 352,
      pixelError: 1,
      memoryMb: 1024,
    });
    expect(sanitizeDetail({ distanceM: 'far', pixelError: 3 }, mobile)).toEqual({
      distanceM: 128,
      pixelError: 3,
      memoryMb: 96,
    });
    // Settings kept before the LOD sliders existed keep their distance.
    expect(sanitizeDetail({ distanceM: 200 }, defaultDetail(false))).toEqual({
      distanceM: 200,
      pixelError: 4,
      memoryMb: 256,
    });
    expect(sanitizeDetail(null, defaultDetail(false))).toEqual(defaultDetail(false));
  });
});
