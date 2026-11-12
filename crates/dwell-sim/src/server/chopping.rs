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

/// Where an aimed chop lands: the cell, the face met (`axis × 2 + positive`) and the point.
type Aimed = ([i32; 3], u8, [f64; 3]);

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
            a @ (ChopAction::Swing { .. } | ChopAction::Sweep { .. }) => self.start_swing(id, a),
            ChopAction::Held { tool, vertical } => self.held_tool(id, tool, vertical),
        }
    }

    /// A swing arrives (it left the client at the click): it starts now, spending the swing's
    /// wait, and lands at its contact (WORK_QUEUE item 25: the head reaching what it hits, the
    /// tool's `swing` data: `windupS` and the arc's time to the hit, from a sweep now), never
    /// before the client's. A swing too soon after the last is refused. What it meets is found
    /// again, and checked, at the contact ([`Self::land_swings`]).
    fn start_swing(&mut self, id: SessionId, a: ChopAction) {
        let (ChopAction::Swing { tool, .. } | ChopAction::Sweep { tool, .. }) = &a else {
            return;
        };
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
        let contact = match &a {
            ChopAction::Sweep { look, vertical, .. } if !self.config.chop2 => {
                let len = look
                    .iter()
                    .map(|v| f64::from(*v).powi(2))
                    .sum::<f64>()
                    .sqrt();
                if !(0.5..=1.5).contains(&len) {
                    return;
                }
                let look = look.map(|v| f64::from(v) / len);
                let eye = self.eye_of(h);
                let arc = dwell_world::swing::Arc::of(t);
                let swept = dwell_world::swing::sweep(&mut self.world, eye, look, *vertical, &arc);
                let step = swept
                    .hit
                    .map(|x| x.step)
                    .or_else(|| swept.cleared.iter().map(|c| c.step).min());
                step.map(|k| t.swing.time_of_step(k))
            }
            // An aimed swing, and the prototype's chop at the crosshair: the arc's middle.
            _ => Some(t.swing.centre_s()),
        };
        let tick = self.tick;
        let s = self.sessions.get_mut(&id).expect("session");
        s.chop_ready = tick + (t.swing_seconds * f64::from(dwell_protocol::SIM_HZ)) as u32;
        // A free swing at nothing lands nothing.
        if let Some(c) = contact {
            s.pending_chops.push((tick + super::contact_ticks(c), a));
        }
    }

    /// The swings whose contact has come land: each swept again from the player's eye now (what
    /// it meets, its reach) and refused if the tool is no longer in hand (switched away before the
    /// contact: a cancelled swing lands nothing).
    pub(super) fn land_swings(&mut self) {
        let tick = self.tick;
        let due: Vec<(SessionId, ChopAction)> = self
            .sessions
            .iter_mut()
            .flat_map(|(&id, s)| {
                let (now, later) = std::mem::take(&mut s.pending_chops)
                    .into_iter()
                    .partition::<Vec<_>, _>(|(at, _)| *at <= tick);
                s.pending_chops = later;
                now.into_iter().map(move |(_, a)| (id, a))
            })
            .collect();
        for (id, a) in due {
            let Some(s) = self.sessions.get(&id) else {
                continue;
            };
            let (ChopAction::Swing { tool, .. } | ChopAction::Sweep { tool, .. }) = &a else {
                continue;
            };
            // Where blocks are unlimited the hotbar's tool is what `Held` said (none said: an
            // older client, trusted); in a mode with items `swing` checks the held slot.
            if s.held_tool.as_ref().is_some_and(|(held, _)| held != tool) {
                continue;
            }
            match a {
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
                _ => {}
            }
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
        if !s.joined() {
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
        // The chopping prototype aims at the crosshair: the chop lands where the look meets the
        // trunk (its height the stump's), on the face it meets (WORK_QUEUE item 7), and clears only
        // what the look passes through on the way (round 2: the arc's clears broke a felled tree's
        // stump wedge, a `log_ramp`, with the next swing).
        let (cleared, aimed) = if self.config.chop2 {
            self.crosshair_hit(t, eye, look)
        } else {
            (
                swept.cleared.iter().map(|c| c.cell).collect(),
                swept.hit.map(|x| (x.hit.cell, x.axis_face(), x.point)),
            )
        };
        self.clear_cells(id, tool, &cleared, eye);
        let wood = aimed.filter(|x| grain_of(self.world.get_voxel_if_loaded(x.0)).is_some());
        match wood {
            Some((cell, face, point)) => self.swing(id, tool, cell, face, point, vertical),
            // A bounce off what is not a wood is a hit: it wears the tool once (owner, 2026-11-12:
            // "Tools should only consume durability on hit"); a miss, or a swing through only what
            // it clears (leaves: cleared, not hit), wears nothing.
            None if aimed.is_some() => self.wear_swung(id, tool),
            None => {}
        }
    }

    /// One landed hit's wear on the tool swung, where a mode wears tools: only while the tool is
    /// the one in hand (a mode with items).
    fn wear_swung(&mut self, id: SessionId, tool: &str) {
        let uses_items = self.uses_items();
        if !self
            .sessions
            .get(&id)
            .is_some_and(|s| s.joined() && self.may_edit(s))
        {
            return;
        }
        let s = self.sessions.get_mut(&id).expect("session");
        let held = s.inventory.slots.get(usize::from(s.inventory.held));
        if uses_items
            && held
                .and_then(|h| h.as_ref())
                .is_none_or(|st| st.item != tool)
        {
            return;
        }
        if super::wear_tool(&mut s.inventory, uses_items) != inv::Wear::None {
            s.inventory_dirty = true;
        }
    }

    /// The first solid cell the look from `eye` meets within the target outline's reach, past the
    /// blocks `t` clears (leaves): the cells it passed through (to clear) and the cell, the face met
    /// (the chopping wire's `axis × 2 + positive`) and the point (the chopping prototype's aim).
    fn crosshair_hit(
        &mut self,
        t: &chop::ToolDef,
        eye: [f64; 3],
        look: [f64; 3],
    ) -> (Vec<[i32; 3]>, Option<Aimed>) {
        let dir = look.map(|v| v as f32);
        let passed = std::cell::RefCell::new(Vec::new());
        let hit = dwell_world::edit::raycast_block_past(
            &mut self.world,
            eye,
            dir,
            // As far as the block-target outline (round 2: a trunk you can outline you can chop).
            dwell_protocol::players::REACH_DISTANCE,
            |c, m| {
                // A wood's fell wedge (a stump's face) stops the look: the prototype keeps it.
                let name = block_of(m).name;
                let through =
                    t.clears_block(name) && !chop::WOODS.iter().any(|w| w.fell_wedge == Some(name));
                if through {
                    passed.borrow_mut().push(c);
                }
                through
            },
        );
        let aimed = hit.map(|hit| {
            let d = f64::from(hit.distance);
            let point = [0, 1, 2].map(|i| eye[i] + look[i] * d);
            (hit.cell, hit.face ^ 1, point)
        });
        (passed.into_inner(), aimed)
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
        // An axe, or a tool a break is made with (`breaking.tools`: a pickaxe, a shovel).
        let tool =
            if chop::tool(&tool).is_some() || chop::BREAK_TOOLS.iter().any(|t| t.item == tool) {
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
        // A chop wears the axe (owner, 2026-11-11: "It should survive 32 hits of durability").
        if super::wear_tool(&mut s.inventory, uses_items) != inv::Wear::None {
            s.inventory_dirty = true;
        }
        // Face-local: the swing's plane from the look (eye to the hit), the grain, the band.
        let f = FaceFrame::at_row(cell[1]);
        let look = f.vector(to.normalize_or_zero().to_array());
        let mut g = [0.0; 3];
        g[grain] = 1.0;
        let share = chop::share(t, chop::swing_normal(vertical, look), f.vector(g));
        let band = band_of(f, cell, grain, point);
        let v2 = self.config.chop2;
        let cut = self.cuts.cells.entry(cell).or_default();
        if v2 {
            // The chopping prototype (WORK_QUEUE item 7): a share of the section a chop, half of
            // it along the grain (the swing's plane more along the grain than across it).
            let along = share < 0.5 * (t.across + t.along);
            cut.chop2(t, wood, grain, side, along);
        } else {
            cut.swing(t, grain, side, band, share);
        }
        let cut = *cut;
        let standing = grain == 1;
        let through = if v2 {
            cut.chop2_through(wood, grain)
        } else if standing {
            cut.cross(grain) >= wood.fell_fraction
        } else {
            cut.cross(grain) >= wood.buck_fraction
        };
        if !through {
            self.send_cuts(vec![(cell, cut)], Vec::new());
            return;
        }
        let centre = crate::felling::cell_centre(cell);
        // A standing wood with a fell wedge (Phase 55 T2; owner, 2026-11-09): the cut cell splits
        // along its diagonal, the stump keeping the ramp rising away from the notch and the trunk
        // above taking the upside-down ramp as its new bottom; the trunk is released tipping toward
        // the notch with the wood's `fellTip` share of the topple spin (owner, 2026-11-09: "make
        // the tree tip if needed, like we used to, maybe 3/4 of the force"; 0: the contact alone).
        // Toward the notch (the prototype: the notch steers, else the deepest cut; today: the
        // deepest cut).
        let toward = if v2 {
            cut.chop2_toward(grain)
        } else {
            cut.deepest(grain)
        };
        let n = chop::side_normal(grain, toward.unwrap_or(side));
        let above = dwell_world::edit::above(cell);
        // The trunk's bottom needs the log above the cut (else no wedges: it falls as before).
        let wedges = wood
            .fell_wedge
            .filter(|_| standing && self.world.get_voxel_if_loaded(above) == m)
            .and_then(|w| {
                // The prototype's split keeps the notch: the trunk's bottom is its upper face.
                let matching = if v2 {
                    wood.chop2.wedge_matching
                } else {
                    wood.fell_wedge_matching
                };
                let trunk = if matching {
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
