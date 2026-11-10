import { describe, expect, it } from 'vitest';
import {
  coverCrop,
  encodeWithinCap,
  MAX_THUMB_CHARS,
  MAX_THUMBS,
  THUMBS_KEY,
  WorldThumbs,
} from './worldThumbs';

class MemoryStore {
  readonly data = new Map<string, string>();
  getItem(key: string): string | null {
    return this.data.get(key) ?? null;
  }
  setItem(key: string, value: string): void {
    this.data.set(key, value);
  }
}

const image = (n = 100) => `data:image/webp;base64,${'A'.repeat(n)}`;

describe('world thumbnails', () => {
  it('round-trips a thumbnail by world id, and forgets a deleted world', () => {
    const store = new MemoryStore();
    const thumbs = new WorldThumbs(store);
    expect(thumbs.get('wabc')).toBeNull();
    expect(thumbs.put('wabc', image(), 1)).toBe(true);
    expect(new WorldThumbs(store).get('wabc')).toBe(image());
    thumbs.remove('wabc');
    expect(thumbs.get('wabc')).toBeNull();
  });

  it('refuses a picture over the size cap or not a picture', () => {
    const thumbs = new WorldThumbs(new MemoryStore());
    expect(thumbs.put('w', image(MAX_THUMB_CHARS), 1)).toBe(false);
    expect(thumbs.put('w', 'data:text/html;base64,AAAA', 1)).toBe(false);
    expect(thumbs.get('w')).toBeNull();
  });

  it('keeps fields and records it does not know when it rewrites the store (append-only)', () => {
    const store = new MemoryStore();
    store.setItem(
      THUMBS_KEY,
      JSON.stringify({
        wabc: { image: image(), at: 1, focus: [0.5, 0.4] },
        wnew: { picture: 'from a newer build' },
      }),
    );
    const thumbs = new WorldThumbs(store);
    thumbs.put('wabc', image(200), 2);
    thumbs.put('wother', image(), 3);
    const raw = JSON.parse(store.getItem(THUMBS_KEY) ?? '{}') as Record<string, unknown>;
    expect(raw.wabc).toEqual({ image: image(200), at: 2, focus: [0.5, 0.4] });
    expect(raw.wnew).toEqual({ picture: 'from a newer build' });
    expect(raw.wother).toEqual({ image: image(), at: 3 });
  });

  it('keeps at most MAX_THUMBS, dropping the oldest captured', () => {
    const store = new MemoryStore();
    const thumbs = new WorldThumbs(store);
    for (let i = 0; i <= MAX_THUMBS; i++) thumbs.put(`w${String(i)}`, image(), i);
    expect(thumbs.get('w0')).toBeNull();
    expect(thumbs.get('w1')).toBe(image());
    expect(thumbs.get(`w${String(MAX_THUMBS)}`)).toBe(image());
  });

  it('reads a damaged store as empty, and works without storage', () => {
    const store = new MemoryStore();
    store.setItem(THUMBS_KEY, 'not json');
    expect(new WorldThumbs(store).get('w')).toBeNull();
    const none = new WorldThumbs(null);
    expect(none.put('w', image(), 1)).toBe(false);
    expect(none.get('w')).toBeNull();
  });

  it('does not throw when storage is full', () => {
    const full = {
      getItem: () => null,
      setItem: () => {
        throw new Error('QuotaExceededError');
      },
    };
    expect(new WorldThumbs(full).put('w', image(), 1)).toBe(false);
  });
});

describe('capturing a thumbnail', () => {
  it('crops the view to 16:9 from its centre', () => {
    expect(coverCrop(1600, 900, 320, 180)).toEqual({ x: 0, y: 0, w: 1600, h: 900 });
    expect(coverCrop(1000, 1000, 320, 180)).toEqual({ x: 0, y: 218.75, w: 1000, h: 562.5 });
    expect(coverCrop(390, 844, 320, 180)).toMatchObject({ x: 0, w: 390 });
  });

  it('lowers the quality until it fits the cap, and falls back to JPEG without WebP', () => {
    const big = (type: string, q: number) =>
      `data:${type};base64,${'A'.repeat(q > 0.5 ? MAX_THUMB_CHARS : 10)}`;
    expect(encodeWithinCap(big)).toBe(`data:image/webp;base64,${'A'.repeat(10)}`);
    // A browser that can't encode WebP hands back PNG.
    const noWebp = (type: string) =>
      type === 'image/webp' ? 'data:image/png;base64,AAAA' : 'data:image/jpeg;base64,AAAA';
    expect(encodeWithinCap(noWebp)).toBe('data:image/jpeg;base64,AAAA');
    expect(
      encodeWithinCap((type) => `data:${type};base64,${'A'.repeat(MAX_THUMB_CHARS)}`),
    ).toBeNull();
  });
});
