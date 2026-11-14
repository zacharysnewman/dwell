//! A Realistic world's top blocks in plan view, near (as chunks place them) beside far (as the LOD
//! colours them, `far_surface` at `--cell` m), each block a colour hashed from its name (sand
//! tan, stone grey, grass green …), to find seams and bands (WORK_QUEUE item 15's map check).
//!
//! `cargo run --release -p dwell-worldgen --example realistic_tops -- --seed 2741893632 --face b
//! --x -29000 --z -29000 --size 6000 --step 12 --cell 32 --out FILE.png`

#[path = "hydrology/png.rs"]
mod png;

use std::sync::Arc;

use dwell_world::Face;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::terrain::TerrainGenerator;
use dwell_worldgen::worldmap::WorldMap;

fn arg(args: &[String], name: &str, default: &str) -> String {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
        .unwrap_or_else(|| default.to_string())
}

fn colour(name: &str) -> [u8; 3] {
    let base = name
        .split('[')
        .next()
        .unwrap_or(name)
        .trim_end_matches("_ramp");
    match base {
        "dwell:air" => [0, 0, 0],
        "dwell:water" => [40, 90, 200],
        "dwell:sand" => [225, 205, 150],
        "dwell:red_sand" => [200, 110, 60],
        "dwell:red_dirt" => [160, 80, 50],
        "dwell:stone" => [128, 128, 128],
        "dwell:grass" | "dwell:grass_block" => [90, 160, 70],
        "dwell:dirt" => [120, 85, 55],
        "dwell:snow" => [245, 245, 250],
        "dwell:basalt" => [50, 46, 56],
        "dwell:obsidian" => [150, 40, 190],
        _ => {
            let mut h: u32 = 2166136261;
            for b in base.bytes() {
                h = (h ^ u32::from(b)).wrapping_mul(16777619);
            }
            [
                (h & 0xff) as u8,
                ((h >> 8) & 0xff) as u8,
                ((h >> 16) & 0xff) as u8,
            ]
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let world: u64 = arg(&args, "--seed", "2741893632").parse().expect("seed");
    let face = if arg(&args, "--face", "b") == "b" {
        Face::B
    } else {
        Face::A
    };
    let cx: i32 = arg(&args, "--x", "0").parse().expect("x");
    let cz: i32 = arg(&args, "--z", "0").parse().expect("z");
    let size: i32 = arg(&args, "--size", "6000").parse().expect("size");
    let step: i32 = arg(&args, "--step", "12").parse().expect("step");
    let cell: i64 = arg(&args, "--cell", "32").parse().expect("cell");
    let out = arg(&args, "--out", "tops.png");
    const NAME: &str = "test:realistic";
    let e = world_config(NAME).unwrap().expect("the Realistic world");
    let seed = face_seed(world, face);
    let map = Arc::new(
        WorldMap::for_height(seed, e.height.expect("height"))
            .with_worldgen(NAME)
            .unwrap(),
    );
    let g = TerrainGenerator::with_map(seed, map)
        .with_biomes_and_geology(&e.biomes, e.map, e.geology)
        .with_vegetation(e.vegetation);
    if args.iter().any(|a| a == "--volcanoes") {
        let t = g.plate_tectonics().expect("plates");
        for (v, st) in t.volcanoes.iter().zip(&t.states) {
            println!(
                "({:.0}, {:.0}) r {:.0} h {:.0} chain {} active {} age {:.2} apron {:.0}",
                v.x, v.z, v.radius, v.height, v.chain, st.active, st.age, st.apron
            );
        }
        return;
    }
    let probe = arg(&args, "--probe", "");
    if !probe.is_empty() {
        let v: Vec<i32> = probe
            .split(',')
            .map(|t| t.parse().expect("probe"))
            .collect();
        for k in 0..v[4] {
            let (x, z) = (v[0] + k * v[2], v[1] + k * v[3]);
            let c = g.column_at(x, z);
            let top = g
                .ground_y(x, z)
                .map(|y| dwell_blocks::info(g.state_at(x, y, z)).name);
            let b = &e.biomes[usize::from(g.biome_mix(x.into(), z.into()).chosen)].name;
            println!(
                "({x}, {z}): h {:.1} band {:?} water {:?} level {:.1} palette {} biome {b} top {top:?}",
                c.height - dwell_world::SEA_LEVEL_M as f32,
                c.band,
                c.water,
                c.level - dwell_world::SEA_LEVEL_M as f32,
                c.palette
            );
        }
        return;
    }
    let n = (size / step) as usize;
    let mut img = vec![0u8; n * 2 * n * 3];
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for j in 0..n {
        for i in 0..n {
            let x = cx - size / 2 + i as i32 * step;
            let z = cz - size / 2 + j as i32 * step;
            let near = match g.ground_y(x, z) {
                Some(y) => dwell_blocks::info(g.state_at(x, y, z)).name,
                None => "dwell:air",
            };
            *counts
                .entry(near.split('[').next().unwrap().to_string())
                .or_default() += 1;
            let far = dwell_blocks::info(g.far_surface(x.into(), z.into(), cell)).name;
            for (k, name) in [near, far].into_iter().enumerate() {
                let p = (j * 2 * n + k * n + i) * 3;
                img[p..p + 3].copy_from_slice(&colour(name));
            }
        }
    }
    std::fs::write(&out, png::encode((2 * n) as u32, n as u32, &img)).expect("write");
    println!("{out}: near (left) and far at {cell} m (right); near tops {counts:?}");
}
