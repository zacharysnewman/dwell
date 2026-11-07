// The Rust client's WASM module (Phase 24e, ADR 0028): built by `npm run build:wasm` into
// src/pkg (wasm-bindgen, `--target web`). One instance per JS context: the page's (the game client)
// and each worker's (jobs, the local world).
import init from '../pkg/dwell_client.js';

export * from '../pkg/dwell_client.js';

let loading: Promise<unknown> | null = null;

/** Bytes of the module downloaded so far, of `total` when the server says (null: compressed). */
export type WasmBytes = (done: number, total: number | null) => void;

/**
 * Loads and instantiates the module once per context. With `onBytes` (the page's loading screen,
 * §6.3) its download is counted as it streams.
 */
export function loadWasm(onBytes?: WasmBytes): Promise<unknown> {
  if (!loading && onBytes && typeof fetch === 'function' && typeof TransformStream === 'function') {
    const url = new URL('../pkg/dwell_client_bg.wasm', import.meta.url);
    loading = init({ module_or_path: fetch(url).then((r) => countedResponse(r, onBytes)) });
  }
  loading ??= init();
  return loading;
}

/**
 * `r` with its body counted as it is read: `onBytes(done, total)` per chunk. The total is the
 * Content-Length when the body is not compressed (a compressed one's length is not the bytes read).
 */
export function countedResponse(r: Response, onBytes: WasmBytes): Response {
  if (!r.body) return r;
  const length = Number(r.headers.get('content-length'));
  const total = !r.headers.get('content-encoding') && length > 0 ? length : null;
  let done = 0;
  const body = r.body.pipeThrough(
    new TransformStream<Uint8Array, Uint8Array>({
      transform(chunk, out) {
        done += chunk.byteLength;
        onBytes(done, total);
        out.enqueue(chunk);
      },
    }),
  );
  return new Response(body, { status: r.status, statusText: r.statusText, headers: r.headers });
}
