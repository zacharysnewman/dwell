//! The server's side of ground items (docs/adr/0071-ground-items.md): the scatter is the shared
//! `dwell_world::ground` function over the server's blocks; the server keeps the gone ones (knocked
//! loose), saves them with the world, checks and applies hits, and tells clients
//! that said `GroundHello`.

use super::{Server, SessionId};
use dwell_blocks::StateId;
use dwell_protocol::{Channel, MAX_GROUND_GONE_PER_MESSAGE, Message};
use dwell_world::FaceFrame;
use dwell_world::ground::{Placed, Rules};

/// How far from the eye a ground item may be hit (m): the block reach and a little.
pub const PICK_REACH_M: f64 = dwell_protocol::players::REACH_DISTANCE as f64 + 0.5;

impl Server {
    /// The ground item lying in `cell` now (not gone), with `was` in `instead`'s cell if given
    /// (what was there before an edit).
    fn ground_item(
        &mut self,
        cell: [i32; 3],
        instead: Option<([i32; 3], StateId)>,
    ) -> Option<Placed> {
        if self.ground.cells.contains(&cell) {
            return None;
        }
        let seed = self.config.world_seed;
        let world = &mut self.world;
        let cache = &mut self.ground_generated;
        // The rules read the terrain as generated (the leaves cleared over a stick leave it); the
        // item's own cell and ground as they are now.
        let mut get = |c: [i32; 3], generated: bool| match instead {
            Some((at, m)) if at == c && !generated => Some(m),
            _ if generated => Some(world.generated_voxel(c, cache)),
            _ => Some(world.get_voxel(c[0], c[1], c[2])),
        };
        Rules::shipped().item_over(&mut get, seed, cell)
    }

    /// `InventoryAction::GroundHello`: the client draws ground items; it is sent every gone one.
    pub(super) fn ground_hello(&mut self, id: SessionId) {
        let Some(s) = self.sessions.get_mut(&id) else {
            return;
        };
        s.ground_hello = true;
        let mut all: Vec<[i32; 3]> = self.ground.cells.iter().copied().collect();
        all.sort_unstable();
        // At least one message (empty when none are gone): the client's sign the server keeps them.
        let parts: Vec<Vec<[i32; 3]>> = if all.is_empty() {
            vec![Vec::new()]
        } else {
            all.chunks(MAX_GROUND_GONE_PER_MESSAGE)
                .map(<[_]>::to_vec)
                .collect()
        };
        for p in parts {
            self.send_reliable(id, &Message::GroundGone(p), Channel::World);
        }
    }

    /// `InventoryAction::Pick`, the break action aimed at a ground item (ADR 0071, amended
    /// 2026-11-09): the item in `cell` is knocked loose if it lies there within reach of the eye
    /// of a player who may edit. It is gone from the scatter and, as a broken block's drops are,
    /// becomes a loose item where it lay in a mode that uses items (walking near collects it); in
    /// creative, where breaking drops nothing (ADR 0066), it simply vanishes.
    pub(super) fn pick_ground(&mut self, id: SessionId, cell: [i32; 3]) -> bool {
        let Some((handle, may_edit)) = self
            .sessions
            .get(&id)
            .filter(|s| s.joined())
            .and_then(|s| s.handle.map(|h| (h, self.may_edit(s))))
        else {
            return false;
        };
        let eye = self.eye_of(handle);
        let Some(it) = self.ground_item(cell, None).filter(|_| may_edit) else {
            return false;
        };
        let p = it.point();
        let d = (0..3).map(|k| (p[k] - eye[k]).powi(2)).sum::<f64>().sqrt();
        let Some(kind) = Rules::shipped().kind(it.kind).copied() else {
            return false;
        };
        if d > PICK_REACH_M {
            return false;
        }
        let s = self.sessions.get_mut(&id).expect("session");
        s.swing = super::next_swing(s.swing, true);
        self.ground.cells.insert(cell);
        self.broadcast_ground_gone(vec![cell]);
        if self.uses_items() {
            self.loose
                .drop(dwell_protocol::inventory::Stack::new(kind.item, 1), p);
        }
        true
    }

    /// Blocks broken under ground items (`broken`: each cell and what it was): each item knocked
    /// loose becomes a loose item where it lay in a mode that uses items (it vanishes in creative,
    /// where breaking drops nothing), and is gone from the scatter, so it does not come back when
    /// the cell is filled again.
    pub(super) fn knock_ground(&mut self, broken: &[([i32; 3], StateId)]) {
        let mut gone = Vec::new();
        for &(cell, was) in broken {
            let f = FaceFrame::at_row(cell[1]);
            let [x, y, z] = f.cell(cell);
            let above = f.cell([x, y + 1, z]);
            let Some(it) = self.ground_item(above, Some((cell, was))) else {
                continue;
            };
            self.ground.cells.insert(above);
            gone.push(above);
            if self.uses_items()
                && let Some(k) = Rules::shipped().kind(it.kind)
            {
                self.loose
                    .drop(dwell_protocol::inventory::Stack::new(k.item, 1), it.point());
            }
        }
        if !gone.is_empty() {
            self.broadcast_ground_gone(gone);
        }
    }

    fn broadcast_ground_gone(&mut self, cells: Vec<[i32; 3]>) {
        let to: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.ground_hello)
            .map(|(&id, _)| id)
            .collect();
        for part in cells.chunks(MAX_GROUND_GONE_PER_MESSAGE) {
            let m = Message::GroundGone(part.to_vec());
            for &id in &to {
                self.send_reliable(id, &m, Channel::World);
            }
        }
    }

    // --- for tests and tools ---

    /// The ground item lying in `cell` now (tests and the bench).
    pub fn ground_item_at(&mut self, cell: [i32; 3]) -> Option<Placed> {
        self.ground_item(cell, None)
    }

    /// The gone ground items' cells.
    pub fn ground_gone(&self) -> &dwell_world::ground::GroundGone {
        &self.ground
    }
}
