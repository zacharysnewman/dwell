import { describe, expect, it } from 'vitest';
import { defaultSsao, sanitizeSsao, ssaoRecord } from './ssaoSettings';

describe('ambient occlusion settings', () => {
  it('reads a stored record and falls back on anything missing or unknown', () => {
    const d = defaultSsao();
    expect(sanitizeSsao(null, d)).toEqual(d);
    // A record from before the look (owner, 2026-11-15) takes the default, Normal.
    expect(sanitizeSsao({ quality: 'low', strength: 0.5 }, d)).toEqual({
      quality: 'low',
      strength: 0.5,
      level: 'normal',
    });
    expect(sanitizeSsao({ quality: 'low', strength: 1, level: 'strong' }, d).level).toBe('strong');
    expect(sanitizeSsao({ level: 'loud' }, d).level).toBe('normal');
    expect(sanitizeSsao({ quality: 'ultra', strength: 'x' }, d)).toEqual(d);
    expect(sanitizeSsao({ strength: 9 }, d).strength).toBe(2);
    expect(sanitizeSsao({ strength: -1 }, d).strength).toBe(0);
  });

  it('keeps the fields a newer build stored when it rewrites the record (append-only)', () => {
    expect(
      ssaoRecord({ radius: 2, quality: 'off' }, { quality: 'high', strength: 1.5, level: 'weak' }),
    ).toEqual({
      radius: 2,
      quality: 'high',
      strength: 1.5,
      level: 'weak',
    });
    expect(ssaoRecord('junk', { quality: 'low', strength: 1, level: 'normal' })).toEqual({
      quality: 'low',
      strength: 1,
      level: 'normal',
    });
  });
});
