//! A Realistic world's map as a PNG (WORK_QUEUE item 15's map checks): one pixel per map cell,
//! sea blue by depth, land hillshaded green → brown → white by height, plate boundaries red.
//!
//! `cargo run --release -p dwell-worldgen --example realistic_map -- --out DIR 1 7 42`

#[path = "hydrology/png.rs"]
mod png;

use dwell_world::Face;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::plates::{PLATES, PlateLayout};
use dwell_worldgen::terrain::FIXTURE_WORLDS;
use dwell_worldgen::worldmap::WorldMap;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut out = ".".to_string();
    let mut seeds = Vec::new();
    let mut faces = vec![Face::A];
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                out = args[i + 1].clone();
                i += 1;
            }
            "--both" => faces = vec![Face::A, Face::B],
            s => seeds.push(s.parse::<u64>().expect("seed")),
        }
        i += 1;
    }
    let h = FIXTURE_WORLDS
        .iter()
        .find(|(n, _)| *n == "test:realistic")
        .expect("realistic")
        .1;
    for s in seeds {
        for &face in &faces {
            let seed = face_seed(s, face);
            let layout = PlateLayout::generate(&PLATES, seed);
            let m = WorldMap::for_height(seed, h);
            let n = m.n;
            let mut rgb = vec![0u8; n * n * 3];
            for c in 0..n * n {
                let (x, z) = m.centre(c);
                let y = f64::from(m.height[c]) - dwell_world::SEA_LEVEL_M;
                let (r, g, b) = if m.height[c] <= 0.0 || m.is_sea(c) {
                    let d = (-y / 400.0).clamp(0.0, 1.0);
                    (
                        20.0 + 40.0 * (1.0 - d),
                        60.0 + 80.0 * (1.0 - d),
                        120.0 + 100.0 * (1.0 - d),
                    )
                } else {
                    let t = (y / 1500.0).clamp(0.0, 1.0);
                    let (gx, gz) = (
                        m.height_at(x + m.cell, z).0 - m.height_at(x - m.cell, z).0,
                        m.height_at(x, z + m.cell).0 - m.height_at(x, z - m.cell).0,
                    );
                    let shade = (1.0 - 0.004 * (gx - gz)).clamp(0.5, 1.4);
                    (
                        shade * (70.0 + 150.0 * t),
                        shade * (130.0 + 60.0 * t),
                        shade * (60.0 + 160.0 * t),
                    )
                };
                let p = layout.plate_at(x, z);
                let edge = [(m.cell, 0.0), (0.0, m.cell)]
                    .iter()
                    .any(|(dx, dz)| layout.plate_at(x + dx, z + dz) != p);
                let (r, g, b) = if edge { (220.0, 30.0, 30.0) } else { (r, g, b) };
                rgb[c * 3] = r.clamp(0.0, 255.0) as u8;
                rgb[c * 3 + 1] = g.clamp(0.0, 255.0) as u8;
                rgb[c * 3 + 2] = b.clamp(0.0, 255.0) as u8;
            }
            let f = if face == Face::A { "a" } else { "b" };
            let path = format!("{out}/realistic-{s}-{f}.png");
            std::fs::write(&path, png::encode(n as u32, n as u32, &rgb)).expect("write");
            println!("{path}");
        }
    }
}
