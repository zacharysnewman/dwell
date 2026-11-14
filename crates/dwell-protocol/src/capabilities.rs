//! Capabilities (ARCHITECTURE.md §8.3 "Capabilities"): optional features named by string
//! (`shared/protocol/capabilities.json`, its names generated into [`crate::capability_names`]),
//! announced at join. **A new optional feature adds a capability, not a flag bit.**
//!
//! The exchange, safe with every build of the line (an unknown *control* message ends the session
//! on either side, so neither sends one unasked):
//!
//! 1. The server lists the capabilities it has in an optional `WorldSettings` field
//!    (`world_settings::tag::CAPABILITIES`, before `Welcome`). A build that doesn't know the tag
//!    skips it, as any optional field. The field is the join's, never saved with the world.
//! 2. A client that sees the field answers, after `Welcome`, with a `Capabilities` message: the
//!    capabilities it has. A client never sends one to a server whose settings had no such field
//!    (an older server), and an older client never sends one.
//! 3. A feature is used on a session only when **both** sides listed its name
//!    ([`Capabilities::has`] on the peer's set; the server checks the client's, the client the
//!    server's).
//!
//! The form, in the settings field and the message alike: `u8 count` (at most
//! `capability_names::MAX_NAMES`), each a `str` (u16 length, UTF-8, at most `MAX_NAME_BYTES`,
//! lowercase `[a-z0-9_.:-]`, no name twice). A name this build doesn't know (a newer build's, a
//! mod's) is kept and simply never matches a feature here.

use crate::bytes::{ByteReader, ByteWriter};
use crate::capability_names::{ALL, MAX_NAME_BYTES, MAX_NAMES};

/// Capabilities this build's server has and its client doesn't say yet (the client's side still to
/// come). None now: `grid.frames` (docs/proposals/MOON_WORLD.md §7.3) left it with M-7c, the game
/// client following its player onto another grid (its chunks, its frame, its sun).
pub const CLIENT_PENDING: &[&str] = &[];

/// A set of capability names, in the order they arrived (or the table's).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Capabilities(Vec<String>);

/// A well-formed capability name: 1..=`MAX_NAME_BYTES` of `[a-z0-9_.:-]`, starting with a letter or
/// digit.
pub fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= MAX_NAME_BYTES
        && n.as_bytes()[0].is_ascii_alphanumeric()
        && n.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b':' | b'-')
        })
}

impl Capabilities {
    /// Every capability this build has (the table's).
    pub fn this_build() -> Self {
        Self(ALL.iter().map(|s| s.to_string()).collect())
    }

    /// The capabilities this build's client says (`this_build` without [`CLIENT_PENDING`]).
    pub fn this_client() -> Self {
        Self(
            ALL.iter()
                .filter(|n| !CLIENT_PENDING.contains(n))
                .map(|s| s.to_string())
                .collect(),
        )
    }

    /// These names (badly formed ones and repeats dropped, at most `MAX_NAMES`).
    pub fn of<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        let mut out: Vec<String> = Vec::new();
        for n in names {
            if out.len() < MAX_NAMES && valid_name(n) && !out.iter().any(|o| o == n) {
                out.push(n.to_string());
            }
        }
        Self(out)
    }

    /// Whether `name` is in the set.
    pub fn has(&self, name: &str) -> bool {
        self.0.iter().any(|n| n == name)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn write(&self, w: &mut ByteWriter) {
        let n = self.0.len().min(MAX_NAMES);
        w.u8(n as u8);
        for name in &self.0[..n] {
            w.str(name);
        }
    }

    /// Reads the form; `None` (the reader failed) when it isn't one.
    pub fn read(r: &mut ByteReader) -> Option<Self> {
        let n = usize::from(r.u8());
        r.check(n <= MAX_NAMES);
        let mut out: Vec<String> = Vec::new();
        for _ in 0..n {
            if !r.ok() {
                return None;
            }
            let name = r.str(MAX_NAME_BYTES);
            r.check(valid_name(&name) && !out.contains(&name));
            out.push(name);
        }
        r.ok().then_some(Self(out))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write(&mut ByteWriter::new(&mut out));
        out
    }

    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        let mut r = ByteReader::new(b);
        let c = Self::read(&mut r)?;
        r.at_end().then_some(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability_names::{GRID_FRAMES, STRUCTURES_PLACE};

    #[test]
    fn this_build_has_the_tables_names_and_they_round_trip() {
        let c = Capabilities::this_build();
        assert!(c.has(STRUCTURES_PLACE));
        assert!(!c.has("structures"));
        assert_eq!(Capabilities::from_bytes(&c.to_bytes()), Some(c.clone()));
        // Every name in the table fits one WorldSettings field (the server announces them there).
        assert!(c.to_bytes().len() <= crate::world_settings::MAX_FIELD_BYTES);
        assert!(ALL.iter().all(|n| valid_name(n)));
    }

    #[test]
    fn the_client_says_all_but_the_pending_ones() {
        assert!(!CLIENT_PENDING.contains(&GRID_FRAMES));
        let c = Capabilities::this_client();
        assert!(c.has(STRUCTURES_PLACE));
        assert!(
            c.has(GRID_FRAMES),
            "the client follows its player onto the moon (M-7c)"
        );
        assert!(Capabilities::this_build().has(GRID_FRAMES));
        assert!(CLIENT_PENDING.iter().all(|n| ALL.contains(n)));
    }

    #[test]
    fn a_name_this_build_doesnt_know_is_kept_and_matches_nothing() {
        let c = Capabilities::of(["mymod:wet.blocks", STRUCTURES_PLACE]);
        let back = Capabilities::from_bytes(&c.to_bytes()).expect("the form");
        assert!(back.has("mymod:wet.blocks") && back.has(STRUCTURES_PLACE));
        assert_eq!(back.names().count(), 2);
    }

    #[test]
    fn a_bad_form_is_refused() {
        let ok = Capabilities::of(["a.b"]).to_bytes();
        assert!(
            Capabilities::from_bytes(&ok[..ok.len() - 1]).is_none(),
            "truncated"
        );
        let mut trailing = ok.clone();
        trailing.push(0);
        assert!(Capabilities::from_bytes(&trailing).is_none(), "trailing");
        let raw = |names: &[&str]| {
            let mut b = vec![names.len() as u8];
            for n in names {
                b.extend((n.len() as u16).to_le_bytes());
                b.extend(n.as_bytes());
            }
            b
        };
        assert!(
            Capabilities::from_bytes(&raw(&["a.b", "a.b"])).is_none(),
            "twice"
        );
        assert!(Capabilities::from_bytes(&raw(&["Upper"])).is_none(), "case");
        assert!(Capabilities::from_bytes(&raw(&[""])).is_none(), "empty");
        let long = "x".repeat(MAX_NAME_BYTES + 1);
        assert!(
            Capabilities::from_bytes(&raw(&[&long])).is_none(),
            "too long"
        );
        let mut many = vec![(MAX_NAMES + 1) as u8];
        for i in 0..=MAX_NAMES {
            let n = format!("n{i}");
            many.extend((n.len() as u16).to_le_bytes());
            many.extend(n.as_bytes());
        }
        assert!(Capabilities::from_bytes(&many).is_none(), "too many");
        // `of` drops what the form would refuse.
        assert_eq!(
            Capabilities::of(["a.b", "a.b", "Bad", ""]),
            Capabilities::of(["a.b"])
        );
    }
}
