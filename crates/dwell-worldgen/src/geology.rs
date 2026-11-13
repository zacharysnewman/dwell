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
    /// The rock provinces from plate history (`rocks.json` `plateProvinces`, R5).
    pub plate_provinces: PlateProvinces,
}

/// A column's setting on its face's plate layout (R5; the code's vocabulary, `plateProvinces`
/// `rules[].setting`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlateSetting {
    /// On an arc's cone.
    Arc,
    /// On a hotspot chain's island.
    Hotspot,
    /// Old seafloor scraped onto the overriding plate at a subducting boundary.
    Accreted,
    /// A collision's core, or a subducting boundary's range crest.
    Core,
    /// A divergent boundary's floor.
    Rift,
    /// A plate's quiet interior, far from every boundary.
    Basin,
    /// Anywhere.
    Default,
}

impl PlateSetting {
    /// Every setting, in the bit order of [`PlateSettings`].
    pub const ALL: [Self; 7] = [
        Self::Arc,
        Self::Hotspot,
        Self::Accreted,
        Self::Core,
        Self::Rift,
        Self::Basin,
        Self::Default,
    ];

    /// The setting named `name` (the data's spelling).
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }

    /// Its name in the data.
    pub fn name(self) -> &'static str {
        match self {
            Self::Arc => "arc",
            Self::Hotspot => "hotspot",
            Self::Accreted => "accreted",
            Self::Core => "core",
            Self::Rift => "rift",
            Self::Basin => "basin",
            Self::Default => "default",
        }
    }

    fn bit(self) -> u8 {
        1 << Self::ALL.iter().position(|&s| s == self).unwrap_or(0)
    }
}

/// The settings that hold at a column (a set; `Default` always).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PlateSettings(u8);

impl PlateSettings {
    pub fn has(self, s: PlateSetting) -> bool {
        self.0 & s.bit() != 0
    }

    fn add(&mut self, s: PlateSetting) {
        self.0 |= s.bit();
    }
}

/// A `plateProvinces` rule: a setting and the stack it takes (by name).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlateRule {
    pub setting: PlateSetting,
    pub stack: &'static str,
}

/// The rock provinces from plate history (`rocks.json` `plateProvinces`, R5): the settings'
/// widths (m) and the rules, the first rule whose setting holds winning.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlateProvinces {
    pub accreted_width: f64,
    pub core_width: f64,
    /// [near, far] m behind a subducting boundary on the overriding side: the range's core.
    pub range_core: [f64; 2],
    pub rift_width: f64,
    pub basin_distance: f64,
    /// How far (m) inside a setting's edge the shaping fades in (0: a sharp edge), so a rule's
    /// province meets the next with no wall where the shaping is on (WORK_QUEUE item 15).
    pub edge_fade: f64,
    pub rules: &'static [PlateRule],
}

impl PlateProvinces {
    /// The settings at (x, z) on the plate tectonics `t` (face-local m).
    pub fn settings(
        &self,
        t: &crate::plate_tectonics::PlateTectonics,
        x: f64,
        z: f64,
    ) -> PlateSettings {
        self.settings_and_edge(t, x, z).0
    }

    /// [`PlateProvinces::settings`] with how far (m) (x, z) lies from the nearest edge where a
    /// setting starts or stops holding (a cone's rim, a width's line, the plate's own boundary).
    pub fn settings_and_edge(
        &self,
        t: &crate::plate_tectonics::PlateTectonics,
        x: f64,
        z: f64,
    ) -> (PlateSettings, f64) {
        use crate::plates::BoundaryKind;
        let mut s = PlateSettings::default();
        let mut edge = f64::INFINITY;
        s.add(PlateSetting::Default);
        for v in &t.volcanoes {
            let r = ((x - v.x).powi(2) + (z - v.z).powi(2)).sqrt();
            edge = edge.min((r - 0.85 * v.radius).abs());
            if r < 0.85 * v.radius {
                s.add(if v.chain == 0 {
                    PlateSetting::Arc
                } else {
                    PlateSetting::Hotspot
                });
            }
        }
        let plate = t.layout.plate_at(x, z);
        let mut nearest = f64::INFINITY;
        for (line, d) in t.lines_of(plate, x, z) {
            nearest = nearest.min(d);
            edge = edge.min(d);
            let mut near = |w: f64| edge = edge.min((d - w).abs());
            match line.kind {
                BoundaryKind::Convergent => match line.over {
                    Some(over) if over == plate => {
                        near(self.accreted_width);
                        near(self.range_core[0]);
                        near(self.range_core[1]);
                        if d < self.accreted_width {
                            s.add(PlateSetting::Accreted);
                        }
                        if (self.range_core[0]..=self.range_core[1]).contains(&d) {
                            s.add(PlateSetting::Core);
                        }
                    }
                    Some(_) => {}
                    None => {
                        near(self.core_width);
                        if d < self.core_width {
                            s.add(PlateSetting::Core);
                        }
                    }
                },
                BoundaryKind::Divergent => {
                    near(self.rift_width);
                    if d < self.rift_width {
                        s.add(PlateSetting::Rift);
                    }
                }
                _ => {}
            }
        }
        edge = edge.min((nearest - self.basin_distance).abs());
        if nearest > self.basin_distance {
            s.add(PlateSetting::Basin);
        }
        (s, edge)
    }

    /// The first rule whose setting holds in `s`, if any.
    pub fn rule(&self, s: PlateSettings) -> Option<&PlateRule> {
        self.rules.iter().find(|r| s.has(r.setting))
    }
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
    /// The noise's stacks (weight above 0), by index; `cumulative` is over these.
    noise_stacks: Vec<usize>,
    /// The face's plate tectonics and each `plateProvinces` rule's stack (index), in a world
    /// whose tectonics come from plates (R5).
    plates: Option<(
        std::sync::Arc<crate::plate_tectonics::PlateTectonics>,
        Vec<usize>,
    )>,
    world_seed: u64,
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
        // A stack of weight 0 is never the noise's (only a rule names it, R5).
        let noise_stacks: Vec<usize> = (0..geology.stacks.len())
            .filter(|&i| geology.stacks[i].weight > 0.0)
            .collect();
        let total: f64 = noise_stacks.iter().map(|&i| geology.stacks[i].weight).sum();
        let mut c = 0.0;
        let cumulative = noise_stacks
            .iter()
            .map(|&i| {
                c += geology.stacks[i].weight / total.max(1e-12);
                c
            })
            .collect();
        Self {
            geology,
            profiles,
            cumulative,
            seed: crate::noise::seed_word(world_seed, 0x6E01_0001),
            tectonic: None,
            noise_stacks,
            plates: None,
            world_seed,
        }
    }

    /// With the rock provinces from the plate tectonics `t` (R5, `rocks.json` `plateProvinces`):
    /// a column's setting on `t` picks its stack by the first rule that holds, else the noise.
    pub fn with_plates(
        mut self,
        t: std::sync::Arc<crate::plate_tectonics::PlateTectonics>,
    ) -> Self {
        let stacks = self
            .geology
            .plate_provinces
            .rules
            .iter()
            .map(|r| {
                self.geology.stack(r.stack).unwrap_or_else(|| {
                    panic!(
                        "rocks.json plateProvinces {}: no stack {}",
                        r.setting.name(),
                        r.stack
                    )
                })
            })
            .collect();
        self.plates = Some((t, stacks));
        self
    }

    /// The face's plate tectonics, in a world whose rocks come from plates (R5; the big map's
    /// layers, `map_layers.rs`).
    pub fn plate_tectonics(
        &self,
    ) -> Option<&std::sync::Arc<crate::plate_tectonics::PlateTectonics>> {
        self.plates.as_ref().map(|(t, _)| t)
    }

    /// The plate settings at (x, z), in a world whose rocks come from plates (R5).
    pub fn plate_settings(&self, x: i64, z: i64) -> Option<PlateSettings> {
        let (t, _) = self.plates.as_ref()?;
        Some(self.geology.plate_provinces.settings(t, x as f64, z as f64))
    }

    /// With the tectonic provinces of `t` (T5): a volcano's ground is the volcanic stack, the old
    /// core's the core stack. Off (`t` disabled): the noise's provinces alone.
    pub fn with_tectonics(mut self, t: crate::tectonics::Tectonics) -> Self {
        if t.from_plates() {
            // R5: the face seed's plates (the world map's, `hydrology::generate_with_height`).
            let plates = crate::plate_tectonics::PlateTectonics::for_seed(self.world_seed);
            self = self.with_plates(std::sync::Arc::new(plates));
        }
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
        if let Some((t, stacks)) = &self.plates {
            let pp = &self.geology.plate_provinces;
            let (s, edge) = pp.settings_and_edge(t, x as f64, z as f64);
            if let Some(k) = pp.rules.iter().position(|r| s.has(r.setting)) {
                // The shaping fades in from the setting's edge (rocks.json `edgeFade`).
                let fade = if pp.edge_fade > 0.0 {
                    (edge / pp.edge_fade).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                return (stacks[k], fade);
            }
        }
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
        (self.noise_stacks[i], (edge / PROVINCE_FADE).clamp(0.0, 1.0))
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
/// line's height and accumulation. Built once per generator, only where a biome erodes. Stored by
/// its exceptions (`grid::SparseGrid`, BUGS.md #70): over the sea every cell is its own line at 0
/// m; the line's height is read from the world map (its cell's), not copied.
#[derive(Clone, Debug)]
pub struct DrainageField {
    n: usize,
    cell: f64,
    x0: f64,
    dist: crate::grid::SparseGrid<f32>,
    /// Each cell's flow accumulation (cells), read at its nearest line's cell.
    acc: crate::grid::SparseGrid<f32>,
    /// Each cell's nearest drainage cell: `SELF` itself, `u32::MAX` none, else its index (the
    /// fronts' sources).
    source: crate::grid::SparseGrid<u32>,
}

/// `DrainageField::source`'s mark of a cell that is its own line.
const SELF: u32 = u32::MAX - 1;

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
        drop(order);
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
        let dist: Vec<f32> = dist.into_iter().map(|d| d.min(1.0e9)).collect();
        let source: Vec<u32> = src
            .iter()
            .enumerate()
            .map(|(c, &s)| match s {
                usize::MAX => u32::MAX,
                s if s == c => SELF,
                s => u32::try_from(s).expect("a map of under 2^32 - 2 cells"),
            })
            .collect();
        drop(src);
        Self {
            n,
            cell: map.cell,
            x0: map.centre(0).0,
            dist: crate::grid::SparseGrid::from_dense(&dist, 0.0),
            acc: crate::grid::SparseGrid::from_dense(&acc, 1.0),
            source: crate::grid::SparseGrid::from_dense(&source, SELF),
        }
    }

    /// Cell c's nearest drainage cell, if any.
    #[inline]
    fn source_of(&self, c: usize) -> Option<usize> {
        match self.source.get(c) {
            u32::MAX => None,
            SELF => Some(c),
            s => Some(s as usize),
        }
    }

    /// The bytes each of its grids holds.
    pub fn grid_bytes(&self) -> Vec<(&'static str, usize)> {
        vec![
            ("drainage distance", self.dist.bytes()),
            ("drainage accumulation", self.acc.bytes()),
            ("drainage source", self.source.bytes()),
        ]
    }

    /// At face-local (x, z), bilinear between the cells' centres: the distance (m) to the nearest
    /// drainage line, that line's height (m over the sea) and its accumulation (cells). `map` is
    /// the map the field was built of (its heights are the lines').
    pub fn at(&self, map: &crate::worldmap::WorldMap, x: f64, z: f64) -> (f64, f64, f64) {
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
                let (height, accumulation) = match self.source_of(c) {
                    Some(s) => (map.height[s].max(0.0), self.acc.get(s)),
                    None => (0.0, 1.0),
                };
                out[0] += w * f64::from(self.dist.get(c));
                out[1] += w * f64::from(height);
                out[2] += w * f64::from(accumulation);
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
                if let Some(s) = self.source_of((j * n + i) as usize) {
                    let s = s as u32;
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

#[cfg(test)]
mod plate_province_tests {
    //! R5 (docs/proposals/REALISTIC_WORLD.md §3.4): the rock provinces from plate history, by
    //! the two-plate fixtures (plates.json `fixtures`: plates at ±5 km on x, the boundary at x 0).
    use super::*;
    use crate::plate_tectonics::{PlateTectonics, UPLIFT};
    use crate::plates::{FIXTURES, PLATES, Plate, PlateLayout};
    use std::sync::Arc;

    fn fixture(name: &str) -> Arc<PlateTectonics> {
        let f = FIXTURES.iter().find(|f| f.name == name).unwrap();
        let plates = f.plates.map(Plate::from).to_vec();
        Arc::new(PlateTectonics::new(
            PlateLayout::of_plates(&PLATES, plates),
            &UPLIFT,
        ))
    }

    fn resolved(g: Geology, t: &Arc<PlateTectonics>) -> Resolved {
        Resolved::new(g, 1).with_plates(t.clone())
    }

    fn stack_at(r: &Resolved, x: f64, z: f64) -> &'static str {
        r.geology.stacks[r.province(x as i64, z as i64).0].name
    }

    fn rule(setting: PlateSetting) -> &'static str {
        GEOLOGY
            .plate_provinces
            .rules
            .iter()
            .find(|r| r.setting == setting)
            .unwrap()
            .stack
    }

    #[test]
    fn each_setting_takes_its_rules_stack() {
        let pp = &GEOLOGY.plate_provinces;
        // Subduction: the continent (+x) overrides; its accreted strip by the line, the range's
        // core behind it, the arc's cones farther in, the quiet interior beyond.
        let t = fixture("ocean_under_continent");
        let r = resolved(GEOLOGY, &t);
        let cones: Vec<_> = t.volcanoes.iter().filter(|v| v.chain == 0).collect();
        assert!(!cones.is_empty(), "the fixture has an arc");
        for v in cones {
            assert!(pp.settings(&t, v.x, v.z).has(PlateSetting::Arc));
            assert_eq!(stack_at(&r, v.x, v.z), rule(PlateSetting::Arc));
        }
        let near = 0.5 * pp.accreted_width;
        assert!(pp.settings(&t, near, 0.0).has(PlateSetting::Accreted));
        assert_eq!(stack_at(&r, near, 0.0), rule(PlateSetting::Accreted));
        // The diving ocean's side has no strip.
        assert!(!pp.settings(&t, -near, 0.0).has(PlateSetting::Accreted));
        let crest = 0.5 * (pp.range_core[0] + pp.range_core[1]);
        assert!(crest > pp.accreted_width);
        assert_eq!(stack_at(&r, crest, 0.0), rule(PlateSetting::Core));
        let far = pp.basin_distance + 3000.0;
        assert!(pp.settings(&t, far, 0.0).has(PlateSetting::Basin));
        assert_eq!(stack_at(&r, far, 0.0), rule(PlateSetting::Basin));
        // Collision: the core astride the line, on both plates.
        let t = fixture("continents_collide");
        let r = resolved(GEOLOGY, &t);
        for x in [-0.5 * pp.core_width, 0.5 * pp.core_width] {
            assert_eq!(stack_at(&r, x, 2000.0), rule(PlateSetting::Core), "x {x}");
        }
        // A rift: its fill along the line.
        let t = fixture("rift_opens");
        let r = resolved(GEOLOGY, &t);
        for x in [-0.5 * pp.rift_width, 0.5 * pp.rift_width] {
            assert_eq!(stack_at(&r, x, 2000.0), rule(PlateSetting::Rift), "x {x}");
        }
    }

    #[test]
    fn a_settings_edge_fades_the_shaping_in() {
        // WORK_QUEUE item 15: with the shaping on, a rule's province meets the next with no wall.
        // On the accreted strip's edge (accretedWidth m from the trench) the fade is 0; edgeFade
        // m inside it, 1; with edgeFade 0 (by data alone), sharp as before.
        let pp = GEOLOGY.plate_provinces;
        assert!(pp.edge_fade > 0.0, "the build fades the edges");
        let t = fixture("ocean_under_continent");
        let r = resolved(GEOLOGY, &t);
        let fade = |r: &Resolved, x: f64| r.province(x as i64, 0).1;
        let edge = pp.accreted_width;
        assert!(
            fade(&r, edge - 1.0) < 0.01,
            "at the edge: {}",
            fade(&r, edge - 1.0)
        );
        let inside = edge - pp.edge_fade - 1.0;
        assert!(pp.settings(&t, inside, 0.0).has(PlateSetting::Accreted));
        assert!(fade(&r, inside) > 0.99, "inside: {}", fade(&r, inside));
        let mid = edge - 0.5 * pp.edge_fade;
        assert!(
            (fade(&r, mid) - 0.5).abs() < 0.01,
            "halfway: {}",
            fade(&r, mid)
        );
        let sharp = Geology {
            plate_provinces: PlateProvinces {
                edge_fade: 0.0,
                ..pp
            },
            ..GEOLOGY
        };
        assert_eq!(fade(&resolved(sharp, &t), edge - 1.0), 1.0);
    }

    #[test]
    fn the_build_names_todays_blocks_for_each_setting() {
        // §7 q5: no new rock blocks; each setting's top rock shows a block of today's registry.
        let face = |s: PlateSetting| {
            let st = &GEOLOGY.stacks[GEOLOGY.stack(rule(s)).unwrap()];
            GEOLOGY.rocks[GEOLOGY.rock(st.layers[0].rock).unwrap()].face
        };
        assert_eq!(face(PlateSetting::Arc), "dwell:basalt");
        assert_eq!(face(PlateSetting::Hotspot), "dwell:basalt");
        assert_eq!(face(PlateSetting::Core), "dwell:stone");
        assert_eq!(face(PlateSetting::Accreted), "dwell:stone");
        assert_eq!(face(PlateSetting::Basin), "dwell:red_sandstone");
        assert_eq!(face(PlateSetting::Rift), "dwell:red_sandstone");
    }

    #[test]
    fn a_rule_or_a_rocks_block_changed_by_data_alone_changes_the_province() {
        let t = fixture("continents_collide");
        let at = (0.0, 2000.0);
        // The core's rule now names the basalt flows: the province follows.
        let mut rules = GEOLOGY.plate_provinces.rules.to_vec();
        for r in &mut rules {
            if r.setting == PlateSetting::Core {
                r.stack = "dwell:basalt_flows";
            }
        }
        let mut g = GEOLOGY;
        g.plate_provinces.rules = Box::leak(rules.into_boxed_slice());
        assert_eq!(stack_at(&resolved(g, &t), at.0, at.1), "dwell:basalt_flows");
        // A later granite block: the core's rock shows it by data alone.
        let mut rocks = GEOLOGY.rocks.to_vec();
        let gi = GEOLOGY.rock("dwell:granite").unwrap();
        rocks[gi].face = "dwell:bedrock";
        let mut g = GEOLOGY;
        g.rocks = Box::leak(rocks.into_boxed_slice());
        let r = resolved(g, &t);
        let (stack, _) = r.province(at.0 as i64, at.1 as i64);
        // The stack's top layer (granite over schist, built from sea level up: 40..100 m).
        let (_, rock, _) = r.profiles[stack].map(70.0);
        assert_eq!(r.geology.rocks[rock].face, "dwell:bedrock");
        // No rule holds (an empty list): the noise's provinces, as without plates.
        let mut g = GEOLOGY;
        g.plate_provinces.rules = &[];
        let (with, without) = (resolved(g, &t), Resolved::new(g, 1));
        assert_eq!(with.province(0, 2000), without.province(0, 2000));
    }

    #[test]
    fn a_zero_weight_stack_is_never_the_noises() {
        let r = Resolved::new(GEOLOGY, 7);
        for i in 0..400 {
            let (x, z) = (i * 1733 - 300_000, i * 977 - 200_000);
            let (k, _) = r.province(x, z);
            assert!(GEOLOGY.stacks[k].weight > 0.0, "{}", GEOLOGY.stacks[k].name);
        }
    }
}
