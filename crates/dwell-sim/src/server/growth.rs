//! The server's side of trees growing over time (docs/TREE_GROWTH.md §11, ADR 0079 S3): the growers
//! step. Every few seconds it records the species trees of loaded chunks it has not looked at
//! (generated trees: day 0 at their drawn stage), then grows each recorded tree whose stage is due
//! by the world's days (`dwell_worldgen::growth::Record::due`), when every chunk its stages reach is
//! loaded: the writes go out as one modification. The records are saved with the world (an optional
//! table, `tree_growth`, that older builds ignore).

use super::{Server, TickChanges};
use dwell_protocol::{SIM_HZ, VoxelModificationReason};
use dwell_worldgen::growth::{Growers, Records};
use std::sync::Arc;

/// How often the growers step runs (s of the tick).
const EVERY_S: u32 = 5;
/// The most chunks looked at for new trees per step (the rest on the next).
const SCANS_PER_STEP: usize = 256;
/// The most chunks looked at for new trees a tick (~0.25 ms each natively, 4.6 at worst).
const SCANS_PER_TICK: usize = 4;

impl Server {
    /// The world's days so far (its clock's day length), times the growth speed (dev, tests).
    pub fn world_days(&self) -> f64 {
        f64::from(self.tick) / f64::from(SIM_HZ) / self.settings.clock.day_length_s()
            * self.config.growth_speed
    }

    fn growers(&mut self) -> Option<Arc<Growers>> {
        let (version, seed) = (self.config.generator_version, self.config.world_seed);
        let override_ = self.config.generator_override.is_some();
        self.growers
            .get_or_insert_with(|| {
                (!override_)
                    .then(|| Growers::for_generator(version, seed))
                    .flatten()
            })
            .clone()
    }

    /// Tests: the growers in place of the generator's.
    pub fn set_growers(&mut self, g: Option<Arc<Growers>>) {
        self.growers = Some(g);
        self.growth_scanned.clear();
    }

    /// The tree growth records (tests, tools).
    pub fn growth(&self) -> &Records {
        &self.growth
    }

    /// Tests, tools: plants a tree's record (its cells are the caller's).
    pub fn plant_tree(&mut self, r: dwell_worldgen::growth::Record) {
        self.growth.trees.insert(r.anchor, r);
    }

    /// The growers step: a few new chunks' trees recorded every tick (`SCANS_PER_TICK`; BUGS.md
    /// #194: 256 at once, ~60 ms, stalled the game, crossing a portal above all), the due trees
    /// grown every `EVERY_S` s.
    pub(super) fn grow_trees(&mut self) {
        self.scan_tree_chunks(SCANS_PER_TICK);
        if self.tick.is_multiple_of(EVERY_S * SIM_HZ) {
            self.grow_due_trees();
        }
    }

    /// How many chunks have been looked at for trees (tests, tools).
    pub fn tree_chunks_scanned(&self) -> usize {
        self.growth_scanned.len()
    }

    /// Records the trees of up to `n` loaded chunks not looked at yet; how many it looked at.
    pub fn scan_tree_chunks(&mut self, n: usize) -> usize {
        let Some(growers) = self.growers() else {
            return 0;
        };
        let fresh: Vec<_> = self
            .world
            .chunks()
            .map(|(c, _)| *c)
            .filter(|c| !self.growth_scanned.contains(c))
            .take(n)
            .collect();
        for &c in &fresh {
            self.growth_scanned.insert(c);
            for r in growers.trees_in_chunk(c) {
                self.growth.trees.entry(r.anchor).or_insert(r);
            }
        }
        fresh.len()
    }

    /// The growers step now (tests, tools): record new chunks' trees (up to `SCANS_PER_STEP`), grow
    /// the due ones.
    pub fn grow_trees_now(&mut self) {
        self.scan_tree_chunks(SCANS_PER_STEP);
        self.grow_due_trees();
    }

    /// Grows each recorded tree whose stage is due, where its box is loaded.
    fn grow_due_trees(&mut self) {
        let growers = self.growers();
        if self.growth.trees.is_empty() {
            return;
        }
        let day = self.world_days();
        let mut changes = TickChanges::default();
        let anchors: Vec<[i32; 3]> = self
            .growth
            .trees
            .values()
            .filter(|r| !r.stopped && r.due(day) > r.stage)
            .filter(|r| {
                let a = r.anchor;
                self.world
                    .find(dwell_world::chunk_of(a[0], a[1], a[2]))
                    .is_some()
            })
            .map(|r| r.anchor)
            .collect();
        for a in anchors {
            let r = self.growth.trees[&a].clone();
            let target = r.due(day);
            let swaps = growers.as_ref().map_or(Vec::new(), |g| g.swaps(&r));
            let step = {
                // What was generated, where a trunk widens over it (a player's block stops it).
                let world = std::cell::RefCell::new(&mut self.world);
                let cache = std::cell::RefCell::new(Default::default());
                dwell_worldgen::growth::step(
                    &r,
                    target,
                    &swaps,
                    &|c| world.borrow().get_voxel_if_loaded(c),
                    &|c| {
                        world
                            .borrow_mut()
                            .generated_voxel(c, &mut cache.borrow_mut())
                    },
                )
            };
            if step.chunks.iter().any(|&k| self.world.find(k).is_none()) {
                continue; // wait until its whole box is loaded
            }
            let rec = self.growth.trees.get_mut(&a).expect("record");
            if step.stopped {
                rec.stopped = true;
                continue;
            }
            if step.blocked {
                continue; // a player's block where its trunk widens: it waits
            }
            // Its own pace (`species::rate`).
            let s = &dwell_worldgen::species::SPECIES[usize::from(rec.species)];
            let per =
                f64::from(s.days_per_stage) / f64::from(dwell_worldgen::species::rate(s, rec.seed));
            rec.since += f64::from(target - rec.stage) * per;
            rec.stage = target;
            for (c, m) in step.writes {
                changes.set(&mut self.world, c, m);
            }
        }
        if !changes.chunks.is_empty() {
            self.broadcast_changes(changes, VoxelModificationReason::Rebake);
        }
    }
}
