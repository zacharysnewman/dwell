//! Hold to keep swinging (WORK_QUEUE item 17; owner, 2026-11-12: "I wanted you to be able to hold
//! the key down for tools like the axe."): while the use (Break) button is held with a tool whose
//! data says `repeatOnHold` (shared/tools: the axes and the sword in `tools`, the pickaxes and
//! shovels in `breaking.tools`), the game swings it again at the tool's rate until the button is
//! let go. Each repeat is a whole new swing, aimed and swept at the moment it starts (what is
//! targeted then), sent to the server as a click's would be: others see each swing by its swing
//! count, and a tool wears only on a hit (the server's rule, unchanged).
//!
//! The mouse, the controller's Break and a still touch in Break mode all hold it
//! (`Game::edit_held` / `Game::release_use`).

pub use dwell_game::build_drag::Holder;

fn slot(h: Holder) -> usize {
    match h {
        Holder::Mouse => 0,
        Holder::Gamepad => 1,
        Holder::Touch => 2,
    }
}

/// How a held tool repeats.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Repeat {
    /// One swing per press.
    No,
    /// A chopping tool (`tools[].repeatOnHold`): the next swing as soon as the last is over
    /// (`Chopping::ready`: the swing's whole length and the server's `swingSeconds`).
    WhenReady,
    /// A break tool (`breaking.tools[].repeatS`): every `s` seconds from the last swing's start.
    Every(f64),
}

impl Repeat {
    /// The repeat of a chopping tool, by its data.
    pub fn of_tool(t: &dwell_protocol::chopping::ToolDef) -> Self {
        if t.repeat_on_hold {
            Repeat::WhenReady
        } else {
            Repeat::No
        }
    }

    /// The repeat of a break with `item` held (a pickaxe, a shovel), by its data.
    pub fn of_break(item: Option<&str>) -> Self {
        item.and_then(|i| {
            dwell_protocol::chopping::BREAK_TOOLS
                .iter()
                .find(|t| t.item == i)
        })
        .and_then(|t| t.repeat_s)
        .map_or(Repeat::No, Repeat::Every)
    }
}

/// The use button's hold, by what holds it (the mouse, the controller, a touch): each lets go of
/// its own (the controller's Break, read every frame, never ends the mouse's).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeldUse {
    held: [bool; 3],
    /// When the last swing (pressed or repeated) started (ms).
    last_ms: Option<f64>,
}

impl HeldUse {
    /// The use button went down, held by `by`.
    pub fn press(&mut self, by: Holder) {
        self.held[slot(by)] = true;
    }

    /// `by` let go (or play stopped): no more repeats unless another still holds it; the swing
    /// under way plays out.
    pub fn release(&mut self, by: Holder) {
        self.held[slot(by)] = false;
    }

    pub fn held(&self) -> bool {
        self.held.iter().any(|&h| h)
    }

    /// A swing started at `now_ms` (the press's, or a repeat's).
    pub fn started(&mut self, now_ms: f64) {
        self.last_ms = Some(now_ms);
    }

    /// Whether to swing again at `now_ms`: held, the tool repeats, and (`ready`) a new swing may
    /// start (the last is over; a break tool's: its contact has landed).
    pub fn due(&self, repeat: Repeat, ready: bool, now_ms: f64) -> bool {
        if !self.held() || !ready {
            return false;
        }
        match repeat {
            Repeat::No => false,
            Repeat::WhenReady => true,
            Repeat::Every(s) => self.last_ms.is_none_or(|l| now_ms - l >= s * 1000.0 - 1e-6),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chop::{Chopping, Impact};
    use dwell_protocol::chopping::{BREAK_TOOLS, ToolDef, tool};

    fn axe() -> &'static ToolDef {
        tool("dwell:stone_axe").unwrap()
    }

    /// A hold of the axe for `hold_s` at 60 fps (its path meeting a wood at the arc's middle step
    /// when `hit`), played as the game does each frame (`Game::repeat_held_use`, then the swing
    /// run's impact): the swings started and the contacts landed, by swing.
    fn hold_axe(hold_s: f64, hit: bool, run_s: f64) -> (usize, Vec<usize>) {
        let t = axe();
        let mut chop = Chopping::default();
        chop.on = true;
        chop.sweep = true;
        chop.vertical = true;
        let mut held = HeldUse::default();
        let impact = hit.then_some(Impact {
            cell: [0, 0, 0],
            face: 0,
            point: [0.0; 3],
            step: t.swing.samples / 2,
            wood: true,
        });
        let start = |chop: &mut Chopping, held: &mut HeldUse, now: f64| {
            if chop.start(t, [0.0, 0.0, 1.0], impact, now).is_some() {
                held.started(now);
                true
            } else {
                false
            }
        };
        let (mut swings, mut contacts) = (0, Vec::new());
        // The press: a swing at once.
        held.press(Holder::Mouse);
        assert!(start(&mut chop, &mut held, 0.0));
        swings += 1;
        contacts.push(0);
        let dt = 1000.0 / 60.0;
        let mut now = 0.0;
        while now < run_s * 1000.0 {
            now += dt;
            if now >= hold_s * 1000.0 {
                held.release(Holder::Mouse);
            }
            let ready = chop.ready(t, now);
            if held.due(Repeat::of_tool(t), ready, now) && start(&mut chop, &mut held, now) {
                swings += 1;
                contacts.push(0);
            }
            if let Some(run) = &mut chop.run
                && run.take_impact(now).is_some()
            {
                *contacts.last_mut().unwrap() += 1;
            }
        }
        (swings, contacts)
    }

    #[test]
    fn the_axe_pick_shovel_and_sword_repeat_by_their_data() {
        assert_eq!(Repeat::of_tool(axe()), Repeat::WhenReady);
        assert_eq!(
            Repeat::of_tool(tool("dwell:sword").unwrap()),
            Repeat::WhenReady
        );
        for p in [
            "dwell:stone_pickaxe",
            "dwell:metal_pickaxe",
            "dwell:wooden_shovel",
            "dwell:metal_shovel",
        ] {
            assert!(
                matches!(Repeat::of_break(Some(p)), Repeat::Every(s) if s > 0.0),
                "{p}"
            );
        }
        // A hand, a block, an axe's break-table entry: one per press.
        assert_eq!(Repeat::of_break(None), Repeat::No);
        assert_eq!(Repeat::of_break(Some("dwell:stone")), Repeat::No);
        assert_eq!(Repeat::of_break(Some("dwell:stone_axe")), Repeat::No);
        // A tool added by data alone (shared/tools/fixtures: the stick club) repeats as its data says.
        let stick = dwell_protocol::chopping::fixture::TOOLS
            .iter()
            .find(|t| t.name == "dwell:stick")
            .unwrap();
        assert_eq!(Repeat::of_tool(stick), Repeat::WhenReady);
        let mut no = *stick;
        no.repeat_on_hold = false;
        assert_eq!(Repeat::of_tool(&no), Repeat::No);
    }

    #[test]
    fn a_hold_of_n_swings_length_swings_n_times_each_with_one_contact() {
        let t = axe();
        for (hit, period) in [
            (true, t.swing.hit_length(t.swing.samples / 2)),
            (false, t.swing.miss_length()),
        ] {
            let period = period.max(t.swing_seconds);
            for n in [1usize, 2, 3, 5] {
                // Held a little under n periods: the nth swing starts just before the release.
                let hold = period * (n as f64 - 1.0) + 0.05;
                let (swings, contacts) = hold_axe(hold, hit, hold + 3.0);
                assert_eq!(
                    swings, n,
                    "hit {hit}: a hold of {hold:.2} s swings {n} times"
                );
                let want = if hit { 1 } else { 0 };
                assert!(
                    contacts.iter().all(|&c| c == want),
                    "hit {hit}: each swing lands {want} contact: {contacts:?}"
                );
            }
        }
    }

    #[test]
    fn letting_go_stops_after_the_swing_under_way() {
        let t = axe();
        let period = t.swing.miss_length().max(t.swing_seconds);
        // Let go mid-swing: that swing plays out (its contact lands), no other starts.
        let (swings, contacts) = hold_axe(period * 0.5, true, 5.0);
        assert_eq!((swings, contacts), (1, vec![1]));
        // Not held: a single press swings once.
        let mut held = HeldUse::default();
        assert!(!held.due(Repeat::WhenReady, true, 0.0));
        held.press(Holder::Mouse);
        held.release(Holder::Mouse);
        assert!(!held.due(Repeat::WhenReady, true, 0.0));
        // The controller letting go (read each frame) never ends the mouse's hold.
        held.press(Holder::Mouse);
        held.release(Holder::Gamepad);
        assert!(held.due(Repeat::WhenReady, true, 0.0));
        held.release(Holder::Mouse);
        assert!(!held.held());
    }

    #[test]
    fn a_held_pickaxe_breaks_every_repeat_s_and_not_before_its_contact() {
        let p = BREAK_TOOLS
            .iter()
            .find(|t| t.item == "dwell:stone_pickaxe")
            .unwrap();
        let s = p.repeat_s.unwrap();
        assert!(s >= p.contact_s);
        let r = Repeat::of_break(Some(p.item));
        let mut held = HeldUse::default();
        held.press(Holder::Touch);
        held.started(0.0);
        let dt = 1000.0 / 60.0;
        let mut starts = 1;
        let mut now = 0.0;
        let hold_ms = (3.0 * s + 0.05) * 1000.0;
        while now < hold_ms + 2000.0 {
            now += dt;
            if now >= hold_ms {
                held.release(Holder::Touch);
            }
            // The contact pending blocks a new one (`PendingHits::busy`).
            let busy = held.last_ms.is_some_and(|l| now < l + p.contact_s * 1000.0);
            if held.due(r, !busy, now) {
                held.started(now);
                starts += 1;
            }
        }
        assert_eq!(
            starts, 4,
            "a hold of 3 × repeatS: the press and three repeats"
        );
    }
}
