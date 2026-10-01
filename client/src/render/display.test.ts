import { describe, expect, it } from 'vitest';
import { displayOptions, formatRenderStats } from './display';

describe('display options', () => {
  it('reads batching and the resolution scale from the address, clamped', () => {
    expect(displayOptions('')).toEqual({ batched: false, scale: 1 });
    expect(displayOptions('?local=1&batch=1&scale=0.5')).toEqual({ batched: true, scale: 0.5 });
    expect(displayOptions('?batch=0&scale=0.01')).toEqual({ batched: false, scale: 0.25 });
    expect(displayOptions('?scale=9')).toEqual({ batched: false, scale: 2 });
    expect(displayOptions('?scale=lots')).toEqual({ batched: false, scale: 1 });
  });

  it('formats the frame’s work for the debug overlay', () => {
    expect(
      formatRenderStats({ calls: 712, triangles: 1_234_567, batched: false, pixelRatio: 2 }),
    ).toBe('render 712 draws · 1.23 M tris · meshes · 2.00× pixels');
    expect(formatRenderStats({ calls: 9, triangles: 48_900, batched: true, pixelRatio: 1 })).toBe(
      'render 9 draws · 49 k tris · batched · 1.00× pixels',
    );
  });
});
