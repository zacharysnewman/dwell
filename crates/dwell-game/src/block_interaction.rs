//! Block interaction (ARCHITECTURE.md §6.5): what the crosshair targets, the infinite creative
//! palette and its selection, and turning break/place actions into `BlockEditRequest`s. The client
//! does not predict edits: the change shows when the server's VoxelModification arrives. Targeting
//! goes through [`TargetSource`], which the client's voxel world implements with
//! `dwell_world::edit::raycast_block`.

use dwell_blocks::{BLOCKS, Facing, STATES, StateId, info};
use dwell_protocol::{BlockEditAction, BlockEditRequest, Message, players};
use dwell_world::VoxelWorld;
use dwell_world::edit::{FACE_DIRS, placeable, raycast_block};

use crate::FaceSign;
pub use crate::input::EditAction;

/// A targeted block: its cell and the face the view ray entered through (0 +X, 1 −X, 2 +Y, 3 −Y,
/// 4 +Z, 5 −Z).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockTarget {
    pub cell: [i32; 3],
    pub face: u8,
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
}

impl TargetSource for VoxelWorld {
    fn target(
        &mut self,
        origin: [f64; 3],
        dir: [f32; 3],
        max_distance: f32,
    ) -> Option<BlockTarget> {
        raycast_block(self, origin, dir, max_distance).map(|h| BlockTarget {
            cell: h.cell,
            face: h.face,
        })
    }
    fn replaceable(&mut self, [x, y, z]: [i32; 3]) -> bool {
        let m = self.get_voxel(x, y, z);
        usize::from(m) < STATES.len() && info(m).replaceable && !info(m).indestructible
    }
}

/// A hotbar slot: one placeable block, or every ladder (the facing follows the placement).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteSlot {
    /// The block id without its namespace, e.g. `stone`.
    pub name: String,
    /// Namespaced block id, e.g. `dwell:stone`.
    pub block: &'static str,
    /// State shown and placed (for ladders: the first placeable one).
    pub material: StateId,
    pub ladder: bool,
}

const LOOK_LADDER: &str = "ladder";
const LOOK_SHAPED: &str = "shaped";

/// The palette (§6.5): one slot per placeable block (its first placeable state), ladders as one,
/// from the block registry's data.
pub fn palette() -> Vec<PaletteSlot> {
    let mut out = Vec::new();
    let mut previous_block = None;
    for (id, s) in STATES.iter().enumerate() {
        let Ok(id) = StateId::try_from(id) else { break };
        if !placeable(id) {
            continue;
        }
        let same_block = previous_block == Some(s.block);
        previous_block = Some(s.block);
        if s.look == LOOK_SHAPED || same_block {
            continue;
        }
        let block = BLOCKS[s.block].name;
        out.push(PaletteSlot {
            name: block[block.find(':').map_or(0, |i| i + 1)..].to_owned(),
            block,
            material: id,
            ladder: s.look == LOOK_LADDER,
        });
    }
    out
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
/// the camera looks: on face B up is −y.
pub fn view_direction(yaw_deg: f64, pitch_deg: f64, face: FaceSign) -> [f64; 3] {
    let (yaw, pitch) = (yaw_deg.to_radians(), pitch_deg.to_radians());
    [
        yaw.sin() * pitch.cos(),
        f64::from(face) * pitch.sin(),
        yaw.cos() * pitch.cos(),
    ]
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
        face: FaceSign,
    ) -> Option<BlockTarget> {
        self.yaw = yaw_deg;
        let dir = view_direction(yaw_deg, pitch_deg, face).map(|v| v as f32);
        self.current = eye.and_then(|e| world.target(e, dir, players::REACH_DISTANCE));
        self.into_target = self.current.is_some_and(|t| world.replaceable(t.cell));
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
            return Some((cell, ladder_for(t.face, self.yaw)));
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
    /// to send, or None when there is no target or it is too soon after the last edit
    /// (`BLOCK_EDIT_INTERVAL_MS`).
    pub fn act(&mut self, action: EditAction, now_ms: f64) -> Option<Message> {
        let t = self.current?;
        if now_ms - self.last_edit_ms < players::BLOCK_EDIT_INTERVAL_MS as f64 {
            return None;
        }
        self.last_edit_ms = now_ms;
        let (action, material) = match action {
            EditAction::Break => (BlockEditAction::Break, 0),
            EditAction::Place => (
                BlockEditAction::Place,
                self.placement().map_or(0, |(_, m)| m),
            ),
        };
        Some(Message::BlockEditRequest(BlockEditRequest {
            action,
            cell: t.cell,
            face: t.face,
            material,
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
    fn offers_every_placeable_material_once_ladders_as_one_slot() {
        let names: Vec<_> = palette().into_iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            ["stone", "dirt", "grass", "ladder", "sand", "tall_grass"]
        );
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
        interaction.update(&mut world, Some([0.0, 1.6, 0.0]), 90.0, 0.0, 1);
        assert_eq!(round(world.rays[0].0.map(f64::from)), [1.0, 0.0, 0.0]);
        assert_eq!(world.rays[0].1, players::REACH_DISTANCE);
        assert_eq!(round(view_direction(0.0, 90.0, 1)), [0.0, 1.0, 0.0]);
        assert_eq!(interaction.update(&mut world, None, 0.0, 0.0, 1), None);
    }

    #[test]
    fn sends_break_and_place_requests_for_the_target_at_most_one_per_interval() {
        let mut world = FakeTarget::default();
        let mut interaction = BlockInteraction::new();
        let interval = players::BLOCK_EDIT_INTERVAL_MS as f64;
        let mut sent = Vec::new();
        assert!(interaction.act(EditAction::Break, 0.0).is_none()); // nothing targeted yet
        interaction.update(&mut world, Some([0.0, 1.6, 0.0]), 0.0, 0.0, 1);
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
                }),
                Message::BlockEditRequest(BlockEditRequest {
                    action: BlockEditAction::Place,
                    cell: [3, 0, 4],
                    face: 5,
                    material: require("dwell:dirt"),
                }),
            ]
        );
    }

    #[test]
    fn places_the_ladder_that_fits_the_targeted_face() {
        let mut world = FakeTarget::default();
        let mut interaction = BlockInteraction::new();
        interaction.update(&mut world, Some([0.0, 1.6, 0.0]), 0.0, 0.0, 1);
        let ladder = interaction.palette().iter().position(|s| s.ladder).unwrap();
        interaction.select(ladder as i64);
        let m = interaction.act(EditAction::Place, 0.0).unwrap();
        assert_eq!(
            name(request(&m).material),
            "dwell:ladder[facing=north,flooded=false]"
        );
    }

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
            .position(|s| s.name == "sand")
            .unwrap();
        interaction.select(sand as i64);
        // Looking +Z at the −Z face (5) of cell (3, 0, 4).
        interaction.update(&mut world, Some([3.5, 1.6, 0.0]), 0.0, 0.0, 1);
        let (cell, material) = interaction.placement().unwrap();
        assert_eq!(cell, [3, 0, 3]);
        assert_eq!(name(material), "dwell:sand");
        let m = interaction.act(EditAction::Place, 0.0).unwrap();
        assert_eq!(request(&m).material, material);
    }

    // Beyond the TS suite: the voxel world is a target source through the shared ray cast.
    #[test]
    fn the_voxel_world_targets_through_the_shared_ray_cast() {
        let mut world = VoxelWorld::streamed();
        // A non-adaptive cube (a lone stone block would be a half pyramid).
        world.set_voxel(3, 0, 4, require("dwell:sandbox_floor"));
        let mut interaction = BlockInteraction::new();
        let t = interaction.update(&mut world, Some([3.5, 0.5, 0.5]), 0.0, 0.0, 1);
        assert_eq!(
            t,
            Some(BlockTarget {
                cell: [3, 0, 4],
                face: 5
            })
        );
    }
}
