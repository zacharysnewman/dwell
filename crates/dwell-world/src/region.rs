//! Region edits (ARCHITECTURE.md §6.5): a box of cells filled, hollowed, replaced or pasted in one
//! request. The server validates a `RegionEdit` here and gets the cells that change; the client
//! uses the same box arithmetic and the paste transform (rotate, mirror) for its tools. Pure
//! functions of the voxel world and the players' capsules, like `edit`.

use dwell_blocks::{Facing, STATES, StateId, info, with_facing};
use dwell_protocol::{RegionEdit, RegionEditOp, region_edit};

use crate::edit::{EditCapsule, REACH_SLACK, flooded_for, overlaps_player, placeable, unbreakable};
use crate::{M, VoxelWorld, WORLD_BOTTOM_Y, WORLD_MAX_Y, chunk_of, inside_world_disc};

/// How far the server lets a region edit's nearest cell be from the eye (m).
pub const DEFAULT_REGION_REACH: f32 = region_edit::REACH + REACH_SLACK;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionCheck {
    Ok,
    /// A side is 0 or longer than `MAX_SIDE`, or a paste's cells don't fill the box.
    BadSize,
    /// The box's nearest cell is farther than the region reach from the eye.
    OutOfReach,
    /// Part of the box is outside the world's rows or disc.
    OutOfWorld,
    /// Fill, Hollow, Replace: the block is neither air nor in the palette. Replace's `from`, or a
    /// pasted state, is an unknown id or an unbreakable block.
    NotPlaceable,
}

/// The cells a valid region edit changes, and the cells it leaves alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionOutcome {
    pub check: RegionCheck,
    /// (cell, new state), in the box's layer order; only cells whose state changes.
    pub changes: Vec<([i32; 3], StateId)>,
    /// Cells holding an unbreakable block, left as they are.
    pub unbreakable: u32,
    /// Solid cells that would overlap a player, left as they are.
    pub into_player: u32,
}

impl RegionOutcome {
    fn fail(check: RegionCheck) -> Self {
        Self {
            check,
            changes: Vec::new(),
            unbreakable: 0,
            into_player: 0,
        }
    }
}

/// The box's sides as numbers.
pub fn box_size(size: [u8; 3]) -> [i32; 3] {
    size.map(i32::from)
}

/// Distance² from `eye` to the nearest point of the box of `size` cells at `min`.
pub fn box_distance_sq(min: [i32; 3], size: [i32; 3], eye: [f64; 3]) -> f64 {
    (0..3)
        .map(|i| {
            let lo = f64::from(min[i]);
            let hi = lo + f64::from(size[i]);
            let g = if eye[i] < lo {
                lo - eye[i]
            } else if eye[i] > hi {
                eye[i] - hi
            } else {
                0.0
            };
            g * g
        })
        .sum()
}

/// The whole box lies in the world's rows and disc.
pub fn box_in_world(min: [i32; 3], size: [i32; 3]) -> bool {
    let lo = min.map(i64::from);
    let hi: [i64; 3] = std::array::from_fn(|i| lo[i] + i64::from(size[i]) - 1);
    // The disc is convex, so the four column corners inside means every column is.
    lo[1] >= i64::from(WORLD_BOTTOM_Y)
        && hi[1] < i64::from(WORLD_MAX_Y)
        && [(lo[0], lo[2]), (lo[0], hi[2]), (hi[0], lo[2]), (hi[0], hi[2])]
            .iter()
            .all(|&(x, z)| inside_world_disc(x, z))
}

fn known(m: StateId) -> bool {
    usize::from(m) < STATES.len()
}

/// What Fill, Hollow and Replace may put down: air, or a block of the creative palette.
fn may_put(m: StateId) -> bool {
    m == M.air || placeable(m)
}

/// Validates a region edit from a player whose eye is at `eye` (§6.5, §11) and lists the cells it
/// changes. Unbreakable cells (the check single breaks make) and solid cells overlapping a player
/// are left as they are. Rate, permissions and liveness are the caller's.
pub fn check_region_edit(
    world: &mut VoxelWorld,
    request: &RegionEdit,
    eye: [f64; 3],
    players: &[EditCapsule],
    reach: f32,
) -> RegionOutcome {
    check_region_edit_with(world, request, eye, players, reach, unbreakable)
}

/// [`check_region_edit`] with the unbreakable test given: the registry may have no unbreakable
/// block for a test to use.
fn check_region_edit_with(
    world: &mut VoxelWorld,
    request: &RegionEdit,
    eye: [f64; 3],
    players: &[EditCapsule],
    reach: f32,
    unbreakable: impl Fn(StateId) -> bool,
) -> RegionOutcome {
    let size = box_size(request.size);
    if size
        .iter()
        .any(|&s| s < 1 || s as usize > region_edit::MAX_SIDE)
        || (request.op == RegionEditOp::Paste && request.cells.len() != request.volume())
    {
        return RegionOutcome::fail(RegionCheck::BadSize);
    }
    let materials_ok = match request.op {
        RegionEditOp::Fill | RegionEditOp::Hollow => may_put(request.material),
        RegionEditOp::Replace => {
            may_put(request.material) && known(request.from) && !unbreakable(request.from)
        }
        RegionEditOp::Paste => request.cells.iter().all(|&m| known(m) && !unbreakable(m)),
    };
    if !materials_ok {
        return RegionOutcome::fail(RegionCheck::NotPlaceable);
    }
    // Reach and bounds before reading anything (reading generates chunks).
    if box_distance_sq(request.min, size, eye) > f64::from(reach) * f64::from(reach) {
        return RegionOutcome::fail(RegionCheck::OutOfReach);
    }
    if !box_in_world(request.min, size) {
        return RegionOutcome::fail(RegionCheck::OutOfWorld);
    }
    let mut out = RegionOutcome::fail(RegionCheck::Ok);
    for y in 0..size[1] {
        for z in 0..size[2] {
            for x in 0..size[0] {
                let cell = [request.min[0] + x, request.min[1] + y, request.min[2] + z];
                let existing = world.get_voxel(cell[0], cell[1], cell[2]);
                let wanted = match request.op {
                    RegionEditOp::Fill => flooded_for(request.material, existing),
                    RegionEditOp::Hollow => {
                        let shell = [x, y, z]
                            .iter()
                            .zip(&size)
                            .any(|(&c, &s)| c == 0 || c == s - 1);
                        if shell {
                            flooded_for(request.material, existing)
                        } else {
                            M.air
                        }
                    }
                    RegionEditOp::Replace if existing == request.from => {
                        flooded_for(request.material, existing)
                    }
                    RegionEditOp::Replace => continue,
                    RegionEditOp::Paste => {
                        request.cells
                            [RegionEdit::cell_index(request.size, x as usize, y as usize, z as usize)]
                    }
                };
                if wanted == existing {
                    continue;
                }
                if unbreakable(existing) {
                    out.unbreakable += 1;
                    continue;
                }
                if overlaps_player(wanted, cell, players) {
                    out.into_player += 1;
                    continue;
                }
                out.changes.push((cell, wanted));
            }
        }
    }
    out
}

/// The states of a box in layer order (a paste's `cells`); `None` if a chunk of it is not loaded
/// (a streamed world reads missing chunks as air, which would be wrong to copy or undo to).
pub fn read_box(world: &mut VoxelWorld, min: [i32; 3], size: [i32; 3]) -> Option<Vec<StateId>> {
    let hi: [i32; 3] = std::array::from_fn(|i| min[i] + size[i] - 1);
    let (a, b) = (chunk_of(min[0], min[1], min[2]), chunk_of(hi[0], hi[1], hi[2]));
    if world.is_streamed() {
        for cy in a.y..=b.y {
            for cz in a.z..=b.z {
                for cx in a.x..=b.x {
                    world.find(crate::ChunkCoord::new(cx, cy, cz))?;
                }
            }
        }
    }
    let mut out = Vec::with_capacity((size[0] * size[1] * size[2]).max(0) as usize);
    for y in 0..size[1] {
        for z in 0..size[2] {
            for x in 0..size[0] {
                out.push(world.get_voxel(min[0] + x, min[1] + y, min[2] + z));
            }
        }
    }
    Some(out)
}

/// A paste's orientation: mirrored across x first (east ↔ west), then turned `quarter_turns`
/// times clockwise seen from above (north → east → south → west; north is −Z, east +X).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PasteTransform {
    pub quarter_turns: u8,
    pub mirror_x: bool,
}

fn turn(f: Facing) -> Facing {
    match f {
        Facing::North => Facing::East,
        Facing::East => Facing::South,
        Facing::South => Facing::West,
        Facing::West => Facing::North,
        Facing::None => Facing::None,
    }
}

impl PasteTransform {
    /// The size of a box of `size` once transformed.
    pub fn size(self, size: [i32; 3]) -> [i32; 3] {
        if self.quarter_turns % 2 == 1 {
            [size[2], size[1], size[0]]
        } else {
            size
        }
    }

    /// One state turned with the box: facing blocks follow (BLOCK_REGISTRY.md `facingProperty`).
    pub fn state(self, m: StateId) -> StateId {
        let mut f = info(m).facing;
        if f == Facing::None {
            return m;
        }
        if self.mirror_x {
            f = match f {
                Facing::East => Facing::West,
                Facing::West => Facing::East,
                other => other,
            };
        }
        for _ in 0..self.quarter_turns % 4 {
            f = turn(f);
        }
        with_facing(m, f)
    }

    /// Box-local cell (x, y, z) of a box of `size` → its place in the transformed box.
    pub fn cell(self, size: [i32; 3], [x, y, z]: [i32; 3]) -> [i32; 3] {
        let (mut x, mut z, mut sx, mut sz) = (x, z, size[0], size[2]);
        if self.mirror_x {
            x = sx - 1 - x;
        }
        for _ in 0..self.quarter_turns % 4 {
            // (x, z) → (−z, x), shifted back into the box: a clockwise turn seen from above.
            (x, z) = (sz - 1 - z, x);
            (sx, sz) = (sz, sx);
        }
        [x, y, z]
    }

    /// The states of a box of `size` (layer order) transformed: the new size and states.
    pub fn apply(self, size: [i32; 3], cells: &[StateId]) -> ([i32; 3], Vec<StateId>) {
        let out_size = self.size(size);
        let mut out = vec![M.air; cells.len()];
        let mut i = 0;
        for y in 0..size[1] {
            for z in 0..size[2] {
                for x in 0..size[0] {
                    let [nx, ny, nz] = self.cell(size, [x, y, z]);
                    out[(nx + out_size[0] * (nz + out_size[2] * ny)) as usize] =
                        self.state(cells[i]);
                    i += 1;
                }
            }
        }
        (out_size, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::empty_generator;
    use dwell_blocks::{parse_state, require};

    fn world() -> VoxelWorld {
        VoxelWorld::new(empty_generator(), None)
    }

    fn fill(min: [i32; 3], size: [u8; 3], material: StateId) -> RegionEdit {
        RegionEdit {
            op: RegionEditOp::Fill,
            min,
            size,
            material,
            from: 0,
            cells: Vec::new(),
        }
    }

    const EYE: [f64; 3] = [0.5, 1.6, 0.5];

    #[test]
    fn fills_hollows_and_replaces() {
        let (stone, sand) = (require("dwell:stone"), require("dwell:sand"));
        let mut w = world();
        let out = check_region_edit(&mut w, &fill([2, 0, 2], [3, 3, 3], stone), EYE, &[], 64.0);
        assert_eq!(out.check, RegionCheck::Ok);
        assert_eq!(out.changes.len(), 27);
        for (c, m) in &out.changes {
            w.set_voxel(c[0], c[1], c[2], *m);
        }
        let hollow = RegionEdit {
            op: RegionEditOp::Hollow,
            ..fill([2, 0, 2], [3, 3, 3], stone)
        };
        let out = check_region_edit(&mut w, &hollow, EYE, &[], 64.0);
        // Only the centre changes (to air): the shell is stone already.
        assert_eq!(out.changes, vec![([3, 1, 3], M.air)]);
        w.set_voxel(3, 1, 3, M.air);
        let replace = RegionEdit {
            op: RegionEditOp::Replace,
            from: stone,
            ..fill([2, 0, 2], [3, 1, 3], sand)
        };
        let out = check_region_edit(&mut w, &replace, EYE, &[], 64.0);
        assert_eq!(out.changes.len(), 9);
        assert!(out.changes.iter().all(|(c, m)| c[1] == 0 && *m == sand));
    }

    #[test]
    fn rejects_bad_sizes_reach_world_and_blocks() {
        let stone = require("dwell:stone");
        let mut w = world();
        let check = |w: &mut VoxelWorld, r: &RegionEdit| check_region_edit(w, r, EYE, &[], 64.0).check;
        assert_eq!(check(&mut w, &fill([0, 0, 0], [33, 1, 1], stone)), RegionCheck::BadSize);
        assert_eq!(check(&mut w, &fill([0, 0, 0], [0, 1, 1], stone)), RegionCheck::BadSize);
        let short = RegionEdit {
            op: RegionEditOp::Paste,
            cells: vec![stone; 3],
            ..fill([0, 0, 0], [2, 2, 1], 0)
        };
        assert_eq!(check(&mut w, &short), RegionCheck::BadSize);
        // 64 m of reach to the nearest cell's face (the eye is at x = 0.5).
        assert_eq!(check(&mut w, &fill([64, 1, 0], [1, 1, 1], stone)), RegionCheck::Ok);
        assert_eq!(check(&mut w, &fill([65, 1, 0], [1, 1, 1], stone)), RegionCheck::OutOfReach);
        let far_eye = [8_191_990.0, 0.0, 0.0];
        assert_eq!(
            check_region_edit(&mut w, &fill([8_191_998, 0, 0], [4, 1, 1], stone), far_eye, &[], 64.0)
                .check,
            RegionCheck::OutOfWorld
        );
        let top = [0.0, f64::from(WORLD_MAX_Y), 0.0];
        assert_eq!(
            check_region_edit(&mut w, &fill([0, WORLD_MAX_Y - 1, 0], [1, 2, 1], stone), top, &[], 64.0)
                .check,
            RegionCheck::OutOfWorld
        );
        let water = require("dwell:water");
        assert_eq!(check(&mut w, &fill([0, 0, 0], [1, 1, 1], water)), RegionCheck::NotPlaceable);
        assert_eq!(check(&mut w, &fill([0, 0, 0], [1, 1, 1], u16::MAX)), RegionCheck::NotPlaceable);
        let bad_paste = RegionEdit {
            op: RegionEditOp::Paste,
            cells: vec![u16::MAX],
            ..fill([0, 0, 0], [1, 1, 1], 0)
        };
        assert_eq!(check(&mut w, &bad_paste), RegionCheck::NotPlaceable);
        // A paste may put down any known breakable state (an undo restores water too).
        let water_paste = RegionEdit {
            cells: vec![water],
            ..bad_paste
        };
        assert_eq!(check(&mut w, &water_paste), RegionCheck::Ok);
    }

    #[test]
    fn never_changes_or_puts_down_an_unbreakable_block() {
        let (stone, sand) = (require("dwell:stone"), require("dwell:sand"));
        // Stone stands in for an unbreakable block (the registry's `indestructible`).
        let locked = |m: StateId| m == stone;
        let mut w = world();
        w.set_voxel(1, 0, 0, stone);
        w.set_voxel(2, 0, 0, sand);
        let check =
            |w: &mut VoxelWorld, r: &RegionEdit| check_region_edit_with(w, r, EYE, &[], 64.0, locked);
        for op in [RegionEditOp::Fill, RegionEditOp::Hollow] {
            let r = RegionEdit {
                op,
                ..fill([0, 0, 0], [3, 1, 1], require("dwell:dirt"))
            };
            let out = check(&mut w, &r);
            assert_eq!((out.check, out.unbreakable), (RegionCheck::Ok, 1), "{op:?}");
            assert!(!out.changes.iter().any(|(c, _)| *c == [1, 0, 0]), "{op:?}");
        }
        let clear = check(&mut w, &fill([0, 0, 0], [3, 1, 1], M.air));
        assert_eq!(clear.changes, vec![([2, 0, 0], M.air)]);
        let paste = RegionEdit {
            op: RegionEditOp::Paste,
            cells: vec![sand, sand, M.air],
            ..fill([0, 0, 0], [3, 1, 1], 0)
        };
        let out = check(&mut w, &paste);
        assert_eq!(out.changes, vec![([0, 0, 0], sand), ([2, 0, 0], M.air)]);
        assert_eq!(out.unbreakable, 1);
        // Nor may a paste put one down, or a replace select one.
        let put = RegionEdit {
            cells: vec![stone, sand, sand],
            ..paste
        };
        assert_eq!(check(&mut w, &put).check, RegionCheck::NotPlaceable);
        let replace = RegionEdit {
            op: RegionEditOp::Replace,
            from: stone,
            ..fill([0, 0, 0], [3, 1, 1], sand)
        };
        assert_eq!(check(&mut w, &replace).check, RegionCheck::NotPlaceable);
        // The real test is the one single breaks use: the registry's flag.
        for (i, s) in STATES.iter().enumerate() {
            assert_eq!(unbreakable(i as StateId), s.indestructible);
        }
    }

    #[test]
    fn leaves_cells_inside_a_player_alone() {
        let stone = require("dwell:stone");
        let mut w = world();
        let player = EditCapsule {
            center: [4.5, 1.0, 4.5],
            radius: 0.3,
            half_cylinder: 0.6,
        };
        let out = check_region_edit(&mut w, &fill([3, 0, 3], [3, 3, 3], stone), EYE, &[player], 64.0);
        assert_eq!(out.check, RegionCheck::Ok);
        assert!(out.into_player >= 2);
        assert_eq!(out.changes.len() as u32 + out.into_player, 27);
        assert!(!out.changes.iter().any(|(c, _)| *c == [4, 1, 4]));
    }

    #[test]
    fn floods_ladders_put_into_water() {
        let mut w = world();
        w.set_voxel(1, 0, 0, require("dwell:water"));
        let ladder = parse_state("dwell:ladder[facing=east]").expect("ladder");
        let out = check_region_edit(&mut w, &fill([0, 0, 0], [2, 1, 1], ladder), EYE, &[], 64.0);
        assert_eq!(info(out.changes[0].1).name, "dwell:ladder[facing=east,flooded=false]");
        assert_eq!(info(out.changes[1].1).name, "dwell:ladder[facing=east,flooded=true]");
    }

    #[test]
    fn reads_a_box_only_where_a_streamed_world_has_chunks() {
        let mut w = VoxelWorld::streamed();
        assert_eq!(read_box(&mut w, [0, 0, 0], [2, 1, 1]), None);
        w.set_voxel(1, 0, 0, require("dwell:sand"));
        assert_eq!(
            read_box(&mut w, [0, 0, 0], [2, 1, 1]),
            Some(vec![M.air, require("dwell:sand")])
        );
        assert_eq!(read_box(&mut w, [30, 0, 0], [4, 1, 1]), None); // crosses into a missing chunk
    }

    #[test]
    fn rotates_clockwise_from_above_and_mirrors_across_x() {
        // A 3 × 1 × 2 box (x, y, z) of six different blocks without a facing, named 1..=6 by
        // position: (0,0) (1,0) (2,0) (0,1) (1,1) (2,1).
        let plain: Vec<StateId> = (0..STATES.len() as StateId)
            .filter(|&m| info(m).facing == Facing::None)
            .take(6)
            .collect();
        assert_eq!(plain.len(), 6);
        let size = [3, 1, 2];
        let named = |ns: &[usize]| ns.iter().map(|&n| plain[n - 1]).collect::<Vec<StateId>>();
        let cells = named(&[1, 2, 3, 4, 5, 6]);
        let id = PasteTransform::default();
        assert_eq!(id.apply(size, &cells), (size, cells.clone()));
        let turn = PasteTransform {
            quarter_turns: 1,
            mirror_x: false,
        };
        // Clockwise from above (+Y): +X goes to +Z, +Z to −X. Old (x, z) → new (1 − z, x).
        let (s, c) = turn.apply(size, &cells);
        assert_eq!(s, [2, 1, 3]);
        assert_eq!(c, named(&[4, 1, 5, 2, 6, 3]));
        // Four turns are none; two are a half turn.
        let four = PasteTransform {
            quarter_turns: 4,
            mirror_x: false,
        };
        assert_eq!(four.apply(size, &cells), (size, cells.clone()));
        let half = PasteTransform {
            quarter_turns: 2,
            mirror_x: false,
        };
        assert_eq!(half.apply(size, &cells).1, named(&[6, 5, 4, 3, 2, 1]));
        let mirror = PasteTransform {
            quarter_turns: 0,
            mirror_x: true,
        };
        assert_eq!(mirror.apply(size, &cells).1, named(&[3, 2, 1, 6, 5, 4]));
        // Mirroring twice is the identity.
        let (s1, c1) = mirror.apply(size, &cells);
        assert_eq!(mirror.apply(s1, &c1), (size, cells));
    }

    #[test]
    fn facing_blocks_turn_with_the_box() {
        let l = |f: &str| parse_state(&format!("dwell:ladder[facing={f},flooded=true]")).expect("ladder");
        let turn = PasteTransform {
            quarter_turns: 1,
            mirror_x: false,
        };
        assert_eq!(turn.state(l("north")), l("east"));
        assert_eq!(turn.state(l("west")), l("north"));
        let mirror = PasteTransform {
            quarter_turns: 0,
            mirror_x: true,
        };
        assert_eq!(mirror.state(l("east")), l("west"));
        assert_eq!(mirror.state(l("north")), l("north"));
        let stone = require("dwell:stone");
        assert_eq!(turn.state(stone), stone);
    }
}
