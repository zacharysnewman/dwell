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

/// The most chunks looked at for new trees per step (the rest on the next).
const SCANS_PER_STEP: usize = 256;
/// The most chunks looked at for new trees a tick (~0.25 ms each natively, 4.6 at worst).
const SCANS_PER_TICK: usize = 1;
/// The most due trees grown a tick.
const TREES_PER_TICK: usize = 1;

impl Server {
    /// The world's days so far (its clock's day length), times the growth speed (dev, tests).
    pub fn world_days(&self) -> f64 {
        f64::from(self.tick) / f64::from(SIM_HZ) / self.settings.clock.day_length_s()
            * self.config.growth_speed
    }

    /// The growers, on the grass's terrain (one build, never on a tick in play: `prepare_growth`
    /// builds it at a join); none until the world maps are in.
    fn growers(&mut self) -> Option<Arc<Growers>> {
        if let Some(g) = &self.growers {
            return g.clone();
        }
        if self.config.generator_override.is_some() {
            self.growers = Some(None);
            return None;
        }
        let t = self.grass_terrain()?;
        let g = Growers::of_shared(t);
        self.growers = Some(g.clone());
        g
    }

    /// Builds the growth steps' terrain (the grass's and the growers', one shared), as a player
    /// joins (behind the loading screen) rather than on the first tick that needs it (~0.6 s in
    /// the browser: priority 0, the owner's "the server never drops below 60fps").
    pub(super) fn prepare_growth(&mut self) {
        let _ = self.growers();
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
    /// #194: 256 at once, ~60 ms, stalled the game, crossing a portal above all), and at most
    /// `TREES_PER_TICK` due trees grown a tick (all at once every 5 s, a giant's stage ~10 ms,
    /// made 20–30 ms ticks: priority 0, the owner's "the server never drops below 60fps"); the
    /// due ones found once a second.
    pub(super) fn grow_trees(&mut self) {
        self.scan_tree_chunks(SCANS_PER_TICK);
        if self.growth_due.is_empty() && self.tick.is_multiple_of(SIM_HZ) {
            self.growth_due = self.due_tree_anchors().into();
        }
        // One costly call a tick at most: the next due tree's old stage grown into the cache, else
        // its new one, else (both kept) its step, which is cheap then.
        let Some(&a) = self.growth_due.front() else {
            return;
        };
        if let Some(r) = self.growth.trees.get(&a).cloned() {
            let target = r.due(self.world_days());
            if target > r.stage
                && (dwell_worldgen::growth::warm(&r, r.stage)
                    || dwell_worldgen::growth::warm(&r, target))
            {
                return; // grown one this tick: the step on a later one
            }
        }
        let n = TREES_PER_TICK.min(self.growth_due.len());
        let next: Vec<[i32; 3]> = self.growth_due.drain(..n).collect();
        self.grow_these_trees(&next, true);
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
        let due = self.due_tree_anchors();
        self.grow_these_trees(&due, false);
    }

    /// The recorded trees whose stage is due, where their anchor's chunk is loaded.
    fn due_tree_anchors(&self) -> Vec<[i32; 3]> {
        let day = self.world_days();
        self.growth
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
            .collect()
    }

    /// Grows each of the trees at `anchors` that is still due, where its box is loaded.
    /// `wait`: an edited chunk's pristine copy not at hand, the tree waits for it (the tick's);
    /// else the chunk is regenerated here (tests, tools).
    fn grow_these_trees(&mut self, anchors: &[[i32; 3]], wait: bool) {
        if anchors.is_empty() {
            return;
        }
        let growers = self.growers();
        let day = self.world_days();
        let mut changes = TickChanges::default();
        for &a in anchors {
            let Some(r) = self.growth.trees.get(&a).cloned() else {
                continue;
            };
            if r.stopped || r.due(day) <= r.stage {
                continue;
            }
            let target = r.due(day);
            let swaps = growers.as_ref().map_or(Vec::new(), |g| g.swaps(&r));
            // What was generated, where a trunk widens over it (a player's block stops it): an
            // unedited chunk's own cells, an edited one's from the jobs' pristine copy, never
            // regenerated on the tick (priority 0, BUGS.md #194); one missing, the tree waits.
            let missing = std::cell::RefCell::new(Vec::new());
            let step = {
                let world = std::cell::RefCell::new(&mut self.world);
                let pristine = &self.pristine;
                let cache = std::cell::RefCell::new(Default::default());
                dwell_worldgen::growth::step(
                    &r,
                    target,
                    &swaps,
                    &|c| world.borrow().get_voxel_if_loaded(c),
                    &|c| {
                        let k = dwell_world::chunk_of(c[0], c[1], c[2]);
                        let edited = world.borrow().find(k).is_some_and(|ch| ch.revision() > 0);
                        if !edited {
                            return world.borrow().get_voxel_if_loaded(c);
                        }
                        if let Some(p) = pristine.get(&k) {
                            return p.get(
                                c[0] - k.x * dwell_world::CHUNK_SIZE,
                                c[1] - k.y * dwell_world::CHUNK_SIZE,
                                c[2] - k.z * dwell_world::CHUNK_SIZE,
                            );
                        }
                        if wait {
                            missing.borrow_mut().push(k);
                            return world.borrow().get_voxel_if_loaded(c);
                        }
                        world
                            .borrow_mut()
                            .generated_voxel(c, &mut cache.borrow_mut())
                    },
                )
            };
            if step.chunks.iter().any(|&k| self.world.find(k).is_none()) {
                continue; // wait until its whole box is loaded
            }
            let mut missing = missing.into_inner();
            if !missing.is_empty() {
                missing.dedup();
                for k in missing {
                    if !self.pristine_wanted.contains(&k) && self.pristine_wanted.len() < 32 {
                        self.pristine_wanted.push(k);
                    }
                }
                self.growth_due.push_back(a); // again once its copies are here
                continue;
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
