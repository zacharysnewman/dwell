// WORK_QUEUE item 30: the Graphics settings for features that had none (ui/featureSettings.ts).
import { describe, expect, it } from 'vitest';
import { defaultFeatures, featureRecord, sanitizeFeatures } from './featureSettings';
import { GRAPHICS_PRESETS, applyPreset, presetOf } from './graphicsPresets';
import { baseSettings, defaultSettings } from './settingsMenu';

describe('feature settings', () => {
  it('default to everything drawn, as before they existed', () => {
    expect(defaultFeatures()).toEqual({
      tallGrass: 1,
      farTrees: true,
      distantShadows: true,
      hitShards: 1,
      particles: 1,
      farHerds: true,
    });
    for (const mobile of [false, true])
      expect(defaultSettings(mobile).features).toEqual(defaultFeatures());
  });

  it('read an older build’s missing or partial record with the defaults', () => {
    const d = defaultFeatures();
    expect(sanitizeFeatures(null, d)).toEqual(d);
    expect(sanitizeFeatures('junk', d)).toEqual(d);
    expect(sanitizeFeatures([1, 2], d)).toEqual(d);
    expect(sanitizeFeatures({ farTrees: false }, d)).toEqual({ ...d, farTrees: false });
  });

  it('clamp shares to 0–1 and ignore values of the wrong kind', () => {
    const d = defaultFeatures();
    expect(
      sanitizeFeatures(
        { tallGrass: 3, hitShards: -1, particles: 'lots', farHerds: 'no', distantShadows: 0 },
        d,
      ),
    ).toEqual({ ...d, tallGrass: 1, hitShards: 0 });
    expect(sanitizeFeatures({ particles: Number.NaN }, d).particles).toBe(1);
  });

  it('keep a newer build’s fields on rewrite (the store is append-only)', () => {
    const rec = featureRecord(
      { futureThing: 7, tallGrass: 0.2 },
      { ...defaultFeatures(), tallGrass: 0.5 },
    );
    expect(rec).toEqual({ ...defaultFeatures(), tallGrass: 0.5, futureThing: 7 });
    expect(featureRecord('junk', defaultFeatures())).toEqual(defaultFeatures());
  });

  it('are set by every preset to what was drawn before (today’s look per preset)', () => {
    for (const p of GRAPHICS_PRESETS.presets) {
      const s = applyPreset(
        {
          ...baseSettings(false),
          features: { ...defaultFeatures(), tallGrass: 0, farTrees: false },
        },
        p,
        false,
      );
      expect(s.features, p.id).toEqual(defaultFeatures());
    }
  });

  it('read Custom once one is changed by hand', () => {
    const s = defaultSettings(false);
    expect(presetOf(s, false)?.id).toBe('high');
    expect(presetOf({ ...s, features: { ...s.features, farHerds: false } }, false)).toBeNull();
    expect(presetOf({ ...s, features: { ...s.features, tallGrass: 0.5 } }, false)).toBeNull();
  });
});
