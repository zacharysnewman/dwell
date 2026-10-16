# Dwell — Physics Player Controller Spec

> Part of the architecture documentation (see [`ARCHITECTURE.md`](./ARCHITECTURE.md) §9).
> Keep this file current under the same rule as `ARCHITECTURE.md` (see `CLAUDE.md`).
>
> Status: **[built]** (Phase 2; on Rust and Rapier since Phase 24, [ADR 0028](./adr/0028-rust-port.md))
> — the controller, voxel queries, terrain collision, networking with prediction and
> reconciliation, and presentation (`crates/dwell-physics`, its suites in
> `crates/dwell-physics/tests`; the server side in `crates/dwell-sim`; the client side in
> `crates/dwell-game` and `crates/dwell-client`). Not yet: animation (no character models), and the
> Tier 1 interactions of §6.6 (push cap, crush, riding clusters — Phase 20).

This spec ports the **Physics Player Controller (PPC)** —
[`zacharysnewman/physics-player-controller`](https://github.com/zacharysnewman/physics-player-controller),
specifically its deterministic Photon Quantum 3 port in `Quantum~/` — into Dwell's architecture:
Rust, Rapier (`rapier3d-f64` with `enhanced-determinism`), a server-authoritative simulation with
client prediction, and a voxel world.

The PPC's **behaviour, layer model, system order, tuning, and bug fixes are kept**. What changes is
the *host*: Quantum ECS systems become ordered Rust passes in `crates/dwell-physics`
(`controller.rs`), Quantum physics becomes Rapier behind Dwell's own wrapper (`physics.rs`: `Physics`,
an opaque `BodyHandle`, `DVec3`/`Vec3`; no Rapier types outside the crate), `FP` fixed-point
becomes `f32` (positions `f64`), Quantum rollback becomes server reconciliation, and generic
colliders/triggers become voxel-grid queries and voxel materials.

---

## 1. Model

The player is a **dynamic Rapier rigid body** (capsule) driven by **velocity layers**:

1. Each tick, independent layers compute velocity contributions — *horizontal* (walk/run,
   external pushes), *vertical* (gravity, jump, ground following, launches), and optional
   *exclusive* layers (climb, swim, fly) that replace the others while active.
2. The aggregate pass sums them into a **target velocity** and writes it directly to the body.
3. The Rapier step resolves contacts. Anything that moved the body away from its target — a
   collision, a falling cluster, an explosion — is detected on the next tick as the difference
   between the body's actual velocity and last tick's contribution, **absorbed** into an
   *external velocity* term, and **decays** (friction on the ground, drag in the air).

That absorption loop is what makes the character *physically reactive* — it is shoved by falling
structures, thrown by explosions, carried by moving clusters, and pushes loose blocks — while the
layers keep the movement tight and responsive.

A kinematic character cannot be pushed by the solver; the PPC model gets that for free and the
Quantum port has already fixed the classic dynamic-controller problems (hovering, slope launches,
stair hops, double jumps).

---

## 2. Mapping: PPC (Quantum) → Dwell

| PPC Quantum | Dwell |
|---|---|
| `PPCCharacter` component (DSL) | `struct PlayerController` — plain data, `Copy` (for history buffers and snapshots) |
| `PPCConfig` asset (`FP` fields) | `struct PlayerControllerConfig` (`config.rs`) — `f32` fields, one Rust definition the server and the client both compile |
| `SystemMainThreadFilter` systems in `PPCSystemGroup` | Ordered passes `Players::step_*` run by `Players::tick()` |
| `PhysicsBody3D` dynamic, `RotationFreeze.FreezeAll`, `GravityScale 0`, no sleeping | `RigidBodyBuilder::dynamic()` with `lock_rotations()`, `gravity_scale(0)`, `can_sleep(false)`, `linear_damping(0)`, and `ccd_enabled(true)` (§6.7) |
| Frictionless `PhysicsMaterial` (created on demand) | Collider `friction(0)`, `restitution(0)`, both with the `Min` combine rule |
| `Shape3D.CreateCapsule(radius, extent)` | `SharedShape::capsule_y(halfCylinder, radius)` — same convention (`extent` = half cylinder height) |
| Crouch shape swap + `ResetCenterOfMass`/`ResetInertia` | `Collider::set_shape` with the crouched capsule; the collider's fixed mass keeps the body's mass |
| `f.Physics3D.RaycastAll` / `OverlapShape` | `VoxelQuery` (grid DDA / cell overlap) for terrain **plus** Rapier's query pipeline (`cast_ray_and_get_normal` / `intersect_shape`) for moving bodies (§5) |
| `PPCLadder` trigger entities | **Climbable blocks** with a facing property (§6.3) |
| `EntityRef` ground | `enum GroundRef { None, Terrain, Tier1Body(BodyHandle), Player(BodyHandle) }` (`query.rs`) |
| `FP` seconds for timers | **Integer ticks** (stored as ticks in the config) |
| `f.DeltaTime` | Fixed `1 / SIM_HZ` (60 Hz) |
| `PPCInputBridge` partial hook | `Input` dequantized from the `PlayerInput` datagram (§8, `net.rs`) |
| `PPCForces.AddVelocity` / `AddExplosion` | `Players::add_velocity()` / `add_explosion()`, called by the server for knockbacks and explosions (ARCHITECTURE §7.2) |
| Quantum events (`PPCJumped`, `PPCLanded`, …) | `events` bits + `landed_speed` per tick; drives fall damage (server) and audio/animation (client) |
| Quantum rollback (full-world determinism) | Server authority + local-player prediction and reconciliation (§8) |
| Unity's left-handed axes | Right-handed, Y up (Rapier, the wgpu renderer): camera-right at yaw 0 is −X (§9) |
| `PPCCameraView`, `PPCAnimatorView`, `PPCDebugView` | Client camera, eye smoothing and debug overlay (`dwell-client`, `dwell-game`; §9) |

**One implementation.** The controller lives only in Rust (`crates/dwell-physics`). The server
(`dwell-sim`) runs it natively; the browser client runs the same crate compiled to
`wasm32-unknown-unknown` inside its own module (`dwell-client`), so prediction and authority never
drift because of two implementations. See ARCHITECTURE §5.

---

## 3. State

```rust
// crates/dwell-physics/src/controller.rs
pub enum State { Idle, Walking, Running, Crouching, Sliding, Jumping, Falling, Climbing, Swimming, Flying }

pub struct Input {                    // one tick; buttons are held state, presses are detected in-sim
    pub move_x: f32, pub move_y: f32, // x = right, y = forward, camera-relative, |move| <= 1
    pub look_yaw: f32,                // degrees; yaw 0 = +Z, 90 = +X
    pub look_pitch: f32,              // degrees; positive = up
    pub jump: bool, pub run: bool, pub crouch: bool,
    pub fly: bool,                    // creative flight mode is on (Dwell addition, §6.7)
    pub fly_speed: u8,                // flight speed level (Dwell addition, §6.7)
}

pub enum GroundRef { None, Terrain, Tier1Body(BodyHandle), Player(BodyHandle) }   // query.rs

pub struct GroundInfo {
    pub grounded: bool, pub was_grounded: bool,  // THE grounded flag every pass uses (PPC semantics)
    pub normal: Vec3, pub slope_angle: f32, pub gap: f32,
    pub ground: GroundRef,
    pub ceiling_blocked: bool, pub touching_wall: bool, pub wall_normal: Vec3,
}

pub struct HorizontalLayer { pub current: Vec3, pub external: Vec3, pub contribution: Vec3 }
pub struct VerticalLayer   { pub accumulated_y: f32, pub platform_y: f32, pub last_platform_y: f32,
                             pub target_y: f32, pub step_grace: u8 }
pub struct JumpState       { pub buffer_ticks: u16, pub coyote_ticks: u16,
                             pub jumping: bool, pub jumped_this_tick: bool }
pub struct ClimbState      { pub climbing: bool, pub ladder: Cell, pub velocity: Vec3,
                             pub has_released: bool, pub released: Cell }
pub struct SwimState       { pub swimming: bool, pub submerged: f32, pub velocity: Vec3 }  // Dwell addition
pub struct FlyState        { pub flying: bool, pub velocity: Vec3 }                        // Dwell addition
pub struct PlatformState   { pub ground: GroundRef, pub ground_velocity: Vec3,
                             pub base_velocity: Vec3, pub yaw_delta: f32 }

#[derive(Clone, Copy)]
pub struct PlayerController {
    pub input: Input, pub previous_input: Input,
    pub state: State,
    pub ground: GroundInfo,
    pub horizontal: HorizontalLayer, pub vertical: VerticalLayer,
    pub jump: JumpState, pub crouching: bool,
    pub climb: ClimbState, pub swim: SwimState, pub fly: FlyState,
    pub platform: PlatformState,
    pub target_velocity: Vec3,
    pub face: i8,          // +1 face A, −1 face B (§11)
    pub events: u32,       // events:: bits raised this tick (JUMPED, LANDED, CROUCH_CHANGED,
    pub landed_speed: f32, //   CLIMB/SWIM/FLY_STARTED/ENDED); LANDED's impact speed
}
```

`PlatformState` has no previous position/rotation: the PPC needed them for platforms moved by
script, but Rapier moves kinematic (velocity-based) bodies by their velocity, so every carrying
body is read with its point velocity (`Physics::point_velocity`, `v + ω × r`; §6.6). `Players`
(`controller.rs`) owns the players of one physics world: `spawn`, `despawn`, `set_input`, `tick`
(the §4 pipeline for every player), `add_velocity`, `add_explosion`, and state access
(`controller`, `restore`, `teleport`, `position`, `velocity`, `feet`, `head`) for snapshots and
reconciliation.

The body and collider handles, the player's config and last step's contact normals are held beside
this struct (in `Players`), not inside it, so `PlayerController` stays a copyable value for history
buffers.

---

## 4. Per-tick pipeline

The server tick (ARCHITECTURE §4.2) runs, for every player, **before** `Physics::step`:

| # | Pass | PPC origin | Job |
|---|---|---|---|
| 1 | `step_input` | `PPCInputSystem` | `previous_input = input`; take this tick's input; switch face if across the midplane (§11); clamp `|move| <= 1` |
| 2 | `step_probe` | `PPCProbeSystem` | Ground ring (16 + centre), ceiling ring, 4 axis wall rays via `VoxelQuery` + moving bodies (§5) |
| 3 | `step_platform` | `PPCPlatformSystem` | Velocity of the ground under the player (`v + ω × r` for Tier 1 bodies; zero for terrain; zero for players unless `carried_by_characters`) |
| 4 | `step_fly` | — (Dwell) | Exclusive creative-flight layer while `fly` is held (§6.7) |
| 5 | `step_crouch` | `PPCCrouchSystem` | Hold-to-crouch; shape swap; feet planted when grounded, head kept in the air; stand-up overlap test |
| 6 | `step_jump` | `PPCJumpSystem` | Buffer + coyote (in ticks); `accumulated_y = max(accumulated_y, jump_v + platform_y)`; no coyote after a real jump; auto-jump (§6.2) |
| 7 | `step_climb` | `PPCClimbSystem` | Exclusive layer on climbable voxels (§6.3); not while flying |
| 8 | `step_swim` | — (Dwell) | Exclusive layer in water voxels (§6.4) and in the flip band (§11); not while climbing or flying |
| 9 | `step_horizontal` | `PPCMovementLayerSystem` | Absorb external; accel/decel/reverse in the platform's frame; air control; step-up; edge guard |
| 10 | `step_vertical` | `PPCVerticalLayerSystem` | Gravity; walk-off keeps platform Y; ceiling cancel; launch detection; ground following + snap |
| 11 | `step_aggregate` | `PPCAggregateSystem` | `target = exclusive ? layer.velocity : horizontal.contribution + (0, vertical.target_y, 0)`; set the body's linear velocity |
| 12 | `step_state` | `PPCStateSystem` | Priority: Climbing > Swimming > Flying > Crouching > Jumping/Falling > Running/Walking/Idle |
| — | *`Physics::step(1/60)`* | Quantum physics step | Contacts resolved; deviations are absorbed next tick |

Each pass's behaviour is exactly the PPC Quantum system's, including its fixes over the Unity
package (listed in the PPC `Quantum~/CHANGELOG.md`): single shared `grounded` flag that turns off
while rising faster than the ground; stair "stay on ground" probe reach; ground following with
one-tick snap (no hovering, no slope launches); capsule always centred on the body during crouch;
no double jump from coyote time; ladder face snap, jump-off launch, look-down threshold. As in
Quantum, the passes run pass-major (each pass for every player, then the next pass). Before the
passes, `tick` makes sure terrain collision exists around every player and syncs changed chunks
(§5), then reads each player body's contact normals from the last step (`Physics::contact_normals`,
at most 8, from Rapier's narrow phase). Sines, cosines and the like come from `libm`, so native and
WASM builds compute them identically (§8.3).

**Dwell deviations** (each covered by a test in `crates/dwell-physics/tests`):
- **Blocked motion is not absorbed.** The PPC absorbs *any* difference between the body's velocity
  and last tick's contribution, including the velocity a wall removes while the player walks into
  it. That leaves a phantom push-back that never decays in the air (air drag 0), so a player
  holding forward against a block could not jump onto it — the core voxel move. The horizontal
  layer therefore removes, per contact normal of the body's contacts in the last step, the part of
  the deviation that only cancels the player's own push into that contact: `into` is last tick's
  whole target velocity (horizontal contribution and vertical target) into the normal, and at most
  `into × |horizontal part of the normal|` is removed along the normal's horizontal direction.
  Counting the vertical drive matters on stairs: the ground snap drives the capsule down into the
  edge of the step it is leaving, and that edge's sideways push-out would otherwise become momentum
  that carries the player down several steps (`stepping_down_a_stairs_corner_gains_no_momentum`).
  Pushes by moving bodies are still absorbed.
  The contacts are the manifolds with a point within Rapier's prediction distance
  (`Physics::contact_normals`), not those with solver contacts: when a pair has several manifolds
  (a player on a terrain chunk's collider and against its wall) Rapier solves merged clusters and
  leaves the manifolds' solver contacts empty, which hid every contact and brought the phantom
  push back (`a_jump_always_clears_a_one_block_step`,
  `crouching_against_a_wall_never_moves_the_player_backwards`).
- **Edge lift is not absorbed.** The same holds vertically. Driving forward into a contact whose
  normal faces partly up (a block's top edge under the capsule's rounded bottom, while jumping
  over it) deflects the drive upwards by `into_h × normal.y` per contact, where `into_h` is last
  tick's horizontal contribution into the normal; the horizontal pass sums this *drive lift*. In
  the air, the vertical layer takes it (at most the upward deviation) out of the deviation before
  absorbing it. Absorbed, it acted as a second jump while clearing a block (apex 2.1 m for a 1.25 m
  jump, landing far past a lone block) and floated the player up a staircase of full blocks on a
  single jump (`clearing_a_blocks_top_edge_does_not_launch`,
  `one_jump_climbs_one_step_of_a_diagonal_staircase`).
- **Edge lift is not a launch.** While grounded, the launch check (an upward deviation above
  `explained = max(target_y, ground's vertical velocity)`) also subtracts the drive lift: the
  solver rolls the capsule over the edge of a step just climbed with up to ~2.6 m/s of upward
  velocity, which would otherwise read as a launch.
- **Step-up nudge.** After lifting onto a step, the capsule also moves forward by
  `radius + step_probe_distance − ring_radius + 1 cm`, so the probe ring (inside the capsule) is
  over the step and the ground snap doesn't pull the player back down. The nudge is part of the
  tick's movement, not extra: that tick's horizontal contribution drops by `nudge / dt` along the
  move (never below zero), so stepping onto a half step, landing on a block's edge from a jump, or
  walking up a slope (a step-up every tick) moves no faster than the speed
  (`stepping_up_never_moves_the_player_faster_than_its_speed`). `current` is unchanged, so full
  speed resumes next tick.
- **Step grace.** For 6 ticks after a step-up, only an upward deviation above 1.5 m/s counts as a
  launch: the solver nudges the capsule up as it crosses the step's edge.
- **Step probe height.** The step ray starts at `max(centre, feet + max_step_height + 5 cm)`, so a
  crouched player (centre below a half step's top) can still step onto half steps.

---

## 5. Voxel queries (probes)

The PPC probes are generic physics ray casts. Dwell answers them from the **voxel grid** for static
terrain and from **Rapier** only for things that move:

```
struct ProbeHit { distance: f32, point: DVec3, normal: Vec3, ground: GroundRef }

cast_ray(origin, dir, max, me, bodies) -> Option<ProbeHit>:
    a = cast_voxels(origin, dir, max)              // Amanatides–Woo walk over the cells' shapes
    b = bodies ? cast_bodies(origin, dir, a.distance or max, me) : None
                                                   // Rapier ray cast, Tier 1 + Character groups, excluding me
    nearest(a, b)

overlaps_solid(capsule, me):                       // crouch stand-up
    overlaps_voxels(capsule)                       // exact capsule axis–shape distance per cell
      || rapier.intersect_shape(capsule, Tier 1 + Character groups, excluding me)
```

Why:
- **Exact and cheap.** A DDA over a few cells is far cheaper than ~40 mesh ray casts per player
  per tick (the PPC's main cost: 16 characters ≈ 3.3 ms/tick in Quantum). It scales to many players
  on one server.
- **No seam artifacts.** Grid normals are exact face normals; there are no triangle-edge normals
  at chunk or mesh seams.
- **Same answer on server and client**, since both hold the same chunk data.

**Built** (`VoxelQuery`, `query.rs`):
- `cast_voxels` walks cells Amanatides–Woo style and intersects the ray with each cell's *shape*
  from the block shape table (`dwell_blocks::shape::ray_enter_shape`; the baseline's solid blocks
  are all full cubes). A hit is the ray *entering* a shape from free space; faces lying against a
  solid neighbour are internal, so a ray that starts inside solid terrain only hits after leaving
  it (like a surface mesh). Positions are double precision (`DVec3`, ADR 0011); the walk runs in
  `f32` relative to the origin's cell, and segment–box distances relative to the capsule centre, so
  probes are equally exact anywhere in the 8,192 km world.
- `cast_bodies` is a Rapier ray cast restricted to the `Tier1` and `Character` collision groups,
  excluding the player's own body. `cast_ray` returns the nearer of the two. A ring of probes
  first asks the broad phase once (`bodies_near`) and skips Rapier when no moving body is in reach
  — the common case — which keeps 64 players' passes at ~0.48 ms/tick (Release; ~0.51 ms
  ~8,000 km from the origin).
- `overlaps_solid` tests the vertical capsule against solid cell shapes exactly (axis segment to
  shape distance, `vertical_segment_distance_sq`) and against moving bodies with Rapier's
  `intersect_shape`. `submerged_fraction` samples the centre column (liquid or flooded cells).
- `FaceQuery` (`frame.rs`) wraps these for a face-B player's mirrored frame (§11).

**Terrain collision** (`TerrainCollision`, `terrain.rs`): one static body with one Rapier (Parry)
`Voxels` collider for the whole world, holding the cells of the chunks around players that collide
as full cubes (shaped cells — adaptive top blocks — are each chunk's triangle mesh, below). Parry
classifies each voxel by its neighbours, so the capsule slides across voxel edges and chunk borders
without catching on internal edges (`walking_across_chunk_seams_in_every_direction_never_loses_the_ground`:
no loss of ground crossing chunk borders in 8 directions). With `f64` positions it is exact anywhere
in the world, so it needs no regions, anchors or seams between bodies. **Shaped cells** (Phase 18)
collide as one Parry `TriMesh` per chunk (`FIX_INTERNAL_EDGES`, at the chunk's corner) of their
derived shapes' faces that no neighbour covers; a chunk's collision reads a two-cell apron, and a
chunk with solid cells is rebuilt when any of the 27 chunks around it changes stamp or revision
(an empty chunk watches only itself). Crossing between chunks'
meshes keeps the ground (`walking_a_grass_field_across_chunk_seams_never_loses_the_ground`).
- Chunks are added on demand: each tick, `Players::tick` makes sure every chunk within
  `3 m + min(2 ticks of travel, 64 m)` of each player is in the collider. Each loaded chunk keeps
  its stamp, revision and a bit mask of its solid cells; a chunk with no solid cells keeps no mask,
  so passing through the sky (fast flight) costs no memory per chunk. Every 64 ticks, chunks more
  than 2 chunks from every player are dropped.
- Rapier caps body speed at 400 m/s by default; spawning a player raises the world's limit to
  cover creative flight at the ceiling (`Physics::raise_max_linear_velocity`, §6.7).

Rules that keep the grid and the physics world consistent:
- A voxel edit updates the collider **in the same tick** on the server: `TerrainCollision::sync`
  (run in every `Players::tick`, before the passes) re-reads every loaded chunk whose stamp or
  revision changed — edits, streamed or replaced chunks — and sets only the cells that differ.
  Block edits (ARCHITECTURE.md §6.5) apply at the start of the server step, before that pass; the
  client applies a `VoxelModification` to its streamed world and its prediction world syncs the
  same way before its next predicted tick (`a_block_placed_in_front_is_collided_with_in_the_same_tick`
  in `scenario.rs`, and
  `a_block_placed_by_one_player_blocks_another_on_the_server_and_in_its_prediction` in
  `crates/dwell-sim/tests/netcode.rs`).
- Probes read voxels from the world (`VoxelWorld::read`, generating a chunk if needed), so a probe
  and the collider see the same cells.
- Non-cube voxel shapes extend the DDA through the block shape table, adaptive blocks through
  their derived shapes (`VoxelQuery::shape`); the probe interface does not change.

---

## 6. Voxel-specific behaviour

### 6.1 Dimensions tuned to the 1 m grid
The PPC default capsule (radius 0.5, height 2.0) is exactly one block wide and cannot pass
through a 1-wide, 2-tall doorway. Dwell's defaults (§7): **radius 0.3, height 1.8** (fits 1×2
openings), **crouch height 0.9** (fits 1-tall crawlspaces with skin to spare).

### 6.2 Blocks, steps, slopes
- **Natural ground is shaped** (Phase 18, [`ADAPTIVE_BLOCKS.md`](./ADAPTIVE_BLOCKS.md)): grass,
  dirt, stone and sand top blocks take their shapes from their neighbours — a level field rests at
  half height, edges slope 27° to the ground, a 1-block step is one even 27° slope across both
  blocks (walked up without jumping), one-wide rows are half-height ridges (45° flanks), a one-wide
  block against a higher one is a 45° ramp, saddles and dimples reach 54.74° — faces steeper than
  44° are slides (owner, 2026-10-15; above). The probes (§5) read the
  derived shapes (`VoxelQuery::shape`, `FaceQuery::shape`, turned over on face B), and the
  terrain collider holds shaped cells as per-chunk triangle meshes (§5). The suites build cube
  geometry from the non-adaptive `dwell:sandbox_floor` (`cube_block()`), and
  `tests/adaptive.rs` walks the adaptive shapes: the Shape Playground's 28 situations built as
  terrain (every shape the rules make), where a player stands still on every gentle block
  (no face steeper than 27°) without sliding and walkers along every row and column never
  launch, hop or jump; a terrace's 1-block step up and down; a grass field across chunk seams
  (the per-chunk meshes) without losing the ground; a block placed beside a standing player
  reshaping the ground under it without a launch — at the origin, far out and on face B. Resting
  on and climbing 45° faces are left to the slide (owner, 2026-10-15). Slope logic matters on **Tier 1 bodies** (rotated clusters; ramps in the tests are
  rotated Tier 1 boxes, `walks_up_and_down_a_20_degree_ramp_staying_grounded`), and half-height
  steps (stairs, the scenario's step) are static Tier 1 boxes (`PlayerTestWorld::half_step`).
- `max_slope_angle` is **50°**: a standard 45° slope face must not hinge on float rounding of its
  normal (`the_slope_limit_leaves_room_for_45_degrees`). **Owner decision (2026-10-15): faces
  steeper than 44° are slides** — the adaptive shapes' 45° ramps and ridge flanks and the saddles'
  54.74° faces make the player slide; stairs become their own block later. Sliding is built on its
  own branch (`claude/slope-slide`), which replaces this limit and the tests of steep faces.
- The bifacial world puts a second face on the disc's underside, where up is −y; the controller
  runs in a face-local frame there (§11, [`BIFACIAL_WORLD.md`](./BIFACIAL_WORLD.md) §6).
- `max_step_height` 0.55 m (PPC: 0.45): half-height steps (0.5 m) are stepped up without leaving
  the ground, full 1 m blocks need a **jump** (jump height 1.25 m clears one block); step-up also
  applies to Tier 1 bodies (the step ray casts moving bodies too). Walking off a 1 m ledge is a
  short fall (larger than the step reach), so there is no stair snapping on full blocks
  (`a_one_metre_ledge_is_a_fall_not_a_step`).
- **Auto-jump** (Dwell addition, config `movement.auto_jump`, on in the touch preset
  `PlayerControllerConfig::touch()`): when grounded and not crouched, moving into a 1-block
  obstacle (top between `max_step_height` and 1.05 m above the feet, probed 0.2 m beyond the
  capsule) with 2 free cells above it triggers a jump. It runs in-sim, so it is predicted like any
  other input.
- **Edge guard** (Dwell addition, optional, off by default): while crouched and grounded, the
  horizontal layer zeroes, per axis (X, Z), movement after which no ray of the ground ring would
  find ground within `max_step_height` (classic sneak-at-edges).

### 6.3 Ladders as voxels
- PPC ladders are trigger colliders with a `PPCLadder` component. In Dwell a ladder is a
  **climbable block** in the block data (`shared/blocks`, `"climbable": true`) with a **facing
  property** (`dwell:ladder[facing=north|east|south|west]`); the generated state table gives each
  state's `climbable`, `facing` and `climb_speed_scale`.
- Detection: any climbable cell the capsule overlaps (capsule–cell test). The ladder is a thin
  plate (0.1 m) on the cell's back face; the ladder frame (facing axis, plate position) comes from
  the cell and its facing, replacing the trigger box's transform and extents in the climb velocity
  and the jump-off direction. The face snap therefore holds the capsule against the wall the
  ladder is mounted on.
- A contiguous ladder column counts as one ladder for `released` purposes: a ladder you let go of is
  not re-grabbed until the capsule has left every cell of that column.
- **Over the top** (voxel adaptation): while climbing, a column's climbable region reaches 0.6 m
  above its top cell, and within 0.3 m of the top, climbing up also moves towards `−facing`
  (without the face snap), so the player gets onto the ledge the ladder leans on. (PPC ladder
  triggers overhang their ledge instead; a voxel ladder cell cannot.) That extra reach only keeps
  a climb going, it never starts one: grabbed from the ledge, the face snap would pull the player
  back against the ledge and block the way down. Getting on at the top means stepping off the
  ledge onto the ladder cells themselves.
- The climb speed is `climb.speed × climb_speed_scale` of the ladder's block state, so other
  climbable blocks can climb at their own speed by data alone.

### 6.4 Water (Dwell addition)
- `submerged` = fraction of the capsule height inside water (liquid or flooded) cells, sampled in
  the centre column. Above `swim.enter_fraction` the exclusive **swim layer** takes over: move
  input in the look direction (pitch included), jump = ascend, crouch = descend; the body's
  velocity is pulled towards that wish velocity with exponential `drag`, plus buoyancy
  `buoyancy × (submerged − float_fraction)`, so a player at rest floats with 70 % submerged (eyes
  above water). Below `swim.exit_fraction` it hands back to the normal layers (hysteresis) with the
  current velocity; holding jump at that moment jumps out (onto a bank). The layer holds the other
  layers exactly like the climb layer does, so leaving the water is not read as an external force.
  Water 1 m deep (≤ 0.55 of the capsule) is walked through.

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
- **Riding:** `step_platform` uses the Tier 1 body's linear + angular velocity at the player's
  position; the horizontal layer accelerates in that frame, and the vertical layer follows its
  vertical velocity, so standing on a falling slab or tipping tower needs no input. `yaw_delta`
  turns the camera with rotating bodies (clamped to `max_platform_yaw_speed`).
- **Push strength [planned, Phase 20].** The aggregate pass sets velocity directly, which by itself
  would let an 80 kg player shove a 20-tonne cluster. Contacts between a player and a Tier 1 body
  are to be limited so the effective push force never exceeds `max_push_force`; clusters heavier
  than `pushable_mass_limit` are immovable by players. The player is still fully pushed *by* the
  cluster. (The config fields exist; the contact mechanism on Rapier is built with the clusters.)
- **Crush [planned, Phase 20]:** from the player's contacts. A player with contacts of roughly
  opposing normals (dot < −0.7) where one is a Tier 1 body approaching faster than `crush_speed`,
  sustained for `crush_ticks`, is crushed (damage/death, ARCHITECTURE §9.6).
- **Other players:** both are dynamic bodies. Standing on a head is grounded but not carried unless
  `carried_by_characters` (PPC default: off).
- **Gravity mismatch (intended):** players use the PPC's snappier `gravity` (20 m/s², Source feel);
  Tier 1 bodies use world gravity (9.81 m/s²). A player standing on a free-falling slab falls faster
  than it, so they stay on it instead of floating off.

### 6.7 Creative flight (Dwell addition) **[built, Phase 4]**
A flight mode for the player's own body — server-authoritative and predicted like every other
layer — so a player can rise from the ground to see the whole world (ARCHITECTURE §6.6).
- **Input.** `fly` is a *mode* bit held in every input frame while flight is on (`input_buttons::FLY`);
  the client toggles it (`dwell-game`'s `FlightToggle`: the F key, `input::FLY_KEY`, or the touch
  Fly button; Jump pressed twice is two jumps — owner, 2026-10-15). The server clears the bit for players its flight policy
  excludes (`dwell-server --flight everyone|ops|nobody`, default everyone; `Welcome` says whether
  this player may fly), so a client that sends it anyway just walks.
- **Layer.** `step_fly` runs before crouch, so flying cancels climbing and swimming and stands a
  crouched player up (the exclusive rule of `step_crouch`). While flying there is no gravity: the
  body's velocity is eased towards the wish with exponential `fly.drag` — horizontal from the move
  input along the camera's yaw, up with jump, down with crouch — at
  `max(min(fly.speed × run × (1 + max(0, feet − SEA_LEVEL) / fly.boost_height), cap),
  fly.speed × run × fly_speed_factor(input.fly_speed))` (run = `fly.run_factor` while running, else
  1; cap = `fly.terrain_speed` below `WORLD_MAX_Y`, else none), so speed grows with height and the
  climb is exponential.
  `input.fly_speed` is the **speed slider's level** (0–`FLY_SPEED_MAX_LEVEL` = 39, carried in bits
  4–9 of the input's `buttons`, protocol v9): `fly_speed_factor(L) = 2^(L/2)` — computed with
  `libm::ldexpf` and the correctly rounded `sqrt(2)`, so native and WASM agree exactly — is a true
  minimum speed; level 0 (factor 1) is the height-based speed alone, and level 39
  (2^19.5 ≈ 741,000) stays just under the factor at the ceiling, within the world's velocity
  limit. Below `WORLD_MAX_Y` the height-based speed is capped at `fly.terrain_speed`: collision
  around the player is built a couple of ticks ahead (its reach is capped at 64 m of travel), and
  the server has to generate the chunks being flown through. The slider's minimum overrides the cap
  (a playtest request): faster than about 64 m per tick near the terrain, the player may pass
  through ground whose collision is not built yet, and the view runs ahead of streaming. Collision
  is unchanged — the player body's continuous collision detection (Rapier CCD) keeps a fast dive
  from tunnelling into the ground (`diving_at_the_bands_top_speed_stops_on_the_ground`).
- **Limits.** The feet stop at `fly.ceiling` (`FLIGHT_CEILING`, 24,000 km: from there the 8,192 km
  disc fills about two thirds of the view) and |x|, |z| at `fly.horizontal_limit` (just past the
  rim); the velocity is clamped to arrive exactly, as a tick up there covers hundreds of
  kilometres. Spawning a player raises the physics world's velocity limit (Rapier's default is
  400 m/s) above the fastest flight speed.
- **Leaving flight** hands back to the normal layers with the current velocity, like leaving
  water: the player falls, and a hard landing does fall damage. Landing while flying does not end
  flight (the vertical layer, which reports `LANDED`, is skipped), so flying into the ground is
  harmless.
- **Prediction.** The client normally waits for the terrain around the player before predicting
  (collision needs it); a flying player keeps predicting, since flight outruns chunk streaming and
  the server's collision corrects any difference (`dwell-game`'s `gate::prediction_may_run`).

---

## 7. Configuration

`PlayerControllerConfig` lives in `crates/dwell-physics/src/config.rs` — both sides compile the
same crate, so it needs no mirror. Its fields are grouped in sections (`cfg.movement.walk_speed`).
Presets: `PlayerControllerConfig::default()` (desktop; the server and the client both use it),
`touch()` (auto-jump), `unity_parity()`. Durations are stored as ticks. Defaults follow the PPC's
**recommended feel** (Source engine movement, checked against Halo 3); rows marked ◆ differ from the
PPC default for voxels.

| Section | Field | Dwell default | PPC default | Notes |
|---|---|---|---|---|
| `body` | `radius` ◆ | 0.3 m | 0.5 | Fits 1-wide gaps |
| | `standing_height` ◆ | 1.8 m | 2.0 | Fits 2-tall doorways |
| | `mass` ◆ | 80 kg | 1 | Real mass: voxel clusters use real densities |
| | `gravity` | 20 m/s² | ×2 of 10 | Absolute value instead of a scale |
| | `eye_height` / `crouch_eye_height` ◆ | 1.62 m / 0.8 m | (view) | Client camera |
| `movement` | `walk_speed` / `run_speed` | 5 / 8 m/s | same | |
| | `acceleration` / `deceleration` / `reverse_deceleration` | 50 / 12 / 60 m/s² | same | |
| | `air_control` | 0.2 | same | |
| | `max_step_height` ◆ | 0.55 m | 0.45 | Half steps step up; full blocks need a jump |
| | `air_external_drag` / `ground_external_friction` | 0 /s / 15 m/s² | same | |
| | `carried_by_characters` | false | same | |
| | `auto_jump` ◆ | false (touch preset: true) | — | Dwell addition |
| | `edge_guard` ◆ | false | — | Dwell addition |
| | `max_push_force` ◆ | 800 N | — | Contact limit (Phase 20) |
| | `pushable_mass_limit` ◆ | 400 kg | — | Heavier clusters are immovable by players (Phase 20) |
| `probes` | `ground_probe_margin` / `ceiling_probe_margin` | 0.15 / 0.10 m | same | |
| | `max_slope_angle` | 50° ◆ (45° would hinge on rounding at a 45° face; owner 2026-10-15: steeper than 44° will slide) | same | |
| `jump` | `height` | 1.25 m | same | Clears one block |
| | `buffer_ticks` / `coyote_ticks` ◆ | 12 / 12 ticks (0.2 / 0.2 s) | 0.2 / 0.1 s | Coyote time doubled: 0.1 s felt stingy next to other games |
| `crouch` | `height` ◆ | 0.9 m | 1.0 | Fits 1-tall crawlspaces |
| | `speed` / `mid_air_boost` | 1.6 m/s / 0 | same | |
| `climb` | `speed` / `look_down_threshold` | 3 m/s / 30° | same | |
| | `jump_off_up` / `jump_off_away` / `snap_strength` | 4 m/s / 3 m/s / 10 /s | `jumpOffVelocity` (0, 4, 3) | |
| `swim` ◆ | `speed` / `enter_fraction` / `exit_fraction` | 3 m/s / 0.6 / 0.4 | — | Dwell addition |
| | `buoyancy` / `drag` / `float_fraction` | 12 m/s² / 2 /s / 0.7 | — | |
| `band` ◆ | `boost_gravities` | 2 | — | The flip band (§11) |
| `fly` ◆ | `speed` / `run_factor` / `boost_height` / `drag` | 11 m/s / 2.5 / 32 m / 8 /s | — | Dwell addition (§6.7): speed × max(1 + height above sea / boost_height, 2^(speed level / 2)) |
| | `terrain_speed` / `ceiling` / `horizontal_limit` | 400 m/s / 24,000 km / 8,400 km | — | Below `WORLD_MAX_Y` / feet height / \|x\|, \|z\| |
| `damage` ◆ | `fall_damage_min_speed` / `fall_damage_per_speed` | 12 m/s / 8 per m/s | — | From the `LANDED` event's impact speed |
| | `crush_speed` / `crush_ticks` | 4 m/s / 6 | — | Phase 20 |
| `advanced` | `step_probe_distance` / `probe_ring_radius` / `wall_check_distance` | 0.01 m / 0.9 / 0.16 m | same | |
| | `external_absorb_threshold` | 0.01 m/s | same | Far above native↔WASM noise (none measured, §8.3) |
| | `max_platform_yaw_speed` | 360 °/s | same | |

A **Unity parity** preset (PPC `ApplyUnityParity`, with Dwell's body dimensions) is kept for
comparison testing (`air_drag_decays_an_air_kick_unity_parity`).

---

## 8. Networking: prediction & reconciliation **[built]**

The PPC relies on Quantum's rollback of the whole world. Dwell predicts **only the local player**
against a small client physics world, and the server is authoritative. The prediction logic is
Rust (`Predictor`, `predictor.rs`): natively it backs the predictor and netcode suites, and in the
browser it runs in the client's WASM module, driven by the game loop (`dwell-client`'s `game.rs`:
fixed 60 Hz ticks, input datagrams, reconciliation, the tick-rate nudge).

### 8.1 Client prediction world
Owned by the `Predictor` (the same `Physics` settings as the server):
- static terrain collision from the chunks the client has streamed (the same `TerrainCollision`
  and `VoxelQuery`);
- **kinematic** (velocity-based) capsules for remote players at their **latest snapshot position,
  dead-reckoned by their velocity** until the next snapshot (at most 0.5 s). Measured against the
  alternative of extrapolating to the predicted present: head-on bumps corrected with 0.17–0.23 m
  per-tick rendered steps and no snaps, versus snaps of 1–2 m (an extrapolated kinematic proxy
  shoves the local player, while on the server two equal-mass bodies stop each other). Tier 1
  proxies (present-time within `PREDICT_PROXY_RADIUS`, ARCHITECTURE §9.4) arrive in Phase 20;
- the local player as the only dynamic body, with the server's body settings.

### 8.2 Loop
- **Input:** each client tick samples input, quantizes it once (`dwell_physics::net::quantize_input`,
  called by `dwell-game`'s input: move clamped to the unit circle, `i8` axes, button bits, yaw as a
  wrapped `i16` fraction of a turn, pitch `i16` for ±90°), predicts with the *dequantized* value
  (`dequantize_input`; the server dequantizes the same integers, so both simulate identical input),
  stores `{seq, input, PlayerController, position, velocity, touched}` in a 256-tick ring, and sends
  a `PlayerInput` datagram carrying the newest 4 inputs (`MAX_INPUTS_PER_DATAGRAM`).
- **Server** (`dwell-sim`): inputs queue per session ordered by `inputSeq` (§11 validation; see
  ARCHITECTURE §8.3). A **jitter buffer** consumes one input per tick once 2 are queued; when it
  runs dry the last input repeats and the buffer re-primes; when the queue stays above 6 inputs for
  a whole 30-tick window (its low-water mark), the excess is skipped to bound input latency (a
  slow client's frame of inputs arrives at once and is not skipped; at most 32 are queued). The
  client catches up at most 15 ticks a frame (250 ms; `MAX_TICKS_PER_FRAME`), so a device drawing
  4 fps or more keeps the server's time: one that dropped time would predict fewer ticks than the
  server simulates, the repeated inputs walking it further than the prediction. The snapshot reports the queue length (`input_buffer`); the client nudges its tick rate
  ±2 % to keep it between 1 and 4 (clock drift between client and server otherwise starves or
  floods it).
- **Snapshot (20 Hz):** compared with the stored entry for `ack_input_seq`: position within 1 cm,
  velocity within 5 cm/s, controller flags equal, external velocity within 5 cm/s → nothing to do
  (the common case). Otherwise the server state is restored (resumable controller state from §8.4
  over the stored entry, which supplies the input) and every unacknowledged input is replayed
  (pipeline + prediction-world step).
- **Smoothing:** the visible position is `predicted + offset`; a replay adds `before − after` to
  the offset so the view doesn't jump, and the offset decays with a 0.1 s time constant. A
  correction above `RECONCILE_SNAP_DISTANCE` (1 m) snaps (offset cleared) — except knockback
  corrections and corrections while the player touched a remote player's proxy, which are smoothed
  up to `KNOCKBACK_SNAP_FACTOR` (4) × that distance. A bump into another player, like a knockback,
  is learnt of a round trip late, and Rapier's player-vs-player contacts mispredict by up to
  ~1.3 m; each history entry records whether the local capsule touched (or nearly touched) a proxy.
- **Knockback:** the server applies server-originated velocity changes after the controller pass,
  before the physics step, and sends `PlayerEvent(Knockback)` with the `input_seq` processed that
  tick. The client records it, restores its own predicted state at `input_seq − 1`, and replays
  with the kick inserted after that input's pipeline. When the snapshot overtakes the event (they
  travel on different channels), the snapshot's `last_knockback_seq` tells the client the
  correction is a knockback.
- Replays are cheap: one dynamic body in a near-empty world, ~6–12 ticks at typical RTTs.

**Measured** (`crates/dwell-sim/tests/netcode.rs`: the real server and per-client predictors over
simulated links, two clients; Release):
- 150 ms RTT, 20 ms jitter, 5 % loss: under 1 % of snapshots replay, position error at the
  acknowledged input p95 < 1 mm, no snaps. The residual corrections are one tick of motion
  (≤ 13 cm at run speed) when jitter starves the server's input buffer.
- Server knockback (14 m/s up while walking): no snaps, largest rendered step ~0.6 m per tick.
- Head-on bump: no snaps, largest rendered step ≤ 0.26 m.
- Input takes effect on the next predicted tick.

### 8.3 Divergence budget **[built]**
Native and WASM builds are not assumed bit-identical; the result is **measured**. Rapier is built
with `enhanced-determinism` and the controller takes its transcendental functions from `libm`. The
five-player golden trace (§10) is stored as shortest round-trip floats and must match bit for bit
in the origin frame, natively and under `wasm32-wasip1` (CI runs the deterministic suites, the whole
`dwell-physics` suite among them, with `cargo test --target wasm32-wasip1`, Node's WASI as the
runner, `tools/wasi-run.mjs`): that run is the native↔WASM divergence check. Measured: positions,
velocities and states bit-identical over all 600 ticks, so no divergence eats into
`external_absorb_threshold` (0.01 m/s).

### 8.4 Wire format
- `PlayerInput`: analog move vector (`i8 moveX, moveY`) for gamepads and touch sticks; `jump`,
  `run`, `crouch`, `fly` and the flight speed level in the `u16` button bitfield; quantized yaw and
  pitch (ARCHITECTURE §8.3).
- The local player's snapshot block carries the body state (capsule centre `f64×3`, velocity
  `f32`), flags, health, `State`, `input_buffer`, `last_knockback_seq`, and the controller state
  needed to resume simulation exactly (`ControllerState`, `to_net` / `from_net` in `net.rs`;
  47 bytes, +12 while climbing, +8 after letting go of a ladder): flags (grounded, jumping,
  crouching, climbing, hasReleased, swimming, flying, faceB); horizontal `current`, `external`, and
  `contribution` (x, z); vertical `accumulated_y`, `platform_y`, `target_y`; the ground's vertical
  velocity; the ground reference (kind + player id; Tier 1 grounds go on the wire with Phase 20);
  jump buffer and coyote ticks; step grace; ladder and released cells. Per-tick scratch (inputs,
  events, probe results, climb, swim and fly velocities) is recomputed.
- Remote players receive only feet position (`i32×3` at 1/256 m, `posfix`), velocity (`f16`), view
  angles, `State`, and flags.
- **[built, Phase 3c — protocol v4, ARCHITECTURE §8.3, ADR 0011]** For the 8,192 km world the
  controller runs on double-precision Rapier (`rapier3d-f64`), the local player's capsule centre is
  `pos64` (exact, so reconciliation compares like for like anywhere) and remote feet positions
  `posfix`. Vertical controller fields (`accumulated_y`, `platform_y`, `target_y`) stay `f32`: they
  are velocities and platform heights, not the player's height. `pos64` accepts ±`POS64_LIMIT`
  (33,554 km), covering creative flight up to its ceiling (protocol v7); a flying player's remote
  `posfix` position clamps at ±8,388 km.

---

## 9. Client presentation (from the PPC view layer) **[built, except animation]**

- **Camera** (`dwell-client`'s `game.rs`, eye height in `dwell-game`'s `eye.rs`): first-person at
  the smoothed render position (interpolated between the last two ticks plus the correction
  offset). **Field of view** (`dwell-lod`'s `base_fov`, re-exported by `dwell-client`'s
  `render/camera.rs`): the player's Field of view setting (default 75° vertical, 60–100°); screens
  wider than ≈ 1.55 get a narrower vertical one so the horizontal stays what the setting gives at
  that aspect (100° at the default). An uncapped wide view made forward and backward walking look faster than strafing
  at the same speed (playtest); the sim itself moves at the same speed in every direction
  (`every_direction_moves_at_the_same_speed`). **Motion effects** (Phase 29,
  [`MOTION_EFFECTS.md`](./MOTION_EFFECTS.md)): the vertical FOV widens with the camera's speed
  (the predicted velocity, so falls and knockback count; scaled by the controller state's gain) —
  eased from 0 at `walk_speed` to the sprint boost at `run_speed`, then log-scaled up to the flight
  boost, as a zoom-out percentage of the base (+10 % / +20 %, `2·atan(tan(base/2)·(1 + boost))`) —
  smoothed by a critically damped spring and clamped to 120°; wind
  streaks blow past the screen's edges. They read the controller's state and never write it, so
  prediction stays deterministic; they reset on a teleport, respawn and the turn-over across the
  midplane, and hold while the menu is open.
  The eye height is computed once per tick from the feet (never the capsule centre, whose height
  changes with crouching) and interpolated between ticks. The sim moves the body in jumps the
  camera must not show, and each is folded into an offset that decays:
  - **Steps** (PPC `SmoothSteps`, extended to both directions): a grounded feet change of 0.1 m up
    to `max_step_height` in one tick (the step-up lift, or the ground snap down a stair) decays at
    4 m/s plus 6/s of the remaining offset, so it never lags more than one step.
  - **Crouch**: the sim's crouch is instant, and in mid-air it moves the feet by the height
    difference (the head stays put). The eye change on the crouch tick, less the tick's own
    vertical motion, decays at 12/s.
  - Offsets beyond a step (or 2 m for crouch) are teleports and are not smoothed.

  `platform.yaw_delta` turns the camera with rotating ground. Dwell's world is **right-handed, Y
  up**: yaw 0 looks along +Z, and right of +Z is −X (the PPC's Unity convention is left-handed); the
  controller's camera-right vector (`camera_right`) follows this.
- **Players:** remote players are capsules with a visor, interpolated 100 ms in the past
  (`dwell-game`'s `remotes.rs`, `INTERP_DELAY_MS`); dead players are drawn grey and lying on the ground (`dwell-client`'s
  `render/geometry.rs`; the
  physics ragdoll moves to Phase 21 with the client debris world). While dead, the camera looks
  down at the body from behind and above, turning around it with the look yaw, until respawn.
- **Animation:** not yet — there are no character models. The parameters listed by the PPC
  (`Speed`, `IsGrounded`, … from `State`, velocity, and flags) are all available client-side.
- **Debug overlay** (F3): state, ground ref and gap, wall/ceiling/submersion, position, velocity,
  layer velocities (current, external, target), prediction stats (snapshots, replays, snaps,
  knockback replays, error at ack, last correction, smoothing offset), server input buffer, tick
  rate, RTT; probe rays (ground ring, walls) and the velocity vector drawn in 3D.
- **Network condition simulator:** `?netsim=<rtt ms>,<jitter ms>,<loss %>` wraps the transport
  (`dwell-client`'s `net/netsim.rs`).

---

## 10. Testing (porting the PPC test suite)

**Built:** the PPC's headless Quantum tests are ported to Rust in `crates/dwell-physics/tests`,
using a `PlayerTestWorld` builder (`tests/common/mod.rs`) with voxel primitives (floor, block
steps, half steps (static Tier 1 boxes), walls, 1×2 doorways, 1-tall crawlspaces, ladder columns,
water pools), Tier 1 boxes (static, kinematic or dynamic) as platforms and ramps, scheduled kicks
and explosions, and per-player event counters. Expectations use Dwell's default config
(recommended feel, voxel capsule). The **movement playground** — a block step, a doorway, a
crawlspace, a ladder to a ledge and a pool near the spawn — is a structure file
(`shared/structures/playground.json`) stamped over the flat world by the netcode suite
(Phase 15; it was generator version 1 with slab stairs, slope features and a launch pad). The
launch-pad knockback test is a server-issued knockback.

| PPC suite | Dwell equivalent |
|---|---|
| Phase1Skeleton | `movement.rs`: the body's settings, stands still without drift or tipping; a heavy cluster pushes the player |
| Phase2Movement | `movement.rs`: probes, speeds, accel/decel/reverse, walls, no diagonal speed-up, step-up on half steps, blocked by full blocks, air control, external absorption and decay |
| Phase3Vertical | `vertical_crouch.rs`: fall and land event, jump apex, buffer, coyote, no double jump, ceiling, launch, a 20° ramp |
| Phase4Crouch | `vertical_crouch.rs`: feet planted, crouch into a crawlspace, blocked stand-up, mid-air tuck |
| Phase5Platform | `platform_climb_swim.rs`: riding translating, rotating, elevating and dynamic Tier 1 bodies; jump-off keeps momentum; explosion |
| Phase6Climb | `platform_climb_swim.rs`: ladder columns: grab, climb, look-down reversal, strafe, jump-off, climb over the top |
| CharacterStacking / GroundedConsistency / StepSmoothness | `scenario.rs`: the same scenarios on voxel geometry (standing on heads, half-step stairs, stair corners, ledges) |
| GoldenTrace | `scenario.rs`: the five-player scenario is identical across repeated runs and matches `tests/golden/scenario.trace` bit for bit in the origin frame (natively and under `wasm32-wasip1`, §8.3); far from the origin and on face B every state must match and positions stay within 1 mm. Re-record with `DWELL_UPDATE_GOLDEN=1` (origin frame only); re-recorded on Rapier in Phase 24 |
| — (Dwell) | Swim enter/float/dive/exit and shallow water (`platform_climb_swim.rs`); creative flight (`flight.rs`: hover, rise/sink/move, speed growing with height, the terrain-band cap and the ceiling, the speed level, flying round the rim to face B, a fast dive stopping on the ground); auto-jump, edge guard, doorways, crawlspaces, block-under-feet removal, same-tick collision with a placed block, chunk seams (`scenario.rs`); voxel ray casts and flooded cells (`voxel_query.rs`); the bifacial controller (`bifacial.rs`) and Tier 1 bodies across the midplane (`tier1_bifacial.rs`); the predictor against an in-process server (`tests/predictor.rs`: no replays without surprises, knockback replays, the render offset decaying, death, remote proxies). Networked players (`crates/dwell-sim/tests/netcode.rs`, the `NetSim` harness: the real server and per-client predictors over simulated links) — input validation and rate limits, fall damage, death and respawn on every client, reconciliation under latency, jitter and loss, knockback replay, player bumps, block edits, the server's flight policy and fast flight. Later phases: crush and push-force cap (Phase 20) |

Performance gate: 64 players' controller passes (excluding the physics step) under 1 ms/tick —
measured at ~0.48 ms in a Release build with `enhanced-determinism` on, ~0.51 ms ~8,000 km from the
origin; checked by `sixty_four_players_controller_passes_cost_under_1_ms_per_tick` (`scenario.rs`;
strict in optimised builds, 25 ms in unoptimised ones; a release step of the CI `rust` job).

**Far from the origin** (ADR 0011): every test describes its world in a local frame that
`tests/common/mod.rs` places in the world; `DWELL_ORIGIN_X=far` puts it 7 999 488 m east, and the
whole `dwell-physics` suite — the golden trace and the performance gate included — and the netcode
suite pass there unchanged (CI runs both; the `wasm32-wasip1` run is in the origin frame).
World-level tests cover the rest: walking off the rim of the disc falls toward the midplane, not
into a void (`crates/dwell-sim/tests/streaming.rs`).

## 11. The bifacial world (Phase 13) **[built]**

([ADR 0023](./adr/0023-bifacial-world.md), [`BIFACIAL_WORLD.md`](./BIFACIAL_WORLD.md) §3, §6.) Down is
always toward the midplane (y = −2,048), so a face-B player stands upside down.

- **The face-local frame** (`frame.rs`). The controller is written for an upright player.
  `PlayerController::face` (±1) says which side the body is on; for face B every pass runs in the
  *mirror image* of the world — heights y → −4,096 − y (voxel rows y → −4,097 − y, the frame the
  face's terrain is generated in), vertical velocities and contact normals negated, shaped blocks
  turned over (`dwell_blocks::mirror`) — where gravity points to −y as ever. `Frame` maps positions,
  velocities, rows and states, and `FaceQuery` (the voxel queries) and the passes' private
  `pos`/`vel`/`set_pos`/`set_vel` cross the boundary; on face A the maps are the identity, so face A
  plays exactly as the upright controller. `Players::position`, `velocity`, `feet` and `head` stay in
  the world (`feet` is above `head` on face B). The face switches (in `step_input`) when the body
  centre is more than 5 cm across the midplane; the layers' vertical state is mirrored with it. An
  upside-down player's right is the frame's left, so `move_x` is negated on face B (the heading
  `yaw` is a world azimuth and unchanged as far as the controller goes; the client turns its mouse
  the other way, and turns its heading by 180° when its face changes, so the server only sees the
  inputs).
- **The flip band.** In open air within `FLIP_BAND` (4 m) of the midplane a player moves as when
  swimming (the swim layer), without buoyancy or drag. Moving toward the midplane it is accelerated
  by `band.boost_gravities` (2) × gravity, so it is carried across; moving away, the midplane's pull
  (`gravity · h / band`) fades to zero at it. The player's own jump/crouch/look input steers as in
  water. Nothing slows the body, so it swings across the midplane under gravity. Dynamic Tier 1
  bodies get the same gravity by side and band from `Physics::step` (`tier1_bifacial.rs`).
- **Crossing.** Digging through the core or going over the rim brings a player to the band; on the
  far side "down" points back, so the player presses jump (up, away from the midplane) to go on.
  `controller_flags::FACE_B` / `player_flags::FACE_B` (protocol v12) carry the face.
- **Mirror equivalence.** `DWELL_FACE=b cargo nextest run -p dwell-physics` puts the controller
  suites' local frame on face B (`tests/common/mod.rs`: geometry, spawns, velocities, inputs and
  rotations mirrored) and every case holds (CI runs it; the two `voxel_query.rs` ray-cast tests,
  which test world-frame geometry, are skipped). Controller decisions are identical: the five-player
  golden trace matches every state on face B with positions within 1 mm.
- **Camera and look** (§9): eye height is measured along the player's up; the view's pitch is toward
  that up; the camera turns over when the face changes: the heading turns by 180° and the view is
  drawn rotated about the camera's right axis by π, easing to 0 over 0.5 s (a half forward
  somersault, nose first; `dwell-game`'s `face_flip.rs`, applied by `dwell-client`'s
  `render/camera.rs`). In the flip band a body can swing across the midplane and back, and each
  crossing turns the heading round again (no hysteresis).
