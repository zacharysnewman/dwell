// Theater mode's in-game data (shared/theater/theater.json, docs/THEATER_MODE.md §2, §6): the
// toggle key and button, the free camera's tunables, the keyframe defaults and the recording
// choices. Validated here so a bad entry fails with a message, not a silent default.

import data from '../../../shared/theater/theater.json';

export interface Aspect {
  name: string;
  w: number;
  h: number;
}

export interface TheaterConfig {
  key: string;
  gamepadToggle: string;
  fly: {
    speedMps: number;
    minSpeedMps: number;
    maxSpeedMps: number;
    wheelStep: number;
    runFactor: number;
    lookDegPerPx: number;
    padLookDegPerS: number;
    deadzone: number;
  };
  /** A phone's flying controls (src/theater/touch.ts). */
  touch: { stickRadiusPx: number; verticalFactor: number };
  keyframes: { gapS: number; moves: string[] };
  record: {
    aspects: Aspect[];
    heights: number[];
    fps: number[];
    defaultAspect: string;
    defaultHeight: number;
    defaultFps: number;
    bitsPerSecond: number;
    mimeTypes: string[];
    /** The same with a sound track (a take with audio), tried in order. */
    audioMimeTypes: string[];
    audioBitsPerSecond: number;
  };
}

/** The moves a segment may take (dwell_theater's `Move` vocabulary, THEATER_MODE.md §3.2). */
export const MOVES = ['cut', 'linear', 'ease', 'orbit', 'track', 'dolly', 'crane', 'pan'] as const;

const positive = (v: unknown, at: string): number => {
  if (typeof v !== 'number' || !(v > 0)) throw new Error(`theater.json: ${at} must be > 0`);
  return v;
};

/** Checks a theater.json; throws naming the bad entry. */
export function parseTheaterConfig(raw: unknown): TheaterConfig {
  const c = raw as TheaterConfig;
  if (typeof c.key !== 'string' || c.key === '') throw new Error('theater.json: key is required');
  for (const [k, v] of Object.entries(c.fly)) if (k !== '$comment') positive(v, `fly.${k}`);
  if (c.fly.minSpeedMps > c.fly.maxSpeedMps)
    throw new Error('theater.json: fly.minSpeedMps is over maxSpeedMps');
  positive(c.touch.stickRadiusPx, 'touch.stickRadiusPx');
  positive(c.touch.verticalFactor, 'touch.verticalFactor');
  positive(c.keyframes.gapS, 'keyframes.gapS');
  for (const m of c.keyframes.moves)
    if (!(MOVES as readonly string[]).includes(m))
      throw new Error(`theater.json: keyframes.moves: unknown move ${m}`);
  const r = c.record;
  if (r.aspects.length === 0) throw new Error('theater.json: record.aspects is empty');
  for (const a of r.aspects) {
    positive(a.w, `record.aspects ${a.name}.w`);
    positive(a.h, `record.aspects ${a.name}.h`);
  }
  if (!r.aspects.some((a) => a.name === r.defaultAspect))
    throw new Error(`theater.json: record.defaultAspect ${r.defaultAspect} is not an aspect`);
  if (!r.heights.includes(r.defaultHeight))
    throw new Error('theater.json: record.defaultHeight is not one of heights');
  if (!r.fps.includes(r.defaultFps))
    throw new Error('theater.json: record.defaultFps is not one of fps');
  if (!Array.isArray(r.audioMimeTypes) || r.audioMimeTypes.length === 0)
    throw new Error('theater.json: record.audioMimeTypes is empty');
  positive(r.audioBitsPerSecond, 'record.audioBitsPerSecond');
  return c;
}

export const THEATER: TheaterConfig = parseTheaterConfig(data);
