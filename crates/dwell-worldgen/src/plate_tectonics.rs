//! Uplift from plate boundaries (docs/proposals/REALISTIC_WORLD.md §3.2, phase R2): a Realistic
//! world's tectonics (`terrain.json` `tectonics.plates` 1) from its face's seed-drawn plate layout
//! (`plates.rs`), not from a hand-placed line. Every boundary, ordered into polylines, adds its
//! kind's landform by rule:
//!
//! - **subducting** (convergent, one side dives): a range on the overriding side `range.distance` m
//!   behind the line, a trench on the diving side, and an arc of dormant cones `arc.distance` m
//!   behind the line on the overriding side, one per `arc.spacing` m of boundary;
//! - **collision** (convergent, two continents): a range astride the line;
//! - **divergent**: a rift valley along the line with raised shoulders;
//! - **transform**: a low ridge along the line (little relief);
//!
//! each scaled by the boundary's rate (closing, opening or slip) over `speedRef`, clamped to
//! `speedScale`. Every hotspot raises a chain of islands from its point **along the plate's
//! motion**, each older one lower and wider (the plate carries them off the fixed hotspot).
//!
//! The vocabulary is code (the four landforms, the arc and the chain rules); the tuning is data
//! (`shared/worldgen/plates.json` `uplift`, generated into `plates.gen.rs` as [`UPLIFT`]). The
//! boundaries' landforms are summed on a coarse grid ([`PlateUplift::cell`], ≥ 500 m) and read
//! bilinearly, so the layer is a function of vectors plus one coarse grid (§4: no new 100 m
//! whole-map grid). Face-local: each face's layout is its own face seed's, so both faces.

use crate::plates::{Boundary, BoundaryKind, PLATES, PlateLayout};
use crate::tectonics::{Tectonics, Volcano};

pub use crate::plates::UPLIFT;

/// A landform across a boundary (plates.json `uplift.*`): `height` m (a depth for a trench or a
/// rift), `width` m half-wide, centred `distance` m from the line (on the side its rule names).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Landform {
    pub height: f64,
    pub width: f64,
    pub distance: f64,
}

/// The arc's rule (plates.json `uplift.arc`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArcRule {
    /// The cones' distance (m) behind the line on the overriding side.
    pub distance: f64,
    /// One cone per this many m of boundary, at most `max_cones` a polyline; none on a polyline
    /// shorter than `min_length` m.
    pub spacing: f64,
    pub min_length: f64,
    pub max_cones: u32,
    pub cone_height: f64,
    pub cone_radius: f64,
    pub crater_radius: f64,
    pub crater_depth: f64,
    /// How much the cones resist the erosion (0..1, as `tectonics.coneHardness`).
    pub hardness: f64,
}

/// A hotspot chain's rule (plates.json `uplift.hotspot`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChainRule {
    pub islands: u32,
    pub spacing: f64,
    pub height: f64,
    pub radius: f64,
    /// Each older island's height as a share of the last's, and how much broader.
    pub decay: f64,
    pub widen: f64,
}

/// The uplift's rules (data: plates.json `uplift`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlateUplift {
    /// The rate (cm/yr) at which a landform has its data's size, and the clamp on that scale.
    pub speed_ref: f64,
    pub speed_scale: [f64; 2],
    /// The polylines: scan points within `link` m are joined, vertices thinned to `segment` m.
    pub link: f64,
    pub segment: f64,
    /// The coarse grid's cell (m) the landforms are summed on.
    pub cell: f64,
    pub range: Landform,
    pub trench: Landform,
    pub collision: Landform,
    pub rift: Landform,
    /// The rift's shoulders: this share of its depth, raised either side.
    pub rift_shoulder: f64,
    pub transform: Landform,
    pub arc: ArcRule,
    pub hotspot: ChainRule,
}

/// A smooth bump: 1 at 0, 0 from |t| ≥ 1 (as `tectonics.rs`).
fn bump(t: f64) -> f64 {
    let t = t.abs();
    if t >= 1.0 {
        0.0
    } else {
        let u = 1.0 - t * t;
        u * u
    }
}

/// The distance (m) from `p` to the segment `a`–`b`, and the share of the way along it.
fn to_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> (f64, f64) {
    let (ex, ez) = (b[0] - a[0], b[1] - a[1]);
    let len2 = ex * ex + ez * ez;
    let t = if len2 > 0.0 {
        (((p[0] - a[0]) * ex + (p[1] - a[1]) * ez) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (dx, dz) = (p[0] - a[0] - t * ex, p[1] - a[1] - t * ez);
    ((dx * dx + dz * dz).sqrt(), t)
}

/// The distance (m) from `p` to a polyline.
fn to_line(p: [f64; 2], line: &[[f64; 2]]) -> f64 {
    if line.len() == 1 {
        return to_segment(p, line[0], line[0]).0;
    }
    line.windows(2)
        .map(|w| to_segment(p, w[0], w[1]).0)
        .fold(f64::INFINITY, f64::min)
}

/// A boundary as the uplift reads it: its polylines and its rule's numbers.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// Index into the layout's boundaries.
    pub boundary: usize,
    pub kind: BoundaryKind,
    /// The overriding plate on a subducting boundary.
    pub over: Option<usize>,
    /// The landforms' scale from the boundary's rate.
    pub scale: f64,
    pub polylines: Vec<Vec<[f64; 2]>>,
}

/// A face's tectonics from its plate layout.
#[derive(Clone, Debug, PartialEq)]
pub struct PlateTectonics {
    pub layout: PlateLayout,
    pub uplift: PlateUplift,
    pub lines: Vec<Line>,
    /// The arcs' cones (`chain` 0), then each hotspot's chain youngest first (`chain` k + 1 for
    /// the k-th island down it; hotspot `h`'s islands follow hotspot `h − 1`'s).
    pub volcanoes: Vec<Volcano>,
    /// The boundaries' summed landform (m) on the coarse grid: `n` × `n` nodes from `x0`.
    field: Vec<f32>,
    n: usize,
    x0: f64,
}

impl PlateTectonics {
    /// The tectonics of face seed `seed` by the build's rules (plates.json).
    pub fn for_seed(seed: u64) -> Self {
        Self::new(PlateLayout::generate(&PLATES, seed), &UPLIFT)
    }

    /// The tectonics of a layout by `uplift`'s rules.
    pub fn new(layout: PlateLayout, uplift: &PlateUplift) -> Self {
        let u = *uplift;
        let lines: Vec<Line> = layout
            .boundaries
            .iter()
            .enumerate()
            .map(|(i, b)| Line {
                boundary: i,
                kind: b.meeting.kind,
                over: b.overriding_plate(),
                scale: Self::scale_of(&u, b),
                polylines: b.polylines(u.link, u.segment),
            })
            .collect();
        let mut t = Self {
            layout,
            uplift: u,
            lines,
            volcanoes: Vec::new(),
            field: Vec::new(),
            n: 0,
            x0: 0.0,
        };
        t.volcanoes = t.place_arcs();
        t.volcanoes.extend(t.place_chains());
        t.fill_field();
        t
    }

    fn scale_of(u: &PlateUplift, b: &Boundary) -> f64 {
        let rate = match b.meeting.kind {
            BoundaryKind::Transform => b.meeting.slip,
            _ => b.meeting.closing.abs(),
        };
        (rate / u.speed_ref.max(1e-9)).clamp(u.speed_scale[0], u.speed_scale[1])
    }

    /// The farthest (m) any boundary's landform reaches from its line.
    fn reach(&self) -> f64 {
        let u = &self.uplift;
        [
            u.range.distance + u.range.width,
            u.trench.distance + u.trench.width,
            u.collision.distance + u.collision.width,
            2.5 * u.rift.width + u.rift.distance,
            u.transform.distance + u.transform.width,
        ]
        .into_iter()
        .fold(0.0, f64::max)
    }

    /// One boundary's landform at `p` (m), on plate `plate`, `d` m from its line.
    fn landform(&self, line: &Line, plate: usize, d: f64) -> f64 {
        let u = &self.uplift;
        let across = |l: &Landform| bump((d - l.distance) / l.width.max(1.0));
        let h = match line.kind {
            BoundaryKind::Convergent => match line.over {
                Some(over) if plate == over => u.range.height * across(&u.range),
                Some(_) => -u.trench.height * across(&u.trench),
                None => u.collision.height * across(&u.collision),
            },
            BoundaryKind::Divergent => {
                let w = u.rift.width.max(1.0);
                -u.rift.height * across(&u.rift)
                    + u.rift_shoulder * u.rift.height * bump((d - u.rift.distance - 1.5 * w) / w)
            }
            BoundaryKind::Transform => u.transform.height * across(&u.transform),
        };
        h * line.scale
    }

    /// The boundaries of plate `plate` and their distance (m) from (x, z) (R5's rock settings).
    pub fn lines_of(&self, plate: usize, x: f64, z: f64) -> impl Iterator<Item = (&Line, f64)> {
        self.lines.iter().filter_map(move |line| {
            let [a, b] = self.layout.boundaries[line.boundary].plates;
            (plate == a || plate == b).then(|| {
                let d = line
                    .polylines
                    .iter()
                    .map(|pl| to_line([x, z], pl))
                    .fold(f64::INFINITY, f64::min);
                (line, d)
            })
        })
    }

    /// The boundaries' landform at (x, z) computed exactly (the coarse grid's nodes take it).
    pub fn landform_exact(&self, x: f64, z: f64) -> f64 {
        let reach = self.reach();
        let plate = self.layout.plate_at(x, z);
        let mut h = 0.0;
        for line in &self.lines {
            let [a, b] = self.layout.boundaries[line.boundary].plates;
            // A boundary shapes only its own two plates' ground.
            if plate != a && plate != b {
                continue;
            }
            let d = line
                .polylines
                .iter()
                .map(|pl| to_line([x, z], pl))
                .fold(f64::INFINITY, f64::min);
            if d < reach {
                h += self.landform(line, plate, d);
            }
        }
        h
    }

    fn fill_field(&mut self) {
        let cell = self.uplift.cell.max(1.0);
        let half = self.layout.rules.reach + self.reach() + cell;
        let n = (2.0 * half / cell).ceil() as usize + 1;
        self.n = n;
        self.x0 = -half;
        let mut field = vec![0.0f32; n * n];
        for j in 0..n {
            for i in 0..n {
                let (x, z) = (self.x0 + i as f64 * cell, self.x0 + j as f64 * cell);
                field[j * n + i] = self.landform_exact(x, z) as f32;
            }
        }
        self.field = field;
    }

    /// The boundaries' landform at (x, z) (m added; bilinear on the coarse grid, 0 beyond it):
    /// ranges and collision ranges up, trenches and rifts down.
    pub fn boundary(&self, x: f64, z: f64) -> f64 {
        let cell = self.uplift.cell.max(1.0);
        let (fx, fz) = ((x - self.x0) / cell, (z - self.x0) / cell);
        if !(fx >= 0.0 && fz >= 0.0) {
            return 0.0;
        }
        let (i, j) = (fx as usize, fz as usize);
        if i + 1 >= self.n || j + 1 >= self.n {
            return 0.0;
        }
        let (tx, tz) = (fx - i as f64, fz - j as f64);
        let at = |i: usize, j: usize| f64::from(self.field[j * self.n + i]);
        let top = at(i, j) * (1.0 - tx) + at(i + 1, j) * tx;
        let bottom = at(i, j + 1) * (1.0 - tx) + at(i + 1, j + 1) * tx;
        top * (1.0 - tz) + bottom * tz
    }

    /// Everything the layer adds at (x, z) (m): the boundaries' landform and the tallest cone.
    pub fn height(&self, x: f64, z: f64) -> f64 {
        let cones = self
            .volcanoes
            .iter()
            .map(|v| Tectonics::cone(v, ((x - v.x).powi(2) + (z - v.z).powi(2)).sqrt()))
            .fold(0.0, f64::max);
        self.boundary(x, z) + cones
    }

    /// How much the erosion is held off at (x, z) (0..`arc.hardness`): the arcs' cones, fully at
    /// the summit and none at the foot, as `Tectonics::shield`.
    pub fn shield(&self, x: f64, z: f64) -> f64 {
        let k = self.uplift.arc.hardness.clamp(0.0, 1.0);
        if k <= 0.0 {
            return 0.0;
        }
        let mut s = 0.0f64;
        for v in self.volcanoes.iter().filter(|v| v.chain == 0) {
            let r = ((x - v.x).powi(2) + (z - v.z).powi(2)).sqrt();
            let t = (1.0 - r / v.radius.max(1.0)).clamp(0.0, 1.0);
            s = s.max(t * t * (3.0 - 2.0 * t));
        }
        k * s
    }

    /// The arcs: along each subducting boundary's polylines at least `min_length` m long, one cone
    /// per `spacing` m (at most `max_cones`), evenly along it, `distance` m off the line on the
    /// overriding plate's side; a cone whose site is not on that plate (a junction) or beyond the
    /// layout's reach is left out.
    fn place_arcs(&self) -> Vec<Volcano> {
        let a = &self.uplift.arc;
        let reach = self.layout.rules.reach;
        let mut out = Vec::new();
        for line in &self.lines {
            let Some(over) = line.over else { continue };
            for pl in &line.polylines {
                let lens: Vec<f64> = pl
                    .windows(2)
                    .map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt())
                    .collect();
                let total: f64 = lens.iter().sum();
                if total < a.min_length || total <= 0.0 {
                    continue;
                }
                let count = ((total / a.spacing.max(1.0)) as u32).clamp(1, a.max_cones.max(1));
                for k in 0..count {
                    let mut s = (f64::from(k) + 0.5) * total / f64::from(count);
                    let mut seg = 0;
                    while seg + 1 < lens.len() && s > lens[seg] {
                        s -= lens[seg];
                        seg += 1;
                    }
                    let (p, q) = (pl[seg], pl[seg + 1]);
                    let t = (s / lens[seg].max(1e-9)).clamp(0.0, 1.0);
                    let at = [p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1])];
                    let len = lens[seg].max(1e-9);
                    let normal = [-(q[1] - p[1]) / len, (q[0] - p[0]) / len];
                    let site = [1.0, -1.0].into_iter().map(|sign| {
                        [
                            at[0] + sign * a.distance * normal[0],
                            at[1] + sign * a.distance * normal[1],
                        ]
                    });
                    let Some(c) = site
                        .into_iter()
                        .find(|c| self.layout.plate_at(c[0], c[1]) == over)
                    else {
                        continue;
                    };
                    if c[0] * c[0] + c[1] * c[1] > reach * reach {
                        continue;
                    }
                    out.push(Volcano {
                        x: c[0],
                        z: c[1],
                        height: a.cone_height * line.scale,
                        radius: a.cone_radius,
                        crater_radius: a.crater_radius,
                        crater_depth: a.crater_depth,
                        chain: 0,
                    });
                }
            }
        }
        out
    }

    /// Each hotspot's chain: `islands` shields from its point along its plate's motion (the
    /// layout's `trail`), `spacing` m apart, each older one `decay` of the last's height and
    /// `widen` broader; none for a still plate past the youngest.
    fn place_chains(&self) -> Vec<Volcano> {
        let c = &self.uplift.hotspot;
        let mut out = Vec::new();
        for hs in &self.layout.hotspots {
            let still = hs.trail == [0.0, 0.0];
            for k in 0..if still { c.islands.min(1) } else { c.islands } {
                let age = f64::from(k);
                let d = age * c.spacing;
                out.push(Volcano {
                    x: hs.at[0] + d * hs.trail[0],
                    z: hs.at[1] + d * hs.trail[1],
                    height: c.height * c.decay.powf(age),
                    radius: c.radius * (1.0 + c.widen * age),
                    crater_radius: 0.0,
                    crater_depth: 0.0,
                    chain: k + 1,
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plates::{FIXTURES, Plate};

    fn fixture(name: &str) -> PlateTectonics {
        let f = FIXTURES.iter().find(|f| f.name == name).unwrap();
        let plates = f.plates.map(Plate::from).to_vec();
        PlateTectonics::new(PlateLayout::of_plates(&PLATES, plates), &UPLIFT)
    }

    #[test]
    fn a_fixture_boundary_orders_into_one_polyline_along_the_bisector() {
        // The fixtures' plates sit at ±5 km on one axis: the boundary is the axis's bisector.
        let t = fixture("ocean_under_continent");
        assert_eq!(t.lines.len(), 1);
        let pl = &t.lines[0].polylines;
        assert_eq!(pl.len(), 1, "one line: {pl:?}");
        let line = &pl[0];
        assert!(line.len() >= 10, "{} vertices", line.len());
        for w in line.windows(2) {
            let d = ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt();
            assert!(
                (0.5 * UPLIFT.segment..=UPLIFT.link + 1.0).contains(&d),
                "{d}"
            );
            // Ordered: each step moves the same way along the line (x ≈ 0, z monotone).
            assert!(w[0][0].abs() <= 500.0);
        }
        let mono = line.windows(2).all(|w| w[1][1] > w[0][1])
            || line.windows(2).all(|w| w[1][1] < w[0][1]);
        assert!(mono, "ordered along the boundary");
    }

    #[test]
    fn each_boundary_kind_shapes_the_ground_by_its_rule() {
        let u = &UPLIFT;
        // Subduction: the continent (plate 1, +x) rides over; its range stands behind the line,
        // the trench on the ocean's side, the arc's cones on the continent's side.
        let t = fixture("ocean_under_continent");
        let range = t.boundary(u.range.distance, 0.0);
        assert!(range > 0.5 * u.range.height * u.speed_scale[0], "{range}");
        assert!(t.boundary(-u.trench.distance, 0.0) < 0.0, "a trench");
        assert!(t.boundary(-u.range.distance, 0.0) < 0.5 * range);
        let arc: Vec<_> = t.volcanoes.iter().filter(|v| v.chain == 0).collect();
        assert!(!arc.is_empty(), "an arc");
        for v in &arc {
            assert!(v.x > 0.0, "on the overriding side: {v:?}");
            assert!((v.x - u.arc.distance).abs() < 600.0, "{v:?}");
        }
        // Collision: a range astride the line, no arc.
        let t = fixture("continents_collide");
        assert!(t.boundary(u.collision.distance, 0.0) > 0.0);
        assert!(t.volcanoes.iter().all(|v| v.chain != 0), "no arc");
        // Divergent: a rift below.
        let t = fixture("rift_opens");
        assert!(t.boundary(0.0, 0.0) < -0.5 * u.rift.height * u.speed_scale[0]);
        // Transform: little relief.
        let t = fixture("plates_slide_past");
        let most = (-40..=40)
            .map(|k| t.boundary(f64::from(k) * 250.0, 0.0).abs())
            .fold(0.0, f64::max);
        assert!(
            most <= u.transform.height * u.speed_scale[1] + 1.0,
            "{most}"
        );
        assert!(most < 0.5 * u.range.height * u.speed_scale[0]);
    }

    #[test]
    fn the_coarse_field_matches_the_exact_landform() {
        let t = PlateTectonics::for_seed(42);
        let (mut worst, mut tall) = (0.0f64, 0.0f64);
        for k in 0..400 {
            let (x, z) = (
                -20000.0 + 100.0 * f64::from(k % 20) * 20.0 + 37.0,
                -20000.0 + 100.0 * f64::from(k / 20) * 20.0 + 11.0,
            );
            let e = t.landform_exact(x, z);
            tall = tall.max(e.abs());
            worst = worst.max((t.boundary(x, z) - e).abs());
        }
        // Within a fifth of the tallest landform (the 500 m bilinear and the side's step).
        assert!(
            worst <= 0.25 * tall.max(1.0),
            "worst {worst:.0} of {tall:.0}"
        );
    }

    #[test]
    fn a_hotspot_chain_ages_along_its_plates_motion() {
        // face-a-only: plate draws are face-seeded; the seeds stand for both faces' draws.
        let mut seen = 0;
        for seed in 0..40 {
            let t = PlateTectonics::for_seed(seed);
            for hs in &t.layout.hotspots {
                let m = t.layout.plates[hs.plate].motion;
                let chain: Vec<_> = t
                    .volcanoes
                    .iter()
                    .filter(|v| v.chain > 0)
                    .filter(|v| {
                        let (dx, dz) = (v.x - hs.at[0], v.z - hs.at[1]);
                        // On this hotspot's line.
                        (dx * hs.trail[1] - dz * hs.trail[0]).abs() < 1.0
                            && dx * hs.trail[0] + dz * hs.trail[1] >= -1.0
                    })
                    .collect();
                if chain.len() < 2 {
                    continue;
                }
                seen += 1;
                for w in chain.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    assert!(b.chain > a.chain && b.height < a.height && b.radius > a.radius);
                    // The older island lies farther along the plate's motion.
                    let along = |v: &Volcano| v.x * m[0] + v.z * m[1];
                    assert!(along(b) > along(a), "seed {seed}");
                }
            }
        }
        assert!(seen >= 5, "{seen} chains");
    }
}
