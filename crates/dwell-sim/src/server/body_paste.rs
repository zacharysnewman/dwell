//! A copied moving body pasted as a moving body again (owner, 2026-11-23: "A moving body again";
//! WORK_QUEUE section 1, "Copying a body as a blueprint").
//!
//! A client with the `bodies.paste` capability sends `BodyPaste` (a Paste's box: its lowest
//! corner, size and world states, `KEEP` for kept cells). Where the world's mode has the
//! `structures` ability (creative), from a player who may edit, the box within the region reach,
//! in the world and clear of solid blocks (air or a liquid at every cell of the body), its cells
//! that aren't air or kept become one built body there, unturned (it floats if over water); every
//! client hears its `EntitySpawn`. Anything else is refused, nothing changed.

use dwell_protocol::capability_names::BODIES_PASTE;
use dwell_protocol::{RegionEdit, region_edit};
use dwell_world::region::{box_distance_sq, box_in_world_of, region_reach};

use super::*;

impl Server {
    /// A `BodyPaste` from session `id`: the new body's id, or None (refused).
    pub(super) fn body_paste(&mut self, id: SessionId, e: RegionEdit) -> Option<u32> {
        let s = self.sessions.get(&id)?;
        if !s.joined()
            || !s.capabilities.has(BODIES_PASTE)
            || !self.may_edit(s)
            || !self.settings.allows(Ability::Structures)
        {
            return None;
        }
        let eye = self.eye_of(s.handle?);
        let size = e.size.map(i32::from);
        if e.cells.len() != e.volume() {
            return None;
        }
        let reach = f64::from(region_reach(self.brushes_allowed()));
        if box_distance_sq(e.min, size, eye) > reach * reach
            || !box_in_world_of(&self.world.bounds, e.min, size)
        {
            return None;
        }
        let mut cells = Vec::new();
        for y in 0..size[1] {
            for z in 0..size[2] {
                for x in 0..size[0] {
                    let m =
                        e.cells[RegionEdit::cell_index(e.size, x as usize, y as usize, z as usize)];
                    if usize::from(m) == region_edit::KEEP || m == M.air {
                        continue;
                    }
                    let i = dwell_blocks::try_info(m)?;
                    if i.liquid || !dwell_world::edit::placeable(m) {
                        return None;
                    }
                    let c = [e.min[0] + x, e.min[1] + y, e.min[2] + z];
                    let here = self.world.get_voxel(c[0], c[1], c[2]);
                    if here != M.air && !info(here).liquid {
                        return None;
                    }
                    cells.push(([x, y, z], m));
                }
            }
        }
        if cells.is_empty() {
            return None;
        }
        // Its first cell's centre is the body's origin (its cells' offsets from it).
        let first = cells[0].0;
        let cells: Vec<([i32; 3], dwell_blocks::StateId)> = cells
            .into_iter()
            .map(|(o, m)| ([o[0] - first[0], o[1] - first[1], o[2] - first[2]], m))
            .collect();
        let at = [
            e.min[0] + first[0],
            e.min[1] + first[1],
            e.min[2] + first[2],
        ];
        let centre = [
            f64::from(at[0]) + 0.5,
            (f64::from(at[1]) + 0.5) * dwell_world::CELL_H,
            f64::from(at[2]) + 0.5,
        ];
        let built = crate::felling::BODY_KINDS.iter().position(|k| k.place)? as u8;
        Some(self.add_body(built, cells, centre))
    }

    /// The body the last `BodyPaste` made, or None if it was refused (tests).
    pub fn last_body_paste(&self) -> Option<u32> {
        self.last_body_paste
    }
}
