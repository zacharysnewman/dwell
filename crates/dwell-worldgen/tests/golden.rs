//! The worldgen goldens: the Rust generator reproduces, hash for hash, the chunks and LOD sections
//! recorded in shared/worldgen/golden (by the C++ generator up to version 12, by the Rust generator
//! since; now generator version 18, the pines of shared/worldgen/placements.json over version 17's flooded shores). The same test runs under wasm32 in CI, so
//! native and WASM agree too (ADR 0010).

use dwell_world::lod::{LodCells, LodCoord, lod_hash};
use dwell_world::{Chunk, ChunkCoord, chunk_hash};
use dwell_worldgen::BifacialTerrain;

fn rows(text: &str) -> (String, Vec<Vec<String>>) {
    let mut registry = String::new();
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(r) = line.strip_prefix("# registry ") {
            registry = r.trim().to_owned();
        } else if !line.starts_with('#') && !line.trim().is_empty() {
            out.push(line.split_whitespace().map(str::to_owned).collect());
        }
    }
    (registry, out)
}

fn check_registry(golden: &str) {
    // The golden was written with this block registry: state ids must mean the same blocks.
    assert_eq!(
        u64::from_str_radix(golden, 16).expect("hex"),
        dwell_blocks::REGISTRY_HASH
    );
}

#[test]
fn chunks_match_the_golden_hashes() {
    let (registry, rows) = rows(include_str!(
        "../../../shared/worldgen/golden/chunk-hashes.txt"
    ));
    check_registry(&registry);
    assert!(rows.len() >= 30);
    let mut failures = Vec::new();
    for r in &rows {
        let seed: u64 = r[0].parse().expect("seed");
        let c = ChunkCoord::new(
            r[1].parse().expect("x"),
            r[2].parse().expect("y"),
            r[3].parse().expect("z"),
        );
        let mut chunk = Chunk::default();
        BifacialTerrain::new(seed).generate(c, &mut chunk);
        let got = format!("{:x}", chunk_hash(&chunk));
        if got != r[4] {
            failures.push(format!("seed {seed} {c:?}: {got}, golden {}", r[4]));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn lod_sections_match_the_golden_hashes() {
    let (registry, rows) = rows(include_str!(
        "../../../shared/worldgen/golden/lod-hashes.txt"
    ));
    check_registry(&registry);
    assert!(rows.len() >= 10);
    let mut failures = Vec::new();
    let mut cells = LodCells::new();
    for r in &rows {
        let seed: u64 = r[0].parse().expect("seed");
        let p: Vec<i32> = r[1..5].iter().map(|v| v.parse().expect("int")).collect();
        let c = LodCoord::new(p[0], p[1], p[2], p[3]);
        let kind = BifacialTerrain::new(seed).generate_lod(c, &mut cells, None, None);
        let got = format!("{} {:x}", kind as u8, lod_hash(kind, &cells));
        let want = format!("{} {}", r[5], r[6]);
        if got != want {
            failures.push(format!("seed {seed} {c:?}: {got}, golden {want}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
