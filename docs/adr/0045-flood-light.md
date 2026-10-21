# 0045. Flood light: per-cell sky and block light on the client

- Status: Accepted (the owner's direction, 2026-10-21)
- Date: 2026-10-21
- Design: [`DAY_NIGHT.md`](../DAY_NIGHT.md) §11; plan: Phase 45

## Context

0.22.5 (Phase 43) baked corner occlusion and a short-ray sky visibility into each terrain vertex and
scaled only the hemisphere light by it. The owner carved a tunnel and found it "lit up like day":
the direct sun and moon ignored all occlusion, and the rays (4 m and a 3.5 m probe) read a cave
roofed higher than that as open sky. A stopgap (scaling the sun by the ray visibility) was
considered and rejected by the owner in favour of proper lighting in two layers: a flood light now,
shadow maps later (built separately).

## Decision

- A Minecraft-style **flood light**: sky light and block light, 0–15 per cell, propagated by
  removal and addition passes, sky light falling straight down along each face's sky direction.
  What blocks, filters and emits light is block data (`lightFilter`, `emits`); the falloffs, curves,
  the direct-light ramp and the block light's colour are `shared/look/light.json`.
- **The client keeps it** for the chunks it holds and recomputes it as chunks arrive; it is not in
  world files or on the wire (only the two block fields, as a registry-sync look tag). The server
  keeps none: no gameplay reads light yet. A patch release.
- **Meshing averages it per vertex** (smooth lighting) beside the baked corner occlusion and sky
  rays, which are kept: they shade the flood light as much as a vertex is open to the sky, so open
  ground is lit exactly as in 0.22.5 and caves by the flood light alone.
- **The direct sun and moon take the sky light's ramp** as the shadow stand-in: the shadow maps
  (Phase 44, `shadow_light(rel, n, stand_in)`) shade them near the eye and fade back to it past
  their last cascade.

## Consequences

- Tunnels and caves go dark with depth, whatever their size; light crosses chunk borders and
  doesn't depend on load order; predicted edits relight at once.
- Lighting a chunk costs about 2 ms native as it arrives, done within the meshing budget; 32 KB per
  chunk that isn't uniform; edits well under a millisecond.
- Integer levels on ½ m cells: light reaches 15 m across but 7.5 m vertically.
- A chunk lit before the chunks above it arrive is relit when they do (the sky enters at the top of
  what is held).
- Players, your arms and held items take the flood light where they are, blended between cells
  and eased over `characters.easeS` (the owner's request, folded in).
