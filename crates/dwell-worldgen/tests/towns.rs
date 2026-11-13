//! Town sites on a generated world (WORK_QUEUE item 56, B4, docs/BLUEPRINTS.md §6): the ground test
//! of the height field finds sites on both faces of the default seed near its spawn, each one's
//! centre on dry land within the band and clear of the shrines, the same each time asked.

use dwell_protocol::world_size::WorldSize;
use dwell_world::blueprint::SHIPPED;
use dwell_world::{GENERATOR_TERRAIN, SEA_LEVEL_M, both_faces};
use dwell_worldgen::{spawn_point_for_sized, town_ground_for_sized};

both_faces!(
    fn the_default_seed_has_town_sites_near_its_spawn_on_dry_land(f: FaceFrame) {
        let ground = town_ground_for_sized(GENERATOR_TERRAIN, 0, WorldSize::DEFAULT)
            .expect("a height field");
        let spawn = f.point(spawn_point_for_sized(
            GENERATOR_TERRAIN,
            0,
            WorldSize::DEFAULT,
        ));
        let g = |x: i32, z: i32| ground(f.face, x, z);
        let t0 = std::time::Instant::now();
        let rules = SHIPPED.towns;
        let sites = rules.sites_near(0, f, spawn[0], spawn[2], 3000.0, &g);
        eprintln!("{} sites within 3 km in {:?}", sites.len(), t0.elapsed());
        assert!(!sites.is_empty(), "towns near the spawn");
        let [lo, hi] = rules.height_m;
        for t in &sites {
            let h = g(t.x, t.z).expect("dry land at its centre");
            assert!(h >= SEA_LEVEL_M + lo && h <= SEA_LEVEL_M + hi);
        }
        assert_eq!(
            rules.sites_near(0, f, spawn[0], spawn[2], 3000.0, &g),
            sites
        );
    }
);
