// Cinematic takes (docs/THEATER_MODE.md §6.1; owner, 2026-11-14: "take more time to render it so it
// looks at its prettiest? Without seeing LODs load in, physics stutters, etc?"): an offline,
// frame-exact render of a camera path. The game's time is decoupled from the wall clock: the local
// world steps exactly the server ticks of 1 / fps per captured frame (its real-time clock held), the
// client's clock (`holdClock`/`stepClockTo`: every animation, the day clock, the camera path) is set
// from the frame count, and the sound is stepped with it (cinematicAudio.ts). Each frame waits for the
// view to load before it is captured (capped, reported); a pre-roll flies the path once first.
// Frames are captured lossless (PNG) at the output size from a supersampled canvas, optionally
// averaged over sub-frames (motion blur), and handed to a sink (the record skill's driver).
//
// The settings are data (shared/theater/cinematic.json, validated here); the pure parts (the frame
// clock, the settle gate, the options) are tested in cinematic.test.ts.

import data from '../../../shared/theater/cinematic.json';
import { SIM_HZ } from '../protocol/constants.gen';
import type { AudioStep, CinematicAudio } from './cinematicAudio';

export interface CinematicConfig {
  fps: number;
  subframes: number;
  supersample: number;
  settle: { minFrames: number; maxWaiting: number; capMs: number; firstCapMs: number };
  preroll: { stepS: number; capMs: number };
  quality: { shadows?: string; view: Record<string, unknown> };
  audio: { on: boolean; leadS: number };
  image: string;
}

type Raw = Record<string, unknown>;

function isObj(v: unknown): v is Raw {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

function num(o: Raw, key: string, where: string, min: number, max: number, int = false): number {
  const v = o[key];
  if (
    typeof v !== 'number' ||
    !Number.isFinite(v) ||
    v < min ||
    v > max ||
    (int && !Number.isInteger(v))
  )
    throw new Error(
      `cinematic: ${where}${key} must be ${int ? 'an integer' : 'a number'} in [${String(min)}, ${String(max)}], got ${JSON.stringify(v)}`,
    );
  return v;
}

const KEYS = [
  '$comment',
  'formatVersion',
  'fps',
  'subframes',
  'supersample',
  'settle',
  'preroll',
  'quality',
  'audio',
  'image',
];

/**
 * Validates cinematic settings (the data file, or it with a take's overrides merged in): every
 * field present and in range, unknown keys refused, so a bad entry fails with a message.
 */
export function parseCinematic(raw: unknown): CinematicConfig {
  if (!isObj(raw)) throw new Error('cinematic: the settings must be an object');
  for (const k of Object.keys(raw))
    if (!KEYS.includes(k))
      throw new Error(`cinematic: unknown key ${k} (${KEYS.slice(2).join(', ')})`);
  const settle = raw.settle;
  const preroll = raw.preroll;
  const quality = raw.quality;
  const audio = raw.audio;
  if (!isObj(settle) || !isObj(preroll) || !isObj(quality) || !isObj(audio))
    throw new Error('cinematic: settle, preroll, quality and audio must be objects');
  if (!isObj(quality.view)) throw new Error('cinematic: quality.view must be an object');
  const shadows = quality.shadows;
  if (
    shadows !== undefined &&
    (typeof shadows !== 'string' || !['off', 'low', 'high'].includes(shadows))
  )
    throw new Error(
      `cinematic: quality.shadows must be off, low or high, got ${JSON.stringify(quality.shadows)}`,
    );
  if (typeof audio.on !== 'boolean') throw new Error('cinematic: audio.on must be true or false');
  if (raw.image !== 'image/png' && raw.image !== 'image/webp')
    throw new Error(
      `cinematic: image must be image/png or image/webp, got ${JSON.stringify(raw.image)}`,
    );
  return {
    fps: num(raw, 'fps', '', 1, 240, true),
    subframes: num(raw, 'subframes', '', 1, 64, true),
    supersample: num(raw, 'supersample', '', 1, 4),
    settle: {
      minFrames: num(settle, 'minFrames', 'settle.', 1, 120, true),
      maxWaiting: num(settle, 'maxWaiting', 'settle.', 0, 1_000_000, true),
      capMs: num(settle, 'capMs', 'settle.', 0, 600_000),
      firstCapMs: num(settle, 'firstCapMs', 'settle.', 0, 600_000),
    },
    preroll: {
      stepS: num(preroll, 'stepS', 'preroll.', 0, 60),
      capMs: num(preroll, 'capMs', 'preroll.', 0, 600_000),
    },
    quality: {
      ...(typeof shadows === 'string' ? { shadows } : {}),
      view: quality.view,
    },
    audio: { on: audio.on, leadS: num(audio, 'leadS', 'audio.', 0, 1) },
    image: raw.image,
  };
}

/** Deep-merges a take's overrides over the settings (objects merge, anything else replaces). */
export function withOverrides(base: Raw, over: unknown): Raw {
  if (!isObj(over)) return base;
  const out: Raw = { ...base };
  for (const [k, v] of Object.entries(over)) {
    const b = out[k];
    out[k] = isObj(b) && isObj(v) ? withOverrides(b, v) : v;
  }
  return out;
}

export const CINEMATIC: CinematicConfig = parseCinematic(data);

/**
 * The take's game time (s) at captured frame `frame`, sub-frame `sub` of `subframes`: computed from
 * the counts, never summed, so a long take has no drift (frame 9000 at 30 fps is exactly 300 s).
 */
export function frameTimeS(frame: number, sub: number, fps: number, subframes: number): number {
  return (frame * subframes + sub) / (fps * subframes);
}

/**
 * The server ticks due by game time `tS` (s) from the take's start: the ticks a step runs are the
 * difference of two of these, so the world's time follows the frames exactly (at 30 fps, 2 a frame).
 */
export function ticksDue(tS: number, hz: number = SIM_HZ): number {
  return Math.floor(tS * hz + 1e-6);
}

/** The view's load state, as `__dwell.state()` reports it. */
export interface ViewState {
  lod?: { waitingInView: number } | null;
  terrain?: { dirty: number; meshing: number } | null;
}

/**
 * The view is loaded: at most `maxWaiting` LOD sections waiting in view (0: none, the record skill's
 * `viewLoaded`) and the full-detail chunks meshed. A fast fly-over at a fine pixel error kept ~70 of
 * ~14 000 drawn sections waiting on every step, so the data allows a small residue.
 */
export function viewLoaded(st: ViewState | null | undefined, maxWaiting = 0): boolean {
  const lod = st?.lod;
  const t = st?.terrain;
  return !!lod && lod.waitingInView <= maxWaiting && (!t || t.dirty + t.meshing === 0);
}

/**
 * The per-frame settle gate: after a step, the frame is captured once the view has been loaded on
 * `minFrames` drawn frames in a row, or when `capMs` (wall time) has passed — captured anyway, and
 * counted as capped.
 */
export class SettleGate {
  private loadedFrames = 0;
  private startMs = 0;
  private capMs = 0;

  constructor(
    private readonly minFrames: number,
    private readonly maxWaiting = 0,
  ) {}

  start(nowMs: number, capMs: number): void {
    this.loadedFrames = 0;
    this.startMs = nowMs;
    this.capMs = capMs;
  }

  /** One frame after the step: 'wait', 'ready' (settled) or 'capped'. */
  update(
    st: ViewState | null | undefined,
    drawn: boolean,
    nowMs: number,
  ): 'wait' | 'ready' | 'capped' {
    if (drawn) this.loadedFrames = viewLoaded(st, this.maxWaiting) ? this.loadedFrames + 1 : 0;
    if (drawn && this.loadedFrames >= this.minFrames) return 'ready';
    if (drawn && nowMs - this.startMs >= this.capMs) return 'capped';
    return 'wait';
  }

  waitedMs(nowMs: number): number {
    return nowMs - this.startMs;
  }
}

/** The output frame's file name (the driver's frame folder: f00000.png …). */
export function frameName(i: number, image: string): string {
  return `f${String(i).padStart(5, '0')}.${image === 'image/webp' ? 'webp' : 'png'}`;
}

/** What a take needs from the page (main.ts wires it). */
export interface CinematicHost {
  /** The client's clock held (true) or released; returns it (ms). */
  holdClock(on: boolean): number;
  stepClockTo(ms: number): void;
  /** The local world's real-time clock held or released, and exact steps (null: no local world). */
  world: { hold(on: boolean): void; step(ticks: number): Promise<number | null> } | null;
  theaterExact(on: boolean): void;
  theaterAdvance(dtS: number): void;
  theaterPlay(json: string): void;
  theaterStop(): void;
  theaterState(): { time: number; duration: number; finished: boolean } | null;
  state(): ViewState | null;
  setView(view: Record<string, unknown>): void;
  setShadows(tier: string): void;
  canvas: HTMLCanvasElement;
  canvasSize(size: [number, number] | null): void;
  /** Resolves after the next animation frame with whether it drew; `capture` runs in that frame, right after the draw. */
  nextFrame(capture: ((canvas: HTMLCanvasElement) => void) | null): Promise<boolean>;
  /** Hands a finished file to the recorder (the driver's `__dwellRecSink`). */
  sink(file: string, bytes: Uint8Array): Promise<void>;
  /** The sound stepped with the frames, or null (no audio). */
  audio(leadS: number): Promise<CinematicAudio | null>;
  wallMs(): number;
}

export interface CinematicTakeOptions {
  path: unknown;
  /** The output size (px), e.g. [1920, 1080]. */
  size: [number, number];
  /** Overrides of shared/theater/cinematic.json's settings for this take. */
  settings?: unknown;
  /** Stop after this many frames (default: the path's length × fps). */
  frames?: number;
}

export interface CinematicReport {
  frames: number;
  fps: number;
  seconds: number;
  wallS: number;
  prerollS: number;
  /** The settle waits (ms): mean, the 95th percentile and the worst, and which frames hit the cap. */
  settle: { meanMs: number; p95Ms: number; maxMs: number; capped: number[] };
  ticks: number;
  audio: AudioStep | null;
}

/** The p-th percentile of `xs` (0–1). */
export function percentile(xs: readonly number[], p: number): number {
  if (!xs.length) return 0;
  const s = [...xs].sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.floor(p * s.length))] ?? 0;
}

/** Frames being encoded at once (each a PNG of the output size). */
const ENCODES = 8;

async function blobBytes(c: OffscreenCanvas, type: string): Promise<Uint8Array> {
  const b = await c.convertToBlob({ type });
  return new Uint8Array(await b.arrayBuffer());
}

/**
 * Runs a cinematic take: pre-roll, then each frame stepped, settled, captured (averaged over
 * sub-frames) and sunk; the sound stepped alongside and sunk as `audio.wav` at the end.
 */
export async function runCinematic(
  host: CinematicHost,
  o: CinematicTakeOptions,
): Promise<CinematicReport> {
  const cfg = parseCinematic(withOverrides(data, o.settings));
  const [w, h] = o.size;
  const json = typeof o.path === 'string' ? o.path : JSON.stringify(o.path);
  const t0 = host.wallMs();
  if (cfg.quality.shadows) host.setShadows(cfg.quality.shadows);
  host.setView(cfg.quality.view);
  host.canvasSize([Math.round(w * cfg.supersample), Math.round(h * cfg.supersample)]);
  const out = new OffscreenCanvas(w, h);
  const ctx2d = out.getContext('2d', { alpha: false });
  if (!ctx2d) throw new Error('cinematic: no 2D canvas');
  ctx2d.imageSmoothingEnabled = true;
  ctx2d.imageSmoothingQuality = 'high';
  const gate = new SettleGate(cfg.settle.minFrames, cfg.settle.maxWaiting);
  const settle = async (capMs: number): Promise<{ ms: number; capped: boolean }> => {
    gate.start(host.wallMs(), capMs);
    for (;;) {
      const drawn = await host.nextFrame(null);
      const r = gate.update(host.state(), drawn, host.wallMs());
      if (r !== 'wait') return { ms: gate.waitedMs(host.wallMs()), capped: r === 'capped' };
    }
  };
  const clock0 = host.holdClock(true);
  host.world?.hold(true);
  host.theaterExact(true);
  let audio: CinematicAudio | null = null;
  try {
    // The pre-roll: the path flown once, the world held, each step settled; nothing captured.
    host.theaterPlay(json);
    const duration = host.theaterState()?.duration ?? 0;
    if (cfg.preroll.stepS > 0) {
      await settle(cfg.settle.firstCapMs);
      for (let t = cfg.preroll.stepS; t < duration + cfg.preroll.stepS; t += cfg.preroll.stepS) {
        host.theaterAdvance(cfg.preroll.stepS);
        await settle(cfg.preroll.capMs);
      }
      host.theaterStop();
      host.theaterPlay(json);
    }
    const prerollS = (host.wallMs() - t0) / 1000;
    const frames = o.frames ?? Math.max(1, Math.round(duration * cfg.fps) + 1);
    const S = cfg.subframes;
    audio = cfg.audio.on ? await host.audio(cfg.audio.leadS) : null;
    const waits: number[] = [];
    const capped: number[] = [];
    let ticks = 0;
    let lastT = 0;
    let sunk: Promise<void> = Promise.resolve();
    const inFlight: Promise<void>[] = [];
    for (let i = 0; i < frames; i++) {
      for (let s = 0; s < S; s++) {
        const t = frameTimeS(i, s, cfg.fps, S);
        if (i > 0 || s > 0) {
          const due = ticksDue(t) - ticks;
          if (due > 0 && host.world) await host.world.step(due);
          ticks += Math.max(0, due);
          host.stepClockTo(clock0 + t * 1000);
          host.theaterAdvance(t - lastT);
          lastT = t;
          // The sound up to this instant: the events of the frame before land on their frame.
          if (audio) await audio.runTo(t);
        }
        const r = await settle(i === 0 && s === 0 ? cfg.settle.firstCapMs : cfg.settle.capMs);
        if (s === 0) {
          waits.push(r.ms);
          if (r.capped) capped.push(i);
        }
        // Capture on the next drawn frame, right after its draw: a running mean over the sub-frames.
        for (;;) {
          const drawn = await host.nextFrame((canvas) => {
            ctx2d.globalAlpha = 1 / (s + 1);
            ctx2d.drawImage(canvas, 0, 0, w, h);
          });
          if (drawn) break;
        }
      }
      ctx2d.globalAlpha = 1;
      // Encoded off the frame loop (a 1080p PNG takes ~1–2 s): the pixels are handed to a new canvas
      // and encoded while the next frames render, at most ENCODES at once, sunk in order.
      const pixels = out.transferToImageBitmap();
      const name = frameName(i, cfg.image);
      const encoded = (async () => {
        const c = new OffscreenCanvas(w, h);
        c.getContext('bitmaprenderer')?.transferFromImageBitmap(pixels);
        return blobBytes(c, cfg.image);
      })();
      sunk = sunk.then(async () => host.sink(name, await encoded));
      inFlight.push(sunk);
      if (inFlight.length >= ENCODES) await inFlight.shift();
    }
    await sunk;
    const seconds = frames / cfg.fps;
    let step: AudioStep | null = null;
    if (audio) {
      step = await audio.finish(seconds);
      await host.sink('audio.wav', step.wav);
    }
    return {
      frames,
      fps: cfg.fps,
      seconds,
      wallS: (host.wallMs() - t0) / 1000,
      prerollS,
      settle: {
        meanMs: waits.reduce((a, b) => a + b, 0) / Math.max(1, waits.length),
        p95Ms: percentile(waits, 0.95),
        maxMs: Math.max(0, ...waits),
        capped,
      },
      ticks,
      audio: step && { ...step, wav: new Uint8Array(0) },
    };
  } finally {
    audio?.close();
    host.theaterExact(false);
    host.theaterStop();
    host.world?.hold(false);
    host.holdClock(false);
    host.canvasSize(null);
  }
}
