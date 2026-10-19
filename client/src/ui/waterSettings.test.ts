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
    expect(defaultWater(true)).toEqual({ quality: 'low' });
    expect(defaultWater(false)).toEqual({ quality: 'medium' });
  });

  it('take the default for anything missing or unknown', () => {
    const fallback = defaultWater(false);
    expect(sanitizeWater({ quality: 'medium' }, fallback)).toEqual({ quality: 'medium' });
    expect(sanitizeWater({ quality: 'ultra' }, fallback)).toEqual(fallback);
    expect(sanitizeWater(null, fallback)).toEqual(fallback);
    expect(sanitizeWater('low', fallback)).toEqual(fallback);
  });

  it('are the renderer levels 0 to 2 in order', () => {
    expect(WATER_QUALITIES.map(waterQualityLevel)).toEqual([0, 1, 2]);
  });

  it('keep the fields a newer build stored when this one rewrites the record', () => {
    const stored = { quality: 'high', ssr: 'half', future: { a: 1 } };
    expect(waterRecord(stored, { quality: 'low' })).toEqual({
      quality: 'low',
      ssr: 'half',
      future: { a: 1 },
    });
    // Not a record (missing, corrupt): only this build's fields.
    expect(waterRecord(null, { quality: 'low' })).toEqual({ quality: 'low' });
    expect(waterRecord([1, 2], { quality: 'low' })).toEqual({ quality: 'low' });
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
    expect(loaded.water).toEqual({ quality: 'low' });
    expect(loaded.exposure).toBe(1.5);

    store.set('dwell.water', JSON.stringify({ quality: 'medium', reflections: 'ssr' }));
    const s = loadSettings(false);
    expect(s.water.quality).toBe('medium');
    saveSettings({ ...s, water: { quality: 'high' } });
    expect(JSON.parse(store.get('dwell.water') ?? '')).toEqual({
      quality: 'high',
      reflections: 'ssr',
    });
    expect(loadSettings(false).water.quality).toBe('high');
  });

  it('shares the water quality in the settings JSON', () => {
    const json = JSON.parse(settingsJson(defaultSettings(true))) as { water: unknown };
    expect(json.water).toEqual({ quality: 'low' });
  });
});
