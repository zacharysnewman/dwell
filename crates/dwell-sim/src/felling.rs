//! Felling (docs/FELLING.md): pieces that lose their support become bodies, fall and tip, and at
//! rest become blocks again. The server runs them (ARCHITECTURE.md §7.1, §7.3); `server.rs` calls
//! [`Felling::detach`] after a tick's edits and [`Felling::step`] / [`Felling::settle`] after the
//! physics step, and sends the voxel changes and the entity messages.

use dwell_blocks::{Axis, StateId, info, state_property, with_property};
use dwell_physics::Collide;
use dwell_physics::{BodyHandle, DQuat, DVec3, Physics, PieceCell};
use dwell_world::support::{SupportRules, detached_pieces};
use dwell_world::{CELL_H, M, VoxelWorld};

/// The felling tunables as data (shared/physics/felling.json → `felling.gen.rs`; seconds, m/s).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FellingData {
    pub rider_reach: u32,
    pub max_system_cells: usize,
    pub max_piece_cells: usize,
    pub topple_spin: f64,
    pub sleep_linear: f64,
    pub sleep_angular: f64,
    pub sleep_seconds: f64,
    pub max_seconds: f64,
    pub max_settles: u8,
    pub hit_speed: f64,
    pub hit_damage_per_speed: f64,
    pub pushable_mass: f64,
    pub heavy_hold_speed: f64,
    /// How long (s) a settling piece is drawn gliding into its blocks' places (the clients').
    pub settle_glide_seconds: f64,
    /// How far past its place (a fraction of the glide) it swings before it rests.
    pub settle_bounce: f64,
}

#[path = "felling.gen.rs"]
mod generated;
pub use generated::FELLING;

/// The felling tunables in the server's units (docs/FELLING.md §5; ARCHITECTURE.md §7.4's sleep
/// values), from [`FELLING`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FellingRules {
    pub support: SupportRules,
    /// A piece slower than these (m/s, rad/s) for `sleep_ticks` settles.
    pub sleep_linear: f64,
    pub sleep_angular: f64,
    pub sleep_ticks: u32,
    /// A piece still moving after this long settles where it is.
    pub max_ticks: u32,
    /// A piece that settles unanchored this many times stays where it is.
    pub max_settles: u8,
    /// The most cells one piece may have (a larger one stands).
    pub max_piece_cells: usize,
    /// The spin (rad/s) a piece cut by a player starts with, tipping it away from them: a trunk
    /// cut straight through would otherwise balance on its stump.
    pub topple_spin: f64,
    /// A piece hitting a player faster than this (m/s toward them) hurts, `hit_damage_per_speed`
    /// points per m/s over it.
    pub hit_speed: f64,
    pub hit_damage_per_speed: f64,
    /// Players push pieces lighter than this (kg); heavier ones they cannot move while slower
    /// than `heavy_hold_speed` (m/s, at any point): held fast while falling, a heavy piece would
    /// squeeze a player under it through the ground.
    pub pushable_mass: f64,
    pub heavy_hold_speed: f64,
}

/// The server's ticks a second.
const TICKS_PER_SECOND: f64 = 60.0;

fn ticks(seconds: f64) -> u32 {
    (seconds * TICKS_PER_SECOND).round() as u32
}

impl From<FellingData> for FellingRules {
    fn from(d: FellingData) -> Self {
        Self {
            support: SupportRules {
                rider_reach: d.rider_reach,
                max_cells: d.max_system_cells,
            },
            sleep_linear: d.sleep_linear,
            sleep_angular: d.sleep_angular,
            sleep_ticks: ticks(d.sleep_seconds),
            max_ticks: ticks(d.max_seconds),
            max_settles: d.max_settles,
            max_piece_cells: d.max_piece_cells,
            topple_spin: d.topple_spin,
            hit_speed: d.hit_speed,
            hit_damage_per_speed: d.hit_damage_per_speed,
            pushable_mass: d.pushable_mass,
            heavy_hold_speed: d.heavy_hold_speed,
        }
    }
}

impl Default for FellingRules {
    fn default() -> Self {
        FELLING.into()
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
    /// The players it has hurt (their keys, `hits`), each hurt once by a piece.
    hurt: Vec<u64>,
    /// Too heavy for players to push (`pushable_mass`), and whether it is held from them now.
    heavy: bool,
    held: bool,
    /// How far its cells reach from its body's origin (m).
    reach: f64,
}

/// A player's body for [`Felling::hits`]: an upright capsule (centre, radius, the half length of
/// its straight part; world metres) and its velocity, keyed by the caller.
#[derive(Clone, Copy, Debug)]
pub struct PlayerBody {
    pub key: u64,
    pub center: DVec3,
    pub radius: f64,
    pub half_cylinder: f64,
    pub velocity: DVec3,
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

/// Whether height `y` (m) lies within the world's chunk rows (a piece past them is lost).
fn in_world_rows(y: f64) -> bool {
    let n = dwell_world::CHUNK_SIZE;
    let lo = dwell_world::m_of_row(dwell_world::MIN_CHUNK_Y * n);
    let hi = dwell_world::m_of_row((dwell_world::MAX_CHUNK_Y + 1) * n);
    (lo..=hi).contains(&y)
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
                let o = [
                    c[0] - reference[0],
                    c[1] - reference[1],
                    c[2] - reference[2],
                ];
                let s = info(m);
                boxes.push(PieceCell {
                    center: offset_m(o),
                    half: DVec3::new(0.5, 0.5 * CELL_H, 0.5),
                    mass: f64::from(s.density_kg_m3).max(1.0) * CELL_H,
                    // Carriers (logs, sand) hit everything; leaves riding them nothing; leaves
                    // falling on their own the world but not players, so they land.
                    // Sand passes players (it may hurt them, `hits`, but never rests on one:
                    // owner, 2026-10-28, "slide off").
                    collide: if k < piece.carriers {
                        if s.support_role == dwell_blocks::SupportRole::Falls {
                            Collide::World
                        } else {
                            Collide::All
                        }
                    } else if piece.carriers == 0 {
                        Collide::World
                    } else {
                        Collide::None
                    },
                });
                cells.push((o, m));
                clear(world, c, if s.flooded { M.water } else { M.air });
            }
            // Too heavy for players to push (docs/FELLING.md §4, `pushableMass`).
            let mass: f64 = boxes.iter().map(|b| b.mass).sum();
            let body = physics.add_piece(origin, &boxes, false);
            let reach = boxes.iter().map(|b| b.center.length()).fold(0.0, f64::max) + 0.9;
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
                hurt: Vec::new(),
                heavy: mass >= self.rules.pushable_mass,
                held: false,
                reach,
            });
            out.push(self.pieces.len() - 1);
        }
        out
    }

    /// The box (world metres) round each piece the terrain collision must cover over the next
    /// tick: its cells' reach from the body, plus a tick's travel and a margin. Every piece: leaves
    /// falling on their own land on the terrain too (owner, 2026-10-28).
    pub fn terrain_boxes(&self, physics: &Physics) -> Vec<(DVec3, DVec3)> {
        self.pieces
            .iter()
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

    /// The players the falling pieces hit hard this tick, before the physics step resolves the
    /// contacts (docs/FELLING.md §4): a carrying cell within reach of a player's capsule by the
    /// next step, closing on it faster than `hit_speed`, hurts `hit_damage_per_speed` points per
    /// m/s over it; each piece hurts each player once. Returns (key, points), at most one per
    /// player (the hardest hit).
    pub fn hits(&mut self, physics: &Physics, players: &[PlayerBody]) -> Vec<(u64, i32)> {
        let r = self.rules;
        let dt = 1.0 / f64::from(dwell_protocol::SIM_HZ);
        let mut out: Vec<(u64, i32)> = Vec::new();
        for p in &mut self.pieces {
            if p.carriers == 0 {
                continue;
            }
            let origin = physics.position(p.body);
            let rotation = physics.rotation(p.body);
            let v = physics.velocity(p.body);
            let w = physics.angular_velocity(p.body);
            let inverse = rotation.inverse();
            for b in players {
                if p.hurt.contains(&b.key) {
                    continue;
                }
                let mut hardest = 0.0f64;
                for &(o, _) in &p.cells[..p.carriers] {
                    let centre = offset_m(o);
                    let half = DVec3::new(0.5, 0.5 * CELL_H, 0.5);
                    // The capsule's axis point nearest the cell, and the cell's point nearest it.
                    let world_centre = origin + rotation * centre;
                    let t = (world_centre.y - b.center.y).clamp(-b.half_cylinder, b.half_cylinder);
                    let axis = b.center + DVec3::Y * t;
                    let local = inverse * (axis - origin) - centre;
                    let near = origin + rotation * (centre + local.clamp(-half, half));
                    let gap = (axis - near).length() - b.radius;
                    let at = near - origin;
                    // The piece's speed toward the player, less the player's away from it (a
                    // player falling onto a still log lands: fall damage, the controller's).
                    let n = (axis - near).normalize_or_zero();
                    let closing = (v + w.cross(at)).dot(n) - b.velocity.dot(n).max(0.0);
                    if closing > 0.0 && gap <= closing * dt + 0.05 {
                        hardest = hardest.max(closing);
                    }
                }
                if hardest > r.hit_speed {
                    p.hurt.push(b.key);
                    let points = ((hardest - r.hit_speed) * r.hit_damage_per_speed).round() as i32;
                    match out.iter_mut().find(|(k, _)| *k == b.key) {
                        Some(e) => e.1 = e.1.max(points),
                        None => out.push((b.key, points)),
                    }
                }
            }
        }
        out
    }

    /// Counts each piece's still ticks after a physics step; returns the indices of those to
    /// settle now (at rest, or out of time), last first (so removing them keeps the rest).
    /// And holds the heavy pieces moving slowly from players' pushes, letting the faster go.
    pub fn step(&mut self, physics: &mut Physics) -> Vec<usize> {
        let r = self.rules;
        let mut due = Vec::new();
        for (i, p) in self.pieces.iter_mut().enumerate() {
            p.age += 1;
            let v = physics.velocity(p.body).length();
            let w = physics.angular_velocity(p.body).length();
            let held = p.heavy && v + w * p.reach < r.heavy_hold_speed;
            if held != p.held {
                physics.set_dominant(p.body, held);
                p.held = held;
            }
            if v < r.sleep_linear && w < r.sleep_angular {
                p.still += 1;
            } else {
                p.still = 0;
            }
            // Anything past the world's rows is lost; the rest settles at rest or out of time
            // (leaves falling on their own too: they land, owner 2026-10-28).
            let y = physics.position(p.body).y;
            let lost = !in_world_rows(y);
            if p.still >= r.sleep_ticks || p.age >= r.max_ticks || lost {
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
    /// (terrain, or a player for a block that blocks a body), asked for each block. A riders-only
    /// piece (leaves falling on their own) lands as leaves too.
    pub fn settle(
        &mut self,
        world: &mut VoxelWorld,
        physics: &mut Physics,
        i: usize,
        taken: &dyn Fn(&mut VoxelWorld, [i32; 3], StateId) -> bool,
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
        let lost = !in_world_rows(y);
        if lost {
            return out;
        }
        let placed = resample(&p.cells, origin, rotation).1;
        let carrier = |m: StateId| info(m).support_role == dwell_blocks::SupportRole::Carrier;
        let falls = |m: StateId| info(m).support_role == dwell_blocks::SupportRole::Falls;
        let mut lift = 0;
        for up in 0..=2 {
            let blocked = placed
                .iter()
                .any(|&(c, m)| carrier(m) && taken(world, [c[0], c[1] + up, c[2]], m));
            if !blocked {
                lift = up;
                break;
            }
        }
        for (c, m) in placed {
            let at = [c[0], c[1] + lift, c[2]];
            if !taken(world, at, m) {
                out.cells.push((at, m));
                continue;
            }
            // Sand landing where a player stands slides off beside them (a free cell next to it
            // on the same row), rather than being lost.
            if falls(m) {
                let beside = [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]]
                    .map(|d| [at[0] + d[0], at[1], at[2] + d[2]])
                    .into_iter()
                    .find(|&n| !taken(world, n, m) && !out.cells.iter().any(|(c, _)| *c == n));
                if let Some(n) = beside {
                    out.cells.push((n, m));
                }
            }
        }
        out
    }
}

/// The grid cells a piece of `cells` covers at `origin` turned by `rotation`, each with its block,
/// and the origin it was moved to: the piece is first moved (under half a cell) so its edges lie on grid lines, then each grid cell
/// takes the piece cell its centre lies in (half-open, so a centre on a shared edge belongs to one
/// cell: cells tipped from ½ m rows into 1 m columns pair up, two rows to a cell).
pub fn resample(
    cells: &[([i32; 3], StateId)],
    origin: DVec3,
    rotation: DQuat,
) -> (DVec3, Vec<([i32; 3], StateId)>) {
    let index: std::collections::HashMap<[i32; 3], StateId> = cells.iter().copied().collect();
    let inverse = rotation.inverse();
    let bounds = |origin: DVec3| {
        let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
        for &(o, _) in cells {
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
    (origin, out)
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
            hurt: Vec::new(),
            heavy: false,
            held: false,
            reach: 1.0,
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
        let cells = resample(&p.cells, origin, tipped).1;
        assert_eq!(cells.len(), 8, "{cells:?}");
        let xs: std::collections::BTreeSet<i32> = cells.iter().map(|(c, _)| c[0]).collect();
        let ys: std::collections::BTreeSet<i32> = cells.iter().map(|(c, _)| c[1]).collect();
        assert_eq!((xs.len(), ys.len()), (4, 2), "{cells:?}");
        assert!(cells.iter().all(|(_, m)| info(*m).axis == Axis::X));
        // Untipped it lands where it was, upright.
        let upright = resample(&p.cells, origin, DQuat::IDENTITY).1;
        let want: Vec<[i32; 3]> = (0..8).map(|y| [10, 40 + y, 3]).collect();
        let got: Vec<[i32; 3]> = upright.iter().map(|(c, _)| *c).collect();
        let mut got = got;
        got.sort_unstable();
        assert_eq!(got, want);
    }

    /// A 1 m log falling at `speed` m/s onto a standing player's head (capsule 0.3 m round,
    /// 1.8 m tall, centre 0.9 m up): what it hurts, twice over (a piece hurts each player once).
    type Hits = Vec<(u64, i32)>;

    fn struck(speed: f64) -> (Hits, Hits) {
        let mut physics = Physics::default();
        let mut f = Felling::default();
        let head = 1.8;
        let at = DVec3::new(0.0, head + 0.25 + 0.01, 0.0);
        let cells = [([0, 0, 0], log()), ([0, 1, 0], log())];
        let boxes: Vec<PieceCell> = cells
            .iter()
            .map(|&(o, _)| PieceCell {
                center: offset_m(o),
                half: DVec3::new(0.5, 0.25, 0.5),
                mass: 300.0,
                collide: Collide::All,
            })
            .collect();
        let body = physics.add_piece(at, &boxes, true);
        physics.set_velocity(body, DVec3::new(0.0, -speed, 0.0));
        f.pieces.push(Falling {
            id: 1,
            body,
            cells: cells.to_vec(),
            carriers: 2,
            settles: 0,
            still: 0,
            age: 0,
            hurt: Vec::new(),
            heavy: false,
            held: false,
            reach: 1.0,
        });
        let player = PlayerBody {
            key: 7,
            center: DVec3::new(0.2, 0.9, 0.1),
            radius: 0.3,
            half_cylinder: 0.6,
            velocity: DVec3::ZERO,
        };
        let first = f.hits(&physics, &[player]);
        (first, f.hits(&physics, &[player]))
    }

    #[test]
    fn a_log_falling_fast_on_a_player_hurts_once_and_a_slow_one_not() {
        let r = FellingRules::default();
        let (first, again) = struck(10.0);
        let points = ((10.0 - r.hit_speed) * r.hit_damage_per_speed).round() as i32;
        assert_eq!(first, vec![(7, points)]);
        assert!(again.is_empty(), "hurt once: {again:?}");
        assert!(struck(r.hit_speed * 0.5).0.is_empty());
        // Moving away from the player: no hit, however fast.
        assert!(struck(-10.0).0.is_empty());
    }

    #[test]
    fn the_tunables_come_from_the_data_file() {
        // shared/physics/felling.json, in ticks at 60 Hz.
        let r = FellingRules::default();
        assert_eq!(r.sleep_ticks, (FELLING.sleep_seconds * 60.0).round() as u32);
        assert_eq!(r.max_ticks, (FELLING.max_seconds * 60.0).round() as u32);
        assert_eq!(r.support.rider_reach, FELLING.rider_reach);
        assert_eq!(r.pushable_mass, FELLING.pushable_mass);
    }
}
