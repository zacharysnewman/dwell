import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  DETAIL_LIMITS,
  defaultDetail,
  detailLimits,
  MOBILE_MEMORY_MAX_MB,
  sanitizeDetail,
} from './detailSettings';
import {
  defaultSettings,
  formatMetres,
  loadSettings,
  saveSettings,
  settingsJson,
  SLIDER_STEPS,
  sliderToValue,
  valueToSlider,
} from './settingsMenu';

describe('settings menu sliders', () => {
  // A log range as the fog's distance slider had (1 km to the world's diameter).
  const min = 1000;
  const max = 100_000;

  it('span the whole range, ends included', () => {
    expect(sliderToValue(0, min, max, true)).toBeCloseTo(min);
    expect(sliderToValue(SLIDER_STEPS, min, max, true)).toBeCloseTo(max);
    expect(sliderToValue(SLIDER_STEPS / 2, 0, 1, false)).toBeCloseTo(0.5);
  });

  it('spread a log range evenly by ratio: the middle is the geometric mean', () => {
    expect(sliderToValue(SLIDER_STEPS / 2, min, max, true)).toBeCloseTo(Math.sqrt(min * max));
  });

  it('round-trip a value to its slider position', () => {
    for (const v of [min, 10_000, 30_000, max]) {
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
      detail: { distanceM: 200.4, pixelError: 2.46, memoryMb: 511.7 },
      exposure: 1.2345,
      audio: { master: 0.8049, effects: 1, ui: 0.333 },
      clouds: 'low',
      water: { quality: 'medium' },
      motion: { speedEffects: false, fovBoost: 0.456, plantSway: false },
      fovDeg: 82.4,
      showArms: false,
    });
    const parsed = JSON.parse(json) as { detail: unknown };
    expect(parsed).toEqual({
      detail: { distanceM: 200, pixelError: 2.5, memoryMb: 512 },
      exposure: 1.23,
      audio: { master: 0.8, effects: 1, ui: 0.33 },
      clouds: 'low',
      water: { quality: 'medium' },
      motion: { speedEffects: false, fovBoost: 0.46, plantSway: false },
      fovDeg: 82,
      showArms: false,
    });
    expect(sanitizeDetail(parsed.detail, defaultDetail(false))).toEqual({
      distanceM: 200,
      pixelError: 2.5,
      memoryMb: 512,
    });
  });
});

describe('the fog', () => {
  const store = new Map<string, string>();
  beforeEach(() => {
    store.clear();
    vi.stubGlobal('localStorage', {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => store.set(k, v),
    });
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("is not a setting: an older build's stored fog is neither read nor shared, and kept", () => {
    const old = JSON.stringify({ distanceM: 4124, density: 0.73, heightM: 1500 });
    store.set('dwell.fog', old);
    const s = loadSettings(false);
    expect(s).not.toHaveProperty('fog');
    expect(defaultSettings(true)).not.toHaveProperty('fog');
    expect(JSON.parse(settingsJson(s))).not.toHaveProperty('fog');
    saveSettings({ ...s, exposure: 1.5 });
    expect(store.get('dwell.fog')).toBe(old);
    expect(store.get('dwell.exposure')).toBe('1.5');
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

  it('stop the distant memory lower on phones, where too much closes the tab', () => {
    // Playtest: mobile Safari reloaded the page (out of memory) with the sliders raised.
    expect(detailLimits(false)).toEqual(DETAIL_LIMITS);
    expect(detailLimits(true).memoryMb).toEqual({ min: 32, max: MOBILE_MEMORY_MAX_MB });
    expect(MOBILE_MEMORY_MAX_MB).toBeLessThan(DETAIL_LIMITS.memoryMb.max);
    // A value kept from before (or set on a desktop) comes down to the phone's ceiling.
    expect(
      sanitizeDetail({ memoryMb: 1024 }, defaultDetail(true), detailLimits(true)).memoryMb,
    ).toBe(MOBILE_MEMORY_MAX_MB);
  });
});
