//! A top-down view of the terrain as chunks generate it (ADR 0049): a square of columns rendered as
//! a hillshaded PNG with water, snow, rock and sand, to judge the world map's terrain without the
//! game. `cargo run --release -p dwell-worldgen --example terrain_view -- --seed 7 --face a
//! --x 0 --z 0 --size 2048 --step 4 --out FILE.png [--preset highlands | --settings land:0.7,…]`,
//! and timings of the map and a few chunks. With a preset or settings (`settings.json`), the world
//! is generated under them; the portals' sites are listed.

use std::time::Instant;

use dwell_world::{Chunk, ChunkCoord, Face, SEA_LEVEL_M};
use dwell_worldgen::terrain::{Band, Water};
use dwell_worldgen::{BifacialTerrain, WorldSettings};

fn arg(args: &[String], name: &str, default: &str) -> String {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
        .unwrap_or_else(|| default.to_string())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seed: u64 = arg(&args, "--seed", "7").parse().expect("seed");
    let face = if arg(&args, "--face", "a") == "b" {
        Face::B
    } else {
        Face::A
    };
    let cx: i32 = arg(&args, "--x", "0").parse().expect("x");
    let cz: i32 = arg(&args, "--z", "0").parse().expect("z");
    let size: i32 = arg(&args, "--size", "2048").parse().expect("size");
    let step: i32 = arg(&args, "--step", "4").parse().expect("step");
    let out = arg(&args, "--out", "terrain.png");
    let preset = arg(&args, "--preset", "");
    let settings = if preset.is_empty() {
        WorldSettings::decode(&arg(&args, "--settings", "")).expect("--settings")
    } else {
        WorldSettings::preset(&preset).expect("--preset names a preset of settings.json")
    };
    println!("settings {:?} (map hash {:016x})", settings.encode(), settings.map_hash());
    let t0 = Instant::now();
    let terrain = BifacialTerrain::with_settings(seed, &settings);
    println!("maps and portals: {:.2} s", t0.elapsed().as_secs_f64());
    println!("portals {:?}", terrain.portal_sites());
    let g = terrain.local(face);
    let map = g.map().clone();
    println!(
        "map digest {:016x}: {} lakes, {} rivers",
        map.digest(),
        map.lakes.len(),
        map.rivers.len()
    );
    if let Some(r) = map.rivers.iter().max_by(|a, b| {
        let qa = a.path.last().map_or(0.0, |v| v.discharge);
        let qb = b.path.last().map_or(0.0, |v| v.discharge);
        qa.total_cmp(&qb)
    }) {
        for k in [
            r.path.len() / 4,
            r.path.len() / 2,
            3 * r.path.len() / 4,
            r.path.len() - 2,
        ] {
            let v = r.path[k];
            println!(
                "largest river vertex {k}/{}: ({}, {}) surface {:.1} m, width {:.1} m, {:.1} m³/s, mouth {:?}",
                r.path.len(),
                v.x,
                v.z,
                v.surface,
                v.width,
                v.discharge,
                r.mouth
            );
        }
    }
    for (k, l) in map.lakes.iter().enumerate() {
        let (x, z) = map.centre(l.entry as usize);
        println!("lake {k}: level {:.1} m, entry ({x}, {z})", l.level);
    }
    let probe = arg(&args, "--probe", "");
    if !probe.is_empty() {
        let mut it = probe.split(',').map(|v| v.parse::<i32>().expect("probe"));
        let (px, pz, dz, k) = (
            it.next().expect("x"),
            it.next().expect("z"),
            it.next().unwrap_or(0),
            it.next().unwrap_or(1),
        );
        for s in 0..k {
            let (x, z) = (px, pz + s * dz);
            let c = g.column_at(x, z);
            let hit = map
                .channels
                .query(f64::from(x), f64::from(z), 0.0, f64::INFINITY);
            let lake = map.shores.query(&map, f64::from(x), f64::from(z));
            let (base, _, _) = map.height_at(f64::from(x), f64::from(z));
            println!(
                "({x}, {z}): base {base:.2} col h {:.2} level {:.2} {:?} {:?} wet {} | river {:?} | lake {:?}",
                c.height - SEA_LEVEL_M as f32,
                c.level - SEA_LEVEL_M as f32,
                c.water,
                c.band,
                c.wet(),
                hit.map(|h| (h.d, h.half, h.level, h.river)),
                lake.map(|l| (l.lake, l.level, l.sdf))
            );
        }
        return;
    }
    let n = (size / step) as usize;
    let mut h = vec![0f32; n * n];
    let mut cols = Vec::with_capacity(n * n);
    let t1 = Instant::now();
    for j in 0..n {
        for i in 0..n {
            let x = cx - size / 2 + i as i32 * step;
            let z = cz - size / 2 + j as i32 * step;
            let c = g.column_at(x, z);
            h[j * n + i] = c.height;
            cols.push(c);
        }
    }
    println!(
        "{} columns: {:.1} µs each",
        n * n,
        t1.elapsed().as_secs_f64() * 1e6 / (n * n) as f64
    );
    let mut img = vec![0u8; n * n * 3];
    for j in 0..n {
        for i in 0..n {
            let c = cols[j * n + i];
            let at = |a: usize, b: usize| h[b.min(n - 1) * n + a.min(n - 1)];
            let dx = at(i + 1, j) - at(i.saturating_sub(1), j);
            let dz = at(i, j + 1) - at(i, j.saturating_sub(1));
            let shade = (1.0 - 0.6 * (dx - dz) / (2.0 * step as f32)).clamp(0.3, 1.6);
            let above = c.height - SEA_LEVEL_M as f32;
            let mut rgb = match c.band {
                Band::Snow => [235.0, 240.0, 248.0],
                Band::Rock => [150.0, 140.0, 135.0],
                Band::Shore => [225.0, 205.0, 150.0],
                Band::Ground => {
                    let t = (above / 600.0).clamp(0.0, 1.0);
                    [110.0 + 40.0 * t, 170.0 - 40.0 * t, 70.0 + 20.0 * t]
                }
            };
            for v in &mut rgb {
                *v *= shade;
            }
            if c.outside {
                rgb = [0.0, 0.0, 0.0];
            } else if c.wet() {
                let depth = (c.level - c.height).clamp(0.0, 20.0) / 20.0;
                rgb = match c.water {
                    Water::River => [60.0, 120.0, 230.0],
                    Water::Lake => [40.0, 90.0, 200.0],
                    _ => [
                        40.0 - 20.0 * depth,
                        100.0 - 40.0 * depth,
                        190.0 - 40.0 * depth,
                    ],
                };
            }
            for k in 0..3 {
                img[(j * n + i) * 3 + k] = rgb[k].clamp(0.0, 255.0) as u8;
            }
        }
    }
    write_png(&out, n as u32, n as u32, &img);
    println!("wrote {out}");
    // Chunk timings round the view's centre, at the ground.
    let t2 = Instant::now();
    let mut count = 0;
    for dz in -2..2 {
        for dx in -2..2 {
            let x = cx / 32 + dx;
            let z = cz / 32 + dz;
            let ground = g.ground_y(x * 32 + 16, z * 32 + 16).unwrap_or(64) / 32;
            for dy in -1..=1 {
                let mut chunk = Chunk::default();
                let y = if face == Face::A {
                    ground + dy
                } else {
                    -1 - (ground + dy)
                };
                terrain.generate(ChunkCoord::new(x, y, z), &mut chunk);
                count += 1;
            }
        }
    }
    println!(
        "{count} chunks: {:.2} ms each",
        t2.elapsed().as_secs_f64() * 1e3 / f64::from(count)
    );
    println!("spawn {:?}", terrain.spawn_point());
}

fn write_png(path: &str, w: u32, h: u32, rgb: &[u8]) {
    // A minimal PNG writer: stored (uncompressed) deflate blocks.
    fn crc(data: &[u8]) -> u32 {
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
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc(&body).to_be_bytes());
    }
    let mut raw = Vec::new();
    for y in 0..h as usize {
        raw.push(0);
        raw.extend_from_slice(&rgb[y * w as usize * 3..(y + 1) * w as usize * 3]);
    }
    let mut z = vec![0x78, 0x01];
    for (i, block) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        z.push(u8::from(last));
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    std::fs::write(path, out).expect("write png");
}
