# Dwell — The Day/Night Cycle

The sky, light and fog follow the time of day: a 30-minute day of 20 minutes daylight and 10 of
night, driven by the server's world time so every player sees the same sky. Phase 32 of
[`IMPLEMENTATION_PLAN.md`](./IMPLEMENTATION_PLAN.md); decision record
[ADR 0036](./adr/0036-day-night-cycle.md); architecture in [`ARCHITECTURE.md`](./ARCHITECTURE.md)
§6.3 (Bifacial world: light and sky) and §6.6 (Day/night cycle).

## 1. Brief and decisions

**Owner, 2026-10-15:** "a day/night cycle and improve the transitions between day and night,
including fog, so it looks good. Make the sun and moon look better too."

Known decisions (owner):

- The day is **30 minutes: 20 of day, 10 of night** (first recorded in
  [`TREE_GROWTH.md`](./TREE_GROWTH.md)). Tree growth counts in-game days, so the day length is **one
  shared data value** both use.
- Fog matters: it must look right at every time of day.

Defaults taken where the owner had not decided (open questions in §8):

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
| `fogDensity` | Multiplies the fog setting's density (denser at dawn and dusk) |
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
