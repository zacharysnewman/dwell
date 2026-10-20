# Growing and varying hand-built trees

**Status:** Adopted (ADR 0033), 2026-10-14 ([ADR 0033](./adr/0033-tree-growth.md)). Built as Phase 30 of the
[implementation plan](./IMPLEMENTATION_PLAN.md) (steps G-ADR, G0–G4). Owner answers in §7. Builds on the
structure system ([`STRUCTURES.md`](./STRUCTURES.md), ADR 0031, Phase 27).

The ask: take a custom-built tree model and extrapolate from it so it "grows" over time, and produce
variation from it.

## 1. The idea

Treat the hand-built tree as the **mature target**, not as a seed for a simulation:

- When the structure generator (`gen.mjs`) loads it, it extracts a **skeleton graph** from the wood
  voxels once.
- **Growth reveals that graph** in order of distance from the root. Thickness follows the pipe model
  (a branch's radius is `sqrt(Σ child radius²)`, as in Dynamic Trees), and leaves fill in around the
  tips grown so far.
- **The last stage is exactly the authored model.** Every stage is a pure function of
  `(structure, variant seed, age)`, so it can be recomputed instead of stored.
- **Variation** jitters the skeleton (branch angles, lengths, droop, dropped branches) and rasterises
  it again; scaling scales the skeleton, not the voxels.

## 2. Reading the model

- **Roles are marked, not inferred**: the palette gains roles — `wood`, `leaf`, optionally `root`
  and `core` (voxels never jittered, e.g. a carved knot). The same block can't otherwise tell trunk
  from branch.
- **The skeleton is inferred**: a breadth-first geodesic distance field from the anchor through
  26-connected wood; voxels at equal distance group into nodes (centroid, radius from the distance
  transform), each linked to its predecessor (Verroust & Lazarus 1999). Topology-preserving thinning
  (Lee et al. 1994) is the alternative. Branch tips are the graph's leaves.
- **Leaf volumes**: each leaf voxel belongs to its nearest skeleton node, so growing a node reveals
  its leaves.

## 3. Growth engines compared

| Engine | Fidelity to the model | Cost | Data |
|---|---|---|---|
| **Skeleton reveal** (recommended) | Final stage exact; intermediate stages inferred but plausible | Cheap: order and radii computed once in `gen.mjs` | One model + growth parameters |
| **Authored stages** | Exact at every stage; no in-between | Free at runtime; 3–5 models per species | A `stages` list of structures |
| **Space colonization** (Runions et al. 2007), canopy as the attractor envelope | Fills the model's shape, not its exact branches | Heavy; once at planting or offline | Envelope + parameters |
| **L-systems** | Poor unless fitted by inverse modelling (research-grade) | Cheap | Hand-written grammars |

Precedents: Minecraft places the whole tree at once from a sapling on random ticks; **Dynamic Trees**
grows block by block from data-driven species with a pipe-model radius and "choked" branches that
stop — the closest match; Vintage Story's trees are fully parametric JSON; Veloren mixes hand-built
`.vox` trees with seeded procedural ones rasterised from segments.

## 4. Variation from one model

- Rotate and mirror (already planned).
- Skeleton jitter: rotate subtrees about their fork (±θ), lengths ×(1±ε), droop by depth, drop a
  branch with probability p; rasterise again with leaf volumes moved with their nodes; `core` stays
  rigid.
- Leaf thinning by a seeded hash; leaf colour by weighted `choices`.
- Scale the skeleton (lengths × s, radii by the pipe model, one-voxel minimum) — naive voxel scaling
  turns twigs into slabs and breaks diagonal joins.

## 5. Growth in the multiplayer world

- **Scheduled, not random ticks**: `stage(t) = f(hash(worldSeed, root, structure), t − planted)`.
  Whenever the tree's chunks are loaded, compute the target stage and apply the difference — so
  catching up after a chunk was unloaded is the same code.
- **Persisted**: a small `growers` record per tree (structure, variant seed, planted tick, stage
  applied, pruned nodes) plus the voxels as usual. Players' changes can't be recomputed, so voxels
  stay authoritative. A new world-file table is a format change: a **new compatibility line**.
- **Obstacles**: never overwrite anything but air or leaves; a blocked node prunes its subtree
  (deterministic, never deletes player blocks).
- **Cuts**: breaking wood prunes that node's subtree from the record; optionally leaves cut off from
  wood decay.
- **Phase 20 physics**: growth only adds connected voxels, so it can't detach anything. A cut trunk
  leaves an unanchored crown that falls as a body — felling for free. Leaves must not count as
  support.
- **Network**: one stage is about 50–300 voxels, 200 B–1.2 KB in one `VoxelModification` with a new
  `Growth` reason (an additive protocol change).
- **Worldgen**: if worldgen places already-grown trees, the growth algorithm becomes terrain a seed
  generates (public API), hashed into the terrain definition. *Since 2026-10-20 (generator
  version 17, ADR 0042) worldgen places the owner's two pines — mature, not growing — as plain
  voxels of the generated chunk ([`WORLD_GENERATION.md`](./WORLD_GENERATION.md) §2).*

## 5a. The owner's persistence model (2026-10-14), mapped onto Dwell

The owner proposed a five-part model for persistent chopping and planting across tens of thousands
of trees. It fits Dwell, and parts of it already exist:

| Owner's part | In Dwell today | What trees add |
|---|---|---|
| **1. Spatial grid**: chunks as the unit of loading, saving, simulating | Built: 32³ chunks, generate-on-access, eviction of unmodified chunks far from players | Tree records keyed by the chunk holding the root; a tree spanning chunks applies a stage only when all its chunks are loaded |
| **2. Seed determinism**: unmodified trees regenerated from the seed, 0 bytes stored | Built for terrain: a revision-0 chunk is a pure function of the seed; the client generates chunks and the LOD from the seed too | Procedural trees: placement by `hash(seed, chunk, index)`, so a tree's id is `(chunk, procedural index)`. Placement is deferred (ADR 0031) and changes seed terrain (new compatibility line) |
| **3. Tombstones**: a sparse set of destroyed procedural tree ids per chunk | Partly: any edit stores the **whole chunk's voxels** (palette + RLE), not a tombstone | A per-chunk sparse set of chopped tree ids, plus per-tree pruned node sets for partial cuts |
| **4. Entity pools**: explicit records for player-planted trees | Not built; the world file (redb) has no entity tables | A `growers` table: structure, variant seed, root, planted tick, pruned nodes |
| **5. Analytical catch-up**: evaluate state from `now − planted` on load | Not built | `stage(t)` as in §5, evaluated on load and on a slow schedule while loaded |

**The one change it forces: keep trees out of the chunk voxels.** Dwell saves any edited chunk whole.
If trees were plain voxels in it, the first edit nearby would freeze its trees at that moment, and a
growing tree would make a revision-0 chunk a function of the seed *and the time*, which the client's
own chunk and LOD generation would then need too. So trees become a layer composed at load, in
order:

1. generated terrain (the seed, as today) and its saved edits (as today);
2. procedural trees not in the chunk's tombstone set, at `stage(now)`;
3. player-planted trees from `growers`, at `stage(now)`;
4. growth never overwrites anything but air or its own leaves (blocked nodes prune).

Chopping a tree voxel adds the node's subtree to that tree's pruned set (or the tree to the tombstone
set when the trunk goes) rather than editing terrain voxels. The client composes the same layers from
the same records (sent with the chunk), so the server and client agree without streaming tree voxels.
Phase 20 sees the composed voxels; a felled crown becomes a body and leaves the tree record pruned.

**Built so far (2026-10-20), a deviation from this layer:** the pines worldgen places
([ADR 0042](./adr/0042-structure-placements.md)) don't grow, so they are part of step 1 — the
generated terrain — not a layer: a chunk stays a pure function of the seed, and an edited chunk is
saved whole as today, so a chopped pine stays chopped with no tombstone. When G3 builds the layer,
procedural trees move into it; their id is already `(placement, grid cell)` (`placements.rs`
`Tree::cell`), and the move is a new generator version.

Costs: an untouched world stores nothing for its trees; a chopped procedural tree costs one id in a
set; a planted tree one small record. The world-file tables and the chunk message's tree records are
format and protocol changes — a new compatibility line, as is procedural placement.

## 6. Data sketch and plan

```json
{ "format": 1, "name": "dwell:oak_a", "kind": "voxels", "size": [15, 22, 15], "anchor": [7, 0, 7],
  "palette": { ".": null,
    "w": { "block": "dwell:proto_wood",   "role": "wood" },
    "r": { "block": "dwell:proto_wood",   "role": "root" },
    "l": { "block": "dwell:proto_leaves", "role": "leaf" } },
  "layers": [["..."]],
  "variants": [
    { "op": "rotate", "angles": [0, 90, 180, 270] },
    { "op": "branch_jitter", "angle_deg": 12, "length": 0.15, "droop": 0.1, "drop_chance": 0.1 },
    { "op": "leaf_thin", "density": [0.8, 1.0] } ],
  "growth": { "engine": "skeleton_reveal", "trunk_days_per_m": 30, "branch_rate": 0.333, "jitter_days": 0.25,
    "thickness": { "op": "pipe_model", "tapering": 0.3, "min": 1 },
    "leaves": { "op": "around_tips", "radius": 3 },
    "blocked": "prune_subtree" } }
```

The engines and ops are a small fixed vocabulary validated by `gen.mjs`; the extracted skeleton is
part of the generated output (stale-checked). Wood and leaf blocks don't exist yet; any added are
prototype content. The 32³ cap and JSON fields must match what S1 actually built.

| Step | Work |
|---|---|
| **G0** | A prototype page (`docs/prototypes/tree-growth.html`): load an exported structure, show the skeleton, scrub age, roll variant seeds. No protocol or release impact; shows whether inferred stages look right |
| **G1** | Roles and `growth` in the format; skeleton extraction in `gen.mjs`; Rust `stage_voxels(structure, seed, age)`; tests: final stage equals the model, a fixture tree by data alone |
| **G2** | Sandbox "preview growth" command (after S2a/S3) |
| **G3** | Growth in the world (§5a): the tree layer composed at chunk load, tombstones, the `growers` table, scheduled catch-up from world time, the `Growth` reason, pruning on obstacles and cuts, the per-world speed setting (a new compatibility line; the owner decides the bump) |
| **G4** | Jitter variants (space colonization not needed for now, owner §7 #5; a second engine would need an ADR) |

**Risks**: ambiguous skeletons on chunky hand-sculpted trunks (author marks fix it); adaptive blocks
(Phase 18) reshaping wood or leaves; trees across chunk borders need all their chunks loaded to grow
a stage; world format and protocol both change at G3.

## 7. Owner answers (2026-10-14)

| # | Question | Answer |
|---|---|---|
| 1 | How fast, in stages or block by block? | **The trunk grows 1 m every 30 in-game days**, in 1 m (one-voxel) steps; **branches lengthen at 1/3 of that speed** (1 m per 90 days), or the tree ends up wonky. In-game days aren't specced yet; assume a 30-minute day (20 min day, 10 min night), so a trunk step every **15 real hours** and a branch step every 45. Rates are in in-game days, so they follow whatever day length is chosen, and the branch ratio is a growth parameter in the structure data (default 1/3). A node appears at `t = trunk distance to its fork / trunk rate + distance along branches / branch rate`, so the final stage is still exactly the model. This needs the trunk told apart from branches: inferred as the thickest path from the root, overridable by a `trunk` palette role |
| 2 | Hand-approved or computed stages? | **Computed** (the skeleton reveal) |
| 3 | Obstacles: stop or grow around? | **Stop**: a blocked node prunes its subtree |
| 4 | Cut trunk, leaf decay? | **Phase 20's plan**: the cut-off part becomes its own physics body, settles and re-bakes into the ground (ARCHITECTURE §7.1–7.3); leaf decay not asked for |
| 5 | Kind of variation? | **Branches placed differently, longer or shorter** (and their leaves with them): skeleton jitter (§4). Space colonization isn't needed |
| 6 | Grafting parts between models? | **No** |
| 7 | Per-world growth speed? | **Yes**: a per-world growth-speed multiplier, stored with the world's settings and applied by the server |
| — | Procedural or planted trees? | **Both**, from the owner's persistence model (§5a) |

Notes:
- The day length now lives in the day/night cycle's data (Phase 32, [`DAY_NIGHT.md`](./DAY_NIGHT.md)):
  `clock` in `shared/look/daynight.json` (`dayS` 1200 + `nightS` 600 = a 30-minute day), generated
  into `dwell_look::daynight::DAY_NIGHT`; `DAY_NIGHT.days(world_s)` is the in-game day count from
  world time (the server's tick, saved in `meta` as `world_tick`), which `stage(t)` reads. Growth catch-up uses world time, not wall-clock time, so a world that isn't
  running doesn't grow.
- With a step every 15 real hours, the sandbox needs a way to preview growth (G2: a "preview growth"
  command or slider) — nobody will wait for it.

## 8. Findings from the G0 prototype (2026-10-15)

The prototype page ([`prototypes/tree-growth.html`](./prototypes/tree-growth.html)) builds the
skeleton reveal on three hand-built samples (oak-like 23 × 22 × 23, conifer-like 15 × 25 × 15,
spreading 25 × 14 × 25). On all three the mature stage equals the model voxel for voxel, and the
intermediate stages read as a sapling, a young tree and a mature one. What it took, for G1:

- **Skeleton inference held up on the samples.** BFS levels through 26-connected wood, grouped into
  connected components per level, gave 49–84 nodes; the trunk as the heaviest path (by subtree wood
  volume) found the right stem on the oak and the conifer without marks. Where it struggles:
  - **Forks merge.** Where limbs leave a thick trunk, they share one level set for a few levels, so
    that node's centroid sits between them and its radius covers all of them. Thickening by "distance
    from the node's own centre" bulked young limbs out at the fork. Fixed by measuring each voxel's
    radial distance from the **nearest core line** (its node's, or a child's passing through), and
    letting it thicken with that line's node.
  - **Loops** (two branches touching) give a node two parents; it keeps the one it touches most.
  - **An even split** (a spreading tree whose trunk becomes five equal limbs) has no heaviest path
    worth the name; the sample marks its trunk with the `trunk` role, which the page honours.
- **Connectivity by construction.** Each node has a core path (shortest wood path from its medial
  voxel to its parent's), revealed as soon as the node grows, so every stage is one connected piece.
  Radial voxels are revealed by geodesic distance from the core, so a threshold never strands one.
- **Thickness.** The pipe model ratio `P_now / P_full` (exponent 2, tips 1) scales each node's
  authored radius, plus ½ voxel of rounding while still growing; without the rounding a trunk of
  authored radius 1 stayed one voxel wide until the very last day.
- **Roots** at the branch rate (a deviation): a one-voxel sapling stood on full-size root flares. The
  page grows roots from 40 % of maturity to 100 %, with their depth. G1 should decide this as a
  growth parameter.
- **Leaves.** Authored leaves are owned by the nearest wood voxel (geodesic through leaves) and
  appear once that voxel shows, within a reach that widens with the tree's age, so every leaf is in
  place at maturity. Without extra leaves, a sapling is a bare stick: the **tip tuft**
  (`around_tips`) adds leaves only at the growing end of each axis (on the trunk, only its leader,
  not buds waiting for a side branch), sized by the leaves the model has within four nodes beyond
  (minimum 1.2). Tufts disappear at maturity, so the last stage is still exact.
- **Timing consequence** of the owner's rates: with branches at a third of the trunk's speed, the
  conifer's lower whorls are the fullest part mid-growth while its leader is still a bare pole; the
  oak reaches its fork height (10 m, day 300) with one-voxel limbs. Both look like young trees; if
  the owner wants fuller young crowns, the tip-leaf radius is the knob, not the rates.
- **Variants.** Branch jitter as similarity transforms per node (rotation about the parent's axis
  and a half-size tilt at each branch start, a length scale, droop as a small bend at every branch
  node), composed down the tree; voxels forward-mapped with 2 × 2 × 2 supersampling (no holes) and
  core lines redrawn between mapped medial voxels (connected). Trunk, roots and any branch holding a
  `core` voxel stay rigid. A variant at any age is the authored stage mapped through the same
  transforms, so variants grow too. Droop reads better in degrees per metre (`droop_deg_per_m`,
  default 1.5) than the sketch's unitless 0.1.
- **Obstacles.** A node is pruned when its mapped core cells hit an obstacle (or its parent is
  pruned); a radial or leaf voxel is skipped when it, or the voxel it grew out from, is blocked.
  Pruning depends only on the obstacle set, so it is deterministic.
- **Format.** §6's sketch writes `variants` as a list; S1 built it as an object of named operations,
  so the page exports `variants.branch_jitter` beside S1's `rotate` / `mirror` / `choices`, and the
  `growth` block as sketched (with `thickness.exponent` instead of `tapering`). The per-world growth
  speed is exported beside them as `$world_settings`, not structure data.

## Sources

- Runions, Lane & Prusinkiewicz 2007, space colonization: https://algorithmicbotany.org/papers/colonization.egwnp2007.html
- TreeSketch (Longay et al. 2012): https://algorithmicbotany.org/papers/TreeSketch.SBM2012.html
- Stava et al. 2014, inverse procedural modelling of trees: https://graphics.uni-konstanz.de/publikationen/Stava2014InverseProceduralModelling/index.html
- Dynamic Trees (`GrowSignal`, `BasicBranchBlock`, `JoCode`): https://github.com/DynamicTreesTeam/DynamicTrees · https://modrinth.com/mod/dynamictrees
- Vintage Story trees: https://wiki.vintagestory.at/index.php/Modding:Trees
- Veloren trees: https://gitlab.com/veloren/veloren/-/blob/master/world/src/layer/tree.rs
- Minecraft random ticks: https://minecraft.wiki/w/Tick
- 3D skeletonization: https://scikit-image.org/docs/stable/auto_examples/edges/plot_skeleton.html
