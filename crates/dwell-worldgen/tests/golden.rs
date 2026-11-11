//! The worldgen goldens: the Rust generator reproduces, hash for hash, the chunks and LOD sections
//! recorded in shared/worldgen/golden (by the C++ generator up to version 12, by the Rust generator
//! since; on the half-height branch, generator version 1020: ½ m cells, stored ramps, main's
//! steep hills, the sea flooding the open part of ramps below sea level, main's pines two cells
//! per block, and V2's slopes: no whole-metre step snap, the old ramp rule). The same test runs under wasm32 in CI, so native and WASM
//! agree too (ADR 0010).

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

/// The goldens hold *by name* across a registry whose state ids moved (a block gained states, ADR
/// 0063): each chunk's voxels mapped to the old registry's ids (by their names) hash to the old
/// golden. `OLD_VECTORS=<old shared/blocks/vectors.txt> OLD_GOLDEN=<old chunk-hashes.txt> cargo test
/// -p dwell-worldgen --test golden -- --ignored`.
#[test]
#[ignore = "needs the old registry's vectors and goldens"]
fn chunks_hold_by_name_against_an_older_registry() {
    let read = |var: &str| std::fs::read_to_string(std::env::var(var).expect(var)).expect(var);
    let ids = |text: &str| -> std::collections::HashMap<String, u16> {
        text.lines()
            .filter(|l| !l.starts_with('#') && !l.starts_with("hash"))
            .filter_map(|l| l.split_once(' '))
            .map(|(id, name)| (name.to_owned(), id.parse().expect("id")))
            .collect()
    };
    let old = ids(&read("OLD_VECTORS"));
    let new = ids(include_str!("../../../shared/blocks/vectors.txt"));
    let mut to_old = vec![u16::MAX; new.values().copied().max().unwrap_or(0) as usize + 1];
    for (name, id) in &new {
        // A property the old registry lacked, at its default, names the old state.
        if let Some(o) = old
            .get(name)
            .or_else(|| old.get(&name.replace(",width=4", "")))
        {
            to_old[*id as usize] = *o;
        }
    }
    let (_, rows) = rows(&read("OLD_GOLDEN"));
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
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &m in chunk.voxels().iter() {
            let o = to_old[usize::from(m)];
            assert_ne!(o, u16::MAX, "state {m} has no name in the old registry");
            for b in o.to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        if format!("{h:x}") != r[4] {
            failures.push(format!("seed {seed} {c:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
