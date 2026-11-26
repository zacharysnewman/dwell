//! Players' looks (the Character painter, docs/CHARACTERS.md §9.4): each player's paint, kept for
//! the session and told to every client with the `players.looks` capability.

use dwell_protocol::capability_names::PLAYERS_LOOKS;
use dwell_protocol::looks::PlayerLook;
use dwell_protocol::{Channel, Message};

use super::{Server, SessionId};

impl Server {
    /// Everyone's looks to a client that has the capability, once; after it, each change.
    pub(super) fn send_looks(&mut self, id: SessionId) {
        let Some(s) = self.sessions.get_mut(&id) else {
            return;
        };
        if s.looks_sent || !s.capabilities.has(PLAYERS_LOOKS) {
            return;
        }
        s.looks_sent = true;
        let all: Vec<PlayerLook> = self
            .sessions
            .iter()
            .filter(|(o, _)| **o != id)
            .filter_map(|(_, s)| s.look.clone())
            .collect();
        self.send_reliable(id, &Message::Looks(all), Channel::World);
    }

    /// A client's own look (the first of its message): kept if well formed, told to the others.
    pub(super) fn set_look(&mut self, id: SessionId, looks: Vec<PlayerLook>) {
        let Some(mut look) = looks.into_iter().next().filter(PlayerLook::valid) else {
            return;
        };
        let Some(s) = self.sessions.get_mut(&id) else {
            return;
        };
        if !s.capabilities.has(PLAYERS_LOOKS) {
            return;
        }
        look.player_id = s.player_id;
        s.look = Some(look.clone());
        self.broadcast_look(Some(id), look);
    }

    /// A departed player's look cleared for the others (a later player may take its id).
    pub(super) fn clear_look(&mut self, player_id: u16) {
        self.broadcast_look(
            None,
            PlayerLook {
                player_id,
                ..PlayerLook::default()
            },
        );
    }

    fn broadcast_look(&mut self, except: Option<SessionId>, look: PlayerLook) {
        let m = Message::Looks(vec![look]);
        let ids: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(o, s)| Some(**o) != except && s.looks_sent)
            .map(|(&o, _)| o)
            .collect();
        for o in ids {
            self.send_reliable(o, &m, Channel::World);
        }
    }

    /// A player's look as the server keeps it (tests).
    pub fn look_of(&self, player_id: u16) -> Option<&PlayerLook> {
        self.sessions
            .values()
            .find(|s| s.player_id == player_id)
            .and_then(|s| s.look.as_ref())
    }
}
