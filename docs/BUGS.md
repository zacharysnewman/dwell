# Known bugs

Open bugs reported by the owner, with how to reproduce them and what is known. Fix them one at a time, each with a
regression test that fails first (CLAUDE.md). When a bug is fixed, move it to **Fixed**, giving its version and the test that
pins it.

## Open

None. Report new ones here.

## Fixed

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
