# Known bugs

Open bugs reported by the owner, with how to reproduce them and what is known. Fix them one at a time, each with a
regression test that fails first (CLAUDE.md). When a bug is fixed, move it to **Fixed**, giving its version and the test that
pins it.

## Open

### 1. Rubber-banding after a high-speed launch or a fast flight into the ground

*Reported 2026-10-26. Long-lived; not new.*

- **Repro A (launch):** start flying, stand still, then start moving at high flight speed and turn flight off as you speed up.
  You're launched in that direction (the owner likes the launch itself). Then you're stuck rubber-banding back to where the move
  started, over and over.
- **Repro B (into the ground):** fly into the ground at medium-high speed. You're stuck the same way. At the highest speed you may
  instead clip through the world to the other face.
- **Observed:** the game isn't frozen (the menu opens). Once in this state you can't move away from that spot. The
  "Loading terrain…" indicator shows during it and disappears right when you snap back.
- **Suspects:** the client and server fall out of sync with no recovery. The server may hold or reject the player at a chunk
  that isn't loaded or generated yet while the client keeps predicting ahead. Tunnelling at high speed may explain the clip to
  the other face.

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

### 7. Placing a block too close to yourself flickers

*Reported 2026-10-26.*

The client lets you try to place a block too close to yourself. It flickers in, then disappears when the server rejects it.

- **Suspect:** the client's placement check doesn't match the server's player-overlap check, e.g. a different body box,
  ½ m cells, or crouch height. The client should refuse the placement exactly as the server does.

## Fixed

(None yet.)
