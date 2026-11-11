//! Steam where lava boils water away (docs/proposals/VOLCANOES.md §6.3): the server's `Steam`
//! cells (sent only to a client that said `SteamHello`) as rising puffs through the blasts' puff
//! system (crate::blasts `Puff`, a smoke puff of steam's own look, render/blasts.rs `steam_look`),
//! its look from shared/fluids/lava.json `steam`. A CPU adapter draws a third.

use crate::blasts::{Puff, PuffKind};
use dwell_world::FaceFrame;

/// Steam's look (shared/fluids/lava.json `steam`): life (s), rise (m/s), size at birth and end
/// (m), peak opacity.
const LOOK: dwell_lava::data::SteamLook = dwell_lava::DATA.steam;
const SECONDS: f64 = LOOK.seconds as f64;
const RISE: f64 = LOOK.rise_ms as f64;
const SIZE: [f64; 2] = [LOOK.size_m[0] as f64, LOOK.size_m[1] as f64];
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
                    alpha: f64::from(LOOK.opacity) * (1.0 - u).powi(2) * (t * 8.0).min(1.0),
                    kind: PuffKind::Smoke,
                    explosive: crate::render::blasts::steam_look(),
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
            let late = s.puffs(10.0 + SECONDS * 0.8);
            assert_eq!((early.len(), late.len()), (1, 1));
            // Rises along the face's up; the later puff is bigger and fainter.
            let up = f.up();
            let rise: f64 = (0..3)
                .map(|k| (late[0].at[k] - early[0].at[k]) * up[k])
                .sum();
            assert!(rise > 0.5 * RISE * SECONDS, "{rise}");
            assert!(late[0].size > early[0].size && late[0].alpha < early[0].alpha);
            s.prune(10.0 + SECONDS + 0.1);
            assert!(
                s.puffs(10.0 + SECONDS + 0.1).is_empty(),
                "gone after its life"
            );
            assert_eq!(
                early[0].explosive,
                crate::render::blasts::steam_look(),
                "steam's own look"
            );
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

    #[test]
    fn the_burning_edge_is_full_at_a_burn_hit_and_fades_over_its_data_seconds() {
        // face-a-only: a screen effect, no direction.
        let span = f64::from(dwell_lava::DATA.burn_edge.seconds) * 1000.0;
        let until = 5000.0 + span;
        assert_eq!(crate::game::burn_edge_strength(until, 5000.0), 1.0);
        let half = crate::game::burn_edge_strength(until, 5000.0 + span / 2.0);
        assert!((half - 0.5).abs() < 1e-9, "{half}");
        assert_eq!(crate::game::burn_edge_strength(until, until + 1.0), 0.0);
        assert_eq!(
            crate::game::burn_edge_strength(0.0, 5000.0),
            0.0,
            "never burned"
        );
    }
}
