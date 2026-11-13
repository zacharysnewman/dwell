//! A player's map markers on the server (docs/MAP_MARKERS.md, WORK_QUEUE item 53): the record
//! loaded from the world's `markers` table at join, sent to that player only once both sides said
//! `markers.private`, replaced whole by the client's `Markers`, and saved under the player's device
//! key. The server keeps it as the client gave it (validated by `dwell_protocol::markers`); it
//! never sends one player's markers to another.

use dwell_protocol::Message;
use dwell_protocol::capability_names::MARKERS_PRIVATE;
use dwell_protocol::markers::Markers;

use super::*;

impl Server {
    /// The player's saved markers (none: a world from before markers, or a damaged record), then
    /// sent if its client already said the capability.
    pub(super) fn load_markers(&mut self, id: SessionId, key: &[u8; 32]) {
        let saved = self
            .config
            .store
            .as_ref()
            .and_then(|st| st.db().load_markers(key))
            .and_then(|b| Markers::from_bytes(&b));
        if let Some(s) = self.sessions.get_mut(&id) {
            s.markers = saved;
            s.markers_loaded = true;
        }
        self.send_markers(id);
    }

    /// Sends the player its own markers, once, when its record is loaded and its client has
    /// `markers.private`.
    pub(super) fn send_markers(&mut self, id: SessionId) {
        let Some(s) = self.sessions.get_mut(&id) else {
            return;
        };
        if s.markers_sent || !s.markers_loaded || !s.capabilities.has(MARKERS_PRIVATE) {
            return;
        }
        s.markers_sent = true;
        let m = Message::Markers(s.markers.clone().unwrap_or_default());
        self.send_reliable(id, &m, Channel::Control);
    }

    /// The client's markers now, replacing its saved ones (only from a client with the capability).
    pub(super) fn set_markers(&mut self, id: SessionId, m: Markers) {
        if let Some(s) = self.sessions.get_mut(&id)
            && s.joined()
            && s.capabilities.has(MARKERS_PRIVATE)
        {
            s.markers = Some(m);
        }
    }

    /// A session's markers record to save, if it has one.
    pub(super) fn markers_record(s: &Session) -> Option<([u8; 32], Vec<u8>)> {
        s.markers.as_ref().map(|m| (s.public_key, m.to_bytes()))
    }
}
