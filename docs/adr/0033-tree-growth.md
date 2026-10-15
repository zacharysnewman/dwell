# 0033. Tree growth: hand-built trees revealed along their skeleton, as a layer composed at load

- Status: Accepted
- Date: 2026-10-14
- Builds on: [0031](0031-data-driven-structures.md) (data-driven structures, the sandbox, worldgen
  placement deferred), [0014](0014-versioned-releases.md) (compatibility lines),
  [0006](0006-world-persistence-sqlite.md) and [0028](0028-rust-port.md) §8 (one world file, redb)
- Design: [`TREE_GROWTH.md`](../TREE_GROWTH.md) (the owner's answers in §7, the persistence model in
  §5a), [`STRUCTURES.md`](../STRUCTURES.md)

## Context

The owner wants a custom-built tree model to "grow" over time, and variations made from it. Trees
are structures (ADR 0031): hand-built in the sandbox, JSON in `shared/structures/`, per build, added
only by the owner. Nothing grows in Dwell today: there are no entities in the world file, no
scheduled world changes, and no day/night cycle (the world file keeps a world time in `meta`). Any
edit stores the whole chunk's voxels, and a chunk nobody edited is a pure function of the seed, which
the client also generates.

The owner's answers (2026-10-14, `TREE_GROWTH.md` §7): the trunk grows **1 m every 30 in-game
days**, in one-voxel steps, and **branches at a third of that speed**; stages are **computed**, not
hand-approved; growth **stops at obstacles**; a cut tree follows **Phase 20's plan**; variation is
**branches placed differently, longer or shorter**; **no grafting**; a **per-world growth-speed
multiplier**; **both procedural and planted trees**, with the owner's five-part persistence model
(spatial grid, seed determinism, tombstones, entity pools, analytical catch-up).

## Options considered

- **The growth engine**: a **skeleton reveal** of the authored model (chosen: the final stage is
  exactly the model, intermediate stages are computed, one model per species); hand-authored stages
  (exact, but 3–5 models per species and no in-between; the owner chose computed); space
  colonization (fills the shape, not its branches; not needed for now); L-systems (poor fidelity to
  a hand-built model).
- **Where tree voxels live**: plain voxels in the saved chunk (simple, but the first edit nearby
  freezes the chunk's trees, and a growing tree makes an unedited chunk depend on time, which the
  client's chunk and LOD generation would then need), or **a tree layer composed at chunk load from
  small records** (chosen: an untouched world stores nothing for its trees).
- **Random ticks** (Minecraft) or **scheduled stages from world time** (chosen: catching up after a
  chunk was unloaded is the same code as growing while loaded).
- **Obstacles**: grow around them, or **stop and prune the blocked subtree** (chosen by the owner;
  deterministic, never deletes a player's blocks).

## Decision

1. **The model is the mature tree.** A tree is a `kind: "voxels"` structure whose palette marks
   **roles** (`wood`, `leaf`, optionally `trunk`, `root` and `core`, the last never jittered) and
   which carries a **`growth`** block (an engine name, `skeleton_reveal` the only one, and its
   parameters). `shared/structures/gen.mjs` extracts a **skeleton graph** from the wood once
   (geodesic distance from the anchor through 26-connected wood, grouped into nodes; the trunk the
   thickest path from the root unless marked `trunk`; each leaf voxel owned by its nearest node) and
   emits it with the structure's canonical output, stale-checked in CI. The engines and operations
   are a small fixed vocabulary validated by the generator.
2. **Growth is a computed reveal of the skeleton.** One Rust function, `stage_voxels(structure,
   variant seed, age)`, is shared by the server, the WASM core and the client. A node appears at
   `t = trunk distance to its fork / trunk rate + distance along branches / branch rate`, in
   one-voxel steps; thickness follows the pipe model; leaves appear with their nodes. The final stage
   is exactly the model (a test pins it). Stages are computed, never hand-approved.
3. **Rates are in in-game days.** The trunk grows 1 m per 30 in-game days (`trunk_days_per_m: 30`)
   and branches at **`branch_rate`** of that (data, default 1/3). The in-game day is not designed
   yet; until it is, assume a **30-minute day (20 minutes day, 10 night)**, so a trunk step every
   15 real hours and a branch step every 45. Growth reads the world time (`meta`), not wall-clock
   time, so a world that isn't running doesn't grow; when the day/night cycle is designed, the rates
   follow its day length.
4. **A per-world growth-speed multiplier**, stored in the world's `settings` and applied by the
   server (default 1).
5. **Obstacles stop growth**: growth writes only into air or the tree's own leaves; a blocked node
   prunes its subtree, deterministically.
6. **Cuts follow Phase 20**: breaking a tree's wood prunes that node's subtree from the tree's record
   (or tombstones the tree when the trunk goes); the cut-off part, no longer anchored, becomes its own
   physics body that settles and re-bakes (ARCHITECTURE §7.1–7.3). Leaves never count as support.
   Growth only adds connected voxels, so it never detaches anything. Leaf decay is not part of this.
7. **Variation is skeleton jitter**: branch positions (rotation about their fork), lengths and droop
   by depth, optionally dropped branches, with leaf volumes moving with their nodes and `core`
   voxels rigid; plus the structure system's rotations, mirroring and weighted block choices. **No
   grafting** between models; **no space colonization** for now (a second engine could be added to
   the vocabulary later by ADR).
8. **The owner's persistence model.** Trees are **a layer composed at chunk load, not voxels in the
   saved chunk**:
   - **Spatial grid**: tree records are keyed by the chunk holding the root; a tree spanning chunks
     applies a stage only when all its chunks are loaded.
   - **Seed determinism**: procedural trees are placed by `hash(seed, chunk, index)`, so a tree's id is
     `(chunk, procedural index)` and an untouched one stores nothing.
   - **Tombstones**: a per-chunk sparse set of chopped procedural tree ids, and per-tree pruned node
     sets for partial cuts.
   - **Entity pool**: a **`growers`** table for player-planted trees (structure, variant seed, root,
     planted world time, pruned nodes).
   - **Analytical catch-up**: `stage(now − planted)` evaluated on load and on a slow schedule while
     loaded.
   A chunk is composed in order: generated terrain and its saved edits (as today); procedural trees
   not tombstoned, at `stage(now)`; planted trees from `growers`, at `stage(now)`; growth never
   overwrites anything but air or its own leaves. The client composes the same layers from the same
   records, which the server sends with the chunk, so tree voxels are not streamed. When a tree
   reaches a new stage while loaded, the server sends the change as a `VoxelModification` with a new
   **`Growth`** reason. Phase 20 sees the composed voxels.
9. **Both procedural and planted trees.** Planted trees come first (G3); procedural placement waits
   for ADR 0031's deferred worldgen placement decision.
10. **Compatibility.** The world-file tables (tombstones, `growers`), the protocol (tree records with
    chunks, the `Growth` reason) and procedural placement (it changes the terrain a seed generates)
    each need **a new compatibility line**; the owner decides the bump (`package.json`). The prototype
    page, the format's roles and `growth`, the generator and `stage_voxels` (G0–G2) change no world
    file, protocol or terrain.

## Consequences

- An untouched world stores nothing for its trees; a chopped procedural tree costs one id in a set; a
  planted tree one small record. Edits near a tree no longer freeze it.
- The world file gains its first non-voxel world state, and chunk loading gains a composition step on
  the server and the client; both must agree, so composition is one Rust function tested natively
  and under WASM.
- Inferred skeletons can be ambiguous on chunky hand-sculpted trunks; the `trunk` and `core` roles
  are the author's fix. Adaptive blocks (Phase 18) may reshape wood or leaves.
- A trunk step every 15 real hours means the sandbox needs a growth preview (G2).
- Depends on Phase 27 (structures), Phase 20 (felling) and the not-yet-designed day/night cycle (the
  day length); procedural trees also on ADR 0031's placement decision.
- Phase 30 of the implementation plan (G-ADR, G0–G4) builds it.
- Reversing it: trees are a separate layer and separate tables; dropping them leaves terrain and
  saved chunks untouched (on a new compatibility line, since the tables and protocol would go).
