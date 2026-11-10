//! The falling pieces as this client sees them (docs/FELLING.md §3): each spawned by the server
//! (`EntitySpawn`, its cells), meshed once into blocks of its own cells, drawn at its pose
//! interpolated `INTERP_DELAY_MS` behind the latest snapshots (as remote players are), and stood in
//! for in the prediction by a kinematic body at its latest pose carried on to the predicted present.
//! A piece near the local player is drawn where that stand-in is, the present the player is drawn
//! in, blended in over `NEAR_BLEND_SECONDS` (docs/BUILT_BODIES.md §6: pushing a raft drawn 100 ms
//! in the past put the player inside it).

use std::collections::VecDeque;

use dwell_blocks::{StateId, info};
use dwell_game::sound::{SoundEvent, SoundKind, speed_intensity};
use dwell_mesh::{ChunkMeshes, PADDED_VOLUME, mesh_chunk, padded_index};
use dwell_physics::{DQuat, DVec3, PieceCell};
use dwell_protocol::{CHUNK_SIZE, EntitySpawn, PieceState, players};
use dwell_world::CELL_H;
use dwell_world::hash::HashMap;

/// How many snapshot poses a piece keeps (the interpolation needs the two round its draw time).
const KEEP: usize = 8;

/// A piece sounds a landing at most this often (ms): a log rolling to rest knocks a few times, not
/// at every snapshot.
const LAND_EVERY_MS: f64 = 250.0;

/// A piece within this (m) of the local player, past its cells' reach, is drawn at the present.
const NEAR_M: f64 = 2.5;
/// How long (s) a piece takes to go between its past (interpolated) and present (predicted) poses.
const NEAR_BLEND_SECONDS: f64 = 0.25;

#[derive(Clone, Copy, Debug)]
struct Pose {
    at_ms: f64,
    /// The input seq the snapshot carrying it acknowledged (0: from its spawn).
    seq: u32,
    position: DVec3,
    rotation: DQuat,
    velocity: DVec3,
    /// Its spin (rad/s, the axis scaled by the rate).
    angular: DVec3,
}

/// How far past its newest snapshot (ms) a piece is drawn going on at its velocity and spin: a
/// snapshot carries each of many pieces only every few (docs/FELLING.md §3.1).
const EXTRAPOLATE_MS: f64 = 250.0;

struct Piece {
    cells: Vec<PieceCell>,
    /// Its cells' offsets and states (this client's), for its settle glide, its edits and aiming
    /// at it.
    blocks: Vec<([i32; 3], StateId)>,
    /// Its kind (`dwell_sim::felling::BODY_KINDS`; 0 natural).
    kind: u8,
    poses: VecDeque<Pose>,
    /// Its stand-in's pose in the prediction (the present), and how far it is drawn there (0 at
    /// its past pose, 1 at the present).
    present: Option<(DVec3, DQuat)>,
    near: f64,
    /// The block its landings sound like (its first carrier's; 0: riders only, silent).
    block: StateId,
    landed_ms: f64,
}

/// A piece's latest pose for the prediction: its id, boxes, position, rotation, velocity and the
/// input seq its snapshot acknowledged.
pub type Latest<'a> = (u32, &'a [PieceCell], DVec3, DQuat, DVec3, u32);

/// How far past its farthest cell a piece's blocks may settle from where it was last sent (m):
/// the server lifts a piece up to two cells to free it, and a snapshot may lag its last moves.
const SETTLE_REACH_M: f64 = 4.0;

/// The pieces falling now.
#[derive(Default)]
pub struct Pieces {
    map: HashMap<u32, Piece>,
}

fn quat(q: [f32; 4]) -> DQuat {
    DQuat::from_xyzw(
        f64::from(q[0]),
        f64::from(q[1]),
        f64::from(q[2]),
        f64::from(q[3]),
    )
    .normalize()
}

impl Pieces {
    /// A piece began to fall: kept with its spawn pose; returns its meshes (positions relative to
    /// its reference cell's centre) and how far they reach (m), and a tree's crack where it went
    /// (`sounds`). Its states are this client's.
    pub fn spawn(
        &mut self,
        e: &EntitySpawn,
        now_ms: f64,
        sounds: &mut Vec<SoundEvent>,
    ) -> (Vec<ChunkMeshes>, f64) {
        let cells: Vec<([i32; 3], StateId)> = e
            .cells
            .iter()
            .map(|c| (c.offset.map(i32::from), c.state))
            .collect();
        let boxes = piece_boxes(&cells, e.kind);
        let pose = Pose {
            at_ms: now_ms,
            seq: 0,
            position: DVec3::from(e.origin),
            rotation: quat(e.rotation),
            velocity: DVec3::ZERO,
            angular: DVec3::ZERO,
        };
        let block = if e.carriers > 0 { cells[0].1 } else { 0 };
        if block != 0 {
            sounds.push(SoundEvent::new(SoundKind::Fell, block, 1.0, Some(e.origin)));
        }
        self.map.insert(
            e.id,
            Piece {
                cells: boxes,
                blocks: cells.clone(),
                kind: e.kind,
                poses: VecDeque::from([pose]),
                present: None,
                near: 0.0,
                block,
                landed_ms: f64::NEG_INFINITY,
            },
        );
        piece_meshes(&cells)
    }

    /// Whether world cell `c` lies within reach of a piece where it was last sent (its farthest
    /// cell and `SETTLE_REACH_M` more): a `Rebake` there may be its blocks settling.
    pub fn near_cell(&self, c: [i32; 3]) -> bool {
        let at = DVec3::from(dwell_world::pos_of_cell(c)) + DVec3::new(0.5, 0.5 * CELL_H, 0.5);
        self.map.values().any(|p| {
            let Some(last) = p.poses.back() else {
                return false;
            };
            let reach = p
                .blocks
                .iter()
                .map(|&(o, _)| {
                    DVec3::new(f64::from(o[0]), f64::from(o[1]) * CELL_H, f64::from(o[2])).length()
                })
                .fold(0.0, f64::max);
            last.position.distance(at) <= reach + SETTLE_REACH_M
        })
    }

    /// The piece settled (or went): forgotten, and returned as it was drawn at `now_ms` and last
    /// sent, for its settle glide.
    pub fn despawn(&mut self, id: u32, now_ms: f64) -> Option<crate::settle_glide::SettledPiece> {
        let p = self.map.remove(&id)?;
        let drawn = p.drawn(now_ms);
        let last = p.poses.back()?;
        Some(crate::settle_glide::SettledPiece {
            cells: p.blocks,
            drawn,
            last: (last.position, last.rotation),
        })
    }

    /// A body's cells changed (`BodyEdit`, docs/BUILT_BODIES.md §4; state 0: removed): its boxes
    /// follow, and its new meshes and reach are returned (None: no such body here).
    pub fn edit(&mut self, e: &dwell_protocol::BodyEdit) -> Option<(Vec<ChunkMeshes>, f64)> {
        let p = self.map.get_mut(&e.body)?;
        for c in &e.cells {
            let o = c.offset.map(i32::from);
            p.blocks.retain(|&(b, _)| b != o);
            if c.state != 0 {
                p.blocks.push((o, c.state));
            }
        }
        p.cells = piece_boxes(&p.blocks, p.kind);
        Some(piece_meshes(&p.blocks))
    }

    /// The cell of a body the ray from `origin` along unit `dir` enters first within `max` (m), at
    /// the poses drawn at `now_ms`: the body, its cell (in its grid), the face entered (0 +X, 1 −X,
    /// 2 +Y, 3 −Y, 4 +Z, 5 −Z, in its frame) and the distance.
    pub fn raycast(&self, origin: DVec3, dir: DVec3, max: f64, now_ms: f64) -> Option<BodyHit> {
        let mut best: Option<BodyHit> = None;
        for (&id, p) in &self.map {
            let (position, rotation) = p.drawn(now_ms);
            let inverse = rotation.inverse();
            let local_origin = inverse * (origin - position);
            let local_dir = inverse * dir;
            let limit = best.map_or(max, |b| b.distance);
            if let Some((cell, face, distance)) =
                ray_cells(local_origin, local_dir, &p.blocks, limit)
            {
                best = Some(BodyHit {
                    body: id,
                    cell,
                    face,
                    distance,
                    position,
                    rotation,
                });
            }
        }
        best
    }

    /// The body `id`'s kind (an index into the body kinds; 0 natural).
    pub fn kind_of(&self, id: u32) -> Option<u8> {
        self.map.get(&id).map(|p| p.kind)
    }

    /// The block in the body `id`'s cell `cell` (its grid).
    pub fn block_at(&self, id: u32, cell: [i32; 3]) -> Option<StateId> {
        let p = self.map.get(&id)?;
        p.blocks.iter().find(|&&(o, _)| o == cell).map(|&(_, m)| m)
    }

    pub fn ids(&self) -> Vec<u32> {
        self.map.keys().copied().collect()
    }

    /// A snapshot's poses (pieces not yet spawned here are skipped), and the landings they show
    /// (`sounds`): a piece whose velocity changed by more than the `piece_land` sound's lowest
    /// speed since the last snapshot hit something (gravity alone changes it by under ½ m/s).
    pub fn on_snapshot(
        &mut self,
        states: &[PieceState],
        seq: u32,
        server_tick: u32,
        now_ms: f64,
        sounds: &mut Vec<SoundEvent>,
    ) {
        for s in states {
            let Some(p) = self.map.get_mut(&s.id) else {
                continue;
            };
            let velocity = DVec3::new(
                f64::from(s.velocity[0]),
                f64::from(s.velocity[1]),
                f64::from(s.velocity[2]),
            );
            let change = p
                .poses
                .back()
                .map_or(0.0, |l| (velocity - l.velocity).length());
            if p.block != 0
                && now_ms - p.landed_ms >= LAND_EVERY_MS
                && let Some(i) = speed_intensity(SoundKind::PieceLand, change)
            {
                p.landed_ms = now_ms;
                sounds.push(SoundEvent::new(
                    SoundKind::PieceLand,
                    p.block,
                    i,
                    Some(s.position),
                ));
            }
            p.poses.push_back(Pose {
                at_ms: now_ms,
                seq,
                position: DVec3::from(s.position),
                rotation: quat(s.rotation),
                velocity,
                angular: DVec3::new(
                    f64::from(s.angular[0]),
                    f64::from(s.angular[1]),
                    f64::from(s.angular[2]),
                ),
            });
            while p.poses.len() > KEEP {
                p.poses.pop_front();
            }
        }
    }

    /// Each piece's pose to draw at `now_ms`: interpolated between the snapshots round
    /// `now_ms − INTERP_DELAY_MS` (the newest one past them).
    pub fn poses(&self, now_ms: f64) -> Vec<(u32, [f64; 3], [f32; 4])> {
        self.map
            .iter()
            .map(|(&id, p)| {
                let (position, rotation) = p.drawn(now_ms);
                let r = rotation;
                (
                    id,
                    position.to_array(),
                    [r.x as f32, r.y as f32, r.z as f32, r.w as f32],
                )
            })
            .collect()
    }

    /// Each piece's latest pose and velocity with its boxes and the input seq its snapshot
    /// acknowledged, for the prediction's stand-ins.
    pub fn latest(&self) -> Vec<Latest<'_>> {
        self.map
            .iter()
            .filter_map(|(&id, p)| {
                let l = p.poses.back()?;
                Some((
                    id,
                    p.cells.as_slice(),
                    l.position,
                    l.rotation,
                    l.velocity,
                    l.seq,
                ))
            })
            .collect()
    }

    /// Each frame: the stand-ins' present poses (`present`, by piece) and the local player's
    /// position: a piece near the player moves toward being drawn at the present, the rest toward
    /// their past poses, over `NEAR_BLEND_SECONDS`.
    pub fn set_present(
        &mut self,
        present: impl Fn(u32) -> Option<(DVec3, DQuat)>,
        player: Option<DVec3>,
        dt: f64,
    ) {
        let step = dt / NEAR_BLEND_SECONDS;
        for (&id, p) in &mut self.map {
            p.present = present(id);
            let near = match (p.present, player) {
                (Some((at, _)), Some(me)) => {
                    let reach = p
                        .blocks
                        .iter()
                        .map(|&(o, _)| {
                            DVec3::new(f64::from(o[0]), f64::from(o[1]) * CELL_H, f64::from(o[2]))
                                .length()
                        })
                        .fold(0.0, f64::max)
                        + 1.0;
                    (me - at).length() < reach + NEAR_M
                }
                _ => false,
            };
            p.near = if near {
                (p.near + step).min(1.0)
            } else {
                (p.near - step).max(0.0)
            };
        }
    }
}

/// A body's cell aimed at ([`Pieces::raycast`]), with the body's drawn pose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyHit {
    pub body: u32,
    pub cell: [i32; 3],
    pub face: u8,
    pub distance: f64,
    pub position: DVec3,
    pub rotation: DQuat,
}

/// A body's boxes for its cells: in a natural body (kind 0) logs collide with players and the rest
/// don't (sand passes players, leaves are passable: the server's `felling.rs`); in a built one every
/// solid, non-passable block does.
fn piece_boxes(cells: &[([i32; 3], StateId)], kind: u8) -> Vec<PieceCell> {
    cells
        .iter()
        .map(|&(o, m)| {
            let s = dwell_blocks::try_info(m);
            let collides = if kind == 0 {
                s.is_some_and(|s| s.support_role == dwell_blocks::SupportRole::Carrier)
            } else {
                s.is_some_and(|s| s.solid && !s.passable)
            };
            PieceCell {
                center: DVec3::new(f64::from(o[0]), f64::from(o[1]) * CELL_H, f64::from(o[2])),
                half: DVec3::new(0.5, 0.5 * CELL_H, 0.5),
                mass: f64::from(info(m).density_kg_m3).max(1.0) * CELL_H,
                collide: if collides {
                    dwell_physics::Collide::All
                } else {
                    dwell_physics::Collide::None
                },
            }
        })
        .collect()
}

/// The first of `cells` (offsets in a body's grid, each a 1 × ½ × 1 m box about its centre) the
/// ray from `o` along `d` (the body's frame) enters within `max`: the cell, the face entered and the
/// distance along `d`.
pub fn ray_cells(
    o: DVec3,
    d: DVec3,
    cells: &[([i32; 3], StateId)],
    max: f64,
) -> Option<([i32; 3], u8, f64)> {
    let half = DVec3::new(0.5, 0.5 * CELL_H, 0.5);
    let mut best: Option<([i32; 3], u8, f64)> = None;
    for &(c, _) in cells {
        let centre = DVec3::new(f64::from(c[0]), f64::from(c[1]) * CELL_H, f64::from(c[2]));
        let (lo, hi) = (centre - half, centre + half);
        let (mut t0, mut t1, mut axis) = (0.0f64, max, usize::MAX);
        let mut miss = false;
        for k in 0..3 {
            if d[k].abs() < 1e-12 {
                if o[k] < lo[k] || o[k] > hi[k] {
                    miss = true;
                    break;
                }
                continue;
            }
            let (a, b) = ((lo[k] - o[k]) / d[k], (hi[k] - o[k]) / d[k]);
            let (near, far) = if a < b { (a, b) } else { (b, a) };
            if near > t0 {
                t0 = near;
                axis = k;
            }
            t1 = t1.min(far);
            if t0 > t1 {
                miss = true;
                break;
            }
        }
        // Started inside a cell: no face to enter (it isn't a target).
        if miss || axis == usize::MAX || best.is_some_and(|b| t0 >= b.2) {
            continue;
        }
        // Entered through the face toward the ray's start.
        let face = 2 * axis as u8 + u8::from(d[axis] > 0.0);
        best = Some((c, face, t0));
    }
    best
}

impl Piece {
    /// Where it is drawn at `now_ms`: its past pose (interpolated `INTERP_DELAY_MS` behind),
    /// blended toward its present (its stand-in's) as it nears the player.
    fn drawn(&self, now_ms: f64) -> (DVec3, DQuat) {
        let past = pose_at(&self.poses, now_ms - players::INTERP_DELAY_MS as f64);
        match self.present {
            Some((at, turn)) if self.near > 0.0 => {
                let t = self.near * self.near * (3.0 - 2.0 * self.near);
                (past.0.lerp(at, t), past.1.slerp(turn, t))
            }
            _ => past,
        }
    }
}

/// The pose at time `at` among `poses` (oldest first): between the two round it, the first before
/// the oldest, and past the newest, on from it at its velocity and spin (EXTRAPOLATE_MS at most).
fn pose_at(poses: &VecDeque<Pose>, at: f64) -> (DVec3, DQuat) {
    let first = poses.front().expect("a pose");
    if at <= first.at_ms {
        return (first.position, first.rotation);
    }
    for w in poses.iter().collect::<Vec<_>>().windows(2) {
        let (a, b) = (w[0], w[1]);
        if at <= b.at_ms {
            let t = ((at - a.at_ms) / (b.at_ms - a.at_ms).max(1e-6)).clamp(0.0, 1.0);
            return (
                a.position.lerp(b.position, t),
                a.rotation.slerp(b.rotation, t),
            );
        }
    }
    // Past the newest: on at its velocity and spin, for up to EXTRAPOLATE_MS.
    let last = poses.back().expect("a pose");
    let dt = (at - last.at_ms).clamp(0.0, EXTRAPOLATE_MS) / 1000.0;
    let turn = DQuat::from_scaled_axis(last.angular * dt);
    (
        last.position + last.velocity * dt,
        (turn * last.rotation).normalize(),
    )
}

/// The blocks (of up to a chunk each, from the piece's lowest corner `lo`) holding a cell of the
/// piece, in order: only these are meshed. A block whose apron alone holds a cell has no faces of
/// its own, but meshing it cost a whole chunk's mesh: a one-cell piece at a block's corner meshed
/// eight (a blast's 500 one-cell pieces stalled the frame 3.8 s, docs/EXPLOSIVES.md).
pub fn own_blocks(cells: &[([i32; 3], StateId)], lo: [i32; 3]) -> Vec<[i32; 3]> {
    let n = CHUNK_SIZE;
    let mut keys: Vec<[i32; 3]> = cells
        .iter()
        .map(|(o, _)| std::array::from_fn(|k| (o[k] - lo[k]).div_euclid(n)))
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// A piece's cells meshed in blocks of up to a chunk each (each with its neighbours' cells in its
/// apron, so faces between blocks are hidden), the positions moved to be relative to its reference
/// cell's centre; and how far its cells reach from it (m).
pub fn piece_meshes(cells: &[([i32; 3], StateId)]) -> (Vec<ChunkMeshes>, f64) {
    piece_meshes_mirrored(cells, false)
}

/// [`piece_meshes`], meshed as face B's chunks are (`mirror`: its faces toward face B's up take
/// the top texture), for blocks drawn where face B's terrain will draw them.
pub fn piece_meshes_mirrored(
    cells: &[([i32; 3], StateId)],
    mirror: bool,
) -> (Vec<ChunkMeshes>, f64) {
    let radius = cells
        .iter()
        .map(|(o, _)| {
            DVec3::new(f64::from(o[0]), f64::from(o[1]) * CELL_H, f64::from(o[2])).length()
        })
        .fold(0.0, f64::max)
        + 1.0;
    if let [(o, m)] = *cells {
        return (one_cell_meshes(o, m, mirror), radius);
    }
    (meshed_blocks(cells, mirror), radius)
}

thread_local! {
    /// One cell of each state meshed alone, per face (docs/EXPLOSIVES.md §4): a blast's ~500
    /// one-cell pieces are a few states, each meshed once.
    static ONE_CELL: std::cell::RefCell<HashMap<(StateId, bool), Vec<ChunkMeshes>>> =
        std::cell::RefCell::new(HashMap::default());
    /// How many blocks this thread has meshed for pieces (tests: what a blast's pieces cost).
    static BLOCKS_MESHED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many chunk-sized blocks this thread has meshed for pieces so far.
pub fn blocks_meshed() -> usize {
    BLOCKS_MESHED.with(std::cell::Cell::get)
}

/// A one-cell piece's meshes: its state's, meshed once at offset 0 and kept, moved to offset `o`.
fn one_cell_meshes(o: [i32; 3], m: StateId, mirror: bool) -> Vec<ChunkMeshes> {
    let mut meshes = ONE_CELL.with(|c| {
        c.borrow_mut()
            .entry((m, mirror))
            .or_insert_with(|| meshed_blocks(&[([0, 0, 0], m)], mirror))
            .clone()
    });
    if o != [0; 3] {
        let shift = [f64::from(o[0]), f64::from(o[1]) * CELL_H, f64::from(o[2])];
        for meshes in &mut meshes {
            for a in [&mut meshes.opaque, &mut meshes.cutout] {
                for p in a.positions.chunks_exact_mut(3) {
                    for k in 0..3 {
                        p[k] = (f64::from(p[k]) + shift[k]) as f32;
                    }
                }
            }
        }
    }
    meshes
}

/// A piece's cells meshed in the blocks holding them (`own_blocks`), each with its neighbours'
/// cells in its apron, in the piece's frame.
fn meshed_blocks(cells: &[([i32; 3], StateId)], mirror: bool) -> Vec<ChunkMeshes> {
    let n = CHUNK_SIZE;
    let lo = cells.iter().fold([i32::MAX; 3], |a, (o, _)| {
        std::array::from_fn(|k| a[k].min(o[k]))
    });
    let own = own_blocks(cells, lo);
    let mut blocks: HashMap<[i32; 3], Vec<StateId>> = HashMap::default();
    for &(o, m) in cells {
        let rel: [i32; 3] = std::array::from_fn(|k| o[k] - lo[k]);
        // Its own block, and any of the piece's own whose apron holds it (only those are meshed).
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let b = [
                        rel[0].div_euclid(n) + dx,
                        rel[1].div_euclid(n) + dy,
                        rel[2].div_euclid(n) + dz,
                    ];
                    let local: [i32; 3] = std::array::from_fn(|k| rel[k] - b[k] * n);
                    if local.iter().all(|&v| (-1..=n).contains(&v)) && own.binary_search(&b).is_ok()
                    {
                        blocks.entry(b).or_insert_with(|| vec![0; PADDED_VOLUME])
                            [padded_index(local[0], local[1], local[2])] = m;
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    for b in own {
        BLOCKS_MESHED.with(|c| c.set(c.get() + 1));
        let Ok(mut meshes) = mesh_chunk(&blocks[&b], mirror) else {
            continue;
        };
        // Block-local metres → the piece's frame (its reference cell's centre at the origin).
        let shift = [
            f64::from(lo[0] + b[0] * n) - 0.5,
            (f64::from(lo[1] + b[1] * n) - 0.5) * CELL_H,
            f64::from(lo[2] + b[2] * n) - 0.5,
        ];
        for m in [&mut meshes.opaque, &mut meshes.cutout] {
            for p in m.positions.chunks_exact_mut(3) {
                for k in 0..3 {
                    p[k] = (f64::from(p[k]) + shift[k]) as f32;
                }
            }
        }
        if !meshes.opaque.positions.is_empty() || !meshes.cutout.positions.is_empty() {
            out.push(meshes);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_world::FaceFrame;

    #[test]
    fn a_one_cell_piece_meshes_one_block_not_the_blocks_whose_apron_holds_it() {
        // docs/EXPLOSIVES.md: a blast's 500 one-cell pieces each meshed eight chunk-sized blocks
        // (its cell in seven aprons), 3.8 s of one frame in Chrome. Red before: 8 blocks.
        let stone = dwell_blocks::require("dwell:stone");
        let cells = vec![([0, 0, 0], stone)];
        assert_eq!(own_blocks(&cells, [0, 0, 0]), vec![[0, 0, 0]]);
        let (meshes, _) = piece_meshes(&cells);
        assert_eq!(meshes.len(), 1);
        let n = CHUNK_SIZE;
        let tall: Vec<_> = (0..n + 2).map(|y| ([0, y, 0], stone)).collect();
        assert_eq!(own_blocks(&tall, [0, 0, 0]), vec![[0, 0, 0], [0, 1, 0]]);
    }

    #[test]
    fn a_piece_taller_than_a_chunk_meshes_in_blocks_round_its_reference_cell() {
        // docs/FELLING.md: a trunk 40 rows tall spans two blocks; its geometry lies round its
        // reference cell (offset 0 at the origin): from −½ m across, up to 20 m.
        let log = dwell_blocks::parse_state("dwell:log[axis=y]").expect("log");
        let cells: Vec<([i32; 3], StateId)> = (0..40).map(|y| ([0, y, 0], log)).collect();
        let (meshes, radius) = piece_meshes(&cells);
        assert_eq!(meshes.len(), 2);
        let ys: Vec<f32> = meshes
            .iter()
            .flat_map(|m| {
                m.opaque
                    .positions
                    .chunks_exact(3)
                    .map(|p| p[1])
                    .collect::<Vec<_>>()
            })
            .collect();
        let (min, max) = ys
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), &y| (a.min(y), b.max(y)));
        assert!(
            (min + 0.25).abs() < 1e-5 && (max - 19.75).abs() < 1e-5,
            "{min}..{max}"
        );
        assert!(radius > 19.0);
    }

    #[test]
    fn poses_interpolate_between_snapshots_behind_the_latest() {
        let delay = players::INTERP_DELAY_MS as f64;
        let mut p = VecDeque::new();
        for (t, x) in [(0.0, 0.0), (50.0, 1.0), (100.0, 2.0)] {
            p.push_back(Pose {
                seq: 0,
                at_ms: t,
                position: DVec3::new(x, 0.0, 0.0),
                rotation: DQuat::IDENTITY,
                velocity: DVec3::ZERO,
                angular: DVec3::ZERO,
            });
        }
        assert!((pose_at(&p, 75.0).0.x - 1.5).abs() < 1e-9);
        assert_eq!(pose_at(&p, -10.0).0.x, 0.0);
        assert_eq!(pose_at(&p, 500.0).0.x, 2.0);
        assert!(delay > 0.0);
    }

    #[test]
    fn past_its_newest_snapshot_a_piece_goes_on_along_its_velocity_and_spin() {
        // Owner, 2026-11-02: with many pieces a snapshot carries each only every few (FELLING.md
        // §3.1); past its newest pose a piece goes on at its velocity and spin (for up to
        // EXTRAPOLATE_MS), not frozen there. Red before: it held its newest pose.
        let mut p = VecDeque::new();
        p.push_back(Pose {
            seq: 0,
            at_ms: 0.0,
            position: DVec3::new(0.0, 10.0, 0.0),
            rotation: DQuat::IDENTITY,
            velocity: DVec3::new(0.0, -4.0, 2.0),
            angular: DVec3::new(0.0, 1.0, 0.0),
        });
        let (at, r) = pose_at(&p, 100.0);
        assert!((at - DVec3::new(0.0, 9.6, 0.2)).length() < 1e-9, "{at}");
        assert!(
            (r.to_axis_angle().1 - 0.1).abs() < 1e-6,
            "turned 0.1 rad: {r:?}"
        );
        // Not past EXTRAPOLATE_MS.
        let (far, _) = pose_at(&p, 10_000.0);
        let most = EXTRAPOLATE_MS / 1000.0;
        assert!((far.y - (10.0 - 4.0 * most)).abs() < 1e-9, "{far}");
    }

    /// A flung one-cell piece as the server moves it at server time `t` (s): 6 m/s across,
    /// gravity toward the face's ground, a 2 rad/s spin about the up axis (a turn about y is the
    /// same on both faces: the mirror flips y alone, and the spin, an axial vector, stays).
    fn flung(t: f64, f: FaceFrame) -> (DVec3, DQuat, DVec3, DVec3) {
        let at = [6.0 * t, 40.0 + 4.0 * t - 4.9 * t * t, 0.0];
        let v = [6.0, 4.0 - 9.8 * t, 0.0];
        (
            DVec3::from(f.point(at)),
            DQuat::from_rotation_y(2.0 * t),
            DVec3::from(f.vector(v)),
            DVec3::new(0.0, 2.0, 0.0),
        )
    }

    /// Plays the flung piece through `Pieces` as the game does (60 Hz server ticks, a snapshot
    /// every 3; each read at the first frame after it arrives, a drawn pose a frame) for 3.5 s of
    /// frames `frame_ms` apart (cycled), each snapshot delivered in 40 ms plus a deterministic
    /// 0–20 ms of jitter. Returns the worst deviation of a frame's drawn speed across and spin
    /// rate from the server's (fractions), after the first second.
    fn drawn_jitter(frame_ms: &[f64], f: FaceFrame) -> (f64, f64) {
        let stone = dwell_blocks::require("dwell:stone");
        let mut pieces = Pieces::default();
        let mut sounds = Vec::new();
        // The local clock runs 1234.5 ms ahead of the server's.
        let skew = 1234.5;
        let mut seed = 7u32;
        let mut jitter = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            f64::from(seed >> 8) / f64::from(1u32 << 24) * 20.0
        };
        let tick_ms = 1000.0 / 60.0;
        let snapshots: Vec<(u32, f64)> = (1..400)
            .map(|k| 3 * k)
            .map(|tick| (tick, f64::from(tick) * tick_ms + skew + 40.0 + jitter()))
            .collect();
        let (origin, r, _, _) = flung(0.0, f);
        let spawn = EntitySpawn {
            id: 1,
            origin: origin.to_array(),
            rotation: [r.x as f32, r.y as f32, r.z as f32, r.w as f32],
            kind: 0,
            carriers: 1,
            cells: vec![dwell_protocol::PieceCellNet {
                offset: [0, 0, 0],
                state: stone,
            }],
        };
        let mut now = skew + 40.0;
        pieces.spawn(&spawn, now, &mut sounds);
        let mut next = 0;
        let mut drawn: Vec<(f64, DVec3, DQuat)> = Vec::new();
        let mut i = 0;
        while now < skew + 3500.0 {
            let dt = frame_ms[i % frame_ms.len()];
            now += dt;
            i += 1;
            while next < snapshots.len() && snapshots[next].1 <= now {
                let tick = snapshots[next].0;
                let (at, q, v, w) = flung(f64::from(tick) * tick_ms / 1000.0, f);
                let state = PieceState {
                    id: 1,
                    position: at.to_array(),
                    rotation: [q.x as f32, q.y as f32, q.z as f32, q.w as f32],
                    velocity: v.as_vec3().to_array(),
                    angular: w.as_vec3().to_array(),
                };
                pieces.on_snapshot(&[state], 0, tick, now, &mut sounds);
                next += 1;
            }
            pieces.set_present(|_| None, None, dt / 1000.0);
            let (_, at, q) = pieces.poses(now)[0];
            drawn.push((now, DVec3::from(at), quat(q)));
        }
        let (mut speed, mut spin) = (0.0f64, 0.0f64);
        for w in drawn.windows(2).filter(|w| w[0].0 > skew + 1000.0) {
            let (a, b) = (w[0], w[1]);
            let dt = (b.0 - a.0) / 1000.0;
            let across = (b.1 - a.1).x / dt;
            speed = speed.max((across / 6.0 - 1.0).abs());
            let turned = (b.2 * a.2.inverse()).normalize().to_axis_angle().1;
            let turned = turned.min(std::f64::consts::TAU - turned);
            spin = spin.max((turned / dt / 2.0 - 1.0).abs());
        }
        (speed, spin)
    }

    dwell_world::both_faces!(
        fn a_moving_piece_is_drawn_smoothly_however_its_snapshots_arrive(f: FaceFrame) {
            // Owner, 2026-11-09: every moving piece shakes, even one alone. Its poses were timed
            // by when a frame read their snapshot, not by the server tick they were taken at, so
            // 50 ms of motion was drawn over 33 or 67 ms as an arrival fell either side of a
            // frame. Red before: the drawn speed off by tens of percent at 60 and 120 Hz.
            let worst: Vec<(&str, f64, f64)> = [
                ("60 Hz", vec![1000.0 / 60.0]),
                ("120 Hz", vec![1000.0 / 120.0]),
                ("uneven", vec![9.0, 24.0, 14.0, 19.0]),
            ]
            .into_iter()
            .map(|(name, frames)| {
                let (speed, spin) = drawn_jitter(&frames, f);
                (name, (speed * 100.0).round(), (spin * 100.0).round())
            })
            .collect();
            assert!(
                worst.iter().all(|&(_, speed, spin)| speed < 5.0 && spin < 5.0),
                "drawn speed and spin off by (%): {worst:?}"
            );
        }
    );

    #[test]
    fn the_ray_enters_the_nearest_cell_of_a_body_through_the_face_toward_it() {
        // face-a-only: a body's own frame has no up (the faces are in it); its pose is the caller's.
        let log = dwell_blocks::parse_state("dwell:log[axis=y]").expect("log");
        let cells: Vec<([i32; 3], StateId)> = (0..4).map(|y| ([0, y, 0], log)).collect();
        // From −x, level with row 2 (1 m up): cell [0, 2, 0], through its −X face (1), 2.5 m.
        let hit = ray_cells(DVec3::new(-3.0, 1.0, 0.0), DVec3::X, &cells, 8.0);
        assert_eq!(hit.map(|h| (h.0, h.1)), Some(([0, 2, 0], 1)));
        assert!((hit.unwrap().2 - 2.5).abs() < 1e-9);
        // From above: the top cell's +Y face (2).
        let top = ray_cells(DVec3::new(0.0, 5.0, 0.0), -DVec3::Y, &cells, 8.0);
        assert_eq!(top.map(|h| (h.0, h.1)), Some(([0, 3, 0], 2)));
        // Out of reach, or passing beside it: none.
        assert!(ray_cells(DVec3::new(-3.0, 1.0, 0.0), DVec3::X, &cells, 2.0).is_none());
        assert!(ray_cells(DVec3::new(-3.0, 1.0, 2.0), DVec3::X, &cells, 8.0).is_none());
    }

    #[test]
    fn a_body_near_the_player_is_drawn_at_its_present_and_one_far_off_in_the_past() {
        // Docs/BUILT_BODIES.md §6 (owner, 2026-11-02: pushing a floating raft "feels soft"): drawn
        // 100 ms in the past, a raft the player pushes showed them inside it. Near the player a
        // body is drawn where its stand-in is (the present), blended in over 0.25 s; far off, in
        // the past as before. Red before: always in the past.
        let mut pieces = Pieces::default();
        let mut sounds = Vec::new();
        let log = dwell_blocks::parse_state("dwell:log[axis=y]").expect("log");
        pieces.spawn(
            &EntitySpawn {
                id: 4,
                kind: 1,
                origin: [0.5, 40.25, 0.5],
                rotation: [0.0, 0.0, 0.0, 1.0],
                carriers: 1,
                cells: vec![dwell_protocol::PieceCellNet {
                    offset: [0, 0, 0],
                    state: log,
                }],
            },
            0.0,
            &mut sounds,
        );
        let present = |_| Some((DVec3::new(1.5, 40.25, 0.5), DQuat::IDENTITY));
        let x = |p: &Pieces| p.poses(1000.0)[0].1[0];
        // Far: the past.
        for _ in 0..30 {
            pieces.set_present(present, Some(DVec3::new(30.5, 40.0, 0.5)), 1.0 / 60.0);
        }
        assert!((x(&pieces) - 0.5).abs() < 1e-9, "far: {}", x(&pieces));
        // Near: halfway in after a few frames, at the present after 0.25 s.
        for _ in 0..5 {
            pieces.set_present(present, Some(DVec3::new(2.5, 40.0, 0.5)), 1.0 / 60.0);
        }
        let partway = x(&pieces);
        assert!(partway > 0.5 && partway < 1.5, "blending: {partway}");
        for _ in 0..15 {
            pieces.set_present(present, Some(DVec3::new(2.5, 40.0, 0.5)), 1.0 / 60.0);
        }
        assert!((x(&pieces) - 1.5).abs() < 1e-9, "near: {}", x(&pieces));
    }

    #[test]
    fn a_body_aimed_at_in_the_world_is_turned_into_its_frame() {
        // face-a-only: as above. A 4-cell log lying along x (turned 90° about z), at (10, 20, 5):
        // aimed at from −y below its second cell, the ray meets that cell.
        let log = dwell_blocks::parse_state("dwell:log[axis=y]").expect("log");
        let mut pieces = Pieces::default();
        let mut sounds = Vec::new();
        let rotation = DQuat::from_rotation_z(-std::f64::consts::FRAC_PI_2);
        let r = rotation;
        pieces.spawn(
            &EntitySpawn {
                id: 4,
                origin: [10.0, 20.0, 5.0],
                rotation: [r.x as f32, r.y as f32, r.z as f32, r.w as f32],
                kind: 0,
                carriers: 4,
                cells: (0..4)
                    .map(|y| dwell_protocol::PieceCellNet {
                        offset: [0, y, 0],
                        state: log,
                    })
                    .collect(),
            },
            0.0,
            &mut sounds,
        );
        // Cell y = 2 is 1 m along the body's +y, which the turn takes to +x.
        let hit = pieces
            .raycast(DVec3::new(11.0, 17.0, 5.0), DVec3::Y, 8.0, 1000.0)
            .expect("a hit");
        assert_eq!((hit.body, hit.cell), (4, [0, 2, 0]));
        assert!((hit.distance - 2.5).abs() < 1e-6, "{}", hit.distance);
        // Broken off by an edit: the ray goes past it.
        pieces.edit(&dwell_protocol::BodyEdit {
            body: 4,
            cells: vec![dwell_protocol::PieceCellNet {
                offset: [0, 2, 0],
                state: 0,
            }],
        });
        assert!(
            pieces
                .raycast(DVec3::new(11.0, 17.0, 5.0), DVec3::Y, 8.0, 1000.0)
                .is_none()
        );
    }

    #[test]
    fn a_tree_cracks_as_it_goes_and_thuds_as_it_lands_once() {
        let log = dwell_blocks::parse_state("dwell:log[axis=y]").expect("log");
        let mut pieces = Pieces::default();
        let mut sounds = Vec::new();
        let spawn = EntitySpawn {
            id: 9,
            origin: [0.0, 100.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            kind: 0,
            carriers: 2,
            cells: (0..2)
                .map(|y| dwell_protocol::PieceCellNet {
                    offset: [0, y, 0],
                    state: log,
                })
                .collect(),
        };
        pieces.spawn(&spawn, 0.0, &mut sounds);
        assert_eq!(sounds.len(), 1);
        assert_eq!(sounds[0].kind, SoundKind::Fell);
        sounds.clear();
        let state = |vy: f32| PieceState {
            id: 9,
            position: [0.0, 99.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            velocity: [0.0, vy, 0.0],
            angular: [0.0; 3],
        };
        // Falling: gravity's ½ m/s a snapshot is no landing.
        for (k, vy) in [-0.5, -1.0, -1.5, -2.0, -2.5, -3.0, -8.0]
            .into_iter()
            .enumerate()
        {
            pieces.on_snapshot(
                &[state(vy)],
                0,
                3 * (k as u32 + 1),
                50.0 * k as f64 + 50.0,
                &mut sounds,
            );
        }
        // -3 to -8 is a shove, not a stop: still a change over 2 m/s, so it sounds (a hit).
        assert_eq!(
            sounds.len(),
            1,
            "{:?}",
            sounds.iter().map(|s| s.kind).collect::<Vec<_>>()
        );
        sounds.clear();
        // It hits the ground: -8 → 0, a landing; the bounce right after stays quiet.
        pieces.on_snapshot(&[state(0.0)], 0, 36, 600.0, &mut sounds);
        pieces.on_snapshot(&[state(-3.0)], 0, 39, 650.0, &mut sounds);
        assert_eq!(
            sounds
                .iter()
                .filter(|s| s.kind == SoundKind::PieceLand)
                .count(),
            1
        );
        assert!(sounds[0].intensity > 0.2);
    }

    #[test]
    fn a_blasts_512_one_cell_pieces_arrive_within_a_frame() {
        // docs/EXPLOSIVES.md §3: a blast's ~500 one-cell pieces all arrive in one frame. Each
        // meshed a chunk-sized block of its own (0.53 ms natively, 273 ms for 512; a 0.63 s frame
        // in Chrome). Red before: 512 blocks meshed. Now each state's cell is meshed once.
        let states = ["dwell:stone", "dwell:dirt", "dwell:sand"].map(dwell_blocks::require);
        let mut pieces = Pieces::default();
        let mut sounds = Vec::new();
        let before = blocks_meshed();
        let t = std::time::Instant::now();
        for id in 0..512u32 {
            let (meshes, radius) = pieces.spawn(
                &EntitySpawn {
                    id,
                    kind: 0,
                    origin: [f64::from(id % 8), 40.25, f64::from(id / 8)],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    carriers: 1,
                    cells: vec![dwell_protocol::PieceCellNet {
                        offset: [0, 0, 0],
                        state: states[id as usize % 3],
                    }],
                },
                0.0,
                &mut sounds,
            );
            assert_eq!(meshes.len(), 1);
            assert!(!meshes[0].opaque.positions.is_empty());
            assert!((radius - 1.0).abs() < 1e-9);
        }
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let meshed = blocks_meshed() - before;
        println!("512 one-cell pieces: {ms:.1} ms, {meshed} blocks meshed");
        assert!(meshed <= states.len(), "{meshed} blocks meshed");
        assert!(ms < 33.0, "{ms:.1} ms for 512 pieces");
    }

    #[test]
    fn a_one_cell_piece_meshes_as_its_cell_alone_wherever_its_offset() {
        // both-faces: the kept one-cell mesh, moved to its offset, is the cell meshed afresh,
        // face A's and face B's (mirrored) alike.
        let stone = dwell_blocks::require("dwell:stone");
        for mirror in [false, true] {
            for o in [[0, 0, 0], [3, -2, 1], [-40, 70, 5]] {
                let (kept, _) = piece_meshes_mirrored(&[(o, stone)], mirror);
                let fresh = meshed_blocks(&[(o, stone)], mirror);
                assert_eq!(kept.len(), fresh.len());
                for (a, b) in kept.iter().zip(&fresh) {
                    assert_eq!(a.opaque.normals, b.opaque.normals);
                    assert_eq!(a.opaque.uvs, b.opaque.uvs);
                    for (p, q) in a.opaque.positions.iter().zip(&b.opaque.positions) {
                        assert!((p - q).abs() < 1e-4, "{o:?} {mirror}: {p} vs {q}");
                    }
                }
            }
        }
    }
}
