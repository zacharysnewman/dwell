# Motion effects: speed FOV and wind (Phase 29)

**Status:** built; the owner's look in a browser outstanding (`IMPLEMENTATION_PLAN.md` Phase 29). Owner's brief (2026-10-14): "add
sprint wind effects and dynamic FOV adjustment (for a 'fast moving' effect). Should be using shaders
for VFX."

The owner was not asked CLAUDE.md's "ask before designing" questions before this was built; the
defaults below were chosen by the coordinating agent and are recorded here with the open questions
at the end.

## Decisions (defaults, 2026-10-14)

| Question | Default chosen |
| --- | --- |
| What gets added later? | More tuning of the same effects (strengths, rates, colours, counts), per controller state; possibly new effect layers (radial blur once the shared frame lands). |
| Who adds it, with what? | Developers, by editing `shared/look/motion.json` (a tool could edit it: it is a flat validated data file). Players only choose on/off and the FOV boost's strength. |
| Fixed in code | The effect vocabulary: a speed → FOV curve (eased walk→run, then log-scaled to a full speed), a critically damped spring, the caps, the streak shader's shape. |
| Data | Every threshold, gain, rate, cap, per-state gain, streak count/length/width/opacity/colour, radii, vignette and flow speed. |
| Per build / world / player | Tunables per build (checked in); the toggle and strength per player (this browser's settings). |

## Inputs

- The **camera's actual velocity**: the predicted player's velocity (`ClientState.velocity`, flight
  included), so knockback and falls count, not raw input. The controller **state** (Running,
  Sliding, Falling, Flying, …) multiplies the targets by a per-state gain from the data (climbing 0,
  swimming 0.4, crouching 0.5, the rest 1).
- The effects **read** state each drawn frame and never write simulation state: prediction stays
  deterministic.
- Walk and run speeds come from the controller config (`movement.walk_speed`, `run_speed`), not
  copies: the curves are expressed relative to them.

## Dynamic FOV

- `target_fov_boost(speed, state)`: 0 at walk speed, eased (smoothstep) to `fov.sprintBoostDeg` at
  run speed, then up to `fov.flightBoostDeg` at `fov.fullMps` on a log scale (flight spans 11 m/s to
  thousands of km/s).
- Smoothed by a **critically damped spring** (exact closed-form step, no overshoot): `widenRate`
  toward a wider target, `settleRate` back (fast to widen, slower to settle).
- `effective_fov(base, boost, aspect)`: the screen's unboosted vertical FOV (75°, capped at 100°
  horizontal) plus the boost, capped at `fov.maxHorizontalDeg` horizontal and `fov.maxVerticalDeg`
  vertical, never narrower than the base. **The cap is raised to 118° for the boost** (from the base
  view's 100°): a 16:9 screen is already at 100° horizontal unboosted, so with the old cap the boost
  would do nothing on most desktops. 118° keeps the distortion a transient effect only at speed.
- The game computes the frame's effective FOV once and hands the **same value** to the renderer
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
  copies), sharing the pixel's view ray and sky colour (the sky is the composite's costly part).

**Cost** (this Mac's GPU, `fly-low-fast --unthrottled`, effects off against on in one build): the
composite pass's GPU time p50 1.13 → 1.37 ms (mean 1.39 → 1.60 ms); frame interval unchanged
within noise (p50 7.8 vs 7.2 ms).

## Settings

"Speed effects" (on/off) and "FOV boost" (0–100 %) in the settings menu's Look group, stored under
their own key, default on (off when the browser prefers reduced motion). Append-only: unknown
fields in the stored record are kept on rewrite.

## Tunables

`shared/look/motion.json` → `node shared/look/gen.mjs` → `crates/dwell-look/src/motion.gen.rs`
(`MOTION`), validated (unknown, missing, out of range: an error naming the entry; every controller
state needs a gain), tested by `node --test shared/look/gen.test.mjs`, stale output failing CI.

## Open questions for the owner

1. Strengths: +10° at sprint and +18° at full flight speed, and the 118° horizontal cap — too much,
   too little?
2. Should the face-B turn-over reset the effects (current) or hold them through the somersault?
3. Should walking forward (5 m/s) show any effect, or only sprinting and faster (current: starts at
   walk speed for the FOV, half-way to run speed for the wind)?
4. Wind colour and density: white-blue streaks at the edges; any wish for tinting by environment
   (dust, water, altitude)?
5. The edge radial blur (in flight only): keep, stronger, or off?
6. Should a remote player's or a vehicle's speed matter later (only the camera's now)?
