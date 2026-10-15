import { describe, expect, it } from 'vitest';
import { readSoundFiles } from '../../../shared/sounds/load.mjs';
import { checkBlockSounds, resolveSounds, UI_KINDS } from '../../../shared/sounds/validate.mjs';
import { BLOCK_DEFS } from '../world/blocks.gen';
import { soundsFor, type Registry } from './events';
import { SOUND_DATA } from './sounds.gen';
import type { SoundData } from './types';

type File = Record<string, unknown> & { sounds: Record<string, unknown> };

/** A fresh copy of the shipped data file(s). */
function files(): File[] {
  return structuredClone(readSoundFiles()) as File[];
}

function dwell(fs: File[]): File {
  const f = fs.find((x) => x.namespace === 'dwell');
  if (!f) throw new Error('no dwell namespace');
  return f;
}

const tick = {
  gain: 0.5,
  variation: { pitch: 1 },
  layers: [{ source: 'noise', color: 'white', gain: 0.5, env: { attack: 0.001, decay: 0.02 } }],
};

describe('sound data', () => {
  it('validates, and the generated data is current', () => {
    const resolved = resolveSounds(readSoundFiles());
    expect({ ...resolved, gaitOrder: SOUND_DATA.gaitOrder }).toEqual(SOUND_DATA);
  });

  it('gives every block of the registry a known set', () => {
    for (const b of BLOCK_DEFS) {
      if (b.sound) expect(SOUND_DATA.sets, b.id).toHaveProperty([b.sound]);
    }
    expect(SOUND_DATA.sets).toHaveProperty([SOUND_DATA.fallback]);
  });

  it('keeps UI sounds identical every time', () => {
    for (const k of UI_KINDS) {
      const id = SOUND_DATA.ui[k];
      expect(id, k).toBeDefined();
      expect(SOUND_DATA.sounds[id ?? '']?.variation).toBe('none');
    }
  });

  const sets = (f: File) => f.sets as Record<string, unknown>;
  const rec = (f: File, key: string) => f[key] as Record<string, unknown>;
  const layer = tick.layers[0];
  const rejects: [string, (f: File) => void, RegExp][] = [
    [
      'an unknown source',
      (f) => {
        f.sounds.tick = { ...tick, layers: [{ ...layer, source: 'fm' }] };
      },
      /sound dwell:tick\.layers\[0\]\.source: "fm" is not one of/,
    ],
    [
      'an unknown field',
      (f) => {
        f.sounds.tick = { ...tick, loud: 1 };
      },
      /sound dwell:tick: unknown field "loud"/,
    ],
    [
      'a filter out of range',
      (f) => {
        f.sounds.tick = {
          ...tick,
          layers: [{ ...layer, filters: [{ type: 'lowpass', freq: 40000 }] }],
        };
      },
      /filters\[0\]\.freq: 40000 is outside/,
    ],
    [
      'a noise layer without an envelope',
      (f) => {
        f.sounds.tick = { ...tick, layers: [{ source: 'noise', color: 'pink', gain: 1 }] };
      },
      /a noise layer needs an env/,
    ],
    [
      'bad modal partials',
      (f) => {
        f.sounds.tick = {
          ...tick,
          layers: [{ source: 'modal', freq: 300, gain: 1, partials: [[1, 2]] }],
        };
      },
      /partials\[0\]: must be \[ratio, gain, decay\]/,
    ],
    [
      'a set naming an unknown sound',
      (f) => {
        sets(f).metal = { step: 'nope', land: 'nope', break: 'nope', place: 'nope', hit: 'nope' };
      },
      /set dwell:metal\.step: unknown sound "nope"/,
    ],
    [
      'a set missing a required event',
      (f) => {
        sets(f).metal = { step: 'stone_step' };
      },
      /set dwell:metal: missing the "land" sound/,
    ],
    [
      'a UI sound that varies',
      (f) => {
        rec(f, 'ui').click = 'stone_step';
      },
      /UI sounds do not vary/,
    ],
    [
      'a control without sounds',
      (f) => {
        rec(f, 'controls').Walk = [];
      },
      /control "Walk": must list its sounds/,
    ],
    [
      'a control naming an unknown sound',
      (f) => {
        rec(f, 'controls').Walk = ['event:moonwalk'];
      },
      /unknown sound reference "event:moonwalk"/,
    ],
    [
      'a silent control without a reason',
      (f) => {
        rec(f, 'controls').Look = { silent: '' };
      },
      /a silent control says why/,
    ],
    [
      'an unknown fallback',
      (f) => {
        f.fallback = 'metal';
      },
      /fallback: unknown set "dwell:metal"/,
    ],
    [
      'a gait out of range',
      (f) => {
        rec(f, 'gaits').walk = { stride: 0, gain: 1 };
      },
      /gait walk\.stride: 0 is outside/,
    ],
    [
      'an envelope in a loop sound',
      (f) => {
        f.sounds.hum = { ...tick, loop: true };
      },
      /sound dwell:hum\.layers\[0\]: env is not for loop sounds/,
    ],
    [
      'tremolo on a one-shot',
      (f) => {
        f.sounds.tick = { ...tick, layers: [{ ...layer, tremolo: { rate: 5, depth: 0.5 } }] };
      },
      /sound dwell:tick\.layers\[0\]: tremolo is for loop sounds/,
    ],
    [
      "a one-shot as a set's slide (the event starts a loop)",
      (f) => {
        (sets(f).sand as Record<string, string>).slide = 'sand_step';
      },
      /set dwell:sand\.slide: dwell:sand_step must be a loop sound/,
    ],
    [
      'a loop update that plays a sound',
      (f) => {
        rec(f, 'events').slide_speed = { loop: 'update', sound: 'jump' };
      },
      /event slide_speed: a loop update plays nothing of its own/,
    ],
    [
      'a fallback set without a slide',
      (f) => {
        delete (sets(f).stone as Record<string, string>).slide;
      },
      /event slide: the fallback set dwell:stone has no "slide" sound/,
    ],
  ];
  for (const [what, change, message] of rejects) {
    it(`rejects ${what}, naming the entry`, () => {
      const fs = files();
      change(dwell(fs));
      expect(() => resolveSounds(fs)).toThrow(message);
    });
  }

  it('rejects a block naming an unknown set', () => {
    expect(() => {
      checkBlockSounds(
        [{ id: 'dwell:iron', sound: 'dwell:metal' }],
        resolveSounds(readSoundFiles()),
      );
    }).toThrow(/dwell:iron: unknown sound set "dwell:metal"/);
  });

  it('takes a new set and a block mapped to it by data alone', () => {
    const fs = files();
    const f = dwell(fs);
    f.sounds.metal_clang = {
      gain: 0.8,
      variation: { pitch: 1, gain: 1 },
      layers: [
        {
          source: 'modal',
          freq: 800,
          gain: 0.3,
          partials: [
            [1, 1, 0.3],
            [2.76, 0.5, 0.2],
          ],
        },
      ],
    };
    (f.sets as Record<string, unknown>).metal = {
      step: 'metal_clang',
      land: 'metal_clang',
      break: 'metal_clang',
      place: 'metal_clang',
      hit: 'metal_clang',
    };
    const data: SoundData = { ...resolveSounds(fs), gaitOrder: SOUND_DATA.gaitOrder };
    const blocks = [{ id: 'dwell:iron', sound: 'dwell:metal' }];
    expect(() => {
      checkBlockSounds(blocks, data);
    }).not.toThrow();
    // A registry whose state 1 is the new block: its steps and breaks play the new set's sound.
    const registry: Registry = {
      blocks: [
        { id: 'dwell:air', first: 0, count: 1, properties: [] },
        { id: 'dwell:iron', first: 1, count: 1, properties: [], sound: 'dwell:metal' },
      ],
      states: [
        {
          id: 0,
          state: 'dwell:air',
          block: 0,
          values: {},
          look: 'cube',
          color: 0,
          opacity: 0,
          shape: 0,
          flooded: false,
          placeable: false,
        },
        {
          id: 1,
          state: 'dwell:iron',
          block: 1,
          values: {},
          look: 'cube',
          color: 0,
          opacity: 1,
          shape: 1,
          flooded: false,
          placeable: true,
        },
      ],
    };
    const kind = (name: string): number => data.events.findIndex((e) => e.name === name);
    const event = (name: string) => ({
      kind: kind(name),
      block: 1,
      intensity: 1,
      at: null,
      gait: null,
      voice: null,
    });
    expect(soundsFor(event('break'), data, registry)).toEqual([
      { sound: 'dwell:metal_clang', gain: 1 },
    ]);
    expect(soundsFor(event('step'), data, registry)).toEqual([
      { sound: 'dwell:metal_clang', gain: 1 },
    ]);
  });
});
