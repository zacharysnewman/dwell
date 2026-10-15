// The mixer (docs/AUDIO.md §3): the effects and UI buses into the master gain, a small room
// reverb on the effects, and a soft limiter before the output: a waveshaper, linear to 0.7, then
// a tanh knee that never passes full scale (a DynamicsCompressorNode adds its own make-up gain).
// Built the same on an AudioContext (the game, the menus) and an OfflineAudioContext (renders and
// their checks).
import { mulberry32 } from './variation';

export interface Volumes {
  master: number;
  effects: number;
  ui: number;
}

export interface Mixer {
  effects: GainNode;
  ui: GainNode;
  /** Into the room: a sound's `reverb` share goes here. */
  reverb: GainNode;
  master: GainNode;
  setVolumes(v: Volumes): void;
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

export const DEFAULT_VOLUMES: Volumes = { master: 0.8, effects: 1, ui: 0.6 };

/** Builds the mixer into `ctx.destination`. */
export function buildMixer(ctx: BaseAudioContext, volumes: Volumes = DEFAULT_VOLUMES): Mixer {
  const master = ctx.createGain();
  const into = ctx.createGain();
  into.gain.value = 1 / LIMIT_RANGE;
  const limiter = ctx.createWaveShaper();
  limiter.curve = softClipCurve();
  // No oversampling: its filters could overshoot full scale; the knee is rarely reached.
  limiter.oversample = 'none';
  master.connect(into);
  into.connect(limiter);
  limiter.connect(ctx.destination);
  const effects = ctx.createGain();
  const ui = ctx.createGain();
  effects.connect(master);
  ui.connect(master);
  const reverb = ctx.createGain();
  const room = ctx.createConvolver();
  room.normalize = false;
  room.buffer = roomImpulse(ctx);
  const wet = ctx.createGain();
  wet.gain.value = 0.9;
  reverb.connect(room);
  room.connect(wet);
  wet.connect(effects);
  const setVolumes = (v: Volumes): void => {
    master.gain.value = v.master;
    effects.gain.value = v.effects;
    ui.gain.value = v.ui;
  };
  setVolumes(volumes);
  return { effects, ui, reverb, master, setVolumes };
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
