//! The 3D pixel-art preview (docs/proposals/PIXEL_ART_LOOK.md): a prototype of the t3ssel8r-style
//! look, opt-in and off by default. Its palettes and presets are data
//! (`shared/look/pixelart.json`); `?pixelart=<preset>` picks one. The scene renders at a fixed low
//! height (`grid`), the composite bands the light (`light_uniforms`) and darkens a 1-pixel edge
//! (`edge_mode`), and a post pass (`pixel_art.wgsl`) snaps every low-res pixel to the palette with
//! a 4 × 4 Bayer ordered dither (`quantize`, its CPU twin, which the tests pin and the shader
//! follows line for line) and draws it on the canvas with a nearest-neighbour integer upscale.
//! This is the platform-free part: the tunables, the maths and the uniforms.

#[path = "pixelart.gen.rs"]
mod generated;
pub use generated::PIXEL_ART;

use crate::outline::{OutlineMode, OutlineTunables};

/// The post pass's shader (WGSL): the quantize, the dither and the upscale.
pub const POST_WGSL: &str = include_str!("pixel_art.wgsl");

/// The most colours a palette holds (`palette` in the post pass's block).
pub const MAX_COLORS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// The integer scale nearest the preset's height; the picture covers the canvas and the
    /// overhang (less than a low-res pixel each side) is cropped.
    Crop,
    /// The largest integer scale that fits the preset's height; bars around the picture.
    Letterbox,
}

/// How the block textures are sampled while a preset is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtlasFilter {
    /// Nearest texel of the full-size atlas (no mips).
    Point,
    /// Nearest texel of the nearest mip level.
    PointMips,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub name: &'static str,
    /// Ramps, dark to light; colours sRGB 0–1.
    pub ramps: &'static [&'static [[f64; 3]]],
}

impl Palette {
    /// Every colour, ramp by ramp.
    pub fn colors(&self) -> Vec<[f64; 3]> {
        self.ramps.iter().flat_map(|r| r.iter().copied()).collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelArtPreset {
    /// As `?pixelart=` names it.
    pub name: &'static str,
    /// The internal picture's height in low-res pixels.
    pub height: u32,
    pub fit: Fit,
    /// Its palette (an index into `PixelArtTunables::palettes`).
    pub palette: usize,
    /// The ordered dither's strength, 0 (nearest colour) to 1 (a full two-colour dither).
    pub dither: f64,
    /// The direct light's toon levels (0: smooth).
    pub bands: u32,
    /// The ambient light's toon levels (0: smooth).
    pub ambient_bands: u32,
    /// How far the unlit part of a surface takes `shadow_tint`'s hue (0–1).
    pub shadow_hue: f64,
    /// sRGB 0–1.
    pub shadow_tint: [f64; 3],
    /// The 1-pixel edge's kinds (dwell-look outline `EDGE_*` bits; 0 none).
    pub edges: u32,
    /// How much the edge darkens the fill (0–1).
    pub edge_darken: f64,
    /// The edge is gone by this distance (m).
    pub edge_fade_m: f64,
    pub atlas: AtlasFilter,
    /// Snap the eye to the low-res pixel grid on the plane `snap_depth_m` ahead.
    pub snap_camera: bool,
    pub snap_depth_m: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelArtTunables {
    pub palettes: &'static [Palette],
    pub presets: &'static [PixelArtPreset],
}

impl PixelArtTunables {
    /// The preset named `name`; none for `off` or a name no preset has.
    pub fn preset(&'static self, name: &str) -> Option<&'static PixelArtPreset> {
        self.presets.iter().find(|p| p.name == name)
    }
    pub fn palette_of(&self, p: &PixelArtPreset) -> &'static Palette {
        &self.palettes[p.palette]
    }
}

/// The preset a page's query asks for (`?pixelart=<name>`); none without it, for `off` or an
/// unknown name, so players see no change unless they ask for it.
pub fn preset_from_query(
    t: &'static PixelArtTunables,
    search: &str,
) -> Option<&'static PixelArtPreset> {
    search
        .trim_start_matches('?')
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == "pixelart")
        .and_then(|(_, v)| t.preset(v))
}

// ----- The low-res grid and the upscale -----

/// Low-res pixels drawn past the visible picture on each side: the room the camera snap's
/// sub-pixel shift moves the picture into.
pub const MARGIN: u32 = 1;

/// How the low-res picture lies on the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grid {
    /// The low-res target's size (the visible picture plus `MARGIN` each side).
    pub low: (u32, u32),
    /// The low-res pixels that show (before the crop).
    pub visible: (u32, u32),
    /// Canvas pixels per low-res pixel (each axis).
    pub scale: u32,
    /// Where the picture shows on the canvas: x0, y0, x1, y1 (canvas pixels; the bars outside).
    pub rect: [u32; 4],
    /// The canvas pixel of the low-res target's top-left corner (negative: off the canvas).
    pub origin: (i32, i32),
}

/// The grid for a `canvas` (drawing-buffer pixels) at a preset's `height` and `fit`.
pub fn grid(canvas: (u32, u32), height: u32, fit: Fit) -> Grid {
    let (w, h) = (canvas.0.max(1), canvas.1.max(1));
    let height = height.max(1);
    let (scale, visible, rect) = match fit {
        Fit::Letterbox => {
            let s = (h / height).max(1);
            let vis = ((w / s).max(1), height.min(h / s).max(1));
            let (pw, ph) = (vis.0 * s, vis.1 * s);
            let (x0, y0) = ((w - pw.min(w)) / 2, (h - ph.min(h)) / 2);
            (s, vis, [x0, y0, x0 + pw, y0 + ph])
        }
        Fit::Crop => {
            let s = ((f64::from(h) / f64::from(height)).round() as u32).max(1);
            (s, (w.div_ceil(s), h.div_ceil(s)), [0, 0, w, h])
        }
    };
    let low = (visible.0 + 2 * MARGIN, visible.1 + 2 * MARGIN);
    // The picture centred on its rect (the crop's overhang split, the odd pixel to the left/top).
    let cx = (rect[0] + rect[2]) as i32;
    let cy = (rect[1] + rect[3]) as i32;
    let origin = (
        (cx - (low.0 * scale) as i32).div_euclid(2),
        (cy - (low.1 * scale) as i32).div_euclid(2),
    );
    Grid {
        low,
        visible,
        scale,
        rect,
        origin,
    }
}

/// The low-res texel the canvas pixel `p` (its centre at `p + 0.5`) shows, with the picture shifted
/// by `shift` low-res pixels (the camera snap's remainder, rounded to whole canvas pixels); none in
/// the bars.
pub fn texel_at(g: &Grid, p: (u32, u32), shift: [f64; 2]) -> Option<(u32, u32)> {
    if p.0 < g.rect[0] || p.0 >= g.rect[2] || p.1 < g.rect[1] || p.1 >= g.rect[3] {
        return None;
    }
    let s = f64::from(g.scale);
    let off = shift_px(g, shift);
    let tx = ((f64::from(p.0) + 0.5 - f64::from(g.origin.0) - off[0]) / s).floor();
    let ty = ((f64::from(p.1) + 0.5 - f64::from(g.origin.1) - off[1]) / s).floor();
    let clamp = |v: f64, n: u32| v.clamp(0.0, f64::from(n - 1)) as u32;
    Some((clamp(tx, g.low.0), clamp(ty, g.low.1)))
}

/// The shift in whole canvas pixels: the picture moves by the snap's remainder at the canvas's
/// resolution, so no low-res pixel changes size.
pub fn shift_px(g: &Grid, shift: [f64; 2]) -> [f64; 2] {
    let s = f64::from(g.scale);
    shift.map(|v| (v.clamp(-0.5, 0.5) * s).round())
}

/// The camera snap: `eye` moved along the view's `right` and `up` (unit) onto the grid of
/// low-res pixels on the plane `depth_m` ahead (a pixel there is `2·depth·tan_y / visible_h`
/// metres), and the remainder in low-res pixels (x right, y down on the screen) that the upscale
/// shifts the picture by, so that plane holds still while the eye moves sideways. Only that plane:
/// nearer and farther things still crawl, and turning changes the grid itself.
pub fn snap_eye(
    eye: [f64; 3],
    right: [f64; 3],
    up: [f64; 3],
    tan_y: f64,
    visible_h: u32,
    depth_m: f64,
) -> ([f64; 3], [f64; 2]) {
    let pixel = 2.0 * depth_m * tan_y / f64::from(visible_h.max(1));
    if !(pixel.is_finite() && pixel > 0.0) {
        return (eye, [0.0; 2]);
    }
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let (er, eu) = (dot(eye, right), dot(eye, up));
    let (dr, du) = (
        (er / pixel).round() * pixel - er,
        (eu / pixel).round() * pixel - eu,
    );
    let snapped = std::array::from_fn(|i| eye[i] + dr * right[i] + du * up[i]);
    // Drawn from `snapped`, which is `dr` right of the eye: the scene there shows `dr` further
    // left than the eye would see it, so the picture moves right by `dr` (and down by `du`'s
    // opposite: the screen's y is down).
    (snapped, [dr / pixel, -du / pixel])
}

// ----- The palette, the dither and the quantize -----

fn srgb_to_linear(c: f64) -> f64 {
    crate::srgb_to_linear(c)
}

/// OKLab of an sRGB (0–1) colour.
pub fn oklab(srgb: [f64; 3]) -> [f64; 3] {
    let [r, g, b] = srgb.map(srgb_to_linear);
    let l = 0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b;
    let m = 0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b;
    let s = 0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b;
    let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
    [
        0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s,
    ]
}

/// The 4 × 4 Bayer matrix's threshold at pixel (x, y), in (0, 1).
pub fn bayer4(x: u32, y: u32) -> f64 {
    const B: [[u32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    (f64::from(B[(y % 4) as usize][(x % 4) as usize]) + 0.5) / 16.0
}

/// The palette index for colour `srgb` at low-res pixel (x, y): the nearest palette colour in
/// OKLab, or the second nearest where the colour lies far enough toward it along the line between
/// them for this pixel's Bayer threshold. `strength` 0 is the nearest colour everywhere; 1 a full
/// ordered dither between the two; between, only colours in the middle `strength` of the way
/// between two palette colours dither, so a flat area near a palette colour stays solid and the
/// checker shows where a gradient or a band edge crosses between two. `lab`: the palette in OKLab.
pub fn quantize(srgb: [f64; 3], x: u32, y: u32, lab: &[[f64; 3]], strength: f64) -> usize {
    let c = oklab(srgb);
    let d2 = |p: &[f64; 3]| (0..3).map(|i| (c[i] - p[i]).powi(2)).sum::<f64>();
    let (mut a, mut b) = (0usize, usize::MAX);
    let (mut da, mut db) = (f64::INFINITY, f64::INFINITY);
    for (i, p) in lab.iter().enumerate() {
        let d = d2(p);
        if d < da {
            (b, db) = (a, da);
            (a, da) = (i, d);
        } else if d < db {
            (b, db) = (i, d);
        }
    }
    if b == usize::MAX || strength <= 0.0 {
        return a;
    }
    let (pa, pb) = (lab[a], lab[b]);
    let ab: [f64; 3] = std::array::from_fn(|i| pb[i] - pa[i]);
    let len2 = ab.iter().map(|v| v * v).sum::<f64>().max(1e-12);
    let t = ((0..3).map(|i| (c[i] - pa[i]) * ab[i]).sum::<f64>() / len2).clamp(0.0, 1.0);
    let threshold = 0.5 + strength.min(1.0) * (bayer4(x, y) - 0.5);
    if t > threshold { b } else { a }
}

/// A light level `x` (0–1) on `n` toon levels from dark to lit (0: smooth), as the composite's
/// `band` (render/shaders.rs).
pub fn band(x: f64, n: u32) -> f64 {
    if n < 2 {
        return x;
    }
    let k = f64::from(n - 1);
    (x.clamp(0.0, 1.0) * k).round() / k
}

// ----- Uniforms -----

/// Floats the preview takes in the frame block (`pixel_art`, `pixel_tint`).
pub const LIGHT_FLOATS: usize = 8;

/// The frame block's light part (`pixel_art`, `pixel_tint` in render/shaders.rs): x the direct
/// light's levels, y the ambient's (0: smooth), z the shadow hue's strength; the shadow tint in
/// linear light at luminance 1 (it moves the hue, not the brightness). Zeros when off.
pub fn light_uniforms(p: Option<&PixelArtPreset>) -> [[f32; 4]; 2] {
    let Some(p) = p else {
        return [[0.0; 4]; 2];
    };
    let lin = p.shadow_tint.map(srgb_to_linear);
    let luma = (0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2]).max(1e-6);
    let t = lin.map(|v| (v / luma) as f32);
    [
        [
            p.bands as f32,
            p.ambient_bands as f32,
            p.shadow_hue as f32,
            0.0,
        ],
        [t[0], t[1], t[2], 0.0],
    ]
}

/// The edge as an ink-outline mode (dwell-look outline.rs): one low-res pixel's taps, black ink
/// at the darkening's strength (`mix(fill, 0, k)`: the fill darkened, its hue kept, then the
/// palette snaps it to a darker step); none without edges.
pub fn edge_mode(p: &PixelArtPreset) -> Option<OutlineMode> {
    (p.edges != 0).then(|| OutlineMode {
        name: p.name,
        edges: p.edges,
        width_px: 0.0,
        fade_start_m: p.edge_fade_m * 0.5,
        fade_end_m: p.edge_fade_m,
        strength: p.edge_darken,
        depth_threshold: 0.04,
        crease_threshold: 0.35,
    })
}

/// The edge's ink: black (it darkens the fill).
pub const EDGE_INK: OutlineTunables = OutlineTunables {
    ink: [0.0; 3],
    modes: &[],
};

/// Floats in the post pass's block (`Post` in pixel_art.wgsl): four vec4s and the palette twice
/// (OKLab, then sRGB).
pub const POST_FLOATS: usize = 16 + 2 * 4 * MAX_COLORS;

/// The post pass's block: the grid (scale, origin, colours), the visible rect, the dither and the
/// shift (whole canvas pixels), the bars' colour; the palette in OKLab and in sRGB.
pub fn post_uniforms(
    t: &PixelArtTunables,
    p: &PixelArtPreset,
    g: &Grid,
    shift: [f64; 2],
) -> Vec<f32> {
    let colors = t.palette_of(p).colors();
    let n = colors.len().min(MAX_COLORS);
    let off = shift_px(g, shift);
    let mut u = vec![0.0f32; POST_FLOATS];
    u[..4].copy_from_slice(&[
        g.scale as f32,
        g.origin.0 as f32,
        g.origin.1 as f32,
        n as f32,
    ]);
    u[4..8].copy_from_slice(&g.rect.map(|v| v as f32));
    u[8..12].copy_from_slice(&[p.dither as f32, off[0] as f32, off[1] as f32, 0.0]);
    // The bars: the palette's darkest colour.
    let dark = colors
        .iter()
        .copied()
        .min_by(|a, b| oklab(*a)[0].total_cmp(&oklab(*b)[0]))
        .unwrap_or([0.0; 3]);
    u[12..16].copy_from_slice(&[dark[0] as f32, dark[1] as f32, dark[2] as f32, 1.0]);
    for (i, c) in colors.iter().take(n).enumerate() {
        let l = oklab(*c);
        u[16 + 4 * i..16 + 4 * i + 3].copy_from_slice(&l.map(|v| v as f32));
        let s = 16 + 4 * MAX_COLORS + 4 * i;
        u[s..s + 3].copy_from_slice(&c.map(|v| v as f32));
    }
    u
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset(name: &str) -> &'static PixelArtPreset {
        PIXEL_ART.preset(name).unwrap()
    }

    fn lab_of(p: &Palette) -> Vec<[f64; 3]> {
        p.colors().into_iter().map(oklab).collect()
    }

    #[test]
    fn the_query_picks_a_preset_and_nothing_is_the_default() {
        assert_eq!(preset_from_query(&PIXEL_ART, ""), None);
        assert_eq!(preset_from_query(&PIXEL_ART, "?seed=3"), None);
        assert_eq!(preset_from_query(&PIXEL_ART, "?pixelart=off"), None);
        assert_eq!(preset_from_query(&PIXEL_ART, "?pixelart=nope"), None);
        for p in PIXEL_ART.presets {
            let q = format!("?local=1&pixelart={}&seed=3", p.name);
            assert_eq!(
                preset_from_query(&PIXEL_ART, &q).map(|p| p.name),
                Some(p.name)
            );
        }
    }

    #[test]
    fn palettes_are_32_to_48_colours_in_ramps() {
        for pal in PIXEL_ART.palettes {
            let n = pal.colors().len();
            assert!((32..=48).contains(&n), "{}: {n}", pal.name);
            assert!(n <= MAX_COLORS);
        }
    }

    #[test]
    fn the_crop_scale_is_the_integer_nearest_the_height_and_covers_the_canvas() {
        // 1440p at 270: ×5 (288 rows), the canvas covered.
        let g = grid((2560, 1440), 270, Fit::Crop);
        assert_eq!(g.scale, 5);
        assert_eq!(g.visible, (512, 288));
        assert_eq!(g.low, (514, 290));
        assert_eq!(g.rect, [0, 0, 2560, 1440]);
        assert_eq!(g.origin, (-5, -5), "the margin off the canvas");
        // An odd overhang: 1366 × 768 at 180: ×4, 342 × 192 cover 1368 × 768.
        let g = grid((1366, 768), 180, Fit::Crop);
        assert_eq!((g.scale, g.visible), (4, (342, 192)));
        assert_eq!(g.origin, (-5, -4));
        // Every canvas pixel shows a texel, and they are all `scale` wide.
        let mut runs = std::collections::BTreeMap::<u32, u32>::new();
        for x in 0..1366 {
            let t = texel_at(&g, (x, 0), [0.0; 2]).unwrap();
            *runs.entry(t.0).or_default() += 1;
        }
        let inner: Vec<u32> = runs.values().copied().collect();
        assert!(
            inner[1..inner.len() - 1].iter().all(|&n| n == 4),
            "{inner:?}"
        );
        // A canvas smaller than the height: ×1.
        assert_eq!(grid((200, 100), 270, Fit::Crop).scale, 1);
    }

    #[test]
    fn the_letterbox_fits_the_height_exactly_with_bars() {
        // 1440p at 270: ×5 is 1350 rows, 45 rows of bars above and below.
        let g = grid((2560, 1440), 270, Fit::Letterbox);
        assert_eq!(g.scale, 5);
        assert_eq!(g.visible, (512, 270));
        assert_eq!(g.rect, [0, 45, 2560, 1395]);
        assert_eq!(texel_at(&g, (10, 44), [0.0; 2]), None, "a bar");
        assert_eq!(texel_at(&g, (10, 1395), [0.0; 2]), None, "a bar");
        assert_eq!(
            texel_at(&g, (0, 45), [0.0; 2]),
            Some((1, 1)),
            "past the margin"
        );
        assert_eq!(texel_at(&g, (2559, 1394), [0.0; 2]), Some((512, 270)));
        // 1920 × 1080 at 270: ×4 exactly, no bars.
        let g = grid((1920, 1080), 270, Fit::Letterbox);
        assert_eq!(
            (g.scale, g.visible, g.rect),
            (4, (480, 270), [0, 0, 1920, 1080])
        );
        // 1000 wide at ×3: 333 columns, a pixel of bar on the left.
        let g = grid((1000, 810), 270, Fit::Letterbox);
        assert_eq!(
            (g.scale, g.visible.0, g.rect[0], g.rect[2]),
            (3, 333, 0, 999)
        );
    }

    #[test]
    fn the_shift_moves_the_picture_by_whole_canvas_pixels_within_the_margin() {
        let g = grid((1920, 1080), 270, Fit::Letterbox);
        assert_eq!(shift_px(&g, [0.3, -0.2]), [1.0, -1.0]);
        assert_eq!(
            shift_px(&g, [2.0, 0.0]),
            [2.0, 0.0],
            "never past half a pixel"
        );
        // Shifted right by half a pixel, the leftmost column shows the margin's texel for 2 px.
        assert_eq!(texel_at(&g, (0, 0), [0.5, 0.0]), Some((0, 1)));
        assert_eq!(texel_at(&g, (2, 0), [0.5, 0.0]), Some((1, 1)));
    }

    #[test]
    fn the_snap_holds_the_plane_still_while_the_eye_moves_sideways() {
        let (right, up) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let tan_y = (35f64).to_radians().tan();
        let pixel = 2.0 * 8.0 * tan_y / 270.0;
        for k in 0..40 {
            let eye = [100.0 + k as f64 * 0.013, 64.0 + k as f64 * 0.007, 5.0];
            let (s, shift) = snap_eye(eye, right, up, tan_y, 270, 8.0);
            // On the grid.
            assert!(((s[0] / pixel) - (s[0] / pixel).round()).abs() < 1e-6);
            assert!(((s[1] / pixel) - (s[1] / pixel).round()).abs() < 1e-6);
            assert_eq!(s[2], eye[2], "nothing along the view");
            assert!(shift[0].abs() <= 0.5 + 1e-9 && shift[1].abs() <= 0.5 + 1e-9);
            // A point on the plane, as drawn from the snapped eye and shifted, is where the eye
            // sees it (in low-res pixels, screen x right, y down).
            let seen = |e: [f64; 3]| [(0.0 - e[0]) / pixel, -(0.0 - e[1]) / pixel];
            let (a, b) = (seen(eye), seen(s));
            assert!(
                (b[0] + shift[0] - a[0]).abs() < 1e-6,
                "x: {b:?} {shift:?} {a:?}"
            );
            assert!((b[1] + shift[1] - a[1]).abs() < 1e-6, "y");
        }
    }

    #[test]
    fn quantize_and_dither_give_palette_colours_only() {
        for pal in PIXEL_ART.palettes {
            let colors = pal.colors();
            let lab = lab_of(pal);
            for strength in [0.0, 0.5, 1.0] {
                // A sweep through the colour cube, and a sky gradient.
                for k in 0..512u32 {
                    let c = [
                        f64::from(k % 8) / 7.0,
                        f64::from(k / 8 % 8) / 7.0,
                        f64::from(k / 64) / 7.0,
                    ];
                    let i = quantize(c, k % 7, k % 5, &lab, strength);
                    assert!(i < colors.len());
                }
            }
        }
    }

    #[test]
    fn a_palette_colour_stays_solid_and_a_midpoint_dithers() {
        let pal = &PIXEL_ART.palettes[0];
        let (colors, lab) = (pal.colors(), lab_of(pal));
        // Flat areas on a palette colour: solid at every strength.
        for (i, c) in colors.iter().enumerate() {
            for (x, y) in [(0, 0), (1, 2), (3, 3)] {
                assert_eq!(quantize(*c, x, y, &lab, 1.0), i);
            }
        }
        // Halfway between two ramp neighbours: a checker of the two, about half each, at full
        // strength; none at 0.
        let (a, b) = (colors[5], colors[6]);
        let (ia, ib) = (5, 6);
        let mid_lab: [f64; 3] = std::array::from_fn(|i| 0.5 * (lab[ia][i] + lab[ib][i]));
        // Search an sRGB colour whose OKLab is near the midpoint (the midpoint in sRGB is close).
        let mid = std::array::from_fn(|i| 0.5 * (a[i] + b[i]));
        let near = oklab(mid);
        assert!(
            (0..3).map(|i| (near[i] - mid_lab[i]).abs()).sum::<f64>() < 0.05,
            "{near:?} {mid_lab:?}"
        );
        let picks: Vec<usize> = (0..16)
            .map(|k| quantize(mid, k % 4, k / 4, &lab, 1.0))
            .collect();
        assert!(picks.iter().all(|&i| i == ia || i == ib), "{picks:?}");
        let nb = picks.iter().filter(|&&i| i == ib).count();
        assert!((4..=12).contains(&nb), "{picks:?}");
        let plain = quantize(mid, 0, 0, &lab, 0.0);
        assert!((0..16).all(|k| quantize(mid, k % 4, k / 4, &lab, 0.0) == plain));
    }

    #[test]
    fn the_bayer_matrix_holds_every_threshold_once() {
        let mut t: Vec<f64> = (0..16).map(|k| bayer4(k % 4, k / 4)).collect();
        t.sort_by(f64::total_cmp);
        for (k, v) in t.iter().enumerate() {
            assert!((v - (k as f64 + 0.5) / 16.0).abs() < 1e-12);
        }
        assert_eq!(bayer4(5, 6), bayer4(1, 2), "it tiles");
    }

    #[test]
    fn bands_step_the_light_and_zero_is_smooth() {
        assert_eq!(band(0.37, 0), 0.37);
        assert_eq!(band(0.2, 3), 0.0);
        assert_eq!(band(0.3, 3), 0.5);
        assert_eq!(band(0.8, 3), 1.0);
        assert_eq!(band(0.4, 4), 1.0 / 3.0);
    }

    #[test]
    fn off_packs_zeros_and_a_preset_packs_its_light() {
        assert_eq!(light_uniforms(None), [[0.0; 4]; 2]);
        let p = preset("180");
        let u = light_uniforms(Some(p));
        assert_eq!(
            u[0][..3],
            [p.bands as f32, p.ambient_bands as f32, p.shadow_hue as f32]
        );
        let luma = 0.2126 * u[1][0] + 0.7152 * u[1][1] + 0.0722 * u[1][2];
        assert!((luma - 1.0).abs() < 1e-5, "a hue, not a brightness");
        assert!(u[1][2] > u[1][0], "the tint is cool");
    }

    #[test]
    fn the_edge_is_one_low_res_pixel_and_darkens() {
        let p = preset("180");
        let m = edge_mode(p).unwrap();
        assert_eq!(crate::outline::tap_px(m.width_px, 1.0), 1.0);
        let u = crate::outline::outline_uniforms(&EDGE_INK, Some(&m), 1.0, 1.0);
        assert_eq!(u[1], [0.0, 0.0, 0.0, p.edge_darken as f32]);
        assert_eq!(edge_mode(preset("360")), None, "no edges");
    }

    #[test]
    fn the_post_block_holds_the_grid_and_the_palette() {
        let p = preset("270");
        let g = grid((1920, 1080), p.height, p.fit);
        let u = post_uniforms(&PIXEL_ART, p, &g, [0.0; 2]);
        assert_eq!(u.len(), POST_FLOATS);
        let n = PIXEL_ART.palette_of(p).colors().len();
        assert_eq!(u[..4], [4.0, -4.0, -4.0, n as f32]);
        assert_eq!(u[4..8], [0.0, 0.0, 1920.0, 1080.0]);
        let first = PIXEL_ART.palette_of(p).colors()[0];
        assert_eq!(
            u[16 + 4 * MAX_COLORS..16 + 4 * MAX_COLORS + 3],
            first.map(|v| v as f32)
        );
        assert_eq!(u[16..19], oklab(first).map(|v| v as f32));
    }

    #[test]
    fn the_post_shader_is_valid_wgsl_and_its_block_matches() {
        let module = naga::front::wgsl::parse_str(POST_WGSL).expect("pixel_art.wgsl parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("pixel_art.wgsl validates");
        assert!(POST_WGSL.contains(&format!("array<vec4f, {MAX_COLORS}>")));
    }
}
