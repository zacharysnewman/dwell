//! The base height field (generator 21, ADR 0049): the Phase 15 noise terrain — one island at the
//! origin, the land rising inland, hills and bumps — kept only as an **input**. The world map
//! (`worldmap`) samples it on its 100 m grid (`hydrology::sample`), lifts mountain ranges over it
//! (the uplift layer) and erodes it; chunks never read it directly, except the island's coast
//! distance (`coast_at`, the portals' inland rule). Its parameters are data
//! (`shared/worldgen/terrain.json` `input`, generated into `terrain.gen.rs`).

use dwell_world::inside_world_disc;

use crate::island::{self, IslandLayout, IslandParams};
use crate::noise::{clamp01, fbm2, noise_offset, octaves_resolved, seed_word, smooth_step};
use crate::terrain::HEIGHT as H;

/// The input field of one face (its generator's seed).
#[derive(Clone, Debug)]
pub struct BaseField {
    hills_seed: u32,
    bumps_seed: u32,
    offset_x: i64,
    offset_z: i64,
    island: IslandLayout,
}

impl BaseField {
    pub fn new(seed: u64) -> Self {
        Self::with(seed, IslandParams::DEFAULT)
    }

    /// The field with a world's island tunables (`settings.rs`).
    pub fn with(seed: u64, island: IslandParams) -> Self {
        Self {
            hills_seed: seed_word(seed, 1),
            bumps_seed: seed_word(seed, 2),
            offset_x: noise_offset(seed_word(seed, 3)),
            offset_z: noise_offset(seed_word(seed, 4)),
            island: IslandLayout::with(seed, island),
        }
    }

    /// The noise lattice's offset (decorations and placements hash their noise from it).
    pub fn offset(&self) -> (i64, i64) {
        (self.offset_x, self.offset_z)
    }

    /// The island's signed distance to the coast (m) at the point itself: > 0 land, < 0 sea.
    pub fn coast_at(&self, x: i64, z: i64) -> f32 {
        self.island.coast_at(x, z, None)
    }

    /// The field's height (m, face-local from the midplane) at (x, z) with the octaves finer than
    /// `cell` m dropped; `None` beyond the world's disc.
    pub fn height_lod(&self, x: i64, z: i64, cell: i64) -> Option<f32> {
        if !inside_world_disc(x, z) {
            return None;
        }
        let (nx, nz) = (x + self.offset_x, z + self.offset_z);
        let coast = self.island.coast_at(
            x,
            z,
            Some(octaves_resolved(
                island::COAST_WAVELENGTH,
                island::COAST_OCTAVES,
                cell,
            )),
        );
        let hills = fbm2(
            self.hills_seed,
            nx,
            nz,
            H.hill_wavelength,
            H.hill_octaves,
            Some(octaves_resolved(H.hill_wavelength, H.hill_octaves, cell)),
        );
        let bumps = fbm2(
            self.bumps_seed,
            nx,
            nz,
            H.bump_wavelength,
            H.bump_octaves,
            Some(octaves_resolved(H.bump_wavelength, H.bump_octaves, cell)),
        );
        // Fractal sums rarely leave ±0.5: × 2 brings the amplitudes to about their nominal values.
        let base = if coast > 0.0 {
            H.inland_rise * (coast / (coast + H.rise_half))
        } else {
            H.sea_floor * clamp01(-coast * (1.0 / H.shelf))
        };
        let land = smooth_step(H.hill_fade_from, H.hill_fade_to, coast);
        let relief = base + land * H.hill_height * 2.0 * hills + H.bump_height * 2.0 * bumps;
        Some(
            (dwell_world::SEA_LEVEL_M as f32 + relief)
                .max((dwell_world::MIDPLANE_M + dwell_world::CORE_M) as f32),
        )
    }
}
