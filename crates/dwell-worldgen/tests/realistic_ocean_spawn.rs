//! A Realistic face may be all ocean (WORK_QUEUE item 15, real-world scale; owner, 2026-11-14: "The
//! just ocean is ok if that just ends up happening. If it's super often we can tune it, it if not,
//! it's ok if you end up in the ocean."). No land is forced and nothing is centred, so the spawn
//! search must cope with a face of open sea: the first seed whose face A is under 5 % land still
//! gives a spawn in budget, standing on solid ground or on the water's surface (never inside the
//! ground or under the sea floor).

use std::time::{Duration, Instant};

use dwell_world::Face;
use dwell_worldgen::bifacial::{BifacialTerrain, face_seed};
use dwell_worldgen::config::worldgen_map_bytes;
use dwell_worldgen::plates::{PLATES, PlateLayout};

const NAME: &str = "test:realistic";

#[test]
fn an_ocean_face_still_spawns() {
    // face-a-only: the world's spawn is face A's (face B's mirror is `spawn_point_b`).
    let land = |l: &PlateLayout| {
        let r = PLATES.reach - PLATES.rim.edge_sea;
        let (mut on, mut all) = (0, 0);
        for j in 0..30 {
            for i in 0..30 {
                let (x, z) = (-r + f64::from(i) * r / 15.0, -r + f64::from(j) * r / 15.0);
                if x.hypot(z) < r {
                    all += 1;
                    on += usize::from(l.is_land(x, z));
                }
            }
        }
        on as f64 / f64::from(all)
    };
    let seed = (0..200u64)
        .find(|&s| land(&PlateLayout::generate(&PLATES, face_seed(s, Face::A))) < 0.05)
        .expect("an ocean face among 200 seeds");
    for f in [Face::A, Face::B] {
        let bytes = worldgen_map_bytes(face_seed(seed, f), NAME).expect("the Realistic map");
        dwell_worldgen::worldmap::install(&bytes).expect("installs");
    }
    let g = BifacialTerrain::new(seed);
    let start = Instant::now();
    let [x, y, z] = g.spawn_point();
    let took = start.elapsed();
    let t = g.local(Face::A);
    let col = t.column_at(x.floor() as i32, z.floor() as i32);
    eprintln!(
        "seed {seed}: an ocean face; spawn ({x:.0}, {y:.1}, {z:.0}) in {took:?}, ground {:.1} m, water {:?}",
        col.height, col.water
    );
    assert!(took < Duration::from_secs(5), "the spawn took {took:?}");
    assert!(
        y + 0.01 >= f64::from(col.height),
        "the spawn under the ground"
    );
}
