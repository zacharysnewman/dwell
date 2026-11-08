import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import tiers from '../../../shared/look/watertiers.json';
import {
  checkWaterTiers,
  defaultWater,
  sanitizeWater,
  waterTier,
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
      waterfalls: true,
    });
    expect(defaultWater(false)).toEqual({
      quality: 'medium',
      flowRipples: true,
      flowFoam: true,
      flowParticles: true,
      waterfalls: true,
    });
  });

  it('take the default for anything missing or unknown', () => {
    const fallback = defaultWater(false);
    expect(sanitizeWater({ quality: 'medium' }, fallback)).toEqual(fallback);
    expect(sanitizeWater({ quality: 'ultra' }, fallback)).toEqual(fallback);
    expect(sanitizeWater(null, fallback)).toEqual(fallback);
    expect(sanitizeWater('low', fallback)).toEqual(fallback);
  });

  it('offer Low and Medium, the renderer levels their indices in the data', () => {
    // The owner (2026-11-07): High folded into Medium; the opaque tier is the CPU adapter's only.
    expect(WATER_QUALITIES).toEqual(['low', 'medium']);
    const ids = (tiers as { tiers: { id: string }[] }).tiers.map((t) => t.id);
    expect(ids).toEqual(['opaque', 'low', 'medium']);
    expect(WATER_QUALITIES.map(waterQualityLevel)).toEqual([1, 2]);
    expect(waterQualityLevel('opaque')).toBe(0);
    expect(waterQualityLevel('high')).toBe(waterQualityLevel('medium'));
  });

  it('read a stored "high" (before 0.50.29) as Medium, and keep it stored as it was', () => {
    const fallback = defaultWater(true);
    const read = sanitizeWater({ quality: 'high', later: 1 }, fallback);
    expect(read.quality).toBe('medium');
    expect(waterRecord({ quality: 'high', later: 1 }, read)).toMatchObject({
      quality: 'high',
      later: 1,
    });
    // Chose another tier: that one is stored.
    expect(waterRecord({ quality: 'high' }, { ...read, quality: 'low' }).quality).toBe('low');
    // The CPU's tier is not a setting: a stored one reads as the default.
    expect(sanitizeWater({ quality: 'opaque' }, fallback).quality).toBe('low');
  });

  it('check the tier data: a bad file fails with a message, a new tier by data alone is offered', () => {
    const file = () => JSON.parse(JSON.stringify(tiers)) as Parameters<typeof checkWaterTiers>[0];
    const twice = file();
    const [opaque, low, medium] = file().tiers;
    if (!opaque || !low || !medium) throw new Error('three tiers');
    twice.tiers.push({ ...low });
    expect(() => checkWaterTiers(twice)).toThrow(/low is listed twice/);
    const noCpu = file();
    noCpu.tiers = [{ ...opaque, cpu: false }, low, medium];
    expect(() => checkWaterTiers(noCpu)).toThrow(/exactly one tier must be the CPU/);
    const alias = file();
    alias.aliases.ultra = 'opaque';
    expect(() => checkWaterTiers(alias)).toThrow(/aliases\.ultra: opaque is not a Settings tier/);
    // A fixture tier added by data alone is known by name and through an alias.
    const more = file();
    more.tiers.push({ ...medium, id: 'glass', label: 'Glass' });
    more.aliases.crystal = 'glass';
    expect(checkWaterTiers(more).tiers.length).toBe(4);
    expect(waterTier('crystal', more)).toBe('glass');
    expect(waterTier('glass', more)).toBe('glass');
    expect(waterTier('ultra', more)).toBeNull();
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
    expect(sanitizeWater({ quality: 'high' }, fallback)).toEqual({
      ...fallback,
      quality: 'medium',
    });
    expect(
      sanitizeWater(
        { quality: 'low', flowRipples: false, flowFoam: true, flowParticles: false },
        fallback,
      ),
    ).toEqual({
      quality: 'low',
      flowRipples: false,
      flowFoam: true,
      flowParticles: false,
      waterfalls: true,
    });
    // Not a boolean: the default.
    expect(sanitizeWater({ flowFoam: 'no' }, fallback).flowFoam).toBe(true);
  });

  it('read the Waterfalls switch (0.49.31) where stored, and default it on where not', () => {
    const fallback = defaultWater(false);
    expect(sanitizeWater({ quality: 'high', flowFoam: false }, fallback).waterfalls).toBe(true);
    expect(sanitizeWater({ waterfalls: false }, fallback).waterfalls).toBe(false);
    expect(sanitizeWater({ waterfalls: 'off' }, fallback).waterfalls).toBe(true);
    expect(waterRecord({}, { ...fallback, waterfalls: false }).waterfalls).toBe(false);
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
    saveSettings({ ...s, water: { ...s.water, quality: 'low' } });
    expect(JSON.parse(store.get('dwell.water') ?? '')).toEqual({
      ...defaultWater(false),
      quality: 'low',
      reflections: 'ssr',
    });
    expect(loadSettings(false).water.quality).toBe('low');
    // A stored "high" (before 0.50.29) loads as Medium and survives a save unchanged.
    store.set('dwell.water', JSON.stringify({ quality: 'high', reflections: 'ssr' }));
    const old = loadSettings(false);
    expect(old.water.quality).toBe('medium');
    saveSettings(old);
    expect(JSON.parse(store.get('dwell.water') ?? '')).toMatchObject({ quality: 'high' });
  });

  it('shares the water quality and flow switches in the settings JSON', () => {
    const json = JSON.parse(settingsJson(defaultSettings(true))) as { water: unknown };
    expect(json.water).toEqual(defaultWater(true));
  });
});
