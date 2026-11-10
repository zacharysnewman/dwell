// Each local world's thumbnail (the main menu's world cards, ARCHITECTURE.md §2.1): a small
// picture of the world's last view, captured from the game's canvas while it plays and when the
// page is hidden or closed, kept in local storage (`dwell.worldThumbs`) by world id.
//
// Like the world index, the store is shared by every app version at the origin (RELEASES.md §6),
// so its format is append-only: a build keeps the fields of a record it does not know and the
// records it cannot read when it rewrites the store.
import type { KeyValueStore } from './worldIndex';

export const THUMBS_KEY = 'dwell.worldThumbs';
/** The thumbnail's size in pixels: 16:9, a card's width at 1× and 2× on phones. */
export const THUMB_WIDTH = 320;
export const THUMB_HEIGHT = 180;
/** The largest data URL kept, in characters: about 20 KB of image once base64 is undone. */
export const MAX_THUMB_CHARS = 28_000;
/** The most thumbnails kept; past it the oldest captured goes. */
export const MAX_THUMBS = 48;
/** How often a playing world captures its view, besides when the page is hidden. */
export const CAPTURE_INTERVAL_MS = 30_000;

interface ThumbRecord {
  image: string;
  at: number;
}

function isRecord(v: unknown): v is ThumbRecord {
  if (typeof v !== 'object' || v === null) return false;
  const r = v as Record<string, unknown>;
  return typeof r.image === 'string' && isThumbImage(r.image) && typeof r.at === 'number';
}

/** Whether a string is a picture the menu can show: a JPEG, WebP or PNG data URL within the cap. */
export function isThumbImage(image: string): boolean {
  return /^data:image\/(jpeg|webp|png);base64,/.test(image) && image.length <= MAX_THUMB_CHARS;
}

export class WorldThumbs {
  constructor(private readonly store: KeyValueStore | null) {}

  /** Every stored entry as parsed, readable or not. */
  private entries(): Record<string, unknown> {
    try {
      const raw: unknown = JSON.parse(this.store?.getItem(THUMBS_KEY) ?? '{}');
      return typeof raw === 'object' && raw !== null && !Array.isArray(raw)
        ? (raw as Record<string, unknown>)
        : {};
    } catch {
      return {};
    }
  }

  private write(entries: Record<string, unknown>): boolean {
    try {
      this.store?.setItem(THUMBS_KEY, JSON.stringify(entries));
      return this.store !== null;
    } catch {
      return false; // full or blocked: the card shows its placeholder
    }
  }

  /** A world's thumbnail as a data URL, or null for none. */
  get(id: string): string | null {
    const r = this.entries()[id];
    return isRecord(r) ? r.image : null;
  }

  /**
   * Keeps a world's thumbnail, captured at `at`: false if the image is over the cap or not one, or
   * storage refused it. Past `MAX_THUMBS` the oldest readable thumbnails of other worlds go.
   */
  put(id: string, image: string, at: number): boolean {
    if (!isThumbImage(image)) return false;
    const entries = this.entries();
    const stored = entries[id];
    entries[id] =
      typeof stored === 'object' && stored !== null && !Array.isArray(stored)
        ? { ...stored, image, at }
        : { image, at };
    const readable = Object.entries(entries)
      .filter((e): e is [string, ThumbRecord] => isRecord(e[1]))
      .sort((a, b) => b[1].at - a[1].at);
    for (const [old] of readable.slice(MAX_THUMBS)) {
      if (old !== id) Reflect.deleteProperty(entries, old);
    }
    return this.write(entries);
  }

  /** Forgets a deleted world's thumbnail. */
  remove(id: string): void {
    const entries = this.entries();
    if (!(id in entries)) return;
    Reflect.deleteProperty(entries, id);
    this.write(entries);
  }
}

/** The source rectangle that fills a `dw`×`dh` picture from a `sw`×`sh` one, centred (cover). */
export function coverCrop(
  sw: number,
  sh: number,
  dw: number,
  dh: number,
): { x: number; y: number; w: number; h: number } {
  const scale = Math.max(dw / sw, dh / sh);
  const w = dw / scale;
  const h = dh / scale;
  return { x: (sw - w) / 2, y: (sh - h) / 2, w, h };
}

/** Qualities tried in turn until the picture fits the cap. */
const QUALITIES = [0.72, 0.55, 0.4, 0.25];

/**
 * Encodes a picture within the cap: WebP where the browser encodes it (a browser that can't hands
 * back PNG), JPEG otherwise, at the best quality that fits; null if none does.
 */
export function encodeWithinCap(encode: (type: string, quality: number) => string): string | null {
  for (const type of ['image/webp', 'image/jpeg']) {
    for (const q of QUALITIES) {
      const url = encode(type, q);
      if (!url.startsWith(`data:${type};`)) break; // this type isn't encoded here
      if (url.length <= MAX_THUMB_CHARS) return url;
    }
  }
  return null;
}

/** A thumbnail of the canvas's last drawn frame, or null (nothing drawn, or it can't be read). */
export function captureThumbnail(canvas: HTMLCanvasElement): string | null {
  if (canvas.width === 0 || canvas.height === 0) return null;
  const out = document.createElement('canvas');
  out.width = THUMB_WIDTH;
  out.height = THUMB_HEIGHT;
  const ctx = out.getContext('2d');
  if (!ctx) return null;
  const c = coverCrop(canvas.width, canvas.height, THUMB_WIDTH, THUMB_HEIGHT);
  try {
    ctx.drawImage(canvas, c.x, c.y, c.w, c.h, 0, 0, THUMB_WIDTH, THUMB_HEIGHT);
    return encodeWithinCap((type, q) => out.toDataURL(type, q));
  } catch {
    return null;
  }
}

/** A running capture: `stop` ends it; `captureNow` reads the next drawn frame into the store. */
export interface ThumbnailCapture {
  stop: () => void;
  /**
   * The player's view now, as the world's picture (owner, 2026-11-10: "a screenshot of the last
   * player view when you quit"): resolves once the next frame is read, or after `timeoutMs` if no
   * frame comes (a hidden page), so a quit never waits on it.
   */
  captureNow: (timeoutMs?: number) => Promise<void>;
}

/**
 * Captures a playing world's view into the store now and then, when the page is hidden or closed
 * (when the world saves too), and on demand (`captureNow`, on Quit to main menu).
 */
export function startThumbnailCapture(
  canvas: HTMLCanvasElement,
  thumbs: WorldThumbs,
  id: string,
  now: () => number = Date.now,
): ThumbnailCapture {
  const read = () => {
    const image = captureThumbnail(canvas);
    if (image) thumbs.put(id, image, now());
  };
  const capture = () => {
    // The next frame's callback runs once it is drawn and shown: read it then.
    requestAnimationFrame(read);
  };
  const hidden = () => {
    // Hidden: no frame comes, so read the last one shown now.
    const image = captureThumbnail(canvas);
    if (image) thumbs.put(id, image, now());
  };
  const onVisibility = () => {
    if (document.visibilityState === 'hidden') hidden();
  };
  const timer = window.setInterval(capture, CAPTURE_INTERVAL_MS);
  window.setTimeout(capture, 5000); // a first picture soon after the join
  window.addEventListener('pagehide', hidden);
  document.addEventListener('visibilitychange', onVisibility);
  return {
    stop: () => {
      window.clearInterval(timer);
      window.removeEventListener('pagehide', hidden);
      document.removeEventListener('visibilitychange', onVisibility);
    },
    captureNow: (timeoutMs = 500) =>
      new Promise<void>((resolve) => {
        const done = window.setTimeout(resolve, timeoutMs);
        requestAnimationFrame(() => {
          read();
          window.clearTimeout(done);
          resolve();
        });
      }),
  };
}
