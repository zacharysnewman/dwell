//! Uplift from plate boundaries (docs/proposals/REALISTIC_WORLD.md §3.2, phase R2): a Realistic
//! world's tectonics (`terrain.json` `tectonics.plates` 1) from its face's seed-drawn plate layout
//! (`plates.rs`), not from a hand-placed line. Every boundary, ordered into polylines, adds the
//! landforms its **pair** gives (realistic batch A, 0.64: plates.json `uplift.pairs`, keyed by the
//! boundary's kind and its two plates' kinds), each on a side by rule:
//!
//! - two continents closing: a collision range astride; an ocean diving under a continent: a trench
//!   on the diving side, a range on the overriding side with an arc of dormant cones on its crest;
//!   two oceans: a trench, an island arc behind it and a back-arc basin behind that;
//! - two continents opening: a **segmented** rift (basins `rift.segmentLength` m long, each its own
//!   depth and width, deepest mid-segment, sills between them stepping down toward the sea,
//!   alternate axes `rift.stepOver` m apart; shoulders beside it, none at sea: a gulf where it
//!   meets the sea); a continent and an ocean opening: a passive margin (a wide shelf, a gentle
//!   coastal plain, no trough); two oceans: a low mid-ocean swell;
//! - transforms: a low ridge;
//!
//! each scaled by the boundary's rate (closing, opening or slip) over `speedRef`, clamped to
//! `speedScale`, and varied **along** the line by 1-D noise (`along`: height and width), so ranges
//! have saddles and peaks. An arc's cones are brought nearer the line when the overriding plate is
//! narrow (down to `arc.minDistance`). Every hotspot raises a chain of islands from its point
//! **along the plate's motion**, each older one lower and wider. Every landform fades to nothing at
//! the rim's coast (`PlateLayout::land_edge`): the outer band stays sea.
//!
//! The vocabulary is code (the sides, the three shapes, the arc modes, the chain rule); the tuning
//! is data (`shared/worldgen/plates.json` `uplift`, generated into `plates.gen.rs` as [`UPLIFT`]).
//! The boundaries' landforms are summed on a coarse grid ([`PlateUplift::cell`], ≥ 500 m) and read
//! bilinearly, so the layer is a function of vectors plus one coarse grid (§4: no new 100 m
//! whole-map grid). Face-local: each face's layout is its own face seed's, so both faces.

use crate::noise::{fbm2, mix32};
use crate::plates::{Boundary, BoundaryKind, PLATES, PlateKind, PlateLayout};
use crate::tectonics::{Tectonics, Volcano};

pub use crate::plates::UPLIFT;

/// Which plate a pair's landform stands on (code vocabulary, plates.json `uplift.pairs[].side`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// The overriding plate of a subducting boundary.
    Over,
    /// The diving plate of a subducting boundary.
    Under,
    /// Either plate.
    Astride,
    /// The pair's continental plate (both, for two continents).
    Continental,
    /// The pair's oceanic plate (both, for two oceans).
    Oceanic,
}

/// A landform's shape across and along its line (code vocabulary).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// A smooth bump `distance` m from the line, `width` m half-wide.
    Bump,
    /// The segmented rift basins ([`RiftRule`]), astride.
    Rift,
    /// A bump `distance` m beside the rift's (stepped) axis, left out at sea (a gulf).
    Shoulder,
}

/// Where a subducting pair's arc of cones stands (code vocabulary).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArcMode {
    None,
    /// On the crest of the pair's raised `over` landform (a continental arc).
    Crest,
    /// `arc.distance` m behind the line (an island arc).
    Island,
}

/// One landform of a pair (plates.json `uplift.pairs[].landforms[]`): `height` m (below 0 cuts),
/// `width` m half-wide, `distance` m from the line, on `side`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PairForm {
    pub side: Side,
    pub shape: Shape,
    pub height: f64,
    pub width: f64,
    pub distance: f64,
}

/// A boundary kind and plate pair's landforms (plates.json `uplift.pairs[]`; plates continental
/// first).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PairRule {
    pub boundary: BoundaryKind,
    pub plates: [PlateKind; 2],
    pub landforms: &'static [PairForm],
    pub arc: ArcMode,
}

/// The landforms' variation along their line (plates.json `uplift.along`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlongRule {
    /// ± shares of height and width.
    pub height: f64,
    pub width: f64,
    pub wavelength: i32,
    pub octaves: u32,
}

/// The segmented rift (plates.json `uplift.rift`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiftRule {
    /// Each segment's length (m), drawn in [min, max].
    pub segment_length: [f64; 2],
    /// Each segment's depth and width as shares of its landform's, drawn in [min, max].
    pub depth: [f64; 2],
    pub width: [f64; 2],
    /// The sills' depth as shares of the landform's, shallow upstream to deep toward the sea.
    pub sill: [f64; 2],
    /// Alternate segments' axes are this far apart (m).
    pub step_over: f64,
}

/// The arc's rule (plates.json `uplift.arc`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArcRule {
    /// An island arc's cones' distance (m) behind the line on the overriding side; any arc's
    /// cones come nearer the line in steps, down to `min_distance` m, to stand on that plate.
    pub distance: f64,
    pub min_distance: f64,
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
    /// At sea the boundaries' landforms raise the ground no higher than this many m below sea
    /// level: the oceans' ridges stay under water (the map check, 2026-11-14).
    pub sea_ridges: f64,
    /// The rate (cm/yr) at which a landform has its data's size, and the clamp on that scale.
    pub speed_ref: f64,
    pub speed_scale: [f64; 2],
    /// The polylines: scan points within `link` m are joined, vertices thinned to `segment` m.
    pub link: f64,
    pub segment: f64,
    /// The coarse grid's cell (m) the landforms are summed on.
    pub cell: f64,
    /// Every boundary kind × plate pair's landforms (all nine, each once).
    pub pairs: &'static [PairRule],
    pub along: AlongRule,
    pub rift: RiftRule,
    pub arc: ArcRule,
    pub hotspot: ChainRule,
    pub cones: ConeVariety,
}

/// Each placed cone and island varied (plates.json `uplift.cones`; the map check, 2026-11-14:
/// "perfectly round island clusters"): radius and height × draws in [min, max], a chain's spacing
/// and an arc's places ± `spacing` of theirs, islands `offset` m off their line either way, every
/// outline irregular by ± `shape` of its radius.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConeVariety {
    pub radius: [f64; 2],
    pub height: [f64; 2],
    pub spacing: f64,
    pub offset: f64,
    pub shape: f64,
    /// Each chain island's own size: its radius × a draw in this range, its height × one in its
    /// square roots' (so an older island stays the lower).
    pub each: [f64; 2],
    /// Satellite cones round each placed cone: `count` [min, max] of them, each `size` [min, max]
    /// of its parent's radius and height, its centre `distance` [min, max] of the parent's radius
    /// out at a seeded angle (islets off an island, parasitic cones on a volcano's flank).
    pub satellites: [f64; 2],
    pub satellite_size: [f64; 2],
    pub satellite_distance: [f64; 2],
    /// The chance an arc's site past its first is left empty (gaps along an arc).
    pub skip: f64,
}

/// A cone's outline factor at the angle `theta` (radians) round it: 1 ± `shape`, the sum of the
/// harmonics 2 to 7 with amplitudes and phases hashed from its centre (summing to 1), so no two
/// cones share an outline and none is a circle or a regular lobed shape.
pub fn outline(v: &Volcano, shape: f64, theta: f64) -> f64 {
    if shape <= 0.0 {
        return 1.0;
    }
    let key = mix32((v.x as i64 as u32) ^ mix32((v.z as i64 as u32) ^ 0x0C0E_0001));
    let (mut wave, mut sum) = (0.0, 0.0);
    for k in 2..=7u32 {
        let a = draw(key, 2 * k, [0.0, 1.0]) / f64::from(k).powf(0.6);
        wave +=
            a * (f64::from(k) * theta + draw(key, 2 * k + 1, [0.0, std::f64::consts::TAU])).sin();
        sum += a;
    }
    1.0 + shape * wave / sum.max(1e-9)
}

impl PlateUplift {
    /// The pair rule of a boundary kind and two plate kinds (in either order).
    pub fn pair(&self, kind: BoundaryKind, a: PlateKind, b: PlateKind) -> &PairRule {
        let key = if a == PlateKind::Continental || b == PlateKind::Oceanic {
            [a, b]
        } else {
            [b, a]
        };
        self.pairs
            .iter()
            .find(|p| p.boundary == kind && p.plates == key)
            .expect("plates.json uplift.pairs holds every pair (gen.mjs checks it)")
    }
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
    project(p, line, &[]).0
}

/// `p` against a polyline: its distance (m), the arc length (m) of its nearest point along the
/// line (with `lengths`, the cumulative length at each vertex; 0 without), and its signed distance
/// (> 0 on the line's left).
fn project(p: [f64; 2], line: &[[f64; 2]], lengths: &[f64]) -> (f64, f64, f64) {
    if line.len() == 1 {
        let d = to_segment(p, line[0], line[0]).0;
        return (d, 0.0, d);
    }
    let mut best = (f64::INFINITY, 0.0, 0.0);
    for (k, w) in line.windows(2).enumerate() {
        let (d, t) = to_segment(p, w[0], w[1]);
        if d < best.0 {
            let (ex, ez) = (w[1][0] - w[0][0], w[1][1] - w[0][1]);
            let cross = ex * (p[1] - w[0][1]) - ez * (p[0] - w[0][0]);
            let s = match (lengths.get(k), lengths.get(k + 1)) {
                (Some(&a), Some(&b)) => a + t * (b - a),
                _ => 0.0,
            };
            best = (d, s, if cross < 0.0 { -d } else { d });
        }
    }
    best
}

/// One segment of a rift polyline ([`RiftRule`]): its span along the line (m), its depth and
/// width shares, the sills' depth shares at its ends (`None`: open, a gulf), and which way its axis
/// steps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiftSegment {
    pub from: f64,
    pub to: f64,
    pub depth: f64,
    pub width: f64,
    pub sill_from: Option<f64>,
    pub sill_to: Option<f64>,
    pub step: f64,
}

/// A seeded draw in [lo, hi] from a word and an index.
fn draw(seed: u32, k: u32, [lo, hi]: [f64; 2]) -> f64 {
    let u = f64::from(mix32(seed ^ mix32(k.wrapping_mul(0x9E37_79B9)))) / 4_294_967_296.0;
    lo + (hi - lo) * u
}

/// A boundary as the uplift reads it: its polylines, its pair's landforms and its rate's scale.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// Index into the layout's boundaries.
    pub boundary: usize,
    pub kind: BoundaryKind,
    /// The overriding and the diving plate on a subducting boundary.
    pub over: Option<usize>,
    pub under: Option<usize>,
    /// Its pair's landforms (plates.json `uplift.pairs`).
    pub pair: PairRule,
    /// The landforms' scale from the boundary's rate.
    pub scale: f64,
    pub polylines: Vec<Vec<[f64; 2]>>,
    /// Each polyline's cumulative length (m) at each vertex.
    pub lengths: Vec<Vec<f64>>,
    /// Each polyline's rift segments (none unless the pair has a rift).
    pub segments: Vec<Vec<RiftSegment>>,
    seed: u32,
}

impl Line {
    /// Whether its pair cuts a rift.
    pub fn has_rift(&self) -> bool {
        self.pair.landforms.iter().any(|f| f.shape == Shape::Rift)
    }

    /// The along-line variation at arc length `s` of polyline `k`: (height, width) factors.
    fn along(&self, a: &AlongRule, k: usize, s: f64) -> (f64, f64) {
        let n = |salt: u32, amp: f64| {
            if amp <= 0.0 {
                return 1.0;
            }
            let v = fbm2(
                mix32(self.seed ^ salt),
                s as i64,
                k as i64 * 1_000_003,
                a.wavelength,
                a.octaves,
                None,
            );
            1.0 + amp * (2.0 * f64::from(v)).clamp(-1.0, 1.0)
        };
        (n(0x4A11, a.height), n(0x4B22, a.width))
    }

    /// The rift at arc length `s` of polyline `k`: (depth share, width share, axis offset m).
    fn rift_at(&self, r: &RiftRule, k: usize, s: f64) -> (f64, f64, f64) {
        let segs = &self.segments[k];
        if segs.is_empty() {
            return (1.0, 1.0, 0.0);
        }
        let i = segs.iter().position(|g| s < g.to).unwrap_or(segs.len() - 1);
        let g = &segs[i];
        let t = ((s - g.from) / (g.to - g.from).max(1.0)).clamp(0.0, 1.0);
        let wave = (std::f64::consts::PI * t).sin();
        let edge = if t < 0.5 { g.sill_from } else { g.sill_to }.unwrap_or(g.depth);
        let depth = edge + (g.depth - edge) * wave;
        // The width blends between the segments' middles.
        let mid = 0.5 * (g.from + g.to);
        let j = if s < mid {
            i.checked_sub(1)
        } else {
            (i + 1 < segs.len()).then_some(i + 1)
        };
        let width = match j {
            Some(j) => {
                let h = &segs[j];
                let other = 0.5 * (h.from + h.to);
                let u = ((s - mid) / (other - mid)).clamp(0.0, 1.0);
                g.width + (h.width - g.width) * 0.5 * u
            }
            None => g.width,
        };
        (depth, width, g.step * 0.5 * r.step_over * wave)
    }
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
            .map(|(i, b)| Self::line_of(&layout, &u, i, b))
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
        let satellites = t.satellites();
        t.volcanoes.extend(satellites);
        t.fill_field();
        t
    }

    fn line_of(layout: &PlateLayout, u: &PlateUplift, i: usize, b: &Boundary) -> Line {
        let [pa, pb] = b.plates.map(|k| layout.plates[k].kind);
        let pair = *u.pair(b.meeting.kind, pa, pb);
        let polylines = b.polylines(u.link, u.segment);
        let lengths: Vec<Vec<f64>> = polylines
            .iter()
            .map(|pl| {
                let mut acc = vec![0.0];
                for w in pl.windows(2) {
                    let d = ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt();
                    acc.push(acc[acc.len() - 1] + d);
                }
                acc
            })
            .collect();
        let first = polylines.first().and_then(|p| p.first()).copied();
        let seed = mix32(
            (b.plates[0] as u32) << 16
                ^ b.plates[1] as u32
                ^ first.map_or(0, |p| mix32(p[0] as i32 as u32) ^ p[1] as i32 as u32),
        );
        let rift = pair.landforms.iter().any(|f| f.shape == Shape::Rift);
        let r = &u.rift;
        let segments = polylines
            .iter()
            .zip(&lengths)
            .enumerate()
            .map(|(k, (pl, len))| {
                if !rift || pl.len() < 2 {
                    return Vec::new();
                }
                let total = len[len.len() - 1];
                let key = mix32(seed ^ (k as u32).wrapping_mul(0x85EB_CA6B));
                // Cut into segments; a short remainder joins the last.
                let mut cuts = vec![0.0];
                let mut j = 0;
                while cuts[cuts.len() - 1] < total {
                    let step = draw(key, 3 * j, r.segment_length);
                    let at = cuts[cuts.len() - 1] + step;
                    if total - at < 0.5 * r.segment_length[0] {
                        cuts.push(total);
                        break;
                    }
                    cuts.push(at.min(total));
                    j += 1;
                }
                let m = cuts.len() - 1;
                // The sills step down toward the end nearer the sea; an end at sea is open.
                let (head, tail) = (pl[0], pl[pl.len() - 1]);
                let (ch, ct) = (
                    layout.coast_distance(head[0], head[1]),
                    layout.coast_distance(tail[0], tail[1]),
                );
                let down_at_tail = ct <= ch;
                let sill = |c: usize| {
                    let share = if m == 0 { 0.0 } else { c as f64 / m as f64 };
                    let share = if down_at_tail { share } else { 1.0 - share };
                    let open = (c == 0 && ch < 0.0) || (c == m && ct < 0.0);
                    (!open).then(|| r.sill[0] + (r.sill[1] - r.sill[0]) * share)
                };
                (0..m)
                    .map(|c| RiftSegment {
                        from: cuts[c],
                        to: cuts[c + 1],
                        depth: draw(key, 3 * c as u32 + 1, r.depth),
                        width: draw(key, 3 * c as u32 + 2, r.width),
                        sill_from: sill(c),
                        sill_to: sill(c + 1),
                        step: if c % 2 == 0 { 1.0 } else { -1.0 },
                    })
                    .collect()
            })
            .collect();
        Line {
            boundary: i,
            kind: b.meeting.kind,
            over: b.overriding_plate(),
            under: b.subducting_plate(),
            pair,
            scale: Self::scale_of(u, b),
            polylines,
            lengths,
            segments,
            seed,
        }
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
        let wide = 1.0 + u.along.width;
        let rw = u.rift.width[1];
        u.pairs
            .iter()
            .flat_map(|p| p.landforms.iter())
            .map(|f| match f.shape {
                Shape::Bump => f.distance + f.width * wide,
                Shape::Rift => f.width * rw * wide + 0.5 * u.rift.step_over,
                Shape::Shoulder => (f.distance + f.width * wide) * rw + 0.5 * u.rift.step_over,
            })
            .fold(0.0, f64::max)
    }

    /// One boundary's landforms (m) at `p`, on plate `plate`.
    fn landform(&self, line: &Line, plate: usize, p: [f64; 2], reach: f64) -> f64 {
        let u = &self.uplift;
        let mut near = (f64::INFINITY, 0.0, 0.0, 0usize);
        for (k, (pl, len)) in line.polylines.iter().zip(&line.lengths).enumerate() {
            let (d, s, signed) = project(p, pl, len);
            if d < near.0 {
                near = (d, s, signed, k);
            }
        }
        let (d, s, signed, k) = near;
        if d >= reach {
            return 0.0;
        }
        let kind = self.layout.plates[plate].kind;
        let (fh, fw) = line.along(&u.along, k, s);
        let mut h = 0.0;
        for f in line.pair.landforms {
            let on = match f.side {
                Side::Over => line.over == Some(plate),
                Side::Under => line.under == Some(plate),
                Side::Astride => true,
                Side::Continental => kind == PlateKind::Continental,
                Side::Oceanic => kind == PlateKind::Oceanic,
            };
            if !on {
                continue;
            }
            h += match f.shape {
                Shape::Bump => f.height * fh * bump((d - f.distance) / (f.width * fw).max(1.0)),
                Shape::Rift => {
                    let (depth, width, off) = line.rift_at(&u.rift, k, s);
                    f.height * depth * bump((signed - off) / (f.width * width * fw).max(1.0))
                }
                Shape::Shoulder => {
                    if self.layout.coast_distance(p[0], p[1]) < 0.0 {
                        continue;
                    }
                    let (_, width, off) = line.rift_at(&u.rift, k, s);
                    let across = (signed - off).abs() - f.distance * width;
                    f.height * fh * bump(across / (f.width * width * fw).max(1.0))
                }
            };
        }
        h * line.scale
    }

    /// How much of the raised landforms stands at (x, z): 1 inland, falling to 0 at the rim's
    /// coast (the outer band stays sea).
    pub fn rim_fade(&self, x: f64, z: f64) -> f64 {
        let w = self.layout.rules.rim.edge_sea.max(1.0);
        let t = (self.layout.land_edge(x, z) / w).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
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

    /// The boundaries' landform at (x, z) computed exactly (the coarse grid's nodes take it): the
    /// raised part faded at the rim ([`PlateTectonics::rim_fade`]).
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
            h += self.landform(line, plate, [x, z], reach);
        }
        if h > 0.0 { h * self.rim_fade(x, z) } else { h }
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

    /// A boundary landform's `rise` (m) over ground `ground` m from sea level at a point the
    /// plates' coast puts `coast` m from it (< 0 at sea): at sea a rise stops `sea_ridges` m
    /// below the surface (a ridge already above, or a cut, is kept as it is).
    pub fn at_sea(&self, rise: f64, ground: f64, coast: f64) -> f64 {
        if coast >= 0.0 || rise <= 0.0 {
            return rise;
        }
        rise.min((-self.uplift.sea_ridges - ground).max(0.0))
    }

    /// Everything the layer adds at (x, z) (m): the boundaries' landform and the tallest cone.
    pub fn height(&self, x: f64, z: f64) -> f64 {
        let cones = self
            .volcanoes
            .iter()
            .map(|v| Tectonics::cone(v, self.cone_distance(v, x, z)))
            .fold(0.0, f64::max);
        let cones = if cones > 0.0 {
            cones * self.rim_fade(x, z)
        } else {
            cones
        };
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
            let r = self.cone_distance(v, x, z);
            let t = (1.0 - r / v.radius.max(1.0)).clamp(0.0, 1.0);
            s = s.max(t * t * (3.0 - 2.0 * t));
        }
        k * s
    }

    /// The distance (m) from a cone's centre to (x, z) as its irregular outline reads it: the
    /// plain distance over [`outline`]'s factor at that angle.
    pub fn cone_distance(&self, v: &Volcano, x: f64, z: f64) -> f64 {
        let (dx, dz) = (x - v.x, z - v.z);
        (dx * dx + dz * dz).sqrt() / outline(v, self.uplift.cones.shape, dz.atan2(dx)).max(0.1)
    }

    /// The arcs: along each subducting boundary whose pair has an arc, on polylines at least
    /// `min_length` m long, one cone per `spacing` m (at most `max_cones`), evenly along it, on the
    /// overriding plate's side: on the crest of the pair's raised `over` landform (a continental
    /// arc) or `distance` m behind the line (an island arc). Where that site is off the overriding
    /// plate (a narrow plate, a junction) the cone comes nearer the line in steps of a fifth, down
    /// to `min_distance`; a cone with no site on the plate, or none inside the rim's coast, is left
    /// out (a site's whole cone stands clear of the rim's fade).
    fn place_arcs(&self) -> Vec<Volcano> {
        let a = &self.uplift.arc;
        let mut out = Vec::new();
        for line in &self.lines {
            let Some(over) = line.over else { continue };
            let base = match line.pair.arc {
                ArcMode::None => continue,
                ArcMode::Island => a.distance,
                ArcMode::Crest => line
                    .pair
                    .landforms
                    .iter()
                    .find(|f| f.side == Side::Over && f.height > 0.0)
                    .map_or(a.distance, |f| f.distance),
            };
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
                let cv = &self.uplift.cones;
                let key = mix32(line.seed ^ (pl.len() as u32).wrapping_mul(0x27D4_EB2F));
                for k in 0..count {
                    // A gap in the arc: a site left empty (`cones.skip`), never the first.
                    if k > 0 && draw(key, 4 * k + 3, [0.0, 1.0]) < cv.skip {
                        continue;
                    }
                    // Each cone's place along the line ± `cones.spacing` of the gap between them.
                    let slot = total / f64::from(count);
                    let jitter = cv.spacing * (draw(key, 4 * k, [-0.5, 0.5])) * slot;
                    let mut s = ((f64::from(k) + 0.5) * slot + jitter).clamp(0.0, total);
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
                    let mut dist = base.max(a.min_distance);
                    let mut found = None;
                    while found.is_none() {
                        found = [1.0, -1.0]
                            .into_iter()
                            .map(|sign| {
                                [
                                    at[0] + sign * dist * normal[0],
                                    at[1] + sign * dist * normal[1],
                                ]
                            })
                            .find(|c| {
                                self.layout.plate_at(c[0], c[1]) == over
                                    && self.layout.land_edge(c[0], c[1])
                                        > self.layout.rules.rim.edge_sea + 0.5 * a.cone_radius
                            });
                        if dist <= a.min_distance {
                            break;
                        }
                        dist = (0.8 * dist).max(a.min_distance);
                    }
                    let Some(c) = found else { continue };
                    out.push(Volcano {
                        x: c[0],
                        z: c[1],
                        height: a.cone_height * line.scale * draw(key, 4 * k + 1, cv.height),
                        radius: a.cone_radius * draw(key, 4 * k + 2, cv.radius),
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
        let cv = &self.uplift.cones;
        for (h, hs) in self.layout.hotspots.iter().enumerate() {
            let still = hs.trail == [0.0, 0.0];
            let key = mix32((hs.at[0] as i64 as u32) ^ mix32(hs.at[1] as i64 as u32 ^ h as u32));
            // The chain's own size (its islands older down it, lower and broader, as before).
            let (hf, rf) = (draw(key, 1, cv.height), draw(key, 2, cv.radius));
            let mut d = 0.0;
            let root = |[lo, hi]: [f64; 2]| [lo.sqrt(), hi.sqrt()];
            for k in 0..if still { c.islands.min(1) } else { c.islands } {
                let age = f64::from(k);
                if k > 0 {
                    d += c.spacing * (1.0 + cv.spacing * draw(key, 8 * k, [-1.0, 1.0]));
                }
                // Off its line either way (no row of islands on a ruler).
                let off = cv.offset * draw(key, 8 * k + 1, [-1.0, 1.0]);
                let (nx, nz) = (-hs.trail[1], hs.trail[0]);
                let (x, z) = (
                    hs.at[0] + d * hs.trail[0] + off * nx,
                    hs.at[1] + d * hs.trail[1] + off * nz,
                );
                out.push(Volcano {
                    x,
                    z,
                    height: c.height * hf * c.decay.powf(age) * draw(key, 8 * k + 2, root(cv.each)),
                    radius: c.radius * rf * (1.0 + c.widen * age) * draw(key, 8 * k + 3, cv.each),
                    crater_radius: 0.0,
                    crater_depth: 0.0,
                    chain: k + 1,
                });
            }
        }
        out
    }

    /// Every placed cone's satellites (`cones.satellites`), after all the cones (so a chain's
    /// islands stay together in `volcanoes`): each at a seeded angle and distance from its parent,
    /// smaller by a draw, of the parent's chain (its age and its rock).
    fn satellites(&self) -> Vec<Volcano> {
        let cv = &self.uplift.cones;
        let mut out = Vec::new();
        for v in &self.volcanoes {
            let key = mix32((v.x as i64 as u32) ^ mix32((v.z as i64 as u32) ^ 0x5A7E_0011));
            let n = draw(key, 1, [cv.satellites[0], cv.satellites[1] + 1.0]).floor() as u32;
            for k in 0..n {
                let th = draw(key, 8 * k + 2, [0.0, std::f64::consts::TAU]);
                let d = v.radius * draw(key, 8 * k + 3, cv.satellite_distance);
                let size = draw(key, 8 * k + 4, cv.satellite_size);
                out.push(Volcano {
                    x: v.x + d * th.cos(),
                    z: v.z + d * th.sin(),
                    height: v.height * size * draw(key, 8 * k + 5, [0.8, 1.2]),
                    radius: v.radius * size,
                    crater_radius: 0.0,
                    crater_depth: 0.0,
                    chain: v.chain,
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

    /// The first landform on `side` of the pair, by data.
    fn form(kind: BoundaryKind, a: PlateKind, b: PlateKind, side: Side) -> PairForm {
        *UPLIFT
            .pair(kind, a, b)
            .landforms
            .iter()
            .find(|f| f.side == side)
            .expect("the pair's landform")
    }

    #[test]
    fn every_boundary_and_plate_pair_has_its_landforms_by_data() {
        use PlateKind::{Continental as C, Oceanic as O};
        for kind in [
            BoundaryKind::Convergent,
            BoundaryKind::Divergent,
            BoundaryKind::Transform,
        ] {
            for (a, b) in [(C, C), (C, O), (O, C), (O, O)] {
                let p = UPLIFT.pair(kind, a, b);
                assert_eq!(p.boundary, kind);
                assert!(!p.landforms.is_empty(), "{kind:?} {a:?} {b:?}");
            }
        }
        // The pairs the brief names: rifts only between continents, a passive margin (no trough on
        // land) between a continent and an ocean, an island arc between oceans.
        let rift = |a, b| {
            UPLIFT
                .pair(BoundaryKind::Divergent, a, b)
                .landforms
                .iter()
                .any(|f| f.shape == Shape::Rift)
        };
        assert!(rift(C, C) && !rift(C, O) && !rift(O, O));
        assert!(
            UPLIFT
                .pair(BoundaryKind::Divergent, C, O)
                .landforms
                .iter()
                .filter(|f| f.side != Side::Oceanic)
                .all(|f| f.distance >= f.width),
            "no trough at a passive margin's coast"
        );
        assert_eq!(
            UPLIFT.pair(BoundaryKind::Convergent, O, O).arc,
            ArcMode::Island
        );
        assert_eq!(
            UPLIFT.pair(BoundaryKind::Convergent, C, O).arc,
            ArcMode::Crest
        );
    }

    #[test]
    fn each_boundary_kind_shapes_the_ground_by_its_rule() {
        use PlateKind::{Continental as C, Oceanic as O};
        let u = &UPLIFT;
        let along = 1.0 - u.along.height;
        // Subduction: the continent (plate 1, +x) rides over; its range stands behind the line,
        // the trench on the ocean's side, the arc's cones on its crest.
        let t = fixture("ocean_under_continent");
        let range = form(BoundaryKind::Convergent, C, O, Side::Over);
        let trench = form(BoundaryKind::Convergent, C, O, Side::Under);
        let up = t.boundary(range.distance, 0.0);
        assert!(up > 0.5 * along * range.height * u.speed_scale[0], "{up}");
        assert!(t.boundary(-trench.distance, 0.0) < 0.0, "a trench");
        assert!(t.boundary(-range.distance, 0.0) < 0.5 * up);
        let arc: Vec<_> = t.volcanoes.iter().filter(|v| v.chain == 0).collect();
        assert!(!arc.is_empty(), "an arc");
        for v in &arc {
            assert!(v.x > 0.0, "on the overriding side: {v:?}");
            // On the crest, but for a cone the rim's coast brought nearer the line (and the
            // satellites, craterless, round each).
            if v.x.hypot(v.z) < 38_000.0 && v.crater_radius > 0.0 {
                assert!((v.x - range.distance).abs() < 600.0, "on the crest: {v:?}");
            }
        }
        // Collision: a range astride the line, no arc.
        let t = fixture("continents_collide");
        assert!(t.boundary(0.0, 0.0) > 0.0);
        assert!(t.volcanoes.iter().all(|v| v.chain != 0), "no arc");
        // Two continents opening: a rift below the plain along the line.
        let t = fixture("continents_part");
        let rift = form(BoundaryKind::Divergent, C, C, Side::Astride);
        let low = (-30..=30)
            .map(|k| t.boundary(0.0, f64::from(k) * 1000.0))
            .fold(f64::INFINITY, f64::min);
        assert!(low < 0.5 * rift.height * u.speed_scale[0], "{low}");
        // A continent and an ocean opening: a passive margin, no trough on the land side.
        let t = fixture("rift_opens");
        let margin = (0..=40)
            .map(|k| t.boundary(-f64::from(k) * 250.0, 0.0))
            .fold(f64::INFINITY, f64::min);
        let cut = form(BoundaryKind::Divergent, C, O, Side::Continental);
        assert!(margin >= cut.height * u.speed_scale[1] - 1.0, "{margin}");
        assert!(t.boundary(-500.0, 0.0) > -5.0, "no trough at the coast");
        // Transform: little relief.
        let t = fixture("plates_slide_past");
        let ridge = form(BoundaryKind::Transform, C, O, Side::Astride);
        let most = (-40..=40)
            .map(|k| t.boundary(f64::from(k) * 250.0, 0.0).abs())
            .fold(0.0, f64::max);
        assert!(
            most <= ridge.height * (1.0 + u.along.height) * u.speed_scale[1] + 1.0,
            "{most}"
        );
        assert!(most < 0.5 * range.height * u.speed_scale[0]);
        // Two oceans closing: an island arc behind the trench, a back-arc basin behind that.
        let t = fixture("oceans_converge_far");
        let arc: Vec<_> = t.volcanoes.iter().filter(|v| v.chain == 0).collect();
        assert!(!arc.is_empty(), "an island arc");
        let over = t.lines[0].over.expect("one dives");
        for v in &arc {
            assert_eq!(t.layout.plate_at(v.x, v.z), over, "{v:?}");
        }
    }

    #[test]
    fn boundaries_follow_the_plates_not_the_scans_grid() {
        // BUGS.md #164: a boundary point at the scan cells' midpoint put a near-vertical or
        // near-horizontal stretch on one grid line (an axis-aligned polyline km long, and every
        // landform and province edge along it). The crossing between the cells is found instead.
        // face-a-only: plate layouts are face-seeded draws; the seeds stand for both faces'.
        let mut longest = 0.0f64;
        for seed in 0..20 {
            let t = PlateTectonics::for_seed(seed);
            for line in &t.lines {
                for pl in &line.polylines {
                    for axis in [0, 1] {
                        let mut run = 0.0;
                        for w in pl.windows(2) {
                            if (w[0][axis] - w[1][axis]).abs() < 1e-6 {
                                run += (w[1][1 - axis] - w[0][1 - axis]).abs();
                                longest = longest.max(run);
                            } else {
                                run = 0.0;
                            }
                        }
                    }
                }
            }
        }
        // The bug's stretches ran up to 30 km; a boundary wiggling across one scan column (the
        // warp's finest layer) can put three vertices on it, a run of two segments.
        assert!(
            longest < 2500.0,
            "an axis-aligned boundary stretch {longest:.0} m long"
        );
    }

    #[test]
    fn a_rift_is_cut_into_basins_with_sills_and_steps() {
        let r = &UPLIFT.rift;
        let t = fixture("continents_part");
        let line = &t.lines[0];
        assert!(line.has_rift());
        let segs = line.segments.iter().max_by_key(|s| s.len()).unwrap();
        assert!(segs.len() >= 3, "{} segments", segs.len());
        for (i, g) in segs.iter().enumerate() {
            let len = g.to - g.from;
            assert!(
                len <= r.segment_length[1] + 0.5 * r.segment_length[0] + 1.0,
                "{len}"
            );
            assert!((r.depth[0]..=r.depth[1]).contains(&g.depth));
            if i + 1 < segs.len() {
                // Neighbours share their sill, and step to opposite sides.
                assert_eq!(g.sill_to, segs[i + 1].sill_from);
                assert_eq!(g.step, -segs[i + 1].step);
                let sill = g.sill_to.expect("an inner sill");
                assert!(sill < g.depth && sill < segs[i + 1].depth);
            }
        }
        // Along the line: deepest mid-segment, shallower at each sill.
        let k = line
            .segments
            .iter()
            .position(|s| s.len() == segs.len())
            .unwrap();
        for g in &segs[1..segs.len() - 1] {
            let mid = line.rift_at(r, k, 0.5 * (g.from + g.to)).0;
            let end = line.rift_at(r, k, g.to).0;
            assert!(mid > end, "{mid} {end}");
        }
        // The sills step one way, down toward one end.
        let sills: Vec<f64> = segs.windows(2).filter_map(|w| w[0].sill_to).collect();
        let up = sills.windows(2).all(|w| w[1] >= w[0]);
        let down = sills.windows(2).all(|w| w[1] <= w[0]);
        assert!(up || down, "{sills:?}");
    }

    #[test]
    fn a_ranges_height_varies_along_its_line() {
        // Saddles and peaks: along a 100 km subducting line the range's crest varies by more than
        // a fifth of its height.
        let t = fixture("ocean_under_continent");
        let d = UPLIFT
            .pair(
                BoundaryKind::Convergent,
                PlateKind::Continental,
                PlateKind::Oceanic,
            )
            .landforms[0]
            .distance;
        let crest: Vec<f64> = (-40..=40)
            .map(|k| t.landform_exact(d, f64::from(k) * 1000.0))
            .collect();
        let (lo, hi) = crest
            .iter()
            .fold((f64::INFINITY, 0.0f64), |(a, b), &h| (a.min(h), b.max(h)));
        assert!(hi - lo > 0.2 * hi, "{lo:.0}..{hi:.0}");
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
            // Each hotspot's islands follow the last's (`volcanoes` after the arcs' cones).
            let mut islands = t.volcanoes.iter().filter(|v| v.chain > 0);
            for hs in &t.layout.hotspots {
                let m = t.layout.plates[hs.plate].motion;
                let n = if hs.trail == [0.0, 0.0] {
                    UPLIFT.hotspot.islands.min(1)
                } else {
                    UPLIFT.hotspot.islands
                };
                let chain: Vec<_> = islands.by_ref().take(n as usize).collect();
                if chain.len() < 2 {
                    continue;
                }
                seen += 1;
                for w in chain.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    // Older is lower; each island's own size draw (`cones.each`) varies the radius.
                    assert!(b.chain > a.chain && b.height < a.height);
                    // The older island lies farther along the plate's motion.
                    let along = |v: &Volcano| v.x * m[0] + v.z * m[1];
                    assert!(along(b) > along(a), "seed {seed}");
                }
            }
        }
        assert!(seen >= 5, "{seen} chains");
    }
}
