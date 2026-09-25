# Dwell — Physics Player Controller Spec

> Part of the architecture documentation (see [`ARCHITECTURE.md`](./ARCHITECTURE.md) §9).
> Keep this file current under the same rule as `ARCHITECTURE.md` (see `CLAUDE.md`).
>
> Status: **[built]** (Phase 2) — the controller, voxel queries, terrain collision, networking with
> prediction and reconciliation, and presentation (`server/core/include/dwell/player`,
> `server/core/src/player`, `server/tests/player`, `client/src/game`). Not yet: animation (no
> character models), and the Tier 1 interactions of §6.6 (push cap, crush, riding clusters —
> Phase 4).

This spec ports the **Physics Player Controller (PPC)** —
[`zacharysnewman/physics-player-controller`](https://github.com/zacharysnewman/physics-player-controller),
specifically its deterministic Photon Quantum 3 port in `Quantum~/` — into Dwell's architecture:
C++20, Jolt Physics, a server-authoritative simulation with client prediction, and a voxel world.

The PPC's **behaviour, layer model, system order, tuning, and bug fixes are kept**. What changes is
the *host*: Quantum ECS systems become ordered C++ passes in `server/core/player`, Quantum physics
becomes Jolt, `FP` fixed-point becomes `float`, Quantum rollback becomes server reconciliation, and
generic colliders/triggers become voxel-grid queries and voxel materials.

---

## 1. Model

The player is a **dynamic Jolt rigid body** (capsule) driven by **velocity layers**:

1. Each tick, independent layers compute velocity contributions — *horizontal* (walk/run,
   external pushes), *vertical* (gravity, jump, ground following, launches), and optional
   *exclusive* layers (climb, swim) that replace the others while active.
2. The aggregate pass sums them into a **target velocity** and writes it directly to the body.
3. The Jolt step resolves contacts. Anything that moved the body away from its target — a
   collision, a falling cluster, an explosion — is detected on the next tick as the difference
   between the body's actual velocity and last tick's contribution, **absorbed** into an
   *external velocity* term, and **decays** (friction on the ground, drag in the air).

That absorption loop is what makes the character *physically reactive* — it is shoved by falling
structures, thrown by explosions, carried by moving clusters, and pushes loose blocks — while the
layers keep the movement tight and responsive.

This supersedes the earlier `CharacterVirtual` design. A kinematic/virtual character cannot be
pushed by the solver; the PPC model gets that for free and the Quantum port has already fixed the
classic dynamic-controller problems (hovering, slope launches, stair hops, double jumps).

---

## 2. Mapping: PPC (Quantum) → Dwell

| PPC Quantum | Dwell |
|---|---|
| `PPCCharacter` component (DSL) | `struct PlayerController` — plain data, trivially copyable (for rewind/replay and snapshots) |
| `PPCConfig` asset (`FP` fields) | `struct PlayerControllerConfig` — `float` fields, defined in `shared/protocol` and identical on server and client |
| `SystemMainThreadFilter` systems in `PPCSystemGroup` | Ordered free functions `player::step*(PlayerCtx&)` called by `player::tick()` |
| `PhysicsBody3D` dynamic, `RotationFreeze.FreezeAll`, `GravityScale 0`, no sleeping | Jolt `EMotionType::Dynamic`, `mAllowedDOFs = EAllowedDOFs::TranslationX\|Y\|Z`, `mGravityFactor = 0`, `mAllowSleeping = false`, `mLinearDamping = 0` |
| Frictionless `PhysicsMaterial` (created on demand) | Body `mFriction = 0`, `mRestitution = 0` |
| `Shape3D.CreateCapsule(radius, extent)` | `JPH::CapsuleShape(halfHeightOfCylinder, radius)` — same convention (`extent` = half cylinder height) |
| Crouch shape swap + `ResetCenterOfMass`/`ResetInertia` | `BodyInterface::SetShape(id, crouchCapsule, updateMassProperties = true, EActivation::Activate)`; both capsules are prebuilt |
| `f.Physics3D.RaycastAll` / `OverlapShape` | `VoxelQuery` (grid DDA / cell overlap) for terrain **plus** Jolt `NarrowPhaseQuery::CastRay` / `CollideShape` for dynamic layers (§5) |
| `PPCLadder` trigger entities | **Climbable voxel materials** with facing variants (§6.3) |
| `EntityRef` ground | `GroundRef { kind: None \| Terrain \| Tier1Body \| Player; id }` |
| `FP` seconds for timers | **Integer ticks** (config seconds rounded to ticks at load) |
| `f.DeltaTime` | Fixed `1 / SIM_HZ` (60 Hz) |
| `PPCInputBridge` partial hook | `PlayerInput` decoded from the `PlayerInput` datagram (§8) |
| `PPCForces.AddVelocity` / `AddExplosion` | `player::addVelocity()`, and the server explosion system (ARCHITECTURE §7.2) calling it for players |
| Quantum events (`PPCJumped`, `PPCLanded`, …) | `PlayerEvents` bitset + payload per tick; drives fall damage (server) and audio/animation (client) |
| Quantum rollback (full-world determinism) | Server authority + local-player prediction and reconciliation (§8) |
| Unity's left-handed axes | Right-handed, Y up (Jolt, Three.js): camera-right at yaw 0 is −X (§9) |
| `PPCCameraView`, `PPCAnimatorView`, `PPCDebugView` | Client `render/` camera rig, animation driver, debug overlay (§9) |

**One implementation.** The controller lives only in C++ (`server/core/player`). The client runs
the same code through the WASM build of `server/core` (the build already needed for local mode),
so prediction and authority never drift because of two implementations. See ARCHITECTURE §5.

---

## 3. State

```cpp
namespace dwell::player {

enum class State : uint8_t { Idle, Walking, Running, Crouching, Sliding, Jumping, Falling, Climbing, Swimming };

struct Input {                    // one tick; buttons are held state, edges are detected in-sim
  float move_x, move_y;           // x = right, y = forward, camera-relative, |move| <= 1
  float look_yaw, look_pitch;     // degrees; yaw 0 = +Z, 90 = +X; pitch positive = up
  bool  jump, run, crouch;
};

struct GroundRef { enum Kind : uint8_t { None, Terrain, Tier1Body, Player } kind; uint32_t id; };  // id: Jolt BodyID

struct GroundInfo {
  bool  grounded, was_grounded;   // THE grounded flag every pass uses (PPC semantics)
  Vec3  normal;  float slope_angle, gap;
  GroundRef ground;
  bool  ceiling_blocked, touching_wall;  Vec3 wall_normal;
};

struct HorizontalLayer { Vec3 current, external, contribution; };
struct VerticalLayer   { float accumulated_y, platform_y, last_platform_y, target_y; uint8_t step_grace; };
struct JumpState       { uint16_t buffer_ticks, coyote_ticks; bool jumping, jumped_this_tick; };
struct CrouchState     { bool crouching; };
struct ClimbState      { bool climbing; Cell ladder; Vec3 velocity; bool has_released; Cell released; };
struct SwimState       { bool swimming; float submerged; Vec3 velocity; };            // Dwell addition
struct PlatformState   { GroundRef ground; Vec3 ground_velocity, base_velocity; float yaw_delta; };

struct PlayerController {        // static_assert(std::is_trivially_copyable_v<PlayerController>)
  Input input, previous_input;
  State state;
  GroundInfo ground;
  HorizontalLayer horizontal;  VerticalLayer vertical;
  JumpState jump;  CrouchState crouch;  ClimbState climb;  SwimState swim;
  PlatformState platform;
  Vec3 target_velocity;
  uint32_t events;                // Events:: bits raised this tick (Jumped, Landed, CrouchChanged,
  float landed_speed;             //   ClimbStarted/Ended, SwimStarted/Ended); Landed's impact speed
};

}
```

`PlatformState` has no previous position/rotation: the PPC needed them for platforms moved by
script, but Jolt moves kinematic bodies by their velocity, so every carrying body is read with
`Body::GetPointVelocity` (§6.6). `Players` (`controller.h`) owns the players of one physics world:
`Spawn`, `SetInput`, `Tick` (the §4 pipeline for every player), `AddVelocity`, `AddExplosion`, and
state access (`controller`, `Restore`, `Teleport`) for snapshots and reconciliation.

The Jolt `BodyID` and the prebuilt standing/crouched capsule shapes are held beside this struct,
not inside it, so `PlayerController` stays a copyable value for history buffers.

---

## 4. Per-tick pipeline

The server main loop (ARCHITECTURE §4.2) runs, for every player, **before** `PhysicsSystem::Update`:

| # | Pass | PPC origin | Job |
|---|---|---|---|
| 1 | `stepInput` | `PPCInputSystem` | `previousInput = input`; take this tick's input; clamp `|move| <= 1` |
| 2 | `stepProbe` | `PPCProbeSystem` | Ground ring (16 + centre), ceiling ring, 4 axis wall rays via `VoxelQuery` + dynamic bodies (§5) |
| 3 | `stepPlatform` | `PPCPlatformSystem` | Velocity of the ground under the player (`v + ω × r` for Tier 1 bodies; zero for terrain; zero for players unless `carriedByCharacters`) |
| 4 | `stepCrouch` | `PPCCrouchSystem` | Hold-to-crouch; shape swap; feet planted when grounded, head kept in the air; stand-up overlap test |
| 5 | `stepJump` | `PPCJumpSystem` | Buffer + coyote (in ticks); `accumulatedY = max(accumulatedY, jumpV + platformY)`; no coyote after a real jump |
| 6 | `stepClimb` | `PPCClimbSystem` | Exclusive layer on climbable voxels (§6.3) |
| 7 | `stepSwim` | — (Dwell) | Exclusive layer in water voxels (§6.4) |
| 8 | `stepHorizontal` | `PPCMovementLayerSystem` | Absorb external; accel/decel/reverse in the platform's frame; air control; step-up |
| 9 | `stepVertical` | `PPCVerticalLayerSystem` | Gravity; walk-off keeps platform Y; ceiling cancel; launch detection; ground following + snap |
| 10 | `stepAggregate` | `PPCAggregateSystem` | `target = exclusive ? layer.velocity : horizontal.contribution + (0, vertical.targetY, 0)`; `BodyInterface::SetLinearVelocity` |
| 11 | `stepState` | `PPCStateSystem` | Priority: Climbing > Swimming > Crouching > Jumping/Falling > Running/Walking/Idle |
| — | *Jolt `PhysicsSystem::Update(1/60)`* | Quantum physics step | Contacts resolved; deviations are absorbed next tick |

Each pass's behaviour is exactly the PPC Quantum system's, including its fixes over the Unity
package (listed in the PPC `Quantum~/CHANGELOG.md`): single shared `grounded` flag that turns off
while rising faster than the ground; stair "stay on ground" probe reach; ground following with
one-tick snap (no hovering, no slope launches); capsule always centred on the body during crouch;
no double jump from coyote time; ladder face snap, jump-off launch, look-down threshold. As in
Quantum, the passes run pass-major (each pass for every player, then the next pass). Before the
passes, `Tick` makes sure terrain collision exists around every player and syncs edited chunks
(§5).

**Dwell deviations** (each covered by a test in `server/tests/player`):
- **Blocked motion is not absorbed.** The PPC absorbs *any* difference between the body's velocity
  and last tick's contribution, including the velocity a wall removes while the player walks into
  it. That leaves a phantom push-back that never decays in the air (air drag 0), so a player
  holding forward against a block could not jump onto it — the core voxel move. The horizontal
  layer therefore removes, per contact normal recorded by a Jolt `ContactListener` during the last
  step, the part of the deviation that only cancels the player's own push into that contact
  (`min(removed, into)` along the normal). Pushes by moving bodies are still absorbed.
- **Step-up nudge.** After lifting onto a step, the capsule also moves forward by
  `radius + stepProbeDistance − ringRadius + 1 cm`, so the probe ring (inside the slimmer voxel
  capsule) is over the step and the ground snap doesn't pull the player back down.
- **Step grace.** For 6 ticks after a step-up, only an upward deviation above 1.5 m/s counts as a
  launch: Jolt's speculative contact on the step's convex edge nudges the capsule up as it crosses.
- **Step probe height.** The step ray starts at `max(centre, feet + maxStepHeight + 5 cm)`, so a
  crouched player (centre below a slab's top) can still step onto slabs.

---

## 5. Voxel queries (probes)

The PPC probes are generic physics ray casts. Dwell answers them from the **voxel grid** for static
terrain and from **Jolt** only for things that move:

```
struct ProbeHit { float distance; Vec3 point, normal; GroundRef ground; };

bool castSolid(origin, dir, maxDist, layers, self, ProbeHit&):
    a = voxelDDA(grid, origin, dir, maxDist)            // Amanatides–Woo walk over solid cells
    b = jolt.CastRay(origin, dir*maxDist, layers ∩ {TIER1, PLAYER}, ignore = self)
    return nearest(a, b)

bool overlapsSolid(center, capsule, self):              // crouch stand-up, climb, swim
    cellsIn(capsule.aabb).any(solid && boxIntersectsCapsule)
      || jolt.CollideShape(capsule, layers ∩ {TIER1, PLAYER}, ignore = self)
```

Why:
- **Exact and cheap.** A DDA over a few cells is far cheaper than ~40 mesh ray casts per player
  per tick (the PPC's main cost: 16 characters ≈ 3.3 ms/tick in Quantum). It scales to many players
  on one server.
- **No mesh-seam artifacts.** Grid normals are exact axis-aligned face normals; there are no
  triangle-edge normals at chunk or greedy-mesh seams.
- **Same answer on server and client**, since both hold the same chunk data.

**Built** (`VoxelQuery`, `voxel_query.h`):
- `CastVoxels` walks cells Amanatides–Woo style and intersects the ray with each cell's *shape box*
  (full cube, or the bottom half for slabs). A hit is the ray *entering* a shape from free space;
  a ray that starts inside solid terrain only hits after leaving it (like a surface mesh).
- `CastBodies` is a Jolt `NarrowPhaseQuery::CastRay` restricted to the `Tier1` and `Character`
  layers, ignoring the player's own body. `CastRay` returns the nearer of the two. A ring of probes
  first asks the broad phase once (`BodiesNear`) and skips Jolt when no moving body is in reach —
  the common case — which keeps 64 players' passes at ~0.36 ms/tick (Release, CI-class machine).
- `OverlapsSolid` tests the vertical capsule against solid cell shapes exactly (segment–box
  distance) and against moving bodies with `CollideShape`. `SubmergedFraction` samples the centre
  column. Voxel lookups cache the last chunk.

**Terrain collision** (`core/terrain_collision.h`) is **one static body** whose
`MutableCompoundShape` holds one `MeshShape` per chunk (added on demand around players, replaced
in place on edit). Chunk meshes emit every exposed face as its own unit quad on the grid — not
greedy-merged — and adjacent slabs hide their shared faces. Both choices remove ghost contacts:
Jolt's enhanced internal edge removal voids edges by shared vertex positions *within one body
pair*, so seams between separate chunk bodies, or T-junctions from merged faces, would bump the
capsule (verified by the seam test: no loss of ground crossing chunk borders in 8 directions).

Rules that keep the grid and the physics world consistent:
- A voxel edit rebuilds the affected chunk's collision `MeshShape` **in the same tick** on the
  server: `TerrainCollision::Sync` (run at the start of every `Players::Tick`) rebuilds chunks
  whose own or neighbours' revisions changed.
- The player body enables Jolt's `mEnhancedInternalEdgeRemoval` so the capsule slides over mesh
  and chunk seams without catching on internal edges.
- Non-cube voxel shapes extend the DDA with a per-material sub-cell shape box; slabs
  (`VoxelShape::kSlabBottom`) are built, and the probe interface does not change.

---

## 6. Voxel-specific behaviour

### 6.1 Dimensions tuned to the 1 m grid
The PPC default capsule (radius 0.5, height 2.0) is exactly one block wide and cannot pass
through a 1-wide, 2-tall doorway. Dwell's defaults (§7): **radius 0.3, height 1.8** (fits 1×2
openings), **crouch height 0.9** (fits 1-tall crawlspaces with skin to spare).

### 6.2 Blocks, steps, slopes
- Static terrain is all axis-aligned: its ground normals are always straight up and walls exactly
  vertical, so slope logic rarely triggers on terrain. It still matters on **Tier 1 bodies**
  (rotated clusters) and on future slab/stair shapes.
- `maxStepHeight` 0.55 m (PPC: 0.45): half-block **slabs** (0.5 m) are stepped up without leaving
  the ground, full 1 m blocks need a **jump** (jump height 1.25 m clears one block); step-up also
  applies to cluster debris. Walking off a 1 m ledge is a short fall (larger than the step reach),
  so there is no stair snapping on full blocks.
- **Auto-jump** (Dwell addition, config `autoJump`, default on for touch input): when grounded and
  not crouched, moving into a 1-block obstacle (top between `maxStepHeight` and 1.05 m above the
  feet, probed 0.2 m beyond the capsule) with 2 free cells above it triggers a jump. It runs
  in-sim, so it is predicted like any other input.
- **Edge guard** (Dwell addition, optional, off by default): while crouched and grounded, the
  horizontal layer zeroes, per axis (X, Z), movement after which no ray of the ground ring would
  find ground within `maxStepHeight` (classic sneak-at-edges).

### 6.3 Ladders as voxels
- PPC ladders are trigger colliders with a `PPCLadder` component. In Dwell a ladder is a
  **climbable material** in the material table (`climbable = true`) with **facing variants**
  (`ladder_n/e/s/w`), so no voxel format change is needed.
- Detection: any climbable cell the capsule overlaps (exact capsule–box test). The ladder is a
  thin plate (0.1 m) on the cell's back face; the ladder frame (facing axis, plate depth) comes from
  the cell and its facing variant, replacing the trigger box's transform and extents in
  `ClimbVelocity`/`AwayFromLadder`. The face snap therefore holds the capsule against the wall the
  ladder is mounted on.
- A contiguous ladder column counts as one ladder for `released` purposes: a ladder you let go of is
  not re-grabbed until the capsule has left every cell of that column.
- **Over the top** (voxel adaptation): a column's climbable region reaches 0.6 m above its top
  cell, and within 0.3 m of the top, climbing up also moves towards `−facing` (without the face
  snap), so the player gets onto the ledge the ladder leans on. (PPC ladder triggers overhang
  their ledge instead; a voxel ladder cell cannot.)
- Vines and scaffolding use the same flag with different speeds (`climbSpeedScale` per material).

### 6.4 Water (Dwell addition)
- `submerged` = fraction of the capsule height inside water cells (centre column). Above
  `swim.enterFraction` the exclusive **swim layer** takes over: move input in the look direction
  (pitch included), jump = ascend, crouch = descend; the body's velocity is pulled towards that
  wish velocity with exponential `drag`, plus buoyancy `buoyancy × (submerged − floatFraction)`, so
  a player at rest floats with 70 % submerged (eyes above water). Below `swim.exitFraction` it hands
  back to the normal layers (hysteresis) with the current velocity; holding jump at that moment
  jumps out (onto a bank). The layer holds the other layers exactly like the climb layer does, so
  leaving the water is not read as an external force. Water 1 m deep (≤ 0.55 of the capsule) is
  walked through.

### 6.5 Terrain changing under or into the player
- **Block removed under the feet:** the next probe finds no ground → walk-off rules apply (keep
  platform Y, start falling). No special case.
- **Block placement into a player:** the server rejects any placement whose cell overlaps a
  player capsule (inset by a small skin). This also covers placement into *other* players.
- **Re-bake** (ARCHITECTURE §7.3): cells overlapping players count as occupied; a player standing on
  the re-baked body changes its ground from `Tier1Body` to `Terrain` in the same tick.
- **Collapse onto a player:** handled by physics (shoved, knocked down) and by crush detection (§6.6).

### 6.6 Interaction with Tier 1 bodies and other players
- **Being pushed / hit:** natural solver response; absorbed as external velocity and decayed.
- **Riding:** `stepPlatform` uses the Tier 1 body's linear + angular velocity at the player's
  position; the horizontal layer accelerates in that frame, and the vertical layer follows its
  vertical velocity, so standing on a falling slab or tipping tower needs no input. `yawDelta` turns
  the camera with rotating bodies.
- **Push strength.** The aggregate pass sets velocity directly, which by itself would let an 80 kg
  player shove a 20-tonne cluster. A `ContactListener` caps this: in `OnContactAdded` /
  `OnContactPersisted` between a player and a Tier 1 body, `ContactSettings::mInvMassScale` for the
  cluster is scaled so the effective push force never exceeds `maxPushForce`. Clusters heavier than
  `pushableMassLimit` are treated as immovable by players (scale 0). The player is still fully
  pushed *by* the cluster.
- **Crush:** from the same contact listener. A player with contacts of roughly opposing normals
  (dot < −0.7) where one is a Tier 1 body approaching faster than `crushSpeed`, sustained for
  `crushTicks`, is crushed (damage/death, ARCHITECTURE §9.6).
- **Other players:** both are dynamic bodies. Standing on a head is grounded but not carried unless
  `carriedByCharacters` (PPC default: off).
- **Gravity mismatch (intended):** players use the PPC's snappier `gravity` (20 m/s², Source feel);
  Tier 1 bodies use world gravity (9.81 m/s²). A player standing on a free-falling slab falls faster
  than it, so they stay on it instead of floating off.

---

## 7. Configuration

`PlayerControllerConfig` lives in `server/core/include/dwell/player/config.h` — both sides run the
same C++ (the client through the WASM core), so it needs no TypeScript mirror. Presets:
`DefaultConfig()` (desktop), `TouchConfig()` (auto-jump), `UnityParityConfig()`. Durations are
stored as ticks. Defaults follow the PPC's **recommended feel** (Source
engine movement, checked against Halo 3); rows marked ◆ differ from the PPC default for voxels.

| Section | Field | Dwell default | PPC default | Notes |
|---|---|---|---|---|
| Body | `radius` ◆ | 0.3 m | 0.5 | Fits 1-wide gaps |
| | `standingHeight` ◆ | 1.8 m | 2.0 | Fits 2-tall doorways |
| | `mass` ◆ | 80 kg | 1 | Real mass: voxel clusters use real densities |
| | `gravity` | 20 m/s² | ×2 of 10 | Absolute value instead of a scale |
| | `eyeHeight` ◆ | 1.62 m / 0.8 m crouched | (view) | Client camera |
| Movement | `walkSpeed` / `runSpeed` | 5 / 8 m/s | same | |
| | `acceleration` / `deceleration` / `reverseDeceleration` | 50 / 12 / 60 m/s² | same | |
| | `airControl` | 0.2 | same | |
| | `maxStepHeight` ◆ | 0.55 m | 0.45 | Slabs step up; full blocks need a jump |
| | `airExternalDrag` / `groundExternalFriction` | 0 /s / 15 m/s² | same | |
| | `carriedByCharacters` | false | same | |
| | `autoJump` ◆ | false (touch preset: true) | — | Dwell addition |
| | `edgeGuard` ◆ | false | — | Dwell addition |
| | `maxPushForce` ◆ | 800 N | — | Contact mass scaling |
| | `pushableMassLimit` ◆ | 400 kg | — | Heavier clusters are immovable by players |
| Probes | `groundProbeMargin` / `ceilingProbeMargin` | 0.15 / 0.10 m | same | |
| | `maxSlopeAngle` | 45° | same | |
| Jump | `height` | 1.25 m | same | Clears one block |
| | `bufferTime` / `coyoteTime` | 0.2 / 0.1 s (12 / 6 ticks) | same | Stored as ticks |
| Crouch | `height` ◆ | 0.9 m | 1.0 | Fits 1-tall crawlspaces |
| | `speed` / `midAirBoost` | 1.6 m/s / 0 | same | |
| Climb | `speed` / `lookDownThreshold` | 3 m/s / 30° | same | |
| | `jumpOffVelocity` / `snapStrength` | (0, 4, 3) m/s / 10 /s | same | |
| Swim ◆ | `speed` / `enterFraction` / `exitFraction` | 3 m/s / 0.6 / 0.4 | — | Dwell addition |
| | `buoyancy` / `drag` / `floatFraction` | 12 m/s² / 2 /s / 0.7 | — | |
| Damage ◆ | `fallDamageMinSpeed` / `fallDamagePerSpeed` | 12 m/s / 8 per m/s | — | From the `Landed` event's impact speed |
| | `crushSpeed` / `crushTicks` | 4 m/s / 6 | — | |
| Advanced | `stepProbeDistance` / `probeRingRadius` / `wallCheckDistance` | 0.01 m / 0.9 / 0.16 m | same | |
| | `externalAbsorbThreshold` | 0.01 m/s | same | Tune against native↔WASM noise (§8.3) |
| | `maxPlatformYawSpeed` | 360 °/s | same | |

A **Unity parity** preset (PPC `ApplyUnityParity`, with Dwell's body dimensions) is kept for
comparison testing.

---

## 8. Networking: prediction & reconciliation **[built]**

The PPC relies on Quantum's rollback of the whole world. Dwell predicts **only the local player**
against a small client physics world, and the server is authoritative. The prediction logic is
C++ (`player::Predictor`, `predictor.h`): natively it backs the latency/loss tests, and in the
browser it runs in the client's own WASM instance of the sim core on the main thread
(`dwell_client_*` exports, `client/src/sim/clientCore.ts`).

### 8.1 Client prediction world
Owned by the `Predictor` (same Jolt settings as the server):
- static terrain collision from the client's chunks (same `TerrainCollision` and `VoxelQuery`;
  Phase 2 clients generate the chunks themselves from the generator version in `Welcome`);
- **kinematic** capsules for remote players at their **latest snapshot position, dead-reckoned by
  their velocity** until the next snapshot (at most 0.5 s). Measured against the alternative of
  extrapolating to the predicted present: head-on bumps corrected with 0.17–0.23 m per-tick
  rendered steps and no snaps, versus snaps of 1–2 m (an extrapolated kinematic proxy shoves the
  local player, while on the server two equal-mass bodies stop each other). Tier 1 proxies
  (present-time within `PREDICT_PROXY_RADIUS`, ARCHITECTURE §9.4) arrive in Phase 4;
- the local player as the only dynamic body, with the server's body settings.

### 8.2 Loop
- **Input:** each client tick samples input, quantizes it once (TypeScript `quantizeInput`,
  mirroring C++ `QuantizeInput`: move clamped to the unit circle, `i8` axes, button bits, yaw as a
  wrapped `i16` fraction of a turn, pitch `i16` for ±90°), predicts with the *dequantized* value
  (the server dequantizes the same integers, so both simulate identical input), stores
  `{inputSeq, input, PlayerController, position, velocity}` in a 256-tick ring, and sends a
  `PlayerInput` datagram carrying the newest 4 inputs.
- **Server:** inputs queue per session ordered by `inputSeq` (§11 validation; see ARCHITECTURE
  §8.3). A **jitter buffer** consumes one input per tick once 2 are queued; when it runs dry the
  last input repeats and the buffer re-primes; beyond 6 queued inputs the oldest are skipped. The
  snapshot reports the queue length (`inputBuffer`); the client nudges its tick rate ±2 % to keep
  it between 1 and 4 (clock drift between client and server otherwise starves or floods it).
- **Snapshot (20 Hz):** compared with the stored entry for `ackInputSeq`: position within 1 cm,
  velocity within 5 cm/s, controller flags equal, external velocity within 5 cm/s → nothing to do
  (the common case). Otherwise the server state is restored (resumable controller state from §8.4
  over the stored entry, which supplies the input) and every unacknowledged input is replayed
  (pipeline + prediction-world step).
- **Smoothing:** the visible position is `predicted + offset`; a replay adds `before − after` to
  the offset so the view doesn't jump, and the offset decays with a 0.1 s time constant. A
  correction above `RECONCILE_SNAP_DISTANCE` snaps (offset cleared) — except knockback
  corrections, smoothed up to 4× that distance.
- **Knockback:** the server applies server-originated velocity changes after the controller pass,
  before the physics step, and sends `PlayerEvent(Knockback)` with the `inputSeq` processed that
  tick. The client records it, restores its own predicted state at `inputSeq − 1`, and replays
  with the kick inserted after that input's pipeline. When the snapshot overtakes the event (they
  travel on different channels), the snapshot's `lastKnockbackSeq` tells the client the
  correction is a knockback.
- Replays are cheap: one dynamic body in a near-empty world, ~6–12 ticks at typical RTTs.

**Measured** (`netcode: prediction`, in-process network simulator, two clients):
- 150 ms RTT, 20 ms jitter, 5 % loss: ~1 % of snapshots replay, position error at the
  acknowledged input p95 < 1 mm, no snaps. The residual corrections are one tick of motion
  (≤ 13 cm at run speed) when jitter starves the server's input buffer.
- Launch-pad knockback: no snaps, largest rendered step 0.6 m per tick.
- Head-on bump: no snaps, largest rendered step ≤ 0.23 m.
- Input takes effect on the next predicted tick.

### 8.3 Divergence budget **[built]**
Native and WASM Jolt are not assumed bit-identical. Jolt is built with
`JPH_CROSS_PLATFORM_DETERMINISTIC` and everything without FMA contraction (`-ffp-contract=off`),
and the result is **measured** in CI: `dwell_scenario_trace` runs the four-player scenario natively
and under Node (WASM), and `server/tools/divergence.mjs` compares them. Measured: positions
bit-identical over all 600 ticks; velocities within 1.2 × 10⁻⁷ m/s (float rounding in the trig
functions), far below `externalAbsorbThreshold` (0.01 m/s), which the check enforces for the
divergence added per tick. The whole ported player and netcode suite also passes under WASM
(`dwell_player_tests.js`).

### 8.4 Wire format
- `PlayerInput`: analog move vector (`i8 moveX, moveY`) for gamepads and touch sticks; `jump`,
  `run`, `crouch` in the `u16` button bitfield; quantized yaw and pitch (ARCHITECTURE §8.3).
- The local player's snapshot block carries the body state (capsule centre and velocity, `f32`),
  flags, health, `State`, `inputBuffer`, `lastKnockbackSeq`, and the controller state needed to
  resume simulation exactly (47 bytes, +12 while climbing, +8 after letting go of a ladder):
  flags (grounded, jumping, crouching, climbing, hasReleased, swimming); horizontal `current`,
  `external`, and `contribution` (x, z); vertical `accumulatedY`, `platformY`, `targetY`; the
  ground's vertical velocity; the ground reference (kind + player id); jump buffer and coyote ticks;
  step grace; ladder and released cells. Per-tick scratch (inputs, events, probe results, climb
  and swim velocities) is recomputed.
- Remote players receive only feet position (`f32`), velocity (`f16`), view angles, `State`, and
  flags.

---

## 9. Client presentation (from the PPC view layer) **[built, except animation]**

- **Camera** (`client/src/game/game.ts`): first-person at the smoothed render position
  (interpolated between the last two ticks plus the correction offset); eye height follows crouch
  with exponential smoothing (the sim's crouch is instant); **step smoothing** (PPC `SmoothSteps`:
  the eye rises at most 4 m/s after a grounded lift up to `maxStepHeight`, never lags more than
  that) hides the one-tick lift onto slabs; `platform.yawDelta` turns the camera with rotating
  ground. Dwell's world is **right-handed, Y up**: yaw 0 looks along +Z, and right of +Z is −X (the
  PPC's Unity convention is left-handed); the controller's camera-right vector follows this.
- **Players:** remote players are capsules with a visor, interpolated 100 ms in the past; dead
  players are drawn lying down (a cosmetic pose; the physics ragdoll moves to Phase 5 with the
  client debris world). While dead, the camera orbits the body until respawn.
- **Animation:** not yet — there are no character models. The parameters listed by the PPC
  (`Speed`, `IsGrounded`, … from `State`, velocity, and flags) are all available client-side.
- **Debug overlay** (F3): state, ground ref and gap, wall/ceiling/submersion, position, velocity,
  layer velocities (current, external, target), prediction stats (snapshots, replays, snaps,
  knockback replays, error at ack, last correction, smoothing offset), server input buffer, tick
  rate, RTT; probe rays (ground ring, walls) and the velocity vector drawn in 3D.
- **Network condition simulator:** `?netsim=<rtt ms>,<jitter ms>,<loss %>` wraps the transport.

---

## 10. Testing (porting the PPC test suite)

**Built:** the PPC's headless Quantum tests are ported to C++ (doctest) in `server/tests/player`,
using a `PlayerTestWorld` builder (`player_test_world.h`) with voxel primitives (floor, block
steps, slab steps, walls, 1×2 doorways, 1-tall crawlspaces, ladder columns, water pools), Tier 1
boxes (kinematic or dynamic) as platforms and ramps, scheduled kicks and explosions, and per-player
event counters. Expectations use Dwell's default config (recommended feel, voxel capsule):

| PPC suite | Dwell equivalent |
|---|---|
| Phase1Skeleton | Stands still without drift or tipping; heavy cluster pushes the player |
| Phase2Movement | Probes, speeds, accel/decel/reverse, walls, no diagonal speed-up, step-up on slabs, blocked by full blocks, air control, external absorption and decay |
| Phase3Vertical | Fall and land event, jump apex, buffer, coyote, no double jump, ceiling, launch |
| Phase4Crouch | Feet planted, crouch into a crawlspace, blocked stand-up, mid-air tuck |
| Phase5Platform | Riding translating, rotating, and falling Tier 1 bodies; jump-off keeps momentum; explosion |
| Phase6Climb | Ladder columns: grab, climb, look-down reversal, strafe, jump-off, climb over the top |
| CharacterStacking / GroundedConsistency / StepSmoothness | Same scenarios on voxel geometry |
| GoldenTrace | Four-player scenario: identical across repeated runs; within 1 mm of `server/tests/player/golden/scenario-trace.txt` (regenerate with `DWELL_UPDATE_GOLDEN=1`); native↔WASM compared by `divergence.mjs` (§8.3) |
| — (Dwell) | Built: swim enter/float/dive/exit, shallow water, auto-jump, edge guard, doorways, crawlspaces, block-under-feet removal, same-tick collision with a placed block, chunk seams, collision-mesh unit tests; networked players (`netcode_test.cpp`, `netsim.h`: the real server and per-client predictors over simulated links) — input validation and rate limits, fall damage, death and respawn on every client, reconciliation under latency, jitter and loss, knockback replay, player bumps. Later phases: placement rejection (3), crush and push-force cap (4) |

Performance gate: 64 players' controller passes (excluding the Jolt step) under 1 ms/tick —
measured at ~0.36 ms in the Release build; checked by `player: performance` (strict under
`NDEBUG`, loose in Debug builds).
