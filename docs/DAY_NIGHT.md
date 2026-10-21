# Dwell — The Day/Night Cycle

The sky, light and fog follow the time of day: a 30-minute day of 20 minutes daylight and 10 of
night, driven by the server's world time so every player sees the same sky. Phase 32 of
[`IMPLEMENTATION_PLAN.md`](./IMPLEMENTATION_PLAN.md); decision record
[ADR 0035](./adr/0035-day-night-cycle.md); architecture in [`ARCHITECTURE.md`](./ARCHITECTURE.md)
§6.3 (Bifacial world: light and sky) and §6.6 (Day/night cycle).

## 1. Brief and decisions

**Owner, 2026-10-15:** "a day/night cycle and improve the transitions between day and night,
including fog, so it looks good. Make the sun and moon look better too."

Known decisions (owner):

- The day is **30 minutes: 20 of day, 10 of night** (first recorded in
  [`TREE_GROWTH.md`](./TREE_GROWTH.md)). Tree growth counts in-game days, so the day length is **one
  shared data value** both use.
- Fog matters: it must look right at every time of day.

Defaults taken where the owner had not decided (open questions in §8); the owner accepted them as
proposed on 2026-10-16 (§8):

| Question | Default |
| --- | --- |
| Time source | The server's world time (its tick), already saved and already sent; no protocol change |
| Where the data lives | `shared/look/daynight.json` (clock, sun path, sun/moon/star looks, keyframes), generated and validated |
| Keyframes | 14 over the day, keyed by the sun's angle along its path, interpolated in Oklab |
| Sun | A limb-darkened disc, larger near the horizon, with a bloom and a glow sized by data |
| Moon | Opposite the sun (as the bifacial world had it), with phases over an 8-day cycle, maria and earthshine |
| Stars | Procedural, turning with the sky, faded by the sky's brightness, twinkling |
| Face B | Shares the one sky: its time of day is face A's + 12 hours (10 min day, 20 min night) |
| The sky rotation setting | Removed (the time of day is the world's); dev switches replace it |

## 2. Time

World time is the server's **tick** (60 Hz). It is saved in the world file's `meta` as `world_tick`
and resumed on load, so a world's clock continues where it stopped, and every `PhysicsSnapshot`
(20 Hz) carries it as `server_tick`. The client keeps a smooth estimate between snapshots
(`dwell-client` `world_clock.rs`, shared with the clouds' drift: the least-delayed snapshot's
offset, never turning back for a late one) and derives everything from it:

- `days = world_s / day_length + startHour / 24` (`DayNight::days`): the time of day is its
  fraction (0 midnight, 0.5 noon on face A), the day number its whole part. A new world starts at
  `startHour` (07:00).
- Local worlds run the same server code in the browser, so their time is their saved tick too.

**Compatibility.** This needs no protocol change: the tick was already in `Welcome` and every
snapshot, in every build of the 0.11 line. Older 0.11 servers drive the cycle as well. The clock
itself (day length, start hour) is now **public API**: two builds that disagree on it show
different skies on one server, and tree growth (Phase 30) will count days with it. Changing it is a
compatibility change until it becomes a per-world setting sent in `Welcome` (§8).

## 3. The sun's path

The sun moves on a great circle through its rising point (`riseYawDeg`) and its setting point
opposite, culminating `noonElevationDeg` (60°) above face A's horizon. Its position on that circle
is the **solar angle**: 0° rising, 90° noon, 180° setting, 270° lowest. To make daylight last
`dayS` of the day, the angle runs unevenly with time: hour angle `2πu + k·sin 2πu` (u = time from
noon as a share of the day) with `k` chosen so the sun sets `dayS / 2` after noon; it moves slower
by day and faster by night, smoothly (k = −0.60 for 20/10 minutes; the generator rejects a split
whose path would turn back). The **moon is always opposite the sun** (BIFACIAL_WORLD.md §6), so it is
up exactly when the sun is down. The stars are fixed to the sky, which turns about the path's axis
by the solar angle.

**Face B** shares the sky. The sun is over face B exactly when it is under face A, so face B's
solar angle is face A's + 180° and its look is the same keyframes at that angle: when face A has
noon, face B has midnight under the moon. Face B therefore has 10 minutes of daylight and 20 of
night (§8).

## 4. Keyframes

Each keyframe sets, at one solar angle, everything the time of day changes (colours `#rrggbb`):

| Field | What |
| --- | --- |
| `name`, `sun` | A label for tools; the solar angle (degrees, in increasing order) |
| `skyZenith`, `skyHorizon` | The sky's gradient (`colour = mix(horizon, zenith, elevation^gradientPower)`) |
| `sunDisc` | The sun's disc colour |
| `glow`, `glowStrength` | The sun's bloom and glow around it |
| `band`, `bandStrength` | The warm band along the horizon toward the sun's azimuth |
| `sunLight`, `sunIntensity`; `moonLight`, `moonIntensity` | The direct lights (the generator requires 0 for a light below the horizon) |
| `hemiSky`, `hemiGround`, `hemiIntensity` | The hemisphere (ambient) light |
| `fogDensity` | Multiplies the fog's density (`shared/look/fog.json`; denser at dawn and dusk) |
| `exposure` | Multiplies the exposure setting |
| `stars`, `moonGlow` | The stars' visibility; the moon's glow (× its lit share) |
| `saturation` | The lit scene's saturation (moonlight desaturates: 0.5 at midnight) |

The 14 keyframes: sunrise (0°), golden hour (7°), morning (30°), noon (90°, Phase 7's approved
sky and lights), afternoon (150°), golden hour (173°), sunset (180°), civil, nautical and
astronomical dusk (187°, 194°, 201°: the sun 6°, 12° and 18° down), midnight (270°), astronomical,
nautical and civil dawn (339°, 346°, 353°).

**Interpolation.** Any angle is a monotone cubic (Fritsch–Butland, as PCHIP) through the keyframes
around it, over the angle, cyclically: continuous and smooth through every keyframe and across
midnight, never overshooting either neighbour (no flash past a keyframe's value). Colours are
interpolated in **Oklab**, so sunset's orange to dusk's violet doesn't pass through grey mud.

**Fog tracks the horizon by construction.** The fog's colour is the sky's colour in the pixel's
direction (`dwell_sky`, the same function that draws the sky), and below the horizon that is the
horizon's colour in that azimuth. So distant terrain melts into the sky at every time of day: into
the warm band toward a setting sun, into the dark blue horizon at night. There is no separate fog
colour to keep in step. The keyframes vary only the fog's density.

## 5. Drawing

`dwell-look` evaluates the cycle on the CPU each frame (`DayNight::state`: the time of day, the
sun, moon and axis, the moon's phase, and each face's keyframe sample) and packs it into the
`DaySky` uniform block (`daynight::DAY_SKY_WGSL`, 17 vec4: the directions, the viewer's sky colours,
and each face's lights). The renderer's frame carries it as `frame.day`; clouds and water read it.

- **The sky** (`sky.wgsl` `dwell_sky`): the gradient, the horizon band toward the sun, the sun's
  bloom and glow (Gaussians sized in degrees by data; the glow is capped so the disc stays brighter
  than the sky), the moon's cool glow. Used for the sky, the fog and reflections.
- **Bodies** (`dwell_sky_bodies`, behind the world only, under the clouds):
  - the **sun**: a disc of `radiusDeg`, `horizonScale` larger at the horizon, limb-darkened, its
    colour from the keyframes (white at noon, orange at sunset), antialiased to a pixel;
  - the **moon**: a disc lit from the sun's side by its phase (full facing the viewer, new facing
    away), with procedural maria (value-noise fBm) and earthshine on the dark part; its frame is built
    from world vectors, so it keeps its face through the camera's turn-over;
  - **stars**: at most one per cell of a grid over the faces of a cube, fixed to the turning sky,
    sized in pixels, most faint and a few bright, tinted warm to cool, twinkling; faded near the
    horizon, in a bright sky and by the keyframes, hidden behind the moon.
  - All cut smoothly at the viewer's horizon.
- **Light**: each fragment takes its own face's sun, moon and hemisphere light (BIFACIAL_WORLD.md
  §6), each direct light weighted by its height over that face's horizon. The moonlight scales
  with the moon's lit share, down to `newMoonLight` (55%) at new moon, so every night stays
  readable.
- **The composite** applies the keyframe's saturation to the lit scene, fogs it toward the sky, lays
  it over the sky behind and **dithers** the 8-bit output (triangular noise, half a step), so the
  smooth night and twilight gradients don't band.
- **The viewer's sky across the faces**: the viewer's sky colours, fog density and exposure are
  face A's and face B's samples mixed by the eye's height across the 4 m flip band, so the sky
  doesn't jump when the camera turns over at the midplane. The sun, moon and stars are world
  directions, so they stay put while the view turns.

Water's reflection uses `sky()`, which is `dwell_sky` with the frame's `DaySky`, so it follows the
cycle; its glint takes the face's sun and moon light from the block too. The clouds (Phase 31) take their light from the same block (`cloud_lighting`: the viewer's
face's sun, moon and sky light; day or night by the sun's height).

## 6. Dev controls

- `?time=0..1` shows a fixed time of day (0 midnight, 0.5 noon) on this client; `?moon=0..1` a
  moon phase (0 new, 0.5 full); `?timespeed=N` runs this client's clock N times faster. They change
  only this client's view, never the world's time (views and scenarios: `settings.timeOfDay`).
- F3 shows the clock: `time 07:12 (t 0.300) · day 3 · sun 48° (face B 228°) · sky morning → noon
  30% · moon phase 0.62, 85% lit`.
- `__dwell.setTime(t, moon?)`; the bench's `time {t, moon?}` and `timelapse {name, frames, from,
  to, waitS}` steps; scenarios `day-night` (each time of day toward and away from the sun, the moon
  at three phases), `day-timelapse` (48 frames), `day-night-face-b` (face B's sky at face A's noon,
  midnight, sunset and sunrise) and `day-night-cost` (GPU time at noon and at midnight, clouds off,
  `--unthrottled`).

**Measured** (Apple M-series GPU, Chrome, 1280×720, `day-night-cost`, clouds off, p50
`gpu.composite`): the horizon at midnight 0.76 ms, looking up at midnight (stars everywhere) 0.71 ms,
looking up at noon (no stars) 0.58 ms: the stars cost about 0.1 ms over a full sky; the sun and
moon only touch the pixels near them. Readings varied up to ×3 between runs (passes overlap on a
tile-based GPU, and thermal state), so treat them as an order of magnitude.

## 7. Extending it

Everything the cycle shows is in `shared/look/daynight.json`; `node shared/look/daynight.mjs`
validates it (an unknown, missing or out-of-range entry, keyframes out of order or a light lit below
the horizon fail with the entry named) and writes `crates/dwell-look/src/daynight.gen.rs`; CI fails
if it is stale. A keyframe added, removed or changed is data alone (tests in
`shared/look/daynight.test.mjs` and `dwell-look`). A keyframe editor page could edit the file
directly: each keyframe is one flat object, and the solar angle maps to face A's clock as
`elevation = asin(sin(sun) · sin(noonElevation))`.

## 8. Open questions for the owner

**Decided (owner, 2026-10-16), as proposed:** face B shares the sky with face A's time + 12 hours
(10 minutes of day, 20 of night); the moon stays opposite the sun; day length per world is fine
as a later addition (not built); the keyframe colours, the sun's size and the star density stand;
the time controls stay as proposed (the dev switches of §6, no player controls yet). Seasons (6)
remain open. The questions as they were put:

1. **Face B's day.** With one shared sky, face B gets the complement of face A's day: 10 minutes
   of daylight and 20 of night. Is that right, or should face B have its own sun?
2. **The moon opposite the sun** keeps every night moonlit (the bifacial design), so phases are
   shading only: a new moon still rises at sunset (dim, `newMoonLight`). Should the moon move
   independently (moonless nights, a daytime moon)?
3. **Day length per world.** The clock is build data now; making it per world (as tree growth's
   speed will be) means sending it in `Welcome` (a protocol change behind a capability flag).
4. **The look.** The keyframe colours, the night's darkness (0.5 saturation, moonlit blue), the
   sun's size (1.5°, ×1.4 at the horizon) and the star density are first passes for review.
5. **Time controls for players**: sleeping through the night, `/time` for hosts, or a "frozen
   time" world option?
6. **Seasons** (the noon elevation and day length drifting over a year) — not built.

## 9. Ambient light: corner occlusion and sky visibility

**[built, Phase 43]** Owner (2026-10-21): with every face lit by the same hemisphere light, nights
came out nearly black (black slabs) and caves and overhangs were as bright as open ground. Now the
chunk mesher bakes an **ambient light** into each terrain vertex (dwell-mesh `sky.rs`; the data is
`shared/look/ambient.json`, validated and generated by `shared/look/ambient.mjs` into
`dwell-look` `ambient.gen.rs`), and the shader scales the hemisphere light by it:
`ambient = hemisphere × (floor + (1 − floor) × ao × visibility)`. The sun and moon are not
touched by it: their shadows are §10's, and the flood light (§11) shades them and the ambient
(a carved tunnel was lit like day without it).

- **Corner occlusion (`ao.strength`, 0.6)**: the classic voxel AO. Each corner of a cell face
  checks the three cells around it in the layer the face looks into (side, side, corner): level 3
  open, 0 when both sides are solid; each level below open keeps `1 − strength/3` less. Cells are
  ½ m tall, so a side face's vertical neighbours are half-height cells (its gradient spans ½ m).
  A quad is split along the diagonal whose corners are the brighter pair, so a lone dark corner
  shades the same whichever way the face points. Faces merge only where their corners' light keys
  match, so a merged quad's corners carry its cells' light exactly.
- **Sky visibility (`sky.rays` 16, `lengthM` 4, `probeM` 3.5)**: the share of rays that escape.
  The rays are a cosine-weighted set about the face's normal (Fibonacci points lifted to the
  hemisphere), turned about it by one of 16 turns picked by a hash of the vertex's face-local
  world position (deterministic: every client and both chunks of a border vertex agree), and
  turned toward the face's sky (a ray below the face-local horizon is mirrored above it, so a wall
  of open ground sees the whole sky and a ceiling none). A ray escapes if it runs `lengthM`
  through the cells (a DDA over 1 m × ½ m × 1 m cells; for cell faces the cells are walked once per
  direction and turn into offset tables) without entering a light-blocking cell, and nothing is
  within `probeM` straight above its end (a cave roof or an overhang beyond the ray's reach).
- **What blocks light**: full cubes that are not translucent (leaves do: a pine's canopy darkens
  the ground under it). Glass, fluids, plants, ladders and the ramps don't. **Ramps** (shaped
  faces) take the sky visibility only (no corner occlusion); **plants** take their ground's (the
  top vertex at the cell's corner); **water** keeps its own lighting (no baked light).
- **Both faces**: the occupancy is built face-local (a face-B chunk is meshed mirrored), so face B
  is face A's mirror image, light included (the mirror parity test compares it).
- **The apron and the cutoff**: mesh jobs carry the chunk with a sky apron of 6 cells across and
  16 rows up and down (`dwell_world::SKY_APRON`; 44 × 64 × 44 ids), inside the 26 neighbours. The
  reach (`lengthM` across, `lengthM + probeM` up) must fit it, which dwell-look checks at build
  time; anything past it would count as open. So a cave roof more than about 7.5 m above its
  floor (4 m of ray and 3.5 m of probe) lets some sky in. An edit within the apron of a border
  re-meshes the chunk across it (chunk_stream.rs `write`: every voxel in the upper or lower half of
  a chunk is in a vertical neighbour's reach).
- **LOD sections are lit as open sky** (light 1): at their distance only open ground shows, which
  near chunks also light at 1, so the seam matches there; creases, overhangs and caves are darker
  in the near chunks than in the LOD past them.
- **Night readability**: the twilight and night keyframes' hemisphere light was raised with it
  (civil 1.3 → 1.5, nautical 0.8 → 1.65, astronomical 0.85 → 1.8, midnight 0.75 → 1.9; midnight's
  `hemiSky` #3a508e → #3e5594, `hemiGround` #1c2438 → #34406a), so open ground under the moon reads
  and caves and overhangs fall dark; `floor` (0.08) is what a fully enclosed vertex keeps.

**Cost** (Apple M1, `cargo run --release -p dwell-mesh --example mesh_cost`, seeds 7, 1, 42, both
faces, 1085 chunks, two runs each): 2.6 ms per chunk before, 5.2 ms with the ambient light (about
1 ms of it the corner occlusion and the occupancy, the rest the rays; 8, 12, 16 and 24 rays cost
within the machine's noise of each other once the ray cells are tabled, so 16); +17% vertices,
+18% triangles, +25% mesh bytes (corner keys break some merges; one more float per vertex). In
Chrome (`load` from page start to a complete view, `--unthrottled`): settled in 11.0–11.5 s both
before and after; `job.chunk_mesh.run` p50 23 ms → 28–30 ms; the steady frame's CPU time unchanged
and `gpu.frame` within noise (13.3–13.6 ms before, 13.7–14.7 ms after).

**Extending it**: every value is in `shared/look/ambient.json` (`node shared/look/ambient.mjs`; CI
fails on a stale `ambient.gen.rs`; `shared/look/ambient.test.mjs` changes values by data alone).
What blocks light is the block data (`render`, the shape), not a list. The apron is a code constant
(it sizes the jobs).

## 10. Sun and moon shadows: cascaded shadow maps

**[built, Phase 44]** Owner (2026-10-21): real sun and moon shadows near the player, the second
layer of the lighting plan. The first layer (Minecraft-style flood-fill sky and block light, built
alongside, §11) scales the direct light by the sky light as a stand-in; the shadow
maps replace that stand-in near the eye. Code: `dwell-client` `render/shadows.rs` (the fit, the
maps, `SHADOW_WGSL`), `dwell-look` `shadows.rs` (the setting, the light that casts); data:
`shared/look/shadows.json` (validated and generated by `shared/look/shadows.mjs` into
`shadows.gen.rs`; `shadows.test.mjs` changes it by data alone; CI fails on stale output).

- **The light**: the sun while it is over the viewer's face's horizon, else the moon; none when
  neither is up (`caster`). Its strength fades in with its elevation (`lights.sun` 0° → 8°,
  `lights.moon` 2° → 12°), so the sun's shadows fade out at dusk and the moon's fade in. Face B
  sees the light over it: the same world directions, the other hemisphere (both faces are tested).
- **Cascades** (`tiers`: Low 2 of 1024² out to 80 m, High 4 of 2048² out to 160 m): the view's
  depth split by the practical split (`splitLambda` between even and logarithmic, the first slice
  from the eye), each cascade an orthographic map fitted to its slice's **bounding sphere**. The
  sphere depends on the slice's shape only, so it doesn't change as the camera turns, and its
  radius is rounded up to `radiusStepM` so the speed boost's changing FOV changes it in steps. The
  light's map axes follow the light alone (its y as near the sky's axis as it can be), and each
  cascade's centre is **snapped to whole texels** across the light (in world f64): moving the eye
  moves the map by whole texels, so shadow edges don't shimmer (a test moves the eye in tenth-texel
  steps and checks a fixed world point keeps its place in the texel grid). The box reaches
  `casterReachM` (120 m) toward the light for casters above the view (cliffs, trees). While the
  spyglass zooms the shadow distance grows with its magnification up to `spyglassMaxM` (640 m), so
  the zoomed view has shadows where it looks.
- **Casters**: chunk terrain and LOD sections (depth only), cutout plants (their texels' alpha; faded
  by the eye's distance as they fade on screen, so a plant's shadow goes with it), pines and leaves
  (terrain), other players' bodies and **your own** (drawn into the maps only, so you have a shadow
  in first person). A draw in an LOD cross-fade keeps the map texels its dither keeps, so the two
  levels share the map as they share the screen; a stand-in keeps its clip box. Water casts none.
  Both sides of every face cast; the casters' pipelines take a slope-scaled depth bias
  (`bias.slopeScale`).
- **Receiving**: `scene_out` (terrain, plants, LOD, every opaque scene surface), the characters'
  key light (a body in shade shows its ramp's middle at most) and the water's glint, sparkle,
  in-scatter and foam. A fragment takes the first cascade that holds it, offsets its lookup along
  its normal by `normalOffsetTexels` of that cascade's texels, subtracts `depthBiasM`, and filters
  with a soft PCF: `filterTaps`² hardware 2×2 compares over `filterTexels`, a painterly soft edge.
- **Composition**: the shadow multiplies **only the direct sun and moon term**
  (`lambert_lit`'s `shadow`; a shader-source test pins it); the hemisphere light stays the ambient
  and AO's (§9) and the flood light's. Over the last `edgeFade` (20 %) of the shadow distance, and
  as the time of day fades the light's shadows, the factor fades to the stand-in (direct × sky
  light from the first layer; 1 until it lands), so there is no edge. `cloud_shadow` is the hook
  for clouds' shadows on the terrain, a future item (CLOUDS.md §6, Phase 31's C7); it returns 1.
- **The Shadows setting**: Off / Low / High in Settings → Look, Low on phones and High on desktops
  by default, Low at most on a CPU adapter (`shadows_for`); kept in the browser's append-only
  `dwell.shadows` record (`ui/shadowSettings.ts`). `?shadows=off|low|high` overrides it for the
  bench's configurations (`shadows-low`, `shadows-off`), and the bench has a `shadows` step.
- **Interactions**: the edge blur and the ink outline read the finished image and the view depth,
  which the shadows don't change; the arms' pass takes the shadow at the arms (your body's shadow
  shades them with the sun behind you); the spyglass as above; the water's bed is terrain, shaded
  before the water reads it.

**Cost**: see the table below (Apple M1, Chrome, `npm run bench -- … --compare --configs
shadows-low,shadows-off --unthrottled`, the baseline High).

`gpu.frame` medians (ms; the lowest frame in brackets) for High, Low and Off, one run each, 1280×720.
**The machine was busy** (other agents' builds and browsers sharing the CPU and GPU; load average
10–17), so the medians are noisy and the lowest frames are the steadier guide.

| Scenario | High | Low | Off | High − Off | Low − Off |
| --- | --- | --- | --- | --- | --- |
| idle-spawn | 17.5 (12.1) | 15.3 (10.1) | 12.7 (8.9) | +4.8 (+3.2) | +2.6 (+1.1) |
| shadows-pines (grove) | 20.4 (14.1) | 16.0 (10.2) | 14.4 (10.1) | +6.0 (+4.0) | +1.6 (+0.1) |
| fly-low-fast (flight) | 12.0 (6.6) | 11.3 (6.8) | 10.4 (5.6) | +1.6 (+1.0) | +0.9 (+1.1) |
| fly-high (flight) | 10.1 (5.5) | 10.7 (5.1) | 11.0 (6.4) | ≈ 0 | ≈ 0 |

Triangles drawn into the maps (`shadow.ktris`): idle 331k on High, 487k on Low (Low's second
cascade covers the whole 80 m slice and is drawn every other frame); grove 186k / 317k; flying
low 35k / 54k; flying high 20k. The targets (High ≤ 3 ms, Low ≤ 1 ms on the M1) are met by Low on
the lowest frames, not yet by High at the spawn and in the grove: the casters' triangles dominate
(the chunk meshes are the scene's own, AO-split and fine), and the plants' alpha-tested blades cost
most per texel. Already in: casters culled to each slice's light-space footprint, opaque casters
drawing only their light-facing faces, the far cascades taking turns (`refreshFrames`), plants in
the nearest cascades only (`plantCascades`), a 120 m caster reach. Next steps for High, a
follow-up: coarser caster meshes for the far cascades, a lower far resolution, or three cascades;
measured again on an idle machine.

**Extending it**: the tiers, the bias, the fades and the distances are data; a tier is a new entry
of `tiers` with the setting's name added to `ShadowQuality` and the menu's choices (the shader
takes up to four cascades). The vocabulary in code is small: the split, the sphere fit, the texel
snap, the PCF grid.

## 11. Flood light: sky and block light per cell

**[built, Phase 45]** ([ADR 0045](./adr/0045-flood-light.md)) Owner (2026-10-21): "I carved a tunnel
in 0.22.5 and it was lit up like day." Measured on 0.22.5: a 2 m tunnel's baked sky visibility was
already 0 two metres in, so the ambient was dark, but the **direct sun and moon** were untouched and
lit its floor and sun-facing wall at full strength 15 m in; and a cave roofed more than about 7.5 m
over its floor baked as open sky (the rays' reach). The owner chose proper lighting over a stopgap:
layer 1 is a Minecraft-style **flood light**; layer 2, shadow maps, is separate and replaces the
direct-light stand-in near the player.

- **Two channels per cell** (0–15 each, a byte; `dwell_world::light`), kept by the client for the
  chunks it holds: recomputed as chunks arrive, never saved or sent (the server keeps none; no
  gameplay reads it yet).
- **Sky light** enters at full strength where a cell's face-local above is in a chunk not held (the
  sky enters at the top of what is held), falls straight down without loss through cells that
  filter nothing — face-local, so on face B it falls toward the midplane — and spreads by flood fill,
  losing `falloff` levels per cell, or the cell's filter if that is more. **Block light** spreads the
  same way from blocks that emit it.
- **Falloff: 1 level per cell, across and vertically** (`falloff`). Cells are 1 m across and ½ m
  tall, so light reaches 15 m across and 7.5 m up or down; a ½-level vertical step (−1 per metre)
  doesn't fit integer levels. Sky light falling straight down keeps full strength, so the vertical
  falloff only matters beside overhangs and in shafts.
- **What blocks and filters it is block data** (`lightFilter`, default 15 for full cubes that are not
  see-through, 0 otherwise; leaves 2, water 1 — the sea floor goes dark about 7 m down) and
  **what emits** (`emits`; no build block yet, the fixture `test:lamp` gives off 14).
- **Updates**: removal then addition passes (brightest first) on every edit, predicted edits
  included, and on chunk loads and unloads, so the light is the same whatever order chunks came in
  (tests compare it with a relight from scratch). The streamer lights arriving chunks nearest the
  player first within a **light budget of its own** (4 ms a frame, or a tenth of a slow frame's
  interval up to 25 ms; apart from the 4 ms mesh budget, so lighting doesn't starve the meshing),
  meshes no chunk while it or a neighbour waits, and re-meshes every chunk whose light changed (a
  roof darkens the column under it across chunks). **Unloads are one update per batch**
  (`LightMap::unload_many`): the removal starts from the dropped chunks' own border levels, and a
  cell next to them lit as brightly some other way (its column's sky, or the open sky now entering
  where they were) is kept, so nothing below or beside it is relit. In 0.22.9 each dropped chunk
  cleared and refilled every open-air column beside and below it, one chunk at a time: 1–2 s long
  tasks when a fast flight dropped a batch (fixed in the release after 0.22.10; a test bounds the
  cells an unload writes).
- **Meshing**: each vertex of a cell face averages the sky and block light of the cells it touches
  in the layer the face looks into (smooth lighting; light-blocking cells that hold none are left
  out); shaped faces take the brightest of their cell and the cells they face; plants their cell.
  The corner occlusion and the sky rays (§9) are **kept**: they shade the flood light as much as a
  vertex is open to the sky.
- **Shading** (`terrain_light`; `dwell_look::ambient::shading` mirrors it for tests):
  - ambient = hemisphere × (floor + (1 − floor) × sky × (baked + (1 − baked) × (1 − sky))), `sky`
    the sky level's brightness on `sky.curve` (4: x / (4 − 3x)), `baked` §9's light. Under the full
    sky this is §9 exactly (the 0.22.5 look on open ground, checked against 0.22.5 shots); deep in a
    cave the flood light alone.
  - direct sun and moon × the sky level's ramp (`sky.direct`: none at level 8, all at 15) — the
    shadow maps' stand-in: they shade the direct light near the eye and fade to it past their last cascade (`shadow_light(rel, n, stand_in)`, §10).
  - block light: a warm glow (`block.color`, `strength`, its own curve).
- **Distant terrain**: LOD sections are lit as open sky, as before, so the seam on open ground
  matches.
- **Characters** (owner, 2026-10-21: "your hand isn't affected... perhaps a blend"): your arms and
  held block, other players' bodies and the items they hold take the flood light where they are —
  your eye, a body's middle a metre over its feet — blended trilinearly between the cell centres
  around it (`LightMap::sample`; cells holding no light, the solid beside it, left out) and eased
  over time per character (`EasedLight`, time constant `characters.easeS`, 0.25 s), so walking into
  a tunnel fades your hand down instead of popping. They are shaded as terrain is (ambient ×
  the sky light, direct × the shadow maps or past them the sky light's ramp, plus the glow;
  `character_light` in `characters.rs`).

**What a carved tunnel looks like now** (both faces): at its mouth the floor takes most of the sun;
5 m in the sun is gone and the ambient about a third; 10 m in about the floor (0.16 against 0.08);
15 m in the floor. A tall cave and a tunnel under a hill are dark whatever their size.

**Cost** (Apple M1, `cargo run --release -p dwell-mesh --example light_cost`, seeds 7, 1, 42, about
790 chunks per face, measured while other agents loaded the machine, load average 9–46: upper
bounds): lighting a chunk as it arrives 1.9–2.5 ms; meshing +3–7% with the flood stream (+2 floats
per vertex); an edit 0.02–0.03 ms (a trench), 0.3–0.4 ms (a 16 m roof placed or taken away); 18 MB
of light for 790 chunks (568 need an array, 32 KB each; the rest are uniform). In Chrome: one long
task at load, as 0.22.5; the frame's GPU time unchanged (one more vertex pair, a few ALU ops).

**Extending it**: the falloffs, curves, ramp and block light are `shared/look/light.json`
(`node shared/look/light.mjs`; CI fails on a stale `light.gen.rs`; `light.test.mjs` changes values
by data alone); per-block filter and emission are block data (`shared/blocks`). The level range
(0–15) and the rules are code.
