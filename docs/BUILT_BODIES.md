# Built bodies: editable moving pieces and the build platform

Design for Phase 51 (ADR 0052). Builds on felling ([`FELLING.md`](./FELLING.md), ADR 0051): pieces are
already server-run Rapier bodies of voxel cells, replicated to clients, ridden and pushed by
players. This design makes those bodies **editable**, and adds **built bodies** that never settle,
are saved with the world and float.

## 1. Brief and the owner's answers (2026-11-02)

> "There's 2 features I want to support for dynamic voxel structures (that are disconnected from the
> normal voxel grid). I want to be able to edit them, like trees, adding or removing (from) them
> while they're still physics affected. And second, I want to have a 'build platform' kind of block
> where anything placed on top of it, is not connected to the normal voxel grid, letting you build
> dynamic voxel objects (these should not settle back into the environment), could be something
> like a boat, or a cart, etc."

| Question | Owner's answer |
|---|---|
| Which edits on a moving piece, and a break that cuts it in two? | "Natural objects like trees can only be broken not built on, while custom structures can have both break and place, and splits for both natural and unnatural." |
| When does what's built on the platform become a moving object? | **A body from the start**: the platform is a body from the moment it's placed; each block placed on it joins that body at once. |
| Saved with the world? | **Yes** (a new compatibility line). |
| What do built objects do in the first version? | **Float in water**; **ride and push**. Driving "by some means later, not for now". |

## 2. Two kinds of body

Every moving piece has a **kind**, as data (`shared/physics/bodies.json`, §8):

| | `natural` (today's pieces: felled trees, falling sand, loose leaves) | `built` (made from a build platform) |
|---|---|---|
| Break a cell of it | yes | yes |
| Place a block against it (it joins) | **no** (owner) | yes |
| A break that cuts it in two | splits into two bodies of the same kind | the same |
| Settles back into blocks at rest | yes (FELLING.md §4) | **never** (owner) |
| Saved with the world | yes, mid-fall (a world saved during a fall comes back falling) | yes |
| Floats | by its cells' density (§6) | the same |

The kind's rules are data, so a later kind (a minecart that only takes rails, a ship that never
splits) is a new entry, not a branch in code. The engine's vocabulary: `place`, `break`, `split`,
`settles`, `saved`.

## 3. The build platform

A block, `dwell:build_platform` (prototype content: one block, a plank-like look), with block data
`"support": "platform"`, a new support role (`dwell_world::support`):

- **Placing it starts a built body** of that one cell, a body from the start (owner): the server
  never writes it into the world grid. It falls, lands and rests like any piece.
- **Blocks placed against a built body join it** (§4): a deck, a hull, a cart's box.
- A body keeps being `built` whether or not it still holds a platform cell: breaking the platform
  out of a finished boat doesn't make the boat settle into the ground.
- The platform is placed from the hotbar like any block (`shared/items/items.json`, `drag: none`:
  each one is its own body; dragging builds onto a body, not a row of new bodies).

## 4. Editing a body

**Targeting.** The crosshair's ray meets the nearest of: the world's voxels (`raycast_block`, as
today) and the bodies near the eye, each tested in its own frame (the ray turned into body space,
the same cell walk over its cells: an exact hit on a cell and a face, at its drawn pose). The
target gains a body id: `Target::Voxel { cell, face }` or `Target::Body { body, cell, face }`, its
`cell` in the body's own grid (offsets from its reference cell, as `EntitySpawn` sends them).

**The request.** A new reliable message, `BodyEditRequest { body, cell: [i16; 3], face, action,
material }`. The server checks it like a block edit (rate, reach to the body's cell at its pose now,
the kind's rule: no place on a natural body, the cell free in the body's grid and not overlapping a
player at its world pose), then:

- **Place:** the cell joins the body: a collider for it (as `Physics::add_piece` makes them), the
  body's mass, centre of mass and inertia recomputed (Rapier does it from the colliders).
- **Break:** the cell's collider goes; if the body's cells are no longer one 6-connected group, it
  **splits**: each group but the largest becomes a new body of the same kind, at the cells' world
  pose, with the velocity of the old body at its centre (`point_velocity`) and the same spin. A
  body of no cells is removed.

**The broadcast.** A new reliable message, `BodyEdit { body, cells: [(cell, state)] }` (state 0:
removed), sent to every client that has the body; a split also sends `EntitySpawn` for each new
body. Clients re-mesh the body (`pieces.rs` `piece_meshes`, a body of up to `maxPieceCells`) and
rebuild its stand-in's colliders in the prediction.

**Prediction.** None in the first version: an edit on a body shows when the server answers (one
round trip), as edits did before Phase 34's prediction. A moving target makes a predicted change
easy to get wrong; added later if it feels slow.

## 5. Physics

- The same Tier 1 body (`Physics::add_piece`): a box collider per cell, carriers colliding with
  everything, riders (leaves) with nothing; built bodies' cells all collide (a deck of leaves is
  still a deck).
- **Built bodies never settle** (`Felling::step` skips them); they **may sleep** (Rapier's sleep:
  still bodies cost almost nothing) and wake when touched, edited or the ground under them changes.
- **Terrain collision** round awake bodies only (`Felling::terrain_boxes`, budgeted per tick,
  0.40.5); a sleeping body needs none until it wakes.
- **Far from every player** a body is **frozen**: kinematic and still while the terrain round it
  isn't loaded (no player near), so it never falls through unloaded ground; it thaws when the
  terrain loads again.
- **Riding and pushing:** as pieces already do (players stand on and ride them, push those lighter
  than `pushableMass`).

## 6. Floating

**Buoyancy per cell** (a new rule in `dwell-physics`): each tick, for each cell of a body, the
fraction of its box under water (the water cells it overlaps, sampled at the box's corners and
centre) pushes up `water density × gravity × the cell's volume × that fraction`, applied at the
cell's centre, so a raft rights itself and a heavy end sinks first. Water also drags: linear and
angular damping by the submerged fraction (data, §8).

- A cell floats if its block is lighter than water (`density` in the block data: logs 700, leaves
  200, ladder 600 float; stone 2400, sand 1600, dirt 1500 sink). A body floats if its average is.
- **Enclosed air is not counted** in the first version: a boat floats by its material, not by a
  hollow hull (a stone hull sinks even if hollow). Counting a hull's enclosed volume is an open
  question (§10).
- Natural pieces float too: a felled tree that falls into a lake floats (and settles where it comes
  to rest on the bottom or a shore, as today).

## 7. Saving

A world's bodies are saved with it (owner): `SaveBatch` gains `bodies`, each `{ id, kind, cells:
[(offset, state)], position, rotation, velocity, angular, settles }` (`dwell-storage`, both the
native file and the browser store; the store's records append-only, RELEASES.md). On load each body
comes back where it was, frozen until the terrain round it loads (§5). Block states are saved by
name, as chunks are (string palettes), so a registry change keeps them.

This is a **change to the saved world format** and the **network protocol** (§4's messages, the
body kind in `EntitySpawn`): a **new compatibility line** (0.41.0), which the owner raises
(CLAUDE.md, versions and releases).

## 8. Data

`shared/physics/bodies.json` (generated into `dwell-sim` like `felling.json`, validated, CI on stale
output):

- `kinds`: `natural` and `built`, each `{ place, break, split, settles, saved }` (§2).
- `buoyancy`: `waterDrag` (/s), `waterAngularDrag` (/s), `samples` (points per cell).
- Body edits reach as far as block edits (`REACH_DISTANCE`).

Block data: `"support": "platform"` (§3). `maxPieceCells` (4096, `felling.json`) bounds a body's
cells, built ones too.

## 9. Both faces

Bodies have no up of their own: their cells are offsets in the body's frame, gravity is the face's
(toward the midplane, as pieces fall today), and buoyancy pushes along the face's up (away from the
midplane). A platform placed on face B starts a body there. Tests: the targeting, the edits, the
splits and the floating on both faces (`dwell_world::both_faces!`).

## 10. The owner's further answers (2026-11-02)

- **Hollow hulls:** not for now: a boat floats by its material (§6).
- **How many built bodies:** no limit for now (`maxBodies` left out).
- **Interest:** as-is for now: every body is sent to every player.
- **The compatibility line:** B1 opens 0.41.0 (the owner raises `package.json`).
- **Body edit prediction:** none in the first version (§4); later if the round trip feels slow.

## 11. Stages (each its own release; B1–B2 on the new compatibility line)

- **B1 — Editing pieces** **[built, 0.41.0]**. Targeting bodies on the client; `BodyEditRequest` / `BodyEdit`; break a
  cell of any piece, splits, the body kinds as data (`natural` only so far). Protocol change: the
  compatibility line opens here (0.41.0, the owner raises `package.json`).
- **B2 — The build platform and built bodies.** The platform block and support role; built bodies
  (place onto them, never settle); saving bodies with the world (the save format change, same line).
- **B3 — Floating.** Buoyancy and water drag per cell; natural and built bodies.
- **Later:** driving (seats, rudders, wheels), hollow hulls, interest management, edit prediction.

**B1 as built.** The client casts the crosshair's ray into every body's frame at its drawn pose
(`pieces.rs` `raycast`, `ray_cells`: an exact cell and face); a body's cell nearer than the block
aimed at is the target, outlined turned with the body (`Scene::set_block_outline_turned`). Break
sends `BodyEditRequest` (the block's break sound and the swing at once; the change when the server
answers); Place on a body is refused (natural bodies, the deny sound). The server queues it per
player (`MAX_QUEUED_EDITS`), pays it from the edit rate, checks the cell is within reach of the eye
at the body's pose, and `Felling::break_cell` takes it: the body's colliders made again
(`Physics::set_piece_cells`), and a body left in more than one 6-connected group split, each group
but the largest a body of the same kind at its cells' world pose, moving as that part did. Clients
hear `BodyEdit` (re-mesh; the prediction's stand-in made again from the next snapshot),
`EntitySpawn` for each part split off, `EntityDespawn` for a body left empty. Tests: the protocol
vectors; `felling` `a_cell_broken_off_a_falling_trunk_goes_and_a_break_through_it_splits_it` (13 → 12
logs settled, the split's 2 cells their own piece, a place refused); `pieces.rs` the ray into a
body's frame and an edit's re-mesh; the body kinds' data tests. Checked on the GPU: a falling log
column's cell broken mid-fall, 8 of 10 logs settled (10 less the cut and the broken one). Not yet:
a player joining mid-fall isn't sent the pieces already falling (needed for saved bodies, B2).

## 12. Not in this design

Driving or powering bodies; joints between bodies (hinges, wheels on axles); bodies merging when
they touch; building a body onto the world grid (a body never becomes terrain, a natural one only
by settling).
