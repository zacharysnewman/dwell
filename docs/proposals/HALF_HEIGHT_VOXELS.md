# Proposal: Half-Height Voxels (1 × ½ × 1 m cells)

> **Status: adopted for a playtest** (2026-10-19, [ADR 0042](../adr/0042-half-height-cells.md),
> Phase 41, design in [`HALF_HEIGHT_GRID.md`](../HALF_HEIGHT_GRID.md)), on the long-lived branch
> `claude/half-height-voxels`. The owner chose ½ m cells, cardinal ramps only (½ m, and 1 m from
> two ½ m cells on steep ground), shapes stored at generation and never reshaped, placed natural
> blocks flat, ½ m per click, and 45° ramps that slide. This file is kept as the investigation's
> record: the comparisons below include options not chosen (adaptive shapes, double blocks,
> corners). Prototype: [Half-Height Voxel Lab](../prototypes/half-height-lab.html).

This grid was considered once before. [ADR 0024](../adr/0024-adaptive-natural-blocks.md) option 3
rejected it in favour of per-block shape rules, citing 2× the voxels, a non-cubic grid and "a
breaking change everywhere", and `ADAPTIVE_BLOCKS.md` §6 notes that the Adaptive Block Lab
prototyped it. This proposal revisits it with two needs that ADR did not weigh: plants that stand on
the ground, and growth in ½ m steps.

## 1. What today's grid does

- **Natural ground already lives on a ½ m height lattice.** An exposed adaptive top block rests at
  half height, and its corners take 0, ½ or 1 of the cell (`ADAPTIVE_BLOCKS.md` §2). So the visible
  surface is at multiples of ½ m. Flat natural ground, though, always rests *inside* its top block,
  and its surface never falls on a cell boundary.
- **Plants float ½ m.** A cross plant is drawn from the floor of its cell (`crates/dwell-mesh/src/mesher.rs`
  `cross_voxel`, `y0 = y`), which is the top of the block below, 1 m up. The grass block under it is
  a top block (a plant is not solid), so it rests at ½ m. Measured on `main` (a throwaway test of
  `mesh_chunk`): on a 7 × 7 meadow one block deep with `dwell:tall_grass` in the middle, the ground's
  top is at **0.5** and the plant's foot at **1.0**. The existing test
  `draws_tall_grass_as_crossed_cutout_quads_from_its_data` asserts the foot at ≥ 1.0, over a lone
  grass block that is a half pyramid ½ m tall. This is a bug in today's game whatever is decided
  here (§6).
- **Growth in ½ m steps** would need a block state property (a `height` per stage), plus a second
  block for the part above 1 m (an "upper" state). The registry supports both (`BLOCK_REGISTRY.md`
  §2), but each stage then needs its own selection box, support rule and mesh height.

## 2. What half-height cells change

A cell is 1 m × ½ m × 1 m. Natural ground is shaped by **four rules**, from the five, as the
prototype implements them:

1. **Top.** A corner of a natural top voxel stays at the voxel's top.
2. **Drop.** If any of the 3 columns around the corner is lower, the corner drops to the voxel's
   floor.
3. **One voxel wide** (today's rule 4). When every corner drops, a ridge runs toward each
   same-height edge neighbour.
4. **Ramp up** (today's rule 5). When every corner drops and exactly one edge neighbour is higher,
   the voxel is a wedge rising to it.

**Rest** and **Rise** disappear. They exist because flat ground rests halfway up its block and must
rise to meet a higher neighbour. With ½ m cells, flat ground already ends on a cell boundary, so
nothing ever needs to reach into the cell above.

| | 1 m cells (Phase 18) | 1 × ½ × 1 m cells |
|---|---|---|
| Rules | Rest, Drop, Rise, One wide, Ramp | Top, Drop, One wide, Ramp |
| Corner heights | 0, ½, 1 of the cell | floor or top |
| Block heights | 1 m | ½ m, or 1 m (a double block) |
| Shapes | 29 (191 with facings and hanging) | ½ m blocks: 11 (full, wedge, outer corner, inner corner, saddle, 6 ridge pieces); 1 m blocks add 45° and mixed pieces |
| Neighbourhoods the table covers | 3⁸ = 6,561 | 1,296 for ½ m blocks (diagonals only matter as lower or not), 20,736 for 1 m blocks |
| Steepest natural slope | 45° (ramps), 54.74° (saddles) | 27° (½ m blocks), 45° (1 m blocks) |
| Flat natural ground | rests ½ m into its top block | the top of a cell, at any ½ m |
| Slope over a ½ m step | 27° across one block | 27° (a wedge in one voxel) |
| One-wide stairs | 1 m steps, 45° ramps that slide | ½ m blocks: a 27° walkable ramp; 1 m blocks: 45° ramps |
| A plant on flat ground | foot ½ m above the ground (bug, §1) | foot on the ground |
| Grass growth by ½ m | a `height` property per stage, and an upper block above 1 m | one more voxel per stage |

The shapes look as they do today at the surface. A lone ½ m voxel is the same half pyramid; an edge
is the same 27° slope. The difference is that each shape fills one ½ m voxel instead of the top half
of a 1 m block.

**1 m double blocks (owner, 2026-10-19).** Blocks come in two heights: a **½ m block** (one voxel)
and a **1 m double block** (two voxels, one block: placed, broken and shaped as one). The rules
above apply to the column's top block, whatever its height. Rule 2 drops a corner to the lowest of
the 3 columns around it, but no lower than the block's floor. So a ½ m block makes 27° slopes, and a
1 m block makes 45° ramps, 1 m ridges and pyramids, or mixed pieces where one neighbour is ½ m lower
and another 1 m (corners at its floor, middle or top). With ½ m blocks alone, a 1 m drop becomes a ½ m
wall under a 27° slope; a 1 m block there makes it one 45° slope instead.

Where double blocks come from:

- **Building.** The 1 m brush places a double block; Remove takes the top block, whichever size.
  Crafted 1 m blocks are double blocks, so 1 m building keeps one click per block.
- **Generation.** The lab's **1 m on steep ground** puts a double block on top of any column that
  is 1 m or more above **any of its 8 neighbours**. The diagonals matter. A ½ m block 1 m above a
  diagonal neighbour can drop only ½ m at that corner, while its edge neighbours drop to the
  diagonal's height, and a triangle of its side shows between them. With edge neighbours only, the
  noise terrain showed dozens of these; with all 8, it shows about as few as the 1 m grid. Gentle
  ground stays ½ m blocks.

The table grows: a 1 m block's corners need "lower by ½ m" and "lower by 1 m" apart, 4⁴ × 3⁴ =
20,736 neighbourhoods, beside the ½ m block's 1,296. That is two tables keyed by block height, both
generated from the same rules.

**Walking 45°.** Faces steeper than 44° slide in the game today (owner, 2026-10-15;
`PLAYER_CONTROLLER.md` §6.8). So a 1 m block's ramps would slide unless that limit moves to 46°, or
unless double blocks are for cliffs and slopes the player isn't meant to walk up. One-wide stairs of
½ m blocks stay a walkable 27° ramp.

## 3. What the prototype shows

[Half-Height Voxel Lab](../prototypes/half-height-lab.html) draws one scene on both grids side by
side, with synced cameras, editing, planting, growth, a per-column cell inspector and floating-plant
counts. Scenes are given as surface heights in metres and realised as each grid best can. On 1 m
cells that is `round(S + ½)` blocks resting ½ m down; on ½ m cells it is `round(S / ½)` voxels.

| Scene (plants at the cell floor, as the game draws them) | 1 m cells | ½ m cells |
|---|---|---|
| Meadow: plants floating | 283 of 283 | 0 of 283 |
| Terrain (value noise, ~10 m relief): plants floating | 198 of 200 | 160 of 200 |
| Terrain: ground cells | 2,137 | 3,728 (1.74×) |
| Terrain: 1 m double blocks (1 m on steep ground) | none | 185 |
| Terrain: distinct shapes in use | 24 | 9 with ½ m blocks only, 15 with 1 m on steep ground |
| Growth rows (plants ½–2 m): plant storage | 162 blocks with a height property | 264 plant voxels |

What this means:

- **Flat ground is fixed by the grid; slopes are not.** A plant over a wedge or corner voxel still
  stands on the cell's top, up to ½ m above the slope. Plants on shaped ground need their foot on
  the surface under them (**Sink to surface** in the lab, which brings both grids to 0 floating).
  Alternatively, plants could grow only on flat tops. Neither grid avoids that work.
- **Steep terrain needs the 1 m blocks.** With ½ m blocks only, the finer grid has many places
  where a column sits more than ½ m below a diagonal neighbour. There the side of the higher block
  shows as a small triangle: the "side between them" that `ADAPTIVE_BLOCKS.md` §1 accepts, appearing
  far more often. Double blocks on any column 1 m above one of its 8 neighbours remove nearly all of
  them. What remains are drops of more than 1 m, as on the 1 m grid.
- **Data roughly doubles.** That is 1.74× here, and 2× underground, where no air is saved.

### Fixed, stored shape sets (2026-10-19)

The owner asked to see a small fixed set of pieces, stored, instead of shapes derived from the
neighbours. The lab's ½ m view has two such sets beside **Adaptive**:

- **Cardinal ramps + corners:** cardinal ramps, outer corners and inner corners at ½ m (27°) and 1 m (45°),
  24 pieces, plus the full block.
- **Cardinal ramps only:** the 8 cardinal ramps (½ m and 1 m), plus the full block.
- **No slopes:** the full block only: ½ m (and 1 m) blocks with flat tops, for comparison.

Each block's piece is chosen once, when the scene is built or the block is placed, from what the
rules would make. A shape the set has no piece for becomes a full block: saddles, ridges and
pyramids, plus corners in Cardinal ramps only. A 1 m block's corner counts as down only at the block's floor.
A neighbour's later edit changes nothing, and a block uncovered by digging is a full block. The lab
counts the **blocks off the rules**: blocks whose stored piece differs from what the rules would
shape now.

| Blocks off the rules | Adaptive | Cardinal ramps + corners | Cardinal ramps only | No slopes |
|---|---|---|---|---|
| Hill, ½ m blocks only | 0 | 5 | 217 | 377 |
| Terrain, ½ m blocks only | 0 | 16 | 257 | 465 |
| Basics, ½ m blocks only | 0 | 9 | 49 | 138 |
| Hill, 1 m on steep ground | 0 | 142 | 245 | 377 |
| Terrain, 1 m on steep ground | 0 | 165 | 293 | 465 |

- **Cardinal ramps + corners on ½ m blocks is nearly the full rule set.** The few blocks off are saddles and
  lone bumps, which become ½ m cubes. Steep ground shows ½ m walls under the slopes, as adaptive
  ½ m blocks do.
- **Cardinal ramps only** puts a ½ m cube at every convex corner and every saddle, and leaves a notch at every
  concave bend. It reads as stepped terrain with ramps between the steps.
- **No slopes** is ½ m terraces: every shaped block is off the rules. With the game's grass side (dirt
  with a grass skirt, which the lab draws on both grids), each ½ m step shows a thin dirt band under
  its grass edge. Every step can be walked up (the step height is 0.55 m).
- **1 m blocks don't fit a fixed set of top-or-floor pieces.** A 1 m block on generated terrain
  often has one neighbour ½ m lower and another 1 m lower. The rules then put a corner at its
  middle, and no fixed piece has one. Snapping it up or down leaves notches; snapping down (as the
  lab does) makes them smaller. A fixed set with 1 m blocks needs mixed pieces, or a generator that
  places 1 m blocks only where every lower neighbour is a full 1 m lower.

### Grass on flat ground only (2026-10-19)

The owner asked that grass grow only on flat surfaces. The lab now plants only on a flat top: a
full block on the ½ m grid, or a level resting top on the 1 m grid. A plant whose ground stops being
flat goes, as one whose ground is dug away does. On flat ground the ½ m grid's plants then stand
exactly on the ground (none floating, at the cell floor), and the 1 m grid's still float ½ m (§1).

On slopes, it leaves little grass. The plants each scene puts on its grass, and how many have flat
ground:

| Scene (155 / 200 plants) | 1 m, adaptive | ½ m, adaptive | ½ m, cardinal ramps + corners | ½ m, cardinal ramps only | ½ m, no slopes |
|---|---|---|---|---|---|
| Hill | 5 | 19 | 21 | 106 | 155 |
| Terrain | 13 | 39 | 47 | 125 | 200 |

With adaptive shapes almost every block on a hillside is a slope or a corner, so hillsides go bare.
Cardinal ramps only keep more, because their corners are flat blocks. If hillsides should be
grassy, grass needs to grow on the gentle (27°) pieces too, sunk to the surface under it (the lab's
**Sink to surface**), or the grass block's own top should carry the grass look on slopes.

## 4. Implications by area

The code assumes one cell is one metre on every axis. There is no voxel-size constant; there are
about 55 `floor() as i32` world-to-cell sites, about 90 `CHUNK_SIZE` uses in 21 files, and about
130 uses of the vertical constants (midplane, sea level, world bounds, flip band) in 27 files. Many
of those use the same number both as a row and as metres. Making the cell height a single named
constant is the first step of any version of this proposal.

- **Chunks and storage.** Keep chunks 32 m on a side: 32 × 64 × 32 cells = 65,536, still
  addressable in 16 bits (`x | y << 5 | z << 11`). A chunk becomes 128 KiB raw instead of 64 KiB.
  Saves, edits on the wire, generation, meshing and collider building all handle twice the cells per
  volume. Measure with the bench (`docs/DEV_TOOLS.md`) before relying on any estimate.
- **Protocol, saves and seeds (breaking).** Every cell coordinate on the wire and on disk changes
  meaning (`ChunkCoordNet`, `BlockEditRequest`, `RegionEdit`, LOD sections), and so does the terrain
  every seed generates. This needs a new compatibility line, the next MINOR (`RELEASES.md`,
  CLAUDE.md), and a protocol version bump. Existing worlds would not load.
- **Shapes and the adaptive table** (`shared/blocks/shapes.mjs`, `adaptive.mjs`, the generated
  Rust). The rule engine shrinks (§2). The shape model's "halves of a cell" become halves of a ½ m
  cell, and the side profiles, the adjacency test and the playground export are regenerated.
- **Meshing.** Positions need a per-axis scale; the shader's single `offset.w` scale becomes a vec3
  (`render/shaders.rs`). Textures need world-space v, so that a 1 m tile spans two cells instead of
  squashing into each.
- **Physics.** Parry's `Voxels` already takes a per-axis voxel size (`terrain.rs`). The voxel DDA
  (`query.rs`), overlap scans, the ladder frame and auto-jump (which asks for a "one-block obstacle"
  and two free cells above) change. The controller's tunables are in metres and mostly survive.
  `max_step_height` 0.55 steps one voxel; the 1.8 m capsule is 3.6 cells; a 2 m doorway is 4 cells.
  The golden `scenario.trace` and about 12 physics test files hard-code 1 m geometry.
- **Double blocks.** A 1 m block is a block that spans two cells, which the engine has no notion of
  today. The upper cell needs a marker (an `upper` part, or a reserved state) that collision, the
  mesher, picking, breaking, support and the network all treat as part of the block below. Saves
  must keep the pair together, and an edit to either cell edits both. That is the kind of special
  handling the half grid otherwise avoids, so it should be one mechanism used by every 1 m block
  (crafted 1 m blocks, natural double blocks, later doors), not per-block code. An alternative
  would be to store a double block's two cells as plain voxels plus a "joined" flag that shaping
  reads; it needs the same care in editing.
- **Building.** With double blocks, a 1 m block is one click to place and one to break. The brush
  tool's shapes (`brush.rs`: ball, ellipsoid, disc) are in cell space and would flatten unless
  scaled.
- **Plants and growth.** A growing plant is a stack of plant voxels. The mesher draws a cross per
  voxel, jittered by column (not by cell) so that the segments line up, with the tip texture where
  the cell above is not the same plant: a derived look, like adaptive shapes. `needsSupport` and
  `replaceable` already cover a stack. Growth itself is a new system (a server-side growth tick, and
  growth rules as block data such as `"grows": { "max": 4, "chance": … }`). Under CLAUDE.md's
  extendable-system requirement it needs its own design questions on either grid.
- **Worldgen, LOD, ray marcher, water, structures.** Heights are metres used directly as rows
  (`top_voxel(h) = ceil(h) − 1`). Those become `ceil(2h) − 1`, and the chunk and LOD golden hashes
  are regenerated. The LOD octree assumes cubic cells (`2^L m`), as does the ray marcher's grid (in
  `t` and in cone LOD). Both need a y scale or ½ m-tall cells at level 0. The water surface drop
  (⅛ of a cell) halves unless restated in metres. Structures are authored in cells: the
  playground's structures, the ladder post and the Tree Growth samples would be re-voxelised (each
  layer doubled).
- **Docs.** `ARCHITECTURE.md` ("Voxel = 1 m cube", §7.4's constants), `PLAYER_CONTROLLER.md`,
  `ADAPTIVE_BLOCKS.md`, `SLOPE_BLOCKS.md`, `TREE_GROWTH.md` (1 m trunk steps), and ADR 0024, which
  would be superseded in part.

## 5. Effort

This is a migration of every crate, not a feature: comparable to the baseline rework's largest
phases. The shape and adaptive work gets simpler. The cost is in the coordinate plumbing, the
goldens, the protocol and save break, and re-authored content. A sensible order:

1. A named cell size: one constant for cell height, used everywhere a row and metres meet.
   Behaviour is unchanged at 1.0 and checked by the existing suite.
2. Set it to ½. Fix every test that breaks, regenerate the goldens, apply the new rules and shape
   table, and use a world-space texture v.
3. Double blocks: blocks that span two cells, as one mechanism (edits, collision, meshing, picking,
   saves), and the second shape table.
4. Building brushes, the protocol and save break, and the compatibility line.
5. Plants as stacks, and growth (its own design).

## 6. The alternative: keep 1 m cells

Most of what was asked can be had on today's grid:

- **Plants on the ground.** Draw a cross from the derived surface of the block below at the
  plant's centre (`ShapeInfo`'s surface is already known to the mesher and the collider). That is a
  bug fix with a regression test, and it fixes slopes too, which the half grid alone does not
  (§3).
- **Growth in ½ m steps.** A `height` property per stage with an `upper` part above 1 m: more
  states and a two-block plant, but no engine change. Alternatively, grow in 1 m steps.

What only the half grid gives:

- flat ground and building surfaces at any ½ m;
- one-wide ½ m stairs that are walkable ramps;
- a smaller rule set and shape table;
- growth as plain voxel edits, so collision, selection, support and networking come for free;
- terrain detail at ½ m.

## 7. Recommendation

Fix the floating plants on the 1 m grid now (§6); that is a bug in the current game. Treat the half
grid as the owner's decision on look and feel, using the lab. If the owner wants ½ m terrain, ½ m
building and walkable one-wide stairs, with 1 m double blocks for steep ground and 1 m building, the
half grid is the cleaner long-term model, and the right
time for it is before more content and saved worlds depend on 1 m: step 1 of §5 costs little and
makes the switch measurable. If ½ m growth is the main reason, the 1 m grid with a `height`
property covers it at a fraction of the cost.

## 8. Questions for the owner

1. Is ½ m terrain and ½ m building wanted for its own sake, or only for plants and growth?
2. Crafted blocks: 1 m double blocks only, ½ m blocks only, or both, per block in the registry?
3. Double blocks: should the generator place them only on steep ground (any of 8 neighbours 1 m
   lower, as in the lab) or as all natural ground below the top ½ m? Should 45° ramps be walkable
   (moving the 44° slide limit), or slide?
4. Plants on slopes: sink to the surface, or grow only on flat tops?
5. Growth: what grows, who adds growing plants (block data, an editor), and is growth per world
   (a world rule) or per block? This is the extendable-system question for growth on either grid.
6. Timing: a breaking change needs a new compatibility line. Is that acceptable now, or should it
   wait for a planned break?
