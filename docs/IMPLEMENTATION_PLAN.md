# Dwell — Phased Implementation Plan

This plan turns the voxel multiplayer physics spec into buildable phases. The architecture
lives in [`ARCHITECTURE.md`](./ARCHITECTURE.md); section references (§) point there.

**Deployment order:** GitHub Pages (static web client) → Electron → Capacitor.
The authoritative server always runs outside GitHub Pages, except for **local mode**, where
the server core runs in the browser as WASM (§2.1).

Each phase lists its **goal**, **deliverables**, and **exit criteria**. A phase is done only
when its exit criteria pass and `ARCHITECTURE.md` reflects what was built.

---

## Phase 0 — Repository, Tooling & GitHub Pages Pipeline

**Status:** code complete. Exit criteria verified locally (client lint/typecheck/test/build and a
headless render check; server build, tests, and smoke run). Remaining: the first GitHub CI run and
Pages deployment (requires Pages source = "GitHub Actions" in repository settings).

**Goal:** A working monorepo skeleton that deploys a blank client to GitHub Pages on every
push to the default branch.

Deliverables
- Monorepo layout per §3: `client/`, `server/`, `shared/protocol/`, `platforms/`, `docs/`.
- `client/`: Vite + TypeScript (strict), ESLint, Prettier, Vitest. `base: '/dwell/'`.
  Three.js (pinned) behind the `client/render` interface (ADR 0002); renders an empty scene
  and a build-info overlay (commit SHA).
- `server/`: CMake project, C++20, Jolt pulled via `FetchContent`, a unit-test target
  (e.g. Catch2/doctest), clang-format config. Rust toolchain pinned with `rust-toolchain.toml`;
  empty `server/net/wt` crate linked through Corrosion (ADR 0001) so the mixed build works from
  day one.
- GitHub Actions:
  - `ci.yml` — lint, typecheck, test, build for client; configure/build/test for server
    (C++ + cargo, with cargo caching, `cargo clippy`, and a stale-`cbindgen`-header check).
  - `pages.yml` — build client, upload artifact, `actions/deploy-pages`.
- `docs/adr/` with an ADR template.

Exit criteria
- `https://dropkickarcade.com/dwell/` serves the blank client (default project path under the
  user site's custom domain; ADR 0005).
- CI is green on both client and server jobs.

---

## Phase 1 — Server Core, Jolt, Headless Voxel Grid & Transport

**Goal:** Spec Phase 1. A C++ server with Jolt and a headless voxel grid, reachable from the
browser client over WebTransport, plus local mode so the Pages build works without a server.

Deliverables
- **Server core (`server/core`)**
  - Fixed-timestep loop: 60 Hz step, 20 Hz snapshot tick (§4.2).
  - Jolt `PhysicsSystem` initialized with object/broad-phase layers
    (static terrain, Tier 1 dynamic, characters).
  - Headless voxel grid: chunk storage (32³, §6.1), material table, flat test world generator.
  - Core exposes a message-in / message-out interface with no OS dependencies (§4.3).
- **Protocol (`shared/protocol`)**
  - v0 message definitions (§8.3) and shared constants (§7.4).
  - Encoders/decoders in TS and C++, with golden-byte test vectors both sides must pass.
- **Server network front-end (`server/net`)**
  - **Spike first:** `wtransport` crate behind the C ABI (ADR 0001) accepts a session from
    Chrome (dev cert via `serverCertificateHashes`) and Electron; one reliable stream message
    and one datagram each way. Fallback if it fails: Google QUICHE behind the same C ABI.
  - WebTransport server on that crate: handshake, `control` + `world` streams, datagram
    send/receive, ping / clock sync; transport events drained into the main loop once per tick.
  - Status query and join handshake with **device-key identity** (ARCHITECTURE §8.3, §10.4,
    ADR 0004): Ed25519 challenge signed by the client, bound to the transport; protocol-version
    rejection with reasons.
  - Dev TLS: the crate generates a short-lived ECDSA cert at startup and prints its SHA-256 for
    `serverCertificateHashes`.
  - WebRTC fallback endpoint (ADR 0008): ICE-lite `str0m` in the same crate, same channel
    mapping and framing. Spike includes the invite-link path (client synthesizes the remote
    description from address + fingerprint + ICE credentials); if it fails, invite-link
    fallback joins require the master server (Phase 7).
- **Client networking (`client/net`)**
  - `Transport` interface with `WebTransportTransport`, `WebRtcTransport`,
    `LoopbackTransport` (§8.1). Auto-select: WebTransport → WebRTC.
  - Connect via invite link `?join=host:port&cert=<sha256>`; `?local=1` forces local mode.
  - Device key: generated on first run (non-extractable WebCrypto Ed25519 in IndexedDB),
    export/import.
  - Connection status + RTT overlay.
- **Local mode**
  - Emscripten build target for `server/core` (Jolt linked in, single-threaded per ADR 0007;
    Jolt `JobSystemSingleThreaded` vs. `JobSystemThreadPool` selected at build time); loaded in a
    Web Worker by the client and wired through `LoopbackTransport`. Added to the Pages workflow.
    The same build later hosts client prediction (Phase 2) and worldgen (Phase 3).
- **Electron shell (`platforms/electron`)** loading the same build; connects to a local server.

Exit criteria
- Browser (Chromium) and Electron connect to a native server on localhost via WebTransport,
  exchange ping/pong over datagrams and a reliable stream, and show RTT.
- Forcing the WebRTC fallback produces the same behavior, including in Safari.
- The Pages deployment runs in local mode with no external server.
- Protocol golden tests pass in both languages.

---

## Phase 2 — Physics Player Controller (Prediction + Reconciliation)

**Goal:** Spec Phase 2. Port the Physics Player Controller to C++/Jolt on voxel terrain, then
network it with client prediction and server reconciliation (§9, `PLAYER_CONTROLLER.md`).

The port follows the PPC Quantum port's own phase order, so each step can be checked against the
behaviour and tests of the original.

Deliverables
- **2a — Skeleton.** `PlayerController` state, `PlayerControllerConfig` (recommended-feel defaults
  with voxel dimensions, `PLAYER_CONTROLLER.md` §7), spawn, dynamic Jolt body (rotation locked,
  gravity 0, frictionless, no sleeping, enhanced internal edge removal), aggregate pass, and the
  ordered pipeline running before `PhysicsSystem::Update`.
- **2b — Voxel collision & queries.** Per-chunk static collision `MeshShape` built from the grid
  and rebuilt in the same tick as an edit; `VoxelQuery` (DDA ray cast, capsule-vs-cell overlap)
  merged with Jolt queries for dynamic layers (`PLAYER_CONTROLLER.md` §5); `PlayerTestWorld`
  builder (floors, block and slab steps, walls, 1×2 doorways, crawlspaces, ladder columns, water).
- **2c — Probes & horizontal layer.** Ground/ceiling rings, wall rays; walk/run, accel/decel/reverse,
  air control, step-up, external absorption and decay.
- **2d — Vertical layer & jump.** Gravity, ground following + snap, walk-off, ceiling, launches;
  buffer/coyote in ticks; `Jumped`/`Landed` events.
- **2e — Crouch.** Shape swap with feet planted / head kept; overlap-tested stand-up; crawlspaces.
- **2f — Climb & swim.** Climbable voxel materials with facing variants; exclusive climb layer;
  water and the exclusive swim layer (Dwell addition); optional auto-jump and edge guard.
- **2g — Test port.** The PPC headless suites ported to C++ on voxel geometry
  (`PLAYER_CONTROLLER.md` §10), including the multi-player golden trace.
- **2h — Networking.**
  - Server: input queue ordered by `inputSeq`, validation and rate limiting (§11); snapshots with
    `ackInputSeq`, body state, and local controller state (~48 B).
  - Client: sim-core WASM build hosts the prediction world (terrain + kinematic proxies + local
    dynamic body); input ring buffer; redundant input datagrams (last 4); analog move vector.
  - Reconciliation: compare prediction at `ackInputSeq`; replay only on mismatch; smooth small
    corrections, snap large ones.
  - Knockback: `PlayerEvent(Knockback, tick)` inserted into prediction history and replayed.
    Tested with a debug launch-pad block.
  - Player-vs-player collision (block/push; standing on heads not carried); remote players as
    interpolated kinematic capsules, animated from `State`.
  - Health, fall damage from `Landed` impact speed, death → cosmetic ragdoll → respawn.
- **2i — Divergence measurement.** Jolt built with `JPH_CROSS_PLATFORM_DETERMINISTIC`, no FMA
  contraction; CI runs the scenario suite natively and under WASM (Node) and reports max per-tick
  divergence, checked against `externalAbsorbThreshold`.
- **Presentation & tools.** First-person camera with crouch eye smoothing and step smoothing;
  debug overlay (state, probe rays, layer velocities, prediction error); network condition
  simulator (latency, jitter, loss).

Exit criteria
- All ported PPC scenarios pass on voxel geometry (natively and in WASM); the golden trace is
  stable across repeated native runs.
- The player fits through 1×2 doorways and, crouched, through 1-tall crawlspaces; full blocks need
  a jump; slabs are stepped up without leaving the ground.
- Two clients see each other move smoothly. With 150 ms RTT, 20 ms jitter and 5 % loss simulated,
  local movement feels immediate, steady-state correction error stays under ~5 cm, and most
  snapshots need no replay.
- Debug knockback plays smoothly (no snap) at 150 ms RTT; players bump into each other without
  jitter.
- Server rejects out-of-range inputs; fall damage and respawn work on all clients.
- 64 players' controller passes cost < 1 ms/tick on the reference server (excluding the Jolt step).

---

## Phase 3 — Static Terrain Streaming

**Goal:** Spec Phase 3. Generate a real procedural world, stream it reliably, and keep client
and server collision identical (§6).

Deliverables
- **Terrain generator** in `server/core/worldgen` (§6.3), built in stages:
  1. Deterministic noise library (integer-hash gradients; fixed-point vs. strict-float
     prototype → ADR for open decision #8).
  2. Climate/biome fields, biome-blended base height, 3D overhang density.
  3. Caves (spaghetti + cheese), surface/strata materials, water to `SEA_LEVEL`, bedrock.
  4. Ores and features (trees, boulders) using order-independent hashed placement.
  5. Stability pass removing small floating components.
- Server worldgen thread pool with per-tick budget; spawn region pre-generated. Client worldgen
  and meshing worker pools with transferable buffers (ADR 0007).
- **Cross-platform determinism:** the generator compiled to WASM for the client worldgen worker
  and local mode; CI golden test comparing chunk hashes between native and WASM builds.
- Handshake carries `worldSeed` + `generatorVersion`; client verification-chunk hash selects
  generated vs. full-chunk mode.
- Chunk encoding: palette + RLE (+ optional compression), with `revision` per chunk;
  `ChunkData` `Generated` / `Explicit` forms (§8.3). Server stores only modified chunks.
- **World persistence** (§6.4, ADR 0006): SQLite + zstd in `core/storage`; schema v1 (`meta`,
  `settings`, `chunks`, `players`, `permissions`); native VFS (WAL) and OPFS VFS in the worker;
  transactional autosave of dirty data off the tick; load on start; migrations framework.
  Local-mode worlds persist across page reloads.
- Debug tooling: seed selector, biome/heightmap overlay, "regenerate chunk and diff" check.
- Interest management: per-client view radius; stream nearest-first; unload far chunks;
  bandwidth budget per client.
- Greedy mesher shared in spirit by both sides:
  - Server: per-chunk Jolt `MeshShape` static bodies, rebuilt on change (built in Phase 2b;
    greedy meshing reduces triangle count here).
  - Client: mesher in a Web Worker producing render mesh + collision triangles; client
    prediction world uses the same collision.
- Block edit loop: client `BlockEditRequest` on `control` → server validation → reliable
  `VoxelModification` broadcast → clients apply in order and re-mesh.
- Revision gap detection → client requests chunk resync.

Exit criteria
- Walking across the world streams chunks without hitches; memory stays bounded when moving.
- A block placed/removed by one client appears for all clients, and the player collides with
  it immediately after the update on both server and client.
- Chunk serialization round-trips byte-for-byte between C++ and TS (golden tests).
- A world edited on a native server and one edited in local mode both survive restarts/reloads;
  a crash mid-save leaves the previous save intact; a world file saved natively opens in the
  browser build and vice versa.
- The same seed produces bit-identical chunks natively, in local mode, and in the client
  worker (CI golden test); untouched chunks cost only a `Generated` message on the wire.
- Generated terrain shows distinct biomes, caves, and overhangs, and the player can walk,
  jump, and swim through it with no collision mismatches.

---

## Phase 4 — Voxel Awakening (Integrity + Flood-Fill → CompoundShapes)

**Goal:** Spec Phase 4. Detached structures become single Jolt bodies (§7.1).

Deliverables
- Anchor definition (bedrock layer + `grounded` flag) and budgeted 6-connected flood-fill
  structural-integrity pass triggered by voxel removal (`INTEGRITY_BUDGET_VOXELS`).
- Clustering of detached components; removal from grid in the same `VoxelModification`
  (reason `Collapse`).
- Cluster → Jolt `StaticCompoundShape` of boxes; mass/COM/inertia from material density.
- `NetworkEntityID` allocation; reliable `EntitySpawn` (voxel layout) / `EntityDespawn`.
- Tier 1 snapshot replication for all clusters (tiers are introduced in Phase 5); client
  interpolation and rendering of cluster meshes; kinematic proxies in client physics worlds.
- **Player ↔ Tier 1 interaction** (§9.2, §9.4):
  - Players push light clusters (contact mass scaling: `maxPushForce`, `pushableMassLimit`);
    moving clusters push players (absorbed as external velocity).
  - Standing on / riding moving clusters: `groundEntityId` + body-local player state in
    snapshots; client predicts in the body's frame.
  - Present-time proxies for Tier 1 bodies within `PREDICT_PROXY_RADIUS`; the ground body is
    rendered at present time.
  - Crush damage / crush death (contact listener, `PLAYER_CONTROLLER.md` §6.6).
  - Port of the PPC platform suite onto Tier 1 bodies (translating, rotating, falling).
- Admin/debug command: cut a pillar / delete a region to trigger collapses on demand.

Exit criteria
- Removing the supports of a tower causes it to fall as one body, on all clients, in sync.
- A 10 000-voxel detached structure is processed without the server tick exceeding its budget
  (work is spread across ticks).
- Unit tests for integrity/clustering on crafted voxel layouts (bridges, overhangs, rings).
- A player can ride a falling slab to the ground at 150 ms RTT without sliding off or
  jittering; a player under a falling tower is crushed on every client consistently.
- Collapsing generated terrain (a cave ceiling, an overhang) works like built structures.

---

## Phase 5 — Tiered Physics (Authoritative Tier 1, Cosmetic Tier 2)

**Goal:** Spec Phase 5. Keep CPU and bandwidth bounded during large explosions (§7.2).

Deliverables
- Explosion system on the server: radius/force, material strength attenuation, impulse to
  existing Tier 1 bodies, newly awakened clusters, and **players** (knockback + damage via
  `PlayerEvent`, replayed into prediction).
- Tier 2 debris collides one-way with the local player (debris bounces off; player movement
  unaffected).
- Tier classification: `TIER1_MIN_BLOCKS`, `alwaysAuthoritative` materials.
- Tier 2 path: voxels removed in `VoxelModification` (reason `Explosion`) + `PhysicsEvent`
  `Explosion(origin, force, radius)` in the same reliable batch; client derives debris from the
  removed voxels and simulates in its local debris world.
- Snapshot packing: quantization (§8.3), priority accumulator, multiple datagrams per tick,
  per-client bandwidth budget.
- Client debris lifecycle: lifetime, at-rest removal, `DEBRIS_MAX_BODIES` cap (platform-based).
- **Threading checkpoint (ADR 0007):** profile a browser-hosted friend world at its host
  profile's caps; if simulation-bound, plan a hosting-only threaded web build behind
  `coi-serviceworker`.
- Performance instrumentation: server tick time, active body count, bytes/sec per client;
  client frame time and debris count, shown in a debug overlay.

Exit criteria
- A large explosion (hundreds of fragments) runs at stable server tick rate; only Tier 1 bodies
  appear in snapshots; per-client bandwidth stays within budget.
- Debris looks plausible on each client and never affects gameplay state or player movement.
- Players caught in a blast are knocked back smoothly and consistently across clients.
- Late-joining clients see correct terrain (debris is not replayed, by design).

---

## Phase 6 — Sleep / Re-bake Cycle

**Goal:** Spec Phase 6. Long-running servers keep a bounded number of dynamic bodies (§7.3).

Deliverables
- Sleep monitor with `SLEEP_LINEAR_THRESHOLD`, `SLEEP_ANGULAR_THRESHOLD`, `SLEEP_SECONDS`.
- Grid snapping to 24 axis-aligned orientations; occupied-cell conflict resolution with
  fallback to Tier 2 debris.
- Player-safe re-bake: cells overlapping players are treated as occupied; riders transition
  from body-relative to static ground without a visible pop.
- Re-bake: destroy body → write grid → rebuild chunk collision → integrity check on placed
  voxels → reliable `VoxelModification(Rebake)` + `EntityDespawn` in one batch.
- `MAX_TIER1_BODIES` enforcement (force re-bake of oldest/smallest).
- Persist in-flight Tier 1 bodies in the `bodies` table (ADR 0006) so a world saved mid-collapse
  resumes it on load.
- Soak test harness: headless bots + scripted explosions for hours; tracks body count, memory,
  tick time.

Exit criteria
- After a collapse settles, every cluster is back in the static grid within
  `SLEEP_SECONDS` + a small margin, and clients show identical static terrain.
- A multi-hour soak test shows flat memory and body count and no tick overruns.

---

## Phase 7 — Player Hosting, Master Server & Platform Packaging

**Goal:** Player-hosted multiplayer with no official game servers (ADR 0003): distributable
dedicated servers, friend worlds hostable from any client, a master server for discovery, and
packaged desktop/mobile apps (ARCHITECTURE §10).

Deliverables
- **Dedicated server distribution:** CI builds native binaries (Windows/macOS/Linux) and a
  Docker image per release; settings and permissions in the world database edited via admin
  commands and a server CLI (ADR 0006); ops, kick, ban by key; allow-list/password;
  UPnP/NAT-PMP with port-forward guidance; rotating SQLite online backups; host-configurable
  physics and view caps. Certificate rotation with hash publication.
- **Master server** (`services/master`; hostname and platform → Open Decision
  #10): registration + heartbeat, reachability-verified public listing (method per #10), join codes, cert-hash
  distribution, rate limiting per key/IP.
- **World export/import** (`.dwellworld`) across dedicated servers, browsers, and apps; Capacitor
  storage VFS verified per platform.
- **Server browser** in the client: listing, search/filter, client-side ping, status query,
  incompatible-version marking; versioned client builds at `/dwell/v/<version>/`.
- **Friend worlds:** WebRTC transport (§8.1) in the client; hosting the integrated server over
  WebRTC; signaling via the master; STUN + TURN relay (Open Decision #11) with short-lived credentials; host profiles (player and physics caps);
  host-backgrounded pause.
- **Electron:** packaging for Windows/macOS/Linux (electron-builder), custom protocol with
  COOP/COEP and the multithreaded sim-core build (ADR 0007),
  "Host world" launching the native server; LAN discovery.
- **Capacitor:** Android and iOS projects; check `SharedArrayBuffer` availability on the app
  scheme (threaded build if available, ADR 0007); verify WebTransport per WebView (WebRTC
  where unavailable); touch
  controls (auto-jump preset); mobile caps; friend-world hosting with backgrounding handling.
- Dedicated-server WebRTC joins through master signaling and TURN (servers behind strict NAT).

Exit criteria
- A player hosts a dedicated server at home from the downloadable binary; players on the GitHub
  Pages site, Electron, and a phone find it in the server browser and see the same collapse.
- A phone hosts a friend world; a browser player and an Electron player join by code, including
  one on mobile data through the TURN relay.
- Certificate rotation on a dedicated server is invisible to players joining through the master.
- An iOS Safari player joins a self-signed dedicated server over WebRTC, including one behind
  strict NAT via TURN.
- An outdated client is rejected with a clear message and offered the matching versioned build.

---

## Cross-Cutting Work (every phase)

- Keep `docs/ARCHITECTURE.md` current (required by `CLAUDE.md`).
- Record significant choices as ADRs in `docs/adr/`.
- Every new message type gets golden-byte tests in both TS and C++.
- CI must stay green; the Pages deployment must stay playable in local mode.
