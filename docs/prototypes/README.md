# Prototypes

Standalone design prototypes, kept so they survive between sessions. Each is one HTML page
(three.js from jsDelivr; open the file in a browser with internet access, or use the published
link). They are not part of any build; Prettier and CI ignore `docs/`.

| Prototype | File | Published (private to the owner) | Used for |
|---|---|---|---|
| Adaptive Block Lab | [`adaptive-block-lab.html`](./adaptive-block-lab.html) | https://claude.ai/artifact/7gvrFMfUyT6GegnyKPTTqV | The adaptive-block rules on whole scenes and three voxel grids |
| Shape Playground | [`shape-playground.html`](./shape-playground.html) | https://claude.ai/artifact/Sc4Pu3UqX8TcpcrRrEYpVz | Every shape and situation, hand-shaped by the owner; the rules fitted to it |
| Half-Height Voxel Lab | [`half-height-lab.html`](./half-height-lab.html) | https://claude.ai/artifact/DGQZG6GH1GhxvZRtPgh5MN | Today's 1 m cells beside 1 × ½ × 1 m cells with 1 m double blocks: shapes, 27° and 45° slopes, plants standing on the ground, ½ m growth ([proposal](../proposals/HALF_HEIGHT_VOXELS.md)) |
| Tree Growth | [`tree-growth.html`](./tree-growth.html) | https://claude.ai/artifact/MRW4RcNKTHkYdxMyRKLLHz | A hand-built tree grown by revealing its skeleton, obstacles, and jitter variants (Phase 30, G0) |

The first two feed [`ADAPTIVE_BLOCKS.md`](../ADAPTIVE_BLOCKS.md) (Phase 18, formerly 9e) and
[ADR 0024](../adr/0024-adaptive-natural-blocks.md); Tree Growth feeds
[`TREE_GROWTH.md`](../TREE_GROWTH.md) and [ADR 0033](../adr/0033-tree-growth.md).

## Adaptive Block Lab

- Scenes: Basics, Terraces, Hill, Terrain (value noise), Wall & pile (natural ground against
  crafted blocks), Flat meadow. Place, remove, orbit; natural and crafted materials.
- **Playground rules** (default): the five rules of `ADAPTIVE_BLOCKS.md` §2. **Previous rules**:
  the earlier designs, for comparison — a height field with resting heights and slope limiting on
  the 1 m grid (with its Heights, Slope and Shape depth settings), and a per-voxel autotile on the
  finer grids.
- Grids: 1 m cubes, 1 × ½ × 1 m voxels, ½ m cubes (with a 1 m placement brush).
- **Copy 5³** copies a 5 × 5 × 5 block neighbourhood as JSON (blocks, column heights, each column's
  shape and surface) for discussing a spot in chat.

## Shape Playground

- 28 labelled situations (A–AB) on a 32 × 32 grid: small shapes, plateaus, a pit, steps, a cliff,
  a diagonal drop, ramps, 1-wide walls and stairs, one situation for each shape the rules can
  produce, a T and a cross.
- Tap a block to cycle its shape, rotate it, or pick from the shape panel (the game's shapes plus
  the 18 the rules add, marked "not in the game yet").
- **Rules & conflicts**: each rule's follow/break counts, outlines (green follows, red breaks,
  amber: no such shape), and a list of the blocks that disagree with the rules, with "Use the rules'
  shape".
- Edits are saved in the browser; **Your last export** restores the owner's final shapes (embedded
  in the page, 2026-10-08: all 250 blocks match the rules; extracted for the game's tests into
  `shared/blocks/playground-export.json`); export and import as JSON.
- **Copy overrides for the game** (Phase 18): every block whose shape differs from the rules', as
  entries for `overrides` in `shared/blocks/adaptive.json` (`{ "around": "+==/=.-/===", "shape",
  "facing" }`); blocks with one neighbourhood and different shapes are reported as conflicts. Paste
  them in and run `node shared/blocks/gen.mjs`, which validates them and the adjacency test.

## Tree Growth

- **Load a tree**: three built-in samples (prototype content: an oak-like tree with marked roles and
  root flares, a conifer-like one without roles, a wide spreading one with a marked `trunk` and a
  `core` knot), or paste or import a structure JSON in S1's format (`STRUCTURES.md` §3). Palette
  values may be `{"block": ..., "role": ...}` with `wood`, `leaf`, `root`, `trunk` or `core`;
  without a role, names with log or wood are wood and names with leaves are leaves.
- **Skeleton**: extracted as `TREE_GROWTH.md` §2 describes, shown as nodes and links (amber trunk,
  teal branches, red pruned, grey not grown yet); notes under the tree say where inference had to
  guess (loops, stray wood, floating leaves).
- **Age**: a slider in in-game days, play/pause at 10–150 days a second, and Seed, Sapling, Young,
  Mature stops; the trunk days per metre (30), branch ratio (1/3), world speed and tip-leaf radius
  are editable, with the real time to maturity at a 30-minute day. A badge checks that the mature
  stage equals the model voxel for voxel.
- **Obstacles**: click a face or the ground to place a stone; a node whose core runs into one is
  pruned with its subtree.
- **Variation**: a seed with Roll, and branch angle, length, droop and drop chance; the variant row
  shows the authored tree and five seeds at full age; "Grow the variant" grows the jittered tree.
- **Export**: the `growth` and `variants` sections as JSON (with the world's growth speed beside
  them), or the whole structure with them and the palette roles. The state is saved in the browser.

## Half-Height Voxel Lab

- One scene on two grids side by side, with synced cameras: today's 1 m cells with the Phase 18
  rules, and 1 × ½ × 1 m cells with the proposal's four rules (`proposals/HALF_HEIGHT_VOXELS.md`
  §2). Either view can be shown alone.
- Scenes: Meadow, Basics (lone ½ m and 1 m bumps, a ridge, ½ m and 1 m one-wide stairs, pits, ½ m
  and 1 m terraces), Hill, Terrain, Growth rows. They are given as surface heights in metres and
  realised as each grid best can.
- Blocks in the half grid are ½ m (one voxel) or 1 m double blocks (two voxels, one block). A
  block's corners drop at most to its floor, so ½ m blocks make 27° slopes and 1 m blocks 45° ramps.
  **1 m on steep ground** (default) puts a double block on top of any column 1 m or more above any
  of its 8 neighbours; **½ m blocks only** compares without them.
- Tools: Place (a ½ m block or a 1 m double block in the half grid) and Remove (the top block),
  Plant and Grow (both views). Grow all, Mow, Scatter, and grow over time.
- Grass grows only on flat tops (a full block on the ½ m grid, a level resting top on the 1 m
  grid); a plant whose ground stops being flat goes. Changing the shape set re-plants the scene's
  grass on what is flat.
- Plant footing: at the cell floor (as the game draws plants) or sunk to the shaped surface.
  Floating plants are tinted red and counted per view.
- **Slope shapes (½ m view)**, its own section at the top: **Adaptive (all shapes)** (reshaped from
  the neighbours on every edit), **Cardinal ramps + corners** (ramps facing N, E, S, W plus outer
  and inner corners, at ½ m and 1 m: 24 pieces and the full block), **Cardinal ramps only** (the 8
  straight ramps and the full block) or **No slopes** (flat ½ m blocks). The fixed sets are
  stored: picked when the scene
  is built or a block is placed, never changed by a neighbour. Dug-out ground and shapes without a
  piece are full blocks. The view counts the blocks off the rules.
- Grass blocks' sides are drawn as the game's grass side texture: dirt with a ragged grass skirt
  (4–9 px of a 32 px tile, 0.12–0.28 m) hanging from the face's top edge, so it follows shaped tops.
- Show cells outlines the top ground block (both voxels of a double block) and the cells a plant
  occupies. The cell inspector lists what each world stores for the
  column under the pointer (`tall_grass {height: 1.5 m}` + `{part: upper}` against three plant
  voxels).

## Updating them

Edit the file here and republish it to the same link, so the URL stays the same (in Claude Code:
"publish `docs/prototypes/<file>` to <link>"; the Artifact tool updates an existing artifact by its
URL after reading it). Commit the file with the change.
