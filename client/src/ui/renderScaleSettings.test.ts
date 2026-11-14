import { describe, expect, it } from 'vitest';
import {
  UPSCALE,
  defaultRenderScale,
  renderScaleFromQuery,
  renderScaleRecord,
  sanitizeRenderScale,
  validateUpscale,
  type UpscaleData,
} from './renderScaleSettings';

describe('render scale settings', () => {
  it('defaults to 50 % stretched (owner, 2026-11-07: "make the default 50% upscaling")', () => {
    expect(defaultRenderScale()).toEqual({ scale: 0.5, upscaler: 'stretch' });
    expect(UPSCALE.scales.map((s) => s.value)).toEqual([1, 0.75, 0.67, 0.5]);
    expect(UPSCALE.upscalers.map((u) => u.id)).toEqual(['stretch', 'sharpen']);
  });

  it('sanitizes stored values: unlisted scales and upscalers take the default', () => {
    expect(sanitizeRenderScale({ scale: 0.5, upscaler: 'sharpen' })).toEqual({
      scale: 0.5,
      upscaler: 'sharpen',
    });
    expect(sanitizeRenderScale({ scale: 0.6, upscaler: 'fsr2' })).toEqual(defaultRenderScale());
    expect(sanitizeRenderScale({ scale: '0.5' })).toEqual(defaultRenderScale());
    expect(sanitizeRenderScale(null)).toEqual(defaultRenderScale());
    expect(sanitizeRenderScale('junk')).toEqual(defaultRenderScale());
  });

  it('keeps fields a newer build stored on rewrite (append-only)', () => {
    expect(
      renderScaleRecord({ future: 3, scale: 1 }, { scale: 0.75, upscaler: 'sharpen' }),
    ).toEqual({ future: 3, scale: 0.75, upscaler: 'sharpen' });
    expect(renderScaleRecord([1], { scale: 1, upscaler: 'stretch' })).toEqual({
      scale: 1,
      upscaler: 'stretch',
    });
  });

  it('takes ?renderscale= in [0.5, 1] and any kind the renderer knows from the query', () => {
    const s = defaultRenderScale();
    expect(renderScaleFromQuery('?renderscale=0.67&upscaler=stretch-pass', s)).toEqual({
      scale: 0.67,
      upscaler: 'stretch-pass',
    });
    expect(renderScaleFromQuery('?renderscale=0.2&upscaler=dlss', s)).toEqual(s);
    expect(renderScaleFromQuery('', { scale: 0.5, upscaler: 'sharpen' })).toEqual({
      scale: 0.5,
      upscaler: 'sharpen',
    });
  });

  it('a scale added by data alone is offered and kept', () => {
    const d: UpscaleData = validateUpscale({
      ...UPSCALE,
      scales: [...UPSCALE.scales, { value: 0.6, label: '60 %' }],
    });
    expect(sanitizeRenderScale({ scale: 0.6, upscaler: 'stretch' }, d).scale).toBe(0.6);
  });

  it('fails a bad entry with a message', () => {
    const bad = (patch: Partial<UpscaleData>) => () => validateUpscale({ ...UPSCALE, ...patch });
    expect(bad({ scales: [{ value: 0.3, label: '30 %' }] })).toThrow(/scales\[0\].value/);
    expect(bad({ scales: [{ value: 0.5, label: '50 %' }] })).toThrow(/include 1/);
    expect(bad({ upscalers: [{ id: 'fsr2', label: 'x', hint: 'x' }] })).toThrow(/fsr2/);
    expect(bad({ default: { scale: 0.9, upscaler: 'stretch' } })).toThrow(/default.scale/);
    expect(bad({ sharpenStops: 9 })).toThrow(/sharpenStops/);
  });
});
