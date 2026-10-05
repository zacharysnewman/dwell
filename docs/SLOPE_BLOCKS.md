# Dwell — Slope Blocks: Shapes, Collision, Building, Terrain and LOD

> **Status: [planned]** — the design for implementation **Phase 10**
> ([`IMPLEMENTATION_PLAN.md`](./IMPLEMENTATION_PLAN.md)). As the phase lands, the built mechanisms
> move into [`ARCHITECTURE.md`](./ARCHITECTURE.md) (§6.1 voxel shapes and the material table, §6.3
> generation, §6.5 building, §6.6 LOD, §8.3 messages) and [`PLAYER_CONTROLLER.md`](./PLAYER_CONTROLLER.md)
> (§6.2 blocks, steps, slopes), and this file keeps the rationale and the shape tables.

Slopes touch nearly every system: the voxel format, the material table and its TypeScript mirror,
the chunk and LOD meshers, terrain collision and the player controller, prediction parity, block
editing and the creative palette, the terrain generator, and later the physics clusters (Phases
11–13). That is why it is its own phase.

## 1. The geometry

### 1.1 Angles (checked)

A slope's angle is `arctan(rise / run)`:

| Slope | Rise | Run | Angle of the walkable face |
|---|---|---|---|
| Standard (1 tall, 1 long) | 1 | 1 | **45°** |
| Gentle (1 tall, 2 long) | 1 | 2 | **26.565°** |

At a corner, two straight slopes meet along a diagonal line. The **hip** (outer corner) or
**valley** (inner corner) line runs across the footprint's diagonal, so its angle is lower:

| Corner line | Rise | Run | Angle of the hip / valley line |
|---|---|---|---|
| Standard corner (1 × 1) | 1 | √2 ≈ 1.414 | **35.264°** (arctan(1/√2), the "magic angle") |
| Gentle corner (2 × 2) | 1 | 2√2 ≈ 2.828 | **19.471°** (arctan(1/(2√2))) |

These are correct, with one clarification that matters for walking and collision: **the faces of
a corner piece keep the pitch of their straight slopes** (45° or 26.565°); only the line where the
two faces meet is at 35.26° or 19.47°. So a player on any piece of the standard family stands on a
45° face, and on the gentle family a 26.57° face.

**Rejected: the tetrahedral corner** (a sloped triangle through three cube corners, as in some
games' "corner" blocks). Its face is the plane x + y + z = 1, at arctan(√2) ≈ **54.74°** — steeper
than the standard slope and unwalkable. Dwell's corners are hip/valley pieces, so every walkable
face in the set is 45° or 26.57°.

### 1.2 The shape set

Every piece fits in one 1 m cell and is described by the heights of its top surface at the cell's
four corners (fractions of the cell; the surface between them is made of planar faces). Canonical
orientation: the piece descends toward +X (east) and, for corners, toward +X and +Z (south-east);
corner heights are listed as (NW, NE, SE, SW), north = −Z.

| # | Shape | Corner heights | Top surface z(x, y) on [0,1]² | Volume | Convex |
|---|---|---|---|---|---|
| — | Full cube (exists) | 1, 1, 1, 1 | 1 | 1 | yes |
| — | Bottom slab (exists) | ½, ½, ½, ½ | ½ | ½ | yes |
| 1 | Wedge | 1, 0, 0, 1 | 1 − x | ½ | yes |
| 2 | Outer corner (hip) | 1, 0, 0, 0 | 1 − max(x, y) | ⅓ | yes |
| 3 | Inner corner (valley) | 1, 1, 0, 1 | 1 − min(x, y) | ⅔ | **no** |
| 4 | Gentle wedge, low | ½, 0, 0, ½ | ½(1 − x) | ¼ | yes |
| 5 | Gentle wedge, high | 1, ½, ½, 1 | 1 − ½x | ¾ | yes |
| 6 | Gentle outer corner, low | ½, 0, 0, 0 | ½(1 − max(x, y)) | ⅙ | yes |
| 7 | Gentle outer corner, high | 1, ½, ½, ½ | 1 − ½ max(x, y) | ⅔ | yes |
| 8 | Gentle inner corner, low | ½, ½, 0, ½ | ½(1 − min(x, y)) | ⅓ | **no** |
| 9 | Gentle inner corner, high | 1, 1, ½, 1 | 1 − ½ min(x, y) | ⅚ | **no** |

(In the formulas x runs east and y south across the cell; heights are in cell units.)

How pieces combine:
- A **standard slope** is one wedge per cell. A **gentle slope** is a low wedge then a high wedge
  (two cells per metre of rise).
- A **standard corner** is one outer (or inner) corner cell. A **gentle corner** spans 2 × 2 cells:
  for an outer corner, the high outer corner in the top cell, a low gentle wedge in each of the two
  side cells, and the low outer corner in the far cell — together the surface `1 − ½ max(x, y)`
  over [0, 2]²; an inner corner likewise from `1 − ½ min(x, y)`: high inner corner, two **high**
  gentle wedges, low inner corner.
- **Orientations:** 4 yaw rotations of each shape, and an **inverted** (ceiling) variant of each
  for building roofs and overhangs: 9 shapes × 4 × 2 = **72 shape variants**, plus the existing
  full cube and bottom slab (a top slab is a natural addition).
- Mixed runs (1:1 one way, 1:2 the other) and saddles (heights 1, 0, 1, 0) are not in the set;
  the terrain rule in §5 maps them to the nearest piece.

Inner corners are not convex: wherever a convex shape is needed (Tier 1 clusters, Phase 11) they
split into two convex wedges. Terrain collision is a triangle mesh and does not care.

## 2. Representation: block states in the existing `u16` voxel

Today a voxel is a `u16` material id, and each material has one fixed shape (`VoxelShape`:
`kEmpty`, `kFull`, `kSlabBottom`); direction is encoded by separate ids (`ladder_n/e/s/w`).
Slopes need (material, shape, orientation) per voxel.

**Options considered** (decided by Phase 10's ADR):

- **Block states (recommended).** The `u16` id becomes an index into a **state table** generated
  from (material × allowed shape variant). Materials flagged `shapeable` (stone, dirt, grass,
  sand, sandstone, gravel, snow, log, …) get the 72 variants plus slabs; the others keep one state.
  Ids are assigned by a fixed formula in a reserved range (e.g. `SHAPED_BASE + slot × 80 +
  variant`, slots appended as materials are added), so **every existing id keeps its value** and
  saved worlds need no migration. Chunk palettes, run-length encoding, the wire and the world file
  are unchanged; a chunk with many shapes just has a longer palette (u16 palette indices above 256
  entries already exist). ~10 shapeable materials ≈ 800 states, far under 65 536.
- **A separate shape layer** (a second per-voxel byte). Cleaner in principle, but it changes the
  chunk encoding, the wire, the world file, the LOD cells, every voxel accessor and the golden
  vectors. Rejected unless the state count ever grows past the id space.

Per state, the table gives what the systems need: base material (for textures, mining drops,
density), shape and orientation, collision triangles, face coverage for culling (§3.1), volume
(mass for Phase 11), and `Placeable`. The C++ table is generated from the formula, and the
TypeScript mirror is generated the same way and checked by the existing mirror test. The protocol
version is bumped because both sides must agree on the state table.

Point queries that read a voxel's shape (`ShapeHeight`, the controller's ground probes, spawn,
block-placement fit checks) gain `SurfaceHeightAt(state, fx, fz)` — the piecewise-planar top
surface above — with the exact same arithmetic on both sides (ADR 0010 rules: `+ − × /`, `min`,
`max`).

## 3. Rendering

### 3.1 Chunk meshing

- Each shape declares, for each of its six cell faces, its **coverage**: full, none, bottom half,
  or one of the four right triangles. A face is culled only when the neighbour's opposite face
  covers it completely (a full face covers anything; a wedge's triangular side is covered by the
  mirrored triangle of a matching neighbour wedge). Sloped faces are never culled.
- Sloped faces are emitted as triangles or quads with their true normals; cube faces keep the
  greedy merge, and runs of identical wedges along their ridge axis may be merged later.
- Textures: sloped faces use the material's **top** tile (grass on a grass slope), projected along
  the slope's dominant axis so texels are not stretched badly; triangular sides use the side tile.
- Shading: Phase 6's per-face tint table is keyed by axis; slopes use the same tint interpolated
  by the face normal (`normal.y` between top and side values), in the shared module both meshers
  import.
- Water next to a slope: water faces against a slope's open part are drawn (the slope does not
  cover them). Slopes **under water** need a "waterlogged" state (water in the air part of the
  cell); until then the generator places no slopes in cells that would hold water (§5) and edits
  that place a slope into water are refused or displace the water (decide in the ADR).

### 3.2 LOD

The LOD's biggest visual gain: distant terrain stops looking like stacked cubes.

- **No format change.** LOD sections keep storing materials per cell. The **LOD mesher derives
  shapes** from surface heights at mesh time, with the same corner-height rule as the generator
  (§5), scaled to the cell size: generated sections already carry each column's exact surface
  (`core::LodSurfaces`); downsampled (modified) sections use the top filled cell of each column.
  Corner heights are the average of the four columns meeting at the corner, quantised to half a
  cell, then matched to a piece.
- Applies at every level, so mountains seen from 50 km read as faceted slopes rather than
  terraces.
- The chunk/LOD boundary keeps the existing two-pass depth split; both sides approximate the same
  surface, so the seam stays small. A test checks the LOD slope surface stays within half a cell
  of the true surface at sampled sites.

## 4. Collision and the player controller

- **Terrain collision** (`terrain_collision.h`): the per-chunk `MeshShape` gains the sloped
  triangles and triangular side faces; Jolt's internal edge removal works within a region body as
  today. Server and client build the identical mesh from the identical chunk data (prediction
  parity).
- **`maxSlopeAngle` is exactly 45° today** (PLAYER_CONTROLLER.md §6.2), the pitch of a standard
  slope, so walkability would hinge on rounding. Raise it to ~50° (a tuning change with a test
  that fails on the old value: walking up a standard slope without jumping).
- **Walking:** up and down standard and gentle slopes, across hips and valleys, and from slope to
  flat and back without hops, bounces, sliding at rest, or speed gain (the "slope launch" problem
  PPC already guards against; ground snapping downward must hold the player to a descending 45°
  slope). Step-up keeps working where slopes meet slabs and blocks; auto-jump does not fire on a
  slope that can be walked.
- **Regression suite:** new controller scenarios (a slope playground in the playground generator:
  standard and gentle ramps, outer and inner corners, a gentle 2 × 2 corner, a slope ending in a
  wall, a slope under a low ceiling for crouching), run natively and in WASM, at the origin and
  ~8,000 km out, plus the native↔WASM divergence check and the golden trace (regenerated only
  if a scenario intentionally changes).

## 5. Terrain generation: shaping the surface

The generator knows the continuous surface height `h(x, z)` (base height plus noise) before it
rounds it to voxels. Slopes are chosen from it **per cell, from corner heights**, so neighbouring
cells agree without reading each other (no neighbour reads, §6.3):

1. For a column's top voxel whose surface is a plain heightfield locally (no overhang noise or
   cave air within the cell's neighbourhood, above any water), take `h` at the cell's four corners
   (corner points are shared by the four cells around them, so adjacent cells see the same
   numbers).
2. Relative to the cell's floor, quantise each corner height to {0, ½, 1} (below → the cell is
   air; above → a full cube, and the cell above is considered instead).
3. Match the four quantised heights to the shape table (§1.2, in any of 4 orientations): full,
   slab, wedge, gentle low/high wedge, outer/inner corners of either family. Combinations not in
   the set (saddles, mixed runs) map to the nearest piece by a fixed rule (e.g. raise the lowest
   corner), so the result is still deterministic.
4. The material is the surface material the column already chooses (grass, sand, snow, …) in its
   shaped state.

Gentle pieces appear on gentle ground and standard pieces on slopes near 45°; anything steeper
stays a cliff of cubes with a shaped lip. Slopes are a generator stage like the others, so this is
a **generator version bump** with regenerated goldens; the point queries (`SolidAt`, `GroundY`)
and `GenerateLod`'s surfaces follow the same rule. Islands (Phase 9) and cave floors keep cubes
at first; extending the rule to them is a later tuning step.

## 6. Building

- **Palette:** the creative palette offers each shapeable material's shapes (a shape selector
  next to the material; a key and a touch button cycle shapes).
- **Orientation from context:** yaw from the player's facing (the slope rises away from the
  player, like stairs), inverted when placing against a ceiling or the upper half of a side face.
  A preview outline shows the exact shape before placing.
- **Validation** (`CheckBlockEdit`): the placed shape must not overlap players — tested against
  the shape's true volume, not the cell — and must be a `Placeable` state. Breaking a shaped
  block gives its base material.
- **Optional tools** (later): a "smooth" brush that applies the terrain rule (§5) to a selected
  area of player-built blocks.

## 7. Later phases

- **Phase 11 (voxel awakening):** integrity treats any two solid voxels sharing a face as connected
  (partial coverage included, so a slope supports what sits on it). Cluster bodies use one convex
  shape per voxel from the state table (inner corners as two wedges), and mass from density ×
  volume.
- **Phases 12–13:** debris and re-baking snap to the 24 orientations; a shaped voxel re-bakes into
  the state whose orientation matches, or as a full block when none does.

## 8. How Phase 10 is checked

- Shape table: the corner heights, volumes, coverage and convexity of every variant computed from
  its triangles and compared to §1.2; the C++ and TypeScript state tables identical; every existing
  material id unchanged (a saved golden world file loads identically).
- Meshing: no holes or z-fighting between any pair of adjacent shapes (an exhaustive test over all
  shape pairs on all six sides: every surface point is covered by exactly one face); sloped faces
  lit by normal.
- Controller: the slope scenarios above, natively and in WASM, both origins; the `maxSlopeAngle`
  test fails on 45° and passes on the new value.
- Terrain: on sampled sites the shaped surface is within half a block of `h`, adjacent cells'
  shared corners agree, no slopes in cells holding water; determinism goldens.
- LOD: the LOD slope surface within half a cell of the true surface; frame time within budget.
- Building: placing every shape in every orientation (e2e, like the existing "break and place every
  palette block" test), seen identically by a second client.
- Manual: walking a sloped landscape and building a sloped roof, reviewed by the owner.
