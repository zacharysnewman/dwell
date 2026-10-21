import { describe, expect, it } from 'vitest';
import { LOADING_KEY, loadingFraction, loadingScreenEnabled } from './loadingScreen';

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
});
