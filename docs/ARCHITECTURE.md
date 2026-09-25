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
| Authoritative server | C++20, Jolt Physics (native) |
| Client | TypeScript; physics/prediction via the shared C++ sim core (Jolt linked in) compiled to WebAssembly |
| Client shells | Browser (GitHub Pages) → Electron (desktop) → Capacitor (iOS/Android) |
| Transport | WebTransport (HTTP/3 / QUIC); WebSocket fallback |
| Simulation | 60 Hz internal physics step, 20 Hz network snapshots |
| Players | Dynamic-body, velocity-layer controller ported from the Physics Player Controller, client-predicted (§9, [`PLAYER_CONTROLLER.md`](./PLAYER_CONTROLLER.md)) |
| Terrain | Seeded deterministic procedural generation, same C++ code on server and client (§6.3) |
| First deployment target | **GitHub Pages** (static client) |

```
                    ┌───────────────────────────────────────────────┐
                    │           Authoritative Server (C++)          │
                    │                                               │
  inputs (dgram) ──▶│ Input validation ─▶ Player sim (Jolt Char.)   │
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
                                    │  WebTransport │
                                    │  (or WS)      ▼
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

### 2.1 GitHub Pages (first target) **[planned]**

GitHub Pages only serves static files. Consequences that shape the architecture:

1. **The authoritative server cannot run on GitHub Pages.** The static client is served
   from Pages; the server is hosted elsewhere (see 2.3). The client locates a server via
   a build-time default plus a `?server=` URL override.
2. **No custom HTTP headers** → no `Cross-Origin-Opener-Policy` / `Cross-Origin-Embedder-Policy`
   → no `SharedArrayBuffer`. The web build of the sim core (and the Jolt inside it) is therefore
   compiled **single-threaded** (no pthreads). A `coi-serviceworker` shim may enable
   cross-origin isolation later (open decision #4). Electron/Capacitor builds can opt into a
   multithreaded build.
3. **Project-site base path.** Assets are built with Vite `base: '/dwell/'`.
4. **Local mode.** So the Pages deployment is playable with no hosted server, the server's
   simulation core is also compiled to WASM (Emscripten) and run in a Web Worker, connected
   through an in-memory `LoopbackTransport` that implements the same interface as the network
   transports. Same code, same protocol, no network.
5. **One WASM build, three uses.** The same Emscripten build of `server/core` provides local mode,
   the client's prediction/debris physics, and the client terrain generator, so every piece of
   simulation logic (player controller, worldgen, physics setup) has exactly one implementation.

Deployment is automated by a GitHub Actions workflow that builds `client/` and publishes it
with `actions/deploy-pages` on pushes to the default branch.

### 2.2 Desktop / Mobile shells **[planned]**

The same Vite build output is wrapped by:
- **Electron** (`platforms/electron`) — Chromium, full WebTransport support.
- **Capacitor** (`platforms/capacitor`) — Android System WebView (Chromium) and iOS
  WKWebView. WKWebView WebTransport support must be verified per iOS version; the
  WebSocket fallback exists primarily for this case.

### 2.3 Server hosting **[planned]**

- Local development: server runs on `localhost` with a self-signed ECDSA certificate
  (≤14-day validity) passed to the browser via WebTransport `serverCertificateHashes`.
- Hosted: any VM/host that allows inbound **UDP** (QUIC) and TCP (WebSocket fallback),
  with a publicly trusted TLS certificate (required because the Pages origin is HTTPS).
  The hosting provider is an open decision.

---

## 3. Repository Layout **[planned]**

```
/client              TypeScript client (Vite). Renderer, prediction, interpolation, debris.
/server              C++20 authoritative server (CMake). Jolt via FetchContent.
  /core              Simulation core: voxel grid, worldgen, integrity, clustering, physics,
                     players, replication.
                     Platform-free; compiled natively AND to WASM (local mode + client physics).
    /player          Physics player controller (PLAYER_CONTROLLER.md).
  /net               WebTransport (+ WebSocket) server front-end. Native only.
/shared/protocol     Protocol spec, constants, and golden-byte test vectors used by both sides.
/platforms/electron  Electron shell.
/platforms/capacitor Capacitor shell.
/docs                ARCHITECTURE.md, PLAYER_CONTROLLER.md, IMPLEMENTATION_PLAN.md, ADRs.
```

---

## 4. Server (Authoritative) **[planned]**

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

### 4.3 Simulation core vs. network front-end
`server/core` has no sockets, OS calls, or threads of its own (parallel work such as worldgen is
exposed as jobs the host schedules); it consumes decoded messages and emits encoded messages
through an interface. This lets the identical core run natively, in the browser's local mode, and
as the client's prediction/debris physics (§2.1). `server/net` owns QUIC/HTTP3 sessions, stream management, and
the WebSocket fallback. The WebTransport library choice (e.g. libwebtransport/quiche/msquic-based)
is an open decision recorded as an ADR when made.

---

## 5. Client **[planned]**

| Module | Responsibility |
|---|---|
| `net/` | `Transport` interface; `WebTransportTransport`, `WebSocketTransport`, `LoopbackTransport`. Framing, encode/decode. |
| `world/` | Chunk store mirrored from server; applies voxel deltas in order. |
| `worldgen/` | Runs the server's C++ terrain generator (WASM) in a Web Worker for `Generated` chunks. |
| `mesh/` | Greedy mesher in a Web Worker; produces render meshes and collision triangles. |
| `render/` | Scene, camera, chunk meshes, dynamic body meshes. Renderer library is an open decision (Three.js proposed). |
| `physics/` | Hosts the sim-core WASM module (C++ `server/core` + Jolt, Emscripten): prediction world (local player + terrain + kinematic proxies) and debris world. The client does not use separate Jolt JS bindings. |
| `predict/` | Input sampling, local prediction, server reconciliation & replay, ground-relative frames, present-time proxies. |
| `interp/` | Snapshot buffer, Tier 1 transform interpolation (and bounded extrapolation). |
| `debris/` | Tier 2 cosmetic debris spawn, simulation, and cleanup. |

The client never mutates the voxel grid on its own authority. Block edits are sent as
requests; the visual change is applied when the server's reliable delta arrives (an optional
optimistic "ghost" may be shown meanwhile).

---

## 6. Voxel World

### 6.1 Grid & chunks
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
- The server keeps only **modified** chunks in memory/storage as full chunks (or as deltas
  against generated output); unmodified chunks are regenerated on demand and evicted freely.
- Handshake sends `worldSeed` and `generatorVersion`. The client generates a verification
  chunk and reports its hash; on mismatch (or on low-power devices by choice) the client uses
  **full-chunk mode** and the server sends every chunk explicitly.
- `ChunkData` has two forms: `Generated(coord, revision)` — "generate this yourself, no
  changes" — and `Explicit(coord, revision, palette+RLE)` for modified chunks. This cuts
  terrain bandwidth for unexplored or untouched areas to a few bytes per chunk.
- Server generation runs on a worker thread pool with a per-tick budget; the spawn region is
  pre-generated at startup. Client generation runs in a Web Worker off the main thread.

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
| `INTERP_DELAY_MS` | 100 | Tier 1 interpolation delay (2 snapshots) |
| `MAX_TIER1_BODIES` | 512 | Server cap; oldest/smallest force-re-baked when exceeded |
| `INTEGRITY_BUDGET_VOXELS` | 32 768 / tick | Flood-fill budget per tick |
| `CHUNK_SIZE` | 32 | Voxels per chunk edge |
| **Players (§9)** | | |
| Controller tuning | see `PLAYER_CONTROLLER.md` §7 | Capsule, speeds, jump, crouch, climb, swim, push, damage |
| `RECONCILE_SNAP_DISTANCE` | 1.0 m | Correction above this snaps instead of smoothing |
| `PREDICT_PROXY_RADIUS` | 16 m | Tier 1 bodies within this use present-time proxies |
| `RESPAWN_SECONDS` | 5 s | Death → respawn delay |
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
Implementations: WebTransport (primary), WebSocket (fallback — datagrams are sent over the
reliable socket; higher latency under loss but functionally identical), Loopback (local mode).

### 8.2 Channels

| Channel | Kind | Content |
|---|---|---|
| Datagrams | Unreliable | Player input (C→S), physics snapshots (S→C) |
| Stream `control` | Reliable, bidi | Handshake, ping/clock sync, chat, block edit requests |
| Stream `world` | Reliable, uni S→C | Chunk data, `VoxelModification`, `PhysicsEvent`, `PlayerEvent`, entity spawn/despawn |

All world-affecting reliable messages go on **one** ordered stream so a voxel removal and the
event/entity that depends on it can never be reordered. Bulk chunk streaming may move to
separate uni streams later if head-of-line blocking is measured to matter.

`MAX_DATAGRAM_BYTES` = 1200 (safe QUIC payload). All multi-byte fields little-endian.

### 8.3 Message formats (v0 — to be finalized in Phase 1)

**Client → Server: `PlayerInput` (datagram)**
```
u8   type = 0x01
u32  lastReceivedSnapshotTick
u8   count                      // redundancy: last N inputs (N ≤ 4)
repeat count:
  u32  inputSeq
  i8   moveX, moveY             // analog move vector (keyboard: -127/0/127), |move| <= 1
  u16  buttons                  // bitfield: jump/crouch/run/use/...
  i16  yaw, i16 pitch           // quantized view angles
```

**Server → Client: `PhysicsSnapshot` (datagram)**
```
u8   type = 0x81
u32  serverTick
u32  ackInputSeq                // last input processed for this client
local player state:
  u32  groundEntityId           // 0 = static terrain / airborne; else pos/vel are body-local
  f32×3 position, f16×3 velocity
  u8   flags                    // grounded | crouched | climbing | swimming | dead
  u8   health
  controller state (~48 B)      // layers, jump timers, crouch/climb/swim, ground ref —
                                // exact layout in PLAYER_CONTROLLER.md §8.4
u8   remotePlayerCount
repeat remotePlayerCount:
  u16  playerId
  u32  groundEntityId
  f32×3 position, f16×3 velocity
  i16  yaw, i16 pitch
  u8   state                    // player::State (animation)
  u8   flags
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

**Server → Client: `Handshake` (reliable, `control`)** — protocol version, `playerId`,
`worldSeed` (u64), `generatorVersion` (u32), server tick. Client replies with the hash of a
generated verification chunk to select generated vs. full-chunk mode (§6.3).

**Server → Client: `ChunkData` (reliable, `world`)**
```
u8   type = 0x11
u8   form                       // Generated = 0 (client generates; no payload) | Explicit = 1
ChunkCoord, u32 revision
[Explicit only: palette + RLE runs]
```

**Server → Client: `PlayerEvent` (reliable, `world`)**
```
u8   type = 0x30
u8   kind                       // Knockback | Damage | Death | Respawn
u16  playerId
u32  serverTick                 // tick the effect was applied (for predicted replay)
f32×3 impulse                   // Knockback only
u8   amount                     // Damage only
u8   cause                      // Fall | Crush | Explosion | ...
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

## 9. Players: Physics-Based Characters **[planned]**

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

### 9.3 Prediction & reconciliation
- Fixed 60 Hz step on both sides. The client runs the controller pipeline for the local player
  against a small prediction world (terrain, kinematic proxies, one dynamic body), records
  `{inputSeq, input, controller state, body state}` per tick, and sends inputs with the previous 3
  for loss resilience.
- The server applies inputs in sequence order, one per step; a missing input repeats the last
  known one. Snapshots carry `ackInputSeq`, the authoritative body state, and the local player's
  controller state (layers, timers, crouch/climb/swim, ground ref).
- On snapshot: if the stored prediction for `ackInputSeq` matches within tolerance, nothing is
  replayed. Otherwise the client restores the server state, replays unacked inputs (pipeline +
  prediction-world step), and smooths the visible correction; errors above
  `RECONCILE_SNAP_DISTANCE` snap.
- **Server-originated impulses** (knockback, explosion) arrive as a reliable `PlayerEvent` with the
  tick they were applied; the client inserts them into history at that tick and replays.
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

### 9.5 Remote players
Remote players are interpolated like Tier 1 bodies (in the ground body's frame when
`groundEntityId` ≠ 0), exist in the client's physics worlds as kinematic capsules, and are
animated from `State` and flags in snapshots.

### 9.6 Health, death & respawn
- Health is server-authoritative; damage sources are fall, crush, explosion.
- On death the client spawns a **cosmetic ragdoll** (Jolt `Ragdoll`, Tier 2 rules — local,
  unsynchronized, despawned on respawn); the server respawns the player after `RESPAWN_SECONDS`.

### 9.7 Anti-cheat implications
Clients only send inputs, never positions, so speed/teleport/fly hacks are structurally
impossible; the server's simulation is the only source of player state.

---

## 10. Security & Validation

- Server is the only authority on voxels, Tier 1 bodies, and player positions.
- Inputs are rate-limited (≤ `SIM_HZ` per second plus redundancy) and range-checked.
- Block edits are checked for reach, line of sight, cooldown, and permissions; placements that
  overlap any player capsule are rejected.
- Message decoders bounds-check every length field; malformed messages drop the connection.

---

## 11. Open Decisions

Record each resolution as an ADR in `docs/adr/` and update the relevant section above.

| # | Decision | Current leaning |
|---|---|---|
| 1 | Server WebTransport/QUIC library | Evaluate C++ options in Phase 1 |
| 2 | Client renderer | Three.js (WebGL2) |
| 3 | Server hosting provider (must allow UDP) | TBD before public multiplayer |
| 4 | Cross-origin isolation on Pages (`coi-serviceworker`) for multithreaded Jolt | Defer; single-threaded first |
| 5 | Persistence of the world across server restarts | Out of scope for Phases 1–6 |
| 6 | Final values for §7.4 tunables | Tune in Phases 4–6 |
| 7 | Worlds larger than ±65 km (Jolt `JPH_DOUBLE_PRECISION`) | Not needed initially |
| 8 | Worldgen noise numerics: fixed-point vs. strict IEEE float | Prototype both in Phase 3; pick by golden-test stability and speed |
| 9 | Movement feel on voxels: PPC recommended feel (walk 5 / run 8 m/s) vs. slower voxel-genre speeds | Start with PPC feel; playtest in Phase 2 |
