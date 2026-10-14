//! Rendering switches from the address (port of render/display.ts): `?scale=0.5` renders at half
//! the resolution. `?batch=1` is accepted and ignored: batching was a WebGL draw-call workaround;
//! the wgpu renderer draws a mesh each.

/// The last frame's work (the debug overlay).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RenderStats {
    /// Draw calls and triangles over both passes.
    pub calls: u32,
    pub triangles: u64,
    pub batched: bool,
    /// Drawing-buffer pixels per CSS pixel (device pixel ratio × ?scale).
    pub pixel_ratio: f64,
    /// GPU memory (bytes): terrain and LOD geometry, and the drawing buffers' estimate.
    pub mesh_bytes: u64,
    pub screen_bytes: u64,
}

pub const SCALE_MIN: f64 = 0.25;
pub const SCALE_MAX: f64 = 2.0;

/// The resolution scale from a query string, clamped (1 if absent or not a number).
pub fn display_scale(search: &str) -> f64 {
    search
        .trim_start_matches('?')
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == "scale")
        .and_then(|(_, v)| v.parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .map_or(1.0, |v| v.clamp(SCALE_MIN, SCALE_MAX))
}

/// The F3 line: "render 712 draws · 1.23 M tris · meshes · 2.00× pixels".
pub fn format_render_stats(s: &RenderStats) -> String {
    let tris = if s.triangles >= 1_000_000 {
        format!("{:.2} M", s.triangles as f64 / 1e6)
    } else {
        format!("{:.0} k", s.triangles as f64 / 1e3)
    };
    format!(
        "render {} draws · {tris} tris · {} · {:.2}× pixels",
        s.calls,
        if s.batched { "batched" } else { "meshes" },
        s.pixel_ratio
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_resolution_scale_clamped() {
        assert_eq!(display_scale(""), 1.0);
        assert_eq!(display_scale("?local=1&batch=1&scale=0.5"), 0.5);
        assert_eq!(display_scale("?batch=0&scale=0.01"), 0.25);
        assert_eq!(display_scale("?scale=9"), 2.0);
        assert_eq!(display_scale("?scale=lots"), 1.0);
    }

    #[test]
    fn formats_the_frames_work() {
        let s = RenderStats {
            calls: 712,
            triangles: 1_234_567,
            pixel_ratio: 2.0,
            ..Default::default()
        };
        assert_eq!(
            format_render_stats(&s),
            "render 712 draws · 1.23 M tris · meshes · 2.00× pixels"
        );
        let s = RenderStats {
            calls: 9,
            triangles: 48_900,
            batched: true,
            pixel_ratio: 1.0,
            ..Default::default()
        };
        assert_eq!(
            format_render_stats(&s),
            "render 9 draws · 49 k tris · batched · 1.00× pixels"
        );
    }
}
