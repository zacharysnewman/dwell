import { describe, expect, it } from 'vitest';
import { frame, FrameReader } from './framing';

describe('framing', () => {
  it('reassembles frames split across chunks', () => {
    const bytes = new Uint8Array([
      ...frame(new Uint8Array([1, 2, 3])),
      ...frame(new Uint8Array([])),
    ]);
    const reader = new FrameReader(1024);
    const out = [
      ...reader.push(bytes.subarray(0, 2)),
      ...reader.push(bytes.subarray(2, 6)),
      ...reader.push(bytes.subarray(6)),
    ];
    expect(out).toEqual([new Uint8Array([1, 2, 3]), new Uint8Array([])]);
  });

  it('rejects oversize frames', () => {
    const reader = new FrameReader(2);
    expect(() => reader.push(frame(new Uint8Array(3)))).toThrow();
  });
});
