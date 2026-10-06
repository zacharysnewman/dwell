import { describe, expect, it } from 'vitest';
import { CHANNEL_KEY, loadChannel, saveChannel, type ChannelStore } from './channel';

function memory(): ChannelStore & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return {
    data,
    getItem: (k) => data.get(k) ?? null,
    setItem: (k, v) => {
      data.set(k, v);
    },
  };
}

describe('the release channel setting', () => {
  it('is stable until the player chooses dev', () => {
    const store = memory();
    expect(loadChannel(store)).toBe('stable');
    saveChannel(store, 'dev');
    expect(store.data.get(CHANNEL_KEY)).toBe('dev');
    expect(loadChannel(store)).toBe('dev');
    saveChannel(store, 'stable');
    expect(loadChannel(store)).toBe('stable');
  });

  it('survives blocked or damaged storage', () => {
    expect(loadChannel(null)).toBe('stable');
    saveChannel(null, 'dev');
    const broken: ChannelStore = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
    };
    expect(loadChannel(broken)).toBe('stable');
    expect(() => {
      saveChannel(broken, 'dev');
    }).not.toThrow();
    const odd = memory();
    odd.setItem(CHANNEL_KEY, 'nightly');
    expect(loadChannel(odd)).toBe('stable');
  });
});
