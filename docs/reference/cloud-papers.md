# Volumetric clouds vs the papers: gap analysis for BUGS.md #100 ("crazy triangles" / "3-sided pyramids")

2026-11-12. Research only. No repo edits, builds or browser runs. One numeric check was a Python
re-implementation of the CPU twin's weather and base-shape maths (`scratchpad/tools/pyr.py`).

## Sources read

- **[HZD15]** Schneider & Vos, "The Real-time Volumetric Cloudscapes of Horizon Zero Dawn", SIGGRAPH 2015
  Advances. https://www.advances.realtimerendering.com/s2015/The%20Real-time%20Volumetric%20Cloudscapes%20of%20Horizon%20-%20Zero%20Dawn%20-%20ARTR.pdf
  (I extracted the speaker notes with PDFKit and rendered slides 34–35 as images. Below, "s.N" is the slide number printed on the page.)
- **[NUBIS23]** Schneider, "Nubis Cubed" (Nubis³), SIGGRAPH 2023 Advances.
  https://advances.realtimerendering.com/s2023/Nubis%20Cubed%20(Advances%202023).pdf (cited by PDF page, "p.N").
  The first ~50 pages recap "Nubis, Evolved" (2022), which describes the vertical-profile method we follow.
- **[FB16]** Hillaire, "Physically Based Sky, Atmosphere and Cloud Rendering in Frostbite", SIGGRAPH 2016 PBS
  course notes. https://media.contentapi.ea.com/content/dam/eacom/frostbite/files/s2016_pbs_frostbite_sky_clouds.pdf
  (cited by section and printed page).
- Formulas from **GPU Pro 7, "Real-Time Volumetric Cloudscapes"** (Schneider 2016, pp. 97–128) are quoted from
  memory, because the 2015 slides show them only as pictures. They are marked *(GP7, from memory)*. Verify them before quoting further.

## 1. The recipe, per part

### Noise
- **[HZD15 s.29–33]** The base shape uses a 3D texture of **128³ RGBA**: R is a "Perlin-Worley" (inverted Worley fBm used "as an
  offset to dilate Perlin noise", which keeps Perlin's connectedness and adds billows), and G, B, A are Worley at
  increasing frequencies. The detail uses a **32³ RGB** texture of Worley at increasing frequencies. The curl noise is a **128² 2D** texture
  ("non divergent … to fake fluid motion"). The notes say: "keep the resolutions as small as possible."
- **[NUBIS23 p.22]** The same base texture is still in use in 2022, at 128³, 4 channels, 2 bytes per texel ("Low Freq Perlin-Worley / Low,
  Med, High Freq Worley").
- **[NUBIS23 p.96]** For the voxel clouds, the noise is sampled with **mip by distance**:
  `mip = log2(1 + abs(distance * cVoxelFineDetailMipMapDistanceScale)); noise.SampleLOD(pos * 0.01, mip)`.
- **[FB16 §5.4 p.34]** The low-frequency noise is Perlin-Worley plus Perlin octaves, and the high-frequency noise is Worley octaves. Frostbite packs each into a
  single channel for bandwidth, "still giving the same final visual result".
- *(GP7, from memory)* Low-frequency fBm is `R·…; fbm = G·0.625 + B·0.25 + A·0.125`. The detail fBm uses the same weights.
- **Ours** (`crates/dwell-look/src/cloud_noise.rs`): the base is **64³** RGBA8. R is a gradient-Perlin fBm, 4 cells across, dilated by a Worley fBm.
  G, B and A are Worley fBm at 4, 8 and 16 cells, weighted ⅝ ¼ ⅛. It tiles every **4096 m**, which makes 64 m texels, Worley cells of 1024, 512 and
  256 m, and no base structure smaller than ~256 m. The detail is **32³** Worley fBm tiling every 512 m (16 m texels). The curl is
  stored in the detail texture's G and B. The sampler is **Linear/Linear**, repeating (`crates/dwell-client/src/render/mod.rs:202-207`), and samples
  at LOD 0 with no mips.
  **The recipe matches, except that the base is half resolution and its features are as large as the weather cells.**

### Density
- **[HZD15 s.34–37]** The notes give the order: "sample 3dTexture1 & multiply this by the height signal to build
  low resolution cloud", then "erode and multiply the result by a coverage signal (0 to 1) and reduce density at
  the base". Three height-gradient presets (stratus, cumulus, cumulonimbus) are blended by cloud type.
  *(GP7, from memory)*:
  `base = remap(PW, -(1-fbm), 1, 0, 1); base *= heightGradient(type, h);`
  `base = remap(base, coverage, 1, 0, 1) * coverage;` (usually `1-coverage`)
  `hf = mix(hfbm, 1-hfbm, saturate(h*10)); final = remap(base, hf*0.2, 1, 0, 1)`.
- **[HZD15 s.37]** The detail erodes "at the edges", and the notes advise to "invert the Worley noise at the base of the clouds" for
  wisps. The detail noise is distorted by the 2D curl.
- **[HZD15 s.40–45]** Coverage and type are **weather-map channels** (R coverage, G precipitation, B type), produced by a weather
  simulation or painted. "From 15 000 m … we start transitioning to cumulus clouds at around 50% coverage", and precipitation
  "transitions the map … to cumulonimbus clouds at 70% coverage". Type is a **regional** field. It is not tied to each cloud.
- **[NUBIS23 p.16, p.19, p.24]** The NDF (2D) fields span **16 km × 16 km**: coverage, cloud min/max height, bottom and top type. Then
  `dimensional_profile = vertical_profile * cloud_coverage` and `cloud_density = saturate(noise - (1.0 - dimensional_profile))`.
  This means the 3D noise subtracts from the profile everywhere: it carves the whole cloud, not only the low-density fringe.
- **[FB16 §5.3.1 p.33]** The weather texture is painted by artists (R density, G type index). A small **type texture** is indexed by
  type × normalised height and holds the density profile and the erosion amount. The low-frequency noise "give[s] a base shape to clouds and break[s]
  down the repeatability of the weather texture".
- **Ours** (`crates/dwell-look/src/clouds.rs`):
  - The weather comes from the 2D deck noise. `cloud_vol_n` (l.1597) is **2 octaves of value noise** (2D, quintic fade, a rotated
    second octave), scaled by `scaleM` **1024 m**, warped by 0.3 cells and contrast-stretched ×1.9 and clamped.
  - `cover = smoothstep(cut, cut + 0.3, n)` (`cloud_cover` l.1621, `weather` l.656).
  - `kind = type + typeFromCoverage·(n − cut) + typeVariation·(2v−1)` (**l.666 CPU, l.1632 WGSL**). `typeFromCoverage` is
    0.9 (`shared/look/clouds.json` l.92).
  - `type_profile` (l.612) mixes the per-type `base`/`top` spans **linearly** in kind. The cumulus top span is [0.15, 0.42] and the cumulonimbus top span is
    [0.82, 0.97].
  - `height_gradient` (l.637) is a smoothstep fade-in times a smoothstep fade-out.
  - `base_shape` (l.686, WGSL `cloud_base_shape` l.1647) is HZD's form: `remap(base·g, 1−cover, 1)·cover` (l.694 / l.1657).
  - `erode` uses `detailErosion` 0.4 (HZD: 0.2) and inverts above `wispHeight` 0.12 (HZD: ×10, so the same).
  - The curl reaches up to 90 m, most of it at the base.

### Sampling
- **[HZD15 s.76–81, s.90]** The march takes 64 potential samples overhead and 128 at the horizon. It uses **cheap samples** (base only) at a larger step
  until it hits the base iso-surface, then **steps back** and switches to full samples. It falls back to cheap samples after several zero-density samples, exits early
  at alpha 1, and uses 6 cone light samples plus 1 far sample, switching the light samples to cheap at alpha 0.3.
- **[HZD15 s.92–93]** The march draws 1 pixel in 16 per frame (a ¼-res buffer of 4×4 blocks) and reprojects the rest, which makes it about 10× faster. Cost: ~2 ms on PS4.
- **[NUBIS23 p.40]** `step = 3 + 60·distance/16384` m, which is 3 m near and ~63 m at 16 km.
  **[NUBIS23 p.163]** Voxel clouds use an adaptive step of `max(1, sqrt(dist)·0.08)` with an SDF skip.
- **[FB16 §5.5.2–5.5.3]** 4 shadow samples at growing distances, jittered samples, and an exponential moving average with reprojection.
- **Ours:** High uses **16 steps** spread evenly over the slab crossing (`dt = (t1−t0)/steps`, l.1788). That is ~200 m per step looking straight up
  and up to ~1.4 km per step near the horizon, where the reach is 22 km. Near the layer, High uses 24 steps. The march jitters with blue noise and uses temporal history, and has
  6 growing light samples. It has **no cheap/full switch and no step back**, and its step size does not depend on distance.

### Lighting
- **[HZD15 s.52–69]** Beer, then Henyey–Greenstein, then the "powdered sugar" term (depth-dependent in-scatter, view-dependent),
  with rain clouds darkened by more absorption. *(GP7, from memory)* `E = 2·e^{−d}·(1 − e^{−2d})`.
- **[NUBIS23 p.29, p.32]** Direct scattering is `T·HG(primary) + MS·HG(secondary)`. Ambient is
  `pow(1 − dimensional_profile, 0.5)`.
- **[FB16 §5.7–5.8 p.40–41]** A dual-lobe HG blended by a weight. Multiple scattering follows Wrenninge's octaves: σs·aⁿ, σe·bⁿ, p(θ·cⁿ),
  with **a ≤ b** for energy conservation. Ambient comes from SH, uses a 0→1 height gradient, and can be biased for bounce.
- **Ours:** Beer with a floor of `max(e^{−τ}, 0.5·e^{−τ/4})` (a two-octave MS), a powder term, dual HG, and sky-from-above plus
  bounce-from-below ambient. **This matches the papers. Lighting is not a triangle cause**, except through the view extinction (below).

## 2. What makes the "3-sided pyramids"

**Cause 1: each cloud is a heightfield of the 2D value noise, extruded upward.** The 3D noise is too coarse to break it up.

1. In each column, `n` from the 2-octave value noise sets both `cover` and `kind`. The cloud's top is where
   `base·g(y) = 1 − cover`, and `g` fades out over `type_profile(kind).top`. That span rises **linearly** with `kind`, which
   rises **linearly** with `n` at a slope of 0.9 (l.666 / l.1632). The core is pushed up to cumulonimbus tops (0.82–0.97 of the
   3150 m layer), and the edges stay at stratocumulus tops.
2. `n` crosses the whole range from `cut` to `cut + 0.3` within a few hundred metres. The cause is the ×1.9 contrast stretch on a 1024 m value-noise cell.
3. So each cloud's top surface is `H(x,z) = f(n(x,z))`: a mountain shaped like the value noise. Value noise has a flat
   plateau at each lattice point (the quintic fade has zero slope there) and near-planar ramps between lattice points. Its contours are rounded
   diamonds and triangles, skewed by the rotated second octave (`next_octave`). The spire's cross-section shrinks with height, which gives
   a pyramid whose faces are those ramps. Where the warp and the rotated octave shut one quadrant, 3 faces show.
4. The base Perlin–Worley cannot break this up. Its smallest structure is ~256 m and its main features are 1024 m, the same scale as the weather
   cell. In the core, `cover` is about 1, so `remap(base·g, 1−cover, 1) = base·g` and the 3D noise only **dims** the density; it carves
   nothing. The detail erosion (0.4) acts only where the density is below 1, so it frays the faces without changing them.

**Numbers from the twin's maths** (base held at 0.75, coverage 0.42, shipped data, `tools/pyr.py`):
- The tallest cloud is **2.5 km tall on a ~550 m wide footprint**: its top is 2512 m at the centre, 1662 m at 200 m out, and 0 m at 280–360 m out.
- The cloud-top slope is a **median of 6–7.6 m of rise per metre across**, and the 90th percentile is 14–20.
- An ASCII map of the top heights shows isolated cone spires with planar flanks and diagonal ridges, following the lattice and the rotated octave.
- With `typeFromCoverage` set to 0.3, the maximum top is 1284 m and the median slope 3.9. At 0, they are 1063 m and 2.9. Real cumulus are about as wide as they are tall.
- The remaining slope at 0 comes from **cover**: a hard 2D cut that goes from 0 to 1 over 0.3 of `n`, a few hundred metres. The edge is still a heightfield cone,
  but a lower one.

**Contrast with the papers:** coverage and type are **regional fields**: a weather simulation or a painted map at
16 km scale ([HZD15 s.40–45], [NUBIS23 p.16]). The **3D noise** defines the individual clouds ([FB16 p.33]: the noise "give[s] a
base shape … and break[s] down the repeatability of the weather texture"). Nubis subtracts the noise from the profile everywhere
(`saturate(noise − (1 − profile))`, [NUBIS23 p.24]). In our model the 2D deck noise is the cloud and the 3D noise is texture
on it. The per-cloud "core towers over its edges" rule (`typeFromCoverage`, CLOUDS.md §9.2) appears in none of the three sources.

**Cause 2: high view extinction exposes the smooth iso-surface.** At `extinction` 0.03 /m and a density of ~0.5, the mean free path is
~65 m. The spire's surface becomes an opaque shell with sharp silhouettes, so its planar faces read like geometry. Before #790 the
same field looked softer. The physical value is plausible; the field it reveals is the problem.

**Cause 3 (secondary): undersampling.** 16 even steps make ~200 m slices overhead and ~1 km or more toward the horizon, across 300–500 m wide
spires. Jitter and history average this out, but the slices add straight-edged stepping when the camera moves.

**Ruled out:**
- Nearest-texel sampling: the sampler is Linear (`render/mod.rs:206-207`).
- Value noise in 3D: the baked base is gradient Perlin plus Worley. Value noise exists only in the 2D weather, where it does matter (above).

## 3. Fixes ranked by visual impact and cost (M1 Air, High)

| # | Fix | Paper | Impact | Cost |
|---|-----|-------|--------|------|
| 1 | **Stop tying cloud type to each cloud's noise.** Set `typeFromCoverage` to 0 (or ≤ 0.2) and draw the type from a separate **low-frequency** field (≥ 8–16 km). Spires go from 2.5 km to ~1 km and slopes from ~7 to ~3. Data-only first A/B. | HZD s.40–45, NUBIS23 p.16/19 | Highest | Zero (data) |
| 2 | **Make coverage regional and let the 3D noise cut the clouds.** Sample the weather `n` at a much larger scale (e.g. `wq/8`, ~8 km cells) so `cover` is a gentle 0.3–0.7 field, not a cookie cutter. Then `remap(base·g, 1−cover, 1)` or Nubis's `saturate(noise − (1 − g·cover))` carves the clouds out of the Perlin–Worley. Cloud size then follows `baseTileM`: shrink it to ~2–3 km so Worley cells are ~500–750 m. | HZD s.35–37, NUBIS23 p.24, FB16 p.33 | Highest (removes the heightfield entirely) | Low: a few lines in `cloud_weather`/`cloud_cover` and their twins, plus tests. The light samples get cheaper because `cover` no longer needs the deck noise at full detail. |
| 3 | **Base noise at 128³** (Schneider's size), or at least keep 64³ while shrinking the tile to match #2. | HZD s.31, NUBIS23 p.22 | Medium (finer billows) | Bake 2.5 s instead of 346 ms (CLOUDS.md §9.4), 8 MiB. GPU cost is the same. |
| 4 | **A/B `?cloudExtinction=0.01–0.015`** to confirm the hard shells come from #790. Keep 0.03 once #1 and #2 give the field real structure, or settle between. | FB16 §2 (σt sets softness) | Medium (diagnostic) | Zero |
| 5 | **Distance-adaptive steps plus a cheap/full switch with a step back**: base-only samples at long steps, stepping back and switching to full samples on a hit, back to cheap after N empty samples, and steps that grow with distance (`3 + 60·d/16384`). This buys more samples where clouds are. | HZD s.77–83, NUBIS23 p.40 | Medium (removes slicing) | Medium. Shader restructure; roughly cost-neutral with the same sample budget. |
| 6 | Erosion 0.4 → 0.2–0.3, with the curl kept. Optionally Nubis's density sharpening `pow(d, lerp(0.3, 0.6, …))`. | GP7, NUBIS23 p.118 | Low–medium | Zero |
| 7 | Mip-mapped 3D noise sampled by distance (far shimmer, not facets). | NUBIS23 p.96 | Low | Low (mips at bake time) |
| 8 | Smoother 2D weather noise (Perlin/gradient or a baked 2D Perlin-Worley weather texture instead of value noise). This matters only if the weather stays at cloud scale, and #2 makes it moot. | — | Low after #2 | Low |

**Suggested order:** #1 and #4 as a data-only A/B now (`typeFromCoverage: 0`, `?cloudExtinction=0.012`), then #2 as the real fix, with a
CPU-twin test: on a fixture, the maximum cloud-top slope stays under ~2 and the top heightfield's correlation with the deck `n` stays below a bound.
That is the pyramid's regression test, and it fails on main at a median slope of ~7.
