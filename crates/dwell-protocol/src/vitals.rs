//! Stamina and oxygen (docs/adr/0069-stamina-and-oxygen.md): the tunables as data
//! (`shared/vitals/vitals.json`, generated into `vitals.gen.rs` by `shared/vitals/gen.mjs`) and the
//! one rule that steps them a tick, used by the server's controller and the client's prediction
//! alike, so both agree. The activities that spend stamina are this module's small fixed vocabulary.

#[rustfmt::skip]
mod table {
    include!("vitals.gen.rs");
}

pub use table::{ACTIVITY_NAMES, VITALS};

/// What spends stamina (`stamina.drains` in the data), at most one a tick. New activities are
/// code: a variant here, its name in `shared/vitals/gen.mjs`'s `ACTIVITIES`, and where the
/// controller recognises it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    /// Running on the ground (the run input held and the controller in its running state).
    Sprint,
    /// In the water with any move input (swimming somewhere).
    Swim,
    /// In the water with none: treading water, which is what holds a swimmer up.
    Tread,
}

impl Activity {
    pub const ALL: [Activity; 3] = [Activity::Sprint, Activity::Swim, Activity::Tread];
    /// The data's name (`shared/vitals/gen.mjs` `ACTIVITIES`).
    pub const fn name(self) -> &'static str {
        match self {
            Activity::Sprint => "sprint",
            Activity::Swim => "swim",
            Activity::Tread => "tread",
        }
    }
}

/// The tunables (ARCHITECTURE.md §7.4; ADR 0069's table).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VitalsDef {
    pub max_stamina: f32,
    /// Points a second each activity spends.
    pub drains: [(Activity, f32); 3],
    pub regen_per_second: f32,
    pub regen_delay_seconds: f32,
    /// The share of `max_stamina` an exhausted player needs back.
    pub recover_fraction: f32,
    /// m/s an exhausted swimmer sinks at.
    pub exhausted_sink_speed: f32,
    pub oxygen_seconds: f32,
    pub oxygen_refill_per_second: f32,
    /// The capsule's submerged share at which the head is under water.
    pub head_fraction: f32,
    pub drown_damage: u8,
    pub drown_interval_seconds: f32,
}

impl VitalsDef {
    pub fn drain(&self, a: Activity) -> f32 {
        self.drains
            .iter()
            .find(|(b, _)| *b == a)
            .map_or(0.0, |&(_, r)| r)
    }
    /// The stamina an exhausted player needs back.
    pub fn recover_at(&self) -> f32 {
        self.max_stamina * self.recover_fraction
    }
}

/// Which parts a world's mode switches on (the `stamina` and `oxygen` abilities, ADR 0065).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VitalsRules {
    pub stamina: bool,
    pub oxygen: bool,
}

impl VitalsRules {
    pub const fn any(self) -> bool {
        self.stamina || self.oxygen
    }
}

/// A player's vitals: a plain value, part of the controller's state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vitals {
    pub stamina: f32,
    /// Seconds of air left.
    pub oxygen: f32,
    /// Reached 0 stamina and not yet back to the recover share.
    pub exhausted: bool,
    /// Ticks since stamina was last spent (capped).
    pub rest_ticks: u16,
}

impl Default for Vitals {
    fn default() -> Self {
        Self::rested(&VITALS)
    }
}

impl Vitals {
    pub fn rested(d: &VitalsDef) -> Self {
        Self {
            stamina: d.max_stamina,
            oxygen: d.oxygen_seconds,
            exhausted: false,
            rest_ticks: u16::MAX,
        }
    }

    /// Whether stamina-heavy actions (sprinting, treading water, a tool's swing) are allowed.
    pub fn can_exert(&self, rules: VitalsRules) -> bool {
        !rules.stamina || !self.exhausted
    }

    /// Spends `amount` points now (a swing's cost), exhausting at 0.
    pub fn spend(&mut self, amount: f32) {
        if amount <= 0.0 {
            return;
        }
        self.stamina = (self.stamina - amount).max(0.0);
        self.rest_ticks = 0;
        if self.stamina <= 0.0 {
            self.exhausted = true;
        }
    }

    /// One tick of `dt` seconds: `activity` spent (if any) and whether the head is under water.
    /// A part the rules switch off is held rested.
    pub fn step(
        &mut self,
        d: &VitalsDef,
        rules: VitalsRules,
        activity: Option<Activity>,
        head_under: bool,
        dt: f32,
    ) {
        if rules.stamina {
            let rate = activity.map_or(0.0, |a| d.drain(a));
            if rate > 0.0 {
                self.spend(rate * dt);
            } else {
                self.rest_ticks = self.rest_ticks.saturating_add(1);
                if f32::from(self.rest_ticks) * dt > d.regen_delay_seconds {
                    self.stamina = (self.stamina + d.regen_per_second * dt).min(d.max_stamina);
                }
            }
            if self.exhausted && self.stamina >= d.recover_at() {
                self.exhausted = false;
            }
        } else {
            self.stamina = d.max_stamina;
            self.exhausted = false;
            self.rest_ticks = u16::MAX;
        }
        if rules.oxygen && head_under {
            self.oxygen = (self.oxygen - dt).max(0.0);
        } else if rules.oxygen {
            self.oxygen = (self.oxygen + d.oxygen_refill_per_second * dt).min(d.oxygen_seconds);
        } else {
            self.oxygen = d.oxygen_seconds;
        }
    }

    /// Whether the server's value differs enough from a prediction to correct it (ADR 0069 §7).
    pub fn differs(&self, other: &Vitals) -> bool {
        self.exhausted != other.exhausted
            || (self.stamina - other.stamina).abs() > 0.5
            || (self.oxygen - other.oxygen).abs() > 0.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;
    const ALL: VitalsRules = VitalsRules {
        stamina: true,
        oxygen: true,
    };

    fn run(v: &mut Vitals, a: Option<Activity>, under: bool, seconds: f32) {
        for _ in 0..(seconds / DT).round() as u32 {
            v.step(&VITALS, ALL, a, under, DT);
        }
    }

    #[test]
    fn the_generated_vocabulary_matches_activity() {
        let names: Vec<&str> = Activity::ALL.iter().map(|a| a.name()).collect();
        assert_eq!(names, ACTIVITY_NAMES);
        for (i, (a, _)) in VITALS.drains.iter().enumerate() {
            assert_eq!(*a, Activity::ALL[i]);
        }
    }

    #[test]
    fn each_activity_drains_at_its_rate_and_rest_regains_after_the_delay() {
        for a in Activity::ALL {
            let mut v = Vitals::default();
            run(&mut v, Some(a), false, 1.0);
            let spent = VITALS.max_stamina - v.stamina;
            assert!(
                (spent - VITALS.drain(a)).abs() < 0.05,
                "{a:?} spent {spent}"
            );
        }
        let mut v = Vitals::default();
        run(&mut v, Some(Activity::Sprint), false, 2.0);
        let low = v.stamina;
        // Within the delay nothing comes back; a second after it, the regen rate.
        run(&mut v, None, false, VITALS.regen_delay_seconds);
        assert_eq!(v.stamina, low);
        run(&mut v, None, false, 1.0);
        let back = v.stamina - low;
        assert!(
            (back - VITALS.regen_per_second).abs() < 0.5,
            "regained {back}"
        );
    }

    #[test]
    fn exhaustion_follows_only_from_zero_and_ends_at_the_recover_share() {
        let d = &VITALS;
        // Rested, 25 % → 24 % is fine: nothing locks.
        let mut v = Vitals {
            stamina: d.recover_at() + 0.1,
            ..Vitals::default()
        };
        v.spend(1.0);
        assert!(v.stamina < d.recover_at() && !v.exhausted && v.can_exert(ALL));
        // Run it to 0: exhausted.
        run(&mut v, Some(Activity::Sprint), false, 5.0);
        assert_eq!(v.stamina, 0.0);
        assert!(v.exhausted && !v.can_exert(ALL));
        // Regaining stays exhausted until the recover share, then it lifts.
        while v.stamina < d.recover_at() - 1.0 {
            v.step(d, ALL, None, false, DT);
            assert!(v.exhausted);
        }
        run(&mut v, None, false, 0.2);
        assert!(v.stamina >= d.recover_at() && !v.exhausted);
        // Lifted, it may drain below the share again without locking.
        v.spend(8.0);
        assert!(v.stamina < d.recover_at() && !v.exhausted);
    }

    #[test]
    fn oxygen_drains_with_the_head_under_and_refills_out() {
        let mut v = Vitals::default();
        run(&mut v, None, true, 5.0);
        assert!((v.oxygen - (VITALS.oxygen_seconds - 5.0)).abs() < 0.05);
        run(&mut v, None, true, VITALS.oxygen_seconds);
        assert_eq!(v.oxygen, 0.0);
        run(&mut v, None, false, 1.0);
        assert!((v.oxygen - VITALS.oxygen_refill_per_second).abs() < 0.05);
    }

    #[test]
    fn the_messages_round_trip_and_the_drown_cause_is_known() {
        use crate::{DamageCause, Message, VitalsMessage, vitals_flags as vf, welcome_flags as wf};
        let m = Message::Vitals(VitalsMessage {
            seq: 77,
            stamina: 24.5,
            oxygen: 3.25,
            flags: vf::EXHAUSTED | vf::STAMINA,
        });
        for m in [Message::VitalsHello, m] {
            let b = m.encode();
            assert_eq!(Message::decode(&b), Some(m.clone()));
            if b.len() > 1 {
                assert_eq!(Message::decode(&b[..b.len() - 1]), None);
            }
        }
        let mut b = Message::VitalsHello.encode();
        b.push(0);
        assert_eq!(Message::decode(&b), None, "trailing bytes");
        assert_eq!(DamageCause::from_u8(4), Some(DamageCause::Drown));
        assert_eq!(wf::VITALS, 128);
    }

    #[test]
    fn without_the_rules_nothing_drains() {
        let mut v = Vitals::default();
        for _ in 0..600 {
            v.step(
                &VITALS,
                VitalsRules::default(),
                Some(Activity::Sprint),
                true,
                DT,
            );
        }
        assert_eq!(v, Vitals::default());
        assert!(v.can_exert(VitalsRules::default()));
    }
}
