//! Theater mode's camera as a simulation viewpoint (docs/THEATER_MODE.md §7, T-4; owner,
//! 2026-11-12: "it uses the camera's position for simulation, perhaps only available in creative
//! mode. Sets us free from the player but can't be used for cheating in adventure").
//!
//! A client with the `theater.viewpoint` capability sends its camera's position while a camera
//! path plays (`TheaterViewpoint`, at most `world::THEATER_VIEWPOINT_HZ` a second) and an empty
//! one when it ends. The server keeps one per player and, **only where the world's mode has the
//! `theaterViewpoint` ability** (`shared/modes/modes.json`: creative), adds it to that player's
//! interest: its chunks are generated and streamed (after the body's view, from the same budget),
//! creatures and NPCs spawn and stay near it, the near ecology's herds come near it, and the lava
//! and the water step there. Elsewhere it is kept but never used, so a mode change takes effect at
//! once and an adventure client gains nothing by sending one. It is held within
//! `world::THEATER_VIEWPOINT_MAX_M` of the body. It only *brings the world near*: animals don't
//! flee it, NPCs don't look at it, nothing collides with it.

use dwell_protocol::capability_names::THEATER_VIEWPOINT;
use dwell_protocol::world as wc;

use super::*;

/// Ticks between viewpoints taken from one player (`world::THEATER_VIEWPOINT_HZ`).
pub const VIEWPOINT_EVERY_TICKS: u32 = SIM_HZ / wc::THEATER_VIEWPOINT_HZ as u32;

/// The viewpoint's distance cap from the body (metres).
pub const VIEWPOINT_MAX_M: f64 = wc::THEATER_VIEWPOINT_MAX_M as f64;

/// `at` pulled straight toward `body` until it is within `max_m` of it.
pub fn capped(at: [f64; 3], body: [f64; 3], max_m: f64) -> [f64; 3] {
    let d = [at[0] - body[0], at[1] - body[1], at[2] - body[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if len <= max_m {
        return at;
    }
    let k = max_m / len;
    [body[0] + d[0] * k, body[1] + d[1] * k, body[2] + d[2] * k]
}

/// A player's viewpoint as its client sent it, and its rate limit.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Viewpoint {
    /// The one in use (taken), whatever the mode; [`Server::viewpoint_of`] applies the mode.
    pub at: Option<[f64; 3]>,
    /// One that came sooner than the rate allows: taken when its turn comes (a newer one replaces
    /// it), so the last position always arrives.
    pub pending: Option<[f64; 3]>,
    /// The tick the last one was taken.
    pub taken_tick: Option<u32>,
    /// The viewpoint's chunk the streaming last saw (a move unloads what it left).
    pub stream_center: Option<ChunkCoord>,
    /// Every chunk of the view around it is with the client.
    pub stream_complete: bool,
}

impl Viewpoint {
    /// A `TheaterViewpoint` arrived at `tick`. A clear is taken at once (it narrows the interest).
    pub fn arrive(&mut self, at: Option<[f64; 3]>, tick: u32) {
        match at {
            None => {
                // The streaming sees the change and unloads what the view no longer holds.
                self.at = None;
                self.pending = None;
            }
            Some(p) => {
                self.pending = Some(p);
                self.take(tick);
            }
        }
    }

    /// Takes the pending one if its turn has come.
    pub fn take(&mut self, tick: u32) {
        let due = self
            .taken_tick
            .is_none_or(|t| tick.wrapping_sub(t) >= VIEWPOINT_EVERY_TICKS);
        if due && let Some(p) = self.pending.take() {
            self.at = Some(p);
            self.taken_tick = Some(tick);
        }
    }
}

impl Server {
    /// A `TheaterViewpoint` from a session's client. Kept only from a client that said the
    /// capability (others never send one).
    pub(super) fn theater_viewpoint(&mut self, id: SessionId, at: Option<[f64; 3]>) {
        let tick = self.tick;
        let Some(s) = self.sessions.get_mut(&id) else {
            return;
        };
        if !s.capabilities.has(THEATER_VIEWPOINT) {
            return;
        }
        s.viewpoint.arrive(at, tick);
    }

    /// Each tick: the viewpoints that came too soon are taken when their turn comes.
    pub(super) fn take_viewpoints(&mut self) {
        let tick = self.tick;
        for s in self.sessions.values_mut() {
            s.viewpoint.take(tick);
        }
    }

    /// Whether the world's mode makes the theater camera a simulation viewpoint.
    pub fn viewpoints_allowed(&self) -> bool {
        self.settings.allows(Ability::TheaterViewpoint)
    }

    /// A session's viewpoint as the server uses it: none unless the world's mode has the ability
    /// and the player is joined on the home grid; held within [`VIEWPOINT_MAX_M`] of the body (or
    /// of where it died).
    pub(super) fn viewpoint_of(&self, s: &Session) -> Option<[f64; 3]> {
        if !self.viewpoints_allowed() || !s.joined() || s.away.is_some() {
            return None;
        }
        let at = s.viewpoint.at?;
        let body = match s.handle {
            Some(h) => self.players.position(&self.physics, h).to_array(),
            None => s.death_position,
        };
        Some(capped(at, body, VIEWPOINT_MAX_M))
    }

    /// Every viewpoint in use (world metres).
    pub(super) fn viewpoints(&self) -> Vec<[f64; 3]> {
        self.sessions
            .values()
            .filter_map(|s| self.viewpoint_of(s))
            .collect()
    }

    /// The viewpoints' chunks.
    pub(super) fn viewpoint_centers(&self) -> Vec<ChunkCoord> {
        self.viewpoints()
            .into_iter()
            .map(|p| chunk_at(p[0], p[1], p[2]))
            .collect()
    }

    /// Where the world is brought near: the living players' bodies, then the viewpoints.
    pub(super) fn presence_points(&self, bodies: &[[f64; 3]]) -> Vec<[f64; 3]> {
        let mut out = bodies.to_vec();
        out.extend(self.viewpoints());
        out
    }

    /// The viewpoint a player's id has in use (tests, tools).
    pub fn viewpoint_of_player(&self, player_id: u16) -> Option<[f64; 3]> {
        self.by_player(player_id).and_then(|s| self.viewpoint_of(s))
    }

    /// Whether a player's client has every chunk of the view around its viewpoint (tests).
    pub fn viewpoint_streamed(&self, player_id: u16) -> bool {
        self.by_player(player_id)
            .is_some_and(|s| s.viewpoint.stream_complete)
    }

    /// The view around a session's viewpoint (§7), nearest first, within what the body's view left
    /// of the budgets: the same view sphere as the body's.
    pub(super) fn stream_viewpoint(&mut self, id: SessionId, center: ChunkCoord, sent: &mut i64) {
        let mut complete = true;
        let mut waiting = false;
        for i in 0..self.view_offsets.len() {
            let c = add(center, self.view_offsets[i]);
            let s = &self.sessions[&id];
            if !dwell_world::WorldBounds::of(self.config.world_size)
                .volume()
                .chunk_may_hold(c)
                || s.streamed.contains(&c)
            {
                continue;
            }
            complete = false;
            if s.chunk_credit <= 0.0 || *sent >= wc::MAX_CHUNKS_PER_TICK {
                break;
            }
            if !self.air_ready(c) || waiting {
                waiting = true;
                continue;
            }
            let Some(m) = self.chunk_message(id, c, false) else {
                // Full mode: generate it first (a later tick sends it), as the body's view does.
                if self.explicit_wanted.len() < MAX_EXPLICIT_WANTED {
                    self.explicit_wanted.push(c);
                }
                continue;
            };
            let form = m.form;
            let bytes = encode(&Message::ChunkData(m));
            let s = self.sessions.get_mut(&id).expect("session");
            s.chunk_credit -= bytes.len() as f64;
            s.streamed.insert(c);
            s.stream_stats.viewpoint_sent += 1;
            Self::count_sent(&mut s.stream_stats, form);
            self.outbox.push(Outgoing {
                session: id,
                kind: OutKind::Reliable(Channel::World),
                bytes,
            });
            *sent += 1;
        }
        self.sessions
            .get_mut(&id)
            .expect("session")
            .viewpoint
            .stream_complete = complete;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_viewpoint_is_capped_toward_the_body() {
        assert_eq!(capped([10.0, 0.0, 0.0], [0.0; 3], 20.0), [10.0, 0.0, 0.0]);
        let c = capped([0.0, 0.0, 300.0], [0.0, 0.0, 100.0], 50.0);
        assert!((c[2] - 150.0).abs() < 1e-9, "{c:?}");
    }

    #[test]
    fn viewpoints_are_taken_at_the_rate_and_the_last_one_arrives() {
        let mut v = Viewpoint::default();
        v.arrive(Some([1.0; 3]), 100);
        assert_eq!(v.at, Some([1.0; 3]), "the first is taken at once");
        v.arrive(Some([2.0; 3]), 101);
        v.arrive(Some([3.0; 3]), 102);
        assert_eq!(v.at, Some([1.0; 3]), "too soon: held");
        v.take(100 + VIEWPOINT_EVERY_TICKS - 1);
        assert_eq!(v.at, Some([1.0; 3]));
        v.take(100 + VIEWPOINT_EVERY_TICKS);
        assert_eq!(v.at, Some([3.0; 3]), "the newest held one, at its turn");
        v.arrive(None, 100 + VIEWPOINT_EVERY_TICKS + 1);
        assert_eq!(v.at, None, "a clear is taken at once");
        v.take(10_000);
        assert_eq!(v.at, None, "and nothing held comes back after it");
    }
}
