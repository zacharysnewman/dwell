# 0050. The world owns its look: sky, fog and clouds per ray, not per viewer

- Status: Accepted (the owner's request, 2026-10-27: "Yes, proceed.")
- Date: 2026-10-27
- Supersedes: the viewer's sky, fog and exposure blended across the flip band (DAY_NIGHT.md §5,
  Phase 32), and the FUTURE.md item "Sky, fog and light by the sun's position"
- Design: [`DAY_NIGHT.md`](../DAY_NIGHT.md) §13; bug: [`BUGS.md`](../BUGS.md) 13

## Context

The owner (2026-10-27): "It's still an abrupt transition between the two world sides when you move
between them. Clouds disappear from one side, appear on the other, lighting changes (I think), and a
white fog band switches sides … How do we support it looking correct on both sides? Regardless of
viewing side?" The sky's colours, the fog's colour and density, the exposure and the cloud deck
were all the viewer's face's, swapped across the 4 m flip band, and only the viewer's deck existed.

## Options

1. **Widen the flip-band blend.** Cheap, but a view still depends on the viewer; it only slows the swap.
2. **The world owns the look.** Each ray takes the sky, clouds and fog of the air it passes
   through; only the eye's adaptation follows the viewer. A second deck hit per pixel, the other
   deck's noise only where a ray crosses the midplane.

## Decision

Option 2. Face A's air is above the midplane and face B's below, each with its sky, fog and deck.
A ray below the eye's horizon leaves by the other face's air: the other face's sky and deck, lit by
its own sun and moon, under the eye's face's haze over the path to the midplane. A point across the
midplane fogs by each face's air over its part of the ray. Exposure and saturation ease toward the
eye's face over `eye.adaptS` (data, 1.5 s). The flip band no longer shapes the look.

## Consequences

- A view down a portal or past the rim is the same from either side of the midplane, tested on the
  CPU twin (`sky_behind`) and checked in Chrome.
- The frame block gains two vec4s (the other deck), and the `DaySky` block carries the other face's
  fog density. Rendering only: no world format, protocol or terrain change.
- Not covered yet: water reflections and the ink's fog keep the eye's face's sky; the inside-cloud
  fog is the eye's deck's only.
- To reverse: blend the viewer's samples by the flip band again and drop the other deck.
