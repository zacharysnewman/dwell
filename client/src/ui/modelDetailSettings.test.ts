import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import {
  MODEL_DETAIL,
  detailAt,
  detailData,
  detailIndex,
  sanitizeModelDetail,
} from './modelDetailSettings';
import { SETTING_FIELDS, baseSettings } from './settingsMenu';

// Owner, 2026-11-10: "make this a slider in the graphics and then later we'll set an opinionated
// value", then "Keep the logic for changing the detail. But we can remove the slider and make 60%
// the default": the levels and default are shared/models/models.json's, with no Settings control.
describe('model detail', () => {
  it('runs through the data levels, full detail first, defaulting to the data', () => {
    expect(MODEL_DETAIL.levels.map((l) => l.id)).toEqual(['full', 'high', 'medium', 'low']);
    expect(MODEL_DETAIL.default).toBe('high');
    expect(MODEL_DETAIL.levels.find((l) => l.id === 'high')?.label).toBe('60 %');
    expect(baseSettings(false).modelDetail).toBe(MODEL_DETAIL.default);
    expect(detailAt(0).id).toBe('full');
    expect(detailAt(2.4).id).toBe('medium');
    expect(detailAt(99).id).toBe('low');
    expect(detailIndex('low')).toBe(3);
    expect(sanitizeModelDetail('nonsense')).toBe(MODEL_DETAIL.default);
    expect(SETTING_FIELDS.some((f) => f.id === 'modelDetail')).toBe(true);
  });

  it('has no slider in Settings, the setting kept for code, pins and ?modeldetail=', () => {
    const menu = readFileSync(new URL('./settingsMenu.ts', import.meta.url), 'utf8');
    expect(menu).not.toMatch(/label: 'Model detail'/);
    expect(menu).not.toMatch(/pin: 'modelDetail'/);
    expect(SETTING_FIELDS.some((f) => f.id === 'modelDetail')).toBe(true);
  });

  it('takes a level added by data alone', () => {
    const d = detailData({
      levels: [
        { id: 'full', label: 'Full', ratio: 1 },
        { id: 'coarse', label: 'Coarse', ratio: 0.5 },
      ],
      default: 'coarse',
    });
    expect(sanitizeModelDetail('gone', d)).toBe('coarse');
    expect(detailAt(1, d).label).toBe('Coarse');
    expect(() => detailData({ levels: [], default: 'x' })).toThrow(/no levels/);
    expect(() => detailData({ levels: d.levels, default: 'x' })).toThrow(/not a level/);
  });
});
