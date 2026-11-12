// BUGS.md #103 (owner, 2026-11-12): walking through tall grass was loud, fast and crackly ("are
// there 3 SFX playing while walking on grass?"): a rustle every 0.8 m (~5 a second) stacked two or
// three deep over each footstep, its white-noise grains clicking. These pin the per-sound voice
// limit (drop, never restart), the retrigger gap, and the level of one step through tall grass.
import { describe, expect, it } from 'vitest';
import { setOfState } from './events';
import { KNEE } from './mixer';
import { SOUND_DATA } from './sounds.gen';
import type { SoundDef } from './types';
import { DEFAULT_SOUND_VOICES, VoiceLimits } from './voices';
import { BLOCK_DEFS } from '../world/blocks.gen';

/** A sound's loudest play at a gait's gain: its samples' level (or its procedural layers' summed
 * level, an upper bound) at full intensity. */
function peak(def: SoundDef, gain: number): number {
  const intensity = def.intensity?.gain?.[1] ?? 1;
  const level = def.samples
    ? (def.samples.gain ?? 1)
    : def.gain * def.layers.reduce((a, l) => a + l.gain, 0);
  return level * gain * intensity;
}

const stateOf = (name: string): number => BLOCK_DEFS.find((d) => d.id === name)?.first ?? -1;

describe('per-sound voice limits', () => {
  it('drop a play over the limit and never restart one that sounds', () => {
    const v = new VoiceLimits();
    const def = { voices: 1, minGapS: 0 };
    expect(v.admit('a', def, 0)).toBe(true);
    v.started('a', 0, 0.4);
    // A second play while the first sounds is dropped; the first plays on to its end.
    expect(v.admit('a', def, 0.2)).toBe(false);
    expect(v.playing('a', 0.3)).toBe(1);
    // Another sound is not limited by it.
    expect(v.admit('b', def, 0.2)).toBe(true);
    // Once it ends, the next plays.
    expect(v.admit('a', def, 0.41)).toBe(true);
  });

  it('allow no second play within minGapS, and DEFAULT_SOUND_VOICES at once by default', () => {
    const v = new VoiceLimits();
    expect(v.admit('a', { minGapS: 0.25 }, 0)).toBe(true);
    v.started('a', 0, 0.05);
    expect(v.admit('a', { minGapS: 0.25 }, 0.2)).toBe(false);
    expect(v.admit('a', { minGapS: 0.25 }, 0.26)).toBe(true);
    const w = new VoiceLimits();
    let n = 0;
    for (let i = 0; i < 20; i++) {
      if (w.admit('x', {}, i * 0.01)) {
        w.started('x', i * 0.01, 10);
        n++;
      }
    }
    expect(n).toBe(DEFAULT_SOUND_VOICES);
  });

  it('play one brush at a time, never retriggered within its gap, for every set that rustles', () => {
    for (const [set, events] of Object.entries(SOUND_DATA.sets)) {
      if (!events.brush) continue;
      const def = SOUND_DATA.sounds[events.brush];
      expect(def, `${set}'s brush`).toBeDefined();
      if (!def) continue;
      expect(def.voices, `${events.brush}.voices`).toBe(1);
      expect(def.minGapS ?? 0, `${events.brush}.minGapS`).toBeGreaterThan(0);
    }
  });
});

describe('a step through tall grass', () => {
  const tall = stateOf('dwell:tall_grass');
  const grass = stateOf('dwell:grass');

  it('is the grass footstep and a soft rustle, together under the limiter knee', () => {
    const brushSet = SOUND_DATA.sets[setOfState(tall, SOUND_DATA)];
    const stepSet = SOUND_DATA.sets[setOfState(grass, SOUND_DATA)];
    const brush = brushSet?.brush ? SOUND_DATA.sounds[brushSet.brush] : undefined;
    const step = stepSet?.step ? SOUND_DATA.sounds[stepSet.step] : undefined;
    expect(brush && step).toBeTruthy();
    if (!brush || !step) return;
    // The loudest gait (the run): one footstep and at most one rustle on it, with headroom left
    // under the limiter's knee for the step before it still ringing.
    const run = SOUND_DATA.gaits.run?.gain ?? 1;
    const stack = peak(step, run) + peak(brush, 1);
    expect(stack).toBeLessThanOrEqual(0.5 * KNEE + 0.1);
    // The rustle sits well under the footstep: a layer, not a second footstep.
    expect(peak(brush, 1)).toBeLessThanOrEqual(0.25 * peak(step, run));
    // Every voice of the footstep at once still under the knee.
    expect((step.voices ?? DEFAULT_SOUND_VOICES) * peak(step, run)).toBeLessThanOrEqual(KNEE);
  });

  it('has no crackle: the rustle is soft noise without sharp grains', () => {
    const set = SOUND_DATA.sets[setOfState(tall, SOUND_DATA)];
    const brush = set?.brush ? SOUND_DATA.sounds[set.brush] : undefined;
    for (const l of brush?.layers ?? []) {
      expect(l.grains, 'no grain bursts').toBeUndefined();
      expect(l.env?.attack ?? 0).toBeGreaterThanOrEqual(0.02);
      expect(l.color).not.toBe('white');
    }
  });

  it('rustles at most once a footstep (the data the Rust game reads)', () => {
    // Off its feet one rustle per `everyM`; at least a walk stride apart, at most ~3 a second.
    expect(SOUND_DATA.brush.everyM).toBeGreaterThanOrEqual(SOUND_DATA.gaits.walk?.stride ?? 0);
    expect(1 / SOUND_DATA.brush.minS).toBeLessThanOrEqual(3.5);
  });
});
