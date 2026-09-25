# Dwell — Physics Player Controller Spec

> Part of the architecture documentation (see [`ARCHITECTURE.md`](./ARCHITECTURE.md) §9).
> Keep this file current under the same rule as `ARCHITECTURE.md` (see `CLAUDE.md`).
>
> Status: **[planned]**

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
  Vec2  move;                     // x = right, y = forward, camera-relative, |move| <= 1
  float lookYaw, lookPitch;       // degrees; pitch positive = up
  bool  jump, run, crouch;
};

struct GroundRef { enum Kind : uint8_t { None, Terrain, Tier1Body, Player } kind; uint32_t id; };

struct GroundInfo {
  bool  grounded, wasGrounded;    // THE grounded flag every pass uses (PPC semantics)
  Vec3  normal;  float slopeAngle, gap;
  GroundRef ground;
  bool  ceilingBlocked, touchingWall;  Vec3 wallNormal;
};

struct HorizontalLayer { Vec3 current, external, contribution; };
struct VerticalLayer   { float accumulatedY, platformY, lastPlatformY, targetY; };
struct JumpState       { uint16_t bufferTicks, coyoteTicks; bool isJumping, jumpedThisTick; };
struct CrouchState     { bool crouching; };
struct ClimbState      { bool climbing; IVec3 ladderCell; Vec3 velocity; IVec3 releasedCell; bool hasReleased; };
struct SwimState       { bool swimming; float submerged; Vec3 velocity; };            // Dwell addition
struct PlatformState   { GroundRef ground; Vec3 prevPosition; Quat prevRotation;
                         Vec3 groundVelocity, baseVelocity; float yawDelta; };

struct PlayerController {
  uint16_t configId;
  Input input, previousInput;
  State state;
  GroundInfo ground;
  HorizontalLayer horizontal;  VerticalLayer vertical;
  JumpState jump;  CrouchState crouch;  ClimbState climb;  SwimState swim;
  PlatformState platform;
  Vec3 targetVelocity;
  uint32_t events;                // PlayerEvent bits raised this tick
};

}
```

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
no double jump from coyote time; ladder face snap, jump-off launch, look-down threshold.

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

Rules that keep the grid and the physics world consistent:
- A voxel edit rebuilds the affected chunk's collision `MeshShape` **in the same tick** on the
  server (synchronously for edited chunks), so probes and contacts never disagree.
- The player body enables Jolt's `mEnhancedInternalEdgeRemoval` so the capsule slides over greedy
  mesh and chunk seams without catching on internal edges.
- Future non-cube voxel shapes (slabs, stairs) extend the DDA with a per-material sub-cell shape
  test; the probe interface does not change.

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
- `maxStepHeight` 0.45 m: full 1 m blocks need a **jump** (jump height 1.25 m clears one block);
  step-up applies to slabs and cluster debris. Walking off a 1 m ledge is a short fall (larger
  than the step reach), so there is no stair snapping on full blocks.
- **Auto-jump** (Dwell addition, config `autoJump`, default on for touch input): when grounded,
  moving into a 1-block obstacle with 2 free cells above it triggers a jump. It runs in-sim, so it
  is predicted like any other input.
- **Edge guard** (Dwell addition, optional, off by default): while crouched and grounded, the
  horizontal layer clamps movement that would take the ground ring off a ledge higher than
  `maxStepHeight` (classic sneak-at-edges).

### 6.3 Ladders as voxels
- PPC ladders are trigger colliders with a `PPCLadder` component. In Dwell a ladder is a
  **climbable material** in the material table (`climbable = true`) with **facing variants**
  (`ladder_n/e/s/w`), so no voxel format change is needed.
- Detection: any cell overlapping the player's capsule AABB with a climbable material. The ladder
  frame (facing axis, face depth) comes from the cell centre and variant, replacing the trigger
  box's transform and extents in `ClimbVelocity`/`AwayFromLadder`.
- A contiguous ladder column counts as one ladder for `released` purposes: a ladder you let go of is
  not re-grabbed until the capsule has left every cell of that column.
- Vines and scaffolding use the same flag with different speeds (`climbSpeedScale` per material).

### 6.4 Water (Dwell addition)
- `submerged` = fraction of the capsule height inside water cells. Above `swim.enterFraction`
  the exclusive **swim layer** takes over: move input in the look direction (pitch included), jump
  = ascend, crouch = descend, buoyancy toward the surface, and linear drag. Below
  `swim.exitFraction` it hands back to the normal layers (hysteresis). The layer holds the other
  layers exactly like the climb layer does, so leaving the water is not read as an external force.

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

`PlayerControllerConfig` lives in `shared/protocol` (one table, ID-referenced so the server can run
several presets, e.g. desktop vs. touch). Defaults follow the PPC's **recommended feel** (Source
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
| | `maxStepHeight` | 0.45 m | same | Full blocks need a jump |
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
| | `buoyancy` / `drag` | 12 m/s² / 2 /s | — | |
| Damage ◆ | `fallDamageMinSpeed` | 12 m/s | — | From the `Landed` event's impact speed |
| | `crushSpeed` / `crushTicks` | 4 m/s / 6 | — | |
| Advanced | `stepProbeDistance` / `probeRingRadius` / `wallCheckDistance` | 0.01 m / 0.9 / 0.16 m | same | |
| | `externalAbsorbThreshold` | 0.01 m/s | same | Tune against native↔WASM noise (§8.3) |
| | `maxPlatformYawSpeed` | 360 °/s | same | |

A **Unity parity** preset (PPC `ApplyUnityParity`) is kept for comparison testing.

---

## 8. Networking: prediction & reconciliation

The PPC relies on Quantum's rollback of the whole world. Dwell predicts **only the local player**
against a small client physics world, and the server is authoritative.

### 8.1 Client prediction world
Built by the client-side WASM core (same Jolt settings as the server):
- static terrain collision from the client's chunks (same meshes, same `VoxelQuery` grid);
- **kinematic** proxies for Tier 1 bodies (present-time extrapolated within `PREDICT_PROXY_RADIUS`,
  ARCHITECTURE §9.4) and remote players (interpolated);
- the local player as the only dynamic body, with the server's body settings.

### 8.2 Loop
- Every tick: sample `Input`, run the §4 pipeline for the local player, step the prediction world,
  store `{inputSeq, Input, PlayerController, position, velocity}` in a ring buffer, send input.
- On snapshot (`ackInputSeq`, authoritative body state + controller state, §8.4): compare with
  the stored entry for `ackInputSeq`. Within tolerance → drop older history, done (the common
  case; no replay). Otherwise → restore the server state, re-run the pipeline + prediction-world
  step for every unacked input, and smooth the visual difference (snap above
  `RECONCILE_SNAP_DISTANCE`).
- Replays are cheap: one dynamic body in a near-empty world, ~6–12 ticks at typical RTTs.
- Server impulses (explosions, knockback) arrive as `PlayerEvent(Knockback, tick)` and are inserted
  at that tick before replaying (ARCHITECTURE §9.3).

### 8.3 Divergence budget
Native and WASM Jolt are not assumed bit-identical. Build Jolt with
`JPH_CROSS_PLATFORM_DETERMINISTIC` and compile without FMA contraction (`-ffp-contract=off`) to
keep them as close as possible, then **measure** it: a CI test runs the PPC scenario suite natively
and under WASM (Node) and records the maximum per-tick position divergence. `externalAbsorbThreshold`
must stay above that noise so solver differences are not absorbed as external forces.

### 8.4 Wire additions
- `PlayerInput` gains an analog move vector (`i8 moveX, moveY`) for gamepads and touch sticks;
  `jump`, `run`, `crouch` stay in the button bitfield.
- The local player's snapshot block carries the controller state needed to resume simulation
  exactly: horizontal `current` + `external`, vertical `accumulatedY` + `targetY`, jump timers
  and flags, crouch, climb (cell and flags), swim, and `platform.ground`. Quantized, about 48 bytes.
  Remote players receive only the transform, velocity, `State`, and flags.

---

## 9. Client presentation (from the PPC view layer)

- **Camera:** first-person rig using `lookYaw/lookPitch` from local input; eye height follows crouch
  with smoothing (the sim's crouch is instant); **step smoothing** (PPC `SmoothSteps`,
  `StepSmoothSpeed`, `MaxStepLag`) hides the one-tick lift or snap on slabs and small steps;
  `platform.yawDelta` turns the camera with rotating clusters.
- **Animation:** parameters from `State`, velocity, and flags (`Speed`, `DirectionX/Y`,
  `IsGrounded`, `IsFalling`, `IsCrouching`, `IsRunning`, `IsClimbing`, `IsSwimming`, `Jump` trigger).
  Local player: from predicted events. Remote players: from state transitions in snapshots.
- **Debug overlay:** state label, probe rays and hits (ground ring, ceiling, walls), layer
  velocities (current, external, target), ground ref, prediction error.

---

## 10. Testing (porting the PPC test suite)

The PPC's 95 headless Quantum tests become C++ tests in `server/core/tests/player`, using a
`PlayerTestWorld` builder with voxel primitives (floor, block steps, slab steps, walls, 1×2
doorways, 1-tall crawlspaces, ladder columns, water pools) plus Tier 1 bodies as moving platforms:

| PPC suite | Dwell equivalent |
|---|---|
| Phase1Skeleton | Stands still without drift or tipping; heavy cluster pushes the player |
| Phase2Movement | Probes, speeds, accel/decel/reverse, walls, no diagonal speed-up, step-up on slabs, blocked by full blocks, air control, external absorption and decay |
| Phase3Vertical | Fall and land event, jump apex, buffer, coyote, no double jump, ceiling, launch |
| Phase4Crouch | Feet planted, crouch into a crawlspace, blocked stand-up, mid-air tuck |
| Phase5Platform | Riding translating, rotating, and falling Tier 1 bodies; jump-off keeps momentum; explosion |
| Phase6Climb | Ladder columns: grab, climb, look-down reversal, strafe, jump-off, climb over the top |
| CharacterStacking / GroundedConsistency / StepSmoothness | Same scenarios on voxel geometry |
| GoldenTrace | Multi-player scenario trace, compared exactly on the same build and with tolerance native↔WASM |
| — (Dwell) | Swim enter/exit, auto-jump, edge guard, block-under-feet removal, placement rejection, crush, push-force cap, reconciliation under simulated latency and loss |

Performance gate: 64 players' controller passes (excluding the Jolt step) under 1 ms/tick on the
reference server.
