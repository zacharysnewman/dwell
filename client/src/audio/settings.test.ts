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
    });
    expect(loadVolumes(memory({ [AUDIO_KEY]: '{not json' }))).toEqual(DEFAULT_VOLUMES);
    expect(loadVolumes(null)).toEqual(DEFAULT_VOLUMES);
  });

  it('round-trip', () => {
    const store = memory();
    saveVolumes({ master: 0.5, effects: 0.25, ui: 0 }, store);
    expect(loadVolumes(store)).toEqual({ master: 0.5, effects: 0.25, ui: 0 });
  });

  it('are an append-only store: a rewrite keeps fields another version wrote', () => {
    // A later version's field (e.g. a music volume) survives this version saving its own.
    const store = memory({
      [AUDIO_KEY]: JSON.stringify({ master: 0.3, music: 0.7, future: { a: 1 } }),
    });
    saveVolumes({ master: 0.9, effects: 1, ui: 0.5 }, store);
    expect(JSON.parse(store.data[AUDIO_KEY] ?? '{}')).toEqual({
      master: 0.9,
      effects: 1,
      ui: 0.5,
      music: 0.7,
      future: { a: 1 },
    });
  });
});
