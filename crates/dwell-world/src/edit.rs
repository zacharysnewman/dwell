//! Block interaction (ARCHITECTURE.md §6.5): the targeting ray cast shared by client (the
//! crosshair's cell) and server (line of sight), and the server's validation of a
//! `BlockEditRequest` (§11). Pure functions of the voxel world and the players' capsules.

use dwell_blocks::shape::{faces_of, ray_enter_shape, shape_of, vertical_segment_distance_sq};
use dwell_blocks::{ShapeInfo, StateId, VoxelShape, info, state_property, with_property};
use dwell_protocol::{BlockEditAction, BlockEditRequest, ChunkChanges, players};

use crate::{ChunkCoord, M, VoxelWorld, inside_build_volume, inside_world_disc};

/// Face index → outward direction: 0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z.
pub const FACE_DIRS: [[i32; 3]; 6] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
];

/// Cells the crosshair can target: anything but air and liquids.
pub fn targetable(m: StateId) -> bool {
    m != M.air && !info(m).liquid
}

/// In the infinite creative palette (§6.5).
pub fn placeable(m: StateId) -> bool {
    usize::from(m) < dwell_blocks::STATES.len() && info(m).placeable
}

/// The cell face a surface normal points through: its dominant axis, ties to the vertical.
fn face_of_normal(n: [f32; 3]) -> u8 {
    let (ax, ay, az) = (n[0].abs(), n[1].abs(), n[2].abs());
    if ay >= ax && ay >= az {
        return if n[1] > 0.0 { 2 } else { 3 };
    }
    if ax >= az {
        return if n[0] > 0.0 { 0 } else { 1 };
    }
    if n[2] > 0.0 { 4 } else { 5 }
}

/// The shape a block is aimed at and seen by: its solid, or the whole cell (a ladder).
fn target_shape(m: StateId) -> &'static ShapeInfo {
    if info(m).shape == VoxelShape::Empty {
        shape_of(M.stone)
    } else {
        shape_of(m)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockHit {
    pub cell: [i32; 3],
    /// The face the ray entered through.
    pub face: u8,
    pub distance: f32,
}

/// The nearest targetable cell along the ray (unit `dir`) within `max_distance`, as its exact
/// shape. A cell containing the origin is skipped. Walks in float relative to the origin's cell,
/// so it is as precise ~8,000 km out as at the origin.
pub fn raycast_block(
    world: &mut VoxelWorld,
    origin: [f64; 3],
    dir: [f32; 3],
    max_distance: f32,
) -> Option<BlockHit> {
    let mut base = [0i32; 3];
    let mut cell = [0i32; 3];
    let mut step = [0i32; 3];
    let mut o = [0f32; 3];
    let mut t_max = [0f32; 3];
    let mut t_delta = [0f32; 3];
    for i in 0..3 {
        base[i] = origin[i].floor() as i32;
        o[i] = (origin[i] - f64::from(base[i])) as f32;
        if dir[i] > 0.0 {
            step[i] = 1;
            t_delta[i] = 1.0 / dir[i];
            t_max[i] = (1.0 - o[i]) / dir[i];
        } else if dir[i] < 0.0 {
            step[i] = -1;
            t_delta[i] = -1.0 / dir[i];
            t_max[i] = -o[i] / dir[i];
        } else {
            t_delta[i] = f32::INFINITY;
            t_max[i] = f32::INFINITY;
        }
    }
    let mut t_cell = 0.0f32;
    while t_cell <= max_distance {
        let m = world.get_voxel(base[0] + cell[0], base[1] + cell[1], base[2] + cell[2]);
        if targetable(m) {
            let corner = cell.map(|c| c as f32);
            if let Some(hit) =
                ray_enter_shape(target_shape(m), corner, o, dir, max_distance, None, 0.0)
                && hit.t <= max_distance
            {
                return Some(BlockHit {
                    cell: std::array::from_fn(|i| base[i] + cell[i]),
                    face: face_of_normal(hit.normal),
                    distance: hit.t,
                });
            }
        }
        let axis = if t_max[0] < t_max[1] {
            if t_max[0] < t_max[2] { 0 } else { 2 }
        } else if t_max[1] < t_max[2] {
            1
        } else {
            2
        };
        t_cell = t_max[axis];
        cell[axis] += step[axis];
        t_max[axis] += t_delta[axis];
    }
    None
}

/// A player's collision capsule (vertical): centre, radius, half the cylinder's height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditCapsule {
    pub center: [f64; 3],
    pub radius: f32,
    pub half_cylinder: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditCheck {
    Ok,
    /// Target farther than reach (+ latency slack) from the eye.
    OutOfReach,
    /// The targeted face cannot be seen from the eye.
    NoLineOfSight,
    /// Break: target is air or liquid; Place: nothing to place against.
    NothingThere,
    Unbreakable,
    /// Place: not in the palette, or an unknown id.
    NotPlaceable,
    /// Place: the cell holds a block.
    Occupied,
    /// Place: a solid block would overlap a player capsule.
    IntoPlayer,
    /// Place: outside the world's rows or disc.
    OutOfWorld,
}

/// Extra reach the server allows over `REACH_DISTANCE`: its view of the player lags the client's.
pub const REACH_SLACK: f32 = 1.0;
pub const DEFAULT_REACH: f32 = players::REACH_DISTANCE + REACH_SLACK;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditOutcome {
    pub check: EditCheck,
    /// The cell that changes (Place: next to the target).
    pub cell: [i32; 3],
    pub material: StateId,
}

impl EditOutcome {
    fn fail(check: EditCheck) -> Self {
        Self {
            check,
            cell: [0; 3],
            material: M.air,
        }
    }
}

/// Validates an edit request from a player whose eye is at `eye` against the world and every
/// player's capsule (§6.5, §11). Rate, permissions and liveness are the caller's.
pub fn check_block_edit(
    world: &mut VoxelWorld,
    request: &BlockEditRequest,
    eye: [f64; 3],
    players: &[EditCapsule],
    reach: f32,
) -> EditOutcome {
    let c = request.cell;
    // Reach first: nothing far away is read (reading would generate it).
    let mut gap2 = 0.0f64;
    for i in 0..3 {
        let lo = f64::from(c[i]);
        let hi = lo + 1.0;
        let g = if eye[i] < lo {
            lo - eye[i]
        } else if eye[i] > hi {
            eye[i] - hi
        } else {
            0.0
        };
        gap2 += g * g;
    }
    if gap2 > f64::from(reach) * f64::from(reach) {
        return EditOutcome::fail(EditCheck::OutOfReach);
    }
    let target = world.get_voxel(c[0], c[1], c[2]);
    if !targetable(target) {
        return EditOutcome::fail(EditCheck::NothingThere);
    }
    // Line of sight: the eye is in front of a surface of the target whose normal points through
    // the requested face, and a ray from it reaches the target first at the polygon's centre or
    // near one of its corners.
    let mut visible = false;
    for polygon in faces_of(target_shape(target)) {
        if visible {
            break;
        }
        let pts = polygon.v;
        let e1 = [
            pts[1][0] - pts[0][0],
            pts[1][1] - pts[0][1],
            pts[1][2] - pts[0][2],
        ];
        let e2 = [
            pts[2][0] - pts[0][0],
            pts[2][1] - pts[0][1],
            pts[2][2] - pts[0][2],
        ];
        let mut normal = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        let length_n =
            (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        if length_n <= 0.0 {
            continue;
        }
        for v in &mut normal {
            *v /= length_n;
        }
        if face_of_normal(normal) != request.face {
            continue;
        }
        let side: f64 = (0..3)
            .map(|k| (eye[k] - (f64::from(c[k]) + f64::from(pts[0][k]))) * f64::from(normal[k]))
            .sum();
        if side <= 0.0 {
            continue;
        }
        let count = usize::from(polygon.count);
        let mut centre = [0f32; 3];
        for p in &pts[..count] {
            for k in 0..3 {
                centre[k] += p[k] / count as f32;
            }
        }
        for sample in -1..count as i32 {
            if visible {
                break;
            }
            let p: [f64; 3] = std::array::from_fn(|k| {
                // The centre, then 70 % of the way from it to each corner.
                let local = if sample < 0 {
                    centre[k]
                } else {
                    centre[k] + 0.7 * (pts[sample as usize][k] - centre[k])
                };
                f64::from(c[k]) + f64::from(local) + 0.01 * f64::from(normal[k])
            });
            let delta = [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
            let length = (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
            if length <= 0.0 {
                continue;
            }
            let dir = delta.map(|d| (d / length) as f32);
            // On past the point: a grazing ray needs a while to cross the 1 cm to the face.
            if let Some(hit) = raycast_block(world, eye, dir, length as f32 + 1.0)
                && hit.cell == c
            {
                visible = true;
            }
        }
    }
    if !visible {
        return EditOutcome::fail(EditCheck::NoLineOfSight);
    }
    if request.action == BlockEditAction::Break {
        if unbreakable(target) {
            return EditOutcome::fail(EditCheck::Unbreakable);
        }
        // A flooded shape leaves the water that filled its open part.
        let material = if info(target).flooded { M.water } else { M.air };
        return EditOutcome {
            check: EditCheck::Ok,
            cell: c,
            material,
        };
    }
    if !placeable(request.material) {
        return EditOutcome::fail(EditCheck::NotPlaceable);
    }
    let n = FACE_DIRS[usize::from(request.face)];
    let cell = [c[0] + n[0], c[1] + n[1], c[2] + n[2]];
    // The build volume bounds building (ADR 0025); the disc, the terrain's columns.
    if !inside_build_volume(
        i64::from(cell[0]),
        i64::from(cell[1]),
        i64::from(cell[2]),
    ) || !inside_world_disc(i64::from(cell[0]), i64::from(cell[2]))
    {
        return EditOutcome::fail(EditCheck::OutOfWorld);
    }
    let existing = world.get_voxel(cell[0], cell[1], cell[2]);
    if targetable(existing) {
        return EditOutcome::fail(EditCheck::Occupied);
    }
    // Placing into a liquid replaces it: not one the data marks indestructible.
    if unbreakable(existing) {
        return EditOutcome::fail(EditCheck::Unbreakable);
    }
    let material = flooded_for(request.material, existing);
    if overlaps_player(material, cell, players) {
        return EditOutcome::fail(EditCheck::IntoPlayer);
    }
    EditOutcome {
        check: EditCheck::Ok,
        cell,
        material,
    }
}

/// A block no edit may change (the registry's `indestructible`): single breaks and region edits
/// both ask this.
pub fn unbreakable(m: StateId) -> bool {
    info(m).indestructible
}

/// Whether a state is flooded follows its cell: put into water it holds water, else none.
pub fn flooded_for(material: StateId, existing: StateId) -> StateId {
    if state_property(material, "flooded").is_some()
        && let Some(s) = with_property(
            material,
            "flooded",
            if info(existing).liquid {
                "true"
            } else {
                "false"
            },
        )
    {
        return s;
    }
    material
}

/// `material` is solid and would overlap one of the players' capsules at `cell`.
pub fn overlaps_player(material: StateId, cell: [i32; 3], players: &[EditCapsule]) -> bool {
    if !info(material).solid {
        return false;
    }
    let shape = shape_of(material);
    players.iter().any(|p| {
        let x = (p.center[0] - f64::from(cell[0])) as f32;
        let z = (p.center[2] - f64::from(cell[2])) as f32;
        let y = p.center[1] - f64::from(cell[1]);
        let bottom = (y - f64::from(p.half_cylinder)) as f32;
        let top = (y + f64::from(p.half_cylinder)) as f32;
        vertical_segment_distance_sq(shape, x, z, bottom, top) < p.radius * p.radius
    })
}

/// Applies one chunk's changes from a `VoxelModification` (a client's world); the chunk takes the
/// modification's revision. A missing chunk is created first.
pub fn apply_chunk_changes(world: &mut VoxelWorld, changes: &ChunkChanges) {
    let chunk = world.get_or_create(ChunkCoord::from_array(changes.coord));
    for v in &changes.changes {
        if usize::from(v.index) < crate::CHUNK_VOLUME {
            chunk.set_at(usize::from(v.index), v.material);
        }
    }
    chunk.set_revision(changes.revision);
}
