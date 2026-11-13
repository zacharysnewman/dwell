//! The base height field (generator 21, ADR 0049): the Phase 15 noise terrain — one island at the
//! origin, the land rising inland, hills and bumps — kept only as an **input**. The world map
//! (`worldmap`) samples it on its 100 m grid (`hydrology::sample`), lifts mountain ranges over it
//! (the uplift layer) and erodes it; chunks never read it directly, except the island's coast
//! distance (`coast_at`, the portals' inland rule). Its parameters are data
//! (`shared/worldgen/terrain.json` `input`, generated into `terrain.gen.rs`).

use std::sync::Arc;

use dwell_protocol::world_size::WorldSize;
use dwell_world::WorldBounds;

use crate::island::IslandLayout;
use crate::noise::{
    clamp01, fbm2, fbm2_turned, noise_offset, octaves_resolved, seed_word, smooth_step,
};
use crate::plates::{PLATES, PlateLayout};
use crate::terrain::{HEIGHT, HeightField};

/// The input field of one face (its generator's seed).
#[derive(Clone, Debug)]
pub struct BaseField {
    hills_seed: u32,
    bumps_seed: u32,
    offset_x: i64,
    offset_z: i64,
    island: IslandLayout,
    islands_seed: u32,
    /// The field's parameters (the build's `HEIGHT` unless `with_height`).
    h: HeightField,
    /// The world's disc (its size's, BIG_MAP.md M2).
    world: WorldBounds,
    /// A Realistic world's plate layout (`tectonics.plates` 1, R2): its coast is the field's
    /// (oceanic plate = sea, continental = land, a sea margin at the edge) in place of the island's.
    plates: Option<Arc<PlateLayout>>,
    /// A Realistic world's relief noises (plates.json `variety.relief`): the warp's two, the
    /// hills' regional amplitude, the basins and swells.
    variety_seeds: [u32; 4],
}

impl BaseField {
    pub fn new(seed: u64) -> Self {
        Self::with_height(seed, HEIGHT)
    }

    /// The field of face seed `seed` for a world of `size` (BIG_MAP.md M2): its disc and its island
    /// (`IslandLayout::sized`). At today's size, [`BaseField::new`] exactly.
    pub fn sized(seed: u64, size: WorldSize) -> Self {
        Self {
            island: IslandLayout::sized(seed, size),
            world: WorldBounds::of(size),
            ..Self::new(seed)
        }
    }

    /// The world's disc radius (m) this field covers.
    pub fn world_radius(&self) -> i64 {
        self.world.radius()
    }

    /// The field of face seed `seed` with these parameters (a fixture world, Chisel's edit).
    pub fn with_height(seed: u64, h: HeightField) -> Self {
        let plates = h
            .tectonics
            .from_plates()
            .then(|| Arc::new(PlateLayout::generate(&PLATES, seed)));
        Self::with_plates(seed, h, plates)
    }

    /// [`BaseField::with_height`] with a given plate layout for its coast (`None`: the island's).
    pub fn with_plates(seed: u64, h: HeightField, plates: Option<Arc<PlateLayout>>) -> Self {
        Self {
            world: WorldBounds::DEFAULT,
            plates,
            islands_seed: seed_word(seed, 0x1514_A010),
            h,
            hills_seed: seed_word(seed, 1),
            bumps_seed: seed_word(seed, 2),
            offset_x: noise_offset(seed_word(seed, 3)),
            offset_z: noise_offset(seed_word(seed, 4)),
            island: IslandLayout::new(seed),
            variety_seeds: [0, 1, 2, 3].map(|k| seed_word(seed, 0x7A11_0E00 + k)),
        }
    }

    /// The noise lattice's offset (decorations and placements hash their noise from it).
    pub fn offset(&self) -> (i64, i64) {
        (self.offset_x, self.offset_z)
    }

    /// The island's signed distance to the coast (m) at the point itself: > 0 land, < 0 sea.
    pub fn coast_at(&self, x: i64, z: i64) -> f32 {
        match &self.plates {
            Some(p) => p.coast_distance(x as f64, z as f64) as f32,
            None => self.island.coast_at(x, z, None),
        }
    }

    /// The field's height (m, face-local from the midplane) at (x, z) with the octaves finer than
    /// `cell` m dropped; `None` beyond the world's disc.
    pub fn height_lod(&self, x: i64, z: i64, cell: i64) -> Option<f32> {
        if !self.world.inside_disc(x, z) {
            return None;
        }
        let h = &self.h;
        let (nx, nz) = (x + self.offset_x, z + self.offset_z);
        let mut coast = match &self.plates {
            Some(p) => p.coast_distance(x as f64, z as f64) as f32,
            None => self.island.coast_at(
                x,
                z,
                Some(octaves_resolved(
                    self.island.shape().coast_wavelength,
                    self.island.shape().coast_octaves,
                    cell,
                )),
            ),
        };
        // The archipelago's knob (ADR 0074 B4): the coast capped and broken by a broad noise, so
        // the land comes as many islands, each rising from its own coast. Off at 0 (the default).
        if h.island_amplitude > 0.0 {
            let n = fbm2(
                self.islands_seed,
                nx,
                nz,
                h.island_wavelength,
                h.island_octaves,
                Some(octaves_resolved(
                    h.island_wavelength,
                    h.island_octaves,
                    cell,
                )),
            );
            coast = coast.min(h.island_cap) + h.island_amplitude * 2.0 * n;
        }
        if self.plates.is_some() {
            return Some(self.plates_relief(x, z, coast, cell));
        }
        let hills = fbm2(
            self.hills_seed,
            nx,
            nz,
            h.hill_wavelength,
            h.hill_octaves,
            Some(octaves_resolved(h.hill_wavelength, h.hill_octaves, cell)),
        );
        let bumps = fbm2(
            self.bumps_seed,
            nx,
            nz,
            h.bump_wavelength,
            h.bump_octaves,
            Some(octaves_resolved(h.bump_wavelength, h.bump_octaves, cell)),
        );
        // Fractal sums rarely leave ±0.5: × 2 brings the amplitudes to about their nominal values.
        let base = if coast > 0.0 {
            h.inland_rise * (coast / (coast + h.rise_half))
        } else {
            h.sea_floor * clamp01(-coast * (1.0 / h.shelf))
        };
        let land = smooth_step(h.hill_fade_from, h.hill_fade_to, coast);
        let relief = base + land * h.hill_height * 2.0 * hills + h.bump_height * 2.0 * bumps;
        Some(
            (dwell_world::SEA_LEVEL_M as f32 + relief)
                .max((dwell_world::MIDPLANE_M + dwell_world::CORE_M) as f32),
        )
    }

    /// A Realistic world's height (m, face-local) at (x, z) over its plates' `coast` distance
    /// (plates.json `variety.relief`, WORK_QUEUE item 15: no "horizontal slices", no "tiled"
    /// look): the inland rise as the default world's, its hills and bumps read at a domain-warped
    /// point with turned, off-lattice octaves and an amplitude varying by region, and basins and
    /// swells over the interior. Octaves finer than `cell` m dropped.
    fn plates_relief(&self, x: i64, z: i64, coast: f32, cell: i64) -> f32 {
        let h = &self.h;
        let r = crate::plates::VARIETY.relief;
        let [_, _, vary_seed, _] = self.variety_seeds;
        let (fx, fz) = ((x + self.offset_x) as f64, (z + self.offset_z) as f64);
        let (wx, wz) = self.warped(x, z);
        let kept = cell as f64;
        let turned = |seed, w: i32, o| {
            fbm2_turned(seed, wx, wz, f64::from(w), o, r.lacunarity, r.rotate, kept)
        };
        let hills = turned(self.hills_seed, h.hill_wavelength, h.hill_octaves);
        let bumps = turned(self.bumps_seed, h.bump_wavelength, h.bump_octaves);
        let t = (0.5
            + 1.6
                * fbm2_turned(
                    vary_seed,
                    fx,
                    fz,
                    r.hill_wavelength,
                    2,
                    r.lacunarity,
                    r.rotate,
                    0.0,
                ))
        .clamp(0.0, 1.0);
        let vary = (r.hill_min + (r.hill_max - r.hill_min) * f64::from(t)) as f32;
        let base = if coast > 0.0 {
            h.inland_rise * (coast / (coast + h.rise_half))
        } else {
            h.sea_floor * clamp01(-coast * (1.0 / h.shelf))
        };
        let land = smooth_step(h.hill_fade_from, h.hill_fade_to, coast);
        let relief =
            base + land * vary * h.hill_height * 2.0 * hills + vary * h.bump_height * 2.0 * bumps;
        (dwell_world::SEA_LEVEL_M as f32 + relief)
            .max((dwell_world::MIDPLANE_M + dwell_world::CORE_M) as f32)
    }

    /// A Realistic world's warped sample point (plates.json `variety.relief.warp`; noise lattice m).
    fn warped(&self, x: i64, z: i64) -> (f64, f64) {
        let r = crate::plates::VARIETY.relief;
        let (fx, fz) = ((x + self.offset_x) as f64, (z + self.offset_z) as f64);
        let warp = |seed| {
            r.warp_amplitude
                * 2.0
                * f64::from(fbm2_turned(
                    seed,
                    fx,
                    fz,
                    r.warp_wavelength,
                    2,
                    r.lacunarity,
                    r.rotate,
                    0.0,
                ))
        };
        (
            fx + warp(self.variety_seeds[0]),
            fz + warp(self.variety_seeds[1]),
        )
    }

    /// A Realistic world's basins and swells (m) at (x, z) on land (plates.json
    /// `variety.relief.swells`), faded in from the coast as the hills are; 0 without plates. The
    /// map adds them whole, as it does the boundaries' landforms (not scaled by the uplift layer's
    /// `keepRelief`).
    pub fn swell_at(&self, x: i64, z: i64) -> f64 {
        if self.plates.is_none() {
            return 0.0;
        }
        let r = crate::plates::VARIETY.relief;
        let (wx, wz) = self.warped(x, z);
        let swell = fbm2_turned(
            self.variety_seeds[3],
            wx,
            wz,
            r.swell_wavelength,
            r.swell_octaves,
            r.lacunarity,
            r.rotate,
            0.0,
        );
        let land = smooth_step(
            self.h.hill_fade_from,
            self.h.hill_fade_to,
            self.coast_at(x, z),
        );
        f64::from(land) * r.swell_height * 2.0 * f64::from(swell)
    }
}
