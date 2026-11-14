import { describe, expect, it } from 'vitest';
import {
  Jobs,
  OVERVIEW_CHUNK,
  OVERVIEW_PASSES,
  OverviewCache,
  chunks,
  pointAt,
  rectOn,
  type Frame,
} from './overview';

// The frame `ChiselPreview.overview_frame(512)` returns for the 50 km world radius (its Rust test
// checks it covers the disc): 512 texels of 196 m from -50 176 m.
const FRAME: Frame = { x0: -50_176, texel: 196, n: 512 };

describe('Chisel world overview', () => {
  it('maps a click to the world point under it, and the detail region back onto it', () => {
    expect(pointAt(FRAME, 0.5, 0.5)).toEqual({ x: 0, z: 0 });
    expect(pointAt(FRAME, 0, 0)).toEqual({ x: -50_176, z: -50_176 });
    expect(pointAt(FRAME, 1, 1)).toEqual({ x: 50_176, z: 50_176 });
    // A quarter across, three quarters down: x -25 088, z +25 088.
    expect(pointAt(FRAME, 0.25, 0.75)).toEqual({ x: -25_088, z: 25_088 });
    // The detail rectangle drawn around the clicked point is centred on the click.
    const p = pointAt(FRAME, 0.3, 0.6);
    const r = rectOn(FRAME, p.x, p.z, 2048);
    expect(r.u + r.w / 2).toBeCloseTo(0.3, 4);
    expect(r.v + r.w / 2).toBeCloseTo(0.6, 4);
    expect(r.w).toBeCloseTo(2048 / (512 * 196), 6);
  });

  it('splits each pass into chunks that tile the whole frame', () => {
    for (const n of OVERVIEW_PASSES) {
      const f = { ...FRAME, n, texel: Math.ceil(100_000 / n) };
      const cs = chunks(f);
      expect(cs.length).toBe((n / Math.min(n, OVERVIEW_CHUNK)) ** 2);
      const covered = cs.reduce((a, c) => a + c.n * c.n, 0);
      expect(covered).toBe(n * n);
      const last = cs[cs.length - 1];
      expect(last && last.x0 + last.n * f.texel).toBe(f.x0 + n * f.texel);
    }
    expect(OVERVIEW_PASSES[0]).toBeLessThan(OVERVIEW_PASSES[1] ?? 0);
  });

  it('runs the detail first and drops a lane’s older generation', () => {
    const q = new Jobs<{ lane: 'detail' | 'overview'; gen: number; tag: string }>();
    q.push({ lane: 'overview', gen: 1, tag: 'o1a' });
    q.push({ lane: 'overview', gen: 1, tag: 'o1b' });
    q.push({ lane: 'detail', gen: 1, tag: 'd1' });
    expect(q.next()?.tag).toBe('d1');
    expect(q.next()?.tag).toBe('o1a');
    // A newer edit: the old overview chunk is cancelled, the new one runs after the new detail.
    q.push({ lane: 'detail', gen: 3, tag: 'd3' });
    q.push({ lane: 'overview', gen: 4, tag: 'o4' });
    expect(q.next()?.tag).toBe('d3');
    expect(q.next()?.tag).toBe('o4');
    expect(q.next()).toBeUndefined();
  });

  it('runs the 3D section after the detail and before the overview, and cancels a stale build', () => {
    const q = new Jobs<{ lane: 'detail' | 'overview' | 'section'; gen: number; tag: string }>();
    q.push({ lane: 'overview', gen: 1, tag: 'o1' });
    q.push({ lane: 'section', gen: 1, tag: 's1 coarse' });
    q.push({ lane: 'section', gen: 1, tag: 's1 fine' });
    q.push({ lane: 'detail', gen: 1, tag: 'd1' });
    expect(q.next()?.tag).toBe('d1');
    expect(q.next()?.tag).toBe('s1 coarse');
    // An edit before the fine pass ran: the stale fine pass is dropped, the new coarse runs.
    q.push({ lane: 'section', gen: 2, tag: 's2 coarse' });
    expect(q.next()?.tag).toBe('s2 coarse');
    expect(q.next()?.tag).toBe('o1');
    expect(q.next()).toBeUndefined();
  });

  it('caches the overview by the edit state: an edit or overlay misses', () => {
    const c = new OverviewCache(4);
    const k = OverviewCache.key(1, '{"biomes":[]}', 1, false, 196, -50_176, -50_176, 128);
    c.set(k, new Uint8ClampedArray([1, 2, 3, 4]));
    expect(c.get(k)).toEqual(new Uint8ClampedArray([1, 2, 3, 4]));
    expect(
      c.get(OverviewCache.key(1, '{"biomes":[1]}', 1, false, 196, -50_176, -50_176, 128)),
    ).toBe(undefined);
    expect(c.get(OverviewCache.key(1, '{"biomes":[]}', 0, false, 196, -50_176, -50_176, 128))).toBe(
      undefined,
    );
    expect(c.get(OverviewCache.key(2, '{"biomes":[]}', 1, false, 196, -50_176, -50_176, 128))).toBe(
      undefined,
    );
    // The relief shade is part of the state.
    expect(c.get(OverviewCache.key(1, '{"biomes":[]}', 1, true, 196, -50_176, -50_176, 128))).toBe(
      undefined,
    );
    for (let i = 0; i < 5; i++) c.set(`k${String(i)}`, new Uint8ClampedArray(1));
    expect(c.size).toBe(4);
    expect(c.get(k)).toBeUndefined();
  });
});
