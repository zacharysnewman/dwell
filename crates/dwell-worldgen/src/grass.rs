//! Tall grass growing and spreading over time (ADR 0075 T3c, docs/ECOLOGY.md §10). The rule is data
//! (`shared/worldgen/grass.json`, generated into `grass.gen.rs` by `shared/worldgen/gen.mjs`): once
//! a world day, an empty cell over a grass block takes tall grass when its column is **fertile** and
//! tall grass stands within `reach` columns (`spreadPerDay`), or when it held generated grass that
//! was eaten or cut (`regrowPerDay`: the roots). With `onRamps` a ½ m ramp of a ground block is
//! ground too, the tuft in the cell over it, as worldgen's decorations put it (HALF_HEIGHT_GRID.md
//! §5); never a 1 m ramp's pieces.
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
//! **Up to 2 m** (owner 2026-11-14, WORK_QUEUE item 58 g): a stand is a column of up to four ½ m
//! cells, its lower cells stalks and its top the stages' tip. A column at full height starts a new
//! cell over it (`height`: the climate's share caps the cells, `daysPerCell` the pace); cutting a cell
//! takes the cells above it (the block stands on itself: `placeOn`, the support rule).
//!
//! Face-local: the step reads and writes world cells through a [`FaceFrame`]; rolls hash face-local
//! cells, so face B's meadow grows as face A's mirror.

use std::collections::HashSet;

use dwell_blocks::{AIR, StateId};
use dwell_world::ramps::{RampFacing, gentle_cell};
use dwell_world::{CHUNK_SIZE, ChunkCoord, FaceFrame};

use crate::noise::{hash3, mix32, seed_word, unit};

/// One growth stage of the grown block (grass.json `stages`, a sprout to full height).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrassStage {
    /// The block's `stage_property` value.
    pub value: &'static str,
    /// The mean days a tuft stays at this stage (it leaves with chance 1/days a day); 0 for the last.
    pub days: f64,
    /// The least climate share a column needs for its tufts to reach this stage.
    pub share: f64,
}

/// A band of the climate's share and the most ½ m cells a column grows to there (grass.json
/// `height.cells`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrassCells {
    /// The least climate share of the band.
    pub share: f64,
    /// The most cells a column of the band grows to.
    pub cells: u32,
}

/// A stand's height in cells (grass.json `height`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrassHeight {
    /// The `stage_property` value of a column's lower cells (full blades).
    pub stalk: &'static str,
    /// The mean days a cell over the first takes from its start to full height.
    pub days_per_cell: f64,
    /// The bands, by rising share.
    pub cells: &'static [GrassCells],
}

/// The growth rule as data (see `gen.mjs` `validateGrass` for the ranges).
#[derive(Clone, Copy, Debug)]
pub struct GrassData {
    /// The block grown, by name.
    pub block: &'static str,
    /// The ground blocks it grows on, by name.
    pub on: &'static [&'static str],
    /// Whether a ½ m ramp of an `on` block (its `<name>_ramp` family's dry upright `ramp` piece,
    /// any facing) is ground too; never a 1 m ramp's pieces.
    pub on_ramps: bool,
    /// The decoration whose per-cell roll and climate envelope set the cap.
    pub decoration: &'static str,
    /// Whether grass grows at all in this build (`grows`, default true; owner, 2026-11-24: "Grass
    /// is a bit out of control, can we just disable grass growing for now"): off, no world's
    /// grass grows or spreads, whatever its own switch; its meadows stay as generated.
    pub grows: bool,
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
    /// The block's property its growth stages are values of.
    pub stage_property: &'static str,
    /// The stages, a sprout to full height (the last the property's default).
    pub stages: &'static [GrassStage],
    /// The stand's height in cells.
    pub height: GrassHeight,
}

mod table {
    use super::{GrassCells, GrassData, GrassHeight, GrassStage};
    include!("grass.gen.rs");
}
pub use table::GRASS;

/// The rows up or down a neighbouring tuft may stand and still seed a cell (½ m rows: 1 m).
const SEED_ROWS: i32 = 2;
/// The most cells a column is followed down to its ground (past the data's most, a stack a player
/// built).
const MAX_COLUMN: usize = 8;

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
    /// The block's states by stage, a sprout to full height.
    stages: Vec<StateId>,
    /// A column's lower cell.
    stalk: StateId,
    /// The ground states, face-local: the `on` blocks and, with `onRamps`, their ½ m ramps.
    on: Vec<StateId>,
    seed: u32,
    /// The seed of the stage rolls (apart from the spread's).
    stage_seed: u32,
}

impl Rule {
    pub fn new(data: GrassData, world_seed: u64) -> Self {
        let block = dwell_blocks::require(data.block);
        let stages = data
            .stages
            .iter()
            .map(|st| {
                dwell_blocks::with_property(block, data.stage_property, st.value).unwrap_or_else(
                    || panic!("{}: no {}={}", data.block, data.stage_property, st.value),
                )
            })
            .collect();
        let stalk = dwell_blocks::with_property(block, data.stage_property, data.height.stalk)
            .unwrap_or_else(|| {
                panic!(
                    "{}: no {}={}",
                    data.block, data.stage_property, data.height.stalk
                )
            });
        Self {
            data,
            block,
            stages,
            stalk,
            stage_seed: seed_word(world_seed, 0x6A55_57A6),
            on: data
                .on
                .iter()
                .flat_map(|&o| {
                    let ramps = RampFacing::ALL
                        .into_iter()
                        .filter(|_| data.on_ramps)
                        .filter_map(move |facing| gentle_cell(o, facing, false));
                    std::iter::once(dwell_blocks::require(o)).chain(ramps)
                })
                .collect(),
            seed: seed_word(world_seed, 0x6A55_0000),
        }
    }

    /// The build's rule.
    pub fn build(world_seed: u64) -> Self {
        Self::new(GRASS, world_seed)
    }

    /// The block grown (its default state: full height).
    pub fn block(&self) -> StateId {
        self.block
    }

    /// Whether `s` is a state of the grown block (any stage).
    pub fn is_tuft(&self, s: StateId) -> bool {
        s != AIR && dwell_blocks::block_of(s).name == self.data.block
    }

    /// The state of stage `i` (0: the sprout; clamped to the last, full height).
    pub fn stage_state(&self, i: usize) -> StateId {
        self.stages[i.min(self.stages.len() - 1)]
    }

    /// The sprout: what grown, regrown and eaten tufts are.
    pub fn sprout(&self) -> StateId {
        self.stages[0]
    }

    /// A column's lower cell (full blades, under the cell over it).
    pub fn stalk(&self) -> StateId {
        self.stalk
    }

    /// The most cells a column of climate share `share` grows to (`height.cells`).
    pub fn max_cells(&self, share: f64) -> usize {
        self.data
            .height
            .cells
            .iter()
            .rev()
            .find(|b| share >= b.share)
            .map_or(1, |b| b.cells as usize)
    }

    /// The stage of a tuft's state (None: not the grown block). A state without a stage the data
    /// knows (a stalk, an alias) is full height, as the property's default.
    pub fn stage_of(&self, s: StateId) -> Option<usize> {
        if !self.is_tuft(s) {
            return None;
        }
        let i = self.stages.iter().position(|&t| t == s);
        Some(i.unwrap_or(self.stages.len() - 1))
    }

    /// What an eaten tuft's cell becomes (face-local `below`, the cell under it): a sprout over the
    /// rule's ground (the roots stay; it regrows stage by stage), else air.
    pub fn eaten(&self, below: StateId) -> StateId {
        if self.grows_on(below) {
            self.sprout()
        } else {
            AIR
        }
    }

    /// The tallest stage a column of climate share `share` grows its tufts to (`stages.share`).
    pub fn top_stage(&self, share: f64) -> usize {
        self.data
            .stages
            .iter()
            .rposition(|st| share >= st.share)
            .unwrap_or(0)
    }

    /// The tuft generation places in a column of climate share `share`: its top stage.
    pub fn mature(&self, share: f64) -> StateId {
        self.stage_state(self.top_stage(share))
    }

    /// Whether a tuft at stage `i` grows a stage on `day` at face-local cell `p`.
    fn grows_up(&self, i: usize, day: u32, [x, y, z]: [i32; 3]) -> bool {
        let days = self.data.stages.get(i).map_or(0.0, |st| st.days);
        if days <= 0.0 {
            return false;
        }
        let r = unit(hash3(
            mix32(self.stage_seed ^ day.wrapping_mul(0x85EB_CA6B)),
            x,
            y,
            z,
        ));
        f64::from(r) < 1.0 / days
    }

    /// The steps (a stage, or a new cell) a column over its first cell takes on `day` at face-local
    /// cell `p` (its tip): `order.length` a `daysPerCell`, the whole steps every day and the rest by
    /// a roll.
    fn height_steps(&self, day: u32, [x, y, z]: [i32; 3]) -> u32 {
        let rate = self.stages.len() as f64 / self.data.height.days_per_cell;
        let whole = rate.floor();
        let r = unit(hash3(
            mix32(self.stage_seed ^ 0x48E1_6A11 ^ day.wrapping_mul(0xC2B2_AE35)),
            x,
            y,
            z,
        ));
        whole as u32 + u32::from(f64::from(r) < rate - whole)
    }

    /// Whether a column may start a new cell at face-local `p` (over its tip): air now, with
    /// `openAbove` cells of air over it, and air or the grown block as generated.
    fn room(&self, f: FaceFrame, cells: &mut dyn Cells, [x, y, z]: [i32; 3]) -> bool {
        let air =
            |cells: &dyn Cells, p: [i32; 3]| cells.now(f.cell(p)).map(|s| f.state(s)) == Some(AIR);
        (0..=self.data.open_above).all(|k| air(cells, [x, y + k, z]))
            && cells
                .generated(f.cell([x, y, z]))
                .map(|s| f.state(s))
                .is_some_and(|s| s == AIR || self.is_tuft(s))
    }

    /// The most share of a column's open grass cells holding tall grass at a climate share.
    pub fn cap(&self, share: f64) -> f64 {
        self.data.max_density * share.clamp(0.0, 1.0)
    }

    /// Whether face-local state `s` is ground the rule grows on.
    pub fn grows_on(&self, s: StateId) -> bool {
        self.on.contains(&s)
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
    /// cells to write (world cell, state). A column (a stand of one or more cells) is grown by the
    /// chunk holding its tip, and may grow into the chunk over it.
    pub fn grow_chunk(
        &self,
        f: FaceFrame,
        chunk: ChunkCoord,
        first_day: u32,
        days: u32,
        env: &dyn Envelope,
        cells: &mut dyn Cells,
    ) -> Vec<([i32; 3], StateId)> {
        self.grow_chunk_part(f, chunk, None, first_day, days, env, cells)
    }

    /// [`Self::grow_chunk`] over one quarter of the chunk's columns (`part` 0–3: x's lower or upper
    /// half, then z's; None: the whole chunk), the server's a quarter a tick (a whole chunk took
    /// ~12 ms of the browser's tick: priority 0, the owner's "the server never drops below 60fps").
    #[allow(clippy::too_many_arguments)]
    pub fn grow_chunk_part(
        &self,
        f: FaceFrame,
        chunk: ChunkCoord,
        part: Option<u8>,
        first_day: u32,
        days: u32,
        env: &dyn Envelope,
        cells: &mut dyn Cells,
    ) -> Vec<([i32; 3], StateId)> {
        if days == 0 {
            return Vec::new();
        }
        let local = f.chunk(chunk);
        let (x0, y0, z0) = (
            local.x * CHUNK_SIZE,
            local.y * CHUNK_SIZE,
            local.z * CHUNK_SIZE,
        );
        // The columns this call grows: the chunk's, or a quarter of them.
        let half = CHUNK_SIZE / 2;
        let (cx0, cx1, cz0, cz1) = match part {
            Some(k) => {
                let (i, j) = (i32::from(k % 2), i32::from(k / 2));
                (
                    x0 + i * half,
                    x0 + (i + 1) * half,
                    z0 + j * half,
                    z0 + (j + 1) * half,
                )
            }
            None => (x0, x0 + CHUNK_SIZE, z0, z0 + CHUNK_SIZE),
        };
        let full = self.stages.len() - 1;
        let now = |cells: &dyn Cells, p: [i32; 3]| cells.now(f.cell(p)).map(|s| f.state(s));
        let is_grass =
            |cells: &dyn Cells, p: [i32; 3]| now(cells, p).is_some_and(|s| self.is_tuft(s));
        // The spots: open cells over grass that the world still holds as generated.
        struct Spot {
            p: [i32; 3],
            roots: bool,
            near: bool,
            /// The column's climate share.
            share: f64,
        }
        let mut spots = Vec::new();
        // The columns standing that may grow, or whose cells need putting right (a stalk left on
        // top by a cut, a stack placed by hand).
        let mut columns: Vec<Column> = Vec::new();
        // Each column's tuft rows round the chunk (its columns and `reach` more each way, its rows
        // and SEED_ROWS more each way), read once: a spot's "tall grass near" looks these up
        // rather than ~245 cells each (~5 ms a chunk on the server's tick: priority 0).
        let reach = self.data.reach;
        let mut tufts: std::collections::HashMap<(i32, i32), Vec<i32>> =
            std::collections::HashMap::new();
        for z in cz0 - reach..cz1 + reach {
            for x in cx0 - reach..cx1 + reach {
                for y in y0 - SEED_ROWS..y0 + CHUNK_SIZE + SEED_ROWS {
                    if is_grass(cells, [x, y, z]) {
                        tufts.entry((x, z)).or_default().push(y);
                    }
                }
            }
        }
        let grass_near = |x: i32, y: i32, z: i32| {
            (-reach..=reach).any(|dx| {
                (-reach..=reach).any(|dz| {
                    (dx, dz) != (0, 0)
                        && tufts
                            .get(&(x + dx, z + dz))
                            .is_some_and(|rows| rows.iter().any(|&ry| (ry - y).abs() <= SEED_ROWS))
                })
            })
        };
        let mut env_cache: Option<((i32, i32), (f32, f64))> = None;
        let mut envelope = |x: i32, z: i32| match env_cache {
            Some((at, e)) if at == (x, z) => e,
            _ => {
                let e = env.at(x, z);
                env_cache = Some(((x, z), e));
                e
            }
        };
        for z in cz0..cz1 {
            for x in cx0..cx1 {
                for y in y0..y0 + CHUNK_SIZE {
                    let p = [x, y, z];
                    let here = now(cells, p);
                    if let Some(stage) = here.and_then(|s| self.stage_of(s)) {
                        if is_grass(cells, [x, y + 1, z]) {
                            continue; // not its column's tip
                        }
                        let mut n = 1;
                        while n < MAX_COLUMN && is_grass(cells, [x, y - n as i32, z]) {
                            n += 1;
                        }
                        let base = [x, y + 1 - n as i32, z];
                        let ground = now(cells, [x, base[1] - 1, z]);
                        let share = envelope(x, z).1;
                        let max = if ground.is_some_and(|g| self.grows_on(g)) {
                            self.max_cells(share)
                        } else {
                            1
                        };
                        let col = Column {
                            base,
                            n,
                            stage,
                            top: self.top_stage(share),
                            max,
                            full,
                        };
                        let tidy = (0..n).all(|k| {
                            now(cells, [x, base[1] + k as i32, z]) == Some(col.state(self, k))
                        });
                        if !tidy || col.grows() {
                            columns.push(col);
                        }
                        continue;
                    }
                    if here != Some(AIR) {
                        continue;
                    }
                    let below = [x, y - 1, z];
                    let Some(ground) = now(cells, below).filter(|g| self.on.contains(g)) else {
                        continue;
                    };
                    if !(1..=self.data.open_above).all(|k| now(cells, [x, y + k, z]) == Some(AIR)) {
                        continue;
                    }
                    let e = envelope(x, z);
                    if !self.fertile(e) {
                        continue;
                    }
                    let generated = cells.generated(f.cell(p)).map(|s| f.state(s));
                    let roots = match generated {
                        Some(AIR) => false,
                        Some(s) if self.is_tuft(s) => true,
                        _ => continue, // a cell a player emptied of something else
                    };
                    if cells.generated(f.cell(below)).map(|s| f.state(s)) != Some(ground) {
                        continue; // ground a player placed
                    }
                    let near = grass_near(x, y, z);
                    spots.push(Spot {
                        p,
                        roots,
                        near,
                        share: e.1,
                    });
                }
            }
        }
        let mut grown: HashSet<[i32; 3]> = HashSet::new();
        let r = self.data.reach;
        let spreads = self.data.spread_per_day > 0.0 || self.data.regrow_per_day > 0.0;
        for day in first_day..first_day + days {
            // The standing columns grow (before today's sprouts, which start tomorrow).
            for c in columns.iter_mut().filter(|c| c.grows()) {
                if c.n == 1 && c.stage < full {
                    // The first cell: a stage at a time, by the stages' days.
                    if self.grows_up(c.stage, day, c.tip()) {
                        c.stage += 1;
                    }
                    continue;
                }
                // Over it: a stage or a new cell a step, `daysPerCell` days a cell.
                for _ in 0..self.height_steps(day, c.tip()) {
                    if c.stage < full {
                        c.stage += 1;
                    } else if c.n < c.max && self.room(f, cells, c.above()) {
                        c.n += 1;
                        c.stage = 0;
                    } else {
                        break;
                    }
                }
            }
            if !spreads {
                continue;
            }
            let mut today = Vec::new();
            for s in spots.iter().filter(|s| !grown.contains(&s.p)) {
                let [x, y, z] = s.p;
                // Near today's sprouts: the few grown, each tested (probing the ±reach box in the set
                // cost ~5 ms a chunk on the server's tick: priority 0).
                let near = s.near
                    || grown.iter().any(|g| {
                        (g[0] - x).abs() <= r
                            && (g[2] - z).abs() <= r
                            && (g[1] - y).abs() <= SEED_ROWS
                    });
                let mut chance: f64 = 0.0;
                if near {
                    chance = chance.max(self.data.spread_per_day);
                }
                if s.roots {
                    chance = chance.max(self.data.regrow_per_day);
                }
                if chance > 0.0 && self.day_roll(day, s.p) < chance {
                    today.push((s.p, s.share));
                }
            }
            for (p, share) in today {
                grown.insert(p);
                columns.push(Column {
                    base: p,
                    n: 1,
                    stage: 0,
                    top: self.top_stage(share),
                    max: self.max_cells(share),
                    full,
                });
            }
        }
        // What changed: each column's cells (stalks under its tip at its stage) that differ now.
        let mut out = Vec::new();
        for c in &columns {
            for k in 0..c.n {
                let p = [c.base[0], c.base[1] + k as i32, c.base[2]];
                let want = c.state(self, k);
                if now(cells, p) != Some(want) {
                    out.push((f.cell(p), f.state(want)));
                }
            }
        }
        out.sort_unstable();
        out
    }
}

/// A stand of tall grass in [`Rule::grow_chunk`]: `n` cells from face-local `base` up, the lower
/// ones stalks, the tip at `stage`.
struct Column {
    base: [i32; 3],
    n: usize,
    stage: usize,
    /// The first cell's top stage (the climate's).
    top: usize,
    /// The most cells (the climate's; 1 off the rule's ground).
    max: usize,
    /// The full stage.
    full: usize,
}

impl Column {
    fn tip(&self) -> [i32; 3] {
        [self.base[0], self.base[1] + self.n as i32 - 1, self.base[2]]
    }

    fn above(&self) -> [i32; 3] {
        [self.base[0], self.base[1] + self.n as i32, self.base[2]]
    }

    /// Whether it may still grow: its first cell short of its top stage, a cell over it short of
    /// full height, or a full tip under its most cells.
    fn grows(&self) -> bool {
        if self.n == 1 && self.stage < self.full {
            return self.stage < self.top;
        }
        self.stage < self.full || self.n < self.max
    }

    /// The state of its cell `k` (0: the base): stalks under the tip.
    fn state(&self, rule: &Rule, k: usize) -> StateId {
        if k + 1 < self.n {
            rule.stalk
        } else {
            rule.stage_state(self.stage)
        }
    }
}

/// The tuft generation places for decoration `decoration`'s state `state` in a column of climate
/// share `share` (docs/ECOLOGY.md §10): the build's grown block at the column's top stage; any other
/// decoration's state as it is. A world of one biome (share 1) places full-height tufts as before.
pub fn generated_tuft(decoration: &str, state: StateId, share: f64) -> StateId {
    static RULE: std::sync::OnceLock<Rule> = std::sync::OnceLock::new();
    if decoration != GRASS.decoration {
        return state;
    }
    let rule = RULE.get_or_init(|| Rule::build(0));
    if rule.is_tuft(state) {
        rule.mature(share)
    } else {
        state
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
