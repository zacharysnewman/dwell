# Known bugs

Open bugs reported by the owner, with how to reproduce them and what is known. Fix them one at a time, each with a
regression test that fails first (CLAUDE.md). When a bug is fixed, move it to **Fixed**, giving its version and the test that
pins it.

## Open

None. Report new ones here.

## Fixed

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
