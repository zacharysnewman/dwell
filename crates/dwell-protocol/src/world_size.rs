//! A world's size (docs/proposals/BIG_MAP.md §6 M0; owner, 2026-11-11): the radius as one value,
//! per world. A size is a whole `radius_scale` times today's disc (`world::WORLD_RADIUS`) and island
//! (`world::ISLAND_RADIUS`); the named sizes are data (`constants.json` `worldSizes`, today 1×, 2×,
//! 4×, 8× and 10×). A world carries its size in its settings (`world_settings::tag::WORLD_SIZE`, an
//! optional tag, ADR 0054), saved with it and sent at join; a world without the tag is the default
//! size, today's.
//!
//! Everything that follows the radius is derived here, so one value moves them all and every
//! derived value at scale 1 is today's:
//!
//! - the LOD's levels ([`WorldSize::lod_max_level`]: the fewest that hold the disc);
//! - the big map's coarse levels' cells ([`WorldSize::map_cell_m`]: × the next power of two, so no
//!   level builds more tiles than today's);
//! - the hydrology grid's cell ([`WorldSize::hydrology_cell_m`]: × the scale, so its whole-map grid
//!   stays at today's ~1 M cells: a 10× world's map is at 1 km, the fine detail the per-chunk
//!   generators');
//! - the island shape's absolute lengths ([`WorldSize::island_length`]: × the scale, the same
//!   island bigger);
//! - the uplift layer's counts ([`WorldSize::uplift_count`]: × the area, the same density of
//!   ranges and plates).
//!
//! The LOD octree's root is compiled (`lod::MAX_LEVEL`) to hold the largest named size (BIG_MAP.md
//! M1, checked by `shared/protocol/gen.mjs`), so every named size fits it
//! ([`WorldSize::fits_lod_root`]); a world's own top level is [`WorldSize::lod_max_level`].

use crate::constants::{lod, world, world_sizes};

/// A world's size: a whole multiple of today's radii.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WorldSize {
    pub radius_scale: u32,
}

/// Why a size can't be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeError {
    /// Not a whole scale `1..=MAX_RADIUS_SCALE`.
    Scale(u32),
    /// No size of that name in `worldSizes`.
    Name,
}

impl Default for WorldSize {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl WorldSize {
    /// Today's size: `worldSizes.default` (checked by `shared/protocol/gen.mjs` to be scale 1).
    pub const DEFAULT: Self = Self { radius_scale: 1 };
    /// The largest named size (`lod.maxLevel` holds its disc).
    pub const LARGEST: Self = {
        let mut i = 0;
        let mut max = 1;
        while i < world_sizes::SIZES.len() {
            if world_sizes::SIZES[i].1 > max {
                max = world_sizes::SIZES[i].1;
            }
            i += 1;
        }
        Self { radius_scale: max }
    };

    /// A size of a whole scale (a newer build's unnamed scale is still a size this build can derive).
    pub fn scaled(radius_scale: u32) -> Result<Self, SizeError> {
        if (1..=world_sizes::MAX_RADIUS_SCALE).contains(&radius_scale) {
            Ok(Self { radius_scale })
        } else {
            Err(SizeError::Scale(radius_scale))
        }
    }
    /// A named size (`worldSizes.sizes`).
    pub fn named(name: &str) -> Result<Self, SizeError> {
        let (_, s) = world_sizes::SIZES
            .iter()
            .find(|(n, _)| *n == name)
            .ok_or(SizeError::Name)?;
        Self::scaled(*s)
    }
    /// Every named size, in the data's order.
    pub fn all() -> impl Iterator<Item = (&'static str, Self)> {
        world_sizes::SIZES
            .iter()
            .map(|&(n, s)| (n, Self { radius_scale: s }))
    }
    /// This size's name, when the data names it.
    pub fn name(self) -> Option<&'static str> {
        Self::all().find(|(_, s)| *s == self).map(|(n, _)| n)
    }
    pub fn is_default(self) -> bool {
        self == Self::DEFAULT
    }
    /// The size a radius scale names for this build to play (BIG_MAP.md M2b: the browser's
    /// workers are handed the scale as a number): today's for none, an unusable scale, or one
    /// this build's LOD root can't hold, as a saved world's size is read.
    pub fn playable(radius_scale: Option<u32>) -> Self {
        radius_scale
            .and_then(|s| Self::scaled(s).ok())
            .filter(|s| s.fits_lod_root())
            .unwrap_or(Self::DEFAULT)
    }

    /// The disc's radius (m).
    pub const fn world_radius_m(self) -> i64 {
        world::WORLD_RADIUS * self.radius_scale as i64
    }
    /// The island's mean radius (m).
    pub const fn island_radius_m(self) -> i64 {
        world::ISLAND_RADIUS * self.radius_scale as i64
    }

    /// The fewest LOD levels whose root (`SECTION_CELLS << level` wide, centred on the origin)
    /// holds the disc: 12 at today's 50 km (131 km), 15 at 10× (1 049 km).
    pub const fn lod_max_level(self) -> u32 {
        let mut level = 0;
        while (lod::SECTION_CELLS << level) / 2 < self.world_radius_m() {
            level += 1;
        }
        level
    }
    /// Whether this build's compiled LOD root holds the disc: every named size since BIG_MAP.md M1;
    /// an unnamed scale above the largest does not (a newer build's world this one can't draw).
    pub const fn fits_lod_root(self) -> bool {
        self.lod_max_level() as i64 <= lod::MAX_LEVEL
    }

    /// The scale rounded up to a power of two: what a power-of-two cell (the big map's) grows by.
    pub const fn pow2_scale(self) -> u32 {
        self.radius_scale.next_power_of_two()
    }
    /// A big-map level's cell (m) for its data `cell_m` at today's size: a level that reaches the
    /// island or the disc follows the radius by the next power of two (a whole power of two still,
    /// and no more tiles than today); the player's level (`follows_radius` false) is unchanged.
    pub const fn map_cell_m(self, cell_m: u32, follows_radius: bool) -> u32 {
        if follows_radius {
            cell_m * self.pow2_scale()
        } else {
            cell_m
        }
    }
    /// The hydrology grid's cell (m) for the profile's `cell` at today's size: × the scale, so the
    /// whole-map grid keeps today's cell count (BIG_MAP.md §5: 1 000 m at 10×; small rivers and
    /// lakes below a cell wait for the regional maps, R1–R4).
    pub const fn hydrology_cell_m(self, cell: i32) -> i32 {
        cell * self.radius_scale as i32
    }
    /// One of the island shape's absolute lengths (m: its radius variation, warp, coast detail
    /// and their wavelengths) at this size: × the scale, so a bigger world's island is today's
    /// shape, bigger. Exact at scale 1.
    pub fn island_length(self, m: f32) -> f32 {
        m * self.radius_scale as f32
    }
    /// An island wavelength (whole m) at this size.
    pub const fn island_wavelength(self, m: i32) -> i32 {
        m * self.radius_scale as i32
    }
    /// One of the uplift layer's counts (ranges, plates) at this size: × the island's area, so the
    /// land keeps today's density of them.
    pub const fn uplift_count(self, n: u32) -> u32 {
        n * self.radius_scale * self.radius_scale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_size_is_todays_and_named_in_the_data() {
        assert_eq!(
            WorldSize::named(world_sizes::DEFAULT),
            Ok(WorldSize::DEFAULT)
        );
        let d = WorldSize::DEFAULT;
        assert_eq!(d.world_radius_m(), world::WORLD_RADIUS);
        assert_eq!(d.island_radius_m(), world::ISLAND_RADIUS);
        assert_eq!(d.lod_max_level(), 12, "today's top level");
        assert!(d.fits_lod_root());
        assert_eq!(
            (
                d.map_cell_m(1, false),
                d.map_cell_m(16, true),
                d.map_cell_m(256, true)
            ),
            (1, 16, 256)
        );
        assert_eq!(d.hydrology_cell_m(100), 100);
        assert_eq!(d.island_length(5_000.0).to_bits(), 5_000.0f32.to_bits());
        assert_eq!(d.island_wavelength(20_000), 20_000);
        assert_eq!((d.uplift_count(3), d.uplift_count(6)), (3, 6));
    }

    #[test]
    fn a_playable_size_is_a_named_scale_or_today_s() {
        assert_eq!(WorldSize::playable(None), WorldSize::DEFAULT);
        assert_eq!(WorldSize::playable(Some(0)), WorldSize::DEFAULT);
        assert_eq!(WorldSize::playable(Some(1)), WorldSize::DEFAULT);
        assert_eq!(
            WorldSize::playable(Some(2)),
            WorldSize::named("2x").unwrap()
        );
        assert_eq!(WorldSize::playable(Some(10)), WorldSize::LARGEST);
        assert_eq!(WorldSize::playable(Some(u32::MAX)), WorldSize::DEFAULT);
    }

    #[test]
    fn every_named_size_derives_as_designed() {
        // (name, disc km, LOD maxLevel, big map island and disc cells, hydrology cell, ranges of 3)
        let want = [
            ("1x", 50, 12, 16, 256, 100, 3),
            ("2x", 100, 13, 32, 512, 200, 12),
            ("4x", 200, 14, 64, 1024, 400, 48),
            ("8x", 400, 15, 128, 2048, 800, 192),
            ("10x", 500, 15, 256, 4096, 1000, 300),
        ];
        let got: Vec<_> = WorldSize::all()
            .map(|(n, s)| {
                (
                    n,
                    s.world_radius_m() / 1000,
                    s.lod_max_level(),
                    s.map_cell_m(16, true),
                    s.map_cell_m(256, true),
                    s.hydrology_cell_m(100),
                    s.uplift_count(3),
                )
            })
            .collect();
        assert_eq!(got, want);
        for (n, s) in WorldSize::all() {
            assert_eq!(s.name(), Some(n));
            assert_eq!(
                s.island_radius_m() * 2,
                s.world_radius_m(),
                "{n}: the island half the disc"
            );
            // The LOD root holds the disc, and one level fewer would not.
            let half = |l: u32| (lod::SECTION_CELLS << l) / 2;
            assert!(half(s.lod_max_level()) >= s.world_radius_m(), "{n}");
            assert!(half(s.lod_max_level() - 1) < s.world_radius_m(), "{n}");
            // The whole-map hydrology grid keeps today's cell count, or fewer.
            let cells = |r: i64, c: i32| (2 * r / i64::from(c) + 2).pow(2);
            assert!(
                cells(s.world_radius_m(), s.hydrology_cell_m(100))
                    <= cells(world::WORLD_RADIUS, 100),
                "{n}"
            );
            // The big map's coarse levels build no more tiles than today's (32-cell tiles).
            for base in [16u32, 256] {
                let across = |r: i64, c: u32| (2 * r) / (i64::from(c) * 32) + 1;
                assert!(
                    across(s.world_radius_m(), s.map_cell_m(base, true))
                        <= across(world::WORLD_RADIUS, base),
                    "{n}: level {base} m"
                );
                assert!(s.map_cell_m(base, true).is_power_of_two());
            }
        }
    }

    #[test]
    fn every_named_size_fits_the_lod_root() {
        for (n, s) in WorldSize::all() {
            assert!(
                s.fits_lod_root(),
                "{n} needs lod.maxLevel {}",
                s.lod_max_level()
            );
        }
        // The root is the largest size's, no bigger: one level fewer would not hold it.
        assert_eq!(
            i64::from(WorldSize::LARGEST.lod_max_level()),
            lod::MAX_LEVEL
        );
        assert_eq!(WorldSize::LARGEST.name(), Some("10x"));
        let beyond = WorldSize::scaled(world_sizes::MAX_RADIUS_SCALE).unwrap();
        assert!(!beyond.fits_lod_root(), "16x needs level 16");
    }

    #[test]
    fn a_scale_outside_the_range_or_an_unknown_name_is_refused() {
        assert_eq!(WorldSize::scaled(0), Err(SizeError::Scale(0)));
        let over = world_sizes::MAX_RADIUS_SCALE + 1;
        assert_eq!(WorldSize::scaled(over), Err(SizeError::Scale(over)));
        assert_eq!(WorldSize::named("3x"), Err(SizeError::Name));
        // An unnamed scale (a newer build's size) still derives.
        let s = WorldSize::scaled(3).unwrap();
        assert_eq!((s.name(), s.lod_max_level()), (None, 14));
    }
}
