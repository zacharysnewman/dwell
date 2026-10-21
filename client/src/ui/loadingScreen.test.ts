import { describe, expect, it } from 'vitest';
import { LOADING_FADE } from './loading.gen';
import {
  LOADING_KEY,
  LoadingFlow,
  fadeDurationS,
  fadeOpacity,
  loadingFraction,
  loadingScreenEnabled,
} from './loadingScreen';

const FADE = {
  holdS: 0.1,
  durationS: 0.8,
  easePower: 3,
  cpuDurationS: 0.2,
  reducedMotionDurationS: 0,
};
const hud = (ready: number, needed: number) => ({ text: String(ready), ready, needed });

const store = (v: string | null) => ({
  getItem: (k: string) => (k === LOADING_KEY ? v : null),
  setItem: () => {},
});

describe('the loading screen', () => {
  it('is on by default and off for ?loading=off or the store set to "off"', () => {
    expect(loadingScreenEnabled('', null)).toBe(true);
    expect(loadingScreenEnabled('?world=flat', store(null))).toBe(true);
    expect(loadingScreenEnabled('?loading=off', null)).toBe(false);
    expect(loadingScreenEnabled('?world=flat&loading=off', store(null))).toBe(false);
    expect(loadingScreenEnabled('', store('"off"'))).toBe(false);
    expect(loadingScreenEnabled('', store('off'))).toBe(false);
    expect(loadingScreenEnabled('', store('"on"'))).toBe(true);
    expect(loadingScreenEnabled('?loading=on', store(null))).toBe(true);
  });

  it('keeps the default when storage throws', () => {
    const blocked = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {},
    };
    expect(loadingScreenEnabled('', blocked)).toBe(true);
  });

  it('fills the bar with the chunks drawn out of those needed', () => {
    expect(loadingFraction({ text: '', ready: 0, needed: 120 })).toBe(0);
    expect(loadingFraction({ text: '', ready: 30, needed: 120 })).toBe(0.25);
    expect(loadingFraction({ text: '', ready: 130, needed: 120 })).toBe(1);
    expect(loadingFraction({ text: '', ready: 0, needed: 0 })).toBe(1);
  });

  it('is up from the start, before the game has a HUD, and through the join', () => {
    // Owner (2026-10-21): "you see ingame UI briefly before loading screen appears".
    const flow = new LoadingFlow(true, FADE, 0.8);
    expect(flow.phase).toBe('waiting');
    expect(flow.update({}, 0)).toBe(1); // the client has not started: no HUD yet
    expect(
      flow.update({ loading: { text: 'Joining…', ready: 0, needed: 0 }, loadingOpens: 0 }, 1),
    ).toBe(1);
    expect(flow.update({ loading: hud(10, 179), loadingOpens: 0 }, 2)).toBe(1);
    expect(flow.phase).toBe('waiting');
    expect(flow.hud).toEqual(hud(10, 179));
  });

  it('fades out from white when it opens, then is gone', () => {
    const flow = new LoadingFlow(true, FADE, 0.8);
    flow.update({ loading: hud(10, 179), loadingOpens: 0 }, 1);
    expect(flow.update({ loading: null, loadingOpens: 1 }, 2)).toBe(1);
    expect(flow.phase).toBe('fading');
    expect(flow.openings).toBe(1);
    expect(flow.update({ loading: null, loadingOpens: 1 }, 2.05)).toBe(1); // the hold
    const mid = flow.update({ loading: null, loadingOpens: 1 }, 2.5);
    expect(mid).toBeGreaterThan(0);
    expect(mid).toBeLessThan(0.5);
    expect(flow.update({ loading: null, loadingOpens: 1 }, 2.95)).toBe(0);
    expect(flow.phase).toBe('open');
    expect(flow.update({ loading: null, loadingOpens: 1 }, 3)).toBe(0);
    expect(flow.openings).toBe(1);
  });

  it('on respawn waits and fades again, even when the ring opens in its first frame', () => {
    const flow = new LoadingFlow(true, FADE, 0.8);
    flow.update({ loading: null, loadingOpens: 1 }, 0);
    flow.update({ loading: null, loadingOpens: 1 }, 5);
    expect(flow.phase).toBe('open');
    // Respawned where the ring still loads: waiting, then the fade.
    expect(flow.update({ loading: hud(3, 179), loadingOpens: 1 }, 6)).toBe(1);
    expect(flow.phase).toBe('waiting');
    flow.update({ loading: null, loadingOpens: 2 }, 7);
    expect(flow.phase).toBe('fading');
    flow.update({ loading: null, loadingOpens: 2 }, 9);
    expect(flow.phase).toBe('open');
    // Respawned where it was loaded already: opened in the frame it started, still from white.
    expect(flow.update({ loading: null, loadingOpens: 3 }, 10)).toBe(1);
    expect(flow.phase).toBe('fading');
    expect(flow.openings).toBe(3);
  });

  it('turned off, it never shows', () => {
    const flow = new LoadingFlow(false, FADE, 0.8);
    expect(flow.phase).toBe('open');
    expect(flow.update({}, 0)).toBe(0);
    expect(flow.update({ loading: null, loadingOpens: 0 }, 1)).toBe(0);
    expect(flow.phase).toBe('open');
  });

  it('a start that fails is uncovered', () => {
    const flow = new LoadingFlow(true, FADE, 0.8);
    flow.abort();
    expect(flow.update({}, 0)).toBe(0);
    expect(flow.update({ loading: hud(0, 0), loadingOpens: 0 }, 1)).toBe(0);
    expect(flow.phase).toBe('open');
  });

  it('pins the fade curve: a hold, then an ease-out (1 − x)^p', () => {
    expect(fadeOpacity(0, FADE, 0.8)).toBe(1);
    expect(fadeOpacity(0.0999, FADE, 0.8)).toBe(1);
    expect(fadeOpacity(0.1, FADE, 0.8)).toBe(1);
    expect(fadeOpacity(0.3, FADE, 0.8)).toBeCloseTo(0.75 ** 3, 10); // a quarter through
    expect(fadeOpacity(0.5, FADE, 0.8)).toBeCloseTo(0.125, 10); // halfway: most of the view is up
    expect(fadeOpacity(0.7, FADE, 0.8)).toBeCloseTo(0.25 ** 3, 10);
    expect(fadeOpacity(0.9, FADE, 0.8)).toBe(0);
    expect(fadeOpacity(5, FADE, 0.8)).toBe(0);
    // Ease-out: it falls fastest at the start.
    const step = (t: number) => fadeOpacity(t, FADE, 0.8) - fadeOpacity(t + 0.1, FADE, 0.8);
    expect(step(0.1)).toBeGreaterThan(step(0.4));
    expect(step(0.4)).toBeGreaterThan(step(0.7));
    // No fade: no hold either.
    expect(fadeOpacity(0, FADE, 0)).toBe(0);
  });

  it('is quick on a CPU adapter and none for reduced motion', () => {
    expect(fadeDurationS(FADE, false, false)).toBe(0.8);
    expect(fadeDurationS(FADE, true, false)).toBe(0.2);
    expect(fadeDurationS(FADE, false, true)).toBe(0);
    expect(fadeDurationS(FADE, true, true)).toBe(0);
    const flow = new LoadingFlow(true, FADE, fadeDurationS(FADE, false, true));
    flow.update({ loading: hud(1, 2), loadingOpens: 0 }, 0);
    expect(flow.update({ loading: null, loadingOpens: 1 }, 1)).toBe(0);
    expect(flow.phase).toBe('open');
    expect(flow.openings).toBe(1);
  });

  it('reads its fade from loading.json (a white or near-white colour, under a second)', () => {
    expect(LOADING_FADE.color).toMatch(/^#[ef][0-9a-f][ef][0-9a-f][ef][0-9a-f]$/);
    expect(LOADING_FADE.durationS).toBeGreaterThanOrEqual(0.6);
    expect(LOADING_FADE.durationS).toBeLessThanOrEqual(1);
    expect(LOADING_FADE.cpuDurationS).toBeLessThanOrEqual(LOADING_FADE.durationS);
  });
});
