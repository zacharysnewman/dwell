# Prototypes

Standalone design prototypes, kept so they survive between sessions. Each is one HTML page
(three.js from jsDelivr; open the file in a browser with internet access, or use the published
link). They are not part of any build; Prettier and CI ignore `docs/`.

| Prototype | File | Published (private to the owner) | Used for |
|---|---|---|---|
| Adaptive Block Lab | [`adaptive-block-lab.html`](./adaptive-block-lab.html) | https://claude.ai/artifact/7gvrFMfUyT6GegnyKPTTqV | The adaptive-block rules on whole scenes and three voxel grids |
| Shape Playground | [`shape-playground.html`](./shape-playground.html) | https://claude.ai/artifact/Sc4Pu3UqX8TcpcrRrEYpVz | Every shape and situation, hand-shaped by the owner; the rules fitted to it |
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
  in the page, 2026-10-08: all 250 blocks match the rules); export and import as JSON.

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

## Updating them

Edit the file here and republish it to the same link, so the URL stays the same (in Claude Code:
"publish `docs/prototypes/<file>` to <link>"; the Artifact tool updates an existing artifact by its
URL after reading it). Commit the file with the change.
