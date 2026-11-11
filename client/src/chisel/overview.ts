// Chisel's world overview (ADR 0074 B3): the whole disc, island and sea, drawn coarse-first beside
// the detail preview, from the same edit and overlay. The pure parts live here (tested in
// overview.test.ts): the frame's texels to world points and back, the passes split into chunks,
// the worker's job queue (the detail first, stale generations dropped) and the overview's cache.

/** The overview's frame: `n` texels a side of `texel` m from face-local (x0, x0) (the whole
 * disc inside; `ChiselPreview.overview_frame`, `dwell-client` `chisel::overview_frame`). */
export interface Frame {
  x0: number;
  texel: number;
  n: number;
}

/** The overview's passes, texels a side: coarse first, then refined. */
export const OVERVIEW_PASSES = [128, 512];
/** The texels a side of one overview job: a pass is split into chunks so a newer edit or a detail
 * tile waits at most one chunk. */
export const OVERVIEW_CHUNK = 128;

/** The world point (face-local x, z) under (u, v), the fraction across and down the overview. */
export function pointAt(f: Frame, u: number, v: number): { x: number; z: number } {
  const span = f.n * f.texel;
  return { x: Math.round(f.x0 + u * span), z: Math.round(f.x0 + v * span) };
}

/** The detail region (centre cx, cz; `size` m a side) on the overview, as fractions. */
export function rectOn(
  f: Frame,
  cx: number,
  cz: number,
  size: number,
): { u: number; v: number; w: number } {
  const span = f.n * f.texel;
  return { u: (cx - size / 2 - f.x0) / span, v: (cz - size / 2 - f.x0) / span, w: size / span };
}

/** A pass's chunks: their origin (world m) and their texel offset in the frame. */
export function chunks(
  f: Frame,
  chunk = OVERVIEW_CHUNK,
): { x0: number; z0: number; i: number; j: number; n: number }[] {
  const out = [];
  const n = Math.min(chunk, f.n);
  for (let j = 0; j < f.n; j += n) {
    for (let i = 0; i < f.n; i += n) {
      out.push({ x0: f.x0 + i * f.texel, z0: f.x0 + j * f.texel, i, j, n });
    }
  }
  return out;
}

export type Lane = 'detail' | 'overview' | 'section';

/** The lanes in the order they run: the detail first, then the 3D section, then the overview. */
const LANE_ORDER: readonly Lane[] = ['detail', 'section', 'overview'];

/** The worker's jobs: a newer generation of a lane drops that lane's older jobs, and the detail
 * lane runs before the 3D section's, which runs before the overview's. */
export class Jobs<T extends { lane: Lane; gen: number }> {
  private q: T[] = [];
  private latest: Record<Lane, number> = { detail: 0, overview: 0, section: 0 };
  push(j: T) {
    this.latest[j.lane] = Math.max(this.latest[j.lane], j.gen);
    this.q.push(j);
  }
  next(): T | undefined {
    this.q = this.q.filter((j) => j.gen >= this.latest[j.lane]);
    for (const lane of LANE_ORDER) {
      const d = this.q.findIndex((j) => j.lane === lane);
      if (d >= 0) return this.q.splice(d, 1)[0];
    }
    return undefined;
  }
  get size() {
    return this.q.length;
  }
}

/** The overview's chunks by the edit state (seed, edit, overlay) and their place: an edit misses. */
export class OverviewCache {
  private m = new Map<string, Uint8ClampedArray>();
  constructor(private limit = 96) {}
  static key(
    seed: number,
    edit: string,
    overlay: number,
    relief: boolean,
    texel: number,
    x0: number,
    z0: number,
    n: number,
  ) {
    return `${String(seed)}|${String(overlay)}${relief ? 'r' : ''}|${String(texel)}|${String(x0)},${String(z0)}|${String(n)}|${edit}`;
  }
  get(k: string): Uint8ClampedArray | undefined {
    const v = this.m.get(k);
    if (v) {
      this.m.delete(k);
      this.m.set(k, v);
    }
    return v;
  }
  set(k: string, v: Uint8ClampedArray) {
    this.m.delete(k);
    this.m.set(k, v);
    while (this.m.size > this.limit) {
      const first = this.m.keys().next().value;
      if (first === undefined) break;
      this.m.delete(first);
    }
  }
  get size() {
    return this.m.size;
  }
}
