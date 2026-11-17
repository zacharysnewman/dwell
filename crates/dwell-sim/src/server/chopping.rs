//! The server's side of the swung tools (`ChopAction`, ADR 0067): a weapon's swings (the sword,
//! `weapons.rs`), what each player holds, and the answers that tell a client the server takes
//! them. The axe is no longer one of them (owner, 2026-11-14: "Let's simplify the axe to be like
//! the shovel and pick"; 2026-11-15: "it was different from the other tools"): it breaks as a
//! pickaxe does, by block edits that take block damage (docs/BLOCK_DAMAGE.md §14). Wood is no
//! longer cut: no notch, kerfs or bites are made, an older save's cuts are ignored, and an older
//! client's axe swing lands as one block-damage hit ([`Server::older_axe_swing`]).

use super::{Server, SessionId, TickChanges};
use dwell_blocks::block_of;
use dwell_physics::DVec3;
use dwell_protocol::chopping as chop;
use dwell_protocol::inventory as inv;
use dwell_protocol::{Channel, ChopAction, CutsMessage, Message, VoxelModificationReason};
use dwell_world::{CELL_H, M, m_of_row};

/// How far past the edit reach a swing's hit point may be (metres): the client's ray and the
/// server's eye differ by a tick.
const REACH_SLACK_M: f64 = 0.75;

impl Server {
    pub(super) fn chop_action(&mut self, id: SessionId, a: ChopAction) {
        match a {
            // The answer is the client's sign the server takes swings (an empty `Cuts`: no wood
            // is cut any more).
            ChopAction::Hello => self.send_no_cuts(id),
            // An older client's axe (no longer a swung tool, a break tool): one hit.
            ChopAction::Swing { ref tool, .. } | ChopAction::Sweep { ref tool, .. }
                if chop::tool(tool).is_none() =>
            {
                self.older_axe_swing(id, a)
            }
            // An exhausted player's swing is refused; a swing draws its cost (ADR 0069 §5).
            ChopAction::Swing { ref tool, .. } | ChopAction::Sweep { ref tool, .. }
                if !self.swing_stamina(id, tool) => {}
            a @ (ChopAction::Swing { .. } | ChopAction::Sweep { .. }) => self.start_swing(id, a),
            ChopAction::Held { tool, vertical } => self.held_tool(id, tool, vertical),
            ChopAction::Guard { raised } => self.guard_action(id, raised),
        }
    }

    /// One empty `Cuts` (an older build's world may hold cuts; they are not sent, docs/FELLING.md
    /// §8), saying whether this client asked for bites, so it stops asking.
    fn send_no_cuts(&mut self, id: SessionId) {
        let Some(s) = self.sessions.get(&id) else {
            return;
        };
        let m = Message::Cuts(CutsMessage {
            cuts: Vec::new(),
            gone: Vec::new(),
            bites: s.bites_hello,
        });
        self.send_reliable(id, &m, Channel::World);
    }

    /// A client that draws the axe's bites (`BitesHello`, WORK_QUEUE item 26; a datagram, repeated
    /// until answered) is answered once: there are none to send.
    pub(super) fn bites_hello(&mut self, id: SessionId) {
        let Some(s) = self.sessions.get_mut(&id) else {
            return;
        };
        if s.bites_hello {
            return;
        }
        s.bites_hello = true;
        self.send_no_cuts(id);
    }

    /// A weapon's swing arrives (it left the client at the click): it starts now, spending the
    /// swing's wait, and lands at its contact (WORK_QUEUE item 25: the first creature its head
    /// meets), never before the client's. A swing too soon after the last is refused. What it
    /// meets is found again, and checked, at the contact ([`Self::land_swings`]).
    fn start_swing(&mut self, id: SessionId, mut a: ChopAction) {
        let (ChopAction::Swing { tool, .. } | ChopAction::Sweep { tool, .. }) = &a else {
            return;
        };
        let Some(t) = chop::tool(tool).filter(|t| t.is_weapon()) else {
            return;
        };
        if let ChopAction::Swing { vertical, .. } | ChopAction::Sweep { vertical, .. } = &mut a {
            *vertical = t.stance(*vertical);
        }
        let s = &self.sessions[&id];
        let Some(h) = s.handle else {
            return;
        };
        if !s.joined() || self.tick < s.chop_ready {
            return;
        }
        // A weapon cuts no wood: only a swing along its path strikes.
        let ChopAction::Sweep { look, vertical, .. } = &a else {
            return;
        };
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
        let contact = self.weapon_contact(t, eye, look, *vertical);
        // The others see the swing start at once, by its direction (WORK_QUEUE item 17); its hit
        // lands at the contact.
        let vertical = *vertical;
        self.chop_swung(id, vertical);
        let tick = self.tick;
        let s = self.sessions.get_mut(&id).expect("session");
        s.chop_ready = tick + (t.swing_seconds * f64::from(dwell_protocol::SIM_HZ)) as u32;
        // A weapon's swing is read by the NPCs fighting it, and drops its block meanwhile.
        let at = contact.unwrap_or_else(|| t.swing.centre_s());
        s.weapon_swing = Some(super::WeaponSwing {
            start: tick,
            contact: tick + super::contact_ticks(at),
            end: s.chop_ready,
            vertical,
            weapon: t.name,
        });
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
            let ChopAction::Sweep {
                tool,
                look,
                vertical,
            } = a
            else {
                continue;
            };
            // Where blocks are unlimited the hotbar's tool is what `Held` said (none said: an
            // older client, trusted); in a mode with items the weapon checks the held slot.
            if s.held_tool.as_ref().is_some_and(|(held, _)| *held != tool) {
                continue;
            }
            if chop::tool(&tool).is_some_and(|t| t.is_weapon()) {
                self.weapon_sweep(id, &tool, look.map(f64::from), vertical);
            }
        }
    }

    /// Steps this player's swing for the others (a chop, `vertical` or level; a break to a client
    /// that didn't say `SwingHello`) when it may swing now.
    fn chop_swung(&mut self, id: SessionId, vertical: bool) {
        let tick = self.tick;
        if let Some(s) = self.sessions.get_mut(&id)
            && s.joined()
            && s.handle.is_some()
            && tick >= s.chop_ready
        {
            s.swung(
                true,
                if vertical {
                    dwell_protocol::SwingKind::ChopVertical
                } else {
                    dwell_protocol::SwingKind::ChopLevel
                },
            );
        }
    }

    /// An older client's axe swing (it still swings the axe as a chopping tool, sending
    /// `ChopAction::Swing` or `Sweep`): the block its aim meets takes one block-damage hit, as
    /// this build's axe break does, if the axe is the right tool for it (`breaking.classes`: a
    /// log). At most one a break tool's `repeatS`; nothing else of the old swing (no arc, no
    /// leaves cleared).
    fn older_axe_swing(&mut self, id: SessionId, a: ChopAction) {
        let (ChopAction::Swing { tool, .. } | ChopAction::Sweep { tool, .. }) = &a else {
            return;
        };
        let Some(b) = chop::BREAK_TOOLS.iter().find(|b| b.item == tool) else {
            return;
        };
        let Some(s) = self.sessions.get(&id) else {
            return;
        };
        let Some(h) = s.handle else {
            return;
        };
        if !s.joined() || !self.may_edit(s) || self.tick < s.chop_ready {
            return;
        }
        let eye = self.eye_of(h);
        // The cell, the edit face struck (`axis × 2 + negative`, the chopping wire's `^ 1`) and
        // the point.
        let aimed = match a {
            ChopAction::Swing {
                cell, face, point, ..
            } => Some((cell, face ^ 1, point.map(f64::from))),
            ChopAction::Sweep { look, .. } => {
                let len = look
                    .iter()
                    .map(|v| f64::from(*v).powi(2))
                    .sum::<f64>()
                    .sqrt();
                if !(0.5..=1.5).contains(&len) {
                    return;
                }
                let look = look.map(|v| f64::from(v) / len);
                dwell_world::edit::raycast_block(
                    &mut self.world,
                    eye,
                    look.map(|v| v as f32),
                    dwell_world::edit::DEFAULT_REACH,
                )
                .map(|hit| {
                    let d = f64::from(hit.distance);
                    (hit.cell, hit.face, [0, 1, 2].map(|i| eye[i] + look[i] * d))
                })
            }
            _ => None,
        };
        let Some((cell, face, point)) = aimed else {
            return;
        };
        let m = self.world.get_voxel_if_loaded(cell);
        let block = block_of(m).name;
        // Any tool breaks any block that takes hits (owner, 2026-11-17: by its proficiency, slower or
        // for nothing when inefficient).
        if !dwell_protocol::damage::takes_damage(block) {
            return;
        }
        let repeat = b.repeat_s.unwrap_or(0.5);
        let tick = self.tick;
        let s = self.sessions.get_mut(&id).expect("session");
        s.chop_ready = tick + (repeat * f64::from(dwell_protocol::SIM_HZ)) as u32;
        let tool = b.item;
        self.tool_hit(id, tool, cell, face, point, eye);
    }

    /// One hit of a break tool on `cell` (an older client's axe swing): refused as an edit's
    /// break is (out of reach, the tool not in hand where items are used, a block it can't break);
    /// else the tool wears, the hit is recorded toward the block's hits to break with it (its
    /// mark at `face`, an edit face, and `point`), a hit short of the count drops its share of the
    /// drops, and the hit that reaches it breaks the block (the drops' rest) and what it held up
    /// falls, tipped away from the eye, as after any break.
    fn tool_hit(
        &mut self,
        id: SessionId,
        tool: &str,
        cell: [i32; 3],
        face: u8,
        point: [f64; 3],
        eye: [f64; 3],
    ) {
        let uses_items = self.uses_items();
        let s = &self.sessions[&id];
        if uses_items
            && s.inventory
                .slots
                .get(usize::from(s.inventory.held))
                .and_then(|h| h.as_ref())
                .is_none_or(|st| st.item != tool)
        {
            return;
        }
        let reach = f64::from(dwell_world::edit::DEFAULT_REACH) + REACH_SLACK_M;
        if (DVec3::from(point) - DVec3::from(eye)).length() > reach {
            return;
        }
        let m = self.world.get_voxel_if_loaded(cell);
        let block = block_of(m).name;
        if uses_items && !chop::may_break_holding(block, Some(tool)) {
            return;
        }
        let need = if self.config.one_hit_breaks {
            Some(1)
        } else {
            dwell_protocol::damage::hits_to_break(block, Some(tool))
        };
        let Some(n) = need else {
            return;
        };
        let n = n.clamp(1, u32::from(u16::MAX));
        let s = self.sessions.get_mut(&id).expect("session");
        if super::wear_tool(&mut s.inventory, uses_items) != inv::Wear::None {
            s.inventory_dirty = true;
        }
        // The mark: the point in 1/255ths of the cell, world-oriented.
        let lo = [f64::from(cell[0]), m_of_row(cell[1]), f64::from(cell[2])];
        let size = [1.0, CELL_H, 1.0];
        let p = std::array::from_fn(|i| {
            ((point[i] - lo[i]) / size[i] * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8
        });
        let mark = dwell_protocol::damage::HitMark {
            chunk: dwell_protocol::damage::hit_chunk(block, Some(tool)),
            ..dwell_protocol::damage::HitMark::at(face, p)
        };
        let k = self.damage.hit(cell, m, n as u16, mark);
        if u32::from(k) < n {
            self.note_hit(id, cell);
        }
        let centre = dwell_world::centre_of_cell(cell);
        if u32::from(k) < n {
            self.send_damage(&[cell], Vec::new());
            if uses_items {
                self.drop_hit(m, centre, n, u32::from(k));
            }
            return;
        }
        let mut changes = TickChanges::default();
        // The logs joined to it keep their shape (BUGS.md #204).
        for (n, frozen) in super::freeze_joins(&mut self.world, cell) {
            changes.set(&mut self.world, n, frozen);
        }
        let left = if dwell_blocks::info(m).flooded {
            M.water
        } else {
            M.air
        };
        changes.set_and_unsupport(&mut self.world, cell, left);
        self.broadcast_changes(changes, VoxelModificationReason::Edit);
        // A tool the block is inefficient with breaks it for nothing (owner, 2026-11-17).
        if uses_items && dwell_protocol::damage::drops_when_broken(block, Some(tool)) {
            self.drop_destroyed(m, centre, n);
        }
        self.fell(&[cell], 0, Some(DVec3::from(eye)));
    }

    /// What a player holds (`ChopAction::Held`): kept, and told to the others that understand it;
    /// the first one a player sends also brings it everyone else's.
    fn held_tool(&mut self, id: SessionId, tool: String, vertical: bool) {
        // A swung tool (the sword), or a tool a break is made with (`breaking.tools`: an axe, a
        // pickaxe, a shovel).
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
        // Its block drops with the weapon put away; a raised one turns with the stance.
        let weapon = chop::tool(&tool).is_some_and(|t| t.is_weapon());
        let guard_changed =
            s.guard_raised && (!weapon || s.held_tool.as_ref().is_none_or(|(_, v)| *v != vertical));
        if !weapon {
            s.guard_raised = false;
        }
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
        if guard_changed {
            self.send_player_guard(id);
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
}
