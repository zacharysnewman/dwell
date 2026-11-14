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

/** A block of recorded samples: its context time (s) and its channels. */
export interface Chunk {
  at: number;
  channels: Float32Array[];
}

/**
 * The recorded blocks laid on one timeline of `n` samples starting at context time `originS`,
 * shifted back `offset` samples (the recorder's latency); gaps stay silent, overlaps take the later.
 */
export function placeChunks(
  chunks: readonly Chunk[],
  originS: number,
  sr: number,
  n: number,
  offset: number,
  channels: number,
): Float32Array[] {
  const out = Array.from({ length: channels }, () => new Float32Array(n));
  for (const c of chunks) {
    const start = Math.round((c.at - originS) * sr) - offset;
    for (let ch = 0; ch < channels; ch++) {
      const src = c.channels[ch];
      const dst = out[ch];
      if (!src || !dst) continue;
      for (let j = 0; j < src.length; j++) {
        const k = start + j;
        if (k >= 0 && k < n) dst[k] = src[j] ?? 0;
      }
    }
  }
  return out;
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
      at: e.playbackTime,
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
      const raw = placeChunks(chunks, origin, sr, n + sr, 0, 3);
      // The recorder's latency: where frame 0's impulse landed (it was scheduled at the origin).
      const offset = Math.max(0, firstAbove(raw[2] ?? new Float32Array(0), 0));
      const track = placeChunks(chunks, origin, sr, n, offset, 3);
      const errs = impulseErrors(
        track[2] ?? new Float32Array(0),
        times.map((t) => Math.round(t * sr)).filter((e) => e < n),
        sr,
      );
      const ok = errs.filter((e) => !Number.isNaN(e));
      return {
        sampleRate: sr,
        samples: n,
        seconds: n / sr,
        buses: tap?.buses ?? [],
        impulses: ok.length,
        meanErrorMs: ok.reduce((a, b) => a + Math.abs(b), 0) / Math.max(1, ok.length),
        maxErrorMs: Math.max(0, ...ok.map(Math.abs)),
        late,
        wav: wavBytes([track[0] ?? new Float32Array(n), track[1] ?? new Float32Array(n)], sr),
      };
    },
    close: closeAll,
  };
}
/* eslint-enable @typescript-eslint/no-deprecated */
