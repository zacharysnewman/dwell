//! Block interaction (ARCHITECTURE.md §6.5): what the crosshair targets, the infinite creative
//! palette and its selection, and turning break/place actions into `BlockEditRequest`s. The client
//! does not predict edits: the change shows when the server's VoxelModification arrives. Targeting
//! goes through [`TargetSource`], which the client's voxel world implements with
//! `dwell_world::edit::raycast_block`.

use dwell_blocks::{Facing, STATES, StateId, info};
use dwell_protocol::{BlockEditAction, BlockEditRequest, Message, players};
use dwell_world::edit::{FACE_DIRS, placeable, raycast_block};
use dwell_world::{FaceFrame, VoxelWorld, inside_build_volume};

pub use crate::input::EditAction;

/// A targeted block: its cell and the face the view ray entered through (0 +X, 1 −X, 2 +Y, 3 −Y,
/// 4 +Z, 5 −Z).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockTarget {
    pub cell: [i32; 3],
    pub face: u8,
    /// Where the aim meets the cell, face-locally up its height (0 its bottom, 1 its top): a 1 m
    /// piece placed against a side goes down from the placed cell when it is under ½, so the piece
    /// is centred on the aim (docs/ITEMS.md).
    pub aim_up: f32,
}

/// What the interaction needs from the client sim: the block along a view ray (unit `dir`).
pub trait TargetSource {
    fn target(&mut self, origin: [f64; 3], dir: [f32; 3], max_distance: f32)
    -> Option<BlockTarget>;
    /// Whether the block in `cell` is replaced by what is placed against it (block data
    /// `replaceable`: tall grass), so placing goes into its cell.
    fn replaceable(&mut self, _cell: [i32; 3]) -> bool {
        false
    }
    /// The block state in `cell` (0, air, if unknown): a ladder placed on a ladder's top or bottom
    /// takes its facing.
    fn state(&mut self, _cell: [i32; 3]) -> StateId {
        0
    }
}

impl TargetSource for VoxelWorld {
    fn target(
        &mut self,
        origin: [f64; 3],
        dir: [f32; 3],
        max_distance: f32,
    ) -> Option<BlockTarget> {
        raycast_block(self, origin, dir, max_distance).map(|h| {
            let y = origin[1] + f64::from(dir[1]) * f64::from(h.distance);
            let rows = y / dwell_world::CELL_H - f64::from(h.cell[1]);
            let up = if FaceFrame::at_row(h.cell[1]).is_b() {
                1.0 - rows
            } else {
                rows
            };
            BlockTarget {
                cell: h.cell,
                face: h.face,
                aim_up: up.clamp(0.0, 1.0) as f32,
            }
        })
    }
    fn replaceable(&mut self, [x, y, z]: [i32; 3]) -> bool {
        let m = self.get_voxel(x, y, z);
        usize::from(m) < STATES.len() && info(m).replaceable && !info(m).indestructible
    }
    fn state(&mut self, [x, y, z]: [i32; 3]) -> StateId {
        self.get_voxel(x, y, z)
    }
}

/// A hotbar slot: an item (docs/ITEMS.md) — its block at a height; a ladder's facing follows the
/// placement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteSlot {
    /// What the hotbar calls it (the item's label), e.g. `Stone, ½ m`.
    pub name: String,
    /// The item, e.g. `dwell:stone_half`.
    pub item: &'static str,
    /// Namespaced block id, e.g. `dwell:stone`.
    pub block: &'static str,
    /// State shown and placed (for ladders: the first placeable one).
    pub material: StateId,
    pub ladder: bool,
    /// Its height in ½ m cells (2: a 1 m block).
    pub rows: u8,
}

const LOOK_LADDER: &str = "ladder";

/// The palette (§6.5): the items' slots (`shared/items/items.json`), each its block's first
/// placeable state; an item whose block this registry can't place is left out.
pub fn palette() -> Vec<PaletteSlot> {
    crate::items::ITEMS
        .iter()
        .filter_map(|it| {
            let block = dwell_blocks::find_block(it.block)?;
            let material = (block.first_state..block.first_state + block.state_count)
                .find(|&id| placeable(id))?;
            Some(PaletteSlot {
                name: it.label.to_owned(),
                item: it.name,
                block: it.block,
                material,
                ladder: info(material).look == LOOK_LADDER,
                rows: it.rows,
            })
        })
        .collect()
}

/// The face a ladder's plate faces out of (0 +X, 1 −X, 4 +Z, 5 −Z): north is −Z, east +X.
fn ladder_face(facing: Facing) -> Option<u8> {
    match facing {
        Facing::East => Some(0),
        Facing::West => Some(1),
        Facing::South => Some(4),
        Facing::North => Some(5),
        Facing::None => None,
    }
}

/// The ladder to place against `face` of the targeted block: mounted on that block, facing out of
/// the face. On a top or bottom face it faces the player (`yaw_deg`: 0 = +Z, 90 = +X). 0 (air) if
/// the registry has no such ladder.
pub fn ladder_for(face: u8, yaw_deg: f64) -> StateId {
    let mut facing = face;
    if face == 2 || face == 3 {
        let yaw = yaw_deg.to_radians();
        let bx = -yaw.sin(); // back towards the player
        let bz = -yaw.cos();
        facing = if bx.abs() > bz.abs() {
            if bx > 0.0 { 0 } else { 1 }
        } else if bz > 0.0 {
            4
        } else {
            5
        };
    }
    (0..STATES.len())
        .filter_map(|i| StateId::try_from(i).ok())
        .find(|&id| {
            let s = info(id);
            s.look == LOOK_LADDER && s.placeable && ladder_face(s.facing) == Some(facing)
        })
        .unwrap_or(0)
}

/// Unit view direction for yaw/pitch in degrees (yaw 0 = +Z, pitch toward the player's up > 0), as
/// the camera looks: on face B up is −y ([`FaceFrame::look`]).
pub fn view_direction(yaw_deg: f64, pitch_deg: f64, face: FaceFrame) -> [f64; 3] {
    face.look(yaw_deg, pitch_deg)
}

/// Targeting, the hotbar selection and edit requests.
#[derive(Clone, Debug)]
pub struct BlockInteraction {
    palette: Vec<PaletteSlot>,
    /// Index into the palette.
    selected: usize,
    /// Touch: what a tap on the view does.
    pub touch_action: EditAction,
    current: Option<BlockTarget>,
    /// The current target is replaceable: placing goes into its cell.
    into_target: bool,
    /// The current target's state.
    target_state: StateId,
    yaw: f64,
    last_edit_ms: f64,
}

impl Default for BlockInteraction {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockInteraction {
    pub fn new() -> Self {
        Self {
            palette: palette(),
            selected: 0,
            touch_action: EditAction::Break,
            current: None,
            into_target: false,
            target_state: 0,
            yaw: 0.0,
            last_edit_ms: f64::NEG_INFINITY,
        }
    }

    pub fn palette(&self) -> &[PaletteSlot] {
        &self.palette
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn target(&self) -> Option<BlockTarget> {
        self.current
    }

    /// Re-targets from the eye along the view; no eye clears the target (dead, loading).
    pub fn update(
        &mut self,
        world: &mut impl TargetSource,
        eye: Option<[f64; 3]>,
        yaw_deg: f64,
        pitch_deg: f64,
        face: FaceFrame,
    ) -> Option<BlockTarget> {
        self.yaw = yaw_deg;
        let dir = view_direction(yaw_deg, pitch_deg, face).map(|v| v as f32);
        self.current = eye.and_then(|e| world.target(e, dir, players::REACH_DISTANCE));
        self.into_target = self.current.is_some_and(|t| world.replaceable(t.cell));
        self.target_state = self.current.map_or(0, |t| world.state(t.cell));
        self.current
    }

    /// The state the selected slot places against the current target, and where; None with no target.
    pub fn placement(&self) -> Option<([i32; 3], StateId)> {
        let t = self.current?;
        let slot = self.palette.get(self.selected)?;
        let dir = FACE_DIRS
            .get(usize::from(t.face))
            .copied()
            .unwrap_or([0; 3]);
        // As the server places (dwell_world::edit::check_block_edit): into a replaceable target.
        let cell = if self.into_target {
            t.cell
        } else {
            std::array::from_fn(|i| t.cell[i] + dir[i])
        };
        if slot.ladder {
            // On a ladder's top or bottom: the next ladder up or down the same wall.
            let target = dwell_blocks::try_info(self.target_state);
            let face = match target {
                Some(s) if s.look == LOOK_LADDER && (t.face == 2 || t.face == 3) => {
                    ladder_face(s.facing).unwrap_or(t.face)
                }
                _ => t.face,
            };
            return Some((cell, ladder_for(face, self.yaw)));
        }
        Some((cell, slot.material))
    }

    /// Selects a slot (wrapping around); returns the new selection, for the hotbar to redraw.
    pub fn select(&mut self, slot: i64) -> usize {
        let n = self.palette.len() as i64;
        if n > 0 {
            self.selected = slot.rem_euclid(n) as usize;
        }
        self.selected
    }

    /// Scroll wheel: next (delta > 0) or previous slot; the new selection, if it was asked to move.
    pub fn scroll(&mut self, delta: i32) -> Option<usize> {
        (delta != 0).then(|| self.select(self.selected as i64 + i64::from(delta.signum())))
    }

    /// Breaks the targeted block or places the selected one against the targeted face: the request
    /// to send, or None when there is no target, the block would be placed outside the build volume
    /// or it is too soon after the last edit (`BLOCK_EDIT_INTERVAL_MS`).
    pub fn act(&mut self, action: EditAction, now_ms: f64) -> Option<Message> {
        let t = self.current?;
        if now_ms - self.last_edit_ms < players::BLOCK_EDIT_INTERVAL_MS as f64 {
            return None;
        }
        let (action, material) = match action {
            EditAction::Break => (BlockEditAction::Break, 0),
            EditAction::Place => {
                let placement = self.placement();
                // The server refuses a block outside the build volume (ADR 0025): not sent.
                if placement.is_some_and(|(c, _)| {
                    !inside_build_volume(i64::from(c[0]), i64::from(c[1]), i64::from(c[2]))
                }) {
                    return None;
                }
                (BlockEditAction::Place, placement.map_or(0, |(_, m)| m))
            }
        };
        self.last_edit_ms = now_ms;
        // A place fills as much as the held item is tall; a break takes one cell (docs/ITEMS.md).
        let rows = match action {
            BlockEditAction::Break => 1,
            _ => self.palette.get(self.selected).map_or(1, |s| s.rows),
        };
        Some(Message::BlockEditRequest(BlockEditRequest {
            action,
            cell: t.cell,
            face: t.face,
            material,
            rows,
            // Against a side, centred on the aim: down when it is in the cell's lower half.
            down: rows > 1 && t.face != 2 && t.face != 3 && t.aim_up < 0.5,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_blocks::require;

    fn name(id: StateId) -> &'static str {
        info(id).name
    }

    fn round(v: [f64; 3]) -> [f64; 3] {
        v.map(|x| (x * 1000.0).round() / 1000.0 + 0.0)
    }

    #[test]
    fn offers_the_items_in_order_1_m_by_default_and_half_pieces_as_their_own_slots() {
        // docs/ITEMS.md: the hotbar is the items' data (shared/items/items.json).
        let slots = palette();
        let names: Vec<_> = slots.iter().map(|s| s.name.as_str()).collect();
        let want: Vec<_> = crate::items::ITEMS.iter().map(|i| i.label).collect();
        assert_eq!(names, want);
        let stone = slots.iter().find(|s| s.item == "dwell:stone").unwrap();
        let half = slots.iter().find(|s| s.item == "dwell:stone_half").unwrap();
        assert_eq!(
            (stone.rows, half.rows, stone.material),
            (2, 1, half.material)
        );
        assert!(slots.iter().any(|s| s.ladder));
    }

    #[test]
    fn mounts_ladders_on_the_clicked_face_facing_out_of_it_or_facing_the_player_on_tops() {
        assert_eq!(
            name(ladder_for(0, 0.0)),
            "dwell:ladder[facing=east,flooded=false]"
        );
        assert_eq!(
            name(ladder_for(1, 0.0)),
            "dwell:ladder[facing=west,flooded=false]"
        );
        assert_eq!(
            name(ladder_for(4, 0.0)),
            "dwell:ladder[facing=south,flooded=false]"
        );
        assert_eq!(
            name(ladder_for(5, 0.0)),
            "dwell:ladder[facing=north,flooded=false]"
        );
        // Looking +Z: the ladder faces −Z.
        assert_eq!(
            name(ladder_for(2, 0.0)),
            "dwell:ladder[facing=north,flooded=false]"
        );
        // Looking +X.
        assert_eq!(
            name(ladder_for(2, 90.0)),
            "dwell:ladder[facing=west,flooded=false]"
        );
        assert_eq!(
            name(ladder_for(3, 180.0)),
            "dwell:ladder[facing=south,flooded=false]"
        );
    }

    /// Always targets the −Z face of (3, 0, 4), recording the rays.
    #[derive(Default)]
    struct FakeTarget {
        rays: Vec<([f32; 3], f32)>,
    }
    impl TargetSource for FakeTarget {
        fn target(&mut self, _: [f64; 3], dir: [f32; 3], max: f32) -> Option<BlockTarget> {
            self.rays.push((dir, max));
            Some(BlockTarget {
                cell: [3, 0, 4],
                face: 5,
                aim_up: 0.5,
            })
        }
    }

    fn request(m: &Message) -> &BlockEditRequest {
        match m {
            Message::BlockEditRequest(r) => r,
            other => panic!("not a BlockEditRequest: {other:?}"),
        }
    }

    #[test]
    fn targets_along_the_view_within_reach() {
        let mut world = FakeTarget::default();
        let mut interaction = BlockInteraction::new();
        interaction.update(&mut world, Some([0.0, 1.6, 0.0]), 90.0, 0.0, FaceFrame::A);
        assert_eq!(round(world.rays[0].0.map(f64::from)), [1.0, 0.0, 0.0]);
        assert_eq!(world.rays[0].1, players::REACH_DISTANCE);
        assert_eq!(
            round(view_direction(0.0, 90.0, FaceFrame::A)),
            [0.0, 1.0, 0.0]
        );
        assert_eq!(
            interaction.update(&mut world, None, 0.0, 0.0, FaceFrame::A),
            None
        );
    }

    #[test]
    fn sends_break_and_place_requests_for_the_target_at_most_one_per_interval() {
        let mut world = FakeTarget::default();
        let mut interaction = BlockInteraction::new();
        let interval = players::BLOCK_EDIT_INTERVAL_MS as f64;
        let mut sent = Vec::new();
        assert!(interaction.act(EditAction::Break, 0.0).is_none()); // nothing targeted yet
        interaction.update(&mut world, Some([0.0, 1.6, 0.0]), 0.0, 0.0, FaceFrame::A);
        sent.extend(interaction.act(EditAction::Break, 1000.0));
        assert_eq!(sent.len(), 1);
        assert!(
            interaction
                .act(EditAction::Place, 1000.0 + interval - 1.0)
                .is_none()
        );
        interaction.select(1);
        sent.extend(interaction.act(EditAction::Place, 1000.0 + interval));
        assert_eq!(
            sent,
            vec![
                Message::BlockEditRequest(BlockEditRequest {
                    action: BlockEditAction::Break,
                    cell: [3, 0, 4],
                    face: 5,
                    material: 0,
                    rows: 1,
                    down: false,
                }),
                Message::BlockEditRequest(BlockEditRequest {
                    action: BlockEditAction::Place,
                    cell: [3, 0, 4],
                    face: 5,
                    material: require("dwell:dirt"),
                    rows: 2,
                    down: false,
                }),
            ]
        );
    }

    #[test]
    fn places_the_ladder_that_fits_the_targeted_face() {
        let mut world = FakeTarget::default();
        let mut interaction = BlockInteraction::new();
        interaction.update(&mut world, Some([0.0, 1.6, 0.0]), 0.0, 0.0, FaceFrame::A);
        let ladder = interaction.palette().iter().position(|s| s.ladder).unwrap();
        interaction.select(ladder as i64);
        let m = interaction.act(EditAction::Place, 0.0).unwrap();
        assert_eq!(
            name(request(&m).material),
            "dwell:ladder[facing=north,flooded=false]"
        );
    }

    dwell_world::both_faces!(
        /// Placing a ladder above or below one (owner, 2026-10-21): aimed at the wall past the
        /// ladder's end, the ray passes the ladder's cell in front of its plate and builds on the
        /// wall; aimed at the plate's top or bottom, the new ladder takes the same facing.
        fn places_ladders_above_and_below_a_ladder(f: FaceFrame) {
            use dwell_world::{VoxelWorld, empty_generator, m_of_row};
            let north = require("dwell:ladder[facing=north]");
            let mut world = VoxelWorld::new(empty_generator(), None);
            for row in 26..36 {
                let c = f.cell([0, row, 2]);
                world.set_voxel(c[0], c[1], c[2], require("dwell:stone"));
            }
            let at = f.cell([0, 30, 1]);
            world.set_voxel(at[0], at[1], at[2], north);
            let mut interaction = BlockInteraction::new();
            let ladder = interaction.palette().iter().position(|s| s.ladder).unwrap();
            interaction.select(ladder as i64);
            // From the eye (face-local) toward the point (face-local): yaw and pitch.
            let mut aim = |eye: [f64; 3], to: [f64; 3], yaw: Option<f64>| {
                let d: [f64; 3] = std::array::from_fn(|k| to[k] - eye[k]);
                let flat = d[0].hypot(d[2]);
                let pitch = d[1].atan2(flat).to_degrees();
                let yaw = yaw.unwrap_or_else(|| d[0].atan2(d[2]).to_degrees());
                interaction.update(&mut world, Some(f.point(eye)), yaw, pitch, f);
                interaction.placement().map(|(c, m)| (c, name(m)))
            };
            let want = |row: i32| Some((f.cell([0, row, 1]), name(north)));
            let base = m_of_row(30);
            // The wall just below the ladder, from above in front of it.
            let below = aim([0.5, base + 0.45, -1.0], [0.5, base - 0.1, 2.0], None);
            assert_eq!(below, want(29), "below, by the wall");
            // The wall just above it, from below in front of it.
            let above = aim([0.5, base + 0.05, -1.0], [0.5, base + 0.6, 2.0], None);
            assert_eq!(above, want(31), "above, by the wall");
            // The plate's top from straight above, looking along +X: still facing north.
            let top = aim([0.5, base + 2.0, 1.94], [0.5, base, 1.94], Some(90.0));
            assert_eq!(top, want(31), "above, by its top");
            let bottom = aim([0.5, base - 1.5, 1.94], [0.5, base, 1.94], Some(90.0));
            assert_eq!(bottom, want(29), "below, by its bottom");
        }
    );

    #[test]
    fn selects_slots_by_number_and_scroll_wrapping_around() {
        let mut interaction = BlockInteraction::new();
        let n = interaction.palette().len();
        let mut seen = Vec::new();
        seen.extend(interaction.scroll(-1));
        assert_eq!(interaction.selected(), n - 1);
        seen.extend(interaction.scroll(1));
        seen.push(interaction.select(4));
        assert_eq!(seen, vec![n - 1, 0, 4]);
    }

    #[test]
    fn places_the_selected_block_against_the_targeted_face() {
        let mut world = FakeTarget::default();
        let mut interaction = BlockInteraction::new();
        let sand = interaction
            .palette()
            .iter()
            .position(|s| s.item == "dwell:sand")
            .unwrap();
        interaction.select(sand as i64);
        // Looking +Z at the −Z face (5) of cell (3, 0, 4).
        interaction.update(&mut world, Some([3.5, 1.6, 0.0]), 0.0, 0.0, FaceFrame::A);
        let (cell, material) = interaction.placement().unwrap();
        assert_eq!(cell, [3, 0, 3]);
        assert_eq!(name(material), "dwell:sand[flooded=false]");
        let m = interaction.act(EditAction::Place, 0.0).unwrap();
        assert_eq!(request(&m).material, material);
    }

    #[test]
    fn does_not_send_a_place_outside_the_build_volume_and_keeps_the_interval_for_one_inside() {
        // The build sphere's top: a block on the top face of (0, 49,999, 0) would be at y = 50,000,
        // outside (ADR 0025); breaking that block, inside, is sent.
        let top = dwell_world::BUILD_VOLUME.y_range().1 as i32;
        let mut world = VoxelWorld::streamed();
        world.set_voxel(0, top - 1, 0, require("dwell:stone"));
        let mut interaction = BlockInteraction::new();
        let eye = [0.5, dwell_world::m_of_row(top) + 1.5, 0.5];
        let t = interaction.update(&mut world, Some(eye), 0.0, -90.0, FaceFrame::A);
        assert_eq!(t.map(|t| (t.cell, t.face)), Some(([0, top - 1, 0], 2)));
        assert_eq!(interaction.placement().unwrap().0, [0, top, 0]);
        assert_eq!(interaction.act(EditAction::Place, 0.0), None);
        // The refused place did not use up the interval.
        let m = interaction.act(EditAction::Break, 1.0).unwrap();
        assert_eq!(request(&m).cell, [0, top - 1, 0]);
    }

    // Beyond the TS suite: the voxel world is a target source through the shared ray cast.
    #[test]
    fn the_voxel_world_targets_through_the_shared_ray_cast() {
        let mut world = VoxelWorld::streamed();
        // A non-adaptive cube (a lone stone block would be a half pyramid).
        world.set_voxel(3, 0, 4, require("dwell:sandbox_floor"));
        let mut interaction = BlockInteraction::new();
        // The eye in the middle of row 0 (HALF_HEIGHT_GRID.md §2).
        let eye = [3.5, 0.5 * dwell_world::CELL_H, 0.5];
        let t = interaction.update(&mut world, Some(eye), 0.0, 0.0, FaceFrame::A);
        assert_eq!(
            t,
            Some(BlockTarget {
                cell: [3, 0, 4],
                face: 5,
                aim_up: 0.5,
            })
        );
    }
}
