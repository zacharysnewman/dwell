//! Defeat (docs/CHARACTERS.md §11.13, WORK_QUEUE item 55; owner, 2026-11-13: "Humans won't die in
//! this game (general design rule)"): at 0 health a human — a player or an NPC — kneels where it
//! fell for `kneel_s`, the view fades to black over `fade_s`, and it stands up at the resurrection
//! shrine nearest where it fell (docs/SHRINES.md), the view fading back in over `fade_s`. The
//! tunables are data (`shared/defeat/defeat.json`, `shared/defeat/gen.mjs` writes `defeat.gen.rs`):
//! the server's timing, the client's camera, fade and clips read them here, so they agree.

/// The defeated player's camera: pulled out to third person over `ease_s`, `back_m` behind the body
/// along its facing and `up_m` over its feet, looking `pitch_deg` (down negative).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DefeatCamera {
    pub back_m: f64,
    pub up_m: f64,
    pub pitch_deg: f64,
    pub ease_s: f64,
}

/// One character's defeat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Defeat {
    /// From the defeat until the fade starts (s).
    pub kneel_s: f64,
    /// The fade to black, and back in after the stand (s).
    pub fade_s: f64,
    pub camera: DefeatCamera,
}

/// The roles defeat plays (the characters' clips, shared/characters/characters.json).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DefeatRoles {
    pub kneel: &'static str,
    pub kneel_armed: &'static str,
    pub stand: &'static str,
    pub stand_armed: &'static str,
}

include!("defeat.gen.rs");

/// The characters.json name of the player (its override in `CHARACTERS`, if any).
pub const PLAYER: &str = "dwell:player";

/// `name`'s defeat (a characters.json name or an NPC kind), the defaults when it has no override.
pub fn of(name: &str) -> &'static Defeat {
    CHARACTERS
        .iter()
        .find(|(n, _)| *n == name)
        .map_or(&DEFAULT, |(_, d)| d)
}

impl Defeat {
    /// From the defeat to standing up at the shrine (s): the kneel, then the fade.
    pub fn down_s(&self) -> f64 {
        self.kneel_s + self.fade_s
    }

    /// How black the defeated player's view is `since_s` after the defeat (0 clear, 1 black).
    pub fn fade_out(&self, since_s: f64) -> f64 {
        ramp(since_s - self.kneel_s, self.fade_s)
    }

    /// How black the view is `since_s` after standing up at the shrine (1 → 0 over the fade).
    pub fn fade_in(&self, since_s: f64) -> f64 {
        1.0 - ramp(since_s, self.fade_s)
    }
}

fn ramp(t: f64, over: f64) -> f64 {
    if over <= 0.0 {
        return if t >= 0.0 { 1.0 } else { 0.0 };
    }
    let u = (t / over).clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_and_an_override_come_from_the_data() {
        assert!(DEFAULT.kneel_s > 0.0 && DEFAULT.fade_s >= 0.0);
        assert_eq!(of("no:such").kneel_s, DEFAULT.kneel_s);
        // shared/defeat/defeat.json: the villager kneels longer, the rest from the defaults.
        let v = of("dwell:villager");
        assert!(v.kneel_s > DEFAULT.kneel_s);
        assert_eq!((v.fade_s, v.camera), (DEFAULT.fade_s, DEFAULT.camera));
    }

    #[test]
    fn the_view_fades_out_after_the_kneel_and_back_in_after_the_stand() {
        let d = &DEFAULT;
        assert_eq!(d.fade_out(0.0), 0.0);
        assert_eq!(d.fade_out(d.kneel_s), 0.0);
        assert_eq!(d.fade_out(d.down_s()), 1.0);
        assert_eq!(d.fade_in(0.0), 1.0);
        assert_eq!(d.fade_in(d.fade_s), 0.0);
        assert!(d.fade_out(d.kneel_s + d.fade_s / 2.0) > 0.0);
    }
}
