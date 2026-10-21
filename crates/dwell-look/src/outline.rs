//! The ink outline (docs/ART_DIRECTION.md §7): a thin dark line the composite draws where the
//! frame's edges are, after the Character Lab's ink outline. Its modes are data
//! (`shared/look/outline.json`); `?outline=<mode>` picks one, and none is the default. This is the
//! platform-free part: the tunables, the mode a query picks, the uniforms the composite reads and a
//! CPU twin of the shader's edge rule (`ink_outline` in dwell-client's render/shaders.rs), which
//! the tests pin and the shader follows line for line.

#[path = "outline.gen.rs"]
mod generated;
pub use generated::OUTLINE;

/// Edge kinds (`OutlineMode::edges` bits): silhouettes, where the depth jumps.
pub const EDGE_DEPTH: u32 = 1;
/// Creases, where the surface turns (its slope in depth changes).
pub const EDGE_NORMAL: u32 = 2;
/// Block edges, where the voxel cell under the pixel changes.
pub const EDGE_BLOCK: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutlineMode {
    /// As `?outline=` names it.
    pub name: &'static str,
    /// The edge kinds it draws (`EDGE_*` bits).
    pub edges: u32,
    /// The line's width in CSS pixels.
    pub width_px: f64,
    /// Full strength out to here, none from `fade_end_m` on (metres from the eye).
    pub fade_start_m: f64,
    pub fade_end_m: f64,
    /// The line's opacity (0–1).
    pub strength: f64,
    /// The jump in inverse depth (relative to the nearer side) that starts a silhouette.
    pub depth_threshold: f64,
    /// The change of the depth's slope (relative to the steeper side) that starts a crease.
    pub crease_threshold: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutlineTunables {
    /// The line's colour (sRGB 0–1, output colour), the palette's `ink`.
    pub ink: [f64; 3],
    pub modes: &'static [OutlineMode],
}

impl OutlineTunables {
    /// The mode named `name`; none for `off` or a name no mode has.
    pub fn mode(&'static self, name: &str) -> Option<&'static OutlineMode> {
        self.modes.iter().find(|m| m.name == name)
    }
}

/// The mode a page's query asks for (`?outline=<name>`); none without it, for `off` or an unknown
/// name, so players see no outline unless they ask for one.
pub fn mode_from_query(t: &'static OutlineTunables, search: &str) -> Option<&'static OutlineMode> {
    search
        .trim_start_matches('?')
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == "outline")
        .and_then(|(_, v)| t.mode(v))
}

/// Floats the outline takes in the frame block (`outline`, `outline_ink`, `outline_cfg`).
pub const OUTLINE_FLOATS: usize = 12;

/// The frame block's outline (`outline*` in render/shaders.rs): x the edge kinds (0: off), y the
/// taps' distance in drawing-buffer pixels (half the width, at least one), z and w the fade's start
/// and end (m) scaled by `zoom` (the spyglass's magnification: a zoomed view keeps its lines as far
/// as it brings things closer); the ink (sRGB) and strength; the two thresholds.
pub fn outline_uniforms(
    t: &OutlineTunables,
    mode: Option<&OutlineMode>,
    pixel_ratio: f64,
    zoom: f64,
) -> [[f32; 4]; 3] {
    let Some(m) = mode else {
        return [[0.0; 4]; 3];
    };
    let zoom = zoom.max(1.0);
    let f = |v: f64| v as f32;
    [
        [
            m.edges as f32,
            f(tap_px(m.width_px, pixel_ratio)),
            f(m.fade_start_m * zoom),
            f(m.fade_end_m * zoom),
        ],
        [f(t.ink[0]), f(t.ink[1]), f(t.ink[2]), f(m.strength)],
        [f(m.depth_threshold), f(m.crease_threshold), 0.0, 0.0],
    ]
}

/// The taps' distance (drawing-buffer pixels) for a line `width_px` CSS pixels wide: a tap either
/// side of the edge marks the pixels within it on both sides, so half the width, at least one.
pub fn tap_px(width_px: f64, pixel_ratio: f64) -> f64 {
    (width_px * pixel_ratio.max(0.01) * 0.5).round().max(1.0)
}

fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How much of the line is left at `distance_m` from the eye: all of it to the fade's start, none
/// from its end.
pub fn distance_fade(distance_m: f64, start_m: f64, end_m: f64) -> f64 {
    1.0 - smoothstep(start_m, end_m, distance_m)
}

/// How much two slopes of inverse depth differ: 0 equal, 1 where one is flat or they have opposite
/// signs. `top` (the nearest tap's inverse depth) scales the floor that keeps two near-flat slopes
/// from reading as a turn.
pub fn slope_change(a: f64, b: f64, top: f64) -> f64 {
    (a - b).abs() / (a.abs() + b.abs() + 1e-4 * top)
}

/// The edge strength (0–1) from a 3 × 3 of inverse view depths (`1 / depth`; 0 where nothing is
/// drawn: the sky is at infinity) at the tap distance, row by row from the top left, the pixel's in
/// the middle. Inverse depth is linear across the screen on any plane, so a flat face at any angle
/// has no edge: a silhouette is a large second difference across the pixel (relative to the
/// nearest tap) and a crease a change of slope along the middle row or column or between the outer
/// ones (the surface's normal turns: a block's side against the ground it stands on, where the
/// depth barely jumps).
pub fn edge_strength(w: [f64; 9], edges: u32, depth_t: f64, crease_t: f64) -> f64 {
    let top = w.iter().copied().fold(0.0, f64::max);
    let low = w.iter().copied().fold(f64::INFINITY, f64::min);
    if top <= 0.0 {
        return 0.0;
    }
    let mut s: f64 = 0.0;
    if edges & EDGE_DEPTH != 0 {
        let e = (w[3] + w[5] - 2.0 * w[4])
            .abs()
            .max((w[1] + w[7] - 2.0 * w[4]).abs())
            / top;
        s = s.max(smoothstep(depth_t, 2.0 * depth_t, e));
    }
    if edges & EDGE_NORMAL != 0 && low > 0.0 {
        let c = slope_change(w[4] - w[3], w[5] - w[4], top)
            .max(slope_change(w[4] - w[1], w[7] - w[4], top))
            .max(slope_change(w[6] - w[0], w[8] - w[2], top))
            .max(slope_change(w[2] - w[0], w[8] - w[6], top));
        s = s.max(smoothstep(crease_t, 2.0 * crease_t, c));
    }
    s
}

/// No tap on a surface that takes no ink (`edge_at`'s `bare`).
pub const NONE_BARE: [bool; 9] = [false; 9];

/// The 3 × 3 of inverse depths with the taps on a surface that takes no ink (`bare`: block data
/// `outline: false`, the cross plants) seen through (docs/ART_DIRECTION.md §7.4): such a tap takes
/// what the plane through the pixel and the opposite tap gives, so the pair has no edge, or the
/// pixel's own where the opposite tap is bare too; never nearer than the sky (0). The frame has no
/// depth behind a plant, so this is the stand-in for it.
pub fn see_through(w: [f64; 9], bare: [bool; 9]) -> [f64; 9] {
    std::array::from_fn(|k| {
        if !bare[k] {
            w[k]
        } else if bare[8 - k] {
            w[4]
        } else {
            (2.0 * w[4] - w[8 - k]).max(0.0)
        }
    })
}

/// The outline at a pixel from the view depths of the 3 × 3 around it at the tap distance (row by
/// row from the top left, the pixel's in the middle; 0: nothing drawn), with `ray_len` the length
/// of its view ray per unit of view depth: (opacity, the edge's view depth: its nearest tap). Taps
/// on a surface that takes no ink (`bare`) are seen through, and a bare pixel has no ink. The
/// depth and normal kinds of the shader's rule; the block kind needs positions and stays in the
/// shader.
pub fn edge_at(depths: [f64; 9], bare: [bool; 9], ray_len: f64, m: &OutlineMode) -> (f64, f64) {
    if bare[4] {
        return (0.0, 0.0);
    }
    let w = see_through(depths.map(|d| if d > 0.0 { 1.0 / d } else { 0.0 }), bare);
    let s = edge_strength(w, m.edges, m.depth_threshold, m.crease_threshold);
    let near = w.iter().copied().fold(0.0, f64::max);
    if near <= 0.0 {
        return (0.0, 0.0);
    }
    let edge_depth = 1.0 / near;
    let fade = distance_fade(edge_depth * ray_len, m.fade_start_m, m.fade_end_m);
    (s * fade * m.strength, edge_depth)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(name: &str) -> &'static OutlineMode {
        OUTLINE.mode(name).unwrap()
    }

    /// Inverse depths of the plane `w = a + bx·x + by·y` at the 3 × 3 (x right, y down).
    fn plane(a: f64, bx: f64, by: f64) -> [f64; 9] {
        std::array::from_fn(|k| a + bx * (k % 3) as f64 - bx + by * (k / 3) as f64 - by)
    }

    /// A 3 × 3 of view depths: the left column `left`, the rest `rest`.
    fn split(left: f64, rest: f64) -> [f64; 9] {
        std::array::from_fn(|k| if k % 3 == 0 { left } else { rest })
    }

    #[test]
    fn the_query_picks_a_mode_and_nothing_is_the_default() {
        assert_eq!(mode_from_query(&OUTLINE, ""), None);
        assert_eq!(mode_from_query(&OUTLINE, "?seed=3"), None);
        assert_eq!(mode_from_query(&OUTLINE, "?outline=off"), None);
        assert_eq!(mode_from_query(&OUTLINE, "?outline=nope"), None);
        for name in ["silhouette", "creases", "blocks"] {
            let q = format!("?local=1&outline={name}&seed=3");
            assert_eq!(mode_from_query(&OUTLINE, &q).map(|m| m.name), Some(name));
        }
    }

    #[test]
    fn the_modes_add_edge_kinds_in_turn() {
        assert_eq!(mode("silhouette").edges, EDGE_DEPTH);
        assert_eq!(mode("creases").edges, EDGE_DEPTH | EDGE_NORMAL);
        assert_eq!(mode("blocks").edges, EDGE_DEPTH | EDGE_NORMAL | EDGE_BLOCK);
    }

    #[test]
    fn off_packs_zeros_and_a_mode_packs_its_tunables() {
        assert_eq!(outline_uniforms(&OUTLINE, None, 2.0, 1.0), [[0.0; 4]; 3]);
        let m = mode("creases");
        let u = outline_uniforms(&OUTLINE, Some(m), 2.0, 4.0);
        assert_eq!(u[0][0], 3.0, "depth and normal bits");
        assert_eq!(u[0][1] as f64, tap_px(m.width_px, 2.0));
        assert_eq!(
            u[0][2] as f64,
            m.fade_start_m * 4.0,
            "the spyglass carries the lines out"
        );
        assert_eq!(u[0][3] as f64, m.fade_end_m * 4.0);
        assert_eq!(
            u[1],
            [
                OUTLINE.ink[0] as f32,
                OUTLINE.ink[1] as f32,
                OUTLINE.ink[2] as f32,
                m.strength as f32
            ]
        );
        assert_eq!(
            u[2][..2],
            [m.depth_threshold as f32, m.crease_threshold as f32]
        );
        // A wider view than the screen's (the speed boost) never pulls the fade in.
        assert_eq!(
            outline_uniforms(&OUTLINE, Some(m), 2.0, 0.8)[0][2] as f64,
            m.fade_start_m
        );
    }

    #[test]
    fn the_width_is_in_css_pixels() {
        assert_eq!(tap_px(1.0, 1.0), 1.0, "never less than a pixel's tap");
        assert_eq!(tap_px(2.0, 2.0), 2.0);
        assert_eq!(tap_px(4.0, 1.0), 2.0);
        assert_eq!(tap_px(4.0, 2.0), 4.0);
    }

    #[test]
    fn a_flat_face_has_no_edge_at_any_angle() {
        // Inverse depth is linear across the screen on a plane.
        let all = EDGE_DEPTH | EDGE_NORMAL;
        for (bx, by) in [(0.0, 0.0), (1e-4, 0.0), (0.0, 0.01), (0.003, -0.02)] {
            let s = edge_strength(plane(0.1, bx, by), all, 0.04, 0.35);
            assert!(s < 1e-9, "slope {bx}, {by}: {s}");
        }
    }

    #[test]
    fn a_silhouette_and_a_crease_are_edges() {
        let all = EDGE_DEPTH | EDGE_NORMAL;
        // A block at 10 m in front of ground at 20 m, and against the sky.
        assert_eq!(edge_strength(split(0.05, 0.1), EDGE_DEPTH, 0.04, 0.35), 1.0);
        assert_eq!(edge_strength(split(0.0, 0.1), EDGE_DEPTH, 0.04, 0.35), 1.0);
        // A block's side facing the eye (flat in depth) against the ground beside it, which
        // recedes up the screen and meets it at the same depth in the middle row: the depth hardly
        // jumps, so only the turn of the surface finds it (the columns' slopes differ).
        let mut w = plane(0.1, 0.0, 0.0);
        for k in [0, 3, 6] {
            w[k] = 0.1 + 0.0006 * ((k / 3) as f64 - 1.0);
        }
        assert!(edge_strength(w, EDGE_DEPTH, 0.04, 0.35) < 0.01);
        assert!(edge_strength(w, all, 0.04, 0.35) > 0.9);
    }

    #[test]
    fn far_pixels_get_no_outline() {
        // A block's silhouette against the sky: inked near, faded by the fade's end.
        for name in ["silhouette", "creases", "blocks"] {
            let m = mode(name);
            let at = |d: f64| edge_at(split(0.0, d), NONE_BARE, 1.0, m).0;
            assert!(
                (at(m.fade_start_m * 0.5) - m.strength).abs() < 1e-9,
                "{name}: full near"
            );
            let mid = at(0.5 * (m.fade_start_m + m.fade_end_m));
            assert!(mid > 0.0 && mid < m.strength, "{name}: fading");
            assert_eq!(at(m.fade_end_m), 0.0, "{name}: none at the fade's end");
            assert_eq!(at(m.fade_end_m * 4.0), 0.0, "{name}: none beyond it");
        }
        // The distance is along the ray (an off-axis pixel is farther than its view depth).
        let m = mode("silhouette");
        let d = m.fade_end_m * 0.8;
        assert_eq!(edge_at(split(0.0, d), NONE_BARE, 1.5, m).0, 0.0);
        // The sky alone has no edge.
        assert_eq!(edge_at([0.0; 9], NONE_BARE, 1.0, m), (0.0, 0.0));
    }

    #[test]
    fn the_edge_is_as_near_as_its_nearest_tap() {
        // A sky pixel beside a block 5 m away is the block's silhouette, at its depth.
        let mut d = [0.0; 9];
        d[3] = 5.0;
        let (a, depth) = edge_at(d, NONE_BARE, 1.0, mode("silhouette"));
        assert_eq!(depth, 5.0);
        assert!(a > 0.0);
    }

    #[test]
    fn plants_that_take_no_ink_are_seen_through() {
        // Owner's report (2026-10-20, `creases`): tall grass turned fields into black scribble.
        // A blade 4 m away in front of ground at 12 m: drawn as a silhouette when inked, nothing
        // when the blade is bare (its edge against the ground), and none on the blade itself.
        let m = mode("creases");
        let ground = |k: usize| 12.0 + 0.3 * ((k / 3) as f64 - 1.0);
        let mut d: [f64; 9] = std::array::from_fn(ground);
        d[5] = 4.0;
        let mut bare = NONE_BARE;
        assert!(
            edge_at(d, bare, 1.0, m).0 > 0.5,
            "an inked plant has its line"
        );
        bare[5] = true;
        assert_eq!(
            edge_at(d, bare, 1.0, m).0,
            0.0,
            "the ground beside a bare plant"
        );
        let mut on = NONE_BARE;
        on[4] = true;
        assert_eq!(edge_at(d, on, 1.0, m), (0.0, 0.0), "no ink on the plant");
        // Against the sky: a sky pixel beside a bare blade, and a blade on both sides of it.
        let mut sky = [0.0; 9];
        sky[3] = 4.0;
        sky[5] = 5.0;
        let both = std::array::from_fn(|k| k == 3 || k == 5);
        assert_eq!(edge_at(sky, both, 1.0, m).0, 0.0);
        let mut one = NONE_BARE;
        one[3] = true;
        sky[5] = 0.0;
        assert_eq!(edge_at(sky, one, 1.0, m).0, 0.0, "seen through to the sky");
        // A real edge beside a bare blade still draws: a block's silhouette against the sky.
        let mut block = split(0.0, 6.0);
        block[7] = 3.0;
        let mut b = NONE_BARE;
        b[7] = true;
        assert!(edge_at(block, b, 1.0, m).0 > 0.5);
        // The plane through the pixel stands in for a bare tap: a flat face stays flat.
        let w: [f64; 9] =
            std::array::from_fn(|k| 0.1 + 0.002 * (k % 3) as f64 + 0.001 * (k / 3) as f64);
        let mut b = NONE_BARE;
        b[0] = true;
        b[5] = true;
        assert_eq!(
            see_through(w, b).map(|v| (v * 1e9).round()),
            w.map(|v| (v * 1e9).round())
        );
    }
}
