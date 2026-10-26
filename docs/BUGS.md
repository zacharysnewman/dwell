# Known bugs

Open bugs reported by the owner, with how to reproduce them and what is known. Fix them one at a time, each with a
regression test that fails first (CLAUDE.md). When a bug is fixed, move it to **Fixed**, giving its version and the test that
pins it.

## Open

### 2. Sun and moon shadows leak through hills in a grid pattern

*Reported 2026-10-26.*

A hill between you and a low sun or moon doesn't fully block the light. Some block faces are lit and others are shadowed,
leaving a grid pattern.

- **Suspects:** the shadow casters cull faces (back faces, merged quads, caster distance), or the depth-only pass's bias or
  normal offset is too large for ½ m steps.

### 3. Tall mountains: a permanent shadow band, and shadowed tops vanish from high above

*Reported 2026-10-26.*

Tall mountains have a shadow that stays put at certain heights. Possibly related: the shadowed tops disappear when you fly a
certain height above them.

- **Suspects:** the horizon-map shadows (0.22.19) or the cascades' depth range not covering 2 km relief, the casters'
  `casterReachM`, or the LOD or horizon-map heightmap levels clipping.

### 4. Gaps between water blocks at height transitions

*Reported 2026-10-26.*

Where water steps down (the river terraces of 0.26.0), there are gaps between the water surfaces.

- **Suspect:** the fluid mesher doesn't close the vertical step face between water of different levels.

### 5. Rivers have tall banks and are always shallow

*Reported 2026-10-26.*

Rivers have a tall border along their edges, which even blocks the way to the ocean. They are also always very shallow.

- **Owner's idea:** the generator may be carving too much, instead of filling low regions and then continuing to flow.
- **Suspects:** the generator 21 channel and levee shaping in `terrain.rs` (lake levees?), and the river depth curve in
  `hydrology.json`.

### 6. Water reflections fade to white at the screen's sides

*Reported 2026-10-26.*

On water, the reflection fades to a white gradient toward the left and right of the screen, where the reflected world should
show.

- **Suspect:** the screen-space reflection running off screen and falling back to a sky or white colour instead of the sky
  probe or environment.
- The owner's note was cut off ("In addition, I would expect to see …"). Ask what else is expected.

## Fixed

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
