//! Terrain decorations (docs/TRANSPARENT_BLOCKS.md §10): blocks the generator puts one cell above
//! its ground — today tall grass in meadows. The rules are data (`shared/worldgen/decorations.json`,
//! generated into `decorations.gen.rs` by `shared/worldgen/gen.mjs`), resolved by name once per
//! generator. Each cell is decided on its own from the world seed and its position (a patch noise,
//! a per-cell hash, the ground block, its height and its slope), so chunks agree across their
//! borders and generate in any order, natively and in WASM alike (only `+ − ×` and comparisons,
//! ADR 0010).

use dwell_blocks::StateId;

use crate::noise::{fbm2, hash2, seed_word, smooth_step, unit};

/// One decoration rule as data (see the generator's validation for the ranges).
#[derive(Clone, Copy, Debug)]
pub struct Decoration {
    pub name: &'static str,
    /// The block put, by id.
    pub block: &'static str,
    /// The ground blocks it may stand on, by id.
    pub on: &'static [&'static str],
    /// The share of eligible cells (inside a patch) that get it.
    pub density: f32,
    /// Patches: a noise of this wavelength (m; 0: none, everywhere) covering about `patch_coverage`
    /// of the ground.
    pub patch_wavelength: i32,
    pub patch_coverage: f32,
    /// The steepest ground it stands on: the most the ground's top differs from its four
    /// neighbours', in blocks.
    pub max_slope: i32,
    /// The ground's top (its top voxel, in metres above sea level) must lie in [min_height,
    /// max_height].
    pub min_height: i32,
    pub max_height: i32,
}

mod table {
    include!("decorations.gen.rs");
}
pub use table::DECORATIONS;

/// A decoration resolved for one world: state ids and noise seeds.
#[derive(Clone, Debug)]
pub struct Placer {
    state: StateId,
    on: Vec<StateId>,
    density: f32,
    patch_wavelength: i32,
    patch_low: f32,
    patch_high: f32,
    patch_seed: u32,
    cell_seed: u32,
    max_slope: i32,
    min_height: i32,
    max_height: i32,
}

/// The patches' soft edge: half its width in the patch noise's units.
const PATCH_EDGE: f32 = 0.06;

impl Placer {
    /// Resolves `rules` for `world_seed` (the `index`-th rule takes its own noise streams).
    pub fn resolve(rules: &[Decoration], world_seed: u64) -> Vec<Placer> {
        rules
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let stream = 0xDEC0_0000 + 2 * i as u32;
                // fbm2 rarely leaves ±0.5: the patch is where it exceeds the coverage's quantile,
                // approximated as a linear map of the coverage onto that range.
                let threshold = 0.5 - d.patch_coverage;
                Placer {
                    state: dwell_blocks::require(d.block),
                    on: d.on.iter().map(|o| dwell_blocks::require(o)).collect(),
                    density: d.density,
                    patch_wavelength: d.patch_wavelength,
                    patch_low: threshold - PATCH_EDGE,
                    patch_high: threshold + PATCH_EDGE,
                    patch_seed: seed_word(world_seed, stream),
                    cell_seed: seed_word(world_seed, stream + 1),
                    max_slope: d.max_slope,
                    min_height: d.min_height,
                    max_height: d.max_height,
                }
            })
            .collect()
    }

    /// The chance a cell gets it (before its ground, height and slope are asked), from its patch
    /// noise position.
    fn chance(&self, noise_x: i64, noise_z: i64) -> f32 {
        let inside = if self.patch_wavelength > 0 {
            let p = fbm2(
                self.patch_seed,
                noise_x,
                noise_z,
                self.patch_wavelength,
                2,
                None,
            );
            smooth_step(self.patch_low, self.patch_high, p)
        } else {
            1.0
        };
        self.density * inside
    }

    /// Whether the cell above ground at (x, z) gets this decoration: its ground block `ground`,
    /// whose top is `top` (m above sea level), with neighbours' tops from `neighbour_top` (+x, −x,
    /// +z, −z; the same).
    pub fn places(
        &self,
        (x, z): (i32, i32),
        (noise_x, noise_z): (i64, i64),
        ground: StateId,
        top: i32,
        neighbour_top: impl Fn(i32, i32) -> i32,
    ) -> bool {
        if !self.on.contains(&ground) || top < self.min_height || top > self.max_height {
            return false;
        }
        let roll = unit(hash2(self.cell_seed, x, z));
        if roll >= self.chance(noise_x, noise_z) {
            return false;
        }
        [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .iter()
            .all(|&(dx, dz)| (neighbour_top(x + dx, z + dz) - top).abs() <= self.max_slope)
    }

    /// The block put.
    pub fn state(&self) -> StateId {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_data_resolves_by_name() {
        let p = Placer::resolve(&DECORATIONS, 7);
        assert_eq!(p.len(), DECORATIONS.len());
        assert_eq!(p[0].state(), dwell_blocks::require("dwell:tall_grass"));
    }

    #[test]
    fn a_rule_added_by_data_alone_places_by_its_ground_height_and_slope() {
        let rule = Decoration {
            name: "test_tufts",
            block: "dwell:tall_grass",
            on: &["dwell:dirt"],
            density: 1.0,
            patch_wavelength: 0,
            patch_coverage: 1.0,
            max_slope: 1,
            min_height: 2,
            max_height: 10,
        };
        let p = &Placer::resolve(&[rule], 3)[0];
        let dirt = dwell_blocks::require("dwell:dirt");
        let grass = dwell_blocks::require("dwell:grass");
        let flat = |_: i32, _: i32| 5;
        assert!(p.places((4, 9), (4, 9), dirt, 5, flat));
        assert!(!p.places((4, 9), (4, 9), grass, 5, flat), "not its ground");
        assert!(!p.places((4, 9), (4, 9), dirt, 11, |_, _| 11), "too high");
        assert!(!p.places((4, 9), (4, 9), dirt, 1, |_, _| 1), "too low");
        assert!(
            !p.places((4, 9), (4, 9), dirt, 5, |x, _| if x == 5 { 7 } else { 5 }),
            "too steep"
        );
        // A density below 1 is a deterministic share of cells, the same on every call.
        let half = &Placer::resolve(
            &[Decoration {
                density: 0.5,
                ..rule
            }],
            3,
        )[0];
        let picks: Vec<bool> = (0..400)
            .map(|x| half.places((x, 0), (x.into(), 0), dirt, 5, flat))
            .collect();
        let n = picks.iter().filter(|&&b| b).count();
        assert!((150..250).contains(&n), "{n}");
        let again: Vec<bool> = (0..400)
            .map(|x| half.places((x, 0), (x.into(), 0), dirt, 5, flat))
            .collect();
        assert_eq!(picks, again);
    }
}
