//! Stamina and oxygen on the server (docs/adr/0069-stamina-and-oxygen.md): the rules from the
//! world's mode, drowning damage, a swing's cost, the `Vitals` message to clients that said
//! `VitalsHello`, and the `vitals` table.

use dwell_physics::controller::{VITALS, Vitals, VitalsRules};
use dwell_protocol::modes::Ability;
use dwell_protocol::{DamageCause, Message, PlayerEvent, VitalsMessage, vitals_flags as vf};

use super::*;

/// A saved record: version 1, stamina and oxygen (f32), the exhausted flag.
const RECORD_VERSION: u8 = 1;

pub(super) fn record_of(v: &Vitals) -> Vec<u8> {
    let mut out = vec![RECORD_VERSION];
    out.extend_from_slice(&v.stamina.to_le_bytes());
    out.extend_from_slice(&v.oxygen.to_le_bytes());
    out.push(u8::from(v.exhausted));
    out
}

/// A saved record this build reads (`None`: another version, or damaged).
pub(super) fn vitals_of(b: &[u8]) -> Option<Vitals> {
    if b.len() != 10 || b[0] != RECORD_VERSION {
        return None;
    }
    let stamina = f32::from_le_bytes(b[1..5].try_into().ok()?);
    let oxygen = f32::from_le_bytes(b[5..9].try_into().ok()?);
    if !stamina.is_finite() || !oxygen.is_finite() || b[9] > 1 {
        return None;
    }
    Some(Vitals {
        stamina: stamina.clamp(0.0, VITALS.max_stamina),
        oxygen: oxygen.clamp(0.0, VITALS.oxygen_seconds),
        exhausted: b[9] == 1,
        rest_ticks: 0,
    })
}

impl Server {
    /// The world's rules, from its mode's `stamina` and `oxygen` abilities.
    pub fn vitals_rules(&self) -> VitalsRules {
        VitalsRules {
            stamina: self.settings.allows(Ability::Stamina),
            oxygen: self.settings.allows(Ability::Oxygen),
        }
    }

    /// A player's vitals now (rested while dead).
    pub fn vitals_of_session(&self, id: SessionId) -> Vitals {
        self.sessions
            .get(&id)
            .and_then(|s| s.handle)
            .map_or_else(Vitals::default, |h| self.players.controller(h).vitals)
    }

    /// The saved vitals record of a session, for the save.
    pub(super) fn vitals_record(&self, s: &Session) -> Vec<u8> {
        let v = s
            .handle
            .map_or_else(Vitals::default, |h| self.players.controller(h).vitals);
        record_of(&v)
    }

    /// A returning player's saved vitals, at join (after its spawn).
    pub(super) fn load_vitals(&mut self, id: SessionId, key: &[u8; 32]) {
        let saved = self
            .config
            .store
            .as_ref()
            .and_then(|st| st.db().load_vitals(key))
            .and_then(|b| vitals_of(&b));
        if let (Some(v), Some(h)) = (saved, self.sessions[&id].handle) {
            self.players.controller_mut(h).vitals = v;
        }
    }

    /// Drowning (ADR 0069 §6): with no air left, `drownDamage` every `drownIntervalSeconds`.
    pub(super) fn drown(&mut self, id: SessionId) {
        let Some(h) = self.sessions[&id].handle else {
            return;
        };
        let out_of_air = self.vitals_rules().oxygen && self.players.controller(h).vitals.oxygen <= 0.0;
        let interval = (VITALS.drown_interval_seconds * SIM_HZ as f32).round().max(1.0) as u32;
        let s = self.sessions.get_mut(&id).expect("session");
        if !out_of_air {
            s.drown_ticks = 0;
            return;
        }
        s.drown_ticks += 1;
        if s.drown_ticks >= interval {
            s.drown_ticks = 0;
            self.damage(id, i32::from(VITALS.drown_damage), DamageCause::Drown);
        }
    }

    /// A tool's swing (ADR 0069 §5): refused while exhausted; otherwise it draws the tool's
    /// `staminaCost`. Swings still waiting on the last one's recovery spend nothing here (the
    /// chopping refuses them).
    pub(super) fn swing_stamina(&mut self, id: SessionId, tool: &str) -> bool {
        let rules = self.vitals_rules();
        let s = &self.sessions[&id];
        let Some(h) = s.handle else {
            return true;
        };
        if !rules.stamina || self.tick < s.chop_ready {
            return true;
        }
        let vitals = &mut self.players.controller_mut(h).vitals;
        if !vitals.can_exert(rules) {
            return false;
        }
        let cost = dwell_protocol::chopping::tool(tool).map_or(0.0, |t| t.swing.stamina_cost);
        vitals.spend(cost as f32);
        true
    }

    /// `Vitals` to each client that said hello, with its snapshot, while the mode has them.
    pub(super) fn send_vitals(&mut self) {
        let rules = self.vitals_rules();
        if !rules.any() {
            return;
        }
        let flags = if rules.stamina { vf::STAMINA } else { 0 } | if rules.oxygen { vf::OXYGEN } else { 0 };
        let out: Vec<(SessionId, Message)> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.joined() && s.vitals_hello)
            .filter_map(|(&id, s)| {
                let v = self.players.controller(s.handle?).vitals;
                Some((
                    id,
                    Message::Vitals(VitalsMessage {
                        seq: s.last_processed_seq,
                        stamina: v.stamina,
                        oxygen: v.oxygen,
                        flags: flags | if v.exhausted { vf::EXHAUSTED } else { 0 },
                    }),
                ))
            })
            .collect();
        for (id, m) in out {
            self.send_datagram(id, &m);
        }
    }

    /// A damage or death event to every joined client: one with the `Drown` cause goes as `Fall`
    /// to those that never said `VitalsHello` (an older client refuses a cause it doesn't know).
    pub(super) fn broadcast_event(&mut self, e: PlayerEvent) {
        if e.cause != DamageCause::Drown {
            self.broadcast_world(&Message::PlayerEvent(e));
            return;
        }
        let new = encode(&Message::PlayerEvent(e.clone()));
        let old = encode(&Message::PlayerEvent(PlayerEvent {
            cause: DamageCause::Fall,
            ..e
        }));
        for (&id, s) in &self.sessions {
            if s.joined() {
                self.outbox.push(Outgoing {
                    session: id,
                    kind: OutKind::Reliable(Channel::World),
                    bytes: if s.vitals_hello { new.clone() } else { old.clone() },
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip_and_others_are_refused() {
        let v = Vitals {
            stamina: 12.5,
            oxygen: 3.0,
            exhausted: true,
            rest_ticks: 0,
        };
        assert_eq!(vitals_of(&record_of(&v)), Some(v));
        let mut b = record_of(&v);
        b[0] = 2;
        assert_eq!(vitals_of(&b), None, "a newer version");
        assert_eq!(vitals_of(&record_of(&v)[..9]), None);
        let mut b = record_of(&v);
        b[1..5].copy_from_slice(&f32::NAN.to_le_bytes());
        assert_eq!(vitals_of(&b), None);
    }
}
