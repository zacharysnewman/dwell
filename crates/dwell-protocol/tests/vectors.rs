//! The golden vectors (`shared/protocol/vectors.txt`, written by an independent Python encoder and
//! also checked by the C++ and TS codecs): every message decodes and re-encodes to the same bytes,
//! and every `!` vector is rejected.

use dwell_protocol::*;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn vectors() -> Vec<(&'static str, Vec<u8>)> {
    include_str!("../../../shared/protocol/vectors.txt")
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (name, bytes) = l.split_once(' ').expect("`name hex`");
            (name, hex(bytes))
        })
        .collect()
}

#[test]
fn messages_round_trip_byte_for_byte() {
    let mut checked = 0;
    for (name, bytes) in vectors() {
        if name.starts_with('!') || name == "auth_transcript" || name == "registry_payload" {
            continue;
        }
        let m = Message::decode(&bytes).unwrap_or_else(|| panic!("{name}: does not decode"));
        assert_eq!(m.encode(), bytes, "{name}: re-encodes differently");
        checked += 1;
    }
    assert!(checked >= 40, "only {checked} vectors");
}

#[test]
fn malformed_messages_are_rejected() {
    let mut checked = 0;
    for (name, bytes) in vectors() {
        if let Some(rest) = name.strip_prefix('!') {
            assert_eq!(Message::decode(&bytes), None, "{rest}: decoded");
            checked += 1;
        }
    }
    assert!(checked >= 20, "only {checked} negative vectors");
}

#[test]
fn auth_transcript_matches() {
    let (_, want) = vectors()
        .into_iter()
        .find(|(n, _)| *n == "auth_transcript")
        .expect("vector");
    let nonce: [u8; 32] = std::array::from_fn(|i| 0xa0 + i as u8);
    let binding: [u8; 32] = std::array::from_fn(|i| 0x10 + i as u8);
    let key: [u8; 32] = std::array::from_fn(|i| i as u8);
    assert_eq!(auth_transcript(&nonce, &binding, &key), want);
}

#[test]
fn decoded_fields_are_the_vectors_values() {
    let v = vectors();
    let get = |n: &str| v.iter().find(|(name, _)| *name == n).expect(n).1.clone();
    let Some(Message::Welcome(w)) = Message::decode(&get("welcome")) else {
        panic!("welcome")
    };
    assert_eq!(w.player_id, 42);
    assert_eq!(w.world_seed, 0x0123_4567_89ab_cdef);
    // A 1 m piece placed down from its cell (docs/ITEMS.md).
    let Some(Message::BlockEditRequest(e)) = Message::decode(&get("block_edit_place_piece")) else {
        panic!("block_edit_place_piece")
    };
    assert_eq!(
        (e.cell, e.face, e.material, e.rows, e.down),
        ([5, -3, 9], 0, 16, 2, true)
    );
    let Some(Message::BlockEditRequest(e)) = Message::decode(&get("block_edit_place")) else {
        panic!("block_edit_place")
    };
    assert_eq!((e.rows, e.down), (1, false));
    let Some(Message::PhysicsSnapshot(s)) = Message::decode(&get("physics_snapshot")) else {
        panic!("physics_snapshot")
    };
    let c = s.local.controller;
    assert_eq!(
        (c.step_grace, c.slide_flags, c.slide_cooldown),
        (2, slide_flags::ACTIVE, 29)
    );
    assert_eq!(c.ladder, [-5, 64, 1_000_000]);
    // v17: what each remote holds and its last swing.
    let r = s.remotes[0];
    assert_eq!(
        (
            r.held & held_item::BLOCK_MASK,
            r.held & held_item::BRUSH != 0
        ),
        (300, true)
    );
    assert_eq!(
        (
            r.swing & swing_flags::COUNT_MASK,
            r.swing & swing_flags::PLACE != 0
        ),
        (5, true)
    );
    assert_eq!((s.remotes[1].held, s.remotes[1].swing), (0, 0));
    // v20: a falling piece's pose (docs/FELLING.md).
    let p = s.pieces[0];
    assert_eq!((p.id, p.position), (7, [10.5, 40.25, -3.5]));
    assert!(
        (p.rotation[3] - 23170.0 / 32767.0).abs() < 1e-6 && (p.velocity[1] + 9.75).abs() < 1e-3
    );
    let Some(Message::EntitySpawn(e)) = Message::decode(&get("entity_spawn")) else {
        panic!("entity_spawn")
    };
    assert_eq!((e.id, e.carriers, e.cells.len()), (7, 2, 3));
    assert_eq!((e.cells[2].offset, e.cells[2].state), ([-1, 1, 2], 21));
    assert_eq!(
        Message::decode(&get("entity_despawn")),
        Some(Message::EntityDespawn(7))
    );
    assert_eq!(
        Message::decode(&get("held_item_brush")),
        Some(Message::HeldItem(held_item::BRUSH | 4))
    );
    let Some(Message::ChunkData(c)) = Message::decode(&get("chunk_data_explicit_solid")) else {
        panic!("chunk")
    };
    assert_eq!(c.voxels.len(), CHUNK_VOLUME);
    assert!(c.voxels.iter().all(|&m| m == c.voxels[0]));
    let Some(Message::RegionEdit(r)) = Message::decode(&get("region_edit_paste")) else {
        panic!("region_edit_paste")
    };
    assert_eq!(r.op, RegionEditOp::Paste);
    assert_eq!(r.min, [-3, 70, 8_191_990]);
    assert_eq!(r.size, [2, 3, 2]);
    assert_eq!(r.cells, [1, 1, 1, 1, 4, 0, 4, 0, 0, 0, 0, 0]);
    // Layer order: x fastest, then z, then y.
    assert_eq!(r.cells[RegionEdit::cell_index(r.size, 0, 1, 1)], 4);
    let Some(Message::RegionEdit(r)) = Message::decode(&get("region_edit_paste_keep")) else {
        panic!("region_edit_paste_keep")
    };
    let keep = region_edit::KEEP as u16;
    assert_eq!(
        (r.size, r.cells.as_slice()),
        ([3, 1, 1], [keep, 2, keep].as_slice())
    );
    let Some(Message::RegionEdit(r)) = Message::decode(&get("region_edit_replace")) else {
        panic!("region_edit_replace")
    };
    assert_eq!(
        (r.op, r.from, r.material, r.size),
        (RegionEditOp::Replace, 2, 6, [1, 32, 2])
    );
    let payload = get("registry_payload");
    let Some(Message::BlockRegistry(b)) = Message::decode(&get("block_registry")) else {
        panic!("block_registry")
    };
    assert_eq!((b.part, b.parts), (0, 1));
    assert_eq!(b.data, payload);
    let Some(Message::BlockRegistry(b)) = Message::decode(&get("block_registry_part")) else {
        panic!("block_registry_part")
    };
    assert_eq!((b.part, b.parts), (1, 2));
    assert_eq!(b.data, payload[40..]);
}

#[test]
fn region_edits_of_the_largest_box_round_trip() {
    let side = region_edit::MAX_SIDE as u8;
    let mut m = RegionEdit {
        op: RegionEditOp::Paste,
        min: [i32::MIN, 0, i32::MAX - 31],
        size: [side; 3],
        material: 0,
        from: 0,
        cells: Vec::new(),
    };
    m.cells = (0..m.volume()).map(|i| (i % 7 + i / 5000) as u16).collect();
    let bytes = Message::RegionEdit(m.clone()).encode();
    assert!(bytes.len() <= MAX_RELIABLE_MESSAGE_BYTES);
    assert_eq!(Message::decode(&bytes), Some(Message::RegionEdit(m)));
}

#[test]
fn chunk_codec_round_trips() {
    let voxels: Vec<u16> = (0..CHUNK_VOLUME).map(|i| ((i / 37) % 300) as u16).collect();
    let bytes = encode_chunk_voxels(&voxels);
    assert_eq!(decode_chunk_voxels(&bytes), Some(voxels));
    let cells: Vec<u16> = (0..LOD_CELL_COUNT).map(|i| (i % 3) as u16).collect();
    assert_eq!(decode_lod_cells(&encode_lod_cells(&cells)), Some(cells));
}

#[test]
fn fixed_positions_round_and_clamp() {
    assert_eq!(to_fixed_position(1.0), 256);
    assert_eq!(to_fixed_position(-0.001), 0);
    assert_eq!(to_fixed_position(f64::NAN), i32::MIN);
    assert_eq!(to_fixed_position(1e12), i32::MAX);
    assert_eq!(from_fixed_position(512), 2.0);
}
