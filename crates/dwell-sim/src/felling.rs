//! Felling (docs/FELLING.md): pieces that lose their support become bodies, fall and tip, and at
//! rest become blocks again. The server runs them (ARCHITECTURE.md §7.1, §7.3); `server.rs` calls
//! [`Felling::detach`] after a tick's edits and [`Felling::step`] / [`Felling::settle`] after the
//! physics step, and sends the voxel changes and the entity messages.

use dwell_blocks::{Axis, StateId, info, state_property, with_property};
use dwell_physics::Collide;
use dwell_physics::predictor::WaterDrag;
use dwell_physics::{BodyHandle, DQuat, DVec3, Physics, PieceCell};
use dwell_world::support::{SupportRules, detached_pieces};
use dwell_world::{CELL_H, FaceFrame, M, VoxelWorld, m_of_row, row_of_m};

/// The felling tunables as data (shared/physics/felling.json → `felling.gen.rs`; seconds, m/s).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FellingData {
    pub rider_reach: u32,
    pub max_system_cells: usize,
    pub max_piece_cells: usize,
    pub topple_spin: f64,
    pub topple_tip_speed: f64,
    pub sleep_linear: f64,
    pub sleep_angular: f64,
    pub rest_drift: f64,
    pub sleep_seconds: f64,
    pub settle_every_ticks: u32,
    pub max_seconds: f64,
    pub max_settles: u8,
    pub hit_speed: f64,
    pub hit_damage_per_speed: f64,
    pub hold_speed: f64,
    pub push_hold_seconds: f64,
    /// How long (s) a settling piece is drawn gliding into its blocks' places (the clients').
    pub settle_glide_seconds: f64,
    /// How far past its place (a fraction of the glide) it swings before it rests.
    pub settle_bounce: f64,
    /// The most terrain-collision chunks built a tick round falling pieces (nearest first).
    pub terrain_chunks_per_tick: u32,
}

#[path = "felling.gen.rs"]
mod generated;
pub use generated::FELLING;

/// A body kind's rules (`shared/physics/bodies.json`; docs/BUILT_BODIES.md §2): what may be done to
/// a moving body of cells. Its index in [`BODY_KINDS`] is the wire's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BodyKind {
    pub name: &'static str,
    /// A block placed against one of its cells joins it.
    pub place: bool,
    /// A cell of it may be broken.
    pub breaks: bool,
    /// A break that leaves it in more than one connected group splits it.
    pub split: bool,
    /// At rest it becomes blocks again.
    pub settles: bool,
    /// Saved with the world.
    pub saved: bool,
    /// Blocks a body of this kind is built on (the build platform): they stay in the world's grid;
    /// a block placed on one's top starts a body of this kind there, held still while it rests on
    /// one (docs/BUILT_BODIES.md §3).
    pub built_on: &'static [&'static str],
}

/// The kind a body built on a block of state `m` is (its block in a kind's `builtOn`).
pub fn kind_built_on(m: StateId) -> Option<u8> {
    let name = info(m).name;
    let block = name.split('[').next().unwrap_or(name);
    BODY_KINDS
        .iter()
        .position(|k| k.built_on.contains(&block))
        .map(|i| i as u8)
}

/// Where a block placed into the world's cell goes instead of the grid (docs/BUILT_BODIES.md §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Divert {
    /// Into the held body `id`, as its cell `cell` (its grid): it touches that body.
    Join { id: u32, cell: [i32; 3] },
    /// A new body of `kind`: it is on a build platform's top.
    New { kind: u8 },
    /// Nowhere: a held body's cell is already there.
    Taken,
}

/// How bodies float (`shared/physics/bodies.json` `water`; docs/BUILT_BODIES.md §6).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterRules {
    /// The drag coefficient of a body moving through water: the water's resistance is
    /// ½ · its density · this · the area under water facing the motion · speed².
    pub drag_coefficient: f64,
    /// A further slowing of a body's sideways velocity in water (/s), by how much of it is under:
    /// what stills a slow drift, which the speed-squared drag barely touches.
    pub drag: f64,
    /// The slowing of its up-and-down velocity (/s), the same way: a bobbing body makes waves,
    /// which still it in a few bobs.
    pub heave_drag: f64,
    /// Slowing of its spin (/s), the same way.
    pub angular_drag: f64,
}

// Included (not a `#[path]` module) so rustfmt leaves the generated file as written.
mod bodies_generated {
    include!("bodies.gen.rs");
}
pub use bodies_generated::{BODY_KINDS, WATER};

/// How water slows body `body` sideways (docs/BUILT_BODIES.md §6), the server's and the clients'
/// stand-ins' alike: ½ · the liquid's density · `WATER.drag_coefficient` · the area of its sides
/// under water facing the motion (its bounds' sides, as deep as the share of it under) · speed²,
/// and `WATER.drag` by that share, relative to the water's current. Nothing out of the water.
pub fn water_drag(world: &mut VoxelWorld, physics: &Physics, body: BodyHandle) -> WaterDrag {
    water_drag_with(
        world,
        physics,
        body,
        &mut dwell_world::falls::FallMemo::default(),
    )
}

/// [`water_drag`], the waterfalls' reads kept in `falls` across bodies (one pass, the world
/// unchanged).
pub fn water_drag_with(
    world: &mut VoxelWorld,
    physics: &Physics,
    body: BodyHandle,
    falls: &mut dwell_world::falls::FallMemo,
) -> WaterDrag {
    let mut centres = Vec::new();
    physics.collider_centres(body, &mut centres);
    let rotation = physics.rotation(body);
    let (mut under, mut liquid) = (0.0, 0.0);
    for &c in &centres {
        let (fraction, density) = cell_in_liquid(world, c, rotation);
        under += fraction;
        liquid += fraction * density;
    }
    let Some((lo, hi)) = physics.aabb(body).filter(|_| under > 0.0) else {
        return WaterDrag::default();
    };
    let share = under / centres.len() as f64;
    let size = hi - lo;
    let depth = size.y * share;
    let k = 0.5 * (liquid / under) * WATER.drag_coefficient / physics.mass(body).max(1e-6);
    // The water's current where the body is (docs/WATER_SURFACE_GRID.md §2c): it drifts with it.
    let [cu, cv] = world.currents.at(physics.position(body).to_array());
    // Under a waterfall (docs/WATER_SURFACE_GRID.md §2e): pushed down and out by it, as players are.
    let centre = ((lo + hi) * 0.5).to_array();
    let fall = dwell_world::falls::push_at_with(
        world,
        centre,
        0.5 * size.y,
        0.5 * size.x.max(size.z),
        falls,
    );
    WaterDrag {
        kx: k * depth * size.z,
        kz: k * depth * size.x,
        linear: WATER.drag * share,
        current: [f64::from(cu), f64::from(cv)],
        fall,
        face: dwell_world::FaceFrame::at_height(centre[1]),
    }
}

/// The kind of wire index `k`; an unknown one is natural (the first).
pub fn body_kind(k: u8) -> &'static BodyKind {
    BODY_KINDS.get(usize::from(k)).unwrap_or(&BODY_KINDS[0])
}

/// What breaking a cell of a body did (docs/BUILT_BODIES.md §4).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BreakOutcome {
    /// The cells that left the body, in its grid: the broken one, then those split off.
    pub removed: Vec<[i32; 3]>,
    /// The new bodies split off it (indices in `pieces`).
    pub spawned: Vec<usize>,
    /// The body had no cells left: it is gone.
    pub gone: bool,
}

/// A cell's box in its piece (`offset` in the piece's grid): its block's mass, colliding as a
/// carrier (logs: everything; falling sand: the world, it passes players) or a rider (leaves:
/// nothing, or the world in a piece of riders only, which lands as leaves).
fn piece_box(offset: [i32; 3], m: StateId, carrier: bool, riders_only: bool) -> PieceCell {
    let s = info(m);
    PieceCell {
        center: offset_m(offset),
        half: DVec3::new(0.5, 0.5 * CELL_H, 0.5),
        mass: f64::from(s.density_kg_m3).max(1.0) * CELL_H,
        collide: if carrier {
            if s.support_role == dwell_blocks::SupportRole::Falls {
                Collide::World
            } else {
                Collide::All
            }
        } else if riders_only {
            Collide::World
        } else {
            Collide::None
        },
        hull: dwell_physics::piece_hull(m),
    }
}

/// Whether a cell of state `m` carries in a piece (it, not riders, collides).
fn carries(m: StateId) -> bool {
    matches!(
        info(m).support_role,
        dwell_blocks::SupportRole::Carrier | dwell_blocks::SupportRole::Falls
    )
}

/// `cells` with the carriers first (as `Falling::cells` keeps them), and how many there are.
fn carriers_first(mut cells: Vec<([i32; 3], StateId)>) -> (Vec<([i32; 3], StateId)>, usize) {
    cells.sort_by_key(|&(_, m)| !carries(m));
    let n = cells.iter().filter(|&&(_, m)| carries(m)).count();
    (cells, n)
}

/// The piece's boxes for its cells (carriers first, `carriers` of them). In a body that isn't
/// natural (`kind` > 0: a built one) every solid, non-passable block collides with everything.
fn piece_boxes(cells: &[([i32; 3], StateId)], carriers: usize, kind: u8) -> Vec<PieceCell> {
    cells
        .iter()
        .enumerate()
        .map(|(k, &(o, m))| {
            if kind == 0 {
                return piece_box(o, m, k < carriers, carriers == 0);
            }
            let s = info(m);
            let mut b = piece_box(o, m, true, false);
            b.collide = if s.solid && !s.passable {
                Collide::All
            } else {
                Collide::None
            };
            b
        })
        .collect()
}

/// The 6-connected groups of `cells` (offsets in a piece's grid), largest first.
fn connected_groups(cells: &[([i32; 3], StateId)]) -> Vec<Vec<([i32; 3], StateId)>> {
    let index: std::collections::HashMap<[i32; 3], StateId> = cells.iter().copied().collect();
    let mut seen = std::collections::HashSet::new();
    let mut groups = Vec::new();
    for &(start, _) in cells {
        if !seen.insert(start) {
            continue;
        }
        let mut group = Vec::new();
        let mut stack = vec![start];
        while let Some(c) = stack.pop() {
            group.push((c, index[&c]));
            for d in [
                [1, 0, 0],
                [-1, 0, 0],
                [0, 1, 0],
                [0, -1, 0],
                [0, 0, 1],
                [0, 0, -1],
            ] {
                let n = [c[0] + d[0], c[1] + d[1], c[2] + d[2]];
                if index.contains_key(&n) && seen.insert(n) {
                    stack.push(n);
                }
            }
        }
        groups.push(group);
    }
    // Largest first; ties in the order found (the cells' order).
    groups.sort_by_key(|g| std::cmp::Reverse(g.len()));
    groups
}

/// The felling tunables in the server's units (docs/FELLING.md §5; ARCHITECTURE.md §7.4's sleep
/// values), from [`FELLING`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FellingRules {
    pub support: SupportRules,
    /// A piece slower than these (m/s, rad/s) for `sleep_ticks` settles.
    pub sleep_linear: f64,
    pub sleep_angular: f64,
    /// Or within this (m, at its farthest point) of where it began counting.
    pub rest_drift: f64,
    pub sleep_ticks: u32,
    /// Pieces due settle together on every this many ticks (one modification, one terrain
    /// collision rebuild a chunk, for all of them).
    pub settle_every_ticks: u32,
    /// A piece still moving after this long settles where it is.
    pub max_ticks: u32,
    /// A piece that settles unanchored this many times stays where it is.
    pub max_settles: u8,
    /// The most cells one piece may have (a larger one stands).
    pub max_piece_cells: usize,
    /// The spin (rad/s) a piece cut by a player starts with, tipping it away from them: a trunk
    /// cut straight through would otherwise balance on its stump.
    pub topple_spin: f64,
    /// At most this fast (m/s) at its farthest point from its foot: a tall trunk is nudged.
    pub topple_tip_speed: f64,
    /// A piece hitting a player faster than this (m/s toward them) hurts, `hit_damage_per_speed`
    /// points per m/s over it.
    pub hit_speed: f64,
    pub hit_damage_per_speed: f64,
    /// A piece slower than this (m/s, at any point) is held from players' contacts (a dominance
    /// group), and moved by their push instead (`Players::step_push`, the same in the clients'
    /// prediction); faster, it moves as the physics says (held fast while falling, it would
    /// squeeze a player under it through the ground). Docs/BUILT_BODIES.md §6.
    pub hold_speed: f64,
    /// A piece pushed is held this many ticks after its last push, however fast it goes.
    pub push_hold_ticks: u32,
    /// The most terrain-collision chunks built a tick round falling pieces.
    pub terrain_chunks_per_tick: u32,
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
            rest_drift: d.rest_drift,
            sleep_ticks: ticks(d.sleep_seconds),
            settle_every_ticks: d.settle_every_ticks,
            max_ticks: ticks(d.max_seconds),
            max_settles: d.max_settles,
            max_piece_cells: d.max_piece_cells,
            topple_spin: d.topple_spin,
            topple_tip_speed: d.topple_tip_speed,
            hit_speed: d.hit_speed,
            hit_damage_per_speed: d.hit_damage_per_speed,
            hold_speed: d.hold_speed,
            push_hold_ticks: ticks(d.push_hold_seconds),
            terrain_chunks_per_tick: d.terrain_chunks_per_tick,
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
    /// Where it was (its origin and rotation) when its still ticks began counting.
    rest: Option<(DVec3, DQuat)>,
    age: u32,
    /// Its kind (an index into [`BODY_KINDS`]; 0 natural).
    pub kind: u8,
    /// The players it has hurt (their keys, `hits`), each hurt once by a piece.
    hurt: Vec<u64>,
    /// Whether it is held from players' contacts now (slow, or pushed lately).
    held: bool,
    /// Ticks it stays held for its last push (`push_hold_ticks`).
    pushed: u32,
    /// Held still on its build platform (docs/BUILT_BODIES.md §3) until no cell of it rests on one.
    anchored: bool,
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
    /// The steps counted (`step`), for `settle_every_ticks`.
    steps: u32,
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
/// How a felled trunk's upside-down wood ramp lies over the stump's ramp (a wood's
/// `fellWedgeTrunk`, shared/tools).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WedgeTrunk {
    /// The notch's upper face: the trunk's ramp thick at the hinge, so the two ramps meet on the
    /// hinge's edge in a V open toward the notch, and the trunk, resting on that edge with its
    /// weight over the notch, pivots into it (an open-face notch).
    Open,
    /// Parallel to the stump's: the trunk drops ½ m onto the slope and rests on it (it stood
    /// there in the sim test: the slope's friction holds it).
    Matching,
}

/// The fell wedges of a felled standing wood (Phase 55 T2; owner, 2026-11-09: the felled cell
/// "should turn into a wood ramp and a wood ramp upside down on the piece above"), in the stored
/// ramps of the block `wedge`, face-local (+y up): the stump's ramp in the cut cell, rising away
/// from the notch (`notch`: the level direction, x and z, from the trunk's axis to its notched
/// side; a ramp *faces* the way it falls, so the stump's faces the notch), and the trunk's new
/// bottom, the upside-down ramp in the cell above, laid as `trunk` says. The cut cell's other half
/// is chips. Not flooded. None if `wedge` has no such ramps.
pub fn fell_wedges(wedge: &str, notch: [f64; 2], trunk: WedgeTrunk) -> Option<(StateId, StateId)> {
    let b = dwell_blocks::find_block(wedge)?;
    // A shape's corners, NW NE SE SW, as (x, z) signs about the cell's middle.
    const CORNERS: [[f64; 2]; 4] = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
    let hinge = |k: usize| CORNERS[k][0] * notch[0] + CORNERS[k][1] * notch[1] < 0.0;
    // The upright ramp full on the side `full` says: its facing is the way it falls.
    let facing = |full: &dyn Fn(usize) -> bool| {
        (b.first_state..b.first_state + b.state_count)
            .find(|&s| {
                let c = dwell_blocks::shape::shape_of(s).corners;
                state_property(s, "shape") == Some("ramp")
                    && state_property(s, "half") == Some("bottom")
                    && (0..4).all(|k| (c[k] == 2) == full(k))
            })
            .and_then(|s| state_property(s, "facing"))
    };
    let to_notch = facing(&hinge)?;
    let to_hinge = facing(&|k| !hinge(k))?;
    let state = |half: &str, facing: &str| {
        let s = with_property(b.first_state, "shape", "ramp")?;
        let s = with_property(s, "half", half)?;
        let s = with_property(s, "facing", facing)?;
        Some(with_property(s, "flooded", "false").unwrap_or(s))
    };
    // A hanging ramp (`half: top`) is the upright one turned over: facing the notch it reaches
    // down at the hinge (open), facing the hinge at the notch (matching).
    let top = match trunk {
        WedgeTrunk::Open => to_notch,
        WedgeTrunk::Matching => to_hinge,
    };
    Some((state("bottom", to_notch)?, state("top", top)?))
}

pub fn cell_centre(c: [i32; 3]) -> DVec3 {
    DVec3::new(
        f64::from(c[0]) + 0.5,
        (f64::from(c[1]) + 0.5) * CELL_H,
        f64::from(c[2]) + 0.5,
    )
}

/// A cell offset from a piece's reference cell, in the body's frame (m).
/// How much of a cell of a body (its centre `centre`, turned by `rotation`; 1 × ½ × 1 m) is in a
/// liquid, and that liquid's density (kg/m³, its block's; docs/BUILT_BODIES.md §6): none or all by
/// its centre and corners, else the share of five columns (its centre and corners) in liquid
/// cells, measured exactly so the buoyancy changes smoothly with depth.
pub fn cell_in_liquid(world: &mut VoxelWorld, centre: DVec3, rotation: DQuat) -> (f64, f64) {
    let half = DVec3::new(0.5, 0.5 * CELL_H, 0.5);
    let mut read = |p: DVec3| {
        let [x, y, z] = dwell_world::cell_of_pos(p.to_array());
        let s = info(world.get_voxel(x, y, z));
        s.liquid.then_some(f64::from(s.density_kg_m3))
    };
    // A quick look: its centre and corners (a little inside, so a point on a cell's face reads the
    // cell it belongs to). All dry: none; all under: all of it.
    let corner = |k: usize, h: f64| {
        DVec3::new(
            if k & 1 == 0 { -half.x } else { half.x } * 0.98,
            h,
            if k & 2 == 0 { -half.z } else { half.z } * 0.98,
        )
    };
    let mut quick = vec![read(centre)];
    for k in 0..4 {
        for h in [-half.y * 0.98, half.y * 0.98] {
            quick.push(read(centre + rotation * corner(k, h)));
        }
    }
    let wet: Vec<f64> = quick.iter().flatten().copied().collect();
    if wet.is_empty() {
        return (0.0, 0.0);
    }
    let density = wet.iter().sum::<f64>() / wet.len() as f64;
    if wet.len() == quick.len() {
        return (1.0, density);
    }
    // At the surface: five columns (its centre and corners), each measured exactly: the length of
    // it in liquid cells, row by row (a sampled share stepped the buoyancy, so a rider's weight
    // sank a raft a whole step).
    let mut under = 0.0;
    for k in 0..5 {
        let at = if k == 4 { DVec3::ZERO } else { corner(k, 0.0) };
        let (a, b) = (
            centre + rotation * (at - DVec3::Y * half.y),
            centre + rotation * (at + DVec3::Y * half.y),
        );
        let (lo, hi) = if a.y <= b.y { (a, b) } else { (b, a) };
        let span = hi.y - lo.y;
        if span < 1e-3 {
            under += f64::from(u8::from(read((lo + hi) * 0.5).is_some())); // lying flat
            continue;
        }
        let mut wet = 0.0;
        let (first, last) = (row_of_m(lo.y), row_of_m(hi.y));
        for row in first..=last {
            let (y0, y1) = (m_of_row(row).max(lo.y), m_of_row(row + 1).min(hi.y));
            if y1 <= y0 {
                continue;
            }
            let mid = lo + (hi - lo) * (((y0 + y1) * 0.5 - lo.y) / span);
            if read(mid).is_some() {
                wet += y1 - y0;
            }
        }
        under += wet / span;
    }
    (under / 5.0, density)
}

/// A cell's centre in its piece's frame (m) from its offset in the piece's grid.
pub fn cell_offset_m(o: [i32; 3]) -> DVec3 {
    offset_m(o)
}

fn offset_m(o: [i32; 3]) -> DVec3 {
    DVec3::new(f64::from(o[0]), f64::from(o[1]) * CELL_H, f64::from(o[2]))
}

impl Felling {
    /// Detaches what the cells `changed` left without support: each piece's cells are cleared
    /// (`clear`: the cell and the state it leaves, water where the block was flooded) and it
    /// becomes a body. Returns the new pieces' indices in `pieces`.
    /// `away_from`: who cut it (a point, e.g. the player's eye), which a carrying piece tips away
    /// from (`topple_spin`), with the share of that spin (1: all of it; a wood's `fellTip`); none
    /// for what a settle left unanchored.
    pub fn detach(
        &mut self,
        world: &mut VoxelWorld,
        physics: &mut Physics,
        changed: &[[i32; 3]],
        settles: u8,
        away_from: Option<(DVec3, f64)>,
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
                // Carriers (logs, sand) hit everything; leaves riding them nothing; leaves
                // falling on their own the world but not players, so they land. Sand passes
                // players (it may hurt them, `hits`, but never rests on one: owner, 2026-10-28,
                // "slide off").
                boxes.push(piece_box(o, m, k < piece.carriers, piece.carriers == 0));
                cells.push((o, m));
                clear(world, c, if s.flooded { M.water } else { M.air });
            }
            let mass: f64 = boxes.iter().map(|b| b.mass).sum();
            let body = physics.add_piece(origin, &boxes, false);
            let reach = boxes.iter().map(|b| b.center.length()).fold(0.0, f64::max) + 0.9;
            // Tipped away from who cut it: spun about the level axis across the line to them, about
            // its foot (the bottom of its cells, face-local, under its centre of mass), so the foot
            // starts still: a body's spin turns it about its centre of mass, which flung a tall
            // trunk's foot back (60 m: 18 m/s; owner, 2026-10-30, "flip out"). Its farthest point
            // at most `topple_tip_speed`: a tall trunk is nudged, gravity does the rest.
            if let Some((from, share)) = away_from.filter(|_| piece.carriers > 0) {
                let up = DVec3::Y * f64::from(FaceFrame::at_row(reference[1]).sign());
                let mut d = origin - from;
                d -= up * d.dot(up);
                if d.length() > 1e-6 {
                    let com = boxes.iter().map(|b| b.center * b.mass).sum::<DVec3>() / mass;
                    let low = boxes
                        .iter()
                        .map(|b| b.center.dot(up) - b.half.y)
                        .fold(f64::INFINITY, f64::min);
                    let foot = com - up * (com.dot(up) - low);
                    let far = boxes
                        .iter()
                        .map(|b| (b.center - foot).length() + b.half.length())
                        .fold(0.0, f64::max);
                    let spin = self
                        .rules
                        .topple_spin
                        .min(self.rules.topple_tip_speed / far.max(1e-6))
                        * share;
                    let w = up.cross(d.normalize()) * spin;
                    physics.set_angular_velocity(body, w);
                    physics.set_velocity(body, w.cross(com - foot));
                }
            }
            self.next_id += 1;
            self.pieces.push(Falling {
                id: self.next_id,
                body,
                cells,
                carriers: piece.carriers,
                kind: 0,
                settles,
                still: 0,
                rest: None,
                age: 0,
                hurt: Vec::new(),
                held: false,
                pushed: 0,
                anchored: false,
                reach,
            });
            out.push(self.pieces.len() - 1);
        }
        out
    }

    /// Breaks the cell `cell` (in its own grid) of the piece `id` (docs/BUILT_BODIES.md §4): its
    /// collider goes and its mass follows; a body left in more than one connected group splits (its
    /// kind's `split`), each group but the largest a new body of the same kind at its cells' world
    /// pose, moving as that part of the old one was; a body with no cells left is removed. None if
    /// Removes the piece `id` whole (a blast destroying it, docs/EXPLOSIVES.md §3.11): its body
    /// leaves the physics world. False if there is no such piece or its kind can't be broken.
    pub fn destroy_piece(&mut self, physics: &mut Physics, id: u32) -> bool {
        let Some(i) = self.pieces.iter().position(|p| p.id == id) else {
            return false;
        };
        if !body_kind(self.pieces[i].kind).breaks {
            return false;
        }
        let p = self.pieces.remove(i);
        physics.remove_body(p.body);
        true
    }

    /// there is no such body or cell, or its kind can't be broken.
    pub fn break_cell(
        &mut self,
        physics: &mut Physics,
        id: u32,
        cell: [i32; 3],
    ) -> Option<BreakOutcome> {
        let i = self.pieces.iter().position(|p| p.id == id)?;
        let kind = *body_kind(self.pieces[i].kind);
        if !kind.breaks {
            return None;
        }
        let k = self.pieces[i].cells.iter().position(|&(o, _)| o == cell)?;
        self.pieces[i].cells.remove(k);
        let mut out = BreakOutcome {
            removed: vec![cell],
            ..BreakOutcome::default()
        };
        if self.pieces[i].cells.is_empty() {
            let p = self.pieces.remove(i);
            physics.remove_body(p.body);
            out.gone = true;
            return Some(out);
        }
        let body = self.pieces[i].body;
        let (position, rotation) = (physics.position(body), physics.rotation(body));
        let spin = physics.angular_velocity(body);
        let mut groups = if kind.split {
            connected_groups(&self.pieces[i].cells)
        } else {
            vec![self.pieces[i].cells.clone()]
        };
        let keep = groups.remove(0);
        for group in groups {
            out.removed.extend(group.iter().map(|&(o, _)| o));
            // Its own frame: its first cell the reference, the old body's turn.
            let reference = group[0].0;
            let origin = position + rotation * offset_m(reference);
            let rel = group
                .iter()
                .map(|&(o, m)| {
                    (
                        [
                            o[0] - reference[0],
                            o[1] - reference[1],
                            o[2] - reference[2],
                        ],
                        m,
                    )
                })
                .collect();
            let (cells, carriers) = carriers_first(rel);
            let boxes = piece_boxes(&cells, carriers, self.pieces[i].kind);
            let reach = boxes.iter().map(|b| b.center.length()).fold(0.0, f64::max) + 0.9;
            let new = physics.add_piece(origin, &boxes, false);
            physics.set_rotation(new, rotation);
            physics.set_velocity(new, physics.point_velocity(body, origin));
            physics.set_angular_velocity(new, spin);
            let old = &self.pieces[i];
            let piece = Falling {
                id: self.next_id + 1,
                body: new,
                cells,
                carriers,
                kind: old.kind,
                settles: old.settles,
                still: 0,
                rest: None,
                age: old.age,
                hurt: old.hurt.clone(),
                held: false,
                pushed: 0,
                anchored: false,
                reach,
            };
            self.next_id += 1;
            self.pieces.push(piece);
            out.spawned.push(self.pieces.len() - 1);
        }
        let (cells, carriers) = carriers_first(keep);
        let boxes = piece_boxes(&cells, carriers, self.pieces[i].kind);
        physics.set_piece_cells(body, &boxes);
        let p = &mut self.pieces[i];
        p.reach = boxes.iter().map(|b| b.center.length()).fold(0.0, f64::max) + 0.9;
        p.cells = cells;
        p.carriers = carriers;
        p.still = 0;
        p.rest = None;
        Some(out)
    }

    /// Places `material` in the cell against face `face` (0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z, in its
    /// frame) of the cell `cell` of the body `id` (docs/BUILT_BODIES.md §4): it joins the body. The
    /// new cell, or None if there is no such body or cell, its kind can't be built on, or the cell
    /// is taken (`taken`: given its world centre, e.g. a player there).
    pub fn place_cell(
        &mut self,
        physics: &mut Physics,
        id: u32,
        cell: [i32; 3],
        face: u8,
        material: StateId,
        taken: &dyn Fn(DVec3) -> bool,
    ) -> Option<[i32; 3]> {
        let i = self.pieces.iter().position(|p| p.id == id)?;
        let p = &self.pieces[i];
        if !body_kind(p.kind).place || !p.cells.iter().any(|&(o, _)| o == cell) {
            return None;
        }
        let d = dwell_world::edit::FACE_DIRS.get(usize::from(face))?;
        let new = [cell[0] + d[0], cell[1] + d[1], cell[2] + d[2]];
        if p.cells.iter().any(|&(o, _)| o == new) {
            return None;
        }
        let at = physics.position(p.body) + physics.rotation(p.body) * offset_m(new);
        if taken(at) {
            return None;
        }
        self.join(physics, i, new, material);
        Some(new)
    }

    /// Adds the cell `new` (its grid) holding `material` to piece `i`: its colliders, mass and reach
    /// follow.
    fn join(&mut self, physics: &mut Physics, i: usize, new: [i32; 3], material: StateId) {
        let p = &self.pieces[i];
        let mut cells = p.cells.clone();
        cells.push((new, material));
        let (cells, carriers) = carriers_first(cells);
        let boxes = piece_boxes(&cells, carriers, p.kind);
        physics.set_piece_cells(p.body, &boxes);
        let p = &mut self.pieces[i];
        p.reach = boxes.iter().map(|b| b.center.length()).fold(0.0, f64::max) + 0.9;
        p.cells = cells;
        p.carriers = carriers;
        p.still = 0;
        p.rest = None;
    }

    /// The world cell each cell of piece `i` is in now.
    fn world_cells(&self, physics: &Physics, i: usize) -> Vec<([i32; 3], [i32; 3])> {
        let p = &self.pieces[i];
        let (position, rotation) = (physics.position(p.body), physics.rotation(p.body));
        p.cells
            .iter()
            .map(|&(o, _)| {
                let at = position + rotation * offset_m(o);
                (o, dwell_world::cell_of_pos(at.to_array()))
            })
            .collect()
    }

    /// Whether any cell of piece `i` rests on a block its kind is built on (face-locally under it).
    fn rests_on_platform(&self, world: &mut VoxelWorld, physics: &Physics, i: usize) -> bool {
        let kind = self.pieces[i].kind;
        self.world_cells(physics, i).into_iter().any(|(_, c)| {
            let below = c[1] - FaceFrame::at_row(c[1]).sign();
            kind_built_on(world.get_voxel(c[0], below, c[2])) == Some(kind)
        })
    }

    /// Where a block of `material` placed into the world's cell `cell` goes instead of the grid
    /// (docs/BUILT_BODIES.md §3): into a held body it touches (or nowhere, a held body's cell being
    /// there), else a new body when it is on a build platform's top; None: the grid. A build
    /// platform itself always goes in the grid (it is ground, never part of a body).
    pub fn divert(
        &self,
        world: &mut VoxelWorld,
        physics: &Physics,
        cell: [i32; 3],
        material: StateId,
    ) -> Option<Divert> {
        if kind_built_on(material).is_some() {
            return None;
        }
        for (i, p) in self.pieces.iter().enumerate() {
            if !p.anchored || !body_kind(p.kind).place {
                continue;
            }
            let cells = self.world_cells(physics, i);
            if cells.iter().any(|&(_, c)| c == cell) {
                return Some(Divert::Taken);
            }
            for &(o, c) in &cells {
                for d in &dwell_world::edit::FACE_DIRS {
                    if [c[0] + d[0], c[1] + d[1], c[2] + d[2]] == cell {
                        // Its grid is the world's turned by none (a held body is never turned).
                        return Some(Divert::Join {
                            id: p.id,
                            cell: [o[0] + d[0], o[1] + d[1], o[2] + d[2]],
                        });
                    }
                }
            }
        }
        let below = cell[1] - FaceFrame::at_row(cell[1]).sign();
        kind_built_on(world.get_voxel(cell[0], below, cell[2])).map(|kind| Divert::New { kind })
    }

    /// Applies a [`Divert`] of `material` at the world's cell `cell`: the body it joined or started
    /// (its id) and, for a join, the new cell in its grid; None for `Taken`. A new one starts held.
    pub fn place_diverted(
        &mut self,
        physics: &mut Physics,
        divert: Divert,
        cell: [i32; 3],
        material: StateId,
    ) -> Option<(u32, Option<[i32; 3]>)> {
        match divert {
            Divert::Taken => None,
            Divert::Join { id, cell: new } => {
                let i = self.pieces.iter().position(|p| p.id == id)?;
                self.join(physics, i, new, material);
                Some((id, Some(new)))
            }
            Divert::New { kind } => {
                let i = self.spawn_body(
                    physics,
                    None,
                    kind,
                    0,
                    vec![([0, 0, 0], material)],
                    cell_centre(cell),
                    DQuat::IDENTITY,
                    DVec3::ZERO,
                    DVec3::ZERO,
                );
                self.pieces[i].anchored = true;
                physics.set_held(self.pieces[i].body, true);
                Some((self.pieces[i].id, None))
            }
        }
    }

    /// Lets go of the held bodies no cell of which rests on a build platform any more (one broken
    /// under it): they move from now on (docs/BUILT_BODIES.md §3).
    pub fn release_unheld(&mut self, world: &mut VoxelWorld, physics: &mut Physics) {
        for i in 0..self.pieces.len() {
            if self.pieces[i].anchored && !self.rests_on_platform(world, physics, i) {
                self.pieces[i].anchored = false;
                physics.set_held(self.pieces[i].body, false);
            }
        }
    }

    /// Starts a body of `kind` with `cells` (offsets in its grid, its reference cell's centre at
    /// `position`), at its pose and velocities (a placed build platform, docs/BUILT_BODIES.md §3; a
    /// body loaded with the world, §7). Its index in `pieces`.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_body(
        &mut self,
        physics: &mut Physics,
        id: Option<u32>,
        kind: u8,
        settles: u8,
        cells: Vec<([i32; 3], StateId)>,
        position: DVec3,
        rotation: DQuat,
        velocity: DVec3,
        angular: DVec3,
    ) -> usize {
        let (cells, carriers) = carriers_first(cells);
        let boxes = piece_boxes(&cells, carriers, kind);
        let reach = boxes.iter().map(|b| b.center.length()).fold(0.0, f64::max) + 0.9;
        let body = physics.add_piece(position, &boxes, false);
        physics.set_rotation(body, rotation);
        physics.set_velocity(body, velocity);
        physics.set_angular_velocity(body, angular);
        let id = id.unwrap_or(self.next_id + 1);
        self.next_id = self.next_id.max(id);
        self.pieces.push(Falling {
            id,
            body,
            cells,
            carriers,
            kind,
            settles,
            still: 0,
            rest: None,
            age: 0,
            hurt: Vec::new(),
            held: false,
            pushed: 0,
            reach,
            anchored: false,
        });
        self.pieces.len() - 1
    }

    /// The bodies to save with the world (their kind's `saved`; docs/BUILT_BODIES.md §7).
    pub fn saved(&self, physics: &Physics) -> Vec<dwell_storage::SavedBody> {
        self.pieces
            .iter()
            .filter(|p| body_kind(p.kind).saved)
            .map(|p| {
                let q = physics.rotation(p.body);
                dwell_storage::SavedBody {
                    id: p.id,
                    kind: p.kind,
                    settles: p.settles,
                    position: physics.position(p.body).to_array(),
                    rotation: [q.x, q.y, q.z, q.w],
                    velocity: physics.velocity(p.body).to_array(),
                    angular: physics.angular_velocity(p.body).to_array(),
                    cells: p
                        .cells
                        .iter()
                        .map(|&(o, m)| (o.map(|v| v as i16), m))
                        .collect(),
                }
            })
            .collect()
    }

    /// Brings back the bodies saved with the world, as they were.
    /// A body unturned and resting on its build platform is held again.
    pub fn restore(
        &mut self,
        world: &mut VoxelWorld,
        physics: &mut Physics,
        bodies: Vec<dwell_storage::SavedBody>,
    ) {
        for b in bodies.into_iter().filter(|b| !b.cells.is_empty()) {
            let r = b.rotation;
            let i = self.spawn_body(
                physics,
                Some(b.id),
                b.kind,
                b.settles,
                b.cells
                    .iter()
                    .map(|&(o, m)| (o.map(i32::from), m))
                    .collect(),
                DVec3::from_array(b.position),
                DQuat::from_xyzw(r[0], r[1], r[2], r[3]).normalize(),
                DVec3::from_array(b.velocity),
                DVec3::from_array(b.angular),
            );
            let turned = physics
                .rotation(self.pieces[i].body)
                .angle_between(DQuat::IDENTITY)
                > 1e-3;
            if !turned && self.rests_on_platform(world, physics, i) {
                self.pieces[i].anchored = true;
                physics.set_held(self.pieces[i].body, true);
            }
        }
    }

    /// Floats the bodies for a tick (docs/BUILT_BODIES.md §6): each cell in a liquid is pushed up
    /// (face-locally: away from the midplane) by the liquid it displaces — its fraction under, its
    /// volume, the liquid's density and gravity — at its centre, so a raft rights itself and a heavy
    /// end sinks first; a body in water slows by `WATER`'s drags times how much of it is under.
    pub fn float(&mut self, world: &mut VoxelWorld, physics: &mut Physics) {
        let dt = 1.0 / f64::from(dwell_protocol::SIM_HZ);
        let g = physics.gravity();
        // A cell is 1 × ½ × 1 m.
        let volume = CELL_H;
        // The bodies only take impulses here: the world stands still, so the falls' reads hold.
        let mut falls = dwell_world::falls::FallMemo::default();
        for p in &mut self.pieces {
            let (position, rotation) = (physics.position(p.body), physics.rotation(p.body));
            let mut under = 0.0;
            for &(o, _) in &p.cells {
                let centre = position + rotation * offset_m(o);
                let (fraction, density) = cell_in_liquid(world, centre, rotation);
                if fraction <= 0.0 {
                    continue;
                }
                let up = DVec3::Y * FaceFrame::at_height(centre.y).sign_f64();
                physics.apply_impulse_at(
                    p.body,
                    up * (density * g * volume * fraction * dt),
                    centre,
                );
                under += fraction;
            }
            if under > 0.0 {
                let share = under / p.cells.len() as f64;
                // Sideways (the vertical axis is the same on both faces), as the clients'
                // stand-ins are slowed; the bob is the heave drag's.
                let v = water_drag_with(world, physics, p.body, &mut falls)
                    .slow(physics.velocity(p.body), dt);
                let heave = (-WATER.heave_drag * share * dt).exp();
                let v = DVec3::new(v.x, v.y * heave, v.z);
                let w = physics.angular_velocity(p.body) * (-WATER.angular_drag * share * dt).exp();
                physics.set_velocity(p.body, v);
                physics.set_angular_velocity(p.body, w);
            }
        }
    }

    /// The bodies players pushed this tick (`Players::pushes`): held from their contacts for
    /// `push_hold_ticks` more, so the push alone moves them (as the clients predict it).
    pub fn pushed(&mut self, physics: &mut Physics, bodies: &[(BodyHandle, DVec3)]) {
        for p in &mut self.pieces {
            if bodies.iter().any(|&(b, _)| b == p.body) {
                p.pushed = self.rules.push_hold_ticks;
                if !p.held {
                    physics.set_dominant(p.body, true);
                    p.held = true;
                }
            }
        }
    }

    /// The box (world metres) round each piece the terrain collision must cover over the next
    /// ticks: its cells' bounds at its pose now, grown by two ticks' travel of its farthest point
    /// (its speed and its spin times its reach) and a margin. Every piece: leaves falling on their
    /// own land on the terrain too (owner, 2026-10-28). Its own bounds, not a cube of its reach
    /// round its reference cell: for a 20 m trunk standing on its foot that cube was 44 m across
    /// and its terrain stalled the cut's tick (owner, 2026-10-30).
    pub fn terrain_boxes(&self, physics: &Physics) -> Vec<(DVec3, DVec3)> {
        // A cell's corners are within this of its centre (1 × ½ × 1 m).
        let half_diagonal = DVec3::new(0.5, 0.5 * CELL_H, 0.5).length();
        let dt = 1.0 / f64::from(dwell_protocol::SIM_HZ);
        self.pieces
            .iter()
            .map(|p| {
                let c = physics.position(p.body);
                let r = physics.rotation(p.body);
                let (mut lo, mut hi) = (c, c);
                let mut reach = 0.0f64;
                for &(o, _) in &p.cells {
                    let at = c + r * offset_m(o);
                    lo = lo.min(at);
                    hi = hi.max(at);
                    reach = reach.max(offset_m(o).length());
                }
                let travel = physics.velocity(p.body).length()
                    + physics.angular_velocity(p.body).length() * (reach + half_diagonal);
                let grow = half_diagonal + 2.0 * travel * dt + 1.0;
                (lo - DVec3::splat(grow), hi + DVec3::splat(grow))
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
    /// And holds the pieces moving slowly (or pushed lately) from players' contacts, letting the
    /// faster go.
    pub fn step(&mut self, physics: &mut Physics) -> Vec<usize> {
        let r = self.rules;
        let mut due = Vec::new();
        self.steps = self.steps.wrapping_add(1);
        let settling = self.steps.is_multiple_of(r.settle_every_ticks.max(1));
        for (i, p) in self.pieces.iter_mut().enumerate() {
            p.age += 1;
            let v = physics.velocity(p.body).length();
            let w = physics.angular_velocity(p.body).length();
            p.pushed = p.pushed.saturating_sub(1);
            let held = p.pushed > 0 || v + w * p.reach < r.hold_speed;
            if held != p.held {
                physics.set_dominant(p.body, held);
                p.held = held;
            }
            // Still: slow, or (jostled by its neighbours in a heap, rocked by water) not gone
            // anywhere since it began counting: within `rest_drift` at its farthest point.
            let (at, turn) = (physics.position(p.body), physics.rotation(p.body));
            let calm = v < r.sleep_linear && w < r.sleep_angular;
            let stayed = p.rest.is_some_and(|(a, q)| {
                (at - a).length() + q.angle_between(turn) * p.reach <= r.rest_drift
            });
            if calm || stayed {
                p.rest.get_or_insert((at, turn));
                p.still += 1;
            } else {
                p.rest = Some((at, turn));
                p.still = 0;
            }
            // Anything past the world's rows is lost; the rest settles at rest or out of time
            // (leaves falling on their own too: they land, owner 2026-10-28).
            let y = physics.position(p.body).y;
            let lost = !in_world_rows(y);
            // A kind that doesn't settle (a built body) stays a body; lost past the rows, it goes.
            let settles = body_kind(p.kind).settles;
            if lost || (settling && settles && (p.still >= r.sleep_ticks || p.age >= r.max_ticks)) {
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
    // Each grid cell's candidates: the piece cells whose box holds its centre, both of them where
    // it lies on their shared face (a ½ m cell tipped onto its side half fills two rows of one
    // column; its centres lie on its faces).
    let mut grid: Vec<([i32; 3], Vec<[i32; 3]>)> = Vec::new();
    for z in lo.z.floor() as i32..=hi.z.floor() as i32 {
        for y in rows(lo.y)..=rows(hi.y) {
            for x in lo.x.floor() as i32..=hi.x.floor() as i32 {
                let l = inverse * (cell_centre([x, y, z]) - origin);
                let along = [l.x + 0.5, l.y / CELL_H + 0.5, l.z + 0.5];
                let options = along.map(|v| {
                    let o = (v + 1e-9).floor() as i32;
                    if (v - v.round()).abs() < 1e-6 {
                        vec![o, o - 1]
                    } else {
                        vec![o]
                    }
                });
                let mut held = Vec::new();
                for &ox in &options[0] {
                    for &oy in &options[1] {
                        for &oz in &options[2] {
                            if index.contains_key(&[ox, oy, oz]) {
                                held.push([ox, oy, oz]);
                            }
                        }
                    }
                }
                if !held.is_empty() {
                    grid.push(([x, y, z], held));
                }
            }
        }
    }
    // Each piece cell fills at most one grid cell (a piece keeps its volume): the grid cells with
    // the fewest candidates first, then the ground side first (face-local lowest), so a lone tipped
    // cell lands on its lower row and a tipped pair fills its column.
    let ground = |y: i32| if FaceFrame::at_row(y).is_b() { -y } else { y };
    grid.sort_by_key(|(c, held)| (held.len(), ground(c[1]), c[0], c[2]));
    let mut used = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (c, held) in grid {
        if let Some(&o) = held.iter().find(|o| !used.contains(*o)) {
            used.insert(o);
            out.push((c, turned(index[&o], rotation)));
        }
    }
    // In grid order (z, then rows, then x), as before.
    out.sort_by_key(|(c, _)| (c[2], c[1], c[0]));
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

/// Which pieces a player's snapshots carry (docs/FELLING.md §3.1): a snapshot's datagram holds
/// only so many (`room`), and nearest first left every piece past the nearest ~28 out of every
/// snapshot, frozen on the player's screen (owner, 2026-11-02: "the server can't handle more than
/// like 30 dynamic blocks"). Each piece gains priority every snapshot it is left out — more for a
/// near or a fast one — and the snapshot takes the highest, so every moving piece is sent, the near
/// and fast ones most often. A piece at rest is sent once and then not again until it moves.
#[derive(Clone, Debug, Default)]
pub struct PieceBook {
    priority: std::collections::HashMap<u32, f64>,
    /// The pose each piece was last sent at (position, rotation).
    sent: std::collections::HashMap<u32, ([f64; 3], [f32; 4])>,
}

/// A piece within this of the pose last sent (m, and the rotation's quaternion components) is at
/// rest for the snapshots: not sent again. By its pose, not its speed: a raft drifting slower than
/// any speed threshold still moves (owner, 2026-11-02: a floating raft "froze until I broke a
/// block": below 0.05 m/s it counted as at rest and was never sent again).
const REST_MOVE: f64 = 0.01;
const REST_TURN: f32 = 0.002;

impl PieceBook {
    /// The pieces of `states` to send to a player at `at` (m), at most `room`, in send order.
    pub fn choose(
        &mut self,
        states: &[dwell_protocol::PieceState],
        at: [f64; 3],
        room: usize,
    ) -> Vec<dwell_protocol::PieceState> {
        let live: std::collections::HashSet<u32> = states.iter().map(|p| p.id).collect();
        self.priority.retain(|id, _| live.contains(id));
        self.sent.retain(|id, _| live.contains(id));
        let speed = |v: [f32; 3]| v.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();
        let mut candidates: Vec<(f64, &dwell_protocol::PieceState)> = Vec::new();
        for p in states {
            let v = speed(p.velocity);
            // At rest where it was last sent: nothing new to send.
            let unmoved = self.sent.get(&p.id).is_some_and(|(at, q)| {
                (0..3).all(|k| (p.position[k] - at[k]).abs() < REST_MOVE)
                    && (0..4).all(|k| (p.rotation[k] - q[k]).abs() < REST_TURN)
            });
            if unmoved && v < REST_MOVE && speed(p.angular) < f64::from(REST_TURN) {
                continue;
            }
            let distance = (0..3)
                .map(|i| (p.position[i] - at[i]).powi(2))
                .sum::<f64>()
                .sqrt();
            let gain = 1.0 + v / 4.0 + 8.0 / (1.0 + distance / 4.0);
            let priority = self.priority.entry(p.id).or_insert(0.0);
            *priority += gain;
            candidates.push((*priority, p));
        }
        candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.id.cmp(&b.1.id)));
        candidates.truncate(room);
        candidates
            .into_iter()
            .map(|(_, p)| {
                self.priority.insert(p.id, 0.0);
                self.sent.insert(p.id, (p.position, p.rotation));
                *p
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The heights (face-local, metres over the cell's floor) of `m`'s hull's top and bottom at
    /// the cell's hinge (+x) and notch (−x) edges, the wedges' notch on −x.
    fn edges(m: StateId) -> [f64; 4] {
        let half = DVec3::new(0.5, 0.25, 0.5);
        let pts = dwell_physics::shape_hull_points(m, half);
        let at = |x: f64| pts.iter().filter(move |p| (p.x - x).abs() < 1e-6);
        let top = |x: f64| at(x).map(|p| p.y + 0.25).fold(f64::NEG_INFINITY, f64::max);
        let bottom = |x: f64| at(x).map(|p| p.y + 0.25).fold(f64::INFINITY, f64::min);
        [top(0.5), top(-0.5), bottom(0.5), bottom(-0.5)]
    }

    #[test]
    fn the_fell_wedges_are_the_stumps_ramp_and_the_trunks_upside_down_ramp_meeting_at_the_hinge() {
        // face-a-only: `fell_wedges` is face-local (the server turns its states over on face B
        // with `FaceFrame::state`; tests/chopping.rs fells on both faces).
        let wedge = dwell_protocol::chopping::wood("dwell:log")
            .and_then(|w| w.fell_wedge)
            .expect("the log's fell wedge, data");
        let (stump, top) = fell_wedges(wedge, [-1.0, 0.0], WedgeTrunk::Open).expect("wedges");
        // The stump: full (½ m) at the hinge, nothing at the notch, standing on its floor.
        assert_eq!(edges(stump), [0.5, 0.0, 0.0, 0.0], "the stump's ramp");
        // The trunk's (the cell above): hanging from its ceiling, reaching its floor at the hinge
        // only, so it rests on the stump's top edge there with the notch open under it.
        assert_eq!(
            edges(top),
            [0.5, 0.5, 0.0, 0.5],
            "the trunk's upside-down ramp"
        );
        // Matching: parallel, reaching its floor at the notch.
        let (_, parallel) = fell_wedges(wedge, [-1.0, 0.0], WedgeTrunk::Matching).expect("wedges");
        assert_eq!(edges(parallel), [0.5, 0.5, 0.5, 0.0]);
        // The notch on +z turns them a quarter.
        let (s2, _) = fell_wedges(wedge, [0.0, 1.0], WedgeTrunk::Open).expect("wedges");
        assert_eq!(state_property(s2, "facing"), Some("south"));
        assert!(fell_wedges("dwell:stone", [-1.0, 0.0], WedgeTrunk::Open).is_none());
        // A shaped piece cell collides as its hull, a full one as its box.
        assert_eq!(dwell_physics::piece_hull(stump), Some(stump));
        assert_eq!(
            dwell_physics::piece_hull(dwell_blocks::require("dwell:log")),
            None
        );
        assert_eq!(dwell_physics::piece_hull(M.air), None);
    }

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
            kind: 0,
            settles: 0,
            still: 0,
            rest: None,
            age: 0,
            hurt: Vec::new(),
            held: false,
            pushed: 0,
            anchored: false,
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

    dwell_world::both_faces!(
        /// A ½ m cell tipped onto its side is ½ m wide and 1 m tall: it half fills two rows of
        /// one column, and settles as one cell, the one nearer the ground (owner, 2026-10-29: it
        /// settled as both, "effectively doubling itself"; tipped the other way, as none). Three
        /// tipped cells settle as three: a pair fills a column, the odd one its lower cell. Either
        /// way over, and the same on face B, mirrored.
        fn a_half_cell_tipped_onto_its_side_settles_as_one_cell(f: dwell_world::FaceFrame) {
            let settle = |f: dwell_world::FaceFrame, n: i32, turn: f64| {
                let p = piece(
                    (0..n)
                        .map(|y| {
                            (
                                [0, if f.is_b() { -y } else { y }, 0],
                                dwell_blocks::require("dwell:stone"),
                            )
                        })
                        .collect(),
                );
                let origin = DVec3::from_array(f.point([10.5, 20.25, 3.5]));
                // Tipped 90° about z (on face B its mirror: the turn about z reversed).
                let a = DQuat::from_rotation_z(turn);
                let tipped = if f.is_b() {
                    DQuat::from_xyzw(-a.x, a.y, -a.z, a.w)
                } else {
                    a
                };
                let mut cells: Vec<[i32; 3]> = resample(&p.cells, origin, tipped)
                    .1
                    .into_iter()
                    .map(|(c, _)| c)
                    .collect();
                cells.sort_unstable();
                cells
            };
            for turn in [-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2] {
                let a = dwell_world::FaceFrame::A;
                let one = settle(a, 1, turn);
                assert_eq!(one.len(), 1, "one cell, turned {turn}: {one:?}");
                let three = settle(a, 3, turn);
                assert_eq!(three.len(), 3, "turned {turn}: {three:?}");
                let lowest = three.iter().map(|c| c[1]).min().unwrap();
                assert_eq!(one[0][1], lowest, "on the ground side, turned {turn}");
                let columns: std::collections::BTreeSet<i32> = three.iter().map(|c| c[0]).collect();
                assert_eq!(columns.len(), 2, "a full column and a half: {three:?}");
                for n in [1, 3] {
                    let mut mirrored: Vec<[i32; 3]> =
                        settle(a, n, turn).iter().map(|&c| f.cell(c)).collect();
                    mirrored.sort_unstable();
                    assert_eq!(settle(f, n, turn), mirrored, "{n} cells turned {turn}");
                }
            }
        }
    );

    dwell_world::both_faces!(
        /// A trunk cut at its foot tips away from who cut it about its foot (owner, 2026-10-30:
        /// 40–120 log columns "flip out"): spun about its centre of mass, a 120-row (60 m) trunk's
        /// foot started skidding back at ~18 m/s and slid 20 m. Its foot starts still, its
        /// farthest point no faster than `topple_tip_speed`, its top away from the cutter: the
        /// same on face B (whose up is −y; tipped about the world's y it went toward them).
        fn a_tall_trunk_tips_about_its_foot_away_from_who_cut_it(f: dwell_world::FaceFrame) {
            let mut world = VoxelWorld::new(dwell_world::empty_generator(), None);
            let mut physics = Physics::new(dwell_physics::PhysicsConfig::default());
            let mut felling = Felling::default();
            let stone = dwell_blocks::require("dwell:stone");
            let n = 120;
            let [x, y, z] = f.cell([0, 40, 0]);
            world.set_voxel(x, y, z, f.state(stone));
            for i in 1..=n {
                let [x, y, z] = f.cell([0, 40 + i, 0]);
                world.set_voxel(x, y, z, f.state(log()));
            }
            // Cut at its foot (row 41), from −x.
            let cut = f.cell([0, 41, 0]);
            world.set_voxel(cut[0], cut[1], cut[2], M.air);
            let from = DVec3::from_array(f.point([-1.0, 42.0 * CELL_H + 1.6, 0.5]));
            let new = felling.detach(
                &mut world,
                &mut physics,
                &[cut],
                0,
                Some((from, 1.0)),
                &mut |w, c, m| w.set_voxel(c[0], c[1], c[2], m),
            );
            assert_eq!(new.len(), 1);
            let body = felling.pieces[new[0]].body;
            let foot = DVec3::from_array(f.point([0.5, 42.0 * CELL_H, 0.5]));
            let top = DVec3::from_array(f.point([0.5, (41.0 + f64::from(n)) * CELL_H, 0.5]));
            let foot_v = physics.point_velocity(body, foot);
            assert!(foot_v.length() < 1e-6, "the foot starts still: {foot_v:?}");
            let top_v = physics.point_velocity(body, top);
            assert!(
                top_v.length() <= FELLING.topple_tip_speed + 1e-6,
                "nudged: {top_v:?}"
            );
            assert!(
                top_v.x > 0.5,
                "its top away from the cutter (+x): {top_v:?}"
            );
        }
    );

    #[test]
    fn a_piece_that_settled_once_stays_put_even_unsupported() {
        // face-a-only: counts settles, no up. Owner, 2026-11-02: "The piece should only fall once
        // not 3 times." A piece that settled on nothing (a log on leaves) stays where it settled.
        // Red before: maxSettles 3 let it fall twice more.
        let mut world = VoxelWorld::new(dwell_world::empty_generator(), None);
        let mut physics = Physics::new(dwell_physics::PhysicsConfig::default());
        let mut felling = Felling::default();
        world.set_voxel(0, 41, 0, log());
        let mut clear =
            |w: &mut VoxelWorld, c: [i32; 3], m: StateId| w.set_voxel(c[0], c[1], c[2], m);
        let first = felling.detach(&mut world, &mut physics, &[[0, 41, 0]], 0, None, &mut clear);
        assert_eq!(first.len(), 1, "a first fall");
        world.set_voxel(4, 41, 0, log());
        let again = felling.detach(&mut world, &mut physics, &[[4, 41, 0]], 1, None, &mut clear);
        assert!(again.is_empty(), "settled once: it stays");
        assert_eq!(world.get_voxel(4, 41, 0), log());
    }

    fn moving(id: u32, x: f64, speed: f32) -> dwell_protocol::PieceState {
        dwell_protocol::PieceState {
            id,
            position: [x, 40.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            velocity: [0.0, -speed, 0.0],
            angular: [0.0; 3],
        }
    }

    #[test]
    fn every_moving_piece_is_sent_though_a_snapshot_holds_few() {
        // face-a-only: which pieces a snapshot carries has no up.
        // Owner, 2026-11-02: past ~28 pieces the rest froze (nearest first, never sent). 100
        // falling pieces, room for 25: within a few snapshots every one has been sent, and the
        // near ones more often than the far. Red before: the 75 farthest never were.
        let states: Vec<_> = (0..100)
            .map(|i| moving(i, f64::from(i) * 2.0, 5.0))
            .collect();
        let mut book = PieceBook::default();
        let mut sent = std::collections::HashMap::<u32, u32>::new();
        for _ in 0..8 {
            for p in book.choose(&states, [0.0, 40.0, 0.0], 25) {
                *sent.entry(p.id).or_default() += 1;
            }
        }
        assert_eq!(sent.len(), 100, "every piece sent: {} of 100", sent.len());
        assert!(
            sent[&0] > sent[&99],
            "near ones more often: {} vs {}",
            sent[&0],
            sent[&99]
        );
    }

    #[test]
    fn a_piece_drifting_slowly_is_sent_as_it_moves() {
        // face-a-only: as above. Owner, 2026-11-02: a floating raft "froze until I broke a block":
        // drifting at 0.03 m/s it counted as at rest and was never sent again. Each time it has
        // moved a centimetre from the pose last sent, it is sent. Red before: once only.
        let mut book = PieceBook::default();
        let mut sends = 0;
        for tick in 0..60 {
            let mut p = moving(1, 0.0, 0.0);
            p.velocity = [0.03, 0.0, 0.0];
            p.position[0] = 0.03 * f64::from(tick) / 3.0; // a snapshot every 3 ticks
            sends += book.choose(&[p], [0.0; 3], 25).len();
        }
        // 0.6 m over the run: a send each centimetre or so (every snapshot here).
        assert!(sends >= 30, "sent {sends} times");
    }

    #[test]
    fn a_piece_at_rest_is_sent_once_until_it_moves() {
        // face-a-only: as above.
        let mut book = PieceBook::default();
        let rest = [moving(1, 0.0, 0.0)];
        assert_eq!(
            book.choose(&rest, [0.0; 3], 25).len(),
            1,
            "its resting pose, once"
        );
        assert!(book.choose(&rest, [0.0; 3], 25).is_empty(), "then nothing");
        assert_eq!(
            book.choose(&[moving(1, 0.0, 3.0)], [0.0; 3], 25).len(),
            1,
            "moving again"
        );
    }

    dwell_world::both_faces!(
        /// How much of a cell is under water, face-locally (docs/BUILT_BODIES.md §6): all of one
        /// inside the water; a quarter of one whose centre is an eighth of a metre over the
        /// surface; none in the air. Water's density, 1000 kg/m³.
        fn a_cells_share_under_water_is_read_on_either_face(f: dwell_world::FaceFrame) {
            let mut world = VoxelWorld::new(dwell_world::empty_generator(), None);
            let water = dwell_blocks::require("dwell:water");
            for x in -2..=2 {
                for z in -2..=2 {
                    for y in 36..40 {
                        let [cx, cy, cz] = f.cell([x, y, z]);
                        world.set_voxel(cx, cy, cz, f.state(water));
                    }
                }
            }
            let at = |y: f64| DVec3::from_array(f.point([0.5, y, 0.5]));
            let surface = 40.0 * CELL_H;
            let (all, density) = cell_in_liquid(&mut world, at(surface - 1.0), DQuat::IDENTITY);
            assert_eq!((all, density), (1.0, 1000.0), "inside the water");
            let (some, _) = cell_in_liquid(&mut world, at(surface + 0.125), DQuat::IDENTITY);
            // Its centre an eighth of a metre over the surface: its lower quarter under.
            assert!((some - 0.25).abs() < 1e-9, "a quarter under: {some}");
            assert_eq!(
                cell_in_liquid(&mut world, at(surface + 1.0), DQuat::IDENTITY).0,
                0.0
            );
        }
    );

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
                hull: None,
            })
            .collect();
        let body = physics.add_piece(at, &boxes, true);
        physics.set_velocity(body, DVec3::new(0.0, -speed, 0.0));
        f.pieces.push(Falling {
            id: 1,
            body,
            cells: cells.to_vec(),
            carriers: 2,
            kind: 0,
            settles: 0,
            still: 0,
            rest: None,
            age: 0,
            hurt: Vec::new(),
            held: false,
            pushed: 0,
            anchored: false,
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
        assert_eq!(r.hold_speed, FELLING.hold_speed);
    }
}
