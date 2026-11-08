# 0064: Light shafts (volumetric glow rays) as data, one reduced-resolution pass

**Status:** Accepted (2026-11-07; air and underwater shafts built in 0.50.24; torch and lamp shafts built in 0.50.26)

## Context

The owner (2026-11-07): "The glow rays is volumetric lighting which we want but don't have
underwater or outside the water yet either." Their answers to the design questions:

- **Where:** under water (shafts from the surface down through the water), in the air (the sun
  through gaps in trees, clouds, cave openings and hills), and torch and lamp light.
- **Quality:** "Full quality everywhere with a cheaper version as a 'low' setting": a Settings →
  Graphics → Light shafts choice, High / Low / Off, High by default on every device, phones
  included. Low is a lower resolution and fewer samples. The CPU adapter draws none (the tier rule:
  `render/mod.rs` `shafts_for`).
- **Look:** soft and subtle; the strength is data per place (the air, each fluid). The underwater
  references: sun rays fanning down from a bright surface patch into blue water. Overall water goal:
  pretty, runs on mobile, stylized is fine, cheap routes are fine if they are decent and cohesive.

What gets added later: more fluids' shafts (by registry name), retuned strengths and tiers, and the
torch and lamp shafts. Who adds them: a data file (`shared/look/shafts.json`), which an editor page
could write. What stays code: the two shaft models (the radial blur and the underwater march) and the
pass.

## Decision

- **Data:** `shared/look/shafts.json` → `dwell-look` `shafts.gen.rs` by `shared/look/shafts.mjs`
  (validated with the bad entry named, a fixture fluid added by data alone in its test, CI stale
  check). `tiers.high` / `tiers.low` (the buffer's scale per axis and the samples), `air.sun` and
  `air.moon` (strength, reach, decay, disc radius, the elevation where they are full, how far off
  screen they fade out), and per fluid by name (with a `default`): strength, reach and fade depth,
  the surface cells' size and drift, and Snell's window's glow (colour, amount, softened rim).
- **One pass, `gpu.shafts`:** after the scene passes and before the composite, `shafts_fs` draws one
  channel (`r16float`) at the tier's share of the drawing buffer (High ½, Low ¼ per axis), reading
  the scene's colour and view depth through its own group (`frame::scene_read_layout`); the
  composite samples it filtered (`with_shafts`, inside `composite_pixel` so the edge blur's taps see
  it) and screens it over the finished colour × the light's colour × the weight. No pass at all when
  the frame's weight is 0 (Off, the light behind the camera or under the horizon).
  - **In the air:** a radial blur toward the light's screen point (dwell-look `screen_point`) over
    the open sky (the scene's coverage), each sample weighed by a disc round the light and decayed
    along the way. The weight (`air_weight`) fades with the light's height over the face's horizon
    and as it leaves the screen. By night the moon, faintly.
  - **Under a fluid:** the view ray is marched to what it shows, the surface or the reach; at each
    point the surface is looked up along the sun's refracted direction (`sun_through`), lit where
    two drifting noise layers are bright, so the shafts are world-space lines that converge on the
    sun's image and flicker with the surface's cells, fading with the depth and the distance.
- **Snell's window** takes the fluid's window glow (a bright white-cyan for water) over the sky
  through it, brightest at its middle, its rim softened into the mirror, and the sun patch is
  stronger (`underwater.json` `sunPatch`).
- **Both faces:** the CPU side works in world vectors and reads elevation through
  `FaceFrame::vector`; the WGSL uses `face_rise`, `face_y`; the tests run on both faces with a
  parity check.
- **Torch and lamp shafts (built, 0.50.26):** the same pass and buffer, drawn alongside the sun's or
  moon's: the nearest placed and held lights (the placed-light list the torch shadows use, and the
  hands' torches) in front of the camera within `lights.rangeM`, faded out near the eye (a torch in
  your own hand), toward the range's end and off screen, at `dayShare` of the strength by day; the
  tier's count (High 4 at 12 samples, Low 2 at 6). Strength and halo per light source by its name
  (`lights.kinds`, `dwell:torch`, `dwell:fuse`; one not listed takes `lights.default`, none; a
  fixture kind is added by data alone in the generator's test). Each light is a radial blur toward
  its screen point over what doesn't hide it (the view depth's first sample behind the flame, or
  the sky), weighed by its halo (`discM` over the light's depth) and decayed; a pixel whose samples
  can't reach the halo skips the light. The buffer became `rgba16float`: rgb the lights' glow in
  their own colours, a the sun's, moon's or fluid's. Not under a fluid yet.
  - **Deviation from the plan** (a short march per view ray against the torch shadow cube maps, High
    only): the cube maps exist only with Torch shadows on (Off on phones), for at most 2–4 lights,
    and would need their bindings in the shafts' group; the screen-space radial blur is the air
    shafts' model, needs no new bindings, works with Torch shadows Off and on Low (owner: "a cheaper
    version as a low setting"), and costs the tier's samples a light only near it on the screen. What
    it gives up: an occluder off screen or behind the light casts no ray, and the glow is a halo
    with rays past what stands in front of the flame, not true in-scatter through the room.

## Consequences

- One small pass per frame with the shafts on: a full-screen triangle at ¼ (High) or 1/16 (Low) of
  the pixels with 24 or 12 samples.
- A new frame uniform block of four vec4s (`frame.shafts*`) and, for the lights, nine more
  (`frame.torch_shafts`) and a scene group with the shafts'
  buffer and a filtering sampler (bindings 2, 3).
- The radial blur is screen-space: shafts vanish with the light off screen (by design, faded), and a
  thin occluder at the screen's edge can't cast one.
- Reversing it: set the strengths to 0 in the data, or remove the pass and `with_shafts`.
