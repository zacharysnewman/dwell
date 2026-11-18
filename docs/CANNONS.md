# Cannons, cannonballs and steering ships

**Status:** C1–C3 built (2026-11-17: the land cannon block, its Use and reload; the ball in flight; its impacts; §5–§7); C4 riding along built (2026-11-18, §8; a cannon on a moving body and recoil to come); the owner's new ball (2026-11-18, §9) replaces C3's two balls; C5 steering and C4's cannons on bodies built (0.75.0, §10). The goal (owner, 2026-11-17): "ship battles where cannons can blow off
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

Owner, 2026-11-18: rudder and sail blocks on hold; no aiming the barrel; a ball doesn't hurt players; sinking and water
filling the hull are next (with a hull's enclosed air as buoyancy).


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

## 9. The new ball (owner, 2026-11-18; replaces §7's knock and explosive balls)

The owner: "I want to double the size of the cannonball." "I also want the cannonball to have a drag force with it
that also detaches neighboring blocks from their structures. 1m radius. Every time the cannonball hits a block at
greater than a certain velocity, it should destroy it, if it drops below the velocity it should activate physics on the
block like it does now. The area drag force should also be equivalent to its speed." "This cannonball design replaces
the 2 types I specced out before." "Also the cannon seems to have a capped fire rate, set that to off or none." Answers:
"8× mass, real steel"; destroy speed 20 m/s; each block slows it "by the block's weight"; the drag "shouldn't be a
velocity setter, it should exert continuous force as it flies around it, in its velocity direction".

- **One ball**, `dwell:iron_ball`: r 0.24 m (0.48 m across), steel 7,850 kg/m³: ~455 kg. Its `impact` (shared/cannons,
  one fixed vocabulary, `cannons::Impact`): `destroySpeedMs` 20, `energyPerKgJ` 40, `dragRadiusM` 1, `dragForcePerSpeedN`
  2,000, `maxLooseCells` 24, `minSpeedMs` 4. `dwell:explosive_ball` is in no ball's data now; the block stays in the
  registry only until the next compatibility line (removing a block breaks the line).
- **No reload:** `reloadS` 0 (the data allows 0): every Use fires.
- **Smash** (`smash`, each tick before the physics step while the ball moves at `minSpeedMs` or more): the cells its
  sphere (drawn in 5 cm, so resting on the ground touches none) overlaps along this tick's path, in the order it meets
  them; each breakable one (solid, not a liquid, indestructible, an explosive or a ball) is **destroyed** while the ball
  moves at `destroySpeedMs` or more, the ball's energy less `energyPerKgJ` × the block's mass (its density × the ½ m³
  cell); the first met slower is **knocked loose** as a natural piece (queued with the blasts' pieces) and the contact
  is the physics'. The breakable cells within `dragRadiusM` of where it met each come loose too (nearest first, at most
  `maxLooseCells`). One `Collapse` modification; what that leaves unsupported falls.
- **Drag** (`drag`, the same ticks): every loose natural piece within `dragRadiusM` of its centre (not a ball), and every
  piece queued to spawn there, is pushed along its velocity by `dragForcePerSpeedN` × its speed for the tick, never
  past the ball's speed that way. No reaction on the ball.
- **Tests** (`dwell-sim` `tests/cannons.rs`, both faces): `a_cannon_fires_on_every_use`,
  `a_fast_ball_smashes_through_and_loosens_the_blocks_round_its_hole` (a one-brick wall: the cell in its path gone, the
  ends standing), `a_thick_wall_slows_the_ball_by_the_weight_it_smashes_until_it_knocks_loose` (12 bricks deep: 8
  smashed, the energy's ~9, then knocked loose, stopped inside), `the_ball_drags_loose_pieces_along_its_path` (a piece
  0.9 m off its line, its box clear of the ball, pushed 0.4 m its way; red with the drag off), and the flat roll breaks
  nothing; `dwell-world` `cannons::tests` (the size, mass, one ball, no reload).

## 10. As built: C5 steering, and cannons on bodies (0.75.0, 2026-11-18)

- **The helm** `dwell:helm` (block data `use: helm`, a new use word; one plank-textured cell facing away from the
  player, the ship's forward; in the hotbar). Its data: shared/cannons/cannons.json `helms` (`thrustN` 6,000,
  `rudderNmPerMs` 4,000; `HelmDef`, `cannons::helm`).
- **A Use on a body** (a Place aimed at a body cell whose block has a `use`, whatever is held; the client sends it as a
  `BodyEditRequest` Place, material 0; the server's `use_body_block`, within reach, one edit credit): a **helm** puts the
  player at it (`helms`: session → body and cell; the controller's `helm`, PLAYER_CONTROLLER.md §6.14); a **cannon**
  fires along its facing turned with the body, the ball given the body's velocity at the cannon too, and the body
  **recoils** by the ball's momentum at the cannon (`launch_ball`'s `recoil`). Reload per cannon and body
  (`cannon_ready` by body id and anchor).
- **Steering** (`steer_ships`, each tick before the physics step, while the body is afloat): forward input × `thrustN`
  along the helm's facing turned with the body (flat), at the centre of mass, as a sail would push; sideways input ×
  `rudderNmPerMs` × the body's speed along that heading, a turn about the face's up (input right turns the heading
  toward heading × up), as a rudder would: none when still, reversed backing. Off the helm (a jump), the body gone or the
  helm broken, the entry goes.
- **The helm's camera** (owner, 2026-11-18: "The camera should smoothly transition to 3rd person when you grab the
  helm"): the helm's data `camera` (`backM` 5, `upM` 1.5, `easeS` 0.6); the client eases a blend toward 1 while its
  predicted controller is at the helm and back to 0 after (`helm_blend_step`), and draws the eye that far back along the
  look and up along the face's up by the blend's smoothstep (`helm_eye`; the aim stays at the eyes); while out, your
  whole body is drawn and your arms aren't. Test: `dwell-client` `the_helm_camera_eases_out_behind_and_up_and_back_in`
  (both faces).
- **Protocol v24**, the **0.75 line**: the controller state's `slide_flags` carries `HELM` (bit 2).
- **Tests:** `dwell-sim` `tests/ships.rs` (both faces): a player at the helm sails a raft north, stands still on the
  deck, turns it right, and a jump leaves the helm; a rudder doesn't turn a still raft.
- **A cannon on a body** (2026-11-18): its rear bottom cell found in the body's own grid with the body's face
  (`cannons::anchor_in`; a body's rows are no world rows to read a face from). The cannon block weighs 1,500 kg/m³
  (~3 t over its four cells, a real cannon's weight; at solid steel's 7,800 it was ~15.6 t and heeled a plank raft
  over ~29°). Test: `tests/ships.rs` `a_cannon_on_a_turned_raft_fires_along_the_raft_and_kicks_it_back` (both faces:
  the raft a quarter turn round and level, the ball along the turned barrel, the raft kicked back).
- **Not yet:** sails as blocks, a rudder block, the explosive_ball block's removal (with the next line).

## 11. Ship battles: balls against bodies, multi-cell blocks on bodies (2026-11-18)

- **A ball against a moving body** (the goal: "ship battles where cannons can blow off pieces of ships";
  `smash_bodies`, after the world's cells in `smash`): the cells of every body along its path this tick (its sphere
  drawn in 5 cm, tested in each body's own grid at its pose), in the order it meets them, are destroyed while the ball
  moves at `destroySpeedMs` or more (paying `energyPerKgJ` × the cell's mass), the first met slower comes off as a
  loose natural piece where it was (moving as the body did there); the cells of that body within `dragRadiusM` of
  where it met each come off loose too (at most `maxLooseCells`). `take_body_cell`: `Felling::break_cell` (what's left
  apart splits off), every client hearing the body's change. Test: `tests/ships.rs` `a_ball_blows_cells_off_the_raft_it_hits`
  (both faces; red with it off).
- **Multi-cell blocks on a body** (BUGS.md #208): a cannon, a door or a 1 m torch placed against a body's cell goes in
  whole (all or none), laid out in the body's grid by the body's face (`parts::partners_in`, `Felling::place_cells`).
- **Dev tools** (`d.scene(op, json)`, `LocalCore::scene_dev`): `body` {kind, at, yaw, cells: [[x, y, z, "state"]…]}
  adds a moving body (its id back); `use` {body, cell} is the local player's Use of a body's block (a cannon fires, a
  helm takes them); `bodies` lists every body (id, kind, pose, cells). For recordings now; the owner's answer
  (2026-11-18): bodies are ultimately to be made with the copy-paste (region) tooling.
- **The harbour scene** (shared/theater/scenes/harbour.json): a broad still lake 5 m deep for rafts and boats.
- **The build platform's look**: a blue grid (owner: "planks look exactly like build platform").
- **Tuning from the first recordings** (2026-11-18): the cannon's `recoilShare` 0.2 (new data: the share of the ball's
  momentum that pushes the body back; the whole of it sent a 12 t raft back 1.7 m/s a shot, out of frame in a few);
  the helm's `thrustN` 20,000 and `rudderNmPerMs` 20,000 (a 16 t boat barely moved at 6,000; 40,000 spun a small raft
  round in 4 s); its camera 8 m back and 3 m up over 0.8 s (5 m and 1.5 m showed mostly deck).
- **The helm's push scales with the ship's mass** (owner, 2026-11-18: "The movement force should be scaled based on the
  mass of the boat, so big boats can move at a reasonable speed too"): `thrustAccelMs2` 1.2 (replacing `thrustN`): the
  force is the body's mass × it. Test: `tests/ships.rs` `a_big_boat_sails_about_as_fast_as_a_small_raft` (both faces:
  8 × 12 × 2 against 4 × 4 × 1 in 3 s; with a fixed 16 kN the big one went 1.3 m to the raft's 8.4 m).

