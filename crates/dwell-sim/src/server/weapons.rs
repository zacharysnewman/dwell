//! The server's side of weapons (WORK_QUEUE item 35): a `weapon` tool's swing (a sword's) is swept
//! as the axe's (`ChopAction::Sweep`, timed to its contact), but at contact it strikes the
//! creatures its head crosses ([`crate::creatures::Creatures::crossed`]: animals, and NPCs by the
//! defender API; never a player), the head stopping at the first solid block. Each creature struck
//! takes the weapon's damage and knockback by data, its sword's block (if raised, facing the
//! striker) deciding by the blades' angle whether the strike is blocked, glances or connects
//! (`GuardDef::clash`); a creature killed drops its kind's `drops`; the weapon wears once a
//! creature struck. A blocked strike recoils the striker (its next swing waits `recoilS`).
//!
//! An NPC's strike at a player (WORK_QUEUE item 38; owner, 2026-11-12: "Villagers should be able to
//! hurt players.") is resolved against the player's raised block by the same angle rule: blocked,
//! the NPC recoils and the player is unhurt; a glance or a connecting strike takes the weapon's
//! `damage` (a glance its `glanceDamage` share) through the damage rules, cause `Weapon` (a mode
//! without the `damage` ability, creative, is neither hurt nor pushed), and pushes the player away
//! by the weapon's `knockbackMps` and `liftMps`. Players never strike players.

use super::{Server, SessionId};
use crate::creatures::{FoeView, NpcSwing, SwingPhase};
use dwell_physics::Vec3;
use dwell_protocol::DamageCause;
use dwell_protocol::capability_names::WEAPONS_CLASH;
use dwell_protocol::chopping::{self as chop, Clash};
use dwell_protocol::inventory::{self as inv, Stack};
use dwell_protocol::modes::Ability;
use dwell_protocol::{
    Channel, ClashNet, Fighter, GuardNet, MAX_GUARDS_PER_MESSAGE, Message, SwordplayMessage,
};
use dwell_world::FaceFrame;

/// A player's weapon swing under way (round 2: the NPCs read it, and it drops the player's block):
/// the ticks it started, lands (its contact; a free swing: the arc's middle) and ends, its plane
/// and its weapon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeaponSwing {
    pub start: u32,
    pub contact: u32,
    pub end: u32,
    pub vertical: bool,
    pub weapon: &'static str,
}

impl WeaponSwing {
    /// Where it is at `tick`, None once it ended. Its contact tick is still coming: the strike
    /// lands in the swings' landing early in the next tick (`land_swings`).
    pub fn phase(&self, tick: u32) -> Option<SwingPhase> {
        if tick >= self.end {
            None
        } else if tick <= self.contact {
            Some(SwingPhase::Coming)
        } else {
            Some(SwingPhase::Recovering)
        }
    }
}

/// One NPC's strike at a player: the clash with the player's raised block, if it met one (None:
/// out of reach), and the health it took (WORK_QUEUE item 38: 0 when blocked or in a mode without
/// player damage).
#[derive(Clone, Debug, PartialEq)]
pub struct NpcStrikeLog {
    pub npc: u32,
    pub player_id: u16,
    pub vertical: bool,
    /// The outcome: None out of reach; else the clash (Connected when no block met it).
    pub outcome: Option<Clash>,
    /// Health taken from the player.
    pub damage: i32,
}

/// Whether `them` (face-local) is within `front_deg` round the facing `yaw` (degrees, 0 faces +z)
/// of one at `me` (face-local).
fn in_front(yaw: f64, me: [f64; 3], them: [f64; 3], front_deg: f64) -> bool {
    let to = (them[2] - me[2]).atan2(them[0] - me[0]);
    let y = yaw.to_radians();
    let facing = y.cos().atan2(y.sin());
    let off = (to - facing + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
        - std::f64::consts::PI;
    off.abs().to_degrees() <= front_deg / 2.0
}

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
        let (path, crossed) = self.weapon_path(t, eye, look, vertical);
        let mut blocked = false;
        for (creature, step) in crossed {
            let table = self.creatures.table;
            let pre = self
                .creatures
                .get(creature)
                .map(|c| (c.guard, c.position, c.yaw, table.creatures[c.kind].rows));
            let Some(strike) = self.creatures.strike(creature, &w, vertical, eye) else {
                continue;
            };
            self.creatures.set_foe_player(creature, player_id);
            blocked |= strike.clash == Some(Clash::Blocked);
            // The outcome to the clients near (a strike on a creature holding no block connects).
            if let Some((guard, feet, yaw, rows)) = pre {
                // Where the blades met: the creature's raised blade when it clashed, else its body.
                let theirs = guard
                    .filter(|_| strike.clash.is_some())
                    .and_then(|g| Some((chop::tool(g.weapon)?.weapon?.guard, g.vertical)))
                    .map(|(g, v)| raised_blade(g.blade(v), feet, yaw));
                let point = clash_point(eye, &path, theirs, body_post(feet, rows));
                self.send_clash(
                    ClashNet {
                        striker: Fighter::Player(player_id),
                        defender: Fighter::Creature(creature),
                        weapon: t.name.to_string(),
                        outcome: strike.clash.unwrap_or(Clash::Connected),
                        point: point.map(|v| v as f32),
                        striker_vertical: vertical,
                        defender_vertical: guard.is_some_and(|g| g.vertical),
                    },
                    w.guard.clash.hear_m,
                );
            }
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
    /// Dev tools (the local world's `spawnNpcs`, docs/DEV_TOOLS.md): `n` NPCs of `kind` in a ring
    /// `radius` m round `centre` (world feet), each its own palette variant (`variant_base` + i),
    /// living as spawned ones do. Their ids; fewer at the world's most creatures.
    pub fn spawn_npcs_dev(
        &mut self,
        kind: &str,
        centre: [f64; 3],
        n: usize,
        radius: f64,
        variant_base: u32,
    ) -> Vec<u32> {
        let Some(k) = self.creatures.kind_of(kind) else {
            return Vec::new();
        };
        let f = dwell_world::FaceFrame::at_height(centre[1]);
        let c = f.point(centre);
        let mut ids = Vec::new();
        for i in 0..n {
            let a = i as f64 / n.max(1) as f64 * std::f64::consts::TAU;
            let at = f.point([c[0] + radius * a.sin(), c[1], c[2] + radius * a.cos()]);
            let yaw = (a.to_degrees() + 180.0).rem_euclid(360.0);
            let Some(id) = self.creatures.add_driven(k, at, yaw, 7 + i as u64) else {
                break;
            };
            if let Some(cr) = self.creatures.get_mut(id) {
                cr.driven = false;
                cr.variant = variant_base.wrapping_add(i as u32);
            }
            ids.push(id);
        }
        ids
    }

    pub fn creatures_mut(&mut self) -> &mut crate::creatures::Creatures {
        &mut self.creatures
    }

    /// The NPCs' strikes on players, oldest first (tests, dev tools).
    pub fn npc_strikes(&self) -> &[NpcStrikeLog] {
        &self.npc_strike_log
    }

    /// Whether player `player_id`'s block guards now (round 2): raised, holding a weapon, not
    /// mid-swing (attacking drops the block). Its stance is its held plane.
    pub fn player_guard(&self, player_id: u16) -> Option<(bool, bool)> {
        let id = self.session_of_player(player_id)?;
        let s = self.sessions.get(&id)?;
        let (tool, vertical) = s.held_tool.as_ref()?;
        chop::tool(tool).filter(|t| t.is_weapon())?;
        let swinging = s.weapon_swing.is_some_and(|w| w.phase(self.tick).is_some());
        Some((*vertical, s.guard_raised && !swinging))
    }

    /// A player raises or lowers its block (`ChopAction::Guard`): kept, and its guard told to the
    /// others with `weapons.clash`.
    pub(super) fn guard_action(&mut self, id: SessionId, raised: bool) {
        let Some(s) = self.sessions.get_mut(&id) else {
            return;
        };
        // Only a weapon held raises a block.
        let weapon = s
            .held_tool
            .as_ref()
            .is_some_and(|(t, _)| chop::tool(t).is_some_and(|t| t.is_weapon()));
        let raised = raised && weapon;
        if s.guard_raised == raised {
            return;
        }
        s.guard_raised = raised;
        self.send_player_guard(id);
    }

    /// This player's guard to the other clients with `weapons.clash` (its block or its stance
    /// changed).
    pub(super) fn send_player_guard(&mut self, id: SessionId) {
        let Some(s) = self.sessions.get(&id) else {
            return;
        };
        let vertical = s.held_tool.as_ref().is_some_and(|(_, v)| *v);
        let g = GuardNet {
            who: Fighter::Player(s.player_id),
            vertical,
            raised: s.guard_raised,
        };
        let m = Message::Swordplay(SwordplayMessage::Guards(vec![g]));
        let to: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(o, s)| **o != id && s.joined() && s.capabilities.has(WEAPONS_CLASH))
            .map(|(&o, _)| o)
            .collect();
        for o in to {
            self.send_reliable(o, &m, Channel::World);
        }
    }

    /// A client said its capabilities: with `weapons.clash` it is sent the raised guards there are
    /// now (players' and NPCs'), at least one message (empty when none is raised).
    pub(super) fn swordplay_hello(&mut self, id: SessionId) {
        let Some(s) = self.sessions.get(&id) else {
            return;
        };
        if !s.capabilities.has(WEAPONS_CLASH) {
            return;
        }
        let mut all: Vec<GuardNet> = self
            .sessions
            .iter()
            .filter(|(o, s)| **o != id && s.guard_raised)
            .map(|(_, s)| GuardNet {
                who: Fighter::Player(s.player_id),
                vertical: s.held_tool.as_ref().is_some_and(|(_, v)| *v),
                raised: true,
            })
            .collect();
        all.extend(self.creatures.guards().into_iter().filter(|g| g.2).map(
            |(c, vertical, raised)| GuardNet {
                who: Fighter::Creature(c),
                vertical,
                raised,
            },
        ));
        let parts: Vec<Vec<GuardNet>> = if all.is_empty() {
            vec![Vec::new()]
        } else {
            all.chunks(MAX_GUARDS_PER_MESSAGE)
                .map(<[_]>::to_vec)
                .collect()
        };
        for p in parts {
            let m = Message::Swordplay(SwordplayMessage::Guards(p));
            self.send_reliable(id, &m, Channel::World);
        }
    }

    /// A strike's outcome to the joined clients with `weapons.clash` whose player is within
    /// `hear_m` of it (an older client is never sent one).
    fn send_clash(&mut self, c: ClashNet, hear_m: f64) {
        let at = c.point.map(f64::from);
        let to: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.joined() && s.capabilities.has(WEAPONS_CLASH))
            .filter(|(_, s)| {
                s.handle.is_some_and(|h| {
                    let p = self.players.position(&self.physics, h).to_array();
                    let d: f64 = (0..3).map(|i| (p[i] - at[i]).powi(2)).sum();
                    d.sqrt() <= hear_m
                })
            })
            .map(|(&id, _)| id)
            .collect();
        let m = Message::Swordplay(SwordplayMessage::Clash(c));
        for id in to {
            self.send_reliable(id, &m, Channel::World);
        }
    }

    /// The NPCs' fights, a tick (after the creatures move): each reads the player it fights, its
    /// guard's changes go to the clients with `weapons.clash`, and its swings landing now are
    /// resolved against the player's block: an outcome sent, a block recoiling the NPC, a strike
    /// past the block hurting the player (WORK_QUEUE item 38).
    pub(super) fn step_npc_fights(&mut self) {
        if self
            .creatures
            .list
            .iter()
            .all(|c| c.foe_player.is_none() && !c.guard.is_some_and(|g| g.raised))
        {
            return;
        }
        let before = self.creatures.guards();
        let tick = self.tick;
        let mut views = std::collections::HashMap::new();
        for s in self.sessions.values().filter(|s| s.joined()) {
            let Some(h) = s.handle else {
                continue;
            };
            let p = self.players.position(&self.physics, h).to_array();
            let swing = s
                .weapon_swing
                .and_then(|w| Some((w.phase(tick)?, w.vertical, w.weapon)));
            views.insert(s.player_id, FoeView { position: p, swing });
        }
        let swings = self.creatures.step_fights(&|p| views.get(&p).copied());
        let after = self.creatures.guards();
        let changed: Vec<GuardNet> = after
            .iter()
            .filter(|g| !before.contains(g))
            .map(|&(c, vertical, raised)| GuardNet {
                who: Fighter::Creature(c),
                vertical,
                raised,
            })
            .collect();
        if !changed.is_empty() {
            let m = Message::Swordplay(SwordplayMessage::Guards(changed));
            let to: Vec<SessionId> = self
                .sessions
                .iter()
                .filter(|(_, s)| s.joined() && s.capabilities.has(WEAPONS_CLASH))
                .map(|(&id, _)| id)
                .collect();
            for id in to {
                self.send_reliable(id, &m, Channel::World);
            }
        }
        for sw in swings {
            self.npc_swing(sw);
        }
    }

    /// An NPC's swing at a player lands: out of its weapon's reach it misses; else it meets the
    /// player's raised block (facing it) by the blades' angle, or connects. Blocked: the NPC
    /// recoils; past the block: the player is hurt and pushed away ([`Self::npc_hit`]).
    fn npc_swing(&mut self, sw: NpcSwing) {
        let Some(w) = chop::tool(sw.weapon).and_then(|t| t.weapon.map(|w| (t, w))) else {
            return;
        };
        let (t, w) = w;
        let Some(id) = self.session_of_player(sw.player) else {
            return;
        };
        let Some(h) = self.sessions[&id].handle else {
            return;
        };
        let eye = self.eye_of(h);
        let f = FaceFrame::at_height(sw.eye[1]);
        let (me, them) = (f.point(sw.eye), f.point(eye));
        let d = (0..3)
            .map(|i| (me[i] - them[i]).powi(2))
            .sum::<f64>()
            .sqrt();
        let outcome = (d <= t.swing.reach_m + 0.5).then(|| {
            let yaw = f64::from(self.players.controller(h).input.look_yaw);
            match self.player_guard(sw.player) {
                Some((vertical, true)) if in_front(yaw, them, me, w.guard.front_deg) => {
                    let theirs = self.sessions[&id]
                        .held_tool
                        .as_ref()
                        .and_then(|(n, _)| chop::tool(n)?.weapon)
                        .map_or(w.guard, |x| x.guard);
                    (w.guard.clash(sw.vertical, &theirs, vertical), vertical)
                }
                g => (Clash::Connected, g.is_some_and(|g| g.0)),
            }
        });
        let mut hurt = 0;
        if let Some((k, defender_vertical)) = outcome {
            if k == Clash::Blocked {
                self.creatures.recoil(sw.npc, w.guard.recoil_s);
            } else {
                hurt = self.npc_hit(id, &w, w.guard.damage_share(k), them, me, f);
            }
            // Where the blades met: the NPC's arc toward the player's eye against the player's
            // raised blade when it clashed, else the player's body.
            let feet = self.feet_of(sw.player).unwrap_or(eye);
            let look = std::array::from_fn(|i| (eye[i] - sw.eye[i]) / d.max(1e-9));
            let path = dwell_world::swing::reaches(
                &mut self.world,
                sw.eye,
                look,
                sw.vertical,
                &dwell_world::swing::Arc::of(t),
            );
            let yaw = f64::from(self.players.controller(h).input.look_yaw);
            let theirs = (k != Clash::Connected)
                .then(|| self.sessions[&id].held_tool.as_ref())
                .flatten()
                .and_then(|(n, _)| chop::tool(n)?.weapon)
                .map(|x| raised_blade(x.guard.blade(defender_vertical), feet, yaw));
            let point = clash_point(sw.eye, &path, theirs, (feet, eye));
            self.send_clash(
                ClashNet {
                    striker: Fighter::Creature(sw.npc),
                    defender: Fighter::Player(sw.player),
                    weapon: t.name.to_string(),
                    outcome: k,
                    point: point.map(|v| v as f32),
                    striker_vertical: sw.vertical,
                    defender_vertical,
                },
                w.guard.clash.hear_m,
            );
        }
        self.npc_strike_log.push(NpcStrikeLog {
            npc: sw.npc,
            player_id: sw.player,
            vertical: sw.vertical,
            outcome: outcome.map(|o| o.0),
            damage: hurt,
        });
        if self.npc_strike_log.len() > STRIKE_LOG {
            self.npc_strike_log.remove(0);
        }
    }
}

impl Server {
    /// A strike past a player's block (an NPC's: WORK_QUEUE item 38): `share` of the weapon's
    /// damage through the damage rules (cause `Weapon`) and its push away from the striker at `from`
    /// (face-local, as the player's eye `at`, both on face `f`): `knockbackMps` along the ground and
    /// `liftMps` up. A mode without player damage (creative) is neither hurt nor pushed. The health
    /// taken.
    fn npc_hit(
        &mut self,
        id: SessionId,
        w: &chop::WeaponDef,
        share: f64,
        at: [f64; 3],
        from: [f64; 3],
        f: FaceFrame,
    ) -> i32 {
        if !self.settings.allows(Ability::Damage) {
            return 0;
        }
        let amount = (w.damage * share).round() as i32;
        let player_id = self.sessions[&id].player_id;
        let (dx, dz) = (at[0] - from[0], at[2] - from[2]);
        let len = dx.hypot(dz);
        let (ax, az) = if len > 1e-9 {
            (dx / len, dz / len)
        } else {
            (0.0, 0.0)
        };
        let v = f.vector([ax * w.knockback_mps, w.lift_mps, az * w.knockback_mps]);
        if v.iter().any(|x| *x != 0.0) {
            self.knockback(player_id, Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32));
        }
        let before = self.sessions[&id].health;
        self.damage(id, amount, DamageCause::Weapon);
        before - self.sessions.get(&id).map_or(before, |s| s.health)
    }
}

/// A stance's raised blade `b` (from the holder's feet, right / up / ahead) held by one standing at
/// `feet` (world) facing `yaw_deg` (0 faces +z): its grip and tip in the world. Face-local, crossed
/// through the face's frame: a body on face B is turned over, so its right is the frame's left.
fn raised_blade(b: chop::RaisedBlade, feet: [f64; 3], yaw_deg: f64) -> ([f64; 3], [f64; 3]) {
    let f = FaceFrame::at_height(feet[1]);
    let l = f.point(feet);
    let (s, c) = yaw_deg.to_radians().sin_cos();
    let right = [f.handed(-c), 0.0, f.handed(s)];
    let ahead = [s, 0.0, c];
    let at = |p: [f64; 3]| {
        f.point([
            l[0] + right[0] * p[0] + ahead[0] * p[2],
            l[1] + p[1],
            l[2] + right[2] * p[0] + ahead[2] * p[2],
        ])
    };
    (at(b.grip), at(b.tip))
}

/// A creature's body post standing at `feet` (world) `rows` tall: its feet and its top (world).
fn body_post(feet: [f64; 3], rows: i32) -> ([f64; 3], [f64; 3]) {
    let f = FaceFrame::at_height(feet[1]);
    let l = f.point(feet);
    (
        feet,
        f.point([l[0], l[1] + f64::from(rows) * dwell_world::CELL_H, l[2]]),
    )
}

/// The nearest points of the segments `a0`–`a1` and `b0`–`b1` (on each, in that order).
fn nearest_points(a0: [f64; 3], a1: [f64; 3], b0: [f64; 3], b1: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let sub = |x: [f64; 3], y: [f64; 3]| [x[0] - y[0], x[1] - y[1], x[2] - y[2]];
    let dot = |x: [f64; 3], y: [f64; 3]| x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
    let at = |o: [f64; 3], d: [f64; 3], t: f64| [o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t];
    let (d1, d2, r) = (sub(a1, a0), sub(b1, b0), sub(a0, b0));
    let (a, e, f) = (dot(d1, d1), dot(d2, d2), dot(d2, r));
    let eps = 1e-12;
    let (s, t) = if a <= eps && e <= eps {
        (0.0, 0.0)
    } else if a <= eps {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = dot(d1, r);
        if e <= eps {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = dot(d1, d2);
            let denom = a * e - b * b;
            let mut s = if denom > eps {
                ((b * f - c * e) / denom).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mut t = (b * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            }
            (s, t)
        }
    };
    (at(a0, d1, s), at(b0, d2, t))
}

/// The sub-steps between two arc steps a clash's point is looked for in (the arc's own steps are
/// ~9° apart: ~0.3 m at a sword's reach).
const CLASH_SUBSTEPS: u32 = 8;

/// Where a swing's blade met (BUGS.md #86): its head sweeps `path` (each step a segment from `eye`
/// along its direction out to its reach, world) and it met the raised blade `theirs` (grip, tip;
/// world), the middle of the two blades' nearest points where the sweep comes nearest it; with no
/// raised blade, the point of the striking blade nearest the defender's `body` (a segment, world).
fn clash_point(
    eye: [f64; 3],
    path: &[([f64; 3], f64)],
    theirs: Option<([f64; 3], [f64; 3])>,
    body: ([f64; 3], [f64; 3]),
) -> [f64; 3] {
    let (t0, t1) = theirs.unwrap_or(body);
    let mut best: Option<(f64, [f64; 3], [f64; 3])> = None;
    let mut consider = |d: [f64; 3], reach: f64| {
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-12);
        let head = std::array::from_fn(|i| eye[i] + d[i] / len * reach);
        let (p, q) = nearest_points(eye, head, t0, t1);
        let gap: f64 = (0..3).map(|i| (p[i] - q[i]).powi(2)).sum();
        if best.is_none_or(|b| gap < b.0) {
            best = Some((gap, p, q));
        }
    };
    for (i, &(d, r)) in path.iter().enumerate() {
        consider(d, r);
        if let Some(&(d2, r2)) = path.get(i + 1) {
            for k in 1..CLASH_SUBSTEPS {
                let u = f64::from(k) / f64::from(CLASH_SUBSTEPS);
                consider(
                    std::array::from_fn(|j| d[j] + (d2[j] - d[j]) * u),
                    r + (r2 - r) * u,
                );
            }
        }
    }
    match best {
        Some((_, p, q)) if theirs.is_some() => std::array::from_fn(|i| (p[i] + q[i]) / 2.0),
        Some((_, p, _)) => p,
        None => eye,
    }
}
