# 0024. Adaptive natural blocks: shapes derived from neighbours by five rules

- Status: Accepted (2026-10-12, amended: the table as data with overrides)
- Date: 2026-10-08
- Amended by: [0028](0028-rust-port.md) (the table is read by the Rust crates, `dwell-blocks`, shared by server and client, instead of C++ and TypeScript readers)
- Amended: 2026-10-12, by the baseline rework ([0025](0025-baseline-world.md),
  [`BASELINE_EVALUATION.md`](../BASELINE_EVALUATION.md) §3.7): the rule table is data, with
  per-neighbourhood overrides the Shape Playground exports; every solid block in the baseline is
  natural, so the placed families are removed outright. Built as Phase 18.
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

**Amended (2026-10-12): the table is data.** Its source of truth is a data file
(`shared/blocks/adaptive.json`): the entries the five rules generate, plus **overrides** per
neighbourhood. The blocks' generator checks that every entry names a known shape and facing and runs
the exhaustive no-holes adjacency test over the result, and emits the C++ and TypeScript tables from
it; CI fails on stale output. The **Shape Playground exports overrides** in that format, so the
tool that designed the rules keeps editing the result. Changing how one neighbourhood is shaped is
then an override, not a change to a rule.
The shape set gains 12 corner shapes and 6 ridge shapes. Adaptive materials lose their placed
slope and slab families; crafted materials keep theirs. **Amended (2026-10-12):** the baseline has
no crafted materials — grass, dirt, stone and sand are all `"adaptive": true`, by data — so every
placed slope and slab family is removed ([0025](0025-baseline-world.md)); the `shapeFamilies`
mechanism stays in the generator, unused, for a future crafted block. The generator's slope stage is removed.
The 1 m grid stays.

## Consequences

- Building with natural blocks is placing and breaking cubes; the ground shapes itself. Edits stay
  one voxel; an edit re-shapes up to 9 neighbouring top blocks, so neighbouring chunks re-mesh and
  rebuild collision.
- Meshing and collision need the diagonal neighbours and the layer above in their apron.
- Generation gets simpler (no slope stage) but its terrain changes: a generator version bump and,
  with the registry hash, a **new compatibility line** (the owner raises `client/package.json`).
- The LOD applies the same table to its cells, so near and far terrain follow one rule.
- Phase 20 (voxel awakening; Phase 14 when this was written) must decide whether a detached
  cluster keeps its blocks' shapes or re-derives them; three ridge shapes are not convex.
- With overrides, the table can drift from the rules; the generator reports how many entries are
  overridden, and the exhaustive adjacency test guards the result whatever its source.
- Reversal: shapes are derived, so turning adaptivity off for a material (a data change) returns
  its blocks to cubes without touching saved worlds.
