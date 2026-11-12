//! The atmosphere (docs/DAY_NIGHT.md §15, WORK_QUEUE item 28): the sky thinning to space as the
//! viewer climbs, from `shared/look/atmosphere.json` (generated into `atmosphere.gen.rs`). Owner,
//! 2026-11-12: "We likely need an atmosphere transition now between sky thinning to space. Clouds
//! shouldn't extend past the earth atmosphere either."
//!
//! Heights are face-local (above sea level on the viewer's own face), so both faces thin alike. At
//! or below `base_m` the sky is today's exactly ([`Air::GROUND`], a golden test pins it); above it
//! the share of the sky's scattering left falls as the air column over the viewer does, to none at
//! `top_m`. [`Air`] is what the sky needs of it per frame: the thinning and where the limb (the
//! horizon's glow gathered at the world's rim) sits below the viewer's horizon. The shader's twin
//! is `dwell_space` in `sky.wgsl`, fed by the `DaySky` block's `space` (`DayState::pack`).

use crate::Rgb;

#[path = "atmosphere.gen.rs"]
mod generated;
pub use generated::ATMOSPHERE;

/// The atmosphere's data (`shared/look/atmosphere.json`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Atmosphere {
    /// Where the thinning starts and where it is complete (m above sea level, face-local).
    pub base_m: f64,
    pub top_m: f64,
    /// The air column's scale height over `base_m` (m): the thinning curve's shape.
    pub scale_height_m: f64,
    /// Space's colour (sRGB).
    pub space_color: Rgb,
    /// The stars' least visibility in space (by day too).
    pub stars_in_space: f64,
    /// The limb: its brightness (× the horizon's colour), width (°), and the rim's distance (m).
    pub limb_brightness: f64,
    pub limb_width_deg: f64,
    pub limb_distance_m: f64,
    /// Past the world's rim the limb fades out over this (m from the disc's axis beyond its radius):
    /// a viewer out in the void has no world under it to make one (BUGS.md #102).
    pub limb_fade_m: f64,
    /// The sun in space: how far its disc goes to white, and its tight white bloom.
    pub sun_whiteness: f64,
    pub sun_bloom: f64,
}

impl Atmosphere {
    /// How much of the sky's scattering is gone at `altitude` m above sea level (face-local): 0 at
    /// or below `base_m` (exactly), 1 at or above `top_m`, between it the air column over the eye
    /// falling with the scale height, `(e^(−x/H) − e^(−X/H)) / (1 − e^(−X/H))` left.
    pub fn thinning(&self, altitude: f64) -> f64 {
        if altitude <= self.base_m || altitude.is_nan() {
            return 0.0;
        }
        if altitude >= self.top_m {
            return 1.0;
        }
        let h = self.scale_height_m;
        let x = (altitude - self.base_m) / h;
        let span = (self.top_m - self.base_m) / h;
        let left = ((-x).exp() - (-span).exp()) / -(-span).exp_m1();
        (1.0 - left).clamp(0.0, 1.0)
    }

    /// The limb's elevation sine below the viewer's horizon at `altitude` (face-local): the world's
    /// rim `limb_distance_m` away, `−sin(atan(h / d))`.
    pub fn limb_rise(&self, altitude: f64) -> f64 {
        let h = altitude.max(0.0);
        -h / (h * h + self.limb_distance_m * self.limb_distance_m).sqrt()
    }

    /// The limb's width as an elevation sine.
    pub fn limb_width(&self) -> f64 {
        self.limb_width_deg.to_radians().sin()
    }

    /// How much of the limb a viewer `from_axis_m` from the disc's axis sees in a world of radius
    /// `world_radius_m`: all of it over the disc, fading out past the rim over `limb_fade_m`.
    pub fn limb_weight(&self, from_axis_m: f64, world_radius_m: f64) -> f64 {
        let past = from_axis_m - world_radius_m;
        if past <= 0.0 {
            return 1.0;
        }
        let x = (past / self.limb_fade_m.max(1e-6)).min(1.0);
        1.0 - x * x * (3.0 - 2.0 * x)
    }

    /// The air for a viewer at `altitude` m above sea level on its own face, over the disc.
    pub fn air(&self, altitude: f64) -> Air {
        let thin = self.thinning(altitude);
        if thin <= 0.0 {
            return Air::GROUND;
        }
        Air {
            thin,
            limb: self.limb_rise(altitude),
            limb_weight: 1.0,
        }
    }

    /// [`Atmosphere::air`] for a viewer `from_axis_m` from the disc's axis in a world of radius
    /// `world_radius_m`: past the rim the limb fades out ([`Atmosphere::limb_weight`]).
    pub fn air_at(&self, altitude: f64, from_axis_m: f64, world_radius_m: f64) -> Air {
        let mut air = self.air(altitude);
        if air.thin > 0.0 {
            air.limb_weight = self.limb_weight(from_axis_m, world_radius_m);
        }
        air
    }
}

/// The viewer's air this frame: how thin (0 the ground's sky, 1 space), the limb's elevation sine
/// in the viewer's face's frame and how much of the limb shows (0 far past the rim).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Air {
    pub thin: f64,
    pub limb: f64,
    pub limb_weight: f64,
}

impl Air {
    /// At or below the atmosphere's base: today's sky, untouched.
    pub const GROUND: Air = Air {
        thin: 0.0,
        limb: 0.0,
        limb_weight: 1.0,
    };

    /// The same air seen in the other face's frame (its up the other way): the limb's rise flips.
    pub fn other(self) -> Air {
        Air {
            limb: -self.limb,
            ..self
        }
    }

    /// What is left of the fog's density (it thins with the air).
    pub fn fog(self) -> f64 {
        1.0 - self.thin
    }

    /// The stars' visibility: the keyframe's `stars`, at least `stars_in_space` × the thinning.
    pub fn stars(self, stars: f64) -> f64 {
        stars.max(self.thin * ATMOSPHERE.stars_in_space)
    }
}

/// The thinned sky (`dwell_space` in `sky.wgsl`): `ground` (today's sky toward the ray) taken toward
/// space by `air.thin`, where space is `space_color`, the limb (`horizon`, the horizon's colour in
/// the ray's azimuth, gathered at the limb's rise; `rise` is the ray's elevation sine) and the sun's
/// tight white bloom (`sun_cos`, the ray's cosine to the sun; `bloom_k` the sun's bloom's width).
pub fn space_sky(
    ground: Rgb,
    horizon: Rgb,
    rise: f64,
    sun_cos: f64,
    bloom_k: f64,
    air: Air,
) -> Rgb {
    if air.thin <= 0.0 || air.thin.is_nan() {
        return ground;
    }
    let a = &ATMOSPHERE;
    let off = (rise - air.limb) / a.limb_width().max(1e-6);
    let limb = a.limb_brightness * air.limb_weight * (-off * off).exp();
    let bloom = a.sun_bloom * ((sun_cos - 1.0) * bloom_k).exp();
    let space: Rgb =
        std::array::from_fn(|i| (a.space_color[i] + horizon[i] * limb + bloom).min(1.0));
    std::array::from_fn(|i| ground[i] + (space[i] - ground[i]) * air.thin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_world::both_faces;

    const A: Atmosphere = ATMOSPHERE;

    #[test]
    fn the_ground_and_everything_under_the_base_is_untouched() {
        for alt in [-500.0, 0.0, 1.6, 1000.0, A.base_m] {
            assert_eq!(A.thinning(alt), 0.0, "{alt}");
            assert_eq!(A.air(alt), Air::GROUND);
        }
        let c = [0.3, 0.5, 0.8];
        assert_eq!(space_sky(c, [1.0; 3], 0.0, 1.0, 100.0, Air::GROUND), c);
    }

    #[test]
    fn the_thinning_rises_monotonically_to_space_at_the_top() {
        let mut last = 0.0;
        for i in 0..=200 {
            let alt = A.base_m + (A.top_m - A.base_m) * f64::from(i) / 200.0;
            let t = A.thinning(alt);
            assert!(t >= last - 1e-12 && (0.0..=1.0).contains(&t), "{alt}: {t}");
            last = t;
        }
        assert_eq!(A.thinning(A.top_m), 1.0);
        assert_eq!(A.thinning(1e9), 1.0);
        // Most of the scattering goes in the first few scale heights.
        assert!(A.thinning(A.base_m + 2.0 * A.scale_height_m) > 0.8);
        // The moon's distance is space.
        assert_eq!(A.thinning(596_800.0), 1.0);
    }

    #[test]
    fn space_is_black_with_the_limb_below_the_horizon_and_the_stars_by_day() {
        let air = A.air(A.top_m * 2.0);
        assert_eq!(air.thin, 1.0);
        assert!(
            air.limb < -0.5,
            "the rim is far below from high up: {}",
            air.limb
        );
        let ground = [0.4, 0.6, 0.85];
        let horizon = [0.9, 0.9, 0.88];
        // Straight up, away from the sun: space's colour.
        let up = space_sky(ground, horizon, 1.0, -0.5, 1000.0, air);
        for i in 0..3 {
            assert!((up[i] - A.space_color[i]).abs() < 1e-9, "{up:?}");
        }
        // At the limb: the horizon's colour.
        let limb = space_sky(ground, horizon, air.limb, -0.5, 1000.0, air);
        assert!(limb[0] > 0.8, "{limb:?}");
        // Above it by a few widths, dark again.
        let above = space_sky(
            ground,
            horizon,
            air.limb + 4.0 * A.limb_width(),
            -0.5,
            1000.0,
            air,
        );
        assert!(above[0] < 0.05, "{above:?}");
        // Stars at full by day.
        assert_eq!(air.stars(0.0), A.stars_in_space);
        assert_eq!(Air::GROUND.stars(0.3), 0.3);
        // No fog in space.
        assert_eq!(air.fog(), 0.0);
        assert_eq!(Air::GROUND.fog(), 1.0);
    }

    #[test]
    fn every_cloud_is_under_the_atmospheres_top() {
        // clouds.mjs refuses data past it; this pins the generated tunables too.
        let c = &crate::clouds::CLOUDS;
        assert!(c.deck.altitude_m <= A.top_m);
        assert!(c.volume.top_m <= A.top_m);
        assert!(c.sources.lava_vent.height_m <= A.top_m);
        assert!(c.plumes.iter().all(|p| p.top_m <= A.top_m));
    }

    both_faces!(
        fn the_air_is_face_local(f: FaceFrame) {
            // The same height above each face's sea level thins alike; the limb is below that face's
            // horizon, and seen in the other face's frame it flips.
            let eye = f.y(dwell_world::SEA_LEVEL_M + 20_000.0);
            let alt = f.y(eye) - dwell_world::SEA_LEVEL_M;
            assert!((alt - 20_000.0).abs() < 1e-6);
            let air = A.air(alt);
            assert!(air.thin > 0.5 && air.limb < 0.0);
            assert_eq!(air.other().limb, -air.limb);
            assert_eq!(air.other().thin, air.thin);
        }
    );
}
