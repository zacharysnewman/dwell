//! A player's map markers in a world (WORK_QUEUE item 53, docs/MAP_MARKERS.md): the icons, colours
//! and labels they placed, and the game's auto markers (spawn, last death, portals), each at a
//! face-local point. One record per player per world, saved in the world's optional `markers` table
//! and sent to that player only (`Markers`, the `markers.private` capability). Its limits are data
//! (shared/ui/markers.json → `markers.gen.rs`); icon and auto kind names are kept as they are, so a
//! name this build doesn't know survives a round trip.
//!
//! The record: `u8 version` (1), `u16 count` (at most [`MAX_MARKERS`]), each marker `u32 id`,
//! `str icon`, `u8 r, g, b`, `str label` (at most [`LABEL_CHARS`] characters), `u8 face` (0 A,
//! 1 B), `f32 x, y, z` (face-local, finite), `str auto` (the auto kind's name; empty: placed by the
//! player). Ids are unique within the record.

use crate::bytes::{ByteReader, ByteWriter};

#[rustfmt::skip]
mod limits {
    include!("markers.gen.rs");
}
pub use limits::*;

/// The record's format version.
pub const VERSION: u8 = 1;

/// One marker.
#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    pub id: u32,
    /// Its icon's name (shared/ui/markers.json `icons[].id`).
    pub icon: String,
    pub rgb: [u8; 3],
    pub label: String,
    /// 0 face A, 1 face B.
    pub face: u8,
    /// Face-local (`dwell_world::FaceFrame::point`).
    pub position: [f32; 3],
    /// The auto kind that placed it (`auto[].kind`); empty: the player's own.
    pub auto: String,
}

/// A player's markers in a world.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Markers(pub Vec<Marker>);

impl Markers {
    pub fn encode_into(&self, w: &mut ByteWriter) {
        w.u8(VERSION);
        w.u16(self.0.len() as u16);
        for m in &self.0 {
            w.u32(m.id);
            w.str(&m.icon);
            for c in m.rgb {
                w.u8(c);
            }
            w.str(&m.label);
            w.u8(m.face);
            for v in m.position {
                w.f32(v);
            }
            w.str(&m.auto);
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode_into(&mut ByteWriter::new(&mut out));
        out
    }

    /// Reads a record from `r`, failing it (`r.ok()` false) on a bad one: an unknown version, too
    /// many markers, a long name or label, a face past B, a point not finite, an id twice.
    pub fn decode_from(r: &mut ByteReader) -> Self {
        let version = r.u8();
        r.check(version == VERSION);
        let n = usize::from(r.u16());
        r.check(n <= MAX_MARKERS);
        let mut out = Vec::new();
        let mut ids = std::collections::HashSet::new();
        for _ in 0..n {
            if !r.ok() {
                break;
            }
            let id = r.u32();
            r.check(ids.insert(id));
            let icon = r.str(NAME_BYTES);
            r.check(!icon.is_empty());
            let rgb = [r.u8(), r.u8(), r.u8()];
            let label = r.str(LABEL_CHARS * 4);
            r.check(label.chars().count() <= LABEL_CHARS);
            let face = r.u8();
            r.check(face <= 1);
            let position = [r.f32(), r.f32(), r.f32()];
            r.check(position.iter().all(|v| v.is_finite()));
            let auto = r.str(NAME_BYTES);
            out.push(Marker {
                id,
                icon,
                rgb,
                label,
                face,
                position,
                auto,
            });
        }
        Self(out)
    }

    /// A whole record, or none if it is malformed or has bytes after it.
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        let mut r = ByteReader::new(b);
        let m = Self::decode_from(&mut r);
        r.at_end().then_some(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn sample() -> Markers {
        Markers(vec![
            Marker {
                id: 1,
                icon: "home".into(),
                rgb: [0xe8, 0x32, 0x2a],
                label: "Base".into(),
                face: 0,
                position: [12.5, 70.0, -3.0],
                auto: String::new(),
            },
            Marker {
                id: 7,
                icon: "skull".into(),
                rgb: [0xf4, 0xf1, 0xea],
                label: "Last death".into(),
                face: 1,
                position: [-40.0, 66.5, 900.25],
                auto: "death".into(),
            },
        ])
    }

    #[test]
    fn a_record_round_trips() {
        let m = sample();
        assert_eq!(Markers::from_bytes(&m.to_bytes()), Some(m));
        assert_eq!(
            Markers::from_bytes(&Markers::default().to_bytes()),
            Some(Markers::default())
        );
    }

    #[test]
    fn an_unknown_icon_name_survives() {
        let mut m = sample();
        m.0[0].icon = "mymod:anchor".into();
        assert_eq!(Markers::from_bytes(&m.to_bytes()), Some(m));
    }

    #[test]
    fn bad_records_are_refused() {
        let bad = |edit: &dyn Fn(&mut Markers)| {
            let mut m = sample();
            edit(&mut m);
            assert_eq!(Markers::from_bytes(&m.to_bytes()), None, "{m:?}");
        };
        bad(&|m| m.0[1].id = 1);
        bad(&|m| m.0[0].label = "x".repeat(LABEL_CHARS + 1));
        bad(&|m| m.0[0].icon = String::new());
        bad(&|m| m.0[0].icon = "i".repeat(NAME_BYTES + 1));
        bad(&|m| m.0[0].face = 2);
        bad(&|m| m.0[0].position[1] = f32::NAN);
        bad(&|m| {
            let one = m.0[0].clone();
            m.0 = (0..=MAX_MARKERS as u32)
                .map(|id| Marker { id, ..one.clone() })
                .collect();
        });
        let mut b = sample().to_bytes();
        b[0] = 2;
        assert_eq!(Markers::from_bytes(&b), None, "an unknown version");
        let mut b = sample().to_bytes();
        b.push(0);
        assert_eq!(Markers::from_bytes(&b), None, "trailing bytes");
        let b = sample().to_bytes();
        assert_eq!(Markers::from_bytes(&b[..b.len() - 1]), None, "truncated");
    }

    #[test]
    fn a_label_of_the_most_characters_is_kept_even_in_wide_letters() {
        let mut m = sample();
        m.0[0].label = "é".repeat(LABEL_CHARS);
        assert_eq!(Markers::from_bytes(&m.to_bytes()), Some(m));
    }
}
