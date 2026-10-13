#!/usr/bin/env -S node --no-warnings
// Runs a wasm32-wasip1 test binary under Node's WASI (Phase 24b): the cargo runner for
// `cargo test --target wasm32-wasip1`, so the worldgen goldens are checked in WASM as well as
// natively (ADR 0010). The repository is preopened at its own path, so tests that read files by
// absolute path work unchanged.
import { readFile } from 'node:fs/promises';
import { WASI } from 'node:wasi';
import { argv, env, exit } from 'node:process';

const [, , wasmPath, ...args] = argv;
const root = new URL('..', import.meta.url).pathname.replace(/\/$/, '');
const wasi = new WASI({ version: 'preview1', args: [wasmPath, ...args], env, preopens: { [root]: root } });
const module = await WebAssembly.compile(await readFile(wasmPath));
const instance = await WebAssembly.instantiate(module, wasi.getImportObject());
exit(wasi.start(instance));
