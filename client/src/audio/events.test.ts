import { describe, expect, it } from 'vitest';
import { BLOCK_DEFS, STATE_DEFS } from '../world/blocks.gen';
import { decodeSounds, EventPlayer, setOfState, soundsFor, STEP_PAN, type SoundEvent } from './events';
import { SOUND_DATA } from './sounds.gen';

const kind = (name: string): number => {
  const i = SOUND_DATA.events.findIndex((e) => e.name === name);
  if (i < 0) throw new Error(`no event ${name}`);
  return i;
};
const state = (s: string): number => {
  const d = STATE_DEFS.find((x) => x.state === s);
  if (!d) throw new Error(`no state ${s}`);
  return d.id;
};
const gait = (name: string): number => SOUND_DATA.gaitOrder.indexOf(name);

describe('sound events', () => {
  it("decode from the Rust client's seven numbers", () => {
    // kind, block, intensity, x, y, z, flags (bit 0 spatial, bits 1–3 gait + 1): as sound.rs's test.
    const v = [kind('step'), 3, 1, 0, 0, 0, (gait('run') + 1) << 1, kind('land'), 2, 0.5, 1, 2, 3, 1];
    expect(decodeSounds(v)).toEqual([
      { kind: kind('step'), block: 3, intensity: 1, at: null, gait: gait('run') },
      { kind: kind('land'), block: 2, intensity: 0.5, at: [1, 2, 3], gait: null },
    ]);
    expect(decodeSounds([1, 2, 3])).toEqual([]);
  });

  it("map each block state to its block's set, and air to the fallback", () => {
    expect(setOfState(state('dwell:stone'), SOUND_DATA)).toBe('dwell:stone');
    expect(setOfState(state('dwell:sand'), SOUND_DATA)).toBe('dwell:sand');
    expect(setOfState(state('dwell:ladder[facing=east,flooded=true]'), SOUND_DATA)).toBe('dwell:wood');
    expect(setOfState(0, SOUND_DATA)).toBe(SOUND_DATA.fallback);
    expect(setOfState(99999, SOUND_DATA)).toBe(SOUND_DATA.fallback);
    for (const s of STATE_DEFS) {
      const b = BLOCK_DEFS[s.block];
      expect(setOfState(s.id, SOUND_DATA)).toBe(b?.sound ?? SOUND_DATA.fallback);
    }
  });

  const event = (name: string, block: number, g: number | null = null): SoundEvent => ({
    kind: kind(name),
    block,
    intensity: 1,
    at: null,
    gait: g,
  });

  it("play the set's sound for set events, with the gait's loudness for steps", () => {
    const grass = state('dwell:grass');
    for (const g of SOUND_DATA.gaitOrder) {
      expect(soundsFor(event('step', grass, gait(g)), SOUND_DATA)).toEqual([
        { sound: SOUND_DATA.sets['dwell:grass']?.step, gain: SOUND_DATA.gaits[g]?.gain },
      ]);
    }
    // Faster gaits are louder.
    const gain = (g: string) => SOUND_DATA.gaits[g]?.gain ?? 0;
    expect(gain('crouch')).toBeLessThan(gain('walk'));
    expect(gain('walk')).toBeLessThan(gain('run'));
    expect(soundsFor(event('break', state('dwell:sand')), SOUND_DATA)[0]?.sound).toBe(
      SOUND_DATA.sets['dwell:sand']?.break,
    );
    // A jump pushes off the ground (its step) and whooshes (its own sound).
    expect(soundsFor(event('jump', grass), SOUND_DATA).map((p) => p.sound)).toEqual([
      SOUND_DATA.sets['dwell:grass']?.step,
      'dwell:jump',
    ]);
    // A set without the event falls back to the fallback set; stone (the fallback) has no splash.
    expect(soundsFor(event('enter', state('dwell:stone')), SOUND_DATA)).toEqual([]);
    expect(soundsFor(event('enter', state('dwell:water')), SOUND_DATA)[0]?.sound).toBe('dwell:water_enter');
    expect(soundsFor({ ...event('step', 0), kind: 999 }, SOUND_DATA)).toEqual([]);
  });

  it("alternate the local player's footsteps left and right; others' are placed instead", () => {
    const pans: (number | undefined)[] = [];
    const ats: unknown[] = [];
    const player = new EventPlayer(SOUND_DATA, (_, o) => {
      pans.push(o.pan);
      ats.push(o.at);
    });
    const stone = state('dwell:stone');
    player.playAll([0, 1, 2, 3].map(() => event('step', stone, gait('walk'))));
    expect(pans).toEqual([-STEP_PAN, STEP_PAN, -STEP_PAN, STEP_PAN]);
    pans.length = 0;
    player.playAll([{ ...event('step', stone, gait('walk')), at: [1, 2, 3] }]);
    expect(pans).toEqual([undefined]);
    expect(ats.at(-1)).toEqual([1, 2, 3]);
  });
});
