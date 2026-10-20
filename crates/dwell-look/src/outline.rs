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

/// One axis's edge strength (0–1) from three taps of inverse view depth (`1 / depth`; 0 where
/// nothing is drawn: the sky is at infinity), the pixel's in the middle. Inverse depth is linear
/// across the screen on any plane, so its second difference is zero on a flat face at any angle:
/// a silhouette is a large one (relative to the nearest tap) and a crease is a change of slope
/// (relative to the steeper side).
pub fn axis_edge(w: [f64; 3], edges: u32, depth_t: f64, crease_t: f64) -> f64 {
    let [wl, w0, wr] = w;
    let top = wl.max(w0).max(wr);
    if top <= 0.0 {
        return 0.0;
    }
    let mut s: f64 = 0.0;
    if edges & EDGE_DEPTH != 0 {
        let e = (wl + wr - 2.0 * w0).abs() / top;
        s = s.max(smoothstep(depth_t, 2.0 * depth_t, e));
    }
    if edges & EDGE_NORMAL != 0 && wl.min(w0).min(wr) > 0.0 {
        let (sl, sr) = (w0 - wl, wr - w0);
        let c = (sr - sl).abs() / (sl.abs().max(sr.abs()) + 1e-4 * top);
        s = s.max(smoothstep(crease_t, 2.0 * crease_t, c));
    }
    s
}

/// The outline at a pixel from its view depth and its four neighbours' at the tap distance
/// (`[left, right, up, down]`; 0: nothing drawn), with `ray_len` the length of its view ray per
/// unit of view depth: (opacity, the edge's view depth: its nearest tap). The depth and normal
/// kinds of the shader's rule; the block kind needs positions and stays in the shader.
pub fn edge_at(depth: f64, around: [f64; 4], ray_len: f64, m: &OutlineMode) -> (f64, f64) {
    let inv = |d: f64| if d > 0.0 { 1.0 / d } else { 0.0 };
    let w0 = inv(depth);
    let [l, r, u, d] = around.map(inv);
    let (dt, ct) = (m.depth_threshold, m.crease_threshold);
    let s = axis_edge([l, w0, r], m.edges, dt, ct).max(axis_edge([u, w0, d], m.edges, dt, ct));
    let near = w0.max(l).max(r).max(u).max(d);
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

    #[test]
    fn the_query_picks_a_mode_and_nothing_is_the_default() {
        assert_eq!(mode_from_query(&OUTLINE, ""), None);
        assert_eq!(mode_from_query(&OUTLINE, "?seed=3"), None);
        assert_eq!(mode_from_query(&OUTLINE, "?outline=off"), None);
        assert_eq!(mode_from_query(&OUTLINE, "?outline=nope"), None);
        for name in ["silhouette", "creases", "blocks"] {
            let q = format!("?local=1&outline={name}&textures=painterly");
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
        for slope in [0.0, 1e-4, 0.01, 0.05] {
            let w = [0.1 - slope, 0.1, 0.1 + slope];
            assert!(
                axis_edge(w, EDGE_DEPTH | EDGE_NORMAL, 0.12, 0.6) < 1e-9,
                "slope {slope}"
            );
        }
    }

    #[test]
    fn a_silhouette_and_a_crease_are_edges() {
        // A block at 10 m in front of ground at 20 m, and against the sky.
        assert_eq!(axis_edge([0.1, 0.1, 0.05], EDGE_DEPTH, 0.12, 0.6), 1.0);
        assert_eq!(axis_edge([0.1, 0.1, 0.0], EDGE_DEPTH, 0.12, 0.6), 1.0);
        // A wall facing the eye (flat in depth) meeting a floor that recedes: only creases see it.
        let crease = [0.1, 0.1, 0.1 - 0.0006];
        assert!(axis_edge(crease, EDGE_DEPTH, 0.12, 0.6) < 0.01);
        assert!(axis_edge(crease, EDGE_NORMAL, 0.12, 0.6) > 0.5);
    }

    #[test]
    fn far_pixels_get_no_outline() {
        // A block's silhouette against the sky: inked near, faded by the fade's end.
        for name in ["silhouette", "creases", "blocks"] {
            let m = mode(name);
            let at = |d: f64| edge_at(d, [d, 0.0, d, d], 1.0, m).0;
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
        assert_eq!(edge_at(d, [d, 0.0, d, d], 1.5, m).0, 0.0);
        // The sky alone has no edge.
        assert_eq!(edge_at(0.0, [0.0; 4], 1.0, m), (0.0, 0.0));
    }

    #[test]
    fn the_edge_is_as_near_as_its_nearest_tap() {
        // A sky pixel beside a block 5 m away is the block's silhouette, at its depth.
        let (a, d) = edge_at(0.0, [5.0, 0.0, 0.0, 0.0], 1.0, mode("silhouette"));
        assert_eq!(d, 5.0);
        assert!(a > 0.0);
    }
}
