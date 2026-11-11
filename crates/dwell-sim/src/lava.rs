//! The server's lava (docs/proposals/VOLCANOES.md §6, V1 slice 2): the world's lava fields
//! ([`dwell_lava::Field`]), each on one face, placed round a vent ([`Lava::place`]), ticked like the
//! water grid (every lava tick near a player, fast-forwarded elsewhere: [`Field::advance`]), and
//! written into the world as blocks so clients get them through the chunk and edit sync:
//!
//! - **Lava cells.** Each column's molten rows ([`Field::lava_rows`]) are the fluid's block (its
//!   name, `dwell:lava`), written only into open cells (air, or a replaceable non-solid block) and
//!   cleared to air where the lava has gone.
//! - **Rock.** A `Rock` event writes its rows as the rock (`dwell:basalt`, or `dwell:obsidian` at
//!   the quench edge): new land. A `Melt` event (a pond breaking its crust dam) clears its rows of
//!   that rock again, and the lava is written there.
//! - **Steam.** A `Steam` event takes `steamWaterM` off the top of that column's water (cells to
//!   air) and is kept as a [`Steam`] for the clients to show later.
//!
//! A field grows with its flow (V2, VOLCANOES.md §6.4): placed covering `radius` columns round its
//! vent, it reads each tile its lava reaches from the world as it runs ([`ground_at`]), up to the
//! data's `maxTiles`, and drops the tiles that are all rock. A tick writes only the tiles its steps
//! visited ([`Field::take_touched`]).
//!
//! A field's columns, beds and rows are face-local; every world cell and point crosses through its
//! [`FaceFrame`]. Saved as one record (`to_bytes`, the world's optional `lava` table).

use std::collections::{BTreeSet, HashMap};

use dwell_blocks::{StateId, info};
pub use dwell_lava::VentKind;
use dwell_lava::{Column, Field, HotFluid, LavaData, LavaEvent, Vent, field_of, record_of};
use dwell_world::{CELL_H, Face, FaceFrame};

const VERSION: u8 = 1;
/// How far above and below a vent's row its field's beds are looked for (rows).
const SCAN_ROWS: i32 = 96;

/// A vent to place (a dev command, a test world, V2's volcanoes): what it is and its field's size.
#[derive(Clone, Debug, PartialEq)]
pub struct VentSpot {
    /// The hot fluid (`shared/fluids/lava.json` `fluids`), by name.
    pub fluid: String,
    pub kind: VentKind,
    pub rate_m3s: f32,
    /// A lake's held surface over the vent column's bed (m); unused for a river.
    pub depth_m: f32,
    /// The field starts reaching this many columns each way from the vent (it grows with its
    /// flow from there).
    pub radius: usize,
}

/// Water turned to steam (the clients' puff, later): the world cell of the water's top it took.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Steam {
    pub cell: [i32; 3],
    pub water_m: f32,
}

/// One field on its face (the rows each column last wrote as lava are the field's `written`).
#[derive(Clone, Debug)]
pub struct Placed {
    pub face: FaceFrame,
    pub field: Field,
    /// Columns (field-local) the world edited since the last lava tick (not by the lava): their
    /// bed and water are read again then ([`Lava::touch`]).
    dirty: BTreeSet<[i32; 2]>,
}

/// World column (`wx`, `wz`) on face `f` as a field reads it: the top solid cell's top within
/// `SCAN_ROWS` of face-local row `row` (looked for from above), and the surface of any liquid over
/// it but `lava`'s own cells. `None`: no ground there (a wall).
fn ground_at(
    f: FaceFrame,
    [wx, wz]: [i32; 2],
    row: i32,
    lava: StateId,
    mut read: impl FnMut([i32; 3]) -> StateId,
) -> Option<Column> {
    let mut water = None;
    for local in (row - SCAN_ROWS..=row + SCAN_ROWS).rev() {
        let m = read(f.cell([wx, local, wz]));
        let s = info(m);
        if s.solid {
            return Some(Column {
                bed: ((f64::from(local) + 1.0) * CELL_H) as f32,
                water,
            });
        }
        if s.liquid && !same_block(m, lava) && water.is_none() {
            water = Some(((f64::from(local) + 1.0) * CELL_H) as f32);
        }
    }
    None
}

/// What a lava tick did to the world: cell writes (world cells, world states) and steam.
#[derive(Debug, Default)]
pub struct Out {
    pub writes: Vec<([i32; 3], StateId)>,
    pub steam: Vec<Steam>,
}

/// The world's lava fields.
#[derive(Clone, Debug, Default)]
pub struct Lava {
    pub fields: Vec<Placed>,
}

/// Whether lava may be written into a cell holding `m`: open (air or a replaceable non-solid
/// block), or lava already.
fn open_for_lava(m: StateId, lava: StateId) -> bool {
    let s = info(m);
    m == 0 || same_block(m, lava) || (s.replaceable && !s.solid && !s.liquid)
}

/// Whether two states are of one block (lava's cells are any of its `heat` states).
fn same_block(a: StateId, b: StateId) -> bool {
    info(a).block == info(b).block
}

/// The fluid's block's state for each heat stop (`dwell_lava::DATA.heat`, VOLCANOES.md §6.5): its
/// `heat` property set to the stop's name, or the block's own state for a fluid whose block has no
/// such property (each stop then writes the same state).
pub fn heat_states(fluid: &str) -> Vec<StateId> {
    let base = dwell_blocks::require(fluid);
    dwell_lava::DATA
        .heat
        .iter()
        .map(|h| dwell_blocks::state(&format!("{fluid}[heat={}]", h.name)).unwrap_or(base))
        .collect()
}

/// The world's reads during a tick, with this tick's writes over them.
struct View<'a, R: FnMut([i32; 3]) -> StateId> {
    read: &'a mut R,
    out: &'a mut Out,
    at: HashMap<[i32; 3], usize>,
}

impl<R: FnMut([i32; 3]) -> StateId> View<'_, R> {
    fn get(&mut self, c: [i32; 3]) -> StateId {
        match self.at.get(&c) {
            Some(&i) => self.out.writes[i].1,
            None => (self.read)(c),
        }
    }
    fn set(&mut self, c: [i32; 3], m: StateId) {
        match self.at.get(&c) {
            Some(&i) => self.out.writes[i].1 = m,
            None => {
                self.at.insert(c, self.out.writes.len());
                self.out.writes.push((c, m));
            }
        }
    }
}

impl Lava {
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Places a vent at world column `column` (x, z) on face `f`, its bed looked for round the
    /// face-local row `row` (the vent's own cell or above it): a new growing field with the vent at
    /// its column (0, 0), covering `radius` columns each way to start, its beds and water read from
    /// the world through `read` (world cells to states). Returns the field's index; an error names
    /// an unknown fluid, a fluid with no block of its name, or a vent column with no ground.
    pub fn place(
        &mut self,
        rules: &LavaData,
        f: FaceFrame,
        column: [i32; 2],
        row: i32,
        spot: &VentSpot,
        mut read: impl FnMut([i32; 3]) -> StateId,
    ) -> Result<usize, String> {
        let mut field = Field::growing(rules, &spot.fluid)?;
        let Some(lava) = dwell_blocks::state(field.fluid.name) else {
            return Err(format!(
                "hot fluid {}: no block of that name to write its cells",
                field.fluid.name
            ));
        };
        field.origin = column;
        if ground_at(f, column, row, lava, &mut read).is_none() {
            return Err(format!(
                "no ground under column ({}, {}) within {SCAN_ROWS} rows of row {row}",
                column[0], column[1]
            ));
        }
        let r = spot.radius as i32;
        let mut ground = |x: i32, z: i32, _: f32| {
            ground_at(f, [column[0] + x, column[1] + z], row, lava, &mut read)
        };
        field.cover([-r, -r], [r, r], &mut ground, 0.0);
        let Some(at) = field.slot(0, 0) else {
            return Err("the vent's column is not in its field".into());
        };
        let bed = field.ground[at];
        field.vents.push(Vent {
            kind: spot.kind,
            x: 0,
            z: 0,
            rate_m3s: spot.rate_m3s,
            level_m: match spot.kind {
                VentKind::Lake => bed + spot.depth_m,
                VentKind::River => 0.0,
            },
        });
        self.fields.push(Placed {
            face: f,
            field,
            dirty: BTreeSet::new(),
        });
        Ok(self.fields.len() - 1)
    }

    /// One lava tick of `real_s` real seconds: each field run (near a player) or owed (far, until
    /// it owes `farEverySeconds`), and what changed written. `players` are world points; `read`
    /// reads a world cell (loading its chunk).
    pub fn tick(
        &mut self,
        real_s: f32,
        players: &[[f64; 3]],
        mut read: impl FnMut([i32; 3]) -> StateId,
    ) -> Out {
        let mut out = Out::default();
        let mut view = View {
            read: &mut read,
            out: &mut out,
            at: HashMap::new(),
        };
        for p in &mut self.fields {
            p.refresh(&mut view);
            let near = p.field.is_near(p.face, players);
            let (f, origin) = (p.face, p.field.origin);
            let lava = dwell_blocks::require(p.field.fluid.name);
            // A tile the lava reaches, read from the world round the bed beside it.
            let mut ground = |x: i32, z: i32, near_m: f32| {
                let row = dwell_lava::field::row_at(near_m);
                ground_at(f, [origin[0] + x, origin[1] + z], row, lava, |c| {
                    view.get(c)
                })
            };
            if p.field.advance_on(real_s, near, Some(&mut ground)) <= 0.0 {
                continue;
            }
            p.apply(&mut view);
        }
        out
    }

    /// A world cell the world changed (an edit, the water; not the lava's own writes): a field
    /// over its column reads that column's bed and water again at its next lava tick, so a dug
    /// channel or poured water changes the flow.
    pub fn touch(&mut self, cell: [i32; 3]) {
        let f = FaceFrame::at_row(cell[1]);
        for p in &mut self.fields {
            if p.face != f {
                continue;
            }
            let (x, z) = (cell[0] - p.field.origin[0], cell[2] - p.field.origin[1]);
            if p.field.slot(x, z).is_some() {
                p.dirty.insert([x, z]);
            }
        }
    }

    /// The hot fluid a player's feet (a world point) stand in, if any.
    pub fn fluid_at(&self, feet: [f64; 3]) -> Option<&'static HotFluid> {
        let f = FaceFrame::at_height(feet[1]);
        self.fields
            .iter()
            .find(|p| p.face == f && p.field.submerged(f, feet))
            .map(|p| p.field.fluid)
    }

    /// The saved record: a version, then each field's face and its `dwell_lava` record.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = vec![VERSION];
        out.extend_from_slice(&(self.fields.len() as u16).to_le_bytes());
        for p in &self.fields {
            out.push(u8::from(p.face.is_b()));
            let r = record_of(&p.field);
            out.extend_from_slice(&(r.len() as u32).to_le_bytes());
            out.extend_from_slice(&r);
        }
        out
    }

    /// The fields a record holds, under `rules` (`None`: another version or damaged; a field of a
    /// fluid this build lacks reads as `None` too, so the record is kept, not written over).
    pub fn from_bytes(b: &[u8], rules: &LavaData) -> Option<Self> {
        let (&v, mut rest) = b.split_first()?;
        if v != VERSION || rest.len() < 2 {
            return None;
        }
        let n = u16::from_le_bytes([rest[0], rest[1]]);
        rest = &rest[2..];
        let mut fields = Vec::new();
        for _ in 0..n {
            if rest.len() < 5 {
                return None;
            }
            let face = match rest[0] {
                0 => FaceFrame::of(Face::A),
                1 => FaceFrame::of(Face::B),
                _ => return None,
            };
            let len = u32::from_le_bytes(rest[1..5].try_into().ok()?) as usize;
            let record = rest.get(5..5 + len)?;
            rest = &rest[5 + len..];
            let field = field_of(record, rules)?;
            fields.push(Placed {
                face,
                field,
                dirty: BTreeSet::new(),
            });
        }
        rest.is_empty().then_some(Self { fields })
    }
}

impl Placed {
    /// Reads each edited column's bed and water again: the bed from the solid run its old top
    /// cell is in (up through blocks set on it, down through cells dug out of it), its water from
    /// the liquid over it (not the lava's own cells). A raised bed pushes the lava above it (its
    /// depth over the new bed, none if buried); a lowered one keeps the lava's depth, so the
    /// column's surface drops and its neighbours flow in. A bed that keeps its top cell keeps its
    /// height (cooled rock's part of a cell).
    fn refresh<R: FnMut([i32; 3]) -> StateId>(&mut self, view: &mut View<'_, R>) {
        if self.dirty.is_empty() {
            return;
        }
        let f = self.face;
        let lava = f.state(dwell_blocks::require(self.field.fluid.name));
        let solid = |view: &mut View<'_, R>, c: [i32; 3]| info(view.get(f.cell(c))).solid;
        for [x, z] in std::mem::take(&mut self.dirty) {
            let Some(i) = self.field.slot(x, z) else {
                continue;
            };
            let (wx, wz) = (self.field.origin[0] + x, self.field.origin[1] + z);
            let old = dwell_lava::field::row_at(self.field.ground[i]);
            let mut bed = old;
            if solid(view, [wx, old - 1, wz]) {
                while bed < old + SCAN_ROWS && solid(view, [wx, bed, wz]) {
                    bed += 1;
                }
            } else {
                while bed > old - SCAN_ROWS && !solid(view, [wx, bed - 1, wz]) {
                    bed -= 1;
                }
            }
            if bed != old {
                let surface = self.field.surface(i);
                let ground = (f64::from(bed) * CELL_H) as f32;
                if ground > self.field.ground[i] {
                    self.field.depth[i] = (surface - ground).max(0.0);
                }
                self.field.ground[i] = ground;
            }
            let top = (bed..bed + SCAN_ROWS).rev().find(|&r| {
                let m = view.get(f.cell([wx, r, wz]));
                !same_block(m, lava) && info(m).liquid
            });
            self.field.water[i] = top.is_some();
            self.field.water_top[i] = top.map_or(dwell_lava::field::DRY, |r| {
                (f64::from(r + 1) * CELL_H) as f32
            });
        }
    }

    /// Writes what the field's last run did: its rock, its lava cells, its steam.
    fn apply<R: FnMut([i32; 3]) -> StateId>(&mut self, view: &mut View<'_, R>) {
        let (f, origin) = (self.face, self.field.origin);
        let lava = f.state(dwell_blocks::require(self.field.fluid.name));
        let air = dwell_blocks::AIR;
        for e in self.field.take_events() {
            match &e {
                LavaEvent::Rock { rock, .. } => {
                    let m = f.state(dwell_blocks::require(rock));
                    for c in e.cells(f, origin) {
                        view.set(c, m);
                    }
                }
                LavaEvent::Melt { .. } => {
                    // The crust melted again: its rock (and only its rock) cleared for the lava.
                    let rock = f.state(self.field.fluid.rock_state());
                    for c in e.cells(f, origin) {
                        if view.get(c) == rock {
                            view.set(c, air);
                        }
                    }
                }
                LavaEvent::Steam { x, z, water_m } => {
                    let Some(i) = self.field.slot(*x, *z) else {
                        continue;
                    };
                    let (wx, wz) = (origin[0] + *x, origin[1] + *z);
                    let bed = dwell_lava::field::row_at(self.field.ground[i]);
                    // The water's top in the column, then as many rows as the steam takes.
                    let top = (bed..bed + SCAN_ROWS)
                        .rev()
                        .find(|&r| info(view.get(f.cell([wx, r, wz]))).liquid);
                    let Some(top) = top else {
                        self.field.water[i] = false;
                        continue;
                    };
                    let rows = ((f64::from(*water_m) / CELL_H).ceil() as i32).max(1);
                    for r in (top - rows + 1..=top).rev() {
                        let c = f.cell([wx, r, wz]);
                        if info(view.get(c)).liquid {
                            view.set(c, air);
                        }
                    }
                    if !info(view.get(f.cell([wx, top - rows, wz]))).liquid {
                        self.field.water[i] = false;
                    }
                    view.out.steam.push(Steam {
                        cell: f.cell([wx, top, wz]),
                        water_m: *water_m,
                    });
                }
            }
        }
        // Each column's cells show its heat stop; a column is rewritten when its rows change or
        // its heat crosses a stop (with the data's hysteresis: once per stop as it cools).
        let stops: Vec<StateId> = heat_states(self.field.fluid.name)
            .into_iter()
            .map(|m| f.state(m))
            .collect();
        let touched = self.field.take_touched();
        let cols: Vec<usize> = touched
            .iter()
            .flat_map(|&k| self.field.tile_columns(k))
            .collect();
        for i in cols {
            let now = self.field.lava_rows(i);
            let was = std::mem::replace(&mut self.field.written[i], now.clone());
            let [x, z] = self.field.col(i);
            let (wx, wz) = (origin[0] + x, origin[1] + z);
            let current = if was.is_empty() {
                None
            } else {
                let m = view.get(f.cell([wx, was.start, wz]));
                stops.iter().position(|&s| s == m)
            };
            let stop = dwell_lava::DATA.heat_stop(self.field.heat[i], current);
            let state = stops.get(stop).copied().unwrap_or(lava);
            if now == was && (now.is_empty() || current == Some(stop)) {
                continue;
            }
            let lo = now.start.min(was.start);
            let hi = now.end.max(was.end);
            for r in lo..hi {
                let c = f.cell([wx, r, wz]);
                let m = view.get(c);
                if now.contains(&r) {
                    if m != state && open_for_lava(m, lava) {
                        view.set(c, state);
                    }
                } else if same_block(m, lava) {
                    view.set(c, air);
                }
            }
        }
    }
}
