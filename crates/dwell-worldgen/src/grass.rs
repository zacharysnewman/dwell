//! Tall grass growing and spreading over time (ADR 0075 T3c, docs/ECOLOGY.md §10). The rule is data
//! (`shared/worldgen/grass.json`, generated into `grass.gen.rs` by `shared/worldgen/gen.mjs`): once
//! a world day, an empty cell over a grass block takes tall grass when its column is **fertile** and
//! tall grass stands within `reach` columns (`spreadPerDay`), or when it held generated grass that
//! was eaten or cut (`regrowPerDay`: the roots).
//!
//! **The cap is the climate's.** A column is fertile when the decoration's own per-cell roll (the
//! one that decides generated tall grass, `Placer::roll`) is under `maxDensity` × the climate's share
//! there (the T3b undergrowth factor of the vegetation kind governing the decoration; 0 in a biome
//! that doesn't list it; 1 without a climate). Generated tufts roll under `density` × patch × share ≤
//! `maxDensity` × share, so they are always fertile, and a stretch of ground never holds more tall
//! grass than `maxDensity` × share of its open grass cells: lush meadows fill, dry ground stays
//! sparse. Far away the ecology's forage regrows toward its ground's most × the same share
//! ([`far_share`]; `dwell_ecology::Island::scale_grazing`).
//!
//! **A player's block wins.** A cell grows only where it is air now and was air or tall grass as
//! generated, over ground that is still its generated block, with `openAbove` cells of air over it.
//!
//! Face-local: the step reads and writes world cells through a [`FaceFrame`]; rolls hash face-local
//! cells, so face B's meadow grows as face A's mirror.

use std::collections::HashSet;

use dwell_blocks::{AIR, StateId};
use dwell_world::{CHUNK_SIZE, ChunkCoord, FaceFrame};

use crate::noise::{hash3, mix32, seed_word, unit};

/// The growth rule as data (see `gen.mjs` `validateGrass` for the ranges).
#[derive(Clone, Copy, Debug)]
pub struct GrassData {
    /// The block grown, by name.
    pub block: &'static str,
    /// The ground blocks it grows on, by name.
    pub on: &'static [&'static str],
    /// The decoration whose per-cell roll and climate envelope set the cap.
    pub decoration: &'static str,
    /// The most share of a stretch's open grass cells holding it, at the climate's best.
    pub max_density: f64,
    /// The chance a day a fertile empty cell next to tall grass takes it.
    pub spread_per_day: f64,
    /// The chance a day a fertile cell whose generated grass is gone (eaten, cut) takes it again.
    pub regrow_per_day: f64,
    /// Columns (each way) a tuft seeds.
    pub reach: i32,
    /// Cells of air needed over the grown cell (light and air).
    pub open_above: i32,
    /// The most days a chunk catches up at once (on its load, after an idle stretch).
    pub catch_up_days: u32,
}

mod table {
    use super::GrassData;
    include!("grass.gen.rs");
}
pub use table::GRASS;

/// The rows up or down a neighbouring tuft may stand and still seed a cell (½ m rows: 1 m).
const SEED_ROWS: i32 = 2;

/// The envelope at a face-local column: (the decoration's per-cell roll, the climate's share).
pub trait Envelope {
    fn at(&self, x: i32, z: i32) -> (f32, f64);
}

impl<F: Fn(i32, i32) -> (f32, f64)> Envelope for F {
    fn at(&self, x: i32, z: i32) -> (f32, f64) {
        self(x, z)
    }
}

/// The world's cells as the step reads them (world coordinates).
pub trait Cells {
    /// The cell now; None where its chunk isn't loaded.
    fn now(&self, c: [i32; 3]) -> Option<StateId>;
    /// The cell as generated; None where unknown (treated as not natural).
    fn generated(&mut self, c: [i32; 3]) -> Option<StateId>;
}

/// The rule resolved by name for one world.
#[derive(Clone, Debug)]
pub struct Rule {
    pub data: GrassData,
    block: StateId,
    on: Vec<StateId>,
    seed: u32,
}

impl Rule {
    pub fn new(data: GrassData, world_seed: u64) -> Self {
        Self {
            data,
            block: dwell_blocks::require(data.block),
            on: data.on.iter().map(|o| dwell_blocks::require(o)).collect(),
            seed: seed_word(world_seed, 0x6A55_0000),
        }
    }

    /// The build's rule.
    pub fn build(world_seed: u64) -> Self {
        Self::new(GRASS, world_seed)
    }

    /// The block grown.
    pub fn block(&self) -> StateId {
        self.block
    }

    /// The most share of a column's open grass cells holding tall grass at a climate share.
    pub fn cap(&self, share: f64) -> f64 {
        self.data.max_density * share.clamp(0.0, 1.0)
    }

    /// Whether a column of this envelope may hold tall grass.
    pub fn fertile(&self, (roll, share): (f32, f64)) -> bool {
        f64::from(roll) < self.cap(share)
    }

    /// The day's roll for face-local cell `p`.
    fn day_roll(&self, day: u32, [x, y, z]: [i32; 3]) -> f64 {
        f64::from(unit(hash3(
            mix32(self.seed ^ day.wrapping_mul(0x9E37_79B9)),
            x,
            y,
            z,
        )))
    }

    /// The days a chunk last grown on whole day `last` grows now at world day `today`: (the first
    /// day, how many), at most `catchUpDays`.
    pub fn catch_up(&self, last: u32, today: u32) -> (u32, u32) {
        let days = today.saturating_sub(last).min(self.data.catch_up_days);
        (today - days, days)
    }

    /// Grows world chunk `chunk` (on face `f`) through `days` world days from `first_day`: the
    /// cells to write (world cell, state).
    pub fn grow_chunk(
        &self,
        f: FaceFrame,
        chunk: ChunkCoord,
        first_day: u32,
        days: u32,
        env: &dyn Envelope,
        cells: &mut dyn Cells,
    ) -> Vec<([i32; 3], StateId)> {
        if days == 0 || self.data.spread_per_day <= 0.0 && self.data.regrow_per_day <= 0.0 {
            return Vec::new();
        }
        let local = f.chunk(chunk);
        let (x0, y0, z0) = (
            local.x * CHUNK_SIZE,
            local.y * CHUNK_SIZE,
            local.z * CHUNK_SIZE,
        );
        let block = f.state(self.block);
        let now = |cells: &dyn Cells, p: [i32; 3]| cells.now(f.cell(p)).map(|s| f.state(s));
        let is_grass = |cells: &dyn Cells, p: [i32; 3]| now(cells, p) == Some(self.block);
        // The spots: open cells over grass that the world still holds as generated.
        struct Spot {
            p: [i32; 3],
            roots: bool,
            near: bool,
        }
        let mut spots = Vec::new();
        let mut env_cache: Option<((i32, i32), (f32, f64))> = None;
        for z in z0..z0 + CHUNK_SIZE {
            for x in x0..x0 + CHUNK_SIZE {
                for y in y0..y0 + CHUNK_SIZE {
                    let p = [x, y, z];
                    if now(cells, p) != Some(AIR) {
                        continue;
                    }
                    let below = [x, y - 1, z];
                    let Some(ground) = now(cells, below).filter(|g| self.on.contains(g)) else {
                        continue;
                    };
                    if !(1..=self.data.open_above).all(|k| now(cells, [x, y + k, z]) == Some(AIR)) {
                        continue;
                    }
                    let e = match env_cache {
                        Some((at, e)) if at == (x, z) => e,
                        _ => {
                            let e = env.at(x, z);
                            env_cache = Some(((x, z), e));
                            e
                        }
                    };
                    if !self.fertile(e) {
                        continue;
                    }
                    let generated = cells.generated(f.cell(p)).map(|s| f.state(s));
                    let roots = match generated {
                        Some(AIR) => false,
                        Some(s) if s == self.block => true,
                        _ => continue, // a cell a player emptied of something else
                    };
                    if cells.generated(f.cell(below)).map(|s| f.state(s)) != Some(ground) {
                        continue; // ground a player placed
                    }
                    let r = self.data.reach;
                    let near = (-r..=r).any(|dx| {
                        (-r..=r).any(|dz| {
                            (dx, dz) != (0, 0)
                                && (-SEED_ROWS..=SEED_ROWS)
                                    .any(|dy| is_grass(cells, [x + dx, y + dy, z + dz]))
                        })
                    });
                    spots.push(Spot { p, roots, near });
                }
            }
        }
        let mut grown: HashSet<[i32; 3]> = HashSet::new();
        let r = self.data.reach;
        for day in first_day..first_day + days {
            let mut today = Vec::new();
            for s in spots.iter().filter(|s| !grown.contains(&s.p)) {
                let [x, y, z] = s.p;
                let near = s.near
                    || (-r..=r).any(|dx| {
                        (-r..=r).any(|dz| {
                            (-SEED_ROWS..=SEED_ROWS)
                                .any(|dy| grown.contains(&[x + dx, y + dy, z + dz]))
                        })
                    });
                let mut chance: f64 = 0.0;
                if near {
                    chance = chance.max(self.data.spread_per_day);
                }
                if s.roots {
                    chance = chance.max(self.data.regrow_per_day);
                }
                if chance > 0.0 && self.day_roll(day, s.p) < chance {
                    today.push(s.p);
                }
            }
            grown.extend(today);
        }
        let mut out: Vec<_> = grown.into_iter().map(|p| (f.cell(p), block)).collect();
        out.sort_unstable();
        out
    }
}

/// The far ecology's share of a cell's most forage at face-local (x, z) (docs/ECOLOGY.md §10): the
/// climate's share of the grass there, so a cell's far forage over its ground's most is its near
/// tall grass over its cap (`Rule::cap`) — both `maxDensity` × share apart.
pub fn far_share(env: &dyn Envelope, x: f64, z: f64) -> f64 {
    env.at(x.floor() as i32, z.floor() as i32).1.clamp(0.0, 1.0)
}

/// The face-local terrain's grass envelope ([`crate::terrain::TerrainGenerator::grass_envelope`]
/// of the rule's decoration; none: the decoration isn't the terrain's).
pub struct TerrainEnvelope<'a> {
    pub terrain: &'a crate::terrain::TerrainGenerator,
    pub decoration: &'static str,
}

impl Envelope for TerrainEnvelope<'_> {
    fn at(&self, x: i32, z: i32) -> (f32, f64) {
        self.terrain
            .grass_envelope(self.decoration, x, z)
            .unwrap_or((1.0, 0.0))
    }
}
