//! Preview sheets of the tree impostor bake (docs/proposals/TREE_IMPOSTORS.md phase I-1): for each
//! species, a mature tree baked at the desktop tier's view size, as three atlases side by side: the
//! albedo lit by a sun from the data's normals (what the drawer will do), the normals, the depth.
//!
//! `cargo run --release -p dwell-worldgen --example impostor_bake -- [out.png] [seed]`
//! (default `target/impostors.png`, seed 3). The committed sheet is
//! `docs/proposals/tree-impostors-bake.png`.

use dwell_worldgen::impostor::{Config, Impostor, Tier, bake};
use dwell_worldgen::species::SPECIES;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args
        .get(1)
        .cloned()
        .unwrap_or("target/impostors.png".into());
    let seed: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let c = Config::get();
    let px = c.px(Tier::Desktop);
    let bakes: Vec<Impostor> = SPECIES
        .iter()
        .map(|s| bake(s, seed, s.stages - 1, c, px))
        .collect();
    let (aw, ah) = bakes[0].size();
    let gap = 8;
    let (w, h) = (3 * aw + 4 * gap, bakes.len() as u32 * (ah + gap) + gap);
    let mut img = vec![236u8; (w * h * 3) as usize];
    let sun = norm([0.4, 0.8, 0.45]);
    for (row, im) in bakes.iter().enumerate() {
        let (stdout_w, _) = im.size();
        println!(
            "{}: {} views, {}×{} texels, {:.1} m radius, {} KB",
            SPECIES[row].name,
            im.views.len(),
            stdout_w,
            ah,
            im.radius,
            im.bytes() / 1024
        );
        let y0 = gap + row as u32 * (ah + gap);
        for y in 0..ah {
            for x in 0..aw {
                let k = (y * aw + x) as usize;
                let a = im.albedo[k];
                let nd = im.normal_depth[k];
                let n = [0, 1, 2].map(|i| nd[i] as f32 / 255.0 * 2.0 - 1.0);
                let lit = 0.35 + 0.75 * dot(n, sun).max(0.0);
                let panels = [
                    [0, 1, 2].map(|i| (a[i] as f32 * lit).min(255.0) as u8),
                    [nd[0], nd[1], nd[2]],
                    [nd[3]; 3],
                ];
                for (p, colour) in panels.iter().enumerate() {
                    if a[3] == 0 {
                        continue;
                    }
                    let ox = gap + p as u32 * (aw + gap) + x;
                    let o = (((y0 + y) * w + ox) * 3) as usize;
                    img[o..o + 3].copy_from_slice(colour);
                }
            }
        }
    }
    std::fs::write(&out, png(w as usize, h as usize, 3, &img)).expect("write the sheet");
    println!("wrote {out} ({w}×{h})");
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt();
    a.map(|c| c / l)
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
