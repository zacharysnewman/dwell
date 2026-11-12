//! Swordplay on the client (WORK_QUEUE item 35, round 2; the `weapons.clash` capability): the
//! player's block input and a clash's feedback. Draw- and input-only: the server decides every
//! outcome (`dwell-sim` `server/weapons.rs`).
//!
//! **The block.** With a weapon held, Place (right-click, the controller's Place, the touch Place
//! button: their bindings are the existing data, `shared/input/gamepad.json` and the touch layout)
//! held past the weapon's `guard.raiseAfterS` raises the block in the current stance; let go, it
//! drops. Let go sooner it is a tap, turning the stance (the swing's plane) as Place always did with
//! a tool. A swing drops the block while it runs (attacking drops block); still held, the block is
//! back once the swing ends. The raised pose per stance is the weapon's data
//! (`guard.stances.*.raisedPose`, a first-person clip; [`GuardInput::pose`]).
//!
//! **A clash.** A `Swordplay` clash is heard (the weapon's `guard.clash.sound`, at the glance share
//! for a glance), sparks fly (its `clash.sparks`, drawn as chips), and when this player struck and
//! was blocked its view recoils (the swing's camera kick at `bounce`, over the guard's `recoilS`).
//! A connect shows none of these: what was struck shows its own reaction.

use dwell_protocol::chopping::{CameraOffset, Clash, GuardDef, ToolDef};
use dwell_protocol::{ClashNet, Fighter};

/// The block's input state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GuardInput {
    /// When Place went down with a weapon held, while it is still down.
    pressed_ms: Option<f64>,
    /// The block is held raised (it guards when not mid-swing: [`Self::blocking`]).
    pub raised: bool,
    /// What the server was last told (`ChopAction::Guard`).
    sent: bool,
}

/// What letting go of Place did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Release {
    /// A tap: turn the stance (the swing's plane), as Place does with any tool.
    Tap,
    /// The raised block drops.
    Lowered,
    /// Place wasn't down.
    Nothing,
}

impl GuardInput {
    /// Place went down with a weapon held.
    pub fn press(&mut self, now_ms: f64) {
        if self.pressed_ms.is_none() {
            self.pressed_ms = Some(now_ms);
        }
    }

    /// Whether Place is down (held for the block, or not yet a tap).
    pub fn pressed(&self) -> bool {
        self.pressed_ms.is_some()
    }

    /// A frame: Place held past `raiseAfterS` raises the block.
    pub fn frame(&mut self, guard: &GuardDef, now_ms: f64) {
        if let Some(t) = self.pressed_ms
            && now_ms - t >= guard.raise_after_s * 1000.0
        {
            self.raised = true;
        }
    }

    /// Place let go: a tap (before the block rose) or the block lowered.
    pub fn release(&mut self, guard: &GuardDef, now_ms: f64) -> Release {
        let Some(t) = self.pressed_ms.take() else {
            return Release::Nothing;
        };
        if self.raised {
            self.raised = false;
            Release::Lowered
        } else if now_ms - t < guard.raise_after_s * 1000.0 {
            Release::Tap
        } else {
            Release::Lowered
        }
    }

    /// The weapon put away (or the player died): no block, Place forgotten.
    pub fn clear(&mut self) {
        self.pressed_ms = None;
        self.raised = false;
    }

    /// Whether the block guards now: raised and not mid-swing (attacking drops the block).
    pub fn blocking(&self, swinging: bool) -> bool {
        self.raised && !swinging
    }

    /// The raised state to tell the server, once each time it changes.
    pub fn take_change(&mut self) -> Option<bool> {
        (self.raised != self.sent).then(|| {
            self.sent = self.raised;
            self.raised
        })
    }

    /// The first-person pose the arms show: the stance's raised pose while blocking (the weapon's
    /// data), else None (the hold, or the swing's clip).
    pub fn pose(&self, guard: &GuardDef, vertical: bool, swinging: bool) -> Option<&'static str> {
        self.blocking(swinging)
            .then(|| guard.raised_pose(vertical))
            .flatten()
    }
}

/// What a clash shows on this client.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClashShow {
    /// The sound event (shared/sounds) and its volume.
    pub sound: Option<(&'static str, f64)>,
    /// How many of the weapon's sparks, if it has any.
    pub sparks: u32,
    /// This player struck and was blocked: its view recoils.
    pub recoil: bool,
}

/// What clash `c` shows to the client of player `me`, by the striker's weapon's `guard.clash` data.
pub fn show(c: &ClashNet, me: u16) -> ClashShow {
    let none = ClashShow {
        sound: None,
        sparks: 0,
        recoil: false,
    };
    let Some(w) = dwell_protocol::chopping::tool(&c.weapon).and_then(|t| t.weapon) else {
        return none;
    };
    let look = w.guard.clash;
    let share = look.share(c.outcome);
    if share <= 0.0 {
        return none;
    }
    ClashShow {
        sound: look.sound.map(|s| (s, share)),
        sparks: look
            .sparks
            .map_or(0, |s| (f64::from(s.count) * share).round() as u32),
        recoil: c.outcome == Clash::Blocked && c.striker == Fighter::Player(me),
    }
}

/// The view's recoil `t` s after this player's strike was blocked: the swing's camera kick (its
/// stance's) at its `bounce`, dying away over the guard's `recoilS`; zero after.
pub fn recoil_camera(tool: &ToolDef, vertical: bool, t: f64) -> CameraOffset {
    let Some(w) = tool.weapon else {
        return CameraOffset::ZERO;
    };
    let len = w.guard.recoil_s.max(1e-6);
    if !(0.0..len).contains(&t) {
        return CameraOffset::ZERO;
    }
    let kick = tool.swing.camera(vertical).kick;
    let u = t / len;
    kick.offset.scaled(-kick.bounce * (1.0 - u) * (1.0 - u))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_protocol::chopping::tool;

    fn sword() -> &'static ToolDef {
        tool("dwell:sword").expect("the sword")
    }

    fn guard() -> GuardDef {
        sword().weapon.unwrap().guard
    }

    #[test]
    fn holding_place_raises_the_block_and_letting_go_drops_it() {
        let g = guard();
        let mut i = GuardInput::default();
        i.press(0.0);
        i.frame(&g, 50.0);
        assert!(!i.raised, "not before raiseAfterS");
        i.frame(&g, g.raise_after_s * 1000.0 + 1.0);
        assert!(i.raised);
        assert_eq!(i.take_change(), Some(true), "told once");
        assert_eq!(i.take_change(), None);
        assert_eq!(i.release(&g, 900.0), Release::Lowered);
        assert!(!i.raised);
        assert_eq!(i.take_change(), Some(false));
    }

    #[test]
    fn a_tap_turns_the_stance_and_raises_nothing() {
        let g = guard();
        let mut i = GuardInput::default();
        i.press(0.0);
        i.frame(&g, 40.0);
        assert_eq!(i.release(&g, 60.0), Release::Tap);
        assert!(!i.raised);
        assert_eq!(i.take_change(), None, "the server is told nothing");
        assert_eq!(i.release(&g, 80.0), Release::Nothing);
    }

    #[test]
    fn attacking_drops_the_block_for_the_swing_and_the_pose_follows_the_stance() {
        let g = guard();
        let mut i = GuardInput::default();
        i.press(0.0);
        i.frame(&g, 1000.0);
        assert!(i.blocking(false));
        assert!(!i.blocking(true), "a swing drops the block");
        assert_eq!(i.pose(&g, false, false), Some("fp_guard_level"));
        assert_eq!(i.pose(&g, true, false), Some("fp_guard_vertical"));
        assert_eq!(i.pose(&g, true, true), None, "mid-swing: the swing's clip");
        // Still held after the swing: the block is back.
        assert!(i.blocking(false));
        i.clear();
        assert!(!i.raised && !i.pressed());
    }

    fn clash(outcome: Clash, striker: Fighter) -> ClashNet {
        ClashNet {
            striker,
            defender: Fighter::Creature(4),
            weapon: "dwell:sword".into(),
            outcome,
            point: [0.0, 10.0, 0.0],
            striker_vertical: false,
            defender_vertical: true,
        }
    }

    #[test]
    fn a_block_clanks_sparks_and_recoils_its_striker_a_glance_less_a_connect_nothing() {
        let look = guard().clash;
        let b = show(&clash(Clash::Blocked, Fighter::Player(2)), 2);
        assert_eq!(b.sound, Some((look.sound.unwrap(), 1.0)));
        assert_eq!(b.sparks, look.sparks.unwrap().count);
        assert!(b.recoil);
        // Another's block: heard and seen, no recoil here.
        assert!(!show(&clash(Clash::Blocked, Fighter::Player(3)), 2).recoil);
        assert!(!show(&clash(Clash::Blocked, Fighter::Creature(9)), 2).recoil);
        let g = show(&clash(Clash::Glanced, Fighter::Player(2)), 2);
        assert_eq!(g.sound.unwrap().1, look.glance_share);
        assert!(g.sparks < b.sparks && g.sparks > 0);
        assert!(!g.recoil);
        let c = show(&clash(Clash::Connected, Fighter::Player(2)), 2);
        assert_eq!((c.sound, c.sparks, c.recoil), (None, 0, false));
        // A weapon this build doesn't know shows nothing.
        let mut x = clash(Clash::Blocked, Fighter::Player(2));
        x.weapon = "mymod:axe".into();
        assert_eq!(show(&x, 2).sparks, 0);
    }

    #[test]
    fn the_recoil_kicks_back_and_dies_away_over_recoil_s() {
        let t = sword();
        let r = t.weapon.unwrap().guard.recoil_s;
        let start = recoil_camera(t, false, 0.0).size();
        assert!(start > 0.0);
        assert!(recoil_camera(t, false, r * 0.5).size() < start);
        assert_eq!(recoil_camera(t, false, r), CameraOffset::ZERO);
        assert_eq!(recoil_camera(t, false, -0.1), CameraOffset::ZERO);
        assert!(
            recoil_camera(t, true, 0.0).size() > 0.0,
            "the vertical stance's kick"
        );
    }
}
