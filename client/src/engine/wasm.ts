// The Rust client's WASM module (Phase 24e, ADR 0028): built by `npm run build:wasm` into
// src/pkg (wasm-bindgen, `--target web`). One instance per JS context: the page's (the game client)
// and each worker's (jobs, the local world).
import init from '../pkg/dwell_client.js';

export * from '../pkg/dwell_client.js';

let loading: Promise<unknown> | null = null;

/** Loads and instantiates the module once per context. */
export function loadWasm(): Promise<unknown> {
  loading ??= init();
  return loading;
}
