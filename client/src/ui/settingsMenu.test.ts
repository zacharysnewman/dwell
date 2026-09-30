import { describe, expect, it } from 'vitest';
import { FOG_LIMITS, sanitizeFog } from '../render/fog';
import { fogJson, formatMetres, SLIDER_STEPS, sliderToValue, valueToSlider } from './settingsMenu';

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
    const json = fogJson({ distanceM: 11313.708, density: 0.6049, heightM: 1499.6 });
    expect(JSON.parse(json)).toEqual({ fog: { distanceM: 11314, density: 0.6, heightM: 1500 } });
    const { fog } = JSON.parse(json) as { fog: unknown };
    expect(sanitizeFog(fog)).toEqual({ distanceM: 11314, density: 0.6, heightM: 1500 });
  });
});
