//! World settings (docs/DAY_NIGHT.md §2, docs/WATER_FLOW.md §4.1, ADR 0065): what a world sets for
//! every player in it — its clock, its water speed, its game mode — sent by the server at join (`WorldSettings`, before
//! `Welcome`) and kept in the world file's meta in the same form. The clients use the server's,
//! so changing one is a world's choice, not a compatibility change.
//!
//! The form is tagged fields, as the block registry's (ADR 0038):
//!
//! ```text
//! u8 field count, each: u8 tag, u16 length, bytes
//! ```
//!
//! A tag with the top bit set is optional: a build that doesn't know it skips it. Any other must be
//! understood: a build that doesn't know it can't join that world (and says so). New settings are
//! added as optional tags whenever an older build on the line can do without them, so adding one
//! needs no new compatibility line.

use crate::bytes::{ByteReader, ByteWriter};

/// Field tags. The top bit marks an optional field.
pub mod tag {
    pub const OPTIONAL: u8 = 0x80;
    /// The day/night clock: `f32` seconds of daylight, `f32` seconds of night (face A), `f32` the
    /// 24-hour clock at world tick 0.
    pub const CLOCK: u8 = 1;
    /// `f32` water speed: water time is game time × this (docs/WATER_FLOW.md §4.1).
    pub const WATER_SPEED: u8 = 0x81;
    /// The game mode's name (`crate::modes`, UTF-8; ADR 0065). Optional: a build before modes
    /// skips it (its world plays as it always did, creative), and a name this build doesn't know
    /// (a newer build's mode) reads as `modes::DEFAULT_MODE`. The server enforces the mode; a
    /// client only predicts and hides with it.
    pub const MODE: u8 = 0x82;
    /// `u64` (LE) FNV-1a hash of the world's worldgen config text (ADR 0074 B7: a world made from
    /// a Chisel edit or a fixture). Required: a build before it can't make the world's terrain, so
    /// it refuses ("needs a newer version") instead of generating the seed's own. A client that
    /// knows it sends `WorldgenRequest`, gets the text (`WorldgenConfig`), checks the hash and
    /// builds its maps from it. Sent only for such a world (a default world's join is unchanged),
    /// and never saved in the world file (the server adds it from its world's maps).
    pub const WORLDGEN: u8 = 2;
}

/// The most fields, and the longest field.
pub const MAX_FIELDS: usize = 64;
pub const MAX_FIELD_BYTES: usize = 1024;

/// A world's clock (docs/DAY_NIGHT.md §2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldClock {
    pub day_s: f32,
    pub night_s: f32,
    pub start_hour: f32,
}

impl WorldClock {
    /// A whole day and night (s).
    pub fn day_length_s(self) -> f64 {
        f64::from(self.day_s) + f64::from(self.night_s)
    }
    fn valid(self) -> bool {
        self.day_s.is_finite()
            && self.night_s.is_finite()
            && self.day_s > 0.0
            && self.night_s > 0.0
            && self.day_length_s() <= 86_400.0 * 365.0
            && (0.0..24.0).contains(&self.start_hour)
    }
}

/// The settings this build knows, read from a world's fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldSettings {
    pub clock: WorldClock,
    pub water_speed: f32,
    /// The game mode's name: always one of `crate::modes::MODES`.
    pub mode: &'static str,
}

impl WorldSettings {
    /// The world's game mode (ADR 0065).
    pub fn game_mode(&self) -> &'static crate::modes::GameMode {
        crate::modes::mode_or_default(self.mode)
    }
    /// Whether the world's mode allows `a`.
    pub fn allows(&self, a: crate::modes::Ability) -> bool {
        self.game_mode().allows(a)
    }
}

/// Why a world's fields can't be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsError {
    /// Not the tagged form (truncated, too many fields, a field too long, a tag twice).
    Malformed,
    /// A field this build must understand and doesn't: a newer build's world.
    Unknown(u8),
    /// A known field out of range.
    Invalid(u8),
}

/// A world's fields as they arrived: kept whole so a field this build doesn't know is passed on
/// (saved again, sent to players) untouched.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsFields(pub Vec<(u8, Vec<u8>)>);

impl SettingsFields {
    pub fn write(&self, w: &mut ByteWriter) {
        w.u8(self.0.len() as u8);
        for (t, b) in &self.0 {
            w.u8(*t);
            w.u16(b.len() as u16);
            w.bytes(b);
        }
    }
    /// Reads the tagged form; `None` when it isn't one.
    pub fn read(r: &mut ByteReader) -> Option<Self> {
        let n = usize::from(r.u8());
        r.check(n <= MAX_FIELDS);
        let mut out: Vec<(u8, Vec<u8>)> = Vec::new();
        for _ in 0..n {
            if !r.ok() {
                return None;
            }
            let t = r.u8();
            let len = usize::from(r.u16());
            r.check(len <= MAX_FIELD_BYTES && !out.iter().any(|(o, _)| *o == t));
            if !r.ok() {
                return None;
            }
            out.push((t, r.bytes(len)));
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
        let f = Self::read(&mut r)?;
        r.at_end().then_some(f)
    }

    fn get(&self, t: u8) -> Option<&[u8]> {
        self.0
            .iter()
            .find(|(o, _)| *o == t)
            .map(|(_, b)| b.as_slice())
    }

    /// The settings, a missing optional field taken from `defaults`.
    pub fn settings(&self, defaults: WorldSettings) -> Result<WorldSettings, SettingsError> {
        for (t, _) in &self.0 {
            if *t & tag::OPTIONAL == 0 && *t != tag::CLOCK && *t != tag::WORLDGEN {
                return Err(SettingsError::Unknown(*t));
            }
        }
        if self.get(tag::WORLDGEN).is_some_and(|b| b.len() != 8) {
            return Err(SettingsError::Invalid(tag::WORLDGEN));
        }
        let mut s = defaults;
        let Some(c) = self.get(tag::CLOCK) else {
            return Err(SettingsError::Malformed); // required
        };
        let mut r = ByteReader::new(c);
        s.clock = WorldClock {
            day_s: r.f32(),
            night_s: r.f32(),
            start_hour: r.f32(),
        };
        if !r.at_end() || !s.clock.valid() {
            return Err(SettingsError::Invalid(tag::CLOCK));
        }
        if let Some(b) = self.get(tag::WATER_SPEED) {
            let mut r = ByteReader::new(b);
            s.water_speed = r.f32();
            if !r.at_end() || !s.water_speed.is_finite() || !(0.0..=1000.0).contains(&s.water_speed)
            {
                return Err(SettingsError::Invalid(tag::WATER_SPEED));
            }
        }
        if let Some(b) = self.get(tag::MODE) {
            // A mode this build doesn't know (a newer build's) plays as the default one.
            let name = std::str::from_utf8(b).map_err(|_| SettingsError::Invalid(tag::MODE))?;
            s.mode = crate::modes::mode_or_default(name).name;
        }
        Ok(s)
    }

    /// The world's worldgen config hash (`tag::WORLDGEN`), when it names one.
    pub fn worldgen_hash(&self) -> Option<u64> {
        let b: [u8; 8] = self.get(tag::WORLDGEN)?.try_into().ok()?;
        Some(u64::from_le_bytes(b))
    }

    /// These fields with `tag::WORLDGEN` set to `hash` (the join's, ADR 0074 B7), or without it.
    pub fn with_worldgen(&self, hash: Option<u64>) -> Self {
        let mut out: Vec<(u8, Vec<u8>)> = self
            .0
            .iter()
            .filter(|(t, _)| *t != tag::WORLDGEN)
            .cloned()
            .collect();
        if let Some(h) = hash {
            out.push((tag::WORLDGEN, h.to_le_bytes().to_vec()));
            out.sort_by_key(|(t, _)| *t);
        }
        Self(out)
    }

    /// These fields with the known ones set to `s` (fields this build doesn't know kept).
    pub fn with(&self, s: WorldSettings) -> Self {
        let mut out: Vec<(u8, Vec<u8>)> = self
            .0
            .iter()
            .filter(|(t, _)| ![tag::CLOCK, tag::WATER_SPEED, tag::MODE, tag::WORLDGEN].contains(t))
            .cloned()
            .collect();
        let field = |f: &dyn Fn(&mut ByteWriter)| {
            let mut b = Vec::new();
            f(&mut ByteWriter::new(&mut b));
            b
        };
        out.push((
            tag::CLOCK,
            field(&|w| {
                w.f32(s.clock.day_s);
                w.f32(s.clock.night_s);
                w.f32(s.clock.start_hour);
            }),
        ));
        out.push((tag::WATER_SPEED, field(&|w| w.f32(s.water_speed))));
        out.push((tag::MODE, s.game_mode().name.as_bytes().to_vec()));
        out.sort_by_key(|(t, _)| *t);
        Self(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: WorldSettings = WorldSettings {
        clock: WorldClock {
            day_s: 800.0,
            night_s: 400.0,
            start_hour: 7.0,
        },
        water_speed: 1.0,
        mode: "creative",
    };

    #[test]
    fn settings_round_trip_through_their_fields() {
        let s = WorldSettings {
            clock: WorldClock {
                day_s: 600.0,
                night_s: 300.0,
                start_hour: 12.0,
            },
            water_speed: 4.0,
            mode: "adventure",
        };
        let f = SettingsFields::default().with(s);
        let back = SettingsFields::from_bytes(&f.to_bytes()).expect("the tagged form");
        assert_eq!(back.settings(D), Ok(s));
    }

    #[test]
    fn an_unknown_optional_field_is_skipped_and_kept_an_unknown_required_one_refuses() {
        let mut f = SettingsFields::default().with(D);
        f.0.push((0x9f, vec![1, 2, 3]));
        assert_eq!(f.settings(D), Ok(D));
        let changed = f.with(WorldSettings {
            water_speed: 2.0,
            ..D
        });
        assert!(
            changed.0.contains(&(0x9f, vec![1, 2, 3])),
            "kept for a newer build"
        );
        f.0.push((0x1f, vec![]));
        assert_eq!(f.settings(D), Err(SettingsError::Unknown(0x1f)));
    }

    #[test]
    fn a_missing_optional_field_takes_the_default_a_bad_one_is_refused() {
        let f = SettingsFields(
            SettingsFields::default()
                .with(D)
                .0
                .into_iter()
                .filter(|(t, _)| *t == tag::CLOCK)
                .collect(),
        );
        assert_eq!(f.settings(D).map(|s| s.water_speed), Ok(D.water_speed));
        let bad = SettingsFields::default().with(WorldSettings {
            clock: WorldClock {
                day_s: -1.0,
                ..D.clock
            },
            ..D
        });
        assert_eq!(bad.settings(D), Err(SettingsError::Invalid(tag::CLOCK)));
        assert_eq!(SettingsFields::from_bytes(&[2, 1, 0, 0]), None, "truncated");
    }

    #[test]
    fn the_mode_is_an_optional_field_a_build_before_modes_skips() {
        let adventure = WorldSettings {
            mode: "adventure",
            ..D
        };
        let f = SettingsFields::default().with(adventure);
        assert!(
            f.0.iter()
                .any(|(t, b)| *t == tag::MODE && b == b"adventure")
        );
        assert_ne!(
            tag::MODE & tag::OPTIONAL,
            0,
            "optional: no new compatibility line"
        );
        // A build before modes reads the same fields with its known ones only: the mode field is
        // skipped as an unknown optional one, and its world plays as before (creative).
        let older = |f: &SettingsFields| {
            for (t, _) in &f.0 {
                if *t & tag::OPTIONAL == 0 && *t != tag::CLOCK {
                    return Err(SettingsError::Unknown(*t));
                }
            }
            Ok(())
        };
        assert_eq!(older(&f), Ok(()));
        // A world saved before modes has no mode field: the default (creative).
        let before = SettingsFields(
            f.0.iter()
                .filter(|(t, _)| *t != tag::MODE)
                .cloned()
                .collect(),
        );
        assert_eq!(
            before.settings(adventure).map(|s| s.mode),
            Ok("adventure"),
            "the caller's default"
        );
        assert_eq!(before.settings(D).map(|s| s.mode), Ok("creative"));
        // A newer build's mode, unknown here, plays as the default.
        let mut newer = before.clone();
        newer.0.push((tag::MODE, b"hardcore".to_vec()));
        assert_eq!(newer.settings(adventure).map(|s| s.mode), Ok("creative"));
        newer.0.retain(|(t, _)| *t != tag::MODE);
        newer.0.push((tag::MODE, vec![0xff, 0xfe]));
        assert_eq!(newer.settings(D), Err(SettingsError::Invalid(tag::MODE)));
    }

    #[test]
    fn the_worldgen_hash_is_a_required_field_an_older_build_refuses() {
        let f = SettingsFields::default().with(D);
        assert_eq!(f.worldgen_hash(), None, "a default world names none");
        assert_eq!(f.with_worldgen(None), f, "and its fields are unchanged");
        let g = f.with_worldgen(Some(0x0123_4567_89ab_cdef));
        assert_eq!(tag::WORLDGEN & tag::OPTIONAL, 0, "required");
        assert_eq!(g.worldgen_hash(), Some(0x0123_4567_89ab_cdef));
        assert_eq!(g.settings(D), Ok(D), "this build knows it");
        let back = SettingsFields::from_bytes(&g.to_bytes()).expect("the tagged form");
        assert_eq!(back.worldgen_hash(), Some(0x0123_4567_89ab_cdef));
        // A build before it reads the same fields as one with an unknown required tag.
        let older = |f: &SettingsFields| {
            for (t, _) in &f.0 {
                if *t & tag::OPTIONAL == 0 && *t != tag::CLOCK {
                    return Err(SettingsError::Unknown(*t));
                }
            }
            Ok(())
        };
        assert_eq!(older(&g), Err(SettingsError::Unknown(tag::WORLDGEN)));
        // Never saved: setting the known fields drops it.
        assert_eq!(g.with(D).worldgen_hash(), None);
        let mut bad = f.clone();
        bad.0.push((tag::WORLDGEN, vec![1, 2, 3]));
        assert_eq!(bad.settings(D), Err(SettingsError::Invalid(tag::WORLDGEN)));
    }
}
