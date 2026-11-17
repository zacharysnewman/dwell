//! A Realistic world's map as a PNG (WORK_QUEUE item 15's map checks): one pixel per map cell,
//! sea blue by depth, land hillshaded green → brown → white by height, plate boundaries red.
//!
//! `cargo run --release -p dwell-worldgen --example realistic_map -- --out DIR 1 7 42`
//!
//! `--rivers`: the rivers drawn (blue) with their sources (yellow; a lake's outlet orange), and each
//! source's height printed as a percentile of the face's land (BUGS.md #196: do rivers rise in the
//! mountains?).

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
    let mut rivers = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                out = args[i + 1].clone();
                i += 1;
            }
            "--both" => faces = vec![Face::A, Face::B],
            "--rivers" => rivers = true,
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
            if rivers {
                let cell_of = |x: f32, z: f32| -> Option<usize> {
                    let i = ((f64::from(x) - m.x0) / m.cell).round();
                    let j = ((f64::from(z) - m.x0) / m.cell).round();
                    (i >= 0.0 && j >= 0.0 && (i as usize) < n && (j as usize) < n)
                        .then(|| j as usize * n + i as usize)
                };
                let mut put =
                    |c: usize, rgb3: [u8; 3]| rgb[c * 3..c * 3 + 3].copy_from_slice(&rgb3);
                let mut land: Vec<f32> = (0..n * n)
                    .filter(|&c| m.height[c] > 0.0 && !m.is_sea(c))
                    .map(|c| m.height[c])
                    .collect();
                land.sort_by(f32::total_cmp);
                let pct =
                    |h: f32| land.partition_point(|&v| v < h) as f64 / land.len() as f64 * 100.0;
                let mut sources = Vec::new();
                for river in &m.rivers {
                    for w in river.path.windows(2) {
                        let steps = (((w[1].x - w[0].x).hypot(w[1].z - w[0].z)) / m.cell as f32)
                            .ceil()
                            .max(1.0) as usize;
                        for k in 0..=steps {
                            let t = k as f32 / steps as f32;
                            if let Some(c) = cell_of(
                                w[0].x + (w[1].x - w[0].x) * t,
                                w[0].z + (w[1].z - w[0].z) * t,
                            ) {
                                put(c, [40, 90, 255]);
                            }
                        }
                    }
                }
                for river in &m.rivers {
                    let Some(v) = river.path.first() else {
                        continue;
                    };
                    let from_lake = river.from_lake != 0;
                    if let Some(c) = cell_of(v.x, v.z) {
                        for (di, dj) in [(0i64, 0i64), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                            let cc = c as i64 + dj * n as i64 + di;
                            if cc >= 0 && (cc as usize) < n * n {
                                put(
                                    cc as usize,
                                    if from_lake {
                                        [255, 140, 0]
                                    } else {
                                        [255, 230, 0]
                                    },
                                );
                            }
                        }
                        if !from_lake {
                            sources.push(pct(m.height[c]));
                        }
                    }
                }
                sources.sort_by(f64::total_cmp);
                let k = sources.len();
                if k > 0 {
                    println!(
                        "seed {s} face {face:?}: {} rivers, {k} from springs; source height percentile of the land: p10 {:.0} p50 {:.0} p90 {:.0}; over the 75th: {}",
                        m.rivers.len(),
                        sources[k / 10],
                        sources[k / 2],
                        sources[k * 9 / 10],
                        sources.iter().filter(|&&p| p >= 75.0).count()
                    );
                }
            }
            let f = if face == Face::A { "a" } else { "b" };
            let path = format!("{out}/realistic-{s}-{f}.png");
            std::fs::write(&path, png::encode(n as u32, n as u32, &rgb)).expect("write");
            println!("{path}");
        }
    }
}
