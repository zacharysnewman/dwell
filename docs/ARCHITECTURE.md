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
| Client | TypeScript, Jolt Physics via WebAssembly |
| Client shells | Browser (GitHub Pages) → Electron (desktop) → Capacitor (iOS/Android) |
| Transport | WebTransport (HTTP/3 / QUIC); WebSocket fallback |
| Simulation | 60 Hz internal physics step, 20 Hz network snapshots |
| First deployment target | **GitHub Pages** (static client) |

```
                    ┌───────────────────────────────────────────────┐
                    │           Authoritative Server (C++)          │
                    │                                               │
  inputs (dgram) ──▶│ Input validation ─▶ Player sim (Jolt Char.)   │
                    │                                               │
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
│                     Client (TS + Jolt WASM)                          │
│  Input → Local prediction (Jolt CharacterVirtual) → Reconciliation   │
│  Chunk store → Mesher (worker) → Renderer                            │
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
   → no `SharedArrayBuffer`. The web build therefore uses the **single-threaded** Jolt WASM
   build. (A `coi-serviceworker` shim may enable cross-origin isolation later; tracked as an
   open question.) Electron/Capacitor builds can opt into the multithreaded Jolt build.
3. **Project-site base path.** Assets are built with Vite `base: '/dwell/'`.
4. **Local mode.** So the Pages deployment is playable with no hosted server, the server's
   simulation core is also compiled to WASM (Emscripten) and run in a Web Worker, connected
   through an in-memory `LoopbackTransport` that implements the same interface as the network
   transports. Same code, same protocol, no network.

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
  /core              Simulation core: voxel grid, integrity, clustering, physics, replication.
                     Platform-free; compiled natively AND to WASM (local mode).
  /net               WebTransport (+ WebSocket) server front-end. Native only.
/shared/protocol     Protocol spec, constants, and golden-byte test vectors used by both sides.
/platforms/electron  Electron shell.
/platforms/capacitor Capacitor shell.
/docs                ARCHITECTURE.md, IMPLEMENTATION_PLAN.md, ADRs.
```

---

## 4. Server (Authoritative) **[planned]**

### 4.1 Responsibilities
- Validate player input (rate, magnitude, reach for block edits, anti-teleport).
- Own the **master voxel grid** — the single source of truth for terrain.
- Compute **structural integrity** and run flood-fill clustering on detached voxels.
- Simulate all **Tier 1** dynamic bodies and all player characters.
- Replicate state: snapshots (unreliable) and voxel deltas / events (reliable).
- Run the **sleep / re-bake** cycle to keep active body count bounded.

### 4.2 Main loop

```
every 16.67 ms (60 Hz):
    drain & validate inputs (per player, ordered by input sequence)
    apply queued voxel edits → integrity → clustering → awaken bodies
    step players (Jolt CharacterVirtual)
    physicsSystem.Update(1/60)
    sleep monitor → re-bake queue → apply re-bakes
every 3rd step (20 Hz):
    build per-client snapshot (interest-managed, prioritized) → send datagrams
    flush reliable queues (voxel deltas, events) in order
```

The loop uses a fixed timestep with an accumulator; the server never steps with a variable dt.

### 4.3 Simulation core vs. network front-end
`server/core` has no sockets, threads, or OS calls; it consumes decoded messages and emits
encoded messages through an interface. This lets the identical core run natively and in the
browser's local mode (§2.1). `server/net` owns QUIC/HTTP3 sessions, stream management, and
the WebSocket fallback. The WebTransport library choice (e.g. libwebtransport/quiche/msquic-based)
is an open decision recorded as an ADR when made.

---

## 5. Client **[planned]**

| Module | Responsibility |
|---|---|
| `net/` | `Transport` interface; `WebTransportTransport`, `WebSocketTransport`, `LoopbackTransport`. Framing, encode/decode. |
| `world/` | Chunk store mirrored from server; applies voxel deltas in order. |
| `mesh/` | Greedy mesher in a Web Worker; produces render meshes and collision triangles. |
| `render/` | Scene, camera, chunk meshes, dynamic body meshes. Renderer library is an open decision (Three.js proposed). |
| `physics/` | Jolt WASM worlds: prediction world (local player + static terrain) and debris world. |
| `predict/` | Input sampling, local prediction, server reconciliation & replay. |
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
| Stream `world` | Reliable, uni S→C | Chunk data, `VoxelModification`, `PhysicsEvent`, entity spawn/despawn |

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
  u16  buttons                  // bitfield: fwd/back/left/right/jump/crouch/sprint/...
  i16  yaw, i16 pitch           // quantized view angles
```

**Server → Client: `PhysicsSnapshot` (datagram)**
```
u8   type = 0x81
u32  serverTick
u32  ackInputSeq                // last input processed for this client
[local player state: pos f32×3, vel f16×3, flags u8]
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

**Server → Client: `ChunkData` (reliable, `world`)** — `ChunkCoord`, `revision`, palette, RLE runs.

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

## 9. Player Movement: Prediction & Reconciliation **[planned]**

- Both sides simulate players with Jolt `CharacterVirtual` using identical parameters and
  the same fixed 60 Hz step.
- Client samples input every step, tags it with `inputSeq`, applies it locally, stores it in a
  ring buffer, and sends it (with the previous 3 inputs for loss resilience).
- Server applies inputs in sequence order, one per step; missing inputs repeat the last known
  input. Snapshots carry `ackInputSeq` and the authoritative player state.
- On snapshot: client rewinds its player to the authoritative state, replays unacknowledged
  inputs, and smooths any visible correction over a few frames. Errors above a threshold snap.
- Remote players are rendered with the same interpolation path as Tier 1 bodies.

Bit-exact determinism between native Jolt and Jolt WASM is **not** assumed; reconciliation
absorbs small divergence.

---

## 10. Security & Validation

- Server is the only authority on voxels, Tier 1 bodies, and player positions.
- Inputs are rate-limited (≤ `SIM_HZ` per second plus redundancy) and range-checked.
- Block edits are checked for reach, line of sight, cooldown, and permissions.
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
