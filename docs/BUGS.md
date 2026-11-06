# Known bugs

Open bugs reported by the owner, with how to reproduce them and what is known. Fix them one at a time, each with a
regression test that fails first (CLAUDE.md). When a bug is fixed, move it to **Fixed**, giving its version and the test that
pins it.

## Open

### 34. e2e play.spec's two-client test times out on CI since 0.49.4

*Found 2026-11-06, from CI.* "two clients on a native server see each other move": page B (on a
150 ms, 5 %-loss link, `?netsim=150,20,5`) never reports its terrain ready within 20 s. It passes
locally on a real GPU (14 s). Main passed it at #336 and #337 (each twice, a re-run today included)
and has failed it on every run since: main at #340, PRs #338, #341 and #342 (three runs), main
with #338 reverted, and main with #340 reverted (both of those include #339). Cutting the water
currents' datagrams to a quarter didn't help, and there is no water near the native server's spawn
(seed 0, (2128, 7544)). Next: re-run main at #340 and bisect #338's pieces on a branch from #336.

### 30. A held Place builds too little at a few frames a second (partly fixed in 0.49.2)

*Found 2026-11-05 (0.48.2), from CI.* Under the e2e suite's software renderer (CI ~5 fps, a Mac's
SwiftShader ~3 fps) a held-Place build in `e2e/buildDrag.spec.ts` built its first piece and no more
(towering, walking back in Line mode, bridging help, held ladders, crouch-jump towers); those tests
had failed on CI since they were written (0.38.5). Three causes. A key pressed and let go before a
tick took the input (a slow frame, or the prediction held while the server caught up) was never
sampled: the jump was lost. The server checked each piece against
its view of the player's body, which lags the client's by the inputs still queued, a whole slow
frame of them: a piece the client placed once its body was clear went into the server's body and was
refused (a region fill's cell skipped). And the client looked for the moment a jump cleared the next
piece once a frame, and at 200–300 ms frames that ~0.25 s moment fell between frames. Fixed in 0.49.2: a key tapped between ticks counts for the next tick (`input.rs` `tapped`); a
place into a player, or a region edit entirely into one, waits up to `EDIT_HOLD_TICKS` (30) checked
each tick (`server.rs`), and the held stroke is checked at every predicted tick (`game.rs`
`drag_tick`). `game_local.rs` `towering_builds_a_metre_a_jump_at_five_and_three_frames_a_second`
(red before: 0 m of 4 at 5 fps) and `a_jump_pressed_and_let_go_between_two_ticks_still_jumps` (red
before: 0 m). The e2e spec now holds each jump for 9 of the controller's ticks by its tick trace, not
150 ms of the clock, which at a few frames a second can be one tick (a hop too low to build under).
At 2 fps a tower still misses some jumps.

**Still open:** on CI after 0.49.2, walking back in Line mode and the crouch-jump tower pass, but
towering, both bridging tests (a tower first) and the held ladder still fall a piece short (34.5 or
35.5 m of 36; the ladder's first piece missing). Main's shard 1 failed six of these before. Next:
the tick trace on CI (it builds a metre a jump locally at 3 fps).

### 23. A block can't be placed against the downhill side of a ramp

*Reported 2026-11-03 (0.44.2).* "I can't place a block against the outer edge of (downhill) side of a
shallow ramp, maybe same problem with steep ramps too?" Not investigated yet: targeting a ramp's low
face, or the placement check against its shape.


## Fixed

### 36. The bottom half of steep slopes under moving water stayed dry (fixed in 0.49.12)

*Reported 2026-11-06.* "The bottom half of steep slopes is still either not flooded or visually not
flooded, creating more water visual artifacts." BUGS.md #26 again, on the surface grid: a ramp (a
shaped block with a `flooded` state; steep slopes are two, `steep_lower` under `steep_upper`) was solid
to the grid (0.48.2), and the grid's drawing never wrote over a solid cell, so water rising over a
ramp never set its `flooded` state and its open part showed air (nor did it dry when the water went).
Fixed: the grid's copy of the world marks such blocks (`cells::HOLDS_DRY`, `HOLDS_WET`), and drawing a
column floods the ones in its rows and its neighbours' while the water beside or over them stands
half a fill step into their cell, dry otherwise (`Water::flood_near`; written as the `flooded`
property by name, `Write::flood`, checked against the world as other writes are). Pinned by
`water_risen_over_a_steep_slope_floods_its_ramps_and_dries_them_when_it_goes` (both faces; red before:
the steep slope's lower ramp stayed dry under the flooded pit).

### 35. The status line read 60 fps on a phone drawing 30 (fixed in 0.49.10)

*Reported 2026-11-06 (0.49.9).* "It's saying I'm running at 60FPS but I'm clearly running at 30 or even
lower, just moving the camera is stuttering." The meter counted every animation frame the browser
offered, but while the GPU is still behind on two frames the game skips drawing one (render/pacing.rs,
`draw.skipped`), so a phone whose GPU took ~33 ms a frame (the water close up, WATER_RENDERING.md §8
"Cost close to the water") drew every other one and still read 60. Fixed: `Client::frame` says whether it
drew, and the meter counts only those (`ui/fps.test.ts`, red: 60 for a page drawing every other of 60
frames, green: 30). The stutter itself is the GPU's: the frame rate now shows it.

### 33. Damming a river stalled the game (fixed in 0.49.7)

*Reported 2026-11-05 (0.49.5).* "Water definitely seems to be hanging my game thread … my frame rate
is fine, but I get stopped while moving … This is when I dam a river usually." The frame rate held
while the server fell behind, so the client held its prediction until it caught up. Water runs on
game time (72 × real at the default day): a water tick, every six server ticks, was 7.2 s of water,
about 45 substeps over every awake chunk. In a native server with a cut lake flooding out, those
ticks took 12–34 ms and grew as chunks woke (the grid's other work under 1 ms). Fixed: a tick does at
most `stepBudget` column-substeps (`flow.json`, 40,000; ~2.5 ms natively with 60 chunks awake) and
the rest of its water time is lost, not counted toward calm. `water::tests`
`a_water_tick_keeps_to_its_step_budget_however_much_water_is_awake` (red before: 50 substeps over
10 awake chunks).

### 32. Dead on face B, the third-person camera was upside down (fixed in 0.49.6)

*Reported 2026-11-06.* "When you die on the upside half of the world, the 3rd person camera is
flipped upside down (again)." When the player dies its body leaves the physics world and the
predictor's controller falls back to face A's frame; the death camera (and the drawn body) used
that face, so on face B the view was face A's: upside down. Fixed: the death camera takes the face
the body lies on (`FaceFrame::at_height` of the death position) and sits 4 m behind it along that
face's look and 2.5 m along its up (`game.rs` `death_camera`). `game.rs`
`the_death_camera_is_upright_on_the_face_the_body_lies_on` (both faces; face B red before).

### 31. Flat blue sheets on rivers behind their steps (fixed in 0.49.5)

*Reported 2026-11-05 (0.49.x).* "Flat darker colored blue faces" on rivers, seen looking upstream:
from below a stepped river's upper pool, the eye saw that pool's surface from underneath through
the step's riser, and the water shader drew a surface's underside as seen from inside the water
(the body's flat colour) though the eye was in the air. Not culling, and no air or unflooded
slope: the cells were water over sand and flooded ramps. Fixed: an underside seen from outside the
water isn't drawn (`underside_from_outside`, WATER_RENDERING.md); the thin risers stay until
rivers aren't drawn in steps (Phase 53 G1b). `shaders.rs`
`a_water_surfaces_underside_is_drawn_only_from_inside_the_water` (red before); checked in the
browser at seed 0's river (6744, 9643): the sheets are gone.

### 28. A junction at the top of a chute can leave a lip a cell short (gone in 0.49.0)

*Found 2026-11-04 (G1, the graph checker).* Seed 0, river 17 at (16555, 2142): a tributary joins
where river 17 drops into a chute, so the step's band is split between the two rivers and river 17's
lip has room for 3 of its 4 cells. One lip in 44,401 on seeds 0–5; the checker allows one in ten
thousand. Fix: plan a junction step's band across both rivers. Gone with the lips (Phase 53 G1a): the generator draws none.

### 27. On a steep diagonal cascade a lip can still pour into a cell too many or too few (gone in 0.49.0)

*Found 2026-11-04 (0.47.0 work, the strict sweep at eight pools a seed).* Seed 0 near (−15250,
91, 1840): pools a couple of cells long, a step every 2 m on a diagonal; a lip's band runs into
the next lip's, and a two-cell pool perched beside the river drains once found. The sweep checks
four pools a seed (it passes); at eight this site fails. Suspects: the next lip's crest not under
this lip's, bank columns lower than a crest's water beside it. Gone with the lips (Phase 53 G1a): the generator draws none.

### 29. Dirt walls stuck out into rivers at their step-downs (fixed in 0.49.0)

*Reported 2026-11-05 (0.48.1), seen "for quite a few versions".* Blocks of the bank (dirt, grass on
top) reach into the channel where a river steps down a row, standing over the water. A river
generation artifact: the step's **lip** (ADR 0058, `terrain.rs` `LipPlan`, `river_lip_rise`): the
generator raises a crest across the channel `lip_cells` wide and walls the columns behind and beside
it `river_lip_rise` rows over the water, so the basin engine's weir law held each pool exactly at its
drawn level. Since 0.48.2 the water surface grid rests the world's water as drawn whatever its shape
(its rest layer, WATER_SURFACE_GRID.md §3), so the lips do nothing. Fixed in 0.49.0 (Phase 53
G1a): the generator draws no lips; `terrain.rs` `no_dry_wall_stands_in_a_rivers_channel` (red before:
seed 0's (−6159, 3946) stood a row over its river's water).

### 26. Water risen over a steep slope left air in the bottom half of its ramps (fixed in 0.47.0)

*Reported 2026-11-04 (0.45.2).* "The bottom half of steep slopes … don't fill with water leaving
ugly air gaps visually in the water around underwater steep slopes." The ramps beside a body as
found held it like walls (`rest`) and still did when a dam's flood rose over them, so they were
never flooded. Fixed: rest cells the water rises over are held no longer (`submerge_rest`). Pinned
by `water_risen_over_a_steep_ramp_floods_both_its_cells` (both faces; red before).

### 25. Water invisibly filled nearby pools (fixed in 0.47.0, ADR 0058)

*Reported 2026-11-04 (0.45.2, seed 3695715780, a dam).* "Water is invisibly filling nearby pools,
essentially the visible water doesn't seem to match what's happening … Water should have to
visibly connect pools, or they shouldn't be connected." The pool below their dam took in the
river's whole flow at its own cells (`feeds`), the pool above none; links and seeps moved water
unseen too, and water stood in walls against the hidden `rest` cells where generated pools met.
Fixed by ADR 0058 (WATER_FLOW.md §18): a pool takes in only what is seen pouring in, no links,
seeps only at the loaded world's edge, and lips planned so each passes what the one above it does
(steps fully walled but for the crest). Pinned by `a_pool_nothing_is_seen_pouring_into_takes_
nothing_in` (red on 0.46) and the strict sweep at twelve pools.

### 22. The river below a dam rose (fixed in 0.45.1 and 0.45.2)

*Found 2026-11-03 (0.44.3, the owner's seed 0 dam, through the server natively); on 0.45.0 the river
below the dam rose 0.31 m, and the rapids further down (~15 pools a few metres apart) 0.2–0.25 m.*

- **Causes:** (1) the dam's split left the link from the pool above with the part below it (0.45.1);
  (2) each body's own inflow was taken again every tick as what its lip carries less what the found
  bodies above bring: a part split off below a dam has no entry of its own, so it passed nothing on
  in the books and the pools below counted the river as their own besides what arrived; (3) a loss
  (a lip above carrying more than a pool's own) drained a pool dry while a dam above filled, and it
  came back as another body; (4) a split left a link's source with the old part, so a fall's water
  was taken off the pool below twice.
- **Fix (0.45.2):** a body's own inflow is kept with the cells it comes in at (`Steady::feeds`): set
  when it is found (what its ways out carry less what the found bodies above bring), taken off once
  as each body above links in (`link_once`), carried by splits and summed by merges; a body with a
  way out of its own passes what that carries; a loss takes only what arrives; a split moves a
  link's source and target to the parts that hold them.
- **Tests:** `a_dam_that_splits_a_pool_leaves_the_river_below_at_its_level` (0.45.1; red: 52.72 m
  against 52.5 m), `a_pool_losing_to_the_ground_keeps_its_water_while_a_dam_above_fills` (red: the
  pool empty at 52.0 m when the spill came back). On the owner's dam every pool below it, down the
  rapids, stands at its found level; the dam's pool at 140.63 m over its 140.50 m crest.

### 24. Rising water vanished and came back higher at each step, and slopes didn't rise with it (fixed in 0.44.5)

*Reported 2026-11-04 (0.44.4, seed 0, the dam at x 2359 z 7437).* "The water definitely isn't filling
smoothly over time, it's still jumping 0.5m in height every few seconds as it rises." "The water
actually disappears and then reappears at a slightly higher level." "And slopes with water don't
correctly show this dynamic raising."

- **Causes** (found in the browser: the cells read every frame, the page recorded and its frames
  compared, the chunks meshed natively): (1) water's writes go out as `Rebake` modifications, and the
  client hid every cell a `Rebake` wrote until a settling piece's glide claimed it or half a second
  passed: each step of a rising pool drew its surface row as air, the row below showing, for half a
  second. (2) The water wrote its cells with `set_voxel`, a chunk revision per cell, then sent them
  as one modification: the client saw a gap every time and asked for the whole chunk again (1,845
  resyncs in one session). (3) A body's surface was drawn at the nearest fill, so a level hovering at
  a rounding point (the pool below the dam, fed in pulses) flipped each tick, and at a row's floor
  the row came and went. (4) A flooded ramp's water was drawn at the full row whatever the water
  beside it.
- **Fix:** only cells within a falling piece's reach wait for its glide (`Pieces::near_cell`); the
  water writes with `VoxelWorld::write_voxel` (no revision per cell); the drawn fill holds within
  `fillHysteresis` (`shared/fluids/flow.json`, ¼ step) past the half step; a flooded ramp's water is
  drawn at the fill of the water beside it in its row, none while the water is only below it.
- **Tests:** `a_rebake_far_from_any_piece_hides_nothing`,
  `the_waters_modifications_reach_a_client_in_revision_order` (red: revision 154 after 129),
  `a_level_hovering_at_a_step_is_drawn_steadily` (both faces; red: the row drawn 0, 22, 0, 22, …),
  `a_flooded_ramp_draws_its_water_at_the_fill_of_the_water_beside_it` (red: 3.875 against 3.219).
  Checked in the browser on the owner's dam: no resyncs, and the recorded pool rises a step at a time
  with no frame drawn lower or darker between.

### 21. A dam across a river didn't raise it (fixed in 0.44.3)

*Reported 2026-11-03 (0.44.2, seed 0 by the spawn, screenshots).* "I dammed that river. When I did, it
flooded that valley's bottom, but didn't continue filling, and there's no visual or effective water
flowing over the banks and down the hill?"

- **Cause:** the river's stretch there has no way out of its own (its riverbed's ramps close both ends,
  walls to the water engine), so it passes the map's flow unseen (a seep). The seep had no place: the
  dam split the stretch in two and both halves kept it, so the half behind the dam drained the river
  through it and never rose.
- **Fix:** a seep is where the river leaves the stretch: its cell farthest down the map's river (on the
  river most of its cells lie on), else farthest from where the water comes in (`Steady::exit`, saved,
  record version 4); only the part holding it has the seep (WATER_FLOW.md §2.2). A split's feed whose
  cell is in no part goes to the nearest part. Rivers through flooded ramps (the owner's preferred
  fix) were tried and collapsed the river's steps in the generated sweep; kept as a later option.
- **Test:** `a_dam_across_a_closed_stretch_holds_the_river_back` (both faces, the dam a block a tick;
  red on 0.44.2: the river behind it stayed where it was found). Checked natively through the server on
  the owner's river (seed 0, x 2359 z 7437): the river behind the dam rose and spilled over it.

### 20. A dam's flood reached only so far up the river, and an edit there lowered it (fixed in 0.43.3)

*Reported 2026-11-03 (0.43.2).* "When I dammed a river, it filled to the new level only so far back
along upriver. And then attempting to update that part of the river by making a block edit, lowered
the level of the river down to that up to the dam and stopped flowing in that direction."

- **Cause:** an unloaded cell is a wall to water, and a basin was found again only when an edit or its
  level changed it, not when the chunk beside it loaded: the flood stopped where the loaded world
  ended when it rose. The river found later beyond that edge, its water touching the flood, had no
  natural outlet, and its seep passed on everything that came in, the flood's water too: it drained
  the flood unseen.
- **Fix:** a basin keeps the unloaded chunks beside it (`edge`) and is found again when one loads; a
  seep passes the body's map flow and what found bodies' natural outlets bring it, not what an edit
  or a flood sends it, which fills it (WATER_FLOW.md §15). Saving the basins and fast-forwarding the
  ones out of reach stay W2.
- **Test:** `a_dams_flood_goes_on_up_the_river_when_its_chunks_load` (both faces; red on 0.43.2: the
  flood stopped at the chunk edge; with only the edge fix, the river found there drained it).

### 19. Water filled up to a wall across a river, not over it and down the other side (fixed in 0.43.1)

*Reported 2026-11-03 (0.43.0, screenshot of a stone wall across a river).* "Water fills up to the
wall blocking the river, not over it and down the other side. And it doesn't flood when the wall is
at the same level as the water."

- **Cause:** the pool stayed one basin over the wall (joined by the open row above it), so one level
  and no step at the crest; the spill over a wall, a few centimetres deep, was under the half row a
  row is drawn at, and running water wasn't drawn at all (W1); the pool below the wall, cut off from
  the map's flow, kept passing only its links' flow and filled with the spill.
- **Fix:** basins joined only above their full rows are separate and merge only through a row both
  fill; spills drawn (the crest as water, the way down as `dwell:running_water`); a found body passes
  what comes in each step, its links found where its natural outlets ran as found (WATER_FLOW.md §15).
- **Test:** `a_wall_across_a_river_is_spilled_over_and_the_river_below_holds` (both faces; a wall
  level with the water and one row higher; red on 0.43.0: the river above didn't rise).

### 18. Ramps beside the water stood dry, the water ending in a wall (fixed in 0.42.2 and 0.43.0)

*Reported 2026-11-03 (0.42.1, screenshot).* "Slopes next to water aren't flooded at generation time
or update: you can see the water wall edge."

- **Cause:** a ramp's open part holds water only in its `flooded` state; neither the water flow's
  writes nor the generator gave it to a ramp beside the water at its row (the generator floods a
  ramp only in a column under water).
- **Fix (update):** the water's writes flood the blocks with a `flooded` state beside the rows they
  wet, and unflood them when no liquid is beside them (`water.rs` `flood_edge`).
- **Fix (generation, 0.43.0, a new compatibility line; owner, 2026-11-03: "world gen should fill
  slopes"):** a ramp piece in a row where a neighbouring column holds water over its ground is
  generated flooded (`terrain.rs` `generate`). Treating ramps as open to water was considered and
  set aside (owner: "weird directional edge cases").
- **Test:** `ramps_at_the_waters_edge_flood_and_dry_with_it` (both faces; red on 0.42.1);
  `dwell-worldgen` `tests/worldmap.rs` `ramps_beside_a_rivers_water_hold_it` (red before: 244 of 499
  ramps beside the fixture river's water dry).

### 17. A block set at the bottom of any river flooded it without end, or drained it (fixed in 0.42.2)

*Reported 2026-11-03 (0.42.1).* "Putting a single block at the bottom of the river causes it to
flood continuously, rising higher and higher … flying around seems to delete basins?" and "just
adding a block to the bottom of any river makes it flood endlessly, or seemingly randomly drain
completely."

- **Cause:** W1 took a body's inflow from what left it just before the edit, by the basins' rules,
  which the generated water doesn't keep: plants, banks and the next step beside it counted as
  outlets (one pool 61 m³/s in, a lake 1,059 m³/s); a higher pool merged with the lower ones it met
  below its surface, the river one basin with every pool's inflow and no way out; a pool found
  downstream filled before its ways were found; 7.2 s ticks overshot small pools; basins whose
  chunks were evicted lost their water, and waiting ones filled with what live ones sent them
  (WATER_FLOW.md §16).
- **Fix:** the redesign (ADR 0055, WATER_FLOW.md §2.2): the world as found is at rest, its natural
  outlets passing the world map's flow, linked into one flow network; merges at one level; flow
  steps; ways found in rounds with water in transit; waiting basins pass on what comes in and wait
  while a chunk of them is unloaded.
- **Test:** `a_block_set_in_a_generated_rivers_bed_changes_nothing_else` (seed 1, two rivers, six
  hours of water time; red on 0.42.1: 173,921 cells changed), `a_river_found_at_rest_keeps_its_banks_dry`,
  `a_basin_waits_while_a_chunk_of_it_is_unloaded`, `a_waiting_basin_passes_on_what_comes_in` (both
  faces; red on 0.42.1).

### 16. Blocks in a riverbed flooded a stepped fall, then the river drained dry (fixed in 0.42.1)

*Reported 2026-11-03 (0.42.0, a local terrain world).* "I added a row of 3 half blocks to the bottom
of the river, just below a ~3 block high stepped water fall. The river flooded up to the full height
of the waterfall top … then it filled downstream, then it suddenly drained completely."

- **Cause:** water flow W1 (docs/WATER_FLOW.md) found the pool and the pools above it together, each
  with the inflow that left it, so the fall's water was counted twice and the pool rose; rising, it
  merged with the pool above at once, though that pool's surface was higher, and so up the falls; the
  merged river had one level, set by its lowest way out, which left the upper steps dry, their
  inflow no longer where it came in.
- **Fix:** a basin's inflow from a pool now found upstream is taken off it (`Water::credit`); basins
  merge only when their levels meet (until then the higher pool's water is a wall, and it falls into
  the lower); inflows are kept where they come in, and a receding flood leaves each step that water
  comes into its own pool (`spin_off_shelves`).
- **Test:** `blocks_set_in_a_riverbed_under_a_stepped_fall_change_nothing` (three pools falling two
  rows each, three cells set under the fall; red on 0.42.0),
  `a_pool_and_the_pool_above_it_found_together_count_the_falls_water_once`,
  `a_flood_over_a_rivers_steps_leaves_each_step_its_water_when_it_recedes` (both faces; red before).

### 15. Cliffs walled the rivers in, and water stood open beside them (fixed in 0.35.0)

*Reported 2026-10-27 (0.34.0, the cliffs world, seed 3764472973 face A near (-3794, 240, -5614)).*
"Seeing issues with rivers in the cliff environment: big edges rising up on sides of river, and
drop off edges without filling low areas."

- **Cause:** the cliffs' slope mask read a river valley's steep banks as a hillside, so it raised
  walls along the water and sank the ground beside it below the water's level (and reshaped wet
  columns themselves).
- **Fix:** water columns are never reshaped; dry ground is never lowered or carved under the sea's
  surface; the cliffs fade in 8–30 m from a river or lake (`volume.json`'s new `water` input).
- **Test:** `cliffs_keep_off_the_rivers` and `no_water_stands_open_beside_lowered_ground` (seed 7,
  both faces: 53,731 / 44,935 open water faces on 0.34.0, none on the plain terrain or now) and
  `cliffs_keep_off_the_river_at_the_report` (4,945 of 5,930 columns by the water reshaped; none now).
  Checked in Chrome at the report's spot, before and after.

### 14. A steep edge up the hills where two river valleys meet (fixed in 0.32.0)

*Reported 2026-10-27 (0.31.4, a phone; seed 1397629041, face A, feet at (-7464, 106, 8376)).* "There's
still a bug with river valley meetups … It's pushing up a steep edge at the join point of the
terrain, though not by the water, but up the hills adjacent."

- **Cause:** each column carved its valley toward the nearest river channel alone (`terrain.rs`
  `column`). Where two rivers' valleys overlapped, the nearest switched from one to the other along
  a line, and the ground jumped there by the difference in their levels, out to the valleys' reach
  (12 m, ~100 m north of the report).
- **Fix:** the valley is carved by every channel whose valley reaches the column, the lowest
  (`Channels::each_near`), which cannot step. Generator 26, a new compatibility line.
- **Test:** `no_seam_where_river_valleys_meet` (round every confluence of seed 7, both faces: 600 and
  64 steps against the slope on the old rule, none now) and
  `no_seam_at_the_reported_meeting_of_valleys` (the report's spot: 297 steps, none now). Checked in
  Chrome from the report's viewpoint, before and after.

### 13. Crossing the midplane swapped the clouds, the fog and the light (fixed in 0.31.2)

*Reported 2026-10-27 (0.31.1).* "It's still an abrupt transition between the two world sides when you
move between them. Clouds disappear from one side, appear on the other, lighting changes (I think),
and a white fog band switches sides. So a clear cloudless night sky turns into cloudy, and cloudy blue
sky turns into vague foggy white."

- **Cause:** the sky's colours, the fog's colour and density, the exposure and the cloud deck were the
  viewer's face's, swapped across the 4 m flip band: only the viewer's deck was drawn, and the other
  face's sky below the horizon was hazed in the viewer's colours.
- **Fix:** the world owns its look (DAY_NIGHT.md §13, ADR 0050): each ray takes the sky, clouds and fog
  of the air it passes through; both decks are drawn, each lit by its own face; the exposure and
  saturation ease over `eye.adaptS`.
- **Test:** `the_sky_behind_is_the_same_from_either_side_of_the_midplane` (dwell-look: every ray 0.3 m
  above and below the midplane; 0.77 apart on 0.31.1, under 0.02 now), with the shader tests in
  DAY_NIGHT.md §13. Checked in Chrome at seed 7's portal axis, before and after.

### 12. A hard dark (day) or white (night) band past the rim of the world (fixed in 0.31.1)

*Reported 2026-10-27 (0.30.x, on a phone).* The 0.28.5 fix for bug 10 "looks really bad on the
horizon … a very harsh transition between the two and you can see a black edge during the day and
a white edge during the night"; "If you can look down off the edge of the world, I don't mind if you
can see a pretty transition."

- **Cause:** below the viewer's horizon the sky behind the world switched to the other face's within
  a pixel. Flying high, the gap between the rim and the horizon showed the other face's sky at the
  opposite time of day: night's navy at noon, the day's pale horizon at midnight.
- **Fix:** as through an atmosphere round the disc, a ray just below the horizon shows the viewer's
  horizon haze (it crosses the viewer's face's air to the rim) and the other face's sky fades in as
  the view turns down, fully from ~27° (`daynight.json` `sky.behindFrom`, `behindTo`;
  `dwell_behind_share`), so a portal's shaft still shows the other face's sky.
- **Test:** `the_other_faces_sky_fades_in_below_the_horizon_not_at_it` (dwell-look: the weight is
  ~0 a degree below the horizon, 1 looking straight down, never a step; the old shader had no such
  weight). Checked in Chrome 3.6 km over seed 7, past the rim, at noon and midnight: the band gone.

### 11. River banks walled off the sea, and tall edges where rivers met (fixed in 0.31.0)

*Reported 2026-10-27 (0.30.x, on a phone).* Rivers had "a 1-2 block high, 1-2 block wide edge along
the whole thing" that blocked "off access to the ocean", and "weird tall edges … where rivers meet".

- **Cause:** two rules of the river columns (`terrain.rs`, `worldmap/channels.rs`). (1) Where the
  land beside a river was lower than its water the bank was raised ½ m over it, falling 1.5 m per m:
  a one-cell ridge along every river through low land, and, at the mouths, where the river is at sea
  level and the land under the sea, a wall of sand out into the sea (119 of 185 levees on a 16 m grid
  of seed 7). (2) A joining river reached the other's level only at the other's centreline (and was
  capped at its own level where lower), so across the other's channel its water stood up to 5 m
  above, or ½ m below, the water beside it: a wall of water.
- **Fix (generator 25):** a levee falls away gently (`rivers.leveeFall` 0.15 m per m), a broad bank,
  and none where the river is at sea level. A joining river meets the other at its level across the
  other's channel (`joinMargin` plus both half-widths), the other's water backing up into it where it
  lies lower, and falls to it at most `joinSlope` 0.25 m per m upstream.
- **Tests:** `rivers_meet_at_one_level` (178 and 34 water walls round seed 7's 15 and 9 confluences
  on faces A and B before, none after) and `river_banks_are_not_ridges` (about 1.6 % of the banks
  ridges before, under 1 in 10,000 after: a natural beach cell remains), both faces. Checked in
  Chrome: seed 7's river mouth 16 km north of spawn, sand walls into the sea before, open after.

### 10. A white patch down a portal after visiting the other face (fixed in 0.28.5)

*Reported 2026-10-26 (0.28.x, on a phone).* After falling down a portal and back up, the middle
seemed to unload: a white, see-through patch showed looking down the shaft (toward the night side).

- **Cause:** not unloaded chunks. The sky behind the world was the viewer's: below the horizon,
  face A's horizon colour (pale at noon, navy at midnight). Fresh, face B's chunks round the far
  opening weren't loaded and the shaft ended in black; once the visit had loaded them, the far
  opening framed face B's night sky drawn in face A's daytime colour, with B's grass and a tree in
  silhouette round it. Found in Chrome on seed 7's portal (x 1779, z 20690) with the LOD off
  (`?lod=off`, added for it) and at both noon and midnight.
- **Fix:** below the viewer's horizon the sky behind the world is the other face's, with its sun,
  moon and stars (`dwell_sky_behind`; the frame's `DaySky` carries the other face's sky colours).
  The fog and the reflections keep the viewer's horizon colour (DAY_NIGHT.md §5).
- **Tests:** `the_block_carries_the_other_faces_sky_for_below_the_horizon` (dwell-look; the old
  block has no such colours, out of bounds) and
  `the_sky_behind_the_world_takes_the_other_faces_below_the_horizon` (the composite uses it).
  Checked in Chrome: from face A at noon, face B's midnight sky with its stars shows down the shaft.

### 8. Dark blue rectangles in rows across lakes and the sea (fixed in 0.28.1)

*Reported 2026-10-26 (0.27.7).* Some water rendered its side solid through the water: darker
blue rectangles and triangles in rows across a lake.

- **Cause:** at the edge of the streamed view a chunk's horizontal neighbours aren't loaded, and
  the mesh apron (`VoxelWorld::padded_box`, and `padded_from_neighbours`) read them as air. So a
  chunk of water meshed its side there as a wall, and the walls of the outermost ring showed through
  the surface in rows at fixed distances (seen in Chrome over seed 7's sea). Not the 0.26.7 strip:
  the rectangles stayed with it reverted.
- **Fix:** where a horizontal neighbour is missing and the chunk's own edge column holds a fluid
  (water or a flooded block), the apron goes on with it. Everything else stays air as before (the
  ground's border, and the open surface where no chunk is above). A neighbour's arrival meshes the
  chunk again with its real voxels.
- **Tests:** `the_apron_goes_on_with_the_water_where_no_neighbour_is_loaded` (dwell-world) and
  `a_water_chunk_at_the_streamed_edge_has_no_wall_into_the_unknown` (dwell-game) failed with the
  air apron and pass now.

### 9. A white edge at the sides of the water's reflection, the mountain's cut off (fixed in 0.28.1)

*Reported 2026-10-26 (0.27.7).* A white edge on the left of the reflection, where the mountain's
reflection was cut off.

- **Cause:** since 0.27.5 a reflected ray leaving through a side of the screen took the sky, so a
  mountain whose mirror image ran off the screen ended in a hard white band (0.27.1's clamp to the
  exit pixel was removed for its streaks over open water, bug 6).
- **Fix:** past the side the march goes on with its pixel held in the edge column (the scene taken
  to go on beyond the edge as it is there), and meets only land: a scene point standing over the
  reflecting surface (`SIDE_LAND_M`, 0.25 m). The floor seen through open water lies under it and
  is never met, so no streaks. In the Rust twin and the shader.
- **Tests:** `the_sides_keep_the_scene_not_a_fade_into_the_sky` checks that land past the left side
  is reflected to the edge (failed with the sky fallback) and that a low ray over open water is
  still a miss; `the_shaders_side_margin_is_this_one` pins the shader's margin. Checked in Chrome on
  seed 7: the range is mirrored to both edges, and the open sea shows no streaks.

### 2. Sun and moon shadows leaked through hills in a grid pattern (fixed in 0.27.6)

*Reported 2026-10-26.* Light leaked through hills in a grid pattern.

- **Cause:** inside the shadow cascades (160 m on High) the factor was the cascades' alone, and
  their casters are front faces within `casterReachM` (120 m) toward the light. With a low sun, the
  light's ray to a step on a hill's shaded side passes through the hill and leaves through its
  sun-facing side, often farther than 120 m away. So no caster shaded the step: each step's top
  was lit and its riser dark, a grid down the shaded slope near the player. Past the cascades the
  same slope was in the hill's shadow (the distant-terrain layer). Seen in Chrome on seed 7's range
  at 0.82 (the sun 5° up).
- **Fix:** inside the cascades the distant terrain's shadow multiplies theirs
  (`lod_shadows::combine`, the shader's `shadow_light`), so a hill beyond the casters' reach shades
  there too.
- **Test:** `a_hill_beyond_the_casters_reach_still_shades_inside_the_cascades` (dwell-look) failed
  on the old `combine` (lit at 1.0) and passes now.

### 3. Tall mountains had a permanent shadow band (fixed in 0.27.4)

*Reported 2026-10-26.* Tall mountains had a shadow that stayed put at certain heights. Possibly
related: the shadowed tops disappeared when flying a certain height above them.

- **Cause:** the distant-terrain shadows (DAY_NIGHT.md §12) draw the terrain from above into
  heightmaps between `heights.bottomM` and `topM` (`shared/look/lodshadows.json`). `topM` was
  1,536 m, from before the world map's ranges (generator 21), which reach ~2.6 km. Everything
  above it was cut from the maps and read as shaded: a flat-edged band at one height on every tall
  mountain, the peaks in shade at noon too. Seen in Chrome on seed 7's 2.3 km range.
- **Fix:** `topM` is 4,096 m.
- **Test:** `the_maps_hold_the_highest_mountains` (render/lod_shadows.rs) samples the highest
  ground of three seeds and checks it stays 256 m under `topM`. It failed at 1,536 m (ground at
  2,632 m) and passes now.
- **To confirm:** the tops "vanishing" from high above was likely the same dark shading. Recheck it
  in play.

### 6. Water reflections faded to white at the screen's sides (fixed in 0.27.1)

*Reported 2026-10-26.* Toward the left and right of the screen, reflections faded to a white gradient
where the reflected world should show. The owner's note continued "In addition, I would expect to
see …" and was cut off (see the rest below).

- **Cause:** the screen-space reflection faded every hit near any screen edge into the sky fallback
  (`edge_weight`, `ssr_edge`). A ray leaving through a side with no hit also took the sky. Low over
  land, the sky's horizon haze is near white.
- **Fix:** hits fade only near the top and bottom edges, in both the Rust twin
  (`render/reflection.rs`) and the shader.
- **Test:** `the_sides_keep_the_scene_not_a_fade_into_the_sky` failed on the old logic and passes now.
- **Regression (0.27.1, fixed in 0.27.5):** 0.27.1 also took the scene at a side's exit pixel for a
  ray leaving through that side. Over open water that pixel shows the floor through the water
  (water writes no depth), so it drew streaks fanning along the sides. The clamp is removed: such a
  ray is a miss (the sky). The same test now also checks that a low ray leaving over open water is
  a miss. It failed with the clamp (a full-weight hit at the left edge) and passes now.
- **The rest (fixed in 0.27.3):** the cut-off note was likely that tall mountains farther off didn't
  show in the water (owner, 2026-10-26). The reflection's reach was 2 km (Medium) and 4 km (High),
  so a ray toward a farther range found nothing and took the sky. The reach is now 24 km and 48 km.
  A hit's fade is measured at the scene's depth where it hit, since the march's own depth at the end
  of a step lay far beyond it on a long reach. Test: `far_mountains_are_reflected_and_near_banks_still_are`
  failed with the 2 km reach (no hit on a range 12 km off) and passes now.

### 5. Rivers had tall banks and were always shallow (fixed in 0.27.0)

*Reported 2026-10-26.* Rivers had a tall border along their edges, even blocking the way to the
ocean, and were always very shallow.

- **Cause:** a river's surface on the map often stood above the land along its banks (the coarse
  map's heights round the channel, and flat coasts). The columns then raised a levee to water + 1 m,
  tapering 1.5 m per m, so the water never stood open beside lower ground. That made walls along the
  banks and across the mouths. The depth came straight from the map's width curve (0.5–6 m, at least
  0.5 m).
- **Fix:**
  - Each river vertex's surface now lies at least the bank height (+ 0.5 m) under the lowest dry
    ground of the map cells round it. It never rises downstream and never drops below the sea, so
    levees are rarely needed.
  - The bank is 0.5 m (was 1 m).
  - Rivers are deeper: the depth curve is 0.8, 2, 4.5 and 9 m (was 0.5, 1.2, 3 and 6), at least 1 m.
  - A new compatibility line: worldgen changed.
- **Test:** `rivers_run_below_their_banks_and_down_to_the_sea` (dwell-worldgen `worldmap`, both
  faces) failed before the fix (7,188 and 6,621 river vertices too high) and passes now. The goldens
  and pinned digests are regenerated as generator 22.

### 4. Gaps between water blocks at height transitions (fixed in 0.26.7)

*Reported 2026-10-26.* There were gaps between water surfaces where a river steps down.

- **Cause:** at a step, the upper reach's column (water over water) sits beside the lower reach's
  last cell, which has air over it and its surface drawn ⅛ of a cell below its top. The mesher
  culled the side face between them as "fluid beside the same fluid", leaving an open strip from the
  lower surface up to the upper column's next cell.
- **Fix:** beside the same fluid whose surface is drawn lower, the cell draws that strip, from the
  neighbour's drawn surface up to its own (`fluid_side` now takes a bottom as well as a top).
- **Test:** `a_river_step_down_closes_the_strip_between_the_two_surfaces` (dwell-mesh) failed
  before the fix and passes now.

### 7. Placing a block too close to yourself flickered (fixed in 0.26.6)

*Reported 2026-10-26.* The block flickered in, then disappeared when the server refused it.

- **Cause:** the client predicted edits with the server's own checks, but passed no player bodies.
  So it showed a block placed into its own body, which the server refuses ("into a player").
- **Fix:** the client's prediction passes its own predicted capsule, built as the server builds its
  players'.
- **Test:** `placing_a_block_into_your_own_body_is_not_predicted` (dwell-client `game_local`)
  failed before the fix ("a block placed into the player's own cell was shown") and passes now.

### 1. Rubber-banding after a high-speed launch or a fast flight (fixed in 0.26.4)

*Reported 2026-10-26.* The most common case was a sudden fast speedup in the air.

- **Repro:** in Chrome, hover high up, then hold Shift+W at flight speed 10 or 12. It snapped back
  about 1 km, and about 10 km at speed 12.
- **Cause:** the server falls behind real time while it generates terrain for a fast flyer (its
  fixed step drops the backlog). The client kept predicting, so its inputs piled up in the server's
  queue. The server then threw inputs away in three places: its latency trim (the queue above 6 for
  a second), its queue cap (96, oldest dropped) and its input rate limit. Each skipped input is
  movement the client predicted but the server never simulated, so every snapshot snapped the
  player back. "Loading terrain…" showed because the client had outrun the streamed terrain.
- **Fix:** the client holds its prediction while the server reports 30 or more inputs queued
  (resuming at 6) or while 45 are unacknowledged. The server's trim became a backstop above 60, and
  it no longer trims a draining queue. Tests: `a_stalled_server_catches_up_without_dropping_the_players_inputs`
  (dwell-client `game_local`) failed before the fix (183 inputs skipped, then a 10.7 m snap) and
  passes now; `a_slow_device_walks_without_snapping` still passes.
