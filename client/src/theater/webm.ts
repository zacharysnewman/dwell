// A small WebM writer for theater mode's frame-exact takes (docs/THEATER_MODE.md §6): the frames
// WebCodecs' VideoEncoder made, each at its own timestamp (frame i at i / fps), in one video track.
// Built whole at the end of the take (its sizes known), so any player and ffmpeg read it. Pure: a
// test reads its elements back.

/** One encoded frame: its time (µs), whether it is a key frame, its bytes. */
export interface WebmFrame {
  timestampUs: number;
  key: boolean;
  data: Uint8Array<ArrayBuffer>;
}

export interface WebmTrack {
  /** The Matroska codec id: `V_VP8` or `V_VP9`. */
  codec: 'V_VP8' | 'V_VP9';
  width: number;
  height: number;
}

/** The element ids used (Matroska's). */
export const ID = {
  EBML: 0x1a45dfa3,
  EBMLVersion: 0x4286,
  EBMLReadVersion: 0x42f7,
  EBMLMaxIDLength: 0x42f2,
  EBMLMaxSizeLength: 0x42f3,
  DocType: 0x4282,
  DocTypeVersion: 0x4287,
  DocTypeReadVersion: 0x4285,
  Segment: 0x18538067,
  Info: 0x1549a966,
  TimecodeScale: 0x2ad7b1,
  Duration: 0x4489,
  MuxingApp: 0x4d80,
  WritingApp: 0x5741,
  Tracks: 0x1654ae6b,
  TrackEntry: 0xae,
  TrackNumber: 0xd7,
  TrackUID: 0x73c5,
  TrackType: 0x83,
  CodecID: 0x86,
  Video: 0xe0,
  PixelWidth: 0xb0,
  PixelHeight: 0xba,
  Cluster: 0x1f43b675,
  Timecode: 0xe7,
  SimpleBlock: 0xa3,
} as const;

type Part = Uint8Array<ArrayBuffer>;

function idBytes(id: number): Part {
  const out: number[] = [];
  for (let v = id; v > 0; v = Math.floor(v / 256)) out.unshift(v & 0xff);
  return Uint8Array.from(out);
}

/** An element size as an EBML variable-length integer (the shortest that holds it). */
export function vint(n: number): Part {
  for (let len = 1; len <= 8; len++) {
    if (n < 2 ** (7 * len) - 1) {
      const out = new Uint8Array(len);
      let v = n;
      for (let i = len - 1; i >= 0; i--) {
        out[i] = v & 0xff;
        v = Math.floor(v / 256);
      }
      out[0] = (out[0] ?? 0) | (0x80 >> (len - 1));
      return out;
    }
  }
  throw new Error(`webm: ${String(n)} bytes is too big`);
}

function concat(parts: readonly Part[]): Part {
  const out = new Uint8Array(parts.reduce((s, p) => s + p.length, 0));
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

function element(id: number, ...body: Part[]): Part {
  const data = concat(body);
  return concat([idBytes(id), vint(data.length), data]);
}

function uint(id: number, v: number): Part {
  const out: number[] = [];
  for (let x = v; x > 0 || out.length === 0; x = Math.floor(x / 256)) out.unshift(x & 0xff);
  return element(id, Uint8Array.from(out));
}

function str(id: number, s: string): Part {
  return element(id, new TextEncoder().encode(s));
}

function float(id: number, v: number): Part {
  const b = new Uint8Array(8);
  new DataView(b.buffer).setFloat64(0, v);
  return element(id, b);
}

/** The longest a cluster runs (ms): a block's time is an int16 from its cluster's. */
const CLUSTER_MS = 30_000;

/** The WebM file of `frames` (in order) in one video track; `durationUs` the take's length. */
export function muxWebm(track: WebmTrack, frames: readonly WebmFrame[], durationUs: number): Part {
  const header = element(
    ID.EBML,
    uint(ID.EBMLVersion, 1),
    uint(ID.EBMLReadVersion, 1),
    uint(ID.EBMLMaxIDLength, 4),
    uint(ID.EBMLMaxSizeLength, 8),
    str(ID.DocType, 'webm'),
    uint(ID.DocTypeVersion, 2),
    uint(ID.DocTypeReadVersion, 2),
  );
  const info = element(
    ID.Info,
    uint(ID.TimecodeScale, 1_000_000),
    float(ID.Duration, durationUs / 1000),
    str(ID.MuxingApp, 'dwell'),
    str(ID.WritingApp, 'dwell theater'),
  );
  const tracks = element(
    ID.Tracks,
    element(
      ID.TrackEntry,
      uint(ID.TrackNumber, 1),
      uint(ID.TrackUID, 1),
      uint(ID.TrackType, 1),
      str(ID.CodecID, track.codec),
      element(ID.Video, uint(ID.PixelWidth, track.width), uint(ID.PixelHeight, track.height)),
    ),
  );
  const clusters: Part[] = [];
  let blocks: Part[] = [];
  let clusterMs = 0;
  const flush = (): void => {
    if (blocks.length > 0)
      clusters.push(element(ID.Cluster, uint(ID.Timecode, clusterMs), ...blocks));
    blocks = [];
  };
  for (const f of frames) {
    const ms = Math.round(f.timestampUs / 1000);
    // A new cluster at each key frame (seeking) or before the block's time would overflow.
    if (blocks.length === 0 || f.key || ms - clusterMs >= CLUSTER_MS) {
      flush();
      clusterMs = ms;
    }
    const rel = ms - clusterMs;
    const head = Uint8Array.from([0x81, (rel >> 8) & 0xff, rel & 0xff, f.key ? 0x80 : 0]);
    blocks.push(element(ID.SimpleBlock, head, f.data));
  }
  flush();
  return concat([header, element(ID.Segment, info, tracks, ...clusters)]);
}

/** An element read back (tests): its id, its body's offset and size. */
export interface ReadElement {
  id: number;
  start: number;
  size: number;
}

/** Reads the elements in `b[from, to)` (one level; tests use it to walk a file). */
export function readElements(b: Uint8Array, from = 0, to = b.length): ReadElement[] {
  const out: ReadElement[] = [];
  let at = from;
  const readVint = (keepMarker: boolean): number => {
    const first = b[at] ?? 0;
    let len = 1;
    while (len <= 8 && !(first & (0x80 >> (len - 1)))) len++;
    let v = keepMarker ? first : first & (0xff >> len);
    for (let i = 1; i < len; i++) v = v * 256 + (b[at + i] ?? 0);
    at += len;
    return v;
  };
  while (at < to) {
    const id = readVint(true);
    const size = readVint(false);
    out.push({ id, start: at, size });
    at += size;
  }
  return out;
}
