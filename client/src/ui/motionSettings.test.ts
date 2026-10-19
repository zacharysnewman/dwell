import { describe, expect, it } from 'vitest';
import { defaultMotion, motionRecord, sanitizeMotion } from './motionSettings';

describe('motion settings', () => {
  it('default on, off when the browser prefers reduced motion', () => {
    expect(defaultMotion(false)).toEqual({ speedEffects: true, fovBoost: 1, plantSway: true });
    expect(defaultMotion(true)).toEqual({ speedEffects: false, fovBoost: 1, plantSway: false });
    expect(sanitizeMotion(null, true)).toEqual(defaultMotion(true));
  });

  it('clamp stored values and default anything missing or of the wrong type', () => {
    expect(sanitizeMotion({ speedEffects: false, fovBoost: 3 }, false)).toEqual({
      speedEffects: false,
      fovBoost: 1,
      plantSway: true,
    });
    expect(sanitizeMotion({ speedEffects: 'yes', fovBoost: Number.NaN }, false)).toEqual(
      defaultMotion(false),
    );
    expect(sanitizeMotion({ fovBoost: -1 }, false).fovBoost).toBe(0);
    // A choice made is kept, whatever the browser prefers now.
    expect(sanitizeMotion({ speedEffects: true }, true).speedEffects).toBe(true);
  });

  it('plant sway: off with reduced motion, a stored choice kept, records of older builds default', () => {
    expect(sanitizeMotion({ speedEffects: true, fovBoost: 1 }, true).plantSway).toBe(false);
    expect(sanitizeMotion({ speedEffects: true, fovBoost: 1 }, false).plantSway).toBe(true);
    expect(sanitizeMotion({ plantSway: false }, false).plantSway).toBe(false);
    expect(sanitizeMotion({ plantSway: true }, true).plantSway).toBe(true);
    expect(sanitizeMotion({ plantSway: 'no' }, false).plantSway).toBe(true);
  });

  it('is append-only: a stored record keeps fields this build does not know', () => {
    const stored = { speedEffects: true, fovBoost: 0.5, radialBlur: 0.3, future: { a: 1 } };
    const out = motionRecord(stored, { speedEffects: false, fovBoost: 0.25, plantSway: false });
    expect(out).toEqual({
      speedEffects: false,
      fovBoost: 0.25,
      plantSway: false,
      radialBlur: 0.3,
      future: { a: 1 },
    });
    // Unreadable records are replaced by what this build writes.
    expect(motionRecord('garbage', { speedEffects: true, fovBoost: 1, plantSway: true })).toEqual({
      speedEffects: true,
      fovBoost: 1,
      plantSway: true,
    });
  });
});
