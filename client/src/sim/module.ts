import type { DwellFiles } from '../local/worldFiles';
import { versionedLocateFile, wasmUrl } from './wasmUrl';

// The Emscripten build of the sim core (server/wasm/wasm_api.cpp): one ES-module factory, one
// instance per use (the local-mode server in its worker; the client sim on the main thread).

/** Module surface (factory created with -sMODULARIZE -sEXPORT_ES6). */
export interface DwellCoreModule {
  HEAPU8: Uint8Array;
  HEAPU16: Uint16Array;
  HEAP32: Int32Array;
  HEAPU32: Uint32Array;
  HEAPF64: Float64Array;
  _malloc(size: number): number;
  _free(ptr: number): void;
  // Local-mode server.
  _dwell_local_create(worldSeed: number, generatorVersion: number, persist: number): number;
  _dwell_local_storage_error(): number;
  _dwell_local_save(): number;
  _dwell_local_host(maxPlayers: number, edits: number, flight: number, hostKeyPtr: number): void;
  _dwell_local_host_status(state: number): void;
  _dwell_local_close_guests(): void;
  _dwell_local_connected(session: number, kind: number, bindingPtr: number): void;
  _dwell_local_disconnected(session: number): void;
  _dwell_local_reliable(session: number, channel: number, ptr: number, len: number): void;
  _dwell_local_datagram(session: number, ptr: number, len: number): void;
  _dwell_local_advance(elapsedSeconds: number): number;
  _dwell_local_take_outbox(outLenPtr: number): number;
  // Client sim.
  _dwell_client_create(): number;
  _dwell_client_chunk_set(cx: number, cy: number, cz: number, revision: number, ptr: number): void;
  _dwell_client_chunk_remove(cx: number, cy: number, cz: number): void;
  _dwell_client_chunk_count(): number;
  _dwell_client_next_seq(): number;
  _dwell_client_tick(
    seq: number,
    moveX: number,
    moveY: number,
    buttons: number,
    yaw: number,
    pitch: number,
  ): void;
  _dwell_client_snapshot(ptr: number, len: number): number;
  _dwell_client_knockback(inputSeq: number, x: number, y: number, z: number): void;
  _dwell_client_set_remote(
    playerId: number,
    x: number,
    y: number,
    z: number,
    vx: number,
    vy: number,
    vz: number,
    crouched: number,
    leadSeconds: number,
  ): void;
  _dwell_client_remove_remote(playerId: number): void;
  _dwell_client_state(): number;
  _dwell_client_chunk_padded(cx: number, cy: number, cz: number): number;
  _dwell_client_chunk_edit(
    cx: number,
    cy: number,
    cz: number,
    revision: number,
    pairsPtr: number,
    count: number,
  ): void;
  _dwell_client_target(
    ox: number,
    oy: number,
    oz: number,
    dx: number,
    dy: number,
    dz: number,
    maxDistance: number,
    outPtr: number,
  ): number;
  _dwell_client_voxel(x: number, y: number, z: number): number;
}

/** Module options: `dwellFiles` gives the local server its world file (local/worldFiles.ts). */
export type DwellCoreFactory = (options?: {
  dwellFiles?: DwellFiles;
  locateFile?: (path: string, prefix: string) => string;
}) => Promise<DwellCoreModule>;

/** URL of the WASM core's JS loader, served from `public/wasm` at the site base (versioned). */
export function dwellCoreUrl(): string {
  return wasmUrl('dwell_core.js');
}

/**
 * Loads the core's factory (browser; tests import the file directly). Its .wasm is fetched with
 * the same build version as the loader (sim/wasmUrl.ts).
 */
export async function importDwellCore(): Promise<DwellCoreFactory> {
  const mod = (await import(/* @vite-ignore */ dwellCoreUrl())) as { default: DwellCoreFactory };
  return (options = {}) => mod.default({ locateFile: versionedLocateFile(), ...options });
}

/** Runs `fn` with `bytes` copied into the module's heap. */
export function withHeapBytes(
  m: DwellCoreModule,
  bytes: Uint8Array,
  fn: (ptr: number) => void,
): void {
  const ptr = m._malloc(Math.max(1, bytes.length));
  try {
    m.HEAPU8.set(bytes, ptr);
    fn(ptr);
  } finally {
    m._free(ptr);
  }
}
