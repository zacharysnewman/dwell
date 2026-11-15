# Screen-space ambient occlusion

**Status:** built (ADR [0080](./adr/0080-ssao.md)). Owner, 2026-11-15: "I'd like to implement screen space
ambient occlusion." Tracked in `docs/WORK_QUEUE.md`.

## 1. The owner's answers

| Question | Answer |
| --- | --- |
| Which presets? | **High and Ultra**: a cheap half-resolution pass on High (within ~1 ms), a fuller one on Ultra; off on Low, Mobile and Medium, phones and the CPU adapter tier. |
| What does it darken? | **Everything, soft**: contact shadows where characters, trees, grass, props and built blocks meet the ground and each other; subtle for the storybook look, not doubling the baked per-vertex corner shading. |
| What is tunable, and where? | **Data + Settings toggle**: radius, strength, samples and resolution per tier in a look data file (`shared/look/ssao.json`, through the look generators); an on/off (with its tier) and a strength in Settings → Graphics, saved like other settings (append-only store). |

What gets added later is tuning (data) and maybe a tier; the estimator and the passes are the fixed
engine. The data is per build; the setting per player (this browser).

## 2. What it does

The world is lit by the sun or moon (direct, shadow-mapped), the hemisphere light (sky above, ground
below, shaded by the baked corner occlusion × sky visibility) and the block light (flood-filled). The
baked occlusion only knows blocks at mesh time: nothing darkens where a villager stands, under a tree's
leaves, round grass and props, or where a placed piece meets a wall at a ½ m step it doesn't see.
SSAO adds that from the frame's own depth, on the **hemisphere and block light only** — never the sun's
or a torch's direct light, so a sunlit field stays bright and the shade gains its contact shadows.

## 3. How

1. **The share** (`render/shaders.rs`, `lambert_lit`): as a fragment is lit, `frame_ao` = the share of
   its irradiance from the hemisphere and block light (by luminance) × (1 − `baked` × (1 − ambient)),
   ambient being the baked hemisphere level: where the baked corner shading already darkens a vertex, the
   SSAO keeps less (`baked` 0.4). `frame_depth` / `self_lit_depth` store it in the view depth's bits 2–4
   (8 steps; the ink and self-lit flags keep bits 0–1). Anything not lit through `lambert_lit` (flames,
   self-lit pixels) stores 0; water writes no view depth; the sky has none.
2. **The occlusion** (`render/ssao.rs`, `ssao_fs`) at the tier's share of the drawing buffer: the
   texel's view-space point from the view depth (sample 0), the normal from its neighbours' depths
   (the nearer side each axis), and a spiral of `samples` points within `radius` m (capped at
   `maxPixels` across), turned per texel by interleaved gradient noise. Each sample above the texel's
   plane occludes by its cosine, less with distance, none past the radius (the alchemy / scalable
   ambient obscurance family). Faded in over `nearFrom`–`nearTo` m (your arms and the lens take none) and
   out over `farFrom`–`farTo` m; × `strength` × the Settings strength.
3. **Blur**: across, then down, `blurRadius` Gaussian taps each way, each weighted by how close its
   depth is to the texel's (`blurSharpness`), so it doesn't bleed over silhouettes.
4. **Apply** (`ssao_apply_fs`, full size): the buffer's four texels round the pixel, bilinear × depth
   similarity, and the scene's resolved colour multiplied (a blend) by 1 − (1 − AO) × share, before the
   clouds, shafts, bloom and composite read it. Being a multiply of the linear lit colour by the
   ambient share, it equals scaling only the ambient term: albedo × (D + A) × (1 − (1 − ao) × A/(D + A))
   = albedo × (D + ao × A).

Both faces: everything is view space; nothing depends on which way is up. The composite already binds
the 16 sampled textures a stage may, so the apply is its own pass rather than a composite read.

## 4. Tiers and settings

| Tier | Preset | Buffer | Samples |
| --- | --- | --- | --- |
| Off | Low, Mobile, Medium | — | — |
| Low | High | ½ per axis | 8 |
| High | Ultra | full | 12 |

A CPU adapter and the phone tier never draw it (`ssao_for`). Settings → Graphics → Lighting: **Ambient
occlusion** (Off / Low / High; preset field `ssao`) and **Ambient occlusion strength** (0–200 %), stored
in the append-only `dwell.ssao` record (`quality`, `strength`). `?ssao=off|low|high` overrides the tier;
the bench's `view` step takes `ssao` and `ssaoStrength`. GPU time: `gpu.ssao`.

## 5. Cost

M1 MacBook Air, Chrome, the High preset (render scale ½, 2560 × 1600 canvas), the starter's broadleaf forest,
unthrottled, 4 s each (`gpu.frame` p50): off 14.8 ms, Low 15.7 ms (+0.9 ms), High 16.7 ms (+1.9 ms). The
`gpu.ssao` span itself reads 4.8–6.5 ms because the M1's pass timestamps overlap (DEV_TOOLS.md); the
frame's difference is the cost. High keeps 60 fps with Low on.

## 6. Known limits

- Under water the floor's share applies to the water's pixel (water writes no view depth): a crevice on
  the floor darkens the water over it a little. Acceptable for shallow water; revisit if it shows.
- Screen-space: what is off screen or hidden occludes nothing (edges of the view lose a little).
- The half-resolution tier can miss thin grass blades' own occlusion; the ground round them still darkens.
