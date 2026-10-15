// Offline renders' helpers (docs/AUDIO.md §7): a render's statistics and checks, and 16-bit PCM
// WAV encoding for listening. Pure: the renders themselves run in a browser (render.ts).

export interface RenderStats {
  /** Largest absolute sample. */
  peak: number;
  rms: number;
  /** Seconds until the signal last exceeds −60 dB of its peak. */
  audible: number;
  /** Seconds rendered. */
  length: number;
}

export function renderStats(channels: Float32Array[], sampleRate: number): RenderStats {
  let peak = 0;
  let sum = 0;
  let n = 0;
  for (const ch of channels) {
    for (const v of ch) {
      const a = Math.abs(v);
      if (a > peak) peak = a;
      sum += v * v;
      n++;
    }
  }
  const floor = peak * 0.001;
  let last = 0;
  for (const ch of channels) {
    for (let i = ch.length - 1; i > last; i--) {
      if (Math.abs(ch[i] ?? 0) > floor) {
        last = i;
        break;
      }
    }
  }
  return {
    peak,
    rms: Math.sqrt(sum / Math.max(1, n)),
    audible: last / sampleRate,
    length: (channels[0]?.length ?? 0) / sampleRate,
  };
}

export interface RenderLimits {
  /** Shortest and longest sensible audible length, seconds. */
  minSeconds: number;
  maxSeconds: number;
}

/** What is wrong with a render, or an empty list: non-silent, no clipping, a sensible length. */
export function checkRender(s: RenderStats, limits: RenderLimits): string[] {
  const problems: string[] = [];
  if (!(s.peak > 0.01)) problems.push(`silent (peak ${s.peak.toFixed(4)})`);
  if (!(s.peak < 1)) problems.push(`clips (peak ${s.peak.toFixed(3)})`);
  if (s.audible < limits.minSeconds)
    problems.push(`too short (${s.audible.toFixed(3)} s < ${String(limits.minSeconds)} s)`);
  if (s.audible > limits.maxSeconds)
    problems.push(`too long (${s.audible.toFixed(3)} s > ${String(limits.maxSeconds)} s)`);
  return problems;
}

/** Interleaved 16-bit PCM WAV of the channels. */
export function encodeWav(channels: Float32Array[], sampleRate: number): Uint8Array {
  const chs = channels.length;
  const frames = channels[0]?.length ?? 0;
  const bytes = new Uint8Array(44 + frames * chs * 2);
  const view = new DataView(bytes.buffer);
  const text = (at: number, s: string): void => {
    for (let i = 0; i < s.length; i++) view.setUint8(at + i, s.charCodeAt(i));
  };
  text(0, 'RIFF');
  view.setUint32(4, 36 + frames * chs * 2, true);
  text(8, 'WAVE');
  text(12, 'fmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, chs, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * chs * 2, true);
  view.setUint16(32, chs * 2, true);
  view.setUint16(34, 16, true);
  text(36, 'data');
  view.setUint32(40, frames * chs * 2, true);
  let at = 44;
  for (let i = 0; i < frames; i++) {
    for (const ch of channels) {
      const v = Math.max(-1, Math.min(1, ch[i] ?? 0));
      view.setInt16(at, Math.round(v < 0 ? v * 0x8000 : v * 0x7fff), true);
      at += 2;
    }
  }
  return bytes;
}
