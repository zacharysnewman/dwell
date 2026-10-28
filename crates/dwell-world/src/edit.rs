//! Block interaction (ARCHITECTURE.md §6.5): the targeting ray cast shared by client (the
//! crosshair's cell) and server (line of sight), and the server's validation of a
//! `BlockEditRequest` (§11). Pure functions of the voxel world and the players' capsules.

use dwell_blocks::shape::{
    faces_of, plant_sink, ray_enter_shape, shape_of, vertical_segment_distance_sq,
};
use dwell_blocks::{
    Facing, Medium, ShapeFace, ShapeInfo, StateId, StateInfo, Support, VoxelShape, block_of, info,
    state_property, with_property,
};
use dwell_protocol::{BlockEditAction, BlockEditRequest, ChunkChanges, MAX_PIECE_ROWS, players};

use crate::{
    CELL, CELL_H, ChunkCoord, FaceFrame, M, VoxelWorld, cell_dir, cell_space, inside_build_volume,
    inside_world_disc, m_of_row, metres, normal_to_m, pos_of_cell, up_of_y,
};

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
    dwell_blocks::try_info(m).is_some_and(|s| s.placeable)
}

/// The cell face a surface normal points through: its dominant axis, ties to the vertical. A 45°
/// slope (an adaptive ramp or ridge) is a tie whatever the rounding of its normal: within 1e-5.
fn face_of_normal(n: [f32; 3]) -> u8 {
    let (ax, ay, az) = (n[0].abs(), n[1].abs(), n[2].abs());
    const TIE: f32 = 1e-5;
    if ay + TIE >= ax && ay + TIE >= az {
        return if n[1] > 0.0 { 2 } else { 3 };
    }
    if ax >= az {
        return if n[0] > 0.0 { 0 } else { 1 };
    }
    if n[2] > 0.0 { 4 } else { 5 }
}

/// The shape a block is aimed at and seen by: its solid — an adaptive block's derived shape — or
/// the whole cell (a ladder).
fn target_shape(world: &mut VoxelWorld, cell: [i32; 3]) -> &'static ShapeInfo {
    let m = world.get_voxel(cell[0], cell[1], cell[2]);
    if info(m).shape == VoxelShape::Empty {
        shape_of(M.stone)
    } else {
        world.shape_at(cell[0], cell[1], cell[2])
    }
}

/// How far the plant in `cell` is drawn below its cell's floor (toward its ground) at (fx, fz) in
/// the cell: a cross that sits on its support (`Cross::sit_on_support`) stands on the support's
/// solid there, as the mesher draws it (TRANSPARENT_BLOCKS.md §4). 0 for anything else.
pub fn plant_sink_at(world: &mut VoxelWorld, cell: [i32; 3], fx: f32, fz: f32) -> f32 {
    let m = world.get_voxel(cell[0], cell[1], cell[2]);
    if !info(m).cross.is_some_and(|c| c.sit_on_support) {
        return 0.0;
    }
    let up = up_of_y(cell[1]);
    let s = [cell[0], cell[1] - up, cell[2]];
    if info(world.get_voxel(s[0], s[1], s[2])).shape == VoxelShape::Empty {
        return 0.0;
    }
    plant_sink(world.shape_at(s[0], s[1], s[2]), fx, fz, up)
}

/// The box (min, max corner in the cell) a block is aimed at, seen by and outlined with: its data's
/// `selectionBox` (turned to stand on face B's ground, and sunk with a plant that sits on its
/// support, so it may reach into the support's cell), else its shape's extent, else the whole
/// cell.
pub fn target_box(world: &mut VoxelWorld, cell: [i32; 3]) -> ([f32; 3], [f32; 3]) {
    // Built face-local (the ground below) and turned into the cell through the face frame.
    let f = FaceFrame::at_row(cell[1]);
    let m = world.get_voxel(cell[0], cell[1], cell[2]);
    if let Some(b) = info(m).selection {
        let sink = plant_sink_at(world, cell, 0.5 * (b[0] + b[3]), 0.5 * (b[2] + b[5]));
        let (y0, y1) = f.in_cell(b[1] - sink, b[4] - sink);
        return ([b[0], y0, b[2]], [b[3], y1, b[5]]);
    }
    let shape = f.shape(target_shape(world, cell));
    let top = if shape.inverted { 1.0 } else { shape.max_y };
    let (y0, y1) = f.in_cell(0.0, top);
    ([0.0, y0, 0.0], [1.0, y1, 1.0])
}

/// The polygons of a box in its cell, outward-wound, tagged with the face each lies on.
fn box_faces(lo: [f32; 3], hi: [f32; 3]) -> [ShapeFace; 6] {
    let [x0, y0, z0] = lo;
    let [x1, y1, z1] = hi;
    let face = |tag: u8, v: [[f32; 3]; 4]| ShapeFace { tag, count: 4, v };
    [
        face(0, [[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]]),
        face(1, [[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]]),
        face(2, [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]]),
        face(3, [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]]),
        face(4, [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]]),
        face(5, [[x0, y0, z0], [x0, y1, z0], [x1, y1, z0], [x1, y0, z0]]),
    ]
}

/// The surfaces a block is seen by (line of sight): its selection box's, or its shape's.
fn target_faces(world: &mut VoxelWorld, cell: [i32; 3]) -> Vec<ShapeFace> {
    let m = world.get_voxel(cell[0], cell[1], cell[2]);
    match info(m).selection {
        Some(_) => {
            let (lo, hi) = target_box(world, cell);
            box_faces(lo, hi).to_vec()
        }
        None => faces_of(target_shape(world, cell)).to_vec(),
    }
}

/// Where the ray `o + t·dir` (cell-relative, the cell's minimum corner at `corner`) enters the box
/// [lo, hi] of the cell, at t in [0, max_t]: (t, outward normal of the face entered).
fn ray_enter_box(
    (lo, hi): ([f32; 3], [f32; 3]),
    corner: [f32; 3],
    o: [f32; 3],
    dir: [f32; 3],
    max_t: f32,
) -> Option<(f32, [f32; 3])> {
    let (mut t_in, mut t_out) = (f32::NEG_INFINITY, f32::INFINITY);
    let mut normal = [0.0; 3];
    for i in 0..3 {
        let (a, b) = (corner[i] + lo[i], corner[i] + hi[i]);
        if dir[i] == 0.0 {
            if o[i] < a || o[i] > b {
                return None;
            }
            continue;
        }
        let (t0, t1) = ((a - o[i]) / dir[i], (b - o[i]) / dir[i]);
        let (near, far) = (t0.min(t1), t0.max(t1));
        if near > t_in {
            t_in = near;
            normal = [0.0; 3];
            normal[i] = if dir[i] > 0.0 { -1.0 } else { 1.0 };
        }
        t_out = t_out.min(far);
    }
    (t_in <= t_out && t_in >= 0.0 && t_in <= max_t).then_some((t_in, normal))
}

/// Whether state `m` may be put into a cell holding `existing` (block data `medium`): an air plant
/// not into a liquid, a water plant only into one.
pub fn medium_allows(m: &StateInfo, existing: &StateInfo) -> bool {
    match m.medium {
        Medium::Any => true,
        Medium::Air => !existing.liquid,
        Medium::Water => existing.liquid,
    }
}

/// The state that stays where `m` is put: an air plant that ends up flooded (a `flooded`
/// variant) gives way to water.
pub fn medium_kept(m: StateId) -> StateId {
    let s = info(m);
    if s.medium == Medium::Air && s.flooded {
        M.water
    } else {
        m
    }
}

/// The cell a block stands on: below it toward its face's ground (−y on face A, +y on face B).
pub fn below(cell: [i32; 3]) -> [i32; 3] {
    let down = -FaceFrame::at_row(cell[1]).sign();
    [cell[0], cell[1] + down, cell[2]]
}

/// The cell over a block: above it toward its face's sky (+y on face A, −y on face B); the
/// inverse of [`below`].
pub fn above(cell: [i32; 3]) -> [i32; 3] {
    let up = FaceFrame::at_row(cell[1]).sign();
    [cell[0], cell[1] + up, cell[2]]
}

/// The four horizontal directions.
const SIDES: [[i32; 3]; 4] = [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]];

/// The cell a `needsSupport: behind` block at `cell` hangs on: opposite its facing (a ladder
/// facing north hangs on the block to its south). None for a block without a facing.
pub fn behind(cell: [i32; 3], m: StateId) -> Option<[i32; 3]> {
    let d = match info(m).facing {
        Facing::North => [0, 0, 1],
        Facing::South => [0, 0, -1],
        Facing::East => [-1, 0, 0],
        Facing::West => [1, 0, 0],
        Facing::None => return None,
    };
    Some([cell[0] + d[0], cell[1], cell[2] + d[2]])
}

/// Whether a block can hold one hung on its side (a ladder): a full, solid block.
pub fn holds_behind(m: StateId) -> bool {
    let s = info(m);
    s.solid && s.shape == VoxelShape::Full
}

/// The name of the block a state belongs to ("" for an id outside the table).
fn block_name(m: StateId) -> &'static str {
    if usize::from(m) < dwell_blocks::state_count() {
        block_of(m).name
    } else {
        ""
    }
}

/// The other cells of the multi-cell block a state at `cell` belongs to (block data `extends`,
/// `partOf`; BLOCK_REGISTRY.md, Multi-cell blocks): each with the block expected there.
fn partners(cell: [i32; 3], m: StateId) -> Vec<([i32; 3], &'static str)> {
    let s = info(m);
    let mut out = Vec::new();
    if let Some(up) = s.extends_up {
        out.push((above(cell), up));
    }
    if let Some(of) = s.part_of {
        out.push((below(cell), of));
    }
    out
}

/// Every write an edit setting `cell` to `material` makes, in order (the server sends them in one
/// `VoxelModification`; the client predicts the same): the cell; a multi-cell block's other cells
/// (a placed torch puts its upper part; breaking either part clears the other); and the blocks that
/// left unsupported (block data `needsSupport`: tall grass on a broken block), each of those with
/// its own parts. Read against `world` before any of it is applied.
pub fn edit_writes(
    world: &mut VoxelWorld,
    cell: [i32; 3],
    material: StateId,
) -> Vec<([i32; 3], StateId)> {
    edit_writes_all(world, &[(cell, material)])
}

/// [`edit_writes`] for several cells set at once (a 1 m piece's two cells; docs/ITEMS.md), each
/// with what it brings and what it leaves unsupported, read against `world` before any is applied.
pub fn edit_writes_all(
    world: &mut VoxelWorld,
    cells: &[([i32; 3], StateId)],
) -> Vec<([i32; 3], StateId)> {
    let mut out: Vec<([i32; 3], StateId)> = Vec::new();
    let mut queue: std::collections::VecDeque<([i32; 3], StateId)> =
        cells.iter().copied().collect();
    // A write never comes back to a cell (a part cleared, or a cell left unsupported, once).
    while let Some((c, m)) = queue.pop_front() {
        if out.iter().any(|(o, _)| *o == c) || out.len() >= 64 {
            continue;
        }
        let current = |world: &mut VoxelWorld, out: &[([i32; 3], StateId)], at: [i32; 3]| {
            out.iter()
                .rev()
                .find(|(o, _)| *o == at)
                .map_or_else(|| world.get_voxel(at[0], at[1], at[2]), |(_, v)| *v)
        };
        let old = current(world, &out, c);
        out.push((c, m));
        if old != m {
            // The parts of what was there go with it.
            for (p, name) in partners(c, old) {
                let there = current(world, &out, p);
                if block_name(there) == name {
                    let left = if info(there).flooded { M.water } else { M.air };
                    queue.push_back((p, left));
                }
            }
        }
        // The parts of what is put there come with it.
        for (p, name) in partners(c, m) {
            if let Some(part) = dwell_blocks::state(name)
                && current(world, &out, p) != part
            {
                queue.push_back((p, part));
            }
        }
        // A block hung on it (a ladder: `needsSupport` behind), if this leaves it nothing to
        // hang on.
        if !holds_behind(m) {
            for d in SIDES {
                let n = [c[0] + d[0], c[1], c[2] + d[2]];
                let there = current(world, &out, n);
                if info(there).needs_support == Support::Behind && behind(n, there) == Some(c) {
                    let left = if info(there).flooded { M.water } else { M.air };
                    queue.push_back((n, left));
                }
            }
        }
        // The block standing on it, if this leaves it unsupported.
        let up = above(c);
        if below(up) == c
            && inside_build_volume(i64::from(up[0]), i64::from(up[1]), i64::from(up[2]))
        {
            let there = current(world, &out, up);
            if there != M.air
                && info(there).needs_support != Support::None
                && !supported_on(there, m)
            {
                let left = if info(there).flooded { M.water } else { M.air };
                queue.push_back((up, left));
            }
        }
    }
    out
}

/// The box a block is aimed at, over every cell of its multi-cell block (a 1 m torch's two cells),
/// in fractions of `cell` (the other cells' boxes offset by their rows): the outline's box.
pub fn outline_box(world: &mut VoxelWorld, cell: [i32; 3]) -> ([f32; 3], [f32; 3]) {
    let (mut lo, mut hi) = target_box(world, cell);
    let m = world.get_voxel(cell[0], cell[1], cell[2]);
    for (p, name) in partners(cell, m) {
        if block_name(world.get_voxel(p[0], p[1], p[2])) != name {
            continue;
        }
        let (plo, phi) = target_box(world, p);
        let dy = (p[1] - cell[1]) as f32;
        for k in 0..3 {
            let off = if k == 1 { dy } else { 0.0 };
            lo[k] = lo[k].min(plo[k] + off);
            hi[k] = hi[k].max(phi[k] + off);
        }
    }
    (lo, hi)
}

/// Whether state `m` may stand on `under` (block data `placeOn`, `needsSupport`): on one of its
/// `placeOn` blocks if it lists any, else, if it needs support, on a solid block; anything else
/// stands anywhere.
pub fn supported_on(m: StateId, under: StateId) -> bool {
    let s = info(m);
    if !s.place_on.is_empty() {
        let name = if usize::from(under) < dwell_blocks::STATES.len() {
            block_of(under).name
        } else {
            ""
        };
        return s.place_on.contains(&name);
    }
    s.needs_support == Support::None || info(under).solid
}

/// Whether `m` in `cell` is held where it is: a block hung on a wall (`needsSupport: behind`, a
/// ladder) by the full block behind it, anything else by what is under it ([`supported_on`]). What
/// a region edit keeps of what it put down (ARCHITECTURE.md §6.5).
pub fn held_in_place(world: &mut VoxelWorld, cell: [i32; 3], m: StateId) -> bool {
    if info(m).needs_support == Support::Behind {
        return behind(cell, m).is_some_and(|[x, y, z]| holds_behind(world.get_voxel(x, y, z)));
    }
    let [x, y, z] = below(cell);
    supported_on(m, world.get_voxel(x, y, z))
}

/// What the world loses after `cell` becomes `material` (block data `needsSupport`): the blocks
/// above it that no longer stand on what is under them, each with what it leaves (air, or water if
/// it was flooded), in order upward — the server sends them in the same `VoxelModification`.
pub fn unsupported_after(
    world: &mut VoxelWorld,
    cell: [i32; 3],
    material: StateId,
) -> Vec<([i32; 3], StateId)> {
    let mut out = Vec::new();
    let (mut at, mut under) = (cell, material);
    loop {
        // The cell standing on `at`: the one whose `below` is `at`.
        let up = FaceFrame::at_row(at[1]).sign();
        let next = [at[0], at[1] + up, at[2]];
        if below(next) != at
            || !inside_build_volume(i64::from(next[0]), i64::from(next[1]), i64::from(next[2]))
        {
            break;
        }
        let m = world.get_voxel(next[0], next[1], next[2]);
        if m == M.air || info(m).needs_support == Support::None || supported_on(m, under) {
            break;
        }
        let left = if info(m).flooded { M.water } else { M.air };
        out.push((next, left));
        (at, under) = (next, left);
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockHit {
    pub cell: [i32; 3],
    /// The face the ray entered through.
    pub face: u8,
    pub distance: f32,
}

/// The nearest targetable cell along the ray (unit `dir`, metres) within `max_distance` (m), as its
/// exact shape. A cell containing the origin is skipped. Walks in float relative to the origin's
/// cell, so it is as precise ~8,000 km out as at the origin. The walk is in cell space (y in rows,
/// HALF_HEIGHT_GRID.md §2) with the direction scaled, not renormalised, so `t` stays in metres.
pub fn raycast_block(
    world: &mut VoxelWorld,
    origin: [f64; 3],
    dir: [f32; 3],
    max_distance: f32,
) -> Option<BlockHit> {
    raycast_block_past(world, origin, dir, max_distance, |_, _| false)
}

/// [`raycast_block`] passing through the targetable cells `see_through(cell, state)` names.
pub fn raycast_block_past(
    world: &mut VoxelWorld,
    origin: [f64; 3],
    dir: [f32; 3],
    max_distance: f32,
    see_through: impl Fn([i32; 3], StateId) -> bool,
) -> Option<BlockHit> {
    let origin = cell_space(origin);
    let dir = cell_dir(dir.map(f64::from)).map(|d| d as f32);
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
        let at: [i32; 3] = std::array::from_fn(|i| base[i] + cell[i]);
        let m = world.get_voxel(at[0], at[1], at[2]);
        if targetable(m) && !see_through(at, m) {
            let corner = cell.map(|c| c as f32);
            let hit = if info(m).selection.is_some() {
                ray_enter_box(target_box(world, at), corner, o, dir, max_distance)
            } else {
                ray_enter_shape(
                    target_shape(world, at),
                    corner,
                    o,
                    dir,
                    max_distance,
                    None,
                    0.0,
                )
                .map(|h| (h.t, h.normal))
            }
            .map(|(t, n)| (t, n, at));
            // A plant sitting on this cell reaches down into it: its box can be met here first.
            let up = up_of_y(at[1]);
            let above = [at[0], at[1] + up, at[2]];
            let plant = if plant_sink_at(world, above, 0.5, 0.5) > 0.0 {
                let c = [corner[0], corner[1] + up as f32, corner[2]];
                ray_enter_box(target_box(world, above), c, o, dir, max_distance)
                    .map(|(t, n)| (t, n, above))
            } else {
                None
            };
            let nearest = match (hit, plant) {
                (Some(a), Some(b)) => Some(if b.0 < a.0 { b } else { a }),
                (a, b) => a.or(b),
            };
            if let Some((t, normal, cell)) = nearest
                && t <= max_distance
            {
                let n = normal_to_m(normal.map(f64::from)).map(|v| v as f32);
                return Some(BlockHit {
                    cell,
                    face: face_of_normal(n),
                    distance: t,
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
    /// Place: the block below isn't one the placed block may stand on (block data `placeOn`, or
    /// `needsSupport` without a solid block below).
    NoSupport,
    /// Place: the cell's medium isn't the block's (block data `medium`: an air plant into water, a
    /// water plant into air).
    WrongMedium,
    /// Place: onto a slope's top, by a block that may not stand there (only `onSlopes` blocks, and
    /// a moldable material of the slope's own, which replaces it; docs/ITEMS.md).
    OnSlope,
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
    /// A 1 m piece's second cell (docs/ITEMS.md), set to `material` too.
    pub also: Option<[i32; 3]>,
}

impl EditOutcome {
    fn fail(check: EditCheck) -> Self {
        Self {
            check,
            cell: [0; 3],
            material: M.air,
            also: None,
        }
    }

    /// Every cell the edit sets to `material`, the placed cell first.
    pub fn cells(&self) -> impl Iterator<Item = [i32; 3]> + '_ {
        std::iter::once(self.cell).chain(self.also)
    }

    /// The writes the edit makes ([`edit_writes_all`] of its cells), `keep` applied to the
    /// material first (the server's `medium_kept`).
    pub fn writes(
        &self,
        world: &mut VoxelWorld,
        keep: impl Fn(StateId) -> StateId,
    ) -> Vec<([i32; 3], StateId)> {
        let m = keep(self.material);
        let cells: Vec<([i32; 3], StateId)> = self.cells().map(|c| (c, m)).collect();
        edit_writes_all(world, &cells)
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
    let corner = pos_of_cell(c);
    for i in 0..3 {
        let lo = corner[i];
        let hi = lo + CELL[i];
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
    // near one of its corners. Surfaces are in cell space (y in rows); which side of a plane the
    // eye is on is the same there as in metres.
    let eye_c = cell_space(eye);
    let mut visible = false;
    for polygon in &target_faces(world, c) {
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
        if face_of_normal(normal_to_m(normal.map(f64::from)).map(|v| v as f32)) != request.face {
            continue;
        }
        let side: f64 = (0..3)
            .map(|k| (eye_c[k] - (f64::from(c[k]) + f64::from(pts[0][k]))) * f64::from(normal[k]))
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
            let p: [f64; 3] = metres(std::array::from_fn(|k| {
                // The centre, then 70 % of the way from it to each corner.
                let local = if sample < 0 {
                    centre[k]
                } else {
                    centre[k] + 0.7 * (pts[sample as usize][k] - centre[k])
                };
                f64::from(c[k]) + f64::from(local) + 0.01 * f64::from(normal[k])
            }));
            let delta = [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
            let length = (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
            if length <= 0.0 {
                continue;
            }
            let dir = delta.map(|d| (d / length) as f32);
            // On past the point: a grazing ray needs a while to cross the 1 cm to the face. Plants
            // and other non-solid blocks don't hide what is behind them (the client targets
            // through the gaps beside a plant's selection box, so the server must not refuse it).
            let past = |at: [i32; 3], m: StateId| at != c && !info(m).solid;
            if let Some(hit) = raycast_block_past(world, eye, dir, length as f32 + 1.0, past)
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
        // A flooded shape leaves the water that filled its open part. A break takes the one cell,
        // whatever the held piece's height (docs/ITEMS.md §2).
        let material = if info(target).flooded { M.water } else { M.air };
        return EditOutcome {
            check: EditCheck::Ok,
            cell: c,
            material,
            also: None,
        };
    }
    if !placeable(request.material) {
        return EditOutcome::fail(EditCheck::NotPlaceable);
    }
    let placed = info(request.material);
    let rows = request.rows.clamp(1, MAX_PIECE_ROWS);
    // A multi-cell block (a torch) brings its own other cells: it is one cell's piece.
    if rows > 1 && placed.extends_up.is_some() {
        return EditOutcome::fail(EditCheck::NotPlaceable);
    }
    let t = info(target);
    // A slope (a stored ramp) is built on by plants only (`onSlopes`); a moldable material of the
    // slope's own group replaces it instead (docs/ITEMS.md).
    let molds = |there: StateId| {
        let s = info(there);
        s.shape == VoxelShape::Shaped && s.moldable.is_some() && s.moldable == placed.moldable
    };
    let face_up = request.face == if FaceFrame::at_row(c[1]).is_b() { 3 } else { 2 };
    if t.shape == VoxelShape::Shaped && !molds(target) && !placed.on_slopes && face_up {
        return EditOutcome::fail(EditCheck::OnSlope);
    }
    // A replaceable target (tall grass) or a slope being molded is placed into; anything else is
    // placed against.
    let n = FACE_DIRS[usize::from(request.face)];
    let cell = if (t.replaceable && !t.indestructible) || molds(target) {
        c
    } else {
        [c[0] + n[0], c[1] + n[1], c[2] + n[2]]
    };
    // A 1 m piece's second cell: away from a top or bottom face; up from a side face (toward the
    // face's sky), or down with `down`, so it lines up with what was aimed at.
    let also = (rows > 1).then(|| {
        let up = FaceFrame::at_row(cell[1]).sign();
        let dy = match request.face {
            2 => 1,
            3 => -1,
            _ if request.down => -up,
            _ => up,
        };
        [cell[0], cell[1] + dy, cell[2]]
    });
    let mut material = request.material;
    for (i, p) in std::iter::once(cell).chain(also).enumerate() {
        // The build volume bounds building (ADR 0025); the disc, the terrain's columns.
        if !inside_build_volume(i64::from(p[0]), i64::from(p[1]), i64::from(p[2]))
            || !inside_world_disc(i64::from(p[0]), i64::from(p[2]))
        {
            return EditOutcome::fail(EditCheck::OutOfWorld);
        }
        let existing = world.get_voxel(p[0], p[1], p[2]);
        if targetable(existing) && !info(existing).replaceable && !molds(existing) {
            return EditOutcome::fail(EditCheck::Occupied);
        }
        // Placing into a liquid replaces it: not one the data marks indestructible.
        if unbreakable(existing) {
            return EditOutcome::fail(EditCheck::Unbreakable);
        }
        if !medium_allows(placed, info(existing)) {
            return EditOutcome::fail(EditCheck::WrongMedium);
        }
        let m = flooded_for(request.material, existing);
        if i == 0 {
            material = m;
        }
        if overlaps_player(m, p, players) {
            return EditOutcome::fail(EditCheck::IntoPlayer);
        }
        // Hung on a wall (a ladder): on the block it was placed against, which must hold it.
        if placed.needs_support == Support::Behind {
            let wall = behind(p, m);
            let held = wall.is_some_and(|w| {
                let there = if w == c {
                    target
                } else {
                    world.get_voxel(w[0], w[1], w[2])
                };
                holds_behind(there)
            });
            if !held || (i == 0 && wall != Some(c)) {
                return EditOutcome::fail(EditCheck::NoSupport);
            }
        }
    }
    // It stands on what is under its lowest cell.
    let lowest = match also {
        Some(a) if below(cell) == a => a,
        _ => cell,
    };
    let under = below(lowest);
    if placed.needs_support != Support::Behind
        && !supported_on(material, world.get_voxel(under[0], under[1], under[2]))
    {
        return EditOutcome::fail(EditCheck::NoSupport);
    }
    // A multi-cell block needs its other cells free too (a 1 m torch's upper cell).
    for (p, name) in partners(cell, material) {
        let Some(part) = dwell_blocks::state(name) else {
            return EditOutcome::fail(EditCheck::NotPlaceable);
        };
        if !inside_build_volume(i64::from(p[0]), i64::from(p[1]), i64::from(p[2]))
            || !inside_world_disc(i64::from(p[0]), i64::from(p[2]))
        {
            return EditOutcome::fail(EditCheck::OutOfWorld);
        }
        let there = world.get_voxel(p[0], p[1], p[2]);
        if (targetable(there) && !info(there).replaceable) || unbreakable(there) {
            return EditOutcome::fail(EditCheck::Occupied);
        }
        if !medium_allows(info(part), info(there)) {
            return EditOutcome::fail(EditCheck::WrongMedium);
        }
        if overlaps_player(part, p, players) {
            return EditOutcome::fail(EditCheck::IntoPlayer);
        }
    }
    EditOutcome {
        check: EditCheck::Ok,
        cell,
        material,
        also,
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
        let y = p.center[1] - m_of_row(cell[1]);
        let bottom = (y - f64::from(p.half_cylinder)) as f32;
        let top = (y + f64::from(p.half_cylinder)) as f32;
        vertical_segment_distance_sq(shape, CELL_H as f32, x, z, bottom, top) < p.radius * p.radius
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
