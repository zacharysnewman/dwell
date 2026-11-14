//! Long straight axis-aligned seams in a Realistic world's biome and surface fields (BUGS.md #154):
//! `cargo run --release -p dwell-worldgen --example seams -- 1 7`.
use std::sync::Arc;
use dwell_world::Face;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::terrain::TerrainGenerator;
use dwell_worldgen::worldmap::WorldMap;
const STEP: i32 = 50;
fn main() {
    let e = world_config("test:realistic").unwrap().unwrap();
    for w in std::env::args().skip(1).map(|a| a.parse::<u64>().unwrap()) {
        for face in [Face::A, Face::B] {
            let seed = face_seed(w, face);
            let map = Arc::new(WorldMap::for_height(seed, e.height.unwrap()).with_worldgen("test:realistic").unwrap());
            let g = TerrainGenerator::with_map(seed, map).with_biomes_and_geology(&e.biomes, e.map, e.geology).with_vegetation(e.vegetation);
            let r = 48_000 / STEP;
            let n = (2 * r + 1) as usize;
            let mut biome = vec![u16::MAX; n * n];
            let mut top = vec![""; n * n];
            for j in 0..n { for i in 0..n {
                let (x, z) = ((i as i32 - r) * STEP, (j as i32 - r) * STEP);
                let c = g.column_at(x, z);
                if c.outside || c.water != dwell_worldgen::terrain::Water::None { continue; }
                biome[j * n + i] = u16::from(g.biome_mix(x.into(), z.into()).chosen);
                if let Some(y) = g.ground_y(x, z) { let s = dwell_blocks::info(g.state_at(x, y, z)).name; top[j*n+i] = s.split('[').next().unwrap(); }
            }}
            for (field, name) in [(0, "biome"), (1, "surface")] {
                let differs = |a: usize, b: usize| if field == 0 { biome[a] != u16::MAX && biome[b] != u16::MAX && biome[a] != biome[b] } else { !top[a].is_empty() && !top[b].is_empty() && top[a] != top[b] };
                // vertical seams: between columns i and i+1, a run of rows j.
                let mut best = (0usize, 0usize, 0usize, 'v');
                for i in 0..n - 1 { let mut run = 0; for j in 0..n { if differs(j*n+i, j*n+i+1) { run += 1; if run > best.0 { best = (run, i, j, 'v'); } } else { run = 0; } } }
                for j in 0..n - 1 { let mut run = 0; for i in 0..n { if differs(j*n+i, (j+1)*n+i) { run += 1; if run > best.0 { best = (run, i, j, 'h'); } } else { run = 0; } } }
                let (run, i, j, d) = best;
                let (x, z) = ((i as i32 - r) * STEP, (j as i32 - r) * STEP);
                let k = j * n + i;
                let other = if d == 'v' { k + 1 } else { k + n };
                let lab = |k: usize| if field == 0 { e.biomes[usize::from(biome[k])].name.to_string() } else { top[k].to_string() };
                println!("{w} {face:?} {name}: longest straight {d} seam {} m ending ({x}, {z}): {} | {}", run as i32 * STEP, lab(k), lab(other));
            }
        }
    }
}
