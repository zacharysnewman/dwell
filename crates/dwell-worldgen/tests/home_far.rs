//! The home world's far-body bake (docs/proposals/MOON_WORLD.md M-6), on both faces: deterministic
//! per seed and different between seeds and faces, its water mask the world map's sea and lakes,
//! its normals out of the mountains' slopes, its colours the surface blocks', its size the data's
//! and its cost and memory bounded.

use std::sync::OnceLock;
use std::time::Instant;

use dwell_world::{Face, both_faces};
use dwell_worldgen::bifacial::BifacialTerrain;
use dwell_worldgen::grids::{CrateredTerrain, FarBake, GRIDS_JSON, parse_grids};
use dwell_worldgen::home_far::{self, HomeFar, MAX_SAMPLES};
use dwell_worldgen::worldmap::{NO_LAKE, WorldMap};

fn terrain(seed: u64) -> &'static BifacialTerrain {
    static T: [OnceLock<BifacialTerrain>; 2] = [OnceLock::new(), OnceLock::new()];
    let slot = match seed {
        5 => &T[0],
        6 => &T[1],
        _ => unreachable!("the tests' seeds"),
    };
    slot.get_or_init(|| BifacialTerrain::new(seed))
}

/// Face A's 256² bake of seed 5, baked once for the tests that read it.
fn bake(face: Face) -> &'static FarBake {
    static B: [OnceLock<FarBake>; 2] = [OnceLock::new(), OnceLock::new()];
    let slot = match face {
        Face::A => &B[0],
        Face::B => &B[1],
    };
    slot.get_or_init(|| HomeFar::new(terrain(5)).bake(face, 256))
}

fn map(face: Face) -> &'static WorldMap {
    terrain(5).local(face).map()
}

fn other(face: Face) -> Face {
    match face {
        Face::A => Face::B,
        Face::B => Face::A,
    }
}

/// The map's sea or a lake at (x, z).
fn wet(m: &WorldMap, x: f64, z: f64) -> bool {
    let c = m.cell_at(x, z);
    m.is_sea(c) || m.lake[c] != NO_LAKE
}

both_faces!(
    /// A pure function of (world seed, face): the same seed bakes the same bytes, another seed and
    /// the other face bake others.
    fn the_home_bake_is_deterministic_and_differs_by_seed_and_face(f: FaceFrame) {
        let size = 64;
        let a = HomeFar::new(terrain(5)).bake(f.face, size);
        assert_eq!(a.normal_height.len(), size * size * 4);
        assert_eq!(a.albedo.len(), size * size * 4);
        assert_eq!(a.water.len(), size * size);
        assert_eq!(
            a,
            HomeFar::new(terrain(5)).bake(f.face, size),
            "deterministic"
        );
        let b = HomeFar::new(terrain(6)).bake(f.face, size);
        assert_ne!(a.normal_height, b.normal_height, "another seed");
        assert_ne!(a.water, b.water, "another seed's coast");
        let o = HomeFar::new(terrain(5)).bake(other(f.face), size);
        assert_ne!(a.normal_height, o.normal_height, "the other face");
        assert_eq!(
            a.radius_m,
            map(f.face).world_radius() as f64,
            "the world's disc"
        );
    }
);

both_faces!(
    /// Each texel's water byte is the share of its samples (the map's cells under it) in the map's
    /// sea or a lake; texels wholly over the sea are 255, outside the disc 0, and the bake's wet
    /// share is the map's inside the disc.
    fn the_water_mask_matches_the_map_s_sea(f: FaceFrame) {
        let (b, m) = (bake(f.face), map(f.face));
        let s = b.texel_m();
        let n = ((s / m.cell).ceil() as usize).clamp(1, MAX_SAMPLES);
        let step = s / n as f64;
        let (mut sea_texels, mut wet_bake, mut wet_map, mut inside) = (0, 0.0, 0.0, 0.0);
        for j in 0..b.size {
            for i in 0..b.size {
                let (x, z) = (b.texel_centre(i), b.texel_centre(j));
                let w = b.water[j * b.size + i];
                if x.hypot(z) > b.radius_m {
                    assert_eq!(w, 0, "outside the disc at ({i}, {j})");
                    continue;
                }
                let mut k = 0;
                for q in 0..n * n {
                    let sx = x - 0.5 * s + (q % n) as f64 * step + 0.5 * step;
                    let sz = z - 0.5 * s + (q / n) as f64 * step + 0.5 * step;
                    k += usize::from(wet(m, sx, sz));
                }
                let want = (k as f64 / (n * n) as f64 * 255.0).round() as u8;
                assert_eq!(w, want, "texel ({i}, {j}): {k}/{} samples wet", n * n);
                if k == n * n && m.is_sea(m.cell_at(x, z)) {
                    sea_texels += 1;
                    assert_eq!(w, 255);
                }
                wet_bake += f64::from(w) / 255.0;
                wet_map += f64::from(u8::from(wet(m, x, z)));
                inside += 1.0;
            }
        }
        assert!(
            sea_texels > 1000,
            "the island stands in a sea: {sea_texels}"
        );
        let (a, c) = (wet_bake / inside, wet_map / inside);
        assert!((a - c).abs() < 0.02, "wet share {a:.3} vs the map's {c:.3}");
        assert!(a > 0.2 && a < 0.9, "both land and sea: {a:.3}");
    }
);

both_faces!(
    /// On the mountains' steep slopes the baked normal leans downhill (against the map's gradient
    /// over two texels),
    /// and over open sea it is straight up: the water bakes flat.
    fn normals_point_out_of_mountain_slopes(f: FaceFrame) {
        let (b, m) = (bake(f.face), map(f.face));
        let (mut steep, mut downhill, mut flat_sea) = (0, 0, 0);
        for j in 2..b.size - 2 {
            for i in 2..b.size - 2 {
                let (x, z) = (b.texel_centre(i), b.texel_centre(j));
                if x.hypot(z) > b.radius_m - 2.0 * b.texel_m() {
                    continue;
                }
                let nrm = b.normal(i, j);
                if b.water[j * b.size + i] == 255 && m.is_sea(m.cell_at(x, z)) {
                    // Whole-sea texels around too: a flat surface.
                    let all_sea = (j - 1..=j + 1)
                        .all(|jj| (i - 1..=i + 1).all(|ii| b.water[jj * b.size + ii] == 255));
                    if all_sea {
                        assert!(nrm[1] > 0.99, "flat sea at ({i}, {j}): {nrm:?}");
                        flat_sea += 1;
                    }
                    continue;
                }
                // The map's slope at the bake's scale (across two texels, as its normals are).
                let at = |dx: f64, dz: f64| m.height_at(x + dx, z + dz).0;
                let s = b.texel_m();
                let gx = (at(s, 0.0) - at(-s, 0.0)) / (2.0 * s);
                let gz = (at(0.0, s) - at(0.0, -s)) / (2.0 * s);
                let g = gx.hypot(gz);
                if at(0.0, 0.0) < 300.0 || g < 0.2 {
                    continue;
                }
                steep += 1;
                downhill += usize::from(nrm[0] * gx + nrm[2] * gz < 0.0);
            }
        }
        assert!(flat_sea > 500, "{flat_sea} open-sea texels");
        assert!(steep > 40, "mountain slopes to test: {steep}");
        assert!(
            downhill * 20 >= steep * 19,
            "{downhill}/{steep} steep texels lean downhill"
        );
    }
);

both_faces!(
    /// Every texel's colour is a block's (blocks.json `color`), more than one (the biomes' and
    /// bands' palettes), and the moon's dry bake has no water.
    fn the_colours_are_the_surface_blocks_and_the_moon_is_dry(f: FaceFrame) {
        let b = bake(f.face);
        let colours: std::collections::HashSet<u32> =
            dwell_blocks::STATES.iter().map(|s| s.color).collect();
        let mut seen = std::collections::HashSet::new();
        for p in b.albedo.chunks(4).filter(|p| p[3] == 255) {
            let c = (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]);
            assert!(colours.contains(&c), "{c:06x} is no block's colour");
            seen.insert(c);
        }
        assert!(seen.len() >= 3, "several surface blocks: {seen:x?}");
        let moon = CrateredTerrain::for_grid("dwell:moon", 5).unwrap();
        assert!(moon.bake_far(f.face, 32).water.iter().all(|&w| w == 0));
    }
);

/// The data's size (256²), its memory exactly the three maps (576 KiB a face) and its time a pass
/// over the map: well under a second natively (CI's debug test build included; the release
/// example measures ~0.17 s a face).
#[test]
fn the_bake_s_size_cost_and_memory_are_bounded() {
    let t = terrain(5);
    assert_eq!(home_far::bake_size(), 256, "grids.json's home bake.size");
    let home = HomeFar::new(t);
    let start = Instant::now();
    let b = home.bake_default(Face::A);
    let s = start.elapsed().as_secs_f64();
    assert_eq!(b.size, 256);
    assert_eq!(b.bytes(), 256 * 256 * 9);
    assert!(b.bytes() <= 600 * 1024, "{} bytes", b.bytes());
    assert!(s < 3.0, "the bake took {s:.2} s");
}

/// The home generator's `bake.size` is validated like the moon's.
#[test]
fn a_bad_home_bake_size_fails_with_a_message() {
    let bad = GRIDS_JSON.replacen(
        "\"bake\": { \"size\": 256 }",
        "\"bake\": { \"size\": 7 }",
        1,
    );
    assert_ne!(bad, GRIDS_JSON);
    let e = parse_grids(&bad).unwrap_err();
    assert!(e.contains("dwell:home") && e.contains("bake.size"), "{e}");
    let none = GRIDS_JSON.replacen(",\n        \"bake\": { \"size\": 256 }", "", 1);
    assert!(parse_grids(&none).unwrap_err().contains("bake.size"));
}
