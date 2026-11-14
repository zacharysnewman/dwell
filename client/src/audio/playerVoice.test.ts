// The player's own voice (BUGS.md #159): running out of stamina sounded like a bear. The breath's
// samples were the pack's `MONSTER_Exhausted_05`/`_07` (a voiced growl, ~110–180 Hz), not a person.
// These pin that every vocal event of the player plays a human voice, at a natural pitch.
import { describe, expect, it } from 'vitest';
import { SampleBank, usesSamples } from './samples';
import { SOUND_DATA } from './sounds.gen';
import type { SoundDef } from './types';
import { mulberry32, semitonesToRate, Variation } from './variation';

/** The game's events voiced by the player themself (AUDIO.md §5: the body's and the server's). */
const PLAYER_VOICE = ['exhausted', 'gasp', 'strain', 'hurt', 'death'];
/** The pack's categories (a file name's first word) that are a human voice. */
const HUMAN = ['GRUNT', 'GROAN', 'BREATH', 'VOICE'];
/** Natural pitch: every play within ±10 % of the recording's rate. */
const RATE = [0.9, 1.1] as const;

const soundOf = (event: string): [string, SoundDef] => {
  const id = SOUND_DATA.events.find((e) => e.name === event)?.sound;
  const s = id === undefined ? undefined : SOUND_DATA.sounds[id];
  if (id === undefined || !s) throw new Error(`event ${event} plays no sound`);
  return [id, s];
};

describe("the player's voice (BUGS.md #159)", () => {
  it('turns semitones into a playback rate', () => {
    expect(semitonesToRate(0)).toBe(1);
    expect(semitonesToRate(12)).toBeCloseTo(2, 12);
    expect(semitonesToRate(-12)).toBeCloseTo(0.5, 12);
    expect(semitonesToRate(-24)).toBeCloseTo(0.25, 12);
    expect(semitonesToRate(1)).toBeCloseTo(1.059463, 6);
    // ±0.5 semitone (a voice's spread) is about ±3 %, nowhere near a growl.
    expect(semitonesToRate(-0.5)).toBeCloseTo(0.9715, 4);
  });

  it("draws a sample's rate from its ± semitones and its shift", () => {
    const bank = new SampleBank<{ duration: number }>(mulberry32(3));
    bank.set('a.m4a', { duration: 1 });
    const def = (pitch: number, shift?: number): SoundDef =>
      ({
        layers: [],
        samples: { files: ['a.m4a'], pitch, ...(shift !== undefined ? { shift } : {}) },
      }) as unknown as SoundDef;
    for (let i = 0; i < 200; i++) {
      const r = bank.pick('x', def(1))?.rate ?? NaN;
      expect(r).toBeGreaterThanOrEqual(semitonesToRate(-1));
      expect(r).toBeLessThanOrEqual(semitonesToRate(1));
    }
    expect(bank.pick('y', def(0, -12))?.rate).toBeCloseTo(0.5, 12);
  });

  for (const event of PLAYER_VOICE)
    it(`${event} plays a human voice, never an animal's or a monster's`, () => {
      const [id, s] = soundOf(event);
      if (!usesSamples(s)) return; // its procedural layers: no recording to be wrong
      const from = s.samples?.from;
      expect(from, `${id} names no pack files (samples.from)`).toBeDefined();
      expect(from?.length).toBe(s.samples?.files.length);
      for (const name of from ?? [])
        expect(HUMAN, `${id} plays ${name}`).toContain(name.split('_')[0]);
    });

  for (const event of PLAYER_VOICE)
    it(`${event} plays at a natural pitch (rate within 0.9–1.1)`, () => {
      const [id, s] = soundOf(event);
      if (usesSamples(s)) {
        const bank = new SampleBank<{ duration: number }>(mulberry32(7));
        for (const f of s.samples?.files ?? []) bank.set(f, { duration: 1 });
        for (let i = 0; i < 300; i++) {
          const r = bank.pick(id, s)?.rate ?? NaN;
          expect(r).toBeGreaterThanOrEqual(RATE[0]);
          expect(r).toBeLessThanOrEqual(RATE[1]);
        }
      }
      // The procedural layers (before the samples decode) vary by the same bounds.
      const v = new Variation(11);
      for (let i = 0; i < 300; i++) {
        const r = semitonesToRate(v.draw(id, s).pitch);
        expect(r).toBeGreaterThanOrEqual(RATE[0]);
        expect(r).toBeLessThanOrEqual(RATE[1]);
      }
    });

  it("the out-of-stamina breath is a man's recorded breath, within ±0.5 semitone", () => {
    const [id, s] = soundOf('exhausted');
    expect(id).toBe('dwell:breath_heavy');
    expect(usesSamples(s), 'breath_heavy plays recorded samples, not only noise').toBe(true);
    const from = s.samples?.from ?? [];
    expect(from.length).toBeGreaterThanOrEqual(2);
    for (const name of from) {
      expect(['GRUNT', 'BREATH', 'VOICE'], name).toContain(name.split('_')[0]);
      expect(name, 'a male voice, like the gasp and the strain').toMatch(/_Male_/i);
    }
    const bank = new SampleBank<{ duration: number }>(mulberry32(5));
    for (const f of s.samples?.files ?? []) bank.set(f, { duration: 1 });
    for (let i = 0; i < 300; i++) {
      const r = bank.pick(id, s)?.rate ?? NaN;
      expect(r).toBeGreaterThanOrEqual(semitonesToRate(-0.5) - 1e-9);
      expect(r).toBeLessThanOrEqual(semitonesToRate(0.5) + 1e-9);
    }
  });

  it("the creatures' voices are named by the pack too (an animal's, not a person's)", () => {
    for (const [id, s] of Object.entries(SOUND_DATA.sounds))
      for (const name of s.samples?.from ?? [])
        if (/(wolf|deer)_(growl|howl|snort)$/.test(id)) expect(name).toMatch(/^ANIMAL_/);
  });
});
