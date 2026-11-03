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

**Corrected 2026-11-02 (0.41.3).** The owner: *"The platform wasn't supposed to 'make up' the
structure. It was supposed to be the platform on which the structure is built."* (0.41.1–0.41.2 made
the platform block itself the body's first cell.) Owner's answers: the platform is a static block of
the world; what is placed on it becomes the moving object; the object is **held still until
released**; blocks touching it join it (**one object if they touch**).

- `dwell:build_platform` (prototype content: planks) is an ordinary block of the world's grid, placed
  and dragged like any block. `shared/physics/bodies.json`'s `built` kind lists it in `builtOn`.
- **A block placed on a platform's top** (the cell face-locally over a `builtOn` block) doesn't go in
  the grid: it starts a body of that kind there, unturned on the grid, **held** (Rapier fixed) while
  any of its cells rests on a `builtOn` block (`Felling::divert`, `place_diverted`).
- **A block placed touching a held body** (any of its six neighbours) joins it, whether by a click on
  the body (`BodyEditRequest`), a click on the ground or platform beside it, or a drag's fill
  (`RegionEdit`): the server diverts every placed cell by the same rule, so a drag that starts on the
  body builds the object on (the client starts the stroke in the world cells next to it).
- **Released**: once no cell rests on a platform (the platform broken from under it), the body moves
  (`Felling::release_unheld`, each tick): it falls, tips, floats, is pushed; it never settles back. A
  released body isn't held again; one loaded with the world unturned on its platform is.
- Tests: `dwell-sim` `tests/built_bodies.rs`: a platform is a grid block and starts no body; a log on
  it is a held body; a log on that and a drag's fill beside it (over open ground) join it; held, the
  overhang doesn't tip it; a joiner sees it; the platform broken, it moves and doesn't settle; saved,
  it comes back held.

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
- **Riding and pushing:** as pieces already do (players stand on and ride them, push them by the
  controller's push, §6).

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
- `water` (as built): `dragCoefficient` (the speed-squared drag), `drag` (/s, sideways), `heaveDrag`
  (/s, the bob) and `angularDrag` (/s, the spin), each by the share under (§6, 0.41.6).
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
- **B2 — The build platform and built bodies** **[built, 0.41.1]**. The platform block and support role; built bodies
  (place onto them, never settle); saving bodies with the world (the save format change, same line).
- **B3 — Floating** **[built, 0.41.2]**. Buoyancy and water drag per cell; natural and built bodies.
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

**B2 as built** (its platform rule corrected in 0.41.3, §3). `dwell:build_platform` (planks, wood:
600 kg/m³) is in the hotbar. Aiming at a built body, Place sends
`BodyEditRequest` with the selected block (`edit_body`); the server's `Felling::place_cell` adds the
cell against the face aimed at (its kind's `place`; refused where a player's capsule is within
0.45 m of the cell's centre beyond its radius) and every client hears `BodyEdit`. Built bodies never
settle (`Felling::step` skips kinds that don't). Every body is sent to a player joining (after
`Welcome`). Bodies are saved with the world on every save (`SaveBatch::bodies`, the `bodies` table:
kind, settles, pose, velocities, cells by state name; `dwell-storage` `encode_body`) and brought back
on load (`Felling::restore`). Tests: `dwell-sim` `tests/built_bodies.rs` (a platform starts a body
that takes a log on its top, never settles, is sent to a joiner, and comes back after a save over the
same world), `dwell-storage` the bodies' round trip; the page's debug state reports `bodyTarget`.
Checked on the GPU: a platform placed, two logs built onto it, the page reloaded, the raft where it
was. **Deviation (§5):** bodies far from every player aren't frozen: the terrain collision loads
round every moving body anyway (`Felling::terrain_boxes`, budgeted per tick since 0.40.5), so none
falls through unloaded ground; idle built bodies keep simulating (Rapier sleep for them is later).
A world opened by 0.41.0 (before B2) ignores the bodies table: its bodies are dropped if it saves.

**B3 as built.** Each tick before the physics step, `Felling::float` pushes every cell of every body
by the liquid it displaces: `cell_in_liquid` reads its centre and corners (none or all), and a cell at
the surface its five columns (centre and corners) measured exactly, row by row (0.41.6; before,
40 points at eight heights, which stepped the buoyancy 6 cm at a time); the push is the liquid's own density (its block's `density`: water 1000 kg/m³) × gravity ×
the cell's ½ m³ × that share, face-locally up, at the cell's centre; a body in water slows by
`shared/physics/bodies.json`'s `water` rules (0.41.6, below; `drag` 1.2 /s on its velocity before),
`angularDrag` 1.5 /s on its spin, times its share under. Tests: `a_cells_share_under_water_is_read_on_either_face` (both faces), `tests/built_bodies.rs`
`a_wooden_raft_floats_and_a_stone_one_sinks` (a 3 × 3 log raft floats 0.1 m under, level; a stone
one rests on the pool's floor), the water rules' data tests. Cost: nine voxel reads a cell a tick
(37k for a body at the 4,096-cell cap). Natural pieces float too: a felled tree in a lake bobs until
`maxSeconds` (30 s) and settles where it is.

**Playtest fixes (0.41.4; owner, 2026-11-02).**
- *"I couldn't push it"* (a 2 × 2 log raft afloat): heavy pieces are held from players' pushes while
  slow (`pushableMass`, a felled trunk's rule); not while any of the piece is in a liquid
  (`Falling::floating`, set by `float`), the water carrying its weight
  (`a_heavy_raft_afloat_is_not_held_from_players_pushes`).
- *"It froze until I broke a block"*: a piece slower than 0.05 m/s counted as at rest and was not sent
  again (0.40.13's ledger), so a drifting raft froze on screen. At rest now means within 1 cm and
  0.002 (quaternion) of the pose last sent to that player (`a_piece_drifting_slowly_is_sent_as_it_moves`).
- *"My character freaks out when standing on a floating object"*: on a dynamic body the contact
  hands the player ~0.12 m/s upward each tick, which the controller read as a launch (over 0.1 m/s), so
  the player never stood (Falling, every tick). On a dynamic body underfoot the launch threshold is
  0.5 m/s (`DYNAMIC_GROUND_LAUNCH_THRESHOLD`; PLAYER_CONTROLLER.md)
  (`a_player_stands_steadily_on_a_floating_raft`: 0 → 180 of 180 ticks grounded).
- *The hover outline tilted unevenly*: the line shader left out a draw's rotation; it turns lines as
  meshes (`the_block_outline_turns_with_its_draw_as_meshes_do`; checked on the GPU on a tipping trunk).

**Pushing, predicted (0.41.5; owner, 2026-11-02: "Movement gets really jittery when pushing a
floating log raft, you can kind of push into it, feels soft"; the owner chose fixes 1 and 2).**
- *The push* (`dwell-physics` `Players::step_push`, after the aggregate pass): a Tier 1 body players
  don't move by contact (`Physics::pushed_by_players`: the server holds a piece slower than
  felling.json's `holdSpeed`, or pushed in the last `pushHoldSeconds`, by a dominance group; a
  client's stand-in is kinematic) that the player walks, swims, flies or slides into from the side is
  given the player's speed into it (the larger of the body's velocity into it and the speed the
  input asks for: against the body the layers' velocity is cut), at most `maxPushForce` (800 N) over
  its mass a tick, at its centre of mass. The server applies the impulse; the prediction moves its
  stand-in by it (`Players::pushes`). A heavy log on the ground doesn't move (friction is more than
  the push); a 1.4 t raft afloat goes ~0.2 m/s. This replaced `pushableMass`: weight now resists by
  friction and water drag, not a cut-off.
- *The stand-in* (`Predictor::set_piece`): at the server's pose as of the input seq its snapshot
  acknowledged, carried on at its velocity to each tick predicted (replays too), plus the local
  player's own pushes since: where the server will have it, not where it was a round trip ago. The
  client sets the pieces before reconciling (`game.rs`).
- *Drawn at the present near the player* (`dwell-client` `pieces.rs` `set_present`): a body within
  2.5 m of the player past its reach is drawn at its stand-in's pose, blended over 0.25 s; others as
  before, 100 ms in the past (so a raft being pushed no longer showed the player inside it).
- Tests: `netcode.rs` `pushing_a_floating_raft_is_predicted_without_snaps_at_150_ms_rtt` (a player
  swims into a 2 × 2 log raft at 150 ms RTT, 20 ms jitter, 5 % loss: it moves; the stand-in within
  3 cm of the server's raft at each input; corrections at most 1.8 cm, 4.9 cm before);
  `pieces.rs` (physics) `a_player_pushes_a_held_light_piece_by_its_push` (red before: 0 m);
  `pieces.rs` (client) `a_body_near_the_player_is_drawn_at_its_present_…` (red before).
- Not predicted then: water drag and buoyancy on the stand-in, and the body's spin (0.41.6 predicts
  the drag and the ground's friction, below).

**Weight, friction and water as in life (0.41.6; owner, 2026-11-03: "It's too heavy", "a single log
should slide on land, at about 0.5m/s", "jumping on the log should push it down, with relevant
force").** Masses stay real (density × ½ m³ a cell: a 1 m log 700 kg, a 2 × 2 log raft 1,400 kg).
- *The push is a person's* (PLAYER_CONTROLLER.md "Push strength"): `max_push_force` 3,050 N at a
  standstill, falling to nothing at the player's speed, less the body's friction on the ground
  (`ground_friction`). A 700 kg log slides at ~0.5 m/s walking; afloat a raft gets ~2 m/s² and nears
  ~1.8 m/s swimming. Contacts with held bodies count within 0.1 m, a lost one keeps the push 4 ticks,
  and the pusher keeps up with the body it pushes. Only pieces are pushed (`Physics::pieces`), not
  moving platforms.
- *Water resists as water does* (`felling::water_drag`): sideways ½ · the liquid's density ·
  `dragCoefficient` (1) · the area of its sides under water facing the motion (its bounds, as deep as
  its share under) · speed², plus `drag` (0.15 /s) to still a slow drift; the bob by `heaveDrag`
  (2 /s), the waves a bob makes. A raft let go at 1 m/s still goes ~0.5 m/s two seconds on (0.19
  before) and drifts metres.
- *Standing on a held body presses it down* (`Players::press`, server only: a stand-in is the
  server's pose): the player's weight at the bodies' gravity (80 kg × 9.81) each tick and on landing
  the momentum it took, at the feet. An 80 kg player sinks a 2 × 2 raft ~2 cm (with the exact
  buoyancy above; the 8-level sampling sank it a 6 cm step), a landing from 2 m dips it further.
- *The stand-in is slowed as the server's body* (`PieceProxy::at`, tick by tick): the push less
  friction, the ground's friction on ticks it isn't pushed, and the water's drag through the sim's
  `water_drag` (`Predictor::set_water_drag`, set by `game.rs`; `dwell-physics` can't see the sim's
  data).
- Tests: physics `pieces.rs` `a_player_slides_a_log_on_the_ground_at_a_walk` (red before: 0 m/s);
  `built_bodies.rs` `a_raft_let_go_in_water_glides_on` (red: 0.16 m/s), `a_player_on_a_floating_raft_
  presses_it_down` (red: 0 cm); `netcode.rs` `pushing_a_log_on_land_is_predicted_without_snaps_at_
  150_ms_rtt` (red: the log went 0 m; now ~1.9 m, corrections at most 1.6 cm) and the raft's (now
  3.95 m in 6 s, corrections at most 2.2 cm, the stand-in within 1.9 cm p95).

## 12. Not in this design

Driving or powering bodies; joints between bodies (hinges, wheels on axles); bodies merging when
they touch; building a body onto the world grid (a body never becomes terrain, a natural one only
by settling).
