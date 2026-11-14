//! A lava field: one hot fluid on its own surface grid of 1 m columns (a bed height, a lava depth
//! and a heat per column), its vents, and the near/far schedule (docs/proposals/VOLCANOES.md §6).
//!
//! **The law.** Lava is a viscous gravity current: the flow per metre of width across a face is
//! `g × h³ × slope / (3 × ν)` (the lubrication law for a thin layer, ν the kinematic viscosity),
//! through the depth `h` that stands above the higher bed less the fluid's yield depth (a Bingham
//! plug: thin lava does not move, so a front stands as a lobe). It is explicit and diffusive, so a
//! substep is at most `cfl × cell² / (4 × g h³ / 3ν)`. Water's grid (`dwell-sim` `water.rs`) moves
//! by the local-inertial law instead; lava's is slower by orders of magnitude for the same slope.
//!
//! **Hardening by rule** (owner, 2026-11-13: lava solidifies "when it stops moving, touches water,
//! or is a source block, not while flowing"; sources stay molten). A column's heat is its rest
//! clock: 1 while its lava flows (faster than `stillMs`: its throughflow over twice its depth), falling at
//! `1 / restSeconds` a second while it is still, and at 0 the column turns to the fluid's rock: its
//! lava is added to its bed and the cells it filled become the rock block, so the lava's volume
//! stays as land where it stopped. Flowing lava never hardens. A source never hardens by rest: a
//! vent's column, the lava joined to it through at most `sourceKeepM` m of lava columns (counted
//! along the connected columns, not in a straight line), and a lake vent's whole lake below its level
//! (a lava lake convects) are held at heat 1; what runs over the rim hardens once it stops.
//!
//! **Quench.** Lava in a water column turns to the quench rock at once, with a steam event (one per
//! quench row) that takes `steamWaterM` of that water (the water grid applies it once wired).
//!
//! **Schedule.** Like the water: a field with a player within `nearRadiusM` runs every lava tick;
//! a far one owes its time and runs it in one go once it owes `farEverySeconds` (fast-forward). A
//! run substeps at the stable step either way, so the two agree.
//!
//! **Tiles (V2).** A field's columns live in tiles of [`TILE`] × [`TILE`], keyed by tile, so it
//! covers the ground its lava actually occupies: a growing field ([`Field::growing`]) asks for the
//! tile past an edge its lava reaches (read from the world by the run's [`Ground`]), up to the data's
//! `maxTiles` (the cap on its area), and drops tiles that are all rock again ([`Field::trim`]). A
//! step only visits the tiles holding lava (molten) and the columns its flow reaches out of them, so
//! its cost follows the molten area, not the field's size. A fixed field ([`Field::new`], V1's
//! rectangle) never grows: its edges are walls.
//!
//! **The sea (V2).** A river stays molten as far as it flows (it never hardens while moving), and
//! its front, once it stops, hardens after `restSeconds`. Lava that runs into a water column quenches there, raising the sea floor; once the rock stands
//! above the water's surface the column is dry land: the coastline moves out.
//!
//! Face-local: a field's columns, beds and depths are face-local (+y up); a world point or cell
//! crosses through [`FaceFrame`] only ([`Field::submerged`], [`Field::is_near`], [`LavaEvent::cells`]).

use std::collections::{BTreeSet, HashMap};
use std::ops::Range;

use dwell_world::FaceFrame;

use crate::data::{HotFluid, LavaData, VentDef, VentKind};

const G: f32 = 9.81;

/// A tile's side (columns): a field grows and trims by whole tiles of `TILE × TILE`.
pub const TILE: usize = 8;
const TN: usize = TILE * TILE;
const NONE: u32 = u32::MAX;
/// The four directions: +x, −x, +z, −z.
const DX: [i32; 4] = [1, -1, 0, 0];
const DZ: [i32; 4] = [0, 0, 1, -1];

/// The cell row boundary nearest a face-local height (m): rows are `CELL_H` tall (½ m,
/// HALF_HEIGHT_GRID.md §2), so a column's lava and rock are written in whole rows.
pub fn row_at(m: f32) -> i32 {
    (f64::from(m) / dwell_world::CELL_H).round() as i32
}
/// A depth below which a column holds no lava (m).
const FILM: f32 = 1e-4;
/// No water over a column (its water's surface): above anything.
pub const DRY: f32 = f32::MAX;

/// A column of the world as a growing field reads it: its bed's top and its water's surface, if
/// water stands over it (face-local m).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    pub bed: f32,
    pub water: Option<f32>,
}

/// What a growing field reads a new tile's columns from: a field-local column (x, z) to its
/// [`Column`], `None` where there is no ground (the column is a wall). `near_m` is the bed of the
/// column asking (face-local m), to look for the ground round it.
pub type Ground<'a> = &'a mut dyn FnMut(i32, i32, f32) -> Option<Column>;

/// A vent in a field: its column (field-local) and what it does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vent {
    pub kind: VentKind,
    pub x: i32,
    pub z: i32,
    pub rate_m3s: f32,
    /// A lake's held surface (face-local m).
    pub level_m: f32,
}

impl From<&VentDef> for Vent {
    fn from(v: &VentDef) -> Self {
        Self {
            kind: v.kind,
            x: v.x as i32,
            z: v.z as i32,
            rate_m3s: v.rate_m3s,
            level_m: v.level_m,
        }
    }
}

/// What a step did that the world (blocks, the water grid, the clients) applies.
#[derive(Clone, Debug, PartialEq)]
pub enum LavaEvent {
    /// Lava turned to `rock` in a column (field-local): the face-local rows `rows` are now that
    /// block.
    Rock {
        x: i32,
        z: i32,
        rows: Range<i32>,
        rock: &'static str,
    },
    /// Lava met water in a column: `water_m` of the water's depth there becomes steam.
    Steam { x: i32, z: i32, water_m: f32 },
    /// A pond broke out through its crust dam: the face-local rows `rows` of the column's rock
    /// are lava again (the world clears that rock, then writes the lava).
    Melt { x: i32, z: i32, rows: Range<i32> },
}

impl LavaEvent {
    /// A rock event's world cells on face `f`, for the field at face-local `origin`.
    pub fn cells(&self, f: FaceFrame, origin: [i32; 2]) -> Vec<[i32; 3]> {
        match self {
            Self::Rock { x, z, rows, .. } | Self::Melt { x, z, rows } => rows
                .clone()
                .map(|r| f.cell([origin[0] + *x, r, origin[1] + *z]))
                .collect(),
            Self::Steam { .. } => Vec::new(),
        }
    }
}

/// A saved column: bed, depth, heat, water's surface, crust, water and open flags.
pub(crate) type SavedColumn = (f32, f32, f32, f32, f32, bool, bool);

/// A per-column vector, moved and grown a tile at a time.
trait Cols {
    fn add_tile(&mut self);
    fn move_tile(&mut self, from: usize, to: usize);
    fn pop_tile(&mut self);
}

impl<T: Clone + Default> Cols for Vec<T> {
    fn add_tile(&mut self) {
        self.resize(self.len() + TN, T::default());
    }
    fn move_tile(&mut self, from: usize, to: usize) {
        for r in 0..TN {
            self[to * TN + r] = self[from * TN + r].clone();
        }
    }
    fn pop_tile(&mut self) {
        self.truncate(self.len() - TN);
    }
}

/// One hot fluid's surface grid, in tiles.
#[derive(Clone, Debug)]
pub struct Field {
    /// The face-local x, z of field-local column (0, 0).
    pub origin: [i32; 2],
    /// Bed top (face-local m): the ground, raised as lava cools on it. Indexed by slot
    /// ([`Field::slot`], [`Field::at`]), as are the other per-column vectors.
    pub ground: Vec<f32>,
    /// Lava depth (m).
    pub depth: Vec<f32>,
    /// Heat, 1 at the vent to 0 (rock).
    pub heat: Vec<f32>,
    /// Columns where water stands (the water grid's, once wired).
    pub water: Vec<bool>,
    /// The water's surface over a water column (face-local m; [`DRY`] when unknown): quenched rock
    /// raised above it makes the column land.
    pub water_top: Vec<f32>,
    /// The rock this field's lava laid on each column and could melt again (m; the fluid's `rock`,
    /// not the quench's): a crust dam a pond breaks out through (`breakoutDepthM`).
    pub crust: Vec<f32>,
    /// Columns that are part of the field (a closed one is a wall: no ground, or past a fixed
    /// field's edge).
    pub open: Vec<bool>,
    /// The rows of each column last written into the world as lava (the server's).
    pub written: Vec<Range<i32>>,
    pub vents: Vec<Vent>,
    pub fluid: &'static HotFluid,
    pub rules: LavaData,
    /// Whether the field grows and trims ([`Field::growing`]).
    pub grows: bool,
    /// Lava time owed while far (s).
    pub owed_s: f32,
    /// m³ the vents brought, m³ turned to rock.
    pub brought: f64,
    pub rocked: f64,
    /// Tiles the cap refused (a growing field at `maxTiles`).
    pub refused: u64,
    /// Columns the steps visited (a cost meter: it follows the molten area), and the substeps.
    pub work: u64,
    pub substeps: u64,
    /// What steps did since the world last took them.
    pub events: Vec<LavaEvent>,
    tiles: Vec<[i32; 2]>,
    index: HashMap<[i32; 2], u32>,
    nb: Vec<[u32; 4]>,
    molten: Vec<bool>,
    touched: Vec<bool>,
    mid_y: Vec<f32>,
    want: BTreeSet<[i32; 2]>,
    qx: Vec<f32>,
    qz: Vec<f32>,
    scale: Vec<f32>,
    /// Each visited column's lava speed this substep (m/s): flowing above `stillMs`.
    through: Vec<f32>,
    /// This step's lake columns (held molten by their vent).
    held: Vec<bool>,
    held_list: Vec<usize>,
    outside: Vec<usize>,
    visit: Vec<usize>,
    /// Each column's open neighbour toward each direction (`NONE`: a wall or a missing tile).
    links: Vec<[u32; 4]>,
}

fn tile_of(x: i32, z: i32) -> [i32; 2] {
    [x.div_euclid(TILE as i32), z.div_euclid(TILE as i32)]
}

impl Field {
    /// An empty field of the fluid `fluid` (by name) under `rules`.
    fn empty(rules: &LavaData, fluid: &str, grows: bool) -> Result<Self, String> {
        let fluid = rules
            .fluid(fluid)
            .ok_or_else(|| format!("no hot fluid named {fluid:?}"))?;
        Ok(Self {
            origin: [0, 0],
            ground: Vec::new(),
            depth: Vec::new(),
            heat: Vec::new(),
            water: Vec::new(),
            water_top: Vec::new(),
            crust: Vec::new(),
            open: Vec::new(),
            written: Vec::new(),
            vents: Vec::new(),
            fluid,
            rules: *rules,
            grows,
            owed_s: 0.0,
            brought: 0.0,
            rocked: 0.0,
            refused: 0,
            work: 0,
            substeps: 0,
            events: Vec::new(),
            tiles: Vec::new(),
            index: HashMap::new(),
            nb: Vec::new(),
            molten: Vec::new(),
            touched: Vec::new(),
            mid_y: Vec::new(),
            want: BTreeSet::new(),
            qx: Vec::new(),
            qz: Vec::new(),
            scale: Vec::new(),
            through: Vec::new(),
            held: Vec::new(),
            held_list: Vec::new(),
            outside: Vec::new(),
            visit: Vec::new(),
            links: Vec::new(),
        })
    }

    /// A fixed dry field of `w × h` columns on flat ground at 0 (V1's rectangle: it never grows),
    /// of the fluid `fluid` (by name) under `rules`. An error names a fluid the data doesn't have.
    pub fn new(rules: &LavaData, fluid: &str, w: usize, h: usize) -> Result<Self, String> {
        let mut f = Self::empty(rules, fluid, false)?;
        let (w, h) = (w as i32, h as i32);
        for tz in 0..(h + TILE as i32 - 1) / TILE as i32 {
            for tx in 0..(w + TILE as i32 - 1) / TILE as i32 {
                f.insert_tile([tx, tz], &mut |x, z, _| {
                    (x < w && z < h).then_some(Column {
                        bed: 0.0,
                        water: None,
                    })
                });
            }
        }
        Ok(f)
    }

    /// An empty growing field (V2): tiles are added by [`Field::cover`] and as its lava reaches
    /// their edges in a run, up to `maxTiles`, and dropped once all rock ([`Field::trim`]).
    pub fn growing(rules: &LavaData, fluid: &str) -> Result<Self, String> {
        Self::empty(rules, fluid, true)
    }

    fn per_column(&mut self) -> [&mut dyn Cols; 14] {
        [
            &mut self.ground,
            &mut self.depth,
            &mut self.heat,
            &mut self.water,
            &mut self.water_top,
            &mut self.crust,
            &mut self.open,
            &mut self.written,
            &mut self.qx,
            &mut self.qz,
            &mut self.scale,
            &mut self.through,
            &mut self.held,
            &mut self.links,
        ]
    }

    /// Adds tile `t` with its columns read through `ground` (no cap). Returns its slot.
    fn insert_tile(
        &mut self,
        t: [i32; 2],
        ground: &mut dyn FnMut(i32, i32, f32) -> Option<Column>,
    ) -> usize {
        self.insert_tile_near(t, ground, 0.0)
    }

    fn insert_tile_near(
        &mut self,
        t: [i32; 2],
        ground: &mut dyn FnMut(i32, i32, f32) -> Option<Column>,
        near_m: f32,
    ) -> usize {
        let k = self.tiles.len();
        for c in self.per_column() {
            c.add_tile();
        }
        self.tiles.push(t);
        self.index.insert(t, k as u32);
        self.molten.push(false);
        self.touched.push(false);
        let mut nb = [NONE; 4];
        for d in 0..4 {
            if let Some(&n) = self.index.get(&[t[0] + DX[d], t[1] + DZ[d]]) {
                nb[d] = n;
                self.nb[n as usize][d ^ 1] = k as u32;
            }
        }
        self.nb.push(nb);
        let (mut sum, mut n) = (0.0f64, 0usize);
        for r in 0..TN {
            let i = k * TN + r;
            let (x, z) = (
                t[0] * TILE as i32 + (r % TILE) as i32,
                t[1] * TILE as i32 + (r / TILE) as i32,
            );
            self.water_top[i] = DRY;
            self.scale[i] = 1.0;
            if let Some(c) = ground(x, z, near_m) {
                self.open[i] = true;
                self.ground[i] = c.bed;
                self.water[i] = c.water.is_some_and(|w| w > c.bed);
                if let Some(w) = c.water {
                    self.water_top[i] = w;
                }
                let g = row_at(c.bed);
                self.written[i] = g..g;
                sum += f64::from(c.bed);
                n += 1;
            }
        }
        self.mid_y
            .push(if n > 0 { (sum / n as f64) as f32 } else { 0.0 });
        self.relink_round(k);
        k
    }

    /// Rebuilds the link cache of tile `k`'s columns.
    fn relink(&mut self, k: usize) {
        for i in k * TN..(k + 1) * TN {
            let mut l = [NONE; 4];
            for (d, l) in l.iter_mut().enumerate() {
                if let Ok(j) = self.nbr(i, d)
                    && self.open[i]
                    && self.open[j]
                {
                    *l = j as u32;
                }
            }
            self.links[i] = l;
        }
    }

    /// Rebuilds the link cache of tile `k` and its neighbours.
    fn relink_round(&mut self, k: usize) {
        self.relink(k);
        for d in 0..4 {
            let n = self.nb[k][d];
            if n != NONE {
                self.relink(n as usize);
            }
        }
    }

    /// A saved tile (its columns' bed, depth, heat, water's surface, water and open flags), for
    /// the save's reader. Returns its slot.
    pub(crate) fn load_tile(&mut self, t: [i32; 2], cols: &[SavedColumn]) -> usize {
        let k = self.insert_tile(t, &mut |_, _, _| None);
        let (mut sum, mut n) = (0.0f64, 0usize);
        for (r, &(g, d, h, top, crust, wet, open)) in cols.iter().enumerate().take(TN) {
            let i = k * TN + r;
            self.ground[i] = g;
            self.depth[i] = d;
            self.heat[i] = h;
            self.water_top[i] = top;
            self.crust[i] = crust;
            self.water[i] = wet;
            self.open[i] = open;
            self.written[i] = self.lava_rows(i);
            if open {
                sum += f64::from(g);
                n += 1;
            }
            self.molten[k] |= d > 0.0;
        }
        self.mid_y[k] = if n > 0 { (sum / n as f64) as f32 } else { 0.0 };
        self.relink_round(k);
        k
    }

    /// Removes tile slot `k` (the last tile moves into its slot).
    fn remove_tile(&mut self, k: usize) {
        // The held lake columns are reckoned again next step (their slots move).
        for i in std::mem::take(&mut self.held_list) {
            self.held[i] = false;
        }
        let last = self.tiles.len() - 1;
        let t = self.tiles[k];
        let mut round: Vec<[i32; 2]> = Vec::new();
        for d in 0..4 {
            let n = self.nb[k][d];
            if n != NONE {
                self.nb[n as usize][d ^ 1] = NONE;
                round.push(self.tiles[n as usize]);
            }
        }
        self.index.remove(&t);
        if k != last {
            for c in self.per_column() {
                c.move_tile(last, k);
            }
            let moved = self.tiles[last];
            self.tiles[k] = moved;
            self.index.insert(moved, k as u32);
            self.nb[k] = self.nb[last];
            self.molten[k] = self.molten[last];
            self.touched[k] = self.touched[last];
            self.mid_y[k] = self.mid_y[last];
            for d in 0..4 {
                let n = self.nb[k][d];
                if n != NONE {
                    self.nb[n as usize][d ^ 1] = k as u32;
                }
            }
        }
        for c in self.per_column() {
            c.pop_tile();
        }
        self.tiles.pop();
        self.nb.pop();
        self.molten.pop();
        self.touched.pop();
        self.mid_y.pop();
        if k < self.tiles.len() {
            // The moved tile's slots changed: its and its neighbours' links.
            self.relink_round(k);
        }
        for t in round {
            if let Some(&n) = self.index.get(&t) {
                self.relink(n as usize);
            }
        }
    }

    /// Adds the tiles covering field-local columns `x0..=x1`, `z0..=z1` that it lacks, read through
    /// `ground`, within the cap. Returns whether all fit.
    pub fn cover(
        &mut self,
        [x0, z0]: [i32; 2],
        [x1, z1]: [i32; 2],
        ground: Ground<'_>,
        near_m: f32,
    ) -> bool {
        let (a, b) = (tile_of(x0, z0), tile_of(x1, z1));
        let mut all = true;
        for tz in a[1]..=b[1] {
            for tx in a[0]..=b[0] {
                all &= self.grow_tile([tx, tz], ground, near_m);
            }
        }
        all
    }

    /// Recomputes each tile's molten flag and mean bed (after its columns were set directly).
    pub fn settle(&mut self) {
        for k in 0..self.tiles.len() {
            let (mut sum, mut n) = (0.0f64, 0usize);
            let mut molten = false;
            for i in k * TN..(k + 1) * TN {
                if self.open[i] {
                    sum += f64::from(self.ground[i]);
                    n += 1;
                    molten |= self.depth[i] > 0.0;
                }
            }
            self.molten[k] = molten;
            self.mid_y[k] = if n > 0 { (sum / n as f64) as f32 } else { 0.0 };
        }
    }

    /// Whether the field has tile `t`.
    pub fn has_tile(&self, t: [i32; 2]) -> bool {
        self.index.contains_key(&t)
    }

    /// Adds tile `t` if it lacks it and the cap allows; whether it is there now.
    fn grow_tile(&mut self, t: [i32; 2], ground: Ground<'_>, near_m: f32) -> bool {
        if self.index.contains_key(&t) {
            return true;
        }
        if self.tiles.len() >= self.rules.max_tiles as usize {
            self.refused += 1;
            return false;
        }
        self.insert_tile_near(t, ground, near_m);
        true
    }

    /// The tiles (tile x, z: field-local columns `TILE × t ..`), in slot order.
    pub fn tiles(&self) -> &[[i32; 2]] {
        &self.tiles
    }

    /// The tiles holding lava now.
    pub fn molten_tiles(&self) -> usize {
        self.molten.iter().filter(|&&m| m).count()
    }

    /// The slot of field-local column (x, z), if the field has its tile (open or not).
    pub fn slot_any(&self, x: i32, z: i32) -> Option<usize> {
        let t = tile_of(x, z);
        let k = *self.index.get(&t)? as usize;
        let (lx, lz) = (
            x.rem_euclid(TILE as i32) as usize,
            z.rem_euclid(TILE as i32) as usize,
        );
        Some(k * TN + lz * TILE + lx)
    }

    /// The slot of field-local column (x, z), if it is part of the field.
    pub fn slot(&self, x: i32, z: i32) -> Option<usize> {
        self.slot_any(x, z).filter(|&i| self.open[i])
    }

    /// The slot of field-local column (x, z) of the field (panics on one it lacks).
    pub fn at(&self, x: usize, z: usize) -> usize {
        self.slot(x as i32, z as i32)
            .unwrap_or_else(|| panic!("column ({x}, {z}) is not in the field"))
    }

    /// The field-local column of slot `i`.
    pub fn col(&self, i: usize) -> [i32; 2] {
        let t = self.tiles[i / TN];
        let r = i % TN;
        [
            t[0] * TILE as i32 + (r % TILE) as i32,
            t[1] * TILE as i32 + (r / TILE) as i32,
        ]
    }

    /// The field's columns (slots of open columns).
    pub fn columns(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.open.len()).filter(|&i| self.open[i])
    }

    /// The columns of tile slot `k`.
    pub fn tile_columns(&self, k: usize) -> impl Iterator<Item = usize> + '_ {
        (k * TN..(k + 1) * TN).filter(|&i| self.open[i])
    }

    /// The tiles any step visited since the last call (their slots): the ones whose lava may
    /// have changed.
    pub fn take_touched(&mut self) -> Vec<usize> {
        let out = (0..self.touched.len())
            .filter(|&k| self.touched[k])
            .collect();
        self.touched.iter_mut().for_each(|t| *t = false);
        out
    }

    /// Adds a vent from data; an error names a vent of another fluid or off the field.
    pub fn add_vent(&mut self, v: &VentDef) -> Result<(), String> {
        if v.fluid != self.fluid.name {
            return Err(format!(
                "vent {}: feeds {}, the field holds {}",
                v.name, v.fluid, self.fluid.name
            ));
        }
        if self.slot(v.x as i32, v.z as i32).is_none() {
            return Err(format!(
                "vent {}: column ({}, {}) is off the field",
                v.name, v.x, v.z
            ));
        }
        self.vents.push(Vent::from(v));
        Ok(())
    }

    /// The lava surface of column `i` (face-local m).
    pub fn surface(&self, i: usize) -> f32 {
        self.ground[i] + self.depth[i]
    }

    /// The face-local cell rows column `i`'s lava fills (empty when it holds none): from its bed's
    /// row boundary to its surface's, so a cell is lava once lava covers most of it.
    pub fn lava_rows(&self, i: usize) -> Range<i32> {
        if self.depth[i] <= 0.0 {
            let r = row_at(self.ground[i]);
            return r..r;
        }
        row_at(self.ground[i])..row_at(self.surface(i)).max(row_at(self.ground[i]))
    }

    /// The lava stored (m³).
    pub fn stored(&self) -> f64 {
        self.depth.iter().map(|&d| f64::from(d)).sum()
    }

    /// The depth of lava over the higher of two beds that moves (m), from the higher surface.
    fn moving(&self, up: usize, down: usize) -> f32 {
        let h = self.surface(up) - self.ground[up].max(self.ground[down]);
        (h.min(self.depth[up]) - self.fluid.yield_depth_m).max(0.0)
    }

    /// The flow across the face from `a` to `b` (m²/s, negative toward `a`).
    fn face(&self, a: usize, b: usize) -> f32 {
        let (ea, eb) = (self.surface(a), self.surface(b));
        let (up, down, sign) = if ea >= eb { (a, b, 1.0) } else { (b, a, -1.0) };
        let he = self.moving(up, down);
        if he <= 0.0 {
            return 0.0;
        }
        sign * G * he * he * he * (ea - eb).abs() / (3.0 * self.fluid.viscosity_m2s)
    }

    /// The longest stable substep now (s).
    pub fn max_step(&self) -> f32 {
        let mut deepest = 0.0f32;
        for k in 0..self.tiles.len() {
            if self.molten[k] {
                for i in k * TN..(k + 1) * TN {
                    deepest = deepest.max(self.depth[i]);
                }
            }
        }
        let he = (deepest.min(self.rules.step_depth_m) - self.fluid.yield_depth_m).max(0.05);
        let d = G * he * he * he / (3.0 * self.fluid.viscosity_m2s);
        (self.rules.cfl / (4.0 * d)).min(5.0)
    }

    /// Column `i`'s neighbour toward `d`: its slot (open or not), or the missing tile it is in.
    fn nbr(&self, i: usize, d: usize) -> Result<usize, [i32; 2]> {
        let k = i / TN;
        let r = i % TN;
        let (nx, nz) = ((r % TILE) as i32 + DX[d], (r / TILE) as i32 + DZ[d]);
        let t = TILE as i32;
        if (0..t).contains(&nx) && (0..t).contains(&nz) {
            return Ok(k * TN + nz as usize * TILE + nx as usize);
        }
        let n = self.nb[k][d];
        if n == NONE {
            let me = self.tiles[k];
            return Err([me[0] + DX[d], me[1] + DZ[d]]);
        }
        Ok(n as usize * TN + nz.rem_euclid(t) as usize * TILE + nx.rem_euclid(t) as usize)
    }

    /// Checks the link cache and tile links against the tiles (tests).
    pub fn check_links(&self) -> Result<(), String> {
        for i in 0..self.open.len() {
            for d in 0..4 {
                let want = self
                    .nbr(i, d)
                    .ok()
                    .filter(|&j| self.open[i] && self.open[j])
                    .map_or(NONE, |j| j as u32);
                if self.links[i][d] != want {
                    return Err(format!(
                        "column {:?} toward {d}: {} not {want}",
                        self.col(i),
                        self.links[i][d]
                    ));
                }
            }
        }
        for (k, t) in self.tiles.iter().enumerate() {
            if self.index.get(t) != Some(&(k as u32)) {
                return Err(format!("tile {t:?} index"));
            }
            for d in 0..4 {
                let want = self
                    .index
                    .get(&[t[0] + DX[d], t[1] + DZ[d]])
                    .copied()
                    .unwrap_or(NONE);
                if self.nb[k][d] != want {
                    return Err(format!(
                        "tile {t:?} toward {d}: {} not {want}",
                        self.nb[k][d]
                    ));
                }
            }
        }
        Ok(())
    }

    /// Column `i`'s open neighbour toward `d`.
    #[inline]
    fn link(&self, i: usize, d: usize) -> Option<usize> {
        let j = self.links[i][d];
        (j != NONE).then_some(j as usize)
    }

    /// Turns column `i`'s lava into `rock`, raising its bed. Whether whole rows turned (an event).
    fn solidify(&mut self, i: usize, rock: &'static str) -> bool {
        let d = self.depth[i];
        if d <= 0.0 {
            return false;
        }
        let from = self.ground[i];
        self.ground[i] += d;
        self.depth[i] = 0.0;
        self.heat[i] = 0.0;
        self.rocked += f64::from(d);
        if rock == self.fluid.rock {
            self.crust[i] += d;
        }
        let rows = row_at(from)..row_at(self.ground[i]);
        if rows.is_empty() {
            return false;
        }
        let [x, z] = self.col(i);
        self.events.push(LavaEvent::Rock { x, z, rows, rock });
        true
    }

    /// One substep of `dt` lava seconds (at most [`Field::max_step`]). Visits the molten tiles'
    /// columns and the columns their flow reaches, no others.
    pub fn step(&mut self, dt: f32) {
        for k in 0..self.vents.len() {
            let v = self.vents[k];
            let Some(i) = self.slot(v.x, v.z) else {
                continue;
            };
            let add = match v.kind {
                VentKind::River => v.rate_m3s * dt,
                VentKind::Lake => (v.level_m - self.surface(i)).clamp(0.0, v.rate_m3s * dt),
            };
            if add > 0.0 {
                self.depth[i] += add;
                self.brought += f64::from(add);
            }
            // The vent's own column is as hot as the vent: a lake's stays molten as it crusts.
            if self.depth[i] > 0.0 {
                self.heat[i] = 1.0;
                self.molten[i / TN] = true;
            }
        }
        let act: Vec<usize> = (0..self.tiles.len()).filter(|&k| self.molten[k]).collect();
        let mut outside = std::mem::take(&mut self.outside);
        outside.clear();
        let mut want = std::mem::take(&mut self.want);
        // The faces: each column's +x and +z face, and its −x and −z ones into a dry tile (whose
        // columns are not visited), so every face with lava on a side is reckoned once.
        // The flux limit: no face moves more than the explicit law's stable share of the two
        // surfaces' difference in a substep (it binds only where lava is deeper than a step is
        // sized for, `stepDepthM`: a pond levels instead of ringing).
        let lim = self.rules.cfl / (4.0 * dt);
        for &k in &act {
            for i in k * TN..(k + 1) * TN {
                if !self.open[i] {
                    continue;
                }
                let wet = self.depth[i] > 0.0;
                let l = self.links[i];
                for (d, &j) in l.iter().enumerate() {
                    if j == NONE {
                        if wet
                            && self.grows
                            && let Err(t) = self.nbr(i, d)
                        {
                            want.insert(t);
                        }
                        continue;
                    }
                    let j = j as usize;
                    let inner = self.molten[j / TN];
                    if d % 2 == 0 {
                        if wet || self.depth[j] > 0.0 {
                            let cap = (self.surface(i) - self.surface(j)).abs() * lim;
                            let q = self.face(i, j).clamp(-cap, cap);
                            if d == 0 {
                                self.qx[i] = q;
                            } else {
                                self.qz[i] = q;
                            }
                            if !inner && q != 0.0 {
                                outside.push(j);
                            }
                        }
                    } else if !inner && wet {
                        let cap = (self.surface(i) - self.surface(j)).abs() * lim;
                        let q = self.face(j, i).clamp(-cap, cap);
                        if d == 1 {
                            self.qx[j] = q;
                        } else {
                            self.qz[j] = q;
                        }
                        if q != 0.0 {
                            outside.push(j);
                        }
                    }
                }
            }
        }
        self.want = want;
        outside.sort_unstable();
        outside.dedup();
        let mut visit = std::mem::take(&mut self.visit);
        visit.clear();
        for &k in &act {
            visit.extend((k * TN..(k + 1) * TN).filter(|&i| self.open[i]));
        }
        visit.extend_from_slice(&outside);
        self.work += visit.len() as u64;
        self.substeps += 1;
        // No column sends more than it holds.
        for &i in &visit {
            let mut out = self.qx[i].max(0.0) + self.qz[i].max(0.0);
            if let Some(w) = self.link(i, 1) {
                out += (-self.qx[w]).max(0.0);
            }
            if let Some(n) = self.link(i, 3) {
                out += (-self.qz[n]).max(0.0);
            }
            let sent = out * dt;
            let d = self.depth[i];
            self.scale[i] = if sent > d { d / sent } else { 1.0 };
        }
        for &i in &visit {
            if self.qx[i] != 0.0 {
                self.qx[i] *= if self.qx[i] > 0.0 {
                    self.scale[i]
                } else {
                    self.link(i, 0).map_or(1.0, |j| self.scale[j])
                };
            }
            if self.qz[i] != 0.0 {
                self.qz[i] *= if self.qz[i] > 0.0 {
                    self.scale[i]
                } else {
                    self.link(i, 2).map_or(1.0, |j| self.scale[j])
                };
            }
        }
        // Each column's throughflow (m³/s through its faces), over twice its depth once moved: its
        // speed, whether it is flowing.
        for &i in &visit {
            let mut through = self.qx[i].abs() + self.qz[i].abs();
            if let Some(w) = self.link(i, 1) {
                through += self.qx[w].abs();
            }
            if let Some(n) = self.link(i, 3) {
                through += self.qz[n].abs();
            }
            self.through[i] = through;
        }
        for &i in &visit {
            for (d, q) in [(0, self.qx[i]), (2, self.qz[i])] {
                if q == 0.0 {
                    continue;
                }
                let Some(j) = self.link(i, d) else {
                    continue;
                };
                let v = q * dt;
                self.depth[i] -= v;
                self.depth[j] += v;
            }
        }
        for &i in &visit {
            let d = self.depth[i];
            if d <= FILM {
                // A film too thin to hold: it cools on the bed.
                self.ground[i] += d.max(0.0);
                self.rocked += f64::from(d.max(0.0));
                self.crust[i] += d.max(0.0);
                self.depth[i] = 0.0;
                self.heat[i] = 0.0;
                self.through[i] = 0.0;
            } else {
                self.through[i] /= 2.0 * d;
            }
            self.qx[i] = 0.0;
            self.qz[i] = 0.0;
        }
        self.hold_sources();
        // Quench, then hardening by rest.
        let fl = self.fluid;
        for &i in &visit {
            if self.depth[i] <= 0.0 {
                continue;
            }
            if self.water[i] {
                let rose = self.solidify(i, fl.quench.rock);
                if self.ground[i] >= self.water_top[i] {
                    // Built up out of the water: new land.
                    self.water[i] = false;
                }
                if rose {
                    let [x, z] = self.col(i);
                    self.events.push(LavaEvent::Steam {
                        x,
                        z,
                        water_m: fl.quench.steam_water_m,
                    });
                }
                continue;
            }
            // A source's lava, and flowing lava, stay molten: the rest clock starts over.
            if self.held[i] || self.through[i] > fl.still_ms {
                self.heat[i] = 1.0;
                continue;
            }
            self.heat[i] -= dt / fl.rest_seconds;
            if self.heat[i] <= 0.0 {
                self.solidify(i, fl.rock);
            }
        }
        self.break_out(&visit);
        for &i in &outside {
            self.molten[i / TN] |= self.depth[i] > 0.0;
            self.touched[i / TN] = true;
        }
        for &k in &act {
            self.touched[k] = true;
            self.molten[k] = (k * TN..(k + 1) * TN).any(|i| self.depth[i] > 0.0);
        }
        self.outside = outside;
        self.visit = visit;
    }

    /// A pond breaks its crust dam (`breakoutDepthM`, 0: never): molten lava at least that deep
    /// beside a dry column of this field's own crust standing over its bed, on lower ground than the
    /// pond's (its way down; a levee beside the channel holds), melts that crust down to its bed
    /// again, at the pond's heat, so the lava runs on through the gap instead of freezing its
    /// overflow into an ever higher wall. Volume is kept (the crust's rock becomes its lava).
    fn break_out(&mut self, visit: &[usize]) {
        let need = self.fluid.breakout_depth_m;
        if need <= 0.0 {
            return;
        }
        for &i in visit {
            if self.depth[i] < need || self.heat[i] <= 0.0 {
                continue;
            }
            for d in 0..4 {
                let Some(j) = self.link(i, d) else {
                    continue;
                };
                let rise = self.ground[j] - self.ground[i];
                if self.depth[j] > 0.0 || self.crust[j] <= 0.0 || rise <= 0.0 {
                    continue;
                }
                // Only a dam on the pond's way down: the ground under the crust lower than under
                // the pond. A levee beside the channel stands on ground as high as the channel's,
                // so it holds and the river stays in its banks.
                if self.ground[j] - self.crust[j] >= self.ground[i] - self.crust[i] {
                    continue;
                }
                let melt = self.crust[j].min(rise);
                let top = self.ground[j];
                self.ground[j] -= melt;
                self.crust[j] -= melt;
                self.depth[j] = melt;
                self.heat[j] = self.heat[i];
                self.rocked -= f64::from(melt);
                self.molten[j / TN] = true;
                self.touched[j / TN] = true;
                let rows = row_at(self.ground[j])..row_at(top);
                if !rows.is_empty() {
                    let [x, z] = self.col(j);
                    self.events.push(LavaEvent::Melt { x, z, rows });
                }
            }
        }
    }

    /// The sources' lava stays molten: every vent's column and the molten columns joined to it
    /// through at most `sourceKeepM` m of molten columns (steps along the connected lava), and a
    /// lake vent's lake (it convects): every molten column joined to the vent through columns whose
    /// bed is below the lake's level. A flow over the rim leaves the lake there and hardens once it
    /// stops, as any other.
    fn hold_sources(&mut self) {
        for i in std::mem::take(&mut self.held_list) {
            self.held[i] = false;
        }
        let mut list = Vec::new();
        for k in 0..self.vents.len() {
            let v = self.vents[k];
            if v.kind != VentKind::Lake {
                continue;
            }
            let Some(start) = self.slot(v.x, v.z) else {
                continue;
            };
            if self.depth[start] <= 0.0 || self.held[start] {
                continue;
            }
            let mut stack = vec![start];
            self.held[start] = true;
            list.push(start);
            while let Some(i) = stack.pop() {
                self.heat[i] = 1.0;
                for d in 0..4 {
                    let Some(j) = self.link(i, d) else {
                        continue;
                    };
                    if !self.held[j] && self.depth[j] > 0.0 && self.ground[j] < v.level_m {
                        self.held[j] = true;
                        list.push(j);
                        stack.push(j);
                    }
                }
            }
        }
        // Within `sourceKeepM` of any vent, along the connected lava (breadth first, so a column's
        // distance is its shortest path).
        let keep = self.fluid.source_keep_m.max(0.0).floor() as u32;
        let mut seen: HashMap<usize, u32> = HashMap::new();
        let mut queue = std::collections::VecDeque::new();
        for k in 0..self.vents.len() {
            let v = self.vents[k];
            if let Some(i) = self.slot(v.x, v.z)
                && self.depth[i] > 0.0
                && !seen.contains_key(&i)
            {
                seen.insert(i, 0);
                queue.push_back(i);
            }
        }
        while let Some(i) = queue.pop_front() {
            let at = seen[&i];
            if !self.held[i] {
                self.held[i] = true;
                list.push(i);
            }
            self.heat[i] = 1.0;
            if at >= keep {
                continue;
            }
            for d in 0..4 {
                let Some(j) = self.link(i, d) else {
                    continue;
                };
                if self.depth[j] > 0.0 && !seen.contains_key(&j) {
                    seen.insert(j, at + 1);
                    queue.push_back(j);
                }
            }
        }
        self.held_list = list;
    }

    /// Runs `t` lava seconds in stable substeps (at most `maxSubsteps`; the rest is lost), on a
    /// fixed footprint. Returns the lava seconds run.
    pub fn run(&mut self, t: f32) -> f32 {
        self.run_on(t, None)
    }

    /// Runs `t` lava seconds, a growing field adding the tiles its lava reaches through `ground`
    /// (none: its edges are walls this run) and dropping the ones all rock at the end.
    pub fn run_on(&mut self, t: f32, mut ground: Option<Ground<'_>>) -> f32 {
        let mut left = t;
        let mut n = 0;
        while left > 1e-6 && n < self.rules.max_substeps {
            let dt = self.max_step().min(left);
            self.step(dt);
            left -= dt;
            n += 1;
            if !self.want.is_empty() {
                let want = std::mem::take(&mut self.want);
                if let Some(g) = ground.as_deref_mut() {
                    for t in want {
                        let near = self.near_bed(t);
                        self.grow_tile(t, g, near);
                    }
                }
            }
        }
        if self.grows {
            self.trim();
        }
        t - left
    }

    /// A bed beside missing tile `t` (the mean of its present neighbours'), to read it round.
    fn near_bed(&self, t: [i32; 2]) -> f32 {
        (0..4)
            .find_map(|d| self.index.get(&[t[0] - DX[d], t[1] - DZ[d]]))
            .map_or(0.0, |&k| self.mid_y[k as usize])
    }

    /// Drops the tiles that are all rock: no lava in them or beside them, no vent, nothing written
    /// as lava left in the world. Returns how many.
    pub fn trim(&mut self) -> usize {
        let vents: BTreeSet<[i32; 2]> = self.vents.iter().map(|v| tile_of(v.x, v.z)).collect();
        let mut n = 0;
        for k in (0..self.tiles.len()).rev() {
            if self.molten[k]
                || vents.contains(&self.tiles[k])
                || self.nb[k]
                    .iter()
                    .any(|&j| j != NONE && self.molten[j as usize])
                || (k * TN..(k + 1) * TN).any(|i| !self.written[i].is_empty())
            {
                continue;
            }
            self.remove_tile(k);
            n += 1;
        }
        n
    }

    /// A lava tick of `real_s` real seconds: run now when near a player, else owe it and run what
    /// is owed in one go once it reaches `farEverySeconds`. Returns the lava seconds run.
    pub fn advance(&mut self, real_s: f32, near: bool) -> f32 {
        self.advance_on(real_s, near, None)
    }

    /// [`Field::advance`], growing through `ground` ([`Field::run_on`]).
    pub fn advance_on(&mut self, real_s: f32, near: bool, ground: Option<Ground<'_>>) -> f32 {
        self.owed_s += real_s * self.rules.speed;
        if near || self.owed_s >= self.rules.far_every_seconds {
            let t = std::mem::take(&mut self.owed_s);
            self.run_on(t, ground)
        } else {
            0.0
        }
    }

    /// Whether a player (world points) is within `nearRadiusM` of any of the field's tiles on face
    /// `f`.
    pub fn is_near(&self, f: FaceFrame, players: &[[f64; 3]]) -> bool {
        let r = f64::from(self.rules.near_radius_m);
        let s = TILE as f64;
        players.iter().any(|&p| {
            let [x, y, z] = f.point(p);
            let (x, z) = (x - f64::from(self.origin[0]), z - f64::from(self.origin[1]));
            self.tiles.iter().zip(&self.mid_y).any(|(t, &my)| {
                let (x0, z0) = (f64::from(t[0]) * s, f64::from(t[1]) * s);
                let dx = (x0 - x).max(0.0).max(x - (x0 + s));
                let dz = (z0 - z).max(0.0).max(z - (z0 + s));
                let dy = y - f64::from(my);
                dx * dx + dy * dy + dz * dz <= r * r
            })
        })
    }

    /// Whether a player's feet (a world point on face `f`) stand in this field's lava.
    pub fn submerged(&self, f: FaceFrame, feet: [f64; 3]) -> bool {
        let [x, y, z] = f.point(feet);
        let cx = x.floor() as i64 - i64::from(self.origin[0]);
        let cz = z.floor() as i64 - i64::from(self.origin[1]);
        let (Ok(cx), Ok(cz)) = (i32::try_from(cx), i32::try_from(cz)) else {
            return false;
        };
        let Some(i) = self.slot(cx, cz) else {
            return false;
        };
        let y = y as f32;
        self.depth[i] > 0.0 && y >= self.ground[i] - 0.25 && y < self.surface(i)
    }

    /// The events since the last take.
    pub fn take_events(&mut self) -> Vec<LavaEvent> {
        std::mem::take(&mut self.events)
    }
}
