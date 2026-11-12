//! A scoop's per-pixel relief (docs/BLOCK_DAMAGE.md §6.1): one flat quad per struck face, its
//! hollow computed in the terrain shader (`dent_*` in dwell-client `render/shaders.rs`): a height
//! field of the face's marks' bowls merged into one hollow, its gradient bending the normal (a
//! normal map computed analytically), the texture marched into it (steep parallax), the rim's
//! shadow, the deeper floor's occlusion and damp. Look only: the face's quad and collision stay.
//!
//! This is the CPU copy of the shader's height field (kept in step by hand; the tests pin its
//! shape) and the packing of a quad's parameters into its vertex colour and light, which the
//! shader reads flat (`TerrainOut.code`).
//!
//! Since the carved mesh (BLOCK_DAMAGE.md §6.2) the same height fields cut real geometry: the
//! struck face is meshed as a grid pushed in by [`Carve::height`] (a [`Dent`]'s bowls for soils,
//! [`Facets`]' angular facets for stone), and the mesher no longer emits relief quads.

/// The marks' distances merge by a smooth minimum this soft (in radii): two bowls closer than
/// about two radii run into one hollow.
pub const MERGE: f64 = 0.5;
/// The rim's bump peaks this far out (radii) and spans ± this half-width: inside the bowl's lip
/// to where the quad ends ([`EXTENT`]), so the relief is flat at the quad's edge.
pub const RIM_AT: f64 = 1.15;
pub const RIM_HALF: f64 = 0.35;
/// The quad reaches this many radii past each mark.
pub const EXTENT: f64 = RIM_AT + RIM_HALF;
/// The relief fades to the face over this distance from the cell's edge (m).
pub const EDGE_FADE_M: f64 = 0.05;

/// The radial profile at `t` radii from the (merged) centre: a smooth bowl `depth` deep (its slope
/// 0 at the centre and at the lip) and the loose rim `rim` high (m; positive: proud of the face).
pub fn profile(t: f64, depth: f64, rim: f64) -> f64 {
    let q = (1.0 - t * t).clamp(0.0, 1.0);
    let x = ((t - RIM_AT) / RIM_HALF).clamp(-1.0, 1.0);
    let b = 1.0 - x * x;
    -depth * q * q + rim * b * b
}

/// A polynomial smooth minimum (k the blend width).
pub fn smin(a: f64, b: f64, k: f64) -> f64 {
    let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
    b + (a - b) * h - k * h * (1.0 - h)
}

/// One face's relief: its marks' centres and the cell's rectangle, in the face's (u, v) metres.
#[derive(Clone, Debug)]
pub struct Dent {
    pub centres: Vec<[f64; 2]>,
    pub lo: [f64; 2],
    pub hi: [f64; 2],
    pub radius: f64,
    pub depth: f64,
    pub rim: f64,
}

impl Dent {
    /// The merged distance (radii) to the marks.
    pub fn distance(&self, p: [f64; 2]) -> f64 {
        self.centres.iter().fold(1e3, |t, c| {
            let d = ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2)).sqrt() / self.radius;
            smin(t, d, MERGE)
        })
    }

    /// The height at `p` (m; negative: into the face), faded to the face at the cell's edge.
    pub fn height(&self, p: [f64; 2]) -> f64 {
        profile(self.distance(p), self.depth, self.rim) * edge_fade(p, self.lo, self.hi)
    }

    /// The lit normal at `p` in the face's frame (u, v, out), from the height's gradient by
    /// central differences, as the shader takes it.
    pub fn normal(&self, p: [f64; 2]) -> [f64; 3] {
        const E: f64 = 0.004;
        let gx = (self.height([p[0] + E, p[1]]) - self.height([p[0] - E, p[1]])) / (2.0 * E);
        let gy = (self.height([p[0], p[1] + E]) - self.height([p[0], p[1] - E])) / (2.0 * E);
        let l = (gx * gx + gy * gy + 1.0).sqrt();
        [-gx / l, -gy / l, 1.0 / l]
    }
}

/// The edge fade (smoothstep over [`EDGE_FADE_M`]) at `p` inside the rectangle `lo`–`hi`: 0 on its
/// edge, so a carved face's boundary vertices stay on the face (watertight with its neighbours).
pub fn edge_fade(p: [f64; 2], lo: [f64; 2], hi: [f64; 2]) -> f64 {
    let e = (p[0] - lo[0])
        .min(hi[0] - p[0])
        .min(p[1] - lo[1])
        .min(hi[1] - p[1]);
    let s = (e / EDGE_FADE_M).clamp(0.0, 1.0);
    s * s * (3.0 - 2.0 * s)
}

/// One shard knocked out of a face: an apex `depth` in, the facets running up from it to an
/// uneven polygon of `corners` on the face (all in the face's (u, v) metres).
#[derive(Clone, Debug)]
pub struct Shard {
    pub apex: [f64; 2],
    pub corners: Vec<[f64; 2]>,
    pub depth: f64,
}

impl Shard {
    /// The shard's height at `p` (m; ≤ 0): the facet `p` lies on, linear across it (a flat facet),
    /// 0 outside the polygon.
    pub fn height(&self, p: [f64; 2]) -> f64 {
        let n = self.corners.len();
        for k in 0..n {
            let (a, b, c) = (self.apex, self.corners[k], self.corners[(k + 1) % n]);
            let det = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
            if det.abs() < 1e-12 {
                continue;
            }
            let wb = ((p[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (p[1] - a[1])) / det;
            let wc = ((b[0] - a[0]) * (p[1] - a[1]) - (p[0] - a[0]) * (b[1] - a[1])) / det;
            let wa = 1.0 - wb - wc;
            if wa >= -1e-9 && wb >= -1e-9 && wc >= -1e-9 {
                return -self.depth * wa;
            }
        }
        0.0
    }

    /// How far the shard reaches from its apex (m).
    pub fn reach(&self) -> f64 {
        self.corners
            .iter()
            .map(|c| ((c[0] - self.apex[0]).powi(2) + (c[1] - self.apex[1]).powi(2)).sqrt())
            .fold(0.0, f64::max)
    }
}

/// A face's facets: its marks' shards, the deepest wins where they overlap, faded to the face at
/// the cell's edge (`lo`–`hi`, the face's metres).
#[derive(Clone, Debug)]
pub struct Facets {
    pub shards: Vec<Shard>,
    pub lo: [f64; 2],
    pub hi: [f64; 2],
}

/// One spalled flake (WORK_QUEUE item 18: "stone doesn't dent, it shatters"): a flat chip-out
/// over a convex, elongated polygon (`corners`, counter-clockwise, face metres), its floor a plane
/// `depth` in at `origin` and rising along `-grad` toward the far end (a conchoidal scar: deep at
/// the strike, feathering out), its walls a steep bevel `bevel` wide inside the polygon's edge.
#[derive(Clone, Debug)]
pub struct Flake {
    pub corners: Vec<[f64; 2]>,
    pub origin: [f64; 2],
    pub depth: f64,
    /// The floor's depth gained per metre (face metres; its plane is `depth + grad·(p − origin)`).
    pub grad: [f64; 2],
    pub bevel: f64,
}

impl Flake {
    /// How far inside the polygon `p` lies (m; negative outside): the least distance to its edges.
    pub fn inside(&self, p: [f64; 2]) -> f64 {
        let n = self.corners.len();
        (0..n).fold(f64::MAX, |d, k| {
            let (a, b) = (self.corners[k], self.corners[(k + 1) % n]);
            let e = [b[0] - a[0], b[1] - a[1]];
            let l = (e[0] * e[0] + e[1] * e[1]).sqrt().max(1e-12);
            // Counter-clockwise: the inside is to the edge's left.
            d.min((e[0] * (p[1] - a[1]) - e[1] * (p[0] - a[0])) / l)
        })
    }

    /// The floor's depth at `p` (m, >= 0): its plane.
    pub fn floor(&self, p: [f64; 2]) -> f64 {
        (self.depth
            + self.grad[0] * (p[0] - self.origin[0])
            + self.grad[1] * (p[1] - self.origin[1]))
            .max(0.0)
    }

    /// The flake's height at `p` (m; <= 0): its floor's plane past the bevel, the bevel's wall
    /// rising to the face at the polygon's edge, 0 outside.
    pub fn height(&self, p: [f64; 2]) -> f64 {
        let s = self.inside(p);
        if s <= 0.0 {
            return 0.0;
        }
        -self.floor(p) * (s / self.bevel).min(1.0)
    }

    /// How far it reaches from its origin (m).
    pub fn reach(&self) -> f64 {
        self.corners
            .iter()
            .map(|c| ((c[0] - self.origin[0]).powi(2) + (c[1] - self.origin[1]).powi(2)).sqrt())
            .fold(0.0, f64::max)
    }
}

/// A face's spall: its marks' flakes, the deepest wins where they overlap (the scars merge and
/// step), faded to the face at the cell's edge (`lo`–`hi`, the face's metres).
#[derive(Clone, Debug)]
pub struct Spall {
    pub flakes: Vec<Flake>,
    pub lo: [f64; 2],
    pub hi: [f64; 2],
    /// How far the fresh scar is lifted toward the look's `color` (0–1).
    pub fresh: f64,
}

/// A struck face's carve: the height field its grid is pushed in by (BLOCK_DAMAGE.md §6.2).
#[derive(Clone, Debug)]
pub enum Carve {
    /// Soils: bowls merged into one hollow, lit by smooth normals.
    Bowl(Dent),
    /// Angular facets, each triangle lit flat.
    Facets(Facets),
    /// Stone: flat spalled flakes, lit flat and lighter.
    Spall(Spall),
}

impl Carve {
    /// The height at `p` (face metres; m, ≤ 0 into the face; 0 on the cell's edge).
    pub fn height(&self, p: [f64; 2]) -> f64 {
        match self {
            Carve::Bowl(d) => d.height(p).min(0.0),
            Carve::Facets(f) => {
                let h = f.shards.iter().map(|s| s.height(p)).fold(0.0, f64::min);
                h * edge_fade(p, f.lo, f.hi)
            }
            Carve::Spall(f) => {
                let h = f.flakes.iter().map(|s| s.height(p)).fold(0.0, f64::min);
                h * edge_fade(p, f.lo, f.hi)
            }
        }
    }

    /// The deepest the field goes (m).
    pub fn depth(&self) -> f64 {
        match self {
            Carve::Bowl(d) => d.depth,
            Carve::Facets(f) => f.shards.iter().map(|s| s.depth).fold(0.0, f64::max),
            Carve::Spall(f) => f
                .flakes
                .iter()
                .map(|s| s.depth + s.grad[0].hypot(s.grad[1]) * s.reach())
                .fold(0.0, f64::max),
        }
    }

    /// How far a carved point's colour is lifted toward the look's fresh `color` (0: none; bowls
    /// and facets are darkened by `darken` instead).
    pub fn fresh(&self) -> f64 {
        match self {
            Carve::Spall(f) => f.fresh,
            _ => 0.0,
        }
    }

    /// Whether it is lit by smooth normals (a bowl) or flat per triangle (facets).
    pub fn smooth(&self) -> bool {
        matches!(self, Carve::Bowl(_))
    }

    /// The rectangle (face metres) outside which the field is the face itself.
    pub fn reach(&self) -> ([f64; 2], [f64; 2]) {
        let (pts, r, lo, hi): (Vec<[f64; 2]>, Vec<f64>, _, _) = match self {
            // Without a rim the bowl ends at its lip (one radius); with one, the rim's reach.
            Carve::Bowl(d) => (
                d.centres.clone(),
                vec![if d.rim > 0.0 { EXTENT } else { 1.0 } * d.radius; d.centres.len()],
                d.lo,
                d.hi,
            ),
            Carve::Facets(f) => (
                f.shards.iter().map(|s| s.apex).collect(),
                f.shards.iter().map(|s| s.reach()).collect(),
                f.lo,
                f.hi,
            ),
            Carve::Spall(f) => (
                f.flakes.iter().map(|s| s.origin).collect(),
                f.flakes.iter().map(|s| s.reach()).collect(),
                f.lo,
                f.hi,
            ),
        };
        let mut a = hi;
        let mut b = lo;
        for (p, r) in pts.iter().zip(r) {
            for i in 0..2 {
                a[i] = a[i].min(p[i] - r);
                b[i] = b[i].max(p[i] + r);
            }
        }
        (
            [a[0].max(lo[0]), a[1].max(lo[1])],
            [b[0].min(hi[0]), b[1].min(hi[1])],
        )
    }
}

/// A quad's parameters as its vertex colour and light carry them (all whole numbers under 2^24,
/// exact in an f32 and read flat): the colour's blue negative marks it (a lava face's green is).
/// `centres` are each mark's place in the cell (0–1 along the face's u and v), `half` whether the
/// cell is ½ m along u and v (a side's height), `steps` the parallax march, `darken` the damp floor
/// (0–1), `rim`, `radius` and `depth` in metres.
#[allow(clippy::too_many_arguments)]
pub fn pack(
    centres: &[[f64; 2]],
    half: [bool; 2],
    steps: u32,
    darken: f64,
    rim: f64,
    radius: f64,
    depth: f64,
) -> ([f32; 3], f32) {
    let q6 = |x: f64| (x.clamp(0.0, 1.0) * 63.0).round() as u32;
    let c = |i: usize| centres.get(i).map_or(0, |p| q6(p[0]) | (q6(p[1]) << 6));
    let count = centres.len().min(4) as u32;
    let r = count
        | (u32::from(half[0]) << 3)
        | (u32::from(half[1]) << 4)
        | (steps.min(15) << 5)
        | (((darken.clamp(0.0, 1.0) * 7.0).round() as u32) << 9)
        | (((rim * 1000.0).round() as u32).min(31) << 12);
    let g = c(0) | (c(1) << 12);
    let b = c(2) | (c(3) << 12);
    let mm2 = |x: f64| ((x * 500.0).round() as u32).min(255);
    let light = mm2(radius) | (mm2(depth) << 8);
    ([r as f32, g as f32, -(1.0 + b as f32)], light as f32)
}

/// [`pack`] read back (the shader's `dent_params`): centres, half, steps, darken, rim, radius, depth.
#[cfg(test)]
#[allow(clippy::type_complexity)]
pub fn unpack(color: [f32; 3], light: f32) -> (Vec<[f64; 2]>, [bool; 2], u32, f64, f64, f64, f64) {
    let r = color[0] as u32;
    let g = color[1] as u32;
    let b = (-color[2] - 1.0) as u32;
    let count = (r & 7) as usize;
    let c = |w: u32| [f64::from(w & 63) / 63.0, f64::from((w >> 6) & 63) / 63.0];
    let all = [c(g & 4095), c(g >> 12), c(b & 4095), c(b >> 12)];
    let l = light as u32;
    (
        all[..count].to_vec(),
        [r & 8 != 0, r & 16 != 0],
        (r >> 5) & 15,
        f64::from((r >> 9) & 7) / 7.0,
        f64::from((r >> 12) & 31) / 1000.0,
        f64::from(l & 255) / 500.0,
        f64::from((l >> 8) & 255) / 500.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(depth: f64) -> Dent {
        Dent {
            centres: vec![[0.5, 0.5]],
            lo: [0.0, 0.0],
            hi: [1.0, 1.0],
            radius: 0.17,
            depth,
            rim: 0.012,
        }
    }

    #[test]
    fn the_bowl_is_lowest_at_its_centre() {
        let d = one(0.12);
        let h0 = d.height([0.5, 0.5]);
        assert!(
            (h0 + 0.12).abs() < 1e-9,
            "the full depth at the centre: {h0}"
        );
        for k in 1..40 {
            let p = [0.5 + 0.17 * 1.5 * f64::from(k) / 40.0, 0.5];
            assert!(d.height(p) > h0, "{p:?} higher than the centre");
        }
    }

    #[test]
    fn the_walls_tilt_toward_the_centre() {
        let d = one(0.12);
        // On the +u wall the lit normal leans back toward −u (the centre); on the +v wall to −v.
        for t in [0.3, 0.5, 0.7] {
            let n = d.normal([0.5 + 0.17 * t, 0.5]);
            assert!(n[0] < -0.05, "t {t}: {n:?}");
            let n = d.normal([0.5, 0.5 + 0.17 * t]);
            assert!(n[1] < -0.05, "t {t}: {n:?}");
        }
    }

    #[test]
    fn more_damage_digs_deeper_and_steeper() {
        let (shallow, deep) = (one(0.06), one(0.14));
        assert!(deep.height([0.5, 0.5]) < shallow.height([0.5, 0.5]));
        let p = [0.5 + 0.17 * 0.5, 0.5];
        assert!(
            deep.normal(p)[2] < shallow.normal(p)[2],
            "a deeper bowl's walls are steeper"
        );
    }

    #[test]
    fn it_is_flat_at_the_quads_edge_and_the_cells() {
        let d = one(0.14);
        // Past EXTENT radii the relief is the face itself: no step where the quad ends.
        for a in 0..16 {
            let th = f64::from(a) * std::f64::consts::TAU / 16.0;
            let p = [
                0.5 + 0.17 * EXTENT * th.cos(),
                0.5 + 0.17 * EXTENT * th.sin(),
            ];
            assert!(d.height(p).abs() < 1e-9, "{p:?}");
            let n = d.normal(p);
            assert!(n[2] > 0.999, "{n:?}");
        }
        // A mark at the cell's edge fades to the face at it.
        let edge = Dent {
            centres: vec![[0.18, 0.5]],
            ..one(0.14)
        };
        assert!(edge.height([0.0, 0.5]).abs() < 1e-9);
    }

    #[test]
    fn the_rim_stands_proud_and_two_hits_merge_into_one_hollow() {
        let d = one(0.12);
        assert!(
            d.height([0.5 + 0.17 * RIM_AT, 0.5]) > 0.005,
            "the rim is raised"
        );
        // Two marks 1.65 radii apart: between them the floor runs down deeper than either bowl's
        // alone there (one hollow), with no rim between them.
        let two = Dent {
            centres: vec![[0.36, 0.5], [0.64, 0.5]],
            ..one(0.12)
        };
        let mid = two.height([0.5, 0.5]);
        let alone = one(0.12).height([0.5 + 0.14, 0.5]);
        assert!(
            mid < 1.5 * alone && alone < 0.0,
            "one hollow between them: {mid} vs {alone}"
        );
    }

    #[test]
    fn its_parameters_round_trip_through_the_vertex() {
        let centres = [[0.2, 0.8], [0.5, 0.5], [0.9, 0.1], [0.0, 1.0]];
        let (c, l) = pack(&centres, [false, true], 6, 0.35, 0.012, 0.171, 0.126);
        assert!(c.iter().all(|x| x.abs() < 16_777_216.0) && l < 16_777_216.0);
        let (back, half, steps, darken, rim, radius, depth) = unpack(c, l);
        assert_eq!(back.len(), 4);
        for (a, b) in back.iter().zip(centres) {
            assert!(
                (a[0] - b[0]).abs() < 0.01 && (a[1] - b[1]).abs() < 0.01,
                "{a:?} {b:?}"
            );
        }
        assert_eq!((half, steps), ([false, true], 6));
        assert!((darken - 0.35).abs() < 0.08 && (rim - 0.012).abs() < 1e-9);
        assert!((radius - 0.171).abs() < 0.002 && (depth - 0.126).abs() < 0.002);
        assert!(
            c[2] < -0.5 && c[1] >= 0.0,
            "marked by its blue, not a lava face's green"
        );
    }
}
