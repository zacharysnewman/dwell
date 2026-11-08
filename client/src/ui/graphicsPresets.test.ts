import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import data from '../../../shared/settings/graphics-presets.json';
import {
  GRAPHICS_PRESETS,
  PRESET_FIELDS,
  applyPreset,
  defaultPreset,
  findPreset,
  presetOf,
  validatePresets,
  type GraphicsPreset,
  type PresetData,
} from './graphicsPresets';
import { defaultSettings, loadSettings, saveSettings, type Settings } from './settingsMenu';

const clone = (): PresetData => JSON.parse(JSON.stringify(data)) as PresetData;

describe('graphics presets data', () => {
  it('runs Low → Mobile → Medium → High → Ultra; phones start on Mobile, desktops on High', () => {
    expect(GRAPHICS_PRESETS.presets.map((p) => p.id)).toEqual([
      'low',
      'mobile',
      'medium',
      'high',
      'ultra',
    ]);
    expect(defaultPreset(true).id).toBe('mobile');
    expect(defaultPreset(false).id).toBe('high');
  });

  it("puts High at the owner's 50 % Stretch and Ultra at 100 % with everything on", () => {
    expect(findPreset('high')?.settings).toMatchObject({ renderScale: 0.5, upscaler: 'stretch' });
    expect(findPreset('ultra')?.settings).toMatchObject({
      renderScale: 1,
      water: 'high',
      shadows: 'high',
      torchShadows: 'high',
      shafts: 'high',
      clouds: 'high',
    });
  });

  it('rejects a bad entry with a message naming it', () => {
    const bad = (edit: (d: PresetData) => void): (() => PresetData) => {
      const d = clone();
      edit(d);
      return () => validatePresets(d);
    };
    expect(bad((d) => ((d.presets[0] as GraphicsPreset).settings.water = 'ultra'))).toThrow(
      /presets\[0\] \(low\): water: "ultra" is not one of/,
    );
    expect(bad((d) => ((d.presets[1] as GraphicsPreset).settings.renderScale = 0.6))).toThrow(
      /\(mobile\): renderScale/,
    );
    expect(
      bad((d) => {
        delete (d.presets[2]?.settings as Partial<Record<string, unknown>>).clouds;
      }),
    ).toThrow(/\(medium\): clouds is missing/);
    expect(
      bad((d) => ((d.presets[3]?.settings as Record<string, unknown>).bloom = 'high')),
    ).toThrow(/unknown setting "bloom"/);
    expect(bad((d) => ((d.presets[4] as GraphicsPreset).id = 'low'))).toThrow(/listed twice/);
    expect(bad((d) => (d.default.mobile = 'phone'))).toThrow(/default.mobile "phone"/);
    expect(bad((d) => ((d.presets[0] as GraphicsPreset).settings.distanceM = 5000))).toThrow(
      /distanceM/,
    );
  });

  it('takes a new preset by data alone', () => {
    const d = clone();
    const potato: GraphicsPreset = {
      id: 'fixture-potato',
      label: 'Potato',
      settings: { ...(findPreset('low')?.settings as GraphicsPreset['settings']), memoryMb: 32 },
    };
    d.presets.unshift(potato);
    d.default.desktop = 'fixture-potato';
    const v = validatePresets(d);
    expect(defaultPreset(false, v).label).toBe('Potato');
    const s = applyPreset(defaultSettings(false), potato, false);
    expect(s.detail.memoryMb).toBe(32);
    expect(presetOf(s, false, v)?.id).toBe('fixture-potato');
  });
});

describe('applying a preset', () => {
  // Every setting but the preset's fields, to see that nothing else moves.
  const others = (s: Settings) => ({
    ...s,
    renderScale: null,
    water: { ...s.water, quality: null },
    shadows: null,
    shafts: null,
    clouds: null,
    detail: null,
  });

  for (const p of GRAPHICS_PRESETS.presets) {
    it(`sets exactly ${p.id}'s settings`, () => {
      const start: Settings = {
        ...defaultSettings(false),
        exposure: 1.7,
        fovDeg: 90,
        water: { ...defaultSettings(false).water, flowFoam: false },
      };
      const s = applyPreset(start, p, false);
      for (const [k, f] of Object.entries(PRESET_FIELDS))
        expect(f.get(s), k).toBe(p.settings[k as keyof typeof PRESET_FIELDS]);
      expect(others(s)).toEqual(others(start));
      expect(presetOf(s, false)?.id).toBe(p.id);
    });
  }

  it('clamps the detail to a phone\'s ranges, and still reads as the preset there', () => {
    const ultra = findPreset('ultra') as GraphicsPreset;
    const s = applyPreset(defaultSettings(true), ultra, true);
    expect(s.detail.memoryMb).toBe(192);
    expect(presetOf(s, true)?.id).toBe('ultra');
  });

  it('reads Custom once any one graphics setting is changed', () => {
    const high = defaultSettings(false);
    expect(presetOf(high, false)?.id).toBe('high');
    const changed: Settings[] = [
      { ...high, clouds: 'low' },
      { ...high, shafts: 'off' },
      { ...high, water: { ...high.water, quality: 'high' } },
      { ...high, shadows: { ...high.shadows, torch: 'high' } },
      { ...high, renderScale: { ...high.renderScale, upscaler: 'sharpen' } },
      { ...high, detail: { ...high.detail, distanceM: 255 } },
    ];
    for (const s of changed) expect(presetOf(s, false)).toBeNull();
    // A setting outside the presets (the exposure) leaves it on High.
    expect(presetOf({ ...high, exposure: 2 }, false)?.id).toBe('high');
  });
});

describe('the defaults', () => {
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

  it('are the device preset: High on desktops, Mobile on phones', () => {
    expect(presetOf(defaultSettings(false), false)?.id).toBe('high');
    expect(presetOf(defaultSettings(true), true)?.id).toBe('mobile');
    expect(presetOf(loadSettings(false), false)?.id).toBe('high');
    expect(presetOf(loadSettings(true), true)?.id).toBe('mobile');
  });

  it('draw at 50 % Stretch for a new player; a stored render scale is kept', () => {
    expect(defaultSettings(false).renderScale).toEqual({ scale: 0.5, upscaler: 'stretch' });
    expect(loadSettings(false).renderScale).toEqual({ scale: 0.5, upscaler: 'stretch' });
    store.set('dwell.renderScale', JSON.stringify({ scale: 1, upscaler: 'sharpen' }));
    expect(loadSettings(false).renderScale).toEqual({ scale: 1, upscaler: 'sharpen' });
  });

  it('store nothing new: a preset is its settings, and the stores stay append-only', () => {
    store.set('dwell.renderScale', JSON.stringify({ scale: 1, upscaler: 'stretch', later: 7 }));
    store.set('dwell.water', JSON.stringify({ quality: 'high', later: 'kept' }));
    store.set('dwell.shadows', JSON.stringify({ quality: 'high', torch: 'low', later: [1] }));
    const low = applyPreset(loadSettings(false), findPreset('low') as GraphicsPreset, false);
    saveSettings(low);
    expect(JSON.parse(store.get('dwell.renderScale') ?? '')).toEqual({
      scale: 0.5,
      upscaler: 'stretch',
      later: 7,
    });
    expect(JSON.parse(store.get('dwell.water') ?? '')).toMatchObject({
      quality: 'low',
      later: 'kept',
    });
    expect(JSON.parse(store.get('dwell.shadows') ?? '')).toEqual({
      quality: 'off',
      torch: 'off',
      later: [1],
    });
    expect([...store.keys()].some((k) => k.includes('preset'))).toBe(false);
    expect(presetOf(loadSettings(false), false)?.id).toBe('low');
  });
});
