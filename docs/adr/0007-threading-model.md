# 0007. Threading model: single-threaded web sim core with worker pools; threads natively

- Status: Accepted
- Date: 2026-09-25
- Resolves: ARCHITECTURE.md Open Decisions #4

## Context

Threaded WebAssembly (Emscripten pthreads, Jolt's multithreaded job system) requires
`SharedArrayBuffer`, which browsers only expose to cross-origin-isolated pages
(`Cross-Origin-Opener-Policy` + `Cross-Origin-Embedder-Policy` headers). GitHub Pages cannot send
custom headers. `coi-serviceworker` can inject them from a service worker, at the cost of a
forced reload on first visit, failure where service workers are unavailable (some private
browsing modes), and COEP blocking any cross-origin resource that does not opt in.

Where threads would help in the browser:
- Client prediction and debris physics: one dynamic body plus debris — no benefit.
- Worldgen, meshing, lighting: parallel work that does not need shared memory; independent
  workers exchanging transferable buffers suffice.
- SQLite over OPFS: sync access handles work without `SharedArrayBuffer`.
- A browser hosting a friend world (full server simulation): the only case that benefits
  (Jolt's parallel solver with many Tier 1 bodies and players).

## Options considered

1. **Single-threaded web sim core + worker pools** for embarrassingly parallel work.
2. **Threaded web build via `coi-serviceworker`**, with a single-threaded fallback build for
   when the service worker is unavailable (two builds, double testing).
3. **Threaded everywhere, requiring cross-origin isolation** — impossible on GitHub Pages without
   the service-worker workaround, and fragile.

## Decision

- **Web (GitHub Pages):** the sim core WASM is **single-threaded**. Parallelism comes from
  **worker pools** of independent WASM instances with no shared memory:
  - sim-core worker (integrated server in local mode / friend-world host; prediction and debris
    world run in the client's own sim-core instance),
  - worldgen pool (N workers, N ≈ `hardwareConcurrency − 2`, min 1),
  - meshing pool (and lighting later).
  Work and results move by `postMessage` with transferable `ArrayBuffer`s. No `coi-serviceworker`.
- **Native dedicated server:** always multithreaded — Jolt job system, worldgen thread pool, I/O
  thread for saves, network runtime threads (ADR 0001).
- **Electron:** multithreaded build. The app serves its own content through a custom protocol
  and can send COOP/COEP, so `SharedArrayBuffer` is available.
- **Capacitor:** single-threaded web build unless `SharedArrayBuffer` is confirmed available on
  the app's scheme in Android WebView and WKWebView (checked in Phase 7).
- The sim core is written to run correctly in both modes (Jolt `JobSystemSingleThreaded` vs.
  `JobSystemThreadPool`, selected at build time).

**Revisit trigger:** if Phase 5 profiling shows browser-hosted friend worlds are simulation-bound
at the host profile's caps, add a threaded web build behind `coi-serviceworker` used only for
hosting, keeping the single-threaded build as the default and fallback.

## Consequences

- One web WASM build to ship and test; no service-worker or COEP constraints on the site.
- Browser friend-world hosts are limited to single-threaded simulation, reflected in their
  host profiles (smaller player and Tier 1 caps than dedicated servers).
- Worker pools duplicate the WASM module per worker (memory cost per worker); pool sizes are
  capped, lower on mobile.
