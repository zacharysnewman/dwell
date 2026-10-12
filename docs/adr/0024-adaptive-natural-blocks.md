# 0024. Adaptive natural blocks: shapes derived from neighbours by five rules

- Status: Proposed
- Date: 2026-10-08
- Partly supersedes: [0016](0016-slope-blocks.md) for natural materials (placed slope families
  and the generator's slope stage); 0016 stands for crafted materials and the shape table.

## Context

Phase 9 ([ADR 0016](0016-slope-blocks.md)) added slope blocks as registry block families that
players place and rotate by hand, and a generator stage that rounds the continuous surface into
slope pieces. Natural ground built by players stays blocky unless they shape every block, and the
generated slopes needed two playtest fixes (gaps under stacked pieces, unsloped steps) because a
stored shape can disagree with its neighbours.

The owner wants natural blocks to always look clean and connected when they touch, without
placing slopes. Two prototypes (`docs/prototypes/`) explored it: a lab applying candidate rules to
whole scenes on three voxel grids, and a playground where the owner shaped labelled situations by
hand and the rules were fitted to those choices. Design and details:
[`ADAPTIVE_BLOCKS.md`](../ADAPTIVE_BLOCKS.md).

## Options considered

1. **Keep placed slopes** (Phase 9 as is). No engine change; natural ground stays blocky unless
   hand-shaped.
2. **A smooth height field** over column tops (shared corner heights, slope limiting). Clean
   surfaces, but shapes cross block boundaries, so it is unclear where a block starts and ends when
   building and breaking; diagonal 2-block drops leave slivers or must cut into lower blocks.
3. **Finer voxels** (1 × ½ × 1 m, or ½ m cubes with multi-voxel blocks) so every shape fits its
   cell. 2× or 8× the voxels for the same volume, a non-cubic grid (option a) or much heavier
   physics (option b), and a breaking change everywhere.
4. **Per-block shapes from the 8 neighbours by a fixed rule set, derived, not stored** (chosen).
   Every shape stays inside its own block; the shape is a pure function of occupancy.

## Decision

Natural blocks (`"adaptive": true` in the block data) are stored as cubes and **shaped at use**
from the 3 × 3 columns around them by five rules (`ADAPTIVE_BLOCKS.md` §2): per corner, **rest**
at half height, **drop** to the floor beside anything lower, **rise** to the top beside something
higher with nothing lower; a block whose corners all drop is a **half-height ridge** toward its
same-height neighbours, or a **45° ramp** up to a single higher neighbour. Only the top block of a
column is shaped, only its upper surface; a block with another on top is a full cube.

The rules are baked into one table (6,561 neighbourhoods → shape and facing) in `shared/blocks/`,
read by the C++ server and the TypeScript client, so collision, prediction and rendering agree.
The shape set gains 12 corner shapes and 6 ridge shapes. Adaptive materials lose their placed
slope and slab families; crafted materials keep theirs. The generator's slope stage is removed.
The 1 m grid stays.

## Consequences

- Building with natural blocks is placing and breaking cubes; the ground shapes itself. Edits stay
  one voxel; an edit re-shapes up to 9 neighbouring top blocks, so neighbouring chunks re-mesh and
  rebuild collision.
- Meshing and collision need the diagonal neighbours and the layer above in their apron.
- Generation gets simpler (no slope stage) but its terrain changes: a generator version bump and,
  with the registry hash, a **new compatibility line** (the owner raises `client/package.json`).
- The LOD applies the same table to its cells, so near and far terrain follow one rule.
- Phase 14 must decide whether a detached cluster keeps its blocks' shapes or re-derives them;
  three ridge shapes are not convex.
- Reversal: shapes are derived, so turning adaptivity off for a material (a data change) returns
  its blocks to cubes without touching saved worlds.
