//! Explosives' fuses and blasts on the server (docs/EXPLOSIVES.md §3): a lit explosive's fuse counts
//! down in ticks; when it is out the blast goes off (`Server::blast`, server.rs). The table is the
//! data's (`dwell_world::explosives::EXPLOSIVES`); tests swap in a fixture table by data alone.

use dwell_world::explosives::{EXPLOSIVES, ExplosiveData, LIMITS};

/// The server's ticks a second.
const TICKS_PER_SECOND: f64 = dwell_protocol::SIM_HZ as f64;

/// A burning fuse: its explosive's cell, its entry in the table and the ticks left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fuse {
    pub cell: [i32; 3],
    pub explosive: usize,
    pub ticks: u32,
}

/// The fuses burning and the table they burn by.
#[derive(Clone, Debug)]
pub struct Fuses {
    pub table: Vec<ExplosiveData>,
    pub burning: Vec<Fuse>,
    /// Blasts set off a tick at most (`limits.blastsPerTick`).
    pub per_tick: u32,
    /// Cells one blast flings at most (`limits.maxPieces`).
    pub max_pieces: usize,
}

impl Default for Fuses {
    fn default() -> Self {
        Self {
            table: EXPLOSIVES.to_vec(),
            burning: Vec::new(),
            per_tick: LIMITS.blasts_per_tick,
            max_pieces: LIMITS.max_pieces as usize,
        }
    }
}

/// `seconds` in whole ticks (at least one).
pub fn ticks_of(seconds: f64) -> u32 {
    ((seconds * TICKS_PER_SECOND).round() as u32).max(1)
}

impl Fuses {
    /// Lights the explosive `explosive` at `cell` with a fuse of `seconds`; one already burning there
    /// keeps the shorter of the two.
    pub fn light(&mut self, cell: [i32; 3], explosive: usize, seconds: f64) {
        let ticks = ticks_of(seconds);
        match self.burning.iter_mut().find(|f| f.cell == cell) {
            Some(f) => f.ticks = f.ticks.min(ticks),
            None => self.burning.push(Fuse {
                cell,
                explosive,
                ticks,
            }),
        }
    }

    /// Whether a fuse burns at `cell`.
    pub fn burning_at(&self, cell: [i32; 3]) -> bool {
        self.burning.iter().any(|f| f.cell == cell)
    }

    /// Burns every fuse a tick; the ones out (at most `per_tick`, the first lit first) leave the list
    /// and are returned. The rest out wait for the next tick.
    pub fn tick(&mut self) -> Vec<Fuse> {
        for f in &mut self.burning {
            f.ticks = f.ticks.saturating_sub(1);
        }
        let mut out = Vec::new();
        let mut k = 0;
        while k < self.burning.len() {
            if self.burning[k].ticks == 0 && out.len() < self.per_tick as usize {
                out.push(self.burning.remove(k));
            } else {
                k += 1;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fuse_runs_its_seconds_then_blows_and_at_most_the_limit_a_tick() {
        let mut f = Fuses {
            per_tick: 1,
            ..Fuses::default()
        };
        f.light([0, 0, 0], 0, 0.5);
        f.light([1, 0, 0], 0, 0.5);
        let mut blown = Vec::new();
        for t in 1..=40 {
            for b in f.tick() {
                blown.push((t, b.cell));
            }
        }
        assert_eq!(blown, vec![(30, [0, 0, 0]), (31, [1, 0, 0])]);
    }

    #[test]
    fn relighting_keeps_the_shorter_fuse() {
        let mut f = Fuses::default();
        f.light([0, 0, 0], 0, 3.0);
        f.light([0, 0, 0], 0, 0.25);
        f.light([0, 0, 0], 0, 2.0);
        assert_eq!(f.burning.len(), 1);
        assert_eq!(f.burning[0].ticks, 15);
    }
}
