// Owner, 2026-11-13: "Sliding and stamina breath expire should be 75% of their current volume."
// Every gain those sounds play at (the slide loops and their stops, the out-of-stamina breath's
// sample and its fallback layers) is 0.75 × the level before the change.
import { describe, expect, it } from 'vitest';
import { SOUND_DATA } from './sounds.gen';

const sound = (id: string) => {
  const s = SOUND_DATA.sounds[`dwell:${id}`];
  if (!s) throw new Error(`no sound ${id}`);
  return s;
};

describe('sliding and the out-of-stamina breath at 75 % (owner, 2026-11-13)', () => {
  it('plays every slide sound at 0.75 × its old gain', () => {
    for (const m of ['stone', 'dirt', 'grass', 'sand']) {
      expect(sound(`${m}_slide`).gain).toBeCloseTo(0.75 * 0.8, 9);
      expect(sound(`${m}_slide_stop`).gain).toBeCloseTo(0.75 * 0.7, 9);
    }
  });

  it("plays the exhausted breath's sample and its fallback at 0.75 × their old gains", () => {
    const id = SOUND_DATA.events.find((e) => e.name === 'exhausted')?.sound;
    expect(id).toBe('dwell:breath_heavy');
    const breath = sound('breath_heavy');
    expect(breath.samples?.gain).toBeCloseTo(0.75 * 0.5, 9);
    expect(breath.gain).toBeCloseTo(0.75 * 0.4, 9);
  });
});
