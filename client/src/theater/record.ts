// Theater mode's recording (docs/THEATER_MODE.md §6, T-3): MediaRecorder on the game canvas's
// stream, the HUD off, the canvas resized to the take's aspect and height (not cropped after, so
// the LOD selects for the real view), downloaded at the end as WebM or MP4, whichever the browser
// records.

import type { Aspect } from './config';

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

/** Starts recording `canvas` at `fps` (the browser must have MediaRecorder). */
export function startTake(
  canvas: HTMLCanvasElement,
  fps: number,
  mimeTypes: readonly string[],
  bitsPerSecond: number,
): Take {
  const stream = canvas.captureStream(fps);
  const mime = pickMime(mimeTypes, (t) => MediaRecorder.isTypeSupported(t));
  const rec = new MediaRecorder(stream, {
    ...(mime ? { mimeType: mime } : {}),
    videoBitsPerSecond: bitsPerSecond,
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
          for (const t of stream.getTracks()) t.stop();
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
