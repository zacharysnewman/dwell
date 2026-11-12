//! The 2D cloud deck's lighting as CPU reference renders (docs/CLOUDS.md §10): a fixed noise patch
//! (8 × 8 noise cells, ~8 km) seen from below straight up, at a high sun (70°) and a low one (5°),
//! for the deck as Low drew it before the restore (no self-shadow sample), the old desktop deck
//! (before the volumetric work: all octaves and a full sample; today's Medium and High) and Low
//! restored (`light.lowShadowOctaves`). Writes PNGs and prints the relief measures.
//!
//! `cargo run -p dwell-look --example deck_lighting -- <out dir>`

use dwell_look::Rgb;
use dwell_look::clouds::{CLOUDS, CloudQuality, CloudTunables, deck_relief, deck_shade, light_hue};
use dwell_look::daynight::DAY_NIGHT;
use dwell_world::FaceFrame;

const N: usize = 256;

/// The morning's solar angle where the sun stands `deg` over the horizon, and the sun's and the
/// sky's hues there.
fn day_at(deg: f64) -> (Rgb, Rgb) {
    let want = deg.to_radians().sin();
    let (mut lo, mut hi) = (0.0, 90.0);
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if DAY_NIGHT.sun_direction(mid)[1] < want {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let s = DAY_NIGHT.sample(lo);
    let lin = |c: Rgb| c.map(dwell_look::srgb_to_linear);
    (light_hue(lin(s.sun_light)), light_hue(lin(s.hemi_sky)))
}

fn render(t: &CloudTunables, octaves: u32, shadow: u32, deg: f64) -> (Vec<u8>, f64, f64) {
    let r = deg.to_radians();
    let light = [r.cos(), r.sin(), 0.0];
    let (sun, sky) = day_at(deg);
    let blue = [0.42, 0.6, 0.86];
    let mut px = Vec::with_capacity(N * N * 3);
    let (mut lum_sum, mut lum_sq, mut n) = (0.0f64, 0.0f64, 0.0f64);
    for j in 0..N {
        for i in 0..N {
            let p = [8.0 * i as f64 / N as f64, 8.0 * j as f64 / N as f64];
            let s = deck_shade(
                t,
                p,
                [0.0; 2],
                light,
                FaceFrame::A,
                [0.0, 1.0, 0.0],
                octaves,
                shadow,
                t.shape.coverage,
                sun,
                sky,
            );
            let a = s.density * t.shape.opacity;
            let c: Vec<f64> = (0..3)
                .map(|k| blue[k] + (s.colour[k] - blue[k]) * a)
                .collect();
            if s.density > 0.15 {
                let l = 0.2126 * s.colour[0] + 0.7152 * s.colour[1] + 0.0722 * s.colour[2];
                lum_sum += l;
                lum_sq += l * l;
                n += 1.0;
            }
            px.extend(c.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
        }
    }
    let mean = lum_sum / n.max(1.0);
    (
        px,
        mean,
        (lum_sq / n.max(1.0) - mean * mean).max(0.0).sqrt(),
    )
}

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

/// An RGB8 PNG, its image data in stored (uncompressed) deflate blocks.
fn png(w: usize, h: usize, rgb: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(h * (w * 3 + 1));
    for row in rgb.chunks(w * 3) {
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
    let (mut a, mut bb) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65_521;
        bb = (bb + a) % 65_521;
    }
    z.extend_from_slice(&((bb << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc(&body).to_be_bytes());
    };
    let mut ihdr = (w as u32).to_be_bytes().to_vec();
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    std::fs::create_dir_all(&out).expect("out dir");
    let t = &CLOUDS;
    let low = CloudQuality::Low;
    let variants = [
        ("before-low", low.octaves(t), 0),
        (
            "old-desktop",
            CloudQuality::Medium.octaves(t),
            CloudQuality::Medium.shadow_octaves(t),
        ),
        ("restored-low", low.octaves(t), low.shadow_octaves(t)),
    ];
    println!(
        "variant        sun  value-noises  sunSide farSide relief  core  thin  lum-mean lum-sd"
    );
    for (name, oct, sh) in variants {
        for deg in [70.0, 5.0] {
            let (px, mean, sd) = render(t, oct, sh, deg);
            let file = format!("{out}/deck-{name}-{deg:.0}deg.png");
            std::fs::write(&file, png(N, N, &px)).expect("write");
            let r = deg.to_radians();
            let rel = deck_relief(t, oct, sh, [r.cos(), r.sin(), 0.0], FaceFrame::A, 96);
            // The warp's two 2-octave fields, the shape's octaves and the sample's.
            let noises = 4 + oct + sh;
            println!(
                "{name:<14} {deg:>3.0}° {noises:>8}      {:.3}   {:.3}   {:+.3}  {:.3} {:.3}  {mean:.3}    {sd:.3}",
                rel.sun_side,
                rel.far_side,
                rel.sun_side - rel.far_side,
                rel.core,
                rel.thin
            );
        }
    }
}
