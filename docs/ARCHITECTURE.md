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
| Terrain | Seeded deterministic procedural generation, same C++ code on server and client (§6.3) |
| First deployment target | **GitHub Pages** at `https://dropkickarcade.com/dwell/` (ADR 0005) |

```
                    ┌───────────────────────────────────────────────┐
                    │           Authoritative Server (C++)          │
                    │                                               │
  inputs (dgram) ──▶│ Input validation ─▶ Player controllers        │
                    │ Worldgen (seed)                               │
                    │        │                                      │
                    │        ▼                                      │
                    │ Master voxel grid ─▶ Structural integrity     │
                    │        ▲                 │  (flood-fill)      │
                    │        │ re-bake         ▼                    │
                    │  Sleep monitor ◀── Tier 1 bodies (Jolt, 60Hz) │
                    │                                               │
                    │ Replication: snapshots (dgram, 20Hz)          │
                    │              voxel deltas / events (reliable) │
                    └───────────────▲───────────────┬───────────────┘
                                    │ WebTransport  │
                                    │  / WebRTC     ▼
┌──────────────────────────────────────────────────────────────────────┐
│                 Client (TS + sim core WASM w/ Jolt)                  │
│  Input → Local prediction (player controller) → Reconciliation       │
│  Chunk store (+ worldgen WASM worker) → Mesher (worker) → Renderer   │
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
   with `?local=1`). Local mode (and, by default, dedicated servers) run the **playground** world
   (generator version 1): the flat world plus movement test features near the spawn.
5. **One WASM build, three uses.** The same Emscripten build of `server/core` provides local mode,
   the client's prediction/debris physics, and the client terrain generator, so every piece of
   simulation logic (player controller, worldgen, physics setup) has exactly one implementation.

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
  world" come in Phase 7.
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
  mechanism on `127.0.0.1`. **[built]** (rotation before expiry: Phase 7)
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
    /storage         World persistence: SQLite + zstd, native and OPFS VFS backends (ADR 0006).
  /wasm              Emscripten build of the core (C exports): local mode (§2.1) and the client sim.
  /tools             dwell_scenario_trace + divergence.mjs (native↔WASM check); WASM test build.
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
sources live in `core/include/dwell/player` and `core/src/player`. `storage/`, `services/master`,
and `platforms/capacitor` are not built yet.

### 3.1 Toolchain & CI **[built]**

| Area | Choice (pinned) |
|---|---|
| Client | Node 22, Vite 8 (module workers), TypeScript 6.0 (strict), ESLint (typescript-eslint strict, type-checked), Prettier, Vitest, Playwright 1.56.1 (e2e), Three.js 0.186 |
| Server C++ | CMake ≥ 3.24 with presets (`dev`, `release`, `wasm`), Ninja, C++20, `-ffp-contract=off`; dependencies via FetchContent: Jolt v5.6.0 (`CROSS_PLATFORM_DETERMINISTIC=ON`), Monocypher 4.0.3 (Ed25519 verification), Corrosion v0.6.1, doctest v2.5.3; clang-format (Google-based) |
| WASM | Emscripten 6.0.10 (`wasm` preset): ES-module factory, single-threaded (ADR 0007), copied to `client/public/wasm` (not committed; `npm run build:wasm`) |
| Server Rust | Toolchain 1.94.1 (`rust-toolchain.toml`), edition 2024; `server/net/wt` (wtransport 0.7.2 with ring, str0m 0.23.1 with aws-lc-rs, tokio) built by Corrosion as a static library; C header generated by cbindgen 0.29.4 (`gen-header.sh`) and checked in |
| Desktop | Electron 44.4.5 (`platforms/electron`) |
| CI (`ci.yml`) | Protocol: regenerated constants and vectors must match. Client: WASM build, the player/netcode suite under WASM (Node), format, lint, typecheck, unit tests (WASM tests required), build. Server: clang-format, `cargo fmt`/`clippy -D warnings`/`test`, stale-header check, CMake configure/build, ctest, smoke-run, then a Release build running the player performance gate and golden trace. E2E: native↔WASM controller divergence check (`server/tools/divergence.mjs`), native server + built client in Playwright Chromium (WebTransport, WebRTC, local mode, session replacement, local-mode movement, two clients seeing each other move with one over a simulated 150 ms link), Electron smoke under Xvfb |
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
players after `RESPAWN_SECONDS`, and every 3rd tick sends each client a `PhysicsSnapshot`. Voxel
edits, worldgen integration, integrity, and re-bake arrive with later phases.

### 4.3 Simulation core vs. network front-end
`server/core` has no sockets, OS calls, or threads of its own (parallel work such as worldgen is
exposed as jobs the host schedules; the host also supplies the Jolt job system and an `Entropy`
source for nonces); it consumes decoded messages and emits encoded messages through an
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
| `game/` **[built]** | `Game`: the fixed 60 Hz loop — samples input, predicts with the client sim, sends `PlayerInput` (newest 4), feeds snapshots and knockback events to the sim, nudges its tick rate from the server's input buffer, streams terrain meshes around the player, drives the first-person camera (crouch and step smoothing) and the HUD. `RemotePlayers`: snapshot buffer, interpolation `INTERP_DELAY_MS` in the past. |
| `sim/` **[built]** | `ClientCore`: the client's own instance of the sim-core WASM on the main thread (`dwell_client_*` exports): the C++ `Predictor` (prediction world with the local player, dead-reckoned remote proxies, terrain), and visible chunk faces for rendering. |
| `predict/` **[built]** | Keyboard + pointer-lock input (WASD, Space, Shift, C/Ctrl, F3); touch controls for phones and tablets (`touch.ts`: floating left-half joystick, drag-to-look right half, Jump/Crouch/Run buttons, one captured Pointer Events pointer per control, merged into the same sampled input); and input quantization mirroring the C++ `QuantizeInput`. |
| `net/` **[built]** | `Transport` interface; `WebTransportTransport` (cert-hash pinning, stream framing), `WebRtcTransport` (builds the ICE-lite server's answer from the invite), `LoopbackTransport`; `openTransport` picks WebTransport and falls back to WebRTC (`?transport=` forces one); invite parsing; `ClientSession` (handshake, reliable and datagram RTT, gameplay messages); `SimulatedTransport` (`?netsim=rtt,jitter,loss%`). |
| `protocol/` **[built]** | Codecs mirroring the C++ ones, constants generated from `shared/protocol`. |
| `identity/` **[built]** | Device key (§10.4): non-extractable Ed25519 WebCrypto key in IndexedDB. |
| `local/` **[built]** | Local mode: `LocalCore` wrapper over the WASM exports and the module worker hosting it. |
| `ui/` **[built]** | Connection status overlay (transport, player id, RTTs, server tick); HUD (crosshair, health, death message) and the F3 debug overlay (PLAYER_CONTROLLER.md §9). |
| `world/` **[in progress]** | Material ids and render styles (mirroring `voxel.h`). Chunk store mirrored from the server, applying voxel deltas in order, arrives in Phase 3 (Phase 2 clients generate the world from its generator version inside the sim core). |
| `worldgen/` | Worldgen worker pool running the server's C++ terrain generator (WASM) for `Generated` chunks. |
| `mesh/` | Greedy-mesher worker pool; produces render meshes and collision triangles. |
| `render/` **[built: terrain chunks, player capsules, camera, debug lines]** | Thin Dwell-owned render interface (chunk meshes, dynamic body meshes, player views, camera rig, debug draw) implemented on **Three.js / WebGL2** ([ADR 0002](./adr/0002-client-renderer.md)). Chunks use packed custom geometry, own shader materials, and a block texture array; rendering is camera-relative. Game code never touches Three.js objects directly. Phase 2: chunk meshes built from the sim core's visible faces (`chunkMesh.ts`: one quad per face, water in a transparent pass), capsule players, the camera, and debug line segments. **Block textures** (`textures.ts`): generated at startup from tiled noise — periodic value-noise fBm whose lattice wraps at the 32-texel tile, so every tile is seamless across blocks — for grass (top, side with a grass fringe, dirt bottom) and stone (also slabs); packed in a 256² atlas with 16-texel wrapped gutters (mipmapped without bleeding, nearest-filtered up close). Faces get UVs within their tile; vertex colours carry face shading (and the flat colour of untextured materials). |
| `physics/` | Debris world (Phase 5) in the sim-core WASM; the prediction world lives in `sim/`. The client does not use separate Jolt JS bindings. |
| `interp/` | Tier 1 transform interpolation (and bounded extrapolation), Phase 4; player interpolation is in `game/remotes.ts`. |
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
| Worldgen | Worker pool (≈ cores − 2) | Worker pool or threads | Thread pool |
| Meshing (lighting later) | Worker pool | Worker pool | — (server does not mesh for rendering) |
| Storage (SQLite) | Inside the sim-core worker (OPFS) | Same | I/O thread |

Workers share no memory; jobs and results are transferable `ArrayBuffer`s. Pool sizes are capped,
lower on mobile.

---

## 6. Voxel World

### 6.1 Grid & chunks **[in progress]**

Built (Phases 1–2, `server/core/include/dwell/core/voxel.h`): material table — air, bedrock,
stone, dirt, grass, stone slab, ladders (`ladder_n/e/s/w`), water, and a debug launch pad — where
each material has a collision **shape** (`Empty`, `Full`, `SlabBottom`), `climbable` + facing,
`liquid`, and `launch_speed` (PLAYER_CONTROLLER.md §6); 32³ chunks with revisions (generated chunks
start at revision 0); generate-on-access `VoxelWorld`; a flat test world (grass top face at y = 0,
bedrock at the bottom); and a **playground** generator (flat world plus slab stairs, a block step,
a 1×2 doorway, a crawlspace, a ladder to a ledge, a pool, and a launch pad near the spawn) for
movement testing. **Terrain collision** (`terrain_collision.h`, Phase 2): one static Jolt body with
a `MutableCompoundShape` of per-chunk `MeshShape`s (unit quads per exposed face, no greedy merge),
built around players and rebuilt in the same tick as an edit (PLAYER_CONTROLLER.md §5). Encoding,
streaming, and procedural generation come in Phase 3.

- Voxel = 1 m cube; `uint16` material ID (0 = air). Material table defines density,
  strength, and render properties and is shared by server and client.
- Chunk = **32 × 32 × 32** voxels, addressed by `ChunkCoord(int32 x, y, z)`.
- Wire / storage encoding: per-chunk **palette + run-length encoding**, then optional
  general-purpose compression for large transfers.
- Each chunk carries a monotonically increasing `revision` so clients can detect and
  discard stale or out-of-order updates and request resync.

### 6.2 Voxel states (the three physics categories)

| State | Where it lives | Physics cost | Synchronized |
|---|---|---|---|
| **Static terrain** | Master grid; baked into per-chunk Jolt `MeshShape` | ~0 (static bodies) | Reliable chunk data + deltas |
| **Tier 1 dynamic** (gameplay-critical) | Server Jolt world as one `CompoundShape` body per cluster | Active | Unreliable snapshots, 20 Hz |
| **Tier 2 dynamic** (cosmetic debris) | Client-local Jolt world only | Client only | Not synchronized; derived from a reliable event |

Static collision uses a per-chunk `MeshShape` built from greedy-meshed faces (a
`HeightFieldShape` cannot represent overhangs/caves; it may be used for far/LOD terrain only).
Both server and client build the same collision mesh from the same chunk data so client
prediction collides with the same geometry the server does.

### 6.3 Terrain Generation **[planned]**

Terrain is **procedural, seeded, and deterministic**: an unmodified chunk is a pure function
`generate(worldSeed, generatorVersion, ChunkCoord)`. The server is authoritative, but because
generation is deterministic, the network and disk only need to carry *differences* from the
generated baseline.

#### World bounds
- Vertical: `WORLD_MIN_Y` = −128 to `WORLD_MAX_Y` = 384 (16 chunks tall).
- Horizontal: bounded to ±`WORLD_HALF_EXTENT` (65 536 m). At that distance float32 precision
  is ~8 mm, acceptable for single-precision Jolt. The client renders relative to a
  floating origin (camera-relative) to avoid visual jitter. Larger worlds would require Jolt's
  `JPH_DOUBLE_PRECISION` build (open decision #7).
- The bottom layers (`y < WORLD_MIN_Y + BEDROCK_LAYERS`) are indestructible **bedrock** — the
  primary anchor for structural integrity (§7.1).

#### Generator pipeline
Executed per chunk; stages that need neighbor context (trees crossing chunk borders) read a
deterministic function of the neighbor's coordinates, never the neighbor's generated data, so
chunks can be generated in any order and in parallel.

1. **Climate / biome field (2D).** Low-frequency noise for temperature, humidity,
   continentalness, and erosion → biome ID per column (plains, forest, desert, mountains,
   ocean, …) with smooth blending at borders.
2. **Base height (2D).** Biome-weighted blend of fractal noise (fBm + ridged noise for
   mountains) → terrain height per column.
3. **Density (3D).** `density = (height − y) + overhangNoise3D(x, y, z)`; solid where
   `density > 0`. Produces overhangs and arches on top of the heightmap.
4. **Caves (3D).** Carve with "spaghetti" (|noiseA| + |noiseB| < t) and "cheese" (large
   blobs) cave noise, attenuated near the surface and near bedrock.
5. **Surface & strata.** Top-down column pass assigns grass/sand/snow, then dirt, then stone
   layers by depth and biome; water fills below `SEA_LEVEL`.
6. **Ores.** Seeded vein placement per chunk (material, depth range, frequency, vein size).
7. **Features / structures.** Trees, boulders, and later hand-authored structures, placed at
   deterministic hashed positions per region; each feature writes only voxels inside the chunk
   being generated.
8. **Stability pass.** Remove small floating components (islands with no path to terrain
   within the chunk's generation neighborhood) so that newly generated terrain does not
   collapse the first time a nearby voxel changes. Large generated overhangs are allowed and
   are subject to normal integrity rules once edited.

#### Determinism
Server (native), local mode (WASM), and client (WASM) must produce **bit-identical** chunks:
- The generator lives in `server/core/worldgen` (C++) and the client runs the **same code
  compiled to WASM** in a worker — there is no second TypeScript implementation.
- Noise uses integer hashing for gradients and evaluates in **fixed-point** (or strict IEEE
  float with `-ffp-contract=off`, no `-ffast-math`, no transcendental library calls) so
  native and WASM results match.
- A golden test generates a fixed set of chunks and compares hashes across native and WASM in CI.
- `generatorVersion` is bumped for any change that alters output; saved worlds record it.

#### Authority, storage, and streaming
- The server keeps only **modified** chunks in memory and in the world database (§6.4);
  unmodified chunks are regenerated on demand and evicted freely.
- Handshake sends `worldSeed` and `generatorVersion`. The client generates a verification
  chunk and reports its hash; on mismatch (or on low-power devices by choice) the client uses
  **full-chunk mode** and the server sends every chunk explicitly.
- `ChunkData` has two forms: `Generated(coord, revision)` — "generate this yourself, no
  changes" — and `Explicit(coord, revision, palette+RLE)` for modified chunks. This cuts
  terrain bandwidth for unexplored or untouched areas to a few bytes per chunk.
- Server generation runs on a worker thread pool with a per-tick budget; the spawn region is
  pre-generated at startup. Client generation runs in the worldgen worker pool (§5.1).

### 6.4 World Persistence **[planned]**

Decision: [ADR 0006](./adr/0006-world-persistence-sqlite.md). Each world is **one SQLite database
file** holding **all** of its data; nothing about a world lives in side files.

| Table | Contents |
|---|---|
| `meta` | Format version, world seed, generator version, spawn, world time, timestamps, preview image |
| `settings` | Name, MOTD, icon, max players, visibility, password hash, online/offline mode, physics/view caps, autosave and backup policy |
| `chunks` | Modified chunks only: `(cx, cy, cz)`, revision, generator version, zstd-compressed palette + RLE blob (same encoding as `ChunkData Explicit`) |
| `players` | Keyed by device public key: display name, state blob (position, health, later inventory), first/last seen |
| `bodies` | In-flight Tier 1 clusters (voxel layout, transform, velocities) |
| `permissions` | Ops, bans, allow-list by public key, with reason/by/when |

- **Same code everywhere:** SQLite and zstd are compiled into `server/core` (`core/storage`).
  Backends: the native file VFS (WAL mode) for dedicated servers; an OPFS
  `FileSystemSyncAccessHandle` VFS inside the sim-core worker for browsers (no cross-origin
  isolation needed; files under a `dwell/` OPFS directory); Capacitor uses the OPFS VFS or a
  native-file plugin VFS per platform.
- **Saving:** every `AUTOSAVE_SECONDS`, dirty chunks, players, bodies, and meta commit in one
  transaction, prepared on the tick and committed off it. Also on shutdown and when a friend-world
  host backgrounds.
- **Backups:** dedicated servers take rotating backups with SQLite's online backup API.
- **Portability:** export/import of a `.dwellworld` file (the database itself) works across
  dedicated servers, browsers, and apps.
- **Migrations:** `meta.format_version` with ordered migrations on open.
- **Settings and permissions** are edited through admin commands and a server CLI; the only
  non-database inputs to a dedicated server are launch options (world file, bind address/port).

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
| `AUTOSAVE_SECONDS` | 30 s | World autosave interval (per-world override in `settings`) |
| `CHUNK_SIZE` | 32 | Voxels per chunk edge |
| **Players (§9)** | | |
| Controller tuning | see `PLAYER_CONTROLLER.md` §7 | Capsule, speeds, jump, crouch, climb, swim, push, damage |
| `RECONCILE_SNAP_DISTANCE` | 1.0 m | Correction above this snaps instead of smoothing |
| `PREDICT_PROXY_RADIUS` | 16 m | Tier 1 bodies within this use present-time proxies |
| `RESPAWN_SECONDS` | 5 s | Death → respawn delay |
| `MAX_HEALTH` | 100 | Player health |
| `MAX_INPUTS_PER_DATAGRAM` | 4 | Input redundancy per `PlayerInput` |
| **Terrain (§6.3)** | | |
| `WORLD_MIN_Y` / `WORLD_MAX_Y` | −128 / 384 | Vertical world bounds |
| `WORLD_HALF_EXTENT` | 65 536 m | Horizontal world bound |
| `BEDROCK_LAYERS` | 4 | Indestructible anchor layers at the bottom |
| `SEA_LEVEL` | 64 | Water fill height |

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
| framing | first byte = channel id, then `u32 LE length ‖ payload` per message | none (SCTP keeps message boundaries) | none |
| transport binding | SHA-256 of the server certificate | same (DTLS uses that certificate) | 32 zero bytes |

### 8.2 Channels

| Channel | Kind | Content |
|---|---|---|
| Datagrams | Unreliable | Player input (C→S), physics snapshots (S→C) |
| Stream `control` | Reliable, bidi | Status query, handshake + identity challenge, ping/clock sync, chat, block edit requests |
| Stream `world` | Reliable, uni S→C | Chunk data, `VoxelModification`, `PhysicsEvent`, `PlayerEvent`, entity spawn/despawn |

All world-affecting reliable messages go on **one** ordered stream so a voxel removal and the
event/entity that depends on it can never be reordered. Bulk chunk streaming may move to
separate uni streams later if head-of-line blocking is measured to matter.

`MAX_DATAGRAM_BYTES` = 1200 (safe QUIC payload); `MAX_RELIABLE_MESSAGE_BYTES` = 1 MiB. WebRTC
reliable messages are additionally capped by SCTP `max-message-size` (256 KiB), so large world
messages (Phase 3 chunk data) must stay under it or be split. All multi-byte fields
little-endian; strings are `u16 byte length ‖ UTF-8`, validated and capped per field
(`shared/protocol/constants.json` `limits`).

### 8.3 Message formats

Every message starts with a `u8` type (`constants.json` `messageTypes`). **Built (protocol v2):**
`DatagramPing` 0x02 / `DatagramPong` 0x82, `StatusRequest` 0x40 / `StatusResponse` 0x41,
`ClientHello` 0x42, `Challenge` 0x43, `ClientAuth` 0x44, `Welcome` 0x45, `Reject` 0x46, `Ping`
0x47 / `Pong` 0x48 (Phase 1); `PlayerInput` 0x01, `PhysicsSnapshot` 0x81, `PlayerEvent` 0x30
(Phase 2) — layouts pinned by `shared/protocol/vectors.txt` (C++, TypeScript, and the Python
reference encoder, including half floats). The remaining formats below are drafts, finalized in the
phase that builds them. Enumerations and bit sets (`inputButtons`, `playerStates`, `playerFlags`,
`controllerFlags`, `groundKinds`, `playerEventKinds`, `damageCauses`) are generated from
`constants.json`; decoders reject unknown values.

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
  f32×3 position                // capsule centre (feet + half height)
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
  f32×3 feet position, f16×3 velocity
  i16  yaw, i16 pitch
  u8   state                    // player::State (animation)
  u8   flags                    // PlayerFlags
```
Phase 4 adds, for riding Tier 1 bodies (§9.4), a `u32 groundEntityId` to the local and remote
blocks (positions become body-local when it is non-zero), and the Tier 1 entity list:
```
u8   entityCount
repeat entityCount:             // ~32 bytes each → ~34 entities per datagram
  u32  networkEntityId
  f32×3 position
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
S→C  Welcome       u16 playerId, u64 worldSeed, u32 generatorVersion, u32 serverTick
     or Reject     u8 reason (ProtocolVersion, Banned, Full, NotAllowListed, AuthFailed,
                   Malformed, Replaced), str message — followed by closing the session
C→S  WorldgenCheck hash of a generated verification chunk → generated vs. full-chunk mode (§6.3)
```
The signature covers `"dwell-auth-v1" ‖ nonce ‖ transport binding ‖ publicKey`; the binding
(§8.1) ties it to the server certificate, so a signed challenge cannot be relayed to a different
server. **[built]** A successful login for a key that already has a joined session replaces it:
the old session receives `Reject(Replaced)` and is closed (so a dropped connection can rejoin
immediately). `Ping`/`Pong` (reliable) and `DatagramPing`/`DatagramPong` carry the client time and
server tick for RTT and clock sync. `WorldgenCheck` arrives in Phase 3.

**Server → Client: `ChunkData` (reliable, `world`)**
```
u8   type = 0x11
u8   form                       // Generated = 0 (client generates; no payload) | Explicit = 1
ChunkCoord, u32 revision
[Explicit only: palette + RLE runs]
```

**Server → Client: `PlayerEvent` (reliable, `world`) [built]** — to every joined client
```
u8   type = 0x30
u8   kind                       // Knockback 1 | Damage 2 | Death 3 | Respawn 4
u16  playerId
u32  serverTick                 // tick the effect was applied
u32  inputSeq                   // that player's input processed on serverTick (predicted replay)
Knockback, Respawn: f32×3       // velocity change / respawn feet position
Damage:  u8 amount, u8 cause    // cause: Fall 1 | Crush 2 | Explosion 3
Death:   u8 cause
```

**Server → Client: `VoxelModification` (reliable, `world`)**
```
u8   type = 0x10
u8   reason                     // Edit | Explosion | Collapse | Rebake
u32  serverTick
u16  chunkCount
repeat: ChunkCoord, u32 newRevision, u16 changeCount,
        repeat: u16 localIndex (x|y<<5|z<<10), u16 material
```

**Server → Client: `PhysicsEvent` (reliable, `world`)**
```
u8   type = 0x20
u8   kind                       // Explosion = 1
f32×3 origin, f32 force, f32 radius
```

**Server → Client: `EntitySpawn` / `EntityDespawn` (reliable, `world`)** — `NetworkEntityID`,
initial transform, and (spawn only) the cluster voxel layout (local offsets + materials).

---

## 9. Players: Physics-Based Characters **[built]** (Tier 1 interactions: Phase 4)

Full specification: **[`PLAYER_CONTROLLER.md`](./PLAYER_CONTROLLER.md)** — a port of the
[Physics Player Controller](https://github.com/zacharysnewman/physics-player-controller)
(its deterministic Quantum 3 version) to C++/Jolt and Dwell's voxel world. This section is the
architectural summary.

### 9.1 Character model
- Each player is a **dynamic Jolt body** (capsule, rotation locked via `EAllowedDOFs`, gravity
  factor 0, frictionless, never sleeps) driven by **velocity layers**: horizontal, vertical, and
  exclusive climb/swim layers. The aggregate pass writes the summed target velocity to the body
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
  golden trace and the 64-player < 1 ms/tick gate). Player bodies use the `Character` object layer
  and a Jolt `ContactListener` (installed by `Players`) that records their contact normals.

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
  (Phase 4), explosion (Phase 5). `Damage`, `Death`, and `Respawn` are `PlayerEvent`s to every
  client; a dead player's body leaves the physics world, and snapshots mark it `dead` where it
  died. The server respawns the player at the spawn point after `RESPAWN_SECONDS`.
- On death clients currently draw the body lying down (a cosmetic pose). The **cosmetic ragdoll**
  (Jolt `Ragdoll`, Tier 2 rules — local, unsynchronized, despawned on respawn) arrives with the
  client debris world in Phase 5.

### 9.7 Anti-cheat implications
Clients only send inputs, never positions, so speed/teleport/fly hacks are structurally
impossible; the server's simulation is the only source of player state.

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
- Operator settings, stored in the world database's `settings` table (§6.4): name, MOTD,
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
A small HTTPS JSON service (`services/master`; hostname chosen in Phase 7); no game
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
  (`identity` store); apps: OS keychain/keystore (Phase 7). Because the key is non-extractable it
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
- Block edits are checked for reach, line of sight, cooldown, and permissions; placements that
  overlap any player capsule are rejected.
- Message decoders bounds-check every length field; malformed messages drop the connection.
- **Servers are player-run and untrusted by clients:** the client validates every server message
  as strictly as the server validates client messages (bounds, sizes, rates), and never executes
  server-provided content.
- **Clients are identified by device key** (§10.4); impersonation requires the private key.
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
| 6 | Final values for §7.4 tunables | Tune in Phases 4–6 |
| 7 | Worlds larger than ±65 km (Jolt `JPH_DOUBLE_PRECISION`) | Not needed initially |
| 8 | Worldgen noise numerics: fixed-point vs. strict IEEE float | Prototype both in Phase 3; pick by golden-test stability and speed |
| 9 | Movement feel on voxels: PPC recommended feel (walk 5 / run 8 m/s) vs. slower voxel-genre speeds | Start with PPC feel; playtest in Phase 2 |
| 10 | Master server platform, database, and hostname | **Deferred to Phase 7** (not needed before). Constraint: $0 during development. Candidates: Cloudflare Workers + Durable Objects + D1 (no UDP → player-attested reachability, managed TURN) or a free-tier VM with a Rust service (UDP → master probes, co-located `coturn`). Either way the service runs locally (Docker / Wrangler) for dev and CI |
| 11 | TURN relay: managed vs. self-hosted `coturn` | **Deferred to Phase 7**, decided with #10; public Google STUN until then |
| 12 | ~~Trusted hostnames for player servers~~ | **Deferred:** out of scope — see [`FUTURE.md`](./FUTURE.md) |
| 13 | ~~Dedicated-server fallback transport~~ | **Resolved:** WebRTC (`str0m`), no WebSocket — [ADR 0008](./adr/0008-dedicated-server-transports.md) |
| 14 | ~~Own subdomain for the client~~ | **Deferred:** out of scope — see [`FUTURE.md`](./FUTURE.md) (ADR 0005) |
| 15 | ~~Friend-world host migration~~ | **Resolved:** no migration; sessions end with the host — [ADR 0009](./adr/0009-friend-world-lifetime.md). Migration and paid cloud worlds in [`FUTURE.md`](./FUTURE.md) |
| 16 | ~~Dedicated servers accepting WebRTC~~ | **Resolved** with #13 — [ADR 0008](./adr/0008-dedicated-server-transports.md) |
