# Dwell — Adaptive Natural Blocks: Shapes From Neighbours

> **Status: [planned, Phase 9e]** — design agreed with the owner in prototypes on 2026-10-07/08
> ([`prototypes/`](./prototypes/README.md)); decision proposed in
> [ADR 0024](./adr/0024-adaptive-natural-blocks.md). Nothing here is built yet. When it lands, the
> built mechanisms move into [`ARCHITECTURE.md`](./ARCHITECTURE.md) (§6.1, §6.3, §6.5, §6.6) and
> this file keeps the rationale and the tables, as [`SLOPE_BLOCKS.md`](./SLOPE_BLOCKS.md) does for
> Phase 9.

Phase 9 gave Dwell slope blocks that players place and pick by hand, and a terrain stage that
rounds the generator's continuous surface into slope pieces. This phase makes **natural blocks
shape themselves**: a dirt, grass, stone, sand or snow block takes its shape from the columns
around it, so touching natural blocks always join into one clean surface, and a player never
places or rotates a slope for them. Crafted blocks — and slabs and the placed slope families of
crafted materials — keep the shapes players give them.

Content is still prototype (`ARCHITECTURE.md` §6.1): which materials are natural is block data
(an `adaptive` flag), not code.

## 1. What the player sees

- **A natural block with another block on top is a full cube.** Only the top block of a column
  is shaped, and only its upper surface: undersides, cave ceilings and overhangs stay cubes.
- **An exposed natural block rests at half height.** A flat field one block deep is a surface
  ½ m up; two blocks deep, 1½ m (a full cube with a half-height top).
- **Edges slope gently to the ground**: ½ m over a whole block (about 27°), flush with what is
  beside them.
- **A 1-block step is one even slope across both blocks**: the lower block rises to its top at the
  step, the upper block sinks to its floor, ½ m per block.
- **One-block-wide shapes are half-height ridges**: a lone block is a half pyramid, a row a ridge
  with end caps, an L a ridge corner, plus T and cross pieces.
- **A one-wide block against a higher one is a full 45° ramp up to it** (stairs of single
  blocks are a run of ramps).
- Steeper ground (2 blocks or more between neighbours) is a cliff: the shaped top blocks meet the
  full cubes below, and the cubes' sides are the cliff face. Where a block sits diagonally below
  another, its side shows between them; that is expected, not a gap.

## 2. The rules

Every top block of an adaptive material is shaped by these five rules, worked out from the
owner's hand-shaped examples in the Shape Playground: the rules reproduce all 151 blocks the owner
shaped (situations A–P), and the owner approved what they make of 12 more situations (Q–AB) that
exercise every shape they can produce — 250 blocks in 28 situations, no conflicts (§7).

Heights are compared column by column: for the block at `(x, y, z)` and each of its 8 horizontal
neighbours, the neighbour is **higher** if it is solid at `y + 1`, **lower** if it is not solid at
`y`, and **the same** otherwise (§3 defines "solid" for shaped and crafted neighbours). The shape
is given by its four **corner heights** in halves of a block above the block's floor: 0 (floor),
1 (half), 2 (top). Each corner looks at the 3 neighbours around it (the two edge neighbours and
the diagonal):

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
of 3⁸ = 6,561 neighbourhoods. It is **baked into a table** (§4.1) that the C++ server and the
TypeScript client both read, so collision, prediction and rendering agree exactly. Edges of the
world treat missing neighbours as the same height.

## 3. Definitions the rules need

- **Adaptive.** A block state is shaped by the rules when its block has `"adaptive": true` in
  `shared/blocks/dwell.json` (the prototype set: grass, dirt, stone, sand, snow; sandstone, gravel
  and ice are owner decisions, see §8). Everything else keeps its own shape.
- **Top block.** An adaptive block whose cell above is not solid (air, water, or a non-solid
  block such as a ladder). Any other adaptive block is a full cube.
- **Solid, for comparing heights.** A neighbour cell counts as solid when its block's shape covers
  the cell's full height at that side — full cubes and crafted blocks always; a slab or a placed
  slope by its own top (a slab beside a block counts as *lower*). An adaptive neighbour that is
  itself shaped counts as solid: the comparison uses cell occupancy, never derived shapes, so the
  rule needs no iteration and neighbours never depend on each other's results.
- **Water.** A shaped top block below a water surface is flooded (its open part filled with
  water), as Phase 9's flooded states are; the water cell above does not count as solid.

## 4. Changes by system

### 4.1 Shapes and the registry (`shared/blocks/`)

- **New shapes.** The game's shape model is four corner heights plus a split diagonal. The rules
  need **12 more corner shapes** and **6 shapes that are not corner shapes** (their ridge runs to
  edge midpoints or the centre):

  | Kind | Shapes (corners NW NE SE SW, canonical facing east) |
  |---|---|
  | Corner shapes | half-and-half left `2 0 0 1` and right `1 0 0 2`; tall half-and-half left `2 2 1 0` and right `2 2 0 1`; tilt left `2 1 1 0` and right `1 2 0 1`; diagonal slope `1 2 1 0`; dimple `2 1 2 0`; saddle `2 0 2 0`; uneven saddle `2 0 1 0`; half saddle `1 0 1 0`; high half saddle `2 1 2 1` |
  | Ridge shapes | half pyramid, half ridge, half ridge end, half ridge corner, half ridge T, half ridge cross |

  The new corner shapes split along their **higher** diagonal (a crest, not a valley). The ridge
  shapes extend the model to 3 × 3 surface points (corners, edge midpoints, centre) — the form the
  prototypes use; `shapes.mjs` bakes their faces like any other shape.
- **The rule table.** `shared/blocks/gen.mjs` also bakes the 6,561-entry table (neighbourhood →
  shape, facing), generated from the five rules in one place, with test vectors for C++ and
  TypeScript (as the shape table and the registry hash have today).
- **Registry.** Adaptive blocks keep **only their cube state**: the shape is derived, never
  stored, so no new block states are needed and world files and edits carry the material alone.
  The placed slope and slab families of materials that become adaptive are removed from the
  registry (their placed shapes would fight the rules); crafted materials keep theirs. This
  changes the registry hash.
- **Not derived shapes in the voxel data.** Deriving (rather than storing a shape state and
  updating neighbours on every edit) keeps an edit a single voxel, needs no cascade of state
  changes over the network, and cannot drift out of date — the class of bug Phase 9's playtests
  found in stored terrain slopes.

### 4.2 Chunk meshing (client) and terrain collision (server and client)

- Both look up each adaptive top block's shape in the rule table from its neighbourhood: the 8
  neighbours at `y` and `y + 1` plus the cell above. Both already read a one-voxel apron around a
  chunk; it must include the diagonal columns and the layer above.
- The derived shape replaces the block's own shape for meshing, face culling, collision polygons,
  ray targeting (`RayEnterShape`) and `SurfaceHeightAt`.
- An edit changes the shapes of up to 9 neighbouring top blocks (the 8 around it and the one
  below): re-mesh and rebuild collision for every chunk that holds one (chunk borders already
  re-mesh neighbours for culling).
- Prediction parity: the client predicts with the same table and the same occupancy, so the
  shapes it walks on are the server's.

### 4.3 Terrain generation

- The generator stops shaping the surface: **step 5b (slopes) is removed**, and the terrain is
  plain occupancy whose top blocks the rules shape. Generation gets simpler and cheaper.
- This changes what a seed's terrain looks like and what the world file holds, so it is a
  **generator version bump** (goldens regenerated) and, with the registry change, a **new
  compatibility line**: the owner raises `client/package.json` (`RELEASES.md`).
- Natural surfaces now rest half a block lower than the generator's top cell. Water levels,
  shores, spawn search (`GroundY`, `LevelSpawnY`), tree and boulder placement and `SurfaceAt`
  must use the shaped surface; the generator's own point queries gain the rule.

### 4.4 Level of detail

- LOD sections apply the same rule table to their cells (a cell at level *k* is a 2ᵏ cube; corner
  heights are halves of a cell), replacing 9d's slopes from surface heights. The LOD format does
  not change. Distant and near terrain then use one rule, so they match at the seam.

### 4.5 Building and the player

- Players place and break adaptive blocks as cubes; the shape follows. Placing on top of a shaped
  block fills the cell above, which makes the block below full. The creative palette's shape key
  applies only to crafted materials.
- Targeting a shaped block hits its true shape; the placement preview shows the cell, and the
  block's shape after placing is the rule's.
- The controller already walks 27° and 45° slopes and steps 0.55 m (`PLAYER_CONTROLLER.md`); the
  new saddle and ridge shapes need scenarios (§5).

### 4.6 Later phases

- **Phase 20 (voxel awakening):** a detached cluster's adaptive blocks keep the shapes they had at
  the moment of detachment (frozen into its body), or re-derive within the cluster — to decide
  there. Convex decomposition: the ridge corner, T and cross are not convex (two or three convex
  parts); the others are.
- **Phases 21–22:** re-baking a settled body writes cube states; shapes re-derive from occupancy.

## 5. Sub-phase plan (9e)

1. **Shapes and the table.** The 18 new shapes in `shapes.mjs` (corner shapes and the 3 × 3
   surface form), the rule table and its generator, `adaptive` in `dwell.json`, test vectors;
   C++ and TS readers; geometry tests (closure, winding, volume, coverage) and an exhaustive
   adjacency test that two neighbouring derived shapes never leave a hole at a shared edge of
   equal-height neighbours.
2. **Meshing, collision, targeting.** Derived shapes in the chunk mesher, terrain collision,
   `RayEnterShape`, `SurfaceHeightAt`; edits re-mesh neighbours; controller scenarios on every new
   shape (walk over ridges, saddles, half-and-half corners, 45° single-block ramps; no launches,
   hops or sliding at rest), native and WASM.
3. **Registry and building.** Remove the slope/slab families of adaptive materials; palette shape
   key for crafted materials only; placing and breaking re-shape neighbours (e2e).
4. **Terrain.** Remove step 5b; the shaped surface in point queries, spawn, water and features;
   generator version bump; goldens.
5. **LOD.** The rule on LOD cells; seam check against chunks.

## 6. How it is checked (exit criteria)

- The rule table equals the five rules for all 6,561 neighbourhoods, in C++ and TS, from shared
  vectors; it reproduces every block of the owner's playground export (`prototypes/`).
- No holes: for every pair of neighbourhoods that share an edge, the two derived shapes meet
  without a gap wherever both blocks are the same height (exhaustive test).
- Controller scenarios on every new shape pass natively and in WASM; the divergence check passes.
- e2e: placing and breaking adaptive blocks re-shapes the neighbours identically on two clients.
- Generation and meshing times, and frame time on a desktop and a phone, against Phase 9's.
- Manual: the owner walks generated terrain and builds with natural blocks, and compares with the
  Adaptive Block Lab.

## 7. Prototypes and evidence

Both prototypes are saved in [`prototypes/`](./prototypes/README.md) with their published links:

- **Shape Playground** — every shape, 28 labelled situations (one for every shape the rules can
  produce, the T and the cross included); tap to set shapes; a live rules-and-conflicts check;
  export/import. The owner's final shapes are embedded: all 250 blocks match the five rules
  (blocks each rule decides — Rest 159, Drop 217, Rise 68, One block wide 49, Ramp up 10).
- **Adaptive Block Lab** — the rules on whole scenes (basics, terraces, hill, noise terrain, a
  wall with a sand pile, a meadow) on three grids, with editing, a Copy 5³ export, and the earlier
  rule designs for comparison.

Design history, for context: a height field with slope limiting (rest at half height, shared
corner heights) made clean surfaces but blurred where blocks start and end, and cut into lower
blocks or left slivers at diagonal 2-block drops; crisp-cliff propagation removed slopes from
whole hillsides. Finer voxel grids (1 × ½ × 1 m, ½ m cubes) were prototyped; the per-block shape
rules made them unnecessary, and the 1 m grid stays.

## 8. Open details (owner decisions)

- Which materials are adaptive: sandstone, gravel and ice; logs (natural, but tree trunks) are
  probably crafted-like.
- Natural ground against crafted blocks: today's rules treat a crafted block like any higher or
  lower column (ground ramps up to it); confirm in the lab.
- Slabs as neighbours: a slab counts as lower than a block beside it (§3); confirm.
- Flooded shaped blocks (§3) and how shores look.
- Phase 20's choice for detached clusters (§4.6).
