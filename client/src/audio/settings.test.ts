import { describe, expect, it } from 'vitest';
import { DEFAULT_VOLUMES } from './mixer';
import { AUDIO_KEY, loadVolumes, sanitizeVolumes, saveVolumes } from './settings';

function memory(initial: Record<string, string> = {}) {
  const data = { ...initial };
  return {
    data,
    getItem: (k: string) => data[k] ?? null,
    setItem: (k: string, v: string) => {
      data[k] = v;
    },
  };
}

describe('volume settings', () => {
  it('default, clamp and survive bad values', () => {
    expect(sanitizeVolumes(null)).toEqual(DEFAULT_VOLUMES);
    expect(sanitizeVolumes({ master: 2, effects: -1, ui: 'loud' })).toEqual({
      master: 1,
      effects: 0,
      ui: DEFAULT_VOLUMES.ui,
      music: DEFAULT_VOLUMES.music,
    });
    expect(loadVolumes(memory({ [AUDIO_KEY]: '{not json' }))).toEqual(DEFAULT_VOLUMES);
    expect(loadVolumes(null)).toEqual(DEFAULT_VOLUMES);
  });

  it('round-trip', () => {
    const store = memory();
    saveVolumes({ master: 0.5, effects: 0.25, ui: 0, music: 0.3 }, store);
    expect(loadVolumes(store)).toEqual({ master: 0.5, effects: 0.25, ui: 0, music: 0.3 });
  });

  it('keep the music volume, 0 (off) included, and default it for an older record', () => {
    // A record an older build wrote, without the music: the default, and its fields kept.
    const old = memory({ [AUDIO_KEY]: JSON.stringify({ master: 0.4, effects: 0.5, ui: 0.2 }) });
    expect(loadVolumes(old).music).toBe(DEFAULT_VOLUMES.music);
    saveVolumes({ ...loadVolumes(old), music: 0 }, old);
    expect(loadVolumes(old)).toEqual({ master: 0.4, effects: 0.5, ui: 0.2, music: 0 });
    expect(sanitizeVolumes({ music: 3 }).music).toBe(1);
    expect(sanitizeVolumes({ music: 'loud' }).music).toBe(DEFAULT_VOLUMES.music);
  });

  it('are an append-only store: a rewrite keeps fields another version wrote', () => {
    // A later version's field survives this version saving its own.
    const store = memory({
      [AUDIO_KEY]: JSON.stringify({ master: 0.3, music: 0.7, future: { a: 1 } }),
    });
    saveVolumes({ master: 0.9, effects: 1, ui: 0.5, music: 0.2 }, store);
    expect(JSON.parse(store.data[AUDIO_KEY] ?? '{}')).toEqual({
      master: 0.9,
      effects: 1,
      ui: 0.5,
      music: 0.2,
      future: { a: 1 },
    });
  });
});
