# 0046. Distant terrain shadows: horizon maps swept from heightmaps of the terrain as drawn

- Status: Accepted (the owner's lighting plan, layer 3, 2026-10-21)
- Date: 2026-10-21
- Design: [`DAY_NIGHT.md`](../DAY_NIGHT.md) §12; plan: Phase 46

## Context

The lighting plan has three layers: the flood light (layer 1, [ADR 0045](0045-flood-light.md)),
cascaded shadow maps near the eye (layer 2, Phase 44, out to 80 or 160 m) and sun shadows on the
distant LOD terrain (layer 3). Past the cascades the direct light was the flood light's stand-in,
which is 1 on open ground: at dusk a mountain cast nothing across the valley below it. The owner
asked for a ray or a horizon angle per texel over an LOD heightmap toward the sun, blending into the
cascades at their far edge, with no main-thread work or frame spikes (a stutter regression had just
been fixed) and a cost of about 1 ms or less on the M1.

## Decision

- **Heightmaps of the terrain as drawn**, not of the generator: the chunks, LOD sections and
  stand-ins drawn this frame, rendered from above (face-local, through `FaceFrame`) into depth maps
  around the eye, so edits, structures and trees cast. One or two levels per tier (4 m and 16 m
  texels on High). A fill draws a budgeted slice of its casters a frame (`fillKtris`, `fillDraws`)
  into a second layer and replaces the map in use when whole; max mips follow. A level is filled
  again only when the eye has moved an eighth of its half-width or 10 s have passed.
- **A horizon map per level**, swept on the GPU from the heightmaps: each texel's march toward the
  light (geometric steps reading the max mip each step's length needs) keeps the terrain's highest
  elevation tangent that way, a band of rows a frame. Swept again only when its heightmap was
  refilled or the light has turned 0.5° about the face's up; the light's elevation needs no sweep.
- **Fragments read one sample**: past the cascades the stand-in is shaded by the horizon at the
  fragment's column against the light's elevation, softly over a penumbra; over the cascades' last
  `edgeFade` it is blended with the cascades' result by the same weight, so neither edge shows
  (`dwell_look::lod_shadows::combine`, tested). The time of day's fade weakens both alike.
- **Settings**: it follows the Shadows setting's tier (Off, Low, High); a CPU adapter draws none.
  Data: `shared/look/lodshadows.json`.

## Rejected

- **A per-fragment march** over the heightmaps (built first): +20 ms of GPU a frame on the M1 with
  24 steps over every far pixel, against about +0.5–0.9 ms with the sweep.
- **A fifth, far shadow cascade**: depends on the light's direction, so it would be redrawn as the
  sun moves, over kilometres of casters at once; the heightmap depends on the eye alone.
- **Heights from the generator in a worker**: misses edits, structures and trees, and is CPU work.

## Consequences

- Distant shadows lag a moving eye by a fill (a fraction of a second to a few seconds), and a
  turning sun by a sweep; neither is visible at the distances they cover.
- A surface under the top of its column (a cliff's foot, an overhang's underside) takes the
  column top's horizon; distant terrain is mostly tops.
- About 16 MB of GPU memory on High (heightmaps with mips and horizon maps, two layers each).
