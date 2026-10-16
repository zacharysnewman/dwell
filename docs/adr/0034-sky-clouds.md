# 0034. Sky clouds: a 2D cloud deck drawn in the sky, its tunables as data

- Status: Proposed (built; the owner's review of the look and of the defaults in `CLOUDS.md` §1 is
  outstanding)
- Date: 2026-10-15
- Builds on: [0029](0029-ray-marched-renderer.md) step A1 (the shared frame: the sky is drawn in the
  composite pass), [0023](0023-bifacial-world.md) (one sky sphere, per-face horizons)
- Design: [`CLOUDS.md`](../CLOUDS.md)

## Context

The owner asked for "procedural clouds in the skybox (not 3D)" (2026-10-15). The sky is a function
of the view direction drawn per pixel in the composite pass (`dwell-look` `sky_wgsl`). Clouds are
new, so CLAUDE.md's design questions weren't put to the owner first: defaults were taken and are
recorded in `CLOUDS.md` §1 with the open questions.

## Options considered

- **A skybox texture or a cloud dome mesh**: an asset, no perspective from the eye's movement, and
  not procedural.
- **Volumetric ray-marched clouds**: 3D, which the owner excluded, and costly on phones.
- **A 2D deck in the sky shader** (chosen): each sky pixel's ray meets a thin, sphere-curved deck at
  a fixed altitude, and the hit's world position drives a domain-warped fBm. It has true perspective
  and parallax as the eye flies, costs one pass's worth of ALU over the sky's pixels only, and needs
  no assets.
- **Noise from a texture**: cheaper ALU but an asset to generate and sample. Procedural value noise
  was measured at about 0.9 ms of composite time on High at 1280 × 720, which is acceptable, and has
  no tiling seams by construction.

## Decision

- The clouds are a function in `dwell-look` (`clouds.rs`, `cloud_wgsl`), called by the renderer's
  composite where the sky shows. The terrain's fog mixes toward the clear sky.
- A periodic noise lattice (with a lattice-preserving octave transform) and offsets wrapped in f64,
  so the shader stays f32-precise anywhere in the 8,000 km world.
- The tunables are data: `shared/look/clouds.json` → `clouds.gen.rs` by `shared/look/clouds.mjs`
  (validated, tested, CI stale check). Coverage is a single run-time parameter for a future weather
  system.
- The light comes from the renderer's frame (the sky frame, the lights' hues), so the clouds follow
  a day/night cycle without changes of their own.
- A Clouds quality setting (Off / Low / High, `dwell.clouds`), High on desktops and Low on touch
  devices.
- Client-side visuals only. The drift clock is the server's tick as the client sees it, so players
  on a server roughly agree.

## Consequences

- One more WGSL module in the renderer and about 0.7–0.9 ms of GPU time on an Apple GPU at 720p
  (less where terrain covers the sky). It can be turned off.
- Clouds aren't visible from above the deck. Clouds from above, cloud shadows and weather are
  follow-ups for the owner to decide (`CLOUDS.md` §6).
- Reversing it means removing `clouds.rs`, the call in `composite_fs`, two frame uniforms and the
  setting. The setting's stored key is then ignored (the store is append-only).
