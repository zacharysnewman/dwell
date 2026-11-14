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

both_faces!(
    fn the_maps_town_sites_are_the_servers_and_the_town_link_stands_by_one(f: FaceFrame) {
        // The map's sites (`towns_round`, the client's `mapTowns`) are the server's siting
        // (`sites_near` on `town_ground_for_sized`), and `?spawn=town` stands the player south of
        // the nearest one, on its face, near enough to found it.
        let terrain = dwell_worldgen::bifacial::BifacialTerrain::for_generator_sized(
            GENERATOR_TERRAIN,
            0,
            WorldSize::DEFAULT,
        )
        .expect("a height field");
        let ground = town_ground_for_sized(GENERATOR_TERRAIN, 0, WorldSize::DEFAULT).unwrap();
        let g = |x: i32, z: i32| ground(f.face, x, z);
        let spawn = f.point(spawn_point_for_sized(
            GENERATOR_TERRAIN,
            0,
            WorldSize::DEFAULT,
        ));
        let rules = SHIPPED.towns;
        let (sx, sz) = (spawn[0].floor() as i32, spawn[2].floor() as i32);
        let map = terrain.towns_round(0, f.face, sx, sz, 2);
        for t in rules.sites_near(0, f, spawn[0], spawn[2], 1200.0, &g) {
            assert!(map.contains(&t), "the map has the server's site {t:?}");
        }
        let feet = dwell_worldgen::town_spawn_for_sized(
            GENERATOR_TERRAIN,
            0,
            WorldSize::DEFAULT,
            f.face,
            24.0,
        )
        .expect("a town near the spawn");
        assert_eq!(
            dwell_world::FaceFrame::at_height(feet[1]),
            f,
            "on the link's face"
        );
        let l = f.point(feet);
        let near = terrain
            .nearest_town(0, f.face, l[0], l[2], 200.0)
            .expect("a site near the feet");
        let d = (f64::from(near.x) + 0.5 - l[0]).hypot(f64::from(near.z) + 0.5 - l[2]);
        assert!(
            d < rules.found_m,
            "{d} m from the site: near enough to found it"
        );
        assert!(l[1] > g(near.x, near.z).unwrap(), "over the ground");
    }
);

both_faces!(
    /// BUGS.md #169: a founded hamlet's blueprints went up round the generated trees standing on
    /// its site (a broadleaf between the well and a hut, another before a hut's door). The
    /// generator places no tree, bush or boulder reaching a planned town's layout box plus
    /// `towns.clearM`, so every blueprint, door and path between them is founded on clear ground.
    fn no_tree_bush_or_boulder_stands_on_a_planned_towns_layout(f: FaceFrame) {
        let rules = SHIPPED.towns;
        let mut sites = 0;
        let mut round = 0;
        for seed in [0u64, 1, 7, 42] {
            let terrain = dwell_worldgen::bifacial::BifacialTerrain::for_generator_sized(
                GENERATOR_TERRAIN,
                seed,
                WorldSize::DEFAULT,
            )
            .expect("a height field");
            let g = terrain.local(f.face);
            let ground = |x: i32, z: i32| terrain.town_ground(f.face, x, z);
            // Round the origin, and seed 0's hamlet the bug was found at (688, 7979 on face A).
            let mut near = rules.sites_near(seed, f, 0.0, 0.0, 2500.0, &ground);
            if seed == 0 {
                near.extend(rules.sites_near(seed, f, 688.0, 7979.0, 64.0, &ground));
            }
            for t in near {
                sites += 1;
                let [x0, z0, x1, z1] = rules.clear_box(&t);
                assert!(
                    x0 < t.x - 9 && x1 > t.x + 9 && z0 < t.z - 11 && z1 > t.z + 10,
                    "the box covers the hamlet's huts, well, tower and wall: {t:?}"
                );
                let pad = 48;
                for tree in g.trees_in((x0 - pad, z0 - pad), (x1 + pad, z1 + pad)) {
                    let fp = g.planters()[tree.planter as usize].footprint(&tree);
                    let on = fp[0] <= x1 && fp[2] >= x0 && fp[1] <= z1 && fp[3] >= z0;
                    assert!(
                        !on,
                        "seed {seed}: a {} at {:?} (footprint {fp:?}) stands on the town {t:?}'s \
                         clear box {:?}",
                        g.planter_name(tree.planter).unwrap_or("?"),
                        tree.anchor,
                        [x0, z0, x1, z1]
                    );
                    round += 1;
                }
            }
        }
        eprintln!("{sites} sites, {round} trees round them");
        assert!(sites >= 4, "towns to check ({sites})");
        assert!(round > 0, "trees grow round the towns, just not on them");
    }
);
