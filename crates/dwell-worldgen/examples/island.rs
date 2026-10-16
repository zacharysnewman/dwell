//! The island's extent and a whole-island height map (Phase 16's island-size check, until Phase
//! 17's `dwell_worldgen_inspect`): `cargo run --release -p dwell-worldgen --example island -- [out
//! dir] [seeds…]` prints, per seed and face, where the coast lies (metres from the origin along 360
//! rays: nearest sea, farthest land, the mean of each ray's farthest land) and the land's area, and
//! writes `island-<seed>.png`, face A over a 100 km square centred on the origin with the world's
//! rim in red.

use std::io::Write;

use dwell_world::{Face, SEA_LEVEL, WORLD_RADIUS};
use dwell_worldgen::BifacialTerrain;

const HALF: i32 = 50_000;
const PIXELS: i32 = 1000;

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| ".".into());
    let mut seeds: Vec<u64> = args.filter_map(|s| s.parse().ok()).collect();
    if seeds.is_empty() {
        seeds = vec![1, 2, 3, 42, 12345];
    }
    println!("seed face nearest_sea_m farthest_land_m mean_coast_m land_km2");
    for &seed in &seeds {
        let terrain = BifacialTerrain::new(seed);
        for face in [Face::A, Face::B] {
            let g = terrain.local(face);
            let land = |x: i32, z: i32| g.column_at(x, z).height > SEA_LEVEL as f32;
            let (mut nearest_sea, mut farthest_land, mut sum) = (f64::MAX, 0f64, 0f64);
            for ray in 0..360 {
                let a = f64::from(ray).to_radians();
                let (mut first_sea, mut last_land) = (None, 0f64);
                let mut r = 0f64;
                while r < 60_000.0 {
                    let (x, z) = ((r * a.cos()) as i32, (r * a.sin()) as i32);
                    if land(x, z) {
                        last_land = r;
                    } else if first_sea.is_none() {
                        first_sea = Some(r);
                    }
                    r += 50.0;
                }
                nearest_sea = nearest_sea.min(first_sea.unwrap_or(60_000.0));
                farthest_land = farthest_land.max(last_land);
                sum += last_land;
            }
            let mut cells = 0u64;
            for z in (-HALF..HALF).step_by(250) {
                for x in (-HALF..HALF).step_by(250) {
                    cells += u64::from(land(x, z));
                }
            }
            println!(
                "{seed} {face:?} {nearest_sea:.0} {farthest_land:.0} {:.0} {:.0}",
                sum / 360.0,
                cells as f64 * 0.0625
            );
        }
        write_map(&terrain, seed, &out);
    }
}

fn write_map(terrain: &BifacialTerrain, seed: u64, out: &str) {
    let g = terrain.local(Face::A);
    let step = 2 * HALF / PIXELS;
    let mut rgb = Vec::with_capacity((PIXELS * PIXELS * 3) as usize);
    for py in 0..PIXELS {
        for px in 0..PIXELS {
            let (x, z) = (-HALF + px * step, -HALF + py * step);
            let r = (f64::from(x).hypot(f64::from(z)) - f64::from(WORLD_RADIUS)).abs();
            let col = g.column_at(x, z);
            let h = col.height - SEA_LEVEL as f32;
            let c = if r < f64::from(step) {
                [255, 40, 40]
            } else if col.outside {
                [0, 0, 0]
            } else if h <= 0.0 {
                let d = (1.0 + h / 25.0).clamp(0.0, 1.0);
                [20, (60.0 + 80.0 * d) as u8, (120.0 + 100.0 * d) as u8]
            } else {
                let t = (h / 150.0).clamp(0.0, 1.0);
                [
                    (70.0 + 160.0 * t) as u8,
                    (140.0 + 90.0 * t) as u8,
                    (60.0 + 170.0 * t) as u8,
                ]
            };
            rgb.extend_from_slice(&c);
        }
    }
    let path = format!("{out}/island-{seed}.png");
    std::fs::File::create(&path)
        .and_then(|mut f| f.write_all(&png(PIXELS as u32, PIXELS as u32, &rgb)))
        .expect("write the map");
}

/// An RGB PNG with stored (uncompressed) deflate blocks: no dependencies.
fn png(w: u32, h: u32, rgb: &[u8]) -> Vec<u8> {
    let crc = |data: &[u8]| {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xedb8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    };
    let mut raw = Vec::new();
    for row in rgb.chunks((w * 3) as usize) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        let n = b.len() as u16;
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (mut a, mut s) = (1u32, 0u32);
    for &b in &raw {
        a = (a + u32::from(b)) % 65_521;
        s = (s + a) % 65_521;
    }
    z.extend_from_slice(&((s << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut chunk = |kind: &[u8], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc(&body).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}
