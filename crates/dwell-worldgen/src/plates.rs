//! The plate layout a Realistic world's seed draws (docs/proposals/REALISTIC_WORLD.md §3.1, phase
//! R1): a few plates, each oceanic or continental with a motion and an age, the boundaries between
//! neighbours with the kind their relative motion gives (convergent, divergent, transform; for a
//! convergent one which side subducts), hotspots fixed under the moving plates, and a land/sea mask
//! from the plates' kinds (owner, 2026-11-11: oceanic plate = sea, continental = land, only a sea
//! margin kept at the disc's edge).
//!
//! The vocabulary is code: the two plate kinds, the three boundary kinds and the subduction rule
//! (oceanic under continental; the older under the younger when both are oceanic; none, a
//! collision, when both are continental). The tuning is data (`shared/worldgen/plates.json`,
//! generated into `plates.gen.rs`): the count, size, speed and age ranges, the kinds' weights, the
//! warp, the transform threshold, the hotspots, and two-plate fixtures the tests check by data alone.
//!
//! The layout is a small vector description (plates, boundary samples, hotspots), not a whole-map
//! grid (§4): the boundaries are found on a coarse scan ([`SCAN_CELL`]) and the plate under a
//! point is a function of the vectors. It is face-local: each face draws its own layout from its
//! face seed (owner, 2026-11-11: face B its own plates, a different region), in that face's x, z.
//! A Realistic world (`tectonics.plates` 1, phase R2) reads it through `plate_tectonics.rs`: the
//! plates' coast is the base field's coast, and the boundaries, ordered into polylines
//! ([`Boundary::polylines`]), carry the uplift.

use dwell_world::Face;

use crate::bifacial::face_seed;
use crate::noise::{fbm2, mix32, seed_word};

/// A plate's crust (code vocabulary; plates.json `kinds` weights them).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlateKind {
    /// Sea floor: sea in the land/sea mask, and the side that subducts under a continent.
    Oceanic,
    /// A continent: land in the mask.
    Continental,
}

/// How two neighbouring plates meet, from their relative motion across the boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BoundaryKind {
    /// Closing: a trench and arc where one side subducts, a collision range where neither does.
    Convergent,
    /// Opening: a rift or a spreading ridge.
    Divergent,
    /// Sliding past: the closing (or opening) rate is under `transform_below` of the slip.
    Transform,
}

/// A plate kind's draw (plates.json `kinds.<kind>`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KindRule {
    /// The share of plates drawn this kind (relative to the other kind's weight).
    pub weight: f64,
    /// The least number of plates of this kind a layout keeps.
    pub min: u32,
    /// The least distance (m) some point of a plate of this kind lies from every other plate and
    /// from the rim inside the land area ([`PlateLayout::interiors`]): no sliver plates.
    pub interior: f64,
}

/// The rim (plates.json `rim`): where the world's disc ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RimRule {
    /// The outer band (m) inside the reach that is always sea, at least `edge_coast` m out.
    pub edge_sea: f64,
    pub edge_coast: f64,
    /// A continent reaching the rim ends this far (m) inside the band, drawn per sector.
    pub shelf: [f64; 2],
    pub sectors: u32,
    /// Every plate-edge coast moves by ±`coast_amplitude` m of noise at `coast_wavelength` m
    /// (`coast_octaves`): bays and headlands, no coast straight for km (the map check, 2026-11-14).
    pub coast_amplitude: f64,
    pub coast_wavelength: i32,
    pub coast_octaves: u32,
    /// The rim's coast moves by ±this much (m) of noise at this wavelength (m).
    pub noise_amplitude: f64,
    pub noise_wavelength: i32,
}

/// The plate layout's rules (data: `shared/worldgen/plates.json`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlateRules {
    /// The seed picks a plate count in `count_min ..= count_max`.
    pub count_min: u32,
    pub count_max: u32,
    /// The disc (m) the layout covers (the world's radius at the default size).
    pub reach: f64,
    /// Plate centres are drawn within this distance (m) of the centre (≥ `reach`).
    pub centres: f64,
    /// The least distance between two plates' centres (m), tried at up to `attempts` points each.
    pub spacing: f64,
    pub attempts: u32,
    pub oceanic: KindRule,
    pub continental: KindRule,
    /// A plate's speed (cm/yr) and age (Myr), drawn uniformly in [min, max].
    pub speed: [f64; 2],
    pub age: [f64; 2],
    /// The boundaries curve by this much noise (m) at this wavelength (m).
    pub warp_amplitude: f64,
    pub warp_wavelength: i32,
    /// Finer warps over the first, (amplitude m, wavelength m) each: sweeping arcs with smaller
    /// bends, not a Voronoi's straight bisectors (owner, 2026-11-14: "our tectonic plates are too
    /// straight").
    pub warp_detail: &'static [(f64, i32)],
    /// A boundary is a transform where |closing rate| < this × the relative speed.
    pub transform_below: f64,
    /// The seed picks a hotspot count in `hotspots_min ..= hotspots_max`, each within
    /// `hotspots_within` m of the centre.
    pub hotspots_min: u32,
    pub hotspots_max: u32,
    pub hotspots_within: f64,
    pub rim: RimRule,
}

/// A fixture plate (plates.json `fixtures[].plates`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FixturePlate {
    pub kind: PlateKind,
    pub at: [f64; 2],
    pub motion: [f64; 2],
    pub age: f64,
}

/// Two plates and the boundary they must give (plates.json `fixtures`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlateFixture {
    pub name: &'static str,
    pub plates: [FixturePlate; 2],
    pub kind: BoundaryKind,
    /// The index (0 or 1) of the plate that dives, if one does.
    pub subducts: Option<usize>,
}

/// How the map's erosion routes each cell's water down (plates.json `variety.flow`, a Realistic
/// world's; WORK_QUEUE item 15).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FlowRouting {
    /// Every cell to the neighbour it was flooded from (the default world's).
    D8,
    /// Each erosion step, every cell to one strictly lower neighbour drawn by its slope to the
    /// power `exponent` (a randomized single-flow direction: the channels leave the grid's lines).
    Random { exponent: f64 },
}

/// The base field's interior relief on a Realistic world's land (plates.json `variety.relief`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InteriorRelief {
    /// The sample point's domain warp: amplitude (m) and wavelength (m).
    pub warp_amplitude: f64,
    pub warp_wavelength: f64,
    /// Basins and swells: about ±`swell_height` m at `swell_wavelength` m.
    pub swell_height: f64,
    pub swell_wavelength: f64,
    pub swell_octaves: u32,
    /// The hills' and bumps' amplitude × a regional noise between `hill_min` and `hill_max`, at
    /// `hill_wavelength` m.
    pub hill_min: f64,
    pub hill_max: f64,
    pub hill_wavelength: f64,
    /// Each octave turned this far (degrees) from the last, and this much finer.
    pub rotate: f64,
    pub lacunarity: f64,
}

/// The Realistic world's variety (plates.json `variety`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlateVariety {
    pub flow: FlowRouting,
    pub relief: InteriorRelief,
}

mod table {
    include!("plates.gen.rs");
}
pub use table::{FIXTURES, PLATES, UPLIFT, VARIETY};

/// The cell of the coarse scan that finds the boundaries (m): ≥ 500 m, so the layout stays a
/// function of vectors at any map size (REALISTIC_WORLD.md §4).
pub const SCAN_CELL: f64 = 500.0;

/// The scan (m) that measures each plate's interior ([`PlateLayout::interiors`]).
pub const INTERIOR_CELL: f64 = 1000.0;

/// The seed streams of the layout (`seed_word` streams; unused elsewhere).
const STREAM: u32 = 0x504C_0000;

/// A plate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plate {
    pub kind: PlateKind,
    /// Its centre (face-local x, z, m).
    pub centre: [f64; 2],
    /// Its motion (cm/yr, face-local x, z).
    pub motion: [f64; 2],
    /// Its age (Myr): of two oceanic plates meeting, the older subducts.
    pub age: f64,
}

impl From<FixturePlate> for Plate {
    fn from(p: FixturePlate) -> Self {
        Self {
            kind: p.kind,
            centre: p.at,
            motion: p.motion,
            age: p.age,
        }
    }
}

/// How two plates meet: the kind, the rates and which side subducts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Meeting {
    pub kind: BoundaryKind,
    /// The rate they close across the boundary (cm/yr; negative where they open).
    pub closing: f64,
    /// The rate they slide along it (cm/yr, ≥ 0).
    pub slip: f64,
    /// Which of the two (0: `a`, 1: `b`) dives under the other, on a convergent boundary.
    pub subducts: Option<usize>,
}

/// The boundary between plates `a` and `b`, from their relative motion across the line between
/// their centres (the boundary's mean normal). `transform_below` is plates.json's threshold.
pub fn classify(a: &Plate, b: &Plate, transform_below: f64) -> Meeting {
    let (nx, nz) = (b.centre[0] - a.centre[0], b.centre[1] - a.centre[1]);
    let len = (nx * nx + nz * nz).sqrt();
    let (nx, nz) = if len > 0.0 {
        (nx / len, nz / len)
    } else {
        (1.0, 0.0)
    };
    // b's motion relative to a; closing when b moves back toward a.
    let (rx, rz) = (b.motion[0] - a.motion[0], b.motion[1] - a.motion[1]);
    let closing = -(rx * nx + rz * nz);
    let slip = (rx * nz - rz * nx).abs();
    let speed = (rx * rx + rz * rz).sqrt();
    let kind = if closing.abs() <= transform_below * speed {
        BoundaryKind::Transform
    } else if closing > 0.0 {
        BoundaryKind::Convergent
    } else {
        BoundaryKind::Divergent
    };
    let subducts = match (kind, a.kind, b.kind) {
        (BoundaryKind::Convergent, PlateKind::Oceanic, PlateKind::Continental) => Some(0),
        (BoundaryKind::Convergent, PlateKind::Continental, PlateKind::Oceanic) => Some(1),
        (BoundaryKind::Convergent, PlateKind::Oceanic, PlateKind::Oceanic) => {
            Some(if b.age > a.age { 1 } else { 0 })
        }
        _ => None,
    };
    Meeting {
        kind,
        closing,
        slip,
        subducts,
    }
}

/// A boundary between two neighbouring plates.
#[derive(Clone, Debug, PartialEq)]
pub struct Boundary {
    /// The two plates (indices into [`PlateLayout::plates`], the lower first).
    pub plates: [usize; 2],
    pub meeting: Meeting,
    /// Points on it (face-local x, z, m): midpoints between scan cells either side, in scan order.
    pub points: Vec<[f64; 2]>,
}

impl Boundary {
    /// The plate that dives under the other, by index into the layout's plates.
    pub fn subducting_plate(&self) -> Option<usize> {
        self.meeting.subducts.map(|i| self.plates[i])
    }

    /// The plate that rides over the other (the arc's side), on a subducting boundary.
    pub fn overriding_plate(&self) -> Option<usize> {
        self.meeting.subducts.map(|i| self.plates[1 - i])
    }

    /// The boundary's scan points ordered into polylines (R2): from a point, walk to the nearest
    /// unvisited point within `link` m, one way then the other; the points within
    /// [`ABSORB`] × [`SCAN_CELL`] of each point taken are the same stretch (the scan's staircase)
    /// and dropped; a gap wider than `link` starts a new polyline. Then each line is thinned to
    /// vertices at least `spacing` m apart (its two ends kept).
    pub fn polylines(&self, link: f64, spacing: f64) -> Vec<Vec<[f64; 2]>> {
        let mut left = self.points.clone();
        let absorb = (ABSORB * SCAN_CELL).powi(2);
        let mut lines = Vec::new();
        while !left.is_empty() {
            let start = left.remove(0);
            left.retain(|&q| dist2(q, start) > absorb);
            let mut line = std::collections::VecDeque::from([start]);
            for front in [false, true] {
                loop {
                    let end = if front { line[0] } else { line[line.len() - 1] };
                    let next = left
                        .iter()
                        .enumerate()
                        .map(|(i, &q)| (dist2(q, end), i))
                        .filter(|&(d, _)| d <= link * link)
                        .min_by(|a, b| a.0.total_cmp(&b.0));
                    let Some((_, i)) = next else { break };
                    let q = left.remove(i);
                    left.retain(|&r| dist2(r, q) > absorb);
                    if front {
                        line.push_front(q);
                    } else {
                        line.push_back(q);
                    }
                }
            }
            let line: Vec<[f64; 2]> = line.into();
            let mut thin = vec![line[0]];
            for (k, &q) in line.iter().enumerate().skip(1) {
                let last = k + 1 == line.len();
                if last || dist2(q, thin[thin.len() - 1]) >= spacing * spacing {
                    thin.push(q);
                }
            }
            lines.push(thin);
        }
        lines
    }
}

/// How many halvings place a boundary point between two scan cells (500 m / 2⁸ ≈ 2 m).
pub const CROSSING_STEPS: u32 = 8;

/// The scan points within this many scan cells of a point taken into a polyline are its stretch.
pub const ABSORB: f64 = 0.8;

/// A hotspot: a point fixed in the mantle under a moving plate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hotspot {
    /// Where it is (face-local x, z, m).
    pub at: [f64; 2],
    /// The plate over it.
    pub plate: usize,
    /// The unit direction its chain runs, older islands that way: along the plate's motion (the
    /// plate carries each island off the fixed hotspot, R2); zero for a still plate.
    pub trail: [f64; 2],
}

/// A face's plate layout.
#[derive(Clone, Debug, PartialEq)]
pub struct PlateLayout {
    pub rules: PlateRules,
    pub plates: Vec<Plate>,
    pub boundaries: Vec<Boundary>,
    pub hotspots: Vec<Hotspot>,
    warp_seeds: (u32, u32),
    /// The rim's shelf (m) at each sector's middle, round the disc from +x toward +z.
    pub rim_shelf: Vec<f64>,
    rim_seed: u32,
}

/// A stream of seeded draws in [0, 1).
struct Draws {
    seed: u32,
    k: u32,
}

impl Draws {
    fn new(seed: u64, stream: u32) -> Self {
        Self {
            seed: seed_word(seed, STREAM + stream),
            k: 0,
        }
    }
    fn next(&mut self) -> f64 {
        self.k += 1;
        f64::from(mix32(self.seed ^ mix32(self.k.wrapping_mul(0x9E37_79B9)))) / 4_294_967_296.0
    }
    fn between(&mut self, [lo, hi]: [f64; 2]) -> f64 {
        lo + (hi - lo) * self.next()
    }
    fn count(&mut self, lo: u32, hi: u32) -> u32 {
        let n = lo + (self.next() * f64::from(hi - lo + 1)) as u32;
        n.min(hi)
    }
    /// A uniform point in the disc of radius `r`.
    fn in_disc(&mut self, r: f64) -> [f64; 2] {
        loop {
            let (x, z) = (2.0 * self.next() - 1.0, 2.0 * self.next() - 1.0);
            if x * x + z * z <= 1.0 {
                return [x * r, z * r];
            }
        }
    }
    /// A uniform unit vector.
    fn direction(&mut self) -> [f64; 2] {
        loop {
            let (x, z) = (2.0 * self.next() - 1.0, 2.0 * self.next() - 1.0);
            let r2 = x * x + z * z;
            if r2 > 1e-4 && r2 <= 1.0 {
                let r = r2.sqrt();
                return [x / r, z / r];
            }
        }
    }
}

fn dist2(a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dz) = (a[0] - b[0], a[1] - b[1]);
    dx * dx + dz * dz
}

impl PlateLayout {
    /// The layout `face` of the world `world_seed` draws from the build's rules (its face seed).
    pub fn for_face(world_seed: u64, face: Face) -> Self {
        Self::generate(&PLATES, face_seed(world_seed, face))
    }

    /// The layout `seed` draws from `rules`.
    pub fn generate(rules: &PlateRules, seed: u64) -> Self {
        let plates = draw_plates(rules, seed);
        let mut d = Draws::new(seed, 4);
        let rim_shelf = (0..rules.rim.sectors.max(1))
            .map(|_| d.between(rules.rim.shelf))
            .collect();
        let mut layout = Self {
            rules: *rules,
            plates,
            boundaries: Vec::new(),
            hotspots: Vec::new(),
            warp_seeds: (seed_word(seed, STREAM + 10), seed_word(seed, STREAM + 11)),
            rim_shelf,
            rim_seed: seed_word(seed, STREAM + 12),
        };
        layout.settle();
        layout.boundaries = layout.scan_boundaries();
        layout.hotspots = layout.draw_hotspots(seed);
        layout
    }

    /// The plates' fixture layout (no warp): its one boundary is classified as any other.
    pub fn of_plates(rules: &PlateRules, plates: Vec<Plate>) -> Self {
        let mut rules = *rules;
        rules.warp_amplitude = 0.0;
        rules.warp_detail = &[];
        let mut layout = Self {
            rules,
            plates,
            boundaries: Vec::new(),
            hotspots: Vec::new(),
            warp_seeds: (0, 0),
            rim_shelf: vec![rules.rim.shelf[0]],
            rim_seed: 0,
        };
        layout.boundaries = layout.scan_boundaries();
        layout
    }

    /// The plate under a face-local point (the nearest centre to the warped point).
    pub fn plate_at(&self, x: f64, z: f64) -> usize {
        let (wx, wz) = self.warped(x, z);
        let mut best = (f64::INFINITY, 0);
        for (i, p) in self.plates.iter().enumerate() {
            let d = dist2([wx, wz], p.centre);
            if d < best.0 {
                best = (d, i);
            }
        }
        best.1
    }

    /// Whether a face-local point is land: inside the plates' coast ([`PlateLayout::coast_distance`]
    /// above 0: a continental plate, its coast moved by the coast noise, inside the rim's coast).
    pub fn is_land(&self, x: f64, z: f64) -> bool {
        !self.plates.is_empty() && self.coast_distance(x, z) > 0.0
    }

    /// The signed distance (m) to the plates' coast at a face-local point: > 0 on land (a
    /// continental plate inside the rim's coast), < 0 at sea; the land/sea mask's sign
    /// ([`PlateLayout::is_land`]). The distance to a plate's edge is its warped Voronoi bisector's
    /// with the nearest plate of the other kind, capped by the distance to the rim's coast
    /// ([`PlateLayout::land_edge`]); in the rim's outer band at most −`edge_coast` (deep sea).
    pub fn coast_distance(&self, x: f64, z: f64) -> f64 {
        let mut edge = self.land_edge(x, z);
        let rim = &self.rules.rim;
        if (x * x + z * z).sqrt() > self.rules.reach - rim.edge_sea {
            edge = edge.min(-rim.edge_coast);
        }
        if self.plates.is_empty() {
            return edge.min(-1.0);
        }
        let (wx, wz) = self.warped(x, z);
        let a = self.plate_at(x, z);
        let pa = &self.plates[a];
        let da = dist2([wx, wz], pa.centre);
        let mut near = f64::INFINITY;
        for (b, pb) in self.plates.iter().enumerate() {
            if b == a || pb.kind == pa.kind {
                continue;
            }
            let gap = dist2(pa.centre, pb.centre).sqrt().max(1.0);
            near = near.min((dist2([wx, wz], pb.centre) - da) / (2.0 * gap));
        }
        let near = near.min(1.0e6);
        let signed = match pa.kind {
            PlateKind::Continental => near,
            PlateKind::Oceanic => -near,
        };
        (signed + self.coast_noise(x, z)).min(edge)
    }

    /// The plate-edge coasts' noise (m, `rim.coast`) at a face-local point.
    pub fn coast_noise(&self, x: f64, z: f64) -> f64 {
        let r = &self.rules.rim;
        if r.coast_amplitude <= 0.0 {
            return 0.0;
        }
        let n = fbm2(
            mix32(self.rim_seed ^ 0xC0A5_7000),
            x as i64,
            z as i64,
            r.coast_wavelength,
            r.coast_octaves,
            None,
        );
        r.coast_amplitude * 2.0 * f64::from(n)
    }

    /// Each plate's interior (m): the farthest any of its points inside the land's edge lies from
    /// every other plate (its warped Voronoi bisectors) and from that edge, on a scan of
    /// [`INTERIOR_CELL`] m. A plate with no point inside has 0.
    pub fn interiors(&self) -> Vec<f64> {
        let mut best = vec![0.0f64; self.plates.len()];
        let reach = self.rules.reach;
        let n = (2.0 * reach / INTERIOR_CELL) as i64 + 1;
        for j in 0..n {
            for i in 0..n {
                let (x, z) = (
                    -reach + (i as f64 + 0.5) * INTERIOR_CELL,
                    -reach + (j as f64 + 0.5) * INTERIOR_CELL,
                );
                let edge = self.land_edge(x, z);
                if edge <= 0.0 {
                    continue;
                }
                let (wx, wz) = self.warped(x, z);
                let a = self.plate_at(x, z);
                let pa = &self.plates[a];
                let da = dist2([wx, wz], pa.centre);
                let mut near = edge;
                for (b, pb) in self.plates.iter().enumerate() {
                    if b == a {
                        continue;
                    }
                    let gap = dist2(pa.centre, pb.centre).sqrt().max(1.0);
                    near = near.min((dist2([wx, wz], pb.centre) - da) / (2.0 * gap));
                }
                best[a] = best[a].max(near);
            }
        }
        best
    }

    /// The distance (m) from a face-local point in to the rim's coast (< 0 beyond it): the
    /// outer band's inner edge less the sector's shelf (cosine-blended round the disc), moved by
    /// the rim's coast noise (at most ± its amplitude, so the band stays sea).
    pub fn land_edge(&self, x: f64, z: f64) -> f64 {
        let rim = &self.rules.rim;
        let r = (x * x + z * z).sqrt();
        let k = self.rim_shelf.len().max(1);
        let a = z.atan2(x).rem_euclid(std::f64::consts::TAU) / std::f64::consts::TAU * k as f64;
        // Sector i's shelf holds at its middle (a = i + 0.5); blend to the next.
        let u = a - 0.5;
        let i0 = u.floor();
        let t = u - i0;
        let at = |i: f64| self.rim_shelf.get(i.rem_euclid(k as f64) as usize).copied();
        let shelf = match (at(i0), at(i0 + 1.0)) {
            (Some(p), Some(q)) => {
                let w = 0.5 - 0.5 * (std::f64::consts::PI * t).cos();
                p + (q - p) * w
            }
            _ => rim.shelf[0],
        };
        let noise = if rim.noise_amplitude > 0.0 {
            let n = fbm2(
                self.rim_seed,
                x as i64,
                z as i64,
                rim.noise_wavelength,
                2,
                None,
            );
            (rim.noise_amplitude * 2.0 * f64::from(n))
                .clamp(-rim.noise_amplitude, rim.noise_amplitude)
        } else {
            0.0
        };
        self.rules.reach - rim.edge_sea - shelf + noise - r
    }

    /// No sliver plates (realistic batch A): while some plate's interior is short of its kind's
    /// `interior`, the shortest (by share) is removed (its ground goes to its neighbours) where its
    /// kind keeps its minimum, else moved 30 % nearer the centre (a plate at the rim drawn in).
    fn settle(&mut self) {
        let need = |r: &PlateRules, k: PlateKind| match k {
            PlateKind::Oceanic => (r.oceanic.interior, r.oceanic.min),
            PlateKind::Continental => (r.continental.interior, r.continental.min),
        };
        for _ in 0..4 * self.plates.len() {
            let inner = self.interiors();
            let worst = inner
                .iter()
                .zip(&self.plates)
                .enumerate()
                .map(|(i, (&d, p))| (d / need(&self.rules, p.kind).0.max(1.0), i))
                .filter(|&(share, _)| share < 1.0)
                .min_by(|a, b| a.0.total_cmp(&b.0));
            let Some((_, i)) = worst else { break };
            let kind = self.plates[i].kind;
            let have = self.plates.iter().filter(|p| p.kind == kind).count() as u32;
            if self.plates.len() > 2 && have > need(&self.rules, kind).1 {
                self.plates.remove(i);
            } else {
                let c = &mut self.plates[i].centre;
                *c = [0.7 * c[0], 0.7 * c[1]];
            }
        }
    }

    pub(crate) fn warped(&self, x: f64, z: f64) -> (f64, f64) {
        let a = self.rules.warp_amplitude;
        if a <= 0.0 {
            return (x, z);
        }
        let (xi, zi) = (x as i64, z as i64);
        let w = self.rules.warp_wavelength;
        let (mut wx, mut wz) = (
            x + a * f64::from(fbm2(self.warp_seeds.0, xi, zi, w, 2, None)),
            z + a * f64::from(fbm2(self.warp_seeds.1, xi, zi, w, 2, None)),
        );
        for (k, &(da, dw)) in self.rules.warp_detail.iter().enumerate() {
            let k = k as u32 + 1;
            wx += da * f64::from(fbm2(mix32(self.warp_seeds.0 ^ k), xi, zi, dw, 2, None));
            wz += da * f64::from(fbm2(mix32(self.warp_seeds.1 ^ k), xi, zi, dw, 2, None));
        }
        (wx, wz)
    }

    /// The boundaries: scan cells within the reach, each neighbour pair on different plates adds
    /// the midpoint to that pair's boundary.
    fn scan_boundaries(&self) -> Vec<Boundary> {
        let reach = self.rules.reach;
        let n = (2.0 * reach / SCAN_CELL) as i64 + 1;
        let at = |i: i64| -reach + (i as f64 + 0.5) * SCAN_CELL;
        let inside = |x: f64, z: f64| x * x + z * z <= reach * reach;
        let mut row = vec![usize::MAX; n as usize];
        let mut found: Vec<Boundary> = Vec::new();
        let mut add = |p: usize, q: usize, point: [f64; 2]| {
            let pair = [p.min(q), p.max(q)];
            match found.iter_mut().find(|b| b.plates == pair) {
                Some(b) => b.points.push(point),
                None => found.push(Boundary {
                    plates: pair,
                    meeting: classify(
                        &self.plates[pair[0]],
                        &self.plates[pair[1]],
                        self.rules.transform_below,
                    ),
                    points: vec![point],
                }),
            }
        };
        for j in 0..n {
            let z = at(j);
            let mut left = usize::MAX;
            for i in 0..n {
                let x = at(i);
                let here = if inside(x, z) {
                    self.plate_at(x, z)
                } else {
                    usize::MAX
                };
                if here != usize::MAX {
                    // The crossing itself, found between the two cells' centres (BUGS.md #164: a
                    // point at the cells' midpoint put every near-vertical or near-horizontal
                    // stretch of boundary on the scan's grid lines, km-long axis-aligned edges).
                    if left != usize::MAX && left != here {
                        add(left, here, self.crossing([x - SCAN_CELL, z], [x, z], left));
                    }
                    let up = row[i as usize];
                    if up != usize::MAX && up != here {
                        add(up, here, self.crossing([x, z - SCAN_CELL], [x, z], up));
                    }
                }
                left = here;
                row[i as usize] = here;
            }
        }
        found.sort_by_key(|b| b.plates);
        found
    }

    /// Where the plate under the segment `a`–`b` changes from `from` (at `a`) to another (at `b`):
    /// [`CROSSING_STEPS`] halvings of the segment.
    fn crossing(&self, mut a: [f64; 2], mut b: [f64; 2], from: usize) -> [f64; 2] {
        for _ in 0..CROSSING_STEPS {
            let m = [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1])];
            if self.plate_at(m[0], m[1]) == from {
                a = m;
            } else {
                b = m;
            }
        }
        [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1])]
    }

    fn draw_hotspots(&self, seed: u64) -> Vec<Hotspot> {
        let r = &self.rules;
        let mut d = Draws::new(seed, 3);
        let n = d.count(r.hotspots_min, r.hotspots_max);
        (0..n)
            .map(|_| {
                let at = d.in_disc(r.hotspots_within);
                let plate = self.plate_at(at[0], at[1]);
                let [vx, vz] = self.plates[plate].motion;
                let s = (vx * vx + vz * vz).sqrt();
                let trail = if s > 0.0 {
                    [vx / s, vz / s]
                } else {
                    [0.0, 0.0]
                };
                Hotspot { at, plate, trail }
            })
            .collect()
    }
}

/// The plates `seed` draws: the count, spaced centres, the kinds by weight with each kind's
/// minimum kept, the motions and ages.
fn draw_plates(r: &PlateRules, seed: u64) -> Vec<Plate> {
    let mut d = Draws::new(seed, 1);
    let n = d.count(r.count_min, r.count_max) as usize;
    let mut centres: Vec<[f64; 2]> = Vec::with_capacity(n);
    for _ in 0..n {
        // The first point at least `spacing` from every centre, else the farthest one tried.
        let mut best = ([0.0, 0.0], f64::NEG_INFINITY);
        for _ in 0..r.attempts.max(1) {
            let p = d.in_disc(r.centres);
            let near = centres
                .iter()
                .map(|&c| dist2(p, c))
                .fold(f64::INFINITY, f64::min);
            if near > best.1 {
                best = (p, near);
            }
            if near >= r.spacing * r.spacing {
                break;
            }
        }
        centres.push(best.0);
    }
    let total = r.oceanic.weight + r.continental.weight;
    let mut kinds: Vec<PlateKind> = (0..n)
        .map(|_| {
            if d.next() * total < r.oceanic.weight {
                PlateKind::Oceanic
            } else {
                PlateKind::Continental
            }
        })
        .collect();
    for (kind, rule, other_rule) in [
        (PlateKind::Oceanic, r.oceanic, r.continental),
        (PlateKind::Continental, r.continental, r.oceanic),
    ] {
        // Turn plates of the other kind, last first, while it keeps its own minimum.
        for i in (0..n).rev() {
            let have = kinds.iter().filter(|&&k| k == kind).count() as u32;
            let others = n as u32 - have;
            if have >= rule.min || others <= other_rule.min {
                break;
            }
            if kinds[i] != kind {
                kinds[i] = kind;
            }
        }
    }
    let mut d = Draws::new(seed, 2);
    centres
        .into_iter()
        .zip(kinds)
        .map(|(centre, kind)| {
            let [dx, dz] = d.direction();
            let speed = d.between(r.speed);
            Plate {
                kind,
                centre,
                motion: [dx * speed, dz * speed],
                age: d.between(r.age),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEEDS: [u64; 6] = [0, 1, 2, 42, 0xDEAD_BEEF, u64::MAX];

    #[test]
    fn every_fixture_gives_the_boundary_its_data_names() {
        assert!(!FIXTURES.is_empty());
        for f in FIXTURES {
            let [a, b] = f.plates.map(Plate::from);
            let m = classify(&a, &b, PLATES.transform_below);
            assert_eq!(m.kind, f.kind, "{}: {m:?}", f.name);
            assert_eq!(m.subducts, f.subducts, "{}: {m:?}", f.name);
            // The same two plates laid out and scanned give one boundary of that kind.
            let layout = PlateLayout::of_plates(&PLATES, vec![a, b]);
            assert_eq!(layout.boundaries.len(), 1, "{}", f.name);
            let boundary = &layout.boundaries[0];
            assert_eq!(boundary.meeting.kind, f.kind, "{}", f.name);
            assert_eq!(boundary.subducting_plate(), f.subducts, "{}", f.name);
            // Its points lie halfway between the centres (no warp: the perpendicular bisector).
            for p in &boundary.points {
                let (da, db) = (dist2(*p, a.centre).sqrt(), dist2(*p, b.centre).sqrt());
                assert!((da - db).abs() <= SCAN_CELL, "{}: {p:?}", f.name);
            }
        }
    }

    #[test]
    fn a_fixture_added_as_data_is_classified_by_its_motion() {
        // Swapping a fixture's plates swaps the subducting side and keeps the kind.
        for f in FIXTURES {
            let [a, b] = f.plates.map(Plate::from);
            let m = classify(&b, &a, PLATES.transform_below);
            assert_eq!(m.kind, f.kind, "{}", f.name);
            assert_eq!(m.subducts, f.subducts.map(|i| 1 - i), "{}", f.name);
        }
    }

    #[test]
    fn boundary_kinds_follow_the_closing_rate() {
        let plate = |kind, x: f64, vx: f64, vz: f64| Plate {
            kind,
            centre: [x, 0.0],
            motion: [vx, vz],
            age: 50.0,
        };
        let o = PlateKind::Oceanic;
        let t = 0.4;
        let m = classify(&plate(o, -1.0, 2.0, 0.0), &plate(o, 1.0, -2.0, 0.0), t);
        assert_eq!((m.kind, m.closing), (BoundaryKind::Convergent, 4.0));
        let m = classify(&plate(o, -1.0, -2.0, 0.0), &plate(o, 1.0, 2.0, 0.0), t);
        assert_eq!((m.kind, m.closing), (BoundaryKind::Divergent, -4.0));
        let m = classify(&plate(o, -1.0, 0.0, 3.0), &plate(o, 1.0, 0.0, -3.0), t);
        assert_eq!((m.kind, m.slip), (BoundaryKind::Transform, 6.0));
        // Equal ages: the first dives (a fixed tie-break).
        assert_eq!(
            classify(&plate(o, -1.0, 2.0, 0.0), &plate(o, 1.0, 0.0, 0.0), t).subducts,
            Some(0)
        );
    }

    dwell_world::both_faces!(
        /// A face's layout is a function of the world seed: drawn twice it is the same, its counts
        /// lie in the data's ranges, every boundary is classified by its plates' motion, and the
        /// land/sea mask follows the plate kinds inside the sea margin.
        fn a_faces_layout_is_deterministic_and_within_the_data(f: FaceFrame) {
            for seed in SEEDS {
                let l = PlateLayout::for_face(seed, f.face);
                assert_eq!(l, PlateLayout::for_face(seed, f.face), "seed {seed}");
                // The draw's count less the slivers removed.
                let n = l.plates.len() as u32;
                assert!((2..=PLATES.count_max).contains(&n), "{n}");
                let h = l.hotspots.len() as u32;
                assert!(
                    (PLATES.hotspots_min..=PLATES.hotspots_max).contains(&h),
                    "{h}"
                );
                for kind in [PlateKind::Oceanic, PlateKind::Continental] {
                    let rule = match kind {
                        PlateKind::Oceanic => PLATES.oceanic,
                        PlateKind::Continental => PLATES.continental,
                    };
                    let have = l.plates.iter().filter(|p| p.kind == kind).count() as u32;
                    assert!(have >= rule.min, "seed {seed}: {have} {kind:?}");
                }
                for p in &l.plates {
                    let s = (p.motion[0] * p.motion[0] + p.motion[1] * p.motion[1]).sqrt();
                    assert!((PLATES.speed[0] - 1e-9..=PLATES.speed[1] + 1e-9).contains(&s));
                    assert!((PLATES.age[0]..=PLATES.age[1]).contains(&p.age));
                    assert!(dist2(p.centre, [0.0, 0.0]) <= PLATES.centres * PLATES.centres);
                }
                assert!(!l.boundaries.is_empty(), "seed {seed}: plates meet");
                for b in &l.boundaries {
                    let [p, q] = b.plates.map(|i| l.plates[i]);
                    assert_eq!(b.meeting, classify(&p, &q, PLATES.transform_below));
                    assert!(!b.points.is_empty());
                }
                for hs in &l.hotspots {
                    assert_eq!(hs.plate, l.plate_at(hs.at[0], hs.at[1]));
                }
                let band = PLATES.reach - PLATES.rim.edge_sea;
                for (x, z) in [(0.0, 0.0), (5000.0, -3000.0), (-12000.0, 9000.0)] {
                    // The plate's kind, but for the coast's noise near its edge.
                    let kind = l.plates[l.plate_at(x, z)].kind;
                    let d = l.coast_distance(x, z);
                    assert_eq!(l.is_land(x, z), d > 0.0, "{x} {z}");
                    if d.abs() > 2.0 * PLATES.rim.coast_amplitude && l.land_edge(x, z) > 0.0 {
                        assert_eq!(l.is_land(x, z), kind == PlateKind::Continental, "{x} {z}");
                    }
                }
                for a in 0..72 {
                    let th = f64::from(a) * std::f64::consts::TAU / 72.0;
                    let (x, z) = (band * th.cos(), band * th.sin());
                    assert!(!l.is_land(x, z), "the outer band is sea");
                    assert!(l.coast_distance(x * 1.01, z * 1.01) <= -PLATES.rim.edge_coast);
                }
                assert!(!l.is_land(0.0, -PLATES.reach - 1000.0));
                // No sliver plates: each plate has its kind's interior.
                for (d, p) in l.interiors().iter().zip(&l.plates) {
                    let need = match p.kind {
                        PlateKind::Oceanic => PLATES.oceanic.interior,
                        PlateKind::Continental => PLATES.continental.interior,
                    };
                    assert!(*d >= need, "seed {seed}: {p:?} interior {d:.0}");
                }
            }
        }
    );

    #[test]
    fn seeds_and_faces_draw_different_layouts() {
        // both-faces: compares face A's layout with face B's for the same world seed.
        let a: Vec<_> = SEEDS
            .iter()
            .map(|&s| PlateLayout::for_face(s, Face::A))
            .collect();
        for i in 0..a.len() {
            for j in i + 1..a.len() {
                assert_ne!(a[i].plates, a[j].plates, "seeds {} {}", SEEDS[i], SEEDS[j]);
            }
        }
        for (s, la) in SEEDS.iter().zip(&a) {
            assert_ne!(
                la.plates,
                PlateLayout::for_face(*s, Face::B).plates,
                "seed {s}"
            );
        }
    }

    #[test]
    fn layouts_hold_both_land_and_sea_by_the_kinds_minimums() {
        let land = (0..20)
            .filter(|&s| {
                let l = PlateLayout::generate(&PLATES, s);
                l.plates.iter().any(|p| p.kind == PlateKind::Oceanic)
                    && l.plates.iter().any(|p| p.kind == PlateKind::Continental)
            })
            .count();
        assert_eq!(land, 20);
    }
}
