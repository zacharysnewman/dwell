//! The Realistic world's variety in plan view (WORK_QUEUE item 15, the realistic batch part B;
//! owner, 2026-11-13, on seed 2741893632: "I meant horizontal slices of noise", "noise also seems
//! very uniform at a macro level, where the same terrain almost looks tiled"). For the seeds 1, 7,
//! 42 and 2741893632 on both faces (`dwell_worldgen::variety`'s measures, on the world map's land):
//!
//! - **Mixed provinces** (rocks.json `plateProvinces.mixing`, each rule's `mix`): no rock province
//!   one region over 250 km² (seed 2741893632 face A was 553 km², all one basin's red beds) and at
//!   least 2.8 distinct provinces per 4 km² (was 1.8–2.1).
//! - **Biome variety** (climate.json `variety`): on the owner's seed no biome over 36 % of the land
//!   (cold desert was 39.7 %), its largest region at most 18 % of the land (was 590 km², ~30 %; 320 km²
//!   before part A's wider continents) and at least 3.5 biomes
//!   per 4 km² (was 3.1); on every seed's face at least 1.9 per 4 km² (a subarctic band's tundra was
//!   1.5).
//! - **No grid streaks** (plates.json `variety.flow` `random`): the map's small-scale slopes face
//!   the grid's 45°/90° directions no more than any other (the eight-fold harmonic under 0.1; the
//!   D8 routing's 0.12–0.29 on these seeds).
//! - **Interior relief** (plates.json `variety.relief`): the basins and swells are the map's own,
//!   none without plates.
//!
//! The default and starter worlds never read any of it (`tests/golden.rs`, `tests/starter_world.rs`).

use std::sync::Arc;

use dwell_world::{FaceFrame, both_faces};
use dwell_worldgen::base::BaseField;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::biomemap::BiomeField;
use dwell_worldgen::config::world_config;
use dwell_worldgen::terrain::{HEIGHT, TerrainGenerator};
use dwell_worldgen::variety::{Labels, anisotropy};
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";
const SEEDS: [u64; 4] = [1, 7, 42, 2_741_893_632];
const OWNERS: u64 = 2_741_893_632;

struct Measured {
    world: u64,
    provinces: Labels,
    biomes: Labels,
    anisotropy: f64,
}

fn measure(world: u64, f: FaceFrame) -> Measured {
    let e = world_config(NAME).unwrap().expect("the Realistic world");
    let seed = face_seed(world, f.face);
    let map = Arc::new(
        WorldMap::for_height(seed, e.height.expect("its height field"))
            .with_worldgen(NAME)
            .unwrap(),
    );
    let g = TerrainGenerator::with_map(seed, map.clone())
        .with_biomes_and_geology(&e.biomes, e.map, e.geology)
        .with_vegetation(e.vegetation);
    let field = BiomeField::with_params(&map, seed, e.map);
    Measured {
        world,
        provinces: Labels::on_land(&map, 1, |x, z, _| g.province_at(x, z)),
        biomes: Labels::on_land(&map, 1, |x, z, h| {
            e.biomes[usize::from(field.mix(&e.biomes, x, z, h).chosen)].name
        }),
        anisotropy: anisotropy(&map),
    }
}

both_faces!(
    /// Every seed's provinces are mixed, its biomes varied and its slopes free of the grid's grain.
    fn the_realistic_world_has_variety_in_plan_view(f: FaceFrame) {
        for m in SEEDS.map(|w| measure(w, f)) {
            let (w, face) = (m.world, f.face);
            let (stack, km2) = m.provinces.largest_region();
            assert!(
                km2 <= 250.0,
                "{w} {face:?}: {stack} one region of {km2:.0} km²"
            );
            let per = m.provinces.distinct_per_4km2();
            assert!(per >= 2.8, "{w} {face:?}: {per:.2} provinces per 4 km²");
            let per = m.biomes.distinct_per_4km2();
            assert!(per >= 1.9, "{w} {face:?}: {per:.2} biomes per 4 km²");
            assert!(
                m.anisotropy < 0.1,
                "{w} {face:?}: slopes along the grid's 45°/90° lines ({:.3})",
                m.anisotropy
            );
            if w == OWNERS {
                let (top, share) = m.biomes.largest_share();
                assert!(
                    share <= 0.36,
                    "{face:?}: {top} on {:.1} % of the land",
                    100.0 * share
                );
                // A share of the land: the realistic batch's part A (0.61) runs the continents out
                // to the rim, so the owner's face A holds 2.7× the land (320 km² was 19 % of it; the largest is 16 %, a red rock mesa on face B).
                let (big, km2) = m.biomes.largest_region();
                let land = m.biomes.land_km2();
                eprintln!("{face:?}: {big} {km2:.0} km² of {land:.0} km² of land");
                assert!(
                    km2 <= 0.18 * land,
                    "{face:?}: {big} one region of {km2:.0} km² ({:.1} % of the land)",
                    100.0 * km2 / land
                );
                let per = m.biomes.distinct_per_4km2();
                assert!(per >= 3.5, "{face:?}: {per:.2} biomes per 4 km²");
            }
        }
    }
);

both_faces!(
    /// The basins and swells (plates.json `variety.relief`) are a Realistic world's: none on a
    /// field without plates (the default's), some of tens of m on the plates' land.
    fn the_basins_and_swells_are_the_plates_worlds_only(f: FaceFrame) {
        let e = world_config(NAME).unwrap().expect("the Realistic world");
        let seed = face_seed(OWNERS, f.face);
        let plain = BaseField::with_height(seed, HEIGHT);
        let plates = BaseField::with_height(seed, e.height.unwrap());
        let (mut most, mut any) = (0.0f64, false);
        for z in (-20_000..=20_000).step_by(1_000) {
            for x in (-20_000..=20_000).step_by(1_000) {
                assert_eq!(plain.swell_at(x, z), 0.0, "no swells without plates");
                if plates.coast_at(x, z) > 5_000.0 {
                    any = true;
                    most = most.max(plates.swell_at(x, z).abs());
                }
            }
        }
        assert!(any, "the plates have an interior");
        assert!(most >= 40.0, "the interior's swells reach {most:.0} m");
    }
);
