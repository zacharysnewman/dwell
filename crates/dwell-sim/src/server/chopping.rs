//! The server's side of chopping (docs/adr/0067-stone-axe-and-chopping.md): aimed axe swings cut
//! wood cells (`dwell_protocol::chopping::Cut`), a standing trunk cut past its fell fraction falls
//! toward the notch (docs/FELLING.md), a lying log cut through drops its wood; the cuts are
//! saved with the world and sent to clients that said `ChopAction::Hello`.

use super::{Server, SessionId, TickChanges};
use dwell_blocks::{block_of, state_property};
use dwell_physics::DVec3;
use dwell_protocol::chopping::{self as chop, Cut};
use dwell_protocol::inventory as inv;
use dwell_protocol::{
    Channel, ChopAction, CutsMessage, MAX_CUTS_PER_MESSAGE, Message, VoxelModificationReason,
};
use dwell_world::{CELL_H, FaceFrame, M, m_of_row};

/// How far past the edit reach a swing's hit point may be (metres): the client's ray and the
/// server's eye differ by a tick.
const REACH_SLACK_M: f64 = 0.75;

/// A wood state's grain: its `axis` property as a world axis (0 x, 1 y, 2 z).
pub(super) fn grain_of(m: dwell_blocks::StateId) -> Option<usize> {
    chop::wood(block_of(m).name)?;
    Some(match state_property(m, "axis") {
        Some("x") => 0,
        Some("z") => 2,
        _ => 1,
    })
}

/// The band (0 or 1, face-locally lower first) a world point is in along a cell's grain.
pub(super) fn band_of(f: FaceFrame, cell: [i32; 3], grain: usize, point: [f64; 3]) -> usize {
    let frac = if grain == 1 {
        let row = f.row(cell[1]);
        (f.y(point[1]) - m_of_row(row)) / CELL_H
    } else {
        point[grain] - f64::from(cell[grain])
    };
    usize::from(frac >= 0.5)
}

impl Server {
    pub(super) fn chop_action(&mut self, id: SessionId, a: ChopAction) {
        match a {
            ChopAction::Hello => {
                let s = self.sessions.get_mut(&id).expect("session");
                s.chop_hello = true;
                // At least one (empty when there are none): the client's sign the server chops.
                let all = self.cuts.sorted();
                let parts: Vec<Vec<_>> = if all.is_empty() {
                    vec![Vec::new()]
                } else {
                    all.chunks(MAX_CUTS_PER_MESSAGE)
                        .map(<[_]>::to_vec)
                        .collect()
                };
                for cuts in parts {
                    let m = Message::Cuts(CutsMessage {
                        cuts,
                        gone: Vec::new(),
                    });
                    self.send_reliable(id, &m, Channel::World);
                }
            }
            // An exhausted player's swing is refused; a swing draws its cost (ADR 0069 §5).
            ChopAction::Swing { ref tool, .. } | ChopAction::Sweep { ref tool, .. }
                if !self.swing_stamina(id, tool) => {}
            ChopAction::Swing {
                tool,
                cell,
                face,
                point,
                vertical,
            } => self.swing(id, &tool, cell, face, point.map(f64::from), vertical),
            ChopAction::Sweep {
                tool,
                look,
                vertical,
            } => self.sweep(id, &tool, look.map(f64::from), vertical),
            ChopAction::Held { tool, vertical } => self.held_tool(id, tool, vertical),
        }
    }

    /// A swing along its path (`ChopAction::Sweep`): the tool's arc swept from this player's eye
    /// (`dwell_world::swing`); the cells of its `clears` it went through are broken
    /// ([`Self::clear_cells`]), its first other solid hit cut as an aimed swing would be; a miss or
    /// a hit on what is not a wood (a bounce) only spends the swing.
    fn sweep(&mut self, id: SessionId, tool: &str, look: [f64; 3], vertical: bool) {
        let Some(t) = chop::tool(tool) else {
            return;
        };
        let s = &self.sessions[&id];
        let Some(h) = s.handle else {
            return;
        };
        if !s.joined() || self.tick < s.chop_ready {
            return;
        }
        let len = (look[0] * look[0] + look[1] * look[1] + look[2] * look[2]).sqrt();
        if !(0.5..=1.5).contains(&len) {
            return;
        }
        let look = look.map(|v| v / len);
        let eye = self.eye_of(h);
        let arc = dwell_world::swing::Arc::of(t);
        let swept = dwell_world::swing::sweep(&mut self.world, eye, look, vertical, &arc);
        let cleared: Vec<[i32; 3]> = swept.cleared.iter().map(|c| c.cell).collect();
        self.clear_cells(id, tool, &cleared, eye);
        let wood = swept
            .hit
            .filter(|x| grain_of(self.world.get_voxel_if_loaded(x.hit.cell)).is_some());
        match wood {
            Some(x) => self.swing(id, tool, x.hit.cell, x.axis_face(), x.point, vertical),
            None => {
                let s = self.sessions.get_mut(&id).expect("session");
                s.chop_ready =
                    self.tick + (t.swing_seconds * f64::from(dwell_protocol::SIM_HZ)) as u32;
            }
        }
    }

    /// Breaks the cells a swing of `tool` went through (its `clears`: leaves, for the axe) in one
    /// edit: what they leave without support falls as after a break by hand, and where blocks
    /// are not unlimited each drops its block's `drops`. Refused as a swing is: a player who can't
    /// edit, or (a mode that uses items) isn't holding the tool.
    fn clear_cells(&mut self, id: SessionId, tool: &str, cells: &[[i32; 3]], eye: [f64; 3]) {
        let Some(t) = chop::tool(tool) else {
            return;
        };
        if cells.is_empty() {
            return;
        }
        let uses_items = self.uses_items();
        let s = &self.sessions[&id];
        if !s.joined() || !self.may_edit(s) {
            return;
        }
        if uses_items {
            let held = s.inventory.slots.get(usize::from(s.inventory.held));
            if held
                .and_then(|h| h.as_ref())
                .is_none_or(|st| st.item != tool)
            {
                return;
            }
        }
        let reach = f64::from(dwell_world::edit::DEFAULT_REACH) + REACH_SLACK_M;
        let mut writes = Vec::new();
        for &c in cells {
            let m = self.world.get_voxel_if_loaded(c);
            let name = block_of(m).name;
            if !t.clears_block(name)
                || (uses_items && !chop::may_break_holding(name, Some(tool)))
                || (DVec3::from(dwell_world::centre_of_cell(c)) - DVec3::from(eye)).length() > reach
            {
                continue;
            }
            // Flooded leaves leave the water they held.
            let left = if dwell_blocks::info(m).flooded {
                M.water
            } else {
                M.air
            };
            writes.push((c, left));
        }
        if writes.is_empty() {
            return;
        }
        let mut changes = TickChanges::default();
        let mut broken = Vec::new();
        for (at, m) in dwell_world::edit::edit_writes_all(&mut self.world, &writes) {
            if uses_items && (m == M.air || dwell_blocks::info(m).liquid) {
                broken.push((at, self.world.get_voxel(at[0], at[1], at[2])));
            }
            changes.set(&mut self.world, at, m);
        }
        let edited = changes.cells();
        self.broadcast_changes(changes, VoxelModificationReason::Edit);
        for (cell, was) in broken {
            self.drop_block(was, dwell_world::centre_of_cell(cell));
        }
        self.fell(&edited, 0, Some(DVec3::from(eye)));
    }

    /// What a player holds (`ChopAction::Held`): kept, and told to the others that understand it;
    /// the first one a player sends also brings it everyone else's.
    fn held_tool(&mut self, id: SessionId, tool: String, vertical: bool) {
        let tool = if chop::tool(&tool).is_some() {
            tool
        } else {
            String::new()
        };
        let Some(s) = self.sessions.get_mut(&id) else {
            return;
        };
        let first = s.held_tool.is_none();
        s.held_tool = Some((tool.clone(), vertical));
        let player_id = s.player_id;
        let entry = dwell_protocol::HeldTool {
            player_id,
            tool,
            vertical,
        };
        let others: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(o, s)| **o != id && s.held_tool.is_some())
            .map(|(&o, _)| o)
            .collect();
        let m = Message::HeldTools(vec![entry]);
        for o in &others {
            self.send_reliable(*o, &m, Channel::World);
        }
        if first {
            let all: Vec<_> = others
                .iter()
                .filter_map(|o| {
                    let s = &self.sessions[o];
                    let (tool, vertical) = s.held_tool.clone()?;
                    (!tool.is_empty()).then_some(dwell_protocol::HeldTool {
                        player_id: s.player_id,
                        tool,
                        vertical,
                    })
                })
                .collect();
            // At least one (empty when nobody holds a tool): the client's sign the server sweeps.
            self.send_reliable(id, &Message::HeldTools(all), Channel::World);
        }
    }

    /// The tool a player holds, by item name (tests).
    pub fn held_tool_of(&self, player_id: u16) -> Option<(String, bool)> {
        self.sessions
            .values()
            .find(|s| s.player_id == player_id)
            .and_then(|s| s.held_tool.clone())
    }

    /// One swing; refused (silently: the client's chips are its own) when the tool isn't one, the
    /// player can't edit, isn't holding it (a mode that uses items), swings too soon, out of
    /// reach, or not at a wood cell's side.
    fn swing(
        &mut self,
        id: SessionId,
        tool: &str,
        cell: [i32; 3],
        face: u8,
        point: [f64; 3],
        vertical: bool,
    ) {
        let Some(t) = chop::tool(tool) else {
            return;
        };
        let uses_items = self.uses_items();
        let s = &self.sessions[&id];
        if !s.joined() || !self.may_edit(s) {
            return;
        }
        let Some(h) = s.handle else {
            return;
        };
        if uses_items {
            let held = s.inventory.slots.get(usize::from(s.inventory.held));
            if held
                .and_then(|h| h.as_ref())
                .is_none_or(|st| st.item != tool)
            {
                return;
            }
        }
        if self.tick < s.chop_ready {
            return;
        }
        let eye = self.eye_of(h);
        let to = DVec3::from(point) - DVec3::from(eye);
        if to.length() > f64::from(dwell_world::edit::DEFAULT_REACH) + REACH_SLACK_M {
            return;
        }
        let lo = DVec3::new(f64::from(cell[0]), m_of_row(cell[1]), f64::from(cell[2]));
        let hi = lo + DVec3::new(1.0, CELL_H, 1.0);
        let slack = DVec3::splat(0.05);
        let p = DVec3::from(point);
        if p.cmplt(lo - slack).any() || p.cmpgt(hi + slack).any() {
            return;
        }
        let m = self.world.get_voxel_if_loaded(cell);
        let Some(grain) = grain_of(m) else {
            return;
        };
        let Some(side) = chop::side_of(grain, usize::from(face / 2), face % 2 == 1) else {
            return;
        };
        let wood = chop::wood(block_of(m).name).expect("a wood");
        let s = self.sessions.get_mut(&id).expect("session");
        s.chop_ready = self.tick + (t.swing_seconds * f64::from(dwell_protocol::SIM_HZ)) as u32;
        // Face-local: the swing's plane from the look (eye to the hit), the grain, the band.
        let f = FaceFrame::at_row(cell[1]);
        let look = f.vector(to.normalize_or_zero().to_array());
        let mut g = [0.0; 3];
        g[grain] = 1.0;
        let share = chop::share(t, chop::swing_normal(vertical, look), f.vector(g));
        let band = band_of(f, cell, grain, point);
        let cut = self.cuts.cells.entry(cell).or_default();
        cut.swing(t, grain, side, band, share);
        let cut = *cut;
        let standing = grain == 1;
        let through = if standing {
            wood.fell_fraction
        } else {
            wood.buck_fraction
        };
        if cut.cross(grain) < through {
            self.send_cuts(vec![(cell, cut)], Vec::new());
            return;
        }
        let centre = crate::felling::cell_centre(cell);
        // A standing wood with a fell wedge (Phase 55 T2; owner, 2026-11-09): the cut cell splits
        // along its diagonal, the stump keeping the ramp rising away from the notch and the trunk
        // above taking the upside-down ramp as its new bottom; the trunk is released tipping toward
        // the notch with the wood's `fellTip` share of the topple spin (owner, 2026-11-09: "make
        // the tree tip if needed, like we used to, maybe 3/4 of the force"; 0: the contact alone).
        let n = chop::side_normal(grain, cut.deepest(grain).unwrap_or(side));
        let above = dwell_world::edit::above(cell);
        // The trunk's bottom needs the log above the cut (else no wedges: it falls as before).
        let wedges = wood
            .fell_wedge
            .filter(|_| standing && self.world.get_voxel_if_loaded(above) == m)
            .and_then(|w| {
                let trunk = if wood.fell_wedge_matching {
                    crate::felling::WedgeTrunk::Matching
                } else {
                    crate::felling::WedgeTrunk::Open
                };
                crate::felling::fell_wedges(w, [f64::from(n[0]), f64::from(n[2])], trunk)
            });
        let mut changes = TickChanges::default();
        if let Some((_, top)) = wedges {
            // The trunk's new bottom, released with it.
            changes.set(&mut self.world, above, f.state(top));
        }
        // Cut through: the cell's wood is gone (chips; its cut goes with it, `cuts_dropped`).
        changes.set_and_unsupport(&mut self.world, cell, M.air);
        self.broadcast_changes(changes, VoxelModificationReason::Edit);
        // The uncut side is the hinge: a felled trunk tips away from a point out past it.
        let hinge = centre - DVec3::new(f64::from(n[0]), 0.0, f64::from(n[2])) * 2.0;
        if let Some((stump, _)) = wedges {
            // Released tipping (its `fellTip`), then the stump's ramp set where it stood.
            let tip = (wood.fell_tip > 0.0).then_some((hinge, wood.fell_tip));
            self.fell_tipped(&[cell], 0, tip);
            let mut changes = TickChanges::default();
            changes.set(&mut self.world, cell, f.state(stump));
            self.broadcast_changes(changes, VoxelModificationReason::Edit);
        } else if standing {
            // No wedge: it falls toward the notch, as real felling does (owner, 2026-11-08): the
            // uncut side is the hinge, so it tips away from a point out past the side opposite the
            // deepest.
            self.fell(&[cell], 0, Some(hinge));
        } else {
            if uses_items {
                let stacks = inv::roll(wood.buck_drops, &mut || self.random());
                for st in stacks {
                    self.loose.drop(st, centre.to_array());
                }
            }
            self.fell(&[cell], 0, Some(DVec3::from(eye)));
        }
    }

    /// The cuts of cells whose block changed are gone (the wood broken, cut through or replaced).
    pub(super) fn cuts_dropped(&mut self, cells: &[([i32; 3], u16)]) {
        if self.cuts.cells.is_empty() {
            return;
        }
        let gone: Vec<[i32; 3]> = cells
            .iter()
            .filter(|(c, was)| self.world.get_voxel_if_loaded(*c) != *was)
            .filter_map(|(c, _)| self.cuts.cells.remove(c).map(|_| *c))
            .collect();
        if !gone.is_empty() {
            self.send_cuts(Vec::new(), gone);
        }
    }

    fn send_cuts(&mut self, cuts: Vec<([i32; 3], Cut)>, gone: Vec<[i32; 3]>) {
        let to: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.chop_hello)
            .map(|(&id, _)| id)
            .collect();
        let m = Message::Cuts(CutsMessage { cuts, gone });
        for id in to {
            self.send_reliable(id, &m, Channel::World);
        }
    }

    /// A wood cell's cut (tests, the bench).
    pub fn cut_of(&self, cell: [i32; 3]) -> Option<Cut> {
        self.cuts.cells.get(&cell).copied()
    }
}
