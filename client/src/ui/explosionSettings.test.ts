import { describe, expect, it } from 'vitest';
import { defaultExplosions, explosionRecord, sanitizeExplosions } from './explosionSettings';

describe('explosion settings', () => {
  it('defaults every effect on, the shake off when reduced motion is preferred', () => {
    expect(defaultExplosions()).toEqual({ flash: true, smoke: true, shake: true });
    expect(defaultExplosions(true).shake).toBe(false);
  });

  it('reads a stored record, taking the default for what is missing or wrong', () => {
    const d = defaultExplosions();
    expect(sanitizeExplosions(null, d)).toEqual(d);
    expect(sanitizeExplosions({ flash: false, smoke: 'no' }, d)).toEqual({
      flash: false,
      smoke: true,
      shake: true,
    });
  });

  it('keeps fields a newer build stored when it rewrites the record', () => {
    const r = explosionRecord(
      { flash: true, future: 3 },
      { flash: false, smoke: true, shake: false },
    );
    expect(r).toEqual({ flash: false, smoke: true, shake: false, future: 3 });
    expect(explosionRecord('junk', defaultExplosions())).toEqual(defaultExplosions());
  });
});
