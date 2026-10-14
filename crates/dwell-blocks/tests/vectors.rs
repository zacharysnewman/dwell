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
