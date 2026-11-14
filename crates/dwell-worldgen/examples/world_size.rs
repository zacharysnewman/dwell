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
use dwell_worldgen::progress::MapStep;

fn main() {
    let mut args = std::env::args().skip(1);
    let name = args.next().unwrap_or_else(|| "1x".into());
    let seed: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let size = WorldSize::named(&name).expect("a size named in constants.json worldSizes");
    // One face's map by itself, each step timed by the builder's progress (BUGS.md #91).
    let steps = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(MapStep, Instant)>::new()));
    let s2 = steps.clone();
    let t = Instant::now();
    let face_seed = dwell_worldgen::bifacial::face_seed(seed, Face::A);
    dwell_worldgen::progress::with_sink(
        move |step, _| {
            let mut v = s2.borrow_mut();
            if v.last().is_none_or(|l| l.0 != step) {
                v.push((step, Instant::now()));
            }
        },
        || dwell_worldgen::worldmap::WorldMap::compute_sized(face_seed, size),
    );
    let end = Instant::now();
    let steps = steps.borrow();
    println!("{name}: face A's map {:>8.2} s", (end - t).as_secs_f64());
    for (k, (step, at)) in steps.iter().enumerate() {
        let next = steps.get(k + 1).map_or(end, |n| n.1);
        println!("  {:<10} {:>8.2} s", step.id(), (next - *at).as_secs_f64());
    }
    if std::env::var_os("DWELL_MAP_ONLY").is_some() {
        return;
    }
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
