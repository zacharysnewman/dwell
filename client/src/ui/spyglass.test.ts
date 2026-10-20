import { describe, expect, it } from 'vitest';
import { spyglassStyle } from './spyglass';

describe('spyglass vignette', () => {
  const state = {
    amount: 1,
    magnification: 4,
    innerRadius: 0.95,
    outerRadius: 1.6,
    opacity: 0.6,
    color: [0, 0.5, 1] as [number, number, number],
  };

  it("rings the view from the data's radii, in units of half the screen height", () => {
    const s = spyglassStyle(state);
    expect(s.background).toBe(
      'radial-gradient(circle at 50% 50%, rgba(0, 128, 255, 0) calc(0.95 * 50vh), ' +
        'rgba(0, 128, 255, 1) calc(1.6 * 50vh))',
    );
    expect(s.opacity).toBe('0.6');
  });

  it('fades in with the zoom', () => {
    expect(spyglassStyle({ ...state, amount: 0.5 }).opacity).toBe('0.3');
    expect(spyglassStyle({ ...state, amount: 0 }).opacity).toBe('0');
    expect(spyglassStyle({ ...state, amount: 3 }).opacity).toBe('0.6');
  });
});
