//! Blueprints (WORK_QUEUE item 56, docs/BLUEPRINTS.md): a structure's cells placed as an entity
//! shown as ghost blocks. This is the geometry both the server and the client use: a record
//! (`dwell_protocol::blueprints::Blueprint`, face-local) resolved to its world cells through its
//! face (`Ghost`), what is left to build, a player's pick along the view ray (only ghost cells,
//! only the ghost's block, only blocks the player has), the have/need counts of the panel, and a
//! builder's order (the lowest row first, supported cells first). The rules are data
//! (shared/blueprints/blueprints.json → `blueprints.gen.rs`).

use std::collections::HashMap;

use dwell_blocks::{StateId, block_of, info, parse_state};
use dwell_protocol::blueprints::Blueprint;

use crate::edit::targetable;
use crate::structure::{Orientation, Structure};
use crate::{CELL_H, Face, FaceFrame, cell_of_pos};

/// A builder's rules (`blueprints.json` `builder`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BuilderRules {
    /// The creature that builds (shared/creatures).
    pub creature: &'static str,
    /// Seconds a piece.
    pub pace_s: f64,
    /// How near its cell it stands to place it (m).
    pub reach_m: f64,
    /// A blueprint with a player this near gets its builders (m).
    pub near_m: f64,
    /// Builders a blueprint gets.
    pub per_blueprint: u32,
}

/// The blueprints' rules (`shared/blueprints/blueprints.json`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rules {
    /// The most blueprints a world holds.
    pub per_world: usize,
    /// One piece every this many ms while Place is held into a blueprint.
    pub player_pace_ms: f64,
    /// The panel's most block rows.
    pub panel_rows: usize,
    /// The most ghost faces drawn per blueprint.
    pub ghost_face_cap: usize,
    pub builder: BuilderRules,
    /// The repository's blueprints offered in the Blueprints list.
    pub catalog: &'static [&'static str],
    /// Town sites from the world seed (B4).
    pub towns: TownRules,
}

/// One blueprint of a town plan: its structure, its anchor's offset from the site (m, x and z)
/// and its quarter turns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlanBlueprint {
    pub structure: &'static str,
    pub at: [i32; 2],
    pub turn: u8,
}

/// A town plan: its blueprints, laid out about the site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TownPlan {
    pub name: &'static str,
    pub weight: u32,
    pub blueprints: &'static [PlanBlueprint],
}

/// The town sites' rules (`blueprints.json` `towns`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TownRules {
    /// One site at most per grid cell this wide (m).
    pub spacing_m: i32,
    /// A cell's points lie within this share of half a cell of its centre.
    pub jitter: f64,
    /// Points tried per cell, in order; the first that suits is its site.
    pub attempts: u32,
    /// Ground this high above sea level (m); the worldgen's ground test applies it.
    pub height_m: [f64; 2],
    /// The most the ground may rise across the footprint (degrees).
    pub max_slope_deg: f64,
    /// The footprint's half width (m).
    pub footprint_m: i32,
    /// A living player this near (m) founds it.
    pub found_m: f64,
    pub plans: &'static [TownPlan],
}

#[rustfmt::skip]
mod generated {
    use super::{BuilderRules, PlanBlueprint, Rules, TownPlan, TownRules};
    include!("blueprints.gen.rs");
}
pub use generated::SHIPPED;

/// The face a record names.
pub fn face_of(b: &Blueprint) -> FaceFrame {
    FaceFrame::of(if b.face == 1 { Face::B } else { Face::A })
}

/// A record's face byte.
pub fn face_byte(f: FaceFrame) -> u8 {
    u8::from(f.is_b())
}

/// A blueprint record of a structure (its cells, palette and anchor), at the face-local cell `at`
/// on face `f`, turned `turn` quarter turns. Air and kept cells are left out (nothing to build).
pub fn record_of(s: &Structure, f: FaceFrame, at: [i32; 3], turn: u8) -> Option<Blueprint> {
    let size: [u8; 3] = std::array::from_fn(|i| u8::try_from(s.size[i]).unwrap_or(0));
    if size
        .iter()
        .any(|&n| n == 0 || n > dwell_protocol::blueprints::MAX_SIDE)
    {
        return None;
    }
    let placed = s.place_oriented(s.anchor, Orientation::default(), 0);
    let mut palette: Vec<String> = Vec::new();
    let mut cells = vec![0u8; s.size.iter().product::<i32>() as usize];
    for (p, state) in placed.cells() {
        if !targetable(state) {
            continue;
        }
        let d: [i32; 3] = std::array::from_fn(|i| p[i] - placed.min[i]);
        let name = info(state).name.to_string();
        let k = match palette.iter().position(|n| *n == name) {
            Some(k) => k,
            None => {
                if palette.len() == dwell_protocol::blueprints::MAX_PALETTE {
                    return None;
                }
                palette.push(name);
                palette.len() - 1
            }
        };
        cells[((d[1] * s.size[2] + d[2]) * s.size[0] + d[0]) as usize] = k as u8 + 1;
    }
    Some(Blueprint {
        id: 0,
        name: s.name.to_string(),
        face: face_byte(f),
        at,
        turn: turn % 4,
        size,
        anchor: std::array::from_fn(|i| s.anchor[i] as u8),
        palette,
        cells,
    })
}

/// A blueprint's cell: where (world and face-local) and what it wants there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GhostCell {
    pub world: [i32; 3],
    pub local: [i32; 3],
    /// The state wanted, as the world holds it (through the face).
    pub state: StateId,
}

/// A blueprint resolved against this build's registry, its cells in the world.
#[derive(Clone, Debug)]
pub struct Ghost {
    pub id: u32,
    pub name: String,
    pub face: FaceFrame,
    /// The box it occupies, world cells, inclusive.
    pub min: [i32; 3],
    pub max: [i32; 3],
    /// Its cells, in the structure's layer order (bottom row first).
    pub cells: Vec<GhostCell>,
    index: HashMap<[i32; 3], usize>,
}

/// Whether a cell holding `now` holds the blueprint's `want`: the same block, whatever its state
/// (a flooded or turned copy counts).
pub fn built(now: StateId, want: StateId) -> bool {
    block_of(now).name == block_of(want).name
}

/// The item a cell of `state` costs to place (its block's `cost`, a ramp's base, else the block;
/// the server's own rule, ADR 0066).
pub fn cost_item(state: StateId) -> &'static str {
    let name = block_of(state).name;
    dwell_protocol::inventory::drops_of(name).map_or(name, |d| d.cost)
}

/// Whether a block can be put in a cell holding `m`: air, a liquid, or a replaceable plant.
pub fn free(m: StateId) -> bool {
    !targetable(m) || info(m).replaceable
}

impl Ghost {
    /// The record's cells in the world; a name this build doesn't know leaves its cells out.
    pub fn of(b: &Blueprint) -> Self {
        let f = face_of(b);
        let size = b.size.map(i32::from);
        let anchor = b.anchor.map(i32::from);
        let states: Vec<Option<StateId>> = b.palette.iter().map(|n| parse_state(n).ok()).collect();
        let cells: Vec<Option<StateId>> = b
            .cells
            .iter()
            .map(|&c| {
                (c > 0)
                    .then(|| states.get(usize::from(c) - 1).copied().flatten())
                    .flatten()
            })
            .collect();
        let s = Structure::from_cells("blueprint", 0, size, anchor, &cells);
        let o = Orientation {
            quarter_turns: b.turn % 4,
            mirror: None,
        };
        let placed = s.place_oriented(b.at, o, 0);
        let mut out = Vec::new();
        let mut index = HashMap::new();
        for (local, state) in placed.cells() {
            let world = f.cell(local);
            index.insert(world, out.len());
            out.push(GhostCell {
                world,
                local,
                state: f.state(state),
            });
        }
        let (lo, hi) = (f.cell(placed.min), f.cell(placed.max));
        Self {
            id: b.id,
            name: b.name.clone(),
            face: f,
            min: std::array::from_fn(|i| lo[i].min(hi[i])),
            max: std::array::from_fn(|i| lo[i].max(hi[i])),
            cells: out,
            index,
        }
    }

    /// The state the blueprint wants at world cell `c`, if it has one there.
    pub fn at(&self, c: [i32; 3]) -> Option<StateId> {
        self.index.get(&c).map(|&i| self.cells[i].state)
    }

    /// Whether world cell `c` lies in its box.
    pub fn contains(&self, c: [i32; 3]) -> bool {
        (0..3).all(|i| c[i] >= self.min[i] && c[i] <= self.max[i])
    }

    /// The cells still to build (not holding their block; an unloaded cell counts as left).
    pub fn remaining<'a>(
        &'a self,
        get: &'a dyn Fn([i32; 3]) -> Option<StateId>,
    ) -> impl Iterator<Item = &'a GhostCell> + 'a {
        self.cells
            .iter()
            .filter(move |c| get(c.world).is_none_or(|now| !built(now, c.state)))
    }

    /// Whether every cell holds its block; None while a cell isn't loaded.
    pub fn complete(&self, get: &dyn Fn([i32; 3]) -> Option<StateId>) -> Option<bool> {
        let mut done = true;
        for c in &self.cells {
            match get(c.world) {
                None => return None,
                Some(now) if !built(now, c.state) => done = false,
                _ => {}
            }
        }
        Some(done)
    }

    /// Whether a block put at `c` would stand against something: a solid block or a built cell
    /// beside it (a place needs a face to go on).
    fn supported(&self, c: [i32; 3], get: &dyn Fn([i32; 3]) -> Option<StateId>) -> bool {
        crate::edit::FACE_DIRS
            .iter()
            .any(|d| get([c[0] + d[0], c[1] + d[1], c[2] + d[2]]).is_some_and(|m| !free(m)))
    }

    /// A player's pick: along the ray from `eye` (world m) by `dir` (unit), within `reach` m, the
    /// first of its cells that is left to build, free, beside something solid and whose block the
    /// player `has`. The ray stops at the first solid cell. Only ghost cells, only their block.
    pub fn pick(
        &self,
        eye: [f64; 3],
        dir: [f64; 3],
        reach: f64,
        get: &dyn Fn([i32; 3]) -> Option<StateId>,
        has: &dyn Fn(StateId) -> bool,
    ) -> Option<GhostCell> {
        let step = 0.05 * CELL_H;
        let mut last = None;
        let mut t = 0.0;
        while t <= reach {
            let c = cell_of_pos(std::array::from_fn(|i| eye[i] + dir[i] * t));
            t += step;
            if last == Some(c) {
                continue;
            }
            last = Some(c);
            let now = get(c)?;
            if let Some(&i) = self.index.get(&c) {
                let g = self.cells[i];
                if free(now) && !built(now, g.state) && has(g.state) && self.supported(c, get) {
                    return Some(g);
                }
            }
            if !free(now) {
                return None;
            }
        }
        None
    }

    /// Whether the ray from `eye` by `dir` crosses its box within `reach` m (the panel shows).
    pub fn aimed(&self, eye: [f64; 3], dir: [f64; 3], reach: f64) -> bool {
        let step = 0.25 * CELL_H;
        let mut t = 0.0;
        while t <= reach {
            if self.contains(cell_of_pos(std::array::from_fn(|i| eye[i] + dir[i] * t))) {
                return true;
            }
            t += step;
        }
        false
    }

    /// The panel's rows: each item still needed (`dwell:log`: what a cell costs, `cost_item`), with
    /// how many of it the player has (`have`, by item name) and how many cells of it are left,
    /// most needed first; and the cells left in all.
    pub fn needs(
        &self,
        get: &dyn Fn([i32; 3]) -> Option<StateId>,
        have: &dyn Fn(&str) -> u32,
    ) -> (Vec<Need>, usize) {
        let mut by: Vec<Need> = Vec::new();
        let mut left = 0;
        for c in self.remaining(get) {
            left += 1;
            let name = cost_item(c.state);
            match by.iter_mut().find(|n| n.block == name) {
                Some(n) => n.need += 1,
                None => by.push(Need {
                    block: name,
                    have: have(name),
                    need: 1,
                }),
            }
        }
        by.sort_by(|a, b| b.need.cmp(&a.need).then(a.block.cmp(b.block)));
        (by, left)
    }

    /// A builder's next cell: of the cells left that are free (an occupied one, a player's other
    /// block, is skipped), the lowest face-local row first, and within a row the ones resting on
    /// something solid first, then the nearest to `near` (the builder's face-local feet, so it
    /// works its way round), then the structure's order. None when nothing is left it can build.
    pub fn builder_next_near(
        &self,
        get: &dyn Fn([i32; 3]) -> Option<StateId>,
        near: Option<[f64; 3]>,
    ) -> Option<GhostCell> {
        let f = self.face;
        self.remaining(get)
            .filter(|c| get(c.world).is_some_and(free))
            .enumerate()
            .min_by_key(|(i, c)| {
                let down = f.cell([c.local[0], c.local[1] - 1, c.local[2]]);
                let resting = get(down).is_some_and(|m| !free(m));
                let d = near.map_or(0, |p| {
                    let (dx, dz) = (
                        f64::from(c.local[0]) + 0.5 - p[0],
                        f64::from(c.local[2]) + 0.5 - p[2],
                    );
                    (dx.hypot(dz) * 4.0) as i64
                });
                (c.local[1], !resting, !self.supported(c.world, get), d, *i)
            })
            .map(|(_, c)| *c)
    }

    /// [`Ghost::builder_next_near`] in the structure's order.
    pub fn builder_next(&self, get: &dyn Fn([i32; 3]) -> Option<StateId>) -> Option<GhostCell> {
        self.builder_next_near(get, None)
    }

    /// Whether any cell is left that a builder can put (free and left to build).
    pub fn buildable_left(&self, get: &dyn Fn([i32; 3]) -> Option<StateId>) -> bool {
        self.builder_next(get).is_some()
    }
}

/// A town site: on a face, the grid cell it is in, its centre (face-local column) and its plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TownSite {
    pub face: FaceFrame,
    pub cell: [i32; 2],
    pub x: i32,
    pub z: i32,
    /// Its plan's index in `TownRules::plans`.
    pub plan: usize,
}

/// The ground test a site needs: the ground's height (face-local m) at a face-local column where it
/// is dry land within the rules' height band, else None (water, a stream, past the world's edge).
pub type TownGround<'a> = &'a dyn Fn(i32, i32) -> Option<f64>;

fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A word from the seed, the face, the cell, a stream: integer hashing only (ADR 0010).
fn town_hash(seed: u64, f: FaceFrame, [i, k]: [i32; 2], stream: u64) -> u64 {
    let mut h = splitmix(seed ^ 0x70E5_0000_0000_0000 ^ u64::from(f.is_b()));
    h = splitmix(h ^ (i as u32 as u64));
    h = splitmix(h ^ ((k as u32 as u64) << 32));
    splitmix(h ^ stream)
}

impl TownRules {
    /// The grid cell holding a face-local column.
    pub fn cell_of(&self, x: i32, z: i32) -> [i32; 2] {
        [x.div_euclid(self.spacing_m), z.div_euclid(self.spacing_m)]
    }

    /// The site in grid cell `cell` on face `f`, if any: the first of its seeded points whose
    /// footprint's ground (its centre and four corners) is all dry land in the band and rises at
    /// most `max_slope_deg` across it. A pure function of the seed, the face, the cell and the
    /// ground: never of another cell.
    pub fn site_in(
        &self,
        seed: u64,
        f: FaceFrame,
        cell: [i32; 2],
        ground: TownGround,
    ) -> Option<TownSite> {
        if self.plans.is_empty() {
            return None;
        }
        let s = f64::from(self.spacing_m);
        let spread = s * 0.5 * self.jitter;
        let r = self.footprint_m;
        let rise = 2.0 * f64::from(r) * self.max_slope_deg.to_radians().tan();
        for a in 0..self.attempts {
            let off = |stream: u64| {
                let u = (town_hash(seed, f, cell, 2 * u64::from(a) + stream) >> 11) as f64
                    / (1u64 << 53) as f64;
                ((u * 2.0 - 1.0) * spread).floor() as i32
            };
            let x = cell[0] * self.spacing_m + self.spacing_m / 2 + off(0);
            let z = cell[1] * self.spacing_m + self.spacing_m / 2 + off(1);
            let hs: Option<Vec<f64>> = [(0, 0), (-r, -r), (r, -r), (-r, r), (r, r)]
                .iter()
                .map(|&(dx, dz)| ground(x + dx, z + dz))
                .collect();
            let Some(hs) = hs else {
                continue;
            };
            let (lo, hi) = hs
                .iter()
                .fold((f64::MAX, f64::MIN), |(l, h), &v| (l.min(v), h.max(v)));
            if hi - lo > rise {
                continue;
            }
            let total: u64 = self.plans.iter().map(|p| u64::from(p.weight)).sum();
            let mut pick = town_hash(seed, f, cell, 1000) % total.max(1);
            let plan = self
                .plans
                .iter()
                .position(|p| {
                    let w = u64::from(p.weight);
                    if pick < w {
                        return true;
                    }
                    pick -= w;
                    false
                })
                .unwrap_or(0);
            return Some(TownSite {
                face: f,
                cell,
                x,
                z,
                plan,
            });
        }
        None
    }

    /// The sites on face `f` whose centre lies within `radius` m of face-local (x, z).
    pub fn sites_near(
        &self,
        seed: u64,
        f: FaceFrame,
        x: f64,
        z: f64,
        radius: f64,
        ground: TownGround,
    ) -> Vec<TownSite> {
        let lo = self.cell_of((x - radius).floor() as i32, (z - radius).floor() as i32);
        let hi = self.cell_of((x + radius).ceil() as i32, (z + radius).ceil() as i32);
        let mut out = Vec::new();
        for k in lo[1]..=hi[1] {
            for i in lo[0]..=hi[0] {
                if let Some(t) = self.site_in(seed, f, [i, k], ground)
                    && (f64::from(t.x) + 0.5 - x).hypot(f64::from(t.z) + 0.5 - z) <= radius
                {
                    out.push(t);
                }
            }
        }
        out
    }

    /// A site's blueprints: each structure's name, its anchor's face-local column and its turn.
    pub fn layout(&self, t: &TownSite) -> Vec<(&'static str, [i32; 2], u8)> {
        self.plans[t.plan]
            .blueprints
            .iter()
            .map(|b| (b.structure, [t.x + b.at[0], t.z + b.at[1]], b.turn))
            .collect()
    }
}

/// One panel row: a block still needed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Need {
    /// The item's name (`dwell:log`: what its cells cost).
    pub block: &'static str,
    /// How many the player has.
    pub have: u32,
    /// How many cells of it are left to build.
    pub need: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::M;
    use crate::both_faces;

    fn hut() -> Structure {
        crate::structure::load("dwell:bp_hut").expect("the test hut")
    }

    /// A flat world: stone up to face-local row `ground` (exclusive), air above; edits in `set`.
    fn world(
        f: FaceFrame,
        ground: i32,
        set: &HashMap<[i32; 3], StateId>,
    ) -> impl Fn([i32; 3]) -> Option<StateId> + '_ {
        move |c| {
            Some(set.get(&c).copied().unwrap_or_else(|| {
                if f.cell(c)[1] < ground {
                    M.stone
                } else {
                    M.air
                }
            }))
        }
    }

    #[test]
    fn the_catalog_s_blueprints_load_and_fit_a_record() {
        for name in SHIPPED.catalog {
            let s = crate::structure::load(name).unwrap_or_else(|e| panic!("{name}: {e}"));
            let b = record_of(&s, FaceFrame::A, [0, 64, 0], 0).expect(name);
            assert_eq!(
                dwell_protocol::blueprints::Blueprint::from_bytes(&b.to_bytes()),
                Some(b.clone())
            );
            assert!(!Ghost::of(&b).cells.is_empty(), "{name} has cells to build");
        }
    }

    both_faces!(
        fn a_ghost_s_cells_are_the_structure_s_turned_through_the_face(f: FaceFrame) {
            let s = hut();
            let at = [10, 64, -5];
            for turn in 0..4 {
                let g = Ghost::of(&record_of(&s, f, at, turn).unwrap());
                let a = Ghost::of(&record_of(&s, FaceFrame::A, at, turn).unwrap());
                // Face parity: the same face-local cells and blocks as face A's.
                let mut mine: Vec<_> = g
                    .cells
                    .iter()
                    .map(|c| (c.local, block_of(c.state).name))
                    .collect();
                let mut theirs: Vec<_> = a
                    .cells
                    .iter()
                    .map(|c| (c.local, block_of(c.state).name))
                    .collect();
                mine.sort();
                theirs.sort();
                assert_eq!(mine, theirs);
                for c in &g.cells {
                    assert_eq!(f.cell(c.local), c.world);
                    assert!(g.contains(c.world));
                }
                // The floor stands on the anchor's row, centred on it.
                assert!(g.cells.iter().any(|c| c.local == at));
                assert!(g.cells.iter().all(|c| c.local[1] >= at[1]));
            }
            // A quarter turn moves the doorway (south, +z) to the west (−x).
            let door = |turn| {
                let g = Ghost::of(&record_of(&s, f, at, turn).unwrap());
                let d = [at[0], at[1] + 1, at[2] + 2];
                let w = [at[0] - 2, at[1] + 1, at[2]];
                (g.at(f.cell(d)).is_some(), g.at(f.cell(w)).is_some())
            };
            assert_eq!(door(0), (false, true));
            assert_eq!(door(1), (true, false));
        }
    );

    both_faces!(
        fn a_pick_is_only_a_ghost_cell_with_its_block_that_the_player_has(f: FaceFrame) {
            let s = hut();
            let at = [0, 64, 0];
            let g = Ghost::of(&record_of(&s, f, at, 0).unwrap());
            let set = HashMap::new();
            let get = world(f, 64, &set);
            // Standing south of the hut, looking down at its floor's front edge (row 64: 32 m).
            let eye = f.point([0.5, 34.0, 4.5]);
            let target = f.point([0.5, 32.25, 2.5]);
            let d: [f64; 3] = std::array::from_fn(|i| target[i] - eye[i]);
            let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            let dir = d.map(|v| v / n);
            let all = |_: StateId| true;
            let p = g.pick(eye, dir, 6.0, &get, &all).expect("the floor's edge");
            assert_eq!(p.local, [0, 64, 2]);
            assert_eq!(block_of(p.state).name, "dwell:stone");
            // Without stone nothing is picked there (it shows red), not another block.
            let no_stone = |m: StateId| block_of(m).name != "dwell:stone";
            assert_eq!(g.pick(eye, dir, 6.0, &get, &no_stone), None);
            // Aimed off the blueprint: nothing.
            let away = f.vector([0.0, -0.2, 1.0]);
            assert_eq!(g.pick(eye, away, 6.0, &get, &all), None);
            // Out of reach: nothing.
            assert_eq!(g.pick(eye, dir, 1.0, &get, &all), None);
            // Built: the next cell along is not that one.
            let mut set = HashMap::new();
            set.insert(f.cell([0, 64, 2]), M.stone);
            let get = world(f, 64, &set);
            assert_ne!(
                g.pick(eye, dir, 6.0, &get, &all).map(|c| c.local),
                Some([0, 64, 2])
            );
        }
    );

    both_faces!(
        fn the_panel_counts_have_and_need_per_block_and_the_cells_left(f: FaceFrame) {
            let g = Ghost::of(&record_of(&hut(), f, [0, 64, 0], 0).unwrap());
            let set = HashMap::new();
            let get = world(f, 64, &set);
            let have = |n: &str| if n == "dwell:log" { 12 } else { 0 };
            let (rows, left) = g.needs(&get, &have);
            // The hut: a 5 × 5 stone floor and roof, log walls 4 rows of the 16 edge cells less a
            // 3-row doorway.
            assert_eq!(left, 25 + 25 + 16 * 4 - 3);
            assert_eq!(
                rows[0],
                Need {
                    block: "dwell:log",
                    have: 12,
                    need: 61
                }
            );
            assert_eq!(
                rows[1],
                Need {
                    block: "dwell:stone",
                    have: 0,
                    need: 50
                }
            );
            // One floor cell built: one fewer stone.
            let mut set = HashMap::new();
            set.insert(f.cell([0, 64, 0]), M.stone);
            let get = world(f, 64, &set);
            let (rows, left2) = g.needs(&get, &have);
            assert_eq!((rows[1].need, left2), (49, left - 1));
            assert_eq!(g.complete(&get), Some(false));
        }
    );

    both_faces!(
        fn town_sites_are_deterministic_per_cell_and_only_on_suitable_ground(f: FaceFrame) {
            let rules = SHIPPED.towns;
            let flat = |_: i32, _: i32| Some(40.0);
            let a = rules.sites_near(7, f, 0.0, 0.0, 5000.0, &flat);
            let b = rules.sites_near(7, f, 0.0, 0.0, 5000.0, &flat);
            assert_eq!(a, b);
            assert!(a.len() > 10, "{} sites", a.len());
            // A cell's site is its own, whatever is asked round it.
            for t in &a {
                assert_eq!(rules.site_in(7, f, t.cell, &flat), Some(*t));
                assert_eq!(rules.cell_of(t.x, t.z), t.cell);
                let near = rules.sites_near(7, f, f64::from(t.x), f64::from(t.z), 10.0, &flat);
                assert_eq!(near, vec![*t]);
            }
            // Another seed or face puts them elsewhere.
            assert_ne!(rules.sites_near(8, f, 0.0, 0.0, 5000.0, &flat), a);
            assert_ne!(
                rules.sites_near(7, f.other(), 0.0, 0.0, 5000.0, &flat)[0].x,
                a[0].x
            );
            // No dry land, no towns; a steep slope, none.
            let sea = |_: i32, _: i32| None;
            assert!(rules.sites_near(7, f, 0.0, 0.0, 5000.0, &sea).is_empty());
            let steep = |x: i32, _: i32| Some(f64::from(x));
            assert!(rules.sites_near(7, f, 0.0, 0.0, 5000.0, &steep).is_empty());
            // A site's layout: the plan's blueprints about its centre.
            let l = rules.layout(&a[0]);
            assert_eq!(l.len(), rules.plans[a[0].plan].blueprints.len());
            assert_eq!(l[0].1, [a[0].x, a[0].z]);
        }
    );

    both_faces!(
        fn a_builder_builds_foundation_up_skips_occupied_cells_and_completes(f: FaceFrame) {
            let g = Ghost::of(&record_of(&hut(), f, [0, 64, 0], 0).unwrap());
            let mut set = HashMap::new();
            // A player's dirt where a wall cell goes: skipped.
            let occupied = f.cell([-2, 65, -2]);
            set.insert(occupied, M.dirt);
            let mut rows = Vec::new();
            for _ in 0..1000 {
                let next = {
                    let get = world(f, 64, &set);
                    g.builder_next(&get)
                        .inspect(|c| assert!(free(get(c.world).unwrap())))
                };
                let Some(c) = next else {
                    break;
                };
                rows.push(c.local[1]);
                set.insert(c.world, c.state);
            }
            assert!(
                rows.windows(2).all(|w| w[0] <= w[1]),
                "lowest row first: {rows:?}"
            );
            assert_eq!(
                rows.len(),
                g.cells.len() - 1,
                "every cell but the occupied one"
            );
            {
                let get = world(f, 64, &set);
                assert_eq!(g.complete(&get), Some(false));
                assert_eq!(g.remaining(&get).count(), 1);
            }
            set.insert(occupied, parse_state("dwell:log").unwrap());
            let get = world(f, 64, &set);
            assert_eq!(g.complete(&get), Some(true));
        }
    );
}
