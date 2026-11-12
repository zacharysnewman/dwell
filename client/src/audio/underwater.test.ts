// The underwater muffle (docs/AUDIO.md §3a, BUGS.md #104) and the swim strokes' levels: the
// filter's targets by the eye's state, the glide at the crossing, the UI and music buses left dry,
// and the strokes' sounds and gains from the data.
import { describe, expect, it } from 'vitest';
import { resolveSounds } from '../../../shared/sounds/validate.mjs';
import { readSoundFiles } from '../../../shared/sounds/load.mjs';
import { BLOCK_DEFS, STATE_DEFS } from '../world/blocks.gen';
import { sampleGain } from './engine';
import { soundsFor } from './events';
import { buildMixer, glide, Muffle, muffles, underwaterTarget, type Glidable } from './mixer';
import { SampleBank } from './samples';
import { SOUND_DATA } from './sounds.gen';
import type { UnderwaterDef } from './types';

const DEF = SOUND_DATA.underwater;

/** An AudioParam that records its automation and evaluates it at a time. */
class FakeParam implements Glidable {
  value = 0;
  readonly events: { kind: string; v: number; t: number }[] = [];
  cancelScheduledValues(t: number): void {
    this.events.push({ kind: 'cancel', v: 0, t });
  }
  setValueAtTime(v: number, t: number): void {
    this.events.push({ kind: 'set', v, t });
  }
  linearRampToValueAtTime(v: number, t: number): void {
    this.events.push({ kind: 'linear', v, t });
  }
  exponentialRampToValueAtTime(v: number, t: number): void {
    this.events.push({ kind: 'exp', v, t });
  }
  /** The value at `t` from the last set and ramp (the Web Audio rules for one segment). */
  at(t: number): number {
    const set = [...this.events].reverse().find((e) => e.kind === 'set');
    const ramp = [...this.events].reverse().find((e) => e.kind === 'linear' || e.kind === 'exp');
    if (!set || !ramp || t <= set.t) return set?.v ?? this.value;
    if (t >= ramp.t) return ramp.v;
    const k = (t - set.t) / (ramp.t - set.t);
    return ramp.kind === 'exp' ? set.v * (ramp.v / set.v) ** k : set.v + (ramp.v - set.v) * k;
  }
}

describe('the underwater muffle', () => {
  it('muffles the world under water and opens in air, as the data says', () => {
    expect(DEF.lowpassHz).toBeGreaterThanOrEqual(500);
    expect(DEF.lowpassHz).toBeLessThanOrEqual(1000);
    expect(DEF.easeS).toBeCloseTo(0.25);
    expect(underwaterTarget(DEF, true)).toEqual({ cutoffHz: DEF.lowpassHz, gain: DEF.gain });
    expect(underwaterTarget(DEF, false)).toEqual({ cutoffHz: DEF.openHz, gain: 1 });
    expect(DEF.gain).toBeLessThan(1);
    // A fixture by data alone: another cutoff and level are what it aims at.
    const other: UnderwaterDef = { ...DEF, lowpassHz: 500, gain: 0.3 };
    expect(underwaterTarget(other, true)).toEqual({ cutoffHz: 500, gain: 0.3 });
  });

  it('glides over easeS at the crossing, and schedules nothing on a frame that changes nothing', () => {
    const cutoff = new FakeParam();
    const level = new FakeParam();
    const m = new Muffle(DEF, cutoff, level);
    expect(cutoff.value).toBe(DEF.openHz);
    expect(level.value).toBe(1);
    m.set(false, 1);
    expect(cutoff.events).toEqual([]);
    m.set(true, 2);
    expect(m.underwater).toBe(true);
    // Under at 2 s: halfway through the glide it is between, at its end the target.
    expect(cutoff.at(2)).toBeCloseTo(DEF.openHz);
    const mid = cutoff.at(2 + DEF.easeS / 2);
    expect(mid).toBeLessThan(DEF.openHz);
    expect(mid).toBeGreaterThan(DEF.lowpassHz);
    // Exponential: halfway in time is the geometric mean (halfway in pitch).
    expect(mid).toBeCloseTo(Math.sqrt(DEF.openHz * DEF.lowpassHz), 3);
    expect(cutoff.at(2 + DEF.easeS)).toBeCloseTo(DEF.lowpassHz);
    expect(level.at(2 + DEF.easeS / 2)).toBeCloseTo((1 + DEF.gain) / 2);
    expect(level.at(2 + DEF.easeS)).toBeCloseTo(DEF.gain);
    const n = cutoff.events.length;
    m.set(true, 2.1);
    expect(cutoff.events.length).toBe(n);
    // Out again: back to open over the same glide.
    level.value = DEF.gain;
    cutoff.value = DEF.lowpassHz;
    m.set(false, 3);
    expect(cutoff.at(3 + DEF.easeS)).toBeCloseTo(DEF.openHz);
    expect(level.at(3 + DEF.easeS)).toBeCloseTo(1);
  });

  it('a glide starts from where the param is (a crossing back mid-glide does not jump)', () => {
    const p = new FakeParam();
    p.value = 1200;
    glide(p, DEF.openHz, 5, 0.25, true);
    expect(p.events[0]).toEqual({ kind: 'cancel', v: 0, t: 5 });
    expect(p.events[1]).toEqual({ kind: 'set', v: 1200, t: 5 });
    expect(p.events[2]).toEqual({ kind: 'exp', v: DEF.openHz, t: 5.25 });
  });

  it('filters the effects and ambience buses, never the UI or the music', () => {
    expect(muffles(DEF, 'effects')).toBe(true);
    expect(muffles(DEF, 'ambience')).toBe(true);
    expect(muffles(DEF, 'ui')).toBe(false);
    expect(muffles(DEF, 'music')).toBe(false);
    // The built graph: what each bus connects to.
    const ctx = new FakeContext();
    const mixer = buildMixer(ctx as unknown as BaseAudioContext);
    const lowpass = ctx.nodes.find((n) => n.kind === 'biquad');
    expect(lowpass?.type).toBe('lowpass');
    const into = (n: unknown): FakeNode[] => (n as FakeNode).outs;
    expect(into(mixer.effects)).toEqual([lowpass]);
    expect(into(mixer.ambience)).toEqual([lowpass]);
    expect(into(mixer.ui)).toEqual([mixer.master]);
    expect(into(mixer.music)).toEqual([mixer.master]);
    // Under water the UI's and the music's path is untouched: only the muffle's params move.
    mixer.setUnderwater(true, 0);
    expect(lowpass?.frequency.events.at(-1)).toMatchObject({ v: DEF.lowpassHz, t: DEF.easeS });
    expect(into(mixer.ui)).toEqual([mixer.master]);
    expect(into(mixer.music)).toEqual([mixer.master]);
  });

  it('rejects a bad underwater entry with a message', () => {
    const bad = (u: Record<string, unknown>): (() => unknown) => {
      const fs = structuredClone(readSoundFiles()) as Record<string, unknown>[];
      const f = fs.find((x) => x.namespace === 'dwell');
      if (!f) throw new Error('no dwell');
      f.underwater = { ...(f.underwater as object), ...u };
      return () => resolveSounds(fs);
    };
    expect(bad({ buses: ['effects', 'radio'] })).toThrow(/underwater.buses/);
    expect(bad({ lowpassHz: 30 })).toThrow(/underwater.lowpassHz/);
    expect(bad({ openHz: 600, lowpassHz: 800 })).toThrow(/underwater.openHz/);
  });
});

describe('the swim strokes', () => {
  const water = SOUND_DATA.sets['dwell:water'];
  const swim = SOUND_DATA.sounds[water?.swim ?? ''];
  const under = SOUND_DATA.sounds[water?.swim_under ?? ''];

  it('water strokes have a surface sound and a deep one, and other sets fall back to none', () => {
    expect(water?.swim).toBe('dwell:water_swim');
    expect(water?.swim_under).toBe('dwell:water_swim_under');
    const kind = SOUND_DATA.events.findIndex((e) => e.name === 'swim_under');
    expect(kind).toBeGreaterThanOrEqual(0);
    // A set without its own deep stroke plays nothing rather than the fallback's.
    expect(SOUND_DATA.events[kind]?.fallback).toBe(false);
  });

  it('play at a bounded, calm level: the surface stroke soft and filtered, the deep one darker', () => {
    // Owner, 2026-11-12: "too loud and high ... like a kid learning to swim splashing". Before:
    // gait gain 0.7 × samples gain 0.6 = 0.42, unfiltered.
    const gait = SOUND_DATA.gaits.swim?.gain ?? 1;
    for (const s of [swim, under]) {
      expect(s).toBeDefined();
      if (!s) continue;
      expect(gait * sampleGain(s, { intensity: 1 })).toBeLessThanOrEqual(0.2);
      expect(s.samples?.lowpassHz).toBeLessThanOrEqual(3000);
      expect(s.samples?.shift).toBeLessThan(0);
      expect(s.samples?.pitch).toBeGreaterThan(0);
    }
    expect(under?.samples?.lowpassHz ?? 1e9).toBeLessThan(swim?.samples?.lowpassHz ?? 0);
    expect(under?.samples?.shift ?? 0).toBeLessThan(swim?.samples?.shift ?? 0);
    // The stroke's stride: one stroke a second at the controller's swim speed (3 m/s).
    expect(SOUND_DATA.gaits.swim?.stride).toBeGreaterThanOrEqual(3);
  });

  it('the event plays the set sound at the swim gait gain, each stroke one play', () => {
    const kind = SOUND_DATA.events.findIndex((e) => e.name === 'swim_under');
    const gait = SOUND_DATA.gaitOrder.indexOf('swim');
    const plays = soundsFor(
      { kind, block: waterState(), intensity: 1, at: null, gait, voice: null },
      SOUND_DATA,
    );
    expect(plays).toEqual([{ sound: 'dwell:water_swim_under', gain: SOUND_DATA.gaits.swim?.gain }]);
  });

  it('a recording plays shifted and through its low-pass', () => {
    const bank = new SampleBank<{ duration: number }>(() => 0.5);
    for (const f of under?.samples?.files ?? []) bank.set(f, { duration: 1 });
    const p = under ? bank.pick('dwell:water_swim_under', under) : null;
    // rng 0.5: no random pitch; the shift alone.
    expect(p?.rate).toBeCloseTo(2 ** ((under?.samples?.shift ?? 0) / 12));
    expect(p?.lowpassHz).toBe(under?.samples?.lowpassHz);
  });
});

/** The state id of `dwell:water` (its block's sound set is `water`). */
function waterState(): number {
  return STATE_DEFS.findIndex((s) => BLOCK_DEFS[s.block]?.sound === 'dwell:water');
}

interface FakeNode {
  kind: string;
  type?: string;
  outs: FakeNode[];
  gain: FakeParam;
  frequency: FakeParam;
  Q: FakeParam;
  connect(n: FakeNode): void;
}

/** Just enough of a BaseAudioContext for buildMixer: nodes that record their connections. */
class FakeContext {
  readonly sampleRate = 8000;
  readonly nodes: FakeNode[] = [];
  readonly destination = this.node('destination');
  node(kind: string): FakeNode {
    const n: FakeNode = {
      kind,
      outs: [],
      gain: new FakeParam(),
      frequency: new FakeParam(),
      Q: new FakeParam(),
      connect(to: FakeNode) {
        this.outs.push(to);
      },
    };
    this.nodes.push(n);
    return n;
  }
  createGain(): FakeNode {
    return this.node('gain');
  }
  createBiquadFilter(): FakeNode {
    return this.node('biquad');
  }
  createWaveShaper(): FakeNode {
    return this.node('shaper');
  }
  createConvolver(): FakeNode {
    return this.node('convolver');
  }
  createBuffer(channels: number, length: number) {
    return { numberOfChannels: channels, length, copyToChannel: () => undefined };
  }
}
