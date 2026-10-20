//! Rewrites the worldgen goldens (`shared/worldgen/golden/`) from this generator: `cargo run -p
//! dwell-worldgen --example goldens`. Only when the terrain a seed generates changes on purpose (a
//! new generator version and compatibility line, RELEASES.md); `tests/golden.rs` then holds every
//! build, native and WASM, to them. The rows: the surface, coast, sea and core rows on both faces,
//! the rim, and the build sphere's edge.

use std::fmt::Write as _;

use dwell_world::lod::{LodCells, LodCoord, lod_hash, section_size};
use dwell_world::{Chunk, ChunkCoord, chunk_hash};
use dwell_worldgen::BifacialTerrain;

const VERSION: u32 = 15;
const SEEDS: [u64; 2] = [0, 20_260_925];

fn section_at(level: i32, p: [i64; 3]) -> LodCoord {
    let s = section_size(level);
    let at = |a: usize, o: i64| (p[a] - o).div_euclid(s) as i32;
    LodCoord::new(
        level,
        at(0, dwell_world::lod::ORIGIN_X),
        at(1, dwell_world::lod::ORIGIN_Y),
        at(2, dwell_world::lod::ORIGIN_Z),
    )
}

fn main() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../shared/worldgen/golden");
    let registry = format!("{:x}", dwell_blocks::REGISTRY_HASH);

    let mut chunks: Vec<(u64, [i32; 3])> = Vec::new();
    for seed in SEEDS {
        // Origin: the core and ground, the sea surface or land up to the hilltops (version 15), air
        // above; face B's mirror rows.
        for y in [-5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 6] {
            chunks.push((seed, [0, y, 0]));
            chunks.push((seed, [5, y, -3]));
        }
        // Inland and across the coast (the island's coast lies 16–32 km out), both faces.
        for (x, z) in [(-14, -36), (40, -12), (-300, 450)] {
            chunks.push((seed, [x, 1, z]));
            chunks.push((seed, [x, -2, z]));
        }
        for x in (600..=1000).step_by(50) {
            chunks.push((seed, [x, 0, 0]));
            chunks.push((seed, [x, 1, 0]));
            chunks.push((seed, [x, -1, 0]));
            chunks.push((seed, [0, 0, -x]));
        }
        // The rim (50 km) on the axis and the diagonal, both faces; beyond it.
        for c in [
            [1561, 0, 0],
            [1562, 0, 0],
            [1562, -1, 0],
            [1562, 1, 0],
            [1104, 0, 1104],
            [1104, -1, 1104],
            [1105, 0, 1105],
            [-1563, 0, 0],
        ] {
            chunks.push((seed, c));
        }
        // The build sphere's edge: above the centre and over the rim (all air, or nothing).
        for c in [
            [0, 1562, 0],
            [0, 1563, 0],
            [0, -1563, 0],
            [0, -1564, 0],
            [1560, 88, 0],
            [1560, -89, 0],
        ] {
            chunks.push((seed, c));
        }
    }
    let mut out = format!(
        "# seed chunk_x chunk_y chunk_z fnv1a64(voxels) - generator version {VERSION}\n# registry {registry}\n"
    );
    for (seed, c) in &chunks {
        let mut chunk = Chunk::default();
        BifacialTerrain::new(*seed).generate(ChunkCoord::from_array(*c), &mut chunk);
        writeln!(
            out,
            "{seed} {} {} {} {:x}",
            c[0],
            c[1],
            c[2],
            chunk_hash(&chunk)
        )
        .expect("write");
    }
    std::fs::write(format!("{root}/chunk-hashes.txt"), out).expect("write chunk-hashes.txt");

    let mut sections: Vec<(u64, LodCoord)> = Vec::new();
    for seed in SEEDS {
        // Every level at the origin, the section just above the midplane and its mirror below.
        for level in 1..=dwell_world::lod::MAX_LEVEL {
            sections.push((seed, section_at(level, [0, 0, 0])));
            sections.push((seed, section_at(level, [0, -1, 0])));
        }
        // Inland, the coast and the open sea, both faces.
        for p in [
            [14_000, 40, 3_000],
            [-9_000, 40, 21_000],
            [25_000, 40, 0],
            [0, 40, -27_000],
            [38_000, 40, 12_000],
            [14_000, -41, 3_000],
            [25_000, -41, 0],
            [38_000, -41, 12_000],
        ] {
            for level in [1, 4, 8] {
                sections.push((seed, section_at(level, p)));
            }
        }
        // The rim and the sphere's edge over it.
        for p in [
            [49_900, 0, 0],
            [49_900, -1, 0],
            [35_300, 0, 35_300],
            [49_900, 2_000, 0],
        ] {
            for level in [1, 4, 8, 10] {
                sections.push((seed, section_at(level, p)));
            }
        }
    }
    let mut out = format!(
        "# seed level i j k kind fnv1a64(kind, cells) - generator version {VERSION}\n# registry {registry}\n"
    );
    let mut cells = LodCells::new();
    for (seed, c) in &sections {
        let kind = BifacialTerrain::new(*seed).generate_lod(*c, &mut cells, None, None);
        writeln!(
            out,
            "{seed} {} {} {} {} {} {:x}",
            c.level,
            c.i,
            c.j,
            c.k,
            kind as u8,
            lod_hash(kind, &cells)
        )
        .expect("write");
    }
    std::fs::write(format!("{root}/lod-hashes.txt"), out).expect("write lod-hashes.txt");
    println!("{} chunks, {} sections", chunks.len(), sections.len());
}
