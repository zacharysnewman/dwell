import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  defaultWater,
  sanitizeWater,
  WATER_QUALITIES,
  waterQualityLevel,
  waterRecord,
} from './waterSettings';
import { defaultSettings, loadSettings, saveSettings, settingsJson } from './settingsMenu';

describe('water settings', () => {
  it('default to Low on phones and Medium on desktops', () => {
    expect(defaultWater(true)).toEqual({
      quality: 'low',
      flowRipples: true,
      flowFoam: true,
      flowParticles: true,
    });
    expect(defaultWater(false)).toEqual({
      quality: 'medium',
      flowRipples: true,
      flowFoam: true,
      flowParticles: true,
    });
  });

  it('take the default for anything missing or unknown', () => {
    const fallback = defaultWater(false);
    expect(sanitizeWater({ quality: 'medium' }, fallback)).toEqual(fallback);
    expect(sanitizeWater({ quality: 'ultra' }, fallback)).toEqual(fallback);
    expect(sanitizeWater(null, fallback)).toEqual(fallback);
    expect(sanitizeWater('low', fallback)).toEqual(fallback);
  });

  it('are the renderer levels 0 to 2 in order', () => {
    expect(WATER_QUALITIES.map(waterQualityLevel)).toEqual([0, 1, 2]);
  });

  it('keep the fields a newer build stored when this one rewrites the record', () => {
    const stored = { quality: 'high', ssr: 'half', future: { a: 1 } };
    const low = { ...defaultWater(true), flowFoam: false };
    expect(waterRecord(stored, low)).toEqual({
      ...low,
      ssr: 'half',
      future: { a: 1 },
    });
    // Not a record (missing, corrupt): only this build's fields.
    expect(waterRecord(null, low)).toEqual(low);
    expect(waterRecord([1, 2], low)).toEqual(low);
  });

  it('read the flow switches (0.49.5) where stored, and default them on where not', () => {
    const fallback = defaultWater(false);
    // An older build's record: no flow fields.
    expect(sanitizeWater({ quality: 'high' }, fallback)).toEqual({ ...fallback, quality: 'high' });
    expect(
      sanitizeWater(
        { quality: 'low', flowRipples: false, flowFoam: true, flowParticles: false },
        fallback,
      ),
    ).toEqual({ quality: 'low', flowRipples: false, flowFoam: true, flowParticles: false });
    // Not a boolean: the default.
    expect(sanitizeWater({ flowFoam: 'no' }, fallback).flowFoam).toBe(true);
  });
});

describe('the settings store with water (append-only)', () => {
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

  it('reads an older store without water as the default, and keeps unknown fields on save', () => {
    store.set('dwell.exposure', '1.5');
    const loaded = loadSettings(true);
    expect(loaded.water).toEqual(defaultWater(true));
    expect(loaded.exposure).toBe(1.5);

    store.set('dwell.water', JSON.stringify({ quality: 'medium', reflections: 'ssr' }));
    const s = loadSettings(false);
    expect(s.water.quality).toBe('medium');
    saveSettings({ ...s, water: { ...s.water, quality: 'high' } });
    expect(JSON.parse(store.get('dwell.water') ?? '')).toEqual({
      ...defaultWater(false),
      quality: 'high',
      reflections: 'ssr',
    });
    expect(loadSettings(false).water.quality).toBe('high');
  });

  it('shares the water quality and flow switches in the settings JSON', () => {
    const json = JSON.parse(settingsJson(defaultSettings(true))) as { water: unknown };
    expect(json.water).toEqual(defaultWater(true));
  });
});
