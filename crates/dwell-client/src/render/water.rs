//! Fluid looks on the GPU (docs/WATER_RENDERING.md W1b, ADR 0030): the fluid table packed into the
//! storage buffer the water shader indexes by each vertex's fluid id, the water quality setting,
//! and the shader's maths in Rust — Schlick's Fresnel, the normal-detail layers and how each fades
//! out where a pixel covers more than its wavelength (the slope it loses widens the glint instead),
//! and the inverse of the composite's tone mapping (the sky's colour back to the scene's linear
//! light, for its reflection). The layer table and the tone-mapping matrices are emitted into the
//! WGSL from here (`wgsl`), so the shader and these tested twins share their numbers.

use dwell_blocks::FluidInfo;
use dwell_look::water_tiers::{WATER_TIERS, WaterTier, tier_bits};
use dwell_look::{hex, srgb_to_linear};

/// vec4s per fluid in the buffer (`Fluid` in the WGSL).
pub const FLUID_VEC4S: usize = 19;
/// Bytes per fluid.
pub const FLUID_STRIDE: u64 = (FLUID_VEC4S * 16) as u64;

/// The normal detail repeats every `WRAP_M` metres (each layer's wavelength is adjusted to divide
/// it), so the shader works on world x and z wrapped to [0, WRAP_M) per draw, exact at any
/// distance from the origin.
pub const WRAP_M: f64 = 1024.0;

/// A draw's origin wrapped for the normal detail (x and z, metres).
pub fn wrapped_origin(origin: [f64; 3]) -> [f32; 2] {
    [
        origin[0].rem_euclid(WRAP_M) as f32,
        origin[2].rem_euclid(WRAP_M) as f32,
    ]
}

/// One scrolling layer of normal detail: gradient noise on a lattice turned by an integer matrix
/// (so it still wraps at `WRAP_M`), scrolled along `dir`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaveLayer {
    /// Its wavelength over the fluid's `waveScale`.
    pub wavelength: f32,
    /// Its slope (height over wavelength) over the fluid's `normalScale`.
    pub slope: f32,
    /// Its scroll speed over the fluid's `flowSpeed`.
    pub speed: f32,
    /// Unit scroll direction (x, z).
    pub dir: [f32; 2],
    /// The lattice's turn: an integer matrix (columns), scaling by `sqrt(det)`.
    pub turn: [[i32; 2]; 2],
}

/// Large, medium and fine ripples (Low draws the first only).
pub const LAYERS: [WaveLayer; 3] = [
    WaveLayer {
        wavelength: 1.0,
        slope: 0.10,
        speed: 1.0,
        dir: [0.9578, 0.2873],
        turn: [[1, 0], [0, 1]],
    },
    WaveLayer {
        wavelength: 0.43,
        slope: 0.08,
        speed: 1.7,
        dir: [-0.5547, 0.8321],
        turn: [[1, -1], [1, 1]],
    },
    WaveLayer {
        wavelength: 0.17,
        slope: 0.06,
        speed: 2.6,
        dir: [0.2425, -0.9701],
        turn: [[2, 1], [-1, 2]],
    },
];

/// A layer is whole while a pixel covers under `FADE_LO` of its wavelength, gone past `FADE_HI`
/// (Nyquist: at least two pixels per wavelength).
pub const FADE_LO: f32 = 0.125;
pub const FADE_HI: f32 = 0.5;

/// A water quality tier (WATER_RENDERING.md §4): an index into the data's tiers (dwell-look
/// `WATER_TIERS`, `shared/look/watertiers.json`), cheapest first, which say what it draws: the
/// scene copy (refraction, absorption, foam, caustics), the ripple layers, the flow streaks and the
/// bank band. The frame uniform carries its switches (`bits`), not its index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaterQuality(usize);

impl Default for WaterQuality {
    /// The desktop default: Medium.
    fn default() -> Self {
        Self::named("medium").expect("watertiers.json has medium")
    }
}

impl WaterQuality {
    /// The tier named `name` (or a removed tier's alias: High reads as Medium).
    pub fn named(name: &str) -> Option<Self> {
        dwell_look::water_tiers::tier_index(name).map(Self)
    }
    /// From the shell's level: the tier's index in the data (past the last, the last).
    pub fn from_level(level: u32) -> Self {
        Self((level as usize).min(WATER_TIERS.len() - 1))
    }
    /// The CPU adapter's tier (opaque: no scene copy).
    pub fn cpu() -> Self {
        Self(dwell_look::water_tiers::cpu_tier())
    }
    /// Its index in the data.
    pub fn index(self) -> usize {
        self.0
    }
    /// Its data.
    pub fn tier(self) -> &'static WaterTier {
        &WATER_TIERS[self.0]
    }
    /// What the frame uniform carries (`water.w`): the tier's switches (dwell-look `tier_bits`).
    pub fn bits(self) -> f32 {
        tier_bits(self.tier()) as f32
    }
    /// Whether the water reads the scene behind it (W3: refraction, depth absorption, foam and
    /// caustics, from a copy of the scene colour). The CPU's tier doesn't, with no copies.
    pub fn reads_scene(self) -> bool {
        self.tier().reads_scene
    }
    /// Whether the caustic tile is drawn for it: the scene-reading tiers' floors, or the blended
    /// pass's lines on the water (`surface_caustics`).
    pub fn draws_caustics(self) -> bool {
        self.tier().reads_scene || self.tier().surface_caustics
    }
    /// Layers of normal detail drawn.
    pub fn layers(self) -> usize {
        (self.tier().ripple_layers as usize).clamp(1, LAYERS.len())
    }
}

/// Fresnel reflectance at normal incidence from the index of refraction (air on the other side).
pub fn f0_from_ior(ior: f32) -> f32 {
    let r = (ior - 1.0) / (ior + 1.0);
    r * r
}

/// Schlick's approximation of the Fresnel reflectance at `cos` (the angle to the normal).
pub fn schlick(cos: f32, f0: f32) -> f32 {
    let m = (1.0 - cos.clamp(0.0, 1.0)).powi(5);
    f0 + (1.0 - f0) * m
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How much of a layer of `wavelength` (m) is drawn where a pixel covers `footprint` (m).
pub fn layer_fade(footprint: f32, wavelength: f32) -> f32 {
    1.0 - smoothstep(wavelength * FADE_LO, wavelength * FADE_HI, footprint)
}

/// The noise tile (render/water_tiles.rs): gradient noise over `NOISE_TILE` lattice cells, wrapping,
/// at `NOISE_TEXELS` texels per cell, so `NOISE_RES` texels square.
pub const NOISE_TILE: i32 = 32;
pub const NOISE_TEXELS: u32 = 16;
pub const NOISE_RES: u32 = NOISE_TILE as u32 * NOISE_TEXELS;

/// The caustic tile: the cell-edge distances over `CAUSTIC_TILE` cells at `CAUSTIC_TEXELS` texels
/// per cell, `CAUSTIC_RES` square, drawn each frame.
pub const CAUSTIC_TILE: i32 = 16;
pub const CAUSTIC_TEXELS: u32 = 32;
pub const CAUSTIC_RES: u32 = CAUSTIC_TILE as u32 * CAUSTIC_TEXELS;

/// Where a pixel covers fewer than this many caustic tile texels, the caustics are evaluated per
/// pixel instead of sampled from the tile (0: always sampled). Sampled at every distance since
/// 2026-11-06: close to the water every floor pixel evaluated two layers of Worley cells, ~4 ms of a
/// 33 ms frame at 2880×1324 (owner: mobile slowed near the water), and magnified the tile (a
/// texel is 1/32 of a cell, ~4 cm for water) draws the same lines.
pub const CAUSTIC_EXACT_TEXELS: f32 = 0.0;

/// How many cells of about `want` metres span `WRAP_M`, rounded to a whole number of `tile`s (a
/// pattern sampled from a tile of `tile` cells then wraps with the position): (cells, cell size).
pub fn tile_cells(want: f32, tile: i32) -> (i32, f32) {
    let tiles = ((WRAP_M as f32) / (want.max(1e-3) * tile as f32))
        .round()
        .max(1.0);
    let n = tiles as i32 * tile;
    (n, WRAP_M as f32 / n as f32)
}

/// A layer's wavelength for a fluid's `wave_scale`, adjusted so a whole number of them spans
/// `WRAP_M` in whole noise tiles (the noise's period in lattice cells, and the wavelength).
pub fn layer_period(layer: &WaveLayer, wave_scale: f32) -> (i32, f32) {
    tile_cells(wave_scale * layer.wavelength, NOISE_TILE)
}

/// The surface's normal from the summed height slopes (dh/dx, dh/dz), toward `up` (±1: the face's
/// up, or −1 seen from below): the normal of y = h(x, z), turned over for −1.
pub fn blend_normal(up: f32, slope: [f32; 2]) -> [f32; 3] {
    let n = [-slope[0], 1.0, -slope[1]];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    [up * n[0] / len, up * n[1] / len, up * n[2] / len]
}

/// The glint's GGX width (α) for a fluid's `roughness` (perceptual) and the slope variance the
/// faded layers no longer draw (Toksvig: what the normals lose, the highlight spreads by).
pub fn glint_alpha(roughness: f32, lost_variance: f32) -> f32 {
    let a = (roughness * roughness).max(MIN_ALPHA);
    (a * a + 2.0 * lost_variance).sqrt()
}

/// The narrowest glint (a sun's disc, roughly; and no division blow-up).
pub const MIN_ALPHA: f32 = 0.05;

fn linear_rgb(c: u32) -> [f32; 3] {
    hex(c).map(|v| srgb_to_linear(v) as f32)
}

/// The fluid table as the storage buffer holds it: `FLUID_VEC4S` vec4s per fluid, indexed by
/// fluid id (`Fluid` in the WGSL). Colours in linear light.
pub fn pack_fluids(fluids: &[FluidInfo]) -> Vec<f32> {
    let mut out = Vec::with_capacity(fluids.len().max(1) * FLUID_VEC4S * 4);
    for f in fluids {
        let s = linear_rgb(f.scatter);
        let foam = linear_rgb(f.foam_color);
        out.extend_from_slice(&[f.absorption[0], f.absorption[1], f.absorption[2], f.opacity]);
        out.extend_from_slice(&[s[0], s[1], s[2], f.roughness]);
        out.extend_from_slice(&[f.ior, f.normal_scale, f.wave_scale, f.flow_speed]);
        out.extend_from_slice(&[foam[0], foam[1], foam[2], f.foam_depth]);
        out.extend_from_slice(&[
            f0_from_ior(f.ior),
            f.caustics,
            f.surface_drop as f32,
            f.scattering,
        ]);
        out.extend_from_slice(&[f.refraction, f.caustic_scale, f.caustic_depth, f.sparkle]);
        // Its flow look by name (shared/look/waterflow.json; none: zeros, no flow shown).
        match dwell_look::waterflow::flow_look(f.name) {
            Some(l) => {
                let c = linear_rgb(l.foam_color);
                out.extend_from_slice(&[
                    l.advect as f32,
                    l.cycle_s as f32,
                    l.foam_from_ms as f32,
                    l.foam_to_ms as f32,
                ]);
                out.extend_from_slice(&[c[0], c[1], c[2], l.foam_strength as f32]);
                out.extend_from_slice(&[
                    l.foam_length_m as f32,
                    l.foam_width_m as f32,
                    l.shore_bands as f32,
                    l.shore_period_s as f32,
                ]);
                out.extend_from_slice(&[
                    l.shore_line_m as f32,
                    l.shore_width_m as f32,
                    l.shore_strength as f32,
                    l.shore_wobble as f32,
                ]);
            }
            None => out.extend_from_slice(&[0.0; 16]),
        }
        // Its underwater look by name (shared/look/underwater.json; none: the default).
        let u = dwell_look::underwater::underwater_look(f.name);
        let tint = hex(u.band_tint);
        out.extend_from_slice(&[
            u.lens_m as f32,
            u.fog_clear_m as f32,
            u.fog_density as f32,
            u.band_depth_m as f32,
        ]);
        out.extend_from_slice(&[
            u.line_amplitude_m as f32,
            u.line_wavelength_m as f32,
            u.line_speed as f32,
            u.underside_texture as f32,
        ]);
        out.extend_from_slice(&[
            tint[0] as f32,
            tint[1] as f32,
            tint[2] as f32,
            u.band_streaks as f32,
        ]);
        out.extend_from_slice(&[
            u.band_rim_px as f32,
            u.band_rim_shade as f32,
            u.band_line_px as f32,
            u.band_line_glow as f32,
        ]);
        out.extend_from_slice(&[
            u.band_glints as f32,
            u.band_breakup as f32,
            u.band_mirror_warp_px as f32,
            u.band_mirror as f32,
        ]);
        out.extend_from_slice(&[
            u.underside_breakup as f32,
            u.underside_sun_patch as f32,
            u.underside_sun_power as f32,
            u.underside_cell_scale as f32,
        ]);
        out.extend_from_slice(&[
            u.window_cells as f32,
            u.window_bodies as f32,
            u.window_sun_glow as f32,
            u.window_sun_power as f32,
        ]);
        // The underwater colour grade (WATER_RENDERING.md §18): the fog's colour (linear) and its
        // share; the in-scatter's brightness, the mirror's, the floor light's absorption share and
        // the window rim's glow.
        let fog = linear_rgb(u.tone_fog_color);
        out.extend_from_slice(&[fog[0], fog[1], fog[2], u.tone_fog_share as f32]);
        out.extend_from_slice(&[
            u.tone_brightness as f32,
            u.tone_mirror as f32,
            u.tone_floor_light as f32,
            u.window_rim_glow as f32,
        ]);
    }
    if out.is_empty() {
        // A storage binding can't be empty: one zero fluid that nothing indexes.
        out.resize(FLUID_VEC4S * 4, 0.0);
    }
    out
}

/// The view ray through screen point `ndc` scaled to view depth 1 (the WGSL `view_ray`): `forward`,
/// `right` and `up` the camera's world axes, `tan` the half-FOV tangents (x, y).
pub fn view_ray(
    forward: [f64; 3],
    right: [f64; 3],
    up: [f64; 3],
    tan: [f64; 2],
    ndc: [f64; 2],
) -> [f64; 3] {
    std::array::from_fn(|k| forward[k] + ndc[0] * tan[0] * right[k] + ndc[1] * tan[1] * up[k])
}

/// The waterline (WATER_RENDERING.md §12; the WGSL `waterline_m` is its twin): how far (m, along
/// the face's up) a pixel's point on the lens plane (`lens_m` ahead of the eye along the camera's
/// forward axis: its view ray scaled to view depth 1, world y `ray_y`, times `lens_m`) from the
/// eye at world y `eye_y` is above the fluid's surface at world y `surface_y`, less the line's
/// wave `wave_m` there. Negative: the pixel looks out from under the surface. The lens points lie
/// on a plane, which the water's plane cuts in a straight line at any eye height and roll; linear
/// in the eye's height, so the line moves smoothly with it.
pub fn waterline_m(
    face: dwell_world::FaceFrame,
    eye_y: f64,
    surface_y: f64,
    ray_y: f64,
    lens_m: f64,
    wave_m: f64,
) -> f64 {
    face.y(eye_y + ray_y * lens_m) - face.y(surface_y) - wave_m
}

/// How many metres of [`waterline_m`] one pixel of the drawing buffer spans across the line (the
/// WGSL `waterline_band`'s `px_m`): `waterline_m` is linear on the screen, its gradient per pixel
/// the lens times `2 tan_y / height` times the length of the camera's up and right axes' world y
/// (the same along x and y, as `tan_x / width = tan_y / height`). The band's rim and line,
/// measured in these, are the same pixel width at any pitch or roll.
pub fn meniscus_px_m(lens_m: f64, tan_y: f64, height_px: f64, up_y: f64, right_y: f64) -> f64 {
    lens_m * 2.0 * tan_y / height_px * up_y.hypot(right_y)
}

/// Where a pixel `d` metres of [`waterline_m`] from the line is in the band (the WGSL
/// `waterline_band`): 0 at its top edge (the line: the surface), 1 at its foot `depth_m` under
/// it; none above the line (the band is the surface's slab under it) or past its foot.
pub fn band_t(d: f64, depth_m: f64) -> Option<f64> {
    (depth_m > 0.0 && d < 0.0 && d > -depth_m).then(|| -d / depth_m)
}

/// The band's edge at a pixel `px` pixels under the line (the WGSL `waterline_band`): the colour's
/// multiplier, `shade` over the dark rim's `rim_px`, then `glow` over the bright line's `line_px`
/// under it, else 1 (a pixel's soft edge between).
pub fn band_edge(px: f64, rim_px: f64, shade: f64, line_px: f64, glow: f64) -> f64 {
    let smooth = |a: f64, b: f64, x: f64| {
        let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let rim = 1.0 - smooth(rim_px - 0.75, rim_px + 0.25, px);
    let line = (1.0 - rim) * (1.0 - smooth(rim_px + line_px - 0.75, rim_px + line_px + 0.25, px));
    (1.0 + (shade - 1.0) * rim) * (1.0 + (glow - 1.0) * line)
}

/// The line's shade across it (the WGSL `composite_pixel` and `waterline_band`), `px` pixels under
/// it (negative: above it): under it [`band_edge`]; above it, within a pixel, the rim's shade by
/// the share of the pixel the line covers, so the rim's top edge is anti-aliased instead of
/// stepping a whole pixel at a time along the wave (§12.2).
pub fn line_shade(px: f64, rim_px: f64, shade: f64, line_px: f64, glow: f64) -> f64 {
    let top = band_edge(px.max(0.0), rim_px, shade, line_px, glow);
    if px >= 0.0 {
        return top;
    }
    1.0 + (top - 1.0) * (1.0 + px).clamp(0.0, 1.0)
}

/// How much of the underside's texture (and its sun patch) reaches an eye `to_surface_m` from
/// the surface along its ray, in a fluid of extinction `extinction` a metre (absorption plus
/// scattering, the mean over rgb) with the look's fog `clear_m` and `density` (the WGSL
/// `composite_underwater`'s mirror, through `underwater`): the fog's transmittance over the
/// distance. Toward level the distance grows without bound and the texture fades into the fog
/// smoothly, with no hard horizon (§12.2).
pub fn underside_contrast(to_surface_m: f64, clear_m: f64, density: f64, extinction: f64) -> f64 {
    (-extinction * density * (to_surface_m - clear_m).max(0.0)).exp()
}

/// The sun seen from under the surface (the WGSL `sun_through`): the direction toward the sun's
/// image through Snell's window, the sun's direction `sun` (world, unit) refracted into a fluid
/// of refractive index `ior` (its slant from the face's up shrunk by `1 / ior`, so a sun on the
/// horizon shows at the critical angle). The underside's sun patch is brightest along it.
pub fn sun_through(f: dwell_world::FaceFrame, sun: [f64; 3], ior: f64) -> [f64; 3] {
    let s = f.vector(sun);
    let h = [s[0] / ior.max(1.0), 0.0, s[2] / ior.max(1.0)];
    let up = (1.0 - h[0] * h[0] - h[2] * h[2]).max(0.0).sqrt();
    f.vector([h[0], up, h[2]])
}

/// The band's texture coordinate (the WGSL `waterline_band`'s `uv`) at `along` screen heights
/// along the line and `t` down the band: a plane receding to the line, so a step on the screen
/// covers more of the texture toward the line (cells shrink toward it, as the surface's do).
pub fn band_uv(along: f64, t: f64) -> [f64; 2] {
    [along * 4.0 / (t + 0.08), 1.0 / (t + 0.08)]
}

/// How many texture units of [`band_uv`] one pixel of the drawing buffer spans at `t` down the
/// band (the WGSL `waterline_band`'s `fp`): x along the line (`along` is in screen heights of
/// `height_px`), y across it (`px_m` metres of `waterline_m` a pixel, [`meniscus_px_m`], over the
/// band's `depth_m`, through `band_uv`'s `1 / (t + 0.08)`). Toward the line the plane recedes and
/// a pixel covers many of the noise's cells.
pub fn band_footprint(t: f64, height_px: f64, px_m: f64, depth_m: f64) -> [f64; 2] {
    let s = t + 0.08;
    [4.0 / height_px / s, px_m / depth_m / (s * s)]
}

/// The band's noise layers (the WGSL `band_ripples`), by their frequency in [`band_uv`]'s units
/// (cycles a unit, x along the line and y across it) for `breakup` `b`: the warp, the streaks, the
/// coming-and-going and the glints (a thresholded noise, so about twice its own frequency).
pub fn band_layers(b: f64) -> [[f64; 2]; 4] {
    [[0.45, 0.3], [1.0 + 1.2 * b, 0.35], [2.6, 0.9], [4.2, 1.6]]
}

/// How much of a band noise layer of frequency `k` (cycles a texture unit) is kept where a pixel
/// spans `fp` units (the WGSL `band_keep`; owner, 2026-11-07: the band "looks pixelated"): all of
/// it under a quarter cycle a pixel, none past Nyquist (half a cycle a pixel), where it would
/// only alias into speckle; faded to its mean between.
pub fn band_keep(k: [f64; 2], fp: [f64; 2]) -> f64 {
    let c = (k[0] * fp[0]).max(k[1] * fp[1]);
    let t = ((c - 0.25) / 0.25).clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

/// Snell's window (the WGSL `sees_out`): whether a ray from under the surface that rises at
/// `rise` (the cosine of its angle from the face's up, after the ripples bend it) leaves the
/// fluid of refractive index `ior` (through the window overhead), or is totally reflected back
/// (rising at a slant flatter than the critical angle, `asin(1 / ior)`: 48.6° from the up for
/// water). Below the surface's plane (rise ≤ 0) it never reaches it.
pub fn sees_out(rise: f64, ior: f64) -> bool {
    rise > 0.0 && 1.0 - rise * rise < 1.0 / (ior * ior)
}

/// Snell's window's critical angle (radians from the surface's normal) in a fluid of refractive
/// index `ior`: `asin(1 / ior)`, 48.6° for water, so the window is a ~97° cone overhead.
pub fn critical_angle(ior: f64) -> f64 {
    (1.0 / ior.max(1.0)).asin()
}

/// The radius (m) of Snell's window on a calm surface `depth_m` above the eye: where the cone of
/// the critical angle meets the surface.
pub fn window_radius(depth_m: f64, ior: f64) -> f64 {
    depth_m.max(0.0) * critical_angle(ior).tan()
}

/// A ray from under the surface bent out into the air (the WGSL `water_exit`; Snell's law), for
/// the calm surface of face `f` (its normal the face's up): `dir` (world, unit) rising inside the
/// window leaves with its slant from the up grown by `ior`, so the whole sky shows inside the
/// window's cone; `None` past the critical angle (totally reflected: the mirror).
pub fn water_exit(f: dwell_world::FaceFrame, dir: [f64; 3], ior: f64) -> Option<[f64; 3]> {
    let d = f.vector(dir);
    if !sees_out(d[1], ior) {
        return None;
    }
    let h = [d[0] * ior, 0.0, d[2] * ior];
    let up = (1.0 - h[0] * h[0] - h[2] * h[2]).max(0.0).sqrt();
    Some(f.vector([h[0], up, h[2]]))
}

/// The sky's light (linear) seen through Snell's window toward the bent-out ray whose sky (the sky
/// pass's, linear) is `sky`, before the fog: the composite's `sky_in` (`composite_underwater`): the
/// sky mixed toward the window's glow (shafts.json `window`: colour `glow_srgb` × the zenith's
/// brightest channel, its share `glow` × (0.55 + 0.45 × `inside`)). With no glow the window shows the
/// sky above unchanged (WATER_RENDERING.md §16).
pub fn window_sky(
    sky: [f64; 3],
    zenith: [f64; 3],
    glow_srgb: [f64; 3],
    glow: f64,
    inside: f64,
) -> [f64; 3] {
    let bright = zenith[0].max(zenith[1]).max(zenith[2]);
    let share = glow * (0.55 + 0.45 * inside);
    std::array::from_fn(|k| sky[k] + (srgb_to_linear(glow_srgb[k]) * bright - sky[k]) * share)
}

/// The length of a view ray inside the water (m): from the surface, at view depth `d_water`, to
/// the scene behind it at `d_behind` (0: nothing drawn there, so as deep as can matter), for a
/// surface point `rel_len` metres from the eye. View depths scale along a ray with distance.
pub fn ray_thickness(d_behind: f32, d_water: f32, rel_len: f32) -> f32 {
    if d_behind <= 0.0 {
        return DEEP_M;
    }
    ((d_behind - d_water).max(0.0) * rel_len / d_water.max(1e-4)).min(DEEP_M)
}

/// Deeper than any absorption can tell apart (m).
pub const DEEP_M: f32 = 1e4;

/// Refraction's foreground check: the offset sample shows the scene behind only when what it hits
/// is behind the water (or nothing); otherwise (a bank, a post in front) the undistorted one.
pub fn refracted_sample(offset_behind: f32, d_water: f32) -> bool {
    offset_behind <= 0.0 || offset_behind > d_water
}

/// Beer–Lambert: what is seen through `length` metres of a fluid with absorption `sigma` and
/// `scattering` (per metre). Along the ray the scene behind keeps exp(−(σ + s)·length) and the
/// fluid's lit body (its in-scatter) fills the rest; the scene was lit by light that came `down`
/// metres through the fluid, which keeps exp(−σ·down) of it.
pub fn through_fluid(
    behind: [f32; 3],
    body: [f32; 3],
    sigma: [f32; 3],
    scattering: f32,
    length: f32,
    down: f32,
) -> [f32; 3] {
    std::array::from_fn(|k| {
        let t = (-(sigma[k] + scattering) * length).exp();
        behind[k] * t * (-sigma[k] * down).exp() + body[k] * (1.0 - t)
    })
}

/// How strongly caustics show at `depth` metres under the surface: whole at the surface, 1/e at
/// the fluid's caustic depth.
pub fn caustic_fade(depth: f32, caustic_depth: f32) -> f32 {
    if depth <= 0.0 {
        return 0.0;
    }
    (-depth / caustic_depth.max(1e-3)).exp()
}

/// The shader's lattice wrap (`wrap_cell`): a cell coordinate in [0, period).
pub fn wrap_cell(c: i32, period: i32) -> i32 {
    ((c % period) + period) % period
}

/// The shader's neighbour step (`wrap_step`): a wrapped coordinate moved by −1, 0 or +1 without
/// another modulo.
pub fn wrap_step(w: i32, o: i32, period: i32) -> i32 {
    let v = w + o;
    if v < 0 {
        v + period
    } else if v >= period {
        v - period
    } else {
        v
    }
}

/// The shader's `hash_dir`: a unit vector from the low two bytes of a hash, without trig.
pub fn hash_dir(h: u32) -> [f32; 2] {
    let v = [(h & 0xff) as f32 - 127.5, ((h >> 8) & 0xff) as f32 - 127.5];
    let len = (v[0] * v[0] + v[1] * v[1]).max(1.0).sqrt();
    [v[0] / len, v[1] / len]
}

/// The caustic pattern's cell size for a fluid's `scale`, adjusted so a whole number of caustic
/// tiles spans `WRAP_M` (it is evaluated on wrapped positions): (cells per wrap, cell size).
pub fn caustic_period(scale: f32) -> (i32, f32) {
    tile_cells(scale, CAUSTIC_TILE)
}

/// three's ACES filmic fit (shaders.rs `aces`): the input and output matrices, by columns.
pub const ACES_INPUT: [[f32; 3]; 3] = [
    [0.59719, 0.07600, 0.02840],
    [0.35458, 0.90834, 0.13383],
    [0.04823, 0.01566, 0.83777],
];
pub const ACES_OUTPUT: [[f32; 3]; 3] = [
    [1.60475, -0.10208, -0.00327],
    [-0.53108, 1.10813, -0.07276],
    [-0.07367, -0.00605, 1.07602],
];
/// The RRT+ODT fit: (v² + a·v − b) / (c·v² + d·v + e).
const FIT: [f32; 5] = [0.0245786, 0.000090537, 0.983729, 0.432951, 0.238081];

fn mat_mul(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[0][r] * v[0] + m[1][r] * v[1] + m[2][r] * v[2])
}

/// A 3×3 matrix's inverse (columns), in f64 for the WGSL constants.
fn mat_inverse(m: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let a = |c: usize, r: usize| f64::from(m[c][r]);
    let cof = |r: usize, c: usize| {
        let (r0, r1) = ((r + 1) % 3, (r + 2) % 3);
        let (c0, c1) = ((c + 1) % 3, (c + 2) % 3);
        a(c0, r0) * a(c1, r1) - a(c1, r0) * a(c0, r1)
    };
    let det: f64 = (0..3).map(|c| a(c, 0) * cof(0, c)).sum();
    // inverse[c][r] (column c, row r) = cofactor(c, r) / det: the adjugate is the transpose.
    std::array::from_fn(|c| std::array::from_fn(|r| (cof(c, r) / det) as f32))
}

/// The tone mapping (`aces` in the WGSL, without the encode): linear scene colour → display
/// linear, for `exposure`.
pub fn aces(color: [f32; 3], exposure: f32) -> [f32; 3] {
    let k = exposure / 0.6;
    let v = mat_mul(&ACES_INPUT, color.map(|c| c * k));
    let [a, b, c, d, e] = FIT;
    let fit = v.map(|v| (v * (v + a) - b) / (v * (c * v + d) + e));
    mat_mul(&ACES_OUTPUT, fit).map(|c| c.clamp(0.0, 1.0))
}

/// The inverse of [`aces`] for colours it doesn't clip: what scene colour shows as `display`
/// (display linear). The sky is drawn in display colour; its reflection needs it in scene light.
pub fn inverse_aces(display: [f32; 3], exposure: f32) -> [f32; 3] {
    let [a, b, c, d, e] = FIT;
    let y = mat_mul(&mat_inverse(&ACES_OUTPUT), display).map(|y| y.clamp(0.0, INVERSE_FIT_MAX));
    // (1 − c·y)·v² + (a − d·y)·v − (b + e·y) = 0, the positive root.
    let v = y.map(|y| {
        let qa = 1.0 - c * y;
        let qb = a - d * y;
        let qc = -(b + e * y);
        (-qb + (qb * qb - 4.0 * qa * qc).sqrt()) / (2.0 * qa)
    });
    mat_mul(&mat_inverse(&ACES_INPUT), v).map(|x| (x * 0.6 / exposure).max(0.0))
}

/// The water's cost-isolation switches (a dev tool, `?waterdbg=`; docs/WATER_RENDERING.md §8): each
/// name turns one piece of the water's shading off, so its GPU time (`gpu.*.water`) can be told
/// apart. Carried to the shader as a bit mask in the frame uniform (`wrap.z`); 0 draws everything.
pub const DEBUG_SWITCHES: [(&str, u32); 14] = [
    ("ripple0", 1),
    ("ripple1", 2),
    ("ripple2", 4),
    ("glint", 8),
    ("sky", 16),
    ("refraction", 32),
    ("absorption", 64),
    ("caustics", 128),
    ("foam", 256),
    ("sparkle", 512),
    ("under", 1024),
    ("flat", 2048),
    ("ssr", 4096),
    ("clouds", 8192),
];

/// The mask for a `?waterdbg=` value: names (`DEBUG_SWITCHES`) separated by commas; `all` sets
/// every switch but `flat`. Unknown names are ignored.
pub fn debug_mask(names: &str) -> u32 {
    names
        .split(',')
        .map(str::trim)
        .map(|n| {
            if n == "all" {
                DEBUG_SWITCHES
                    .iter()
                    .filter(|(k, _)| *k != "flat")
                    .fold(0, |m, (_, b)| m | b)
            } else {
                DEBUG_SWITCHES
                    .iter()
                    .find(|(k, _)| *k == n)
                    .map_or(0, |(_, b)| *b)
            }
        })
        .fold(0, |m, b| m | b)
}

/// The fit's output nears 1 as its input grows without bound: inverting stops short of it.
const INVERSE_FIT_MAX: f32 = 0.995;

fn wgsl_mat(m: &[[f32; 3]; 3]) -> String {
    let col = |c: &[f32; 3]| format!("vec3f({:?}, {:?}, {:?})", c[0], c[1], c[2]);
    format!("mat3x3f({}, {}, {})", col(&m[0]), col(&m[1]), col(&m[2]))
}

/// The WGSL constants the shaders take from here: the tone mapping's matrices, fit and inverses,
/// the inverse fit's limit, the wrap and the fades (the layer table is unrolled into
/// `water_slope`, shaders.rs).
pub fn wgsl() -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "const ACES_INPUT: mat3x3f = {};\nconst ACES_OUTPUT: mat3x3f = {};\n",
        wgsl_mat(&ACES_INPUT),
        wgsl_mat(&ACES_OUTPUT)
    ));
    s.push_str(&format!(
        "const ACES_INPUT_INV: mat3x3f = {};\nconst ACES_OUTPUT_INV: mat3x3f = {};\nconst INVERSE_FIT_MAX: f32 = {:?};\n",
        wgsl_mat(&mat_inverse(&ACES_INPUT)),
        wgsl_mat(&mat_inverse(&ACES_OUTPUT)),
        INVERSE_FIT_MAX
    ));
    let [a, b, c, d, e] = FIT;
    s.push_str(&format!(
        "const FIT_A: f32 = {a:?};\nconst FIT_B: f32 = {b:?};\nconst FIT_C: f32 = {c:?};\nconst FIT_D: f32 = {d:?};\nconst FIT_E: f32 = {e:?};\n"
    ));
    s.push_str(&format!(
        "const WRAP_M: f32 = {:?};\nconst FADE_LO: f32 = {:?};\nconst FADE_HI: f32 = {:?};\nconst MIN_ALPHA: f32 = {:?};\nconst WAVE_LAYERS: i32 = {};\n",
        WRAP_M as f32,
        FADE_LO,
        FADE_HI,
        MIN_ALPHA,
        LAYERS.len()
    ));
    s.push_str(&format!(
        "const NOISE_TILE: i32 = {NOISE_TILE};\nconst NOISE_TEXELS: f32 = {NOISE_TEXELS}.0;\nconst CAUSTIC_TILE: i32 = {CAUSTIC_TILE};\nconst CAUSTIC_TEXELS: f32 = {CAUSTIC_TEXELS}.0;\nconst CAUSTIC_EXACT_TEXELS: f32 = {CAUSTIC_EXACT_TEXELS:?};\n"
    ));
    let b = dwell_look::water_tiers::BANK_BAND;
    s.push_str(&format!(
        "const BANK_REACH_M: f32 = {:?};\nconst BANK_SLACK: f32 = {:?};\nconst BANK_SPREAD_PX: f32 = {:?};\nconst BANK_BLUR_PX: f32 = {:?};\nconst BANK_DARKEN: f32 = {:?};\nconst BANK_STRENGTH: f32 = {:?};\n",
        b.reach_m as f32,
        b.depth_slack as f32,
        b.spread_px as f32,
        b.blur_px as f32,
        b.darken as f32,
        b.strength as f32
    ));
    let c = dwell_look::water_tiers::SURFACE_CAUSTICS;
    s.push_str(&format!(
        "const SURFACE_CAUSTICS_M: f32 = {:?};\nconst SURFACE_CAUSTICS_GAIN: f32 = {:?};\n",
        c.depth_m as f32, c.gain as f32
    ));
    for (name, bit) in DEBUG_SWITCHES {
        s.push_str(&format!(
            "const DBG_{}: u32 = {bit}u;\n",
            name.to_uppercase()
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    #[test]
    fn neighbours_wrap_without_a_modulo_as_the_modulo_would() {
        use super::{wrap_cell, wrap_step};
        for period in [1, 2, 7, 731] {
            for c in [
                -3 * period - 1,
                -1,
                0,
                5,
                period - 1,
                period,
                4 * period + 3,
            ] {
                let w = wrap_cell(c, period);
                assert!((0..period).contains(&w));
                for o in -1..=1 {
                    assert_eq!(wrap_step(w, o, period), (c + o).rem_euclid(period));
                }
            }
        }
    }

    #[test]
    fn hashed_directions_are_unit_and_spread_round_the_circle() {
        let mut quadrants = [0; 4];
        for k in 0..4096u32 {
            let h = k.wrapping_mul(0x9e37_79b9) ^ (k >> 3);
            let [x, y] = super::hash_dir(h);
            assert!(((x * x + y * y).sqrt() - 1.0).abs() < 1e-4);
            quadrants[usize::from(x < 0.0) + 2 * usize::from(y < 0.0)] += 1;
        }
        assert!(quadrants.iter().all(|&q| q > 800), "{quadrants:?}");
    }

    #[test]
    fn the_debug_switches_parse_by_name_and_are_distinct_bits() {
        use super::{DEBUG_SWITCHES, debug_mask};
        assert_eq!(debug_mask(""), 0);
        assert_eq!(debug_mask("caustics"), 128);
        assert_eq!(debug_mask("caustics, foam,nonsense"), 128 | 256);
        let all = DEBUG_SWITCHES.iter().fold(0, |m, (_, b)| {
            assert_eq!(m & b, 0, "bits overlap");
            assert!(b.is_power_of_two());
            m | b
        });
        assert_eq!(debug_mask("all"), all & !2048);
    }

    use super::*;

    fn close(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn packs_each_fluid_at_its_id_with_its_own_parameters_a_second_fluid_by_data_alone() {
        // shared/blocks/fixtures/test.json adds `test:oil` and nothing else: it gets its own
        // parameters, at its own id, beside water's.
        use dwell_blocks::fixture::FLUIDS;
        let buf = pack_fluids(&FLUIDS);
        assert_eq!(buf.len(), FLUIDS.len() * FLUID_VEC4S * 4);
        let at = |name: &str| {
            let id = FLUIDS.iter().position(|f| f.name == name).expect(name);
            &buf[id * FLUID_VEC4S * 4..(id + 1) * FLUID_VEC4S * 4]
        };
        let oil = at("test:oil");
        let water = at("dwell:water");
        // absorption, opacity
        assert_eq!(&oil[0..4], &[2.0, 2.5, 3.0, 0.8]);
        assert_eq!(&water[0..4], &[1.0, 0.16, 0.045, 0.72]);
        // scatter (linear), roughness
        let s = linear_rgb(0x2a2410);
        assert_eq!(&oil[4..8], &[s[0], s[1], s[2], 0.02]);
        assert_eq!(water[7], 0.08);
        // ior, normal scale, wave scale, flow speed
        assert_eq!(&oil[8..12], &[1.47, 0.4, 4.0, 0.005]);
        assert_eq!(&water[8..12], &[1.333, 1.0, 2.0, 0.3]);
        // foam colour (linear), depth
        assert_eq!(oil[15], 0.0);
        assert_eq!(
            water[15],
            dwell_blocks::require_fluid("dwell:water").foam_depth
        );
        // F0 from the IOR, caustics, surface drop
        assert!(close(oil[16], f0_from_ior(1.47), 1e-7));
        assert_eq!(&oil[17..20], &[0.2, 0.25, 1.0]);
        assert_eq!(water[18], 0.125);
        // refraction, caustic scale, caustic depth, sparkle
        assert_eq!(&oil[20..24], &[0.05, 1.5, 0.5, 0.0]);
        let w = dwell_blocks::require_fluid("dwell:water");
        assert_eq!(
            &water[20..24],
            &[w.refraction, w.caustic_scale, w.caustic_depth, w.sparkle]
        );
        assert_eq!((water[17], water[19]), (w.caustics, w.scattering));
    }

    #[test]
    fn the_ray_in_the_water_runs_from_the_surface_to_the_scene_behind() {
        // Looking straight along the view axis: the depths' difference.
        assert!(close(ray_thickness(12.0, 10.0, 10.0), 2.0, 1e-6));
        // Off-axis, a point 20 m away at view depth 10: the ray is twice its depth difference.
        assert!(close(ray_thickness(12.0, 10.0, 20.0), 4.0, 1e-6));
        // Nothing behind (the sky, past the far plane): as deep as matters.
        assert_eq!(ray_thickness(0.0, 10.0, 10.0), DEEP_M);
        // Never negative where the scene is in front by a hair (MSAA edges).
        assert_eq!(ray_thickness(9.99, 10.0, 10.0), 0.0);
    }

    #[test]
    fn refraction_falls_back_where_the_offset_sample_is_in_front_of_the_water() {
        assert!(refracted_sample(15.0, 10.0), "the floor behind");
        assert!(refracted_sample(0.0, 10.0), "nothing drawn there");
        assert!(
            !refracted_sample(4.0, 10.0),
            "a bank in front: no foreground bleed"
        );
    }

    #[test]
    fn shallow_water_shows_the_floor_and_deep_water_its_body_graded_by_channel() {
        let floor = [0.8, 0.7, 0.4];
        let body = [0.0, 0.1, 0.4];
        let sigma = [0.5, 0.1, 0.03];
        assert_eq!(through_fluid(floor, body, sigma, 0.2, 0.0, 0.0), floor);
        let deep = through_fluid(floor, body, sigma, 0.2, 500.0, 50.0);
        for k in 0..3 {
            assert!(close(deep[k], body[k], 1e-3), "{deep:?}");
        }
        // Red goes first: a metre in, the floor has lost more red than blue.
        let m = through_fluid(floor, [0.0; 3], sigma, 0.0, 1.0, 0.0);
        assert!(m[0] / floor[0] < m[2] / floor[2]);
        // Light that came further down is dimmer.
        let lit = through_fluid(floor, body, sigma, 0.2, 1.0, 0.0);
        let dim = through_fluid(floor, body, sigma, 0.2, 1.0, 3.0);
        assert!((0..3).all(|k| dim[k] < lit[k]));
        // Scattering brings the body in sooner, without dimming the light on the way down.
        let murky = through_fluid(floor, body, sigma, 1.0, 1.0, 0.0);
        assert!(murky[2] > lit[2] && murky[0] < lit[0]);
    }

    #[test]
    fn the_builds_water_grades_from_clear_through_turquoise_to_blue_never_green() {
        // Tropical water over sand (the sand tile's colour, linear), seen straight down: a
        // hand's depth is nearly the sand; once the red has gone (a metre or so) it reads
        // turquoise to cyan, green no more than a little over blue; deep is a rich blue (blue
        // well above green, red faint). Never green.
        let w = dwell_blocks::require_fluid("dwell:water");
        let sand = linear_rgb(0xeed9a4);
        let body = linear_rgb(w.scatter).map(|c| c * 0.8);
        let at = |d: f32| through_fluid(sand, body, w.absorption, w.scattering, d, d);
        let hand = at(0.1);
        assert!(
            (0..3).all(|k| close(hand[k], sand[k], 0.25 * sand[k])),
            "{hand:?}"
        );
        for d in [1.0_f32, 2.0, 4.0, 8.0, 16.0, 40.0] {
            let c = at(d);
            assert!(c[1] <= c[2] * 1.1, "green at {d} m: {c:?}");
            assert!(c[0] < 0.6 * c[1], "red gone at {d} m: {c:?}");
        }
        let two = at(2.0);
        assert!(two[1] > 0.6 * two[2], "turquoise to cyan at 2 m: {two:?}");
        let deep = at(40.0);
        assert!(
            deep[2] > 2.0 * deep[1] && deep[0] < 0.05 * deep[2],
            "blue when deep: {deep:?}"
        );
    }

    #[test]
    fn caustics_sample_the_tile_at_every_distance() {
        // Per-pixel evaluation close to the water cost a phone its frame rate (owner, 2026-11-06).
        assert_eq!(CAUSTIC_EXACT_TEXELS, 0.0);
        let wgsl = crate::render::shaders::source(1);
        assert!(wgsl.contains("const CAUSTIC_EXACT_TEXELS: f32 = 0.0;"));
    }

    #[test]
    fn caustics_are_whole_at_the_surface_and_fade_with_depth_and_wrap() {
        assert_eq!(caustic_fade(0.0, 8.0), 0.0, "nothing above the surface");
        assert!(close(caustic_fade(1e-6, 8.0), 1.0, 1e-4));
        assert!(close(caustic_fade(8.0, 8.0), (-1.0f32).exp(), 1e-6));
        assert!(caustic_fade(30.0, 8.0) < 0.03);
        for scale in [1.5, 2.5, 3.3] {
            let (n, cell) = caustic_period(scale);
            assert_eq!(n % CAUSTIC_TILE, 0, "whole caustic tiles");
            assert!(close(n as f32 * cell, WRAP_M as f32, 1e-3));
            assert!(close(
                cell,
                scale,
                0.6 * scale * scale * CAUSTIC_TILE as f32 / WRAP_M as f32
            ));
        }
    }

    #[test]
    fn the_builds_fluids_fill_the_buffer_and_an_empty_table_still_binds() {
        assert_eq!(
            pack_fluids(&dwell_blocks::FLUIDS).len(),
            dwell_blocks::FLUIDS.len() * FLUID_VEC4S * 4
        );
        assert_eq!(pack_fluids(&[]).len(), FLUID_VEC4S * 4);
    }

    #[test]
    fn fresnel_is_f0_head_on_and_total_at_grazing() {
        let f0 = f0_from_ior(1.333);
        assert!(close(f0, 0.0204, 1e-4), "{f0}");
        assert!(close(schlick(1.0, f0), f0, 1e-7));
        assert!(close(schlick(0.0, f0), 1.0, 1e-7));
        // Monotonic between, and about 6 % at 60° from the normal for water.
        let mut last = f0;
        for i in (0..=100).rev() {
            let r = schlick(i as f32 / 100.0, f0);
            assert!(r >= last - 1e-7);
            last = r;
        }
        assert!(close(schlick(0.5, f0), 0.0510, 1e-3));
    }

    #[test]
    fn the_normal_is_up_for_a_flat_surface_and_leans_against_the_slope_on_either_face() {
        assert_eq!(blend_normal(1.0, [0.0, 0.0]), [0.0, 1.0, 0.0]);
        assert_eq!(blend_normal(-1.0, [0.0, 0.0]), [-0.0, -1.0, -0.0]);
        // Rising toward +x: the normal leans back toward −x; unit length.
        let n = blend_normal(1.0, [0.2, 0.0]);
        assert!(n[0] < 0.0 && n[1] > 0.9 && n[2] == 0.0);
        assert!(close(n.iter().map(|c| c * c).sum::<f32>(), 1.0, 1e-6));
        // Face B (BIFACIAL_WORLD.md §6): the same surface mirrored, its normal turned over.
        let b = blend_normal(-1.0, [0.2, 0.0]);
        assert_eq!(b, n.map(|c| -c));
    }

    #[test]
    fn layers_fade_out_before_a_pixel_covers_half_their_wavelength_and_widen_the_glint() {
        assert_eq!(layer_fade(0.01, 1.0), 1.0);
        assert_eq!(layer_fade(0.125, 1.0), 1.0);
        assert_eq!(layer_fade(0.5, 1.0), 0.0);
        assert_eq!(layer_fade(5.0, 1.0), 0.0);
        let mid = layer_fade(0.3, 1.0);
        assert!(mid > 0.0 && mid < 1.0);
        // Nothing lost: the fluid's own roughness; the lost slope widens it.
        assert!(close(glint_alpha(0.3, 0.0), 0.09, 1e-6));
        assert!(glint_alpha(0.3, 0.01) > 0.09);
        assert_eq!(glint_alpha(0.0, 0.0), MIN_ALPHA);
    }

    #[test]
    fn each_layer_wraps_exactly_at_the_wrap_and_the_lattice_turns_are_integer() {
        for scale in [0.5, 2.0, 3.3, 4.0] {
            for l in &LAYERS {
                let (n, wavelength) = layer_period(l, scale);
                assert!(n >= 1);
                assert_eq!(n % NOISE_TILE, 0, "whole noise tiles");
                assert!(close(n as f32 * wavelength, WRAP_M as f32, 1e-3));
                // Within half a tile's rounding over the wrap.
                let want = scale * l.wavelength;
                assert!(close(
                    wavelength,
                    want,
                    0.6 * want * want * NOISE_TILE as f32 / WRAP_M as f32 + 1e-3
                ));
            }
        }
        for l in &LAYERS {
            assert!(close(l.dir[0].hypot(l.dir[1]), 1.0, 1e-3));
        }
        assert_eq!(wrapped_origin([-1.0, 5.0, 2048.0 + 3.0]), [1023.0, 3.0]);
    }

    #[test]
    fn the_inverse_tone_mapping_gives_back_the_scene_colour() {
        for exposure in [0.6, 1.0, 2.0] {
            for c in [
                [0.02, 0.05, 0.1],
                [0.3, 0.5, 0.9],
                [0.8, 0.7, 0.6],
                [0.01, 0.01, 0.01],
            ] {
                let shown = aces(c, exposure);
                let back = inverse_aces(shown, exposure);
                for k in 0..3 {
                    assert!(
                        close(back[k], c[k], 2e-3 + 2e-3 * c[k]),
                        "{c:?} → {shown:?} → {back:?}"
                    );
                }
            }
        }
        // And the sky's colours (display) survive the round trip the other way.
        let sky = [0.55, 0.7, 0.9];
        let again = aces(inverse_aces(sky, 1.0), 1.0);
        for k in 0..3 {
            assert!(close(again[k], sky[k], 1e-3), "{again:?}");
        }
    }

    #[test]
    fn tiers_by_name_and_level_and_low_sees_through_with_one_layer() {
        let (low, medium) = (
            WaterQuality::named("low").unwrap(),
            WaterQuality::named("medium").unwrap(),
        );
        for q in [low, medium, WaterQuality::cpu()] {
            assert_eq!(WaterQuality::from_level(q.index() as u32), q);
        }
        assert_eq!(
            WaterQuality::from_level(99),
            medium,
            "past the last: the last"
        );
        assert_eq!(
            WaterQuality::named("high"),
            Some(medium),
            "High folded into Medium"
        );
        assert_eq!(WaterQuality::default(), medium, "the desktop default");
        assert_eq!((low.layers(), medium.layers()), (1, 3));
        // The owner (2026-11-07): Low sees through without the scene copy (no pass split), with
        // the caustic tile's lines; Medium reads the scene; the CPU's tier is opaque.
        assert!(!low.reads_scene() && low.draws_caustics() && medium.reads_scene());
        assert!(!WaterQuality::cpu().reads_scene() && !WaterQuality::cpu().draws_caustics());
        assert_eq!(
            low.bits(),
            dwell_look::water_tiers::tier_bits(low.tier()) as f32
        );
    }

    #[test]
    fn the_ripples_visibly_move() {
        // The owner (2026-10-26): the water's shading moved so slowly it hardly seemed to. Each
        // layer crosses its own wavelength in a few seconds (a breeze's ripples), the largest in
        // under 10 s.
        let water = dwell_blocks::require_fluid("dwell:water");
        for l in LAYERS {
            let wavelength = l.wavelength * water.wave_scale;
            let seconds = wavelength / (water.flow_speed * l.speed);
            assert!(seconds < 10.0, "{l:?}: {seconds} s a wavelength");
            assert!(seconds > 0.25, "{l:?}: {seconds} s, no blur of motion");
        }
    }

    #[test]
    fn each_fluid_packs_its_underwater_look_and_one_without_takes_the_default() {
        use dwell_look::underwater::{DEFAULT_UNDERWATER, underwater_look};
        let buf = pack_fluids(&dwell_blocks::FLUIDS);
        for (id, f) in dwell_blocks::FLUIDS.iter().enumerate() {
            let v = &buf[id * FLUID_VEC4S * 4..(id + 1) * FLUID_VEC4S * 4];
            let u = underwater_look(f.name);
            let under = [u.lens_m, u.fog_clear_m, u.fog_density, u.band_depth_m];
            let line = [
                u.line_amplitude_m,
                u.line_wavelength_m,
                u.line_speed,
                u.underside_texture,
            ];
            let rim = [
                u.band_rim_px,
                u.band_rim_shade,
                u.band_line_px,
                u.band_line_glow,
            ];
            // the shore foam's bands (shared/look/waterflow.json `shore`)
            if let Some(l) = dwell_look::waterflow::flow_look(f.name) {
                assert_eq!(v[34..36], [l.shore_bands as f32, l.shore_period_s as f32]);
                let shore = [
                    l.shore_line_m,
                    l.shore_width_m,
                    l.shore_strength,
                    l.shore_wobble,
                ];
                assert_eq!(v[36..40], shore.map(|x| x as f32));
            }
            assert_eq!(v[40..44], under.map(|x| x as f32));
            assert_eq!(v[44..48], line.map(|x| x as f32));
            assert_eq!(v[51], u.band_streaks as f32);
            assert_eq!(v[52..56], rim.map(|x| x as f32));
            let extra = [
                u.band_glints,
                u.band_breakup,
                u.band_mirror_warp_px,
                u.band_mirror,
            ];
            assert_eq!(v[56..60], extra.map(|x| x as f32));
            let under = [
                u.underside_breakup,
                u.underside_sun_patch,
                u.underside_sun_power,
                u.underside_cell_scale,
            ];
            assert_eq!(v[60..64], under.map(|x| x as f32));
            assert_eq!(v[64..66], [u.window_cells as f32, u.window_bodies as f32]);
            assert_eq!(
                v[66..68],
                [u.window_sun_glow as f32, u.window_sun_power as f32]
            );
            // The colour grade (WATER_RENDERING.md §18).
            let fog = linear_rgb(u.tone_fog_color);
            assert_eq!(v[68..72], [fog[0], fog[1], fog[2], u.tone_fog_share as f32]);
            let lift = [
                u.tone_brightness,
                u.tone_mirror,
                u.tone_floor_light,
                u.window_rim_glow,
            ];
            assert_eq!(v[72..76], lift.map(|x| x as f32));
        }
        // A fluid with no look of its own takes the default's (fog from the eye, the registry's rates).
        assert_eq!(underwater_look("test:oil"), &DEFAULT_UNDERWATER);
        assert_eq!(DEFAULT_UNDERWATER.fog_density, 1.0);
    }

    dwell_world::both_faces!(
        fn the_waterline_splits_a_view_from_the_surface_by_each_pixels_ray(f: FaceFrame) {
            // Face-local fixture: the surface at 10.875, the eye exactly on it; rays a little up
            // and a little down from level (world y of the face-local direction through `f`).
            let surface = 10.875;
            let (eye, s) = (f.y(surface), f.y(surface));
            let dir_y = |local_y: f64| f.vector([0.0, local_y, 1.0])[1];
            let lens = 0.3;
            assert!(
                waterline_m(f, eye, s, dir_y(0.1), lens, 0.0) > 0.0,
                "a ray above level: above water"
            );
            assert!(
                waterline_m(f, eye, s, dir_y(-0.1), lens, 0.0) < 0.0,
                "a ray below level: under water"
            );
            // The line's wave lifts it: a ray just above level is under a crest.
            assert!(waterline_m(f, eye, s, dir_y(0.01), lens, 0.01) < 0.0);
            // The eye 0.2 m over the surface: a level ray is above; one steeply down reaches under.
            let high = f.y(surface + 0.2);
            assert!(waterline_m(f, high, s, dir_y(0.0), lens, 0.0) > 0.0);
            assert!(waterline_m(f, high, s, dir_y(-1.0) / 2f64.sqrt(), lens, 0.0) < 0.0);
            // Deep (1 m under), every ray within the lens is under: the whole view, as before.
            let deep = f.y(surface - 1.0);
            assert!(waterline_m(f, deep, s, dir_y(1.0) / 2f64.sqrt(), lens, 0.0) < 0.0);
        }
    );

    /// A camera at `pitch` and `roll` (radians), face-local through `f`: forward, right, up.
    fn camera(f: dwell_world::FaceFrame, pitch: f64, roll: f64) -> [[f64; 3]; 3] {
        let (sp, cp) = pitch.sin_cos();
        let (sr, cr) = roll.sin_cos();
        let fwd = [0.0, sp, cp];
        let up0 = [0.0, cp, -sp];
        let right0 = [1.0, 0.0, 0.0];
        let right: [f64; 3] = std::array::from_fn(|k| cr * right0[k] + sr * up0[k]);
        let up: [f64; 3] = std::array::from_fn(|k| cr * up0[k] - sr * right0[k]);
        [f.vector(fwd), f.vector(right), f.vector(up)]
    }

    /// The default FOV at 16:10 (100° across, 75°-ish down: the bench's view).
    const TAN: [f64; 2] = [1.1918, 0.7449];

    /// A pixel's waterline at `ndc` for an eye `eye` metres over the surface (face-local).
    fn pixel_d(
        f: dwell_world::FaceFrame,
        cam: [[f64; 3]; 3],
        eye: f64,
        ndc: [f64; 2],
        lens: f64,
    ) -> f64 {
        let surface = 10.875;
        let ray = view_ray(cam[0], cam[1], cam[2], TAN, ndc);
        waterline_m(f, f.y(surface + eye), f.y(surface), ray[1], lens, 0.0)
    }

    dwell_world::both_faces!(
        fn the_waterline_is_a_straight_line_at_any_eye_height_and_roll(f: FaceFrame) {
            // The lens points lie on a plane, so the water cuts them in a line: for a level camera
            // a screen row is all under or all over (the old sphere of points bent it into an arc,
            // its ends rising toward the screen's edges: WATER_GPU_SURFACE.md §1.2).
            let lens = dwell_look::underwater::underwater_look("dwell:water").lens_m;
            let level = camera(f, 0.0, 0.0);
            for eye in [0.1, 0.05, -0.05, -0.1, -0.2] {
                // The row the line crosses at the centre, then rows a pixel's worth either side.
                let row = -eye / (lens * TAN[1]);
                for (dy, under) in [(-0.002, true), (0.002, false)] {
                    for k in -10..=10 {
                        let x = f64::from(k) / 10.0;
                        let d = pixel_d(f, level, eye, [x, row + dy], lens);
                        assert_eq!(d < 0.0, under, "eye {eye}, ndc ({x}, {}): d {d}", row + dy);
                    }
                }
            }
            // Rolled and pitched: the under pixels are a half-plane, so three points on any
            // screen line are never under, over, under.
            let tilted = camera(f, 0.2, 0.3);
            for eye in [0.12, -0.08] {
                for k in -10..=10 {
                    let t = f64::from(k) / 10.0;
                    let a = pixel_d(f, tilted, eye, [-1.0, t], lens);
                    let m = pixel_d(f, tilted, eye, [0.0, t], lens);
                    let b = pixel_d(f, tilted, eye, [1.0, t], lens);
                    assert!((m - (a + b) / 2.0).abs() < 1e-9, "linear across the screen");
                }
            }
        }
    );

    dwell_world::both_faces!(
        fn the_waterline_moves_smoothly_with_the_eye_and_enters_without_a_pop(f: FaceFrame) {
            let look = dwell_look::underwater::underwater_look("dwell:water");
            let lens = look.lens_m;
            let level = camera(f, 0.0, 0.0);
            // The level view's line at the centre column, by bisection over the screen.
            let line_row = |eye: f64| {
                let (mut lo, mut hi) = (-1.0, 1.0);
                for _ in 0..50 {
                    let mid = (lo + hi) / 2.0;
                    if pixel_d(f, level, eye, [0.0, mid], lens) < 0.0 {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                lo
            };
            // Centimetre steps from 0.25 m over to 0.25 m under: the line moves a little each
            // step (no jump), monotonically up the screen as the eye sinks.
            let mut last = line_row(0.25);
            assert!(
                last > -1.0 && last < -0.8,
                "at +0.25 m the line is near the bottom edge"
            );
            for cm in (-25..25).rev() {
                let row = line_row(f64::from(cm) / 100.0);
                assert!(
                    row >= last && row - last < 0.05,
                    "at {cm} cm: {last} → {row}"
                );
                last = row;
            }
            assert!(
                last > 0.8 && last < 1.0,
                "at −0.25 m the line is near the top edge"
            );
            // Entering the split (the eye `reach_m` over the surface, else no fluid found) or
            // leaving it for the whole view under (`reach_m` under): no pixel at any pitch or
            // roll is on the other side there, so the switch changes nothing.
            let reach = look.reach_m();
            for pitch in [-1.5, -0.8, -0.3, 0.0, 0.3, 0.8, 1.5] {
                for roll in [0.0, 0.7] {
                    let cam = camera(f, pitch, roll);
                    for ndc in [
                        [-1.0, -1.0],
                        [1.0, -1.0],
                        [-1.0, 1.0],
                        [1.0, 1.0],
                        [0.0, 0.0],
                    ] {
                        let wave = look.line_amplitude_m;
                        assert!(pixel_d(f, cam, reach, ndc, lens) - wave > 0.0);
                        assert!(pixel_d(f, cam, -reach, ndc, lens) + wave < 0.0);
                    }
                }
            }
        }
    );

    #[test]
    fn lens_ray_max_covers_every_field_of_view() {
        // both-faces: a camera's FOV is the same on either face.
        use dwell_lod::frustum::{FOV_SETTING_RANGE, base_fov};
        for setting in [FOV_SETTING_RANGE.0, 75.0, FOV_SETTING_RANGE.1] {
            for aspect in [0.45, 0.75, 1.0, 1.33, 1.6, 2.0, 2.4, 3.0] {
                let v = (base_fov(setting, aspect).to_radians() / 2.0).tan();
                let h = v * aspect;
                let corner = (1.0 + v * v + h * h).sqrt();
                assert!(
                    corner <= dwell_look::underwater::LENS_RAY_MAX,
                    "setting {setting}, aspect {aspect}: {corner}"
                );
            }
        }
    }

    #[test]
    fn the_rims_top_edge_is_anti_aliased_across_the_line() {
        // face-a-only: the shade across the line is a function of pixels from it (face-free).
        // Was: the pixels above the line took none of the rim and the first under it all of it,
        // so the rim's top edge stepped a whole pixel along the wave (the 0.50.20 descent).
        let l = dwell_look::underwater::underwater_look("dwell:water");
        let shade = |px: f64| {
            line_shade(
                px,
                l.band_rim_px,
                l.band_rim_shade,
                l.band_line_px,
                l.band_line_glow,
            )
        };
        let jump = (shade(-1e-3) - shade(1e-3)).abs();
        assert!(jump < 0.01, "no step at the line: {jump}");
        assert!(
            (shade(-0.5) - (1.0 + shade(0.0)) / 2.0).abs() < 0.01,
            "half a pixel, half the rim"
        );
        assert_eq!(shade(-1.0), 1.0, "a pixel above, untouched");
        assert!(shade(0.5) < 0.6, "the rim under the line");
    }

    dwell_world::both_faces!(
        fn the_undersides_texture_fades_into_the_fog_toward_level(f: FaceFrame) {
            // Was: looking level from 1 m down, the textured underside met the fog at a hard
            // horizon (its cells at full strength to the last ray that reached the surface).
            let l = dwell_look::underwater::underwater_look("dwell:water");
            let water = dwell_blocks::require_fluid("dwell:water");
            let ext = (0..3).map(|k| water.absorption[k] as f64).sum::<f64>() / 3.0
                + water.scattering as f64;
            let below = 1.0;
            // A ray rising at `rise` (face-local) from 1 m under reaches the surface below/rise away.
            let contrast = |rise: f64| {
                let dir = f.vector([0.0, rise, (1.0 - rise * rise).sqrt()]);
                let to_surface = below / f.vector(dir)[1];
                underside_contrast(to_surface, l.fog_clear_m, l.fog_density, ext)
            };
            assert_eq!(
                contrast(0.9),
                1.0,
                "overhead, within the clear distance: all of it"
            );
            let mut last = 1.0;
            for k in 1..60 {
                let c = contrast(0.9 * (1.0 - k as f64 / 60.0));
                assert!(c <= last + 1e-12, "fades monotonically toward level");
                last = c;
            }
            assert!(
                contrast(0.005) < 0.02,
                "gone at the horizon: {}",
                contrast(0.005)
            );
        }
    );

    dwell_world::both_faces!(
        fn the_sun_patch_follows_the_sun_through_snells_window(f: FaceFrame) {
            let ior = dwell_blocks::require_fluid("dwell:water").ior as f64;
            let rise = |v: [f64; 3]| f.vector(v)[1];
            // Overhead: straight up.
            let z = sun_through(f, f.vector([0.0, 1.0, 0.0]), ior);
            assert!((rise(z) - 1.0).abs() < 1e-9);
            // On the horizon: at the critical angle (48.6 deg from the up for water), toward it.
            let h = sun_through(f, f.vector([1.0, 0.0, 0.0]), ior);
            let critical = (1.0 / ior).asin();
            assert!((rise(h) - critical.cos()).abs() < 1e-9 && h[0] > 0.0);
            // Lower suns show lower in the window, monotonically.
            let mut last = 2.0;
            for k in 0..=10 {
                let a = std::f64::consts::FRAC_PI_2 * (1.0 - k as f64 / 10.0);
                let r = rise(sun_through(f, f.vector([a.cos(), a.sin(), 0.0]), ior));
                assert!(r < last + 1e-12);
                last = r;
            }
        }
    );

    dwell_world::both_faces!(
        fn the_waterline_band_sits_under_the_line_with_a_dark_rim_over_a_bright_line(f: FaceFrame) {
            // Owner, 2026-11-07: "the band isn't just a white line, it's supposed to be like an
            // actual thing", "a darker line over a lighter line". The old meniscus was a thin
            // light band straddling the line, and drew above the split.
            let look = dwell_look::underwater::underwater_look("dwell:water");
            let (lens, h) = (look.lens_m, 1600.0);
            for (pitch, roll) in [(0.0, 0.0), (0.5, 0.0), (-0.4, 0.3)] {
                let cam = camera(f, pitch, roll);
                let px_m = meniscus_px_m(lens, TAN[1], h, cam[2][1], cam[1][1]);
                // Down the centre column in pixels (r from the top); the split is where d crosses 0.
                let d = |r: f64| pixel_d(f, cam, 0.03, [0.0, 1.0 - 2.0 * r / h], lens);
                let mut split = 0.0;
                while d(split + 1.0) >= 0.0 && split < h {
                    split += 1.0;
                }
                assert!(split > 0.0 && split < h, "the line is on screen");
                // Nothing of the band above the line; its top edge is the first pixel under it.
                for r in [split - 3.0, split - 1.0, split] {
                    assert!(
                        band_t(d(r), look.band_depth_m).is_none(),
                        "above the line at {r}"
                    );
                }
                let top = band_t(d(split + 1.0), look.band_depth_m).expect("the band's top");
                assert!(top < 0.05, "the top edge is the surface: {top}");
                // A slab of real thickness: rows down to its foot (2.5–4.5 % of the screen level;
                // halved in 0.50.23, underwater.test.mjs's 2.5–4 % of the lens plane).
                let mut foot = split + 1.0;
                while band_t(d(foot + 1.0), look.band_depth_m).is_some() {
                    foot += 1.0;
                }
                let share = (foot - split) / h;
                if pitch == 0.0 {
                    assert!(
                        share > 0.025 && share < 0.045,
                        "level: {share} of the screen"
                    );
                }
                // Along the edge: darker first, then lighter, then the band's own colour, in
                // pixels across the line at any pitch and roll.
                let edge = |r: f64| {
                    let px = -d(r) / px_m;
                    band_edge(
                        px,
                        look.band_rim_px,
                        look.band_rim_shade,
                        look.band_line_px,
                        look.band_line_glow,
                    )
                };
                // The row `px` pixels across the line under it, down the column (d is linear).
                let slope = d(split) - d(split + 1.0);
                let zero = split + d(split) / slope;
                let row = |px: f64| zero + px * px_m / slope;
                assert!(
                    edge(row(0.5)) < 0.6,
                    "a dark rim on top, pitch {pitch} roll {roll}"
                );
                let lit = edge(row(look.band_rim_px + look.band_line_px / 2.0));
                assert!(lit > 1.3, "a bright line under it: {lit}");
                assert!((edge(row(look.band_rim_px + look.band_line_px + 3.0)) - 1.0).abs() < 1e-9);
            }
            // The texture recedes to the line: a pixel near it spans more of the texture.
            let rate = |t: f64| band_uv(0.0, t + 0.01)[1] - band_uv(0.0, t)[1];
            assert!(
                rate(0.0).abs() > 5.0 * rate(0.9).abs(),
                "compressed toward the line"
            );
            let wide = |t: f64| band_uv(0.1, t)[0] - band_uv(0.0, t)[0];
            assert!(wide(0.0) > 5.0 * wide(0.9), "and across it");
        }
    );

    #[test]
    fn snells_window_is_48_6_degrees_and_outside_it_the_surface_reflects() {
        // both-faces: `rise` is face-local (the cosine from the face's own up).
        let ior = 1.333;
        let edge = (1.0f64 / ior).asin().to_degrees();
        assert!((edge - 48.6).abs() < 0.05, "{edge}");
        let at = |deg: f64| sees_out(deg.to_radians().cos(), ior);
        assert!(
            at(0.0) && at(30.0) && at(48.0),
            "inside the window: out to the sky"
        );
        assert!(
            !at(49.5) && !at(80.0) && !at(89.0),
            "outside it: totally reflected"
        );
        assert!(
            !sees_out(-0.2, ior),
            "a ray going down never reaches the surface"
        );
    }

    #[test]
    fn snells_window_is_a_97_degree_cone_and_its_radius_grows_with_depth() {
        // both-faces: the angle and radius are face-local scalars.
        let ior = 1.333;
        let cone = 2.0 * critical_angle(ior).to_degrees();
        assert!((cone - 97.2).abs() < 0.1, "{cone}");
        // 2 m down the window is ~2.27 m across in radius, and twice that 4 m down.
        assert!(
            (window_radius(2.0, ior) - 2.27).abs() < 0.01,
            "{}",
            window_radius(2.0, ior)
        );
        assert!((window_radius(4.0, ior) - 2.0 * window_radius(2.0, ior)).abs() < 1e-9);
        assert_eq!(window_radius(0.0, ior), 0.0);
        assert_eq!(
            window_radius(-1.0, ior),
            0.0,
            "above the surface: no window"
        );
    }

    dwell_world::both_faces!(
        fn the_sky_through_snells_window_is_refracted_out_by_snells_law(f: FaceFrame) {
            // Owner, 2026-11-11: the surface from under should look transparent and show the light
            // shining down. Inside the window the sky is seen along the ray bent out of the water:
            // straight up stays straight up, the slant from the up grows by sin θ_air = n sin θ_water,
            // and a ray at the critical angle leaves along the horizon. Outside it: None (mirror).
            let ior = 1.333;
            let local = |deg: f64| {
                let r = deg.to_radians();
                f.vector([r.sin(), r.cos(), 0.0])
            };
            let up = water_exit(f, local(0.0), ior).expect("straight up leaves");
            let l = f.vector(up);
            assert!((l[1] - 1.0).abs() < 1e-12 && l[0].abs() < 1e-12, "{l:?}");
            for deg in [10.0, 30.0, 45.0, 48.0] {
                let out = f.vector(water_exit(f, local(deg), ior).expect("inside the window"));
                let air = out[0].atan2(out[1]).to_degrees();
                let want = (ior * deg.to_radians().sin()).asin().to_degrees();
                assert!((air - want).abs() < 1e-9, "{deg}°: {air} vs {want}");
                assert!(
                    air > deg,
                    "the sky is squeezed into the window: {deg}° shows {air}°"
                );
                assert!(
                    ((out[0] * out[0] + out[1] * out[1] + out[2] * out[2]) - 1.0).abs() < 1e-12
                );
            }
            let edge = critical_angle(ior).to_degrees();
            let rim = f.vector(water_exit(f, local(edge - 1e-6), ior).expect("just inside"));
            assert!(rim[1] < 1e-3, "the window's rim shows the horizon: {rim:?}");
            for deg in [edge + 0.1, 60.0, 85.0] {
                assert!(
                    water_exit(f, local(deg), ior).is_none(),
                    "{deg}°: totally reflected"
                );
                assert!(!sees_out(
                    local(deg)
                        .iter()
                        .zip(f.vector([0.0, 1.0, 0.0]))
                        .map(|(a, b)| a * b)
                        .sum(),
                    ior
                ));
            }
            assert!(
                water_exit(f, local(120.0), ior).is_none(),
                "a ray going down never leaves"
            );
        }
    );

    dwell_world::both_faces!(
        fn looking_up_through_the_window_shows_the_sky_seen_from_just_above(f: FaceFrame) {
            // Owner, 2026-11-11: "you should be looking from just outside the water up compared to
            // just under the water up, because that's the mismatch." 0.3 m under, straight up at
            // midnight (full moon), at the moon 15° up and at midday, the window shows the sky the
            // sky pass draws straight up 0.3 m above: the ray leaves straight up, the window's
            // glow no longer mixes a white-cyan over it, and the fog does not reach 0.3 m.
            let water = dwell_look::underwater::underwater_look("dwell:water");
            let shafts = dwell_look::shafts::fluid_shafts("dwell:water");
            let ior = 1.333;
            let glow_srgb = dwell_look::hex(shafts.window_color);
            let up = f.vector([0.0, 1.0, 0.0]);
            let out = water_exit(f, up, ior).expect("straight up leaves");
            assert!((0..3).all(|k| (out[k] - up[k]).abs() < 1e-12));
            for (t, moon) in [(0.0, 0.5), (0.8689, 0.5), (0.5, 0.5)] {
                let day = dwell_look::daynight::DAY_NIGHT.state(0.0, Some(t), Some(moon));
                let lin = |c: [f64; 3]| c.map(srgb_to_linear);
                let above = lin(dwell_look::sky_color(up, f, day.of(f), &day));
                let zenith = above;
                let through = lin(dwell_look::sky_color(out, f, day.of(f), &day));
                let below = window_sky(through, zenith, glow_srgb, shafts.window_glow, 1.0);
                for k in 0..3 {
                    let rel = (below[k] - above[k]).abs() / above[k].max(1e-4);
                    assert!(
                        rel < 0.02,
                        "t {t}: the window's sky {below:?} is not the sky above {above:?} (channel {k}: {rel:.3})"
                    );
                }
            }
            // The fog's clear distance covers the window's whole cone from 0.3 m under: the rim's
            // ray to the surface is 0.3 / cos(critical angle) long.
            let rim = 0.3 / critical_angle(ior).cos();
            assert!(
                water.fog_clear_m > rim,
                "{} m clear, the rim {rim} m",
                water.fog_clear_m
            );
        }
    );

    #[test]
    fn the_window_shows_the_real_sky_and_bodies_scaled_by_its_light() {
        // both-faces: a source check; the composite's face handling is `face_up`.
        let src = super::super::shaders::source(1);
        let at = src.find("fn composite_underwater(").expect("composite");
        let b = &src[at..at + src[at..].find("\n}\n").expect("end")];
        assert!(
            b.contains(
                "let out = water_exit(dir, normalize(up + vec3f(slope.x, 0.0, slope.y)), ior);"
            ),
            "the sky is looked up along the refracted ray, bent by the ripples"
        );
        assert!(b.contains("let seen = window_sky(out, out_px, f.window.y);"));
        assert!(
            !b.contains("mix(sky_radiance(dir), glow"),
            "not along the ray under water, nor a fixed glow"
        );
        // The window's sky is the sky pass's own (owner, 2026-11-11: looking up from just under
        // the water should match looking up from just above): one function draws the sky above the
        // horizon for both, with the stars, the moon, the sun and the clouds, on every tier.
        let at = src.find("fn window_sky(").expect("window_sky");
        let ws = &src[at..at + src[at..].find("\n}\n").expect("end")];
        assert!(ws.contains("return sky_above_with_clouds(out, sky(out), deck, fw, bodies);"));
        let at = src.find("fn sky_behind_with_clouds(").expect("sky pass");
        let sp = &src[at..at + src[at..].find("\n}\n").expect("end")];
        assert!(sp.contains("return sky_above_with_clouds(dir, clear, deck, fw, 1.0);"));
        let at = src.find("fn sky_above_with_clouds(").expect("the one sky");
        let one = &src[at..at + src[at..].find("\n}\n").expect("end")];
        assert!(
            one.contains("dwell_sky_bodies(dir, face, frame.day, clear)")
                && one.contains("dwell_clouds(lit, lit, dir, face, deck, fw,"),
            "the stars, moon, sun and clouds"
        );
        assert!(
            !b.contains("frame.reflect_sky.y"),
            "the bodies show on every tier, as the sky pass draws them"
        );
        // At night the water seen from inside takes the moonlight's grey as the scene above does
        // (`scene_display`'s saturation), not a saturated glowing blue; the window's sky keeps its own.
        assert!(b.contains("let mirror = moonlit(display("));
        assert!(b.contains("let shown = moonlit(display(underwater(lit,"));
        assert!(!b.contains("let window = moonlit("));
        let at = src.find("fn moonlit(").expect("moonlit");
        assert!(src[at..].contains("frame.day.horizon.a);"));
        let at = src.find("fn scene_display(").expect("scene_display");
        assert!(
            src[at..].contains("frame.day.horizon.a"),
            "the scene's saturation is the same"
        );
        assert!(
            b.contains("let window = display(underwater(sky_in * mix(1.0, look.x, f.window.x), to_surface, f));"),
            "the underside's cells over the window's sky at the look's share"
        );
        let w = dwell_look::underwater::underwater_look("dwell:water");
        assert!(
            w.window_cells < 0.5,
            "water's window is mostly the clear sky"
        );
        assert!(
            (w.window_bodies - 1.0).abs() < 1e-9,
            "and shows the stars, sun and moon as the sky above does"
        );
        assert!(
            b.contains(
                "inverse_display(frame.shafts_window.rgb) * max(zenith.r, max(zenith.g, zenith.b))"
            ),
            "the window's glow follows the sky's light (dim at night)"
        );
        // The CPU tier's surface from below is the simple see-through tinted sheet
        // (`from_below`'s Fresnel).
        let cpu = WATER_TIERS.iter().position(|t| t.cpu).expect("a CPU tier");
        assert!(!WATER_TIERS[cpu].reads_scene);
        let at = src.find("fn from_below(").expect("from_below");
        let fb = &src[at..at + src[at..].find("\n}\n").expect("end")];
        assert!(
            fb.contains("return vec4f(body * (look.x * (1.0 + look.y)), fres);"),
            "see-through by the Fresnel"
        );
    }

    dwell_world::both_faces!(
        fn no_band_pattern_is_above_nyquist_where_the_plane_compresses_it(f: FaceFrame) {
            // Owner, 2026-11-07: the band "looks pixelated", still stippled after 0.50.24's blur.
            // The band's plane recedes to the line, so near it a pixel spans many noise cells and
            // every layer aliased into speckle. Each layer is now kept only under the pixel's
            // Nyquist rate (half a cycle a pixel), at any drawing-buffer height (the owner's 50 %
            // render scale too) and pitch, and kept whole where it can be resolved.
            let look = dwell_look::underwater::underwater_look("dwell:water");
            for h in [360.0, 720.0, 1600.0] {
                for (pitch, roll) in [(0.0, 0.0), (0.5, 0.0), (-0.4, 0.3)] {
                    let cam = camera(f, pitch, roll);
                    let px_m = meniscus_px_m(look.lens_m, TAN[1], h, cam[2][1], cam[1][1]);
                    let mut aliased_somewhere = false;
                    let mut resolved_somewhere = false;
                    for i in 0..=100 {
                        let t = i as f64 / 100.0;
                        let fp = band_footprint(t, h, px_m, look.band_depth_m);
                        for k in band_layers(look.band_breakup) {
                            let cycles = (k[0] * fp[0]).max(k[1] * fp[1]);
                            let keep = band_keep(k, fp);
                            assert!(
                                cycles * keep <= 0.5 + 1e-9 && (cycles <= 0.5 || keep == 0.0),
                                "h {h} pitch {pitch} t {t}: {cycles} cycles a pixel kept {keep}"
                            );
                            if cycles <= 0.25 {
                                assert_eq!(keep, 1.0, "resolved layers stay whole");
                                resolved_somewhere = true;
                            }
                            aliased_somewhere |= cycles > 0.5;
                        }
                    }
                    // The case the clamp exists for: near the line the layers do alias.
                    assert!(aliased_somewhere && resolved_somewhere, "h {h}");
                }
            }
        }
    );

    #[test]
    fn the_shader_decides_the_waterline_as_its_cpu_twin_does() {
        // both-faces: the WGSL crosses into the face through face_y as waterline_m does through FaceFrame::y.
        let src = super::super::shaders::source(1);
        assert!(src.contains(
            "return face_y(face, frame.eye.x + lens.y) - face_y(face, frame.water.z) - wave;"
        ));
        // The view ray at view depth 1, not normalized: the lens plane, not a sphere.
        assert!(src.contains("let lens = ray * f.under.x;"));
        assert!(src.contains("let d = waterline_m(ray, f);"));
        assert!(!src.contains("waterline_m(normalize("));
        // The band as band_t, band_edge, band_uv; Snell's window as sees_out; the underside's
        // texture from below and the body's partial shadow (the blotch).
        assert!(src.contains("let t = -d / depth;"));
        assert!(src.contains("return with_shafts(waterline_band(p, composite_underwater(p, ray, textureLoad(scene_color, p, 0)), d, f), p);"));
        assert!(src.contains("let rim = 1.0 - smoothstep(f.rim.x - 0.75, f.rim.x + 0.25, px);"));
        assert!(src.contains("let uv = vec2f(along * 4.0, 1.0) / (t + 0.08)"));
        // The band's layers faded past Nyquist (band_footprint, band_layers, band_keep).
        assert!(src.contains("let fp = vec2f(4.0 / size.y / s, across.x / depth / (s * s));"));
        assert!(src.contains("return 1.0 - smoothstep(0.25, 0.5, max(k.x * fp.x, k.y * fp.y));"));
        for k in [
            "vec2f(0.45, 0.3)",
            "vec2f(1.0 + 1.2 * b, 0.35)",
            "vec2f(2.6, 0.9)",
            "vec2f(4.2, 1.6)",
        ] {
            assert!(src.contains(&format!("band_keep({k}, fp)")), "{k}");
        }
        assert!(src.contains("1.0 - bent * bent < 1.0 / (ior * ior)"));
        assert!(src.contains("let look = underside_look(xz, -s.v, footprint, f, fluid);"));
        assert!(!src.contains("BODY_SHADOW"));
        // §12.2: the rim anti-aliased above the line (line_shade), the underside under the fog
        // over its distance (underside_contrast), the sun patch along sun_through.
        assert!(src.contains("rim_above = 1.0 + (band_edge(0.0, f) - 1.0) * clamp(1.0 - d / waterline_across(f).x, 0.0, 1.0);"));
        assert!(src.contains(
            "let mirror = moonlit(display(underwater(underwater(vec3f(0.0), 1e6, f) * light, to_surface, f)));"
        ));
        assert!(src.contains("let h = (sun - up * face_rise(face, sun)) / max(ior, 1.0);"));
        assert!(src.contains("return h + up * sqrt(max(1.0 - dot(h, h), 0.0));"));
        assert!(src.contains("let d = max(dist - f.under.y, 0.0) * f.under.z;"));
    }
}
