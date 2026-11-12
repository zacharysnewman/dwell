# Sky clouds

Phase 31, [ADR 0034](./adr/0034-sky-clouds.md). The owner's brief (2026-10-15): "procedural clouds in
the skybox (not 3D)". This is a 2D cloud layer drawn as part of the sky, with no geometry. Since
2026-11-11 the High setting draws a ray-marched volumetric layer instead (§8, first slice); the deck
below stays Low's, the CPU-light paths' and the sky's elsewhere. Architecture: `ARCHITECTURE.md` §6.6 (Clouds) and §5 (`dwell-look`, `render/`,
`ui/`).

## 1. Decisions (defaults, not yet asked)

Clouds are new, so the owner wasn't asked CLAUDE.md's design questions before the build. These
defaults were chosen and are open to change (§6):

| Question | Default taken |
|---|---|
| What gets added later? | More tunables in the data, a weather system driving the coverage (decided, §6), cloud shadows on the terrain (decided, §6), and maybe more cloud layers (one deck now). |
| Who adds them, and with what? | A developer edits `shared/look/clouds.json` (validated, generated, CI stale check). A weather system would call `setCloudCoverage` / `Renderer::set_cloud_coverage`. |
| What stays fixed in code? | The deck model (a sphere-curved plane), the noise (value-noise fBm with a domain warp), the lighting terms, the octave transform. |
| What is data? | Altitude, curvature, fades, cell size, wrap, octaves (High and Low), gain, warp, coverage, edge softness, opacity, wind and evolve speeds and directions, the colour tints, absorption, the shadow step, the silver lining, the sunset warmth, the cross fade, the in-cloud fog and the top light. |
| Per build, per world or per player? | Tunables per build. The Clouds quality setting per browser. Coverage per frame (a uniform, for weather). |

## 2. The model

- **The deck.** Each face has its deck, a thin layer `altitudeM` (1.8 km) above that face's sea level; both
  are drawn, and a ray takes the deck of the air it passes through (DAY_NIGHT.md §13). It
  lies on a sphere about a centre `curvatureM` (400 km; 120 km before C5, raised so that from high up the deck reaches far toward the flat world's horizon) below sea level, under the eye. From an eye
  at sea level, a sky pixel's view ray `d` meets it at distance `t = c / (r·y + √(r²y² + c))`, with
  `c = 2rh + h²`, `y = d·up` and `h` the deck's height above the eye (§7 has the general form, from
  any height). This form doesn't cancel: `t = h` straight up and `√(2rh + h²)`
  ≈ 38 km at the horizon. Below the horizon the clouds are hidden. They fade in over `horizonFade`,
  fade out between `fadeStartM` and `fadeEndM` along the ray, and fade as the eye comes within
  `crossFadeM` of the deck. From above, the deck's top shows (§7).
- **Only over the disc** (BUGS.md #101). The deck and the volume reach the world's radius (its size's,
  at the join) plus `rimMarginM` (2 km) from the disc's axis, fading out over the last `rimFadeM`
  (6 km): the frame's `cloud_disc` (the eye's world x and z, the edge, the fade; `cloud_disc` in
  `clouds.rs`), `dwell_cloud_rim` on the deck's hit and on each march sample's cover, and
  `cloud_rim_span` cutting the march to the part of the ray over the disc (a far eye's march takes no
  samples). CPU twins `rim_cover` and `rim_span`. Without it both were a layer round the eye, drawn in
  the void under a camera far past the rim.
- **World-anchored, f32-safe.** The hit's horizontal offset from the eye is added to the noise point
  under the eye. That point is `(eye − wind·t) mod wrapM` in cells, computed in f64 on the CPU. The
  lattice repeats every `wrapM` (128 km, a whole number of `scaleM` cells), and each octave's
  transform `[[2, 1], [−1, 2]]` maps the integer lattice onto itself, so every octave repeats with
  the first and the shader never sees a large coordinate. The transform also turns each octave
  26.6°, which hides the lattice's axes.
- **Noise.** The base is value noise with quintic interpolation and an integer hash, periodic. A
  two-octave warp field moves the sample point (`warp`, in cells). The cloud is the fBm of
  `octaves`, its weights falling by `gain`. Each octave fades to its mean once the pixel's footprint
  (`fwidth` of the deck point) spans it, so the far deck doesn't shimmer.
- **Shape.** The fBm is stretched about its mean, then cut at `1 − coverage` with a soft edge of
  `edgeSoftness`, so `coverage` is roughly the share of the sky covered (a test pins it). Coverage is
  one parameter: a frame uniform, defaulting to the data's value.
- **Drift.** The pattern moves at `speedMps` toward `directionDeg`. The warp field moves at
  `evolveMps` toward `evolveDirectionDeg`, so the shapes change as they go. The clock is world time:
  the server's tick from its snapshots (`world_clock.rs`: the largest server-minus-local offset seen,
  reset when a server starts over), or the local clock before the first snapshot.
- **Light.** It comes from the day/night cycle (Phase 32, [`DAY_NIGHT.md`](./DAY_NIGHT.md)): the
  renderer's `cloud_lighting_for` reads the frame's `DaySky` block for the deck's own face:
  - The light is the sun or the moon, whichever is over the deck's face.
  - The data's `sunlit`, `shade`, `nightLit` and `nightShade` colours tint the hues of the sun's,
    the day sky's ambient, the moon's and the night sky's ambient lights.
  - Day or night by the sun's height over the face (the cycle has one time of day all round the
    sky; before it, a turned sky could be night on one side and day on the other).
  - Self-shadow: `lit = exp(−absorption × (0.35·d + d'))`, with `d'` the density
    `shadowStepM` toward the light at `shadowOctaves` (Medium, High) or `lowShadowOctaves` (Low)
    octaves; with 0 of them, `0.6·d` alone (Low until 2026-11-12, §10).
  - A silver lining brightens thin cloud near the light. A low sun warms the lit parts toward
    `sunset`.
  - Distant cloud mixes toward the clear sky behind it, which already carries the horizon haze.
  - Clouds cast no shadows on the terrain.
- **In the composite.** The deck point and its footprint come first, in uniform control flow. Fully
  covered pixels skip the clouds. Where the sky shows, the clouds go over the clear sky. The
  terrain's fog still mixes toward the clear sky, so distant ground never takes a cloud's colour.
- **Face B.** Up is `(0, face, 0)` and the deck height is face-local, so B's clouds are over B's own
  horizon. B samples the noise half a period away, so its clouds aren't A's above the same spot.
  The other face's deck rides in the frame as `clouds_o` and `cloud_cfg_o` (its noise point and its
  height above the eye in its own frame), drawn below the eye's horizon under the eye's haze
  (`sky_behind_with_clouds`), so the clouds stay put as the player crosses the midplane.
  View rays are world directions from the same basis as the terrain, so the camera's turn-over
  needs nothing of its own (checked in a browser, §5).

## 3. Settings

The Clouds setting has three values: Off, Low (`lowOctaves` = 3, no self-shadow) and High (`octaves`
= 5 and the self-shadow). It is a row of buttons in Settings → Look. It defaults to High on desktops
and Low on touch devices (`prefersTouch`, the same test the LOD's phone budgets use). It is kept
under `dwell.clouds`, its own key in the browser's append-only settings store. Named views and bench
scenarios can set it, and the time of day too (`settings.clouds`, `settings.timeOfDay`: `?time=`).

## 4. Cost

Measured with `npm run bench -- clouds-high clouds-low clouds-off --unthrottled` at a 1280 × 720
window on an Apple GPU (Metal), looking 25° up (mostly sky):

| Clouds | `gpu.composite` p50 | mean | `gpu.frame` p50 |
|---|---|---|---|
| Off | 0.67 ms | 0.95 ms | 2.04 ms |
| Low | 1.34 ms | 1.61 ms | 2.63 ms |
| High | 1.59 ms | 1.88 ms | 2.81 ms |

Clouds cost about 0.9 ms of composite time on High and 0.7 ms on Low at this size, and scale with
the sky's share of the screen (covered pixels skip them unless the deck is in front). Phones default
to Low. Not yet measured on a phone.

**From above (C5)**, measured with `clouds-above` against `clouds-above-off` (the camera 3.5 km up,
looking down 40°, the deck over most of the screen; `--unthrottled`, the same window and GPU, one
run at a quiet moment on a GPU other work shared):

| Clouds | `gpu.composite` p50 |
|---|---|
| Off | 1.51 ms |
| High, from above | 2.42 ms |

About 0.9 ms more: nearly every pixel takes the clouds, but without the self-shadow's second
sample. In the same run the sky's clouds (`clouds-high` against `clouds-off`) measured 1.65 against
0.60 ms. Runs varied by up to 4× between repeats on the shared machine; treat these as rough.

## 5. Checks

- Unit tests (`dwell-look` `clouds.rs`): the deck intersection (straight up, the horizon, on the
  sphere, the flat limit, none at or above the deck), face-local height, the seamless wrap, coverage
  as the covered share, a tunable changed by data changing the shader and the clouds, the quality
  setting's octaves and shadow, offsets staying in one period far out and drifting with the wind,
  and Naga validation of the WGSL (alone and in the renderer's module).
- `shared/look/clouds.test.mjs`: the checked-in Rust is what the data generates, a data edit changes
  it, and bad entries fail with their path. `world_clock.rs`: follows ticks, ignores late
  snapshots, resets.
- Browser (bench scenarios `clouds-high`, `-low`, `-off`, `-night`, `-dusk`, `-fly` and `-altitude`):
  looking up, the horizon at midday, toward the sun, night, a low sun, flying at speed (parallax and
  drift), and the fade near the deck. Face B by day and night was checked with a scratch scenario
  (the camera placed over B).

## 6. The owner's answers (2026-10-15) and what's still open

Decided:

1. **The look**: the stylised look is right.
2. **Clouds from above**: yes. The deck is drawn below an eye above it, over the terrain (§7).
3. **Weather**: a weather system will drive the coverage later. Not built: it's a future item in
   Phase 31 (C6). The hook (`setCloudCoverage`, a frame uniform) is there.
4. **Shadows**: clouds will cast shadows on the terrain later. Not built: it's a future item in
   Phase 31 (C7).

Still open:

5. **Synchronisation.** The drift runs on the server's tick, so players on one server see roughly the
   same sky. Is that enough, or should the pattern be tied to a world seed? A weather system (C6)
   would decide this with it.
6. **The data's location.** These tunables sit beside the motion effects' `shared/look/motion.json`
   (another branch), each with its own generator. Should they be folded into one look generator?

## 7. Clouds from above (C5)

The owner asked for the deck to be visible from above (answer 2 in §6).

- **One sphere, both sides.** The deck is the sphere of radius `curvatureM + altitudeM` about a centre
  `curvatureM` below sea level, under the eye. The eye is `h` below the deck (negative: above it).
  - From below (inside the sphere), every ray meets the deck at the far root, and the clouds show
    above the horizon as before.
  - From above (outside), only rays aimed down past the deck's own horizon meet it, at the near
    root: the deck's top.
  - Both roots are in forms that don't cancel. `deck_distance` is the f64 twin, tested from below,
    above and inside.
  - From above, the clouds fade where the ray grazes the deck (`horizonFade`, on the sine of the
    angle it meets the sphere at). The distance fade reaches out with the deck's own horizon
    (`deck_fade`: from 40% of the horizon's distance to all of it), so from high up the deck reads
    as a cloud sea out to its horizon. From above, the clouds are also denser (the square root of
    the density: the deck is seen through its whole depth).
- **Over the scene, by depth.** The deck hit is `t` along the unit ray. The view ray scaled to view
  depth 1 is `|ray|` long, so the hit's view depth is `t / |ray|`. The composite draws the clouds
  over a covered pixel only when that is nearer than the stored view depth (A1's `r32float` target,
  the mean of the covered samples). The view depth has one meaning across the far and near passes,
  so chunks and LOD sections compare alike. Seen from above, the deck is also in front of any
  scene point below its altitude (the ray's face-local rise times the depth is under the deck's
  height above the eye): the ray crossed the deck to reach it. The hit alone isn't enough there:
  the sphere sinks below sea level √(2 · curvature · altitude) out (38 km), so from tens of km up
  the far sea came out in front of it and the clouds showed only in a circle round the eye and over
  the void past the world's edge (owner, 2026-10-21). This is `dwell_cloud_in_front`, called only
  through the composite's `cloud_in_front_of`; its twin `cloud_in_front` is tested at chunk and LOD
  distances and from 40 km up out to the world's edge (bench `clouds-far-above`). It also puts the deck correctly in front of
  a peak seen through it from below. The terrain still fogs toward the clear sky first, and the
  clouds go over that.
- **Bright tops.** Seen from above with the light over the face, the light reaches the top through
  less cloud: the self-shadow's absorption is cut by `topLight` (0.9). By day the tops also take the
  light's tint at half strength (a warm sun otherwise reads as sand on a deck seen only lit). Day or night follows the sky
  in the ray's level direction, as `dwell_sky` takes it below the horizon.
- **The crossing.** The deck fades out over `crossFadeM` (400 m) either side of it, from below as
  before and now from above. As it does, an in-cloud fog fades in: the whole view mixes toward the
  cloud's lit colour by the density of the cloud at the eye's position, times `insideFog` (0.9), times
  how near the eye is to the deck. The CPU computes that density once a frame with the noise's twin
  (`cloud_uniforms`' third vector). Clear air at the deck gives no fog, a cloud gives a white-out,
  and nothing pops.
- **Edge-on.** Within the layer the deck is seen edge-on, as a band at the horizon
  (`deck_band`): along a ray within 0.12 (elevation sine) of level, cloud where the ray leaves the
  layer (`crossFadeM` thick each side), out to the fade's start. The band is the layer seen
  through its length, so its density is the coverage (the noise there would show as vertical
  stripes). It is strongest with the eye at the deck and the ray level, and it hands over to the
  deck proper as the eye leaves the layer.
- **Cost.** From above, the self-shadow's second sample is skipped: the sunlit top barely shows it.
- **Face B** uses its own deck: the height is face-local, and up is `(0, face, 0)`.

## 8. Volumetric clouds (2026-11-11)

The owner (2026-11-11): "volumetric clouds in the sky. These clouds will likely be useful for weather
sim, and volcano smoke. So they need to be performant." [ADR 0034](./adr/0034-sky-clouds.md)'s
amendment records the decision. Built in two rounds the same day: the march and its data, then the reduced-resolution pass with accumulation (§8.6); §8.5 lists what is left.

### 8.1 The model

- **A layer, ray-marched.** On a tier with steps (`tiers` in `clouds.json`), the clouds pass (§8.6) marches each
  pixel's ray through the layer between `volume.baseM` and `volume.topM` above the viewer's face's sea
  level (flat, face-local; the deck's sphere is not used), out to the layer's end or `maxDistanceM`
  (the composite puts the scene in front, §8.6). The steps are spread evenly over the ray's span
  inside the layer, offset per pixel and frame (`cloud_jitter`), and the march stops once the cloud
  is 98 % opaque. With no light samples, a step's own density stands for the light's path.
- **Density:** the shape model of §9 (since 2026-11-12): the deck's noise as the weather map (cover, cloud
  type, precipitation), height profiles per cloud type, a baked Perlin–Worley base shape eroded by baked
  Worley detail. Until then it was the deck's coverage cut rising with height and eroded by 3D value
  noise, which read as white blobs (the owner).
- **Extra sources: plumes,** at run time (§8.7): vertical columns (steam over lava, a volcano's ash,
  a smoke stack) the game adds, moves and removes, marched apart from the layer. `clouds.json`'s
  `plumes` are the list's defaults (empty as shipped).
- **Light** (§8.8): the sun or the moon, whichever is over the face, as the deck. At each step with
  cloud, `lightSteps` growing samples toward the light give the optical depth (Beer with a
  multiple-scattering floor and a powder term), a dual-lobe Henyey–Greenstein phase gives the silver
  lining toward the light, and the ambient is the sky's light from above and the ground's darker
  bounce from below, each dimmed by the cloud over or under the sample: shaded bases, lit tops. The
  colours are the deck's (its shade and lit, the low sun's warmth, the moon's at night), and the far
  haze toward the clear sky is the deck's.
- **Inside.** Flying into a cloud, the march starts at the eye, so the view fogs by the density
  around it. The deck's CPU in-cloud fog (`insideFog`) is turned off on a volume tier
  (`cloud_uniforms`' third vector's x is 0) so the two don't stack.
- **What keeps the 2D deck.** The volume replaces only the eye's deck in the composite. Snell's window
  (`sky_above_with_clouds`, WATER_RENDERING.md §16), the water's reflections and the other face's
  deck below the horizon keep the deck: the cheap path. A test pins that only the clouds pass marches
  and that `sky_above_with_clouds` neither marches nor samples the volume.

### 8.2 Tiers

`tiers` gives each Clouds setting value its march: steps (0: the 2D deck), light samples per step
(at most 6, `MAX_LIGHT_STEPS`; a lower setting never takes more than a higher one, the generator
checks; 0: the cloud above the sample along the light's slant stands for its path; until 2026-11-12
High took 1 and Medium none, §8.8),
the resolution divisor of the clouds pass (§8.6) and the share of each new frame in the accumulated
picture (`historyBlend`):

| Setting | Steps | Light samples | Resolution | Blend | Who |
|---|---|---|---|---|---|
| High | 16 | 6 | ½ | 0.2 | opt-in (the Ultra preset) |
| Medium | 10 | 4 | ¼ | 0.2 | desktops' default (and the Medium and High presets) |
| Low | 0 (the 2D deck) | — | — | — | touch devices' default |
| Off | — | — | — | — | |

High is opt-in because from above it is at its budget's edge (§8.3). The CPU adapter (CI's software
renderer) still draws no clouds (`clouds_for`): its frames are the e2e suite's bottleneck. The
setting's stored values gained `medium` (the store is append-only; an older build reading it falls
back to its default).

### 8.3 Cost

Measured 2026-11-11 with a scratch scenarios file (`--warm --unthrottled`, 1280 × 720, Apple GPU on
the owner's Mac; all configurations in one run). Two views: the noon sky from the ground (yaw 240,
pitch 45) and the camera 3.5 km up looking 35° down with the layer under most of the screen. The
cost is `gpu.clouds` (the clouds pass) plus `gpu.composite`, minus Off's `gpu.composite`, p50:

| Tier (run) | From the ground | From above | Budget |
|---|---|---|---|
| First slice: full resolution, 20 steps, 1 light sample | +1.0 ms | +2.5 ms | 1.5 ms |
| High ½, 24 steps, 2 light samples | +0.9 ms | +2.1 ms | 1.5 ms |
| High ½, 18 steps, 1 light sample | +0.1 ms | +1.7 ms | 1.5 ms |
| **High ½, 16 steps, 1 light sample (shipped)** | **+0.5 ms** | **+1.5 ms** | 1.5 ms |
| Medium ¼, 16 steps, 1 light sample | +0.3 ms | +1.0 ms | 0.8 ms |
| Medium ¼, 12 steps, 1 light sample | +0.2 ms | +1.0 ms | 0.8 ms |
| **Medium ¼, 10 steps, no light sample (shipped)** | **+0.3 ms** | **+0.9 ms** | 0.8 ms |

Repeats vary by about ±0.3 ms. From the ground both tiers are well within budget. From above, High
sits at its 1.5 ms and Medium is 0.1 ms over its 0.8 ms. The clouds pass barely got cheaper as the
steps and resolution fell (Medium's pass took 0.8–0.9 ms at a quarter resolution with 10, 12 or 16
steps), so its cost from above isn't the march's samples. It's probably a fixed part (the pass's
start on a tile GPU after the heavy scene, the history copy) or the timestamps taking in the scene's
tail. A Chrome trace of the pass is the next step before cutting more samples.

### 8.4 How it looks

Screenshots (bench shots, judged honestly):

- **Noon from the ground:** soft, white, rounded cumulus with flat-ish bases and real parallax. With
  the accumulation the first slice's dither grain is gone: the clouds are smooth. They are a little
  soft-edged from the reduced resolution, and their shading is still mild.
- **From above:** the tops over the terrain with the ground between them, now with grey shaded
  sides; flat-lit tops still read mostly by their outlines.
- **Inside a cloud:** the view fogs by density around the eye, with no pop and no grain.
- **Sunset:** the low sun warms the lit sides.
- **The fixture plume** (a scratch build with a test plume 4 km away): a dark ash-grey column rising
  out of the hills through the white clouds (`color` per plume). Its foot over the hills shows a
  little of the reduced resolution where the hills cut it.

### 8.5 Left for the next rounds

- Find the clouds pass's fixed cost from above (a trace), so Medium meets 0.8 ms and High gains
  headroom; then perhaps a second light sample on High (better shading from above). The shader read
  of §8.7 says it isn't the march's arithmetic; the bench numbers with §8.7's cuts are to take on a GPU.
- A CPU twin of the whole density (the in-cloud fog for other systems, e.g. the ink's fade).
- The far field: past `maxDistanceM` the volume ends; the deck could take over there.
- Plumes don't shadow the layer or take its light samples, and overlapping plumes are put
  together by their first distance (not sample by sample): fine for apart columns (§8.7).
- A remote server's lava with no column loaded near its vent has no plume (the client looks only in
  the cells it holds); a server message for plumes (eruptions' ash) comes with the eruptions.
- A phone check (phones keep the deck, Low, by default).

### 8.6 The clouds pass: reduced resolution, accumulated

- **The pass.** Before the composite, `clouds_fs` marches the volume into the clouds buffer at the
  tier's divisor of the drawing buffer (`cloud_buffer_size`): premultiplied colour and cover
  (`rgba16float`), and the scene's span under the texel (`rg16float`, §8.9). It marches to the
  farthest scene pixel under the texel (the layer's end where any of it is sky), §8.9.
- **Accumulation.** The step offset turns each frame (interleaved gradient noise plus a golden-ratio
  step over 16 frames). Each texel blends its new march with the last frame's buffer, reprojected by
  direction through the last frame's camera (`CloudPrev`: right, up, forward, tangents). Clouds are
  far, so direction is enough; since §9.6 a resolve pass after the march clamps the history into its
  neighbourhood instead of handing a texel near the eye to its raw march. The history starts over with new buffers, the first frame, or a
  move over `CLOUD_HISTORY_JUMP_M` (200 m) in a frame (`cloud_prev_block`, tested). The pass then
  copies its buffer into the history for the next frame.
- **Depth-aware upsample.** The composite weighs the four texels round the pixel by their scene
  span (`upsampled_clouds`, §8.9).
- **Plumes' colour.** Each plume has a `color`; its own march (§8.7) tints its lit cloud by it.
- **Timing:** `gpu.clouds` in the telemetry.

### 8.7 Run-time plumes and the pass's cost (2026-11-12)

The owner (WORK_QUEUE item 13): "These clouds will likely be useful for weather sim, and volcano smoke.
So they need to be performant." Next was "trace the clouds pass's fixed cost; plumes as run-time
uniforms for a volcano or weather".

- **The list** (`dwell_look::plumes::PlumeList`): plumes by name, at most `plumeSlots` (8; the
  uniform's size, data), each on a face (`FaceFrame`) with its axis at world x, z, its base and top
  face-local over sea level, a radius growing by `spread` to the top, a density, a colour and
  `riseMps` (its billows rise). `set` adds or replaces one by name (refused when full, or for a
  plume that can't be drawn: non-finite numbers, its top not over its base, no radius),
  `move_to`, `remove`, `remove_prefixed` (a source's own), `clear`. It starts with the data's
  `plumes` (on face A). The renderer holds it (`Renderer::plumes`); a source replaces its own
  plumes through `Scene::set_plumes(prefix, …)`.
- **The uniform** (`PlumeList::pack`, `CloudPlumes` at the clouds pass's group 3, binding 4): a head
  (plumes in use, samples through each: the tier's `plumeSteps`) and the slots (axis offset from the
  eye in metres, worked out in f64 on the CPU, so no wrap; radius, spread; base, top, density, the
  billows' rise so far; colour). Only the eye's face's plumes within `maxDistanceM` (plus their
  widest radius) across, nearest first; on a tier with no plume samples (Low, Off; the CPU adapter
  runs no clouds pass) the head says none. One fixed-size uniform: a new plume is a buffer write,
  never a pipeline.
- **The march** (`cloud_march_wgsl`, only in the clouds pass's module, so the deck-only modules need
  no binding): the layer as before, then each plume the ray crosses: its bounding column (the
  widest radius, base to top) met analytically (`cloud_plume_span`, twin `plume_span`), and
  `plumeSteps` jittered samples over that span alone, so a 15 m steam column gets its samples where
  it is, not 1–2 km apart along the layer's span. Density as before (`plume_density`) broken up by
  rising 3D value noise of cells the plume's width (`volume.plumeBillow`); lit by Beer over its own
  radius toward the light, tinted by its colour. Plume and layer are put together front to back by
  their first distance; the silver lining, haze and distance fade apply once to the result.
- **Tiers:** `plumeSteps` High 8, Medium 6, Low 0.
- **The first source: steam over lava vents** (`dwell-client` `vent_plumes`): the world's vents
  from its worldgen config (`lava_vent_sites`, on the faces each lists), and every 2 s each vent
  column is looked down in the cells the client holds (air where unloaded) for its top
  `dwell:lava` cell: a plume of `sources.lavaVent`'s look (14 m wide at the lava, ×3.5 at the top,
  180 m tall, density 0.35, near-white, rising 3 m/s) over it, removed when no lava is found. Local
  and remote servers alike, with no protocol change.
- **Dev tools:** `__dwell.plume(op, args)` (DEV_TOOLS.md).

**The pass's fixed cost, read from the shader** (no GPU in the cloud session that built this; the
timings are left for a local `npm run bench -- --warm` before and after). Counting a 2D value-noise
look-up as 4 hashes (~45 ALU) and a 3D one as 8 (~100 ALU): a layer step with no cloud is the weather
map (a one-octave warp and two octaves: 4 look-ups, ~180 ALU); a cloudy step adds the detail (~100)
and each light sample (another ~180). Medium's 10 steps are ~1.8–3k ALU a texel; at ¼ of 1280 × 720
(57.6k texels) that is ~0.15 G ALU a frame, well under 0.1 ms on the Apple GPU the 0.8–0.9 ms was
measured on. So the pass's cost from above isn't the march's arithmetic, as §8.3 suspected: it is
the fixed part (the pass's start and end on a tile GPU after the heavy scene, the history copy, the
timestamps around it). Cut here, cheaply:

| Cut | Before | After |
|---|---|---|
| The clear sky the haze needs (`sky(dir)`, the sky gradient) | every texel | only texels with cloud |
| Data plumes in the layer's density | every layer step and light sample, per plume (~25 ALU each, ×2 a cloudy step) | none: plumes marched apart, only where the ray crosses one |
| No coverage (a clear-sky weather) | every step sampled the weather map | no layer samples |
| Plumes off the view or past the reach | in the shader's code for every texel | culled on the CPU; a missed column costs ~30 ALU (its span test) |

A plume the ray crosses costs `plumeSteps` × ~140 ALU (Medium: ~0.8k) on the texels it covers only.
The march's resolution and step counts were already by tier (§8.2). Left: the trace on a GPU of the
pass's start and copy (§8.5).


### 8.8 Lighting (2026-11-12)

The owner (WORK_QUEUE item 13): "the lighting on the new clouds just looks like it's all bright white so the
actual shape is difficult or impossible to read. Our old 2D clouds had better lighting, use that as a
reference", and again: "the volumetric clouds still look bad no proper lighting or shading?". The cause:
Medium (the desktops' default) took no light samples, so a sample's light was its own density's
(`exp(−1.06 d)` at most), mixed from the deck's shade colour to its lit one, which are both near
white; the deck had a sample toward the sun and its whole fBm's contrast. Every sample now takes
(`volume` in `clouds.json`, the CPU twin in `dwell_look::clouds`: `light_depth`, `sample_light`,
`sample_colour`, `cloud_phase`, `soft_clip`; the shader's `cloud_vol_light`):

- **The sun's light:** `lightSteps` samples toward the light (the tier's), the first `lightStepM`
  (160 m) long and each `lightStepGrowth` (1.6) times the last, so Medium's 4 reach 1.5 km, the
  layer's depth; the path stops where it leaves the layer (an overhead sun from a top takes one or
  two). Their optical depth through Beer's law (`light.absorption`, `extinction`) with
  `multiScatter` the floor a wider, weaker Beer keeps (light scattered many times still reaches the
  inside), a powder term (`powder`, strongest looking away from the light), and the **phase**: a
  dual-lobe Henyey–Greenstein (`phaseForward` 0.5, `phaseBack` −0.2 at `phaseBackWeight` 0.3,
  shaping the light by `phaseStrength` 0.6), so thin cloud toward the sun glows (the silver lining:
  ~4× the side-on light) and the light behind the viewer lifts a little; times `sunStrength`, the lit
  colour. It replaces the deck's added rim on the volume.
- **The ambient:** the shade colour (the sky's light) at `skyAmbient` from above and at
  `groundBounce`, tinted by `bounceTint` (a warm grey), from below, each dimmed by e every
  `ambientDepthM` (400 m) of cloud over or under the sample. That cloud is read from the sample's
  own weather noise at no cost: the coverage's cut rises with height, so the column's top is where
  it passes the noise (`volume_column_top`), half dense on average. With no light samples it also
  stands for the light's path along the light's slant.
- **The colours** stay the deck's palette (`light.shade` × the day sky's hue, `light.sunlit` × the
  sun's, the sunset warmth, the moon's and the night sky's at night), so the volume reads like the
  deck did, with its shape: from the CPU twin with the shipped data, a broad cloud lit from overhead
  goes from ~0.47 luminance at its base to ~0.97 at its top, and a low sun's 0.40 to 0.80 (side-on),
  up to 1.1 looking toward it. The brightest roll off a little over white (`soft_clip`).
- **Plumes** take the same light, with their own radius toward the light and their column over and
  under the sample.

**Cost, read from the shader** (§8.7's counting: a 2D value-noise look-up ~45 ALU, a 3D one ~100). The
march reuses its noise: one warp per view sample, its light samples reuse it and take only the
two-octave weather noise (2 look-ups, ~90 ALU, half of the old light sample's 4) with no detail; the
phase and the powder's strength are once per ray; the lighting is ~40 ALU a cloudy sample (four
`exp`). A march stops once 98 % opaque (~7 dense samples), so per texel:

| Tier | Before (empty – cloudy texel) | After (empty – cloudy texel) |
|---|---|---|
| Medium ¼, 10 steps | 1.8k – 2.6k (no light samples) | 1.8k – 5.3k (4 light samples) |
| High ½, 16 steps | 2.9k – 4.8k (1 light sample of 4 look-ups) | 2.9k – 7.6k (6 light samples of 2) |

At 1280 × 720 that is at most ~0.3 G ALU a frame on Medium (~0.1 ms on an M1) and ~1.75 G on High
(opt-in): Medium stays inside its budget as the pass's fixed part dominates (§8.7). The GPU timings
(`npm run bench -- --warm`, the §8.3 views, before and after) and the in-game look against the old
deck side by side are for a local session; tune by the data (`lightSteps`, `ambientDepthM`,
`sunStrength`) if Medium from above goes over.

Tests (`crates/dwell-look/src/clouds.rs`, `volume_tests`): a cloud's sun-facing top is brighter than
its base on every tier's light samples (and the base not black); deeper from the sun is darker; the
phase brightens toward the sun (and is even with no strength); the tiers' light samples come from
the data and the shader's loop bound is the cap; the march reuses one warp per sample; the light is
face-local (both faces); the shaders validate with naga. `clouds.test.mjs` checks the new entries'
ranges and the tiers' order.

### 8.9 Against the scene: the march stops at the rock (2026-11-12, BUGS.md #88)

The owner: "a bug with volumetric clouds z fighting against tall terrain like mountains." The pass marched
the whole layer and kept the distance to its first cloud, jittered every frame by up to a step (hundreds of
metres); the composite cut the cloud at the scene by a ±5 % band of that distance, so where a mountain
stood inside the layer the cut flipped frame to frame, and the colour held the cloud behind the rock too.
Now:

- **A reduced depth.** `clouds_fs` reads the scene's view depth under its texel (the divisor's square of
  full-resolution pixels, sample 0 of each: Medium's ¼ 16 loads, High's ½ 4; `cloud_scene_span`) as
  distances along the ray: the nearest and the farthest, the sky and past the reach at `maxDistanceM`. No
  extra pass: the clouds pass already binds the scene's depth.
- **The march stops at the scene.** It ends at the farthest pixel (`dwell_cloud_march_end`, twin
  `march_end`), or covers the layer where any of the texel is sky: no cloud behind a texel's rock.
- **Stable.** The texel keeps that span (`rg16float`, the second target), not the first cloud: the scene's
  depth doesn't move with the jitter, so nothing it decides flickers.
- **The composite** takes the four texels round the pixel by their bilinear share times
  `dwell_cloud_upsample_weight` (twin `upsample_weight`): 1 where the texel's march ended at the pixel's
  scene within a band of the texel's depth range, at least `depthBand` (3 %) and at most `depthBandRange`
  (15 %) of the distance, soft out to twice the band. A rock pixel takes the cloud in front of the rock, a
  sky pixel beside it the whole layer's. Where no texel matches (a spire under a texel), the nearest match
  leads by a floor weight (`UPSAMPLE_FALLBACK`): still depths alone.
- **The history** is reprojected the same way: the last frame's span is copied beside its colour
  (`cloud_dist_history`, group 3 binding 5), and the four history texels are weighed by whether their march
  ended where this texel's does, so the sky's cloud beside a peak doesn't smear onto it as the view turns;
  with none matching, the texel takes the new frame.

**Cost, read from the shader** (§8.7's counting): the clouds pass adds the span's loads (Medium 16, High 4)
and ~8 ALU each, and the history's 8 loads for its 1 filtered sample, ~0.2k ALU a texel against Medium's
1.8k–5.3k march; a rock texel inside the layer now marches its steps over a shorter span (finer, the same
count) and one wholly in front of the layer takes none. The composite takes 8 loads (4 colour, 4 span) for
its 1 filtered sample and gather, ~60 ALU a pixel. Within the tiers' budgets (§8.3); the GPU timings are
for a local `npm run bench -- --warm` (§8.3's views and a mountain in the layer).

Tests (`crates/dwell-look/src/clouds.rs`, `depth_tests`, a CPU model of the pass and the composite over a
fixture slope 4 km out, 1.7 km up, inside the layer): no sample past the scene and no cloud behind the rock on
any of the jitter's 16 frames; a cloud round the rock draws its part in front, with the cut the same on every
frame (red before: 0 to 0.79 over the sequence); at a silhouette a rock pixel takes the rock's texel and a sky
pixel the sky's; both faces; the twins validate with naga. `dwell-client` `shaders.rs` pins the wiring.

## 9. Cloud shape (2026-11-12)

The owner (WORK_QUEUE item 13): "The clouds have lighting but they don't follow cloud shapes at all, do some
research into cloud simulation. They look like white blobs." And earlier: "useful for weather sim, and volcano
smoke. So they need to be performant." The budget: High at 60 fps on an M1 MacBook Air; Low and phones keep the
deck.

### 9.1 What makes clouds read as clouds (research)

From the production talks (no web access in the session that wrote this; summarised from the talks themselves):

- **Schneider & Vos, "The Real-time Volumetric Cloudscapes of Horizon Zero Dawn" (SIGGRAPH 2015 Advances;
  GPU Pro 7, "Real-Time Volumetric Cloudscapes").** The model nearly every real-time sky since uses. A
  **weather map** (2D, per column: coverage, precipitation, cloud type) says where clouds are and what kind;
  a **height gradient per cloud type** (stratus thin and low, cumulus tall, cumulonimbus towering) says how
  each fills the layer; the **base shape** is a 128³ tiling 3D texture of **Perlin–Worley** noise (Perlin
  dilated by inverted Worley: billowy, connected blobs, which plain fBm isn't) plus three Worley octaves
  combined as an fBm; it is **remapped** by the coverage (`remap(base, 1 − coverage, 1, 0, 1) × coverage`),
  so low coverage keeps only the densest cores and the cloud shrinks to rounded cores rather than fading;
  a 32³ **Worley detail** texture **erodes only the edges** by a second remap (`remap(base,
  detail × k, 1, 0, 1)`: where the base is 1, the core, nothing is lost), its fBm **inverted with height**:
  wispy streaks at the base, billows at the top; a 2D **curl noise** distorts the detail's point (turbulent
  wisps). **Wind** moves the layer and **pushes the tops downwind** (shear, a leaning cloud); the light march
  samples the cheap base shape only (detail is invisible in the light's few samples); Beer–powder lighting.
  Rendered at ¼ resolution over 16 frames with temporal reprojection.
- **Schneider, "Nubis: Authoring Real-Time Volumetric Cloudscapes with the Decima Engine" (SIGGRAPH 2017
  Advances).** The production follow-up: the **anvil** (`coverage` raised toward the top by an anvil bias,
  so a cumulonimbus spreads), height gradients authored per type and mixed by the weather map's type value,
  adaptive march steps (long in empty air, short in cloud), and the cost per pixel kept low by sampling the
  detail only where the base shape has density. **"Nubis³: Methods (and madness) to model and render
  immersive real-time voxel-based clouds" (SIGGRAPH 2022 Advances, Forbidden West)**: flying *through*
  clouds made the 2D-weather-map model's thin parts read badly up close, so the shapes moved to authored
  voxel clouds (a signed-distance field plus density) with the same noise erosion; and the near clouds got
  more samples than the far ones. We keep the 2015/2017 procedural model (a weather system and smoke will
  drive it at run time; no authored voxels) and take the near-cloud lesson (§9.6).
- **Hillaire, "Physically Based Sky, Atmosphere and Cloud Rendering in Frostbite" (SIGGRAPH 2016 Physically
  Based Shading course).** The same density model (weather texture, height profiles by type, Perlin–Worley
  base and Worley erosion), with energy-conserving integration of the march, a dual-lobe phase, and the
  multiple-scattering approximation (octaves of wider, weaker Beer); what §8.8's lighting already follows.
- **Bauer, "Creating the Atmospheric World of Red Dead Redemption 2: A Complete and Integrated Solution"
  (SIGGRAPH 2019 Advances).** The weather drives everything: cloud types and coverage come from the weather
  system's state, with the same base-and-erosion density, the clouds' shadows and the fog from it; the
  lesson for us is the weather map as the one input a weather sim writes (`weather` in §9.2).
- **Häggström, "Real-time rendering of volumetric clouds" (Master's thesis, Umeå University, 2018).** A
  reimplementation of the Schneider model with its costs measured: the noise bake, the textures' sizes,
  temporal reprojection's ghosting and how a neighbourhood clamp keeps it stable; confirms the 128³/32³
  sizes as plenty and that the light march needs no detail.

What we adopt, by part: the weather map (from our deck noise, so the volume's clouds still sit where the
deck's would and one coverage drives both), cloud types with height profiles and an anvil as data, the
Perlin–Worley base remapped by the cover, remap erosion by Worley detail inverted with height, curl
distortion, wind shear, the light march on the base shape only, blue-noise jitter and a clamped history
(§9.6). Not adopted (yet): authored voxel clouds (Nubis³), adaptive steps, the clouds' shadows on the ground.

### 9.2 The weather map and the cloud types (data)

- **Weather** (`clouds.json` `weather`, `cloud_weather`, twins `weather`, `column_weather`): **regional
  fields**, not the clouds (BUGS.md #100, §9.9; the papers' weather map at 8–16 km). **Cover**: the global
  coverage (the uniform a weather system sets) moved by ± `coverVariation` × 4c(1 − c) (so none and overcast stay
  so) by a one-octave value-noise field with cells `coverScaleM` (8 km) across, then × (1 ± `deckBreakup`) by the
  deck's own stretched noise `n` (1 km cells, warped: breaks the base texture's 2 km repeat); **type** along the
  types' axis `type + typeVariation × (2v − 1) + typeFromCoverage × (regional cover − coverage)`, `v` a second
  field with cells `typeScaleM` (16 km) across (`typeFromCoverage` 0: a cloud's core no longer towers over its
  edges); **precipitation** (thicker, darker bases: each type's `darkening`). Both scales are whole multiples of
  the deck's cells dividing its period (the generator checks), so the fields tile with it. Today type and
  precipitation are the data's; a weather system will write them (§9.7).
- **Cloud types** (`cloudTypes`, an ordered list; `cloud_type_profile` is generated from it, twin
  `type_profile`): each a place on the type axis (`at`), a height profile as two spans in shares of the layer
  (`base`: the fade-in from its floor, short, so a **flat base**; `top`: the fade-out, long, so the noise's
  weaker parts end lower and the **top is rounded**), an `anvil`, a `density` and a rain `darkening`;
  between two types every field is mixed. Shipped (the layer is 1350–4500 m, 3150 m deep):

| Type | at | base | top | anvil | density |
|---|---|---|---|---|---|
| stratus | 0 | 0–0.02 | 0.04–0.10 (≤ 315 m thick) | 0 | 0.6 |
| stratocumulus | 0.3 | 0–0.03 | 0.08–0.20 | 0 | 0.8 |
| cumulus | 0.6 | 0–0.03 | 0.04–0.42 (≤ 1.3 km; the fade from just over the base: flat base, domed top) | 0 | 1.0 |
| cumulonimbus | 1.0 | 0–0.03 | 0.82–0.97 (≈ 3 km) | 0.8 | 1.4 |

  With the shipped weather (`type` 0.42, `typeVariation` 0.3 over 16 km, `typeFromCoverage` 0) the sky is
  stratocumulus to cumulus by region; a cumulonimbus is a region's (a weather system's), not every core's. A new type is a list
  entry (the generator checks the order, spans and names; a test adds a fixture tower by data alone).

### 9.3 The shape

- **The noise, baked once** (`dwell_look::cloud_noise`): the **base** texture `form.baseSize`³ (64³) RGBA8 —
  R a Perlin–Worley (three Perlin octaves 4 cells across dilated by the Worley fBm: `remap(perlin, 0, 1,
  worley, 1)`), G, B, A Worley fBm (⅝, ¼, ⅛ of three octaves) 4, 8 and 16 cells across — repeating every
  `form.baseTileM` (2048 m: Worley and Perlin cells ~512 m, the single clouds' size); the **detail** texture `form.detailSize`³ (32³) — R a Worley fBm 4 cells across,
  G, B the curl across of a Perlin field (the wisps' distortion, sampled every `curlTileM`, 2048 m), A a 32²
  **blue-noise** tile (void-and-cluster; the march's jitter, §9.6) — repeating every `detailTileM` (512 m).
  Every lattice is periodic over its texture, so both tile in all three axes; each tile divides `shape.wrapM`,
  so the shader's wrapped coordinates have no seam. Sampled trilinearly with a repeating sampler
  (`Noise3::sample` is its CPU twin).
- **Base shape** (`cloud_base_shape`, twin `base_shape`; Nubis 2023 p.24): `base = remap(R, −(1 − fbm(G, B,
  A)), 1, 0, 1)`, stretched from `form.baseLow`..`baseHigh` (0.80–0.95: the dilated noise lies in ~0.73–0.94)
  to 0–1, then `saturate(remap(base, 1 − g × cover, 1, 0, 1))` with `g` the column's type's height gradient
  and the cover raised toward a cumulonimbus's top by its anvil (`cover^(1 − ½ anvil)` over the top fade's
  start). So the 3D noise carves the single clouds everywhere (not only their fringe), and the gradient,
  falling from just over the base to the top, raises the cut with height: flat bases, domed tops.
- **Detail** (`cloud_detail_fbm`, `cloud_erode`; twins `ShapeNoise::detail_at`, `erode`): the detail's point
  moved by the curl up to `form.curlM` (90 m, most at the floor) and drifting with the wind's evolve offset;
  its fBm `hf` below `form.wispHeight` (12 % of the layer), `1 − hf` above (wispy bases, billowy tops), takes
  away `remap(d, m × detailErosion, 1, 0, 1)` (`detailErosion` 0.25; HZD's 0.2): only where the base is under 1 — the core stays.
- **Wind and shear:** the layer drifts with the deck's wind (as before); the shape point leans downwind by
  `form.shear` (0.25 m per m up: a tower's top 750 m over), and the detail drifts against the base.
- **The march** (`cloud_layer_march`): a step with no cover or under the profile takes no texture fetch;
  a cloudy step takes the base (one fetch), the detail (two: curl and fBm), the type's profile (one more
  value-noise look-up); the light samples reuse the column's cover (regional: it barely changes along the
  light's path), one base fetch each, **no 2D noise and no detail**; the ambient's cloud above and below comes from the column's profile at no cost
  (`column_cover_m`).

### 9.4 Bake cost and memory

Measured natively (release, one thread, `cargo test -p dwell-look --release -- --ignored bake_cost`):

| Texture | Bake | Memory |
|---|---|---|
| base 64³ (shipped) | 346 ms | 1 MiB |
| base 128³ (Schneider's size; not shipped) | 2.5 s | 8 MiB |
| detail 32³ | 25 ms | 128 KiB |

So the bake runs in the job workers, never on the main thread: when a volume tier first draws (none on Low,
Off or the CPU adapter, so phones and CI bake nothing), the game sends `Job::CloudNoise` slabs of 4 slices
(16 base jobs of ~22 ms, 8 detail jobs) and writes each slab into its texture as it returns; until both are
whole the frame draws the deck (Low), so the volume appears a moment after the world, with no hitch. The WASM
bake is slower than native (an estimate of ~2×: ~45 ms a slab on a worker); 64³ at 64 m a texel over a
4 km tile is plenty for the base shape, which the detail sharpens.

### 9.5 Cost per texel (read from the shader)

§8.7's counting (a 2D value-noise look-up ~45 ALU, a trilinear 3D fetch ~20 ALU of filtering plus its memory):

| Sample | Before (§8.8) | After |
|---|---|---|
| empty step (no cover) | ~180 (warp, weather) | ~270 (+ the two regional fields, one octave each, BUGS.md #100) |
| cloudy step | ~280 + light | ~400 (+ type look-up, profile, 3 fetches, remaps) + light |
| one light sample | ~90 | ~55 (a base fetch; the column's cover reused, BUGS.md #100; was ~145) |
| Medium texel, 10 steps, 4 light (empty – cloudy) | 1.8k – 5.3k | 1.8k – 7.4k |
| High texel, 16 steps, 6 light | 2.9k – 7.6k | 2.9k – 10.5k |
| resolve (§9.6), per texel | — | 9 + 9 loads, ~90 |

At 1280 × 720 that is at most ~0.43 G ALU a frame on Medium (¼) and ~2.4 G on High (½), from above with
the whole screen cloudy; the M1's ~2.6 TFLOPS takes High's worst case in ~1 ms. Near the layer (§9.6) Medium
marches at ½ with 14 steps (~2.3 G at worst) and High at ½ with 24 (~3.6 G). The GPU timings at §8.3's views,
at a mountain in the layer and flying through the layer are for a local `npm run bench -- --warm`, before and
after; tune by the data (`tiers`, `form.baseSize`, the profiles) if High over the M1's budget.

### 9.6 Dither at cloud level (BUGS.md #90)

The owner: "The clouds also look really badly dithered when they overlap? I think mostly when you get to their
level? Looks very pixelated." The cause: near the clouds the eye's move over their distance (the parallax)
handed every texel to its raw jittered march (§8.6's rule), so the march's dither showed at ¼ resolution,
its interleaved-gradient jitter clumping. Now:

- **Blue-noise jitter** (`cloud_jitter`: the detail texture's blue-noise tile, `dwell_cloud_blue`, turned
  by the golden ratio each frame): neighbouring texels' steps far apart, the error high-frequency.
- **A resolve pass** (`clouds_resolve_fs`, twins `neighbourhood_box`, `resolve`): the march writes its raw
  result; the resolve reprojects the history as before but **clamps** it into the box (mean ±
  `HISTORY_CLAMP` 1.5 deviations) of this frame's 3 × 3 neighbours whose scene matched the texel's (so the
  box doesn't mix a peak with the sky), and blends by the tier's `historyBlend` — never dropping the history
  for parallax.
- **More near the layer** (`tiers.*.nearSteps`, `nearDivisor`, `volume.nearM` 600 m; `near_layer` with a
  band so hovering at the edge doesn't swap buffers): Medium ¼ → ½ and 10 → 14 steps, High 16 → 24 steps.
- **The upsample** stays the depth-aware one of §8.9 (bilinear shares × each texel's match to the pixel's
  scene), which keeps a silhouette sharp.
- **No cloud from behind a near pixel** (BUGS.md #97, `dwell_cloud_upsample_split`, `upsample_cover`):
  where none of a pixel's four texels matched its scene (leaves with sky in every texel), the fallback's
  small weights were normalised to the whole cloud of the texels' marches, which ran past the leaves to the
  sky: the cloud behind the tree drew over it, in the texel grid's and the gaps' pattern under the jitter.
  A fallback texel whose first cloud (the second target's z) is past the pixel's scene (beyond its band,
  soft to twice it) now lends its weight with no cloud. Matched texels are unchanged.

### 9.7 Checks and what's left

Tests (`crates/dwell-look/src/clouds.rs` `shape_tests`, `cloud_noise.rs`): a cumulus is flat-based (near its
widest just over its base) and rounded at its top (narrowing to under half near it); a cumulonimbus's anvil is
wider than its trunk and towers where a cumulus has ended; coverage 0 is no cloud; the detail only takes away
and a core (base 1) keeps its density, wispy at the base and billowy at the top; the noise tiles seamlessly
in every axis and the shape repeats a tile over; a fixture cloud type by data alone shapes the clouds and the
generated shader, which validates with naga; both faces; the near tier from the data; the blue noise spreads
its values and the resolve is stable under the jitter as the eye moves (variance under a quarter of the raw
march's), clamps stale history and the upsample keeps a silhouette sharp. `clouds.test.mjs`: the new entries'
checks. `dwell-client`: the codec carries the new job, the shaders' wiring.

`pyramid_tests` (BUGS.md #100, §9.9): over fixture patches on both faces the cloud tops' median slope is
under 2 m per m (main: 4.1 and 2.6), no cloud is taller than its type allows, footprints are 300–1500 m,
the columns tile with the wrap, a cloud behind 400 m of cloud is ≤ 15 % visible (9 %), and at noon the
patch's clouds are fair-weather white (median base 0.77 of the sunlit white; main 0.44).

Left: the in-game look (sun high, low sun, from above, night, flying through the layer) and the GPU cost
(local bench, §9.5); the weather map's type and precipitation as run-time uniforms for a weather system; the
clouds' shadows on the ground; adaptive steps; plumes taking the base noise for their billows.

### 9.8 The low sun's tint and the view's opacity (2026-11-12, WORK_QUEUE item 13)

The owner: "they don't look pretty and tinted during sunrise and sunset like our old 2D clouds", and on
opacity: "light should still go through the clouds, but what is looking bad is the clouds behind the one in
front and you usually can't see other separate clouds blocked behind ones in front of you".

**What the deck tints at a low sun, and what the volume did.** Every term of the 2D deck (`dwell_cloud_layer`)
that warms a cloud at a low sun:

| Term | The deck | The volume before | Now |
|---|---|---|---|
| The sun's colour by elevation | `light.sunlit` × the hue of the day/night's `sunLight` (`#ff9a5c` at sunrise) | the same, the lit colour | the same |
| The golden-hour ramp | `low` = `light.sunWarmth` × (1 − smoothstep(0.02, 0.4, sun's sine)): all of it up to ~1°, none from ~24° | the same ramp, on the lit colour only | one function for both (`cloud_low_sun`, `low_sun`) |
| The warm tint | the **whole** colour toward `light.sunset` × mix(0.75, 1, lit) by `low` × lit: shaded sides warm at 0.75 | only the sun's light toward `sunset`; the shade (the sky's light and the ground's bounce) untinted | the shade too, toward `sunset` × 0.75 (the deck's shaded side) by `low` × `volume.sunsetShade` (1.5, the mix capped at 1) |
| The sky's colour | the shade colour × half the hue of the hemisphere sky (`hemiSky`, a cool lavender at sunrise) | the same | the same, then warmed as above |
| The horizon colour by distance | `mix(col, sky behind, 0.85 × smoothstep(0, deck.fadeEndM, distance))` | the same expression, by the first cloud's distance | one function for both (`cloud_far_haze`, `far_haze`): far clouds fade into the horizon and the low sun's band |
| Tone mapping | both composite in display colour, the same path | — | no change needed |

So the tint was lost in the ambient, not in the sun's colour or the tone mapping: a volume sample's light is
mostly the sky's (the CPU twin: at 3° the sun reaches a lit top through ~1.5 km of cloud, so the sky's light,
cool lavender, is ~70 % of it), where the deck's is 50–90 % the sun's hue. The lit side read warm in hue but
grey: its warmth (red over blue) 1.38 against the deck's 1.73 at a lit 0.7 side; its base 1.14, near white.
Now (`day_shading`, the shader's `cloud_shading`): the lit side 1.69 (within 8 % of the deck's), the base 1.51
(peach-pink); at noon the ramp is 0 and the shading is exactly what it was.

**The view's opacity, apart from the light's.** Before, the light march's Beer took `volume.extinction` too
(`tau = light.absorption × volume.extinction × od`), so a more opaque view would also have darkened the light
through the cloud. Now: `volume.extinction` (0.03 /m, was 0.006) is the view ray's alone, `volume.lightExtinction`
(0.006, the old value) the light's path (lit edges, the silver lining, the glow: unchanged), and
`volume.plumeExtinction` (0.006) the plumes' view (unchanged). For the owner's A/B, `?cloudExtinction=0.006`
(or `0.05`) overrides the view's live, through the clouds pass's uniform (`CloudPrev.tan.z`; 0 the data's),
and `__dwell.cloudExtinction(x)` changes it mid-run.

**What it does (CPU twin).** An eroded mid cloud (density 0.25) at High's and the near tier's ~300 m steps:
at 0.006 a ray takes 7 samples through a 2 km bank and still sees ~4 % of what is behind (a cloud behind 400 m
of it shows at 55 %); at 0.03 and at 0.05 two samples end the march (that cloud behind at 5 % and 0.7 %). A dense core at
Medium's ~900 m steps was one sample before and is still. A thin eroded edge (density 0.05 over 50 m) still
lets 93 % through, so the edges stay soft. **Cost** (§9.5's counting, ~1 k ALU a cloudy Medium sample with its
4 light samples): a cloudy texel that met mid-density cloud ends up to 5 samples sooner, up to ~−5 k of
Medium's 7.4 k worst case and of High's 10.5 k (about −40 % to −65 %); empty texels are unchanged. Measured cost (`gpu.clouds`) is
for the local bench, before and after.

Tests (`volume_tests`): at a sun 3° up (from the day/night data) the volume's lit side has the deck's tint
within 8 % and its base warms (both failed with the old shading: red → green, the lit side 20 % short); noon
is unchanged and differs from sunrise; `sunWarmth` 0 is no tint; the haze and the ramp are one function each
on the deck and the volume; the light's Beer takes `lightExtinction` and the view the run-time value (the
data's by default), the plumes theirs; a sample's light doesn't move with the view's opacity; the early exit
at 0.006 / 0.03 / 0.05; both faces (the tint from the sun's face-local up part). `clouds.test.mjs`: the new
entries' ranges.

Left (local, no GPU in the cloud session): the look at sunrise and sunset against `?clouds=` the deck, and the
A/B of `?cloudExtinction=0.006` / `0.03` / `0.05` with `gpu.clouds`; tune by `volume.sunsetShade`,
`light.sunWarmth` and `volume.extinction`.

### 9.9 The pyramids and the gloom (BUGS.md #100, WORK_QUEUE item 13, 2026-11-12)

The owner: the clouds "look like crazy triangles", "3 sided pyramids", and "gloomy and dark like not-quite storm
clouds". The gap analysis against Schneider 2015 (Horizon Zero Dawn), Nubis³ 2023 and Hillaire 2016 is
[`reference/cloud-papers.md`](reference/cloud-papers.md). Each cloud was a heightfield of the deck's 2D value
noise standing upright: the type rose with how far a column was past the coverage's cut (`typeFromCoverage`
0.9), so each core towered toward cumulonimbus over its edges; the cover was a hard 2D cut, so in the core
`remap(base · g, 1 − cover, 1) × cover` was `base · g` and the 3D noise carved nothing; the view extinction
0.03 /m made that surface a hard shell. The same cores, lit as cumulonimbus (denser, three times as deep, the
sky's light dimmed by ~1.5 km of cloud above), were the gloom. The fix follows the papers in their order:

1. **Type from its own regional field** (16 km cells, smooth; `typeFromCoverage` 0), as the papers' weather
   map's type channel (HZD s.40–45, Nubis p.16/19): no cloud's core is a tower of its own.
2. **Coverage regional and the clouds cut by the 3D noise** (Nubis p.24's `saturate(noise − (1 − profile ×
   coverage))` in remap form; Frostbite p.33: the noise "give[s] a base shape to clouds"): the cover a gentle
   8 km field (0.2–0.6 at coverage 0.42), the base noise windowed to 0–1, the base tile 2 km (clouds
   ~300–1000 m across), the profile fading from just over the base (flat bases, domed tops), erosion 0.25.
3. **View extinction 0.015 /m** (was 0.03; `?cloudExtinction=` still A/Bs it): soft edges, and a cloud behind
   400 m of cloud is still ~9 % visible in the median over the fixture (the owner's "hide the ones behind").
4. **Not done:** distance-adaptive steps with HZD's cheap/full switch and step back (s.76–83, Nubis p.40); the
   even 16 steps on High still slice far clouds (§9.7).

Lighting against the papers (no change needed for the gloom): Beer with a multiple-scattering floor
`max(e^−τ, 0.5 e^−τ/4)` is Wrenninge's two octaves with a = 0.5, b = 0.25 (Frostbite §5.8 wants a ≤ b for energy
conservation, so ours errs bright, not dark); the view (0.015) is now 2.3× the light's (0.006 × absorption
1.1), was 4.5×; the sky's ambient dims by e every `ambientDepthM` 400 m of cloud above (Nubis's
`pow(1 − profile, 0.5)` dims by the profile instead), which only matters under deep clouds; `darkening` acts
only with precipitation (0). At noon a cumulus's sunlit top is ≥ 1.0 of the sunlit white and its base 0.57
(stratocumulus 0.83, cumulonimbus 0.23): the papers' look of bright tops and light-grey bases, only the storm
cloud dark (`at_noon_fair_weather_clouds_are_white_and_only_storm_clouds_dark`).

## 10. The deck's lighting, then and now (2026-11-12, WORK_QUEUE item 13)

Owner: "our new 2D clouds seem to have lost most of their lighting from the original too … They almost looked
3D." The deck as it was before the volumetric work (`76e4e772`, the parent of `93fd6c26`) against today's, term by
term. The shading code (`dwell_cloud_layer`) is the same function: the lighting rework (`857f4a86`) left the
deck alone and the golden-hour change (`5c27c068`) only moved its low-sun ramp and far haze into shared
functions (`cloud_low_sun`, `cloud_far_haze`, the same numbers). What changed is **which deck players see**:
before, the Medium, High and Ultra presets drew the deck at the setting `high` (5 octaves and the self-shadow
sample) and only phones drew `low`; since the volume, the deck in the sky is only Low (phones, the Mobile preset,
`?clouds=low`, and every tier while the volume's noise bakes), which never took the sample. So the deck people
compared was the old phone deck.

| Term | Old desktop deck (`high`, before 93fd6c26) | Deck until this change (Low) | Now |
|---|---|---|---|
| Relief / normal from the noise gradient | none (the relief came from the sample below) | none | none: the sample does it |
| Self-shadow sample toward the sun | yes: `d'` `shadowStepM` (320 m) toward the light's horizontal part, all 5 octaves | **no** | yes on every tier: Low at `lowShadowOctaves` 2, Medium and High (reflections, Snell's window) at `shadowOctaves` 5 |
| Thickness darkening (undersides) | `exp(−1.1 × (0.35·d + d'))`: a core's own density counted ~1.35× | `exp(−1.1 × 0.6·d)`: cores ~0.52 lit, grey-white | as old (Low's cores 0.26 lit at noon) |
| Rim / silver lining toward the sun | `silverLining` × (view·light)^`silverPower` × (1 − d)² | same | same |
| Ambient / sky colour | `shade` × half the day sky's hue; `sunlit` × the sun's | same | same |
| Edge softness | `edgeSoftness` 0.16, contrast 1.9 | same | same |
| Sunset ramp | `sunWarmth` up to ~24°, the lit part toward `sunset`, shade at 0.75 of it | same (shared with the volume since 5c27c068) | same |
| Far haze | 0.85 toward the clear sky by the fade end | same (shared) | same |
| Octaves | 5 | 3 (`lowOctaves`) | 3 on Low (detail, not relief: kept for the phones' cost) |
| From above | the sample skipped, `topLight` | same | same |

**Restored.** The self-shadow sample is data: `light.shadowOctaves` (Medium and High) and
`light.lowShadowOctaves` (Low), each at most its tier's octaves (the generator checks; `cloud_cfg.z` carries the
count, 0 none; the reflections take the sky's count when their tier's shadow bit is set). Low's sample is
coarser than the shape (2 of 3 octaves): the big shapes cast the shading, the fine ones don't matter to it.

**Cost (value-noise evaluations a deck pixel, each four hashes and a quintic blend).** Low: 7 → 9 (the warp's
two 2-octave fields, 3 shape octaves, +2 for the sample; +29 %). The old desktop deck was 14. Medium and High
draw the deck only in reflections and Snell's window, where the reflection tier's shadow bit already gates the
sample: 9 → 14 there when it is set.

**CPU reference** (`cargo run -p dwell-look --release --example deck_lighting -- <dir>`): an 8 × 8-cell patch
(~8 km) from below, straight up, at a 70° and a 5° sun, written as PNGs, with `deck_relief`'s measures (mean
light share of sun-facing edges, far edges, cores with density ≥ 0.85, thin edges ≤ 0.35):

| Deck | Sun | Noises | Sun side | Far side | Relief | Cores | Thin |
|---|---|---|---|---|---|---|---|
| Low before | 70° | 7 | 0.734 | 0.723 | +0.010 | 0.521 | 0.926 |
| Low before | 5° | 7 | 0.734 | 0.723 | +0.010 | 0.521 | 0.926 |
| Old desktop (= Medium/High now) | 70° | 14 | 0.687 | 0.427 | +0.260 | 0.286 | 0.780 |
| Old desktop | 5° | 14 | 0.672 | 0.517 | +0.155 | 0.382 | 0.733 |
| Low restored | 70° | 9 | 0.611 | 0.412 | +0.199 | 0.264 | 0.733 |
| Low restored | 5° | 9 | 0.659 | 0.450 | +0.209 | 0.350 | 0.708 |

In the images the old and restored decks have warm, bright edges on the sun's side and greyer interiors and far
edges at 5°, and blue-grey cores under white rims at noon; Low before is one flat tone (white at noon, flat
orange at 5°). Before, Low's light share was a function of the density alone, so the two sides measured the same.

Tests (`deck_relief_tests`, both faces, face-local parity): Low's sun-facing edges at least 0.12 brighter than
its far edges at 5°, its cores at most 0.4 lit and 0.3 below its thin edges at 70° (both red before: +0.010, cores
0.521); every tier that draws the deck takes the sample and Low keeps ≥ 70 % of the old relief; the sample off
by data is flat again. `clouds.test.mjs`: the new entries' bounds.

Left (local, a GPU): the deck in game at `?clouds=low` against the old screenshots (sun high, low sun, from
above, night) and `gpu.composite` on Low before and after; tune by `lowShadowOctaves` and `shadowStepM`.
