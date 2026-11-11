// Chisel's preview worker (ADR 0074 B3): the real generator through WASM (`ChiselPreview`), one
// world's face A built once per seed; each request puts the page's edit on it and rasterises a tile.
// Three lanes (overview.ts `Jobs`): the detail preview's tiles first, then the 3D section's passes
// (the 3D section, meshed here), then the world overview's passes, split into chunks and cached by the edit
// state; a newer generation drops a lane's older jobs.
import { ChiselPreview, loadWasm } from '../engine/wasm';
import { Jobs, OVERVIEW_PASSES, OverviewCache, chunks, type Frame, type Lane } from './overview';

/** A detail tile, the whole world's overview (lane 'overview', expanded here into chunks), or a
 * 3D section pass (lane 'section': `n` × `n` columns of `texel` m from (x0, z0)). */
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
  /** Hill-shade the tile (2D lanes). */
  relief: boolean;
}
interface Job extends TileRequest {
  /** An overview chunk's frame, pass and texel offset in it. */
  frame?: Frame;
  pass?: number;
  i?: number;
  j?: number;
  last?: boolean;
}
/** A meshed section (`ChiselSection`): vertices of four u32 (x, y, z f32 bits; `0xNNRRGGBB`). */
export interface SectionMesh {
  x0: number;
  z0: number;
  n: number;
  texel: number;
  ground: Uint32Array;
  groundIndex: Uint32Array;
  water: Uint32Array;
  waterIndex: Uint32Array;
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
  | { id: number; lane: 'section'; gen: number; mesh: SectionMesh; ms: number }
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

function generator(r: Job): ChiselPreview {
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
  return preview;
}

function runSection(r: Job) {
  const t0 = performance.now();
  const s = generator(r).section(r.x0, r.z0, r.n, r.texel);
  const [x0 = r.x0, z0 = r.z0] = s.origin();
  const mesh: SectionMesh = {
    x0,
    z0,
    n: r.n,
    texel: r.texel,
    ground: s.ground_verts(),
    groundIndex: s.ground_index(),
    water: s.water_verts(),
    waterIndex: s.water_index(),
  };
  s.free();
  const reply: TileReply = {
    id: r.id,
    lane: 'section',
    gen: r.gen,
    mesh,
    ms: performance.now() - t0,
  };
  (self as unknown as Worker).postMessage(reply, [
    mesh.ground.buffer,
    mesh.groundIndex.buffer,
    mesh.water.buffer,
    mesh.waterIndex.buffer,
  ]);
}

function run(r: Job) {
  if (r.lane === 'section') {
    runSection(r);
    return;
  }
  const t0 = performance.now();
  const key =
    r.lane === 'overview'
      ? OverviewCache.key(r.seed, r.edit, r.overlay, r.relief, r.texel, r.x0, r.z0, r.n)
      : '';
  const hit = key ? cache.get(key) : undefined;
  const cached = hit !== undefined;
  let rgba: Uint8ClampedArray;
  if (hit) {
    rgba = hit.slice();
  } else {
    const bytes = generator(r).tile(r.x0, r.z0, r.n, r.texel, r.overlay, r.relief);
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
  if (r.lane !== 'overview') {
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
