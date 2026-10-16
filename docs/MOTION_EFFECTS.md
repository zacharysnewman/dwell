# Motion effects: speed FOV and wind (Phase 29)

**Status:** built, the owner's answers (2026-10-15) applied; the owner's look in a browser outstanding (`IMPLEMENTATION_PLAN.md` Phase 29). Owner's brief (2026-10-14): "add
sprint wind effects and dynamic FOV adjustment (for a 'fast moving' effect). Should be using shaders
for VFX."

The owner was not asked CLAUDE.md's "ask before designing" questions before this was built; the
defaults below were chosen by the coordinating agent and are recorded here with the questions the
owner then answered (2026-10-15, at the end).

## Decisions (defaults, 2026-10-14)

| Question | Default chosen |
| --- | --- |
| What gets added later? | More tuning of the same effects (strengths, rates, colours, counts), per controller state; possibly new effect layers (radial blur once the shared frame lands). |
| Who adds it, with what? | Developers, by editing `shared/look/motion.json` (a tool could edit it: it is a flat validated data file). Players only choose on/off and the FOV boost's strength. |
| Fixed in code | The effect vocabulary: a speed → FOV curve (eased walk→run, then log-scaled to a full speed), the zoom formula, a critically damped spring, the streak shader's shape. |
| Data | Every threshold, boost percentage, rate, per-state gain, streak count/length/width/opacity/colour, radii, vignette, flow speed and blur. |
| Per build / world / player | Tunables per build (checked in); the Field of view setting, the toggle and the boost's strength per player (this browser's settings). |

## Inputs

- The **camera's actual velocity**: the predicted player's velocity (`ClientState.velocity`, flight
  included), so knockback and falls count, not raw input. The controller **state** (Running,
  Sliding, Falling, Flying, …) multiplies the targets by a per-state gain from the data (climbing 0,
  swimming 0.4, crouching 0.5, the rest 1).
- The effects **read** state each drawn frame and never write simulation state: prediction stays
  deterministic.
- Walk and run speeds come from the controller config (`movement.walk_speed`, `run_speed`), not
  copies: the curves are expressed relative to them.

## Field of view setting (owner, 2026-10-15)

"Field of view" in the settings menu's Look group: the vertical FOV in degrees, default **75°**
(the former constant), 60–100° in 1° steps, stored in the append-only `dwell.view` record
(`{ fovDeg }`; unknown fields kept on rewrite). It is the **base** everywhere: the projection, the
LOD's pixel error and frustum, each pass's culling (the block-target ray goes through the screen
centre, which no FOV moves).

The wide-screen rule is derived from the setting by one function, `dwell_lod::base_fov(setting,
aspect)` (the renderer's `camera.rs` re-exports it): the old rule was "75° vertical, but at most
100° horizontal", which bites beyond the aspect `A = tan(50°) / tan(37.5°) ≈ 1.553`. The same `A`
holds for every setting: up to `A` the vertical FOV is the setting; wider screens keep the
horizontal FOV the setting has at `A` — `base = 2·atan(tan(setting / 2) · A / aspect)`. At the
default this is the old rule exactly (100° horizontal on wide screens); at 100° the horizontal cap
is ≈ 123°, at 60° ≈ 84°.

## Dynamic FOV

- `target_fov_boost(speed, state)` (a percentage): 0 at walk speed, eased (smoothstep) to
  `fov.sprintBoostPct` (10 %) at run speed, then up to `fov.flightBoostPct` (20 %) at `fov.fullMps`
  on a log scale (flight spans 11 m/s to thousands of km/s).
- Smoothed by a **critically damped spring** (exact closed-form step, no overshoot): `widenRate`
  toward a wider target, `settleRate` back (fast to widen, slower to settle).
- `effective_fov(base, boost)`: the boost is a **zoom-out** of the base, applied to the view's
  half-height tangent as a camera zoom is: **`effective = 2·atan(tan(base / 2) · (1 + boost))`**
  (owner, 2026-10-15). It feels the same at any base and tapers at wide ones: at 75° about 80.3°
  sprinting and 85.3° at top flight speed; at 60° 64.8° and 69.4°; at 100° 105.3° and 110.1°. The
  result is clamped to 120° vertical as a safety net. No other caps (the earlier 118°/100° boosted
  caps are gone): a wide base gets a proportionate boost.
- The game computes the frame's effective FOV once (`base_fov` of the setting at the drawing's
  aspect, then `effective_fov`) and hands the **same value** to the renderer
  (projection, the sky's rays, each pass's culling) and to the LOD selection (frustum and the
  pixel-error / screen-space size). The block-target ray goes through the screen centre, which the
  FOV does not move.
- **Resets** (to no boost, no wind): a teleport (the eye moved more than `teleportM` beyond what its
  velocity explains), death and respawn (the camera is not the player's eyes), a tool camera, the
  face-B turn-over (the somersault across the midplane), turning the effects off.
- **Menu open**: nothing changes (the spring and the animation are held).

## Wind (shader)

Part of the shared frame's composite pass (Phase 25 A1; `render/shaders.rs` `MOTION`, uniforms
`frame.wind_*`), applied over the composited picture only while the wind is above zero:

- Thin soft streaks radiating from the **vanishing point of the velocity** (its direction projected
  into the frame's view, so strafing or falling moves it; moving away flips the flow inward), one per
  angular cell around it with hashed jitter, width, speed and on/off per cycle — procedural, no
  textures.
- Concentrated toward the screen edges (faded in between `innerRadius` and `outerRadius` from the
  screen centre), so the crosshair area stays clear; faded out near the vanishing point.
- Intensity from speed above a threshold (`wind.start` of the way from walk to run speed),
  `sprintIntensity` at run speed, 1 at `fullMps` (log scale), smoothed by its own spring; a second,
  denser, faster and thinner layer fades in above sprint intensity (fast flight).
- A subtle edge vignette scaled by the intensity.
- A **radial blur** of the composited scene toward the vanishing point at the edges (`wind.blur` ×
  intensity², so it is a flight effect): four taps jittered per pixel (smooth rather than ghosted
  copies), sharing the pixel's view ray and its sky with the clouds (the sky is the composite's
  costly part; over a cloudless sky of their own, the clouds faded past the blur's inner radius at
  a hard circular edge: the owner's 2026-10-16 report).

**Cost** (this Mac's GPU, `fly-low-fast --unthrottled`, effects off against on in one build): the
composite pass's GPU time p50 1.13 → 1.37 ms (mean 1.39 → 1.60 ms); frame interval unchanged
within noise (p50 7.8 vs 7.2 ms).

## Settings

"Field of view" (60–100°, above) in the Look group; "Speed effects" (on/off) and "FOV boost"
(0–100 % of the data's boost) in the Motion group, stored under `dwell.motion`, default on (off
when the browser prefers reduced motion). Both records are append-only: unknown fields in a stored
record are kept on rewrite.

## Tunables

`shared/look/motion.json` → `node shared/look/gen.mjs` → `crates/dwell-look/src/motion.gen.rs`
(`MOTION`), validated (unknown, missing, out of range: an error naming the entry; every controller
state needs a gain), tested by `node --test shared/look/gen.test.mjs`, stale output failing CI.

## Decided (owner deferred to the recommendation, 2026-10-15)

1. **Strengths:** a zoom-out of +10 % at a sprint and +20 % at top flight speed (above), with a
   Field of view setting as the base.
2. **Face-B turn-over:** the effects keep resetting on it — a zoom during the somersault would be
   disorienting.
3. **Walking speed:** no effect (the FOV boost starts at walk speed, the wind half-way to run).
4. **Streak tinting:** none for now. Follow-up: underwater gets its own treatment with the water
   work (Phase 26, W4).
5. **Edge blur:** kept, as subtle as it is.
6. **Speed sources:** the camera's actual velocity, whatever moves it (as now).
