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
}

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
        }
    }

    /// The province's stack at (x, z) and how fully it shapes there (fading to 0 at a border).
    pub fn province(&self, x: i64, z: i64) -> (usize, f64) {
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
