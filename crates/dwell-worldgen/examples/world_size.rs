//! A sized world's creation, timed (docs/proposals/BIG_MAP.md M2): both faces' world maps (on two
//! threads, as a server builds them), the terrain over them and the spawn search, for a named
//! size. Point `DWELL_MAP_CACHE` at an empty directory so the maps are computed, not read; run
//! under `/usr/bin/time -v` (Linux) or `-l` (macOS) for the peak memory.
//!
//! `cargo run --release -p dwell-worldgen --example world_size -- [size] [seed]` (size `2x`, …).

use std::time::Instant;

use dwell_protocol::world_size::WorldSize;
use dwell_world::Face;
use dwell_worldgen::BifacialTerrain;

fn main() {
    let mut args = std::env::args().skip(1);
    let name = args.next().unwrap_or_else(|| "1x".into());
    let seed: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let size = WorldSize::named(&name).expect("a size named in constants.json worldSizes");
    let t = Instant::now();
    let terrain = BifacialTerrain::sized(seed, size);
    let maps = t.elapsed().as_secs_f64();
    let m = terrain.local(Face::A).map();
    println!(
        "{name}: disc {} km, map {} × {} cells of {} m",
        size.world_radius_m() / 1000,
        m.n,
        m.n,
        m.cell
    );
    println!("  both faces' maps and terrain {maps:>8.1} s");
    let t = Instant::now();
    let s = terrain.spawn_point();
    println!(
        "  spawn search                 {:>8.1} s  (at {:.0}, {:.0})",
        t.elapsed().as_secs_f64(),
        s[0],
        s[2]
    );
}
