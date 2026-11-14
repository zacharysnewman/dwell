//! Structure placements (docs/WORLD_GENERATION.md §2, docs/STRUCTURES.md §8, ADR 0043): committed structures
//! (`shared/structures`) the generator stands on its ground — today the owner's two pines, in
//! groves. The rules are data (`shared/worldgen/placements.json`, generated into
//! `placements.gen.rs` by `shared/worldgen/gen.mjs`), resolved by name once per generator.
//!
//! Each placement divides the ground into `grid`-metre cells holding one candidate each, at a
//! hashed point in the cell. A candidate is kept by its chance (the density, inside the grove noise
//! or `grove_outside` of it, scaled by the height's `rise`), then only where the ground at its
//! anchor (and, with `footprint_on`, every column under it) is one of `on`, dry, in the height band
//! and no steeper than the slope limit. Of two kept candidates whose boxes come within `spacing`
//! blocks of each other, the one with the lower priority (a hash) is dropped — whether or not the
//! higher one is itself dropped, so the answer for a candidate needs only its neighbours, never a
//! chain. Everything is a pure function of the world seed and positions, so a chunk finds the trees
//! reaching into it from its neighbours exactly as they do, in any order, natively and in WASM
//! (only `+ − ×`, comparisons and integer hashing, ADR 0010).

use dwell_blocks::StateId;
use dwell_world::structure::{self, Orientation, Placed, Structure};

use crate::noise::{clamp01, fbm2, floor_div, hash3, seed_word, smooth_step, unit};

/// One placement rule as data (see the generator's validation for the ranges).
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub name: &'static str,
    /// The structures placed, by name, with their weights.
    pub structures: &'static [(&'static str, u32)],
    /// The ground blocks it stands on, by id.
    pub on: &'static [&'static str],
    /// Every column under the structure must be one of `on` (else only the anchor's).
    pub footprint_on: bool,
    /// The ground under the trunk's columns (the structure's lowest cells) is levelled up to the
    /// trunk's base, ramps there made full cells (`pad: "trunk"`, WORK_QUEUE item 44).
    pub pad_trunk: bool,
    /// One candidate per cell of this size (m).
    pub grid: i32,
    /// The chance a cell's candidate is kept, inside a grove.
    pub density: f32,
    /// Groves: a noise of this wavelength (m; 0: none, everywhere) covering about `grove_coverage`
    /// of the ground; outside them the chance is `grove_outside` of the density.
    pub grove_wavelength: i32,
    pub grove_coverage: f32,
    pub grove_outside: f32,
    /// The ground at the anchor (its top voxel, m above sea level) must lie in [min_height,
    /// max_height]. The chance rises linearly from `rise_low` of itself at `min_height` to all of
    /// it at `rise_full`.
    pub min_height: i32,
    pub max_height: i32,
    pub rise_full: i32,
    pub rise_low: f32,
    /// The steepest ground at the anchor: the squared gradient of the height field over ±2 m.
    pub max_gradient_sq: f32,
    /// The least gap between two placements' boxes (blocks).
    pub spacing: i32,
    /// Quarter turns about +Y, one picked per placement.
    pub rotations: &'static [u8],
    /// Block swaps by the anchor's height (the snowy leaves of pines high up).
    pub variants: &'static [Variant],
    /// The density × this where the erosion leaves the ground standing (a mesa's or butte's top,
    /// `TerrainGenerator::erosion_at`'s margin above 0; 1: no matter). Sagebrush mostly on the
    /// floors (WORK_QUEUE item 15, 0.66: "tops read green under thick sagebrush").
    pub tops: f32,
}

/// A swap of a placed structure's blocks by its anchor's height (m above sea level): none at
/// `from`, every structure from `full`, in proportion between (a hash per structure).
#[derive(Clone, Copy, Debug)]
pub struct Variant {
    pub name: &'static str,
    /// (from, to) blocks, by id.
    pub replace: &'static [(&'static str, &'static str)],
    pub from: i32,
    pub full: i32,
}

mod table {
    include!("placements.gen.rs");
}
pub use table::PLACEMENTS;

/// A column's ground as placements see it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ground {
    /// The top voxel (face-local y).
    pub top: i32,
    /// The top voxel's height above sea level (m).
    pub above_sea: i32,
    /// The height field there (m, continuous; any datum, only differences are used).
    pub height: f32,
    /// The top voxel's block.
    pub block: StateId,
    /// Nothing but air over it (no water).
    pub dry: bool,
}

/// One structure of a placement, resolved.
#[derive(Clone, Debug)]
struct Shape {
    structure: Structure,
    weight: u32,
    /// A species' tree: (species index, seed, stage) it was grown at (ADR 0079), else `None`.
    grown: Option<(u16, u32, u32)>,
    /// Its box relative to the anchor in each quarter turn: (min x, min z, max x, max z).
    boxes: [[i32; 4]; 4],
    /// The rise of its top voxel above its anchor (blocks).
    rise: i32,
    /// Its trunk's columns relative to the anchor in each quarter turn (the columns of its lowest
    /// row's cells: a species tree's foot logs, a hand-built tree's trunk), and that row's offset
    /// from the anchor.
    trunk: [Vec<(i32, i32)>; 4],
    base: i32,
    /// Its stem in each quarter turn: the bottom cell of the trunk column nearest the anchor's,
    /// relative to the anchor, and how many cells stand straight up from it (at most
    /// [`STEM_CAP`]): the cells a far tree's impostor checks to see the tree felled.
    stem: [([i32; 3], u8); 4],
}

/// The most stem cells a far tree is checked by (½ m cells: 8 m, above a player's reach for a cut).
pub const STEM_CAP: u8 = 16;

/// A placement resolved for one world: block ids, structures and noise seeds.
#[derive(Clone, Debug)]
pub struct Planter {
    shapes: Vec<Shape>,
    total_weight: u32,
    on: Vec<StateId>,
    rule: Placement,
    grove_low: f32,
    grove_high: f32,
    grove_seed: u32,
    cell_seed: u32,
    offset: (i64, i64),
    /// The LOD's tint: the block most of its structures' cells are, and the share of the ground
    /// their crowns cover where every candidate is kept.
    canopy: StateId,
    cover: f32,
    /// The variants' swaps, resolved by name.
    variants: Vec<Vec<(StateId, StateId)>>,
    /// The most the biomes' `trees` densities raise its chance by (ADR 0074 B5): 0 where no biome
    /// of the world lists it (it stands nowhere), 1 by default; the factor brings it down per column.
    boost: f32,
}

/// A placed structure's stem (placed at the origin): the bottom cell of its trunk column (`cols`,
/// its lowest row's columns) nearest the anchor's, and the solid cells straight up from it, at most
/// [`STEM_CAP`].
fn stem_of(placed: &dwell_world::structure::Placed, cols: &[(i32, i32)]) -> ([i32; 3], u8) {
    let Some(&(x, z)) = cols.iter().min_by_key(|(x, z)| (x * x + z * z, *x, *z)) else {
        return ([0, 0, 0], 0);
    };
    let low = placed
        .cells()
        .filter(|(c, _)| c[0] == x && c[2] == z)
        .map(|(c, _)| c[1])
        .min()
        .unwrap_or(0);
    let solid = |y: i32| matches!(placed.at([x, y, z]), Some(m) if m != dwell_world::M.air);
    let n = (0..i32::from(STEM_CAP))
        .take_while(|&h| solid(low + h))
        .count();
    ([x, low, z], n as u8)
}

/// The share of candidates the slope, footprint and overlap tests keep, as measured over seeds
/// (`pines_stand_on_grass_in_the_height_band_at_about_the_target_density`: ~0.55): the LOD's tint
/// covers the ground about as the trees do.
const KEPT_SHARE: f32 = 0.55;

/// The grove noise's soft edge: half its width in the noise's units.
const GROVE_EDGE: f32 = 0.06;

/// A structure the generator places.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tree {
    /// Which planter, and its grid cell.
    pub planter: u8,
    pub cell: (i32, i32),
    /// The anchor voxel: on the ground's top voxel + 1 (face-local).
    pub anchor: [i32; 3],
    /// Which of the planter's structures, turned how far.
    pub shape: u8,
    pub quarter_turns: u8,
    /// Drawn from the seed: decides overlaps (higher wins) and the structure's block choices.
    pub priority: u32,
    /// Its variant (index + 1 into the rule's `variants`), or 0.
    pub variant: u8,
}

impl Tree {
    fn rank(&self) -> (u32, u8, i32, i32) {
        (self.priority, self.planter, self.cell.0, self.cell.1)
    }
}

impl Planter {
    /// Resolves `rules` for `world_seed`, the grove noise sampled at a position plus `offset`.
    pub fn resolve(rules: &[Placement], world_seed: u64, offset: (i64, i64)) -> Vec<Planter> {
        rules
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let stream = 0x7EE5_0000 + 2 * i as u32;
                // A name may be a committed structure or a tree species (ADR 0079): a species
                // stands its seeded, staged trees, each of them at the entry's weight.
                let shapes: Vec<Shape> = p
                    .structures
                    .iter()
                    .flat_map(|&(name, weight)| {
                        let built = match crate::species::index(name) {
                            Some(i) => {
                                let s = &crate::species::SPECIES[i];
                                crate::species::structures(s).map(|v| {
                                    v.into_iter()
                                        .zip(crate::species::wild(s))
                                        .map(|(t, (seed, stage))| {
                                            (t, Some((i as u16, seed, stage)))
                                        })
                                        .collect::<Vec<_>>()
                                })
                            }
                            None => structure::load(name).map(|s| vec![(s, None)]),
                        };
                        built
                            .unwrap_or_else(|e| panic!("placement {}: {e}", p.name))
                            .into_iter()
                            .map(move |(s, g)| (s, weight, g))
                    })
                    .map(|(structure, weight, grown)| {
                        let boxes = std::array::from_fn(|t| {
                            let placed = structure.place_oriented(
                                [0, 0, 0],
                                Orientation {
                                    quarter_turns: t as u8,
                                    mirror: None,
                                },
                                0,
                            );
                            [placed.min[0], placed.min[2], placed.max[0], placed.max[2]]
                        });
                        let rise = structure.size[1] - 1 - structure.anchor[1];
                        let trunk = std::array::from_fn(|t| {
                            let placed = structure.place_oriented(
                                [0, 0, 0],
                                Orientation {
                                    quarter_turns: t as u8,
                                    mirror: None,
                                },
                                0,
                            );
                            let low = placed.cells().map(|(c, _)| c[1]).min().unwrap_or(0);
                            let mut cols: Vec<(i32, i32)> = placed
                                .cells()
                                .filter(|(c, _)| c[1] == low)
                                .map(|(c, _)| (c[0], c[2]))
                                .collect();
                            cols.sort_unstable();
                            cols.dedup();
                            cols
                        });
                        let base = trunk_base(&structure).unwrap_or(0);
                        let stem = std::array::from_fn(|t| {
                            let placed = structure.place_oriented(
                                [0, 0, 0],
                                Orientation {
                                    quarter_turns: t as u8,
                                    mirror: None,
                                },
                                0,
                            );
                            stem_of(&placed, &trunk[t])
                        });
                        Shape {
                            structure,
                            weight,
                            grown,
                            boxes,
                            rise,
                            trunk,
                            base,
                            stem,
                        }
                    })
                    .collect();
                let threshold = 0.5 - p.grove_coverage;
                // The canopy: the commonest state over the structures' cells; the cover: their
                // mean footprint (by weight) over a grid cell's area.
                let mut counts: Vec<(StateId, usize)> = Vec::new();
                let (mut area, mut weights) = (0.0f32, 0.0f32);
                for s in &shapes {
                    for (_, m) in s.structure.place([0, 0, 0], 0).cells() {
                        match counts.iter_mut().find(|(k, _)| *k == m) {
                            Some(c) => c.1 += 1,
                            None => counts.push((m, 1)),
                        }
                    }
                    let b = s.boxes[0];
                    area += s.weight as f32 * ((b[2] - b[0] + 1) * (b[3] - b[1] + 1)) as f32;
                    weights += s.weight as f32;
                }
                let canopy = counts
                    .iter()
                    .max_by_key(|(_, n)| *n)
                    .map_or(dwell_world::M.air, |(m, _)| *m);
                let cover =
                    (area / weights.max(1.0) / (p.grid * p.grid) as f32 * KEPT_SHARE).min(1.0);
                Planter {
                    canopy,
                    cover,
                    variants: p
                        .variants
                        .iter()
                        .map(|v| {
                            v.replace
                                .iter()
                                .map(|(a, b)| (dwell_blocks::require(a), dwell_blocks::require(b)))
                                .collect()
                        })
                        .collect(),
                    total_weight: shapes.iter().map(|s| s.weight).sum(),
                    shapes,
                    on: p.on.iter().map(|o| dwell_blocks::require(o)).collect(),
                    rule: *p,
                    grove_low: threshold - GROVE_EDGE,
                    grove_high: threshold + GROVE_EDGE,
                    grove_seed: seed_word(world_seed, stream),
                    cell_seed: seed_word(world_seed, stream + 1),
                    offset,
                    boost: 1.0,
                }
            })
            .collect()
    }

    /// The most the biomes raise its chance by (0: it stands nowhere; ADR 0074 B5).
    pub fn boost(&self) -> f32 {
        self.boost
    }

    /// Sets [`Planter::boost`] (the generator, from its biomes' `trees`).
    pub fn set_boost(&mut self, boost: f32) {
        self.boost = boost;
    }

    /// Whether it stands on ground block `b`.
    pub fn stands_on(&self, b: StateId) -> bool {
        self.on.contains(&b)
    }

    /// Whether it can stand anywhere in this world.
    pub fn active(&self) -> bool {
        self.boost > 0.0
    }

    /// The farthest any of its structures reaches from the anchor horizontally (blocks).
    pub fn reach(&self) -> i32 {
        self.shapes
            .iter()
            .flat_map(|s| s.boxes.iter().flatten())
            .fold(0, |r, v| r.max(v.abs()))
    }

    /// The highest any of its structures rises over the anchor (blocks).
    pub fn rise(&self) -> i32 {
        self.shapes.iter().map(|s| s.rise).max().unwrap_or(0)
    }

    /// The grove factor at a point: 1 inside a grove, `grove_outside` far from one.
    pub fn grove(&self, x: i32, z: i32) -> f32 {
        if self.rule.grove_wavelength <= 0 {
            return 1.0;
        }
        let p = fbm2(
            self.grove_seed,
            i64::from(x) + self.offset.0,
            i64::from(z) + self.offset.1,
            self.rule.grove_wavelength,
            2,
            None,
        );
        let inside = smooth_step(self.grove_low, self.grove_high, p);
        self.rule.grove_outside + (1.0 - self.rule.grove_outside) * inside
    }

    /// The height factor for ground `above_sea` m above sea level (0 outside the band).
    pub fn rise_factor(&self, above_sea: i32) -> f32 {
        let r = &self.rule;
        if above_sea < r.min_height || above_sea > r.max_height {
            return 0.0;
        }
        if r.rise_full <= r.min_height {
            return 1.0;
        }
        let t = clamp01((above_sea - r.min_height) as f32 / (r.rise_full - r.min_height) as f32);
        r.rise_low + (1.0 - r.rise_low) * t
    }

    /// The chance a candidate at (x, z) on ground `above_sea` m above sea level is kept (before its
    /// ground, slope and neighbours are asked).
    pub fn chance(&self, x: i32, z: i32, above_sea: i32) -> f32 {
        self.rule.density * self.grove(x, z) * self.rise_factor(above_sea)
    }

    /// The hash `k` of grid cell (gx, gz).
    fn h(&self, gx: i32, gz: i32, k: i32) -> u32 {
        hash3(self.cell_seed, gx, k, gz)
    }

    /// The candidate point of grid cell (gx, gz).
    pub fn point(&self, gx: i32, gz: i32) -> (i32, i32) {
        let g = self.rule.grid as u32;
        (
            gx * self.rule.grid + (self.h(gx, gz, 0) % g) as i32,
            gz * self.rule.grid + (self.h(gx, gz, 1) % g) as i32,
        )
    }

    /// The candidate of grid cell (gx, gz), if its rolls keep it and its ground takes it (`ground`
    /// answers any column; `None` where there is none, beyond the world's rim).
    pub fn candidate(
        &self,
        index: u8,
        (gx, gz): (i32, i32),
        ground: &dyn Fn(i32, i32) -> Option<Ground>,
    ) -> Option<Tree> {
        self.candidate_with(index, (gx, gz), ground, None)
    }

    /// [`Planter::candidate`] with the climate's `factor` (planter index, x, z → 0..1) on its
    /// chance (systemic terrain T3, `vegetation.rs`), asked only of a candidate its other rolls keep.
    pub fn candidate_with(
        &self,
        index: u8,
        (gx, gz): (i32, i32),
        ground: &dyn Fn(i32, i32) -> Option<Ground>,
        factor: Option<&Factor<'_>>,
    ) -> Option<Tree> {
        let (x, z) = self.point(gx, gz);
        // The roll against the chance without its height factor (at most 1) first: most cells
        // end here without asking for ground.
        if !self.active() {
            return None;
        }
        let roll = unit(self.h(gx, gz, 2));
        let open = self.rule.density * self.grove(x, z);
        // A biome's density above 1 raises the most the chance can be (B5; 1 by default).
        let most = open * self.boost.max(1.0);
        if roll >= most {
            return None;
        }
        let g = ground(x, z)?;
        if !g.dry || !self.on.contains(&g.block) {
            return None;
        }
        if roll >= most * self.rise_factor(g.above_sea) {
            return None;
        }
        if let Some(f) = factor
            && roll >= open * self.rise_factor(g.above_sea) * f(index as usize, x, z)
        {
            return None;
        }
        let mut pick = self.h(gx, gz, 3) % self.total_weight.max(1);
        let mut shape = 0;
        for (i, s) in self.shapes.iter().enumerate() {
            if pick < s.weight {
                shape = i;
                break;
            }
            pick -= s.weight;
        }
        let rotations = self.rule.rotations;
        let quarter_turns = rotations[(self.h(gx, gz, 4) % rotations.len() as u32) as usize];
        let tree = Tree {
            planter: index,
            cell: (gx, gz),
            anchor: [x, g.top + 1, z],
            shape: shape as u8,
            quarter_turns,
            priority: self.h(gx, gz, 5),
            variant: self.variant_at(g.above_sea, self.h(gx, gz, 6)),
        };
        if !self.slope_ok(x, z, ground) {
            return None;
        }
        if self.rule.footprint_on {
            let [x0, z0, x1, z1] = self.footprint(&tree);
            for cz in z0..=z1 {
                for cx in x0..=x1 {
                    match ground(cx, cz) {
                        Some(c) if c.dry && self.on.contains(&c.block) => {}
                        _ => return None,
                    }
                }
            }
        }
        Some(tree)
    }

    /// The slope test at the anchor: the height field's gradient by central differences over ±2 m.
    fn slope_ok(&self, x: i32, z: i32, ground: &dyn Fn(i32, i32) -> Option<Ground>) -> bool {
        let at = |dx: i32, dz: i32| ground(x + dx, z + dz).map(|c| c.height);
        let (Some(e), Some(w), Some(s), Some(n)) = (at(2, 0), at(-2, 0), at(0, 2), at(0, -2))
        else {
            return false;
        };
        let gx = (e - w) * 0.25;
        let gz = (s - n) * 0.25;
        gx * gx + gz * gz <= self.rule.max_gradient_sq
    }

    /// A tree's columns: (min x, min z, max x, max z), inclusive.
    pub fn footprint(&self, t: &Tree) -> [i32; 4] {
        let b = self.shapes[t.shape as usize].boxes[t.quarter_turns as usize % 4];
        [
            t.anchor[0] + b[0],
            t.anchor[2] + b[1],
            t.anchor[0] + b[2],
            t.anchor[2] + b[3],
        ]
    }

    /// A tree's pad (`pad: "trunk"`): the trunk's columns (x, z) and its base row (face-local y:
    /// the lowest row of its cells); empty for a placement without one. The ground under each
    /// column is levelled up to the row below the base (WORK_QUEUE item 44).
    pub fn pad(&self, t: &Tree) -> (Vec<(i32, i32)>, i32) {
        let s = &self.shapes[t.shape as usize];
        if !self.rule.pad_trunk {
            return (Vec::new(), t.anchor[1] + s.base);
        }
        let cols = s.trunk[t.quarter_turns as usize % 4]
            .iter()
            .map(|&(dx, dz)| (t.anchor[0] + dx, t.anchor[2] + dz))
            .collect();
        (cols, t.anchor[1] + s.base)
    }

    /// The variant (index + 1) a structure anchored `above_sea` m above sea level takes by its
    /// hash `h`: the first whose band takes it.
    fn variant_at(&self, above_sea: i32, h: u32) -> u8 {
        let roll = unit(h);
        for (i, v) in self.rule.variants.iter().enumerate() {
            let share = if v.full <= v.from {
                f32::from(u8::from(above_sea >= v.from))
            } else {
                clamp01((above_sea - v.from) as f32 / (v.full - v.from) as f32)
            };
            if roll < share {
                return (i + 1) as u8;
            }
        }
        0
    }

    /// A tree's voxels, as placed (its variant's blocks swapped in).
    pub fn place(&self, t: &Tree) -> Placed {
        let mut placed = self.shapes[t.shape as usize].structure.place_oriented(
            t.anchor,
            Orientation {
                quarter_turns: t.quarter_turns,
                mirror: None,
            },
            u64::from(t.priority),
        );
        if let Some(swaps) = self.variants.get(usize::from(t.variant).wrapping_sub(1)) {
            for &(from, to) in swaps {
                placed.replace(from, to);
            }
        }
        placed
    }

    /// Whether any of its structures is a species tree (ADR 0079: they grow).
    pub fn grows(&self) -> bool {
        self.shapes.iter().any(|s| s.grown.is_some())
    }

    /// A tree's stem (face-local): its bottom cell and the cells standing straight up from it
    /// (`Shape::stem`), what a far tree's impostor reads to see it felled.
    pub fn stem(&self, t: &Tree) -> ([i32; 3], u8) {
        let Some(s) = self.shapes.get(usize::from(t.shape)) else {
            return (t.anchor, 0);
        };
        let (d, n) = s.stem[t.quarter_turns as usize % 4];
        (std::array::from_fn(|i| t.anchor[i] + d[i]), n)
    }

    /// The species tree a tree is, if it is one: (species index, seed, stage) (ADR 0079).
    pub fn grown(&self, t: &Tree) -> Option<(u16, u32, u32)> {
        self.shapes.get(usize::from(t.shape)).and_then(|s| s.grown)
    }

    /// The block swaps of variant `v` (index + 1; 0 or unknown: none).
    pub fn swaps(&self, v: u8) -> &[(StateId, StateId)] {
        self.variants
            .get(usize::from(v).wrapping_sub(1))
            .map_or(&[], Vec::as_slice)
    }

    /// The structure a tree is, by name.
    pub fn structure_name(&self, t: &Tree) -> &'static str {
        self.shapes[t.shape as usize].structure.name
    }

    /// The rule's data.
    pub fn rule(&self) -> &Placement {
        &self.rule
    }

    /// The block the LOD tints a canopy with.
    pub fn canopy(&self) -> StateId {
        self.canopy
    }

    /// Whether the LOD's column at (x, z) of a level coarser than the trees shows canopy: a share
    /// of columns as large as the trees' cover there, on ground of `on`, dry, in the band and no
    /// steeper than the limit (`gradient_sq`, from the LOD's own columns).
    pub fn canopy_at(&self, at: (i32, i32), level: i32, g: Ground, gradient_sq: f32) -> bool {
        self.canopy_at_with(at, level, g, gradient_sq, None)
    }

    /// [`Planter::canopy_at`] with the climate's factor on the share (systemic terrain T3), asked
    /// only of a column the share would otherwise keep.
    pub fn canopy_at_with(
        &self,
        (x, z): (i32, i32),
        level: i32,
        g: Ground,
        gradient_sq: f32,
        factor: Option<&dyn Fn() -> f32>,
    ) -> bool {
        if !self.active()
            || !g.dry
            || !self.on.contains(&g.block)
            || gradient_sq > self.rule.max_gradient_sq
        {
            return false;
        }
        let roll = unit(hash3(self.cell_seed ^ 0xC0A0_0000, x, level, z));
        let share = self.chance(x, z, g.above_sea) * self.cover;
        if roll >= share * self.boost.max(1.0) {
            return false;
        }
        factor.is_none_or(|f| roll < share * f())
    }
}

/// The lowest row of a structure's cells relative to its anchor.
fn trunk_base(s: &Structure) -> Option<i32> {
    s.place([0, 0, 0], 0).cells().map(|(c, _)| c[1]).min()
}

/// The pads of `trees` (each `Planter::pad`) over columns [x0, x1] × [z0, z1]: for each column
/// (row-major from (x0, z0)) the highest base row of a trunk standing on it, if any.
pub fn pads_over(
    planters: &[Planter],
    trees: &[Tree],
    (x0, z0): (i32, i32),
    (x1, z1): (i32, i32),
) -> Vec<Option<i32>> {
    let w = (x1 - x0 + 1).max(0);
    let l = (z1 - z0 + 1).max(0);
    let mut out = vec![None; (w * l) as usize];
    for t in trees {
        let (cols, base) = planters[t.planter as usize].pad(t);
        for (x, z) in cols {
            if (x0..=x1).contains(&x) && (z0..=z1).contains(&z) {
                let k = ((z - z0) * w + (x - x0)) as usize;
                out[k] = Some(out[k].map_or(base, |b: i32| b.max(base)));
            }
        }
    }
    out
}

/// How far beyond a rectangle `trees_in` asks for ground (m): a neighbour's reach and spacing,
/// its own reach, and the slope test's 2 m.
pub fn ground_margin(planters: &[Planter]) -> i32 {
    let on = || planters.iter().filter(|p| p.active());
    let reach = on().map(Planter::reach).max().unwrap_or(0);
    // A negative spacing (crowns interleaving) needs no more ground than none.
    let spacing = on().map(|p| p.rule.spacing).max().unwrap_or(0).max(0);
    3 * reach + spacing + reach.max(2) + 1
}

/// The highest any placement rises over its anchor (blocks).
pub fn max_rise(planters: &[Planter]) -> i32 {
    planters
        .iter()
        .filter(|p| p.active())
        .map(Planter::rise)
        .max()
        .unwrap_or(0)
}

/// The climate's factor on a placement's chance: (planter index, x, z) → 0..1 (`vegetation.rs`).
pub type Factor<'a> = dyn Fn(usize, i32, i32) -> f32 + 'a;

/// A placement's spacing at an anchor (planter index, x, z) → blocks, with the most it can be:
/// the biomes' `spacing` (0.52.65).
pub struct Spacing<'a> {
    pub at: &'a dyn Fn(usize, i32, i32) -> i32,
    pub most: i32,
}

/// Every placed structure whose box reaches the columns [x0, x1] × [z0, z1], in a fixed order
/// (planter, then grid cell). `ground` must answer columns up to `ground_margin` beyond them.
pub fn trees_in(
    planters: &[Planter],
    lo: (i32, i32),
    hi: (i32, i32),
    ground: &dyn Fn(i32, i32) -> Option<Ground>,
) -> Vec<Tree> {
    trees_in_with(planters, lo, hi, ground, None, None)
}

/// [`trees_in`] with the climate's `factor` on each candidate's chance (systemic terrain T3).
pub fn trees_in_with(
    planters: &[Planter],
    (x0, z0): (i32, i32),
    (x1, z1): (i32, i32),
    ground: &dyn Fn(i32, i32) -> Option<Ground>,
    factor: Option<&Factor<'_>>,
    spacing_at: Option<&Spacing<'_>>,
) -> Vec<Tree> {
    let on = || planters.iter().filter(|p| p.active());
    let reach = on().map(Planter::reach).max().unwrap_or(0);
    let spacing = on()
        .map(|p| p.rule.spacing)
        .max()
        .unwrap_or(0)
        .max(spacing_at.map_or(0, |s| s.most));
    // Trees reaching the rectangle have anchors within `reach` of it; those that could overlap
    // them, within another 2 × reach + spacing.
    let near = 3 * reach + spacing;
    let mut all = Vec::new();
    for (i, p) in planters.iter().enumerate() {
        if !p.active() {
            continue;
        }
        let g = p.rule.grid;
        for gz in floor_div(z0 - near, g)..=floor_div(z1 + near, g) {
            for gx in floor_div(x0 - near, g)..=floor_div(x1 + near, g) {
                let (x, z) = p.point(gx, gz);
                if x < x0 - near || x > x1 + near || z < z0 - near || z > z1 + near {
                    continue;
                }
                if let Some(t) = p.candidate_with(i as u8, (gx, gz), ground, factor) {
                    all.push(t);
                }
            }
        }
    }
    let boxes: Vec<[i32; 4]> = all
        .iter()
        .map(|t| planters[t.planter as usize].footprint(t))
        .collect();
    // Each candidate's spacing: its placement's, or its anchor's biome's.
    let gaps: Vec<i32> = all
        .iter()
        .map(|t| match spacing_at {
            Some(s) => (s.at)(t.planter as usize, t.anchor[0], t.anchor[2]),
            None => planters[t.planter as usize].rule.spacing,
        })
        .collect();
    let mut out = Vec::new();
    for (i, t) in all.iter().enumerate() {
        let b = boxes[i];
        if b[0] > x1 || b[2] < x0 || b[1] > z1 || b[3] < z0 {
            continue;
        }
        let gap = gaps[i];
        let beaten = all.iter().enumerate().any(|(j, o)| {
            let c = boxes[j];
            let gap = gap.max(gaps[j]);
            j != i
                && o.rank() > t.rank()
                && b[0] <= c[2] + gap
                && c[0] <= b[2] + gap
                && b[1] <= c[3] + gap
                && c[1] <= b[3] + gap
        });
        if !beaten {
            out.push(*t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(block: &'static str) -> impl Fn(i32, i32) -> Option<Ground> {
        let b = dwell_blocks::require(block);
        move |_, _| {
            Some(Ground {
                top: 100,
                above_sea: 68,
                height: 101.0,
                block: b,
                dry: true,
            })
        }
    }

    #[test]
    fn the_data_resolves_by_name() {
        let p = Planter::resolve(&PLACEMENTS, 7, (0, 0));
        assert_eq!(p.len(), PLACEMENTS.len());
        assert_eq!(p[0].reach(), 3, "the tall pine reaches 3 m from its trunk");
        // In cells: the 9 m pine is 18 ½ m cells on the half-height grid, its top 17 over the anchor.
        assert_eq!(p[0].rise(), 9 * dwell_world::rows_for_m(1.0) - 1);
    }

    #[test]
    fn a_placement_naming_a_species_stands_its_seeded_trees() {
        // face-a-only: the planter's rules are face-local; the face is crossed by the generator.
        let rule = Placement {
            name: "test_oaks",
            structures: &[("dwell:oak", 1)],
            on: &["dwell:grass"],
            footprint_on: false,
            pad_trunk: false,
            grid: 12,
            density: 1.0,
            grove_wavelength: 0,
            grove_coverage: 1.0,
            grove_outside: 1.0,
            min_height: 0,
            max_height: 100,
            rise_full: 0,
            rise_low: 1.0,
            max_gradient_sq: 1.0,
            spacing: 0,
            rotations: &[0, 1, 2, 3],
            variants: &[],
            tops: 1.0,
        };
        let p = Planter::resolve(&[rule], 3, (0, 0));
        let oak = crate::species::find("dwell:oak").unwrap();
        assert_eq!(p[0].shapes.len(), oak.variants as usize);
        let trees = trees_in(&p, (0, 0), (239, 239), &flat("dwell:grass"));
        assert!(trees.iter().all(|t| p[0].structure_name(t) == "dwell:oak"));
        let shapes: std::collections::BTreeSet<u8> = trees.iter().map(|t| t.shape).collect();
        assert!(
            shapes.len() >= 6,
            "seeded oaks of several heights: {shapes:?}"
        );
        for t in &trees {
            let placed = p[0].place(t);
            let foot = placed.at(t.anchor).expect("a trunk at the anchor");
            assert_eq!(
                dwell_blocks::block_of(foot).name,
                "dwell:log",
                "a trunk at the anchor"
            );
        }
    }

    #[test]
    fn a_placement_added_by_data_alone_places_its_structure_by_its_rules() {
        // A fixture rule: the proto rock on dirt, everywhere, one per 10 m cell.
        let rule = Placement {
            name: "test_rocks",
            structures: &[("dwell:proto_rock", 1)],
            on: &["dwell:dirt"],
            footprint_on: true,
            pad_trunk: true,
            grid: 10,
            density: 1.0,
            grove_wavelength: 0,
            grove_coverage: 1.0,
            grove_outside: 1.0,
            min_height: 0,
            max_height: 100,
            rise_full: 0,
            rise_low: 1.0,
            max_gradient_sq: 1.0,
            spacing: 0,
            rotations: &[0],
            variants: &[],
            tops: 1.0,
        };
        let p = Planter::resolve(&[rule], 3, (0, 0));
        let dirt = flat("dwell:dirt");
        let trees = trees_in(&p, (0, 0), (99, 99), &dirt);
        let names: Vec<_> = trees.iter().map(|t| p[0].structure_name(t)).collect();
        assert!(names.iter().all(|n| *n == "dwell:proto_rock"));
        // Nearly one per cell (some dropped where two boxes meet), and those reaching in.
        assert!((60..=121).contains(&trees.len()), "{}", trees.len());
        assert!(
            trees_in(&p, (0, 0), (99, 99), &flat("dwell:grass")).is_empty(),
            "not its ground"
        );
        let high = |x, z| {
            dirt(x, z).map(|g| Ground {
                above_sea: 101,
                ..g
            })
        };
        assert!(
            trees_in(&p, (0, 0), (99, 99), &high).is_empty(),
            "above its band"
        );
        let steep = |x: i32, z| {
            dirt(x, z).map(|g| Ground {
                height: x as f32 * 1.5,
                ..g
            })
        };
        assert!(
            trees_in(&p, (0, 0), (99, 99), &steep).is_empty(),
            "too steep"
        );
        let wet = |x, z| dirt(x, z).map(|g| Ground { dry: false, ..g });
        assert!(
            trees_in(&p, (0, 0), (99, 99), &wet).is_empty(),
            "under water"
        );
        // The climate's factor: 1 everywhere places the same; 0 west of x = 50 none there.
        let one = |_: usize, _: i32, _: i32| 1.0;
        assert_eq!(
            trees_in_with(&p, (0, 0), (99, 99), &dirt, Some(&one), None),
            trees
        );
        let east = |_: usize, x: i32, _: i32| f32::from(u8::from(x >= 50));
        let half = trees_in_with(&p, (0, 0), (99, 99), &dirt, Some(&east), None);
        assert!(!half.is_empty() && half.iter().all(|t| t.anchor[0] >= 50));
    }
}
