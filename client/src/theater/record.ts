// Theater mode's recording (docs/THEATER_MODE.md §6, T-3): MediaRecorder on the game canvas's
// stream, the HUD off, the canvas resized to the take's aspect and height (not cropped after, so
// the LOD selects for the real view), downloaded at the end as WebM or MP4, whichever the browser
// records.

import type { Aspect } from './config';
import { type WebmFrame, muxWebm } from './webm';

/** The canvas size for a take: `height` px tall at `aspect`, both sides even (encoders need it). */
export function recordSize(aspect: Aspect, height: number): { width: number; height: number } {
  const even = (v: number): number => Math.max(2, 2 * Math.round(v / 2));
  return { width: even((height * aspect.w) / aspect.h), height: even(height) };
}

/**
 * How the canvas is shown on screen while it renders at the take's size: scaled to fit the
 * window, centred (CSS px).
 */
export function fitOnScreen(
  size: { width: number; height: number },
  screen: { width: number; height: number },
): { width: number; height: number; left: number; top: number } {
  const k = Math.min(screen.width / size.width, screen.height / size.height);
  const width = size.width * k;
  const height = size.height * k;
  return { width, height, left: (screen.width - width) / 2, top: (screen.height - height) / 2 };
}

/** The first of `types` the browser records, or '' (the browser's default). */
export function pickMime(types: readonly string[], supported: (t: string) => boolean): string {
  return types.find((t) => supported(t)) ?? '';
}

/** The file extension for a recorder's MIME type. */
export function extensionOf(mime: string): 'mp4' | 'webm' {
  return mime.startsWith('video/mp4') ? 'mp4' : 'webm';
}

/** A take in progress: stop() resolves with the file. */
export interface Take {
  stop(): Promise<Blob>;
  readonly mime: string;
}

/** A take's sound: the mix's tap (`AudioEngine.tapStream`), its MIME types and bit rate. */
export interface TakeAudio {
  stream: MediaStream;
  mimeTypes: readonly string[];
  bitsPerSecond: number;
  /** Disconnects the tap when the take ends. */
  close(): void;
}

/** The tracks a take records: the canvas's video, and the tap's audio when the take has sound. */
export function takeTracks<T>(
  video: { getTracks(): T[] },
  audio: { getAudioTracks(): T[] } | null,
): T[] {
  return [...video.getTracks(), ...(audio ? audio.getAudioTracks() : [])];
}

/**
 * Starts recording `canvas` at `fps` (the browser must have MediaRecorder), with `audio`'s track
 * when given (the game's effects and ambience, never the music: docs/THEATER_MODE.md §6).
 */
export function startTake(
  canvas: HTMLCanvasElement,
  fps: number,
  mimeTypes: readonly string[],
  bitsPerSecond: number,
  audio: TakeAudio | null = null,
): Take {
  const video = canvas.captureStream(fps);
  const stream = new MediaStream(takeTracks(video, audio?.stream ?? null));
  const mime = pickMime(audio ? audio.mimeTypes : mimeTypes, (t) =>
    MediaRecorder.isTypeSupported(t),
  );
  const rec = new MediaRecorder(stream, {
    ...(mime ? { mimeType: mime } : {}),
    videoBitsPerSecond: bitsPerSecond,
    ...(audio ? { audioBitsPerSecond: audio.bitsPerSecond } : {}),
  });
  const chunks: Blob[] = [];
  rec.ondataavailable = (e) => {
    if (e.data.size > 0) chunks.push(e.data);
  };
  rec.start(1000);
  return {
    mime: mime || rec.mimeType,
    stop: () =>
      new Promise((resolve) => {
        rec.onstop = () => {
          for (const t of video.getTracks()) t.stop();
          audio?.close();
          resolve(new Blob(chunks, { type: mime || rec.mimeType || 'video/webm' }));
        };
        rec.stop();
      }),
  };
}

/** Offers `blob` as a download named `name`. */
export function download(doc: Document, blob: Blob, name: string): void {
  const url = URL.createObjectURL(blob);
  const a = doc.createElement('a');
  a.href = url;
  a.download = name;
  doc.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => {
    URL.revokeObjectURL(url);
  }, 10_000);
}

/**
 * A frame-exact take (docs/THEATER_MODE.md §6): the shell captures the canvas once per drawn
 * frame and stamps frame i at i / fps, whatever the page's real speed, so a slow machine still
 * makes a smooth video (the path moves one frame of the rate per captured frame,
 * `Client.theaterAdvance`). WebCodecs encodes, `muxWebm` writes the file. Silent: the game's sound
 * runs in real time and wouldn't match.
 */
export class ExactTake {
  readonly mime = 'video/webm';
  frames = 0;
  private readonly encoded: WebmFrame[] = [];
  private readonly encoder: VideoEncoder;
  private failed: Error | null = null;

  constructor(
    private readonly size: { width: number; height: number },
    private readonly fps: number,
    bitsPerSecond: number,
  ) {
    this.encoder = new VideoEncoder({
      output: (chunk) => {
        const data = new Uint8Array(chunk.byteLength);
        chunk.copyTo(data);
        this.encoded.push({ timestampUs: chunk.timestamp, key: chunk.type === 'key', data });
      },
      error: (e) => {
        this.failed = e instanceof Error ? e : new Error(String(e));
      },
    });
    this.encoder.configure({
      codec: 'vp8',
      width: size.width,
      height: size.height,
      bitrate: bitsPerSecond,
      framerate: fps,
    });
  }

  /** Whether WebCodecs can make one in this browser. */
  static supported(): boolean {
    return typeof VideoEncoder !== 'undefined' && typeof VideoFrame !== 'undefined';
  }

  /** Whether the encoder keeps up: the next frame may be captured (else the path waits). */
  get ready(): boolean {
    return this.failed === null && this.encoder.encodeQueueSize < 4;
  }

  /** Captures the canvas as the next frame. */
  capture(canvas: HTMLCanvasElement): void {
    const timestamp = exactTimestampUs(this.frames, this.fps);
    const frame = new VideoFrame(canvas, { timestamp, duration: Math.round(1e6 / this.fps) });
    this.encoder.encode(frame, { keyFrame: this.frames % Math.max(1, this.fps * 2) === 0 });
    frame.close();
    this.frames++;
  }

  async stop(): Promise<Blob> {
    await this.encoder.flush();
    this.encoder.close();
    if (this.failed) throw this.failed;
    const file = muxWebm(
      { codec: 'V_VP8', ...this.size },
      this.encoded,
      exactTimestampUs(this.frames, this.fps),
    );
    return new Blob([file], { type: this.mime });
  }
}

/** Frame `i`'s time in a frame-exact take at `fps` (µs). */
export function exactTimestampUs(i: number, fps: number): number {
  return Math.round((i * 1e6) / fps);
}
