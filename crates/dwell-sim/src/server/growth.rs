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

    /// The growers step (every `EVERY_S` s).
    pub(super) fn grow_trees(&mut self) {
        if !self.tick.is_multiple_of(EVERY_S * SIM_HZ) {
            return;
        }
        self.grow_trees_now();
    }

    /// The growers step now: record new chunks' trees, grow the due ones.
    pub fn grow_trees_now(&mut self) {
        let growers = self.growers();
        if let Some(growers) = &growers {
            let fresh: Vec<_> = self
                .world
                .chunks()
                .map(|(c, _)| *c)
                .filter(|c| !self.growth_scanned.contains(c))
                .take(SCANS_PER_STEP)
                .collect();
            for c in fresh {
                self.growth_scanned.insert(c);
                for r in growers.trees_in_chunk(c) {
                    self.growth.trees.entry(r.anchor).or_insert(r);
                }
            }
        }
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
                let world = &self.world;
                dwell_worldgen::growth::step(&r, target, &swaps, &|c| {
                    world.get_voxel_if_loaded(c)
                })
            };
            if step.chunks.iter().any(|&k| self.world.find(k).is_none()) {
                continue; // wait until its whole box is loaded
            }
            let rec = self.growth.trees.get_mut(&a).expect("record");
            if step.stopped {
                rec.stopped = true;
                continue;
            }
            let per = f64::from(
                dwell_worldgen::species::SPECIES[usize::from(rec.species)].days_per_stage,
            );
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
