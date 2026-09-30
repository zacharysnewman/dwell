# Dwell — Phased Implementation Plan

This plan turns the voxel multiplayer physics spec into buildable phases. The architecture
lives in [`ARCHITECTURE.md`](./ARCHITECTURE.md); section references (§) point there.

**Deployment order:** GitHub Pages (static web client) → Electron → Capacitor.
The authoritative server always runs outside GitHub Pages, except for **local mode**, where
the server core runs in the browser as WASM (§2.1).

Each phase lists its **goal**, **deliverables**, and **exit criteria**. A phase is done only
when its exit criteria pass and `ARCHITECTURE.md` reflects what was built.

## Progress

Deliverables and exit criteria below are checkboxes, ticked in the same commit that completes
them (see `CLAUDE.md`). This table summarizes each phase.

| Phase | Status | PR |
|---|---|---|
| 0 — Repository, tooling & Pages | ✅ Complete | #2 |
| 1 — Server core, protocol, transports, local mode | ✅ Complete | #3 |
| 2 — Physics player controller | ✅ Complete (playtested; follow-up fixes merged in #7, #8 and #10) | #4, #5, #6, #7, #8, #10 |
| 3 — Terrain generation & streaming | 🚧 In progress — every sub-phase built: 3a–3c merged; 3d (block edits, meshing workers) and 3e (persistence, debug tooling) done on `claude/phase-3d-3e`, PR pending. Outstanding: playtests for the long walk (3b) and walking/jumping/swimming the terrain | #7 (3a), #9 (3b), #11 (re-scope), #12 (3c) |
| 4 — World LOD & whole-world view | 🚧 In progress — 4a, 4b and 4c built, the dev camera replaced by creative flight (merged in #14); playtest follow-ups — fog off, super tall mountains (generator version 4) — merged in #15; chunks shown first on slow devices (#16), no popping when turning and matching distant colours (#17), flight/HUD/transport fixes and the distant-water comparison (#18), distant terrain at its true height and tinted distant water (#19); seamless see-through distant water and no cracks at section borders (#20); z-fighting on distant water fixed (#21); height fog with a settings menu (PR pending); outstanding: the frame-rate check on a desktop and a mobile device | #14–#21 |
| 5 — Voxel awakening | ⏳ Not started | — |
| 6 — Tiered physics | ⏳ Not started | — |
| 7 — Sleep / re-bake | ⏳ Not started | — |
| 8 — Player hosting, master server & packaging | ⏳ Not started | — |

Phase numbering: Phase 4 was inserted on 2026-09-29 for the planet-scale world (ADRs 0011, 0012);
the former Phases 4–7 are now 5–8, and Phase 3's former 3c and 3d are now 3d and 3e.

---

## Phase 0 — Repository, Tooling & GitHub Pages Pipeline

**Status:** complete — CI green and the Pages deployment live (PR #2).

**Goal:** A working monorepo skeleton that deploys a blank client to GitHub Pages on every
push to the default branch.

Deliverables
- [x] Monorepo layout per §3: `client/`, `server/`, `shared/protocol/`, `platforms/`, `docs/`.
- [x] `client/`: Vite + TypeScript (strict), ESLint, Prettier, Vitest. `base: '/dwell/'`.
  Three.js (pinned) behind the `client/render` interface (ADR 0002); renders an empty scene
  and a build-info overlay (commit SHA).
- [x] `server/`: CMake project, C++20, Jolt pulled via `FetchContent`, a unit-test target
  (e.g. Catch2/doctest), clang-format config. Rust toolchain pinned with `rust-toolchain.toml`;
  empty `server/net/wt` crate linked through Corrosion (ADR 0001) so the mixed build works from
  day one.
- [x] GitHub Actions:
  - [x] `ci.yml` — lint, typecheck, test, build for client; configure/build/test for server
    (C++ + cargo, with cargo caching, `cargo clippy`, and a stale-`cbindgen`-header check).
  - [x] `pages.yml` — build client, upload artifact, `actions/deploy-pages`.
- [x] `docs/adr/` with an ADR template.

Exit criteria
- [x] `https://dropkickarcade.com/dwell/` serves the blank client (default project path under the
  user site's custom domain; ADR 0005).
- [x] CI is green on both client and server jobs.

---

## Phase 1 — Server Core, Jolt, Headless Voxel Grid & Transport

**Status:** complete (PR #3), including the manual Safari check.

| Exit criterion | Result |
|---|---|
| Chromium and Electron connect to a native server on localhost via WebTransport, exchange ping/pong over datagrams and a reliable stream, and show RTT | ✅ Automated: Playwright e2e (Chromium) and the Electron `--smoke` run in CI |
| Forcing the WebRTC fallback produces the same behavior, including in Safari | ✅ Chromium (e2e) and Electron (manual, against a routable IP). Safari: ✅ manual check |
| The Pages deployment runs in local mode with no external server | ✅ e2e test against the production build; Pages workflow builds the WASM core |
| Protocol golden tests pass in both languages | ✅ C++ (doctest) and TS (Vitest) against vectors from an independent Python encoder |

Deviations from the deliverables below:
- WebRTC uses its own UDP port (WebTransport port + 1) instead of sharing one (ADR 0008 notes).
- Device keys are non-extractable, so key export/import was dropped (ADR 0004 amendment).
- Added beyond plan: session replacement on re-login (reject reason `Replaced`), a strict CSP, and
  an e2e CI job.
- Electron loads the single-threaded WASM build; the threaded build comes later (ADR 0007).

**Goal:** Spec Phase 1. A C++ server with Jolt and a headless voxel grid, reachable from the
browser client over WebTransport, plus local mode so the Pages build works without a server.

Deliverables
- [x] **Server core (`server/core`)**
  - [x] Fixed-timestep loop: 60 Hz step (§4.2). The 20 Hz snapshot tick moved to Phase 2, where
    snapshots first exist.
  - [x] Jolt `PhysicsSystem` initialized with object/broad-phase layers
    (static terrain, Tier 1 dynamic, characters).
  - [x] Headless voxel grid: chunk storage (32³, §6.1), material table, flat test world generator.
  - [x] Core exposes a message-in / message-out interface with no OS dependencies (§4.3).
- [x] **Protocol (`shared/protocol`)**
  - [x] v0 message definitions (§8.3) and shared constants (§7.4).
  - [x] Encoders/decoders in TS and C++, with golden-byte test vectors both sides must pass.
- [x] **Server network front-end (`server/net`)**
  - [x] **Spike first:** `wtransport` crate behind the C ABI (ADR 0001) accepts a session from
    Chrome (dev cert via `serverCertificateHashes`) and Electron; one reliable stream message
    and one datagram each way. Fallback if it fails: Google QUICHE behind the same C ABI.
  - [x] WebTransport server on that crate: handshake, `control` + `world` streams, datagram
    send/receive, ping / clock sync; transport events drained into the main loop once per tick.
  - [x] Status query and join handshake with **device-key identity** (ARCHITECTURE §8.3, §10.4,
    ADR 0004): Ed25519 challenge signed by the client, bound to the transport; protocol-version
    rejection with reasons.
  - [x] Dev TLS: the crate generates a short-lived ECDSA cert at startup and prints its SHA-256 for
    `serverCertificateHashes`.
  - [x] WebRTC fallback endpoint (ADR 0008): ICE-lite `str0m` in the same crate, same channel
    mapping and framing. Spike includes the invite-link path (client synthesizes the remote
    description from address + fingerprint + ICE credentials); if it fails, invite-link
    fallback joins require the master server (Phase 8).
- [x] **Client networking (`client/net`)**
  - [x] `Transport` interface with `WebTransportTransport`, `WebRtcTransport`,
    `LoopbackTransport` (§8.1). Auto-select: WebTransport → WebRTC.
  - [x] Connect via invite link `?join=host:port&cert=<sha256>[&rtc=<port>&ice=<ufrag>:<pwd>]`;
    `?local=1` forces local mode.
  - [x] Device key: generated on first run (non-extractable WebCrypto Ed25519 in IndexedDB; no
    export — ADR 0004 amendment).
  - [x] Connection status + RTT overlay.
- [x] **Local mode**
  - [x] Emscripten build target for `server/core` (Jolt linked in, single-threaded per ADR 0007;
    Jolt `JobSystemSingleThreaded` vs. `JobSystemThreadPool` selected at build time); loaded in a
    Web Worker by the client and wired through `LoopbackTransport`. Added to the Pages workflow.
    The same build later hosts client prediction (Phase 2) and worldgen (Phase 3).
- [x] **Electron shell (`platforms/electron`)** loading the same build; connects to a local server.

Exit criteria
- [x] Browser (Chromium) and Electron connect to a native server on localhost via WebTransport,
  exchange ping/pong over datagrams and a reliable stream, and show RTT.
- [x] Forcing the WebRTC fallback produces the same behavior (Chromium, Electron).
- [x] … including in Safari (manual check).
- [x] The Pages deployment runs in local mode with no external server.
- [x] Protocol golden tests pass in both languages.

---

## Phase 2 — Physics Player Controller (Prediction + Reconciliation)

**Status:** complete — every deliverable and exit criterion is verified by automated tests (C++
natively and under WASM, TypeScript unit tests, Playwright e2e). Moved out: the cosmetic death
ragdoll and animation from `State` (Phase 6, see below). Playtested by a human (Open Decision
#9): two findings, fixed in #7: forward/back looked faster than
strafing, which was the camera's wide horizontal field of view rather than the sim (now capped
at 100°, `client/src/render/fov.ts`), and the touch Crouch button now holds instead of toggling.
Later finding: jumping onto a block while holding forward gave a
burst of speed as the player came down on the edge — the step-up's forward nudge was added on top
of the tick's movement (also on every slab step, and on slopes every tick); it is now taken out of
that tick's velocity (PLAYER_CONTROLLER.md §4). The fix merged in #8.
Later finding (fix on the branch, PR pending): jumping over a lone block while pressed against it
launched the player forward, and one jump floated the player up a diagonal staircase of full
blocks — the block's top edge deflected the forward drive upwards and the airborne vertical layer
absorbed that as an external force (a second jump). That lift is no longer absorbed
(PLAYER_CONTROLLER.md §4, "Edge lift is not absorbed").

| Exit criterion | Result |
|---|---|
| Ported PPC scenarios pass natively and in WASM; golden trace stable | ✅ `dwell_tests` (Debug and Release) and `dwell_player_tests.js` under Node, in CI |
| Doorways, crawlspaces, blocks need a jump, slabs step up | ✅ `player: voxel geometry`, `crouch`, `steps and air`, `step smoothness` |
| Two clients see each other; 150 ms / 20 ms / 5 % | ✅ `netcode: prediction` (~1 % of snapshots replay, p95 error at ack < 1 mm, no snaps; input applies on the next tick) and e2e `two clients on a native server see each other move` |
| Knockback smooth at 150 ms; bumps without jitter | ✅ launch pad: no snaps, largest rendered step 0.6 m/tick; head-on bump: no snaps, ≤ 0.23 m/tick |
| Out-of-range inputs rejected; fall damage and respawn on all clients | ✅ `netcode: server` |
| 64 players < 1 ms/tick | ✅ ~0.36 ms (native Release), ~0.42 ms (WASM) |

Deviations from the deliverables below:
- `maxStepHeight` is 0.55 m (PPC 0.45) so half-block slabs step up, as the exit criteria require.
- Terrain collision is one static body with a `MutableCompoundShape` of per-chunk meshes, and chunk
  meshes use unit quads rather than greedy-merged faces: separate chunk bodies and T-junctions both
  caused ghost contacts at seams (PLAYER_CONTROLLER.md §5). Phase 3's greedy mesher is therefore for
  render meshes only.
- Controller fixes beyond the PPC (PLAYER_CONTROLLER.md §4): blocked motion isn't absorbed as
  external velocity (needed to jump onto a block while holding forward), step-up nudge and grace,
  crouched step probe height; ladders gained an over-the-top region.
- `PlayerControllerConfig` lives in the C++ core rather than `shared/protocol`: the client runs the
  same C++ in WASM, so no TypeScript copy is needed.
- Added beyond plan: a playground world generator (generator version 1) with every test feature
  near the spawn; `inputBuffer`-driven client tick-rate nudging and a 2-input server jitter buffer;
  `lastKnockbackSeq` in snapshots; the F3 debug overlay's in-world probe rays.
- Remote-player proxies in the prediction world sit at the latest snapshot position,
  dead-reckoned, rather than being interpolated or extrapolated: measured to correct bumps far more
  gently (PLAYER_CONTROLLER.md §8.1).
- Dwell uses right-handed axes: the controller's camera-right vector is the mirror of the PPC's
  (Unity, left-handed) so that strafing matches the screen.
- The snapshot's `groundEntityId` fields are deferred to Phase 5, when Tier 1 bodies can be stood
  on (ARCHITECTURE §8.3).
- Deferred to Phase 6: the cosmetic death ragdoll (Jolt `Ragdoll`) — dead players are drawn lying
  down until the client debris world exists — and animating players from `State` (players are
  capsules; there are no character models yet).

**Goal:** Spec Phase 2. Port the Physics Player Controller to C++/Jolt on voxel terrain, then
network it with client prediction and server reconciliation (§9, `PLAYER_CONTROLLER.md`).

The port follows the PPC Quantum port's own phase order, so each step can be checked against the
behaviour and tests of the original.

Deliverables
- [x] **2a — Skeleton.** `PlayerController` state, `PlayerControllerConfig` (recommended-feel defaults
  with voxel dimensions, `PLAYER_CONTROLLER.md` §7), spawn, dynamic Jolt body (rotation locked,
  gravity 0, frictionless, no sleeping, enhanced internal edge removal), aggregate pass, and the
  ordered pipeline running before `PhysicsSystem::Update`.
- [x] **2b — Voxel collision & queries.** Per-chunk static collision `MeshShape` built from the grid
  (as sub-shapes of one terrain body) and rebuilt in the same tick as an edit; `VoxelQuery` (DDA ray cast, capsule-vs-cell overlap)
  merged with Jolt queries for dynamic layers (`PLAYER_CONTROLLER.md` §5); `PlayerTestWorld`
  builder (floors, block and slab steps, walls, 1×2 doorways, crawlspaces, ladder columns, water).
- [x] **2c — Probes & horizontal layer.** Ground/ceiling rings, wall rays; walk/run, accel/decel/reverse,
  air control, step-up, external absorption and decay.
- [x] **2d — Vertical layer & jump.** Gravity, ground following + snap, walk-off, ceiling, launches;
  buffer/coyote in ticks; `Jumped`/`Landed` events.
- [x] **2e — Crouch.** Shape swap with feet planted / head kept; overlap-tested stand-up; crawlspaces.
- [x] **2f — Climb & swim.** Climbable voxel materials with facing variants; exclusive climb layer;
  water and the exclusive swim layer (Dwell addition); optional auto-jump and edge guard.
- [x] **2g — Test port.** The PPC headless suites ported to C++ on voxel geometry
  (`PLAYER_CONTROLLER.md` §10), including the multi-player golden trace.
- [x] **2h — Networking.**
  - [x] 20 Hz snapshot tick in the server loop (moved from Phase 1).
  - [x] Server: input queue ordered by `inputSeq`, validation and rate limiting (§11); snapshots with
    `ackInputSeq`, body state, and local controller state (47 B + conditional ladder fields).
  - [x] Client: sim-core WASM build hosts the prediction world (terrain + kinematic proxies + local
    dynamic body); input ring buffer; redundant input datagrams (last 4); analog move vector.
  - [x] Reconciliation: compare prediction at `ackInputSeq`; replay only on mismatch; smooth small
    corrections, snap large ones.
  - [x] Knockback: `PlayerEvent(Knockback, tick)` inserted into prediction history and replayed.
    Tested with a debug launch-pad block.
  - [x] Player-vs-player collision (block/push; standing on heads not carried); remote players as
    interpolated kinematic capsules. (Animation from `State` moved to Phase 6.)
  - [x] Health, fall damage from `Landed` impact speed, death → respawn. (The cosmetic ragdoll moved
    to Phase 6; dead players are drawn lying down.)
- [x] **2i — Divergence measurement.** Jolt built with `JPH_CROSS_PLATFORM_DETERMINISTIC`, no FMA
  contraction; CI runs the scenario suite natively and under WASM (Node) and reports max per-tick
  divergence, checked against `externalAbsorbThreshold`. (Measured: positions bit-identical,
  velocities within 1.2 × 10⁻⁷ m/s.)
- [x] **Presentation & tools.** First-person camera with crouch eye smoothing and step smoothing;
  debug overlay (state, probe rays, layer velocities, prediction error); network condition
  simulator (latency, jitter, loss).

Exit criteria
- [x] All ported PPC scenarios pass on voxel geometry (natively and in WASM); the golden trace is
  stable across repeated native runs.
- [x] The player fits through 1×2 doorways and, crouched, through 1-tall crawlspaces; full blocks need
  a jump; slabs are stepped up without leaving the ground.
  Automated: `player: voxel geometry`, `player: crouch`, `player: steps and air`, `player: step
  smoothness`.
- [x] Two clients see each other move smoothly. With 150 ms RTT, 20 ms jitter and 5 % loss simulated,
  local movement feels immediate, steady-state correction error stays under ~5 cm, and most
  snapshots need no replay.
- [x] Debug knockback plays smoothly (no snap) at 150 ms RTT; players bump into each other without
  jitter.
- [x] Server rejects out-of-range inputs; fall damage and respawn work on all clients.
- [x] 64 players' controller passes cost < 1 ms/tick on the reference server (excluding the Jolt step).
  ~0.36 ms/tick in the Release build (`player: performance`, run strict in CI's Release step).

---

## Phase 3 — Static Terrain Streaming

**Status:** in progress — all sub-phases built; two exit criteria await playtests (the long walk,
and walking/jumping/swimming the terrain). Sub-phases: **3a — generator** (done, #7); **3b — streaming** (done,
merged in #9: chunk encoding, `Generated`/`Explicit`, the verification chunk, interest management,
server and client worldgen pools; the walking-without-hitches exit criterion awaits a playtest);
**3c — scale foundations** (done, merged in #12: the planet-scale world of ADR 0011 — bounds and
the rim, double-precision physics with region-anchored terrain collision, protocol v4 positions,
generator version 3, air chunks, spherical streaming; every 3c exit criterion verified);
**3d — block edits** (done, PR pending: protocol v5 edit loop, block interaction and infinite
inventory on desktop and touch, revision gaps and resync, the greedy meshing worker pool; every 3d
exit criterion verified);
**3e — persistence and debug tooling** (done, PR pending: SQLite + zstd world files natively and in
OPFS, autosave off the tick, migrations, crash-safe saves, settings and permissions from launch
options; the in-game terrain map and regenerate-and-diff checks; every 3e exit criterion verified).
3c comes before edits and persistence so the world's
bounds, generator version and wire formats change before saved worlds depend on them.

**Goal:** Spec Phase 3. Generate a real procedural world, stream it reliably, and keep client
and server collision identical (§6) — at planet scale: an 8,192 km disc, 8,192 m tall (ADR 0011).

Deliverables
- [x] **Terrain generator** in `server/core` (`dwell/worldgen`, §6.3), built in stages:
  1. [x] Deterministic noise library (integer-hash gradients; fixed-point vs. strict-float
     prototype → [ADR 0010](./adr/0010-worldgen-noise-numerics.md): strict float).
  2. [x] Climate/biome fields, biome-blended base height, 3D overhang density.
  3. [x] Caves (spaghetti + cheese), surface/strata materials, water to `SEA_LEVEL`, bedrock.
  4. [x] Ores and features (trees, boulders) using order-independent hashed placement.
  5. [x] Stability pass removing small floating components.
- [x] Server worldgen thread pool with per-tick budget; spawn region pre-generated. Client worldgen
  worker pool with transferable buffers (ADR 0007). *(3b)*
- **Scale foundations** *(3c;* [ADR 0011](./adr/0011-planet-scale-world.md)*, §6.3)*:
  - [x] World constants in `shared/protocol/constants.json`: `WORLD_RADIUS` (8 192 000 m),
    `WORLD_MIN_Y` / `WORLD_MAX_Y` (−2 048 / 6 144), `SEA_LEVEL` (0), `POSITION_FIXED_SCALE` (256);
    `WORLD_HALF_EXTENT` and `VIEW_HEIGHT_CHUNKS` removed.
  - [x] Jolt built with `JPH_DOUBLE_PRECISION` natively and in every WASM build; `RVec3` world
    positions through the server, player controller, probes, terrain collision (regions with
    anchors, PLAYER_CONTROLLER.md §5) and predictor; the client sim's state block in doubles.
  - [x] Protocol v4: `pos64` for the local player's snapshot state and `Respawn`, `posfix`
    (1/256 m `i32`) for remote players (entities and `PhysicsEvent` get it when built); C++ and TS
    codecs; golden vectors regenerated from the Python reference encoder (positions near the rim,
    a NaN and an out-of-range position among the malformed ones).
  - [x] Generator version 3: split-coordinate noise (integer lattice cell + float offset, no whole
    world coordinate converted to float); large-scale variation across the disc (a placeholder
    continent/ocean layer with ranges and basins — terrain style stays prototype, §6.1); relief
    rescaled to the new vertical bounds with sea level at 0; nothing generated outside the disc
    (in every generator); spawn search unchanged apart from the new sea level; golden hashes
    regenerated, adding chunks near the rim, ~8,000 km out, and at the top and bottom of the world;
    `dwell_worldgen_inspect` accepts far coordinates.
  - [x] Air test (`AirTestFor`, `TerrainGenerator::IsAirChunk`): the generator's own sky shortcut,
    cached per chunk column; unmodified all-air chunks are neither generated nor stored, and travel
    as payload-free `Air` messages (see deviations).
  - [x] Spherical interest management (`x² + y² + z² ≤ r² + r`, unloading outside r + margin),
    clipped to the world's rows; the server's generation region and startup pre-generation skip
    air chunks.
- [x] Client meshing worker pool. *(3d: `mesh/pool.ts`, `cores − 2` workers, 1–4)*
- **Cross-platform determinism:**
  - [x] The generator compiled to WASM for local mode and the client sim; CI golden test comparing
    chunk hashes between native and WASM builds (`dwell_tests`, `dwell_worldgen_tests.js`).
  - [x] The generator in the client worldgen worker (`dwell_worldgen.wasm`; the client test
    reproduces the golden hashes). *(3b)*
- [x] Handshake carries `worldSeed` + `generatorVersion` + the verification chunk; the client's
  `WorldgenCheck` hash selects generated vs. full-chunk mode. *(3b)*
- [x] Chunk encoding: palette + RLE, with `revision` per chunk; `ChunkData` `Generated` /
  `Explicit` forms and `ChunkUnload` (§8.3). The server keeps modified chunks and evicts unmodified
  ones far from players. *(3b; zstd compresses chunks in the world file, 3e; the wire stays
  uncompressed — see the 3e deviations)*
- [x] **World persistence** (§6.4, ADR 0006): SQLite + zstd in `core/storage`; schema v1 (`meta`,
  `settings`, `chunks`, `players`, `permissions`); native VFS (WAL) and OPFS VFS in the worker;
  transactional autosave of dirty data off the tick; load on start; migrations framework.
  Local-mode worlds persist across page reloads. *(3e)*
- Debug tooling:
  - [x] Seed and generator selection (`?seed=`, `?world=`; `dwell_server --seed --generator`).
  - [x] Biome/heightmap overview: `dwell_worldgen_inspect` (ASCII map, biome shares, timings,
    spawn, vertical sections); in game, the F4 terrain map (biome and hill-shaded height around the
    player, from a worldgen worker). *(overlay: 3e)*
  - [x] "Regenerate chunk and diff" check: `dwell_world FILE diff [cx cy cz]` over a world file,
    and in game the F3 overlay's line for the player's chunk. *(3e)*
- [x] Interest management: per-client view radius; stream nearest-first; unload far chunks;
  bandwidth budget per client. *(3b)*
- [x] Greedy mesher shared in spirit by both sides:
  - [x] Server: per-chunk Jolt `MeshShape`s, rebuilt on change (built in Phase 2b, as sub-shapes
    of one terrain body with unit-quad faces; greedy merging is not used for collision because
    its T-junctions cause ghost contacts — PLAYER_CONTROLLER.md §5).
  - [x] Client: greedy mesher in a Web Worker producing render meshes; the client prediction world
    uses the same collision as the server (the sim core's C++ `TerrainCollision`, not triangles from
    the worker — see deviations). *(3d)*
- [x] Block edit loop: client `BlockEditRequest` on `control` → server validation → reliable
  `VoxelModification` broadcast → clients apply in order and re-mesh. *(3d; protocol v5)*
- [x] Revision gap detection → client requests chunk resync (`ChunkResync`). *(3d)*
- [x] **Block interaction** (§6.5) *(3d)*:
  - [x] Targeting: voxel ray cast from the eye within `REACH_DISTANCE`; outline on the targeted
    cell.
  - [x] Break (left click) and place against the targeted face (right click); touch: tap the view,
    with a Break/Place toggle button.
  - [x] Infinite creative inventory: every placeable material of the prototype set (§6.1); hotbar
    HUD; number keys, scroll wheel, or tapping a slot selects.
  - [x] Server validation (§11): reach, line of sight, cooldown, permissions, no placement into a
    player capsule, bedrock unbreakable.

Deviations and additions (3a):
- The generator lives in `server/core/{include/dwell,src}/worldgen` (the core's layout) rather than
  a `server/core/worldgen` directory.
- Procedural terrain (generator version 2) replaced the playground as the default world for
  dedicated servers and local mode; the playground stays available as version 1.
- Players spawn at a generator-chosen point (level, open land near the origin) instead of a fixed
  configured one; `ServerConfig.spawn` still overrides it.
- The stability pass works within the chunk (components touching a chunk face are kept), so it
  stays a pure function of the chunk coordinate; small pieces crossing a chunk border survive.
- Until the worker pools (3b), the client generated and meshed chunks on the main thread within a
  4 ms per-frame budget (it was two chunks per frame; procedural chunks cost ~1.5–4 ms each).
  3b moved generation to the worldgen workers; meshing stays budgeted on the main thread until 3d.
- Found along the way: WebTransport datagram writes queued behind a slow main thread, so on slow
  frames the server received inputs seconds late; datagrams now coalesce (newest per type).
- The e2e two-client test walks 2 s instead of 1 s: two pages rendering terrain on CI's software
  renderer can run below 60 ticks/s, and the test checks visibility, not speed.

Deviations and additions (3b):
- The default view is `VIEW_RADIUS_CHUNKS` = 3 and `VIEW_HEIGHT_CHUNKS` = 1 (about 110 chunks;
  3a drew 5 × 5 × 3). A radius of 5 and ±2 rows (~485 chunks) streamed fine, but CI's software
  renderer drew the one-quad-per-face meshes at 2–6 fps, starving prediction. Raise the view once
  render meshes are greedy-merged and meshed in a worker (3d).
- The wire carries palette + RLE without general-purpose compression: generated mode sends 18-byte
  `Generated` messages for untouched chunks. zstd comes with storage (3e).
- `server/core` now owns threads: the worldgen pool (`ServerConfig::worldgen_threads`; 0 in the
  browser, where local mode generates on its tick within a 4 ms budget) rather than exposing jobs
  for the host to schedule.
- The client worldgen workers run a separate, generator-only WASM build (`dwell_worldgen.wasm`,
  ~30 KB) instead of the full sim core, so each worker stays small.
- The client sim's world is streamed: missing chunks read as air, and prediction waits until the
  chunks around the player have arrived ("Loading terrain…").
- `WorldgenCheck` with hash 0 requests full-chunk mode; `?chunks=full` does that for testing.
- The e2e walks count predicted ticks instead of wall time, and wait for the terrain to finish
  loading: two pages on CI's few cores run well below 60 ticks/s otherwise.

Deviations and additions (3c):
- Terrain collision is split into region bodies (2 048 m, each at its region's centre) so sub-shape
  offsets stay exact. Dividing chunks between bodies by position bumped the capsule 5.6 cm at
  region borders, so bodies hold the chunks around the players anchored to them and a player
  collides only with its anchor's body (Jolt group filter, anchor hysteresis 8 chunks); Jolt is
  now built with RTTI to allow the filter (ADR 0011 implementation notes).
- Air chunks are sent as a payload-free `Air` form of `ChunkData` instead of not at all: the client
  waits for the chunks around the player before predicting, so it must know they exist. Neither
  side generates, stores or meshes them.
- Tests can move their world: `--dwell-origin-x=far` places every player/netcode test 7 999 488 m
  east; `ServerConfig::generator_override` lets the network simulation shift its playground.
- Found along the way: debug lines were written to the vertex buffer in absolute float32
  coordinates (0.5 m steps far out); they are now relative to their first point
  (`render/debugLines.ts`, with a test).
- The golden player trace was regenerated: double precision resolves a borderline "fits standing"
  check (a crouched player pressed exactly against a ledge) the other way from tick 320; the far
  run matches the origin within 1 µm.
- The flat and playground generators respect the rim too, so test worlds near the rim behave like
  the terrain.

Deviations and additions (3d):
- Render meshing moved entirely to TypeScript (`mesh/mesher.ts`, in workers): the sim core no
  longer builds render faces (`BuildRenderFaces` and `dwell_client_chunk_faces` removed) and instead
  hands out each chunk's voxels with a one-voxel apron (`dwell_client_chunk_padded`). The workers
  produce render meshes only: collision stays in the client sim's C++ `TerrainCollision`, the
  server's own code, which is how the prediction world collides with exactly the server's geometry.
- Greedy merging applies to full cubes and water; slabs and ladders stay one quad per face. The
  chunk shader repeats textures per block across merged quads (fract + `textureGrad` into the atlas)
  instead of switching to a texture array.
- Bedrock is not placeable (placed bedrock could never be removed); the palette is every material
  but air, liquids, bedrock and the launch pad, with the four ladders as one slot whose facing
  follows the placement.
- The edit cooldown is a token bucket (one per `BLOCK_EDIT_INTERVAL_MS` = 100 ms, bursts of 3) so
  edits bunched by the network are not rejected, and the server allows 1 m of reach beyond
  `REACH_DISTANCE` for its lagging view of the player. Permissions are an edit policy (everyone /
  ops / nobody) with ops by device key; the `permissions` table (3e) will supply them.
- The line-of-sight check samples five points on the targeted face and follows each ray 1 m past
  it: at grazing angles a ray needs a while to cross the last centimetre (found by the netcode test).
- Found along the way: creating a chunk where the air test had read open sky did not advance the
  world's epoch, so probes kept a cached pointer to the shared air chunk and did not see a block
  placed there (`block edit: probes see a block placed into a chunk they read as open sky`, red
  before the fix).
- Touch taps allow 500 ms (the software-rendered e2e page delivers the lift ~435 ms later).
- The view radius stays 3: raising it now that meshes are greedy and built in workers is left to a
  measured change (CI still renders with SwiftShader).

Deviations and additions (3e):
- The browser VFS is Dwell's own (`core/src/storage/opfs_vfs.cpp`, SQLite built with
  `SQLITE_OS_OTHER`), calling OPFS sync access handles the worker opens up front (`dwellFiles`),
  rather than SQLite's JS/WASM distribution: the storage code is the core's C++ in both builds.
  Having no shared memory, it uses exclusive locking and a rollback journal (`TRUNCATE`) instead of
  WAL; each side converts the other's file on open.
- Saved chunks are read on demand (≤ 16 per tick around players, or when streamed) from an index
  loaded at startup, and modified chunks are evicted once saved, so memory stays bounded as
  edited worlds grow.
- Local mode saves every 5 s (not `AUTOSAVE_SECONDS`) and when the page is hidden or closed: a tab
  can go away at any time. One world file per generator and seed; a second tab on the same world
  runs without persistence (sync access handles are exclusive).
- Settings and permissions come from launch options saved into the world (`--name`, `--motd`,
  `--max-players`, `--edits`, `--op`, `--ban`); in-game admin commands stay in Phase 8. Bans and an
  allow-list (`allow_list` setting) are enforced at join. `permissions` records who granted an
  entry (`granted_by`).
- The wire stays without zstd: Explicit chunks are rare in generated mode, and QUIC/SCTP framing
  already bounds the cost; storage uses it.
- `dwell_server` now saves to `world.dwellworld` in its working directory by default (`--world ""`
  for an in-memory world); CI's smoke run and the e2e server use their own.
- SQLite comes from sqlite.org's amalgamation (3.53.4). This sandbox could not reach sqlite.org, so
  local builds here used the same version's amalgamation as bundled by the better-sqlite3 npm package
  (via `FETCHCONTENT_SOURCE_DIR_SQLITE3`);
  CI downloads the official archive.
- Found along the way: the e2e test `local mode: the predicted player walks forward` could walk off
  a 1 m terrain ledge (its walk overshoots the 60 counted ticks by however long releasing the key
  takes); it now walks on the flat world.
- Backups, export/import UI, `bodies` and `lod_sections` remain for later phases (§6.4).

Exit criteria
- [x] *(3c)* With double-precision Jolt, the controller scenarios, golden trace and netcode tests
  pass natively and in WASM both at the origin and ~8,000 km from it, and 64 players still take
  < 1 ms per tick (native Release and WASM). *`dwell_tests` and `dwell_player_tests.js` with and
  without `--dwell-origin-x=far` (CI runs both); golden trace within 1 µm at 8,000 km; 0.42 ms per
  tick at the origin, 0.48 ms far out (native Release); `divergence.mjs` native↔WASM 1.2e-7 m/s,
  no state mismatches.*
- [x] *(3c)* Terrain near the rim has the same detail as near the origin: an automated check finds
  no float quantization in noise sampled at 1 m steps there. *`noise is as detailed ~8,000 km from
  the origin as at it` (also shows the old float-coordinate approach failing there).*
- [x] *(3c)* Walking off the rim of the disc falls into the void and kills the player. *`world
  rim: walking off the edge of the disc falls into the void and kills` (fails with the rim
  disabled).*
- [x] *(3c)* Open sky costs nothing: all-air chunks are neither generated nor stored, travel
  without payload, and the per-client chunk set stays bounded with 256 rows. *`streaming: open sky
  costs nothing; the view is a sphere across the world's rows`, `chunks the air test reports as
  air generate as all air`, and the existing unload/memory test.*
- [ ] Walking across the world streams chunks without hitches; memory stays bounded when moving.
  *Automated for the server (`streaming_test.cpp`: chunks are generated ahead of a moving player,
  never on the tick; world, collision and per-client chunk sets stay bounded) and the client
  (`chunkStream.test.ts`: unloads drop chunks and meshes; e2e: the view streams in and the player
  walks). Outstanding: a playtest walking a long distance.*
- [x] *(3d)* A player can break and place every placeable block type (desktop and touch), picking
  it from the hotbar; invalid edits (out of reach, into a player, bedrock) are rejected. *e2e
  `edit.spec.ts` (all 14 slots, by number key and hotbar click) and `touch.spec.ts` (hotbar tap,
  Break/Place toggle, taps on the view); `block_edit_test.cpp` (reach, line of sight, bedrock,
  occupied cells, players, the world's bounds, rate, edit policy).*
- [x] *(3d)* A block placed/removed by one client appears for all clients, and the player collides
  with it immediately after the update on both server and client. *e2e `an edit by one client
  appears for another on a native server`; `netcode: block edits` (a wall placed by one player
  stops another on the server and in its prediction, at 100 ms RTT, at the origin and ~8,000 km
  out); `collides with a block right after the edit arrives` (WASM client sim).*
- [x] Chunk serialization round-trips byte-for-byte between C++ and TS (golden tests:
  `chunk_data_*` in `shared/protocol/vectors.txt`, from the Python reference encoder).
- [x] *(3e)* A world edited on a native server and one edited in local mode both survive
  restarts/reloads; a crash mid-save leaves the previous save intact; a world file saved natively
  opens in the browser build and vice versa. *`persistence: an edited world survives a server
  restart` (native and WASM); e2e `local mode: an edited world is saved in the browser and survives
  a reload` (OPFS in Chromium); `storage: a crash at any point of a save leaves the previous save or
  the new one` (a VFS dropping every write after the Nth, each N: 231 crash points natively over
  WAL, 84 in WASM over the rollback journal); `storage: world files written natively and in WASM
  open in both builds` (golden files in `server/tests/storage/golden`).*
- [x] The same seed produces bit-identical chunks natively, in local mode, and in the client
  worker (CI golden test: `dwell_tests`, `dwell_worldgen_tests.js`, and
  `worldgen/generator.test.ts`); untouched chunks cost only a `Generated` message on the wire
  (`streaming_test.cpp`). Re-verified for generator version 3 (3c), including chunks near the rim
  and ~8,000 km out.
- [ ] Generated terrain shows varied relief (prototype style, §6.1), caves, and overhangs, and the
  player can walk, jump, and swim through it with no collision mismatches.

---

## Phase 4 — World LOD & Whole-World View

**Status:** in progress — every deliverable built (4a grid and generation, 4b propagation and
streaming, protocol v6, 4c the client's LOD system, rendering and creative flight — protocol v7),
in PR #14 (merged); playtest follow-ups (fog off, super tall mountains) in PR #15; a fix for
z-fighting on distant water in #21; height fog with a settings menu pending review.
Outstanding: the frame-rate part of 4c's second exit criterion, which needs a desktop GPU and a
phone (this sandbox renders with SwiftShader). Added 2026-09-29 with [ADR 0012](./adr/0012-lod-octree.md) (concepts from
the Distant Horizons mod, adapted to 3D). Sub-phases: **4a — LOD data and generation**; **4b —
propagation and streaming**; **4c — rendering and the dev camera** (built as creative flight, see
deviations). Depends on Phase 3c (planet
scale) and 3d (meshing worker pool); 4b's propagation cache lands in the database from 3e.

**Goal:** Everything that should be visible from the camera is visible, at a detail that drops
with distance — from the ground, a mountain top, or a dev camera high enough to see the whole
8,192 km disc — including every player's builds (§6.6).

Deliverables
- **LOD data and generation** *(4a)*:
  - [x] LOD grid and coordinates (`(L, i, j, k)` from the corner (−2²³, `WORLD_MIN_Y`, −2²³);
    levels 0–`LOD_MAX_LEVEL`), shared C++/TS; section content as 32³ materials plus a one-cell
    apron, encoded with the chunk palette + RLE codec.
  - [x] `GenerateLod(seed, generatorVersion, L, i, j, k)` in `server/core/worldgen`: generator
    evaluated at cell centres, octaves and features smaller than a cell dropped; all-air and
    buried sections skipped from column height bounds. Built into `dwell_worldgen.wasm`.
  - [x] `Downsample` of 8 children (≥ 4 of 8 solid → solid; else ≥ 4 liquid → liquid; most common
    material, ties to the upper cells), with unit tests on crafted layouts (one-voxel walls and
    floors survive a level; pillars thinner than a cell do not).
  - [x] Golden hashes of `GenerateLod` sections at several levels (surface, mountains, ocean, the
    rim, the root), checked natively and under WASM in CI.
- **Propagation and streaming** *(4b)*:
  - [x] Server propagation: chunk changes (every `VoxelModification` source) mark level-1 sections
    dirty; a budgeted off-tick job (`LOD_PROPAGATION_SECTIONS_PER_TICK`) downsamples dirty
    sections nearest to players first up to the root, assigning `lodRevision`s; the
    `lod_sections` cache table and its migration (§6.4); rebuilt on a generator version change.
  - [x] The LOD index at `LOD_INDEX_LEVEL`: `LodIndex` after `WorldgenCheck`, coalesced
    `LodIndexUpdate` broadcasts.
  - [x] `LodRequest` / `LodData` (`Generated` | `Explicit` | `Unchanged`); the `lod` stream on
    WebTransport, WebRTC (data channel 3) and loopback; `LOD_BYTES_PER_SECOND` budget and
    `LOD_REQUESTS_PER_SECOND` limit; full-chunk mode answers `Explicit`. Golden vectors for every
    new message in C++, TS and the Python reference encoder.
- **Rendering and the dev camera** *(4c)*:
  - [x] `lod/`: octree walk around the camera by screen-space error (`LOD_PIXEL_ERROR`, lower
    quality on mobile), parent-until-all-children-ready swaps with level-0 nodes backed by the
    streamed chunks, coarsest-then-nearest job scheduling to the worldgen and meshing pools,
    bounded cache (`LOD_CACHE_MB`).
  - [x] LOD section meshing in the meshing worker pool: greedy-merged, flat colour per material,
    border faces culled only against same-level neighbours.
  - [x] Two-pass depth split at `LOD_NEAR_SPLIT_M` (far LOD pass, depth clear, near pass).
  - [x] ~~`devcam/`: free-fly dev camera~~ → creative flight for the player's body (see
    deviations): toggled by double-tapping Space / Jump or the touch Fly button, speed scaled with
    altitude, able to rise until the whole disc is in view; server-authoritative and predicted,
    with a `--flight everyone|ops|nobody` policy (protocol v7).
  - [x] Debug: F3 overlay shows LOD node counts per level, pending jobs, cache use and LOD bytes/s;
    optional per-level colouring of LOD sections.

Deviations and additions (4a):
- **Where a cell samples.** A cell samples the pipeline at its centre column and *bottom voxel*,
  not its centre: under the ≥ 4-of-8 rule a floor keeps a cell solid exactly when the cell's
  bottom voxel is solid, so this is what makes generated and downsampled sections agree (and a
  centre sample would drop terrain that fills less than half of the cells above level ~12, where
  cells are taller than the world's relief).
- **Downsample material.** The most common material among the top filled cell of each of the
  block's four columns (ties to the upper cells), not among all qualifying cells: with the
  latter, rock beneath the surface outvotes it on uneven ground and coarse levels turn grey.
- **What `GenerateLod` drops:** fractal octaves finer than a cell (fBm still normalised by the
  full amplitude, ridged sums by the kept one so ranges do not sink), tunnels outside 2 m cells,
  caves deeper than three cells below the surface, trees above 4 m cells and boulders above 2 m,
  ores and the stability pass. The apron below the world reads as bedrock (the floor is never
  drawn).
- **Bounds.** `LodBoundsAt(L, i, k)` classifies a whole column of sections from its 2D fields
  (`Empty` above, `Buried` below), so the client needs one cheap job per column rather than one
  per section to skip sky and rock; `GenerateLod` uses the same bounds.
- **Content layout.** Section content is stored in layer order (the mesher's padded layout), so
  the palette + RLE codec takes cells as they are (`EncodeLodCells`, 34³ cells, palettes up to
  65 535); the chunk codec now shares the implementation.
- The flat and playground generators share a flat `GenerateLod` (the playground's features are
  below a cell), which equals the downsample of their chunks exactly.
- Section content round-trips the codec in C++ (`lod: encoding`) and TypeScript
  (`chunkVoxels.test.ts`); golden *wire* vectors come with the `LodData` message in 4b.

Deviations and additions (4b):
- **Modified sections keep generated octants.** A modified section is `GenerateLod` of itself with
  only the octants of *modified* children replaced by their downsample, rather than the downsample
  of all 8 children with unmodified ones generated at the level below: one generation instead of
  up to eight per write, and a build changes only the octants above it (less popping when a
  section turns from generated to modified). The two agree within 4a's tolerance.
- **Apron.** Stored sections keep their generated apron; the server fills it from modified
  same-level neighbours' borders when sending (`CellsForClient`).
- **`Generated` means "nothing modified yet".** A section dirty but not yet written answers
  `Generated`; the index update after its level-8 ancestor is written (propagation goes bottom-up)
  makes the client ask again. So a client may treat a `Generated` answer as covering the whole
  subtree.
- **Wire details:** `LodIndex` has a flags byte (1 = last message) and may be empty; index messages
  carry at most 16 384 entries (`limits.maxLodIndexEntries`, under SCTP's 256 KiB); `LodRequest`
  levels are 1–19. Type ids: `LodIndex` 0x13, `LodIndexUpdate` 0x14, `LodData` 0x15, `LodRequest`
  0x4C; the `lod` channel is id 2 (its WebTransport stream's first byte; WebRTC data channel 3).
- **Threads.** Propagation (and full-chunk-mode generation) runs on one thread when the server has
  worldgen threads, else on the tick within `lod_budget_us` = 2 ms (the browser's local mode).
- **Storage format 2** adds `lod_sections` with a generator version column; empty blobs bind as
  empty, not NULL (found by the persistence test).
- Requests beyond the bucket or a 256-deep queue are dropped (counted in `LodStats`); the client
  retries what goes unanswered (4c).

Deviations and additions (4c):
- **`LOD_PIXEL_ERROR` 4 px desktop / 8 px mobile** (was 2 / 4): at 2 px a view from the ground
  draws ~8,200 sections (one or more draw calls each), at 4 px ~2,800; the level table in §6.6
  moves accordingly (level L from ~176 × 2^L m).
- **Job order by projected cell size** instead of "coarsest first, then nearest" globally: with
  the latter the camera's own ground stayed at 1 km cells (and the chunks under the player hidden)
  until every coarse section to the horizon was done — seen in the browser. Parents still come
  before children and coarse before fine.
- **Near sections refine off-frustum:** a coarse section beside or behind the camera has its
  surface rounded up to its cells and showed as a wall at the edge of the view; nodes closer than
  their own size refine regardless of the frustum.
- **Chunks as level 0:** a level-1 section refines into its chunks when all 8 are drawable; other
  chunks are hidden while LOD is active (all show until the root is ready). The refine test
  includes the chunk regions in its coverage check.
- **Requests:** `Generated` covers a subtree, re-asks go top down after an index change, level ≥ 8
  children are re-asked only when indexed; pacing 60/s with a 5 s retry.
- **Skirts** are separate meshes per side (only non-empty ones are created), shown per frame.
- **Renderer:** the scene lost its background colour (three.js clears with it in every `render()`,
  wiping the far pass — found in the browser); the far pass's near plane follows altitude; fog
  scales with altitude. Then, from playtest feedback, fog is off for now (after briefly reaching
  512 km): the whole world is drawn without haze (`fog.test.ts` failed while fog was on); later
  replaced by height fog (below).
- **Coarse surfaces:** a column's top cell takes its surface's material, not the bedrock a cell
  taller than the relief samples at its bottom (found in the browser: the disc was grey from
  orbit; `lod: surface` failed before the fix, and the LOD golden hashes were regenerated).
- **Liquids count as filled** in `Downsample` (≥ 4 of 8 non-air, material from the columns' top
  filled cells) and a coarse `GenerateLod` cell over the sea is water: with solids and liquids
  counted apart, oceans showed their floor from afar (playtest feedback; `lod: sea` failed before
  the change). From level 3 liquids meshed opaque (since replaced, below). LOD
  golden hashes regenerated.
- **Creative flight replaces the dev camera** (playtest feedback: the "dev camera" was meant as a
  creative flying mode for the player). A new exclusive controller layer (PLAYER_CONTROLLER.md
  §6.7) driven by a held `fly` input bit: no gravity, move along the view's yaw, jump up / crouch
  down, speed `11 m/s × (run ? 2.5) × (1 + height above sea / 32 m)`, capped at 400 m/s below
  `WORLD_MAX_Y`, feet stopping at `FLIGHT_CEILING` (24,000 km). Server policy `--flight`
  (default everyone) clears the bit for others and is told to the client in `Welcome` (u8 flags);
  `pos64` decoders accept ±`POS64_LIMIT` (33,554 km) — protocol v7. The player body's Jolt velocity
  limit is raised for it. The client keeps predicting while flying even where streamed terrain has
  not arrived (it deadlocked otherwise: found in the browser, `gate.test.ts` failed before the
  fix). `devcam/`, F8 and `?devcam=1` are gone; the LOD camera is the eye.
- **Super tall mountains** (playtest request): generator version 4 adds massifs in the cores of
  the largest ranges, crests rising a further 3,600 m (peaks ~5.3–5.6 km, ~1 % of land above
  3 km), so the whole-world view has something tall to see. Prototype relief (§6.1); version 3 is
  retired like version 2. `worldgen: terrain` "super tall peaks" failed on version 3 (highest
  ~1,900 m); the golden chunk and LOD sets gained a massif and were regenerated.
- **Chunks first on slow devices** (phone playtest: nothing drawn, though collision and block
  outlines worked): chunks around the player showed only once every LOD level above them was
  generated. Drawable chunks now wait at most `FORCE_CHUNKS_AFTER_MS` (1 s); after that the
  walk descends to them regardless, leaving unready coarse siblings empty until ready.
  `lodSystem.test.ts` "shows the streamed chunks … not ready" failed before the fix.
- **No popping when turning** (phone playtest): out-of-view sections were kept coarse, so detail
  popped in wherever the view swept. Refinement now depends on distance alone; the view only
  orders the work (out of view ranks 8× lower). Settled, ~65% more sections are loaded (all
  around rather than in view); desktop's cache reached ~85 MB after 75 s in the browser (256 MB
  budget; mobile's 96 MB is to be checked on a phone). "turning around shows the detail already
  loaded" failed before the change.
- **LOD colours match the chunks** (phone playtest: distant land paler): LOD vertex colours were
  sRGB bytes used as linear, while the chunks' sRGB texture is decoded before lighting. They are
  now linear, and a tile's average is taken in linear light. `lodMesher.test.ts` "writes linear
  vertex colours" failed before the fix.
- **Distant water as a tinted floor** (playtest): compared side by side with opaque water blocks
  (via a temporary `?lodwater=tint` switch), the tinted floor was chosen: coarse liquids (level 3
  up) are left out and the floor under them is recoloured as seen through the near water. The
  opaque mode and the switch are gone. `lodSystem.test.ts` "draws coarse water as the floor under
  it, tinted" failed while opaque was the default. *Since replaced (next item).*
- **Distant water see-through at every level** (playtest: a seam and a height step where near
  water met LOD water, a brighter band at the transition, darker tinted seas, a hard edge where
  the surface stopped at level 3): every level now draws the see-through surface over the floor
  at its true depth, 1/8 m below the cell grid as the chunks draw it, at the chunks' opacity and
  from both sides; the tint mode is gone. Compared with an improved tint (lit like the water's
  surface) for cost: per-section water meshes added ~25% draw calls in a coastal view (triangles
  +3–8%, meshing time equal), so all LOD water is one `BatchedMesh` — draw calls within a few
  percent of the tint's. `lodMesher.test.ts` "draws a sea floor inside a water cell under a water
  surface at the chunks' water height" and `lodSystem.test.ts` "draws water at every level as
  see-through, at the chunks' water height" failed before the change.
- **Cracks at section borders** (playtest: sky-blue gaps along lines in the terrain, since
  column surfaces): a step between surfaces across a section border was drawn only as a skirt,
  which is hidden when the neighbour section is at the same level. Border steps are now opaque
  walls of the section that owns them (skirts keep only the part the apron hides). In the browser,
  four downward views from 400 m (seed 5) showed 234–368 sky-coloured pixels before, 0 after;
  `lodMesher.test.ts` "closes steps between surfaces across the section border without skirts"
  failed before the fix.
- **Z-fighting on distant water** (playtest, since column surfaces): a sea floor inside a water
  cell, rounded to half cells, could land on the cell's top — level with the water surface, 1/8 m
  from it — or on its bottom, in the plane of the solid cell's own top below (drawn too, in
  another colour). The floor now stays at most half a cell up the top water cell, and the covered
  top below it is not drawn. `lodMesher.test.ts` "never draws a sea floor in the water surface or
  over the cell below" failed before the fix.
- **Snapshots dropped while flying (and swimming)** (phone playtest: terrain never finished
  loading after fast flight): `PlayerFlagsOf` resolved `kClimbing`/`kSwimming`/`kFlying` to the
  *ControllerFlags* constants of the same names, so a flying player's snapshot carried an
  unknown player-flag bit and every decoder dropped it; the client predicted on alone and the
  server streamed around its own, lagging copy. Swimming had the same defect since Phase 2 (and
  climbing showed as swimming to others). "player flags use the PlayerFlags bits" and "fast
  creative flight keeps snapshots coming" failed before the fix.
- **Loading status in a corner, and phone overlays** (playtest): "Loading terrain…" moved from the
  centre of the view to a bottom-left status (`hudText.test.ts` failed before); on touch screens
  the connection status and F3 overlay stack below the hotbar instead of under it
  (`touch.spec.ts` layout test failed before); an ⓘ button top right toggles the debug overlay
  on touch screens, which have no F3 (its e2e test failed before).
- **Replaced sessions lost their Reject** (CI flake, seen twice): a client write racing the
  server's close failed first and dropped the pending `Reject(Replaced)`; the WebTransport client
  now reads the control stream to its end before reporting the close (`webTransport.test.ts`
  failed before; the e2e passed 15/15 after).
- **Distant terrain at its true height** (playtest: the horizon, oceans included, looked too
  tall, with a solid edge): cells fill from their bottom voxel, so cell tops lifted land by up to
  a cell (+220 m at level 8, +2 km at 12) and seas to +2,048 / +6,144 m at levels 12 / 13.
  `GenerateLod` now also returns each column's exact surface, and the client draws column tops at
  it in half-cell steps (1.2–2× the triangles; exact per-column tops measured 10–70×). `lod:
  column surfaces` (unbiased within a few metres at every level) and the `lodMesher.test.ts`
  surface tests failed before. Cells, server, protocol and golden hashes are unchanged.
- **Height fog and a settings menu** (playtest request: a light atmospheric haze that fades out
  higher up, so the whole map shows from the flight ceiling): fog was linear and scaled with
  height, then off. It is now an exponential atmosphere's optical depth along each view ray,
  capped at a maximum density (§6.6), with distance, density and height sliders (distance up to
  the world's diameter) in a new ☰ settings menu, top left; the connection status and the F4 map
  moved clear of the button. `fog.test.ts` (horizon hazed, clearer with altitude, the whole disc
  clear from the ceiling) failed against the fog-off code; `settingsMenu.test.ts` covers the
  log-scaled sliders. Checked in Chromium (SwiftShader): no shader errors, haze on the horizon,
  a clear disc from the ceiling, sliders applied live and kept after a reload.
- Debug hooks: `window.__dwell.fly(on)`; `?lod=0` disables LOD, `?lodcolors=1` tints sections by
  level.

Exit criteria
- [x] *(4a)* `GenerateLod` is bit-identical natively and in WASM (CI golden test), and a section
  generated at level L agrees with the downsample of generated level-0 chunks within a stated
  tolerance on crafted and sampled terrain. *`lod: golden` (`lod-hashes.txt`, 11 sections from
  level 1 to the root, natively and in `dwell_worldgen_tests.js`) and `worldgen module (WASM)`
  in the client; crafted: the flat world equals its downsample exactly at levels 1–3; sampled:
  at the spawn and a site of each biome (levels 1–2, 3 in the mountains) ≥ 95% of cells agree in
  class and ≥ 95% of column surfaces are within one cell, mean difference under half a cell
  (measured: ≥ 96.2%, ≥ 97.7%, 0.2 cells).*
- [x] *(4b)* A block placed by one client changes the LOD sections above it on the server within
  a bounded time, and another client far away receives the change (index update → request →
  `Explicit`) without re-downloading unchanged sections; a client with no modifications in view
  receives no `LodData` beyond the index. *Server side verified: `lod: streaming` — an edit reaches
  the root within 19 ticks and a client 50 km away gets the index update 8 ticks after it; its
  re-requests come back `Explicit` on the changed path, `Generated` beside it, and `Unchanged` for
  a held section a second edit did not touch; a client 60 km the other way receives only the index
  and its updates. Client side: `lodSystem.test.ts` — no requests while the index has no
  modification in view; under an entry, requests go top down and only to the modified path and
  its children (`Generated` covers the rest); an index change re-asks with the held revision and
  nothing more.*
- [x] *(4c)* From the ground, the view reaches the horizon with no holes: automated check that the
  selected node set covers the view frustum at every frame while moving, and that swaps never
  leave a region without a drawn node. *`lodSystem.test.ts` "covers the view with no holes or
  overlaps": walking, turning and rising to 2,000 km with jobs finishing in random order, every
  frame's sampled view points lie in exactly one drawn, empty, buried or chunk-refined section,
  and every drawn section has a mesh.*
- [ ] *(4c)* Flying up (was: with the dev camera), the whole disc becomes visible within 30 s of reaching altitude
  on a desktop build, and the frame rate stays above 60 fps (desktop) / 30 fps (mobile) with
  memory within `LOD_CACHE_MB`. *Verified: e2e `lod.spec.ts` (local mode, Chromium with
  SwiftShader) — the player flies to the 24,000 km ceiling (~65 s under SwiftShader, where the
  sim runs slower than real time; ~27 s by the formula) and the disc is covered by drawn sections
  on arrival (well within the 30 s), with the cache under
  `LOD_CACHE_MB`. Outstanding: the frame rates, on a desktop GPU and a phone.*
- [x] *(4c)* A structure built by another player is visible in LOD from 50 km away and from
  altitude. *`lod: builds from afar`: a 512 m × 512 m wall (1 m thick) is solid in the sections
  drawn from 50 km (level 8) and 100 km up (level 9) — a wall or floor survives every level once
  it spans a cell in two dimensions, so what a build needs to be seen from distance d is ~d / 90 m
  across (4 px at 1080p); `lod: streaming` carries it to another client.*

---

## Phase 5 — Voxel Awakening (Integrity + Flood-Fill → CompoundShapes)

**Goal:** Spec Phase 4. Detached structures become single Jolt bodies (§7.1).

Deliverables
- [ ] Anchor definition (bedrock layer + `grounded` flag) and budgeted 6-connected flood-fill
  structural-integrity pass triggered by voxel removal (`INTEGRITY_BUDGET_VOXELS`).
- [ ] Clustering of detached components; removal from grid in the same `VoxelModification`
  (reason `Collapse`).
- [ ] Cluster → Jolt `StaticCompoundShape` of boxes; mass/COM/inertia from material density.
- [ ] `NetworkEntityID` allocation; reliable `EntitySpawn` (voxel layout) / `EntityDespawn`.
- [ ] Tier 1 snapshot replication for all clusters (tiers are introduced in Phase 6); client
  interpolation and rendering of cluster meshes; kinematic proxies in client physics worlds.
- [ ] **Player ↔ Tier 1 interaction** (§9.2, §9.4):
  - [ ] Players push light clusters (contact mass scaling: `maxPushForce`, `pushableMassLimit`);
    moving clusters push players (absorbed as external velocity).
  - [ ] Standing on / riding moving clusters: `groundEntityId` + body-local player state in
    snapshots; client predicts in the body's frame.
  - [ ] Present-time proxies for Tier 1 bodies within `PREDICT_PROXY_RADIUS`; the ground body is
    rendered at present time.
  - [ ] Crush damage / crush death (contact listener, `PLAYER_CONTROLLER.md` §6.6).
  - [ ] Port of the PPC platform suite onto Tier 1 bodies (translating, rotating, falling).
- [ ] Admin/debug command: cut a pillar / delete a region to trigger collapses on demand.

Exit criteria
- [ ] Removing the supports of a tower causes it to fall as one body, on all clients, in sync.
- [ ] A 10 000-voxel detached structure is processed without the server tick exceeding its budget
  (work is spread across ticks).
- [ ] Unit tests for integrity/clustering on crafted voxel layouts (bridges, overhangs, rings).
- [ ] A player can ride a falling slab to the ground at 150 ms RTT without sliding off or
  jittering; a player under a falling tower is crushed on every client consistently.
- [ ] Collapsing generated terrain (a cave ceiling, an overhang) works like built structures.

---

## Phase 6 — Tiered Physics (Authoritative Tier 1, Cosmetic Tier 2)

**Goal:** Spec Phase 5. Keep CPU and bandwidth bounded during large explosions (§7.2).

Deliverables
- [ ] Explosion system on the server: radius/force, material strength attenuation, impulse to
  existing Tier 1 bodies, newly awakened clusters, and **players** (knockback + damage via
  `PlayerEvent`, replayed into prediction).
- [ ] Tier 2 debris collides one-way with the local player (debris bounces off; player movement
  unaffected).
- [ ] Tier classification: `TIER1_MIN_BLOCKS`, `alwaysAuthoritative` materials.
- [ ] Tier 2 path: voxels removed in `VoxelModification` (reason `Explosion`) + `PhysicsEvent`
  `Explosion(origin, force, radius)` in the same reliable batch; client derives debris from the
  removed voxels and simulates in its local debris world.
- [ ] Snapshot packing: quantization (§8.3), priority accumulator, multiple datagrams per tick,
  per-client bandwidth budget.
- [ ] Client debris lifecycle: lifetime, at-rest removal, `DEBRIS_MAX_BODIES` cap (platform-based).
- [ ] **Threading checkpoint (ADR 0007):** profile a browser-hosted friend world at its host
  profile's caps; if simulation-bound, plan a hosting-only threaded web build behind
  `coi-serviceworker`.
- [ ] Cosmetic death ragdoll (Jolt `Ragdoll`, Tier 2 rules) in the client debris world; player
  models animated from `State` (both moved from Phase 2).
- [ ] Performance instrumentation: server tick time, active body count, bytes/sec per client;
  client frame time and debris count, shown in a debug overlay.

Exit criteria
- [ ] A large explosion (hundreds of fragments) runs at stable server tick rate; only Tier 1 bodies
  appear in snapshots; per-client bandwidth stays within budget.
- [ ] Debris looks plausible on each client and never affects gameplay state or player movement.
- [ ] Players caught in a blast are knocked back smoothly and consistently across clients.
- [ ] Late-joining clients see correct terrain (debris is not replayed, by design).

---

## Phase 7 — Sleep / Re-bake Cycle

**Goal:** Spec Phase 6. Long-running servers keep a bounded number of dynamic bodies (§7.3).

Deliverables
- [ ] Sleep monitor with `SLEEP_LINEAR_THRESHOLD`, `SLEEP_ANGULAR_THRESHOLD`, `SLEEP_SECONDS`.
- [ ] Grid snapping to 24 axis-aligned orientations; occupied-cell conflict resolution with
  fallback to Tier 2 debris.
- [ ] Player-safe re-bake: cells overlapping players are treated as occupied; riders transition
  from body-relative to static ground without a visible pop.
- [ ] Re-bake: destroy body → write grid → rebuild chunk collision → integrity check on placed
  voxels → reliable `VoxelModification(Rebake)` + `EntityDespawn` in one batch.
- [ ] `MAX_TIER1_BODIES` enforcement (force re-bake of oldest/smallest).
- [ ] Persist in-flight Tier 1 bodies in the `bodies` table (ADR 0006) so a world saved mid-collapse
  resumes it on load.
- [ ] Soak test harness: headless bots + scripted explosions for hours; tracks body count, memory,
  tick time.

Exit criteria
- [ ] After a collapse settles, every cluster is back in the static grid within
  `SLEEP_SECONDS` + a small margin, and clients show identical static terrain.
- [ ] A multi-hour soak test shows flat memory and body count and no tick overruns.

---

## Phase 8 — Player Hosting, Master Server & Platform Packaging

**Goal:** Player-hosted multiplayer with no official game servers (ADR 0003): distributable
dedicated servers, friend worlds hostable from any client, a master server for discovery, and
packaged desktop/mobile apps (ARCHITECTURE §10).

Deliverables
- [ ] **Dedicated server distribution:** CI builds native binaries (Windows/macOS/Linux) and a
  Docker image per release; settings and permissions in the world database edited via admin
  commands and a server CLI (ADR 0006); ops, kick, ban by key; allow-list/password;
  UPnP/NAT-PMP with port-forward guidance; rotating SQLite online backups; host-configurable
  physics and view caps. Certificate rotation with hash publication.
- [ ] **Master server** (`services/master`; hostname and platform → Open Decision
  #10): registration + heartbeat, reachability-verified public listing (method per #10), join codes, cert-hash
  distribution, rate limiting per key/IP.
- [ ] **World export/import** (`.dwellworld`) across dedicated servers, browsers, and apps; Capacitor
  storage VFS verified per platform.
- [ ] **Server browser** in the client: listing, search/filter, client-side ping, status query,
  incompatible-version marking; versioned client builds at `/dwell/v/<version>/`.
- [ ] **Friend worlds:** WebRTC transport (§8.1) in the client; hosting the integrated server over
  WebRTC; signaling via the master; STUN + TURN relay (Open Decision #11) with short-lived credentials; host profiles (player and physics caps);
  host-backgrounded pause.
- [ ] **Electron:** packaging for Windows/macOS/Linux (electron-builder), custom protocol with
  COOP/COEP and the multithreaded sim-core build (ADR 0007),
  "Host world" launching the native server; LAN discovery.
- [ ] **Capacitor:** Android and iOS projects; check `SharedArrayBuffer` availability on the app
  scheme (threaded build if available, ADR 0007); verify WebTransport per WebView (WebRTC
  where unavailable); touch
  controls (auto-jump preset); mobile caps; friend-world hosting with backgrounding handling.
- [ ] Dedicated-server WebRTC joins through master signaling and TURN (servers behind strict NAT).

Exit criteria
- [ ] A player hosts a dedicated server at home from the downloadable binary; players on the GitHub
  Pages site, Electron, and a phone find it in the server browser and see the same collapse.
- [ ] A phone hosts a friend world; a browser player and an Electron player join by code, including
  one on mobile data through the TURN relay.
- [ ] Certificate rotation on a dedicated server is invisible to players joining through the master.
- [ ] An iOS Safari player joins a self-signed dedicated server over WebRTC, including one behind
  strict NAT via TURN.
- [ ] An outdated client is rejected with a clear message and offered the matching versioned build.

---

## Cross-Cutting Work (every phase)

- Materials and terrain style are prototype placeholders (ARCHITECTURE §6.1): phases build and
  test mechanisms with them; none of them commits to a block set or world look.

- Keep `docs/ARCHITECTURE.md` current (required by `CLAUDE.md`).
- Record significant choices as ADRs in `docs/adr/`.
- Every new message type gets golden-byte tests in both TS and C++.
- CI must stay green; the Pages deployment must stay playable in local mode.
