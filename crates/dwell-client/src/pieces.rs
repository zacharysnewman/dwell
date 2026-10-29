//! The falling pieces as this client sees them (docs/FELLING.md §3): each spawned by the server
//! (`EntitySpawn`, its cells), meshed once into blocks of its own cells, drawn at its pose
//! interpolated `INTERP_DELAY_MS` behind the latest snapshots (as remote players are), and stood in
//! for in the prediction by a kinematic body at its latest pose.

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

#[derive(Clone, Copy, Debug)]
struct Pose {
    at_ms: f64,
    position: DVec3,
    rotation: DQuat,
    velocity: DVec3,
}

struct Piece {
    cells: Vec<PieceCell>,
    poses: VecDeque<Pose>,
    /// The block its landings sound like (its first carrier's; 0: riders only, silent).
    block: StateId,
    landed_ms: f64,
}

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
        let boxes = cells
            .iter()
            .enumerate()
            .map(|(k, &(o, m))| PieceCell {
                center: DVec3::new(f64::from(o[0]), f64::from(o[1]) * CELL_H, f64::from(o[2])),
                half: DVec3::new(0.5, 0.5 * CELL_H, 0.5),
                mass: f64::from(info(m).density_kg_m3).max(1.0) * CELL_H,
                // Players stand on a piece's carriers only: not its leaves, nor falling sand,
                // which passes them (the server's `felling.rs`).
                collide: if k < usize::from(e.carriers)
                    && dwell_blocks::try_info(m)
                        .is_some_and(|s| s.support_role != dwell_blocks::SupportRole::Falls)
                {
                    dwell_physics::Collide::All
                } else {
                    dwell_physics::Collide::None
                },
            })
            .collect();
        let pose = Pose {
            at_ms: now_ms,
            position: DVec3::from(e.origin),
            rotation: quat(e.rotation),
            velocity: DVec3::ZERO,
        };
        let block = if e.carriers > 0 { cells[0].1 } else { 0 };
        if block != 0 {
            sounds.push(SoundEvent::new(SoundKind::Fell, block, 1.0, Some(e.origin)));
        }
        self.map.insert(
            e.id,
            Piece {
                cells: boxes,
                poses: VecDeque::from([pose]),
                block,
                landed_ms: f64::NEG_INFINITY,
            },
        );
        piece_meshes(&cells)
    }

    pub fn despawn(&mut self, id: u32) {
        self.map.remove(&id);
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
                position: DVec3::from(s.position),
                rotation: quat(s.rotation),
                velocity,
            });
            while p.poses.len() > KEEP {
                p.poses.pop_front();
            }
        }
    }

    /// Each piece's pose to draw at `now_ms`: interpolated between the snapshots round
    /// `now_ms − INTERP_DELAY_MS` (the newest one past them).
    pub fn poses(&self, now_ms: f64) -> Vec<(u32, [f64; 3], [f32; 4])> {
        let at = now_ms - players::INTERP_DELAY_MS as f64;
        self.map
            .iter()
            .map(|(&id, p)| {
                let (position, rotation) = pose_at(&p.poses, at);
                let r = rotation;
                (
                    id,
                    position.to_array(),
                    [r.x as f32, r.y as f32, r.z as f32, r.w as f32],
                )
            })
            .collect()
    }

    /// Each piece's latest pose and velocity with its boxes, for the prediction's stand-ins.
    pub fn latest(&self) -> Vec<(u32, &[PieceCell], DVec3, DQuat, DVec3)> {
        self.map
            .iter()
            .filter_map(|(&id, p)| {
                let l = p.poses.back()?;
                Some((id, p.cells.as_slice(), l.position, l.rotation, l.velocity))
            })
            .collect()
    }
}

/// The pose at time `at` among `poses` (oldest first): between the two round it, the first before
/// the oldest, the newest past the newest.
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
    let last = poses.back().expect("a pose");
    (last.position, last.rotation)
}

/// A piece's cells meshed in blocks of up to a chunk each (each with its neighbours' cells in its
/// apron, so faces between blocks are hidden), the positions moved to be relative to its reference
/// cell's centre; and how far its cells reach from it (m).
pub fn piece_meshes(cells: &[([i32; 3], StateId)]) -> (Vec<ChunkMeshes>, f64) {
    let n = CHUNK_SIZE;
    let lo = cells.iter().fold([i32::MAX; 3], |a, (o, _)| {
        std::array::from_fn(|k| a[k].min(o[k]))
    });
    let mut blocks: HashMap<[i32; 3], Vec<StateId>> = HashMap::default();
    for &(o, m) in cells {
        let rel: [i32; 3] = std::array::from_fn(|k| o[k] - lo[k]);
        // Its own block, and any whose apron holds it.
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let b = [
                        rel[0].div_euclid(n) + dx,
                        rel[1].div_euclid(n) + dy,
                        rel[2].div_euclid(n) + dz,
                    ];
                    let local: [i32; 3] = std::array::from_fn(|k| rel[k] - b[k] * n);
                    if local.iter().all(|&v| (-1..=n).contains(&v)) {
                        blocks.entry(b).or_insert_with(|| vec![0; PADDED_VOLUME])
                            [padded_index(local[0], local[1], local[2])] = m;
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    let mut keys: Vec<[i32; 3]> = blocks.keys().copied().collect();
    keys.sort_unstable();
    for b in keys {
        let Ok(mut meshes) = mesh_chunk(&blocks[&b], false) else {
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
    let radius = cells
        .iter()
        .map(|(o, _)| {
            DVec3::new(f64::from(o[0]), f64::from(o[1]) * CELL_H, f64::from(o[2])).length()
        })
        .fold(0.0, f64::max)
        + 1.0;
    (out, radius)
}

#[cfg(test)]
mod tests {
    use super::*;

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
                at_ms: t,
                position: DVec3::new(x, 0.0, 0.0),
                rotation: DQuat::IDENTITY,
                velocity: DVec3::ZERO,
            });
        }
        assert!((pose_at(&p, 75.0).0.x - 1.5).abs() < 1e-9);
        assert_eq!(pose_at(&p, -10.0).0.x, 0.0);
        assert_eq!(pose_at(&p, 500.0).0.x, 2.0);
        assert!(delay > 0.0);
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
            pieces.on_snapshot(&[state(vy)], 50.0 * k as f64 + 50.0, &mut sounds);
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
        pieces.on_snapshot(&[state(0.0)], 600.0, &mut sounds);
        pieces.on_snapshot(&[state(-3.0)], 650.0, &mut sounds);
        assert_eq!(
            sounds
                .iter()
                .filter(|s| s.kind == SoundKind::PieceLand)
                .count(),
            1
        );
        assert!(sounds[0].intensity > 0.2);
    }
}
