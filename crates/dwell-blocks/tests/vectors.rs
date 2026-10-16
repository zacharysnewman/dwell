//! The shared vector (`shared/blocks/vectors.txt`, also read by the C++ and TS tests) pins the ids,
//! canonical strings and hash.

use dwell_blocks as blocks;

#[test]
fn ids_strings_and_hash_match_the_shared_vector() {
    let text = include_str!("../../../shared/blocks/vectors.txt");
    let mut count = 0;
    for line in text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let (key, value) = line.split_once(' ').expect("`key value` lines");
        if key == "hash" {
            assert_eq!(format!("{:016x}", blocks::REGISTRY_HASH), value);
            continue;
        }
        let id: blocks::StateId = key.parse().expect("numeric id");
        assert_eq!(blocks::info(id).name, value);
        assert_eq!(blocks::state(value), Some(id));
        count += 1;
    }
    assert_eq!(count, blocks::STATES.len());
    assert_eq!(blocks::compute_hash(), blocks::REGISTRY_HASH);
}

#[test]
fn names_resolve_and_air_is_zero() {
    assert_eq!(blocks::require("dwell:air"), blocks::AIR);
    assert_eq!(blocks::state("dwell:nonexistent"), None);
    // A block id alone resolves to its default (first) state.
    let ladder = blocks::require("dwell:ladder");
    assert_eq!(blocks::block_of(ladder).first_state, ladder);
    assert!(blocks::info(blocks::require("dwell:water")).liquid);
}

#[test]
fn states_reference_valid_rows() {
    for s in &blocks::STATES {
        assert!(s.block < blocks::BLOCKS.len());
        let shape = &blocks::SHAPES[usize::from(s.shape_index)];
        assert!(usize::from(shape.first_face + shape.face_count) <= blocks::SHAPE_FACES.len());
    }
}

/// The registry payload's golden vector (`shared/protocol/vectors.txt` `registry_payload`, written
/// by the independent Python encoder): it decodes, re-encodes byte for byte, and maps.
#[test]
fn the_registry_payload_vector_decodes_and_maps() {
    use blocks::sync::{Payload, definitions_hash, map};
    let text = include_str!("../../../shared/protocol/vectors.txt");
    let hex = text
        .lines()
        .find_map(|l| l.strip_prefix("registry_payload "))
        .expect("registry_payload vector");
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex"))
        .collect();
    let p = Payload::decode(&bytes).expect("decodes");
    assert_eq!(p.encode(), bytes);
    let names: Vec<String> = p.states().map(|(_, n, _)| n).collect();
    assert_eq!(
        names,
        ["dwell:air", "test:lamp[lit=false]", "test:lamp[lit=true]"]
    );
    let m = map(&p, definitions_hash(&bytes)).expect("maps");
    assert_eq!(m.client(0), Some(blocks::AIR));
    let lamp = blocks::info(m.client(2).expect("lamp"));
    assert_eq!(lamp.name, "test:lamp[lit=true]");
    assert_eq!(lamp.density_kg_m3, 1200.0);
    assert_eq!(lamp.facing, blocks::Facing::East);
    assert!(lamp.solid && lamp.placeable && !lamp.liquid);
    assert_eq!(lamp.color, 0xffcc00);
    // Its shape (bytes 01 02 03) is not this build's: a full cube, and the prompt says so.
    assert_eq!(lamp.shape, blocks::VoxelShape::Full);
    assert!(m.differences.added[1].fields[0].contains("full cube"));
    assert!(m.differences.client_only.iter().any(|n| n == "dwell:stone"));
}
