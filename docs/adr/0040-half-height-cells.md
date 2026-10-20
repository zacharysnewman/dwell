# 0040. Half-height cells with stored cardinal ramps, shaped at generation

- Status: Accepted for a playtest on a long-lived branch (`claude/half-height-voxels`); it
  reaches `main` only if the owner keeps it after playing it
- Date: 2026-10-19
- Design: [`HALF_HEIGHT_GRID.md`](../HALF_HEIGHT_GRID.md); investigation:
  [`proposals/HALF_HEIGHT_VOXELS.md`](../proposals/HALF_HEIGHT_VOXELS.md) (adopted) and the
  [Half-Height Voxel Lab](../prototypes/half-height-lab.html)
- Supersedes on the branch: [0024](0024-adaptive-natural-blocks.md) (adaptive natural blocks: the
  shapes are stored, not derived); revisits [0024](0024-adaptive-natural-blocks.md)'s rejected
  option 3 (finer voxels)

## Context

On today's 1 m grid, exposed natural ground rests half a block down, so its corners take three
levels (floor, half, top). From those come the shapes the owner calls weird: half-and-half, tilt,
dimple, the saddles and the ridge pieces. Flat ground ends inside its top block, so anything
standing on it needs an offset. Tall grass is drawn from the floor of its cell and floats ½ m above
the meadow it grows on (measured on `main`, 2026-10-19).

The owner investigated a half-height grid in the lab (2026-10-19): ½ m cells, 1 m double blocks,
and fixed sets of stored shapes against the adaptive rules. The owner then decided what to try in
the game.

## Options considered

1. **Keep 1 m cells with adaptive shapes** (Phase 18). This is built and tested, and needs half the
   data. It keeps the three-level shapes, and the plant offset needs fixing separately.
2. **½ m cells with adaptive shapes.** The rule set shrinks (no Rest or Rise) and flat ground falls
   on a cell boundary. Shapes are still derived on every edit, which the owner does not want.
3. **½ m cells with ½ m and 1 m double blocks.** This gives 1 m building in one click, but a block
   that spans two cells is a new engine concept, and 1 m blocks between ½ m and 1 m drops need
   mixed pieces (the lab's 142–165 blocks off the rules).
4. **½ m cells, a small set of stored cardinal ramps, shaped once at generation** (chosen). A 1 m
   rise is two ½ m cells holding the two halves of one 45° ramp. Shapes are block states; nothing
   reshapes after generation.

## Decision

The owner's decisions (2026-10-19), with their answers to the follow-up questions:

- **Cells are 1 × ½ × 1 m.** There are no double blocks: every block is one ½ m cell.
- **Natural ground uses cardinal ramps only**, each facing north, east, south or west:
  - a **½ m ramp** (27°) in one cell;
  - on steep ground, a **1 m ramp** (45°) built from two ½ m cells: a lower piece (flat for half its
    run, then sloping) and an upper piece (sloping, then open).
  
  There are no corner, saddle or ridge pieces. A spot the set has no piece for is a flat block.
- **Shapes are chosen once, by the generator.** Natural blocks have no adaptive behaviour at run
  time. Breaking or placing a block changes no other block.
- **A placed natural block is a flat block.**
- **Building is ½ m per click.**
- **45° ramps slide.** The 44° slide limit (owner, 2026-10-15) stays; ½ m steps and 27° ramps are
  walked.
- **Tall grass grows only on flat tops.**
- **It is built on a long-lived branch** and playtested before anything reaches `main`.

## Consequences

- The shape vocabulary falls from 29 adaptive shapes (191 states with facings) to 12 pieces: the
  ½ m ramp and the two 45° pieces in 4 facings each. A block is identifiable from its state, for
  example `dwell:grass[shape=ramp,facing=east]`.
- An edit changes one cell: no neighbour re-derivation, and no collider rebuild across the 27
  surrounding chunks. Ground that has been edited goes blocky and can go stale: a ramp beside a
  dug hole keeps facing the way it did. That is the trade the owner chose.
- About twice the cells for the same volume: twice the chunk rows, and more data in saves,
  generation, meshing and collision. To be measured with the bench.
- A breaking change to the world format, the protocol and seed terrain. On `main` it would be a new
  compatibility line, which only the owner raises. The branch does not raise `package.json`.
- Phase 18's adaptive machinery (the rule table, overrides, apron derivation) is turned off by data
  on the branch, not deleted, so that the comparison stays possible until the owner decides.
- Reversal: drop the branch. If kept, the merge supersedes ADR 0024 on `main`.
