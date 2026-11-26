//! Players' looks (the Character painter, docs/CHARACTERS.md §9.4; owner, 2026-11-26): each player's
//! paint, drawn on their body for everyone. Sent only with the `players.looks` capability on both
//! sides.
//!
//! The record: `u16 player_id` (ignored C→S), `u8 n` slots (at most [`MAX_SLOTS`]) each a `str`
//! palette ramp name, then the faces' slots as runs: `u16 runs`, each `u16 count` (≥ 1) and `u8 slot`
//! (< n), the counts summing to the painted mesh's faces (at most [`MAX_FACES`]). A client that finds
//! the face count isn't its model's, or a ramp it doesn't know, draws that player as the model is.

use crate::bytes::{ByteReader, ByteWriter};

/// The most colour slots a look has.
pub const MAX_SLOTS: usize = 7;
/// The most faces a look paints.
pub const MAX_FACES: usize = 8192;
/// The longest ramp name (bytes).
pub const NAME_BYTES: usize = 48;
/// The most looks one message carries.
pub const MAX_LOOKS: usize = 1024;

/// One player's look.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlayerLook {
    pub player_id: u16,
    /// Each slot's palette ramp.
    pub slots: Vec<String>,
    /// Each face's slot, in the painted mesh's index order.
    pub faces: Vec<u8>,
}

impl PlayerLook {
    /// Whether it is well formed: slots and faces within bounds, every face's slot a slot.
    pub fn valid(&self) -> bool {
        self.slots.len() <= MAX_SLOTS
            && self
                .slots
                .iter()
                .all(|s| !s.is_empty() && s.len() <= NAME_BYTES)
            && self.faces.len() <= MAX_FACES
            && self
                .faces
                .iter()
                .all(|&f| usize::from(f) < self.slots.len())
    }

    pub fn encode_into(&self, w: &mut ByteWriter) {
        w.u16(self.player_id);
        let n = self.slots.len().min(MAX_SLOTS);
        w.u8(n as u8);
        for s in &self.slots[..n] {
            w.str(crate::bytes::clamp_str(s, NAME_BYTES));
        }
        let faces = &self.faces[..self.faces.len().min(MAX_FACES)];
        let mut runs: Vec<(u16, u8)> = Vec::new();
        for &f in faces {
            match runs.last_mut() {
                Some((c, s)) if *s == f => *c += 1,
                _ => runs.push((1, f)),
            }
        }
        w.u16(runs.len() as u16);
        for (c, s) in runs {
            w.u16(c);
            w.u8(s);
        }
    }

    pub fn decode_from(r: &mut ByteReader) -> Self {
        let player_id = r.u16();
        let n = usize::from(r.u8());
        r.check(n <= MAX_SLOTS);
        let mut slots = Vec::new();
        for _ in 0..n {
            if !r.ok() {
                break;
            }
            slots.push(r.str(NAME_BYTES));
        }
        let runs = usize::from(r.u16());
        r.check(runs <= MAX_FACES);
        let mut faces = Vec::new();
        for _ in 0..runs {
            if !r.ok() {
                break;
            }
            let count = usize::from(r.u16());
            let slot = r.u8();
            r.check(count >= 1 && faces.len() + count <= MAX_FACES && usize::from(slot) < n);
            if !r.ok() {
                break;
            }
            faces.extend(std::iter::repeat_n(slot, count));
        }
        Self {
            player_id,
            slots,
            faces,
        }
    }
}

/// The `Looks` message: C→S your look (one, its id ignored); S→C each changed player's, and every
/// player's once after the client's `Capabilities`.
pub fn encode_looks(looks: &[PlayerLook], w: &mut ByteWriter) {
    let n = looks.len().min(MAX_LOOKS);
    w.u16(n as u16);
    for l in &looks[..n] {
        l.encode_into(w);
    }
}

pub fn decode_looks(r: &mut ByteReader) -> Vec<PlayerLook> {
    let n = usize::from(r.u16());
    r.check(n <= MAX_LOOKS);
    let mut out = Vec::new();
    for _ in 0..n {
        if !r.ok() {
            break;
        }
        out.push(PlayerLook::decode_from(r));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_look_round_trips_in_runs() {
        // face-a-only: bytes, no geometry.
        let mut faces = vec![0u8; 600];
        faces.extend([3u8; 40]);
        faces.extend([6u8; 611]);
        let looks = vec![
            PlayerLook {
                player_id: 7,
                slots: [
                    "skin_fair",
                    "hair_brown",
                    "heartwood",
                    "wool_green",
                    "flint",
                    "wool_red",
                    "brass",
                ]
                .map(String::from)
                .to_vec(),
                faces,
            },
            PlayerLook::default(),
        ];
        let mut out = Vec::new();
        encode_looks(&looks, &mut ByteWriter::new(&mut out));
        assert!(out.len() < 120, "{} bytes", out.len());
        let mut r = ByteReader::new(&out);
        assert_eq!(decode_looks(&mut r), looks);
        assert!(r.ok() && r.at_end());
        assert!(looks.iter().all(PlayerLook::valid));
    }

    #[test]
    fn a_face_past_the_slots_is_refused() {
        // face-a-only: bytes, no geometry.
        let bad = PlayerLook {
            player_id: 1,
            slots: vec!["stone".into()],
            faces: vec![0, 1],
        };
        assert!(!bad.valid());
        let mut out = Vec::new();
        encode_looks(std::slice::from_ref(&bad), &mut ByteWriter::new(&mut out));
        let mut r = ByteReader::new(&out);
        decode_looks(&mut r);
        assert!(!r.ok());
    }
}
