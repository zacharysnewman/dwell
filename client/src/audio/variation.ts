// Per-play variation (docs/AUDIO.md §4): a seeded RNG draws each play's pitch, gain, cutoff and
// time offsets within the sound's ranges, the round-robin alternative per layer (never the
// previous one), the grain bursts, and the noise start offsets. Sounds with `variation: "none"`
// (the UI) draw the same parameters every time. Pure: the synth (synth.ts) turns a draw into nodes.
import type { LayerDef, LayerFields, SoundDef } from './types';

export type Rng = () => number;

/** mulberry32: a small, fast, seeded generator of floats in [0, 1). */
export function mulberry32(seed: number): Rng {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** FNV-1a 32 of a string: a fixed seed per sound name. */
export function hashSeed(s: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h;
}

export interface Burst {
  /** Seconds after the layer's start. */
  at: number;
  gain: number;
  /** Semitones. */
  pitch: number;
}

export interface LayerPlay {
  /** Whether the layer plays this time (`chance`). */
  play: boolean;
  /** Index into `alts`, or −1. */
  alt: number;
  /** The grain bursts; one burst at 0 without `grains`. */
  bursts: Burst[];
  /** Noise layers: where in the noise buffer to start, 0–1. */
  noiseOffset: number;
}

/** One play's drawn parameters. */
export interface PlayParams {
  /** Semitones. */
  pitch: number;
  /** dB. */
  gainDb: number;
  /** Factor on every filter frequency. */
  cutoff: number;
  /** Factor on every envelope time. */
  time: number;
  /** 0–1, the event's (a landing's impact). */
  intensity: number;
  layers: LayerPlay[];
}

/** A layer with its alternative merged over it. */
export function layerAt(layer: LayerDef, alt: number): LayerDef {
  const a: LayerFields | undefined = alt >= 0 ? layer.alts?.[alt] : undefined;
  return a ? { ...layer, ...a } : layer;
}

/** Closest two successive plays of one sound may be in pitch (semitones) before a redraw. */
export const MIN_PITCH_STEP = 0.25;

const between = (rng: Rng, lo: number, hi: number): number => lo + (hi - lo) * rng();
const spread = (rng: Rng, range: number | undefined): number =>
  range ? between(rng, -range, range) : 0;

interface Last {
  pitch: number;
  alts: number[];
}

/** Draws plays; remembers each sound's last play so the next one differs. */
export class Variation {
  private readonly rng: Rng;
  private readonly last = new Map<string, Last>();

  constructor(seed = 1) {
    this.rng = mulberry32(seed);
  }

  draw(name: string, sound: SoundDef, intensity = 1): PlayParams {
    if (sound.variation === 'none') return fixedPlay(name, sound, intensity);
    const v = sound.variation;
    const rng = this.rng;
    const last = this.last.get(name);
    let pitch = spread(rng, v.pitch);
    // Never the same twice in a row: a pitch too close to the last play's is drawn again.
    for (let i = 0; i < 8 && last && v.pitch && Math.abs(pitch - last.pitch) < MIN_PITCH_STEP; i++)
      pitch = spread(rng, v.pitch);
    const gainDb = spread(rng, v.gain);
    const cutoff = 1 + spread(rng, v.cutoff);
    const time = 1 + spread(rng, v.time);
    const layers = sound.layers.map((layer, i) => {
      const n = layer.alts?.length ?? 0;
      let alt = -1;
      if (n > 0) {
        const prev = last?.alts[i] ?? -1;
        // Round robin without repeats: any alternative but the previous one.
        alt = Math.floor(rng() * (prev >= 0 ? n - 1 : n));
        if (prev >= 0 && alt >= prev) alt++;
      }
      return drawLayer(rng, layerAt(layer, alt), alt);
    });
    this.last.set(name, { pitch, alts: layers.map((l) => l.alt) });
    return { pitch, gainDb, cutoff, time, intensity, layers };
  }
}

function drawLayer(rng: Rng, layer: LayerDef, alt: number): LayerPlay {
  const play = layer.chance === undefined || rng() < layer.chance;
  const g = layer.grains;
  let bursts: Burst[] = [{ at: 0, gain: 1, pitch: 0 }];
  if (g) {
    const count = g.count[0] + Math.floor(rng() * (g.count[1] - g.count[0] + 1));
    bursts = Array.from({ length: count }, () => ({
      at: rng() * g.spread,
      gain: between(rng, g.gain[0], g.gain[1]),
      pitch: spread(rng, g.pitch),
    })).sort((a, b) => a.at - b.at);
  }
  return { play, alt, bursts, noiseOffset: rng() };
}

/** `variation: "none"`: the same draw every time (grains fixed by the sound's name). */
function fixedPlay(name: string, sound: SoundDef, intensity: number): PlayParams {
  const rng = mulberry32(hashSeed(name));
  return {
    pitch: 0,
    gainDb: 0,
    cutoff: 1,
    time: 1,
    intensity,
    layers: sound.layers.map((layer) => {
      const l = drawLayer(rng, layer, -1);
      return { ...l, play: true };
    }),
  };
}

/** The intensity's factor for one of gain, cutoff or time: linear between the data's pair. */
export function intensityFactor(
  sound: SoundDef,
  key: 'gain' | 'cutoff' | 'time',
  intensity: number,
): number {
  const pair = sound.intensity?.[key];
  if (!pair) return 1;
  const t = Math.min(1, Math.max(0, intensity));
  return pair[0] + (pair[1] - pair[0]) * t;
}
