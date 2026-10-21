# Sky clouds

Phase 31, [ADR 0034](./adr/0034-sky-clouds.md). The owner's brief (2026-10-15): "procedural clouds in
the skybox (not 3D)". This is a 2D cloud layer drawn as part of the sky. It isn't volumetric and
has no geometry. Architecture: `ARCHITECTURE.md` §6.6 (Clouds) and §5 (`dwell-look`, `render/`,
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

- **The deck.** The deck is a thin layer `altitudeM` (1.8 km) above the viewer's face's sea level. It
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
  renderer's `cloud_lighting` reads the frame's `DaySky` block for the viewer's face:
  - The light is the sun or the moon, whichever is over the viewer's face.
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
