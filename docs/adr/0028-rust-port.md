# 0028. One Rust codebase: server, sim core and client, with Rapier physics and a wgpu renderer (WebGPU only)

- Status: Accepted
- Date: 2026-10-13
- Supersedes: [0027](0027-webgpu-renderer.md) (Three's `WebGPURenderer`, TSL, the WebGL2 fallback and
  the settings menu's renderer readout, decision D4 of [`BASELINE_EVALUATION.md`](../BASELINE_EVALUATION.md)
  §7); [0002](0002-client-renderer.md)'s Three.js render interface; the C++ core and its Emscripten
  build (ARCHITECTURE.md §2, §3.1); Jolt Physics as the physics engine
- Builds on: [0001](0001-webtransport-server-library.md) and [0008](0008-dedicated-server-transports.md) (the `dwell-net` crate
  is already Rust), [0014](0014-versioned-releases.md) (compatibility lines),
  [0025](0025-baseline-world.md) (the baseline the port reproduces)

## Context

The build and test pipeline is slow. CI run 208 (2026-10-13) took 11 minutes, about half of it
compiling: the native server six times and the WASM core five times per run, through CMake,
FetchContent (Jolt, zstd, SQLite, Monocypher) and Emscripten. Three toolchains (CMake/C++,
Emscripten, Cargo) and two languages for the same sim, with a C ABI between them, also slow down
local work. (The CI layout itself is being fixed separately: one build shared by the e2e shards,
ccache, parallel suites.)

The owner wants one language for everything performance-critical, on the same output platforms
(web on GitHub Pages, Electron, Capacitor, native dedicated servers). The `dwell-net` crate
(WebTransport and WebRTC) is already Rust. Jolt usage is modest: rigid bodies, a capsule character,
mutable compound shapes, object layers, collision groups, double-precision positions (`RVec`, for
~8,000 km from the origin, ADR 0011). The client is about 16k lines of TypeScript on Three.js,
whose sim already runs the C++ core as WASM.

The baseline rework (Phases 14–19) is in progress: Phase 15 is built, 16–19 are not started.

## Options considered

Physics:
1. **Rapier** (chosen). Native Rust, an `f64` build for far-from-origin positions, an
   `enhanced-determinism` feature for bit-identical results natively and in WASM, compound shapes,
   islands and sleeping (needed by Phases 20–22). It's a different engine, so the controller is
   re-tuned and its golden traces re-recorded.
2. Jolt through bindings: behaviour kept, but C++ (and a WASM C++ toolchain) stays in the build,
   which defeats the purpose.
3. An in-house solver: full control, most work.

Strategy:
1. Incremental, module by module behind the C ABI: shippable throughout, but every step pays for
   two languages and the bridge.
2. **Big-bang** (chosen): a parallel Rust tree reproduces today's baseline, then replaces the C++
   and the TS sim and renderer in one cutover. Nothing ships from the new tree until then.

Renderer:
1. Three's `WebGPURenderer` in TS (ADR 0027): smallest change, keeps two languages on the client.
2. **wgpu in Rust** (chosen): rendering, meshing, LOD and prediction move into the Rust client;
   TS shrinks to the DOM UI and the shell.
3. Hand-written WebGPU in TS.

Compatibility: bit-exact with today's worlds, terrain and protocol, or **a new compatibility line**
(chosen: the port may change formats, terrain hashes and the protocol).

## Decision

1. **One Cargo workspace** holds the server, the sim core, the client and the tools. There is no
   CMake and no Emscripten: native builds are `cargo build`, the browser build is
   `wasm32-unknown-unknown` with `wasm-bindgen`. Dependencies are pure Rust where a good crate
   exists; C that a crate builds with `cc` (e.g. `ring`) is accepted.
2. **Physics is Rapier** (`rapier3d-f64`, `enhanced-determinism`), wrapped by Dwell's own physics
   module so game code doesn't depend on Rapier types (as the core wraps Jolt today). The player
   controller follows [`PLAYER_CONTROLLER.md`](../PLAYER_CONTROLLER.md) and is re-tuned on Rapier;
   golden traces are re-recorded; the native↔WASM divergence check remains.
3. **The client is Rust on wgpu, WebGPU only.** Browsers, WebViews and GPUs without WebGPU get an
   "unsupported" screen; there is no WebGL2 fallback and no renderer readout in settings.
   Shaders are WGSL. Meshing and LOD run in Web Workers, each with its own WASM instance and
   message passing: no `SharedArrayBuffer` (as today, [0007](0007-threading-model.md)), because GitHub Pages cannot send the cross-origin
   isolation headers it needs.
4. **The port is big-bang, and it comes next.** Feature work in C++ and TS stops after Phase 15.
   The port (Phase 24 of the implementation plan) reproduces the Phase 15 baseline in Rust; Phases
   16–18 are then built in Rust; Phase 19's WebGPU work is done by the port, and its look registry
   and textures-as-data are built on wgpu.
5. **A new compatibility line.** The port may change the world file format, terrain hashes and the
   protocol. The owner raises `package.json` to the next MINOR when the port ships (RELEASES.md).
6. **What stays TypeScript** (recommended by Claude for build speed and performance, 2026-10-13;
   the owner asked for a recommendation, so it's open to change):
   - **Menus and UI as HTML/DOM** (main menu, settings, server browser, HUD) over the canvas,
     talking to the Rust client through a small `wasm-bindgen` API. DOM text, input, accessibility
     and mobile keyboards are free; an in-canvas UI toolkit would add WASM size and rebuild that.
   - **The master server** (`services/master`, Cloudflare Workers): small, not part of the sim, and
     its pipeline already runs in about 30 seconds.
   - **The launcher and release scripts** (`client/scripts/release.ts`, `site.ts`).
   - **Browser networking moves to Rust** through `web-sys` (WebTransport, WebRTC data channels),
     with a TS shim only for anything `web-sys` lacks, so the protocol codec exists once.
7. **Generated data stays generated.** `shared/blocks` and `shared/protocol` keep their data files
   and generators, which emit Rust instead of C++ (and TS only where TS still reads them, e.g. the
   master's vectors); CI still fails on stale output.

8. **Storage, compression and crypto** (the owner left these to Claude, optimising for speed of
   the game and of the build, 2026-10-13):
   - **World storage is `redb`**, a pure-Rust embedded key-value store (ACID, copy-on-write B-trees):
     chunks are blobs keyed by chunk coordinates, plus a metadata table. Natively it uses files; in
     the browser a `redb` storage backend over an **OPFS sync access handle** in the worker that runs
     local mode. One format, one code path, no C. SQLite and its WASM build go.
   - **Compression is LZ4** (`lz4_flex`, pure Rust) for chunk blobs on disk and on the wire: it
     decompresses several times faster than zstd at a somewhat lower ratio, and voxel chunks (long
     runs of few block states) still compress well. zstd goes.
   - **Crypto is `ed25519-dalek`** (pure Rust). Monocypher is used only for Ed25519 signature checks
     (`crypto_ed25519_check`, the master's receipts); the master's vectors pin compatibility.

## Consequences

- One toolchain and one language for the sim, server and client; CI compiles with Cargo
  (`sccache`, `rust-cache`), tests with `cargo nextest`, and builds the WASM once per run.
  The pipeline target is under 6 minutes on a warm cache.
- **Risk: no shipping during the port.** The C++/TS tree stays releasable for fixes (patch releases
  on the current line) but gets no features until the cutover.
- **Risk: physics behaviour changes.** Rapier's contacts and friction differ from Jolt's; the
  controller scenarios (steps, crawlspaces, slopes, knockback, far origin, face B) are the
  acceptance tests, and the 64-player < 1 ms/tick budget is re-measured. `enhanced-determinism`
  turns off SIMD, so the budget is checked with it on.
- **Risk: WebGPU only drops players.** Firefox on some platforms, iOS before 26, Android WebView
  before Chrome 121 and some Linux GPU drivers can't play. CI needs WebGPU in headless Chromium
  (a software adapter); this is proven first, before the renderer work.
- The Phase 7 look (palette, face tints, sky, height fog, tone mapping, exposure) is ported, not
  redesigned; the owner compares before/after screenshots.
- `ARCHITECTURE.md` is rewritten at the cutover; until then it describes the C++/TS build, with the
  port marked **[planned]**.
- Reversing it means keeping the C++/TS tree, which stays intact until the cutover.
