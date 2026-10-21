import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  defaultShadows,
  sanitizeShadows,
  SHADOW_QUALITIES,
  shadowLevel,
  shadowRecord,
  shadowsFromQuery,
  torchShadowsFromQuery,
} from './shadowSettings';
import { defaultSettings, loadSettings, saveSettings, settingsJson } from './settingsMenu';

describe('shadow settings', () => {
  it('default to Low on phones and High on desktops', () => {
    expect(defaultShadows(true)).toEqual({ quality: 'low', torch: 'off' });
    expect(defaultShadows(false)).toEqual({ quality: 'high', torch: 'low' });
  });

  it('take the default for anything missing or unknown', () => {
    const fallback = defaultShadows(false);
    expect(sanitizeShadows({ quality: 'off' }, fallback)).toEqual({ quality: 'off', torch: 'low' });
    expect(sanitizeShadows({ quality: 'off', torch: 'high' }, fallback)).toEqual({
      quality: 'off',
      torch: 'high',
    });
    expect(sanitizeShadows({ torch: 'sideways' }, fallback)).toEqual(fallback);
    expect(sanitizeShadows({ quality: 'ultra' }, fallback)).toEqual(fallback);
    expect(sanitizeShadows(null, fallback)).toEqual(fallback);
    expect(sanitizeShadows('low', fallback)).toEqual(fallback);
  });

  it('are the renderer levels 0 to 2 in order (dwell-look ShadowQuality::from_level)', () => {
    expect(SHADOW_QUALITIES.map(shadowLevel)).toEqual([0, 1, 2]);
  });

  it('keep the fields a newer build stored when this one rewrites the record', () => {
    const stored = { quality: 'high', cloudShadows: true, future: { a: 1 } };
    expect(shadowRecord(stored, { quality: 'low', torch: 'high' })).toEqual({
      quality: 'low',
      torch: 'high',
      cloudShadows: true,
      future: { a: 1 },
    });
    expect(shadowRecord(null, { quality: 'off', torch: 'off' })).toEqual({
      quality: 'off',
      torch: 'off',
    });
    expect(shadowRecord([1, 2], { quality: 'off', torch: 'low' })).toEqual({
      quality: 'off',
      torch: 'low',
    });
  });

  it('read a page query override, and nothing else', () => {
    expect(shadowsFromQuery('?shadows=low&x=1')).toBe('low');
    expect(shadowsFromQuery('?shadows=ultra')).toBeNull();
    expect(shadowsFromQuery('')).toBeNull();
    expect(torchShadowsFromQuery('?torchshadows=high')).toBe('high');
    expect(torchShadowsFromQuery('?shadows=high')).toBeNull();
  });
});

describe('the settings store with shadows (append-only)', () => {
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

  it('reads an older store without shadows as the default, and keeps unknown fields on save', () => {
    store.set('dwell.water', JSON.stringify({ quality: 'medium' }));
    expect(loadSettings(true).shadows).toEqual({ quality: 'low', torch: 'off' });
    expect(loadSettings(false).shadows).toEqual({ quality: 'high', torch: 'low' });

    store.set('dwell.shadows', JSON.stringify({ quality: 'low', cascadesShown: 2 }));
    const s = loadSettings(false);
    expect(s.shadows.quality).toBe('low');
    saveSettings({ ...s, shadows: { quality: 'off', torch: 'high' } });
    expect(JSON.parse(store.get('dwell.shadows') ?? '')).toEqual({
      quality: 'off',
      torch: 'high',
      cascadesShown: 2,
    });
    expect(loadSettings(false).shadows.torch).toBe('high');
    expect(loadSettings(false).shadows.quality).toBe('off');
    // The water's record is untouched by it.
    expect(JSON.parse(store.get('dwell.water') ?? '')).toEqual({ quality: 'medium' });
  });

  it('shares the shadows in the settings JSON', () => {
    const json = JSON.parse(settingsJson(defaultSettings(false))) as { shadows: unknown };
    expect(json.shadows).toEqual({ quality: 'high', torch: 'low' });
  });
});
