//! The home world's far-body bake (docs/proposals/MOON_WORLD.md M-6), timed, with previews: both
//! faces' maps at the data's size, written as PNG images — each face lit by a low sun from +x with
//! the water drawn in the water block's colour (`lit`), its albedo, and its water mask — and one
//! sheet of both faces lit (`home-<seed>-sheet.png`, the image in the proposal).
//!
//! `cargo run --release -p dwell-worldgen --example home_bake -- [seed] [out-dir]`

use std::time::Instant;

use dwell_world::Face;
use dwell_worldgen::bifacial::BifacialTerrain;
use dwell_worldgen::grids::FarBake;
use dwell_worldgen::home_far::HomeFar;

fn main() {
    let mut args = std::env::args().skip(1);
    let seed: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let out = args.next();
    let t = Instant::now();
    let terrain = BifacialTerrain::new(seed);
    println!(
        "world maps and generators: {:.0} ms",
        t.elapsed().as_secs_f64() * 1e3
    );
    let home = HomeFar::new(&terrain);
    let mut sheet: Vec<(usize, Vec<u8>)> = Vec::new();
    for (face, name) in [(Face::A, "a"), (Face::B, "b")] {
        let t = Instant::now();
        let bake = home.bake_default(face);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        let wet = bake.water.iter().filter(|&&w| w > 127).count();
        println!(
            "face {name}: {0} × {0} in {ms:.0} ms, {1} KiB, heights {2:.1}..{3:.1} m, {4:.1}% water",
            bake.size,
            bake.bytes() / 1024,
            bake.height_range[0],
            bake.height_range[1],
            100.0 * wet as f64 / (bake.size * bake.size) as f64
        );
        let Some(dir) = &out else { continue };
        let (lit, albedo, water) = previews(&bake);
        for (what, px, channels) in [
            ("lit", &lit, 3),
            ("albedo", &albedo, 3),
            ("water", &water, 1),
        ] {
            let path = format!("{dir}/home-{seed}-{name}-{what}.png");
            std::fs::write(&path, png(bake.size, bake.size, channels, px)).expect("write");
            println!("  wrote {path}");
        }
        sheet.push((bake.size, lit));
    }
    if let (Some(dir), [(s, a), (_, b)]) = (&out, sheet.as_slice()) {
        let gap = 8;
        let w = 2 * s + gap;
        let mut px = vec![0u8; w * s * 3];
        for j in 0..*s {
            px[j * w * 3..(j * w + s) * 3].copy_from_slice(&a[j * s * 3..(j + 1) * s * 3]);
            px[(j * w + s + gap) * 3..(j * w + w) * 3]
                .copy_from_slice(&b[j * s * 3..(j + 1) * s * 3]);
        }
        let path = format!("{dir}/home-{seed}-sheet.png");
        std::fs::write(&path, png(w, *s, 3, &px)).expect("write");
        println!("wrote {path}");
    }
}

/// The lit preview (RGB), the albedo (RGB) and the water mask (gray).
fn previews(bake: &FarBake) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let sun = {
        let (x, y): (f64, f64) = (0.8, 0.45);
        let l = x.hypot(y);
        [x / l, y / l, 0.0]
    };
    let w = dwell_blocks::info(dwell_world::M.water).color;
    let sea = [(w >> 16) & 0xff, (w >> 8) & 0xff, w & 0xff].map(f64::from);
    let (mut lit, mut albedo, mut water) = (Vec::new(), Vec::new(), Vec::new());
    for j in 0..bake.size {
        for i in 0..bake.size {
            let p = (j * bake.size + i) * 4;
            let n = bake.normal(i, j);
            let k = (n[0] * sun[0] + n[1] * sun[1] + n[2] * sun[2]).max(0.0);
            let a = &bake.albedo[p..p + 4];
            let wet = f64::from(bake.water[j * bake.size + i]) / 255.0;
            for c in 0..3 {
                let ground = f64::from(a[c]) * k * 1.5;
                let v = if a[3] == 0 {
                    0.0
                } else {
                    ground * (1.0 - wet) + sea[c] * 0.8 * wet
                };
                lit.push(v.min(255.0) as u8);
                albedo.push(a[c]);
            }
            water.push(bake.water[j * bake.size + i]);
        }
    }
    (lit, albedo, water)
}

/// A PNG of 8-bit gray (`channels` 1) or RGB (3) pixels, its deflate stream stored (no
/// compression): no image dependency for a preview.
fn png(w: usize, h: usize, channels: usize, px: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((w * channels + 1) * h);
    for row in px.chunks(w * channels) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (k, b) in blocks.iter().enumerate() {
        z.push(u8::from(k + 1 == blocks.len()));
        let len = b.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &v in &raw {
        a = (a + u32::from(v)) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, if channels == 1 { 0 } else { 2 }, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    for (kind, data) in [(b"IHDR", ihdr), (b"IDAT", z), (b"IEND", Vec::new())] {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut crc_in = kind.to_vec();
        crc_in.extend_from_slice(&data);
        out.extend_from_slice(&crc_in);
        out.extend_from_slice(&crc32(&crc_in).to_be_bytes());
    }
    out
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}
