import { describe, expect, it } from 'vitest';
import {
  RENDERER_CHOICES,
  choiceOf,
  dividerShare,
  formatDifference,
  rendererParams,
} from './renderer';

describe('the renderer switch', () => {
  it('reads ?renderer= and ?compare= only when the build has the switch', () => {
    expect(rendererParams('?renderer=compare&compare=raster', true)).toEqual({
      renderer: 'compare',
      compare: 'raster',
    });
    expect(rendererParams('?renderer=march', false)).toEqual({});
    expect(rendererParams('', true)).toEqual({});
  });

  it('finds the settings choice for what the client says draws', () => {
    expect(choiceOf('raster')?.id).toBe('raster');
    expect(choiceOf('march')?.id).toBe('march');
    expect(choiceOf('compare raster | march')?.id).toBe('compare');
    expect(choiceOf('compare raster | raster')?.id).toBe('compare-raster');
    expect(choiceOf('voxels')).toBeUndefined();
    expect(new Set(RENDERER_CHOICES.map((c) => c.id)).size).toBe(RENDERER_CHOICES.length);
  });

  it('places the divider from the pointer, kept on the canvas', () => {
    expect(dividerShare(150, 100, 200)).toBe(0.25);
    expect(dividerShare(0, 100, 200)).toBe(0);
    expect(dividerShare(900, 100, 200)).toBe(1);
    expect(dividerShare(10, 0, 0)).toBe(0.5);
  });

  it('says how many pixels disagree', () => {
    expect(formatDifference(null)).toMatch(/waiting/);
    expect(formatDifference({ pixels: 1000, coverage: 0, depth: 0, colour: 0 })).toBe(
      'difference 0 of 1000 px (0.000 %) · coverage 0 · depth 0 · colour 0',
    );
    expect(formatDifference({ pixels: 200, coverage: 1, depth: 2, colour: 3 })).toMatch(
      /^difference 6 of 200 px \(3\.000 %\)/,
    );
  });
});
