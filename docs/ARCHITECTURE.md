# Dwell — Game Architecture

> **Living document.** This file describes the architecture as it is *intended and
> built*. Any change that adds, removes, or alters an architectural element
> (components, process boundaries, protocols, message formats, data formats,
> physics tiers, tunable thresholds, deployment topology, third-party engines)
> must update this file in the same change. See `CLAUDE.md`.
>
> Status legend used below: **[planned]** not yet built · **[in progress]** · **[built]**

---

## 1. System Overview

Dwell is a **server-authoritative, multiplayer-first voxel sandbox** that supports both
small- and large-scale dynamic physics (collapsing structures, explosions, debris).

| Concern | Choice |
|---|---|
| Authoritative server | C++20, Jolt Physics; **hosted by players** — dedicated servers or friend worlds (§10, ADR 0003) |
| Client | TypeScript; physics/prediction via the shared C++ sim core (Jolt linked in) compiled to WebAssembly |
| Renderer | Three.js, WebGL2 first, behind a thin render interface (ADR 0002) |
| Client shells | Browser (GitHub Pages) → Electron (desktop) → Capacitor (iOS/Android) |
| Transport | Dedicated servers: WebTransport (HTTP/3 / QUIC), WebRTC fallback. Friend worlds: WebRTC data channels (ADR 0008) |
| Services | Master server: server listing, join codes, cert-hash distribution, WebRTC signaling, TURN credentials (§10) |
| Identity | Device keys (Ed25519) now; optional accounts later (§10.4, ADR 0004) |
| Simulation | 60 Hz internal physics step, 20 Hz network snapshots |
| Players | Dynamic-body, velocity-layer controller ported from the Physics Player Controller, client-predicted (§9, [`PLAYER_CONTROLLER.md`](./PLAYER_CONTROLLER.md)) |
| World | **[built, Phase 3c]** A disc of radius 8,192 km, 8,192 m tall (¾ above sea level), double-precision physics (§6.3, ADR 0011) |
| Terrain | Seeded deterministic procedural generation, same C++ code on server and client (§6.3) |
| Whole-world view | **[planned, Phase 4]** 3D level-of-detail octree around the camera, generated on the client from the seed; only modified sections are sent (§6.6, ADR 0012) |
| First deployment target | **GitHub Pages** at `https://dropkickarcade.com/dwell/` (ADR 0005) |

```
                    ┌───────────────────────────────────────────────┐
                    │           Authoritative Server (C++)          │
                    │                                               │
  inputs (dgram) ──▶│ Input validation ─▶ Player controllers        │
                    │ Worldgen (seed) · LOD propagation (off tick)  │
                    │        │                                      │
                    │        ▼                                      │
                    │ Master voxel grid ─▶ Structural integrity     │
                    │        ▲                 │  (flood-fill)      │
                    │        │ re-bake         ▼                    │
                    │  Sleep monitor ◀── Tier 1 bodies (Jolt, 60Hz) │
                    │                                               │
                    │ Replication: snapshots (dgram, 20Hz)          │
                    │              voxel deltas / events (reliable) │
                    │              LOD sections (reliable, `lod`)   │
                    └───────────────▲───────────────┬───────────────┘
                                    │ WebTransport  │
                                    │  / WebRTC     ▼
┌──────────────────────────────────────────────────────────────────────┐
│                 Client (TS + sim core WASM w/ Jolt)                  │
│  Input → Local prediction (player controller) → Reconciliation       │
│  Chunk store (+ worldgen WASM worker) → Mesher (worker) → Renderer   │
│  LOD octree (+ worldgen / mesher workers) → far render pass          │
│  Snapshot buffer → Interpolation of Tier 1 bodies                    │
│  Local-only Jolt world → Tier 2 cosmetic debris                      │
└──────────────────────────────────────────────────────────────────────┘
   Shells: Browser (GitHub Pages) │ Electron │ Capacitor (iOS / Android)
```

---

## 2. Deployment Topology

### 2.1 GitHub Pages (first target) **[built]** (items 1–4)

GitHub Pages only serves static files. Consequences that shape the architecture:

1. **The authoritative server cannot run on GitHub Pages.** The static client is served
   from Pages; servers are hosted by players (§2.3, §10). The client finds them through the
   master server, a join code, or an invite link (§10.1).
2. **No custom HTTP headers** → no `Cross-Origin-Opener-Policy` / `Cross-Origin-Embedder-Policy`
   → no `SharedArrayBuffer`. The web build of the sim core (and the Jolt inside it) is therefore
   compiled **single-threaded** (no pthreads); parallel work runs in **worker pools** of
   independent WASM instances exchanging transferable buffers (§5.1,
   [ADR 0007](./adr/0007-threading-model.md)). No `coi-serviceworker`. Electron uses a
   multithreaded build; the native server is always multithreaded.
3. **Default project path.** The site is served at `https://dropkickarcade.com/dwell/` (ADR 0005),
   Vite `base: '/dwell/'`. The origin is shared with other `dropkickarcade.com` games, so all
   browser storage is `dwell`-namespaced, the device key is non-extractable, and any service
   worker is scoped to `/dwell/`. Older client builds are kept at `/dwell/v/<version>/` so
   players can join servers that have not updated. A move to its own subdomain is a documented
   future option (ADR 0005).
4. **Local mode** **[built]**. So the Pages deployment is playable with no hosted server, the
   server's simulation core is also compiled to WASM (`server/wasm`, Emscripten) and run in a
   module Web Worker (`client/src/local/worker.ts`), connected through `LoopbackTransport`, which
   implements the same interface as the network transports. Same code, same protocol, same
   device-key handshake, no network. The page starts local mode when it has no invite link (or
   with `?local=1`). Local mode and dedicated servers generate the **procedural terrain** world
   (generator version 4, §6.3) by default; `?world=playground|flat` and `?seed=N` (local mode) or
   `--generator N` and `--seed N` (`dwell_server`) pick another generator or seed. The
   **playground** (version 1) is the flat world plus movement test features near the spawn.
   **[built, Phase 3e]** Local worlds are saved in the browser: one world file per generator and
   seed in OPFS (`dwell/worlds/local-g<generator>-s<seed>.dwellworld`, §6.4), every 5 s and when
   the page is hidden or closed; a reload continues the same world.
5. **One C++ core, compiled for the browser twice.** The Emscripten build of `server/core`
   (`dwell_core.wasm`) provides local mode and the client's prediction/debris physics; its
   generator sources alone form `dwell_worldgen.wasm` (~30 KB, no Jolt) for the client's worldgen
   workers. Every piece of simulation logic (player controller, worldgen, physics setup) has
   exactly one implementation.

Deployment is automated by `.github/workflows/pages.yml` **[built]**: it builds the WASM core
(Emscripten) and `client/`, and publishes them with `actions/deploy-pages` on pushes to `main`
(the repository's Pages source is "GitHub Actions"). The page carries a strict
Content-Security-Policy `<meta>` tag, since Pages cannot send headers (§11).

### 2.2 Desktop / Mobile shells **[in progress]**

The same Vite build output is wrapped by:
- **Electron** (`platforms/electron`) **[built: shell]** — Chromium, full WebTransport support.
  `main.js` serves the client build from a privileged, secure `dwell://` protocol (so WebCrypto,
  IndexedDB, and WebTransport work) with COOP/COEP headers, ready for the multithreaded sim-core
  build (ADR 0007; it currently loads the single-threaded one). Invite queries are passed with
  `--join=<query>`; `--smoke` exits 0 once joined (used by CI under Xvfb). Packaging and "Host
  world" come in Phase 8.
- **Capacitor** (`platforms/capacitor`) — Android System WebView (Chromium) and iOS
  WKWebView. Where WebTransport is unavailable (possibly WKWebView), the client uses WebRTC. Single-threaded sim core unless
  `SharedArrayBuffer` is confirmed available on the app scheme (ADR 0007).

### 2.3 Server hosting **[planned]**

There are **no official game servers**; players host (ADR 0003, details in §10):
- **Dedicated servers:** the native server binary (Windows/macOS/Linux) or Docker image on a
  player's machine or rented VM, or launched by the Electron app ("Host world"). Needs inbound
  UDP (QUIC and WebRTC); UPnP/NAT-PMP is attempted, otherwise the host
  forwards the port.
- **Friend worlds:** any client (browser, phone, desktop) hosts its integrated server (the sim
  core in a worker) over WebRTC.
- **Certificates:** dedicated servers generate a self-signed ECDSA P-256 certificate at startup
  (13-day validity, by `server/net/wt`) used by **both** WebTransport and WebRTC DTLS, and
  publish its SHA-256 through the master server or the invite link; clients pass it as
  WebTransport `serverCertificateHashes` or pin it as the DTLS fingerprint. Hosts with their own
  domain may use a publicly trusted certificate instead. Local development uses the same
  mechanism on `127.0.0.1`. **[built]** (rotation before expiry: Phase 8)
- **Our infrastructure** is limited to the static site (GitHub Pages), the master server, and a
  TURN relay.

---

## 3. Repository Layout **[in progress]**

```
/client              TypeScript client (Vite). Renderer, prediction, interpolation, debris.
  /e2e               Playwright end-to-end tests against a native server.
/server              C++20 authoritative server (CMake). Jolt via FetchContent.
  /core              Simulation core: voxel grid, worldgen, integrity, clustering, physics,
                     players, replication.
                     Platform-free; compiled natively AND to WASM (local mode + client physics).
    /player          Physics player controller (PLAYER_CONTROLLER.md).
    (lod.h)          Level of detail (§6.6): the section grid, Downsample, GenerateLod's interface.
    /storage         World persistence: SQLite + zstd; the native file VFS and the browser's OPFS VFS
                     (ADR 0006).
  /wasm              Emscripten build of the core (C exports): local mode (§2.1) and the client sim.
  /tools             dwell_scenario_trace + divergence.mjs (native↔WASM check); dwell_worldgen_inspect;
                     dwell_world (world file info, regenerate-and-diff); WASM test builds.
  /app               dwell_server executable (event loop, CLI).
  /net               Network front-end (C++). Native only.
    /wt              Rust crate: WebTransport (`wtransport`) + WebRTC (`str0m`) behind a C ABI (ADRs 0001, 0008).
/shared/protocol     constants.json (single source of protocol constants) + gen.mjs (→ C++ and TS
                     headers), make_vectors.py (independent reference encoder) → vectors.txt
                     (golden bytes both codecs must match).
/services/master     Master server (listing, join codes, signaling, TURN credentials).
/platforms/electron  Electron shell (incl. "Host world" launching the native server).
/platforms/capacitor Capacitor shell.
/docs                ARCHITECTURE.md, PLAYER_CONTROLLER.md, IMPLEMENTATION_PLAN.md, FUTURE.md.
  /adr               Architecture decision records.
/.github/workflows   ci.yml (protocol, client, server, e2e jobs), pages.yml (deploy).
rust-toolchain.toml  Pinned Rust toolchain.
```

Built so far (Phases 0–2): `client/` (renderer, networking, identity, local mode, game loop with
prediction, HUD, e2e tests), `server/` (`core`: Jolt, voxels, terrain collision,
protocol, server, `player` controller; `net/wt`: WebTransport + WebRTC; `wasm`; `app`; `tests`,
including `tests/player`; `tools`), `shared/protocol`, and `platforms/electron`. The player controller's
sources live in `core/include/dwell/player` and `core/src/player`, the storage layer's (Phase 3e) in
`core/include/dwell/storage` and `core/src/storage`. `services/master` and `platforms/capacitor` are
not built yet.

### 3.1 Toolchain & CI **[built]**

| Area | Choice (pinned) |
|---|---|
| Client | Node 22, Vite 8 (module workers), TypeScript 6.0 (strict), ESLint (typescript-eslint strict, type-checked), Prettier, Vitest, Playwright 1.56.1 (e2e), Three.js 0.186 |
| Server C++ | CMake ≥ 3.24 with presets (`dev`, `release`, `wasm`), Ninja, C++20, `-ffp-contract=off`; dependencies via FetchContent: Jolt v5.6.0 (`CROSS_PLATFORM_DETERMINISTIC=ON`, `DOUBLE_PRECISION=ON` for the 8,192 km world, `CPP_RTTI_ENABLED=ON` so Dwell can subclass Jolt's `GroupFilter`), Monocypher 4.0.3 (Ed25519 verification), SQLite 3.53.4 (the amalgamation from sqlite.org; `SQLITE_THREADSAFE=2` natively, `=0` with `SQLITE_OS_OTHER` and Dwell's OPFS VFS in WASM), zstd 1.5.7 (portable C, single-threaded), Corrosion v0.6.1, doctest v2.5.3; clang-format (Google-based) |
| WASM | Emscripten 6.0.10 (`wasm` preset): ES-module factory, single-threaded (ADR 0007), copied to `client/public/wasm` (not committed; `npm run build:wasm`) |
| Server Rust | Toolchain 1.94.1 (`rust-toolchain.toml`), edition 2024; `server/net/wt` (wtransport 0.7.2 with ring, str0m 0.23.1 with aws-lc-rs, tokio) built by Corrosion as a static library; C header generated by cbindgen 0.29.4 (`gen-header.sh`) and checked in |
| Desktop | Electron 44.4.5 (`platforms/electron`) |
| CI (`ci.yml`) | Protocol: regenerated constants and vectors must match. Client: WASM build, the player/netcode suite under WASM (Node) at the origin and ~8,000 km from it, the worldgen and storage suites under WASM (the storage suite through the browser's VFS, reading the natively written golden world file), format, lint, typecheck, unit tests (WASM tests required), build. Server: clang-format, `cargo fmt`/`clippy -D warnings`/`test`, stale-header check, CMake configure/build, ctest, the player and netcode suites ~8,000 km from the origin, smoke-run, then a Release build running the player performance gate and golden trace at both origins. E2E: native↔WASM controller divergence check (`server/tools/divergence.mjs`), native server + built client in Playwright Chromium (WebTransport, WebRTC, local mode, session replacement, local-mode movement, two clients seeing each other move with one over a simulated 150 ms link, breaking and placing every palette block, an edit seen by a second client, touch edits, a local world surviving a reload, the whole-world view — LOD around the player, then the whole disc after flying up to the flight ceiling), Electron smoke under Xvfb |
| Enforced boundaries | ESLint forbids importing `three` outside `client/src/render/three` (ADR 0002) and `node:` built-ins outside tests |

---

## 4. Server (Authoritative) **[in progress]**

### 4.1 Responsibilities
- Validate player input (rate, magnitude, reach for block edits, anti-teleport).
- Own the **master voxel grid** — the single source of truth for terrain — and generate
  unmodified chunks from the world seed (§6.3).
- Compute **structural integrity** and run flood-fill clustering on detached voxels.
- Simulate all **Tier 1** dynamic bodies and all player characters.
- Replicate state: snapshots (unreliable) and voxel deltas / events (reliable).
- Run the **sleep / re-bake** cycle to keep active body count bounded.

### 4.2 Main loop

```
every 16.67 ms (60 Hz):
    drain & validate inputs (per player, ordered by input sequence)
    integrate completed worldgen jobs (thread pool, budgeted)
    apply queued voxel edits → integrity → clustering → awaken bodies
    player controller pipeline per player (probes → layers → set body velocity; PLAYER_CONTROLLER.md §4)
    physicsSystem.Update(1/60)
    sleep monitor → re-bake queue → apply re-bakes
every 3rd step (20 Hz):
    build per-client snapshot (interest-managed, prioritized) → send datagrams
    flush reliable queues (voxel deltas, events) in order
```

The loop uses a fixed timestep with an accumulator; the server never steps with a variable dt.

**Built (Phase 1):** `dwell_server` (`server/app/main.cpp`) polls transport events and feeds them
to the core, flushes the core's outbox immediately (so request/response latency doesn't wait for
a tick), runs the due 60 Hz steps (`FixedStep`: carries remainders, drops backlogs beyond 8
steps), and sleeps until the next step or at most 2 ms. Physics steps with Jolt's thread pool.
**Built (Phase 2):** each `Server::Step` takes one input per joined player from its jitter-buffered
queue (§9.3), runs the player controller pipeline, applies its server-side consequences (fall
damage, the void below `WORLD_MIN_Y`, debug launch pads → knockback), steps physics, respawns dead
players after `RESPAWN_SECONDS`, and every 3rd tick sends each client a `PhysicsSnapshot`.
**Built (Phase 3a):** the world generates from `(worldSeed, generatorVersion)` (§6.3); players
spawn at the generator's spawn point. **Built (Phase 3b):** each step first integrates the chunks
the worldgen pool finished, and ends by streaming chunks to each client (interest-managed, within
its bandwidth budget), telling the pool what to generate next, and (every 64 ticks) evicting
unmodified chunks far from players (§6.3). **Built (Phase 3d):** next, the block edits queued since
the last step are validated and applied, and one `VoxelModification` goes to every client streaming
a changed chunk (§6.5); terrain collision rebuilds in the controller pass of the same step.
**Built (Phase 3e):** the step starts by collecting saves the I/O thread finished, and ends
with the autosave when due (§6.4). Integrity and re-bake arrive with later phases.

### 4.3 Simulation core vs. network front-end
`server/core` has no sockets, and its only OS calls are the world file's (SQLite, §6.4); its only
threads are the optional worldgen pool's (`ServerConfig::worldgen_threads`; the dedicated server
enables `cores − 2`, the browser's single-threaded local mode none, §6.3) and the world store's I/O
thread (native only); the host supplies the Jolt job system and an `Entropy`
source for nonces; it consumes decoded messages and emits encoded messages through an
interface. **[built]** `dwell::core::Server`: `OnConnected(session, transportKind, binding)`,
`OnReliable`, `OnDatagram`, `OnDisconnected`, `Step()`, and `TakeOutbox()` returning
`{session, Reliable | Datagram | Close, channel, bytes}`. This lets the identical core run natively, in the browser's local mode, and
as the client's prediction/debris physics (§2.1). `server/net` owns sessions and stream
management for both server transports.

**WebTransport stack (ADR [0001](./adr/0001-webtransport-server-library.md)).** WebTransport is
provided by the Rust crate `wtransport`, wrapped in `server/net/wt` and exposed to C++ through a
narrow, `cbindgen`-generated C ABI **[built, ABI v3]**: `dwell_net_start(config)` /
`dwell_net_stop`, `dwell_net_poll(event)` (Connected / Disconnected / Reliable / Datagram; payload
valid until the next poll), `dwell_net_send_reliable(session, channel, bytes)`,
`dwell_net_send_datagram`, `dwell_net_close` (flushes the control stream first), plus accessors
for the certificate hash, ports, and ICE credentials. The Rust async runtime (tokio, 2 workers)
runs on its own threads; events reach the main loop through a queue it drains, so no Rust
callback ever enters the simulation. The crate is built by cargo and linked via Corrosion in the
CMake build. Sockets bind dual-stack `[::]` and fall back to IPv4 where IPv6 is unavailable.

**WebRTC fallback (ADR [0008](./adr/0008-dedicated-server-transports.md)) [built].** The same
crate hosts an ICE-lite WebRTC endpoint built on `str0m` (sans-I/O, one tokio task driving every
WebRTC client), on its own UDP port (default: WebTransport port + 1), with the same C ABI and
event queue. **No signaling:** the server has fixed ICE credentials (random per start) published
in the invite link; it learns each client's ICE username from the client's first STUN binding
request and then creates a `str0m::Rtc` for it (DTLS server role, SCTP, three pre-negotiated data
channels). DTLS uses the WebTransport certificate, so both transports share one fingerprint.
Client certificates are not verified (str0m fingerprint verification off): the client pins the
server fingerprint and proves its identity with the device-key handshake, which is bound to that
fingerprint. Clients that don't open all channels within 10 s are dropped, and half-open clients
are capped at 512. Reliable writes queue while SCTP buffers are full.

---

## 5. Client **[in progress]**

| Module | Responsibility |
|---|---|
| `game/` **[built]** | `Game`: the fixed 60 Hz loop — samples input, predicts with the client sim, sends `PlayerInput` (newest 4), feeds snapshots and knockback events to the sim, nudges its tick rate from the server's input buffer, streams terrain around the player (chunk data and voxel modifications to `world/`, mesh jobs within a per-frame budget), drives the first-person camera (per-tick eye height with crouch and step-up/down smoothing, `eye.ts`), block targeting and its outline, and the HUD. `RemotePlayers`: snapshot buffer, interpolation `INTERP_DELAY_MS` in the past. |
| `sim/` **[built]** | `ClientCore`: the client's own instance of the sim-core WASM on the main thread (`dwell_client_*` exports): a streamed world holding the chunks the server sent (`setChunk` / `removeChunk`) and the voxel edits applied to them (`editChunk`), the C++ `Predictor` (prediction world with the local player, dead-reckoned remote proxies, terrain), its state block as 64 doubles (positions anywhere in the 8,192 km world), block targeting (`target`: the same `RaycastBlock` the server checks line of sight with, §6.5), and each chunk's voxels with a one-voxel apron for the meshing workers (`paddedChunk`). |
| `predict/` **[built]** | Keyboard + pointer-lock input (WASD, Space, Shift, C/Ctrl, F3, F4; while the pointer is locked, left/right click break/place, number keys and the wheel pick a block); the creative-flight toggle (`flight.ts`, Phase 4: double-tap Space or Jump, or the touch Fly button; only if `Welcome` allows it; §9.1); touch controls for phones and tablets (`touch.ts`: floating left-half joystick, drag-to-look right half — a short, still touch there is a *tap* that breaks or places — held Jump and Crouch buttons, latching Run and Break/Place buttons, a Fly button, one captured Pointer Events pointer per control, merged into the same sampled input); and input quantization mirroring the C++ `QuantizeInput`. |
| `net/` **[built]** | `Transport` interface; `WebTransportTransport` (cert-hash pinning, stream framing; a closing connection first reads the control stream to its end, up to 2 s, so the server's last message — a `Reject` such as `Replaced` — is not lost to a write racing the close; datagram writes never queue — one in flight and only the newest waiting per message type, `datagramSender.ts`, so slow frames cannot build input latency), `WebRtcTransport` (builds the ICE-lite server's answer from the invite), `LoopbackTransport`; `openTransport` picks WebTransport and falls back to WebRTC (`?transport=` forces one); invite parsing; `ClientSession` (handshake, reliable and datagram RTT, gameplay messages); `SimulatedTransport` (`?netsim=rtt,jitter,loss%`). |
| `protocol/` **[built]** | Codecs mirroring the C++ ones (including the chunk palette + RLE, `chunkVoxels.ts`), constants generated from `shared/protocol`. |
| `identity/` **[built]** | Device key (§10.4): non-extractable Ed25519 WebCrypto key in IndexedDB. |
| `local/` **[built]** | Local mode: `LocalCore` wrapper over the WASM exports and the module worker hosting it; `world.ts` reads `?world=` and `?seed=`. **[built, Phase 3e]** `worldFiles.ts`: the world file's OPFS sync access handles (the database, its journal, and a WAL for opening a dedicated server's file), opened by the worker before the core starts and handed to its VFS as `dwellFiles`; the page asks for a save when hidden or closed. |
| `ui/` **[built]** | Connection status overlay (transport, player id, RTTs, server tick); HUD (crosshair, health, death message; background work such as terrain loading is a small status in the bottom-left corner, never over the view — `game/hudText.ts`) and the F3 debug overlay (on touch screens the connection status and the overlay stack below the top hotbar) (PLAYER_CONTROLLER.md §9; Phase 3e adds the player's chunk regenerated and diffed against the world's: its revision and how many voxels differ from generation); the F4 terrain map (`mapOverlay.ts`, Phase 3e: 128² columns at 8 m around the player from a worldgen worker, coloured by biome and hill-shaded, with the player's heading); the block hotbar (`hotbar.ts`, §6.5: a swatch per palette slot cut from the texture atlas, the selected one highlighted and named; tapping a slot selects it). |
| `interact/` **[built]** | `BlockInteraction` (§6.5): targets the block under the crosshair each frame (`ClientCore.target` from the eye, `REACH_DISTANCE`), the palette (`PALETTE`: every placeable material, ladders as one slot whose facing follows the placement) and its selection, and break/place actions turned into `BlockEditRequest`s at most once per `BLOCK_EDIT_INTERVAL_MS`. |
| `world/` **[built]** | Material ids, render styles and the placeable set (mirroring `voxel.h`, checked by tests). `ChunkStreamer` (`chunkStream.ts`): applies `ChunkData` (Generated via the worldgen pool, Explicit decoded) and `ChunkUnload` to the client sim, applies `VoxelModification`s in revision order (holding those of chunks still generating; a gap sends `ChunkResync`), starts mesh jobs for changed chunks nearest first, and tells the game when the terrain around the player is loaded (§6.3). |
| `worldgen/` **[built]** | Worldgen worker pool (`pool.ts`, `worker.ts`): module workers each running `dwell_worldgen.wasm` — the server's C++ terrain generator alone — for `Generated` chunks and the verification hash; jobs in request order, cancellable until handed to a worker (§5.1, §6.3); also samples the terrain's biome/height map for the debug map (Phase 3e). **[built, Phase 4]** The module exports `GenerateLod` and the LOD column bounds (`ChunkGenerator.lod`, `.lodBounds`), which the pool runs for LOD sections behind chunk jobs (§6.6). |
| `lod/` **[built, Phase 4]** | The grid and coordinates (`grid.ts`, mirroring `lod.h`); `LodSystem` (`lodSystem.ts`): the LOD octree around the camera (§6.6, ADR 0012) — screen-space-error selection (`frustum.ts`), parent-until-children-ready swaps with the streamed chunks as level 0, the LOD index and `LodRequest`s for modified sections, jobs by projected cell size to the worldgen and meshing pools, skirts, and a cache bounded by `LOD_CACHE_MB`. |
| `mesh/` **[built, Phase 3d]** | Greedy mesher (`mesher.ts`, pure TypeScript) and its worker pool (`pool.ts`, `worker.ts`: `cores − 2` module workers, 1–4, two jobs each; voxels in and geometry out as transferred buffers). Input: a chunk's voxels with a one-voxel apron from its neighbours (34³). Faces are culled like collision (hidden by full cubes; water by water; slab sides by slabs; a slab's top always open; a ladder draws only its facing plate); faces of full cubes and water merge into rectangles of one material per slice, slabs and ladders stay one quad per face. Render meshes only: collision stays in the sim core (`TerrainCollision`, the same C++ as the server, unit quads, PLAYER_CONTROLLER.md §5), so prediction collides with exactly the server's geometry. **[built, Phase 4c]** LOD sections in the same workers (`lodMesher.ts`, §6.6): 34³ cells in, flat-coloured greedy meshes in cell units plus per-side skirts out. |
| `render/` **[built: terrain chunks, LOD sections, player capsules, camera, debug lines, block outline]** | Thin Dwell-owned render interface (chunk meshes, dynamic body meshes, player views, camera rig, debug draw) implemented on **Three.js / WebGL2** ([ADR 0002](./adr/0002-client-renderer.md)). Chunks use packed custom geometry and a Lambert material whose shader repeats a texture once per block across merged quads (`uv` in blocks, a per-vertex atlas `tile` rectangle, `textureGrad` of tile + fract(uv) so mip selection has no seams); positions are camera-relative. Game code never touches Three.js objects directly. **[built, Phase 4c]** LOD section meshes (flat colour per material, skirts toggled per frame, chunks hidden where LOD draws) and a two-pass depth split — a far pass, then a depth clear and a near pass (§6.6). Built: chunk meshes from the meshing workers (water in a transparent pass), capsule players, the camera (75° vertical field of view, capped at 100° horizontal on wide screens, `fov.ts`), debug line segments, and the outline of the targeted block (Phase 3d; half height on slabs). **Block textures** (`textures.ts`): generated at startup from tiled noise — periodic value-noise fBm whose lattice wraps at the 32-texel tile, so every tile is seamless across blocks — for grass (top, side with a grass fringe, dirt bottom), stone (also slabs), the terrain generator's sand, banded sandstone, gravel, snow, logs (bark sides, ringed ends), leaves, and coal, iron, and gold ores (stone with mineral clusters), plus dirt, cracked bedrock, rippled water, ladders (rails and rungs), and the launch pad (ring and arrow); every visible material is textured (a test checks it); packed in a 512² atlas (8 × 8 cells) with 16-texel wrapped gutters (mipmapped without bleeding, nearest-filtered up close), built once per page (`sharedAtlas`; the hotbar's swatches come from it). Vertex colours carry face shading (and the flat colour of untextured materials). |
| `physics/` | Debris world (Phase 6) in the sim-core WASM; the prediction world lives in `sim/`. The client does not use separate Jolt JS bindings. |
| `interp/` | Tier 1 transform interpolation (and bounded extrapolation), Phase 5; player interpolation is in `game/remotes.ts`. |
| `debris/` | Tier 2 cosmetic debris spawn, simulation, and cleanup. |

The client never mutates the voxel grid on its own authority. Block edits are sent as
requests; the visual change is applied when the server's reliable delta arrives (an optional
optimistic "ghost" may be shown meanwhile).

### 5.1 Threads and workers

Per [ADR 0007](./adr/0007-threading-model.md):

| Context | Web (Pages) / Capacitor | Electron | Native server |
|---|---|---|---|
| Main thread | Input, render, UI, networking, client sim (prediction) | same | Main loop (tick) |
| Sim core | Single-threaded WASM in a worker (integrated server) + the client's own instance on the main thread (prediction; debris later) | Multithreaded WASM (COOP/COEP) | Native, Jolt `JobSystemThreadPool` |
| Worldgen (and `GenerateLod`, Phase 4) | **[built]** Worker pool (LOD jobs queue behind chunk jobs) (cores − 2, 1–4) for the client; the local-mode server generates on its tick within a budget | Worker pool or threads | **[built]** Thread pool (cores − 2) |
| Meshing (chunks, LOD sections; lighting later) | **[built]** Worker pool (cores − 2, 1–4) | Worker pool | — (server does not mesh for rendering; it downsamples LOD sections off the tick, §6.6) |
| Storage (SQLite) | **[built]** Inside the sim-core worker (OPFS), committed between ticks | Same | **[built]** I/O thread |

Workers share no memory; jobs and results are transferable `ArrayBuffer`s. Pool sizes are capped,
lower on mobile.

---

## 6. Voxel World

### 6.1 Grid & chunks **[in progress]**

> **Prototype content.** Every material that exists today — its name, look, and role (the
> grass/dirt/stone layering, sand, snow, logs and leaves, the coal/iron/gold ores) — and the
> terrain's style (§6.3: biomes, trees, boulders, ore distribution) are **placeholders** that
> exercise the systems: meshing, textures, collision shapes, streaming, generation, and editing.
> They are not Dwell's block set or world design and deliberately do not constrain it. The
> architecture fixes only the *mechanisms*: a `u16` material id with per-material properties
> (shape, liquid, climbable, strength, density, render style), one material table shared by
> server and client (mirrored in TypeScript and checked by a test), and an indestructible anchor layer at the bottom of the world. Real
> content replaces the prototype set later, through the generator version and the material table,
> without architectural change.

Built (Phases 1–3a, `server/core/include/dwell/core/voxel.h`): material table — air, bedrock,
stone, dirt, grass, stone slab, ladders (`ladder_n/e/s/w`), water, a debug launch pad, and the
terrain generator's sand, sandstone, gravel, snow, log, leaves, and coal/iron/gold ores — where
each material has a collision **shape** (`Empty`, `Full`, `SlabBottom`), `climbable` + facing,
`liquid`, and `launch_speed` (PLAYER_CONTROLLER.md §6); 32³ chunks with revisions (generated chunks
start at revision 0); `VoxelWorld`, generate-on-access with eviction of unmodified chunks on the
server, and *streamed* (no generator; missing chunks read as air) in the client sim; a flat test world (grass top face at y = 0,
bedrock at the bottom); and a **playground** generator (flat world plus slab stairs, a block step,
a 1×2 doorway, a crawlspace, a ladder to a ledge, a pool, and a launch pad near the spawn) for
movement testing. **Terrain collision** (`terrain_collision.h`, Phase 2; regions Phase 3c): static
Jolt bodies with a `MutableCompoundShape` of per-chunk `MeshShape`s (unit quads per exposed face, no
greedy merge), built around players, rebuilt in the same tick as an edit, and unloaded far from
players. For the 8,192 km world each body sits at the centre of a 2 048 m *region* and holds every
chunk around the players *anchored* to it, so its sub-shape offsets stay small; a player collides
only with its anchor's body and re-anchors, with hysteresis, well inside the next region — there is
no seam under a player (PLAYER_CONTROLLER.md §5). The procedural terrain generator and streaming are
built (§6.3), and so are block edits (§6.5, Phase 3d): the placeable set is derived from the table
(`Placeable`: not air, liquid, indestructible or a launch pad), mirrored by the client's `placeable`
flags and checked by tests on both sides.

- Voxel = 1 m cube; `uint16` material ID (0 = air). Material table defines density,
  strength, and render properties and is shared by server and client.
- Chunk = **32 × 32 × 32** voxels, addressed by `ChunkCoord(int32 x, y, z)`. The grid is 3D in
  every respect — generation, storage, streaming (§6.3) and LOD (§6.6). The world is 256 chunk
  rows (8,192 m) tall; `int32` coordinates reach the rim of the 8,192 km disc (chunk ±256 000) with
  room to spare.
- Wire / storage encoding **[built]**: per-chunk **palette + run-length encoding**
  (`ChunkData Explicit`, §8.3). Voxels are taken in layer order (x fastest, then z, then y) so
  horizontal strata make long runs; the palette lists materials in order of first appearance; each
  run is a LEB128 length and a palette index (u8, or u16 above 256 entries). The encoding is
  canonical, so C++ and TypeScript round-trip each other's bytes exactly (golden vectors).
  In the world file the same bytes are zstd-compressed (§6.4); the wire does without general-purpose
  compression (QUIC and SCTP do not compress, but Explicit chunks are rare in generated mode).
- Each chunk carries a monotonically increasing `revision` so clients can detect and
  discard stale or out-of-order updates and request resync. **[built, Phase 3d]** Generated chunks
  are revision 0; every `VoxelModification` that touches a chunk advances it by exactly one
  (`Chunk::SetAt` + `BumpRevision`), so the next expected revision is always current + 1.

### 6.2 Voxel states (the three physics categories)

| State | Where it lives | Physics cost | Synchronized |
|---|---|---|---|
| **Static terrain** | Master grid; baked into per-chunk Jolt `MeshShape` | ~0 (static bodies) | Reliable chunk data + deltas |
| **Tier 1 dynamic** (gameplay-critical) | Server Jolt world as one `CompoundShape` body per cluster | Active | Unreliable snapshots, 20 Hz |
| **Tier 2 dynamic** (cosmetic debris) | Client-local Jolt world only | Client only | Not synchronized; derived from a reliable event |

Static collision uses a per-chunk `MeshShape` of unit face quads (a `HeightFieldShape` cannot
represent overhangs/caves), held in per-region bodies (§6.1). Collision exists only for full-detail chunks
near players; LOD terrain (§6.6) is render-only.
Both server and client build the same collision mesh from the same chunk data so client
prediction collides with the same geometry the server does.

### 6.3 Terrain Generation **[in progress]**

Terrain is **procedural, seeded, and deterministic**: an unmodified chunk is a pure function
`generate(worldSeed, generatorVersion, ChunkCoord)`. The server is authoritative, but because
generation is deterministic, the network and disk only need to carry *differences* from the
generated baseline.

The pipeline structure below (deterministic stages, lattice-sampled fields, order-independent
features) is architecture; its current *content* — the biomes, surface materials, ores, trees and
boulders — is prototype (§6.1).

**Built (Phases 3a, 3c):** the generator (`server/core/include/dwell/worldgen/terrain.h`,
`src/worldgen/`) is **generator version 4** (3c: the planet-scale world as version 3; Phase 4 adds
super tall massifs as version 4; versions 2 and 3 are retired — a world saved with one loads as the
flat world) and
the default for dedicated servers and local mode. Versions 0 (flat) and 1 (playground) remain for
tests and movement work. Players spawn at the generator's spawn point: the first level, open,
tree-free land found in an 8 m spiral from the origin. A chunk takes ~1.2 ms to generate natively
(Release) and ~1.5 ms in WASM, anywhere in the world (`dwell_worldgen_inspect`). Debug tooling:
`dwell_worldgen_inspect [seed] [x] [z] [m/char] [slice]` prints an ASCII biome/height map (with
biome shares, timings, and the spawn; blank beyond the rim) or a 1:1 vertical section, at any
coordinates. In game (Phase 3e), F4 shows the terrain's biome/height map around the player and
the F3 overlay the player's chunk regenerated and diffed against the world's; `dwell_world FILE
diff` does the same for a world file (§6.4). **Built (Phase 3b):** streaming, the verification chunk, and the generation pools
(below).

#### World bounds **[built, Phase 3c]**
A planet-scale world ([ADR 0011](./adr/0011-planet-scale-world.md)):
- **Horizontal:** a disc of `WORLD_RADIUS` = 8 192 000 m (8,192 km, ≈ 2.108 × 10⁸ km²) centred on
  the origin: a column (x, z) is inside when x² + z² < `WORLD_RADIUS`² (`InsideWorldDisc`).
  Columns outside it generate nothing — not even bedrock, in every generator — so the rim drops
  into the void, which kills (a later phase will develop the edge).
- **Vertical:** `WORLD_MIN_Y` = −2 048 to `WORLD_MAX_Y` = 6 144 (8,192 m, 256 chunk rows), with
  `SEA_LEVEL` = 0: three quarters of the height above sea level, one quarter below.
- **Precision:** float32 steps by 0.5 m at 8,192 km, so Jolt is built with
  `JPH_DOUBLE_PRECISION` on the server and in every WASM build (`RVec3` world positions through the
  controller, probes and prediction; terrain collision in regions, §6.1); wire positions are f64 or
  1/256 m fixed point (§8.3); worldgen never converts a whole world coordinate to float (see
  Determinism). The client sim reports positions as doubles; Three.js combines object and camera
  translations in double precision, and debug lines are drawn relative to their first point. The
  player controller suite, golden trace and netcode tests run both at the origin and ~8,000 km
  from it (`--dwell-origin-x=far`), natively and in WASM, with the same results.
- **Scale of terrain:** a placeholder planet-scale layer (prototype content, §6.1; generator
  version 3, extended in 4): continents and oceans a few hundred kilometres across (a 262 km fBm
  shifting continentalness), ranges up to ~1 950 m on large landmasses, **massifs** in the cores
  of the largest ranges whose crests rise a further 3,600 m (peaks ~5.3–5.6 km, about 1 % of land
  above 3 km; kept under `WORLD_MAX_Y` with room for trees), and basins to ~−540 m under large
  oceans. The full-detail world (~5 × 10¹³ chunks) is never generated wholesale — distant terrain
  comes from the LOD system (§6.6).

Everywhere, the generator leaves everything below `WORLD_MIN_Y` (the void, which kills) and at or
above `WORLD_MAX_Y` empty, and the bottom layers (`y < WORLD_MIN_Y + BEDROCK_LAYERS`) are
indestructible **bedrock** — the primary anchor for structural integrity (§7.1).

#### Generator pipeline **[built]**
Executed per chunk. Every stage reads only noise and hashes of world coordinates, never another
chunk's data, so chunks can be generated in any order and in parallel. 2D fields are sampled on a
4-column lattice and 3D noise on a 4-voxel lattice, then interpolated (bilinear in x, z, then
linear in y); the chunk path and the point queries (`ColumnAt`, `SolidAt`, `GroundY`) share that
arithmetic, so features placed by point queries agree with the chunks.

1. **Climate (2D).** Low-frequency fBm for continentalness, erosion, temperature, and humidity.
   Biome weights (desert, snowy, forest, plains) blend smoothly across borders; the column's biome
   (ocean, beach, plains, forest, desert, snowy, mountains) is the dominant one after height rules.
   Version 3 adds the planet-scale fields (a 262 km fBm for land and ocean, a 49 km ridged fBm
   for ranges).
2. **Base height (2D).** A continentalness spline (deep ocean ~−42 m → coast ~2 m → uplands
   ~40 m; sea level 0), plus biome-blended hills (fBm, amplitude 4–12 m by biome), plus ridged
   fractal mountains where continentalness is high and erosion low (up to ~190 m), plus the
   planet-scale ranges (1,800 m at a crest, 5,400 m where the 262 km field is highest: the
   massifs of version 4) and basins.
3. **Density (3D).** `density = (height − y) + overhang × overhangNoise3D(x, y, z)`; solid where
   `density > 0`. The overhang amplitude is ~3.5 m on land and up to ~17 m in mountains, giving
   overhangs and cliffs.
4. **Caves (3D).** Carve "spaghetti" tunnels (`a² + b² < t` of two noises) and "cheese" caverns
   (one noise above a threshold), faded in from 3 m to 15 m below the surface and out just above
   the bedrock.
5. **Surface & strata.** A top-down column pass counts solid voxels below open sky or sea (cave
   air does not start a surface): grass over dirt (plains, forest, mountain slopes), snow over dirt
   (snowy; mountain tops above 900 m), sand over sandstone (desert, beach), sand or gravel under
   water, bare stone on steep slopes; stone below. Open space below `SEA_LEVEL` fills with water;
   the bottom `BEDROCK_LAYERS` are bedrock.
6. **Stability pass.** Solid components that do not touch a chunk face and have fewer than 48
   voxels are removed (to water in open sea, otherwise air), so newly generated terrain does not
   collapse the first time a nearby voxel changes. The check is within the chunk: a piece that
   crosses a chunk border is kept. Large generated overhangs are allowed and are subject to normal
   integrity rules once edited.
7. **Ores.** Seeded blobs per 16³ cell replace stone: coal (y ≤ 136), iron (y ≤ 8), gold
   (y ≤ −48).
8. **Features.** Boulders (24 m cells) and trees (7 m cells; oaks, and spruces in snowy and
   mountain biomes) at hashed positions per cell, on the ground found by `GroundY`; each writes
   only voxels inside the chunk being generated. Boulders and leaves fill only air, logs air and
   leaves, and features apply in a fixed order (boulders, then trees, each by cell), so
   overlapping features resolve the same way in every chunk. Hand-authored structures come later.

#### Determinism **[built]**
Server (native), local mode (WASM), and client (WASM) must produce **bit-identical** chunks
([ADR 0010](./adr/0010-worldgen-noise-numerics.md)):
- The generator lives in `server/core` (C++), and the client runs the **same code compiled to
  WASM** — there is no second TypeScript implementation.
- Noise uses integer hashing for gradients and evaluates in strict IEEE float: only `+ − × /`
  and comparisons, `-ffp-contract=off`, no `-ffast-math`, no library calls.
- A golden test (`server/tests/worldgen/golden/chunk-hashes.txt`) hashes chunks across the
  pipeline for two seeds — surface, caves, deep rock, bedrock, sky, the top of the world, ocean,
  mountains, the rim, terrain ~8,000 km out, and a massif ~5.4 km up; CI runs it natively and under WASM (Node) and in
  the client's worldgen module.
- `generatorVersion` is bumped for any change that alters output (and the golden hashes are
  regenerated); saved worlds record it.
- **[built, Phase 3c]** Planet-scale coordinates (ADR 0011): noise never converts a whole world
  coordinate to float. World coordinates are integers and wavelengths integer metres; each octave
  splits `x · 2^octave` (64-bit) into an integer lattice cell (feeding the hash) and the offset
  within it, `remainder / wavelength` — one correctly rounded float division (`Lattice`,
  `noise.h`). A test shows float coordinates losing a fine octave's detail ~8,000 km out and the
  split ones not.
- **[built, Phase 4a]** `GenerateLod` (§6.6) follows the same rules and has its own golden
  hashes (`server/tests/worldgen/golden/lod-hashes.txt`: the surface near the spawn, mountains,
  ocean, the rim, terrain ~8,000 km out, the index level and the root), checked natively, under
  WASM (Node) and in the client's worldgen module.

#### Authority, storage, and streaming **[built]**
- **Server storage.** The server's `VoxelWorld` holds chunks only while they are needed: every 64
  ticks, chunks more than 3 chunks (Chebyshev) from every player are evicted if they can be had
  again — unmodified ones (revision 0) by generating them, modified ones once saved (their revision
  in the world file) by reading them back; modified chunks not yet saved stay. With a world file
  (§6.4, Phase 3e) the server knows every saved chunk's coordinates and revision from startup, reads
  saved chunks around players from the file (≤ 16 per tick) instead of generating them, never treats
  them as air, and streams them Explicit. Terrain collision likewise unloads chunks more than 2 chunks from
  every player anchored to a region (`TerrainCollision::Retain`, reusing compound slots and
  destroying unused region bodies), so memory stays bounded while players move.
- **Server generation** (`core/worldgen_pool.h`). Each tick the server asks a `WorldgenPool` for
  the missing, non-air chunks within 2 chunks of each player (nearest first; collision reaches 1 chunk and
  meshing reads its neighbours), then those full-mode clients wait for; results are collected at
  the start of the next tick. The dedicated server runs `cores − 2` generation threads; the
  browser's local mode has no threads and generates on the tick within a 4 ms budget. A chunk
  still missing when collision needs it is generated on the spot (counted as
  `generated_on_access`; a test checks walking never needs it). The spawn region (radius 2
  chunks) and the verification chunk are generated at startup.
- **Air chunks** **[built, Phase 3c]**. A generator's air test (`AirTestFor`; for terrain
  `TerrainGenerator::IsAirChunk`, exactly its own "sky above everything" shortcut, cached per chunk
  column) names unmodified chunks that are all air: outside the world's rows or disc, or above the
  highest reach (height + overhang + tree margin) of every column around them. The server never
  generates or stores them — a generated `VoxelWorld` reads them as air — and streams them as
  payload-free `Air` messages in either mode, so open sky costs neither side generation, memory or
  meaningful bandwidth (a test streams a view high in the sky with no chunk generated).
- **Verification.** `Welcome` carries `worldSeed`, `generatorVersion`, and the **verification
  chunk** (the chunk under the spawn). The client generates it in a worldgen worker and replies
  `WorldgenCheck(hash)` (FNV-1a 64 of its voxels, `ChunkHash`). A matching hash puts the session in
  **generated mode**; a mismatch, or 0 (`?chunks=full`, or no working worker), puts it in
  **full-chunk mode**, where every chunk is sent explicitly. Nothing is streamed before the check.
- **`ChunkData`** has three forms: `Generated(coord, revision)` — "generate this yourself, no
  changes" (18 bytes); `Air(coord)` — an unmodified all-air chunk: nothing to generate or store
  (18 bytes); and `Explicit(coord, revision, palette+RLE)` for modified chunks and for
  full-chunk mode (§6.1 encoding; for terrain chunks near the surface, median ~2 KB, up to ~8 KB;
  all-air or all-stone ones 26 bytes). `ChunkUnload(coords)` tells the
  client to drop chunks.
- **Interest management** (per client, `Server::StreamChunks`). The view is a **sphere** of
  `VIEW_RADIUS_CHUNKS` around the player's chunk (`x² + y² + z² ≤ r² + r`), clipped to the world's
  rows. Chunks the client lacks are sent nearest first, at most
  `MAX_CHUNKS_PER_TICK` per tick and within `CHUNK_BYTES_PER_SECOND` (a byte credit refilled each
  tick; one chunk may overdraw it). When the player's chunk changes, chunks beyond the view plus
  `UNLOAD_MARGIN_CHUNKS` (hysteresis) are unloaded (outside the sphere of r + margin). Dead players
  stream around their body. Beyond the view, the LOD system (§6.6, Phase 4) takes over.
- **Client** (`world/chunkStream.ts`, `worldgen/`). Generated chunks go to the worldgen worker
  pool (`cores − 2` module workers, 1–4, each with its own `dwell_worldgen.wasm` — the generator
  alone, ~30 KB; up to 4 jobs queued per worker; voxels come back as transferred buffers) and then
  into the client sim's streamed world; Explicit chunks are decoded on the main thread; Air
  chunks only count as loaded (nothing stored or meshed). Pending
  generations are cancelled on unload. Render meshes are built by the meshing worker pool (§5
  `mesh/`): changed chunks are sent nearest first, a few jobs per worker in flight, their voxels
  (with the apron) copied out of the client sim within a 4 ms frame budget; a chunk arriving marks
  its loaded neighbours for re-meshing, and a chunk waits while a neighbour is still being generated.
  Voxel modifications (§6.5) apply in revision order: those of a chunk still being generated are
  held until it arrives, one for a chunk held as Air creates it, stale ones are dropped, and a gap
  sends `ChunkResync` for the chunk (answered with its current state) and ignores it until then. Prediction starts once the chunks within 1 of the player
  are loaded ("Loading terrain…"). The client sim treats missing chunks as air, and its terrain
  collision rebuilds a chunk when it or a neighbour arrives.

### 6.4 World Persistence **[built, Phase 3e; LOD cache Phase 4b]** (bodies, backups, export UI: later)

Decision: [ADR 0006](./adr/0006-world-persistence-sqlite.md). Each world is **one SQLite database
file** holding **all** of its data; nothing about a world lives in side files.

| Table | Contents |
|---|---|
| `meta` | Format version, world seed, generator version, spawn, world time, timestamps, preview image |
| `settings` | Name, MOTD, icon, max players, visibility, password hash, online/offline mode, physics/view caps, autosave and backup policy |
| `chunks` | Modified chunks only: `(cx, cy, cz)`, revision, generator version, zstd-compressed palette + RLE blob (same encoding as `ChunkData Explicit`) |
| `lod_sections` | **[built, Phase 4b]** Cache of modified LOD sections (§6.6): `(level, i, j, k)`, `lodRevision`, dirty flag, generator version, zstd-compressed palette + RLE blob of the 34³ cells (empty while dirty and never computed); derivable from `chunks`, rebuilt on a generator version change |
| `players` | Keyed by device public key: display name, state blob (position, health, later inventory), first/last seen |
| `bodies` | In-flight Tier 1 clusters (voxel layout, transform, velocities) |
| `permissions` | Ops, bans, allow-list by public key, with reason/by/when |

**Built (schema v1):** `meta` (key/value: `format_version`, `world_seed`, `generator_version`,
`spawn_x/y/z`, `world_tick`, `created_at`, `saved_at`), `settings` (key/value text), `chunks`
(`WITHOUT ROWID`, keyed by coordinate), `players` (state blob v1: feet `f64×3`, health) and
`permissions` (`op` / `ban` / `allow`, reason, granted by, when). **Schema v2 (Phase 4b)** adds
`lod_sections` (a migration from v1, tested). The server restores it at startup (dirty rows are
recomputed), drops and rebuilds it from the chunks when its rows belong to another generator
version, and derives it for saved chunks it does not cover (worlds saved before Phase 4); each
autosave writes the sections written since the last save and those still to compute (flagged
dirty) in the same transaction as the chunks. `bodies` (Phase 5) arrives as a migration.

- **Same code everywhere:** SQLite and zstd are compiled into `server/core` (`core/storage`:
  `WorldDb`, the file's schema and records; `WorldStore`, the server's handle on it).
  Backends: the native file VFS (WAL mode, `synchronous=NORMAL`) for dedicated servers; for
  browsers, Dwell's VFS over OPFS `FileSystemSyncAccessHandle`s inside the sim-core worker
  (`opfs_vfs.cpp` + `client/src/local/worldFiles.ts`: no cross-origin isolation needed; files under
  the `dwell/worlds/` OPFS directory). It has no shared memory, so browser files use exclusive
  locking and a rollback journal (`TRUNCATE`, `synchronous=FULL`); opening converts a file written
  by the other side, so a world file moves between them unchanged (golden files written natively and
  in WASM are opened by both builds in CI). Handles are exclusive: a second tab on the same world
  runs without persistence. Capacitor will use the OPFS VFS or a native-file plugin VFS per platform.
- **Loading:** a saved world's seed, generator version and spawn replace the launch options'; its
  world time resumes; the saved chunks' index is read at startup and chunks are read on demand
  (§6.3); a returning player (by device key) spawns where it was saved with its health, unless it
  was dead.
- **Saving:** every `AUTOSAVE_SECONDS` (local mode: 5 s), the dirty chunks (copied), every joined
  player, the players who left since, and meta are prepared on the tick and committed in one
  transaction — natively by the `WorldStore`'s I/O thread on its own connection (WAL: the tick's
  reads never wait), in the browser inline (no threads). A chunk counts as saved at the revision
  that committed; a failed save leaves its chunks dirty for the next. Also on shutdown
  (`dwell_server` saves on SIGINT/SIGTERM and waits for the commit) and when the local-mode page is
  hidden or closed. A new world's meta is written at once.
- **Crash safety:** a save is one SQLite transaction. A test VFS that drops every write after the
  Nth (simulating a crash at each write of a save, natively over WAL and in WASM over the rollback
  journal) always leaves the previous save or the new one, intact.
- **Backups** **[planned]:** dedicated servers take rotating backups with SQLite's online backup
  API.
- **Portability:** a `.dwellworld` file is the database itself and opens natively and in the
  browser build (tested); **[planned]** export/import UI across dedicated servers, browsers and apps.
- **Migrations:** `PRAGMA user_version` (mirrored in `meta.format_version`) with ordered migrations
  on open, each in its own transaction; a file from a newer build is refused.
- **Settings and permissions** **[built: launch options]:** `dwell_server --world FILE` (default
  `world.dwellworld`; `""` keeps the world in memory) opens the world; `--name`, `--motd`,
  `--max-players`, `--edits everyone|ops|nobody` and `--flight everyone|ops|nobody` (creative
  flight, §9.1; default everyone) are saved into `settings`, and `--op KEY` /
  `--ban KEY` into `permissions`; stored settings and permissions apply at startup (ops may edit
  under `--edits ops` and fly under `--flight ops`, banned keys are refused with `Reject(Banned)`, and with `allow_list` set only
  `allow` keys may join, else `Reject(NotAllowListed)`). **[planned, Phase 8]** Admin commands.
- **Tooling:** `dwell_world FILE info` (format, meta, settings, permissions, integrity) and
  `dwell_world FILE diff [cx cy cz]` (regenerate saved chunks from the world's seed and generator
  and diff them: changed voxels per chunk, flagging saved chunks identical to generation).

### 6.5 Block Interaction & Inventory **[built, Phase 3d]**

Players break and place blocks. Server-authoritative like every voxel change
(`core/block_edit.h`, `client/src/interact/`):

- **Targeting (client):** each frame `RaycastBlock` (a DDA from the eye along the view,
  `REACH_DISTANCE` long, in float relative to the eye's cell so it is exact anywhere in the world)
  finds the first *targetable* cell — anything but air and liquids, as its shape's box (slabs the
  bottom half, everything else the whole cell) — and the face the ray entered; the renderer
  outlines the cell. The client sim runs the same C++ as the server.
- **Actions:** desktop — left click breaks the targeted block, right click places the selected
  block against the targeted face (only while the pointer is locked; the first click locks it).
  Touch — a tap on the view (a touch on the look half that moves ≤ 12 px and lifts within 500 ms)
  acts, and a latching Break/Place button picks which action. The client sends at most one
  `BlockEditRequest` (§8.3) per `BLOCK_EDIT_INTERVAL_MS`.
- **Validation and application (server):** requests queue per session (≤ 8 per tick) and apply at
  the start of the next step, in order: the player must be alive and allowed to edit
  (`ServerConfig::edits`: everyone, ops only, or nobody), within a token-bucket rate (one edit per
  `BLOCK_EDIT_INTERVAL_MS`, bursts of 3, so edits bunched by the network pass), then
  `CheckBlockEdit`: the target cell within `REACH_DISTANCE` + 1 m (latency slack) of the server's
  view of the eye (checked first, so a far request reads nothing); a targetable cell; the eye in
  front of the targeted face and a ray to its centre or one of four points near its corners
  reaching the target first (line of sight); Break — not indestructible (bedrock); Place — a
  placeable material, into an air or liquid cell inside the world's rows and disc, and (solid
  blocks) not overlapping any player's capsule. Accepted edits of a step are batched into one
  `VoxelModification` (one entry per chunk, its revision + 1) sent to each client streaming a
  changed chunk; clients streaming it later get the chunk Explicit. The client does not predict
  edits: the change shows when the modification arrives (one RTT), and terrain collision — server
  and client alike — rebuilds before the next tick's probes (PLAYER_CONTROLLER.md §5). Rejections
  are counted per session (`SessionStats`), with the last reason.
- **Inventory:** creative-style and infinite — every *placeable* material in the (prototype, §6.1)
  material table is always available: all but air, liquids (water), the indestructible anchor
  (bedrock: placed bedrock could never be removed) and debug materials (the launch pad); nothing is
  consumed or collected. Ladders are one hotbar slot: placed against a side face the ladder is
  mounted on that block, facing out of it; on a top or bottom face it faces the player. A hotbar
  HUD shows the palette with the selected block highlighted; number keys (1–9, 0) and the scroll
  wheel (desktop) or tapping a hotbar slot (touch) change the selection. Selection is client-side
  UI state and travels in each `BlockEditRequest`. Collected, finite inventories are out of scope
  for now.

### 6.6 Level of Detail: the Whole-World View **[built, Phase 4]** (frame-rate check on real hardware outstanding)

Decision: [ADR 0012](./adr/0012-lod-octree.md) (concepts from the Distant Horizons mod, adapted
to 3D). Everything that should be visible from the camera — on a mountain, in the air, or flying
high above the disc — is drawn, at a detail that drops with distance.

**Grid.** A 3D octree of **sections**. At level L a cell is a 2^L m cube and a section is 32³
cells (32 × 2^L m on a side); **level 0 is the chunk grid**. Section coordinates `(L, i, j, k)`
count from the corner (−2²³, `WORLD_MIN_Y`, −2²³), so cells of consecutive levels nest exactly
and one root section at `LOD_MAX_LEVEL` = 19 (16 777 km) holds the whole disc. From level 8
(8,192 m sections) upward a section spans the world's full height, so those levels have a single
row and the octree behaves as a quadtree.

| Level | Cell | Section | Drawn at distances of about (1080p, 75° FOV, 4 px) |
|---|---|---|---|
| 0 | 1 m | 32 m | The streamed chunks around the player (§6.3) |
| 1–4 | 2–16 m | 64–512 m | From the edge of the streamed view to ~5.6 km |
| 5–8 | 32–256 m | 1–8 km | 5.6–90 km; level 8 is the LOD index level |
| 9–19 | 512 m–524 km | 16–16 777 km | Beyond 90 km, out to the whole disc from altitude |

(Level L is drawn from about 176 × 2^L m under those settings; the world is flat, so from the
ground the view reaches the rim wherever terrain does not block it. `LOD_PIXEL_ERROR` was 2 px
in ADR 0012; at 2 px a view from the ground draws ~8,200 sections, at 4 px ~2,800 — measured in
`lodSystem.test.ts`'s scenario — so the defaults are 4 px desktop, 8 px mobile.)

**[built, Phase 4a]** The grid and coordinates (`server/core/include/dwell/core/lod.h`, mirrored by
`client/src/lod/grid.ts` and tested on both sides), `Downsample`, `GenerateLod` for every
generator (the terrain's in `worldgen/terrain.cpp`; the flat and playground worlds share a flat
one, the playground's features being far below a cell) and the column bounds that classify
sections without generating them (`LodBoundsAt`, `LodKindFromBounds`), exported by
`dwell_worldgen.wasm` (`dwell_worldgen_lod`, `dwell_worldgen_lod_bounds`; `ChunkGenerator.lod`
and `.lodBounds` in `worldgen/generator.ts`). A section takes 1–9 ms to generate in WASM, its
bounds ~1.5 ms.

**Content.** A section is 34³ `u16` materials: its 32³ cells and a one-cell apron from its
neighbours for culling border faces, in layer order (x fastest, then z, then y — the chunk
mesher's padded layout), encoded with the chunk palette + RLE codec taking the cells in that
order (`EncodeLodCells`; `writeLodCells` in TypeScript); a terrain section near the surface is
~1.5–6 KB.
- *Unmodified* sections come from `GenerateLod(seed, generatorVersion, L, i, j, k)` in
  `server/core/worldgen`: the generator evaluated per cell, with noise octaves and features
  smaller than a cell dropped (no aliasing). Deterministic native vs WASM (§6.3 Determinism), so
  the client generates them itself. Each cell samples the pipeline at its **centre column and
  bottom voxel** — the voxel whose solidity decides a floor under `Downsample` (below) — so
  generated and downsampled sections agree: fractal octaves whose lattice is finer than a cell
  are dropped (still normalised by the full amplitude; ridged sums by the kept one), tunnels
  (~4 m wide) are carved only in 2 m cells, caves only within three cells of the surface (deeper
  cave air is never seen from afar, and leaving it solid keeps LOD meshes free of enclosed
  faces), trees only in cells up to 4 m and boulders up to 2 m (a cell takes a feature's
  material when the feature fills at least half of it), ores and the stability pass not at all;
  surface materials follow the column's depth in metres. The apron below the world reads as
  bedrock, so the world's floor is never drawn. Measured against the downsample of generated
  chunks at the spawn and a site of each biome (levels 1–2, 3 in the mountains): ≥ 96% of cells
  agree in class (air, liquid, solid) and ≥ 97% of columns' surfaces are within one cell
  (`lod: generation` tests the tolerance: 95%, 95%, mean under half a cell).
- *Modified* sections (any modified chunk below them) are the downsample of their 8 children,
  recursively; unmodified children come from `GenerateLod`, level-0 children are chunks. A 2×2×2
  block becomes filled if ≥ 4 cells are not air — solids and liquids alike (a one-voxel wall or
  floor survives a level; a 1 × 1 pillar does not; a sea keeps its surface at levels whose cells
  are deeper than the sea instead of showing its floor). The material is the most common one
  among the **top filled cell of each of the block's four columns** (the surface seen from
  above), ties to the upper cells — so surface materials survive to the coarsest levels instead
  of the rock beneath them (`DownsampleBlock`, tested on crafted layouts). The flat world's
  generated sections equal the downsample of its chunks exactly (levels 1–3, tested).
- Sections that the generator's column height bounds prove all air, or buried with no exposed
  face, are skipped without generating anything: the bounds of a column of sections
  (`LodBoundsAt(L, i, k)`, from the 34 × 34 columns' 2D fields) give the heights above which
  every cell is air or sea and below which every cell is solid; `GenerateLod` classifies by the
  same bounds, so it returns `Empty` or `Buried` exactly when the bounds say so.

**Server** **[built, Phase 4b]** (`core/lod_propagation.h`, `Server::UpdateLod`). A chunk edit (any
source: edits now; collapses and re-bakes later) marks its level-1 section dirty. Each tick at most
`LOD_PROPAGATION_SECTIONS_PER_TICK` dirty sections — lowest level first (children before parents),
then nearest to a player — are snapshotted on the tick (the content of their modified children:
chunks for level 1, cached sections above) and computed off it: `GenerateLod` of the section with
the octant of each *modified* child replaced by that child's downsample (unmodified octants keep
the generated cells, so a build changes only the octants above it). Writing a section gives it the
next `lodRevision` from a server-wide counter and marks its parent dirty, up to the root (19
sections per edited chunk; a section dirtied again while being computed is recomputed after).
The dedicated server computes on one thread; the browser's local mode on its tick within 2 ms.
A modified section's stored apron is generated; the server fills it from its modified same-level
neighbours' borders when it sends the section. Sections are cached in `lod_sections` (§6.4). In a
test, an edit reaches the root within 19 ticks and a far client's index 8 ticks after the edit.

**Streaming** (§8.3) **[built on the server, Phase 4b; client 4c]**. After `WorldgenCheck`, the server sends the **LOD index**: every modified
section at `LOD_INDEX_LEVEL` = 8 with its revision (`LodIndex`), then changes as propagation writes
them (`LodIndexUpdate`, coalesced to at most one per `LOD_INDEX_UPDATE_MS`). From the index the
client knows exactly which sections at level ≥ 8 are modified, and that any section below an
unindexed level-8 section is not; it generates all unmodified sections itself. For the rest it
sends `LodRequest(L, coord, knownRevision)` and receives `LodData` — `Generated`, `Explicit`, or
`Unchanged` — on the `lod` stream within `LOD_BYTES_PER_SECOND` (a byte credit per tick, like
chunks). `Generated` means nothing under the section is modified *yet*: a section still being
computed answers `Generated`, and the index update that follows its level-8 ancestor's write makes
the client ask again. Requests are limited to `LOD_REQUESTS_PER_SECOND` per client (a token
bucket; the rest are dropped and counted) and at most 256 queued. When an index entry's revision
changes, the client re-requests the sections it holds below it. In full-chunk mode (§6.3) every
request is answered `Explicit` (unmodified sections generated by the server's LOD thread, 256 kept).
Every player's builds are therefore visible from anywhere.

**Client** **[built, Phase 4c]** (`lod/lodSystem.ts`, `mesh/lodMesher.ts`, `render/three`).
Each frame the octree is walked from the root around the **camera** (the eye):
- A node is refined while its cells project larger than `LOD_PIXEL_ERROR` pixels (screen-space
  error; equivalent to a log-distance rule at a fixed field of view, and right for altitude and
  zoom) — in every direction: detail depends on distance only, not on where the camera looks,
  so turning shows what is already loaded instead of popping in (playtest feedback). The view
  decides only the load order: work outside it (and not closer than its own size) ranks 8× lower,
  so what the camera faces loads first and the ring around it after. Level-0 nodes are the streamed chunks: a level-1 section refines into its 8
  chunks when all are loaded and meshed (or air); chunks whose level-1 section is not refined are
  hidden, since LOD draws there (all chunks show until the root is ready). Away from the body the
  finest level drawn is 1. **The player's surroundings never wait for coarse levels:** a level-1
  section around the camera whose chunks have been drawable for `FORCE_CHUNKS_AFTER_MS` (1 s)
  is always reached — the walk descends to it through ancestors whose children are not all
  ready, drawing the ready siblings and leaving the unready ones empty (sky) until they are,
  never drawing a coarse section over the chunks. A device that keeps up never takes this path;
  one whose LOD generation is slow or stalled still shows the world around the player.
- A parent stays drawn until **all its children** are ready (meshed, or known empty or buried),
  then they swap in; unused children are evicted only as whole sibling sets, least recently used
  first, while the cache is over `LOD_CACHE_MB` — the view never has holes. A test walks, turns
  and rises to 2,000 km with jobs finishing in random order, and checks every frame that sampled
  points of the view lie in exactly one drawn, empty, buried or chunk-refined section.
- Empty and buried sections come from their column's bounds (one `lodBounds` job per column of
  sections), the rest from `GenerateLod` in the worldgen pool (a queue behind chunk jobs) and
  meshing in the meshing pool, with at most the pools' capacities in flight. Jobs go in order of
  **projected cell size** (largest first): a parent always before its children, coarse before
  fine and near before far — ADR 0012's "coarsest first, then nearest" globally left the camera's
  own ground coarse until the whole horizon was done.
- Requests: a node asks the server when it may be modified — at level ≥ 8 when the index has an
  entry under it, below when its parent's answer was `Explicit` — so a `Generated` answer covers
  a whole subtree, and a view with no modification in it asks nothing. An index change marks the
  entry's section and its ancestors stale; an `Explicit` answer with a new revision marks the
  held children stale (level ≥ 8 children only if indexed): re-asks go top down with the held
  revision, and `Unchanged` sections keep their content. Requests are paced under
  `LOD_REQUESTS_PER_SECOND` (60/s, 32 per message) and re-sent after 5 s unanswered. In full-chunk
  mode every node asks.
- Meshes are greedy-merged with one flat colour per material (the average of its texture tile in
  linear light, per face group, as a mipmapped sRGB texture averages from afar) and face shading,
  written as linear vertex colours like the chunks' decoded texels (sRGB values used directly drew
  distant land paler than the chunks — playtest), in cell units (a group scaled by the cell size). Border faces
  the apron hides go to a per-side **skirt** mesh, shown when the neighbour on that side is not
  drawn at the same level (and not buried), closing cracks between levels. A generated section
  next to a modified one takes that neighbour's border into its apron before meshing.
- **Depth:** 5 cm to beyond 16 000 km does not fit one depth buffer, so the frame renders a far
  pass and then, after a depth clear, a near pass (0.05 m to `LOD_NEAR_SPLIT_M`); everything is in
  both passes and each camera's frustum culls, so a section straddling the split is clipped at it.
  The far pass's near plane moves out to 0.8 × the camera's height above `WORLD_MAX_Y` (nothing is
  nearer up there), keeping depth precise from orbit. There is no distance fog for now: the whole
  world is drawn without haze out to the rim (`render/fog.ts` holds the switch; when on, its start
  and full-fog distances grow with height far above the terrain).
  The scene has no background colour — three.js would clear the far pass with it — the renderer's
  clear colour is the sky.
- Coarse sections: where a cell is taller than the relief, the column's top cell takes the
  material of the column's surface (not the bedrock its bottom voxel samples; a regression test
  covers it); a sea whose water no cell samples (the cell is deeper than the sea) shows water on
  top, as `Downsample` keeps it (`lod: sea`). From level 3 (`OPAQUE_LIQUID_LEVEL`, 8 m cells)
  liquids mesh opaque, culled and culling like solids and with skirts — a coarse sea has no floor
  beneath its surface to see through to; levels 1–2 keep the chunks' see-through water. Under
  comparison (`?lodwater=tint`, off by default): from level 3, liquids are left out and the floor
  faces they cover take the colour seen through the near water (its colour blended over the floor
  at its opacity, in linear light).
- Debug: the F3 overlay shows sections drawn per level, those shown as chunks, nodes, jobs in
  flight (generation, meshing, requests), cache use and LOD bytes/s; `?lodcolors=1` tints sections
  by level, `?lod=0` turns the LOD off, `?lodwater=tint` draws distant water as a tinted floor. Measured under Node with a fake worker pool, one frame's update
  (selection, skirts, jobs) costs ~2–4 ms for ~2,800 drawn sections.

**Seeing it from above: creative flight** **[built, Phase 4]** (§9.1, PLAYER_CONTROLLER.md §6.7).
The player flies — the body itself, simulated by the server and predicted like any movement — up
to `FLIGHT_CEILING` (24,000 km), from where the whole disc fills about two thirds of the view.
Speed grows with height above the sea, so the climb is exponential (capped at 400 m/s within the
terrain band). In the browser test (local mode, SwiftShader) the player flies to the ceiling
and the whole disc is drawn by the time it arrives.

---

## 7. Physics Pipeline

### 7.1 Voxel Awakening & Clustering **[planned]**

Triggered when the server registers an explosion, a block removal, or a structural failure.

1. **Event trigger.** Server resolves the set of voxels directly removed (e.g. sphere of
   radius *R* for an explosion, attenuated by material strength).
2. **De-chunking.** Removed voxels are cleared from the master grid immediately. A reliable
   `VoxelModification` is queued for all interested clients; affected chunk collision meshes
   are rebuilt (server) / re-meshed (client).
3. **Structural integrity.** Starting from solid voxels adjacent to the removed region, the
   server flood-fills (6-connectivity) looking for an **anchor** (bedrock layer, or any voxel
   flagged as grounded). Components that reach an anchor stay static. Components that do not
   are **detached**.
   - The search is budgeted (max voxels visited per tick). A component that exceeds the
     budget is treated as anchored for this tick and re-queued, so a single event can never
     stall the tick.
4. **Clustering.** Each detached component (from the same flood-fill) is one cluster. Its
   voxels are removed from the grid (part of the same reliable delta).
5. **Tier classification** (§7.2) by block count and materials.
6. **Awakening (Tier 1 only).** One Jolt body per cluster:
   - Shape: `StaticCompoundShape` of per-voxel `BoxShape`s (future optimization: merge runs
     into larger boxes).
   - Mass / center of mass / inertia from per-voxel material density.
   - Inherits impulse from the triggering explosion, if any.
   - Assigned a `NetworkEntityID` (`uint32`, never reused within a session) and added to the
     dynamic simulation. The cluster's voxel layout is sent reliably once
     (`EntitySpawn`) so clients can build the render mesh.

### 7.2 Tiered Synchronization **[planned]**

**Tier 1 — Authoritative sync**
- Threshold: `blockCount >= TIER1_MIN_BLOCKS` **or** the cluster contains a material flagged
  `alwaysAuthoritative` (heavy/gameplay materials).
- Server sends position, rotation, linear and angular velocity for each awake Tier 1 body
  via unreliable datagrams at 20 Hz (interest-managed and prioritized, §8.3).
- Client renders with **interpolation** ~`INTERP_DELAY_MS` behind the latest snapshot;
  bounded extrapolation using velocities when snapshots are late.
- Client also inserts Tier 1 bodies into its local worlds as **kinematic** bodies driven by
  interpolated transforms so the local player and debris collide with them.

**Tier 2 — Cosmetic debris**
- Threshold: `blockCount < TIER1_MIN_BLOCKS` (i.e. 1–3 blocks).
- Server removes the voxels (included in the reliable `VoxelModification`) and sends a
  reliable `PhysicsEvent::Explosion(origin, force, radius)` **in the same reliable
  message batch**, so ordering is guaranteed.
- Client derives debris from the voxels that the modification removed, spawns bodies in its
  local debris world, applies the explosive impulse, and simulates locally. The server ignores
  these bodies entirely; they have no gameplay effect and may differ between clients.

### 7.3 Voxel Sleeping & Re-baking **[planned]**

1. **Sleep detection.** For each Tier 1 body, if `|v| < SLEEP_LINEAR_THRESHOLD` and
   `|ω| < SLEEP_ANGULAR_THRESHOLD` continuously for `SLEEP_SECONDS`, it enters the re-bake
   queue. (Jolt's own sleeping may accelerate this but is not relied on as the signal.)
2. **Grid snapping.** Round the body's rotation to the nearest of the 24 axis-aligned
   orientations and its position to the nearest grid cell; map every cluster voxel to a world
   cell.
3. **Conflict resolution.** If a target cell is occupied, try small offsets (±1 cell,
   upward first); voxels that still cannot be placed are dropped and emitted as Tier 2 debris.
   Cells overlapping a player capsule count as occupied, so re-baking never embeds a player
   in terrain; a player standing on the body switches `groundEntityId` to 0 (static) in the
   same tick.
4. **Re-bake.** Destroy the Jolt body → write voxels into the master grid → rebuild affected
   chunk collision → run a structural-integrity check on the placed voxels (they may be
   unsupported) → broadcast reliable `VoxelModification` with reason `Rebake` plus
   `EntityDespawn(NetworkEntityID)` in the same batch.
5. **Cosmetic cleanup (client).** Tier 2 debris is despawned after `DEBRIS_LIFETIME_SECONDS`
   or once it comes to rest, and is capped at `DEBRIS_MAX_BODIES` (oldest removed first).

### 7.4 Tunable constants

The source spec left several thresholds unspecified. The values below are **initial defaults**
to be tuned; they live in `shared/protocol/constants` and are consumed by both sides.

| Constant | Default | Meaning |
|---|---|---|
| `SIM_HZ` | 60 | Physics steps per second (server and client prediction) |
| `SNAPSHOT_HZ` | 20 | Snapshot broadcast rate |
| `TIER1_MIN_BLOCKS` | 4 | Clusters at or above this size are Tier 1 |
| `SLEEP_LINEAR_THRESHOLD` | 0.05 m/s | Sleep velocity threshold |
| `SLEEP_ANGULAR_THRESHOLD` | 0.05 rad/s | Sleep angular threshold |
| `SLEEP_SECONDS` | 2.0 s | Time below threshold before re-bake |
| `DEBRIS_LIFETIME_SECONDS` | 5.0 s | Max lifetime of Tier 2 debris |
| `DEBRIS_MAX_BODIES` | 256 (desktop) / 96 (mobile) | Client debris cap |
| `INTERP_DELAY_MS` | 100 | Tier 1 and remote-player interpolation delay (2 snapshots) |
| `MAX_TIER1_BODIES` | 512 | Server cap; oldest/smallest force-re-baked when exceeded |
| `INTEGRITY_BUDGET_VOXELS` | 32 768 / tick | Flood-fill budget per tick |
| `AUTOSAVE_SECONDS` | 30 s | World autosave interval (per-world `autosave_seconds` setting; local mode 5 s) |
| `CHUNK_SIZE` | 32 | Voxels per chunk edge |
| **Players (§9)** | | |
| Controller tuning | see `PLAYER_CONTROLLER.md` §7 | Capsule, speeds, jump, crouch, climb, swim, push, damage |
| `RECONCILE_SNAP_DISTANCE` | 1.0 m | Correction above this snaps instead of smoothing |
| `PREDICT_PROXY_RADIUS` | 16 m | Tier 1 bodies within this use present-time proxies |
| `RESPAWN_SECONDS` | 5 s | Death → respawn delay |
| `MAX_HEALTH` | 100 | Player health |
| `MAX_INPUTS_PER_DATAGRAM` | 4 | Input redundancy per `PlayerInput` |
| `REACH_DISTANCE` | 5 m | Block break/place reach from the eye (§6.5; the server allows 1 m more for latency) |
| `BLOCK_EDIT_INTERVAL_MS` | 100 ms | Minimum time between a player's block edits (§6.5; server: token bucket, bursts of 3) |
| `MAX_RESYNC_CHUNKS` | 64 | Chunks per `ChunkResync` request |
| `MAX_LOD_INDEX_ENTRIES` | 16 384 | Entries per `LodIndex` / `LodIndexUpdate` message (~192 KB, under SCTP's 256 KiB) |
| `LOD_MAX_REQUEST_SECTIONS` | 32 | Sections per `LodRequest` |
| **Terrain (§6.3)** | | |
| `WORLD_MIN_Y` / `WORLD_MAX_Y` | −2 048 / 6 144 | Vertical world bounds (8,192 m, ¾ above sea level) |
| `WORLD_RADIUS` | 8 192 000 m | Radius of the world disc; beyond it, the void |
| `POSITION_FIXED_SCALE` | 256 per m | Fixed-point wire positions (`i32`, ±8 388 km at 3.9 mm, §8.3) |
| `POS64_LIMIT` | 33 554 432 m | Range decoders accept for `pos64` positions (§8.3) |
| `FLIGHT_CEILING` | 24 000 000 m | Highest feet position in creative flight (§9.1; PLAYER_CONTROLLER.md §6.7 has the other flight tunables) |
| `BEDROCK_LAYERS` | 4 | Indestructible anchor layers at the bottom |
| `SEA_LEVEL` | 0 | Water fill height |
| `VIEW_RADIUS_CHUNKS` | 3 | Radius of the sphere of chunks streamed around each player |
| Terrain collision region | 64 chunks (2 048 m), anchor hysteresis 8 chunks | `TerrainCollision::kRegionChunks`, `kAnchorHysteresisChunks` (§6.1; code constants) |
| `UNLOAD_MARGIN_CHUNKS` | 1 | Hysteresis before chunks leaving the view are unloaded |
| `CHUNK_BYTES_PER_SECOND` | 1 MiB/s | Terrain bandwidth budget per client |
| `MAX_CHUNKS_PER_TICK` | 32 | Chunk messages per client per tick |
| **LOD (§6.6), Phase 4** | | |
| `LOD_SECTION_CELLS` | 32 | Cells per LOD section edge (level 0 = a chunk) |
| `LOD_MAX_LEVEL` | 19 | Root level; one section holds the whole disc |
| `LOD_INDEX_LEVEL` | 8 | Level of the modified-section index sent to clients |
| `LOD_PIXEL_ERROR` | 4 px (desktop) / 8 px (mobile) | Refine a node while its cells project larger than this (ADR 0012's 2 px drew ~3× the sections, §6.6) |
| `LOD_NEAR_SPLIT_M` | 1 024 m | Distance splitting the near and far depth passes |
| `LOD_CACHE_MB` | 256 (desktop) / 96 (mobile) | Client cache of LOD section content and meshes |
| `LOD_BYTES_PER_SECOND` | 256 KiB/s | LOD bandwidth budget per client (`lod` stream) |
| `LOD_REQUESTS_PER_SECOND` | 64 | Per-client `LodRequest` rate limit |
| `LOD_INDEX_UPDATE_MS` | 1 000 ms | Coalescing interval for `LodIndexUpdate` broadcasts |
| `LOD_PROPAGATION_SECTIONS_PER_TICK` | 8 | Server downsampling budget (off the tick) |

---

## 8. Networking

### 8.1 Transport abstraction
```
interface Transport {
  sendDatagram(bytes)                // unreliable, unordered, ≤ MAX_DATAGRAM_BYTES
  sendReliable(channel, bytes)       // reliable, ordered per channel
  onDatagram / onReliable / onClose
}
```
Implementations:
- **WebTransport** — dedicated servers (primary).
- **WebRTC** — friend worlds (§10.2) and the dedicated-server fallback (ADR 0008). One unordered,
  `maxRetransmits: 0` data channel carries datagrams; one ordered, reliable data channel per
  reliable channel (`control`, `world`). The peer's DTLS fingerprint comes from signaling, the
  master server, or the invite link, so no CA certificate is involved.
- **Loopback** — local single-player (integrated server in a worker).

**Wire mapping [built]:**

| | WebTransport | WebRTC | Loopback |
|---|---|---|---|
| `control` | client-opened bidi stream | data channel id 0, reliable, ordered | worker message |
| `world` | server-opened uni stream | data channel id 1, reliable, ordered | worker message |
| datagrams | QUIC datagrams | data channel id 2, unordered, `maxRetransmits: 0` | worker message |
| `lod` **[built, Phase 4b]** | second server-opened uni stream (first byte 2) | data channel id 3, reliable, ordered | worker message |
| framing | first byte = channel id, then `u32 LE length ‖ payload` per message | none (SCTP keeps message boundaries) | none |
| transport binding | SHA-256 of the server certificate | same (DTLS uses that certificate) | 32 zero bytes |

### 8.2 Channels

| Channel | Kind | Content |
|---|---|---|
| Datagrams | Unreliable | Player input (C→S), physics snapshots (S→C) |
| Stream `control` | Reliable, bidi | Status query, handshake + identity challenge, ping/clock sync, chat, block edit requests, chunk resync requests, LOD requests |
| Stream `world` | Reliable, uni S→C | Chunk data, `VoxelModification`, `PhysicsEvent`, `PlayerEvent`, entity spawn/despawn |
| Stream `lod` **[built, Phase 4b]** | Reliable, uni S→C | `LodIndex`, `LodIndexUpdate`, `LodData` (§6.6); requests go on `control` |

All world-affecting reliable messages go on **one** ordered stream so a voxel removal and the
event/entity that depends on it can never be reordered. Bulk chunk streaming may move to
separate uni streams later if head-of-line blocking is measured to matter. LOD traffic (§6.6) is
bulky and needs no ordering against `world` (sections carry their own revisions), so it gets its
own `lod` stream and budget and never delays chunk data or voxel deltas.

`MAX_DATAGRAM_BYTES` = 1200 (safe QUIC payload); `MAX_RELIABLE_MESSAGE_BYTES` = 1 MiB. WebRTC
reliable messages are additionally capped by SCTP `max-message-size` (256 KiB), so large world
messages (Phase 3 chunk data) must stay under it or be split. All multi-byte fields
little-endian; strings are `u16 byte length ‖ UTF-8`, validated and capped per field
(`shared/protocol/constants.json` `limits`).

### 8.3 Message formats

Every message starts with a `u8` type (`constants.json` `messageTypes`). **Built (protocol v7):**
`DatagramPing` 0x02 / `DatagramPong` 0x82, `StatusRequest` 0x40 / `StatusResponse` 0x41,
`ClientHello` 0x42, `Challenge` 0x43, `ClientAuth` 0x44, `Welcome` 0x45, `Reject` 0x46, `Ping`
0x47 / `Pong` 0x48 (Phase 1); `PlayerInput` 0x01, `PhysicsSnapshot` 0x81, `PlayerEvent` 0x30
(Phase 2); `WorldgenCheck` 0x49, `ChunkData` 0x11, `ChunkUnload` 0x12, and the verification chunk
in `Welcome` (Phase 3b; protocol v3); planet-scale positions and the `Air` chunk form (Phase 3c;
protocol v4); `BlockEditRequest` 0x4A, `ChunkResync` 0x4B and `VoxelModification` 0x10 (Phase 3d;
protocol v5); `LodIndex` 0x13, `LodIndexUpdate` 0x14, `LodData` 0x15, `LodRequest` 0x4C and the
`lod` channel (Phase 4b; protocol v6); `Welcome` flags, the `fly` input button, the `Flying`
state and flags, and the wider `pos64` range for creative flight (Phase 4; protocol v7) — layouts pinned by `shared/protocol/vectors.txt` (C++, TypeScript, and the Python
reference encoder, including half floats). The remaining formats below are drafts, finalized in the
phase that builds them. Enumerations and bit sets (`inputButtons`, `playerStates`, `playerFlags`,
`controllerFlags`, `welcomeFlags`, `groundKinds`, `playerEventKinds`, `damageCauses`, `chunkForms`, `lodForms`,
`blockEditActions`, `voxelModificationReasons`) are generated from
`constants.json`; decoders reject unknown values.

**Positions at planet scale — protocol v4 [built, Phase 3c]** ([ADR 0011](./adr/0011-planet-scale-world.md)).
f32 steps by 0.5 m near the rim of the 8,192 km disc, so world positions have their own types:
- **`pos64`** (`f64×3`) where client prediction must match the server exactly: the local
  player's position in `PhysicsSnapshot` and the `Respawn` position in `PlayerEvent`. Decoders
  reject values outside ±`POS64_LIMIT` (33,554 km: the world and the sky above it, up to the
  creative-flight ceiling; protocol v7), NaN included.
- **`posfix`** (`i32×3`, 1/`POSITION_FIXED_SCALE` = 1/256 m, ±8 388 km) for remote players, and
  later Tier 1 entities and `PhysicsEvent` origins — the same 12 bytes as f32×3, with a uniform
  3.9 mm step. Encoders round to the nearest step (halves up, identically in C++ and TypeScript)
  and clamp to i32 (so a player flying above 8,388 km appears to others at that height).
- Velocities, rotations and body-local offsets keep their types.

**Client → Server: `PlayerInput` (datagram) [built]**
```
u8   type = 0x01
u32  lastReceivedSnapshotTick
u8   count                      // 1..MAX_INPUTS_PER_DATAGRAM (4): the newest inputs, oldest first
repeat count:
  u32  inputSeq
  i8   moveX, moveY             // analog move vector ×127, clamped to the unit circle
  u16  buttons                  // jump 1 | run 2 | crouch 4
  i16  yaw                      // wrapped fraction of a turn (65536 per 360°); 0 = +Z
  i16  pitch                    // ±32767 for ±90°
```

**Server → Client: `PhysicsSnapshot` (datagram, SNAPSHOT_HZ) [built for players]**
```
u8   type = 0x81
u32  serverTick
u32  ackInputSeq                // last input processed for this client
local player:
  f64×3 position                // pos64: capsule centre (feet + half height)
  f32×3 velocity
  u8   flags                    // PlayerFlags: grounded | crouched | climbing | swimming | dead
  u8   health
  u8   state                    // player::State
  u8   inputBuffer              // inputs queued on the server (client tick-rate steering)
  u32  lastKnockbackSeq         // inputSeq of the latest knockback applied (0 = none)
  controller state (47 B + 12 while climbing + 8 after releasing a ladder;
                    exact fields in PLAYER_CONTROLLER.md §8.4)
u8   remotePlayerCount          // nearest first, as many as fit in one datagram
repeat remotePlayerCount:
  u16  playerId
  i32×3 feet position           // posfix
  f16×3 velocity
  i16  yaw, i16 pitch
  u8   state                    // player::State (animation)
  u8   flags                    // PlayerFlags
```
Phase 5 adds, for riding Tier 1 bodies (§9.4), a `u32 groundEntityId` to the local and remote
blocks (positions become body-local when it is non-zero), and the Tier 1 entity list:
```
u8   entityCount
repeat entityCount:             // ~32 bytes each → ~34 entities per datagram
  u32  networkEntityId
  i32×3 position                // posfix
  u32  rotation                 // smallest-three quaternion, 2+10+10+10 bits
  f16×3 linearVelocity
  f16×3 angularVelocity
```
When more awake bodies are relevant than fit, a **priority accumulator** (distance, size,
speed, time since last sent) selects which bodies go in each snapshot; multiple datagrams per
tick are allowed up to a per-client bandwidth budget.

**Status query (reliable, `control`)** — `StatusRequest` / `StatusResponse`: protocol version,
server name, MOTD, player count / max, icon, online/offline mode. Answered without joining; used
by the server browser and by reachability verification (§10.3).

**Join handshake (reliable, `control`)**
```
C→S  ClientHello   u16 protocolVersion, str clientVersion, u8[32] publicKey (Ed25519),
                   str displayName
S→C  Challenge     u8[32] nonce
C→S  ClientAuth    u8[64] signature over (nonce ‖ transport binding ‖ publicKey)
                   [optional: account attestation — online mode, §10.4]
S→C  Welcome       u16 playerId, u64 worldSeed, u32 generatorVersion, u32 serverTick,
                   i32×3 verificationChunk, u8 flags (WelcomeFlags: 1 = flight — this
                   player may use creative flight)
     or Reject     u8 reason (ProtocolVersion, Banned, Full, NotAllowListed, AuthFailed,
                   Malformed, Replaced), str message — followed by closing the session
C→S  WorldgenCheck u64 hash — FNV-1a 64 of the client-generated verification chunk (u16 LE
                   voxels in chunk index order); 0 asks for full-chunk mode. Once, after
                   Welcome; the server streams nothing before it (§6.3)
```

**Client → Server: `BlockEditRequest` (reliable, `control`) [built, Phase 3d]** (§6.5)
```
u8   type = 0x4A
u8   action                     // Break 1 | Place 2
i32×3 cell                      // world voxel coordinate of the targeted cell
u8   face                       // 0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z (the targeted face)
Place only: u16 material        // the block placed against the face
```

**Client → Server: `ChunkResync` (reliable, `control`) [built, Phase 3d]** — a revision gap (§6.3)
```
u8   type = 0x4B
u16  count                      // 1..MAX_RESYNC_CHUNKS (64)
repeat count: i32×3 chunkCoord  // answered with ChunkData for those the client is streamed
```
The signature covers `"dwell-auth-v1" ‖ nonce ‖ transport binding ‖ publicKey`; the binding
(§8.1) ties it to the server certificate, so a signed challenge cannot be relayed to a different
server. **[built]** A successful login for a key that already has a joined session replaces it:
the old session receives `Reject(Replaced)` and is closed (so a dropped connection can rejoin
immediately). `Ping`/`Pong` (reliable) and `DatagramPing`/`DatagramPong` carry the client time and
server tick for RTT and clock sync.

**Server → Client: `ChunkData` (reliable, `world`) [built]**
```
u8   type = 0x11
u8   form                       // Generated = 0 (client generates; no payload) | Explicit = 1
                                 // | Air = 2 (all air: nothing to generate or store; no payload)
i32×3 chunkCoord, u32 revision
Explicit only (§6.1): u16 paletteCount (1..32768), u16 × paletteCount materials,
     then runs in layer order until 32768 voxels: LEB128 length (1..32768, minimal),
     palette index (u8, or u16 when paletteCount > 256)
```

**Server → Client: `ChunkUnload` (reliable, `world`) [built]**
```
u8   type = 0x12
u16  count                      // 1..65535
repeat count: i32×3 chunkCoord
```

**Server → Client: `PlayerEvent` (reliable, `world`) [built]** — to every joined client
```
u8   type = 0x30
u8   kind                       // Knockback 1 | Damage 2 | Death 3 | Respawn 4
u16  playerId
u32  serverTick                 // tick the effect was applied
u32  inputSeq                   // that player's input processed on serverTick (predicted replay)
Knockback: f32×3                // velocity change
Respawn: f64×3                  // pos64: respawn feet position
Damage:  u8 amount, u8 cause    // cause: Fall 1 | Crush 2 | Explosion 3
Death:   u8 cause
```

**LOD messages [built, Phase 4b; protocol v6]** (§6.6). Section coordinates are
`u8 level, i32×3 (i, j, k)` counted from the LOD grid's corner.
```
S→C LodIndex 0x13       (lod) u8 flags (1 = last), u32 count (≤ 16384),
                              repeat: i32 i, i32 k, u32 lodRevision
                              // modified sections at LOD_INDEX_LEVEL (one row, so no j);
                              // may span several messages, the last flagged; count 0 allowed
S→C LodIndexUpdate 0x14 (lod) u16 count (1..16384), repeat: i32 i, i32 k, u32 lodRevision
                              // coalesced, at most one per LOD_INDEX_UPDATE_MS
C→S LodRequest 0x4C (control) u8 count (1..32), repeat: u8 level (1..19), i32×3 section,
                              u32 knownRevision (0 = none)
S→C LodData 0x15        (lod) u8 form (Generated 0 | Explicit 1 | Unchanged 2),
                              u8 level, i32×3 section, u32 lodRevision,
                              Explicit only: palette + RLE as ChunkData Explicit over the
                              34³ cells of the section and its one-cell apron
```

**Server → Client: `VoxelModification` (reliable, `world`) [built, Phase 3d: reason Edit]**
```
u8   type = 0x10
u8   reason                     // Edit 1 | Explosion 2 | Collapse 3 | Rebake 4
u32  serverTick
u16  chunkCount                 // ≥ 1: the changed chunks this client streams
repeat: i32×3 chunkCoord, u32 newRevision (the chunk's previous revision + 1), u16 changeCount (≥ 1),
        repeat: u16 localIndex (x|y<<5|z<<10, < 32768), u16 material
```

**Server → Client: `PhysicsEvent` (reliable, `world`)**
```
u8   type = 0x20
u8   kind                       // Explosion = 1
i32×3 origin (posfix), f32 force, f32 radius
```

**Server → Client: `EntitySpawn` / `EntityDespawn` (reliable, `world`)** — `NetworkEntityID`,
initial transform, and (spawn only) the cluster voxel layout (local offsets + materials).

---

## 9. Players: Physics-Based Characters **[built]** (Tier 1 interactions: Phase 5)

Full specification: **[`PLAYER_CONTROLLER.md`](./PLAYER_CONTROLLER.md)** — a port of the
[Physics Player Controller](https://github.com/zacharysnewman/physics-player-controller)
(its deterministic Quantum 3 version) to C++/Jolt and Dwell's voxel world. This section is the
architectural summary.

### 9.1 Character model
- Each player is a **dynamic Jolt body** (capsule, rotation locked via `EAllowedDOFs`, gravity
  factor 0, frictionless, never sleeps) driven by **velocity layers**: horizontal, vertical, and
  exclusive climb/swim/fly layers. The aggregate pass writes the summed target velocity to the body
  before each physics step.
- Whatever the solver does to the body (collisions, falling clusters, explosions) shows up next
  tick as a deviation from the target, is **absorbed** as external velocity, and decays. That makes
  players physically reactive without losing tight, predictable movement.
- The controller is plain C++ in `server/core/player` (a copyable `PlayerController` value plus
  ordered passes). The client runs the same code via the sim-core WASM build (§2.1).
- Probes (ground/ceiling rings, wall rays, overlap tests) query the **voxel grid** directly (DDA)
  for terrain and Jolt only for moving bodies (PLAYER_CONTROLLER.md §5).
- **[built]** `dwell::player::Players` runs the pipeline for every player of a physics world; the
  whole PPC test suite is ported to voxel geometry (`server/tests/player`, including a four-player
  golden trace and the 64-player < 1 ms/tick gate), and runs both at the origin and ~8,000 km from
  it. Player bodies use the `Character` object layer, double-precision positions (ADR 0011), a
  terrain collision group for their anchor (§6.1), and a Jolt `ContactListener` (installed by
  `Players`) that records their contact normals.
- **Creative flight** **[built, Phase 4]** (PLAYER_CONTROLLER.md §6.7): an exclusive layer while
  the input's `fly` mode bit is set — no gravity, move along the view's yaw, jump up, crouch down,
  speed growing with height above the sea up to `FLIGHT_CEILING`. The client toggles it (double-tap
  Space or Jump, or the touch Fly button); the server's flight policy (`--flight`, told to the
  client in `Welcome`) decides who may, clearing the bit for anyone else. While flying the client
  keeps predicting even where the streamed terrain has not arrived yet.

### 9.2 Physical interactions

| Interaction | Behavior | Authority |
|---|---|---|
| Voxel terrain | Collide; step-up ≤ 0.45 m (slabs, debris); full blocks need a jump (optional auto-jump); ladders are climbable materials; water switches to the swim layer | Server; client predicts |
| Player → Tier 1 body | Solver push, capped by contact mass scaling (`maxPushForce`, `pushableMassLimit`) | Server only; client does not predict body motion |
| Tier 1 body → player | Solver push, absorbed as external velocity and decayed | Server; client corrected via reconciliation |
| Standing on a Tier 1 body | Carried by the body's linear + angular velocity at the player's position (PPC platform model); camera turns with it | Server; client predicts in the body's frame (§9.4) |
| Crushing | Opposing contacts with an approaching Tier 1 body sustained for `crushTicks` | Server |
| Other players | Both dynamic: they block and push each other; standing on heads is grounded but not carried by default | Server; remote players are kinematic on the client |
| Tier 2 debris | Bounces off the local player; never affects player movement | Client only |
| Explosions | Radial velocity change with falloff and upward bias (PPC `AddExplosion`), plus damage | Server; client replays at event tick (§9.3) |
| Falling | Fall damage from the controller's `Landed` impact speed | Server |
| Voxel edits | Block removed under feet → normal walk-off; placement into any player capsule is rejected | Server |

### 9.3 Prediction & reconciliation **[built]**
Details and measurements: PLAYER_CONTROLLER.md §8.
- Fixed 60 Hz step on both sides. The client runs the controller pipeline for the local player
  against a small prediction world (terrain, kinematic proxies, one dynamic body), records
  `{inputSeq, input, controller state, body state}` per tick, and sends inputs with the previous 3
  for loss resilience.
- The server applies inputs in sequence order, one per step, through a 2-input jitter buffer; a
  missing input repeats the last known one (and re-primes the buffer). The client nudges its tick
  rate ±2 % to keep the server's queue (reported in snapshots) between 1 and 4. Snapshots carry `ackInputSeq`, the authoritative body state, and the local player's
  controller state (layers, timers, crouch/climb/swim, ground ref).
- On snapshot: if the stored prediction for `ackInputSeq` matches within tolerance, nothing is
  replayed. Otherwise the client restores the server state, replays unacked inputs (pipeline +
  prediction-world step), and smooths the visible correction; errors above
  `RECONCILE_SNAP_DISTANCE` snap.
- **Server-originated impulses** (knockback, explosion) arrive as a reliable `PlayerEvent` with the
  tick and the player's `inputSeq` they were applied at; the client inserts them into history at
  that input and replays. Snapshots carry `lastKnockbackSeq`, so a snapshot that overtakes its event
  is smoothed like a knockback rather than snapped. Built with the debug launch pad (Phase 2).
- The prediction logic is C++ (`player::Predictor`), shared by the browser client (its own WASM
  instance on the main thread) and the native latency/loss tests.
- Native and WASM Jolt are not assumed bit-identical. Divergence is minimized (Jolt
  `JPH_CROSS_PLATFORM_DETERMINISTIC`, no FMA contraction) and measured in CI; reconciliation
  absorbs the rest.

### 9.4 Moving platforms & time frames
The locally predicted player lives in the *present*, while Tier 1 bodies are normally rendered
`INTERP_DELAY_MS` in the *past*. Mixing the two naively makes riding or dodging a falling
structure unplayable. Therefore:
- **Ground-relative state.** When a player stands on a Tier 1 body, snapshots report
  `groundEntityId` and the player position/velocity **in that body's local frame**. The client
  predicts in the same frame, so riding a moving body is stable regardless of latency.
- **Present-time proxies.** Client collision proxies for Tier 1 bodies within
  `PREDICT_PROXY_RADIUS` of the local player are **extrapolated to predicted time** from the
  latest snapshot (position + velocities), not interpolated. The body the player stands on is
  also *rendered* at present time so the player's feet stay planted. Bodies farther away stay
  interpolated.
- Mispredicted contacts with these proxies are corrected by normal reconciliation.

### 9.5 Remote players **[built]**
Remote players are interpolated like Tier 1 bodies (in the ground body's frame when
`groundEntityId` ≠ 0), exist in the client's physics worlds as kinematic capsules, and are
animated from `State` and flags in snapshots.

### 9.6 Health, death & respawn **[built: fall damage, death, respawn]**
- Health (`MAX_HEALTH` 100) is server-authoritative; damage sources are fall (built: from the
  `Landed` impact speed above 12 m/s, 8 points per m/s, and falling out of the world), crush
  (Phase 5), explosion (Phase 6). `Damage`, `Death`, and `Respawn` are `PlayerEvent`s to every
  client; a dead player's body leaves the physics world, and snapshots mark it `dead` where it
  died. The server respawns the player at the spawn point after `RESPAWN_SECONDS`.
- On death clients currently draw the body lying down (a cosmetic pose). The **cosmetic ragdoll**
  (Jolt `Ragdoll`, Tier 2 rules — local, unsynchronized, despawned on respawn) arrives with the
  client debris world in Phase 6.

### 9.7 Anti-cheat implications
Clients only send inputs, never positions, so speed/teleport/fly hacks are structurally
impossible; the server's simulation is the only source of player state. Creative flight is an
input the server honours only for players its flight policy allows.

---

## 10. Multiplayer Hosting, Discovery & Identity **[in progress]**

Decisions: [ADR 0003](./adr/0003-multiplayer-hosting-model.md) (hosting model),
[ADR 0004](./adr/0004-player-identity.md) (identity), [ADR 0005](./adr/0005-domain-and-origins.md)
(domain). Two hosting tiers — always-on dedicated servers and client-hosted friend worlds —
follow from browser constraints: pages cannot accept inbound connections, and hosts rarely have
trusted certificates.

### 10.1 Dedicated servers
- Distributed as native binaries (Windows/macOS/Linux) and a Docker image, built by CI per
  release. The Electron app can launch the same binary as a background process ("Host world").
- Transports: WebTransport, WebRTC fallback (ADR 0008). Certificates per §2.3. Invite links
  **[built]**: `?join=host:port&cert=<sha256 hex>[&rtc=<port>&ice=<ufrag>:<pwd>]`. The WebRTC part
  requires an IP-literal host (WebRTC host candidates can't carry DNS names). `dwell_server
  --advertise <ip>` sets the address printed in the link and used as the WebRTC candidate.
- Operator settings, stored in the world database's `settings` table (§6.4; **[built]** name, MOTD,
  max players, edit policy and autosave interval, set by launch options): name, MOTD,
  icon, max players, visibility (public / unlisted / none), password or allow-list,
  online/offline mode (§10.4), physics and view-distance caps (`MAX_TIER1_BODIES`, view radius),
  autosave and backup schedule. Edited by admin commands or the server CLI.
- Admin commands (ops, kick, ban by public key), UPnP/NAT-PMP port mapping with port-forwarding
  guidance when it fails.

### 10.2 Friend worlds
- Any client hosts its **integrated server** — the sim core already used for local
  single-player — and "opens" it through the master server. Browser, Capacitor, and Electron
  all qualify.
- Guests connect over **WebRTC** (§8.1). The master relays the offer/answer and ICE candidates
  (signaling); STUN enables direct connections; a **TURN relay** carries traffic when NAT
  traversal fails (common on mobile data).
- Host profiles cap the load: e.g. mobile 4 players, desktop browser 8, with reduced physics
  caps. When the host backgrounds the app/tab, the world pauses and guests are notified; when
  the host quits, the session ends (no host migration — [ADR 0009](./adr/0009-friend-world-lifetime.md)).
- Saves live on the host (browser: OPFS; apps: OPFS or app storage) as the same SQLite world
  file as dedicated servers (§6.4), so a friend world can be exported to a dedicated server.

### 10.3 Master server
A small HTTPS JSON service (`services/master`; hostname chosen in Phase 8); no game
traffic passes through it.

| Function | Detail |
|---|---|
| Registration & heartbeat | Dedicated servers register with a server key and heartbeat every ~30 s: address, port, current cert SHA-256, name, MOTD, players, protocol version, tags, visibility. Missed heartbeats delist. |
| Server browser | Public listing with search/filter; clients ping candidates themselves. A server is marked verified only after reachability verification — either a master-side status probe or player-attested join receipts, depending on the platform chosen for Open Decision #10. |
| Join codes | Short codes (e.g. `KQ7-XM4`) resolve to the current address + cert hash (dedicated) or to a signaling session (friend world). |
| Signaling | WebRTC offer/answer/ICE relay for friend worlds. |
| TURN credentials | Short-lived TURN credentials for the TURN relay, rate-limited per player key. |
| Accounts (later) | Sign-in and account attestations (§10.4). |

Direct invite links (`?join=host:port&cert=<sha256>`) work without the master server.

### 10.4 Identity
- **Device keys (now) [built on web]:** each install generates an Ed25519 key pair; the public
  key is the player ID. Web: non-extractable WebCrypto key in the `dwell` IndexedDB database
  (`identity` store); apps: OS keychain/keystore (Phase 8). Because the key is non-extractable it
  cannot be exported; moving an identity between devices waits for accounts, which link several
  device keys (ADR 0004 amendment). Proven on every join by signing the server's challenge
  (§8.3), verified server-side with Monocypher. Servers key bans, allow-lists, ops, and player
  data by public key.
- **Accounts (later, additive):** sign-in through the master server; the master issues short-lived
  signed **attestations** binding device keys to an account, which servers verify offline with
  the master's published keys. Servers choose **offline mode** (any key) or **online mode**
  (attested keys only). Enables friends lists, cross-device identity, recovery.

### 10.5 Versioning
- The handshake rejects incompatible `protocolVersion`s with a clear reason; the server browser
  marks incompatible servers.
- The Pages site keeps older client builds at `/dwell/v/<version>/`; the browser can open the build
  matching a server's version.

### 10.6 Platform reachability

| Joining from → | Dedicated server | Friend world |
|---|---|---|
| Browsers with WebTransport (Chromium, Firefox), Electron, Android app | ✅ WebTransport + cert hash | ✅ WebRTC |
| Browsers / WebViews without WebTransport (e.g. Safari, iOS) | ✅ WebRTC + DTLS fingerprint | ✅ WebRTC |
| Server behind strict NAT | ✅ WebRTC via TURN relay | ✅ via TURN |

No platform needs a trusted certificate to join any server (ADR 0008).

---

## 11. Security & Validation

- Server is the only authority on voxels, Tier 1 bodies, and player positions.
- Inputs are rate-limited and range-checked **[built]**: at most 2 × `SIM_HZ` `PlayerInput`
  datagrams per second (excess dropped); an input is rejected if its move vector is longer than 1
  (beyond quantization slack), its pitch is out of range, or its `inputSeq` is more than 256 ahead;
  unknown button bits fail decoding. The per-session queue is capped at 16. Rejections are counted
  per session (`SessionStats`).
- **[built, Phase 3d]** Block edits are checked for reach, line of sight, rate (token bucket),
  and permissions (`ServerConfig::edits`); bedrock cannot be broken, only palette materials can be
  placed, and placements into blocks, outside the world, or overlapping any player capsule are
  rejected (§6.5). At most 8 requests per session queue per tick; `ChunkResync` answers only chunks
  the client is streamed, at most `MAX_RESYNC_CHUNKS` per request.
- Message decoders bounds-check every length field; malformed messages drop the connection.
- **[built, Phase 4b]** `LodRequest`s are rate-limited (`LOD_REQUESTS_PER_SECOND`, a token
  bucket; at most 256 queued, the rest dropped and counted in `LodStats`) and bounded (levels
  1–19 in the decoder; sections outside the world are answered `Generated` without work); LOD
  replies share the `LOD_BYTES_PER_SECOND` budget.
  By design, LOD data shows every player's builds from anywhere, including the coarse layout of
  enclosed rooms (2 m cells at level 1) — an accepted trade-off (ADR 0012); sealing enclosed voids
  server-side is a possible later mitigation.
- **Servers are player-run and untrusted by clients:** the client validates every server message
  as strictly as the server validates client messages (bounds, sizes, rates), and never executes
  server-provided content.
- **Clients are identified by device key** (§10.4); impersonation requires the private key.
  **[built, Phase 3e]** Banned keys are refused after the identity check, and a world with an
  allow-list admits only its keys (§6.4).
- **World files** are only opened by their host (never received from peers); saved chunk blobs
  are bounds-checked when read (zstd frame size, canonical palette + RLE).
- **Content-Security-Policy** (`client/index.html`): `default-src 'self'`; scripts only from the
  site plus `'wasm-unsafe-eval'` for the WASM core; `connect-src 'self' https: ws: wss:` for
  WebTransport to player servers (and the Vite dev server); no objects, no inline scripts.
- The Electron shell uses context isolation, the renderer sandbox, and no Node integration.
- WebRTC endpoint: half-open clients time out (10 s) and are capped (512) so STUN floods can't
  allocate unbounded state.
- Master server: per-key and per-IP rate limits; verified listing requires successful reachability
  check; server keys can be revoked.

---

## 12. Open Decisions

Record each resolution as an ADR in `docs/adr/` and update the relevant section above. Ideas
deliberately out of scope for the current implementation live in [`FUTURE.md`](./FUTURE.md).

| # | Decision | Current leaning |
|---|---|---|
| 1 | ~~Server WebTransport/QUIC library~~ | **Resolved:** Rust `wtransport` behind a C ABI — [ADR 0001](./adr/0001-webtransport-server-library.md) |
| 2 | ~~Client renderer~~ | **Resolved:** Three.js on WebGL2 behind a thin render interface — [ADR 0002](./adr/0002-client-renderer.md) |
| 3 | ~~Server hosting~~ | **Resolved:** player-hosted dedicated servers + friend worlds + master server — [ADR 0003](./adr/0003-multiplayer-hosting-model.md); identity [ADR 0004](./adr/0004-player-identity.md); domain [ADR 0005](./adr/0005-domain-and-origins.md) |
| 4 | ~~Browser multithreading / cross-origin isolation~~ | **Resolved:** single-threaded web sim core + worker pools; threads natively and in Electron — [ADR 0007](./adr/0007-threading-model.md) |
| 5 | ~~World persistence format~~ | **Resolved:** one SQLite database per world holding all data — [ADR 0006](./adr/0006-world-persistence-sqlite.md) |
| 6 | Final values for §7.4 tunables | Tune in Phases 5–7 |
| 7 | ~~Worlds larger than ±65 km (Jolt `JPH_DOUBLE_PRECISION`)~~ | **Resolved:** an 8,192 km disc, 8,192 m tall, with double-precision Jolt and f64 / fixed-point wire positions — [ADR 0011](./adr/0011-planet-scale-world.md); whole-world view via a 3D LOD octree — [ADR 0012](./adr/0012-lod-octree.md) |
| 8 | ~~Worldgen noise numerics~~ | **Resolved:** strict IEEE float with integer-hash gradients, enforced by a native-vs-WASM golden test — [ADR 0010](./adr/0010-worldgen-noise-numerics.md) |
| 9 | Movement feel on voxels: PPC recommended feel (walk 5 / run 8 m/s) vs. slower voxel-genre speeds | Start with PPC feel; playtest in Phase 2 |
| 10 | Master server platform, database, and hostname | **Deferred to Phase 8** (not needed before). Constraint: $0 during development. Candidates: Cloudflare Workers + Durable Objects + D1 (no UDP → player-attested reachability, managed TURN) or a free-tier VM with a Rust service (UDP → master probes, co-located `coturn`). Either way the service runs locally (Docker / Wrangler) for dev and CI |
| 11 | TURN relay: managed vs. self-hosted `coturn` | **Deferred to Phase 8**, decided with #10; public Google STUN until then |
| 12 | ~~Trusted hostnames for player servers~~ | **Deferred:** out of scope — see [`FUTURE.md`](./FUTURE.md) |
| 13 | ~~Dedicated-server fallback transport~~ | **Resolved:** WebRTC (`str0m`), no WebSocket — [ADR 0008](./adr/0008-dedicated-server-transports.md) |
| 14 | ~~Own subdomain for the client~~ | **Deferred:** out of scope — see [`FUTURE.md`](./FUTURE.md) (ADR 0005) |
| 15 | ~~Friend-world host migration~~ | **Resolved:** no migration; sessions end with the host — [ADR 0009](./adr/0009-friend-world-lifetime.md). Migration and paid cloud worlds in [`FUTURE.md`](./FUTURE.md) |
| 16 | ~~Dedicated servers accepting WebRTC~~ | **Resolved** with #13 — [ADR 0008](./adr/0008-dedicated-server-transports.md) |
