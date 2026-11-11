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

/// The island's shape tunables at a world size (BIG_MAP.md M0): today's constants, each absolute
/// length × the size's scale (`WorldSize::island_length`), so a bigger world's island is today's
/// shape, bigger. At today's size, the constants exactly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IslandShape {
    pub radius: f32,
    pub radius_variation: f32,
    pub radius_wavelength: i32,
    pub warp_amplitude: f32,
    pub warp_wavelength: i32,
    pub coast_wavelength: i32,
    pub coast_octaves: u32,
    pub coast_amplitude: f32,
}

impl IslandShape {
    pub fn sized(size: dwell_protocol::world_size::WorldSize) -> Self {
        Self {
            radius: size.island_radius_m() as f32,
            radius_variation: size.island_length(RADIUS_VARIATION),
            radius_wavelength: size.island_wavelength(RADIUS_WAVELENGTH),
            warp_amplitude: size.island_length(WARP_AMPLITUDE),
            warp_wavelength: size.island_wavelength(WARP_WAVELENGTH),
            coast_wavelength: size.island_wavelength(COAST_WAVELENGTH),
            coast_octaves: COAST_OCTAVES,
            coast_amplitude: size.island_length(COAST_AMPLITUDE),
        }
    }
}

#[derive(Clone, Debug)]
pub struct IslandLayout {
    shape: IslandShape,
    radius_seed: u32,
    warp_x_seed: u32,
    warp_z_seed: u32,
    coast_seed: u32,
    offset_x: i64,
    offset_z: i64,
}

impl IslandLayout {
    pub fn new(world_seed: u64) -> Self {
        Self::sized(world_seed, dwell_protocol::world_size::WorldSize::DEFAULT)
    }

    /// The layout of a world of `size` (BIG_MAP.md M0; the generators read only today's yet, M2).
    pub fn sized(world_seed: u64, size: dwell_protocol::world_size::WorldSize) -> Self {
        Self {
            shape: IslandShape::sized(size),
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
        let k = &self.shape;
        // Fractal sums rarely leave ±0.5: × 2 brings the amplitudes to about their nominal values.
        let wx =
            2.0 * k.warp_amplitude * fbm2(self.warp_x_seed, nx, nz, k.warp_wavelength, 2, None);
        let wz =
            2.0 * k.warp_amplitude * fbm2(self.warp_z_seed, nx, nz, k.warp_wavelength, 2, None);
        let px = x as f64 + f64::from(wx);
        let pz = z as f64 + f64::from(wz);
        // Correctly rounded in IEEE double on every target: deterministic.
        let distance = (px * px + pz * pz).sqrt() as f32;
        let radius = k.radius
            + 2.0
                * k.radius_variation
                * fbm2(self.radius_seed, nx, nz, k.radius_wavelength, 2, None);
        let detail = 2.0
            * k.coast_amplitude
            * fbm2(
                self.coast_seed,
                nx,
                nz,
                k.coast_wavelength,
                k.coast_octaves,
                kept,
            );
        radius - distance + detail
    }
}

#[cfg(test)]
mod size_tests {
    use super::*;
    use dwell_protocol::world_size::WorldSize;

    #[test]
    fn todays_size_is_todays_island_and_a_bigger_one_is_it_scaled() {
        // BIG_MAP.md M0: at 1× the shape is the constants, bit for bit (goldens unchanged); at a
        // size s the coast at s·(x, z) is s × today's at (x, z) wherever the noise's lattice
        // scales with it (its wavelengths × s: the same field, stretched).
        let d = IslandShape::sized(WorldSize::DEFAULT);
        assert_eq!(
            d,
            IslandShape {
                radius: RADIUS,
                radius_variation: RADIUS_VARIATION,
                radius_wavelength: RADIUS_WAVELENGTH,
                warp_amplitude: WARP_AMPLITUDE,
                warp_wavelength: WARP_WAVELENGTH,
                coast_wavelength: COAST_WAVELENGTH,
                coast_octaves: COAST_OCTAVES,
                coast_amplitude: COAST_AMPLITUDE,
            }
        );
        let a = IslandLayout::new(7);
        let b = IslandLayout::sized(7, WorldSize::DEFAULT);
        for (x, z) in [
            (0, 0),
            (24_000, -3_000),
            (-17_500, 19_000),
            (40_000, 40_000),
        ] {
            assert_eq!(
                a.coast_at(x, z, None).to_bits(),
                b.coast_at(x, z, None).to_bits()
            );
        }
        for (n, size) in WorldSize::all() {
            let s = size.radius_scale as f32;
            let k = IslandShape::sized(size);
            assert_eq!(k.radius, RADIUS * s, "{n}");
            assert_eq!(
                k.coast_amplitude / k.radius,
                COAST_AMPLITUDE / RADIUS,
                "{n}"
            );
            assert_eq!(
                k.warp_wavelength,
                WARP_WAVELENGTH * size.radius_scale as i32,
                "{n}"
            );
            // The centre is land and the disc's rim is sea at every size.
            let l = IslandLayout::sized(3, size);
            assert!(l.coast_at(0, 0, None) > 0.0, "{n}");
            let rim = size.world_radius_m() * 9 / 10;
            for (x, z) in [(rim, 0), (0, -rim), (-rim, 0), (0, rim)] {
                assert!(l.coast_at(x, z, None) < 0.0, "{n} at ({x}, {z})");
            }
        }
    }
}
