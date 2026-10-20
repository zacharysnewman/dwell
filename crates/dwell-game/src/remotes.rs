//! Remote players: snapshot buffer and interpolation (ARCHITECTURE.md §9.5). Remote players are
//! drawn `INTERP_DELAY_MS` in the past, interpolated between the snapshots around that time.

use std::collections::VecDeque;

use dwell_physics::net::{dequantize_pitch, dequantize_yaw};
use dwell_protocol::{PlayerState, RemotePlayerState, SIM_HZ, player_flags, players};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RemoteView {
    pub player_id: u16,
    pub feet: [f64; 3],
    pub velocity: [f32; 3],
    /// Degrees.
    pub yaw: f32,
    /// Degrees.
    pub pitch: f32,
    pub state: PlayerState,
    pub flags: u8,
    /// What it holds (`held_item`, the server's block id; protocol v17).
    pub held: u16,
    /// Its last swing (`swing_flags`; v17).
    pub swing: u8,
}

impl RemoteView {
    pub fn is_dead(&self) -> bool {
        self.flags & player_flags::DEAD != 0
    }
    pub fn is_crouched(&self) -> bool {
        self.flags & player_flags::CROUCHED != 0
    }
    /// On face B: upside down, its feet above its head (BIFACIAL_WORLD.md §6).
    pub fn is_face_b(&self) -> bool {
        self.flags & player_flags::FACE_B != 0
    }
}

#[derive(Clone, Debug)]
struct Sample {
    server_ms: f64,
    state: RemotePlayerState,
}

#[derive(Clone, Debug)]
struct Remote {
    player_id: u16,
    samples: VecDeque<Sample>,
    last_seen: f64,
}

const TICK_MS: f64 = 1000.0 / SIM_HZ as f64;
/// Players not in any snapshot for this long are removed.
const FORGET_MS: f64 = 1000.0;
/// Samples kept per player.
const MAX_SAMPLES: usize = 32;

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Shortest-path interpolation of angles in degrees.
fn lerp_angle(a: f64, b: f64, t: f64) -> f64 {
    let d = ((((b - a) % 360.0) + 540.0) % 360.0) - 180.0;
    a + d * t
}

fn view(s: &RemotePlayerState) -> RemoteView {
    RemoteView {
        player_id: s.player_id,
        feet: s.position,
        velocity: s.velocity,
        yaw: dequantize_yaw(s.yaw),
        pitch: dequantize_pitch(s.pitch),
        state: s.state,
        flags: s.flags,
        held: s.held,
        swing: s.swing,
    }
}

/// The snapshot buffer of every remote player, in the order they first appeared.
#[derive(Clone, Debug)]
pub struct RemotePlayers {
    remotes: Vec<Remote>,
    /// Estimated (local clock − server clock) in ms; the smallest seen, i.e. the fastest delivery.
    clock_offset: Option<f64>,
    delay_ms: f64,
}

impl Default for RemotePlayers {
    fn default() -> Self {
        Self::new(players::INTERP_DELAY_MS as f64)
    }
}

impl RemotePlayers {
    pub fn new(delay_ms: f64) -> Self {
        Self {
            remotes: Vec::new(),
            clock_offset: None,
            delay_ms,
        }
    }

    /// Adds one snapshot's remote players, received at local time `now_ms`.
    pub fn push(&mut self, server_tick: u32, remotes: &[RemotePlayerState], now_ms: f64) {
        let server_ms = f64::from(server_tick) * TICK_MS;
        let offset = now_ms - server_ms;
        // Track the minimum one-way delay, drifting up slowly so clock skew can't pin it forever.
        self.clock_offset = Some(match self.clock_offset {
            None => offset,
            Some(c) => offset.min(c + 0.05),
        });
        for r in remotes {
            let i = match self.remotes.iter().position(|p| p.player_id == r.player_id) {
                Some(i) => i,
                None => {
                    self.remotes.push(Remote {
                        player_id: r.player_id,
                        samples: VecDeque::new(),
                        last_seen: now_ms,
                    });
                    self.remotes.len() - 1
                }
            };
            let entry = &mut self.remotes[i];
            if entry
                .samples
                .back()
                .is_some_and(|s| s.server_ms >= server_ms)
            {
                continue; // reordered
            }
            entry.samples.push_back(Sample {
                server_ms,
                state: *r,
            });
            while entry.samples.len() > MAX_SAMPLES {
                entry.samples.pop_front();
            }
            entry.last_seen = now_ms;
        }
        self.remotes.retain(|p| now_ms - p.last_seen <= FORGET_MS);
    }

    pub fn ids(&self) -> Vec<u16> {
        self.remotes.iter().map(|p| p.player_id).collect()
    }

    /// Latest received state of each remote player (for the prediction world's proxies).
    pub fn latest(&self) -> Vec<RemoteView> {
        self.remotes
            .iter()
            .filter_map(|p| p.samples.back().map(|s| view(&s.state)))
            .collect()
    }

    /// Interpolated views at local time `now_ms` (`INTERP_DELAY_MS` in the past).
    pub fn sample(&self, now_ms: f64) -> Vec<RemoteView> {
        let Some(offset) = self.clock_offset else {
            return Vec::new();
        };
        let t = now_ms - offset - self.delay_ms;
        let mut out = Vec::with_capacity(self.remotes.len());
        for p in &self.remotes {
            let list = &p.samples;
            if list.is_empty() {
                continue;
            }
            let mut i = list.len() - 1;
            while i > 0 && list[i].server_ms > t {
                i -= 1;
            }
            let a = &list[i];
            let Some(b) = list.get(i + 1).filter(|_| t > a.server_ms) else {
                out.push(view(&a.state)); // before the buffer or no newer sample: hold
                continue;
            };
            let f = ((t - a.server_ms) / (b.server_ms - a.server_ms)).min(1.0);
            let (va, vb) = (view(&a.state), view(&b.state));
            let ff = f as f32;
            out.push(RemoteView {
                feet: std::array::from_fn(|k| lerp(va.feet[k], vb.feet[k], f)),
                velocity: std::array::from_fn(|k| {
                    va.velocity[k] + (vb.velocity[k] - va.velocity[k]) * ff
                }),
                yaw: lerp_angle(f64::from(va.yaw), f64::from(vb.yaw), f) as f32,
                pitch: va.pitch + (vb.pitch - va.pitch) * ff,
                // Discrete state switches at the midpoint.
                state: if f < 0.5 { va.state } else { vb.state },
                flags: if f < 0.5 { va.flags } else { vb.flags },
                ..vb
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_close;

    fn remote(x: f64, flags: u8) -> RemotePlayerState {
        RemotePlayerState {
            player_id: 4,
            position: [x, 0.0, 0.0],
            velocity: [1.0, 0.0, 0.0],
            yaw: 0,
            pitch: 0,
            state: PlayerState::Walking,
            flags,
            held: 0,
            swing: 0,
        }
    }

    #[test]
    fn interpolates_interp_delay_ms_in_the_past_between_snapshots() {
        let mut r = RemotePlayers::new(100.0);
        // Snapshots every 3 ticks (50 ms), each arriving 30 ms after it was sent.
        for i in 0..6u32 {
            r.push(
                i * 3,
                &[remote(f64::from(i), 0)],
                f64::from(i) * 50.0 + 30.0,
            );
        }
        // At local 280 ms: server time 250 − 100 = 150 ms → between snapshot 3 (150 ms) and 4.
        assert_close(r.sample(280.0)[0].feet[0], 3.0, 2);
        assert_close(r.sample(305.0)[0].feet[0], 3.5, 2);
    }

    #[test]
    fn holds_the_newest_sample_when_the_buffer_runs_dry_and_forgets_silent_players() {
        let mut r = RemotePlayers::new(100.0);
        r.push(0, &[remote(0.0, 0)], 0.0);
        r.push(3, &[remote(1.0, player_flags::DEAD)], 50.0);
        let late = r.sample(1000.0)[0];
        assert_eq!(late.feet[0], 1.0);
        assert!(late.is_dead());
        r.push(100, &[], 2000.0);
        assert_eq!(r.ids(), Vec::<u16>::new());
    }

    #[test]
    fn ignores_reordered_snapshots() {
        let mut r = RemotePlayers::new(0.0);
        r.push(6, &[remote(2.0, 0)], 100.0);
        r.push(3, &[remote(1.0, 0)], 110.0);
        assert_eq!(r.latest()[0].feet[0], 2.0);
    }
}
