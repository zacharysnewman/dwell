//! Other players' hits, seen in the server's damage updates (docs/BLOCK_DAMAGE.md §6, AUDIO.md §5):
//! a cell whose damage view changed was hit, at its newest mark. Our own hits already sounded and
//! burst on the request (the server's echo of them is passed over), and the views a join's hello
//! brings are the world's damage so far, not hits now. Nothing new on the wire: a DAMAGE client
//! is sent every change already.
use dwell_protocol::DamageMessage;
use dwell_protocol::damage::{DamageView, HitMark};
use dwell_world::hash::HashMap;

/// How long (ms) after our own hit on a cell the server's update of it is its echo, not another
/// player's hit.
pub const OWN_HIT_MS: f64 = 1500.0;
/// How long (ms) after the hello the damage updates are the world's damage so far (silent).
pub const SYNC_QUIET_MS: f64 = 2000.0;

#[derive(Clone, Debug, Default)]
pub struct RemoteHits {
    views: HashMap<[i32; 3], DamageView>,
    /// Our own hits not yet echoed: the cell and when.
    own: Vec<([i32; 3], f64)>,
    quiet_until_ms: f64,
}

impl RemoteHits {
    /// The hello went (now): the updates for a while are the damage so far.
    pub fn hello(&mut self, now_ms: f64) {
        self.quiet_until_ms = now_ms + SYNC_QUIET_MS;
    }

    /// We hit `cell` now (its sound and chips already shown).
    pub fn own_hit(&mut self, cell: [i32; 3], now_ms: f64) {
        self.own.push((cell, now_ms));
    }

    /// A damage update: the hits in it that another player struck, each its cell and newest mark.
    pub fn on_damage(&mut self, m: &DamageMessage, now_ms: f64) -> Vec<([i32; 3], HitMark)> {
        self.own.retain(|&(_, t)| now_ms - t < OWN_HIT_MS);
        for c in &m.gone {
            self.views.remove(c);
        }
        let mut out = Vec::new();
        for (cell, view) in &m.cells {
            let before = self.views.insert(*cell, view.clone());
            if before.as_ref() == Some(view) || now_ms < self.quiet_until_ms {
                continue;
            }
            if let Some(i) = self.own.iter().position(|(c, _)| c == cell) {
                self.own.swap_remove(i);
                continue;
            }
            if let Some(&mark) = view.marks.last() {
                out.push((*cell, mark));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(cell: [i32; 3], fraction: u8, points: &[[u8; 3]]) -> DamageMessage {
        DamageMessage {
            cells: vec![(
                cell,
                DamageView {
                    fraction,
                    marks: points.iter().map(|&point| HitMark { face: 3, point }).collect(),
                },
            )],
            gone: Vec::new(),
        }
    }

    // face-a-only: cells and marks pass through as the server sent them (no face-local reading).
    #[test]
    fn another_players_hit_shows_once_at_its_newest_mark_and_ours_is_not_doubled() {
        let mut h = RemoteHits::default();
        h.hello(0.0);
        let c = [4, 120, -3];
        // The hello's damage so far: silent.
        assert!(h.on_damage(&update(c, 51, &[[1, 255, 1]]), 100.0).is_empty());
        // Another player's hit: once, at the newest mark.
        let hits = h.on_damage(&update(c, 102, &[[1, 255, 1], [200, 255, 9]]), 3000.0);
        assert_eq!(hits, [(c, HitMark { face: 3, point: [200, 255, 9] })]);
        // The same view again (a resend) is no hit.
        assert!(h.on_damage(&update(c, 102, &[[1, 255, 1], [200, 255, 9]]), 3100.0).is_empty());
        // Our own hit: shown on the request, so the server's echo of it is silent here …
        h.own_hit(c, 4000.0);
        let echo = update(c, 153, &[[1, 255, 1], [200, 255, 9], [9, 255, 9]]);
        assert!(h.on_damage(&echo, 4080.0).is_empty());
        // … and only once: the next change is another player's again.
        let next = update(c, 204, &[[1, 255, 1], [200, 255, 9], [9, 255, 9], [7, 255, 7]]);
        assert_eq!(h.on_damage(&next, 4200.0).len(), 1);
        // A stale own hit (no echo came) does not hide a later one.
        let d = [0, 100, 0];
        h.own_hit(d, 5000.0);
        assert_eq!(h.on_damage(&update(d, 51, &[[0, 255, 0]]), 5000.0 + OWN_HIT_MS + 1.0).len(), 1);
        // A cell gone (broken) and hit afresh shows again.
        h.on_damage(
            &DamageMessage {
                cells: Vec::new(),
                gone: vec![d],
            },
            9000.0,
        );
        assert_eq!(h.on_damage(&update(d, 51, &[[0, 255, 0]]), 9100.0).len(), 1);
    }
}
