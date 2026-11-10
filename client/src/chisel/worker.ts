// Chisel's preview worker (ADR 0074 B3): the real generator through WASM (`ChiselPreview`), one
// world's face A built once per seed; each request puts the page's edit on it and rasterises a tile.
// Two lanes (overview.ts `Jobs`): the detail preview's tiles first, then the world overview's
// passes, split into chunks and cached by the edit state; a newer generation drops older jobs.
import { ChiselPreview, loadWasm } from '../engine/wasm';
import { Jobs, OVERVIEW_PASSES, OverviewCache, chunks, type Frame, type Lane } from './overview';

/** A detail tile, or (lane 'overview') the whole world's overview, expanded here into chunks. */
export interface TileRequest {
  id: number;
  lane: Lane;
  gen: number;
  seed: number;
  edit: string;
  x0: number;
  z0: number;
  n: number;
  texel: number;
  overlay: number;
}
interface Job extends TileRequest {
  /** An overview chunk's frame, pass and texel offset in it. */
  frame?: Frame;
  pass?: number;
  i?: number;
  j?: number;
  last?: boolean;
}
export type TileReply =
  | {
      id: number;
      lane: Lane;
      gen: number;
      rgba: Uint8ClampedArray;
      n: number;
      ms: number;
      frame?: Frame | undefined;
      pass?: number | undefined;
      i?: number | undefined;
      j?: number | undefined;
      last?: boolean | undefined;
      cached?: boolean;
    }
  | { id: number; lane: Lane; gen: number; error: string };

let preview: ChiselPreview | null = null;
let seed = -1;
let edit = '';
const jobs = new Jobs<Job>();
const cache = new OverviewCache();
let busy = false;

function frameOf(n: number): Frame {
  const [x0 = 0, texel = 1] = ChiselPreview.overview_frame(n);
  return { x0, texel, n };
}

function run(r: Job) {
  const t0 = performance.now();
  const key =
    r.lane === 'overview'
      ? OverviewCache.key(r.seed, r.edit, r.overlay, r.texel, r.x0, r.z0, r.n)
      : '';
  const hit = key ? cache.get(key) : undefined;
  const cached = hit !== undefined;
  let rgba: Uint8ClampedArray;
  if (hit) {
    rgba = hit.slice();
  } else {
    if (!preview || seed !== r.seed) {
      preview?.free();
      preview = new ChiselPreview(BigInt(r.seed));
      seed = r.seed;
      edit = '';
    }
    if (edit !== r.edit) {
      preview.edit(r.edit);
      edit = r.edit;
    }
    const bytes = preview.tile(r.x0, r.z0, r.n, r.texel, r.overlay);
    rgba = new Uint8ClampedArray(bytes);
    if (key) cache.set(key, rgba.slice());
  }
  const { frame, pass, i, j, last } = r;
  const reply: TileReply = {
    id: r.id,
    lane: r.lane,
    gen: r.gen,
    rgba,
    n: r.n,
    ms: performance.now() - t0,
    frame,
    pass,
    i,
    j,
    last,
    cached,
  };
  (self as unknown as Worker).postMessage(reply, [rgba.buffer]);
}

/** Runs one job, then yields so newer requests arrive (and drop stale jobs) before the next. */
function pump() {
  if (busy) return;
  busy = true;
  setTimeout(() => {
    const r = jobs.next();
    if (r) {
      try {
        run(r);
      } catch (err) {
        edit = '';
        (self as unknown as Worker).postMessage({
          id: r.id,
          lane: r.lane,
          gen: r.gen,
          error: String(err),
        } satisfies TileReply);
      }
    }
    busy = false;
    if (jobs.size) pump();
  }, 0);
}

self.onmessage = async (ev: MessageEvent<TileRequest>) => {
  const r = ev.data;
  await loadWasm();
  if (r.lane === 'detail') {
    jobs.push(r);
  } else {
    for (const pass of OVERVIEW_PASSES) {
      const frame = frameOf(pass);
      const cs = chunks(frame);
      cs.forEach((c, k) => {
        jobs.push({
          ...r,
          x0: c.x0,
          z0: c.z0,
          n: c.n,
          texel: frame.texel,
          frame,
          pass,
          i: c.i,
          j: c.j,
          last: k === cs.length - 1,
        });
      });
    }
  }
  pump();
};
