//! What a world's spawn costs natively (BUGS.md): the spawn search and the spawn's shrine per face,
//! for the default world or a fixture world. `cargo run --release -p dwell-worldgen --example
//! spawn_cost [seed] [worldgen]` (e.g. `42 test:realistic`).

use std::time::Instant;

use dwell_world::Face;
use dwell_worldgen::BifacialTerrain;

fn main() {
    let mut args = std::env::args().skip(1);
    let seed = args.next().and_then(|s| s.parse().ok()).unwrap_or(0u64);
    let worldgen = args.next().unwrap_or_default();
    let t = Instant::now();
    if !worldgen.is_empty() {
        for face in [Face::A, Face::B] {
            let s = dwell_worldgen::bifacial::face_seed(seed, face);
            let bytes = dwell_worldgen::config::worldgen_map_bytes(s, &worldgen).expect("worldgen");
            dwell_worldgen::worldmap::install(&bytes).expect("install");
        }
        println!("maps {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    }
    let t = Instant::now();
    let g = BifacialTerrain::new(seed);
    println!("terrain {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    for face in [Face::A, Face::B] {
        let l = g.local(face);
        let t = Instant::now();
        let k = l.spawn_key();
        println!(
            "{face:?}: spawn_key {} bytes {:.0} ms",
            k.len(),
            t.elapsed().as_secs_f64() * 1e3
        );
        let t = Instant::now();
        let p = l.spawn_search_point();
        println!(
            "{face:?}: spawn search {:.0} ms ({p:?})",
            t.elapsed().as_secs_f64() * 1e3
        );
        if let Some(sh) = l.shrines() {
            let t = Instant::now();
            let s = sh.spawn_shrine(l);
            println!(
                "{face:?}: spawn shrine {s:?} {:.0} ms",
                t.elapsed().as_secs_f64() * 1e3
            );
        }
    }
    let t = Instant::now();
    let p = g.spawn_point();
    println!(
        "spawn_point {p:?} {:.0} ms",
        t.elapsed().as_secs_f64() * 1e3
    );
}
