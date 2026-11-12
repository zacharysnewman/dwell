//! The server's side of weapons (WORK_QUEUE item 35): a `weapon` tool's swing (a sword's) is swept
//! as the axe's (`ChopAction::Sweep`, timed to its contact), but at contact it strikes the
//! creatures its head crosses ([`crate::creatures::Creatures::crossed`]: animals, and NPCs by the
//! defender API; never a player), the head stopping at the first solid block. Each creature struck
//! takes the weapon's damage and knockback by data, its sword's block (if raised, facing the
//! striker) deciding by the blades' angle whether the strike is blocked, glances or connects
//! (`GuardDef::clash`); a creature killed drops its kind's `drops`; the weapon wears once a
//! creature struck. A blocked strike recoils the striker (its next swing waits `recoilS`).

use super::{Server, SessionId};
use dwell_protocol::chopping::{self as chop, Clash};
use dwell_protocol::inventory::{self as inv, Stack};

/// The most strikes kept for tests and the dev report ([`Server::strikes`]).
const STRIKE_LOG: usize = 64;

/// One creature struck by a player's weapon.
#[derive(Clone, Debug, PartialEq)]
pub struct StrikeLog {
    pub player_id: u16,
    pub creature: u32,
    /// The arc step the head met it at.
    pub step: u32,
    /// The clash with its raised sword, if it met one.
    pub clash: Option<Clash>,
    /// Health taken (0: blocked).
    pub damage: f64,
    pub killed: bool,
}

impl Server {
    /// The head's path of `tool`'s swing from this player's eye now: each step's direction and
    /// reach, and the creatures it crosses with the step each is met at.
    pub(super) fn weapon_path(
        &mut self,
        t: &chop::ToolDef,
        eye: [f64; 3],
        look: [f64; 3],
        vertical: bool,
    ) -> (dwell_world::swing::Path, Vec<(u32, u32)>) {
        let arc = dwell_world::swing::Arc::of(t);
        let path = dwell_world::swing::reaches(&mut self.world, eye, look, vertical, &arc);
        let max = t.weapon.map_or(1, |w| w.max_targets) as usize;
        let crossed = self.creatures.crossed(eye, &path, max);
        (path, crossed)
    }

    /// When a weapon's swing lands (s after its start): at the first creature its head meets, else
    /// at the block that stops it, else None (a free swing at nothing lands nothing).
    pub(super) fn weapon_contact(
        &mut self,
        t: &chop::ToolDef,
        eye: [f64; 3],
        look: [f64; 3],
        vertical: bool,
    ) -> Option<f64> {
        let (path, crossed) = self.weapon_path(t, eye, look, vertical);
        let step = crossed.first().map(|c| c.1).or_else(|| {
            let last = path.len().checked_sub(1)?;
            (path[last].1 < t.swing.reach_m - 1e-9).then_some(last as u32)
        })?;
        Some(t.swing.time_of_step(step))
    }

    /// A weapon's swing at its contact: swept again from the player's eye now, striking the
    /// creatures it crosses. Refused (silently) for a player not joined or, in a mode with items,
    /// not holding the weapon.
    pub(super) fn weapon_sweep(
        &mut self,
        id: SessionId,
        tool: &str,
        look: [f64; 3],
        vertical: bool,
    ) {
        let Some(t) = chop::tool(tool) else {
            return;
        };
        let Some(w) = t.weapon else {
            return;
        };
        let uses_items = self.uses_items();
        let Some(s) = self.sessions.get(&id) else {
            return;
        };
        let Some(h) = s.handle else {
            return;
        };
        if !s.joined() {
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
        let player_id = s.player_id;
        let len = (look[0] * look[0] + look[1] * look[1] + look[2] * look[2]).sqrt();
        if !(0.5..=1.5).contains(&len) {
            return;
        }
        let look = look.map(|v| v / len);
        let eye = self.eye_of(h);
        let (_, crossed) = self.weapon_path(t, eye, look, vertical);
        let mut blocked = false;
        for (creature, step) in crossed {
            let Some(strike) = self.creatures.strike(creature, &w, vertical, eye) else {
                continue;
            };
            blocked |= strike.clash == Some(Clash::Blocked);
            let damage = w.damage * strike.clash.map_or(1.0, |k| w.guard.damage_share(k));
            let killed = strike.hurt.is_some_and(|x| x.killed);
            if let Some(hurt) = strike.hurt.filter(|x| x.killed) {
                let drops = self.creatures.table.creatures[hurt.kind].drops;
                for &(item, min, max) in drops {
                    let n =
                        min + ((self.random() * f64::from(max - min + 1)) as u32).min(max - min);
                    if n > 0 {
                        self.loose.drop(Stack::new(item, n as u16), hurt.position);
                    }
                }
            }
            self.strike_log.push(StrikeLog {
                player_id,
                creature,
                step,
                clash: strike.clash,
                damage: if strike.hurt.is_some() { damage } else { 0.0 },
                killed,
            });
            if self.strike_log.len() > STRIKE_LOG {
                self.strike_log.remove(0);
            }
            self.wear_weapon(id, tool);
        }
        // A blocked strike recoils: the striker's next swing waits the weapon's recoil.
        if blocked {
            let ticks = (w.guard.recoil_s * f64::from(dwell_protocol::SIM_HZ)).ceil() as u32;
            let tick = self.tick;
            if let Some(s) = self.sessions.get_mut(&id) {
                s.chop_ready = s.chop_ready.max(tick + ticks);
            }
        }
    }

    /// One strike's wear on the weapon in hand, where a mode wears tools (a mode with items: only
    /// while it is the one held).
    fn wear_weapon(&mut self, id: SessionId, tool: &str) {
        let uses_items = self.uses_items();
        let Some(s) = self.sessions.get_mut(&id) else {
            return;
        };
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

    /// The last strikes of players' weapons on creatures, oldest first (tests, dev tools).
    pub fn strikes(&self) -> &[StrikeLog] {
        &self.strike_log
    }

    /// Sets a creature's sword (the defender API; tests, NPCs). False: no such creature.
    pub fn set_creature_guard(&mut self, id: u32, guard: Option<crate::creatures::Guard>) -> bool {
        self.creatures.set_guard(id, guard)
    }

    /// The creatures' data (tests).
    pub fn creature_table(&self) -> crate::creatures::Table {
        self.creatures.table
    }

    /// The creatures, mutably (tests: placing one in a swing's way).
    pub fn creatures_mut(&mut self) -> &mut crate::creatures::Creatures {
        &mut self.creatures
    }
}
