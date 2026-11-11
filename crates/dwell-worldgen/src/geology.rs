//! Geology (systemic terrain T2, ADR 0075; data: `shared/worldgen/rocks.json`): rock types, the
//! strata stacks they form, the rock provinces that place a stack per region, and the rule-based
//! shaping of the ground by its rock.
//!
//! A stack is a profile over one period of height (its layers' thicknesses, repeating from sea
//! level): a monotone, piecewise-linear map from the ground's height to the shaped height. Each
//! bed of a hard rock is a near-vertical wall (little input height for its whole thickness) and a
//! tread over it (input height mapped to one output: a flat cap); a soft rock's bed rises over more
//! input height (a talus slope); vertical jointing makes one sheer wall the unit's height; a
//! massive rock rises unstepped. The shaped ground is the profile's `strength` of the way from the
//! height field to the profile's, so strength 0 (the default world's) changes nothing, and the
//! change is bounded by one period. Face-local throughout: it maps a column's height above the sea.

/// A rock's jointing (a fixed vocabulary).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Jointing {
    /// Breaks along its beds: a wall and a tread every `bed` m.
    Bedded,
    /// Columnar: one sheer wall the unit's height, a tread on top.
    Vertical,
    /// No joints: rises unstepped.
    Massive,
}

/// A rock type (rocks.json `rocks`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rock {
    pub name: &'static str,
    /// 0..1: hard rock stands as walls and caps, soft rock wastes to talus.
    pub hardness: f64,
    pub jointing: Jointing,
    /// A bed's thickness (m): a bedded rock steps this often.
    pub bed: f64,
    /// 0..1, unused yet (karst later).
    pub solubility: f64,
    /// The block its walls show.
    pub face: &'static str,
    /// The block its treads show.
    pub cap: &'static str,
    /// The block its talus shows.
    pub scree: &'static str,
    /// Its colour (0xrrggbb), Chisel's rock overlay's.
    pub color: u32,
}

/// One layer of a stack: a rock and its thickness (m).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layer {
    pub rock: &'static str,
    pub thickness: f64,
}

/// A strata stack (rocks.json `stacks`): its layers top to bottom, its weight in the provinces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stack {
    pub name: &'static str,
    pub weight: f64,
    pub layers: &'static [Layer],
}

/// The geology (rocks.json): the rocks, the stacks, the provinces' scale and the shaping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geology {
    pub rocks: &'static [Rock],
    pub stacks: &'static [Stack],
    /// The provinces' noise wavelength (m).
    pub province_wavelength: i32,
    /// 0..1 of the way from the height field to the profile's (0: unchanged).
    pub strength: f64,
    /// How wide a hard bed's tread is (input height per m of bed, by hardness²).
    pub tread: f64,
    /// How narrow a hard wall is: a bed's rise takes (1 − hardness)^sheer of its height as input.
    pub sheer: f64,
    /// The rule-based erosion (T4).
    pub erosion: Erosion,
    /// The tectonic provinces' stacks (T5, `rocks.json` `tectonic`): a volcano's, the old core's.
    pub volcanic_stack: &'static str,
    pub core_stack: &'static str,
    /// The snow line's cover (`rocks.json` `tectonic.snow`; `terrain.json` `tectonics.snowBelowC`).
    pub snow_block: &'static str,
    /// The glacier's cover (`rocks.json` `tectonic.ice`; `terrain.json` `tectonics.iceBelowC`).
    pub ice_block: &'static str,
}

mod table {
    include!("rocks.gen.rs");
}
pub use table::{GEOLOGY, ROCKS, STACKS};

impl Geology {
    /// The rock named `name`.
    pub fn rock(&self, name: &str) -> Option<usize> {
        self.rocks.iter().position(|r| r.name == name)
    }

    /// The stack named `name`.
    pub fn stack(&self, name: &str) -> Option<usize> {
        self.stacks.iter().position(|s| s.name == name)
    }
}

/// What part of a profile a height falls on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// A hard bed's rise: a wall, its rock's face.
    Wall,
    /// A soft bed's rise: talus, its rock's scree.
    Talus,
    /// Over a bed: a flat cap, its rock's cap.
    Tread,
    /// A massive rock's unstepped rise.
    Plain,
}

/// One piece of a profile: input heights `u0..u1` of the period map to `v0..v1`.
#[derive(Clone, Copy, Debug)]
struct Piece {
    u0: f64,
    u1: f64,
    v0: f64,
    v1: f64,
    rock: u16,
    part: Part,
}

/// A stack's profile over one period of height.
#[derive(Clone, Debug)]
pub struct Profile {
    period: f64,
    pieces: Vec<Piece>,
}

impl Profile {
    /// The profile of `stack` (layers top to bottom; panics on a rock `g` lacks: the data's
    /// validation checks the names).
    pub fn of(g: &Geology, stack: &Stack) -> Self {
        // (input, output, rock, part), bottom first.
        let mut raw: Vec<(f64, f64, u16, Part)> = Vec::new();
        for l in stack.layers.iter().rev() {
            let ri = g
                .rock(l.rock)
                .unwrap_or_else(|| panic!("stack {}: no rock {}", stack.name, l.rock));
            let r = &g.rocks[ri];
            let (k, d) = (r.hardness.clamp(0.0, 1.0), l.thickness);
            let rise = |dd: f64, steep: f64| dd * steep * (1.0 - k).powf(g.sheer);
            let part = if k >= 0.5 { Part::Wall } else { Part::Talus };
            match r.jointing {
                Jointing::Massive => raw.push((d, d, ri as u16, Part::Plain)),
                Jointing::Bedded | Jointing::Vertical => {
                    let (n, steep) = if r.jointing == Jointing::Bedded {
                        ((d / r.bed).round().max(1.0), 1.0)
                    } else {
                        (1.0, 0.25)
                    };
                    let dd = d / n;
                    for _ in 0..n as u32 {
                        raw.push((rise(dd, steep), dd, ri as u16, part));
                        raw.push((dd * k * k * g.tread, 0.0, ri as u16, Part::Tread));
                    }
                }
            }
        }
        let period: f64 = raw.iter().map(|p| p.1).sum();
        let input: f64 = raw.iter().map(|p| p.0).sum();
        let scale = if input > 0.0 { period / input } else { 1.0 };
        let (mut u, mut v) = (0.0, 0.0);
        let mut pieces = Vec::new();
        for (i, o, rock, part) in raw {
            let p = Piece {
                u0: u,
                u1: u + i * scale,
                v0: v,
                v1: v + o,
                rock,
                part,
            };
            u = p.u1;
            v = p.v1;
            pieces.push(p);
        }
        Self { period, pieces }
    }

    /// The period (m): the stack's thickness.
    pub fn period(&self) -> f64 {
        self.period
    }

    /// The shaped height of ground at `h` m over the sea, and the rock and part it falls on.
    pub fn map(&self, h: f64) -> (f64, usize, Part) {
        let u = h.rem_euclid(self.period);
        let base = h - u;
        let p = self
            .pieces
            .iter()
            .find(|p| u < p.u1 && p.u1 > p.u0)
            .or(self.pieces.last())
            .expect("a profile has pieces");
        let t = if p.u1 > p.u0 {
            ((u - p.u0) / (p.u1 - p.u0)).clamp(0.0, 1.0)
        } else {
            1.0
        };
        (base + p.v0 + (p.v1 - p.v0) * t, usize::from(p.rock), p.part)
    }
}

/// A geology resolved for a generator: each stack's profile and the provinces' weights.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub geology: Geology,
    pub profiles: Vec<Profile>,
    /// The stacks' cumulative weights over 0..1 (the provinces' thresholds).
    cumulative: Vec<f64>,
    seed: u32,
    /// The tectonics (T5) and their volcanoes, when on: the stacks they place, by index.
    tectonic: Option<Tectonic>,
}

/// The tectonics, their volcanoes, and the volcanic and core stacks (indices).
type Tectonic = (
    crate::tectonics::Tectonics,
    Vec<crate::tectonics::Volcano>,
    usize,
    usize,
);

/// How near (in the provinces' noise) to a border between stacks the shaping fades out, so a
/// province's edge is no cliff.
const PROVINCE_FADE: f64 = 0.03;

impl Resolved {
    pub fn new(geology: Geology, world_seed: u64) -> Self {
        let profiles = geology
            .stacks
            .iter()
            .map(|s| Profile::of(&geology, s))
            .collect();
        let total: f64 = geology.stacks.iter().map(|s| s.weight).sum();
        let mut c = 0.0;
        let cumulative = geology
            .stacks
            .iter()
            .map(|s| {
                c += s.weight / total.max(1e-12);
                c
            })
            .collect();
        Self {
            geology,
            profiles,
            cumulative,
            seed: crate::noise::seed_word(world_seed, 0x6E01_0001),
            tectonic: None,
        }
    }

    /// With the tectonic provinces of `t` (T5): a volcano's ground is the volcanic stack, the old
    /// core's the core stack. Off (`t` disabled): the noise's provinces alone.
    pub fn with_tectonics(mut self, t: crate::tectonics::Tectonics) -> Self {
        if t.enabled() {
            let find = |n: &str| {
                self.geology
                    .stack(n)
                    .unwrap_or_else(|| panic!("rocks.json tectonic: no stack {n}"))
            };
            let (v, c) = (
                find(self.geology.volcanic_stack),
                find(self.geology.core_stack),
            );
            self.tectonic = Some((t, t.volcanoes(), v, c));
        }
        self
    }

    /// The province's stack at (x, z) and how fully it shapes there (fading to 0 at a border).
    pub fn province(&self, x: i64, z: i64) -> (usize, f64) {
        if let Some((t, v, volcanic, core)) = &self.tectonic
            && let Some(on_volcano) = t.province(v, x as f64, z as f64)
        {
            return (if on_volcano { *volcanic } else { *core }, 1.0);
        }
        let n = crate::noise::fbm2(self.seed, x, z, self.geology.province_wavelength, 2, None);
        // fbm2 is about ±0.5: spread to 0..1.
        let t = (0.5 + 1.6 * f64::from(n)).clamp(0.0, 1.0);
        let i = self
            .cumulative
            .iter()
            .position(|&c| t < c)
            .unwrap_or(self.cumulative.len() - 1);
        let lo = if i == 0 { -1.0 } else { self.cumulative[i - 1] };
        let hi = if i + 1 == self.cumulative.len() {
            2.0
        } else {
            self.cumulative[i]
        };
        let edge = (t - lo).min(hi - t);
        (i, (edge / PROVINCE_FADE).clamp(0.0, 1.0))
    }
}

/// The rule-based erosion (systemic terrain T4, ADR 0075; rocks.json `erosion`): how far the
/// scarps have retreated from the world map's drainage lines. A biome opts in with its `erosion`
/// (and its `gullies`), absent 0, so the default world is unchanged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Erosion {
    /// Scales every biome's `erosion` (0: none).
    pub strength: f64,
    /// How far (m) a drainage line's scarps have retreated, before the rock and the hill.
    pub reach: f64,
    /// The extra retreat per doubling of the line's flow accumulation over the threshold (a bigger
    /// valley cuts back further).
    pub valley: f64,
    /// The retreat × (1 − resist × the stack's cap rock's hardness): soft rock retreats faster.
    pub resist: f64,
    /// The retreat lost per m of the hill's height over the floor (taller ground survives).
    pub relief: f64,
    /// The flow accumulation (map cells) that makes a drainage line: lower, denser drainage.
    pub threshold: f64,
    /// 0..1 of the way from the drainage line's height down to the top of the stack's lowest soft
    /// layer (the base level).
    pub snap: f64,
    /// The wall's width (m) at a remnant's edge.
    pub wall: f64,
    /// The talus skirt's width (m) below a wall.
    pub talus_width: f64,
    /// The share of the hill's height the talus skirt rises to.
    pub talus_share: f64,
    /// The outline noise's amplitude (a share of the retreat).
    pub outline: f64,
    /// The outline noise's wavelength (m).
    pub outline_wavelength: f64,
    /// The badlands' rills (a biome's `gullies`): their noise's wavelength (m).
    pub gully_wavelength: f64,
    /// The rills' depth (m).
    pub gully_depth: f64,
}

/// The world map's drainage seen from above (T4): per map cell, the distance (m) to the nearest
/// drainage line (a cell with at least `threshold` cells of flow accumulation, or the sea), that
/// line's height and accumulation. Built once per generator, only where a biome erodes.
#[derive(Clone, Debug)]
pub struct DrainageField {
    n: usize,
    cell: f64,
    x0: f64,
    dist: Vec<f32>,
    height: Vec<f32>,
    accumulation: Vec<f32>,
    /// Each cell's nearest drainage cell (its index; `u32::MAX` none): the fronts' sources.
    source: Vec<u32>,
}

impl DrainageField {
    /// The field of `map` with drainage lines at `threshold` cells of accumulation.
    pub fn of(map: &crate::worldmap::WorldMap, threshold: f64) -> Self {
        let n = map.n;
        let len = n * n;
        // Flow accumulation: each cell passes its count to its receiver, highest first.
        let mut order: Vec<usize> = (0..len).collect();
        order.sort_by(|&a, &b| map.height[b].total_cmp(&map.height[a]).then(a.cmp(&b)));
        let mut acc = vec![1.0f32; len];
        for &c in &order {
            if let Some((dx, dz)) = map.flow(c) {
                let (i, j) = (
                    (c % n) as i64 + i64::from(dx),
                    (c / n) as i64 + i64::from(dz),
                );
                if i >= 0 && j >= 0 && (i as usize) < n && (j as usize) < n {
                    let r = j as usize * n + i as usize;
                    if r != c {
                        acc[r] += acc[c];
                    }
                }
            }
        }
        // Distance to the nearest drainage cell: a Dijkstra over the grid's 8 neighbours.
        let mut dist = vec![f32::INFINITY; len];
        let mut src = vec![usize::MAX; len];
        let mut heap = std::collections::BinaryHeap::new();
        for c in 0..len {
            if map.is_sea(c) || f64::from(acc[c]) >= threshold {
                dist[c] = 0.0;
                src[c] = c;
                heap.push(std::cmp::Reverse((0u32, c)));
            }
        }
        let s2 = std::f32::consts::SQRT_2;
        let moves = [
            (1i64, 0i64, 1.0f32),
            (-1, 0, 1.0),
            (0, 1, 1.0),
            (0, -1, 1.0),
            (1, 1, s2),
            (1, -1, s2),
            (-1, 1, s2),
            (-1, -1, s2),
        ];
        // Non-negative f32s order as their bits.
        while let Some(std::cmp::Reverse((bits, c))) = heap.pop() {
            let d = f32::from_bits(bits);
            if d > dist[c] {
                continue;
            }
            let (ci, cj) = ((c % n) as i64, (c / n) as i64);
            for (dx, dz, w) in moves {
                let (i, j) = (ci + dx, cj + dz);
                if i < 0 || j < 0 || i as usize >= n || j as usize >= n {
                    continue;
                }
                let r = j as usize * n + i as usize;
                let nd = d + w * map.cell as f32;
                if nd < dist[r] {
                    dist[r] = nd;
                    src[r] = src[c];
                    heap.push(std::cmp::Reverse((nd.to_bits(), r)));
                }
            }
        }
        let height = src
            .iter()
            .map(|&s| {
                if s == usize::MAX {
                    0.0
                } else {
                    map.height[s].max(0.0)
                }
            })
            .collect();
        let accumulation = src
            .iter()
            .map(|&s| if s == usize::MAX { 1.0 } else { acc[s] })
            .collect();
        let dist = dist.into_iter().map(|d| d.min(1.0e9)).collect();
        let source = src
            .iter()
            .map(|&s| if s == usize::MAX { u32::MAX } else { s as u32 })
            .collect();
        Self {
            n,
            cell: map.cell,
            x0: map.centre(0).0,
            dist,
            height,
            accumulation,
            source,
        }
    }

    /// At face-local (x, z), bilinear between the cells' centres: the distance (m) to the nearest
    /// drainage line, that line's height (m over the sea) and its accumulation (cells).
    pub fn at(&self, x: f64, z: f64) -> (f64, f64, f64) {
        let n = self.n as i64;
        let fx = ((x - self.x0) / self.cell).clamp(0.0, (n - 1) as f64);
        let fz = ((z - self.x0) / self.cell).clamp(0.0, (n - 1) as f64);
        let (i, j) = (fx.floor() as i64, fz.floor() as i64);
        let (tx, tz) = (fx - i as f64, fz - j as f64);
        let mut out = [0.0f64; 3];
        for (dj, wz) in [(0, 1.0 - tz), (1, tz)] {
            for (di, wx) in [(0, 1.0 - tx), (1, tx)] {
                let c = ((j + dj).min(n - 1) * n + (i + di).min(n - 1)) as usize;
                let w = wx * wz;
                out[0] += w * f64::from(self.dist[c]);
                out[1] += w * f64::from(self.height[c]);
                out[2] += w * f64::from(self.accumulation[c]);
            }
        }
        (out[0], out[1], out[2])
    }
}

impl DrainageField {
    /// The distances (m) at face-local (x, z) to the nearest drainage cell and to the nearest one
    /// on the other side (seen from here at more than a right angle from the first), from the
    /// sources of the 4 × 4 cells around it; None with no line on the other side. Two cut-back
    /// fronts meet on the crest between them, where the two are equal (the fins, 0.52.66).
    pub fn fronts(&self, x: f64, z: f64) -> Option<(f64, f64)> {
        let n = self.n as i64;
        let fx = ((x - self.x0) / self.cell).floor() as i64;
        let fz = ((z - self.x0) / self.cell).floor() as i64;
        let mut near: [(f64, f64, f64); 16] = [(f64::INFINITY, 0.0, 0.0); 16];
        let mut k = 0;
        for j in fz - 1..=fz + 2 {
            for i in fx - 1..=fx + 2 {
                let (i, j) = (i.clamp(0, n - 1), j.clamp(0, n - 1));
                let s = self.source[(j * n + i) as usize];
                if s != u32::MAX {
                    let (si, sj) = (f64::from(s % self.n as u32), f64::from(s / self.n as u32));
                    let (dx, dz) = (self.x0 + si * self.cell - x, self.x0 + sj * self.cell - z);
                    near[k] = ((dx * dx + dz * dz).sqrt(), dx, dz);
                    k += 1;
                }
            }
        }
        let near = &near[..k];
        let first = near.iter().min_by(|a, b| a.0.total_cmp(&b.0))?;
        let second = near
            .iter()
            .filter(|s| s.1 * first.1 + s.2 * first.2 < 0.0)
            .min_by(|a, b| a.0.total_cmp(&b.0))?;
        Some((first.0, second.0))
    }
}

/// Where a column falls on an eroded profile (T4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cut {
    /// Cut down to the floor.
    Floor,
    /// The talus skirt below a wall.
    Talus,
    /// A remnant's wall.
    Wall,
    /// Standing: a remnant's top.
    Standing,
}

/// The eroded height of ground at `h` m given the erosion's `margin` (m past the retreated
/// scarp: below 0 cut away) and its `floor`: the floor with a talus skirt, a wall `wall` m wide,
/// then the ground as it was. A pure rule over its inputs (face-local heights).
pub fn eroded(e: &Erosion, h: f64, floor: f64, margin: f64) -> (f64, Cut) {
    let floor = floor.min(h);
    let hill = h - floor;
    let foot = floor + e.talus_share * hill;
    if margin >= e.wall {
        (h, Cut::Standing)
    } else if margin >= 0.0 {
        (foot + (h - foot) * (margin / e.wall.max(1e-6)), Cut::Wall)
    } else if e.talus_width > 0.0 && margin > -e.talus_width {
        let t = 1.0 + margin / e.talus_width;
        (floor + (foot - floor) * t * t, Cut::Talus)
    } else {
        (floor, Cut::Floor)
    }
}

/// The top (m within a period) of `stack`'s lowest soft layer (hardness under ½), or 0.
pub fn soft_top(g: &Geology, stack: &Stack) -> f64 {
    let mut below = 0.0;
    for l in stack.layers.iter().rev() {
        below += l.thickness;
        if g.rock(l.rock).is_some_and(|r| g.rocks[r].hardness < 0.5) {
            return below;
        }
    }
    0.0
}
