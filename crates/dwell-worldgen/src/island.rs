//! The interim land layout (Phase 15, ARCHITECTURE.md §6.3): one island at the origin. Its coast is
//! a circle whose radius varies by smooth noise, seen through a domain warp, with fractal coast
//! detail. A pure function of (world seed, world coordinates). Its size is an absolute distance in
//! data (`world.islandRadius`, constants.json), independent of the world's radius (owner,
//! 2026-10-15: the island keeps its Phase 15 size when the world shrinks to 50 km). The other
//! parameters are code constants for now: Phase 17 moves the layout into the terrain definition
//! (data, ADR 0026).

use crate::noise::{fbm2, noise_offset, seed_word};

/// The island's mean radius (m, data) and how far (m) the radius noise moves its coast, at that
/// noise's wavelength (m).
pub const RADIUS: f32 = dwell_protocol::world::ISLAND_RADIUS as f32;
pub const RADIUS_VARIATION: f32 = 5_000.0;
pub const RADIUS_WAVELENGTH: i32 = 20_000;
/// The domain warp: amplitude (m) and wavelength (m), 2 octaves.
pub const WARP_AMPLITUDE: f32 = 3_000.0;
pub const WARP_WAVELENGTH: i32 = 6_000;
/// Coast detail: octaves from this wavelength (m), and their amplitude (m).
pub const COAST_WAVELENGTH: i32 = 4_000;
pub const COAST_OCTAVES: u32 = 5;
pub const COAST_AMPLITUDE: f32 = 1_600.0;

/// The island's tunables a world's settings change (`settings.rs`: `island.radius`,
/// `island.coastAmplitude`); [`IslandParams::DEFAULT`] is the constants above.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IslandParams {
    pub radius: f32,
    pub coast_amplitude: f32,
}

impl IslandParams {
    pub const DEFAULT: Self = Self {
        radius: RADIUS,
        coast_amplitude: COAST_AMPLITUDE,
    };
}

impl Default for IslandParams {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Clone, Debug)]
pub struct IslandLayout {
    params: IslandParams,
    radius_seed: u32,
    warp_x_seed: u32,
    warp_z_seed: u32,
    coast_seed: u32,
    offset_x: i64,
    offset_z: i64,
}

impl IslandLayout {
    pub fn new(world_seed: u64) -> Self {
        Self::with(world_seed, IslandParams::DEFAULT)
    }

    /// The layout with a world's island tunables.
    pub fn with(world_seed: u64, params: IslandParams) -> Self {
        Self {
            params,
            radius_seed: seed_word(world_seed, 0x1514_A001),
            warp_x_seed: seed_word(world_seed, 0x1514_A002),
            warp_z_seed: seed_word(world_seed, 0x1514_A003),
            coast_seed: seed_word(world_seed, 0x1514_A004),
            offset_x: noise_offset(seed_word(world_seed, 0x1514_A005)),
            offset_z: noise_offset(seed_word(world_seed, 0x1514_A006)),
        }
    }

    /// The signed distance to the coast (m): > 0 on the island, < 0 at sea. With `kept` (level of
    /// detail), only the coast detail's first octaves; the warp and the radius are always whole.
    pub fn coast_at(&self, x: i64, z: i64, kept: Option<u32>) -> f32 {
        let (nx, nz) = (x + self.offset_x, z + self.offset_z);
        // Fractal sums rarely leave ±0.5: × 2 brings the amplitudes to about their nominal values.
        let wx = 2.0 * WARP_AMPLITUDE * fbm2(self.warp_x_seed, nx, nz, WARP_WAVELENGTH, 2, None);
        let wz = 2.0 * WARP_AMPLITUDE * fbm2(self.warp_z_seed, nx, nz, WARP_WAVELENGTH, 2, None);
        let px = x as f64 + f64::from(wx);
        let pz = z as f64 + f64::from(wz);
        // Correctly rounded in IEEE double on every target: deterministic.
        let distance = (px * px + pz * pz).sqrt() as f32;
        let radius = self.params.radius
            + 2.0 * RADIUS_VARIATION * fbm2(self.radius_seed, nx, nz, RADIUS_WAVELENGTH, 2, None);
        let detail = 2.0
            * self.params.coast_amplitude
            * fbm2(
                self.coast_seed,
                nx,
                nz,
                COAST_WAVELENGTH,
                COAST_OCTAVES,
                kept,
            );
        radius - distance + detail
    }
}
