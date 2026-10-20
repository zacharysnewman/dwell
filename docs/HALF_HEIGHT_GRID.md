# Dwell — The Half-Height Grid: ½ m Cells and Stored Cardinal Ramps

> **Status: [in progress, Phase 41: H0–H4 and H6 (main merged) built, H5 (playtest) next]** on the long-lived branch `claude/half-height-voxels`, for
> the owner to playtest before anything reaches `main` ([ADR 0042](./adr/0042-half-height-cells.md)).
> The investigation is [`proposals/HALF_HEIGHT_VOXELS.md`](./proposals/HALF_HEIGHT_VOXELS.md) and
> the [Half-Height Voxel Lab](./prototypes/half-height-lab.html), whose **Cardinal ramps only**,
> **½ m blocks only** and **1 m on steep ground** settings preview this design (as 1 m double
> blocks; here the 1 m ramp is two ½ m cells). On the branch, this file and `ARCHITECTURE.md`
> describe the branch; on `main` they describe `main`.

## 1. The owner's decisions (2026-10-19)

- **Cells are 1 × ½ × 1 m.** Every block is one ½ m cell; there are no blocks that span cells.
- **Natural ground has cardinal ramps only**, facing north, east, south or west:
  - the **½ m ramp** rises ½ m over 1 m (26.57°) in one cell;
  - the **1 m ramp** rises 1 m over 1 m (45°) across two cells, the **steep lower** and **steep
    upper** pieces.
  
  There are no corners, saddles or ridges. Where none of these fits, a block is flat.
- **Shapes are chosen at generation** and stored as block states. Nothing reshapes at run time:
  breaking or placing a block leaves every other block as it is.
- **A placed natural block is flat.** Only the generator makes ramps.
- **½ m per click** for placing and breaking. There is no 1 m brush.
- **45° ramps slide** (the 44° limit stays, `PLAYER_CONTROLLER.md` §6.8). ½ m steps are stepped up
  (the step height is 0.55 m), and ½ m ramps are walked.
- **Tall grass grows only on flat tops.**

## 2. Units

- A **cell** is 1 m × `cellHeight` × 1 m, with `cellHeight` = 0.5 in `shared/protocol/constants.json`
  (generated into Rust and TypeScript). **Cell space** is the world with y counted in rows. Every
  voxel algorithm works in cell space, unchanged: chunk indexing, meshing, shapes as fractions of a
  cell, the LOD octree, the cell DDA, region and brush boxes.
- A **chunk** stays 32³ cells: 32 m × 16 m × 32 m. Its storage, codec and wire format are
  unchanged; the world simply has twice as many chunk rows.
- **Metres** are used by physics, the controller, the camera and rendering, audio, positions on the
  wire and in saves, and terrain heights. Conversions sit at those boundaries and go through one
  set of helpers in `dwell-world` `voxel.rs`: the cell size as a vector, a row from a height, a
  height from a row, a cell from a position, and a chunk from a position.
- The vertical world constants (`midplaneY`, `worldBottomY`, `worldMaxY`, `seaLevel`) stay **rows**
  in `constants.json`, because integer code depends on their exact values (the midplane on a chunk
  boundary, the mirror arithmetic, the LOD's and the marcher's row offsets). The generator emits a
  metre twin of each (`MIDPLANE_M` and so on, row × `cellHeight`) for every metre consumer: physics,
  the controller, the shaders, fog and clouds. `flipBand` is already metres and has no row twin.
  At the switch to ½ m the row values double, so the world keeps its depth in metres.

## 3. The pieces

Each piece is a shape in the existing shape mechanism (`shared/blocks/shapes.mjs`, `SLOPE_BLOCKS.md`
§1), in canonical orientation descending toward +X (east), as fractions of its cell (halves: 0, 1,
2):

| Piece | Form | Surface | Slope |
|---|---|---|---|
| Full | corners `2 2 2 2` | the cell's top | — |
| Ramp | corners `2 0 0 2` | top at the west edge down to the floor at the east edge | ½ m over 1 m, 26.57° |
| Steep lower | 3 × 3 points, every row `2 2 0` | flat to the middle, then down to the floor at the east edge | 45° |
| Steep upper | 3 × 3 points, every row `2 0 0` | from the top at the west edge down to the floor at the middle; open beyond | 45° |

Stacked, a steep upper on a steep lower of the same facing is one 45° plane from the top of the
upper cell at the west edge to the floor of the lower cell at the east edge.

On face B (up is −y, `BIFACIAL_WORLD.md`), the same pieces hang: the shape mechanism's inverted
form, keyed by the block's face as the generator already does for face B's terrain.

**As block states.** Natural blocks opt into one shape family by data (`shapeFamilies` in
`shared/blocks/dwell.json`, the mechanism Phase 9 built). The family has these properties:

- `shape`: `full`, `ramp`, `steep_lower` or `steep_upper`;
- `facing`: north, east, south or west;
- `half`: `bottom` (face A) or `top` (face B);
- `flooded`: the open part holds the fluid around it, as ladders do (§5).

`"adaptive"` is turned off for every material, so Phase 18's derivation is unused on the branch
(kept, not deleted, until the owner decides).

## 4. The generation rule

For a natural column whose top cell is row `t − 1` (the column holds rows up to `t − 1`), and its
8 neighbours' column heights in rows (from the terrain's height function, so chunk borders need
nothing extra):

1. **Ramp.** The top cell is a ramp facing `d` if the edge neighbour toward `d` is lower (`< t`),
   the opposite edge neighbour is not lower (`≥ t`), and either:
   - **(slope)** both side neighbours, and the two diagonals on the opposite side, are not lower;
     or
   - **(one wide)** the opposite neighbour is higher (`> t`) and both side neighbours are lower:
     a path or stair one block wide, which becomes a run of ramps.

   At most one facing can satisfy this (each pair of facings contradicts on an edge neighbour).
2. **Steep.** A ramp facing `d` whose neighbour toward `d` is at least 1 m lower (`≤ t − 2`)
   becomes a 1 m ramp instead: the top cell is the steep upper piece and the cell below it the
   steep lower piece, both facing `d` and both of the surface material.
3. **Otherwise flat.**

Everything below the top cell (or the top two, on steep ground) stays a full block. The rule is a
pure function of the 9 column heights, so the generator, its tests and the LOD can apply it alike.
It is a small fixed vocabulary in code. The pieces it may name are data, and naming one the
registry lacks fails generation with a message.

## 5. Edits, plants and water

- **Breaking** a cell removes that cell. No other cell changes; a piece left without the cell it
  leaned on stays as it was. That includes a steep upper piece over a removed lower one: blocks
  float as they always have.
- **Placing** a natural block places it flat (`shape=full`). A crafted block keeps its own shape.
- **Tall grass** is placed by generation and by players only on a top whose shape is `full`, and
  it is removed if the cell under it stops being full (the existing `needsSupport` rule, extended
  to the shape).
- **Water.** A ramp piece whose open part lies under the fluid level, or beside fluid in its row,
  is generated with `flooded=true` and drawn holding the fluid, as Phase 18's shores were.

## 6. What changes where

The plan's Phase 41 lists the steps and their checks. In short:

- **Physics:** the terrain collider's voxel size is the cell (1, ½, 1); trimesh vertices of shaped
  cells are scaled to metres; the voxel queries step in cell space. The controller's tunables are
  in metres and stay. Probes that counted blocks (auto-jump's "one-block obstacle", the ladder
  column, footstep cells) count metres.
- **Rendering:** chunk and LOD meshes are emitted in metres, so normals of sloped faces stay
  correct. Textures run in metres, so a 1 m tile spans two cells. The ray marcher steps in cell
  space with its rays transformed into it (not yet: §7).
- **Worldgen:** terrain heights stay in metres; rows are `ceil(height ÷ cellHeight) − 1`; decoration
  heights are in metres. Generator version bump; chunk and LOD goldens regenerated.
- **Structures:** authored in cells, so existing content would come out half as tall. The branch
  doubles each layer of the prototype structures once, by script.
- **Compatibility:** protocol version bump (cell coordinates change meaning), new registry hash.
  `package.json` is not raised on the branch (the owner decides at a merge).

## 7. As built (H4, 2026-10-20)

What the switch to ½ m cells did, and what it left for the playtest:

- **Built:** `cellHeight` 0.5 with the rows doubled (metres kept); protocol version 14; generator
  version 14 (now 1016, §8) with its chunk and LOD goldens re-recorded at the same places in metres; no block
  adaptive; texture v in metres from each cell's top (the grass fringe hangs from every grass
  cell); worldgen depths and the shore in metres; the decoration rule's heights and slope in
  metres (`decorations.json`); the movement playground's layers doubled. The controller suites
  walk generated ground (`dwell-physics` `tests/ramps.rs`, replacing Phase 18's adaptive suite): ½ m
  ramps and placed ½ m blocks are walked without jumping, 45° ramps slide, and placing a block
  beside a player leaves the ground under them alone.
- **Found and fixed:** face B's LOD sections were mirrored about a hard-coded row (−4,096, the
  1 m grid's 2 × midplane), so on ½ m cells face B's distant terrain read the wrong rows; the
  LOD section id gave each level 512 row slots, fewer than level 0's 1,024 rows on ½ m cells
  (ids collided). Both have regression tests.
- **Found (H4; fixed in H6, §8):** the prototype terrain then never stepped a metre between neighbouring columns, so it
  generates only ½ m ramps; the 45° ramps exist where the ground is steeper (tests build them).
- **Not converted (deviations, for the playtest to judge):**
  - the ray marcher (local builds only, A7) still assumes 1 m cells: it draws ½ m cells stretched;
  - brushes stay in cells: radii and heights count cells, so a ball is half as tall as wide;
  - a fluid's `surfaceDrop` stays a fraction of its cell (water's surface 1/16 m below a ½ m cell's
    top instead of 1/8 m);
  - the chunk view sphere (`VIEW_RADIUS_CHUNKS`) is in chunks, so it reaches half as far up and
    down in metres;
  - the LOD draws natural tops flat (a ½ m ramp is below a level-1 cell);
  - structures carry no cell height: one saved on `main` (1 m cells) comes out half as tall here.

## 8. After merging `main` (H6, 2026-10-20)

- **Versions:** protocol and generator **1016**, 1000 + the `main` version last merged, so a dev
  build never joins or loads one of `main`'s.
- **Phase 16's world:** the midplane at row 0; sea level, the flat worlds' top and the core are rows
  (64, 64, 20: 32 m, 32 m, 10 m), with metre twins. The build sphere (`bounds`) is in cells, so it
  reaches 25 km up and down (50 km across); nothing gets near either.
- **Steep ground (main's v16 hills):** the height field and its steepness are main's
  (`shared/worldgen/terrain.json`). Where a 4 m lattice cell rises at least `steps.fromSlope`
  (0.5 m/m), heights snap to whole `steps.height` (1 m), so steep ground is 1 m steps of two ½ m
  cells; the rule (§4) tops each with a 45° ramp, and a slope near 45° is a run of them. About 13 %
  of land tops are 45° pieces (the slide's share), against 3 % without the steps.
- **Brushes:** radii, heights and the preview's cells count cells (a ball is half as tall as it is
  wide); the preview is drawn in metres; reach is metres. A metric brush is a later step.
- **Edits:** the server's line of sight passes plants, so a block under tall grass can be broken
  where the client targets it (on ½ m cells the plant's cell is half as tall, and every sight ray
  crossed it).

