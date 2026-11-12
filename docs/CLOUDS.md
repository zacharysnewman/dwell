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
  - Self-shadow (High): `lit = exp(−absorption × (0.35·d + d'))`, with `d'` the density
    `shadowStepM` toward the light. Low uses `0.6·d` alone.
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
- **Density** (`dwell_cloud_density`): the deck's own noise is the weather map (the same noise point,
  wind and coverage, so the volume's clouds sit where the deck's would, and a weather system drives
  both with one coverage); for the march's budget it takes a one-octave warp and two octaves. The
  coverage's cut rises with height (`topFloor`), so each cloud narrows into a rounded top over a
  flat base and the densest clouds are the tallest. 3D value noise of `detailCellM` cells erodes it
  (`detailErosion`; it tiles with the deck: `scaleM` is a whole multiple of it).
- **Extra sources: plumes,** at run time (§8.7): vertical columns (steam over lava, a volcano's ash,
  a smoke stack) the game adds, moves and removes, marched apart from the layer. `clouds.json`'s
  `plumes` are the list's defaults (empty as shipped).
- **Light.** The sun or the moon, whichever is over the face, as the deck. At each step with cloud,
  `lightSteps` samples toward the light (without the detail) give the optical depth; Beer's law over
  it, a powder term (`powder`) darkening thin unlit edges, mixed between the deck's shade and lit
  colours (`ambient` the floor). The low sun's warmth, the silver lining and the far haze toward the
  clear sky are the deck's.
- **Inside.** Flying into a cloud, the march starts at the eye, so the view fogs by the density
  around it. The deck's CPU in-cloud fog (`insideFog`) is turned off on a volume tier
  (`cloud_uniforms`' third vector's x is 0) so the two don't stack.
- **What keeps the 2D deck.** The volume replaces only the eye's deck in the composite. Snell's window
  (`sky_above_with_clouds`, WATER_RENDERING.md §16), the water's reflections and the other face's
  deck below the horizon keep the deck: the cheap path. A test pins that only the clouds pass marches
  and that `sky_above_with_clouds` neither marches nor samples the volume.

### 8.2 Tiers

`tiers` gives each Clouds setting value its march: steps (0: the 2D deck), light samples per step,
the resolution divisor of the clouds pass (§8.6) and the share of each new frame in the accumulated
picture (`historyBlend`):

| Setting | Steps | Light samples | Resolution | Blend | Who |
|---|---|---|---|---|---|
| High | 16 | 1 | ½ | 0.2 | opt-in (the Ultra preset) |
| Medium | 10 | 0 (the step's own density) | ¼ | 0.2 | desktops' default (and the Medium and High presets) |
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
  (`rgba16float`), and the distance to the first cloud (`r16float`, 65000 for none). It marches to
  the layer's end, not to the scene, so its history never holds the terrain's edges.
- **Accumulation.** The step offset turns each frame (interleaved gradient noise plus a golden-ratio
  step over 16 frames). Each texel blends its new march with the last frame's buffer, reprojected by
  direction through the last frame's camera (`CloudPrev`: right, up, forward, tangents). Clouds are
  far, so direction is enough; where they are near, the eye's move over the cloud's distance hands
  the texel over to the new frame. The history starts over with new buffers, the first frame, or a
  move over `CLOUD_HISTORY_JUMP_M` (200 m) in a frame (`cloud_prev_block`, tested). The pass then
  copies its buffer into the history for the next frame.
- **Depth-aware upsample.** The composite filters the buffer at the pixel (`upsampled_clouds`) and
  hides the clouds where the scene is nearer than the nearest of the four texels' first cloud
  (`textureGather`), so no cloud bleeds over a mountain in front of it. A cloud with terrain inside
  it is drawn whole over that terrain: the march doesn't stop at the scene.
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

