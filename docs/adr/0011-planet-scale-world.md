# 0011. Planet-scale world: an 8,192 km disc, 8,192 m tall, with double-precision physics

- Status: Accepted
- Date: 2026-09-29
- Resolves: ARCHITECTURE.md Open Decisions #7

## Context

The world was bounded to ±65 536 m horizontally and 512 m vertically (−128 to 384), sized so that
single-precision Jolt stays within ~8 mm of precision. The game now needs a much larger world:

- **Horizontal:** a disc of radius **8,192 km** (8 192 000 m), about 2.108 × 10⁸ km².
- **Vertical:** **8,192 m** in total, three quarters above sea level and one quarter below.
- **Edge:** past the disc's rim the world drops into the void (to be developed later).
- **Whole-world view:** everything that should be visible stays visible at a reduced level of
  detail, even from high up (ADR 0012).

At 8,192 km a float32 has a resolution of 0.5 m (values in [2²², 2²³) step by 0.5), so
single-precision physics, float32 wire positions, and float32 noise coordinates all break down
long before the rim. The full-detail world is also far too large to generate or store:
2.1 × 10¹⁴ m² / 32² ≈ 2 × 10¹¹ chunk columns × 256 rows ≈ 5 × 10¹³ chunks.

## Options considered

- **Keep single precision; cap the world.** Rejected: it does not meet the requirement.
- **Single precision with per-region physics worlds and origin rebasing.** Each populated area
  gets its own Jolt world near a local origin. Keeps float speed, but players, bodies and
  collision crossing region borders need hand-off logic, and players far apart need separate
  worlds anyway — complexity in the most delicate code (prediction, reconciliation).
- **Jolt `JPH_DOUBLE_PRECISION`.** Body positions (`RVec3`) become doubles; shapes, velocities and
  most internal math stay float relative to the body. One world, no hand-off; a modest cost in
  memory and speed. WebAssembly has native f64, so the WASM sim core pays the same small cost.

For the wire:

- **f64 everywhere.** Exact, but doubles the size of every position (24 bytes).
- **Fixed point `i32` at 1/256 m.** Covers ±8 388 608 m — just past the 8 192 000 m radius — with a
  uniform 3.9 mm step: the same 12 bytes as today's f32×3, and more precise than f32 already was
  at 65 km. Not exact enough for reconciling the local player's own state.

## Decision

1. **World bounds.** `WORLD_RADIUS` = 8 192 000 m (a disc centred on the origin, replacing
   `WORLD_HALF_EXTENT`); `WORLD_MIN_Y` = −2 048, `WORLD_MAX_Y` = 6 144 (256 chunk rows),
   `SEA_LEVEL` = 0. Columns outside the disc generate nothing — not even bedrock — so the rim is a
   drop into the existing void below `WORLD_MIN_Y`, which kills.
2. **Physics.** Jolt is built with `JPH_DOUBLE_PRECISION` on the server and in every WASM build of
   the sim core. Game code uses `RVec3` for world positions; client rendering stays
   camera-relative (JavaScript numbers are already doubles).
3. **Wire positions (protocol v4).** The local player's own state in `PhysicsSnapshot`, and
   positions that seed prediction (`PlayerEvent` Respawn), are **f64×3**. Remote players, Tier 1
   entities and `PhysicsEvent` origins are **fixed point `i32×3` at 1/256 m**
   (`POSITION_FIXED_SCALE` = 256). Velocities stay f32/f16.
4. **Worldgen coordinates.** Noise keeps ADR 0010's strict IEEE float, but a world coordinate is
   never converted to float whole. Each octave splits the integer voxel coordinate into an integer
   lattice cell (integer arithmetic, feeding the hash) and a float offset within the cell (small,
   so exact enough); frequencies whose lattice spacing is not a power of two scale the integer
   part in 64-bit integers first. The golden test gains chunks near the rim. This is a generator
   version bump (version 3).
5. **Vertical streaming.** With 256 rows, the per-player view becomes a sphere of chunks, and
   chunks the generator can prove are all air (above the column's bounded surface) and that are
   unmodified are neither generated nor sent: the client already reads missing chunks as air.
6. **Distant terrain** is never produced by generating full-detail chunks; it comes from the LOD
   system (ADR 0012).

## Consequences

- One physics world for the whole disc; players can be anywhere. Jolt's double-precision build is
  slower; Phase 3c measures the controller and 64-player benchmarks (native and WASM) against the
  Phase 2 numbers before switching the default.
- Protocol v4 changes snapshot, event and (later) entity layouts; golden vectors are regenerated
  from the Python reference encoder.
- `ChunkCoord` and voxel coordinates stay `int32` (the rim is at chunk 256 000, voxel 8 192 000).
- The generator needs variation at planet scale (hundreds to thousands of kilometres) and
  kilometre-scale relief, or 16 000 km of the same noise will look the same everywhere. Phase 3c
  adds a placeholder version; like all current materials and terrain style it is prototype
  content (ARCHITECTURE.md §6.1), and its numbers are generator tunables, not architecture.
- Reversal: dropping back to single precision would require re-capping the world or moving to
  per-region physics worlds; the wire format would not need to change again.

## Implementation notes (2026-09-29, Phase 3c)

Built as decided, with two refinements found while implementing:

- **Terrain collision regions with anchors.** Point 2's double-precision bodies still hold terrain
  as float sub-shape offsets, so terrain collision is split into region bodies (2 048 m), each at
  its region's centre. Splitting chunks between bodies by position put a seam at every region
  border that bumped a running capsule 5.6 cm (Jolt's internal edge removal works within one body).
  Instead, each region's body holds the chunks around the players *anchored* to it, wherever they
  lie (chunk meshes are shared); a Jolt group filter lets a character collide only with its
  anchor's body, and a player re-anchors, with 8 chunks of hysteresis, well inside the next region,
  where both bodies hold the ground. There is no seam under a player (PLAYER_CONTROLLER.md §5).
  Jolt is built with RTTI (`CPP_RTTI_ENABLED`) so Dwell can subclass its `GroupFilter`.
- **Air chunks are sent, without payload.** Point 5 said unmodified all-air chunks would be neither
  generated nor sent. The client, though, waits for the chunks around the player before it
  predicts, so it must know which ones exist and are empty. They travel as a payload-free `Air`
  form of `ChunkData` (18 bytes, like `Generated`); neither side generates, stores or meshes them.
- **Measured.** 64 players' controller passes: 0.43 ms/tick before, 0.42 ms with double precision,
  0.48 ms ~8,000 km from the origin (Release). Chunk generation: ~1.2 ms native (Release), ~1.5 ms
  in WASM, the same anywhere in the world. The player, netcode and golden-trace suites pass at the
  origin and ~8,000 km out, natively and in WASM; the golden trace was regenerated once, for a
  borderline crouch fit that double precision resolves the other way.
