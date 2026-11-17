//! Damaged blocks heal (owner, 2026-11-15: "We should probably have them heal over time whether
//! you're there or not (like via accumulated time)"; "blocks healing should happen on a delay (like
//! 10s) before it starts. And the last 3 blocks you hit shouldn't heal. So you'd have hit a 4th block
//! for the 1st block to start healing. Each player gets their own last 3 blocks."). The timings are
//! data (`breaking.heal` in shared/tools/tools.json, `dwell_protocol::chopping::HEAL`).
//!
//! Every damaged cell heals on the world's own time (the server's ticks, loaded chunk or not): from
//! `delay_s` after its last hit, a hit back every `full_s / need`, its record gone once whole. A cell
//! among the `keep_last` a player hit last waits; its delay starts when it leaves that list (a
//! player hitting a fourth block, or leaving). Clients hear of each hit healed (the damage stage they
//! draw) and of a cell made whole. A loaded world's records have no time: they heal from the load.

use super::{Server, SessionId};
use dwell_protocol::SIM_HZ;

/// How often the heal step runs (s of the tick).
const EVERY_S: u32 = 1;

impl Server {
    fn now_s(&self) -> f64 {
        f64::from(self.tick) / f64::from(SIM_HZ)
    }

    /// A player's hit on `cell` (damaged, not broken): its clock restarts and it joins the player's
    /// last blocks.
    pub(super) fn note_hit(&mut self, id: SessionId, cell: [i32; 3]) {
        let Some(heal) = dwell_protocol::chopping::HEAL else {
            return;
        };
        let now = self.now_s();
        self.heal_since.insert(cell, now);
        let last = self.recent_hits.entry(id).or_default();
        last.retain(|c| *c != cell);
        last.push_back(cell);
        while last.len() > heal.keep_last {
            // Out of the player's last blocks: its delay starts now.
            if let Some(out) = last.pop_front() {
                self.heal_since.insert(out, now);
            }
        }
    }

    /// A player leaves: their last blocks start their delay now.
    pub(super) fn forget_hits(&mut self, id: SessionId) {
        let now = self.now_s();
        for c in self.recent_hits.remove(&id).unwrap_or_default() {
            self.heal_since.insert(c, now);
        }
    }

    /// The heal step (every `EVERY_S` s).
    pub(super) fn heal_damage(&mut self) {
        if !self.tick.is_multiple_of(EVERY_S * SIM_HZ) {
            return;
        }
        self.heal_damage_now();
    }

    /// The heal step now: each damaged cell no player keeps heals the hits its time has earned.
    pub fn heal_damage_now(&mut self) {
        let Some(heal) = dwell_protocol::chopping::HEAL else {
            return;
        };
        if self.damage.cells.is_empty() {
            self.heal_since.clear();
            return;
        }
        let now = self.now_s();
        let kept: std::collections::HashSet<[i32; 3]> =
            self.recent_hits.values().flatten().copied().collect();
        let (mut changed, mut gone) = (Vec::new(), Vec::new());
        let cells: Vec<[i32; 3]> = self.damage.cells.keys().copied().collect();
        for c in cells {
            if kept.contains(&c) {
                continue;
            }
            // A record without a time (a loaded world's) heals from now.
            let since = *self.heal_since.entry(c).or_insert(now);
            let d = self.damage.cells.get_mut(&c).expect("listed");
            let per = heal.full_s / f64::from(d.need.max(1));
            let due = ((now - since - heal.delay_s) / per).floor();
            if due < 1.0 {
                continue;
            }
            let n = (due as u32).min(u32::from(d.hits)) as u16;
            d.hits -= n;
            // The next hit heals `per` after this one.
            self.heal_since.insert(c, since + f64::from(n) * per);
            if d.hits == 0 {
                self.damage.cells.remove(&c);
                self.heal_since.remove(&c);
                gone.push(c);
            } else {
                changed.push(c);
            }
        }
        self.heal_since
            .retain(|c, _| self.damage.cells.contains_key(c));
        if !changed.is_empty() || !gone.is_empty() {
            self.send_damage(&changed, gone);
        }
    }

    /// A cell's damage hits (tests).
    pub fn damage_hits(&self, cell: [i32; 3]) -> Option<u16> {
        self.damage.cells.get(&cell).map(|d| d.hits)
    }
}
