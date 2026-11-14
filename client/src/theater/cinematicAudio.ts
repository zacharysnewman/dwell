// A cinematic take's sound (docs/THEATER_MODE.md §6.1; owner, 2026-11-14: "If we can, let's update sound
// to align with frame exact time, at least in recording mode."). The game's own mix (effects and
// ambience, never music: the mixer's tap) is rendered by the page's AudioContext, but the context is
// stepped with the frames: suspended between them, resumed until its clock reaches the next frame's
// time. While the take holds the engine's clock (`AudioEngine.holdClock`), every sound is scheduled
// at its frame's time on the context's timeline (`C(t) = base + t`, plus `leadS` so a step that
// overshoots by a hardware buffer is never late), so Web Audio's sample-accurate scheduling puts it
// on its frame however long the frame took to render. A recorder (a ScriptProcessor) keeps every
// rendered sample with its time; a third channel carries an impulse scheduled like a sound at every
// frame, which calibrates the recorder's latency and measures the alignment. Live play is unchanged:
// only a take holds the clock.

/** What the take needs from the audio engine (engine.ts). */
export interface TakeAudioEngine {
  holdClock(clock: (() => number) | null): AudioContext | null;
  tapInto(dest: AudioNode): { buses: string[]; close(): void } | null;
}

/** The finished track and how well it lined up. */
export interface AudioStep {
  sampleRate: number;
  samples: number;
  seconds: number;
  buses: string[];
  /** Frame impulses found, and their error from their frame's time (ms). */
  impulses: number;
  meanErrorMs: number;
  maxErrorMs: number;
  /** How far the recorder had slipped a frame's mark before the re-timing (ms). */
  rawMaxSlipMs: number;
  /** Steps whose context ran past the next sound's scheduled time (a sound would have been late). */
  late: number;
  wav: Uint8Array;
}

export interface CinematicAudio {
  /** Steps the context to take time `tS` (s); sounds triggered after this land at `tS`. */
  runTo(tS: number): Promise<void>;
  /** Renders the rest and returns the track, exactly `seconds` long. */
  finish(seconds: number): Promise<AudioStep>;
  close(): void;
}

/** A block of recorded samples, in the order the recorder received it. */
export interface Chunk {
  channels: Float32Array[];
}

/**
 * The recorded blocks joined in order: the context's rendered stream, sample for sample. (Placing
 * each block by its reported time instead put the frame marks up to 16 ms off and lost some: the
 * times jump when the context is suspended and resumed.) `n` samples from `start`, silence past the end.
 */
export function joinChunks(
  chunks: readonly Chunk[],
  channels: number,
  start: number,
  n: number,
): Float32Array[] {
  const out = Array.from({ length: channels }, () => new Float32Array(n));
  let at = -start;
  for (const c of chunks) {
    const len = c.channels[0]?.length ?? 0;
    if (at + len > 0 && at < n)
      for (let ch = 0; ch < channels; ch++) {
        const src = c.channels[ch];
        const dst = out[ch];
        if (!src || !dst) continue;
        for (let j = Math.max(0, -at); j < len && at + j < n; j++) dst[at + j] = src[j] ?? 0;
      }
    at += len;
  }
  return out;
}

/** The recorded length in samples. */
export function chunkSamples(chunks: readonly Chunk[]): number {
  return chunks.reduce((a, c) => a + (c.channels[0]?.length ?? 0), 0);
}

/** The first sample index where `ch` rises over `threshold` at or after `from` (−1: none). */
export function firstAbove(ch: Float32Array, from: number, threshold = 0.5): number {
  for (let i = Math.max(0, from); i < ch.length; i++) if ((ch[i] ?? 0) > threshold) return i;
  return -1;
}

/**
 * Each expected impulse (sample index) matched to the impulse found nearest it in `ch` (within half
 * the gap to the next); the errors in ms, NaN for one not found.
 */
export function impulseErrors(ch: Float32Array, expected: readonly number[], sr: number): number[] {
  const found: number[] = [];
  for (let i = 0; i < ch.length; i++) if ((ch[i] ?? 0) > 0.5) found.push(i);
  return expected.map((e, k) => {
    const next = expected[k + 1] ?? e + sr;
    const prev = expected[k - 1] ?? e - sr;
    const lo = e - (e - prev) / 2;
    const hi = e + (next - e) / 2;
    let best = NaN;
    for (const f of found)
      if (f >= lo && f < hi && (Number.isNaN(best) || Math.abs(f - e) < Math.abs(best - e)))
        best = f;
    return Number.isNaN(best) ? NaN : ((best - e) / sr) * 1000;
  });
}

/**
 * Each frame's mark in a recorded stream: the impulse nearest where it should be (the last mark plus
 * the frames' spacing), within half a frame; a mark not found is put at that prediction.
 */
export function findMarks(
  ch: Float32Array,
  expected: readonly number[],
  threshold = 0.5,
): number[] {
  const marks: number[] = [];
  for (let i = 0; i < expected.length; i++) {
    const e = expected[i] ?? 0;
    const prevE = expected[i - 1] ?? e;
    const guess = i === 0 ? e : (marks[i - 1] ?? 0) + (e - prevE);
    const half = Math.max(1, Math.floor(((expected[i + 1] ?? e + (e - prevE || 1)) - e) / 2));
    let best = -1;
    for (let k = Math.max(0, guess - half); k < Math.min(ch.length, guess + half); k++)
      if ((ch[k] ?? 0) > threshold && (best < 0 || Math.abs(k - guess) < Math.abs(best - guess)))
        best = k;
    marks.push(best < 0 ? guess : best);
  }
  return marks;
}

/**
 * The stream re-timed by its frame marks: frame i's audio (from its mark on) laid at its expected
 * sample, so every frame's sounds start exactly on their frame whatever the recorder slipped (it slips
 * a block now and then across a suspend). Within a frame the samples run on unbroken; a frame whose
 * recorded span was longer or shorter drops or repeats a few samples at its end.
 */
export function realign(
  raw: readonly Float32Array[],
  marks: readonly number[],
  expected: readonly number[],
  n: number,
): Float32Array[] {
  return raw.map((src) => {
    const out = new Float32Array(n);
    for (let i = 0; i < expected.length; i++) {
      const from = expected[i] ?? 0;
      const to = Math.min(n, expected[i + 1] ?? n);
      const m = marks[i] ?? from;
      for (let k = from; k < to; k++) out[k] = src[m + (k - from)] ?? 0;
    }
    return out;
  });
}

/** A 16-bit PCM WAV of the channels. */
export function wavBytes(channels: readonly Float32Array[], sr: number): Uint8Array {
  const nch = channels.length;
  const n = channels[0]?.length ?? 0;
  const data = n * nch * 2;
  const buf = new ArrayBuffer(44 + data);
  const v = new DataView(buf);
  const str = (o: number, s: string): void => {
    for (let i = 0; i < s.length; i++) v.setUint8(o + i, s.charCodeAt(i));
  };
  str(0, 'RIFF');
  v.setUint32(4, 36 + data, true);
  str(8, 'WAVE');
  str(12, 'fmt ');
  v.setUint32(16, 16, true);
  v.setUint16(20, 1, true);
  v.setUint16(22, nch, true);
  v.setUint32(24, sr, true);
  v.setUint32(28, sr * nch * 2, true);
  v.setUint16(32, nch * 2, true);
  v.setUint16(34, 16, true);
  str(36, 'data');
  v.setUint32(40, data, true);
  let o = 44;
  for (let i = 0; i < n; i++)
    for (let ch = 0; ch < nch; ch++) {
      const x = Math.max(-1, Math.min(1, channels[ch]?.[i] ?? 0));
      v.setInt16(o, Math.round(x * 32767), true);
      o += 2;
    }
  return new Uint8Array(buf);
}

/** The engine's own lead on a play (engine.ts `now + 0.005`). */
const PLAY_LEAD_S = 0.005;

const yieldTask = (): Promise<void> =>
  new Promise((r) => {
    const c = new MessageChannel();
    c.port1.onmessage = () => {
      r();
    };
    c.port2.postMessage(0);
  });

// The recorder is a ScriptProcessor: deprecated, but it needs no module served for it (an AudioWorklet
// does), and it runs only during a cinematic take.
/* eslint-disable @typescript-eslint/no-deprecated */
/** Starts a take's stepped sound, or null without Web Audio. */
export async function startCinematicAudio(
  engine: TakeAudioEngine,
  leadS: number,
): Promise<CinematicAudio | null> {
  let base = 0;
  let now = 0;
  const ctx = engine.holdClock(() => base + now + leadS);
  if (!ctx || typeof ctx.createScriptProcessor !== 'function') {
    engine.holdClock(null);
    return null;
  }
  const sr = ctx.sampleRate;
  const tapIn = ctx.createGain();
  const split = ctx.createChannelSplitter(2);
  const merge = ctx.createChannelMerger(3);
  const rec = ctx.createScriptProcessor(1024, 3, 1);
  const chunks: Chunk[] = [];
  rec.onaudioprocess = (e) => {
    const b = e.inputBuffer;
    chunks.push({
      channels: [0, 1, 2].map((c) => b.getChannelData(c).slice()),
    });
  };
  tapIn.connect(split);
  split.connect(merge, 0, 0);
  split.connect(merge, 1, 1);
  merge.connect(rec);
  rec.connect(ctx.destination);
  const tap = engine.tapInto(tapIn);
  const one = ctx.createBuffer(1, 1, sr);
  one.getChannelData(0)[0] = 1;
  const impulse = (when: number): void => {
    const s = ctx.createBufferSource();
    s.buffer = one;
    s.connect(merge, 0, 2);
    s.start(when);
  };
  await ctx.resume();
  base = ctx.currentTime + 0.1;
  const times: number[] = [0];
  impulse(base + leadS + PLAY_LEAD_S);
  let late = 0;
  const run = async (target: number): Promise<void> => {
    if (ctx.currentTime < target) {
      await ctx.resume();
      while (ctx.currentTime < target) await yieldTask();
    }
    await ctx.suspend();
  };
  await run(base);
  const closeAll = (): void => {
    tap?.close();
    for (const n of [tapIn, split, merge, rec]) n.disconnect();
    engine.holdClock(null);
    void ctx.resume().catch(() => undefined);
  };
  return {
    async runTo(tS) {
      await run(base + tS);
      // A sound now is scheduled at base + tS + leadS: late if the context already passed it.
      if (ctx.currentTime > base + tS + leadS) late++;
      now = tS;
      times.push(tS);
      impulse(base + tS + leadS + PLAY_LEAD_S);
    },
    async finish(seconds) {
      const origin = base + leadS + PLAY_LEAD_S;
      // Past the end by the recorder's buffer twice over, so its last blocks arrive.
      await run(origin + seconds + (3 * 1024) / sr + 0.05);
      await ctx.resume();
      const flushUntil = performance.now() + 300;
      while (performance.now() < flushUntil) await yieldTask();
      await ctx.suspend();
      const n = Math.round(seconds * sr);
      // The stream from frame 0's mark (scheduled at the origin): the recorder's latency drops out.
      const whole = joinChunks(chunks, 3, 0, chunkSamples(chunks));
      const start = Math.max(0, firstAbove(whole[2] ?? new Float32Array(0), 0));
      const raw = joinChunks(chunks, 3, start, n + sr);
      const expected = times.map((t) => Math.round(t * sr)).filter((e) => e < n);
      // Where each frame's mark landed in the raw stream (the recorder's slips), then re-timed on them.
      const marks = findMarks(raw[2] ?? new Float32Array(0), expected);
      const slips = marks.map((m, i) => ((m - (expected[i] ?? 0)) / sr) * 1000);
      const track = realign(raw, marks, expected, n);
      const errs = impulseErrors(track[2] ?? new Float32Array(0), expected, sr);
      const ok = errs.filter((e) => !Number.isNaN(e));
      return {
        sampleRate: sr,
        samples: n,
        seconds: n / sr,
        buses: tap?.buses ?? [],
        impulses: ok.length,
        meanErrorMs: ok.reduce((a, b) => a + Math.abs(b), 0) / Math.max(1, ok.length),
        maxErrorMs: Math.max(0, ...ok.map(Math.abs)),
        rawMaxSlipMs: Math.max(0, ...slips.map(Math.abs)),
        late,
        wav: wavBytes([track[0] ?? new Float32Array(n), track[1] ?? new Float32Array(n)], sr),
      };
    },
    close: closeAll,
  };
}
/* eslint-enable @typescript-eslint/no-deprecated */
