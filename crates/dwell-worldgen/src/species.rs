//! Tree species (docs/TREE_GROWTH.md §11, ADR 0079): oak, pine and palm as data
//! (`shared/trees/*.json`, emitted as `species.gen.rs`), and the tree builder that grows one —
//! a pure function of (species, seed, stage), ported from the Tree Growth prototype's
//! `growTree` (docs/prototypes/tree-growth.html) onto the ½ m grid: 1 m wide, ½ m tall cells.
//!
//! The engine is a fixed vocabulary: trunks (straight, or a palm's lean in whole-cell steps one
//! axis at a time), branches (`heavy`, `whorl`, `none`) and crowns (`lumpy`, `cone`, `fronds`).
//! A tree comes out as a [`Structure`], so placements (`placements.json`) stand species as they
//! stand committed structures: a placement naming a species places `growth.variants` seeded
//! trees of it, each at a stage drawn by `growth.wild`.
//!
//! Face-local: +y up, the ground below the anchor; the structure is stood on face B through the
//! placements' `FaceFrame` like any structure.

use dwell_blocks::{StateId, parse_state};
use dwell_world::structure::Structure;
use dwell_world::tree_tint::Tint;
use std::collections::BTreeMap;

mod species_gen {
    include!("species.gen.rs");
}
pub use species_gen::SPECIES;

/// The test fixtures (shared/trees/fixtures): species added by data alone.
#[cfg(test)]
pub(crate) fn species_gen_fixtures() -> &'static [Species] {
    &species_gen::FIXTURES
}

/// Height of a cell (m): the ½ m grid.
const CELL: f32 = 0.5;

/// A tree species, from `shared/trees/<name>.json` (lengths in metres).
#[derive(Clone, Copy, Debug)]
pub struct Species {
    pub name: &'static str,
    pub log: &'static str,
    pub leaf: &'static str,
    pub sapling_height: f32,
    pub mature_height: (f32, f32),
    pub height_step: f32,
    /// The trunk's width (m): sapling, mature, step; drawn by the log's `crossSection.widths`.
    pub width: (f32, f32, f32),
    /// The palm's lean at the top (m), drawn per seed; (0, 0) for none.
    pub lean: (f32, f32),
    pub branches: Branches,
    pub crown: Crown,
    /// The crown's scale over the stages: sapling, mature.
    pub crown_scale: (f32, f32),
    /// The crown's scale per seed.
    pub crown_variety: (f32, f32),
    /// The leaves' lightness factor per seed (a per-tree tint, `dwell_world::tree_tint`).
    pub lightness: (f32, f32),
    /// The leaves' hue shift per seed (turns of the HSL wheel).
    pub hue: (f32, f32),
    pub stages: u32,
    /// A world's days from one stage to the next (growth over time).
    pub days_per_stage: f32,
    /// The weight of each stage among generated trees.
    pub wild: &'static [u32],
    /// How many seeded trees a placement draws of it.
    pub variants: u32,
    /// The item a player plants it from, if any (ADR 0079 S4).
    pub sapling: Option<Sapling>,
}

/// A species' sapling (`sapling` in its data): the block item a player places, the ground it takes
/// on, and the chance a broken leaf of one of its trees drops it.
#[derive(Clone, Copy, Debug)]
pub struct Sapling {
    pub item: &'static str,
    pub plant_on: &'static [&'static str],
    pub drop_chance: f32,
}

impl Sapling {
    /// Whether it takes on ground block `ground` (by name).
    pub fn takes_on(&self, ground: &str) -> bool {
        self.plant_on.contains(&ground)
    }
}

/// The species whose sapling is item (block) `item`: its `SPECIES` index.
pub fn of_sapling(item: &str) -> Option<usize> {
    SPECIES
        .iter()
        .position(|s| s.sapling.is_some_and(|p| p.item == item))
}

#[derive(Clone, Copy, Debug)]
pub enum Branches {
    None,
    Heavy {
        count: (f32, f32),
        from: (f32, f32),
        reach: (f32, f32),
        rise: (f32, f32),
        emerge_at: f32,
    },
    Whorl {
        per_whorl: u32,
        reach: f32,
        emerge_at: f32,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum Crown {
    Lumpy {
        centre: f32,
        radius: [f32; 3],
        lump_radius: (f32, f32),
        lump_height: (f32, f32),
        roughness: f32,
    },
    Cone {
        base: f32,
        top: f32,
        radius: f32,
        taper: f32,
        tier_period: f32,
        tier_upper: f32,
        roughness: f32,
    },
    Fronds {
        fronds: (f32, f32),
        length: (f32, f32),
        lift: f32,
        droop: f32,
    },
}

/// A species by name.
pub fn find(name: &str) -> Option<&'static Species> {
    SPECIES.iter().find(|s| s.name == name)
}

/// A species' index in [`SPECIES`] by name.
pub fn index(name: &str) -> Option<usize> {
    SPECIES.iter().position(|s| s.name == name)
}

/// What a grown tree puts in a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Part {
    /// A log along y (0), x (1) or z (2), its width in ¼ m (4: the full log; only standing
    /// trunk logs are narrower).
    Log(u8, u8),
    Leaf,
}

/// A grown tree: its cells relative to the trunk's foot (x, y in ½ m cells, z), its trunk's
/// height (m) and width (m).
#[derive(Clone, Debug, PartialEq)]
pub struct Grown {
    pub cells: BTreeMap<[i32; 3], Part>,
    pub height: f32,
    pub width: f32,
}

/// mulberry32, as the prototype draws (a sequence from one seed).
struct Rng(u32);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x6D2B_79F5);
        let a = self.0;
        let mut t = (a ^ (a >> 15)).wrapping_mul(1 | a);
        t = (t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t))) ^ t;
        ((t ^ (t >> 14)) as f64 / 4_294_967_296.0) as f32
    }
    fn range(&mut self, (a, b): (f32, f32)) -> f32 {
        a + (b - a) * self.next()
    }
}

fn hash(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^ (h >> 33)
}

/// A per-cell noise in [0, 1), the same at every stage (so a crown grows, not reshuffles).
fn noise(seed: u32, [x, y, z]: [i32; 3]) -> f32 {
    let k = (u64::from(seed) << 32)
        ^ ((x as u64 & 0xffff) << 32).rotate_left(7)
        ^ ((y as u64 & 0xffff) << 16)
        ^ (z as u64 & 0xffff);
    (hash(k) >> 40) as f32 / (1u64 << 24) as f32
}

fn qz(v: f32, step: f32) -> f32 {
    (v / step).round() * step
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// The leaf tint a tree of `species` grown from `seed` takes (ADR 0079 `variety`): drawn as the
/// Tree Growth prototype's `growTree` draws it, from the stream after the crown's scale — the hue
/// shift, then the lightness. The same at every stage. [`Tint::NONE`] for a species whose
/// variety draws none.
pub fn tint(species: &Species, seed: u32) -> Tint {
    let mut vr = Rng(seed.wrapping_mul(104_729).wrapping_add(7));
    vr.range(species.crown_variety);
    let hue = vr.range(species.hue);
    let lightness = vr.range(species.lightness);
    Tint { hue, lightness }
}

/// Grows `species` from `seed` to `stage` (0 = sapling .. stages − 1 = mature): a pure function.
pub fn grow(species: &Species, seed: u32, stage: u32) -> Grown {
    let n = species.stages.max(2);
    let g = stage.min(n - 1) as f32 / (n - 1) as f32;
    let mut r = Rng(seed.wrapping_mul(7919).wrapping_add(13));
    let mature = r.range(species.mature_height);
    let h = species.sapling_height.max(qz(
        lerp(species.sapling_height, mature, g),
        species.height_step,
    ));
    let mut vr = Rng(seed.wrapping_mul(104_729).wrapping_add(7));
    let s = lerp(species.crown_scale.0, species.crown_scale.1, g) * vr.range(species.crown_variety);
    let trunk_cells = (h / CELL).round() as i32;
    let (w0, w1, ws) = species.width;
    let width = qz(lerp(w0, w1, g), ws).clamp(w0, w1);
    let quarters = ((width * 4.0).round() as u8).clamp(1, 4);
    let mut logs: BTreeMap<[i32; 3], Part> = BTreeMap::new();
    let mut leaves: Vec<[i32; 3]> = Vec::new();
    let cell_y = |y: f32| (y / CELL).floor() as i32;
    let centre_y = |yc: i32| (yc as f32 + 0.5) * CELL;
    let mut straight = true;

    match species.crown {
        Crown::Lumpy {
            centre,
            radius,
            lump_radius,
            lump_height,
            roughness,
        } => {
            let cy = h + centre * s;
            let mut blobs = vec![[0.0, 0.0, 0.0, radius[0], radius[1], radius[2]]];
            let mut tips = Vec::new();
            if let Branches::Heavy {
                count,
                from,
                reach,
                rise,
                ..
            } = species.branches
            {
                let k = r.range(count).round() as i32;
                let a0 = r.next() * std::f32::consts::TAU;
                for i in 0..k {
                    let a =
                        a0 + i as f32 * std::f32::consts::TAU / k as f32 + (r.next() - 0.5) * 0.9;
                    let (f, re, ri) = (r.range(from), r.range(reach), r.range(rise));
                    let (lr, lh) = (r.range(lump_radius), r.range(lump_height));
                    tips.push((a, f, re, ri));
                    blobs.push([
                        a.cos() * re,
                        ri - centre,
                        a.sin() * re,
                        lr,
                        lr * 0.75 * lh,
                        lr,
                    ]);
                }
            }
            let reach = (5.0 * s).ceil() as i32 + 1;
            for yc in cell_y(cy - reach as f32).max(1)..=cell_y(cy + reach as f32) {
                for x in -reach..=reach {
                    for z in -reach..=reach {
                        let (ux, uy, uz) = (x as f32, centre_y(yc) - cy, z as f32);
                        let f = blobs
                            .iter()
                            .map(|b| {
                                let dx = (ux - b[0] * s) / (b[3] * s + 0.35);
                                let dy = (uy - b[1] * s) / (b[4] * s + 0.35);
                                let dz = (uz - b[2] * s) / (b[5] * s + 0.35);
                                1.0 - (dx * dx + dy * dy + dz * dz)
                            })
                            .fold(f32::MIN, f32::max);
                        if f + (noise(seed, [x, yc, z]) - 0.5) * roughness > 0.0 {
                            leaves.push([x, yc, z]);
                        }
                    }
                }
            }
            if let Branches::Heavy { emerge_at, .. } = species.branches
                && g >= emerge_at
            {
                for (a, f, re, ri) in tips {
                    let y0 = cell_y((h * f).floor().max(1.0)).min(trunk_cells - 1).max(0);
                    let tx = (a.cos() * re * s).round() as i32;
                    let tz = (a.sin() * re * s).round() as i32;
                    let ty = cell_y(cy + ri * s - centre * s).max(y0);
                    branch(&mut logs, [0, y0, 0], [tx, ty, tz]);
                }
            }
        }
        Crown::Cone {
            base,
            top,
            radius,
            taper,
            tier_period,
            tier_upper,
            roughness,
        } => {
            let base = qz(h * base, 1.0).max(0.5);
            let top = h + top * s.max(0.6);
            let big_r = radius * s;
            for yc in cell_y(base)..cell_y(top).max(cell_y(base) + 1) {
                let y = centre_y(yc);
                let tt = (y - base) / (top - base);
                if !(0.0..=1.0).contains(&tt) {
                    continue;
                }
                let k = (y - base).floor() as i32;
                let wide = k % tier_period as i32 == 0;
                let mut rad =
                    (big_r * (1.0 - tt).powf(taper) + 0.45) * if wide { 1.0 } else { tier_upper };
                if y < h {
                    rad = rad.max(1.05);
                }
                let rr = rad.ceil() as i32 + 1;
                for x in -rr..=rr {
                    for z in -rr..=rr {
                        let d = ((x * x + z * z) as f32).sqrt();
                        if d <= rad + (noise(seed, [x, yc, z]) - 0.5) * roughness * 2.0 {
                            leaves.push([x, yc, z]);
                        }
                    }
                }
                if let Branches::Whorl {
                    per_whorl,
                    reach,
                    emerge_at,
                } = species.branches
                {
                    // The stubs on each tier's wide layer, on its lower ½ m.
                    let first = (y - CELL - base).floor() as i32 != k;
                    if g >= emerge_at && wide && first && rad > 1.4 && y < h - 1.0 {
                        let len = ((rad * reach).floor() as i32).max(1);
                        let rot = (k / tier_period as i32) & 1;
                        for i in 0..per_whorl.min(4) as i32 {
                            let [dx, dz] =
                                [[1, 0], [0, 1], [-1, 0], [0, -1]][((i + rot) % 4) as usize];
                            branch(&mut logs, [0, yc, 0], [dx * len, yc, dz * len]);
                        }
                    }
                }
            }
            let apex = cell_y(top);
            leaves.push([0, apex, 0]);
            leaves.push([0, apex + 1, 0]);
        }
        Crown::Fronds {
            fronds,
            length,
            lift,
            droop,
        } => {
            straight = false;
            let lean = r.range(species.lean) * g.max(0.15);
            let la = r.next() * std::f32::consts::TAU;
            let (mut px, mut pz) = (0, 0);
            for i in 0..trunk_cells {
                let u = (i as f32 + 0.5) / trunk_cells as f32;
                let e = lean * u * u;
                let mut nx = (la.cos() * e).round() as i32;
                let mut nz = (la.sin() * e).round() as i32;
                nx = nx.clamp(px - 1, px + 1);
                nz = nz.clamp(pz - 1, pz + 1);
                // One axis per step: a diagonal step would need a 2 × 2 knee; z waits a segment.
                if nx != px && nz != pz {
                    nz = pz;
                }
                if i > 0 && (nx, nz) != (px, pz) {
                    // The knee: the segment below reaches over, one squared log across.
                    logs.insert([nx, i - 1, nz], Part::Log(if nx != px { 1 } else { 2 }, 4));
                }
                px = nx;
                pz = nz;
                logs.insert([px, i, pz], Part::Log(0, quarters));
            }
            leaves.push([px, trunk_cells, pz]);
            leaves.push([px, trunk_cells + 1, pz]);
            let k = r.range(fronds).round() as i32;
            let origin = [px as f32 + 0.5, h + 0.35, pz as f32 + 0.5];
            for i in 0..k {
                let a = i as f32 / k as f32 * std::f32::consts::TAU + (r.next() - 0.5) * 0.5;
                let l = r.range(length) * s;
                let up = i % 3 == 0;
                let fl = lift * if up { 1.7 } else { 1.0 };
                let fd = droop * if up { 0.45 } else { 1.0 } * (0.85 + r.next() * 0.3);
                for step in 1..=12 {
                    let u = step as f32 / 12.0;
                    let along = l * u * (1.0 - 0.18 * u * u);
                    let p = [
                        origin[0] + a.cos() * along,
                        origin[1] + l * (fl * u - fd * u * u * 0.8),
                        origin[2] + a.sin() * along,
                    ];
                    leaves.push([
                        p[0].floor() as i32,
                        cell_y(p[1]).max(1),
                        p[2].floor() as i32,
                    ]);
                }
            }
        }
    }
    if straight {
        for yc in 0..trunk_cells {
            logs.insert([0, yc, 0], Part::Log(0, quarters));
        }
    }
    let mut cells = logs;
    for c in leaves {
        cells.entry(c).or_insert(Part::Leaf);
    }
    Grown {
        cells,
        height: h,
        width,
    }
}

/// A branch as a stepped path of log cells from `a` to `b` (the trunk cell `a` excluded), each
/// log along the axis of its step (ADR 0072 squares the corners where they join).
fn branch(logs: &mut BTreeMap<[i32; 3], Part>, a: [i32; 3], b: [i32; 3]) {
    let mut p = a;
    for _ in 0..40 {
        if p == b {
            break;
        }
        // Vertical steps count 0.8 against a 1 m step: ½ m cells, the prototype's 1.6 per metre.
        let d = [
            (b[0] - p[0]) as f32,
            (b[1] - p[1]) as f32 * 0.8,
            (b[2] - p[2]) as f32,
        ];
        let ax = (0..3)
            .max_by(|&i, &j| d[i].abs().total_cmp(&d[j].abs()).then(j.cmp(&i)))
            .unwrap_or(0);
        p[ax] += (b[ax] - p[ax]).signum();
        let axis = [1, 0, 2][ax];
        logs.entry(p).or_insert(Part::Log(axis, 4));
    }
}

impl Grown {
    /// The tree as a structure (anchor: the trunk's foot), its blocks resolved by name.
    pub fn structure(&self, species: &Species, hash: u64) -> Result<Structure, String> {
        let state = |n: String| parse_state(&n).map_err(|e| format!("{}: {n}: {e}", species.name));
        let logs: Vec<StateId> = ["y", "x", "z"]
            .iter()
            .map(|a| state(format!("{}[axis={a}]", species.log)))
            .collect::<Result<_, _>>()?;
        // The standing log of each width in ¼ m: the log's state along y whose cross-section is
        // that wide (by the block's data, `crossSection.widths`), else the full log.
        let block = dwell_blocks::find_block(species.log)
            .ok_or_else(|| format!("{}: no block {}", species.name, species.log))?;
        let standing = |q: u8| {
            block
                .states()
                .map(|i| i as StateId)
                .find(|&m| {
                    let i = dwell_blocks::info(m);
                    i.axis == dwell_blocks::Axis::Y
                        && i.cross_section
                            .is_some_and(|c| (c.width * 4.0).round() as u8 == q)
                })
                .unwrap_or(logs[0])
        };
        let leaf = state(species.leaf.to_string())?;
        let (mut lo, mut hi) = ([0; 3], [0; 3]);
        for p in self.cells.keys() {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        let size: [i32; 3] = std::array::from_fn(|i| hi[i] - lo[i] + 1);
        let mut cells = vec![None; size.iter().product::<i32>() as usize];
        for (p, part) in &self.cells {
            let [x, y, z] = std::array::from_fn(|i| p[i] - lo[i]);
            cells[((y * size[2] + z) * size[0] + x) as usize] = Some(match part {
                Part::Log(0, q) => standing(*q),
                Part::Log(a, _) => logs[*a as usize],
                Part::Leaf => leaf,
            });
        }
        Ok(Structure::from_cells(
            species.name,
            hash,
            size,
            [-lo[0], -lo[1], -lo[2]],
            &cells,
        ))
    }
}

/// The trees a placement stands of `species`: `growth.variants` seeds, each at a stage drawn
/// by `growth.wild` (fixed per build, so every chunk and every client agrees).
pub fn wild(species: &Species) -> Vec<(u32, u32)> {
    let total: u32 = species.wild.iter().sum();
    (0..species.variants)
        .map(|seed| {
            let mut pick = (hash(0x7EE5_5EED ^ u64::from(seed)) % u64::from(total.max(1))) as u32;
            let mut stage = 0;
            for (i, &w) in species.wild.iter().enumerate() {
                if pick < w {
                    stage = i as u32;
                    break;
                }
                pick -= w;
            }
            (seed, stage)
        })
        .collect()
}

/// The structures a placement stands of `species` ([`wild`]'s trees, grown).
pub fn structures(species: &Species) -> Result<Vec<Structure>, String> {
    wild(species)
        .into_iter()
        .map(|(seed, stage)| {
            let h = hash(
                species.name.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
                    (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
                }) ^ (u64::from(seed) << 8)
                    ^ u64::from(stage),
            );
            grow(species, seed, stage).structure(species, h)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::species_gen::FIXTURES;
    use super::*;

    fn trunk_cells(t: &Grown) -> i32 {
        (0..)
            .take_while(|&y| {
                t.cells.get(&[0, y, 0]) == Some(&Part::Log(0, 4))
                    || matches!(t.cells.get(&[0, y, 0]), Some(Part::Log(0, _)))
            })
            .count() as i32
    }

    #[test]
    fn the_species_resolve_and_grow_by_name() {
        // face-a-only: a tree is built face-local; the placements stand it on either face.
        for name in ["dwell:oak", "dwell:pine", "dwell:palm"] {
            let s = find(name).unwrap();
            let all = structures(s).unwrap();
            assert_eq!(all.len(), s.variants as usize);
            assert!(all.iter().all(|t| t.size[1] > 1), "{name}");
        }
    }

    #[test]
    fn a_species_added_by_data_alone_grows() {
        // face-a-only: a tree is built face-local.
        let shrub = &FIXTURES[0];
        assert_eq!(shrub.name, "test:shrub");
        let t = grow(shrub, 1, shrub.stages - 1);
        assert!(t.cells.values().any(|p| *p == Part::Leaf));
        assert!(trunk_cells(&t) >= 2);
        shrub.stages.checked_sub(1).unwrap();
        assert_eq!(structures(shrub).unwrap().len(), 2);
        // Its sapling, by data alone (ADR 0079 S4): it takes on stone only, and its stage 0 is a
        // tree to plant.
        let sapling = shrub.sapling.expect("the fixture names a sapling");
        assert_eq!(sapling.item, "dwell:tall_grass");
        assert!(sapling.takes_on("dwell:stone") && !sapling.takes_on("dwell:grass"));
        assert_eq!(sapling.drop_chance, 1.0);
        assert!(trunk_cells(&grow(shrub, 9, 0)) >= 1);
    }

    #[test]
    fn every_species_names_its_sapling() {
        // face-a-only: names only.
        // The old-growth species (WORK_QUEUE item 32), the rainforest's tall broadleaf and the mesa's
        // sagebrush (0.64) are wild only: no sapling of their own yet.
        let wild: Vec<_> = SPECIES
            .iter()
            .filter(|s| s.sapling.is_none())
            .map(|s| s.name)
            .collect();
        assert_eq!(
            wild,
            [
                "dwell:old_oak",
                "dwell:old_pine",
                "dwell:sagebrush",
                "dwell:tall_broadleaf"
            ]
        );
        for (i, s) in SPECIES.iter().enumerate() {
            let Some(p) = s.sapling else { continue };
            assert_eq!(of_sapling(p.item), Some(i), "{}", s.name);
            assert!(
                dwell_blocks::find_block(p.item).is_some(),
                "{} is a block",
                p.item
            );
            for g in p.plant_on {
                assert!(dwell_blocks::find_block(g).is_some(), "{g} is a block");
            }
        }
        assert_eq!(of_sapling("dwell:stone"), None);
    }

    #[test]
    fn seeds_draw_different_leaf_tints_within_the_species_variety() {
        // face-a-only: a tint is a colour factor; the meshers apply it on either face.
        let within = |v: f32, (a, b): (f32, f32)| v >= a.min(b) - 1e-6 && v <= a.max(b) + 1e-6;
        let tinted = FIXTURES.iter().find(|s| s.name == "test:tinted").unwrap();
        for s in SPECIES.iter().chain([tinted]) {
            let tints: Vec<Tint> = (0..s.variants).map(|seed| tint(s, seed)).collect();
            for t in &tints {
                assert!(within(t.hue, s.hue), "{}: hue {t:?}", s.name);
                assert!(
                    within(t.lightness, s.lightness),
                    "{}: lightness {t:?}",
                    s.name
                );
            }
            assert_ne!(tints[0], tints[1], "{}: two seeds, two tints", s.name);
            assert_eq!(
                tint(s, 3),
                tint(s, 3),
                "{}: a pure function of the seed",
                s.name
            );
        }
        // The fixture's ranges, by data alone: far from every species'.
        let t = tint(tinted, 1);
        assert!(
            (0.5..=0.6).contains(&t.lightness) && (0.2..=0.25).contains(&t.hue),
            "{t:?}"
        );
        // A variety that draws nothing tints nothing.
        let shrub = FIXTURES.iter().find(|s| s.name == "test:shrub").unwrap();
        assert!((0..4).all(|seed| tint(shrub, seed).is_none()));
    }

    #[test]
    fn a_tree_is_a_pure_function_of_species_seed_and_stage() {
        // face-a-only: a tree is built face-local.
        for s in &SPECIES {
            for stage in [0, 3, s.stages - 1] {
                assert_eq!(grow(s, 5, stage), grow(s, 5, stage), "{}", s.name);
            }
            assert_ne!(
                grow(s, 5, s.stages - 1),
                grow(s, 6, s.stages - 1),
                "{}",
                s.name
            );
        }
    }

    #[test]
    fn seeds_draw_mature_heights_inside_the_species_range() {
        // face-a-only: a tree is built face-local.
        for s in &SPECIES {
            let mut heights = Vec::new();
            for seed in 0..40 {
                let t = grow(s, seed, s.stages - 1);
                let (lo, hi) = s.mature_height;
                assert!(
                    t.height >= lo - s.height_step && t.height <= hi + s.height_step,
                    "{} seed {seed}: {}",
                    s.name,
                    t.height
                );
                if !matches!(s.crown, Crown::Fronds { .. }) {
                    // A branch may rise from the trunk's top, so the column is at least the trunk.
                    assert!(
                        trunk_cells(&t) >= (t.height / CELL).round() as i32,
                        "{}",
                        s.name
                    );
                }
                heights.push((t.height * 2.0) as i32);
            }
            heights.sort_unstable();
            heights.dedup();
            // At least four heights, or every step the range holds (a shrub's is short).
            let (lo, hi) = s.mature_height;
            let steps = ((hi - lo) / s.height_step).round() as usize + 1;
            assert!(
                heights.len() >= 4.min(steps),
                "{}: seeds vary the height: {heights:?}",
                s.name
            );
        }
    }

    #[test]
    fn a_tree_grows_taller_and_wider_stage_by_stage() {
        // face-a-only: a tree is built face-local.
        let span = |t: &Grown| {
            let ys = t.cells.keys().map(|p| p[1]);
            let xs = t.cells.keys().map(|p| p[0].abs().max(p[2].abs()));
            (ys.max().unwrap(), xs.max().unwrap(), t.cells.len())
        };
        for s in &SPECIES {
            for seed in 0..8 {
                let mut last = (0, 0, 0);
                for stage in 0..s.stages {
                    let t = grow(s, seed, stage);
                    let now = span(&t);
                    assert!(
                        now.0 >= last.0,
                        "{} seed {seed} stage {stage}: top {now:?} < {last:?}",
                        s.name
                    );
                    assert!(
                        now.1 + 1 >= last.1,
                        "{} seed {seed} stage {stage}: spread {now:?} < {last:?}",
                        s.name
                    );
                    assert!(
                        now.2 + now.2 / 5 >= last.2,
                        "{} seed {seed} stage {stage}: cells {now:?} < {last:?}",
                        s.name
                    );
                    last = now;
                }
                let (young, old) = (grow(s, seed, 1), grow(s, seed, s.stages - 1));
                assert!(
                    old.height > young.height && old.cells.len() > young.cells.len() * 2,
                    "{}: young {} m {} cells, old {} m {} cells",
                    s.name,
                    young.height,
                    young.cells.len(),
                    old.height,
                    old.cells.len()
                );
            }
        }
    }

    #[test]
    fn a_palm_leans_one_axis_a_step_without_two_by_two_knees() {
        // face-a-only: a tree is built face-local.
        let palm = find("dwell:palm").unwrap();
        let mut leaned = 0;
        for seed in 0..40 {
            let t = grow(palm, seed, palm.stages - 1);
            let logs: Vec<[i32; 3]> = t
                .cells
                .iter()
                .filter(|(_, p)| matches!(p, Part::Log(..)))
                .map(|(c, _)| *c)
                .collect();
            for y in 0..(t.height / CELL).round() as i32 {
                let layer: Vec<_> = logs.iter().filter(|c| c[1] == y).collect();
                assert!(
                    !layer.is_empty() && layer.len() <= 2,
                    "seed {seed} layer {y}: {layer:?}"
                );
                if let [a, b] = layer[..] {
                    assert_eq!(
                        (a[0] - b[0]).abs() + (a[2] - b[2]).abs(),
                        1,
                        "seed {seed}: a knee is one step on one axis"
                    );
                }
            }
            // Each vertical log touches the one above it by a face (a path of logs to the ground).
            for c in logs
                .iter()
                .filter(|c| matches!(t.cells[*c], Part::Log(0, _)))
            {
                if c[1] == 0 {
                    continue;
                }
                let below = logs
                    .iter()
                    .any(|d| d[1] == c[1] - 1 && d[0] == c[0] && d[2] == c[2]);
                assert!(below, "seed {seed}: {c:?} floats");
            }
            if logs.iter().any(|c| c[0] != 0 || c[2] != 0) {
                leaned += 1;
            }
        }
        assert!(leaned > 20, "most mature palms lean: {leaned}/40");
    }

    #[test]
    fn generated_trees_are_mostly_mature() {
        // face-a-only: no geometry.
        for s in &SPECIES {
            let stages: Vec<u32> = wild(s).into_iter().map(|(_, st)| st).collect();
            let mature = stages.iter().filter(|&&st| st + 2 >= s.stages).count();
            assert!(mature * 2 >= stages.len(), "{}: {stages:?}", s.name);
            assert!(
                stages.iter().any(|&st| st + 2 < s.stages),
                "{}: some young: {stages:?}",
                s.name
            );
        }
    }

    #[test]
    fn a_trunk_thickens_stage_by_stage_on_the_log_widths() {
        // face-a-only: a tree is built face-local.
        let log = dwell_blocks::find_block("dwell:log").unwrap();
        for s in &SPECIES {
            let quarters = |stage| {
                grow(s, 3, stage)
                    .cells
                    .get(&[0, 0, 0])
                    .map(|p| match p {
                        Part::Log(_, q) => *q,
                        Part::Leaf => 0,
                    })
                    .unwrap()
            };
            let (young, old) = (quarters(0), quarters(s.stages - 1));
            assert_eq!(
                f32::from(young) / 4.0,
                s.width.0,
                "{}: a sapling is thin",
                s.name
            );
            assert_eq!(f32::from(old) / 4.0, s.width.1, "{}", s.name);
            for st in 1..s.stages {
                assert!(quarters(st) >= quarters(st - 1), "{}", s.name);
            }
            // The structure stands the narrow log state at the foot.
            let t = structures(s).unwrap();
            let _ = t;
            let g = grow(s, 3, 0).structure(s, 1).unwrap();
            let foot = g.place([0, 0, 0], 0).at([0, 0, 0]).unwrap();
            assert!(log.states().contains(&usize::from(foot)), "{}", s.name);
            let w = dwell_blocks::info(foot).cross_section.unwrap().width;
            assert_eq!(w, s.width.0, "{}: the foot is the narrow log", s.name);
        }
    }
}
