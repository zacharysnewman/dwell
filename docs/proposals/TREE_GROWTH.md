# Proposal: growing and varying hand-built trees

**Status:** investigated 2026-10-14, not adopted. Owner questions open (§7). Builds on the adopted
structure system ([`docs/STRUCTURES.md`](../STRUCTURES.md), ADR 0031, Phase 27).

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
- Later: graft a crown from model B onto a trunk from model A at marked attach points; space
  colonization in a jittered envelope for "same species, different tree".

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
  generates (public API), hashed into the terrain definition — and placement itself is deferred
  (ADR 0031).

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
  "growth": { "engine": "skeleton_reveal", "stages": 8, "stage_ticks": [72000, 0.25],
    "thickness": { "op": "pipe_model", "tapering": 0.3, "min": 1 },
    "leaves": { "op": "around_tips", "radius_by_stage": [1, 1, 2, 2, 3, 3, 3, 3] },
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
| **G3** | Planting in the world: the `growers` table (new compatibility line), scheduled catch-up, the `Growth` reason, pruning on obstacles and cuts |
| **G4** | Jitter variants; space colonization as a second engine if wanted |

**Risks**: ambiguous skeletons on chunky hand-sculpted trunks (author marks fix it); adaptive blocks
(Phase 18) reshaping wood or leaves; trees across chunk borders need all their chunks loaded to grow
a stage; world format and protocol both change at G3.

## 7. Questions for the owner

1. Growth visible over minutes or days? In stages or block by block?
2. Must every stage be hand-approved (authored stages), or may in-between stages be inferred?
3. Trees planted by players, placed already grown by worldgen, or both?
4. Obstacles: stop the branch, or grow around?
5. Does a cut trunk fell the tree (a Phase 20 body), and do leaves decay?
6. Variation as "same tree, wiggled" (jitter) or "same species, different tree" (space colonization)?
7. Grafting parts between models: wanted?
8. Growth per build only, or a per-world growth-speed setting?

## Sources

- Runions, Lane & Prusinkiewicz 2007, space colonization: https://algorithmicbotany.org/papers/colonization.egwnp2007.html
- TreeSketch (Longay et al. 2012): https://algorithmicbotany.org/papers/TreeSketch.SBM2012.html
- Stava et al. 2014, inverse procedural modelling of trees: https://graphics.uni-konstanz.de/publikationen/Stava2014InverseProceduralModelling/index.html
- Dynamic Trees (`GrowSignal`, `BasicBranchBlock`, `JoCode`): https://github.com/DynamicTreesTeam/DynamicTrees · https://modrinth.com/mod/dynamictrees
- Vintage Story trees: https://wiki.vintagestory.at/index.php/Modding:Trees
- Veloren trees: https://gitlab.com/veloren/veloren/-/blob/master/world/src/layer/tree.rs
- Minecraft random ticks: https://minecraft.wiki/w/Tick
- 3D skeletonization: https://scikit-image.org/docs/stable/auto_examples/edges/plot_skeleton.html
