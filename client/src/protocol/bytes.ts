// Little-endian byte writer/reader for protocol messages (ARCHITECTURE.md §8.2).

const encoder = new TextEncoder();
const decoder = new TextDecoder('utf-8', { fatal: true });

export class ByteWriter {
  private buf = new Uint8Array(64);
  private view = new DataView(this.buf.buffer);
  private len = 0;

  private reserve(n: number): void {
    if (this.len + n <= this.buf.length) return;
    let size = this.buf.length * 2;
    while (size < this.len + n) size *= 2;
    const next = new Uint8Array(size);
    next.set(this.buf.subarray(0, this.len));
    this.buf = next;
    this.view = new DataView(next.buffer);
  }

  u8(v: number): void {
    this.reserve(1);
    this.view.setUint8(this.len, v);
    this.len += 1;
  }
  u16(v: number): void {
    this.reserve(2);
    this.view.setUint16(this.len, v, true);
    this.len += 2;
  }
  u32(v: number): void {
    this.reserve(4);
    this.view.setUint32(this.len, v, true);
    this.len += 4;
  }
  u64(v: bigint): void {
    this.reserve(8);
    this.view.setBigUint64(this.len, v, true);
    this.len += 8;
  }
  f64(v: number): void {
    this.reserve(8);
    this.view.setFloat64(this.len, v, true);
    this.len += 8;
  }
  bytes(b: Uint8Array): void {
    this.reserve(b.length);
    this.buf.set(b, this.len);
    this.len += b.length;
  }
  /** u16 byte length + UTF-8, truncated to `maxBytes` on a code-point boundary. */
  str(s: string, maxBytes: number): void {
    const b = clampUtf8(encoder.encode(s), maxBytes);
    this.u16(b.length);
    this.bytes(b);
  }

  finish(): Uint8Array {
    return this.buf.slice(0, this.len);
  }
}

function clampUtf8(b: Uint8Array, maxBytes: number): Uint8Array {
  if (b.length <= maxBytes) return b;
  let end = maxBytes;
  while (end > 0 && ((b[end] ?? 0) & 0xc0) === 0x80) end--;
  return b.subarray(0, end);
}

export class DecodeError extends Error {
  override name = 'DecodeError';
}

/** Bounds-checked reader; throws DecodeError on any malformed input. */
export class ByteReader {
  private readonly view: DataView;
  private pos = 0;

  constructor(private readonly buf: Uint8Array) {
    this.view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  }

  private need(n: number): void {
    if (this.buf.length - this.pos < n) throw new DecodeError('truncated message');
  }

  u8(): number {
    this.need(1);
    return this.view.getUint8(this.pos++);
  }
  u16(): number {
    this.need(2);
    const v = this.view.getUint16(this.pos, true);
    this.pos += 2;
    return v;
  }
  u32(): number {
    this.need(4);
    const v = this.view.getUint32(this.pos, true);
    this.pos += 4;
    return v;
  }
  u64(): bigint {
    this.need(8);
    const v = this.view.getBigUint64(this.pos, true);
    this.pos += 8;
    return v;
  }
  f64(): number {
    this.need(8);
    const v = this.view.getFloat64(this.pos, true);
    this.pos += 8;
    return v;
  }
  fixed(n: number): Uint8Array {
    this.need(n);
    const out = this.buf.slice(this.pos, this.pos + n);
    this.pos += n;
    return out;
  }
  str(maxBytes: number): string {
    const len = this.u16();
    if (len > maxBytes) throw new DecodeError('string too long');
    const bytes = this.fixed(len);
    try {
      return decoder.decode(bytes);
    } catch {
      throw new DecodeError('invalid UTF-8');
    }
  }
  end(): void {
    if (this.pos !== this.buf.length) throw new DecodeError('trailing bytes');
  }
}
