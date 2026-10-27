//! Felling (docs/FELLING.md): pieces that lose their support become bodies, fall and tip, and at
//! rest become blocks again. The server runs them (ARCHITECTURE.md §7.1, §7.3); `server.rs` calls
//! [`Felling::detach`] after a tick's edits and [`Felling::step`] / [`Felling::settle`] after the
//! physics step, and sends the voxel changes and the entity messages.

use dwell_blocks::{Axis, StateId, info, state_property, with_property};
use dwell_physics::{BodyHandle, DQuat, DVec3, Physics, PieceCell};
use dwell_world::support::{SupportRules, detached_pieces};
use dwell_world::{CELL_H, M, VoxelWorld};

/// The felling tunables (docs/FELLING.md §5; ARCHITECTURE.md §7.4's sleep values).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FellingRules {
    pub support: SupportRules,
    /// A piece slower than these (m/s, rad/s) for `sleep_ticks` settles.
    pub sleep_linear: f64,
    pub sleep_angular: f64,
    pub sleep_ticks: u32,
    /// A riders-only piece (leaves) falls this long, then is gone.
    pub loose_rider_ticks: u32,
    /// A piece still moving after this long settles where it is.
    pub max_ticks: u32,
    /// A piece that settles unanchored this many times stays where it is.
    pub max_settles: u8,
    /// The most cells one piece may have (a larger one stands).
    pub max_piece_cells: usize,
    /// The spin (rad/s) a piece cut by a player starts with, tipping it away from them: a trunk
    /// cut straight through would otherwise balance on its stump.
    pub topple_spin: f64,
}

impl Default for FellingRules {
    fn default() -> Self {
        Self {
            support: SupportRules::default(),
            sleep_linear: 0.05,
            sleep_angular: 0.05,
            sleep_ticks: 120,
            loose_rider_ticks: 90,
            max_ticks: 60 * 30,
            max_settles: 3,
            max_piece_cells: 4096,
            topple_spin: 0.6,
        }
    }
}

/// A falling piece: its body and its cells (offsets in cells from its reference cell, whose centre
/// was the body's origin when it spawned, and their states).
#[derive(Clone, Debug)]
pub struct Falling {
    pub id: u32,
    pub body: BodyHandle,
    pub cells: Vec<([i32; 3], StateId)>,
    /// How many of `cells` (the first) are carriers; 0: riders only (no collider).
    pub carriers: usize,
    /// How many times its cells have settled without an anchor before.
    pub settles: u8,
    still: u32,
    age: u32,
}

/// What a settle (or a timeout) leaves: the cells written and the piece's id.
pub struct Settled {
    pub id: u32,
    pub cells: Vec<([i32; 3], StateId)>,
    /// The piece's settle count, for the integrity pass on what it placed.
    pub settles: u8,
}

#[derive(Default)]
pub struct Felling {
    pub pieces: Vec<Falling>,
    next_id: u32,
    pub rules: FellingRules,
}

/// The centre of world cell `c` (m).
pub fn cell_centre(c: [i32; 3]) -> DVec3 {
    DVec3::new(
        f64::from(c[0]) + 0.5,
        (f64::from(c[1]) + 0.5) * CELL_H,
        f64::from(c[2]) + 0.5,
    )
}

/// A cell offset from a piece's reference cell, in the body's frame (m).
fn offset_m(o: [i32; 3]) -> DVec3 {
    DVec3::new(f64::from(o[0]), f64::from(o[1]) * CELL_H, f64::from(o[2]))
}

impl Felling {
    /// Detaches what the cells `changed` left without support: each piece's cells are cleared
    /// (`clear`: the cell and the state it leaves, water where the block was flooded) and it
    /// becomes a body. Returns the new pieces' indices in `pieces`.
    /// `away_from`: who cut it (a point, e.g. the player's eye), which a carrying piece tips away
    /// from (`topple_spin`); none for what a settle left unanchored.
    pub fn detach(
        &mut self,
        world: &mut VoxelWorld,
        physics: &mut Physics,
        changed: &[[i32; 3]],
        settles: u8,
        away_from: Option<DVec3>,
        clear: &mut dyn FnMut(&mut VoxelWorld, [i32; 3], StateId),
    ) -> Vec<usize> {
        let mut out = Vec::new();
        for piece in detached_pieces(world, changed, self.rules.support) {
            if piece.cells.len() > self.rules.max_piece_cells || settles >= self.rules.max_settles {
                continue;
            }
            let reference = piece.cells[0];
            let origin = cell_centre(reference);
            let mut cells = Vec::with_capacity(piece.cells.len());
            let mut boxes = Vec::with_capacity(piece.cells.len());
            for (k, &c) in piece.cells.iter().enumerate() {
                let m = world.get_voxel(c[0], c[1], c[2]);
                let o = [c[0] - reference[0], c[1] - reference[1], c[2] - reference[2]];
                let s = info(m);
                boxes.push(PieceCell {
                    center: offset_m(o),
                    half: DVec3::new(0.5, 0.5 * CELL_H, 0.5),
                    mass: f64::from(s.density_kg_m3).max(1.0) * CELL_H,
                    collides: k < piece.carriers,
                });
                cells.push((o, m));
                clear(world, c, if s.flooded { M.water } else { M.air });
            }
            let body = physics.add_piece(origin, &boxes);
            // Tipped away from who cut it: spun about the level axis across the line to them.
            if let Some(from) = away_from.filter(|_| piece.carriers > 0) {
                let mut d = origin - from;
                d.y = 0.0;
                if d.length() > 1e-6 {
                    let axis = DVec3::Y.cross(d.normalize());
                    physics.set_angular_velocity(body, axis * self.rules.topple_spin);
                }
            }
            self.next_id += 1;
            self.pieces.push(Falling {
                id: self.next_id,
                body,
                cells,
                carriers: piece.carriers,
                settles,
                still: 0,
                age: 0,
            });
            out.push(self.pieces.len() - 1);
        }
        out
    }

    /// The box (world metres) round each piece the terrain collision must cover over the next
    /// tick: its cells' reach from the body, plus a tick's travel and a margin.
    pub fn terrain_boxes(&self, physics: &Physics) -> Vec<(DVec3, DVec3)> {
        self.pieces
            .iter()
            .filter(|p| p.carriers > 0)
            .map(|p| {
                let c = physics.position(p.body);
                let reach = p
                    .cells
                    .iter()
                    .map(|&(o, _)| offset_m(o).length())
                    .fold(0.0f64, f64::max)
                    + 2.0
                    + physics.velocity(p.body).length() / f64::from(dwell_protocol::SIM_HZ) * 2.0;
                (c - DVec3::splat(reach), c + DVec3::splat(reach))
            })
            .collect()
    }

    /// Counts each piece's still ticks after a physics step; returns the indices of those to
    /// settle now (at rest, or out of time), last first (so removing them keeps the rest).
    pub fn step(&mut self, physics: &Physics) -> Vec<usize> {
        let r = self.rules;
        let mut due = Vec::new();
        for (i, p) in self.pieces.iter_mut().enumerate() {
            p.age += 1;
            let v = physics.velocity(p.body).length();
            let w = physics.angular_velocity(p.body).length();
            if v < r.sleep_linear && w < r.sleep_angular {
                p.still += 1;
            } else {
                p.still = 0;
            }
            // Riders alone fall a while and are gone; anything past the world's rows is lost.
            let y = physics.position(p.body).y;
            let lost = !(dwell_world::m_of_row(dwell_world::FACE_MIN_Y)
                ..=dwell_world::m_of_row(dwell_world::FACE_MAX_Y))
                .contains(&y);
            let loose = lost || (p.carriers == 0 && p.age >= r.loose_rider_ticks);
            if p.still >= r.sleep_ticks || p.age >= r.max_ticks || loose {
                due.push(i);
            }
        }
        due.reverse();
        due
    }

    /// Settles piece `i` (docs/FELLING.md §4): its body is removed and its cells resampled onto the
    /// grid at its rotation snapped to the nearest of the 24 axis-aligned ones, each grid cell
    /// whose centre lies in a cell of the piece taking that cell's block (a log's axis turned with
    /// it). A rider whose cell is taken is destroyed; if a carrier's is, the whole piece tries one
    /// and two rows higher, else that carrier is lost. `taken` says whether a cell can't be filled
    /// (terrain, a player). A riders-only piece leaves nothing.
    pub fn settle(
        &mut self,
        world: &mut VoxelWorld,
        physics: &mut Physics,
        i: usize,
        taken: &dyn Fn(&mut VoxelWorld, [i32; 3]) -> bool,
    ) -> Settled {
        let p = self.pieces.remove(i);
        let origin = physics.position(p.body);
        let rotation = snap(physics.rotation(p.body));
        physics.remove_body(p.body);
        let mut out = Settled {
            id: p.id,
            cells: Vec::new(),
            settles: p.settles + 1,
        };
        let y = origin.y;
        let lost = !(dwell_world::m_of_row(dwell_world::FACE_MIN_Y)
            ..=dwell_world::m_of_row(dwell_world::FACE_MAX_Y))
            .contains(&y);
        if p.carriers == 0 || lost {
            return out;
        }
        let placed = resample(&p, origin, rotation);
        let carrier = |m: StateId| info(m).support_role == dwell_blocks::SupportRole::Carrier;
        let mut lift = 0;
        for up in 0..=2 {
            let blocked = placed
                .iter()
                .any(|&(c, m)| carrier(m) && taken(world, [c[0], c[1] + up, c[2]]));
            if !blocked {
                lift = up;
                break;
            }
        }
        for (c, m) in placed {
            let at = [c[0], c[1] + lift, c[2]];
            if taken(world, at) {
                continue;
            }
            out.cells.push((at, m));
        }
        out
    }
}

/// The grid cells the piece covers at `origin` turned by `rotation`, each with its block: the
/// piece is first moved (under half a cell) so its edges lie on grid lines, then each grid cell
/// takes the piece cell its centre lies in (half-open, so a centre on a shared edge belongs to one
/// cell: cells tipped from ½ m rows into 1 m columns pair up, two rows to a cell).
fn resample(p: &Falling, origin: DVec3, rotation: DQuat) -> Vec<([i32; 3], StateId)> {
    let index: std::collections::HashMap<[i32; 3], StateId> = p.cells.iter().copied().collect();
    let inverse = rotation.inverse();
    let bounds = |origin: DVec3| {
        let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
        for &(o, _) in &p.cells {
            for corner in 0..8 {
                let d = DVec3::new(
                    if corner & 1 == 0 { -0.5 } else { 0.5 },
                    if corner & 2 == 0 { -0.5 } else { 0.5 } * CELL_H,
                    if corner & 4 == 0 { -0.5 } else { 0.5 },
                );
                let w = origin + rotation * (offset_m(o) + d);
                lo = lo.min(w);
                hi = hi.max(w);
            }
        }
        (lo, hi)
    };
    let (lo, _) = bounds(origin);
    let origin = origin
        + DVec3::new(
            lo.x.round() - lo.x,
            (lo.y / CELL_H).round() * CELL_H - lo.y,
            lo.z.round() - lo.z,
        );
    let (lo, hi) = bounds(origin);
    let rows = |m: f64| (m / CELL_H).floor() as i32;
    let mut out = Vec::new();
    for z in lo.z.floor() as i32..=hi.z.floor() as i32 {
        for y in rows(lo.y)..=rows(hi.y) {
            for x in lo.x.floor() as i32..=hi.x.floor() as i32 {
                let l = inverse * (cell_centre([x, y, z]) - origin);
                // The piece cell holding it, half-open: [o − ½, o + ½) on each axis.
                let o = [
                    (l.x + 0.5 + 1e-9).floor() as i32,
                    (l.y / CELL_H + 0.5 + 1e-9).floor() as i32,
                    (l.z + 0.5 + 1e-9).floor() as i32,
                ];
                if let Some(&m) = index.get(&o) {
                    out.push(([x, y, z], turned(m, rotation)));
                }
            }
        }
    }
    out
}

/// `m` with its axis turned by `rotation` (a log lying down).
fn turned(m: StateId, rotation: DQuat) -> StateId {
    if state_property(m, "axis").is_none() {
        return m;
    }
    let a = match info(m).axis {
        Axis::X => DVec3::X,
        Axis::Y => DVec3::Y,
        Axis::Z => DVec3::Z,
    };
    let r = rotation * a;
    let (x, y, z) = (r.x.abs(), r.y.abs(), r.z.abs());
    let name = if y >= x && y >= z {
        "y"
    } else if x >= z {
        "x"
    } else {
        "z"
    };
    with_property(m, "axis", name).unwrap_or(m)
}

/// The nearest of the 24 axis-aligned rotations to `q`.
pub fn snap(q: DQuat) -> DQuat {
    let mut best = (f64::NEG_INFINITY, DQuat::IDENTITY);
    for r in axis_rotations() {
        let d = q.dot(r).abs();
        if d > best.0 {
            best = (d, r);
        }
    }
    best.1
}

/// The 24 rotations taking the axes onto the axes.
pub fn axis_rotations() -> Vec<DQuat> {
    let axes = [
        DVec3::X,
        DVec3::NEG_X,
        DVec3::Y,
        DVec3::NEG_Y,
        DVec3::Z,
        DVec3::NEG_Z,
    ];
    let mut out = Vec::with_capacity(24);
    for &x in &axes {
        for &y in &axes {
            if x.dot(y).abs() > 0.5 {
                continue;
            }
            let z = x.cross(y);
            let m = dwell_physics::DMat3::from_cols(x, y, z);
            out.push(DQuat::from_mat3(&m));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log() -> StateId {
        dwell_blocks::parse_state("dwell:log[axis=y]").expect("log")
    }

    fn piece(cells: Vec<([i32; 3], StateId)>) -> Falling {
        Falling {
            id: 1,
            body: Physics::default().add_box(dwell_physics::BoxBody::new(
                dwell_physics::Motion::Dynamic,
                DVec3::ZERO,
                DVec3::splat(0.5),
            )),
            carriers: cells.len(),
            cells,
            settles: 0,
            still: 0,
            age: 0,
        }
    }

    #[test]
    fn there_are_24_axis_rotations_and_a_near_one_snaps_to_its_own() {
        let all = axis_rotations();
        assert_eq!(all.len(), 24);
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert!(a.dot(*b).abs() < 0.99, "distinct");
            }
            let nudged = (*a * DQuat::from_rotation_y(0.2)).normalize();
            assert!(snap(nudged).dot(*a).abs() > 0.999);
        }
    }

    #[test]
    fn an_upright_trunk_tipped_onto_its_side_lies_as_logs_along_x() {
        // Eight rows of trunk (4 m) tipped 90° about z: it lies along x, 4 cells long and a metre
        // (two rows) high, every cell a log along x (docs/FELLING.md §4).
        let p = piece((0..8).map(|y| ([0, y, 0], log())).collect());
        let origin = DVec3::new(10.5, 20.25, 3.5);
        let tipped = DQuat::from_rotation_z(-std::f64::consts::FRAC_PI_2);
        let cells = resample(&p, origin, tipped);
        assert_eq!(cells.len(), 8, "{cells:?}");
        let xs: std::collections::BTreeSet<i32> = cells.iter().map(|(c, _)| c[0]).collect();
        let ys: std::collections::BTreeSet<i32> = cells.iter().map(|(c, _)| c[1]).collect();
        assert_eq!((xs.len(), ys.len()), (4, 2), "{cells:?}");
        assert!(cells.iter().all(|(_, m)| info(*m).axis == Axis::X));
        // Untipped it lands where it was, upright.
        let upright = resample(&p, origin, DQuat::IDENTITY);
        let want: Vec<[i32; 3]> = (0..8).map(|y| [10, 40 + y, 3]).collect();
        let got: Vec<[i32; 3]> = upright.iter().map(|(c, _)| *c).collect();
        let mut got = got;
        got.sort_unstable();
        assert_eq!(got, want);
    }
}
