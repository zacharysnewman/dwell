import { describe, expect, it } from 'vitest';
import { readSoundFiles } from '../../../shared/sounds/load.mjs';
import {
  AMBIENCE_INPUTS,
  resolveSounds,
  SAMPLES_MAX_BYTES,
} from '../../../shared/sounds/validate.mjs';
import {
  AmbiencePlayer,
  bedGains,
  decodeSurroundings,
  INPUTS,
  SILENT,
  type BedOutput,
  type Surroundings,
} from './ambience';
import { DEFAULT_VOLUMES } from './mixer';
import { sanitizeVolumes } from './settings';
import { SOUND_DATA } from './sounds.gen';

const A = SOUND_DATA.ambience;

/** Open grassland at sea level by day, no water, no trees, no fire. */
const base: Surroundings = {
  altitude: 4,
  height: 1.6,
  open: 1,
  waterShare: 0,
  waterM: 99,
  flow: 0,
  trees: 0,
  day: 1,
  fireM: 99,
};
const at = (s: Partial<Surroundings>): Record<string, number> => bedGains(A, { ...base, ...s });

describe('ambience beds (AUDIO.md §2c)', () => {
  it('reads the probe in the Rust order', () => {
    expect([...INPUTS]).toEqual(AMBIENCE_INPUTS);
    expect(decodeSurroundings([])).toBeNull();
    const s = decodeSurroundings([1, 2, 3, 4, 5, 6, 7, 8, 9]);
    expect(s?.altitude).toBe(1);
    expect(s?.fireM).toBe(9);
  });

  it('blows harder up high and in the open', () => {
    const low = at({ altitude: 2 })['dwell:wind'] ?? 0;
    const high = at({ altitude: 80 })['dwell:wind'] ?? 0;
    const sheltered = at({ altitude: 80, open: 0.3 })['dwell:wind'] ?? 0;
    expect(high).toBeGreaterThan(low * 2);
    expect(sheltered).toBeLessThan(high);
  });

  it('hears the sea by the coast, a river by running water, a stream by a narrow one', () => {
    const coast = at({ waterShare: 0.5, waterM: 3 });
    expect(coast['dwell:sea']).toBeGreaterThan(0.1);
    expect(at({})['dwell:sea']).toBe(0);
    expect(at({ waterShare: 0.5, waterM: 60 })['dwell:sea']).toBe(0);
    const river = at({ waterShare: 0.1, waterM: 3, flow: 2 });
    expect(river['dwell:river']).toBeGreaterThan(0.1);
    expect(at({ waterShare: 0.1, waterM: 3, flow: 0 })['dwell:river']).toBe(0);
    const stream = at({ waterShare: 0.02, waterM: 2 });
    expect(stream['dwell:stream']).toBeGreaterThan(0.1);
    expect(stream['dwell:sea']).toBe(0);
  });

  it('sings in daytime forests and quietens at night', () => {
    const day = at({ trees: 0.4, day: 1 })['dwell:birds'] ?? 0;
    const night = at({ trees: 0.4, day: 0 })['dwell:birds'] ?? 0;
    expect(day).toBeGreaterThan(night * 3);
    expect(at({ trees: 0 })['dwell:birds']).toBe(0);
    expect(at({ trees: 0.4 })['dwell:leaves']).toBeGreaterThan(0);
  });

  it('crackles quietly by a fire, louder nearer, and not at all far from one', () => {
    const far = at({ fireM: 99 })['dwell:fire'] ?? 0;
    const near = at({ fireM: 4 })['dwell:fire'] ?? 0;
    const beside = at({ fireM: 0.5 })['dwell:fire'] ?? 0;
    expect(far).toBe(0);
    expect(near).toBeGreaterThan(0);
    expect(beside).toBeGreaterThan(near);
    // Flavour, not function: under the cap and quieter than the wind's full level.
    expect(beside).toBeLessThanOrEqual(A.cap);
    expect(beside).toBeLessThanOrEqual(0.25);
  });

  it('keeps every bed under the quiet cap, and the Ambience volume low by default', () => {
    const extremes: Partial<Surroundings>[] = [
      { altitude: 500, open: 1, waterShare: 1, waterM: 0, flow: 9, trees: 1, day: 1, fireM: 0 },
      { altitude: -50, open: 0, waterShare: 0, waterM: 99, flow: 0, trees: 0, day: 0 },
    ];
    for (const e of extremes)
      for (const g of Object.values(at(e))) {
        expect(g).toBeGreaterThanOrEqual(0);
        expect(g).toBeLessThanOrEqual(A.cap);
      }
    expect(A.cap).toBeLessThanOrEqual(0.5);
    expect(DEFAULT_VOLUMES.ambience).toBeLessThanOrEqual(0.5);
    expect(sanitizeVolumes({}).ambience).toBe(A.volume);
  });
});

/** A recording bed output. */
function recorder(canStart = true): BedOutput & { log: string[]; levels: Map<string, number> } {
  const log: string[] = [];
  const levels = new Map<string, number>();
  return {
    log,
    levels,
    start: (id) => {
      log.push(`start ${id}`);
      return canStart;
    },
    level: (id, g) => {
      levels.set(id, g);
    },
    stop: (id) => {
      log.push(`stop ${id}`);
      levels.delete(id);
    },
  };
}

describe('the ambience player', () => {
  it('starts a bed when it is heard, glides it, and stops it when it is not', () => {
    const out = recorder();
    const p = new AmbiencePlayer(A, out);
    p.update({ ...base, altitude: 80 });
    expect(p.beds).toEqual(['dwell:wind']);
    const loud = out.levels.get('dwell:wind') ?? 0;
    p.update({ ...base, altitude: 2 });
    expect(out.log).toEqual(['start dwell:wind']);
    expect(out.levels.get('dwell:wind')).toBeLessThan(loud);
    // Into a forest by the coast: the wind fades as the others come in (a crossfade).
    p.update({ ...base, altitude: 2, open: 0.1, trees: 0.4, waterShare: 0.5, waterM: 3 });
    expect(out.log).toContain('stop dwell:wind');
    expect(p.beds.sort()).toEqual(['dwell:birds', 'dwell:leaves', 'dwell:sea']);
    // Nothing to read (left the game): all of them fade.
    p.update(null);
    expect(p.beds).toEqual([]);
  });

  it('tries again while a bed cannot start yet (no context, not decoded)', () => {
    const out = recorder(false);
    const p = new AmbiencePlayer(A, out);
    p.update({ ...base, altitude: 80 });
    p.update({ ...base, altitude: 80 });
    expect(out.log).toEqual(['start dwell:wind', 'start dwell:wind']);
    expect(p.beds).toEqual([]);
    expect(SILENT).toBeLessThan(0.01);
  });
});

type File = Record<string, unknown> & { ambience: Record<string, unknown> };
const files = (): File[] => structuredClone(readSoundFiles()) as File[];
const dwell = (fs: File[]): File & { ambience: { beds: Record<string, unknown> } } => {
  const f = fs.find((x) => x.namespace === 'dwell');
  if (!f) throw new Error('no dwell namespace');
  return f as File & { ambience: { beds: Record<string, unknown> } };
};

describe('ambience data', () => {
  it('takes a new bed by data alone', () => {
    const fs = files();
    dwell(fs).ambience.beds.campfire = {
      files: ['wind-loop-1.m4a'],
      gain: 0.2,
      when: [{ input: 'fireM', from: 12, full: 2 }],
    };
    const data = resolveSounds(fs);
    const bed = data.ambience.beds['dwell:campfire'];
    expect(bed?.files[0]).toMatch(/^wind-loop-1-[0-9a-f]{8}\.m4a$/);
    const near = bedGains(data.ambience, { ...base, fireM: 1 })['dwell:campfire'];
    const far = bedGains(data.ambience, { ...base, fireM: 30 })['dwell:campfire'];
    expect(near).toBeCloseTo(0.2);
    expect(far).toBe(0);
  });

  it.each([
    [{ files: [], gain: 0.1, when: [] }, 'files'],
    [{ files: ['nope.m4a'], gain: 0.1, when: [] }, 'no file "nope.m4a"'],
    [{ files: ['wind-loop-1.m4a'], gain: 0.9, when: [] }, 'over the cap'],
    [
      { files: ['wind-loop-1.m4a'], gain: 0.1, when: [{ input: 'rain', from: 0, full: 1 }] },
      'unknown input "rain"',
    ],
    [
      { files: ['wind-loop-1.m4a'], gain: 0.1, when: [{ input: 'day', from: 1, full: 1 }] },
      'must differ',
    ],
  ])('rejects a bad bed (%j) naming it', (bed, message) => {
    const fs = files();
    dwell(fs).ambience.beds.bad = bed;
    expect(() => resolveSounds(fs)).toThrow(message);
    expect(() => resolveSounds(fs)).toThrow('dwell:bad');
  });

  it('ships the beds inside the samples budget', () => {
    const shipped = new Set(SOUND_DATA.sampleFiles.map((f) => f.file));
    for (const b of Object.values(A.beds)) for (const f of b.files) expect(shipped).toContain(f);
    expect(SOUND_DATA.sampleBytes).toBeLessThanOrEqual(SAMPLES_MAX_BYTES);
  });
});
