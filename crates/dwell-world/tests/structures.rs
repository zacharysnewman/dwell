//! Structures (ARCHITECTURE.md §6.3, docs/STRUCTURES.md §3): the generated tables load, the
//! fixtures in `shared/structures/test/` stamp by data alone, variants are deterministic and turn
//! facing blocks with the geometry, and the converted movement playground is the old one exactly.

use std::collections::BTreeSet;
use std::sync::Arc;

use dwell_blocks::{StateId, info, require, state_property};
use dwell_world::structure::*;
use dwell_world::*;

mod fixtures {
    use dwell_world::structure::{Axis, Body, Choice, StructureDef, Variant};
    include!("fixtures/structures.gen.rs");
}
use fixtures::TEST_STRUCTURES;

fn s(name: &str) -> StateId {
    require(name)
}

fn fixture(name: &str) -> Structure {
    let def = TEST_STRUCTURES
        .iter()
        .find(|d| d.name == name)
        .expect("fixture");
    Structure::from_def(def).expect("loads")
}

/// A base generator that fills every chunk with sand, so kept cells show as sand.
fn sand_generator() -> ChunkGenerator {
    let sand = s("dwell:sand");
    Arc::new(move |_, chunk| chunk.voxels_mut().fill(sand))
}

const ALL_ORIENTATIONS: [Orientation; 8] = {
    let mut out = [Orientation {
        quarter_turns: 0,
        mirror: None,
    }; 8];
    let mut i = 0;
    while i < 8 {
        out[i] = Orientation {
            quarter_turns: (i % 4) as u8,
            mirror: if i < 4 { None } else { Some(Axis::X) },
        };
        i += 1;
    }
    out
};

#[test]
fn every_build_structure_loads_and_matches_its_hash() {
    check_all().expect("the build's structures");
    for def in STRUCTURES.iter().chain(TEST_STRUCTURES.iter()) {
        assert_eq!(
            structure_hash(&canonical_text(def)),
            def.hash,
            "{}: the Rust canonical text must hash as gen.mjs's does",
            def.name
        );
    }
    assert!(
        load("dwell:nope")
            .expect_err("unknown")
            .contains("dwell:nope")
    );
}

#[test]
fn a_fixture_stamps_by_data_alone() {
    let post = fixture("test:ladder_post");
    assert_eq!((post.size, post.anchor), ([3, 2, 2], [0, 0, 0]));
    let placed = Arc::new(post.place_oriented([10, 5, 20], Orientation::default(), 0));
    assert_eq!((placed.min, placed.max), ([10, 5, 20], [12, 6, 21]));
    let mut world = VoxelWorld::new(with_structure(sand_generator(), placed.clone()), None);
    let at = |w: &mut VoxelWorld, x, y, z| w.get_voxel(10 + x, 5 + y, 20 + z);
    assert_eq!(at(&mut world, 0, 0, 0), s("dwell:stone"));
    assert_eq!(at(&mut world, 1, 0, 0), s("dwell:ladder[facing=north]"));
    assert_eq!(
        at(&mut world, 2, 0, 0),
        s("dwell:sand"),
        "null keeps the base"
    );
    assert_eq!(
        at(&mut world, 2, 0, 1),
        s("dwell:air"),
        "explicit air carves"
    );
    assert_eq!(
        at(&mut world, 1, 1, 1),
        placed.at([11, 6, 21]).expect("a choice")
    );
    assert_eq!(at(&mut world, 3, 0, 0), s("dwell:sand"), "outside the box");

    let air = with_structure_air(Arc::new(|_| true), placed);
    assert!(!air(ChunkCoord::new(0, 0, 0)));
    assert!(air(ChunkCoord::new(1, 0, 0)));
}

/// Unit vector of a compass facing (north = −Z, east = +X).
fn facing_dir(state: StateId) -> [i32; 3] {
    match state_property(state, "facing").expect("a facing block") {
        "north" => [0, 0, -1],
        "east" => [1, 0, 0],
        "south" => [0, 0, 1],
        _ => [-1, 0, 0],
    }
}

#[test]
fn rotation_and_mirroring_turn_the_geometry_and_the_ladder_together() {
    let post = fixture("test:ladder_post");
    let ladder = |p: &Placed| {
        let mut found = None;
        for y in p.min[1]..=p.max[1] {
            for z in p.min[2]..=p.max[2] {
                for x in p.min[0]..=p.max[0] {
                    if let Some(st) = p.at([x, y, z]).filter(|&st| info(st).climbable) {
                        found = Some(([x, y, z], st));
                    }
                }
            }
        }
        found.expect("the ladder")
    };
    // Exact cells: a quarter turn clockwise takes +X to +Z and north to east.
    let turned = post.place_oriented([0, 0, 0], ALL_ORIENTATIONS[1], 0);
    assert_eq!(ladder(&turned), ([0, 0, 1], s("dwell:ladder[facing=east]")));
    assert_eq!(turned.at([-1, 0, 1]), Some(s("dwell:stone")));
    let half = post.place_oriented([0, 0, 0], ALL_ORIENTATIONS[2], 0);
    assert_eq!(ladder(&half), ([-1, 0, 0], s("dwell:ladder[facing=south]")));
    let mirrored = post.place_oriented(
        [0, 0, 0],
        Orientation {
            quarter_turns: 0,
            mirror: Some(Axis::X),
        },
        0,
    );
    assert_eq!(
        ladder(&mirrored),
        ([-1, 0, 0], s("dwell:ladder[facing=north]"))
    );
    let mirrored_z = post.place_oriented(
        [0, 0, 0],
        Orientation {
            quarter_turns: 0,
            mirror: Some(Axis::Z),
        },
        0,
    );
    assert_eq!(
        ladder(&mirrored_z),
        ([1, 0, 0], s("dwell:ladder[facing=south]"))
    );
    // Every orientation: the ladder still backs onto its wall (the cell behind its facing), the
    // anchor stays put, and the box holds the same cells.
    for o in ALL_ORIENTATIONS {
        let p = post.place_oriented([0, 0, 0], o, 0);
        let (cell, st) = ladder(&p);
        let d = facing_dir(st);
        let wall = [cell[0] - d[0], cell[1], cell[2] - d[2]];
        assert_eq!(p.at(wall), Some(s("dwell:stone")), "{o:?}");
        assert_eq!(p.at([0, 0, 0]), Some(s("dwell:stone")), "{o:?}");
        let vol: i32 = (0..3).map(|i| p.max[i] - p.min[i] + 1).product();
        assert_eq!(vol, 12, "{o:?}");
    }
}

#[test]
fn variants_are_deterministic_from_the_seed() {
    let post = fixture("test:ladder_post");
    assert_eq!(post.place([3, 4, 5], 42), post.place([3, 4, 5], 42));
    let mut orientations = BTreeSet::new();
    let (mut dirt, mut sand) = (0, 0);
    for seed in 0..400u64 {
        let o = post.orientation(seed);
        orientations.insert((o.quarter_turns, o.mirror == Some(Axis::X)));
        assert_ne!(o.mirror, Some(Axis::Z), "only the listed axes");
        let p = post.place_oriented([0, 0, 0], Orientation::default(), seed);
        match p.at([1, 1, 1]) {
            Some(x) if x == s("dwell:dirt") => dirt += 1,
            Some(x) if x == s("dwell:sand") => sand += 1,
            other => panic!("not a listed choice: {other:?}"),
        }
    }
    assert_eq!(
        orientations.len(),
        8,
        "every rotation and mirror is reachable"
    );
    // Weights 1 : 3.
    assert!(
        (60..140).contains(&dirt) && (260..340).contains(&sand),
        "{dirt} : {sand}"
    );
    // The orientation does not change which block a cell draws.
    for seed in 0..16 {
        let a = post.place_oriented([0, 0, 0], ALL_ORIENTATIONS[0], seed);
        let b = post.place_oriented([0, 0, 0], ALL_ORIENTATIONS[1], seed);
        assert_eq!(a.at([1, 1, 1]), b.at([-1, 1, 1]));
    }
}

#[test]
fn a_stale_or_bad_table_entry_fails_naming_the_structure() {
    let good = TEST_STRUCTURES[0];
    let stale = StructureDef {
        hash: good.hash ^ 1,
        ..good
    };
    let e = Structure::from_def(&stale).expect_err("stale");
    assert!(e.contains("test:ladder_post") && e.contains("hash"), "{e}");

    let Body::Voxels { size, anchor, .. } = good.body;
    let mut bad = StructureDef {
        name: "test:bad",
        hash: 0,
        body: Body::Voxels {
            size,
            anchor,
            palette: &[('s', Some("dwell:bedrock"))],
            cells: "ssssssssssss",
        },
        variants: &[],
    };
    bad.hash = structure_hash(&canonical_text(&bad));
    let e = Structure::from_def(&bad).expect_err("unknown block");
    assert!(e.contains("test:bad") && e.contains("dwell:bedrock"), "{e}");
}

/// The old `playground.structure` (`fill x0 y0 z0 x1 y1 z1 <state>`, inclusive boxes, later ones
/// win), transcribed: the reference the converted file was proven against (red on a one-cell
/// change) before the text parser was removed.
const OLD_PLAYGROUND: [([i32; 3], [i32; 3], &str); 7] = [
    ([-6, 0, 6], [-4, 0, 8], "dwell:stone"),
    ([-2, 0, 12], [2, 3, 12], "dwell:stone"),
    ([0, 0, 12], [0, 1, 12], "dwell:air"),
    ([4, 1, 6], [6, 1, 9], "dwell:stone"),
    ([8, 0, 10], [10, 3, 13], "dwell:stone"),
    ([9, 0, 9], [9, 3, 9], "dwell:ladder[facing=north]"),
    ([12, -2, 6], [15, -1, 9], "dwell:water"),
];

fn old_playground_at(p: [i32; 3]) -> Option<StateId> {
    OLD_PLAYGROUND
        .iter()
        .rev()
        .find(|(lo, hi, _)| (0..3).all(|i| p[i] >= lo[i] && p[i] <= hi[i]))
        .map(|(_, _, name)| s(name))
}

#[test]
fn the_converted_playground_stamps_exactly_as_the_old_fill_file() {
    let new = Arc::new(
        load("dwell:playground")
            .expect("the playground")
            .place([0, 0, 0], 0),
    );
    for y in -4..6 {
        for z in -2..16 {
            for x in -8..18 {
                assert_eq!(
                    old_playground_at([x, y, z]),
                    new.at([x, y, z]),
                    "({x}, {y}, {z})"
                );
            }
        }
    }
    let mut a = VoxelWorld::new(
        Arc::new(move |c, chunk| {
            flat_generator()(c, chunk);
            let v = chunk.voxels_mut();
            for lz in 0..CHUNK_SIZE {
                for ly in 0..CHUNK_SIZE {
                    for lx in 0..CHUNK_SIZE {
                        let p = [
                            c.x * CHUNK_SIZE + lx,
                            c.y * CHUNK_SIZE + ly,
                            c.z * CHUNK_SIZE + lz,
                        ];
                        if let Some(m) = old_playground_at(p) {
                            v[local_index(lx, ly, lz)] = m;
                        }
                    }
                }
            }
        }),
        None,
    );
    let mut b = VoxelWorld::new(with_structure(flat_generator(), new), None);
    for cy in -1..=0 {
        for cz in -1..=1 {
            for cx in -1..=1 {
                let c = ChunkCoord::new(cx, cy, cz);
                assert_eq!(
                    chunk_hash(a.get_or_create(c)),
                    chunk_hash(b.get_or_create(c)),
                    "chunk {c:?}"
                );
            }
        }
    }
}
