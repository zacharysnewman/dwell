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
| What gets added later? | More tunables in the data, a weather system driving the coverage, and maybe more cloud layers (one deck now). |
| Who adds them, and with what? | A developer edits `shared/look/clouds.json` (validated, generated, CI stale check). A weather system would call `setCloudCoverage` / `Renderer::set_cloud_coverage`. |
| What stays fixed in code? | The deck model (a sphere-curved plane), the noise (value-noise fBm with a domain warp), the lighting terms, the octave transform. |
| What is data? | Altitude, curvature, fades, cell size, wrap, octaves (High and Low), gain, warp, coverage, edge softness, opacity, wind and evolve speeds and directions, the colour tints, absorption, the shadow step, the silver lining, the sunset warmth. |
| Per build, per world or per player? | Tunables per build. The Clouds quality setting per browser. Coverage per frame (a uniform, for weather). |

## 2. The model

- **The deck.** The deck is a thin layer `altitudeM` (1.8 km) above the viewer's face's sea level. It
  lies on a sphere of radius `curvatureM` (120 km) centred that far below the eye. A sky pixel's view
  ray `d` meets it at distance `t = c / (r·y + √(r²y² + c))`, with `c = 2rh + h²`, `y = d·up` and `h`
  the deck's height above the eye. This form doesn't cancel: `t = h` straight up and `√(2rh + h²)`
  ≈ 21 km at the horizon. Below the horizon the clouds are hidden. They fade in over `horizonFade`,
  fade out between `fadeStartM` and `fadeEndM` along the ray, and fade as the eye rises within
  `ceilingFadeM` of the deck (none above it).
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
- **Light.** It comes from the renderer's frame, so the clouds follow whatever sets the sky frame
  and the lights (a day/night cycle included):
  - The light is the sun or the moon, whichever is over the viewer's face.
  - The data's `sunlit`, `shade`, `nightLit` and `nightShade` colours tint the hues of the sun's,
    the day sky's ambient, the moon's and the night sky's ambient lights.
  - Day or night follows the sky behind the pixel: the face's dayness times the direction's.
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
scenarios can set it, and the sky rotation too (`settings.clouds`, `settings.skyRotation`).

## 4. Cost

Measured with `npm run bench -- clouds-high clouds-low clouds-off --unthrottled` at a 1280 × 720
window on an Apple GPU (Metal), looking 25° up (mostly sky):

| Clouds | `gpu.composite` p50 | mean | `gpu.frame` p50 |
|---|---|---|---|
| Off | 0.67 ms | 0.95 ms | 2.04 ms |
| Low | 1.34 ms | 1.61 ms | 2.63 ms |
| High | 1.59 ms | 1.88 ms | 2.81 ms |

Clouds cost about 0.9 ms of composite time on High and 0.7 ms on Low at this size, and scale with
the sky's share of the screen (covered pixels skip them). Phones default to Low. Not yet measured
on a phone.

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

## 6. Open questions for the owner

1. **The look.** Is this the right style? It is soft, stylised, painterly flat cumulus with warm lit
   edges and cool shade. Should it be puffier, flatter, more or less coverage, faster wind?
2. **Clouds from above.** Flying through the deck makes the clouds fade out, with nothing above it.
   Should the deck be visible from above (looking down at it over the terrain), or rise with the
   camera?
3. **Weather.** Should a weather system drive coverage (and later colour), per world and
   synchronised? The hook (`setCloudCoverage`) exists, but nothing calls it.
4. **Shadows.** Should cloud shadows on the terrain come later? They are out of scope now.
5. **Synchronisation.** The drift runs on the server's tick, so players on one server see roughly the
   same sky. Is that enough, or should the pattern be tied to a world seed?
6. **The data's location.** These tunables sit beside the motion effects' `shared/look/motion.json`
   (another branch), each with its own generator. Should they be folded into one look generator?
