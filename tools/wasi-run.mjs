#!/usr/bin/env -S node --no-warnings --no-turbo-fast-api-calls
// Runs a wasm32-wasip1 test binary under Node's WASI (Phase 24b): the cargo runner for
// `cargo test --target wasm32-wasip1`, so the worldgen goldens are checked in WASM as well as
// natively (ADR 0010). The repository is preopened at its own path, so tests that read files by
// absolute path work unchanged.
//
// `--no-turbo-fast-api-calls`: Node 22 calls its WASI functions from WASM as V8 fast API calls, and
// `fd_write` there can trigger a garbage collection (uvwasi's allocator reports external memory to
// V8), which crashes the process (SIGSEGV walking the stack in IterateTurbofanOptimizedFrame) once
// a test has grown the WASM memory enough to put V8 under external-memory pressure: `dwell-lod`'s
// bifacial rim test did, and the run died after it with no output. The slow path is a normal call,
// where a collection is safe; WASI calls are few, so it costs nothing measurable.
import { readFile } from 'node:fs/promises';
import { WASI } from 'node:wasi';
import { argv, env, exit } from 'node:process';

const [, , wasmPath, ...args] = argv;
const root = new URL('..', import.meta.url).pathname.replace(/\/$/, '');
const wasi = new WASI({ version: 'preview1', args: [wasmPath, ...args], env, preopens: { [root]: root } });
const module = await WebAssembly.compile(await readFile(wasmPath));
const instance = await WebAssembly.instantiate(module, wasi.getImportObject());
exit(wasi.start(instance));
