//! Steam where lava boils water away (docs/proposals/VOLCANOES.md §6.3): the server's `Steam`
//! cells (sent only to a client that said `SteamHello`) as rising puffs through the blasts' puff
//! system (crate::blasts `Puff`, drawn with the explosions' smoke). A CPU adapter draws a third.

use crate::blasts::{Puff, PuffKind};
use dwell_world::FaceFrame;

/// How long a puff lives (s), how fast it rises (m/s), its size at birth and at the end (m).
const SECONDS: f64 = 2.6;
const RISE: f64 = 1.3;
const SIZE: [f64; 2] = [0.45, 1.5];
/// The most puffs alive at once (a lake boiling a sea away sends many cells).
const MAX_LIVE: usize = 240;

fn hash(seed: u32, k: u32) -> f64 {
    let mut h = seed.wrapping_mul(0x9e37_79b9) ^ k.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    f64::from(h & 0xffff) / 65535.0
}

#[derive(Clone, Copy, Debug)]
struct SteamPuff {
    at: [f64; 3],
    start_s: f64,
    seed: u32,
}

#[derive(Debug, Default)]
pub struct Steam {
    list: Vec<SteamPuff>,
    /// A CPU adapter: a third of the puffs.
    pub cpu: bool,
}

impl Steam {
    /// The server's steam `cells` (world cells: x, z metres, y a ½ m row) at `now_s`: a puff
    /// over each (a third on a CPU adapter), the oldest dropped past `MAX_LIVE`.
    pub fn add(&mut self, cells: &[[i32; 3]], now_s: f64) {
        for (i, c) in cells.iter().enumerate() {
            if self.cpu && i % 3 != 0 {
                continue;
            }
            let f = FaceFrame::at_height(dwell_world::m_of_row(c[1]));
            let up = f.up();
            let top = dwell_world::m_of_row(c[1]) + 0.25;
            let seed = (c[0] as u32).wrapping_mul(73_856_093)
                ^ (c[1] as u32).wrapping_mul(19_349_663)
                ^ (c[2] as u32).wrapping_mul(83_492_791);
            let at = [
                f64::from(c[0]) + 0.5,
                top + 0.25 * up[1],
                f64::from(c[2]) + 0.5,
            ];
            self.list.push(SteamPuff {
                at,
                start_s: now_s,
                seed,
            });
        }
        let over = self.list.len().saturating_sub(MAX_LIVE);
        self.list.drain(..over);
    }

    pub fn prune(&mut self, now_s: f64) {
        self.list.retain(|p| now_s - p.start_s < SECONDS);
    }

    /// The live puffs at `now_s`: rising along their face's up, growing, fading out.
    pub fn puffs(&self, now_s: f64) -> Vec<Puff> {
        self.list
            .iter()
            .filter_map(|p| {
                let t = now_s - p.start_s;
                if !(0.0..SECONDS).contains(&t) {
                    return None;
                }
                let u = t / SECONDS;
                let up = FaceFrame::at_height(p.at[1]).up();
                let drift = [hash(p.seed, 1) - 0.5, 0.0, hash(p.seed, 2) - 0.5];
                let at = std::array::from_fn(|k| p.at[k] + RISE * t * up[k] + drift[k] * t * 0.4);
                Some(Puff {
                    at,
                    size: SIZE[0] + (SIZE[1] - SIZE[0]) * u.sqrt(),
                    alpha: 0.7 * (1.0 - u).powi(2) * (t * 8.0).min(1.0),
                    kind: PuffKind::Smoke,
                    explosive: 0,
                    seed: p.seed,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    dwell_world::both_faces!(
        fn a_steam_cell_puffs_up_its_face_and_fades(f: FaceFrame) {
            let mut s = Steam::default();
            let cell = f.cell([3, 200, -4]);
            s.add(&[cell], 10.0);
            let early = s.puffs(10.2);
            let late = s.puffs(12.0);
            assert_eq!((early.len(), late.len()), (1, 1));
            // Rises along the face's up; the later puff is bigger and fainter.
            let up = f.up();
            let rise: f64 = (0..3)
                .map(|k| (late[0].at[k] - early[0].at[k]) * up[k])
                .sum();
            assert!(rise > 1.5, "{rise}");
            assert!(late[0].size > early[0].size && late[0].alpha < early[0].alpha);
            s.prune(13.0);
            assert!(s.puffs(13.0).is_empty(), "gone after its life");
        }
    );

    #[test]
    fn a_cpu_adapter_draws_a_third_and_the_live_puffs_are_capped() {
        // face-a-only: counts, not directions.
        let cells: Vec<[i32; 3]> = (0..30).map(|i| [i, 200, 0]).collect();
        let mut cpu = Steam {
            cpu: true,
            ..Default::default()
        };
        cpu.add(&cells, 0.0);
        assert_eq!(cpu.puffs(0.1).len(), 10);
        let mut s = Steam::default();
        for _ in 0..20 {
            s.add(&cells, 0.0);
        }
        assert_eq!(s.puffs(0.1).len(), MAX_LIVE);
    }
}
