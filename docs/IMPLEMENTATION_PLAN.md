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

**Goal:** A working monorepo skeleton that deploys a blank client to GitHub Pages on every
push to the default branch.

Deliverables
- Monorepo layout per §3: `client/`, `server/`, `shared/protocol/`, `platforms/`, `docs/`.
- `client/`: Vite + TypeScript (strict), ESLint, Prettier, Vitest. `base: '/dwell/'`.
  Renders a blank canvas and a build-info overlay (commit SHA).
- `server/`: CMake project, C++20, Jolt pulled via `FetchContent`, a unit-test target
  (e.g. Catch2/doctest), clang-format config.
- GitHub Actions:
  - `ci.yml` — lint, typecheck, test, build for client; configure/build/test for server.
  - `pages.yml` — build client, upload artifact, `actions/deploy-pages`.
- `docs/adr/` with an ADR template.

Exit criteria
- `https://<user>.github.io/dwell/` serves the blank client.
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
  - WebTransport server (choose library → ADR #1). Handshake, `control` + `world` streams,
    datagram send/receive, ping / clock sync.
  - Dev TLS: generate short-lived ECDSA cert; print its SHA-256 for `serverCertificateHashes`.
  - WebSocket fallback endpoint carrying the same framing.
- **Client networking (`client/net`)**
  - `Transport` interface with `WebTransportTransport`, `WebSocketTransport`,
    `LoopbackTransport` (§8.1). Auto-select: WebTransport → WebSocket.
  - Server URL from build config, overridable with `?server=`; `?local=1` forces local mode.
  - Connection status + RTT overlay.
- **Local mode**
  - Emscripten build target for `server/core` (Jolt linked in, single-threaded); loaded in a
    Web Worker by the client and wired through `LoopbackTransport`. Added to the Pages workflow.
    The same build later hosts client prediction (Phase 2) and worldgen (Phase 3).
- **Electron shell (`platforms/electron`)** loading the same build; connects to a local server.

Exit criteria
- Browser (Chromium) and Electron connect to a native server on localhost via WebTransport,
  exchange ping/pong over datagrams and a reliable stream, and show RTT.
- Forcing the WebSocket fallback produces the same behavior.
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
  - Server: input queue ordered by `inputSeq`, validation and rate limiting (§10); snapshots with
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
- Server worldgen thread pool with per-tick budget; spawn region pre-generated.
- **Cross-platform determinism:** the generator compiled to WASM for the client worldgen worker
  and local mode; CI golden test comparing chunk hashes between native and WASM builds.
- Handshake carries `worldSeed` + `generatorVersion`; client verification-chunk hash selects
  generated vs. full-chunk mode.
- Chunk encoding: palette + RLE (+ optional compression), with `revision` per chunk;
  `ChunkData` `Generated` / `Explicit` forms (§8.3). Server stores only modified chunks.
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
- Soak test harness: headless bots + scripted explosions for hours; tracks body count, memory,
  tick time.

Exit criteria
- After a collapse settles, every cluster is back in the static grid within
  `SLEEP_SECONDS` + a small margin, and clients show identical static terrain.
- A multi-hour soak test shows flat memory and body count and no tick overruns.

---

## Phase 7 — Platform Packaging & Hosted Multiplayer

**Goal:** Ship beyond localhost: a hosted server that the GitHub Pages client connects to, and
packaged desktop/mobile builds.

Deliverables
- Server hosting (decide provider → ADR #3): container image, UDP-capable host, trusted TLS
  certificate, health checks, basic metrics/logging.
- Pages build defaults to the hosted server; local mode remains available.
- Electron: packaging for Windows/macOS/Linux (electron-builder), multithreaded Jolt build.
- Capacitor: Android and iOS projects; verify WebTransport in each WebView, fall back to
  WebSocket where unavailable; touch controls; mobile debris/body caps.
- Protocol version handshake so old clients are rejected cleanly.

Exit criteria
- A player on the GitHub Pages site, one on Electron, and one on a phone share a world and see
  the same collapse.

---

## Cross-Cutting Work (every phase)

- Keep `docs/ARCHITECTURE.md` current (required by `CLAUDE.md`).
- Record significant choices as ADRs in `docs/adr/`.
- Every new message type gets golden-byte tests in both TS and C++.
- CI must stay green; the Pages deployment must stay playable in local mode.
