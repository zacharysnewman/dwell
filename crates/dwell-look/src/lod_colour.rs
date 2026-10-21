//! LOD colour (ARCHITECTURE.md §6.6 "Colour"): what a coarse LOD riser stands for. The makeup of
//! the fine rise it replaces, by the ground's gradient, from `shared/look/lodcolour.json`: how much
//! of it shows the top material's top tile, its side tile's top band (the fringe), and the rest.
//! The LOD mesher (`dwell-mesh` `lod_mesher.rs`) mixes the tile colours by it.

#[path = "lodcolour.gen.rs"]
mod generated;
pub use generated::LOD_COLOUR;

/// The rise's makeup at one gradient (m per m); the rest (1 − top − fringe) is the riser's own
/// material below the fringe band.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RisePoint {
    pub gradient: f64,
    pub top: f64,
    pub fringe: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodColourTunables {
    /// Risers' gradients are quantised to this (so equal walls merge).
    pub gradient_step: f64,
    /// By increasing gradient.
    pub rise: &'static [RisePoint],
}

impl LodColourTunables {
    /// The makeup [top, fringe, rest] at `gradient`: interpolated between points, held beyond the
    /// ends.
    pub fn makeup(&self, gradient: f64) -> [f64; 3] {
        let pts = self.rise;
        let p = match pts.iter().position(|p| p.gradient >= gradient) {
            None => *pts.last().expect("validated: non-empty"),
            Some(0) => pts[0],
            Some(i) => {
                let (a, b) = (pts[i - 1], pts[i]);
                let f = (gradient - a.gradient) / (b.gradient - a.gradient);
                RisePoint {
                    gradient,
                    top: a.top + (b.top - a.top) * f,
                    fringe: a.fringe + (b.fringe - a.fringe) * f,
                }
            }
        };
        [p.top, p.fringe, (1.0 - p.top - p.fringe).max(0.0)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_between_points_and_holds_beyond_the_ends() {
        let t = LodColourTunables {
            gradient_step: 0.1,
            rise: &[
                RisePoint {
                    gradient: 0.5,
                    top: 0.2,
                    fringe: 0.6,
                },
                RisePoint {
                    gradient: 1.5,
                    top: 0.0,
                    fringe: 0.2,
                },
            ],
        };
        let close = |a: [f64; 3], b: [f64; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-12);
        assert!(close(t.makeup(0.0), [0.2, 0.6, 0.2]));
        assert!(close(t.makeup(1.0), [0.1, 0.4, 0.5]));
        assert!(close(t.makeup(9.0), [0.0, 0.2, 0.8]));
    }

    #[test]
    fn the_data_sums_to_at_most_one_and_cliffs_are_mostly_their_rest() {
        for g in [0.0, 0.3, 0.7, 1.0, 3.0, 20.0] {
            let m = LOD_COLOUR.makeup(g);
            assert!(m.iter().all(|&v| (0.0..=1.0).contains(&v)), "{g}: {m:?}");
            assert!((m.iter().sum::<f64>() - 1.0).abs() < 1e-9, "{g}: {m:?}");
        }
        assert!(LOD_COLOUR.makeup(8.0)[2] > 0.9);
    }
}
