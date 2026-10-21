//! Remote players: snapshot buffer and interpolation (ARCHITECTURE.md §9.5). Remote players are
//! drawn `INTERP_DELAY_MS` in the past, interpolated between the snapshots around that time.

use std::collections::VecDeque;

use dwell_physics::net::{dequantize_pitch, dequantize_yaw, face_of_flags};
use dwell_protocol::{PlayerState, RemotePlayerState, SIM_HZ, player_flags, players};
use dwell_world::FaceFrame;

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
    /// A jump whose snapshot arrived since the previous `sample`: a frame that comes after the
    /// jump's rise (a slow page, a long frame, snapshots read late) still starts the body's jump
    /// (`body_role`).
    pub jumped: bool,
}

impl RemoteView {
    pub fn is_dead(&self) -> bool {
        self.flags & player_flags::DEAD != 0
    }
    pub fn is_crouched(&self) -> bool {
        self.flags & player_flags::CROUCHED != 0
    }
    /// The face it is on: upside down on face B, its feet above its head (BIFACIAL_WORLD.md §6).
    pub fn face(&self) -> FaceFrame {
        face_of_flags(self.flags, player_flags::FACE_B)
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
    /// The server time of a jump not yet reported: a snapshot that was Jumping after one that was
    /// not, noted as it arrives (`sample` reports it once the interpolation time reaches it).
    jump_pending: Option<f64>,
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
        jumped: false,
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
                        jump_pending: None,
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
            if r.state == PlayerState::Jumping
                && entry
                    .samples
                    .back()
                    .is_some_and(|s| s.state.state != PlayerState::Jumping)
            {
                entry.jump_pending = Some(server_ms);
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

    /// Interpolated views at local time `now_ms` (`INTERP_DELAY_MS` in the past), each saying
    /// whether a jump has started since the last call (`RemoteView::jumped`).
    pub fn sample(&mut self, now_ms: f64) -> Vec<RemoteView> {
        let Some(offset) = self.clock_offset else {
            return Vec::new();
        };
        let t = now_ms - offset - self.delay_ms;
        let mut out = Vec::with_capacity(self.remotes.len());
        for p in &mut self.remotes {
            let list = &p.samples;
            if list.is_empty() {
                continue;
            }
            // A jump that arrived since the last call, once its time is drawn: by arrival, not
            // by its time, since a slow page reads snapshots after the time they were sent has
            // been passed, and more of them at once than `MAX_SAMPLES`.
            let jumped = p.jump_pending.is_some_and(|at| at <= t);
            if jumped {
                p.jump_pending = None;
            }
            let mut i = list.len() - 1;
            while i > 0 && list[i].server_ms > t {
                i -= 1;
            }
            let a = &list[i];
            let Some(b) = list.get(i + 1).filter(|_| t > a.server_ms) else {
                // Before the buffer or no newer sample: hold.
                out.push(RemoteView {
                    jumped,
                    ..view(&a.state)
                });
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
                jumped,
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
    fn a_jump_that_started_since_the_last_sample_is_reported_once() {
        let mut r = RemotePlayers::new(100.0);
        let jump = |tick: u32, state| RemotePlayerState {
            state,
            ..remote(f64::from(tick), 0)
        };
        let at = |tick: u32| f64::from(tick) * TICK_MS + 30.0;
        // Idle, then Jumping from tick 30 to 72, then Idle: snapshots every 3 ticks.
        let state = |tick| match tick {
            30..72 => PlayerState::Jumping,
            _ => PlayerState::Idle,
        };
        let mut jumped = Vec::new();
        for tick in (0..150).step_by(3) {
            r.push(tick, &[jump(tick, state(tick))], at(tick));
            // A frame every 2 snapshots, then one long frame over the whole jump.
            if tick % 6 == 0 && !(20..90).contains(&tick) {
                jumped.push((tick, r.sample(at(tick))[0].jumped));
            }
        }
        let seen: Vec<u32> = jumped.iter().filter(|(_, j)| *j).map(|(t, _)| *t).collect();
        assert_eq!(seen, [90], "{jumped:?}");
        // At a frame every snapshot, the frame after the jump's start says so, once.
        let mut r = RemotePlayers::new(100.0);
        let mut seen = Vec::new();
        for tick in (0..150).step_by(3) {
            r.push(tick, &[jump(tick, state(tick))], at(tick));
            if r.sample(at(tick))[0].jumped {
                seen.push(tick);
            }
        }
        assert_eq!(seen.len(), 1, "{seen:?}");
    }

    /// A page that draws a frame a second or slower (CI's software renderer) reads its snapshots
    /// late: in a burst, and a frame behind (the frames run back to back, and the reads queue
    /// behind them). The clock offset is the fastest delivery's, so the interpolation time is
    /// then ahead of every snapshot read, and a jump's arrive after the time they were sent has
    /// been passed; a long frame also reads more of them than the buffer keeps. Either way the
    /// jump is reported, once.
    #[test]
    fn a_jump_is_reported_however_late_its_snapshots_are_read() {
        let jump = |tick: u32, state| RemotePlayerState {
            state,
            ..remote(f64::from(tick), 0)
        };
        let ms = |tick: u32| f64::from(tick) * TICK_MS;
        for frame_ticks in [60u32, 90, 180] {
            // The jump (Jumping for 42 ticks) starts at every phase of a frame.
            for start in (0..frame_ticks).step_by(3) {
                let jumped_at = 240 + 2 * frame_ticks + start;
                let state = |tick: u32| match tick.checked_sub(jumped_at) {
                    Some(0..42) => PlayerState::Jumping,
                    _ => PlayerState::Idle,
                };
                let mut r = RemotePlayers::new(100.0);
                // A fast page first: each snapshot read 30 ms after it was sent, a frame each.
                let mut tick = 0;
                while tick < 240 {
                    r.push(tick, &[jump(tick, state(tick))], ms(tick) + 30.0);
                    assert!(!r.sample(ms(tick) + 30.0)[0].jumped);
                    tick += 3;
                }
                // Then a frame every `frame_ticks`, which reads the snapshots sent by the frame
                // before it.
                let mut seen = Vec::new();
                for frame in 1..=7u32 {
                    let now = ms(240 + frame * frame_ticks) + 30.0;
                    while tick < 240 + (frame - 1) * frame_ticks {
                        r.push(tick, &[jump(tick, state(tick))], now);
                        tick += 3;
                    }
                    if r.sample(now)[0].jumped {
                        seen.push(frame);
                    }
                }
                assert_eq!(
                    seen.len(),
                    1,
                    "frames {frame_ticks} ticks apart, jump at tick {jumped_at}: {seen:?}"
                );
            }
        }
    }

    #[test]
    fn ignores_reordered_snapshots() {
        let mut r = RemotePlayers::new(0.0);
        r.push(6, &[remote(2.0, 0)], 100.0);
        r.push(3, &[remote(1.0, 0)], 110.0);
        assert_eq!(r.latest()[0].feet[0], 2.0);
    }
}
