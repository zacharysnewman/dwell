# Cannons, cannonballs and steering ships

**Status:** C1–C3 built (2026-11-17: the land cannon block, its Use and reload; the ball in flight; its impacts; §5–§7); C4 riding along built (2026-11-18, §8; a cannon on a moving body and recoil to come); C5 to come. The goal (owner, 2026-11-17): "ship battles where cannons can blow off
pieces of ships." Sinking by holes below the waterline, and water filling a hull instead of standing in it, come after
this design (WORK_QUEUE; questions first).

## 1. The owner's answers (2026-11-17)

1. **Firing:** "using the same input as doors, you can fire the cannon, clicking on it. It faces the direction of
   placement (still correctly rotates with dynamic objects), no re-aiming for now. We can add aiming later."
2. **Balls:** "2 cannonball variants one explosive, one simple impact knocking loose cells it strikes (and maybe knocks
   loose in an area around where it collides). Should have high mass as it's a heavy steel ball."
3. **Settling:** anything that comes to rest on a moving ship (a ball, a piece knocked loose) rides along with it, not
   falling through or sliding off: it joins that body rather than settling into the world's grid.
4. **Data:** cannon and ball types are data, so more are added without code.
5. **Ships:** "ship should be moveable with player controls, force should be exerted like a sail would, from the
   center of mass I think. Turning like you have a rudder." The wheel steers; no test push.

The first cannon is a land cannon: "cannon takes up 4 blocks 2m long by 1m tall, shoots physics cannon ball in aimed
direction." No ammunition or reloading for now.

## 2. Data (`shared/cannons/cannons.json`, generator `shared/cannons/gen.mjs`)

- **cannons**: each a block (by name) with its barrel along the block's facing: the muzzle's offset from the block's
  anchor cell (face-local, turned with the facing), the ball it fires, the muzzle speed (m/s), the time between shots
  (s), the recoil impulse on a body it stands on, the firing sound and smoke.
- **balls**: each a name, radius (m), density (kg/m³; steel 7,850: a 0.12 m ball weighs ~57 kg), its look, and its
  impact: `impact` (`knockRadiusM`: cells within it of the contact become loose pieces, `maxCells`, `minSpeedMs` below
  which it knocks nothing, the speed it keeps per cell knocked) or `explosive` (an entry of
  `shared/explosives/explosives.json`, blown at the contact, as dynamite's blast).
- Validation by name; a fixture cannon and ball added by data alone in the generator's tests. The engine's vocabulary
  is fixed: the two impact kinds, the muzzle and the facing.

## 3. Phases (each its own release)

- **C1 — the land cannon block.** A four-cell block (2 m long × 1 m tall, 1 m wide; `part` as the door's), placed
  facing away from the player, a prototype look (textured cells). Using it (the door's input) asks the server to fire;
  the server checks the time between shots and answers with a shot. Tests: placing makes its four cells, using it
  fires once per interval, on both faces.
- **C2 — the cannonball.** A server physics body: a sphere collider of the ball's radius and mass, spawned at the
  muzzle with the muzzle speed along the barrel (the cannon's facing turned by the body it stands on, if any), its
  recoil pushing that body; sent to clients as an entity and drawn as its look. Gravity and water drag as other
  bodies. At rest it settles as a loose item (a ball to pick up later) or, on a body, joins it (C4).
- **C3 — impacts.** On the first contact above `minSpeedMs`: the impact ball turns the cells within `knockRadiusM` of
  the contact (terrain or a body's) into loose pieces (felling's one-cell bodies, flung along the ball's path), up to
  `maxCells`, and keeps going slowed; the explosive ball blows its explosive's blast there and is gone. Both faces.
- **C4 — riding along.** A body or piece that comes to rest on a built body (a ship) joins it instead of settling into
  the grid; a ball resting on a deck rides with it.
- **C5 — steering a ship.** A wheel block: using it puts the player at the helm (their movement keys steer). Forward
  is a push at the body's centre of mass along its heading, as a sail would give (strength by data); turning is a
  rudder's: a turning force that grows with the ship's speed through the water (none when still). A rudder block may
  be required for turning (owner: "while it'll be required, it'll be mostly for show"); sails later.

## 4. Open (later)

- Aiming (turning and raising the barrel), ammunition and reloading by hand.
- Sinking by holes below the waterline; water filling a hull rather than standing inside it (owner: "We'll clarify
  some questions first").
- Damage to players from a ball's hit.

## 5. As built: C1 (2026-11-17)

- **The block** `dwell:cannon` (shared/blocks/dwell.json): four cells, `part` rear_bottom, rear_top, front_bottom,
  front_top; `column: "part"` with **`columnAlong: 2`**, a new rule of the block vocabulary (shared/blocks/gen.mjs,
  `Column::along`, synced as registry tag 17 `COLUMN_ALONG`; an older build reads the column straight up, the server's
  writes still whole): a column's values fill each upright row of `count / along` cells bottom to top, row after row
  forward along the block's facing (`dwell_world::parts::forward`: north −z). Placed whole by its rear bottom cell, the
  barrel away from the player (items.json `facing: away`), cleared whole; `needsSupport: below`; 12 base hits
  (pickaxe proficient). Prototype look: basalt-textured cells.
- **Use:** the block rule **`use: "fire"`** (registry tag 18 `USE_ACTION`): a Place aimed at it is a Use
  (`EditOutcome::used`), nothing written or paid, whatever is held; the server's `use_block` fires the cannon at its
  rear bottom cell (`cannons::anchor_of`) once its `reloadS` has passed (`shots_fired`; C2 launches the ball).
- **Data:** shared/cannons/cannons.json (generator shared/cannons/gen.mjs → crates/dwell-world/src/cannons.gen.rs):
  the cannon (its ball, muzzle speed 45 m/s, muzzle 0.6 m up and 2.1 m forward, reload 2 s) and two balls, `iron_ball`
  (r 0.12 m, steel 7,850 kg/m³: ~57 kg; impact `knock`, 0.8 m, up to 6 cells, from 8 m/s) and `explosive_ball`
  (impact `explode`: dynamite's blast).
- **Tests:** `dwell-world` `tests/cannons.rs` (both faces: four cells two along and two up, any cell's break clears
  all, every cell's anchor, a Use writes nothing and reports it); `dwell-sim` `tests/cannons.rs` (both faces: a Use
  fires once, not again while reloading, again after); `cannons::tests`; shared/cannons/gen.test.mjs (a fixture cannon
  and ball by data alone, bad entries named).

## 6. As built: C2 (2026-11-17)

- **The balls are blocks** `dwell:iron_ball` and `dwell:explosive_ball` (shared/blocks/dwell.json; their names are the
  cannons data's balls): shape empty, the new look **`ball`** (shared/blocks/gen.mjs: needs shape empty), a sphere of the
  ball's radius on the cell's floor face-local (`dwell-mesh` `ball_voxel`, a 10 × 6 UV sphere in the block's colour),
  `needsSupport: below`, 1 hit. Not placeable or in the hotbar.
- **The shot:** the server's `launch_ball` spawns a one-cell natural body of the ball's block at the muzzle
  (`muzzleForwardM` from the rear bottom cell's centre along the facing, `muzzleUpM` above its floor) moving at
  `muzzleSpeedMs` along the barrel, and broadcasts its `EntitySpawn` (no protocol change: clients draw it as any body's
  cells). Its collider is a sphere of the radius and the ball's mass (`dwell_physics::ball_of`, `piece_hull` gives a
  ball cell its own collider; on face B it sits toward +y), CCD on; gravity, water and settling as any natural body: at
  rest it settles as its block. A piece's riders-only groups: it hits the world and bodies, not players (yet).
- **Rolling resistance** (the browser, 2026-11-17: a fired ball rolled past 200 m on the flat; rapier has no rolling
  friction): each ball's data `rollDamping` (1/s, the iron ball 1.5) damps its speed while a solid cell lies just under
  it (the server's `roll_balls`, every tick before the physics step), the air's usual damping otherwise. The iron ball
  fired over flat ground rests ~39 m out (`a_ball_rolls_to_rest_not_on_for_hundreds_of_metres`, both faces).
- **Not yet:** the cannon on a moving body (C4 turns the facing with it), recoil, the firing sound and smoke, impacts (C3).
- **Tests:** `dwell-sim` `tests/cannons.rs` `a_shot_launches_a_heavy_ball_along_the_barrel` (both faces: at the muzzle,
  the ball's mass, flying along the barrel, landing on the floor beyond); `dwell-mesh`
  `a_cannonball_is_a_sphere_of_its_radius_on_its_cells_floor` (both faces).

## 7. As built: C3 (2026-11-17)

- **A strike** is a change in a ball's velocity of its impact's `minSpeedMs` or more in one tick (the server's
  `roll_balls`, each tick before the physics step, against the velocity it had the tick before): a grazing landing
  changes it little and strikes nothing; a wall hit head on changes it by nearly its speed. Away from what it hit:
  the contact is the sphere's surface against that change.
- **Knock** (the iron ball: 0.8 m, up to 6 cells, from 8 m/s): the breakable cells (solid, not liquid, not
  indestructible, not an explosive) whose centres lie within `knockRadiusM` of the contact, nearest first, at most
  `maxCells`, are cleared (`Collapse`, no flash) and queued as natural pieces with the blast's pieces
  (`blasts::Queued`, spawned a few a tick) flung along the ball's path at half its speed; what that leaves without
  support falls. Having knocked any, the ball flies on at half its speed (it can strike again).
- **Explode** (the explosive ball: dynamite, from 6 m/s): the ball's body is destroyed and its explosive's blast set off
  at its cell (`blast_at`, the dynamite blast's body split from its fuse check: the same destroy, fling, water,
  knockback and damage, `Explosion` so clients flash and boom).
- **Not yet:** a ball's hit on a player or a moving body's cells (C4), the impact's own sound and dust.
- **Tests** (`dwell-sim` `tests/cannons.rs`, both faces): `an_iron_ball_knocks_loose_the_cells_round_where_it_strikes_a_wall`
  (the hit cell among those gone, no more than `maxCells` a strike), `an_explosive_ball_blows_up_where_it_strikes`
  (the ball gone, more of the wall than a knock, not all of it), and the flat roll knocks nothing. `set_cannon_ball`
  makes the cannons fire another ball (tests, the dev tools).

## 8. As built: C4 riding along (2026-11-18)

- A settling piece (a natural one: knocked-loose cells, a felled trunk, a ball) resting on a body that never settles (a
  built one, a ship) — one of that body's cells just under one of its own, face-local, in the body's grid at its pose
  (`Felling::support_of`) — never settles into the world's grid there; once still **relative to that body** (its
  velocity less the body's at its place, its spin less the body's, under the settling thresholds) for `sleep_ticks`, it
  **joins** the body (`Felling::merge`): each cell to the cell of the body's grid its centre is in (a falling block as it
  settles), a filled one skipped; clients hear `EntityDespawn` for the piece and a `BodyEdit` adding the cells. No
  search at all while no body that never settles exists (a blast's pieces cost nothing).
- **Not yet:** a cannon standing on a moving body firing along the body's turned facing, and recoil.
- **Test:** `dwell-sim` `tests/cannons.rs` `what_comes_to_rest_on_a_built_body_joins_it_not_the_grid` (both faces: a
  dirt clump and an iron ball dropped on a plank deck join it, nothing of them in the grid; red with the joins off).

