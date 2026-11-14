import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import {
  ANTIALIAS,
  ANTIALIAS_KEY,
  antialiasFromQuery,
  antialiasMode,
  antialiasRecord,
  sanitizeAntialias,
  validateAntialias,
} from './antialiasSettings';
import { SETTING_FIELDS, baseSettings, loadSettings, saveSettings } from './settingsMenu';

describe('anti-aliasing settings', () => {
  it('offers honest modes: Off, MSAA 4× and 4× + FXAA, defaulting to 4× and 8× anisotropy', () => {
    expect(ANTIALIAS.modes.map((m) => [m.id, m.samples, m.fxaa])).toEqual([
      ['off', 1, false],
      ['msaa4', 4, false],
      ['msaa4-fxaa', 4, true],
    ]);
    expect(ANTIALIAS.anisotropy.map((a) => a.value)).toEqual([1, 2, 4, 8, 16]);
    expect(baseSettings(false).antialias).toEqual({ mode: 'msaa4', anisotropy: 8 });
    expect(ANTIALIAS.hint).toMatch(/only 4×/);
    expect(antialiasMode('nonsense').id).toBe('msaa4');
    expect(antialiasMode('off').samples).toBe(1);
    expect(SETTING_FIELDS.map((f) => f.id)).toEqual(
      expect.arrayContaining(['antialias.mode', 'antialias.anisotropy']),
    );
  });

  it('refuses sample counts WebGPU lacks and anisotropy past 16', () => {
    const ok = structuredClone(ANTIALIAS);
    expect(() =>
      validateAntialias({ ...ok, modes: [{ id: 'x8', label: '8×', samples: 8, fxaa: false }] }),
    ).toThrow(/1 or 4/);
    expect(() => validateAntialias({ ...ok, anisotropy: [{ value: 32, label: '32×' }] })).toThrow(
      /\[1, 16\]/,
    );
    // A mode added by data alone is offered and read back.
    const more = validateAntialias({
      ...ok,
      modes: [...ok.modes, { id: 'fxaa', label: 'FXAA', samples: 1, fxaa: true }],
    });
    expect(sanitizeAntialias({ mode: 'fxaa', anisotropy: 4 }, more)).toEqual({
      mode: 'fxaa',
      anisotropy: 4,
    });
  });

  it('reads unknown values as the default and keeps unknown fields on rewrite', () => {
    expect(sanitizeAntialias({ mode: 'msaa16', anisotropy: 3 })).toEqual(ANTIALIAS.default);
    expect(antialiasRecord({ mode: 'off', future: 1 }, { mode: 'msaa4', anisotropy: 2 })).toEqual({
      mode: 'msaa4',
      anisotropy: 2,
      future: 1,
    });
    expect(antialiasFromQuery('?aa=off&aniso=16', ANTIALIAS.default)).toEqual({
      mode: 'off',
      anisotropy: 16,
    });
    expect(antialiasFromQuery('?aa=bogus&aniso=5', ANTIALIAS.default)).toEqual(ANTIALIAS.default);
  });

  describe('in the store', () => {
    const store = new Map<string, string>();
    beforeEach(() => {
      store.clear();
      globalThis.localStorage = {
        getItem: (k: string) => store.get(k) ?? null,
        setItem: (k: string, v: string) => store.set(k, v),
        removeItem: (k: string) => store.delete(k),
      } as unknown as Storage;
    });
    afterEach(() => {
      delete (globalThis as { localStorage?: Storage }).localStorage;
    });

    it('persists the choice under dwell.antialias, a newer build’s fields kept', () => {
      store.set(ANTIALIAS_KEY, JSON.stringify({ newer: 'kept' }));
      const s = loadSettings(false);
      saveSettings({ ...s, antialias: { mode: 'off', anisotropy: 16 } });
      expect(JSON.parse(store.get(ANTIALIAS_KEY) ?? '{}')).toEqual({
        newer: 'kept',
        mode: 'off',
        anisotropy: 16,
      });
      expect(loadSettings(false).antialias).toEqual({ mode: 'off', anisotropy: 16 });
    });
  });
});
