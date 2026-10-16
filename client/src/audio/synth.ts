// The synthesis vocabulary (docs/AUDIO.md §3) in plain Web Audio: a sound's layers — noise,
// oscillator or modal partials, through biquad filters, under an envelope, repeated as grains —
// built as short-lived nodes for one play. Works on any BaseAudioContext, so the offline renders
// (OfflineAudioContext) use the same code as the game. A library could replace this file behind the
// same data.
import { hashSeed, intensityFactor, layerAt, mulberry32, type PlayParams } from './variation';
import type { EnvDef, FilterDef, LayerDef, NoiseColor, SoundDef } from './types';

/** Seconds of each noise buffer. */
const NOISE_SECONDS = 2;
/** Envelopes decay to this fraction of their peak (−60 dB), then stop. */
const FLOOR = 0.001;

export type NoiseBank = Record<NoiseColor, AudioBuffer>;

/** Seeded white, pink (Paul Kellet's filter) and brown (leaky integrated) noise for a context. */
export function makeNoise(ctx: BaseAudioContext, seed = 7): NoiseBank {
  const n = Math.floor(ctx.sampleRate * NOISE_SECONDS);
  const rng = mulberry32(seed);
  const make = (fill: (out: Float32Array) => void): AudioBuffer => {
    const b = ctx.createBuffer(1, n, ctx.sampleRate);
    const data = new Float32Array(n);
    fill(data);
    b.copyToChannel(data, 0);
    return b;
  };
  const white = make((o) => {
    for (let i = 0; i < n; i++) o[i] = rng() * 2 - 1;
  });
  const pink = make((o) => {
    let b0 = 0,
      b1 = 0,
      b2 = 0,
      b3 = 0,
      b4 = 0,
      b5 = 0,
      b6 = 0;
    for (let i = 0; i < n; i++) {
      const w = rng() * 2 - 1;
      b0 = 0.99886 * b0 + w * 0.0555179;
      b1 = 0.99332 * b1 + w * 0.0750759;
      b2 = 0.969 * b2 + w * 0.153852;
      b3 = 0.8665 * b3 + w * 0.3104856;
      b4 = 0.55 * b4 + w * 0.5329522;
      b5 = -0.7616 * b5 - w * 0.016898;
      o[i] = (b0 + b1 + b2 + b3 + b4 + b5 + b6 + w * 0.5362) * 0.11;
      b6 = w * 0.115926;
    }
  });
  const brown = make((o) => {
    let last = 0;
    for (let i = 0; i < n; i++) {
      last = (last + 0.02 * (rng() * 2 - 1)) / 1.02;
      o[i] = last * 3.5;
    }
  });
  return { white, pink, brown };
}

/** Seconds an envelope lasts, times scaled. */
function envLength(env: EnvDef, time: number): number {
  return (env.attack + (env.hold ?? 0) + env.decay) * time;
}

/** Seconds one burst of a layer lasts. */
function burstLength(layer: LayerDef, time: number): number {
  if (layer.source === 'modal') {
    const modes = Math.max(...(layer.partials ?? []).map((p) => p[2])) * time;
    return Math.max(modes, layer.env ? envLength(layer.env, time) : 0);
  }
  return layer.env ? envLength(layer.env, time) : 0;
}

/** Seconds from the start of a play until it is silent (before reverb). */
export function playLength(sound: SoundDef, p: PlayParams): number {
  const time = p.time * intensityFactor(sound, 'time', p.intensity);
  let end = 0;
  sound.layers.forEach((base, i) => {
    const lp = p.layers[i];
    if (!lp?.play) return;
    const layer = layerAt(base, lp.alt);
    const last = lp.bursts[lp.bursts.length - 1]?.at ?? 0;
    end = Math.max(end, (layer.at ?? 0) * time + last + burstLength(layer, time));
  });
  return end;
}

const semis = (s: number): number => Math.pow(2, s / 12);

function clampFreq(ctx: BaseAudioContext, f: number): number {
  return Math.min(ctx.sampleRate * 0.45, Math.max(20, f));
}

/** Applies an envelope to `g` from `t0` with peak `peak`; returns its end. */
function envelope(g: AudioParam, env: EnvDef, t0: number, peak: number, time: number): number {
  const attack = Math.max(0.0003, env.attack * time);
  const hold = (env.hold ?? 0) * time;
  const end = t0 + attack + hold + env.decay * time;
  // Silent before the start too: a param's intrinsic value (1) would pass a source's first
  // sample when it starts between two frames.
  g.value = 0;
  g.setValueAtTime(0, t0);
  g.linearRampToValueAtTime(peak, t0 + attack);
  if (hold > 0) g.setValueAtTime(peak, t0 + attack + hold);
  g.exponentialRampToValueAtTime(Math.max(peak * FLOOR, 1e-6), end);
  g.setValueAtTime(0, end + 0.001);
  return end;
}

function filters(
  ctx: BaseAudioContext,
  defs: FilterDef[] | undefined,
  t0: number,
  end: number,
  factor: number,
): AudioNode[] {
  return (defs ?? []).map((f) => {
    const b = ctx.createBiquadFilter();
    b.type = f.type;
    const from = clampFreq(ctx, f.freq * factor);
    b.frequency.setValueAtTime(from, t0);
    if (f.to !== undefined)
      b.frequency.exponentialRampToValueAtTime(clampFreq(ctx, f.to * factor), end);
    b.Q.value =
      f.q ?? (f.type === 'bandpass' || f.type === 'peaking' || f.type === 'notch' ? 1 : 0.707);
    if (f.gain !== undefined) b.gain.value = f.gain;
    return b;
  });
}

function chain(nodes: AudioNode[]): void {
  for (let i = 1; i < nodes.length; i++) nodes[i - 1]?.connect(nodes[i] as AudioNode);
}

/** Extra per play: a stereo position (footsteps left and right), and a gain. */
export interface PlayOptions {
  pan?: number;
  gain?: number;
}

/**
 * Schedules one play of `sound` with drawn parameters `p` at `when` into `out`; returns when it
 * ends. Every node it makes stops by then and is released.
 */
export function playSound(
  ctx: BaseAudioContext,
  noise: NoiseBank,
  out: AudioNode,
  sound: SoundDef,
  p: PlayParams,
  when: number,
  options: PlayOptions = {},
): number {
  const time = p.time * intensityFactor(sound, 'time', p.intensity);
  const cut = p.cutoff * intensityFactor(sound, 'cutoff', p.intensity);
  const pitch = semis(p.pitch);
  const level =
    sound.gain *
    Math.pow(10, p.gainDb / 20) *
    intensityFactor(sound, 'gain', p.intensity) *
    (options.gain ?? 1);
  let sink: AudioNode = out;
  if (options.pan) {
    const pan = ctx.createStereoPanner();
    pan.pan.value = Math.max(-1, Math.min(1, options.pan));
    pan.connect(out);
    sink = pan;
  }
  let end = when;
  sound.layers.forEach((base, i) => {
    const lp = p.layers[i];
    if (!lp?.play) return;
    const layer = layerAt(base, lp.alt);
    const start = when + (layer.at ?? 0) * time;
    for (const burst of lp.bursts) {
      const t0 = start + burst.at;
      const k = pitch * semis(burst.pitch);
      const peak = level * layer.gain * burst.gain;
      const length = burstLength(layer, time);
      const stop = t0 + length + 0.02;
      const amp = ctx.createGain();
      if (layer.env) envelope(amp.gain, layer.env, t0, peak, time);
      else amp.gain.value = peak;
      const fx = filters(ctx, layer.filters, t0, t0 + length, cut * k);
      chain([...fx, amp]);
      const head: AudioNode = fx[0] ?? amp;
      amp.connect(sink);
      if (layer.source === 'noise') {
        const src = ctx.createBufferSource();
        src.buffer = noise[layer.color ?? 'white'];
        src.loop = true;
        src.playbackRate.value = k;
        src.connect(head);
        src.start(t0, lp.noiseOffset * NOISE_SECONDS * 0.9);
        src.stop(stop);
      } else if (layer.source === 'osc') {
        const osc = ctx.createOscillator();
        osc.type = layer.wave ?? 'sine';
        const f = clampFreq(ctx, (layer.freq ?? 440) * k);
        osc.frequency.setValueAtTime(f, t0);
        if (layer.to !== undefined)
          osc.frequency.exponentialRampToValueAtTime(clampFreq(ctx, layer.to * k), t0 + length);
        osc.connect(head);
        osc.start(t0);
        osc.stop(stop);
      } else {
        // Modal: one decaying sine per mode, phases seeded so a fixed sound is identical.
        const rng = mulberry32(hashSeed(`${String(i)}:${String(layer.freq)}`));
        for (const [ratio, gain, decay] of layer.partials ?? []) {
          const osc = ctx.createOscillator();
          const f = (layer.freq ?? 440) * ratio * k;
          if (f >= ctx.sampleRate * 0.45) continue;
          osc.frequency.value = f;
          const g = ctx.createGain();
          envelope(g.gain, { attack: 0.0008 + rng() * 0.0004, decay }, t0, gain, time);
          osc.connect(g);
          g.connect(head);
          osc.start(t0);
          osc.stop(stop);
        }
      }
      end = Math.max(end, stop);
    }
  });
  return end;
}
