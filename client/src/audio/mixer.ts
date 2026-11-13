// The mixer (docs/AUDIO.md §3): the effects, UI and music buses into the master gain, a small room
// reverb on the effects, and a soft limiter before the output: a waveshaper, linear to 0.7, then
// a tanh knee that never passes full scale (a DynamicsCompressorNode adds its own make-up gain).
// The buses the data's `underwater` lists reach the master through a low-pass and a level that
// close and drop while the eye is under water (§3a, BUGS.md #104); the others go straight in.
// Built the same on an AudioContext (the game, the menus) and an OfflineAudioContext (renders and
// their checks).
import { SOUND_DATA } from './sounds.gen';
import { mulberry32 } from './variation';
import type { Bus, UnderwaterDef } from './types';

export interface Volumes {
  master: number;
  effects: number;
  ui: number;
  /** The music (AUDIO.md §6a); 0 mutes it. */
  music: number;
  /** The ambience beds (AUDIO.md §2c); quiet by default. */
  ambience: number;
}

export interface Mixer {
  effects: GainNode;
  ui: GainNode;
  /** The music bus (dry: no room). */
  music: GainNode;
  /** The ambience beds' bus (dry). */
  ambience: GainNode;
  /** Into the room: a sound's `reverb` share goes here. */
  reverb: GainNode;
  master: GainNode;
  setVolumes(v: Volumes): void;
  /** Muffles the data's buses (the eye under water) or opens them, gliding from `now`. */
  setUnderwater(under: boolean, now: number): void;
  /**
   * A parallel tap of the mix into `dest` (a recording, docs/THEATER_MODE.md §6): only `buses`, through
   * the tap's own master level, underwater muffle and limiter, so the speakers are unchanged. `close()`
   * disconnects it.
   */
  tap(dest: AudioNode, buses: readonly Bus[]): MixTap;
}

/** A tap of the mix (`Mixer.tap`). */
export interface MixTap {
  readonly buses: readonly Bus[];
  close(): void;
}

/** Which buses a tap carries (`__dwell.audioTap`): true includes a bus. */
export interface TapOptions {
  effects?: boolean;
  ambience?: boolean;
  /** Off unless asked: showcases never carry the music (owner, WORK_QUEUE item 40). */
  music?: boolean;
  /** Off unless asked: menu clicks don't belong in a recording. */
  ui?: boolean;
}

/** The buses a tap with `o` carries: effects and ambience unless turned off, music and UI only if asked. */
export function tapBuses(o: TapOptions = {}): Bus[] {
  const on: Record<Bus, boolean> = {
    effects: o.effects ?? true,
    ambience: o.ambience ?? true,
    music: o.music ?? false,
    ui: o.ui ?? false,
  };
  return (['effects', 'ambience', 'music', 'ui'] as const).filter((b) => on[b]);
}

/** What the underwater muffle aims at: the filter's cutoff and the muffled buses' level. */
export interface UnderwaterTarget {
  cutoffHz: number;
  gain: number;
}

/** The muffle's target with the eye under water or in air (§3a). */
export function underwaterTarget(def: UnderwaterDef, under: boolean): UnderwaterTarget {
  return under ? { cutoffHz: def.lowpassHz, gain: def.gain } : { cutoffHz: def.openHz, gain: 1 };
}

/** Whether a bus goes through the underwater muffle (the data's `buses`; never the UI or music
 * unless listed). */
export function muffles(def: UnderwaterDef, bus: Bus): boolean {
  return def.buses.includes(bus);
}

/** The part of an AudioParam a glide uses (tests fake it). */
export interface Glidable {
  value: number;
  cancelScheduledValues(t: number): unknown;
  setValueAtTime(v: number, t: number): unknown;
  linearRampToValueAtTime(v: number, t: number): unknown;
  exponentialRampToValueAtTime(v: number, t: number): unknown;
}

/**
 * Glides `param` from where it is to `to` over `seconds` from `now`: exponential for a frequency
 * (even steps in pitch), linear for a level.
 */
export function glide(
  param: Glidable,
  to: number,
  now: number,
  seconds: number,
  exponential: boolean,
): void {
  const from = param.value;
  param.cancelScheduledValues(now);
  param.setValueAtTime(from, now);
  if (exponential && from > 0 && to > 0) param.exponentialRampToValueAtTime(to, now + seconds);
  else param.linearRampToValueAtTime(to, now + seconds);
}

/**
 * The underwater muffle on its buses: one low-pass and one level, both gliding over the data's
 * `easeS` when the eye crosses the surface (only then: a frame that changes nothing schedules
 * nothing).
 */
export class Muffle {
  private under = false;

  constructor(
    private readonly def: UnderwaterDef,
    private readonly cutoff: Glidable,
    private readonly level: Glidable,
  ) {
    const t = underwaterTarget(def, false);
    cutoff.value = t.cutoffHz;
    level.value = t.gain;
  }

  get underwater(): boolean {
    return this.under;
  }

  set(under: boolean, now: number): void {
    if (under === this.under) return;
    this.under = under;
    const t = underwaterTarget(this.def, under);
    glide(this.cutoff, t.cutoffHz, now, this.def.easeS, true);
    glide(this.level, t.gain, now, this.def.easeS, false);
  }
}

/** Seconds of the room's tail. */
const ROOM_SECONDS = 0.55;

/** A small room: early reflections and a decaying stereo tail that darkens as it fades. */
export function roomImpulse(ctx: BaseAudioContext, seed = 3): AudioBuffer {
  const rate = ctx.sampleRate;
  const n = Math.floor(rate * ROOM_SECONDS);
  const b = ctx.createBuffer(2, n, rate);
  for (let ch = 0; ch < 2; ch++) {
    const rng = mulberry32(seed + ch * 101);
    const data = new Float32Array(n);
    let lp = 0;
    for (let i = 0; i < n; i++) {
      const t = i / rate;
      // Darker with time: a one-pole lowpass whose cutoff falls.
      const a = 0.25 + 0.7 * Math.min(1, t / ROOM_SECONDS);
      lp = lp * a + (rng() * 2 - 1) * (1 - a);
      data[i] = lp * Math.exp(-t / 0.11) * (t < 0.004 ? t / 0.004 : 1);
    }
    // Early reflections.
    for (const [ms, g] of [
      [7, 0.5],
      [13, 0.35],
      [19, 0.28],
      [29, 0.2],
    ] as const) {
      const at = Math.floor(((ms + ch * 1.7) / 1000) * rate);
      if (at < n) data[at] = (data[at] ?? 0) + g * (ch ? -1 : 1);
    }
    // Normalised to unit energy, so the send level means the same whatever the length.
    let energy = 0;
    for (const v of data) energy += v * v;
    const k = 1 / Math.sqrt(energy || 1);
    for (let i = 0; i < n; i++) data[i] = (data[i] ?? 0) * k;
    b.copyToChannel(data, ch);
  }
  return b;
}

/** Linear up to KNEE, then bending smoothly toward (never past) full scale. */
export const KNEE = 0.7;
export function softClip(x: number): number {
  const a = Math.abs(x);
  if (a <= KNEE) return x;
  return Math.sign(x) * (KNEE + (1 - KNEE) * Math.tanh((a - KNEE) / (1 - KNEE)));
}

/** Input range the limiter shapes: a WaveShaperNode's curve spans [−1, 1], so its input is
 * scaled by 1 / LIMIT_RANGE first and the curve undoes it. */
const LIMIT_RANGE = 2;

/** The soft limiter's transfer curve (inputs past ±LIMIT_RANGE hold at the ends). */
function softClipCurve(): Float32Array<ArrayBuffer> {
  const n = 4097;
  const curve = new Float32Array(n);
  for (let i = 0; i < n; i++) curve[i] = softClip(((i / (n - 1)) * 2 - 1) * LIMIT_RANGE) * 0.98;
  return curve;
}

export const DEFAULT_VOLUMES: Volumes = {
  master: 0.8,
  effects: 1,
  ui: 0.6,
  music: 0.6,
  ambience: SOUND_DATA.ambience.volume,
};

/** Builds the mixer into `ctx.destination`. */
export function buildMixer(
  ctx: BaseAudioContext,
  volumes: Volumes = DEFAULT_VOLUMES,
  underwater: UnderwaterDef = SOUND_DATA.underwater,
): Mixer {
  // The master level, the limiter and the underwater muffle into `out`: the speakers' chain, and
  // each tap's own copy of it.
  const chain = (
    out: AudioNode,
  ): { master: GainNode; lowpass: BiquadFilterNode; muffle: Muffle } => {
    const master = ctx.createGain();
    const into = ctx.createGain();
    into.gain.value = 1 / LIMIT_RANGE;
    const limiter = ctx.createWaveShaper();
    limiter.curve = softClipCurve();
    // No oversampling: its filters could overshoot full scale; the knee is rarely reached.
    limiter.oversample = 'none';
    master.connect(into);
    into.connect(limiter);
    limiter.connect(out);
    // The underwater muffle: one low-pass and one level shared by the buses it lists.
    const lowpass = ctx.createBiquadFilter();
    lowpass.type = 'lowpass';
    lowpass.Q.value = underwater.q;
    const muffled = ctx.createGain();
    lowpass.connect(muffled);
    muffled.connect(master);
    return { master, lowpass, muffle: new Muffle(underwater, lowpass.frequency, muffled.gain) };
  };
  const { master, lowpass, muffle } = chain(ctx.destination);
  const taps = new Set<{ master: GainNode; muffle: Muffle }>();
  const bus = (name: Bus): GainNode => {
    const g = ctx.createGain();
    g.connect(muffles(underwater, name) ? lowpass : master);
    return g;
  };
  const effects = bus('effects');
  const ui = bus('ui');
  const music = bus('music');
  const ambience = bus('ambience');
  const reverb = ctx.createGain();
  const room = ctx.createConvolver();
  room.normalize = false;
  room.buffer = roomImpulse(ctx);
  const wet = ctx.createGain();
  wet.gain.value = 0.9;
  reverb.connect(room);
  room.connect(wet);
  wet.connect(effects);
  const buses: Record<Bus, GainNode> = { effects, ui, music, ambience };
  const setVolumes = (v: Volumes): void => {
    master.gain.value = v.master;
    for (const t of taps) t.master.gain.value = v.master;
    effects.gain.value = v.effects;
    ui.gain.value = v.ui;
    music.gain.value = v.music;
    ambience.gain.value = v.ambience;
  };
  setVolumes(volumes);
  const setUnderwater = (under: boolean, now: number): void => {
    muffle.set(under, now);
    for (const t of taps) t.muffle.set(under, now);
  };
  const tap = (dest: AudioNode, want: readonly Bus[]): MixTap => {
    const t = chain(dest);
    t.master.gain.value = master.gain.value;
    if (muffle.underwater) t.muffle.set(true, 0);
    taps.add(t);
    const links = want.map((b) => {
      const to = muffles(underwater, b) ? t.lowpass : t.master;
      buses[b].connect(to);
      return [buses[b], to] as const;
    });
    return {
      buses: [...want],
      close: () => {
        if (!taps.delete(t)) return;
        for (const [from, to] of links) from.disconnect(to);
        t.master.disconnect();
      },
    };
  };
  return { effects, ui, music, ambience, reverb, master, setVolumes, setUnderwater, tap };
}

/** The node a play goes into: the bus, with a send to the room for its `reverb` share. */
export function soundInput(
  ctx: BaseAudioContext,
  mixer: Mixer,
  bus: 'effects' | 'ui',
  reverb: number,
  after?: AudioNode,
): GainNode {
  const input = ctx.createGain();
  const dest = after ?? mixer[bus];
  if (after) after.connect(mixer[bus]);
  input.connect(dest);
  if (reverb > 0) {
    const send = ctx.createGain();
    send.gain.value = reverb;
    input.connect(send);
    send.connect(mixer.reverb);
  }
  return input;
}
