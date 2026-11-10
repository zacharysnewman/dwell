// Chisel's preview worker (ADR 0074 B3): the real generator through WASM (`ChiselPreview`), one
// world's face A built once per seed; each request puts the page's edit on it and rasterises a tile.
import { ChiselPreview, loadWasm } from '../engine/wasm';

export interface TileRequest {
  id: number;
  seed: number;
  edit: string;
  x0: number;
  z0: number;
  n: number;
  texel: number;
  overlay: number;
}
export type TileReply =
  { id: number; rgba: Uint8ClampedArray; n: number; ms: number } | { id: number; error: string };

let preview: ChiselPreview | null = null;
let seed = -1;
let edit = '';

self.onmessage = async (ev: MessageEvent<TileRequest>) => {
  const r = ev.data;
  try {
    await loadWasm();
    const t0 = performance.now();
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
    const rgba = new Uint8ClampedArray(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const reply: TileReply = { id: r.id, rgba, n: r.n, ms: performance.now() - t0 };
    (self as unknown as Worker).postMessage(reply, [rgba.buffer]);
  } catch (err) {
    edit = '';
    (self as unknown as Worker).postMessage({ id: r.id, error: String(err) } satisfies TileReply);
  }
};
