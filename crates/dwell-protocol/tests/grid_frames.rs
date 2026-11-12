//! The `grid.frames` wire (docs/proposals/MOON_WORLD.md §7.2): the snapshot's grid tail and the
//! chunks' grid byte, each absent (the bytes an older build reads) unless sent.

use dwell_protocol::*;

fn snapshot(grid: Option<GridView>, swing_kinds: bool) -> PhysicsSnapshot {
    PhysicsSnapshot {
        server_tick: 9,
        ack_input_seq: 4,
        local: LocalPlayerState {
            position: [1.5, 84.9, -16.5],
            ..LocalPlayerState::default()
        },
        remotes: vec![RemotePlayerState {
            player_id: 2,
            position: [0.0; 3],
            velocity: [0.0; 3],
            yaw: 0,
            pitch: 0,
            state: PlayerState::Idle,
            flags: 0,
            held: 0,
            swing: 0,
            swing_kind: if swing_kinds { 1 } else { 0 },
        }],
        pieces: Vec::new(),
        swing_kinds,
        grid,
    }
}

#[test]
fn a_snapshot_carries_the_players_grid_only_when_sent() {
    let moon = GridView {
        grid: 1,
        origin: [-12_000.5, 596_000.25, 3.0],
        rotation: [0.1, 0.2, 0.3, 0.927_361_849_549_570_4],
    };
    let home = GridView::default();
    for s in [
        snapshot(None, false),
        snapshot(None, true),
        snapshot(Some(home), false),
        snapshot(Some(home), true),
        snapshot(Some(moon), false),
        snapshot(Some(moon), true),
    ] {
        let m = Message::PhysicsSnapshot(s.clone());
        let mut back = s.clone();
        // A grid sent without swing kinds still writes them (empty kinds read as Plain).
        if s.grid.is_some() {
            back.swing_kinds = true;
        }
        assert_eq!(
            Message::decode(&m.encode()),
            Some(Message::PhysicsSnapshot(back))
        );
    }
    // Without a grid, the bytes are an older build's.
    let old = Message::PhysicsSnapshot(snapshot(None, false)).encode();
    let home_bytes = Message::PhysicsSnapshot(snapshot(Some(home), false)).encode();
    let moon_bytes = Message::PhysicsSnapshot(snapshot(Some(moon), false)).encode();
    assert_eq!(
        home_bytes.len(),
        old.len() + 3,
        "the kinds (a count, one remote's) and the grid byte"
    );
    assert_eq!(
        moon_bytes.len(),
        home_bytes.len() + 7 * 8,
        "the frame off home"
    );
}

#[test]
fn a_chunk_names_its_grid_only_off_home() {
    for grid in [0u8, 1, 7] {
        for form in [ChunkForm::Generated, ChunkForm::Air, ChunkForm::Explicit] {
            let c = ChunkData {
                form,
                coord: [3, -1, 7],
                revision: u32::from(form == ChunkForm::Explicit),
                voxels: if form == ChunkForm::Explicit {
                    vec![0; CHUNK_VOLUME]
                } else {
                    Vec::new()
                },
                grid,
            };
            let m = Message::ChunkData(c.clone());
            assert_eq!(
                Message::decode(&m.encode()),
                Some(m.clone()),
                "{grid} {form:?}"
            );
            let home = Message::ChunkData(ChunkData { grid: 0, ..c }).encode();
            assert_eq!(m.encode().len(), home.len() + usize::from(grid != 0));
        }
        let u = Message::ChunkUnload(ChunkUnload {
            coords: vec![[1, 2, 3], [4, 5, 6]],
            grid,
        });
        assert_eq!(Message::decode(&u.encode()), Some(u));
    }
}
