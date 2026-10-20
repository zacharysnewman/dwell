import { describe, expect, it } from 'vitest';
import {
  MAX_RELIABLE_MESSAGE_BYTES,
  MAX_WEBRTC_MESSAGE_BYTES,
  MessageType,
  WEBRTC_FRAGMENT_TAG,
} from '../protocol/constants.gen';
import { FragmentError, Reassembler, fragment } from './fragment';

const message = (n: number): Uint8Array => {
  const m = new Uint8Array(n);
  for (let i = 0; i < n; i++) m[i] = (i * 7) % 256;
  return m;
};
/** The first index where `a` and `b` differ, or -1. `toEqual` on a 1 MiB array takes seconds. */
const firstDifference = (a: Uint8Array, b: Uint8Array): number => {
  const n = Math.min(a.length, b.length);
  for (let i = 0; i < n; i++) if (a[i] !== b[i]) return i;
  return a.length === b.length ? -1 : n;
};
const nth = <T>(a: T[], i: number): T => {
  const v = a[i];
  if (v === undefined) throw new Error(`no element ${String(i)}`);
  return v;
};

describe('WebRTC fragments', () => {
  it('use a first byte no message type has', () => {
    expect(Object.values(MessageType)).not.toContain(WEBRTC_FRAGMENT_TAG);
  });

  it('are not used for a message that fits', () => {
    const m = message(1000);
    expect(fragment(m, 1000)).toEqual([m]);
    expect(new Reassembler(MAX_RELIABLE_MESSAGE_BYTES).push(m)).toEqual(m);
  });

  it('carry a large message in pieces under the limit, as the Rust transports do', () => {
    const cases: [number, number][] = [
      [MAX_WEBRTC_MESSAGE_BYTES, MAX_WEBRTC_MESSAGE_BYTES + 1],
      [262_144, 300_000],
      [262_144, MAX_RELIABLE_MESSAGE_BYTES],
    ];
    for (const [link, n] of cases) {
      const m = message(n);
      const parts = fragment(m, link);
      expect(parts.length).toBe(Math.ceil(n / (MAX_WEBRTC_MESSAGE_BYTES - 5)));
      const r = new Reassembler(MAX_RELIABLE_MESSAGE_BYTES);
      const out = parts.map((p) => {
        expect(p.length).toBeLessThanOrEqual(MAX_WEBRTC_MESSAGE_BYTES);
        // Tag, then the whole message's length (u32 little-endian).
        expect([...p.subarray(0, 5)]).toEqual([255, n & 255, (n >> 8) & 255, (n >> 16) & 255, 0]);
        return r.push(p);
      });
      expect(out.slice(0, -1).every((o) => o === null)).toBe(true);
      const whole = out.at(-1);
      expect(whole).toBeInstanceOf(Uint8Array);
      expect(whole?.length).toBe(n);
      expect(firstDifference(whole ?? new Uint8Array(), m)).toBe(-1);
      expect(r.push(Uint8Array.of(17, 1))).toEqual(Uint8Array.of(17, 1));
    }
  });

  it('refuse oversize, interleaved and malformed input', () => {
    const r = () => new Reassembler(100_000);
    expect(() => r().push(message(100_001))).toThrow(FragmentError);
    expect(() => r().push(nth(fragment(message(100_001), 65_536), 0))).toThrow(FragmentError);
    const parts = fragment(message(90_000), 65_536);
    const a = r();
    a.push(nth(parts, 0));
    expect(() => a.push(Uint8Array.of(17))).toThrow(FragmentError);
    const b = r();
    b.push(nth(parts, 0));
    expect(() => b.push(nth(fragment(message(80_000), 65_536), 1))).toThrow(FragmentError);
    const c = r();
    c.push(nth(parts, 0));
    const long = new Uint8Array(nth(parts, 1).length + 10);
    long.set(nth(parts, 1));
    expect(() => c.push(long)).toThrow(FragmentError);
    expect(() => r().push(Uint8Array.of(255, 1, 0, 0, 0))).toThrow(FragmentError);
  });
});
