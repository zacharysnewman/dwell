//! What a many-biome world's biome sets hold in a process (BUGS.md #70: the starter world reloaded
//! iOS Safari's tab): both faces' sets of the starter world, each whole-map grid's bytes as held
//! (its allocation, `BiomeSet::grid_bytes`). Build-time scratch is dropped by then. The terrain a
//! seed generates is a public API, and the build's goldens (`tests/golden.rs`) are the default
//! world's, whose one biome has no grids: the starter's own terrain is pinned here too, so storing
//! the grids smaller cannot change it.

use std::sync::Arc;

use dwell_world::lod::{LodCells, LodCoord, lod_hash, section_size, sections_across};
use dwell_world::{Chunk, Face, GENERATOR_TERRAIN, WORLD_RADIUS, chunk_hash, chunk_of};
use dwell_worldgen::bifacial::{BifacialTerrain, face_seed};
use dwell_worldgen::config::worldgen_map_bytes;
use dwell_worldgen::worldmap::WorldMap;

const SEED: u64 = 7;

/// Installs the starter's two face maps (once per process) and returns them by face seed.
fn starter_maps() -> Vec<(u64, Arc<WorldMap>)> {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        for f in [Face::A, Face::B] {
            let bytes =
                worldgen_map_bytes(face_seed(SEED, f), "test:starter").expect("the starter's map");
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

const MB: f64 = 1024.0 * 1024.0;

/// A process's starter biome sets (both faces) hold at most this (BUGS.md #70: 53.6 MB before,
/// 15.9 MB since; the brief's bar was 40 MB).
const MAX_HELD: usize = 20 << 20;

#[test]
fn the_starter_worlds_biome_sets_fit_a_phones_budget() {
    // both-faces: measures the two faces' sets together, as a process holds them.
    let maps = starter_maps();
    let sets: Vec<_> = maps
        .iter()
        .map(|(s, m)| dwell_worldgen::terrain::shared_biomes(m, *s))
        .collect();
    let mut held = 0;
    for ((_, m), set) in maps.iter().zip(&sets) {
        println!("face map {} × {} cells:", m.n, m.n);
        for (name, bytes) in set.grid_bytes() {
            println!("  {name:<28} {:>7.2} MB", bytes as f64 / MB);
            held += bytes;
        }
    }
    println!("both faces: {:.2} MB", held as f64 / MB);
    assert!(
        held <= MAX_HELD,
        "the starter's biome sets hold {:.1} MB in a process (at most {:.0} MB)",
        held as f64 / MB,
        MAX_HELD as f64 / MB
    );
}

/// FNV-1a over bytes.
fn fnv(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= u64::from(b);
        *h = h.wrapping_mul(0x0100_0000_01b3);
    }
}

/// The starter's terrain on both faces, hashed: every grid's reads (the biome geography, the
/// rainfall, the erosion's cut-back) and the column every 128 m over the land, the chunks at the
/// surface of every 512th of those columns, and LOD sections over the face at two levels.
/// `STARTER_PIN=print` prints the hashes to re-pin after a deliberate change of the starter world
/// (a worldgen change: a new compatibility line).
///
/// Re-pinned 2026-11-11 for the steppe's `undergrowth` 1 → 0.75 (`shared/worldgen/worlds/starter.json`,
/// sparse tall grass in its dry lee): the earlier pin was recorded on code from before that change
/// (with the steppe back at 1 the old hashes match exactly, so it was stale, not a float difference).
/// Re-pinned 2026-11-12 for the meadow grass (WORK_QUEUE item 21, the 0.54 line): tall grass over
/// ½ m grass ramps, wider and denser patches (`decorations.json`), the steppe's `undergrowth`
/// 0.75 → 0.85.
/// Re-pinned 2026-11-12 for the old-growth forest (WORK_QUEUE item 32): `preset:old_growth` on the
/// starter's cool, wet lowland (its old-growth species, the `old_growth` placement and undergrowth); a
/// change of the starter's terrain, so the owner raises the next line (0.55).
/// Re-pinned 2026-11-12 for the trunk pads (WORK_QUEUE item 44, the 0.55 line): the ground under a
/// placed tree's trunk levelled to its base, ramps there made full.
/// Re-pinned 2026-11-12 for tall grass's growth stages (WORK_QUEUE item 14, docs/ECOLOGY.md §10): the
/// state ids after `dwell:tall_grass` moved (its `stage` property), and its tufts generate at their
/// climate's top stage, so the starter's drier biomes grow shorter grass; a change of the starter's
/// terrain, so it waits on the owner's next line (0.55) with item 32's.
/// Re-pinned for both together (the 0.55 line).
const STARTER_PIN: [(Face, &str); 2] =
    [(Face::A, "2b6acef641e7c3c1"), (Face::B, "6bda0e9d7104d279")];

#[test]
fn the_starter_worlds_terrain_is_unchanged() {
    // both-faces: pins each face's terrain.
    starter_maps();
    let terrain = BifacialTerrain::for_generator(GENERATOR_TERRAIN, SEED).expect("the terrain");
    let mut got = Vec::new();
    for (face, _) in STARTER_PIN {
        let g = terrain.local(face);
        let f = dwell_world::FaceFrame::of(face);
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let r = WORLD_RADIUS * 3 / 4;
        let mut k = 0u32;
        let (mut eroded, mut chunks) = (0u32, 0u32);
        let mut chunk = Chunk::default();
        for z in (-r..=r).step_by(128) {
            for x in (-r..=r).step_by(128) {
                let (xi, zi) = (i64::from(x), i64::from(z));
                // The geography's own fields, as the pin has always hashed them (R4's `celsius` and
                // `rain` follow from them in a world without a climate, as the starter is: its °C by
                // vegetation.json, no rain).
                let geo = match g.biome_geography(xi, zi) {
                    Some(g) => {
                        assert!(g.rain.is_none(), "the starter has no climate");
                        format!(
                            "Some(Geography {{ temperature: {:?}, wetness: {:?}, elevation: {:?}, coast: {:?} }})",
                            g.temperature, g.wetness, g.elevation, g.coast
                        )
                    }
                    None => "None".to_string(),
                };
                fnv(&mut h, geo.as_bytes());
                fnv(&mut h, format!("{:?}", g.biome_rainfall(xi, zi)).as_bytes());
                let e = g.erosion_at(xi, zi);
                eroded += u32::from(e.is_some());
                fnv(&mut h, format!("{e:?}").as_bytes());
                let col = g.column_at(x, z);
                fnv(&mut h, format!("{col:?}").as_bytes());
                k += 1;
                if k.is_multiple_of(512)
                    && let Some(y) = g.ground_y(x, z)
                {
                    let c = chunk_of(x, f.row(y), z);
                    terrain.generate(c, &mut chunk);
                    chunks += 1;
                    fnv(&mut h, &chunk_hash(&chunk).to_le_bytes());
                }
            }
        }
        let mut cells = LodCells::new();
        for (level, step, rows) in [(6, 3, 3), (8, 1, 2)] {
            let half = (sections_across(level) / 2) as i32;
            let n = (i64::from(r) / section_size(level)) as i32;
            for k in (-n..n).step_by(step) {
                for i in (-n..n).step_by(step) {
                    for d in 0..rows {
                        // The section rows from the midplane outward, on the face's side.
                        let j = if face == Face::A {
                            half + d
                        } else {
                            half - 1 - d
                        };
                        let c = LodCoord::new(level, half + i, j, half + k);
                        let kind = terrain.generate_lod(c, &mut cells, None, None);
                        fnv(&mut h, &[kind as u8]);
                        fnv(&mut h, &lod_hash(kind, &cells).to_le_bytes());
                    }
                }
            }
        }
        // The pin reads the erosion's grids where a biome erodes, and generates chunks.
        assert!(
            eroded > 1000 && chunks > 100,
            "{face:?}: {eroded} eroded columns, {chunks} chunks"
        );
        got.push((face, format!("{h:016x}")));
    }
    if std::env::var("STARTER_PIN").is_ok() {
        println!("{got:?}");
    }
    for ((face, want), (_, h)) in STARTER_PIN.iter().zip(&got) {
        assert_eq!(h, want, "the starter world's face {face:?} terrain changed");
    }
}
