//! Trees growing over time (docs/TREE_GROWTH.md §11, ADR 0079 S3): a generated species tree gets a
//! record, keyed by its anchor, of its species, seed, stage and the world day it reached that stage;
//! the growers step advances it a stage every `growth.daysPerStage` world days, writing the
//! difference between its stages' cells (`species::grow`) only where the world still holds what the
//! older stage put there (or air where it put nothing): a player's edit wins, and a felled tree
//! (its foot log gone) stops growing for good.
//!
//! Face-local inside: a tree is grown face-local and crossed into the world through its record's
//! `FaceFrame` (`cell`, `state`), so face B's trees grow as face A's mirrored.

use std::collections::BTreeMap;
use std::sync::Arc;

use dwell_blocks::StateId;
use dwell_world::structure::{Orientation, Placed};
use dwell_world::{CHUNK_SIZE, ChunkCoord, Face, FaceFrame};

use crate::bifacial::BifacialTerrain;
use crate::species::{self, SPECIES};

/// One tree's growth record.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    /// Its anchor (world cell): the cell above its ground, its trunk's foot.
    pub anchor: [i32; 3],
    pub face: Face,
    /// The species (`SPECIES` index) and seed it grows from.
    pub species: u16,
    pub seed: u32,
    /// The stage it stands at, and the world day it reached it (generated trees: day 0).
    pub stage: u8,
    pub since: f64,
    /// Its turn, and its planter and variant (the variant's block swaps, snowy leaves).
    pub turns: u8,
    pub planter: u8,
    pub variant: u8,
    /// Felled (its foot log gone): it grows no more.
    pub stopped: bool,
}

impl Record {
    /// The stage it should stand at on world day `day`: a stage every `daysPerStage` days since
    /// it reached its own, up to the species' last.
    pub fn due(&self, day: f64) -> u8 {
        let Some(s) = SPECIES.get(usize::from(self.species)) else {
            return self.stage;
        };
        let per = f64::from(s.days_per_stage).max(1e-6);
        let n = ((day - self.since) / per).floor().max(0.0);
        let last = s.stages.saturating_sub(1) as f64;
        (f64::from(self.stage) + n).min(last) as u8
    }
}

/// Every tree's record, by anchor.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Records {
    pub trees: BTreeMap<[i32; 3], Record>,
}

const VERSION: u8 = 1;

impl Records {
    /// The saved form (the world's optional `tree_growth` table; ADR 0079): a version byte, then per
    /// tree its anchor, face, species by name, seed, stage, since, turns, planter, variant, stopped.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = vec![VERSION];
        out.extend((self.trees.len() as u32).to_le_bytes());
        for r in self.trees.values() {
            for v in r.anchor {
                out.extend(v.to_le_bytes());
            }
            out.push(u8::from(r.face == Face::B));
            let name = SPECIES
                .get(usize::from(r.species))
                .map_or("", |s| s.name)
                .as_bytes();
            out.push(name.len() as u8);
            out.extend(name);
            out.extend(r.seed.to_le_bytes());
            out.push(r.stage);
            out.extend(r.since.to_le_bytes());
            out.extend([r.turns, r.planter, r.variant, u8::from(r.stopped)]);
        }
        out
    }

    /// Reads the saved form; `None` for one this build can't read (a newer version: kept unread).
    /// A tree of a species this build doesn't have is dropped.
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        let mut at = 0usize;
        let mut take = |n: usize| -> Option<&[u8]> {
            let s = b.get(at..at + n)?;
            at += n;
            Some(s)
        };
        if take(1)?[0] != VERSION {
            return None;
        }
        let n = u32::from_le_bytes(take(4)?.try_into().ok()?);
        let mut trees = BTreeMap::new();
        for _ in 0..n {
            let mut anchor = [0i32; 3];
            for v in &mut anchor {
                *v = i32::from_le_bytes(take(4)?.try_into().ok()?);
            }
            let face = if take(1)?[0] == 1 { Face::B } else { Face::A };
            let len = usize::from(take(1)?[0]);
            let name = std::str::from_utf8(take(len)?).ok()?.to_owned();
            let seed = u32::from_le_bytes(take(4)?.try_into().ok()?);
            let stage = take(1)?[0];
            let since = f64::from_le_bytes(take(8)?.try_into().ok()?);
            let t = take(4)?;
            let (turns, planter, variant, stopped) = (t[0], t[1], t[2], t[3] == 1);
            if let Some(species) = species::index(&name) {
                trees.insert(
                    anchor,
                    Record {
                        anchor,
                        face,
                        species: species as u16,
                        seed,
                        stage,
                        since,
                        turns,
                        planter,
                        variant,
                        stopped,
                    },
                );
            }
        }
        Some(Self { trees })
    }
}

/// The planter byte of a tree a player planted from a sapling (ADR 0079 S4): no placement's planter,
/// so no variant's block swaps.
pub const PLANTED: u8 = u8::MAX;

/// A planted sapling's seed: a hash of its anchor (world cell), so a sapling planted at a place
/// always grows the same tree there.
pub fn seed_at(anchor: [i32; 3]) -> u32 {
    let mut h = 0x9E37_79B9_7F4A_7C15u64;
    for v in anchor {
        h ^= u64::from(v as u32);
        h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h ^= h >> 31;
    }
    (h ^ (h >> 32)) as u32
}

/// Why a sapling is not planted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The block placed is no species' sapling.
    NotASapling,
    /// The ground under it is not one of its species' `plantOn` blocks.
    WrongGround,
}

/// The record of species sapling `item` planted at world cell `anchor` (the cell above its ground)
/// on world day `day`, the ground under it block `ground` (by name): stage 0, its seed from the
/// position, its turn from the seed. Refused on ground its species doesn't take on.
pub fn plant(item: &str, anchor: [i32; 3], ground: &str, day: f64) -> Result<Record, Refused> {
    let species = species::of_sapling(item).ok_or(Refused::NotASapling)?;
    let sapling = SPECIES[species].sapling.ok_or(Refused::NotASapling)?;
    if !sapling.takes_on(ground) {
        return Err(Refused::WrongGround);
    }
    let seed = seed_at(anchor);
    Ok(Record {
        anchor,
        face: FaceFrame::at_row(anchor[1]).face,
        species: species as u16,
        seed,
        stage: 0,
        since: day,
        turns: (seed >> 29) as u8 & 3,
        planter: PLANTED,
        variant: 0,
        stopped: false,
    })
}

/// How far (cells) from a tree's anchor its leaves may reach, sideways and up: the bound
/// [`tree_of_leaf`] looks within.
const LEAF_REACH: i32 = 24;
const LEAF_RISE: i32 = 64;

/// The recorded tree whose current stage put a leaf at world cell `cell` (its species' leaf block),
/// if any: whose broken leaves drop its species' sapling (ADR 0079 S4).
pub fn tree_of_leaf(records: &Records, cell: [i32; 3]) -> Option<&Record> {
    let f = FaceFrame::at_row(cell[1]);
    let local = f.cell(cell);
    records.trees.values().find(|r| {
        if r.face != f.face || r.stopped {
            return false;
        }
        let a = f.cell(r.anchor);
        let (dx, dy, dz) = (local[0] - a[0], local[1] - a[1], local[2] - a[2]);
        if dx.abs() > LEAF_REACH || dz.abs() > LEAF_REACH || !(0..=LEAF_RISE).contains(&dy) {
            return false;
        }
        let Some(s) = SPECIES.get(usize::from(r.species)) else {
            return false;
        };
        placed(r, r.stage, &[])
            .and_then(|p| p.at(local))
            .is_some_and(|m| dwell_blocks::block_of(m).name == s.leaf)
    })
}

/// The sapling a broken leaf of tree `r` drops on a `roll` in [0, 1): its species' `sapling.item`
/// when the roll is under the species' `sapling.dropChance` (ADR 0079 S4).
pub fn sapling_drop(r: &Record, roll: f64) -> Option<&'static str> {
    let p = SPECIES.get(usize::from(r.species))?.sapling?;
    (roll < f64::from(p.drop_chance)).then_some(p.item)
}

/// What one growth step does: the cells to write (world cells), and whether the tree is found felled.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Step {
    pub writes: Vec<([i32; 3], StateId)>,
    pub stopped: bool,
    /// The world chunks its stages' boxes reach (all must be loaded to grow).
    pub chunks: Vec<ChunkCoord>,
}

/// A tree's stage placed face-local at its local anchor, its variant's swaps made.
fn placed(r: &Record, stage: u8, swaps: &[(StateId, StateId)]) -> Option<Placed> {
    let s = SPECIES.get(usize::from(r.species))?;
    let structure = species::grow(s, r.seed, u32::from(stage))
        .structure(s, 0)
        .ok()?;
    let f = FaceFrame { face: r.face };
    let mut p = structure.place_oriented(
        f.cell(r.anchor),
        Orientation {
            quarter_turns: r.turns,
            mirror: None,
        },
        0,
    );
    for &(a, b) in swaps {
        p.replace(a, b);
    }
    Some(p)
}

/// The cells (world cells and states) of the record's tree at `stage`, no variant (tests, tools).
pub fn cells_at(r: &Record, stage: u8) -> Vec<([i32; 3], StateId)> {
    let f = FaceFrame { face: r.face };
    placed(r, stage, &[]).map_or(Vec::new(), |p| {
        p.cells().map(|(c, m)| (f.cell(c), f.state(m))).collect()
    })
}

/// The step from the record's stage to `target`: the writes, reading the world with `read` (world
/// cells). A pure function of the record, the target and the world it reads.
pub fn step(
    r: &Record,
    target: u8,
    swaps: &[(StateId, StateId)],
    read: &dyn Fn([i32; 3]) -> StateId,
) -> Step {
    let mut out = Step::default();
    if r.stopped || target <= r.stage {
        return out;
    }
    let (Some(old), Some(new)) = (placed(r, r.stage, swaps), placed(r, target, swaps)) else {
        return out;
    };
    let f = FaceFrame { face: r.face };
    let local = f.cell(r.anchor);
    // Felled: the foot is no longer the trunk it was.
    if old.at(local).map(|m| f.state(m)) != Some(read(r.anchor)) {
        out.stopped = true;
        return out;
    }
    let lo: [i32; 3] = std::array::from_fn(|i| old.min[i].min(new.min[i]));
    let hi: [i32; 3] = std::array::from_fn(|i| old.max[i].max(new.max[i]));
    let (ya, yb) = (f.row(lo[1]), f.row(hi[1]));
    let (a, b) = (
        dwell_world::chunk_of(lo[0], ya.min(yb), lo[2]),
        dwell_world::chunk_of(hi[0], ya.max(yb), hi[2]),
    );
    for cy in a.y..=b.y {
        for cz in a.z..=b.z {
            for cx in a.x..=b.x {
                out.chunks.push(ChunkCoord::new(cx, cy, cz));
            }
        }
    }
    let air = dwell_world::M.air;
    for y in lo[1]..=hi[1] {
        for z in lo[2]..=hi[2] {
            for x in lo[0]..=hi[0] {
                let p = [x, y, z];
                let (was, now) = (old.at(p), new.at(p));
                if was == now {
                    continue;
                }
                let w = f.cell(p);
                let expect = was.map_or(air, |m| f.state(m));
                if read(w) != expect {
                    continue; // edited (or grown into): the world's wins
                }
                out.writes.push((w, now.map_or(air, |m| f.state(m))));
            }
        }
    }
    out
}

/// The growers of a world's generator: finds its species trees and their block swaps.
pub struct Growers {
    terrain: Arc<BifacialTerrain>,
}

impl Growers {
    /// The growers of a generator version and seed, if its world places any species tree.
    pub fn for_generator(version: u32, world_seed: u64) -> Option<Arc<Self>> {
        let terrain = BifacialTerrain::for_generator(version, world_seed)?;
        Self::of(terrain)
    }

    /// The growers of a terrain, if either face has an active planter of species trees.
    pub fn of(terrain: BifacialTerrain) -> Option<Arc<Self>> {
        let any = [Face::A, Face::B].into_iter().any(|f| {
            terrain
                .local(f)
                .planters()
                .iter()
                .any(|p| p.active() && p.grows())
        });
        any.then(|| {
            Arc::new(Self {
                terrain: Arc::new(terrain),
            })
        })
    }

    /// The species trees anchored in world chunk `c`, as new records (day 0).
    pub fn trees_in_chunk(&self, c: ChunkCoord) -> Vec<Record> {
        let f = FaceFrame::at_chunk_y(c.y);
        let local = f.chunk(c);
        let g = self.terrain.local(f.face);
        let (x0, z0) = (c.x * CHUNK_SIZE, c.z * CHUNK_SIZE);
        let (y0, y1) = (local.y * CHUNK_SIZE, local.y * CHUNK_SIZE + CHUNK_SIZE);
        let mut out = Vec::new();
        for t in g.trees_in((x0, z0), (x0 + CHUNK_SIZE - 1, z0 + CHUNK_SIZE - 1)) {
            let a = t.anchor;
            let inside = (x0..x0 + CHUNK_SIZE).contains(&a[0])
                && (z0..z0 + CHUNK_SIZE).contains(&a[2])
                && (y0..y1).contains(&a[1]);
            let planter = &g.planters()[usize::from(t.planter)];
            if let (true, Some((species, seed, stage))) = (inside, planter.grown(&t)) {
                out.push(Record {
                    anchor: f.cell(a),
                    face: f.face,
                    species,
                    seed,
                    stage: stage as u8,
                    since: 0.0,
                    turns: t.quarter_turns,
                    planter: t.planter,
                    variant: t.variant,
                    stopped: false,
                });
            }
        }
        out
    }

    /// The block swaps of a record's variant.
    pub fn swaps(&self, r: &Record) -> Vec<(StateId, StateId)> {
        self.terrain
            .local(r.face)
            .planters()
            .get(usize::from(r.planter))
            .map_or(Vec::new(), |p| p.swaps(r.variant).to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// An oak's record on face `f`.
    fn oak(f: FaceFrame, stage: u8) -> Record {
        Record {
            anchor: f.cell([3, 200, -5]),
            face: f.face,
            species: species::index("dwell:oak").unwrap() as u16,
            seed: 7,
            stage,
            since: 0.0,
            turns: 1,
            planter: 0,
            variant: 0,
            stopped: false,
        }
    }

    /// A world of the tree at `r`'s stage (world cells), air elsewhere.
    fn world_of(r: &Record) -> HashMap<[i32; 3], StateId> {
        let p = placed(r, r.stage, &[]).unwrap();
        let f = FaceFrame { face: r.face };
        p.cells().map(|(c, m)| (f.cell(c), f.state(m))).collect()
    }

    fn grow_to(world: &mut HashMap<[i32; 3], StateId>, r: &mut Record, target: u8) -> Step {
        let s = {
            let w = &*world;
            step(r, target, &[], &|c| {
                *w.get(&c).unwrap_or(&dwell_world::M.air)
            })
        };
        for (c, m) in &s.writes {
            if *m == dwell_world::M.air {
                world.remove(c);
            } else {
                world.insert(*c, *m);
            }
        }
        if s.stopped {
            r.stopped = true;
        } else {
            r.stage = r.stage.max(target);
        }
        s
    }

    #[test]
    fn a_tree_is_due_a_stage_every_days_per_stage() {
        // face-a-only: no geometry, only the clock.
        let r = oak(FaceFrame::A, 2);
        let per = f64::from(SPECIES[usize::from(r.species)].days_per_stage);
        assert_eq!(r.due(0.0), 2);
        assert_eq!(r.due(per * 0.99), 2);
        assert_eq!(r.due(per), 3);
        assert_eq!(r.due(per * 3.0 + 0.1), 5);
        assert_eq!(r.due(per * 100.0), 7, "capped at the last stage");
    }

    dwell_world::both_faces!(
        fn a_tree_grown_n_stages_is_that_stages_tree(f: FaceFrame) {
            let mut r = oak(f, 2);
            let mut world = world_of(&r);
            let s = grow_to(&mut world, &mut r, 5);
            assert!(!s.stopped && !s.writes.is_empty());
            assert_eq!(world, world_of(&oak(f, 5)), "stage 2 grown to 5 is stage 5");
            assert!(!s.chunks.is_empty());
            assert!(
                grow_to(&mut world, &mut r, 5).writes.is_empty(),
                "nothing more due"
            );
        }
    );

    dwell_world::both_faces!(
        fn a_players_edit_is_not_grown_over(f: FaceFrame) {
            let mut r = oak(f, 2);
            let mut world = world_of(&r);
            let five = world_of(&oak(f, 5));
            let stone = dwell_blocks::require("dwell:stone");
            // A cell stage 5 fills that stage 2 left empty: a player built there.
            let built = *five.keys().find(|c| !world.contains_key(*c)).unwrap();
            world.insert(built, stone);
            // A stage-2 leaf a player broke, which stage 5 also has: it stays broken.
            let leaf = dwell_blocks::require("dwell:leaves");
            let broken = *world
                .iter()
                .find(|(c, m)| **m == leaf && five.get(*c) == Some(&leaf))
                .unwrap()
                .0;
            world.remove(&broken);
            grow_to(&mut world, &mut r, 5);
            assert_eq!(world.get(&built), Some(&stone));
            assert_eq!(world.get(&broken), None);
        }
    );

    dwell_world::both_faces!(
        fn a_felled_tree_grows_no_more(f: FaceFrame) {
            let mut r = oak(f, 2);
            let mut world = world_of(&r);
            world.remove(&r.anchor);
            let s = grow_to(&mut world, &mut r, 5);
            assert!(s.stopped && s.writes.is_empty());
            assert!(r.stopped);
            assert!(step(&r, 7, &[], &|_| dwell_world::M.air).writes.is_empty());
        }
    );

    #[test]
    fn records_round_trip_their_saved_form() {
        // both-faces: one record per face in one table.
        let mut rs = Records::default();
        for f in [FaceFrame::A, FaceFrame::B] {
            let mut r = oak(f, 3);
            r.since = 12.5;
            r.variant = 1;
            rs.trees.insert(r.anchor, r);
        }
        let b = rs.to_bytes();
        assert_eq!(Records::from_bytes(&b), Some(rs));
        let mut newer = b.clone();
        newer[0] = 99;
        assert_eq!(
            Records::from_bytes(&newer),
            None,
            "a newer version is left unread"
        );
        assert_eq!(Records::from_bytes(&b[..b.len() - 2]), None);
    }

    /// The ground name and anchor of a sapling planted on face `f` at local (x, z).
    fn planted(f: FaceFrame, item: &str, ground: &str) -> Result<Record, Refused> {
        plant(item, f.cell([4, 120, -2]), ground, 1.5)
    }

    dwell_world::both_faces!(
        fn a_planted_sapling_is_a_stage_0_record_that_grows_by_days(f: FaceFrame) {
            for (item, ground) in [
                ("dwell:oak_sapling", "dwell:grass"),
                ("dwell:pine_sapling", "dwell:dirt"),
                ("dwell:palm_sapling", "dwell:sand"),
            ] {
                let mut r = planted(f, item, ground).expect(item);
                let s = &SPECIES[usize::from(r.species)];
                assert_eq!(s.sapling.unwrap().item, item);
                assert_eq!((r.stage, r.since, r.face), (0, 1.5, f.face));
                assert_eq!(r.planter, PLANTED);
                assert_eq!(r.seed, seed_at(r.anchor), "the seed is the position's");
                // Its stage 0 stands its trunk's foot at the anchor.
                let zero = world_of(&r);
                assert!(
                    zero.contains_key(&r.anchor),
                    "{item}: the foot at the anchor"
                );
                let per = f64::from(s.days_per_stage);
                for n in 1..s.stages as u8 {
                    assert_eq!(r.due(1.5 + per * f64::from(n) + 1e-6), n);
                }
                // Grown n stages, it is stage n's tree.
                let mut world = zero;
                let n = 3;
                grow_to(&mut world, &mut r, n);
                let mut want = r.clone();
                want.stage = n;
                assert_eq!(world, world_of(&want), "{item}: stage {n}");
            }
        }
    );

    dwell_world::both_faces!(
        fn a_sapling_on_the_wrong_ground_is_refused(f: FaceFrame) {
            assert_eq!(
                planted(f, "dwell:palm_sapling", "dwell:grass"),
                Err(Refused::WrongGround)
            );
            assert_eq!(
                planted(f, "dwell:oak_sapling", "dwell:sand"),
                Err(Refused::WrongGround)
            );
            assert_eq!(
                planted(f, "dwell:oak_sapling", "dwell:stone"),
                Err(Refused::WrongGround)
            );
            assert_eq!(
                planted(f, "dwell:stone", "dwell:grass"),
                Err(Refused::NotASapling)
            );
        }
    );

    dwell_world::both_faces!(
        fn a_leaf_is_found_to_be_its_trees(f: FaceFrame) {
            let r = oak(f, 5);
            let mut rs = Records::default();
            rs.trees.insert(r.anchor, r.clone());
            let leaf = dwell_blocks::require("dwell:leaves");
            let w = world_of(&r);
            let at = *w.iter().find(|(_, m)| **m == f.state(leaf)).unwrap().0;
            assert_eq!(tree_of_leaf(&rs, at).map(|t| t.anchor), Some(r.anchor));
            // Its trunk is no leaf; a cell beside the tree is no one's.
            assert!(tree_of_leaf(&rs, r.anchor).is_none());
            let away = f.cell({
                let a = f.cell(r.anchor);
                [a[0] + 40, a[1], a[2]]
            });
            assert!(tree_of_leaf(&rs, away).is_none());
        }
    );

    #[test]
    fn a_planted_record_round_trips_its_saved_form() {
        // both-faces: one planted record per face in one table.
        let mut rs = Records::default();
        for f in [FaceFrame::A, FaceFrame::B] {
            let mut r = planted(f, "dwell:palm_sapling", "dwell:sand").unwrap();
            r.stage = 2;
            rs.trees.insert(r.anchor, r);
        }
        assert_eq!(Records::from_bytes(&rs.to_bytes()), Some(rs));
    }
}
