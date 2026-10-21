//! Smooth LOD transitions (ARCHITECTURE.md §6.6 "Swaps"): the tunables from
//! `shared/look/lodblend.json` — the cross-fade's length, its per-pixel dither pattern and the
//! selection's coarsening hysteresis — and the pattern's threshold, in Rust (the CPU twin, for
//! tests) and in WGSL (the renderer's `fade_threshold`). The swap state machine is `dwell-lod`'s
//! `fade.rs`; the renderer's scene shaders discard by `fade_threshold` against each draw's fade.

#[path = "lodblend.gen.rs"]
mod generated;
pub use generated::LOD_BLEND;

/// A stable screen-space threshold per pixel in [0, 1): the dither the cross-fade discards by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FadePattern {
    /// A 4 × 4 ordered (Bayer) dither: 16 steps.
    Bayer4,
    /// An 8 × 8 ordered dither: 64 steps.
    Bayer8,
    /// Interleaved gradient noise (no visible tile; uneven steps).
    Noise,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodBlendTunables {
    /// Seconds a swap cross-fades over (0: in one frame).
    pub fade_s: f64,
    pub pattern: FadePattern,
    /// A refined section coarsens only once its cells project under the pixel error / this.
    pub coarsen_ratio: f64,
}

impl FadePattern {
    /// The Bayer matrix's bits per axis (0 for the noise).
    fn bits(self) -> u32 {
        match self {
            Self::Bayer4 => 2,
            Self::Bayer8 => 3,
            Self::Noise => 0,
        }
    }

    /// The threshold at pixel (x, y) (integer pixel coordinates), in [0, 1). The WGSL
    /// `fade_threshold` computes the same.
    pub fn threshold(self, x: u32, y: u32) -> f32 {
        if self == Self::Noise {
            let d = x as f32 * 0.067_110_56 + y as f32 * 0.005_837_15;
            return (52.982_918 * d.fract()).fract();
        }
        let bits = self.bits();
        let mut v = 0u32;
        for i in 0..bits {
            let (a, b) = ((x >> i) & 1, (y >> i) & 1);
            v += (((a ^ b) << 1) | b) << (2 * (bits - 1 - i));
        }
        (v as f32 + 0.5) / (1u32 << (2 * bits)) as f32
    }

    /// `fn fade_threshold(p: vec2f) -> f32` for the shaders (`p`: the fragment's position).
    pub fn wgsl(self) -> String {
        let body = match self {
            Self::Noise => {
                "    return fract(52.982918 * fract(dot(floor(p), vec2f(0.06711056, 0.00583715))));\n"
                    .to_string()
            }
            _ => {
                let bits = self.bits();
                format!(
                    "    let q = vec2u(floor(p));\n    var v = 0u;\n    for (var i = 0u; i < {bits}u; i++) {{\n        let a = (q.x >> i) & 1u;\n        let b = (q.y >> i) & 1u;\n        v += (((a ^ b) << 1u) | b) << (2u * ({bits}u - 1u - i));\n    }}\n    return (f32(v) + 0.5) / {}.0;\n",
                    1u32 << (2 * bits)
                )
            }
        };
        format!(
            "// The LOD cross-fade's per-pixel threshold (dwell-look lod_blend.rs `FadePattern`, from\n// shared/look/lodblend.json): stable in screen space, in [0, 1).\nfn fade_threshold(p: vec2f) -> f32 {{\n{body}}}\n"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bayer_patterns_are_the_ordered_dither_and_use_every_step_once_per_tile() {
        let row: Vec<f32> = (0..4)
            .map(|x| FadePattern::Bayer4.threshold(x, 0))
            .collect();
        assert_eq!(row, [0.5, 8.5, 2.5, 10.5].map(|v| v / 16.0));
        for (p, n) in [(FadePattern::Bayer4, 4u32), (FadePattern::Bayer8, 8)] {
            let mut seen: Vec<f32> = (0..n * n).map(|k| p.threshold(k % n, k / n)).collect();
            seen.sort_by(f32::total_cmp);
            for (k, v) in seen.iter().enumerate() {
                assert_eq!(*v, (k as f32 + 0.5) / (n * n) as f32, "{p:?}");
            }
            // It tiles.
            assert_eq!(p.threshold(3, 5), p.threshold(3 + n, 5 + 2 * n));
        }
    }

    #[test]
    fn every_pattern_stays_in_the_unit_interval_and_its_wgsl_parses() {
        for p in [FadePattern::Bayer4, FadePattern::Bayer8, FadePattern::Noise] {
            for y in 0..37 {
                for x in 0..53 {
                    let t = p.threshold(x * 7, y * 3);
                    assert!((0.0..1.0).contains(&t), "{p:?} {t}");
                }
            }
            let src = p.wgsl();
            naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{p:?}: {e}\n{src}"));
        }
    }

    #[test]
    fn the_checked_in_tunables_are_a_fade_of_under_a_second_and_some_hysteresis() {
        assert!(LOD_BLEND.fade_s > 0.0 && LOD_BLEND.fade_s < 1.0);
        assert!(LOD_BLEND.coarsen_ratio > 1.0);
    }
}
