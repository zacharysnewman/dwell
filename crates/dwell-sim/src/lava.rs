//! The server's lava (docs/proposals/VOLCANOES.md §6, V1 slice 2): the world's lava fields
//! ([`dwell_lava::Field`]), each on one face, placed round a vent ([`Lava::place`]), ticked like the
//! water grid (every lava tick near a player, fast-forwarded elsewhere: [`Field::advance`]), and
//! written into the world as blocks so clients get them through the chunk and edit sync:
//!
//! - **Lava cells.** Each column's molten rows ([`Field::lava_rows`]) are the fluid's block (its
//!   name, `dwell:lava`), written only into open cells (air, or a replaceable non-solid block) and
//!   cleared to air where the lava has gone.
//! - **Rock.** A `Rock` event writes its rows as the rock (`dwell:basalt`, or `dwell:obsidian` at
//!   the quench edge): new land.
//! - **Steam.** A `Steam` event takes `steamWaterM` off the top of that column's water (cells to
//!   air) and is kept as a [`Steam`] for the clients to show later.
//!
//! A field's columns, beds and rows are face-local; every world cell and point crosses through its
//! [`FaceFrame`]. Saved as one record (`to_bytes`, the world's optional `lava` table).

use std::collections::{BTreeSet, HashMap};
use std::ops::Range;

use dwell_blocks::{StateId, info};
pub use dwell_lava::VentKind;
use dwell_lava::{Field, HotFluid, LavaData, LavaEvent, Vent, field_of, record_of};
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
    /// The field reaches this many columns each way from the vent.
    pub radius: usize,
}

/// Water turned to steam (the clients' puff, later): the world cell of the water's top it took.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Steam {
    pub cell: [i32; 3],
    pub water_m: f32,
}

/// One field on its face, and the rows of each column last written as lava.
#[derive(Clone, Debug)]
pub struct Placed {
    pub face: FaceFrame,
    pub field: Field,
    written: Vec<Range<i32>>,
    /// Columns the world edited since the last lava tick (not by the lava): their bed and water
    /// are read again then ([`Lava::touch`]).
    dirty: BTreeSet<usize>,
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
    m == 0 || m == lava || (s.replaceable && !s.solid && !s.liquid)
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
    /// face-local row `row` (the vent's own cell or above it): a new field of `2 × radius + 1`
    /// columns square centred on it, its beds and water read from the world through `read` (world
    /// cells to states). Returns the field's index; an error names an unknown fluid, a fluid with
    /// no block of its name, or a column with no ground.
    pub fn place(
        &mut self,
        rules: &LavaData,
        f: FaceFrame,
        column: [i32; 2],
        row: i32,
        spot: &VentSpot,
        mut read: impl FnMut([i32; 3]) -> StateId,
    ) -> Result<usize, String> {
        let n = 2 * spot.radius + 1;
        let mut field = Field::new(rules, &spot.fluid, n, n)?;
        if dwell_blocks::state(field.fluid.name).is_none() {
            return Err(format!(
                "hot fluid {}: no block of that name to write its cells",
                field.fluid.name
            ));
        }
        let r = spot.radius as i32;
        field.origin = [column[0] - r, column[1] - r];
        for z in 0..n {
            for x in 0..n {
                let (wx, wz) = (field.origin[0] + x as i32, field.origin[1] + z as i32);
                let mut bed = None;
                let mut wet = false;
                for local in (row - SCAN_ROWS..=row + SCAN_ROWS).rev() {
                    let s = info(read(f.cell([wx, local, wz])));
                    if s.solid {
                        bed = Some(local);
                        break;
                    }
                    wet |= s.liquid;
                }
                let Some(bed) = bed else {
                    return Err(format!(
                        "no ground under column ({wx}, {wz}) within {SCAN_ROWS} rows of row {row}"
                    ));
                };
                let i = field.at(x, z);
                field.ground[i] = ((f64::from(bed) + 1.0) * CELL_H) as f32;
                field.water[i] = wet;
            }
        }
        let c = spot.radius;
        let bed = field.ground[field.at(c, c)];
        field.vents.push(Vent {
            kind: spot.kind,
            x: c,
            z: c,
            rate_m3s: spot.rate_m3s,
            level_m: match spot.kind {
                VentKind::Lake => bed + spot.depth_m,
                VentKind::River => 0.0,
            },
        });
        let written = (0..n * n).map(|i| field.lava_rows(i)).collect();
        self.fields.push(Placed {
            face: f,
            field,
            written,
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
            if p.field.advance(real_s, near) <= 0.0 {
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
            if (0..p.field.w as i32).contains(&x) && (0..p.field.h as i32).contains(&z) {
                p.dirty.insert(p.field.at(x as usize, z as usize));
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
            let written = (0..field.w * field.h).map(|i| field.lava_rows(i)).collect();
            fields.push(Placed {
                face,
                field,
                written,
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
        for i in std::mem::take(&mut self.dirty) {
            let (x, z) = (i % self.field.w, i / self.field.w);
            let (wx, wz) = (
                self.field.origin[0] + x as i32,
                self.field.origin[1] + z as i32,
            );
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
            self.field.water[i] = (bed..bed + SCAN_ROWS).any(|r| {
                let m = view.get(f.cell([wx, r, wz]));
                m != lava && info(m).liquid
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
                LavaEvent::Steam { x, z, water_m } => {
                    let i = self.field.at(*x, *z);
                    let (wx, wz) = (origin[0] + *x as i32, origin[1] + *z as i32);
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
        for i in 0..self.field.w * self.field.h {
            let now = self.field.lava_rows(i);
            let was = std::mem::replace(&mut self.written[i], now.clone());
            if now == was {
                continue;
            }
            let (x, z) = (i % self.field.w, i / self.field.w);
            let (wx, wz) = (origin[0] + x as i32, origin[1] + z as i32);
            let lo = now.start.min(was.start);
            let hi = now.end.max(was.end);
            for r in lo..hi {
                let c = f.cell([wx, r, wz]);
                let m = view.get(c);
                if now.contains(&r) {
                    if m != lava && open_for_lava(m, lava) {
                        view.set(c, lava);
                    }
                } else if m == lava {
                    view.set(c, air);
                }
            }
        }
    }
}
