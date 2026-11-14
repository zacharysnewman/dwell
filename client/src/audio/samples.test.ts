import { cpSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { readSoundFiles } from '../../../shared/sounds/load.mjs';
import {
  checkBlockSounds,
  resolveSounds,
  SAMPLES_DIR,
  SAMPLES_MAX_BYTES,
} from '../../../shared/sounds/validate.mjs';
import { BLOCK_DEFS, STATE_DEFS } from '../world/blocks.gen';
import { roomSend, sampleGain } from './engine';
import { soundsFor, type Registry } from './events';
import { SampleBank, usesSamples } from './samples';
import { SOUND_DATA } from './sounds.gen';
import type { SoundData, SoundDef } from './types';
import { mulberry32 } from './variation';

type File = Record<string, unknown> & {
  sounds: Record<string, Record<string, unknown>>;
  sets: Record<string, unknown>;
};

const files = (): File[] => structuredClone(readSoundFiles()) as File[];
const dwell = (fs: File[]): File => {
  const f = fs.find((x) => x.namespace === 'dwell');
  if (!f) throw new Error('no dwell namespace');
  return f;
};
const sound = (id: string): SoundDef => {
  const s = SOUND_DATA.sounds[id];
  if (!s) throw new Error(`no sound ${id}`);
  return s;
};

/** A copy of the shipped samples' directory with the named (tiny) files added. */
function sampleDir(names: string[], bytes = 16): string {
  const dir = mkdtempSync(join(tmpdir(), 'dwell-samples-'));
  cpSync(SAMPLES_DIR, dir, { recursive: true });
  for (const n of names) writeFileSync(join(dir, n), Buffer.alloc(bytes, 1));
  return dir;
}

const clang = {
  gain: 0.5,
  variation: { pitch: 1 },
  layers: [{ source: 'modal', freq: 600, gain: 0.5, partials: [[1, 1, 0.3]] }],
};

const event = (data: SoundData, name: string, block: number) => ({
  kind: data.events.findIndex((e) => e.name === name),
  block,
  intensity: 1,
  at: null,
  gait: null,
  voice: null,
});

describe('sound samples: data', () => {
  it('fails on a missing file, a bad name, a loop with samples and `play` without samples', () => {
    const cases: [(f: File) => void, string][] = [
      [
        (f) => {
          (f.sounds.stone_step as Record<string, unknown>).samples = { files: ['nope-1.m4a'] };
        },
        'no file "nope-1.m4a"',
      ],
      [
        (f) => {
          (f.sounds.stone_step as Record<string, unknown>).samples = { files: ['Bad Name.wav'] };
        },
        'is not a lower-case .m4a or .mp3 name',
      ],
      [
        (f) => {
          (f.sounds.stone_slide as Record<string, unknown>).samples = { files: ['a.m4a'] };
        },
        'loop sounds are procedural only',
      ],
      [
        (f) => {
          (f.sounds.ui_click as Record<string, unknown>).play = 'procedural';
        },
        'needs samples to choose between',
      ],
      [
        (f) => {
          (f.sounds.stone_step as Record<string, unknown>).play = 'recorded';
        },
        'is not one of samples, procedural',
      ],
      [
        (f) => {
          (f.sounds.hurt as Record<string, unknown>).samples = {
            files: ['hurt-1.m4a', 'hurt-2.m4a'],
            from: ['GRUNT_Male_B_Hurt_Short_01'],
          };
        },
        "must name the pack's file for each of files",
      ],
      [
        (f) => {
          (f.sounds.hurt as Record<string, unknown>).samples = {
            files: ['hurt-1.m4a'],
            from: ['grunt male'],
          };
        },
        'is not a pack file name',
      ],
    ];
    for (const [edit, message] of cases) {
      const fs = files();
      edit(dwell(fs));
      expect(() => resolveSounds(fs), message).toThrow(message);
    }
  });

  it('fails when the samples together are over the budget', () => {
    const fs = files();
    const f = dwell(fs);
    f.sounds.stone_step = { ...clang, samples: { files: ['big.m4a'] } };
    const dir = sampleDir(['big.m4a'], SAMPLES_MAX_BYTES + 1);
    expect(() => resolveSounds(fs, dir)).toThrow('byte budget');
  });

  it('ships within the ~5 MB budget, every file content-named', () => {
    expect(SOUND_DATA.sampleBytes).toBeGreaterThan(0);
    expect(SOUND_DATA.sampleBytes).toBeLessThanOrEqual(5_000_000);
    for (const s of SOUND_DATA.sampleFiles) expect(s.file).toMatch(/-[0-9a-f]{8}\.m4a$/);
  });

  it('keeps the slide procedural, and gives the most-heard events samples', () => {
    for (const [id, s] of Object.entries(SOUND_DATA.sounds))
      if (id.includes('slide') || s.loop) expect(usesSamples(s), id).toBe(false);
    for (const id of [
      'dwell:stone_step',
      'dwell:grass_step',
      'dwell:stone_break',
      'dwell:dynamite_boom',
    ])
      expect(usesSamples(sound(id)), id).toBe(true);
  });
});

describe('sound samples: a material by data alone', () => {
  it('a new material set with samples plays its own files on its block, else its layers', () => {
    const fs = files();
    const f = dwell(fs);
    f.sounds.metal_clang = { ...clang, samples: { files: ['metal-1.m4a', 'metal-2.m4a'] } };
    f.sets.metal = Object.fromEntries(
      ['step', 'land', 'break', 'place', 'hit'].map((e) => [e, 'metal_clang']),
    );
    const data: SoundData = {
      ...resolveSounds(fs, sampleDir(['metal-1.m4a', 'metal-2.m4a'])),
      gaitOrder: SOUND_DATA.gaitOrder,
    };
    checkBlockSounds([{ id: 'dwell:iron', sound: 'dwell:metal' }], data);
    const stone = STATE_DEFS[1]; // any state: the fixture overrides what the sounds read
    if (!stone) throw new Error('no states');
    const registry: Registry = {
      blocks: [
        { id: 'dwell:air', first: 0, count: 1, properties: [] },
        { id: 'dwell:iron', first: 1, count: 1, properties: [], sound: 'dwell:metal' },
      ],
      states: [
        { ...stone, id: 0, state: 'dwell:air', block: 0 },
        { ...stone, id: 1, state: 'dwell:iron', block: 1 },
      ],
    };
    const [play] = soundsFor(event(data, 'step', 1), data, registry);
    expect(play?.sound).toBe('dwell:metal_clang');
    const def = data.sounds['dwell:metal_clang'];
    if (!def?.samples) throw new Error('no samples');
    const bank = new SampleBank<{ duration: number }>(mulberry32(1));
    // Nothing decoded yet: the procedural layers play.
    expect(bank.pick('dwell:metal_clang', def)).toBeNull();
    const [one, two] = def.samples.files;
    if (!one || !two) throw new Error('two files');
    bank.set(one, { duration: 0.2 });
    expect(bank.pick('dwell:metal_clang', def)?.file).toBe(one);
    bank.set(two, { duration: 0.3 });
    // Never the same variation twice running.
    let prev = bank.pick('dwell:metal_clang', def)?.file;
    for (let i = 0; i < 20; i++) {
      const next = bank.pick('dwell:metal_clang', def)?.file;
      expect(next).not.toBe(prev);
      prev = next;
    }
    // `play: "procedural"` keeps the layers even with the samples loaded.
    expect(bank.pick('dwell:metal_clang', { ...def, play: 'procedural' })).toBeNull();
  });

  it("the shipped blocks' materials pick their own footsteps", () => {
    const grass = BLOCK_DEFS.find((b) => b.id === 'dwell:grass');
    const stone = BLOCK_DEFS.find((b) => b.id === 'dwell:stone');
    if (!grass || !stone) throw new Error('no grass or stone');
    const data = SOUND_DATA;
    expect(soundsFor(event(data, 'step', grass.first), data)[0]?.sound).toBe('dwell:grass_step');
    expect(soundsFor(event(data, 'step', stone.first), data)[0]?.sound).toBe('dwell:stone_step');
    const files = sound('dwell:grass_step').samples?.files ?? [];
    expect(files.every((f) => f.startsWith('grass-step-'))).toBe(true);
  });

  // Owner, 2026-11-12: the "no" sound for clicking when nothing is there was "way to loud and
  // jarring". The old one (UI_Error_Tone_01, peak -0.5 dBFS, played at gain 0.4) peaked near
  // -8.5 dBFS; the new soft knock (peak -6.1 dBFS) plays at most ~12 dB below that.
  it('keeps the deny sound soft: a quiet knock, ~12 dB under the old error tone', () => {
    const deny = sound('dwell:deny');
    expect(usesSamples(deny)).toBe(true);
    expect(deny.samples?.files).toHaveLength(1);
    expect(deny.samples?.files[0]).toMatch(/^deny-knock-1-[0-9a-f]{8}\.m4a$/);
    const filePeakDb = -6.1; // the file's measured peak (ffmpeg volumedetect)
    const oldPeakDb = -0.5 + 20 * Math.log10(0.4);
    const peakDb = filePeakDb + 20 * Math.log10(sampleGain(deny, {}));
    expect(peakDb).toBeLessThanOrEqual(oldPeakDb - 11);
    expect(peakDb).toBeGreaterThan(oldPeakDb - 18); // still heard
  });

  it('scales a sample by its gain, the play and the intensity range', () => {
    const s: SoundDef = { ...sound('dwell:stone_land'), samples: { files: ['x.m4a'], gain: 0.5 } };
    const range = s.intensity?.gain ?? [1, 1];
    expect(sampleGain(s, { intensity: 0, gain: 2 })).toBeCloseTo(0.5 * 2 * range[0]);
    expect(sampleGain(s, { intensity: 1 })).toBeCloseTo(0.5 * range[1]);
  });
});

// Owner, 2026-11-10: the recorded effects sounded "like they're in an echoey room": the recordings
// carry their own room, and the sound's `reverb` (made for its procedural layers) put them in a
// second one. A recording plays dry unless its samples ask for the room.
describe('recordings play dry', () => {
  const sound = {
    reverb: 0.12,
    samples: { files: ['a.m4a'] },
  } as unknown as Parameters<typeof roomSend>[0];
  it('sends a recorded play nothing to the room by default', () => {
    expect(roomSend(sound, true)).toBe(0);
  });
  it('keeps the room for the procedural layers', () => {
    expect(roomSend(sound, false)).toBe(0.12);
  });
  it('lets a sample set ask for some room by data', () => {
    const wet = { ...sound, samples: { files: ['a.m4a'], reverb: 0.05 } } as typeof sound;
    expect(roomSend(wet, true)).toBe(0.05);
  });
});
