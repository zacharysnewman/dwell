# Felling: pieces that lose their support fall, tip and settle

**Status: F1 built (0.36.0); F2 built (0.37.0); F3 built (0.37.1)** ([ADR 0051](./adr/0051-felling.md)). The first use of the
collapse pipeline planned in ARCHITECTURE.md §7.1–7.3 (Phases 20 and 22), for trees, on a general
rule. Trees as unique things that grow over time stay [`TREE_GROWTH.md`](./TREE_GROWTH.md)'s.

## 1. Brief and the owner's answers (2026-10-27)

> "I want trees to be unique things, eventually they'll grow over time, for now though I want to be
> able to 'cut' them down. Any piece should become a separate entity when disconnected from their
> root, fall, tipping over, and then settling back to a static block."

| Question | Answer |
|---|---|
| What can fall? | **Trees first, as a general rule in block data**: what each block counts as for support. Logs and leaves opt in now; terrain and player builds can opt in later by data alone. |
| What holds a tree up? | **Logs carry, leaves ride.** A tree stands while its logs connect down to the ground; leaves hold nothing up and fall with their logs. *Falling* leaves don't collide with terrain, and when a piece settles, a leaf that would land inside another block is destroyed. |
| Leaves and players | "Leaves should no longer be solid to a player with this update": players pass through leaves, standing or falling. |
| Multiplayer | **Server-run**, as ARCHITECTURE.md §7 plans: the server simulates each falling piece and replicates it; every client sees the same fall and the same blocks after. |
| Players and falling pieces | **All three:** a falling piece pushes players, can hurt them, and can be stood on and ridden. |

## 2. Support as block data

Each block has a `support` role (`shared/blocks/dwell.json`, generated like the rest of the block
data; a bad value fails generation):

| Role | Meaning | Blocks today |
|---|---|---|
| `ground` (default) | Anchors whatever touches it; never falls. | Every block not listed below: terrain, player builds. |
| `carrier` | Stands while a path of carriers leads to a `ground` block it touches; holds up the riders on it. | `dwell:log` |
| `rider` | Holds nothing up; stays while a carrier it belongs to stands. | `dwell:leaves`, `dwell:snowy_leaves` |
| `falls` | Rests on what is under it; with nothing solid there it falls, with what of its kind is stacked on it, and lands as itself. Anchors a carrier as `ground` does. (0.40.0, owner 2026-10-28: "make sand fall".) | `dwell:sand` |

And `passable: true` (leaves): players pass through it (and a block of it may go, or land, where a
player stands: `overlaps_player`); targeting still hits it,
so it can be broken and built on.

**The rule (integrity).** When a block is removed (or a piece settles), the server looks at the
carriers and riders around the change, budgeted per tick (`INTEGRITY_BUDGET_VOXELS`, §7.4):

1. The carriers connected to the change through carriers (face neighbours) are flood-filled into
   components; a component is **anchored** if any of its carriers touches a `ground` block.
2. Each rider nearby belongs to the carrier nearest it through riders (a multi-source search from the
   carriers, at most `riderReach` cells), so two trees' touching canopies split between their own
   trunks and leaves never hold up a neighbour's crown.
3. Each unanchored component, with its riders, becomes one **piece**: its cells leave the grid in the
   same reliable change (reason `Collapse`) and it becomes a body. Riders that belong to no carrier
   within reach fall alone (a leaves-only piece, §3) — unless they rest on something solid under
   them (0.40.0: a leaf placed on the ground stays; owner, 2026-10-28: "all leaves fall to the
   ground unless they're connected … to a tree trunk").
4. A `falls` block (sand) next to the change with nothing solid under it falls with the column of
   its kind stacked on it (a piece of all carriers). It collides with the terrain and other pieces
   but passes players (`Collide::World`): it may hurt one (`hits` is by the geometry, not contact)
   but never rests on or in one; sand landing where a player stands slides off into a free cell
   beside them (owner, 2026-10-28: "slide off for now").

## 3. The falling piece

- **Body:** one dynamic Rapier body per piece (`dwell-physics`), a compound of one box per carrier
  cell (1 × ½ × 1 m), its mass, centre of mass and inertia from every cell's `density` (riders add
  mass but no collider). Gravity is its face's (toward the midplane, through the flip band as
  bodies already do). A trunk cut low tips over because its centre of mass is over the cut.
- **What it hits:** terrain, other pieces, players (`Collide::All`: logs; sand passes players, §2). The leaves riding a
  trunk don't collide (`Collide::None`). A leaves-only piece (§2) collides with the terrain and
  other pieces but not players (`Collide::World`): it falls, lands and settles as leaves (0.40.0;
  owner, 2026-10-28: detached leaves "become solid to blocks when detached from the tree, falling
  down to the ground, settling", non-solid to players always). The terrain collision covers every
  piece's box, leaves-only ones too (red before: they fell through the world far from players).
- **Players:** pushed by it (as by any Tier 1 body, PLAYER_CONTROLLER.md §6.6); pushing it back
  only while it is lighter than `pushableMass` — a heavier piece is held from players' pushes
  (Rapier's dominance group) while it moves slower than `heavyHoldSpeed` (held while falling fast,
  it squeezed a player under it through the ground, `a_falling_trunk_hurts_…`); hurt by it: a
  carrying cell closing on a player faster than `hitSpeed` (its speed toward them, less theirs away;
  a player falling onto a still log takes fall damage instead) does `hitDamagePerSpeed` points per
  m/s over it, `Crush`, once per piece and player (`Felling::hits`, before the physics step); and
  able to stand on and ride it (`step_platform`, on the server; in the prediction its kinematic
  stand-in carries the player the same way). The wire's ground kind stays `None` for a body: the
  prediction finds the stand-in under the player by its own query each tick.
- **Network (new messages, a new protocol version):** reliable `EntitySpawn` (id, the piece's cells
  and states, its pose) and `EntityDespawn`; its pose and velocities in the snapshot datagrams at the
  snapshot rate; clients interpolate it (`INTERP_DELAY_MS`) and insert it as a kinematic body so
  their own player collides with and rides it. Drawn as a greedy mesh of its cells with the body's
  transform (ADR 0044).

## 4. Settling

When a piece has moved less than `SLEEP_LINEAR_THRESHOLD` / `SLEEP_ANGULAR_THRESHOLD` for
`SLEEP_SECONDS` (§7.3) it becomes blocks again:

1. Its rotation snaps to the nearest of the 24 axis-aligned orientations and its position to the
   grid; each cell maps to a world cell, its state turned with it (a log's `axis`: a felled trunk
   lies along x or z).
2. A rider whose cell is taken (by terrain, a player or another piece's cell) is destroyed (the
   owner). A carrier whose cell is taken tries one cell up, then sideways; else it is destroyed.
   Cells overlapping a player count as taken, so nobody is buried.
3. The cells are written in one reliable change (reason `Rebake`) with `EntityDespawn`; the placed
   carriers then pass through integrity again (a log left resting only on leaves falls again; a
   piece that settles three times without anchoring stays where it is).

### 4.1 The settle glide (clients; owner, 2026-10-29)

"No visual teleporting" between a piece's resting pose and its blocks. Owner's answers: **visual
only** (the server writes the blocks at once, so collision and targeting have them at once);
**settle with a small bounce**; the **snap to the grid and moved cells** glide (cells lost at
settling simply go); **one setting in `felling.json`** (`settleGlideSeconds`, `settleBounce`), the
same for every block.

- **Hidden while gliding:** a `Rebake` modification's cells are left out of the terrain meshes
  (`ChunkStreamer::hide_cells`: meshed as air, and a mesh job in flight for one is dropped). The
  world holds them. A cell no piece claims within 0.5 s is drawn again.
- **Where each cell went:** at the piece's `EntityDespawn` the client resamples its cells the
  server's way (`felling::resample` at `snap` of its latest pose). It takes the lift (0–2 rows)
  that most cells were written at. A falling-role cell written beside its spot (sand that slid off
  a player) is a part of its own. Nothing is added to the wire.
- **The glide:** each part's blocks, meshed as the terrain will mesh them (face B's mirrored), are
  drawn from where the piece was drawn (the rigid move from its settled pose to its drawn pose) to
  their cells over `settleGlideSeconds`. The easing is "back" ease-out, swinging `settleBounce` of
  the move past the end. Then the cells are drawn by the terrain again, and the glide's meshes go
  once the terrain has meshed them (at most 1 s later).
- **Tests:** `settle_glide.rs` (start pose on both faces, a lift, slid sand, the easing, the
  lifecycle) and `chunk_stream.rs` `hidden_cells_mesh_as_air_until_shown_…`.

## 5. Data and tunables

Block data: `support` (`ground` | `carrier` | `rider` | `falls`), `passable`, `density` (exists), log `axis`
(`y` | `x` | `z`, its texture turned by it). Felling tunables (`shared/physics/felling.json`,
generated into `crates/dwell-sim/src/felling.gen.rs` by `shared/physics/gen.mjs`, validated, CI
failing on stale output): `riderReach` (cells, 6), `maxSystemCells` (16384), `maxPieceCells`
(4096), `toppleSpin` (0.6 rad/s), `sleepLinear` / `sleepAngular` (0.05), `sleepSeconds` (2),
`maxSeconds` (30), `maxSettles` (3), `hitSpeed` (4 m/s),
`hitDamagePerSpeed` (6), `pushableMass` (400 kg), `heavyHoldSpeed` (1 m/s), `settleGlideSeconds`
(0.3 s) and `settleBounce` (0.12, §4.1). Sounds
(`shared/sounds/dwell.json`): `fell` (the crack, `fell_crack`, as a carrying piece spawns) and
`piece_land` (the block's `land` sound when a piece's velocity changes by over 2 m/s between
snapshots, at most every 250 ms).

## 6. Stages (each its own release)

- **F1 — Data and leaves** **[built, 0.36.0]:** `support`, `passable` and the log's `axis` as block
  data, generated, validated and synced; leaves passable to players (out of the terrain collider and
  the controller's probes; `a_wall_of_leaves_is_walked_through`, both faces, red on 0.35.1); logs drawn
  along their axis (end grain on the ends, the bark turned; mesher test red on 0.35.1); and, the
  owner asking "we'll likely need to do some kind of new rendering for when your camera is inside
  leaf blocks", leaves' faces dissolve within `nearFade` (1.2 m) of the eye, dithered, so a canopy
  the eye is in shows round it without faces against the lens (checked in Chrome). No falling yet.
- **F2 — The loop** **[built, 0.37.0]:** integrity on removal (`dwell_world::support`, tests on
  crafted trees: a cut trunk, touching canopies, branches, loose leaves, the bound); pieces as
  server bodies (`dwell-sim` `felling.rs`), tipped away from the nearest player (`topple_spin`
  0.6 rad/s: a trunk cut straight through otherwise balanced on its stump), with terrain collision
  loaded round them (a piece far from every player fell through the world: test red without it);
  the protocol messages (v20); client meshing in chunk-sized blocks, interpolation `INTERP_DELAY_MS`
  behind and kinematic stand-ins in the prediction (`dwell-client` `pieces.rs`; a quaternion per
  draw in the renderer); settling by resampling onto the grid (`an_upright_trunk_tipped_onto_its_side_lies_as_logs_along_x`).
  Server tests: a cut trunk lies on the ground as logs along x or z. Checked in Chrome on the flat
  world: a tree with a crown tips, falls, its leaves through the ground, and settles. (The
  tunables were Rust defaults in F2; data since F3.)
- **F3 — Players and polish** **[built, 0.37.1]:** the tunables as data (`shared/physics/felling.json`,
  generator tests); push limits (`dwell-physics` `tests/pieces.rs`: a light log pushed, a 1.5 t one
  not — red without the dominance, on both faces); hit damage (unit tests, and a server test: a
  trunk falling on a second player hurts them once, not the cutter, and leaves them alive on the
  floor); riding (a player on a sliding piece goes with it, both faces; the prediction rides a
  piece's stand-in); falling pieces cast shadows (their pose in their caster identity, so a kept
  cascade is drawn again as they move; unit test, checked in Chrome); sounds (`fell`, `piece_land`,
  unit test); the bench scenario `felling`. Deviation: no e2e spec — main's e2e has failed since
  before this work (PR #236), and the server tests cover the loop end to end.

## 7. Not in this design

Trees as records that grow (TREE_GROWTH.md §5); terrain and builds falling (opt in by data later,
with Phase 20's remaining exit criteria: large structures across ticks, crush); explosions and
Tier 2 debris (Phase 21).
