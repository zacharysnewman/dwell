// Settings follow the build's defaults unless the player pinned them (ui/settingsPins.ts; owner,
// 2026-11-07): an unpinned setting moves with a changed default, a pinned one doesn't, a chosen
// preset follows its retuned definition, and the stores stay append-only and readable by older
// builds.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import data from '../../../shared/settings/graphics-presets.json';
import { PRESET_FIELDS, type PresetData } from './graphicsPresets';
import {
  ALL_PINS,
  PRESET_PINS,
  SETTING_FIELDS,
  baseSettings,
  changeSetting,
  choosePreset,
  clearPin,
  clearPreset,
  defaultSettings,
  loadSettingsState,
  resetSettings,
  saveSettings,
  saveSettingsState,
  shownPreset,
  storedSettings,
  type DefaultsSource,
  type Settings,
  type SettingsState,
} from './settingsMenu';
import { getAt, pinnedRecord, withAt } from './settingsPins';

const store = new Map<string, string>();
beforeEach(() => {
  store.clear();
  vi.stubGlobal('localStorage', {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => store.set(k, v),
    removeItem: (k: string) => store.delete(k),
  });
});
afterEach(() => {
  vi.unstubAllGlobals();
});

const stored = (k: string): unknown => JSON.parse(store.get(k) ?? 'null');

/** A later build whose own defaults changed: a wider view, no screen shake, quieter music. */
const buildB: DefaultsSource = {
  base: (mobile, reduced) => {
    const s = baseSettings(mobile, reduced);
    return {
      ...s,
      fovDeg: 90,
      explosions: { ...s.explosions, shake: false },
      audio: { ...s.audio, music: 0.3 },
    };
  },
};

/** The presets with Low retuned: clouds Low and a longer view. */
function retunedLow(): PresetData {
  const d = JSON.parse(JSON.stringify(data)) as PresetData;
  const low = d.presets.find((p) => p.id === 'low');
  if (!low) throw new Error('no low preset');
  low.settings.clouds = 'low';
  low.settings.distanceM = 112;
  return d;
}

/** Every leaf of the settings, as paths. */
function leaves(v: unknown, at: string[] = []): string[] {
  if (typeof v !== 'object' || v === null) return [at.join('.')];
  return Object.entries(v).flatMap(([k, x]) => leaves(x, [...at, k]));
}

describe('the stored settings vocabulary', () => {
  it('names every setting once, and every preset field once', () => {
    const ids = SETTING_FIELDS.map((f) => f.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const mobile of [false, true])
      expect([...ids].sort()).toEqual(leaves(defaultSettings(mobile)).sort());
    const presetNames = SETTING_FIELDS.flatMap((f) => (f.preset ? [f.preset] : []));
    expect([...presetNames].sort()).toEqual(Object.keys(PRESET_FIELDS).sort());
  });

  it('reads and writes paths without touching anything else', () => {
    const v = { a: { b: 1, c: 2 }, d: 3 };
    expect(getAt(v, ['a', 'b'])).toBe(1);
    expect(withAt(v, ['a', 'b'], 5)).toEqual({ a: { b: 5, c: 2 }, d: 3 });
    expect(v.a.b).toBe(1);
  });
});

describe('unpinned settings', () => {
  it('follow a changed default: nothing of them is stored', () => {
    // Build A: a new player opens the menu and changes nothing.
    const a = loadSettingsState(false);
    expect(a.pins.size).toBe(0);
    expect(a.settings).toEqual(defaultSettings(false));
    saveSettingsState(a);
    expect([...store.keys()]).toEqual([]);
    // Build B changed its defaults: the player gets them.
    const b = loadSettingsState(false, buildB);
    expect(b.settings.fovDeg).toBe(90);
    expect(b.settings.explosions.shake).toBe(false);
    expect(b.settings.audio.music).toBe(0.3);
  });

  it('follow a retuned device preset (no preset chosen)', () => {
    const s = loadSettingsState(true, { presets: retunedLow() });
    expect(s.settings.clouds).toBe(defaultSettings(true).clouds); // phones start on Mobile
    const d = retunedLow();
    d.default.mobile = 'low';
    expect(loadSettingsState(true, { presets: d }).settings.detail.distanceM).toBe(112);
  });
});

describe('pinned settings', () => {
  it("are kept whatever the defaults become, even one set to build A's default", () => {
    let s = loadSettingsState(false);
    s = changeSetting(s, 'fovDeg', { ...s.settings, fovDeg: 80 });
    // Pressed to the value that was the default: still the player's choice.
    s = changeSetting(s, 'explosions.shake', { ...s.settings, explosions: s.settings.explosions });
    saveSettingsState(s);
    expect(stored('dwell.view')).toEqual({ fovDeg: 80 });
    expect(stored('dwell.explosions')).toEqual({ shake: true });
    const b = loadSettingsState(false, buildB);
    expect(b.settings.fovDeg).toBe(80);
    expect(b.settings.explosions.shake).toBe(true);
    // The rest follows build B.
    expect(b.settings.audio.music).toBe(0.3);
    expect([...b.pins].sort()).toEqual(['explosions.shake', 'fovDeg']);
  });

  it("from an older build's whole records read as pinned, and are kept as they are", () => {
    const old: Settings = { ...defaultSettings(false), fovDeg: 75 };
    saveSettings(old); // how builds before 0.50.31 stored: every field
    const s = loadSettingsState(false, buildB);
    expect(s.pins).toEqual(ALL_PINS);
    expect(s.settings.fovDeg).toBe(75);
    expect(s.settings.explosions.shake).toBe(true);
    expect(s.preset).toBeNull();
  });

  it('clear by the marker: the setting follows the default again and leaves the store', () => {
    let s = loadSettingsState(false);
    s = changeSetting(s, 'fovDeg', { ...s.settings, fovDeg: 62 });
    s = changeSetting(s, 'audio.music', {
      ...s.settings,
      audio: { ...s.settings.audio, music: 0 },
    });
    saveSettingsState(s);
    s = clearPin(s, 'fovDeg', false);
    expect(s.settings.fovDeg).toBe(defaultSettings(false).fovDeg);
    expect(s.pins.has('fovDeg')).toBe(false);
    expect(s.settings.audio.music).toBe(0);
    saveSettingsState(s);
    expect(store.has('dwell.view')).toBe(false);
    expect(stored('dwell.audio')).toEqual({ music: 0 });
    // A graphics setting's marker takes it back to the preset's value.
    s = choosePreset(s, 'low', false);
    s = changeSetting(s, 'clouds', { ...s.settings, clouds: 'high' });
    expect(clearPin(s, 'clouds', false).settings.clouds).toBe('off');
  });
});

describe('presets', () => {
  it('are stored by id, and the settings they set follow the retuned preset', () => {
    let s = loadSettingsState(false);
    s = changeSetting(s, 'clouds', { ...s.settings, clouds: 'low' });
    s = changeSetting(s, 'exposure', { ...s.settings, exposure: 1.4 });
    s = choosePreset(s, 'low', false);
    // Choosing it clears the graphics pins; the exposure (outside the presets) stays pinned.
    expect([...s.pins]).toEqual(['exposure']);
    expect(s.settings.clouds).toBe('off');
    saveSettingsState(s);
    expect(stored('dwell.graphicsPreset')).toEqual({ id: 'low' });
    expect(store.has('dwell.clouds')).toBe(false);
    const later = loadSettingsState(false, { presets: retunedLow() });
    expect(later.preset).toBe('low');
    expect(later.settings.clouds).toBe('low');
    expect(later.settings.detail.distanceM).toBe(112);
    expect(later.settings.exposure).toBe(1.4);
    expect(shownPreset(later, false).base.id).toBe('low');
    expect(shownPreset(later, false).custom).toBe(false);
  });

  it('read Custom once one setting they set is changed; the others still follow the preset', () => {
    let s = choosePreset(loadSettingsState(false), 'low', false);
    s = changeSetting(s, 'shadows.quality', {
      ...s.settings,
      shadows: { ...s.settings.shadows, quality: 'high' },
    });
    expect(shownPreset(s, false)).toMatchObject({ custom: true, base: { id: 'low' } });
    saveSettingsState(s);
    const later = loadSettingsState(false, { presets: retunedLow() });
    expect(shownPreset(later, false).custom).toBe(true);
    expect(later.settings.shadows.quality).toBe('high');
    expect(later.settings.clouds).toBe('low'); // the retune still reaches it
    // A setting outside the presets is not Custom.
    const e = changeSetting(choosePreset(s, 'low', false), 'exposure', s.settings);
    expect(shownPreset(e, false).custom).toBe(false);
  });

  it("an id the data no longer lists reads as the device's default preset", () => {
    store.set('dwell.graphicsPreset', JSON.stringify({ id: 'potato' }));
    const s = loadSettingsState(false);
    expect(s.preset).toBeNull();
    expect(s.settings).toEqual(defaultSettings(false));
  });

  it("the Preset marker goes back to the device's default preset", () => {
    let s = choosePreset(loadSettingsState(false), 'low', false);
    s = changeSetting(s, 'clouds', { ...s.settings, clouds: 'high' });
    s = clearPreset(s, false);
    expect(s.preset).toBeNull();
    expect([...s.pins].filter((p) => PRESET_PINS.has(p))).toEqual([]);
    expect(s.settings).toEqual(defaultSettings(false));
  });
});

describe('Reset', () => {
  it('clears every pin and the preset: everything follows the defaults', () => {
    let s: SettingsState = choosePreset(loadSettingsState(false), 'ultra', false);
    s = changeSetting(s, 'fovDeg', { ...s.settings, fovDeg: 99 });
    s = changeSetting(s, 'hud.shown.crosshair', {
      ...s.settings,
      hud: { ...s.settings.hud, shown: { ...s.settings.hud.shown, crosshair: false } },
    });
    saveSettingsState(s);
    s = resetSettings(false);
    expect(s.pins.size).toBe(0);
    expect(s.preset).toBeNull();
    expect(s.settings).toEqual(defaultSettings(false));
    saveSettingsState(s);
    expect([...store.keys()]).toEqual([]);
  });
});

describe('the stores', () => {
  it('stay append-only: fields and records a newer build stored are kept', () => {
    store.set('dwell.water', JSON.stringify({ quality: 'low', later: 'kept' }));
    store.set('dwell.hud', JSON.stringify({ shown: { crosshair: false, newPart: true }, x: 1 }));
    store.set('dwell.graphicsPreset', JSON.stringify({ id: 'low', later: [1] }));
    let s = loadSettingsState(false);
    expect(s.pins.has('water.quality')).toBe(true);
    s = clearPin(s, 'water.quality', false);
    s = clearPin(s, 'hud.shown.crosshair', false);
    s = clearPreset(s, false);
    saveSettingsState(s);
    expect(stored('dwell.water')).toEqual({ later: 'kept' });
    expect(stored('dwell.hud')).toEqual({ shown: { newPart: true }, x: 1 });
    expect(stored('dwell.graphicsPreset')).toEqual({ later: [1] });
  });

  it('a key holding a bare value is removed when it follows the default', () => {
    expect(pinnedRecord(SETTING_FIELDS, 'dwell.clouds', 'low', new Set())).toBeUndefined();
    expect(pinnedRecord(SETTING_FIELDS, 'dwell.clouds', 'low', new Set(['clouds']))).toBe('low');
  });

  it('read by an older build: the pinned fields, and its own defaults for the rest', () => {
    let s = loadSettingsState(true);
    s = changeSetting(s, 'detail.memoryMb', {
      ...s.settings,
      detail: { ...s.settings.detail, memoryMb: 160 },
    });
    s = changeSetting(s, 'water.flowFoam', {
      ...s.settings,
      water: { ...s.settings.water, flowFoam: false },
    });
    s = choosePreset(s, 'low', true);
    saveSettingsState(s);
    expect(stored('dwell.detail')).toBeNull(); // the memory is a preset's: choosing Low cleared it
    expect(stored('dwell.water')).toEqual({ flowFoam: false });
    // Builds before 0.50.31 read every key with its sanitizer over their defaults (and ignore
    // `dwell.graphicsPreset`): the device's default preset, the foam off.
    const old = storedSettings(true, defaultSettings(true));
    expect(old).toEqual({
      ...defaultSettings(true),
      water: { ...defaultSettings(true).water, flowFoam: false },
    });
  });
});
