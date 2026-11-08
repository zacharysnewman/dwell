//! Game modes (ADR 0065): what a world's players may do, as data (`shared/modes/modes.json`,
//! generated into `modes.gen.rs` by `shared/modes/gen.mjs`). A mode is a name and the named
//! abilities it allows; the abilities are this module's small fixed vocabulary. A world's mode is
//! one of its settings (`crate::world_settings`, the optional `MODE` field): sent at join, saved
//! with the world, changed by the host. The server enforces it; clients only hide what it doesn't
//! allow.

#[rustfmt::skip]
mod table {
    include!("modes.gen.rs");
}

pub use table::{DEFAULT_MODE, MODES};

/// What a mode may allow (`abilities` in the data). New abilities are code: a variant here, the
/// name in `shared/modes/gen.mjs`'s `ABILITIES`, and the check where it applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ability {
    /// Players are hurt: falls, blasts, falling pieces (every `DamageCause`).
    Damage,
    /// Creative flight (PLAYER_CONTROLLER.md §6.7).
    Flight,
    /// The brush tools (docs/BRUSHES.md) and their ranged reach.
    Brushes,
    /// Blocks are not used up by placing them, and breaking drops nothing (ADR 0066); without
    /// it, placing takes an item from the player's inventory and breaking drops the block's items.
    UnlimitedBlocks,
}

impl Ability {
    pub const ALL: [Ability; 4] = [
        Ability::Damage,
        Ability::Flight,
        Ability::Brushes,
        Ability::UnlimitedBlocks,
    ];
    /// The data's name (`shared/modes/gen.mjs` `ABILITIES`).
    pub const fn name(self) -> &'static str {
        match self {
            Ability::Damage => "damage",
            Ability::Flight => "flight",
            Ability::Brushes => "brushes",
            Ability::UnlimitedBlocks => "unlimitedBlocks",
        }
    }
    pub const fn bit(self) -> u32 {
        1 << self as u32
    }
}

/// A game mode: its name (how worlds refer to it), its label, and the abilities it allows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameMode {
    pub name: &'static str,
    pub label: &'static str,
    /// Offered, but marked experimental (the New world form).
    pub experimental: bool,
    /// `Ability::bit`s.
    pub abilities: u32,
}

impl GameMode {
    pub const fn allows(&self, a: Ability) -> bool {
        self.abilities & a.bit() != 0
    }
}

/// The mode of that name.
pub fn mode(name: &str) -> Option<&'static GameMode> {
    mode_in(MODES, name)
}

/// [`mode`] over a table of modes (the data's, or a test's fixture).
pub fn mode_in<'a>(modes: &'a [GameMode], name: &str) -> Option<&'a GameMode> {
    modes.iter().find(|m| m.name == name)
}

/// The mode a world plays when it names none, or one this build doesn't know (a newer build's):
/// `DEFAULT_MODE`.
pub fn default_mode() -> &'static GameMode {
    mode(DEFAULT_MODE).expect("defaultMode names a mode (shared/modes/gen.mjs checks it)")
}

/// The mode of that name, or the default one.
pub fn mode_or_default(name: &str) -> &'static GameMode {
    mode(name).unwrap_or_else(default_mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creative_is_the_default_and_allows_no_damage_adventure_hurts_without_flight_or_brushes() {
        let c = default_mode();
        assert_eq!(c.name, "creative");
        assert!(!c.allows(Ability::Damage));
        assert!(c.allows(Ability::Flight) && c.allows(Ability::Brushes));
        let a = mode("adventure").expect("adventure is in the data");
        assert!(a.experimental);
        assert!(a.allows(Ability::Damage));
        assert!(!a.allows(Ability::Flight) && !a.allows(Ability::Brushes));
        // ADR 0066: adventure uses items up and drops them; creative has unlimited blocks.
        assert!(c.allows(Ability::UnlimitedBlocks) && !a.allows(Ability::UnlimitedBlocks));
        assert_eq!(mode_or_default("a_newer_builds_mode").name, "creative");
    }

    #[test]
    fn a_mode_added_by_data_alone_is_looked_up_by_name() {
        let fixture = [
            *default_mode(),
            GameMode {
                name: "spectator",
                label: "Spectator",
                experimental: false,
                abilities: Ability::Flight.bit(),
            },
        ];
        let s = mode_in(&fixture, "spectator").expect("by name");
        assert!(s.allows(Ability::Flight));
        assert!(!s.allows(Ability::Damage) && !s.allows(Ability::Brushes));
        assert_eq!(mode_in(&fixture, "adventure"), None);
    }

    #[test]
    fn the_generators_vocabulary_is_this_one_in_order() {
        let script = include_str!("../../../shared/modes/gen.mjs");
        let names: Vec<String> = Ability::ALL
            .iter()
            .map(|a| format!("'{}'", a.name()))
            .collect();
        assert!(
            script.contains(&format!("ABILITIES = [{}];", names.join(", "))),
            "shared/modes/gen.mjs ABILITIES must list {names:?}"
        );
    }

    #[test]
    fn the_abilities_bits_are_distinct_and_named() {
        let mut all = 0;
        for a in Ability::ALL {
            assert_eq!(all & a.bit(), 0);
            all |= a.bit();
            assert!(!a.name().is_empty());
        }
    }
}
