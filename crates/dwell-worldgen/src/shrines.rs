//! Resurrection shrines (docs/SHRINES.md; owner, 2026-11-12: "'resurrection shrines' these should
//! work like Guild Wars. You respawn at the nearest shrine when you die"): a small built structure
//! with a glow, about every `spacing` metres across each face, where a dead player stands up again.
//!
//! Everything is data (`shared/worldgen/shrines.json`, generated into `shrines.gen.rs`): the grid's
//! spacing and jitter, the siting rules, the structure, where a player stands on it. A face's ground
//! is cut into `spacing` m cells, each holding at most one shrine: the first of `attempts` hashed
//! points near the cell's centre whose ground suits. A cell's shrine is a pure function of the face's
//! seed, the cell, the rules, the portals' sites and the face's height field — never of any other
//! cell's — so a chunk finds the shrines reaching it from the cells round it alone, in any order, on
//! any thread, natively or in WASM (integer hashing and the height field's deterministic
//! arithmetic, ADR 0010), and the server finds the one nearest a death the same way.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use dwell_world::SEA_LEVEL_M;
use dwell_world::structure::{Orientation, Placed, Structure};

use crate::noise::{floor_div, hash2, hash3, seed_word, unit};
use crate::portals::Portal;
use crate::terrain::{TerrainGenerator, Water};

/// How a face's shrines are sited and built (`shrines.json`; validated by `gen.mjs`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShrineRules {
    /// The grid's cell (m): one shrine at most in each.
    pub spacing: f32,
    /// How far from the cell's centre a site may lie, as a share of half the cell (0 at the centre).
    pub jitter: f32,
    /// Hashed points tried in a cell, in order; the first that suits is the cell's shrine.
    pub attempts: u32,
    /// A site lies at least this far inland of the face's coast (m).
    pub inland: f32,
    /// The footprint's ground lies in [min_height, max_height] m above sea level.
    pub min_height: f32,
    pub max_height: f32,
    /// The footprint's rise (its highest ground less its lowest) is at most its width × the tangent.
    pub max_slope_deg: f32,
    /// A site lies at least this far from every portal's axis (m).
    pub portal_clear: f32,
    /// No tree stands within this far of the structure's footprint (m).
    pub clear: f32,
    /// The structure built at each site (`shared/structures`), by name.
    pub structure: &'static str,
    /// The LOD draws it at levels whose cells are at most this many metres.
    pub lod_cell: i64,
    /// Where a dead player stands up: this column from the anchor (x, z), on its floor.
    pub stand: [i32; 2],
    /// The nearest shrine to a death is searched in the cells within this many of the death's.
    pub search_cells: i32,
    /// Ground flatter than this across the footprint takes the seeded quarter turn, not the
    /// downhill one.
    pub flat_below_deg: f32,
    /// The world spawn's shrine lies at most this far from the spawn search's point (m): failing
    /// one, a shrine is built at the point itself.
    pub spawn_within: f32,
}

mod table {
    include!("shrines.gen.rs");
}
pub use table::{SHRINE_WORLDS, SHRINES};

/// The rules for a world setting by name (`shrines.json` `worlds`), else the build's.
pub fn rules_for(world: Option<&str>) -> ShrineRules {
    world
        .and_then(|n| SHRINE_WORLDS.iter().find(|(w, _)| *w == n))
        .map_or(SHRINES, |(_, r)| *r)
}

/// A shrine: the column its structure's anchor stands on (face-local x, z) and its quarter turn
/// about +y (its open half faces +z unturned).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Shrine {
    pub x: i32,
    pub z: i32,
    pub turn: u8,
}

impl Shrine {
    /// The turn as a structure orientation.
    pub fn orientation(self) -> Orientation {
        Orientation {
            quarter_turns: self.turn % 4,
            mirror: None,
        }
    }

    /// The direction (x, z) its open half faces.
    pub fn facing(self) -> [i32; 2] {
        let [x, _, z] = self.orientation().apply([0, 0, 1]);
        [x, z]
    }
}

/// One face's shrines, resolved: the rules, the structure, and the cells searched so far.
#[derive(Debug)]
pub struct Shrines {
    pub rules: ShrineRules,
    pub structure: Arc<Structure>,
    /// The face's seed (each face sites its own).
    seed: u64,
    /// The portals' axes, which shrines keep `portal_clear` from.
    portals: Arc<Vec<Portal>>,
    /// The structure's bottom-layer columns from its anchor in each quarter turn: its footprint.
    foot: [Vec<(i32, i32)>; 4],
    /// How far (whole columns) the footprint and the clear ground reach from the anchor.
    reach: i32,
    clear_reach: i32,
    /// The cell's shrine, by cell (a cache only: each entry is a pure function of its cell).
    cells: RwLock<HashMap<(i32, i32), Option<Shrine>>>,
    /// The shrine built at the spawn search's point when no grid shrine lies within
    /// `spawn_within` of it (resolved once, on demand).
    spawn: OnceLock<Option<Shrine>>,
    /// The spawn search's point, once run (it takes ~0.6 s natively).
    search: OnceLock<[f64; 3]>,
}

impl Shrines {
    /// A face's shrines by `rules`, kept clear of `portals`, from the face's seed.
    pub fn new(seed: u64, rules: ShrineRules, portals: Arc<Vec<Portal>>) -> Self {
        let structure = Arc::new(
            dwell_world::structure::load(rules.structure).unwrap_or_else(|e| panic!("{e}")),
        );
        let foot = std::array::from_fn(|q| {
            let o = Orientation {
                quarter_turns: q as u8,
                mirror: None,
            };
            let probe = structure.place_oriented([0, 0, 0], o, 0);
            probe
                .cells()
                .filter(|(p, _)| p[1] == probe.min[1])
                .map(|(p, _)| (p[0], p[2]))
                .collect::<Vec<_>>()
        });
        let reach = foot
            .iter()
            .flatten()
            .map(|&(x, z)| x.abs().max(z.abs()))
            .max()
            .unwrap_or(0);
        Self {
            rules,
            structure,
            seed,
            portals,
            foot,
            reach,
            clear_reach: reach + rules.clear.ceil() as i32,
            cells: RwLock::new(HashMap::new()),
            spawn: OnceLock::new(),
            search: OnceLock::new(),
        }
    }

    fn spacing(&self) -> i32 {
        self.rules.spacing as i32
    }

    /// How far (whole columns) a shrine's footprint reaches from its anchor, in any turn.
    pub fn reach(&self) -> i32 {
        self.reach
    }

    /// The grid cell holding column (x, z).
    pub fn cell_of(&self, x: i32, z: i32) -> (i32, i32) {
        (floor_div(x, self.spacing()), floor_div(z, self.spacing()))
    }

    /// The `attempt`th point tried in cell (i, k): near its centre, within the jitter.
    pub fn candidate(&self, (i, k): (i32, i32), attempt: u32) -> (i32, i32) {
        let s = self.spacing();
        let spread = f64::from(s) * 0.5 * f64::from(self.rules.jitter);
        let a = attempt as i32;
        let hx = hash3(seed_word(self.seed, 0x5A71_0E00), i, a, k);
        let hz = hash3(seed_word(self.seed, 0x5A71_0E01), i, a, k);
        let off = |h: u32| ((f64::from(unit(h)) * 2.0 - 1.0) * spread).floor() as i32;
        let centre = |c: i32| c * s + s / 2;
        (centre(i) + off(hx), centre(k) + off(hz))
    }

    /// The quarter turn a shrine at (x, z) takes: its open half facing the quarter nearest
    /// downhill (the height field's fall across the footprint), or the seeded turn where the ground
    /// is flatter than `flat_below_deg` (owner, 2026-11-12: "rotate it so it faces out from
    /// hillsides"). Face-local: on face B the same ground, mirrored, gives the same turn.
    pub fn turn_at(&self, g: &TerrainGenerator, x: i32, z: i32) -> u8 {
        let r = self.reach.max(1);
        let h = |x: i32, z: i32| f64::from(g.column_at(x, z).height);
        let gx = h(x + r, z) - h(x - r, z);
        let gz = h(x, z + r) - h(x, z - r);
        let run = f64::from(2 * r);
        let slope = (gx.hypot(gz) / run).atan().to_degrees();
        if slope < f64::from(self.rules.flat_below_deg) {
            return (hash2(seed_word(self.seed, 0x5A71_0E02), x, z) % 4) as u8;
        }
        let (dx, dz) = (-gx, -gz); // downhill
        (0..4u8)
            .max_by(|&a, &b| {
                let dot = |q: u8| {
                    let [fx, fz] = Shrine { x, z, turn: q }.facing();
                    f64::from(fx) * dx + f64::from(fz) * dz
                };
                // Ties to the lower turn.
                dot(a).total_cmp(&dot(b)).then(b.cmp(&a))
            })
            .unwrap_or(0)
    }

    /// Cell (i, k)'s shrine on the face `g` generates, if any of its points suits.
    pub fn in_cell(&self, g: &TerrainGenerator, cell: (i32, i32)) -> Option<Shrine> {
        if let Some(s) = self.cells.read().expect("shrine cells").get(&cell) {
            return *s;
        }
        let found = (0..self.rules.attempts)
            .map(|a| self.candidate(cell, a))
            .find_map(|(x, z)| self.site(g, x, z));
        self.cells
            .write()
            .expect("shrine cells")
            .insert(cell, found);
        found
    }

    /// The shrine anchored at (x, z) when the face's ground there suits one.
    pub fn site(&self, g: &TerrainGenerator, x: i32, z: i32) -> Option<Shrine> {
        let r = &self.rules;
        let clear2 = f64::from(r.portal_clear).powi(2);
        let near_portal = self.portals.iter().any(|p| {
            let (dx, dz) = (f64::from(p.x - x), f64::from(p.z - z));
            dx * dx + dz * dz < clear2
        });
        if near_portal {
            return None;
        }
        let (lo, hi) = (
            SEA_LEVEL_M as f32 + r.min_height,
            SEA_LEVEL_M as f32 + r.max_height,
        );
        let ok = |px: i32, pz: i32| {
            let c = g.column_at(px, pz);
            (!c.outside && c.water == Water::None && !c.stream && c.height >= lo && c.height <= hi)
                .then_some(c.height)
        };
        // The anchor first (cheap rejections), then the coast, then the whole footprint.
        if ok(x, z).is_none() || g.coast_at(x, z) < r.inland {
            return None;
        }
        let turn = self.turn_at(g, x, z);
        let (mut min, mut max) = (f32::MAX, f32::MIN);
        for &(dx, dz) in &self.foot[usize::from(turn)] {
            let h = ok(x + dx, z + dz)?;
            min = min.min(h);
            max = max.max(h);
        }
        let width = (2 * self.reach + 1) as f32;
        (max - min <= width * r.max_slope_deg.to_radians().tan()).then_some(Shrine { x, z, turn })
    }

    /// The grid's shrines whose clear ground reaches columns [x0, x1] × [z0, z1] (`pad` more
    /// columns round the box).
    fn grid_near(
        &self,
        g: &TerrainGenerator,
        lo: (i32, i32),
        hi: (i32, i32),
        pad: i32,
    ) -> Vec<Shrine> {
        let r = self.clear_reach + pad;
        let (i0, k0) = self.cell_of(lo.0 - r, lo.1 - r);
        let (i1, k1) = self.cell_of(hi.0 + r, hi.1 + r);
        let mut out = Vec::new();
        for k in k0..=k1 {
            for i in i0..=i1 {
                if let Some(s) = self.in_cell(g, (i, k))
                    && reaches(s, r, lo, hi)
                {
                    out.push(s);
                }
            }
        }
        out
    }

    /// The shrines whose clear ground reaches columns [x0, x1] × [z0, z1]: the grid's, and the
    /// spawn's when it reaches them. The spawn's is looked for only where no grid shrine lies within
    /// `spawn_within` less its reach (the spawn's shrine exists only where none is that near it),
    /// so most chunks never run the spawn search.
    /// Whether column (x, z) is within `margin` m of a shrine's clear ground or within the rules'
    /// `portal_clear` of a portal's axis (a town keeps off both, docs/BLUEPRINTS.md §6).
    pub fn crowds(&self, g: &TerrainGenerator, x: i32, z: i32, margin: i32) -> bool {
        let clear2 = f64::from(self.rules.portal_clear).powi(2);
        self.portals.iter().any(|p| {
            let (dx, dz) = (f64::from(p.x - x), f64::from(p.z - z));
            dx * dx + dz * dz < clear2
        }) || !self
            .near(g, (x - margin, z - margin), (x + margin, z + margin))
            .is_empty()
    }

    pub fn near(&self, g: &TerrainGenerator, lo: (i32, i32), hi: (i32, i32)) -> Vec<Shrine> {
        let mut out = self.grid_near(g, lo, hi, 0);
        let bound = self.rules.spawn_within.floor() as i32 - 2 * self.clear_reach;
        let covered = bound > 0 && !self.grid_near(g, lo, hi, bound).is_empty();
        if !covered
            && let Some(s) = self.spawn_extra(g)
            && reaches(s, self.clear_reach, lo, hi)
        {
            out.push(s);
        }
        out
    }

    /// The shrines in the grid cells within `cells` of (x, z)'s, and the spawn's if it is there
    /// (a map's view, a death's search).
    pub fn in_cells_round(&self, g: &TerrainGenerator, x: i32, z: i32, cells: i32) -> Vec<Shrine> {
        let (ci, ck) = self.cell_of(x, z);
        let mut out = Vec::new();
        for k in ck - cells..=ck + cells {
            for i in ci - cells..=ci + cells {
                out.extend(self.in_cell(g, (i, k)));
            }
        }
        let s = f64::from(self.spacing());
        let r = f64::from(cells) * s;
        if let Some(sp) = self.spawn.get().copied().flatten()
            && (f64::from(sp.x - x)).abs() <= r + s
            && (f64::from(sp.z - z)).abs() <= r + s
        {
            out.push(sp);
        }
        out
    }

    /// The shrine nearest (x, z) among the cells within `search_cells` of its own (ties: the
    /// lower x, then z), if any.
    pub fn nearest(&self, g: &TerrainGenerator, x: f64, z: f64) -> Option<Shrine> {
        // The spawn's shrine is resolved first, so a death near it finds it.
        self.spawn_extra(g);
        self.nearest_in(g, x, z, self.rules.search_cells, true)
    }

    /// [`Self::nearest`] without running the spawn search (a client's guess, cheap: the spawn's
    /// shrine counts only once something resolved it).
    pub fn nearest_known(&self, g: &TerrainGenerator, x: f64, z: f64) -> Option<Shrine> {
        self.nearest_in(g, x, z, self.rules.search_cells, true)
    }

    fn nearest_in(
        &self,
        g: &TerrainGenerator,
        x: f64,
        z: f64,
        cells: i32,
        spawn: bool,
    ) -> Option<Shrine> {
        let (xi, zi) = (x.floor() as i32, z.floor() as i32);
        let mut all = if spawn {
            self.in_cells_round(g, xi, zi, cells)
        } else {
            let (ci, ck) = self.cell_of(xi, zi);
            (ck - cells..=ck + cells)
                .flat_map(|k| (ci - cells..=ci + cells).map(move |i| (i, k)))
                .filter_map(|c| self.in_cell(g, c))
                .collect()
        };
        all.sort();
        all.into_iter()
            .min_by(|a, b| dist2(*a, x, z).total_cmp(&dist2(*b, x, z)))
    }

    /// The world spawn's shrine: the grid's nearest the spawn search's point within
    /// `spawn_within`, else the one built at the point itself (owner, 2026-11-12: "target the
    /// nearest shrine to that point for initial spawn, so all spawns are at shrines").
    pub fn spawn_shrine(&self, g: &TerrainGenerator) -> Shrine {
        if let Some(s) = self.spawn_extra(g) {
            return s;
        }
        let [x, _, z] = self.search_point(g);
        self.nearest_grid_within(g, x, z)
            .expect("a grid shrine within spawn_within (else the spawn's own)")
    }

    fn nearest_grid_within(&self, g: &TerrainGenerator, x: f64, z: f64) -> Option<Shrine> {
        let cells = (self.rules.spawn_within / self.rules.spacing).ceil() as i32 + 1;
        let w2 = f64::from(self.rules.spawn_within).powi(2);
        self.nearest_in(g, x, z, cells, false)
            .filter(|&s| dist2(s, x, z) <= w2)
    }

    /// The shrine built at the spawn search's point, when no grid shrine lies within
    /// `spawn_within` of it; resolved once per world (the search is a pure function of the seed).
    fn spawn_extra(&self, g: &TerrainGenerator) -> Option<Shrine> {
        *self.spawn.get_or_init(|| {
            let key = format!("{}|{:?}|{}", self.seed, self.rules, g.spawn_key());
            cached_spawn(key, || {
                let [x, _, z] = self.search_point(g);
                if self.nearest_grid_within(g, x, z).is_some() {
                    return None;
                }
                let (xi, zi) = (x.floor() as i32, z.floor() as i32);
                Some(Shrine {
                    x: xi,
                    z: zi,
                    turn: self.turn_at(g, xi, zi),
                })
            })
        })
    }

    /// The spawn search's point on this face, run once.
    fn search_point(&self, g: &TerrainGenerator) -> [f64; 3] {
        *self.search.get_or_init(|| g.spawn_search_point())
    }

    /// The structure at `shrine` with its bottom row at `base_row`, turned.
    pub fn placed_at(&self, shrine: Shrine, base_row: i32) -> Placed {
        self.structure
            .place_oriented([shrine.x, base_row, shrine.z], shrine.orientation(), 0)
    }

    /// The columns under the structure's bottom layer at `shrine`: where it stands.
    pub fn feet(&self, shrine: Shrine) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.foot[usize::from(shrine.turn % 4)]
            .iter()
            .map(move |&(dx, dz)| (shrine.x + dx, shrine.z + dz))
    }

    /// The column a player stands up on at `shrine` (`stand`, turned with it).
    pub fn stand_column(&self, shrine: Shrine) -> (i32, i32) {
        let [dx, dz] = self.rules.stand;
        let [x, _, z] = shrine.orientation().apply([dx, 0, dz]);
        (shrine.x + x, shrine.z + z)
    }

    /// True when the box [x0, x1] × [z0, z1] reaches any grid shrine's footprint or clear ground
    /// (the trees keep off them; the spawn's shrine stands on the spawn's patch, clear already).
    pub fn box_reaches_clear(&self, g: &TerrainGenerator, lo: (i32, i32), hi: (i32, i32)) -> bool {
        !self.grid_near(g, lo, hi, 0).is_empty()
    }
}

fn reaches(s: Shrine, r: i32, (x0, z0): (i32, i32), (x1, z1): (i32, i32)) -> bool {
    s.x + r >= x0 && s.x - r <= x1 && s.z + r >= z0 && s.z - r <= z1
}

fn dist2(s: Shrine, x: f64, z: f64) -> f64 {
    (f64::from(s.x) + 0.5 - x).powi(2) + (f64::from(s.z) + 0.5 - z).powi(2)
}

/// The spawn shrines resolved so far, by face seed, rules and generator (some tools build a
/// generator per chunk; the spawn search once per world).
fn cached_spawn(key: String, resolve: impl FnOnce() -> Option<Shrine>) -> Option<Shrine> {
    static CACHE: Mutex<Vec<(String, Option<Shrine>)>> = Mutex::new(Vec::new());
    if let Some((_, s)) = CACHE
        .lock()
        .expect("spawn shrines")
        .iter()
        .find(|(k, _)| *k == key)
    {
        return *s;
    }
    let s = resolve();
    let mut cache = CACHE.lock().expect("spawn shrines");
    if cache.len() >= 64 {
        cache.clear();
    }
    cache.push((key, s));
    s
}
