// Reliable-channel framing (ARCHITECTURE.md §8.2): a stream starts with a one-byte channel id,
// then carries messages as u32 little-endian length ‖ payload.

export class FrameError extends Error {
  override name = 'FrameError';
}

export function frame(payload: Uint8Array): Uint8Array {
  const out = new Uint8Array(4 + payload.length);
  new DataView(out.buffer).setUint32(0, payload.length, true);
  out.set(payload, 4);
  return out;
}

/** Reassembles length-prefixed frames from arbitrary stream chunks. */
export class FrameReader {
  private buf = new Uint8Array(0);

  constructor(private readonly maxFrameBytes: number) {}

  /** Adds a chunk and returns every frame it completed. Throws FrameError on oversize frames. */
  push(chunk: Uint8Array): Uint8Array[] {
    const merged = new Uint8Array(this.buf.length + chunk.length);
    merged.set(this.buf);
    merged.set(chunk, this.buf.length);
    const frames: Uint8Array[] = [];
    let pos = 0;
    while (merged.length - pos >= 4) {
      const len = new DataView(merged.buffer, pos, 4).getUint32(0, true);
      if (len > this.maxFrameBytes) throw new FrameError(`frame of ${String(len)} bytes`);
      if (merged.length - pos - 4 < len) break;
      frames.push(merged.slice(pos + 4, pos + 4 + len));
      pos += 4 + len;
    }
    this.buf = merged.slice(pos);
    return frames;
  }
}
