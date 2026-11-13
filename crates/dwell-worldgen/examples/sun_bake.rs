//! The sun world's far-body bake (docs/SUN_WORLD.md S-3), timed, with previews: both faces'
//! normal/height and albedo maps at the data's size, written as PPM images (the normal map lit by a
//! low sun from +x, and the albedo) for a look without the game.
//!
//! `cargo run --release -p dwell-worldgen --example sun_bake -- [seed] [out-dir]`

use std::time::Instant;

use dwell_world::Face;
use dwell_worldgen::volcanic::VolcanicTerrain;

fn main() {
    let mut args = std::env::args().skip(1);
    let seed: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let out = args.next();
    let sun = VolcanicTerrain::for_grid("dwell:sun", seed).expect("the sun grid");
    for (face, name) in [(Face::A, "a"), (Face::B, "b")] {
        let t = Instant::now();
        let bake = sun.bake_far_default(face);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        let bytes = bake.normal_height.len() + bake.albedo.len();
        println!(
            "face {name}: {0} × {0} in {ms:.0} ms, {1} KiB, heights {2:.1}..{3:.1} m",
            bake.size,
            bytes / 1024,
            bake.height_range[0],
            bake.height_range[1]
        );
        let Some(dir) = &out else { continue };
        let light = {
            let (x, y): (f64, f64) = (0.8, 0.35);
            let l = (x * x + y * y).sqrt();
            [x / l, y / l, 0.0]
        };
        let mut lit = Vec::new();
        let mut albedo = Vec::new();
        for j in 0..bake.size {
            for i in 0..bake.size {
                let p = (j * bake.size + i) * 4;
                let n = bake.normal(i, j);
                let k = (n[0] * light[0] + n[1] * light[1] + n[2] * light[2]).max(0.0);
                let a = &bake.albedo[p..p + 4];
                for c in 0..3 {
                    let v = if a[3] == 0 {
                        0.0
                    } else {
                        f64::from(a[c]) * k * 1.6
                    };
                    lit.push(v.min(255.0) as u8);
                    albedo.push(a[c]);
                }
            }
        }
        for (what, px) in [("lit", lit), ("albedo", albedo)] {
            let path = format!("{dir}/sun-{seed}-{name}-{what}.ppm");
            let mut file = format!("P6 {0} {0} 255\n", bake.size).into_bytes();
            file.extend_from_slice(&px);
            std::fs::write(&path, file).expect("write");
            println!("  wrote {path}");
        }
    }
}
