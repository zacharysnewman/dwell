//! The Realistic world's own goldens and its memory (docs/proposals/REALISTIC_WORLD.md R6): the
//! world offered in the New world form as Realistic (`shared/worldgen/worlds/realistic.json`
//! `newWorld`) is held to `shared/worldgen/golden/realistic-hashes.txt` (chunks at the ground and
//! LOD sections on both faces of one seed), as the default world is to `tests/golden.rs`'s files.
//! While it is a test world its seeds' terrain may change between releases (owner, §7 q7): then
//! `REALISTIC_GOLDEN=write` rewrites the file, with a note in the change. The default world's
//! goldens never read this world (`tests/golden.rs`).
//!
//! The memory check (R6, BUGS.md #70's bar): both faces' biome sets of a Realistic world as a
//! process holds them, against the starter's (`tests/starter_memory.rs`).

use std::fmt::Write as _;
use std::sync::Arc;

use dwell_world::lod::{LodCells, LodCoord, lod_hash, section_size};
use dwell_world::{CHUNK_SIZE, Chunk, Face, FaceFrame, GENERATOR_TERRAIN, chunk_hash, chunk_of};
use dwell_worldgen::bifacial::{BifacialTerrain, face_seed};
use dwell_worldgen::config::worldgen_map_bytes;
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";
const SEED: u64 = 42;
const GOLDEN: &str = include_str!("../../../shared/worldgen/golden/realistic-hashes.txt");

/// Installs the Realistic world's two face maps (once per process) and returns them.
fn maps() -> Vec<(u64, Arc<WorldMap>)> {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        for f in [Face::A, Face::B] {
            let bytes = worldgen_map_bytes(face_seed(SEED, f), NAME).expect("the Realistic map");
            dwell_worldgen::worldmap::install(&bytes).expect("installs");
        }
    });
    [Face::A, Face::B]
        .into_iter()
        .map(|f| {
            let s = face_seed(SEED, f);
            (s, dwell_worldgen::worldmap::for_seed(s))
        })
        .collect()
}

/// The golden's rows: `chunk x y z hash` and `lod level i j k kind hash`, computed now.
fn rows() -> String {
    maps();
    let terrain = BifacialTerrain::for_generator(GENERATOR_TERRAIN, SEED).expect("the terrain");
    let mut out = String::new();
    let mut chunk = Chunk::default();
    let mut cells = LodCells::new();
    for face in [Face::A, Face::B] {
        let g = terrain.local(face);
        let f = FaceFrame::of(face);
        // Columns across the land and the coast every 6 km, out to 30 km: the chunk at the ground
        // and the one above it (a cone's, a range's or a river's, as the seed puts them).
        for z in (-30_000..=30_000).step_by(6_000) {
            for x in (-30_000..=30_000).step_by(6_000) {
                let Some(y) = g.ground_y(x, z) else { continue };
                for dy in [0, 1] {
                    let c = chunk_of(x, f.row(y + dy * CHUNK_SIZE), z);
                    terrain.generate(c, &mut chunk);
                    writeln!(
                        out,
                        "chunk {} {} {} {:x}",
                        c.x,
                        c.y,
                        c.z,
                        chunk_hash(&chunk)
                    )
                    .expect("write");
                }
            }
        }
        // LOD sections over the face at three levels, on the face's side of the midplane.
        for level in [2, 5, 8] {
            let s = section_size(level);
            for z in (-24_000i64..=24_000).step_by(12_000) {
                for x in (-24_000i64..=24_000).step_by(12_000) {
                    let y = g
                        .ground_y(x as i32, z as i32)
                        .map_or(0, |y| i64::from(f.row(y)));
                    let at = |p: i64, o: i64| (p - o).div_euclid(s) as i32;
                    let c = LodCoord::new(
                        level,
                        at(x, dwell_world::lod::ORIGIN_X),
                        at(y, dwell_world::lod::ORIGIN_Y),
                        at(z, dwell_world::lod::ORIGIN_Z),
                    );
                    let kind = terrain.generate_lod(c, &mut cells, None, None);
                    writeln!(
                        out,
                        "lod {} {} {} {} {} {:x}",
                        c.level,
                        c.i,
                        c.j,
                        c.k,
                        kind as u8,
                        lod_hash(kind, &cells)
                    )
                    .expect("write");
                }
            }
        }
    }
    out
}

#[test]
fn the_realistic_world_matches_its_golden_hashes() {
    // both-faces: hashes each face's chunks and sections.
    let got = rows();
    let registry = format!("{:x}", dwell_blocks::REGISTRY_HASH);
    if std::env::var("REALISTIC_GOLDEN").as_deref() == Ok("write") {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../shared/worldgen/golden/realistic-hashes.txt"
        );
        let head = format!(
            "# The Realistic world (test:realistic), seed {SEED}, both faces: chunk x y z fnv1a64(voxels) | lod level i j k kind fnv1a64(kind, cells)\n# Rewrite: REALISTIC_GOLDEN=write cargo test --release -p dwell-worldgen --test realistic_golden\n# registry {registry}\n"
        );
        std::fs::write(path, head + &got).expect("write realistic-hashes.txt");
        return;
    }
    let want_registry = GOLDEN
        .lines()
        .find_map(|l| l.strip_prefix("# registry "))
        .expect("a registry line")
        .trim();
    assert_eq!(
        want_registry, registry,
        "the golden was written with another block registry: rewrite it (REALISTIC_GOLDEN=write)"
    );
    let want: Vec<&str> = GOLDEN
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .collect();
    let got: Vec<&str> = got.lines().collect();
    assert!(want.len() >= 100, "{} golden rows", want.len());
    assert_eq!(got.len(), want.len(), "the golden's rows");
    let failures: Vec<String> = got
        .iter()
        .zip(&want)
        .filter(|(g, w)| g != w)
        .map(|(g, w)| format!("{g}, golden {w}"))
        .collect();
    assert!(
        failures.is_empty(),
        "the Realistic world's terrain changed ({} rows): if on purpose, rewrite with \
         REALISTIC_GOLDEN=write and note it\n{}",
        failures.len(),
        failures.join("\n")
    );
}

const MB: f64 = 1024.0 * 1024.0;

/// The starter's budget (`tests/starter_memory.rs` `MAX_HELD`, BUGS.md #70).
const MAX_HELD: usize = 20 << 20;

#[test]
fn the_realistic_worlds_biome_sets_fit_a_phones_budget() {
    // both-faces: measures the two faces' sets together, as a process holds them.
    let maps = maps();
    let mut held = 0;
    for (s, m) in &maps {
        let set = dwell_worldgen::terrain::shared_biomes(m, *s);
        println!("face map {} × {} cells:", m.n, m.n);
        for (name, bytes) in set.grid_bytes() {
            println!("  {name:<28} {:>7.2} MB", bytes as f64 / MB);
            held += bytes;
        }
    }
    println!("both faces: {:.2} MB", held as f64 / MB);
    assert!(
        held <= MAX_HELD,
        "the Realistic world's biome sets hold {:.1} MB in a process (at most {:.0} MB)",
        held as f64 / MB,
        MAX_HELD as f64 / MB
    );
}

#[test]
fn the_big_maps_layers_sample_the_realistic_worlds_plates_uplift_rain_and_rocks() {
    // both-faces: each face's own plates and climate (R1, R3) show in its layers.
    use dwell_worldgen::map_layers::{MapField, sample};
    maps();
    let terrain = BifacialTerrain::for_generator(GENERATOR_TERRAIN, SEED).expect("the terrain");
    // The whole disc (the realistic batch's plates reach the rim, 0.64).
    let n = 64;
    let (x0, step) = (-48_000, 96_000 / n as i64);
    for face in [Face::A, Face::B] {
        let g = terrain.local(face);
        let p = sample(g, MapField::Plates, x0, x0, step, n).expect("plates in a Realistic world");
        assert_eq!(p.names, ["oceanic", "continental"]);
        assert!(
            p.cats.contains(&0) && p.cats.contains(&1),
            "{face:?}: both kinds"
        );
        let mut ids = p.ids.clone();
        ids.sort_unstable();
        ids.dedup();
        assert!(ids.len() >= 3, "{face:?}: {} plates", ids.len());
        let u = sample(g, MapField::Uplift, x0, x0, step, n).expect("uplift");
        let hi = u.values.iter().copied().fold(f32::MIN, f32::max);
        let lo = u.values.iter().copied().fold(f32::MAX, f32::min);
        // Raised and cut ground: a range where the seed has a subducting or colliding boundary,
        // else a margin's or a ridge's tens of metres.
        eprintln!("{face:?}: uplift {lo}..{hi} m");
        assert!(hi > 30.0 && lo < 0.0, "{face:?}: uplift {lo}..{hi} m");
        let r = sample(g, MapField::Rain, x0, x0, step, n).expect("rain from the climate");
        let wet = r.values.iter().copied().fold(f32::MIN, f32::max);
        let dry = r.values.iter().copied().fold(f32::MAX, f32::min);
        assert!(
            dry >= 0.0 && wet > 3.0 * dry.max(50.0),
            "{face:?}: rain {dry}..{wet} mm"
        );
        let k = sample(g, MapField::Rocks, x0, x0, step, n).expect("rocks");
        let used: std::collections::BTreeSet<&str> = k
            .cats
            .iter()
            .map(|&c| k.names[usize::from(c)].as_str())
            .collect();
        assert!(used.len() >= 3, "{face:?}: rocks {used:?}");
        assert!(
            k.colors.iter().all(|&c| c <= 0xff_ffff),
            "every rock has its colour"
        );
    }
}
