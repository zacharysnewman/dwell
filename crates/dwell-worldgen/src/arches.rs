//! Arches (ADR 0074 B8): openings carved through narrow fins of rock, a span of rock left above.
//! A biome opts in with its `arches` (data, `shared/worldgen/biomes/*.json`; none in the default
//! world, so its terrain is unchanged). Sites lie on a fixed grid of [`CELL`] m squares, one chance
//! per square from the biome's density; a site is an arch only where the ground at its centre
//! stands a fin above the ground `fin`/2 m to either side along its axis (the erosion's cut-back
//! leaves such fins between the drainage lines, ADR 0075). Everything is a function of the world
//! position and the height field, so neighbouring chunks carve the same cells. Face-local: face B's
//! chunks are generated face-local and mirrored (`bifacial.rs`). The column tops never change (a
//! `lintel` m of rock stays over every carved cell), so the LOD, spawn, the maps and the water read
//! the height field as before; the LOD does not show the opening (WORLD_GENERATION.md §5).

use crate::noise::{hash2, unit};

/// A biome's arches (data): sites per km², the opening's span across the fin (m), its rise (m),
/// the rock left above it (m), the thickest fin it cuts through (m, along the axis), and whether a
/// scarp's site is cut as an overhang.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arches {
    pub density: f64,
    pub span: [f64; 2],
    pub rise: [f64; 2],
    pub lintel: [f64; 2],
    pub fin: f64,
    /// Whether a site on a scarp (low on one side only) is cut as an overhang.
    pub overhangs: bool,
}

/// The site grid's square (m): one chance of an arch per square.
pub const CELL: i32 = 32;
/// The most sites per km² (one per square).
pub const DENSITY_MAX: f64 = 1_000_000.0 / (CELL as f64 * CELL as f64);

/// One arch: its centre, axis (unit, face-local x/z: the way through), half the fin it cuts
/// (along the axis), half its span (across), its rise, the rock left above (m) and the opening's
/// floor (m, face-local height).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Site {
    pub centre: [f64; 2],
    pub axis: [f64; 2],
    pub half_fin: f64,
    pub half_span: f64,
    pub rise: f64,
    pub lintel: f64,
    pub floor: f64,
    /// Through a fin (an arch), else cut into a scarp from its low side (an overhang).
    pub through: bool,
}

/// A site candidate before the fin test: its square's draw.
#[derive(Clone, Copy, Debug)]
pub struct Draw {
    pub centre: [f64; 2],
    /// The chance roll in [0, 1).
    pub roll: f64,
    hashes: [u32; 4],
}

/// The draw of square (gx, gz) under `seed`.
pub fn draw(seed: u32, gx: i32, gz: i32) -> Draw {
    let h = |k: u32| hash2(seed.wrapping_add(k.wrapping_mul(0x9E37_79B9)), gx, gz);
    let c = CELL as f64;
    Draw {
        centre: [
            (f64::from(gx) + 0.1 + 0.8 * f64::from(unit(h(1)))) * c,
            (f64::from(gz) + 0.1 + 0.8 * f64::from(unit(h(2)))) * c,
        ],
        roll: f64::from(unit(h(0))),
        hashes: [h(3), h(4), h(5), h(6)],
    }
}

fn pick(r: [f64; 2], h: u32) -> f64 {
    r[0] + (r[1] - r[0]) * f64::from(unit(h))
}

/// The site a draw makes under a biome's `arches`, given the ground's height (m) at a point: none
/// when the roll misses the density or the ground there is no fin (the centre at least the rise
/// and lintel over both sides).
pub fn site(a: &Arches, d: &Draw, ground: impl Fn(f64, f64) -> Option<f64>) -> Option<Site> {
    let chance = a.density / DENSITY_MAX;
    if d.roll >= chance {
        return None;
    }
    let (half_span, rise, lintel) = (
        pick(a.span, d.hashes[1]) / 2.0,
        pick(a.rise, d.hashes[2]),
        pick(a.lintel, d.hashes[3]),
    );
    let need = rise + lintel;
    let half_fin = a.fin / 2.0;
    let [cx, cz] = d.centre;
    let top = ground(cx, cz)?;
    // Four axes (x, z and the diagonals; square roots only, IEEE-exact so native and WASM agree),
    // from a hashed first: a fin (the ground low on both sides) makes an arch on its best axis,
    // else a scarp (low on one side) an overhang, an opening cut into it from the low side.
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let axes = [[1.0, 0.0], [h, h], [0.0, 1.0], [-h, h]];
    let first = (d.hashes[0] % 4) as usize;
    // The best axis of each kind: its drop, the axis, the opening's floor.
    type Best = Option<(f64, [f64; 2], f64)>;
    let (mut arch, mut overhang): (Best, Best) = (None, None);
    for k in 0..4 {
        let axis = axes[(first + k) % 4];
        let s1 = ground(cx - axis[0] * half_fin, cz - axis[1] * half_fin)?;
        let s2 = ground(cx + axis[0] * half_fin, cz + axis[1] * half_fin)?;
        let (lo, hi) = (s1.min(s2), s1.max(s2));
        if top - hi >= need && arch.is_none_or(|(d, _, _)| top - hi > d) {
            arch = Some((top - hi, axis, hi));
        }
        if top - lo >= need && overhang.is_none_or(|(d, _, _)| top - lo > d) {
            overhang = Some((top - lo, axis, lo));
        }
    }
    let (through, (_, axis, floor)) = match (arch, overhang) {
        (Some(a), _) => (true, a),
        (None, Some(o)) if a.overhangs => (false, o),
        _ => return None,
    };
    Some(Site {
        centre: d.centre,
        axis,
        half_fin,
        half_span,
        rise,
        lintel,
        floor,
        through,
    })
}

impl Site {
    /// How far (m) the site reaches from its centre.
    pub fn reach(&self) -> f64 {
        (self.half_fin * self.half_fin + self.half_span * self.half_span).sqrt()
    }

    /// The opening's roof (m, face-local height) over column (x, z) (its centre), none outside
    /// it: a half ellipse across the axis, its crown `rise` over the floor.
    pub fn roof(&self, x: f64, z: f64) -> Option<f64> {
        let (dx, dz) = (x - self.centre[0], z - self.centre[1]);
        let along = dx * self.axis[0] + dz * self.axis[1];
        let across = -dx * self.axis[1] + dz * self.axis[0];
        if along.abs() > self.half_fin || across.abs() >= self.half_span {
            return None;
        }
        let t = across / self.half_span;
        Some(self.floor + self.rise * (1.0 - t * t).sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Arches = Arches {
        density: DENSITY_MAX,
        span: [8.0, 8.0],
        rise: [4.0, 4.0],
        lintel: [2.0, 2.0],
        fin: 12.0,
        overhangs: false,
    };

    #[test]
    fn a_fin_makes_an_arch_and_flat_ground_does_not() {
        let d = draw(7, 3, 4);
        let flat = site(&A, &d, |_, _| Some(10.0));
        assert!(flat.is_none());
        // A ridge 20 m tall along every direction's centre (a cone 2 m/m).
        let [cx, cz] = d.centre;
        let cone = |x: f64, z: f64| Some(30.0 - 2.0 * ((x - cx).powi(2) + (z - cz).powi(2)).sqrt());
        let s = site(&A, &d, cone).expect("a fin makes an arch");
        assert!(s.through, "low on both sides: an arch");
        assert!((s.floor - 18.0).abs() < 1e-9);
        assert!((s.roof(cx, cz).unwrap() - (s.floor + 4.0)).abs() < 1e-9);
        assert!(s.roof(cx + 100.0, cz).is_none());
    }

    #[test]
    fn a_scarp_makes_an_overhang_only_when_asked() {
        let d = draw(7, 3, 4);
        let [cx, _] = d.centre;
        // High at x < cx + 3, a 20 m drop past it.
        let scarp = |x: f64, _: f64| Some(if x < cx + 3.0 { 30.0 } else { 10.0 });
        assert!(site(&A, &d, scarp).is_none(), "overhangs off: none");
        let o = Arches {
            overhangs: true,
            ..A
        };
        let s = site(&o, &d, scarp).expect("an overhang");
        assert!(!s.through);
        assert_eq!(s.floor, 10.0);
    }

    #[test]
    fn zero_density_makes_none() {
        let a = Arches { density: 0.0, ..A };
        let d = draw(7, 3, 4);
        let [cx, cz] = d.centre;
        assert!(
            site(&a, &d, |x, z| Some(
                30.0 - 2.0 * ((x - cx).powi(2) + (z - cz).powi(2)).sqrt()
            ))
            .is_none()
        );
    }
}
