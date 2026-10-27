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
  doesn't jump when the player crosses the midplane. The sun, moon and stars are world
  directions, so they stay put while the view turns.
- **The sky below the viewer's horizon** **[built, 0.28.5]**: behind the world (nothing drawn
  there), a ray below the viewer's horizon can only be seeing the other face's sky (down a portal's
  shaft, or past the rim), so the composite draws that face's sky and its sun, moon and stars
  (`dwell_sky_behind`: the frame's `DaySky` carries the other face's colours, `other_*`, the
  viewer's blend mirrored, so they meet across the flip band; a pixel's width of blend at the
  horizon). The fog still fades to the viewer's horizon colour (`sky()`), and so do the
  reflections. Before, it was the viewer's horizon colour: from face A at noon, face B's night
  through a portal showed as a white patch (docs/BUGS.md 10). The broader goal, sky, fog and light
  by the sun's height over each point's face, is in FUTURE.md.

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
  top vertex at the cell's corner); **water** takes no baked light (its shader lights it; it takes
  the flood light, §11).
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
- **Cascades** (`tiers`: Low 2 of 1024² out to 80 m, High 3 of 2048² out to 160 m): the shadow
  distance split by the practical split (`splitLambda` between even and logarithmic, the first
  cascade from the eye), each cascade an orthographic map of a **box around the eye** that holds
  everything within its distance (2026-10-21, the cost pass: before, each was fitted to the bounding
  sphere of its slice of the view, which turned with the camera). The box depends on the eye and
  the light alone, never on where the camera looks, as Minecraft shaderpacks (BSL, Complementary,
  Sildur's) centre their one shadow map on the player; its half-size is rounded up to `radiusStepM`.
  The light's map axes follow the light alone (its y as near the sky's axis as it can be), and each
  box's centre is **snapped to whole texels** across the light (in world f64): moving the eye
  moves the map by whole texels, so shadow edges don't shimmer (a test moves the eye in tenth-texel
  steps and checks a fixed world point keeps its place in the texel grid). The box reaches
  `casterReachM` (120 m) toward the light for casters above the view (cliffs, trees). While the
  spyglass zooms the shadow distance grows with its magnification up to `spyglassMaxM` (640 m), so
  the zoomed view has shadows where it looks. Shadows fade to the stand-in over the last
  `edgeFade` of the distance from the eye (a sphere, whichever way the camera looks).
- **The light in steps** (2026-10-21, the shadow flicker fix): the maps see the light in steps of
  `lightStepDeg` (0.05°) along its path (`shadows::stepped`: turned about the sky's axis to the
  nearest whole step), and the texel grid runs through an anchor that holds while the light holds
  its step (`Cache::anchor`). The clock turns the sun about 0.15° a second by day (0.3° by night),
  a little every frame, and texel snapping holds the grid still only while the light does: before,
  the nearest cascade, drawn every frame, turned its grid with the light every frame, and with the
  grid anchored at the world's origin the turn slid it under the eye by kilometres × the turn (a
  new share of a texel every frame), so every shadow edge's stair-steps crawled, the owner's
  "constant moving noise" at Low and High. Now the nearest cascade's matrix is the same from frame
  to frame between steps. At a step the anchor moves to where the nearest cascade was drawn (a
  point of the old grid at the eye), so the grid turns about the eye: the shadows under you move a
  twentieth of a texel, those farther off by their distance × the step. Tests: 600 still frames
  with the clock running change the nearest cascade only at the steps (599 times before), a step
  moves the grid under the feet under 0.1 texel (0.6–0.9 anchored at the origin), both faces.
  Measured with the bench's `burst` step (`shadows-still`: 16 frames back to back from a still
  camera, the clock running, the share of pixels whose brightness moves more than 3 of 255 from one
  frame to the next, the HUD hidden; Off's is the plants' sway and the sky): looking down at the
  ground under you, Low went from 1.27 % a frame to 0.20 % (Off 0.13 %), with a 1 % step about
  every third frame; High from 0.24 % to 0.18 %; across the grove, Low from 0.9 % to 0.16 %. A
  far cascade redrawn after `lightTurnDeg` still changes 5–9 % of the pixels in one frame every
  1–2 s, as in 0.22.22–0.22.24 (0.22.21 redrew every cascade every frame: 2–5 % every frame).
  Redrawing the far ones at every step instead was tried and is worse (2–4 % every few frames:
  each redraw slides their coarser grid under the middle distance). The cost is unchanged
  (`gpu.shadows` p50 within 0.1 ms on idle-spawn and shadows-pines).
- **Kept maps** (`shadows::Cache`, the cost pass): the nearest cascade is drawn every frame (your
  own body and other players move in it). A far cascade's box is `slack` (10 %) wider than its
  distance, and its map is kept, read where it was drawn (rebased to the eye each frame), until it
  must be drawn again: the eye has left the slack, the light has turned `lightTurnDeg` (0.25°; the
  sun turns about 0.15° a second), the casters in its box changed (an order-free signature of their
  mesh identities, skirts and whether they fade: a chunk meshed, replaced or dropped, an LOD swap
  begun or ended), or other players are about (once every `refreshFrames`). At most `farPerFrame`
  (1) far cascades are drawn in a frame, the most urgent first and then the longest kept, so a
  frame never draws them all, whatever changed (no spikes); one not drawn yet for this light holds
  nothing (`NO_CASCADE`), and its fragments take the next cascade or the far shadows. Standing or
  walking, a frame draws only the nearest cascade. Tests pin the schedule (one a frame, kept until
  something changes, the most urgent first, moving casters every `refreshFrames`), the slack and
  the culling (both faces).
- **Casters**: chunk terrain and LOD sections (depth only), cutout plants (their texels' alpha; faded
  by the eye's distance as they fade on screen, so a plant's shadow goes with it), pines and leaves
  (terrain), other players' bodies and **your own** (drawn into the nearest cascade's map only, so you
  have a shadow in first person and no kept far map holds where you were). A draw in an LOD cross-fade keeps the map texels its dither keeps, so the two
  levels share the map as they share the screen; a stand-in keeps its clip box. Water casts none.
  Opaque casters draw their faces toward the light, those drawn in full through a depth-only
  vertex stage that reads the position alone (`shadow_terrain_vs`, `shadow_flat_vs`); plants cast
  from both sides; a caster is drawn into a cascade when its box overlaps the cascade's box. The
  casters' pipelines take a slope-scaled depth bias (`bias.slopeScale`).
- **Receiving**: `scene_out` (terrain, plants, LOD, every opaque scene surface; a face turned
  away from both lights skips the lookup, `lit_by_direct`, since nothing multiplies it), the characters'
  key light (a body in shade shows its ramp's middle at most) and the water's glint, sparkle,
  in-scatter and foam. A fragment takes the first cascade that holds it, offsets its lookup along
  its normal by `normalOffsetTexels` of that cascade's texels, subtracts `depthBiasM`, and filters
  with a soft PCF: `filterTaps`² hardware 2×2 compares over `filterTexels`, a painterly soft edge.
- **Composition**: the shadow multiplies **only the direct sun and moon term**
  (`lambert_lit`'s `shadow`; a shader-source test pins it); the hemisphere light stays the ambient
  and AO's (§9) and the flood light's. Over the last `edgeFade` (20 %) of the shadow distance, and
  as the time of day fades the light's shadows, the factor fades to the stand-in (direct × sky
  light from the first layer) shaded by the distant terrain's shadows (§12), so there is no edge. `cloud_shadow` is the hook
  for clouds' shadows on the terrain, a future item (CLOUDS.md §6, Phase 31's C7); it returns 1.
- **The Shadows setting** (labelled *Sun & moon shadows* since Torch shadows joined it, LIGHTS.md §4): Off / Low / High in Settings → Look, Low on phones and High on desktops
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

**After the cost pass** (2026-10-21: eye-centred boxes, kept far maps, a depth-only vertex stage,
no lookup on faces turned from the light, High at 3 cascades and a 2×2 PCF). `gpu.frame` p50 (ms)
on the same machine and settings, one run each, load average 4–8 (the before row above was taken
the same morning at a load average of 5–18):

| Scenario | High | Low | Off | High − Off | Low − Off | `gpu.shadows` p50 High (before) |
| --- | --- | --- | --- | --- | --- | --- |
| idle-spawn | 8.4 | 8.2 | 8.3 | ≈ 0 (was +5.3) | ≈ 0 (was +1.7) | 0.52 (3.83) |
| shadows-pines | 11.2 | 10.9 | 10.0 | +1.2 (was +3.0) | +0.9 (was +1.3) | 0.37 (1.03) |
| climb-fast | 4.8 | 4.6 | 4.5 | +0.3 (was +0.8) | +0.2 (was +0.4) | 0.36 (0.45) |

Standing, a frame draws the nearest cascade alone (`shadow.drawn` 1). Climbing at 176 m/s it draws
at most one far cascade more; the frame interval's max is the same as Off's (32.9 against 32.6 ms),
so no spikes. High meets its target (≤ 1.5 ms over Off on the M1) and stays the desktop default.
What is left on High is mostly the receivers' lookup in the near pass (about +1 ms in the grove).
The look is unchanged in the bench's shots: near shadows, the cascades' far edge and the dusk
mountains, on both faces.

Minecraft mods' patterns, taken and not taken:
- *Taken:* the shaderpacks' player-centred shadow box (BSL, Complementary, Sildur's on
  Iris/OptiFine). Here as cascades, so the map can be kept while the camera turns.
- *Taken:* the shadow pass draws opaque geometry depth-only, with plants only near and entities
  only where they matter (Sodium/Iris).
- *Taken:* Distant Horizons leaves far shadows to cheaper means: the cascades end at 160 m on the
  horizon maps (§12).
- *Taken:* caching and staggering, and texel snapping to keep cached maps steady (engine
  practice, not the mods').
- *Not taken:* the shaderpacks' single map with a fisheye distortion. Our greedy-merged quads are
  long, and a nonlinear warp of their vertices bends their shadows. The warp also moves with the
  player, so the texel grid can't be snapped and edges shimmer as you move.
- *Not taken:* the camera frustum extruded toward the sun (Iris's shadow culling). It would tie a
  far map to the view direction, and turning would draw it again; the box test around the eye
  culls almost as much once the map is kept.

**Extending it**: the tiers, the bias, the fades, the distances, the light's step and the kept
maps' slack, turn and budget are data; a tier is a new entry
of `tiers` with the setting's name added to `ShadowQuality` and the menu's choices (the shader
takes up to four cascades). The vocabulary in code is small: the split, the box around the eye,
the light's steps and the grid's anchor, the texel snap, the schedule's reasons, the PCF grid.

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
  **what emits** (`emits`; the torch gives off 14, LIGHTS.md; the fixture `test:lamp` 14).
- **Updates**: removal then addition passes (brightest first) on every edit, predicted edits
  included, and on chunk loads and unloads, so the light is the same whatever order chunks came in
  (tests compare it with a relight from scratch). The streamer lights arriving chunks nearest the
  player first within a **light budget of its own**, apart from the mesh budget so lighting
  doesn't starve the meshing (each 4 ms a frame, or a tenth of a slow frame's interval up to
  25 ms, so a slow page still lights and meshes several chunks a frame),
  meshes no chunk while it or a neighbour waits, and re-meshes every chunk whose light changed (a
  roof darkens the column under it across chunks). **Unloads are one update per batch**
  (`LightMap::unload_many`): the removal starts from the dropped chunks' own border levels, and a
  cell next to them lit as brightly some other way (its column's sky, or the open sky now entering
  where they were) is kept, so nothing below or beside it is relit. In 0.22.9 each dropped chunk
  cleared and refilled every open-air column beside and below it, one chunk at a time: 1–2 s long
  tasks when a fast flight dropped a batch (fixed in 0.22.12; a test bounds the cells an unload
  writes). Only the cells that may change are seeded (one whose light could have come from there,
  or one under a dropped chunk that may now take more sky). **A chunk held over open air** (the
  chunks above arrive after the ones below when flying up) is lit first; a cell under it keeps
  its full sky light when the cell of the chunk over it is at full sky light (full sky light
  only falls from the top, so it can't have come back up from below), and only the columns it
  shades are cleared and relit (0.22.12 cleared them all: 5–9 ms native a chunk, 100–150
  thousand cell writes; a test bounds it). A randomized test of load and unload batches checks the
  shortcuts against a relight from scratch.
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
- **Water and see-through faces** (Phase 45 follow-up; owner, 2026-10-21: "support darkness even with
  directional shadows off"): the transparent mesh carries the flood light too (each face the cells
  it looks into, the corners averaged as terrain's; corner keys of the flood light alone, so merged
  water quads break only where it changes), and the water shader lights by it: the body and the
  foam by `terrain_light(1, flood)`, the glint and sparkle by its direct share (shaded by the
  shadow maps near the eye, as terrain's), the sky's reflection by the sky light's brightness.
  Before, every fluid surface was lit as open sky, so with Shadows Off the sea under a 31 m roof
  drew at 33 % of white, the same as the open sea, sun glint included (High 16 %: the maps took
  the glint, not the sky's reflection); now 2 % (median 0.6 %, Off and High alike). Underwater fog
  is still lit as open sky (W4).
- **Distant terrain**: LOD sections are lit as open sky, as before, so the seam on open ground
  matches; LOD water too (open sea).
- **Torches** (Phase 47, [`LIGHTS.md`](./LIGHTS.md), ADR 0047): a placed torch (`dwell:torch`, `emits`
  14) lights through this flood light; a held torch is a dynamic point light drawn per fragment and
  never enters it (LIGHTS.md §3); Torch shadows, a setting of its own beside the Shadows (sun and moon) setting, give the nearest torches cube shadow maps (LIGHTS.md §4).
- **Characters** (owner, 2026-10-21: "your hand isn't affected... perhaps a blend"): your arms and
  held block, other players' bodies and the items they hold take the flood light where they are —
  your eye, a body's middle a metre over its feet — blended trilinearly between the cell centres
  around it (`LightMap::sample`; cells holding no light, the solid beside it, left out) and eased
  over time per character (`EasedLight`, time constant `characters.easeS`, 0.25 s), so walking into
  a tunnel fades your hand down instead of popping. They are shaded as terrain is (ambient ×
  the sky light, direct × the shadow maps or past them the sky light's ramp, plus the glow;
  `character_light` in `characters.rs`).

**Compared with Minecraft.** Vanilla Minecraft has no directional shadows at all: caves and
overhangs are dark from the flood light alone. Each block holds a **sky light** (0–15: 15 under
open sky, falling straight down without loss, −1 per block sideways or through a filter — water and
leaves take more) and a **block light** (0–15, spread the same way from torches and lava). The
renderer never relights the world at night: both levels index a 16 × 16 **lightmap texture**, a
brightness table rebuilt each frame from a gamma curve with a minimum ambient (the darkest level is
never black, raised by the Brightness setting), whose sky axis is dimmed by the time of day (the
"sky darken" subtracted from the sky light) and tinted toward blue at night, the block axis warm
and flickering. **Smooth lighting** averages each vertex's light over the four cells around it in
the layer the face looks into and adds corner AO. Ours is the same model: the same 0–15 levels,
straight-down sky light and falloff (cells ½ m tall: 15 m across, 7.5 m up or down), filters and
emitters as block data, smooth per-vertex averaging and corner AO. Where it differs: the brightness
of a level is `sky.curve` and `block.curve` on a hemisphere light and a warm glow (no lightmap
texture; `ambient.json`'s `floor`, 0.08 of the hemisphere light, is the minimum ambient); night is the day/night keyframes' hemisphere and moon light, which the sky level scales
(the same effect as the sky darken: no relight); the baked sky rays (§9) shade the open part; and
the sun and moon are a direct light, ramped in by the sky level (`sky.direct`), the stand-in for
shadows that the shadow maps (§10, §12) sharpen near the eye. So darkness needs no shadow maps:
with Shadows Off a cave, a sealed room, an overhang and a canopy are as dark as with them (the
table below), and the maps only add directional shadows on top.

Measured with Shadows Off, High and Low (0.22.21, Apple M1, Chrome, 1280×720; the mean linear
luminance of the view as % of white, the HUD and the arms left out; scenes built by region edits
in front of seed 7's spawn):

| Scene (face A, noon) | Off | High | Midnight (Off) |
| --- | --- | --- | --- |
| Tunnel 3 m wide, 5 m in | 0.57 % | 0.57 % | 0.30 % |
| Tunnel 15 m in | 0.48 % | 0.48 % | 0.41 % |
| Sealed room 13 × 13 m | 0.22 % | 0.22 % | 0.07 % |
| Under a 31 m roof 5 m up (looking at the ground) | 1.33 % | 1.24 % | 0.08 % |
| Under a 31 m leaf canopy, 4 m thick | 1.33 % | 1.19 % | 0.08 % |
| The sea under a 31 m roof, before the water took the flood light | 33.2 % (median 30.8 %) | 16.3 % | — |
| The sea under a 31 m roof, after | 2.11 % (median 0.58 %) | 1.93 % | — |
| The open sea, for scale | 33.4 % | 34.5 % | — |

Face B's tunnel and room read the same within 0.3 %. Face B's roof and canopy views read 9 % because
their view takes in the open hills past the edge; the ground under them reads 0.34 % (face A 0.25 %;
the lower part of the view only), so the view's mean rises from what lies beyond the roof, not
from the light under it (the after shot of the sea keeps the open sea
beyond the roof in its top third).

**What a carved tunnel looks like now** (both faces): at its mouth the floor takes most of the sun;
5 m in the sun is gone and the ambient about a third; 10 m in about the floor (0.16 against 0.08);
15 m in the floor. A tall cave and a tunnel under a hill are dark whatever their size.

**Cost** (Apple M1, `cargo run --release -p dwell-mesh --example light_cost`, seeds 7, 1, 42, about
790 chunks per face, measured while other agents loaded the machine, load average 9–46: upper
bounds): lighting a chunk as it arrives 1.2–1.4 ms on average, 4.5 ms the slowest chunk on an idle
machine (0.22.9–0.22.12: 2.3–2.9 ms, the slowest 13–16 ms); unloading the square's edge (a
flight's batch) 0.3–0.6 ms; meshing +3–7% with the flood stream (+2 floats
per vertex); an edit 0.02–0.03 ms (a trench), 0.3–0.4 ms (a 16 m roof placed or taken away); 18 MB
of light for 790 chunks (568 need an array, 32 KB each; the rest are uniform). In Chrome: one long
task at load, as 0.22.5; the frame's GPU time unchanged (one more vertex pair, a few ALU ops).
The bench's `climb-fast`, `fly-high` and `fly-low-fast` (real GPU, M1, load average 41): p99
frame interval 18.7 ms, the worst 35 ms, no long task over 50 ms, as 0.22.8 (0.22.10: worst
1.2–1.3 s); at the spawn 50 chunks meshed 0.42 s after the view was generated and 200 at 4.3 s, at
60 fps (0.22.8: 0.37 s and 4.5–5.9 s).

**Extending it**: the falloffs, curves, ramp and block light are `shared/look/light.json`
(`node shared/look/light.mjs`; CI fails on a stale `light.gen.rs`; `light.test.mjs` changes values
by data alone); per-block filter and emission are block data (`shared/blocks`). The level range
(0–15) and the rules are code.

## 12. Distant terrain shadows: horizon maps

**[built, Phase 46]** ([ADR 0046](./adr/0046-distant-terrain-shadows.md)) Owner (2026-10-21): the
lighting plan's layer 3, sun shadows on the distant LOD terrain past the cascades (§10): mountains
shading valleys at dusk, blending into the cascades at their far edge, with no main-thread work or
frame spikes. Code: `dwell-client` `render/lod_shadows.rs` (the maps, their fills and sweeps, the
WGSL), `dwell-look` `lod_shadows.rs` (the tunables, the march and the blend in f64); data:
`shared/look/lodshadows.json` (validated and generated by `shared/look/lodshadows.mjs` into
`lodshadows.gen.rs`; `lodshadows.test.mjs` changes it by data alone; CI fails on stale output).

- **Heightmaps of the terrain as drawn**: the chunks, LOD sections and stand-ins shown, drawn from
  above into depth maps around the eye (the view looks along the face-local down through
  `FaceFrame`, so face B's are its mirror image; both sides of every face; no bias), each texel the
  highest surface over it, face-local. High: two levels, 1024² texels of 4 m (4 km) and of 16 m
  (16 km); Low: one, 512² of 8 m. The coarser level skips the chunks (the finer one holds them). Each
  map's centre is snapped to whole texels, so a refill of unchanged terrain gives the same map.
- **Fills, spread**: a level is filled into its second layer a slice of its casters a frame (High
  80 thousand triangles and 96 draws at most, Low 60 and 64), then its max mips (each texel the
  highest of the four under it), and replaces the map in use when whole. The levels take turns,
  and a level is filled again only when the eye has moved `moveShare` (⅛) of its half-width from
  where it was filled or `everyS` (10 s) have passed (edits, LOD swaps, chunks arriving): standing
  still costs nothing.
- **Horizon maps, swept**: per level an `r16float` map (two layers). A sweep marches every texel's
  ray toward the light over the heightmaps in use (from 1.5 texels out, 24 steps on High and 16 on
  Low growing geometrically to `reachM`, 5 km or 2 km, each reading the max mip its length needs, so
  a long step sees every ridge it passes) and keeps the terrain's highest elevation tangent seen
  from the texel's height plus `biasM`. 32 rows a frame; it replaces the map in use when whole. A
  level is swept again when its heightmap was refilled or the light has turned `turnDeg` (0.5°)
  about the face's up; the light's elevation needs no sweep (the fragment compares it).
- **Receiving**: every fragment on the face (inside the cascades too, below) reads its column's horizon (bilinear, the finest
  level holding it) and takes `smoothstep(−1, 1, (tan elevation − horizon) / tan penumbra)` of the
  light (`penumbraDeg` 1.5°: a painterly soft edge), faded out over the outermost map's last
  `edgeFade` (20 %). Only fragments on the viewer's face, and only while a light casts.
- **Composition**: `shadow_light`'s factor is `combine(stand_in, pcf, near, strength, far)`:
  inside the cascades `mix(stand_in, pcf × far, strength)`; past them `stand_in × mix(1, far,
  strength)`; over the cascades' last `edgeFade` the two blended by the same weight, so the factor
  is continuous across the edge (a test steps through the band) and equals the second layer's alone
  when `far` is 1. The time of day's fade weakens both alike. Inside the cascades the far shadow
  multiplies theirs (2026-10-26, docs/BUGS.md 2): the cascades' casters are front faces within
  `casterReachM` (120 m) toward the light. A low sun's ray to a step on a hill's shaded side leaves
  the hill through its sun-facing side, often farther than that, so nothing cast into the cascades
  there. Each step's top was lit and each riser dark (a grid down the shaded slope), while past the
  cascades the same slope was in the hill's shadow. Seen in Chrome on seed 7 at 0.82.
- **Settings**: the Shadows setting's tier (Off draws none); none on a CPU adapter (CI's
  SwiftShader), whatever the setting. `?lodshadows=off` turns it off for the bench
  (configuration `lod-shadows-off`); scenarios `lod-shadows-dusk` and `lod-shadows-dusk-b` (face B)
  shoot and record a low sun over seed 7's hills from 520 m up.
- **Limits**: a surface under the top of its column (a cliff's foot, under an overhang) takes the
  column top's horizon; the shadows lag a moving eye by a fill and a turning sun by a sweep (a
  fraction of a second to a few seconds); clouds cast none yet (`cloud_shadow`).

**Cost** (Apple M1, Chrome, 1280×720, `npm run bench -- --compare idle-spawn fly-high climb-fast
lod-shadows-dusk lod-shadows-dusk-b --configs lod-shadows-off --unthrottled`, Shadows High, one run
each). **The machine was busy** (other agents; load average 8 rising to 30 during the run), so
treat these as upper bounds and noisy. `gpu.frame` mean (p95), ms:

| Scenario | On | Off | On − Off |
| --- | --- | --- | --- |
| idle-spawn | 15.5 (20.0) | 16.1 (22.2) | ≈ 0 (noise) |
| climb-fast | 10.3 (16.5) | 9.8 (15.5) | +0.5 |
| fly-high | 10.4 (15.0) | 9.8 (14.5) | +0.6 |
| lod-shadows-dusk | 14.8 (19.3) | 13.9 (18.2) | +0.9 |
| lod-shadows-dusk-b | 13.0 (17.6) | 12.2 (16.3) | +0.8 |

The frame interval's p50, p99 and worst frame and the main thread's `frame.cpu` were unchanged
(climb-fast worst 71 vs 72 ms, fly-high 20 vs 24 ms; no new long tasks): the fills' draws are the
only main-thread share, a budget a frame. The fill and sweep passes' own timestamps read 2–3 ms
each, but on the M1's tiled GPU a small pass's timestamps include the work it waits behind; the
frame's total is the measure. The first build marched every far fragment (24 steps) instead of
sweeping: +20 ms a frame, the reason for the sweep (ADR 0046). 16 MB of GPU memory on High, 2 MB on
Low.

**Extending it**: the heights' range, the march, the refresh rules and the tiers are data; a tier is
a new entry of `tiers` with its Shadows setting. The vocabulary in code is small: the fill from
above, the max mips, the sweep's march and the fragment's shade.
