//! What the resurrection shrines cost (docs/SHRINES.md): siting the island's cells and the spawn
//! search the spawn's shrine needs, per face, natively. `cargo run --release -p dwell-worldgen
//! --example shrine_cost [seed]`.

use std::time::Instant;

use dwell_world::Face;
use dwell_worldgen::BifacialTerrain;

fn main() {
    let seed = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0u64);
    let t = Instant::now();
    let g = BifacialTerrain::new(seed);
    println!("terrain {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    for face in [Face::A, Face::B] {
        let l = g.local(face);
        let t = Instant::now();
        let n = g.shrines_round(face, 0, 0, 6).len();
        let cells = 13 * 13;
        let ms = t.elapsed().as_secs_f64() * 1e3;
        println!(
            "{face:?}: {n} shrines in {cells} cells, {ms:.0} ms ({:.2} ms a cell)",
            ms / f64::from(cells)
        );
        let t = Instant::now();
        let p = l.spawn_search_point();
        println!(
            "{face:?}: spawn search {:.0} ms ({p:?})",
            t.elapsed().as_secs_f64() * 1e3
        );
        let t = Instant::now();
        let s = l.shrines().expect("shrines").spawn_shrine(l);
        println!(
            "{face:?}: spawn shrine {s:?} {:.0} ms",
            t.elapsed().as_secs_f64() * 1e3
        );
    }
}
