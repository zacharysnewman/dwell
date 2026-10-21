# Dwell — Adaptive Natural Blocks: Shapes From Neighbours

> **Status: superseded (2026-10-20) by Phase 41's ½ m cells ([ADR 0042](./adr/0042-half-height-cells.md),
> [`HALF_HEIGHT_GRID.md`](./HALF_HEIGHT_GRID.md)): the adaptive blocks were removed; this file is kept as
> the record of the design.** Was: **[built, Phase 18]** (formerly 9e) — design agreed with the owner in prototypes on
> 2026-10-07/08 ([`prototypes/`](./prototypes/README.md)); decision in
> [ADR 0024](./adr/0024-adaptive-natural-blocks.md) (as built). The built mechanisms are described
> in [`ARCHITECTURE.md`](./ARCHITECTURE.md) — §6.1 (the flag, the table as data, the derivation,
> shapes, terrain collision), §6.3 (spawn on the shaped surface), §6.5 (targeting), §6.6 (the rule
> on LOD cells) — and [`PLAYER_CONTROLLER.md`](./PLAYER_CONTROLLER.md) §5 and §6.2 (collision,
> walking; steeper than 44° slides, owner 2026-10-15). This file keeps the rationale and the tables, as
> [`SLOPE_BLOCKS.md`](./SLOPE_BLOCKS.md) does for Phase 9. Outstanding (Phase 18 in
> `IMPLEMENTATION_PLAN.md`): the two-client e2e, the playground's override export, the owner's walk.

Natural blocks **shape themselves**: a grass, dirt, stone or sand block takes its shape from the
columns around it, so touching natural blocks always join into one clean surface, and a player never
places or rotates a slope. Which materials are natural is block data (`"adaptive": true` in
`shared/blocks/dwell.json`), not code; the content is prototype (`ARCHITECTURE.md` §6.1). The
sandbox floor, water and the ladder are not adaptive: a flat build surface stays cubes.

## 1. What the player sees

- **A natural block with another block on top is a full cube.** Only the top block of a column
  is shaped, and only its upper surface: undersides, cave ceilings and overhangs stay cubes.
- **An exposed natural block rests at half height.** A flat field one block deep is a surface
  ½ m up; two blocks deep, 1½ m (a full cube with a half-height top).
- **Edges slope gently to the ground**: ½ m over a whole block (about 27°), flush with what is
  beside them.
- **A 1-block step is one even slope across both blocks**: the lower block rises to its top at the
  step, the upper block sinks to its floor, ½ m per block — walked up without jumping.
- **One-block-wide shapes are half-height ridges**: a lone block is a half pyramid, a row a ridge
  with end caps, an L a ridge corner, plus T and cross pieces.
- **A one-wide block against a higher one is a full 45° ramp up to it** (stairs of single
  blocks are a run of ramps). **Owner decision (2026-10-15): faces steeper than 44° are
  slides** — 45° ramps and ridge flanks and the saddles' 54.74° faces make the player slide
  ([`PLAYER_CONTROLLER.md`](./PLAYER_CONTROLLER.md) §6.8), so single-block stairs are no longer
  walked up; a stairs block will come later. The shapes stay as designed.
- Steeper ground (2 blocks or more between neighbours) is a cliff: the shaped top blocks meet the
  full cubes below, and the cubes' sides are the cliff face. Where a block sits diagonally below
  another, its side shows between them; that is expected, not a gap.
- **Shores:** a shaped top block with water above it or beside it in its row holds water in its open
  part (flooded), so the water meets the bank without a wall of water inside it.

## 2. The rules

Every top block of an adaptive material is shaped by five rules, worked out from the owner's
hand-shaped examples in the Shape Playground: they reproduce all 151 blocks the owner shaped
(situations A–P; `shared/blocks/adaptive.test.mjs` and `crates/dwell-physics/tests/adaptive.rs`
check it), and the owner approved what they make of 12 more situations (Q–AB) that exercise every
shape they can produce — 250 blocks in 28 situations, no conflicts (§6).

Heights are compared column by column: for the block at `(x, y, z)` and each of its 8 horizontal
neighbours, the neighbour is **higher** if it is filled at `y + 1`, **lower** if it is not filled at
`y`, and **the same** otherwise. The shape is given by its four **corner heights** in halves of a
block above the block's floor: 0 (floor), 1 (half), 2 (top). Each corner looks at the 3 neighbours
around it (the two edge neighbours and the diagonal):

1. **Rest.** None of the 3 is higher or lower: the corner sits at half height (1).
2. **Drop.** Any of the 3 is lower: the corner drops to the floor (0).
3. **Rise.** One is higher and none is lower: the corner rises to the top (2).

When rules 1–3 drop **every** corner, the block is one block wide (or alone), and the whole block
is chosen from its 4 edge neighbours instead:

4. **One block wide.** A half-height ridge runs from the centre toward each edge neighbour of the
   same height: none → half pyramid; one → ridge end; two in line → ridge; two at a right angle →
   ridge corner; three → ridge T; four → ridge cross.
5. **Ramp up.** If exactly one edge neighbour is higher and none is the same height, the block is
   a wedge (full 45° ramp) rising to the higher one. Rule 5 is checked before rule 4.

Because the rules only ask "higher, the same, or lower" of 8 neighbours, the shape is a function
of 3⁸ = 6,561 neighbourhoods, baked into one table read by the server, the WASM client, both
meshers, the collider and the probes. Of the 6,561, rules 1–3 decide 4,448, rule 4 1,789 and
rule 5 324. On face B (up is −y) the same table is read in the mirror image and the shapes hang.

## 3. Definitions the rules need

- **Adaptive.** A block whose data has `"adaptive": true` — grass, dirt, stone and sand (sandstone,
  gravel and ice were cut with the baseline). Only a full cube may be adaptive, and its one allowed
  property is `flooded` (false, true; 2026-10-20) — the generator rejects anything else.
- **Top block.** An adaptive block whose cell above is not solid (air, water, or a non-solid
  block such as a ladder). Any other adaptive block is a full cube.
- **Filled, for comparing heights.** An adaptive block (whatever shape it takes) or any solid block
  whose own shape is a full cube. A partial shape (a slab or placed slope of a future crafted
  block) counts as open, so ground beside it is *lower*. The comparison uses cell occupancy, never
  derived shapes, so nothing iterates and neighbours never depend on each other's results.
- **Water.** A shaped top block with a liquid above it or beside it in its row is flooded (§1), and
  so is one whose state is `flooded=true`: the terrain generator writes every top block below sea
  level so, and the sea fills its open part ([`WATER_RENDERING.md`](./WATER_RENDERING.md) §9). A
  buried or full-shaped block holds no water, whatever its state.
- **The rules say full.** When every corner rises (a one-block pit), the derived shape is the cube:
  it collides and is meshed as one.

## 4. The table as data

`shared/blocks/adaptive.json` is the source of truth: `"rules": "five-rules"` fills every
neighbourhood, and `overrides` replace single ones — none today, since the owner's shapes all
follow the rules. An override is

```json
{ "around": "+==/=.-/===", "shape": "saddle", "facing": "east" }
```

with `around` the 3 × 3 grid (rows north to south, each row west to east, `+` higher, `=` the same,
`-` lower, `.` the block), `shape` a shape name (§5) and `facing` north, east, south or west (the way
a slope descends; a ridge end the way its crest runs; `half_ridge` `east_west` or `north_south`;
none for `full`, `slab`, `half_pyramid` and `half_ridge_cross`). `node shared/blocks/gen.mjs`
rejects unknown shapes and facings, malformed or repeated neighbourhoods (each message names the
entry), runs the exhaustive adjacency test (below), reports how many entries each rule decides and
how many are overridden, and writes `crates/dwell-blocks/src/adaptive.gen.rs` and the readable
`shared/blocks/adaptive_vectors.txt`. The Shape Playground is to export overrides in this form
(outstanding).

**The adjacency test** runs over every pair of neighbourhoods of two blocks of the same height side
by side (east–west and north–south, 3¹⁰ pairs each): the baked side faces must match the profiles
the meshers cull by (so wherever one block's side stands above the other's it is drawn — no hole),
and the two surfaces must meet at their shared corners (no step). They may differ at the side's
middle — a ridge's end stands half a block above a slope beside it and shows its side. The mesher's
own pairwise test checks every two shapes of the table on every side.

## 5. The shapes

The game's shape model was four corner heights plus a split diagonal (Phase 9, `SLOPE_BLOCKS.md`).
The rules use the cube, the slab and the nine Phase 9 slope pieces, plus **12 corner shapes** and
**6 ridge shapes** (`shared/blocks/shapes.mjs`):

| Kind | Shapes (corners NW NE SE SW, canonical facing east) |
|---|---|
| Corner shapes | half-and-half left `2 0 0 1` and right `1 0 0 2`; tall half-and-half left `2 2 1 0` and right `2 2 0 1`; tilt left `2 1 1 0` and right `1 2 0 1`; diagonal slope `1 2 1 0`; dimple `2 1 2 0`; saddle `2 0 2 0`; uneven saddle `2 0 1 0`; half saddle `1 0 1 0`; high half saddle `2 1 2 1` |
| Ridge shapes (3 × 3 points, rows N–S) | half pyramid `000/010/000`; half ridge `000/111/000`; half ridge end `000/011/000`; half ridge corner `000/011/010`; half ridge T `010/111/000`; half ridge cross `010/111/010` |

The new corner shapes split along their **higher** diagonal (a crest, not a valley). The ridge
shapes use 3 × 3 surface points (corners, edge midpoints, centre) — the form the prototypes use;
their surface is the fan of 8 triangles around the centre, merged into convex polygons. With their
facings, upright and hanging, the table appends 191 shapes. Convexity (for Phase 20): the half
pyramid, ridge and ridge end are convex; the ridge corner, T and cross are not.

Slopes, for walking: the gentle slopes are 26.57° (walkable), ramps, ridge flanks and the
standard pieces 45°, and the saddle's and dimple's faces from their crest to a floor corner
arctan √2 ≈ **54.74°** — the "tetrahedral corner" Phase 9 rejected for placed slopes, which the
owner's rules make. Everything steeper than 44° is a slide (owner, 2026-10-15; PLAYER_CONTROLLER.md
§6.8); the controller's slope limit (56°) stays above the saddle's faces, so they are ground that
slides rather than walls.

## 6. Rationale, prototypes and evidence

Why derived, not stored: deriving (rather than storing a shape state and updating neighbours on
every edit) keeps an edit a single voxel, needs no cascade of state changes over the network, and
cannot drift out of date — the class of bug Phase 9's playtests found in stored terrain slopes.
Saved worlds and the wire carry the material alone; turning adaptivity off for a material (a data
change) returns its blocks to cubes without touching saved worlds.

Both prototypes are saved in [`prototypes/`](./prototypes/README.md) with their published links:

- **Shape Playground** — every shape, 28 labelled situations (one for every shape the rules can
  produce, the T and the cross included); tap to set shapes; a live rules-and-conflicts check;
  export/import. The owner's final shapes are embedded (and extracted into
  `shared/blocks/playground-export.json`): all 250 blocks match the five rules (blocks each rule
  decides — Rest 159, Drop 217, Rise 68, One block wide 49, Ramp up 10).
- **Adaptive Block Lab** — the rules on whole scenes (basics, terraces, hill, noise terrain, a
  wall with a sand pile, a meadow) on three grids, with editing, a Copy 5³ export, and the earlier
  rule designs for comparison.

Design history, for context: a height field with slope limiting (rest at half height, shared
corner heights) made clean surfaces but blurred where blocks start and end, and cut into lower
blocks or left slivers at diagonal 2-block drops; crisp-cliff propagation removed slopes from
whole hillsides. Finer voxel grids (1 × ½ × 1 m, ½ m cubes) were prototyped; the per-block shape
rules made them unnecessary, and the 1 m grid stays.

## 7. Decisions made in building it (Phase 18)

- **Terrain collision:** full-cube cells stay in the `Voxels` collider; shaped cells are one
  triangle mesh per chunk of their uncovered faces. Crossing between the meshes keeps the ground.
- **Chunk borders:** the chunk mesher has a one-voxel apron, so an adaptive top block in the apron
  is taken to cover none of its sides (faces toward it are drawn; any it does cover lie inside it).
  The collider reads a two-voxel apron and derives exactly. The LOD treats apron columns the same way.
- **Edits:** a voxel modification re-meshes every chunk the changed voxels touch, diagonals
  included, and the collider rebuilds a chunk when any of the 27 around it changes.
- **The placement check** tests a placed block against players as a cube (its shape is only known
  after placing, and the cube is the larger).
- **Generation was unchanged** by Phase 18: the generator wrote the same voxels; spawn and the
  point query read the shaped surface. Phase 17's evaluator must keep that. Since generator version
  17 (2026-10-20) the top voxel below sea level is written `flooded=true` (§3, Water).
- **LOD:** the rule on dry columns' top cells; wet columns and shores keep the generated surface's
  exact heights.

## 8. Open details (owner decisions)

- Natural ground against crafted blocks: today's rules treat a full crafted block like any higher
  or lower column (ground ramps up to it); confirm in the lab once crafted blocks exist.
- Slabs as neighbours: a partial shape counts as lower than a block beside it (§3); confirm.
- How shores look (flooded banks, §1) — the owner's walk.
- Phase 20's choice for detached clusters: keep the shapes they had at detachment, or re-derive
  within the cluster; the ridge corner, T and cross are not convex (two or three convex parts).
