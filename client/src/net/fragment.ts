// Fragmentation of reliable messages on WebRTC data channels (ARCHITECTURE.md §8.2), the same
// format as the Rust transports' (crates/dwell-protocol/src/fragment.rs). A message the link's
// max-message-size can carry is sent unchanged; a larger one goes as fragments, back to back on
// its reliable, ordered channel: WEBRTC_FRAGMENT_TAG ‖ u32 little-endian total length ‖ the next
// bytes. No message type is the tag (shared/protocol/gen.mjs checks it), so the receiver tells
// the two apart. Fragments are at most MAX_WEBRTC_MESSAGE_BYTES on every link.
import { MAX_WEBRTC_MESSAGE_BYTES, WEBRTC_FRAGMENT_TAG } from '../protocol/constants.gen';

const HEADER_BYTES = 5;

/** The SCTP messages (copies) that carry `message` to a peer accepting `maxSctpBytes` each. */
export function fragment(message: Uint8Array, maxSctpBytes: number): Uint8Array<ArrayBuffer>[] {
  if (message.length <= maxSctpBytes) return [message.slice()];
  const body = Math.max(1, Math.min(maxSctpBytes, MAX_WEBRTC_MESSAGE_BYTES) - HEADER_BYTES);
  const out: Uint8Array<ArrayBuffer>[] = [];
  for (let at = 0; at < message.length; at += body) {
    const part = message.subarray(at, Math.min(at + body, message.length));
    const f = new Uint8Array(HEADER_BYTES + part.length);
    f[0] = WEBRTC_FRAGMENT_TAG;
    new DataView(f.buffer).setUint32(1, message.length, true);
    f.set(part, HEADER_BYTES);
    out.push(f);
  }
  return out;
}

/** The largest SCTP message `pc`'s peer accepts, or 64 KiB before that is known. */
export function maxMessageSize(pc: RTCPeerConnection): number {
  const n = pc.sctp?.maxMessageSize;
  return typeof n === 'number' && n >= 1024 ? n : MAX_WEBRTC_MESSAGE_BYTES;
}

/** Thrown for an oversize or malformed message: the channel can't be read past it. */
export class FragmentError extends Error {}

/** Reassembles one reliable channel's SCTP messages into protocol messages. */
export class Reassembler {
  private total = 0;
  private buf: Uint8Array | null = null;
  private filled = 0;

  constructor(private readonly maxBytes: number) {}

  /** One SCTP message in: a whole message out, or null while a fragmented one is arriving. */
  push(sctp: Uint8Array): Uint8Array | null {
    if (sctp[0] !== WEBRTC_FRAGMENT_TAG) {
      if (this.buf) throw new FragmentError('a message inside a fragmented one');
      if (sctp.length > this.maxBytes) throw new FragmentError('message too large');
      return sctp;
    }
    if (sctp.length <= HEADER_BYTES) throw new FragmentError('empty fragment');
    const total = new DataView(sctp.buffer, sctp.byteOffset, sctp.byteLength).getUint32(1, true);
    const part = sctp.subarray(HEADER_BYTES);
    if (!this.buf) {
      if (total > this.maxBytes) throw new FragmentError('message too large');
      this.total = total;
      this.buf = new Uint8Array(total);
      this.filled = 0;
    } else if (total !== this.total) {
      throw new FragmentError('a fragment of another message');
    }
    if (this.filled + part.length > this.total) throw new FragmentError('fragment past its total');
    this.buf.set(part, this.filled);
    this.filled += part.length;
    if (this.filled < this.total) return null;
    const done = this.buf;
    this.buf = null;
    return done;
  }
}
