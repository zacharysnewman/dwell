import { describe, expect, it } from 'vitest';
import { readSoundFiles } from '../../../shared/sounds/load.mjs';
import { resolveSounds } from '../../../shared/sounds/validate.mjs';
import { soundsFor, type SoundEvent } from './events';
import { SOUND_DATA } from './sounds.gen';

const kind = (name: string): number => SOUND_DATA.events.findIndex((e) => e.name === name);
const event = (name: string, model: number): SoundEvent => ({
  kind: kind(name),
  block: model,
  intensity: 1,
  at: [1, 2, 3],
  gait: null,
  voice: null,
});
const models = Object.keys(SOUND_DATA.creatures.models);

describe('creature and body sounds (AUDIO.md §2d, §5)', () => {
  it("plays a creature's own step, startle and call by its model's index", () => {
    const wolf = models.indexOf('wolf');
    const deer = models.indexOf('deer');
    expect(soundsFor(event('creature_step', wolf), SOUND_DATA)).toEqual([
      { sound: 'dwell:wolf_step', gain: 1 },
    ]);
    expect(soundsFor(event('creature_call', wolf), SOUND_DATA)[0]?.sound).toBe('dwell:wolf_howl');
    expect(soundsFor(event('creature_startle', deer), SOUND_DATA)[0]?.sound).toBe(
      'dwell:deer_snort',
    );
    // A deer has no call; an unknown index plays nothing.
    expect(soundsFor(event('creature_call', deer), SOUND_DATA)).toEqual([]);
    expect(soundsFor(event('creature_step', 99), SOUND_DATA)).toEqual([]);
  });

  it('breathes, gasps and strains from the vitals events', () => {
    for (const [e, s] of [
      ['exhausted', 'dwell:breath_heavy'],
      ['gasp', 'dwell:gasp'],
      ['strain', 'dwell:strain'],
    ] as const)
      expect(soundsFor(event(e, 0), SOUND_DATA)[0]?.sound).toBe(s);
    expect(SOUND_DATA.sounds['dwell:fell_crack']?.samples?.files[0]).toMatch(/^fell-crack-impact-/);
  });

  it('takes a new creature by data alone, and names a bad one', () => {
    const fs = structuredClone(readSoundFiles()) as {
      namespace: string;
      creatures?: { models: Record<string, unknown> };
    }[];
    const dwell = fs.find((f) => f.namespace === 'dwell');
    if (!dwell?.creatures) throw new Error('no creatures');
    dwell.creatures.models.fox = {
      stride: 0.6,
      step: 'wolf_step',
      call: 'wolf_howl',
      callEveryS: [20, 40],
    };
    expect(resolveSounds(fs).creatures.models.fox?.call).toBe('dwell:wolf_howl');
    dwell.creatures.models.fox = { stride: 0.6, call: 'yip' };
    expect(() => resolveSounds(fs)).toThrow('creature fox.call');
    dwell.creatures.models.fox = { stride: 0.6, callEveryS: [20, 40] };
    expect(() => resolveSounds(fs)).toThrow('needs a call');
  });
});
