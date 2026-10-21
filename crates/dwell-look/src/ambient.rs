//! Ambient lighting (docs/DAY_NIGHT.md §10): the classic voxel corner occlusion and the baked sky
//! visibility the chunk mesher bakes into every terrain vertex, and the floor the shader keeps for
//! a fully occluded one. The data is `shared/look/ambient.json`; the mesher (dwell-mesh
//! `sky.rs`) does the work and the renderer only multiplies the hemisphere light by it.

#[path = "ambient.gen.rs"]
mod generated;
pub use generated::AMBIENT;

use dwell_world::{CELL_H, SKY_APRON};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AmbientTunables {
    /// How much the three corner-occlusion levels below open darken (0 none, 1 a full crease
    /// keeps no ambient): level `l` (0–3, 3 open) keeps `1 − strength × (3 − l) / 3`.
    pub ao_strength: f64,
    /// Sky-visibility rays per vertex (0: off, every vertex sees the whole sky).
    pub rays: u32,
    /// How far each ray runs (m).
    pub length_m: f64,
    /// How far above a ray's end the probe looks for a roof (m, face-local up).
    pub probe_m: f64,
    /// The share of the ambient a fully occluded vertex keeps.
    pub floor: f64,
}

impl AmbientTunables {
    /// What corner-occlusion level `level` (0 a full crease … 3 open) leaves of the ambient.
    pub fn ao_factor(&self, level: u8) -> f64 {
        1.0 - self.ao_strength * f64::from(3 - level.min(3)) / 3.0
    }

    /// Whether the rays' reach fits the meshers' apron (cells): `length_m` across in x and z, and
    /// `length_m + probe_m` up, each with a cell to spare for the start's nudge.
    pub const fn fits(&self, apron: [i32; 3]) -> bool {
        let across = self.length_m + 1.0;
        let up = (self.length_m + self.probe_m) / CELL_H + 1.0;
        across <= apron[0] as f64 && across <= apron[2] as f64 && up <= apron[1] as f64
    }

    /// The ambient's multiplier for a baked vertex light `light` (corner occlusion × sky
    /// visibility, 0–1): the floor, and the rest by the light (`terrain_ambient` in the shader).
    pub fn ambient(&self, light: f64) -> f64 {
        self.floor + (1.0 - self.floor) * light.clamp(0.0, 1.0)
    }
}

const _: () = assert!(
    AMBIENT.fits(SKY_APRON),
    "shared/look/ambient.json: the sky rays reach past dwell_world::SKY_APRON"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_corners_keep_all_and_creases_lose_the_strength() {
        let t = AmbientTunables {
            ao_strength: 0.6,
            ..AMBIENT
        };
        assert_eq!(t.ao_factor(3), 1.0);
        assert!((t.ao_factor(0) - 0.4).abs() < 1e-12);
        assert!(t.ao_factor(1) < t.ao_factor(2));
    }

    #[test]
    fn the_floor_is_what_a_dark_vertex_keeps() {
        let t = AmbientTunables {
            floor: 0.1,
            ..AMBIENT
        };
        assert_eq!(t.ambient(1.0), 1.0, "open ground is lit as before");
        assert!((t.ambient(0.0) - 0.1).abs() < 1e-12);
    }

    #[test]
    fn the_reach_is_checked_against_the_apron() {
        assert!(AMBIENT.fits(SKY_APRON));
        let long = AmbientTunables {
            length_m: 12.0,
            ..AMBIENT
        };
        assert!(!long.fits(SKY_APRON));
        let tall = AmbientTunables {
            probe_m: 16.0,
            ..AMBIENT
        };
        assert!(!tall.fits(SKY_APRON));
    }
}
