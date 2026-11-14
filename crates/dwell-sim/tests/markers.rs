//! A player's map markers through the server (docs/MAP_MARKERS.md, WORK_QUEUE item 53): sent to
//! that player only, after both sides said `markers.private`; replaced by the client's `Markers`;
//! saved with the world under the player's device key and back at the next join; never sent to a
//! client without the capability, and a client without it can't set them. Face-local points kept as
//! they are on both faces.

mod common;

use common::*;
use dwell_protocol::markers::{Marker, Markers};
use dwell_protocol::*;
use dwell_sim::server::{ServerConfig, SessionId};
use dwell_storage::{WorldDb, WorldStore};
use dwell_world::{FLAT_TOP_Y, FaceFrame, both_faces, empty_generator, m_of_row};

const BASE: i32 = FLAT_TOP_Y + 40;

fn config(f: FaceFrame, db: &std::sync::Arc<WorldDb>) -> ServerConfig {
    ServerConfig {
        generator_override: Some(empty_generator()),
        spawn: Some(f.point([0.5, m_of_row(BASE), 0.5])),
        store: Some(WorldStore::new(db.clone(), false)),
        ..flat()
    }
}

fn step(fx: &mut Fixture, n: u32) {
    for _ in 0..n {
        fx.server.step();
    }
}

fn caps() -> Message {
    Message::Capabilities(capabilities::Capabilities::this_client())
}

/// The `Markers` messages sessions 1 and 2 were sent since the last call.
fn markers_sent(fx: &mut Fixture) -> [Vec<Markers>; 2] {
    let mut all = fx.take_all();
    let mut of = |id: SessionId| {
        all.remove(&id)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|m| match m {
                Message::Markers(m) => Some(m),
                _ => None,
            })
            .collect()
    };
    [of(1), of(2)]
}

/// A marker at a face-local point on `f`'s face.
fn pin(f: FaceFrame, id: u32, label: &str, at: [f32; 3]) -> Marker {
    Marker {
        id,
        icon: "flag".into(),
        rgb: [0x2f, 0x7d, 0xf6],
        label: label.into(),
        face: u8::from(f.is_b()),
        position: at,
        auto: String::new(),
    }
}

both_faces!(
    fn markers_are_private_saved_with_the_world_and_back_at_the_next_join(f: FaceFrame) {
        let db = WorldDb::in_memory().expect("db");
        let mine = Markers(vec![pin(f, 1, "Base", [12.5, 70.0, -3.0])]);
        {
            let mut fx = Fixture::new(config(f, &db));
            fx.join(1, &Client::new(1));
            fx.join(2, &Client::new(2));
            fx.send(1, caps());
            fx.send(2, caps());
            step(&mut fx, 3);
            // A world from before markers: each is sent an empty record.
            let none = vec![Markers::default()];
            assert_eq!(markers_sent(&mut fx), [none.clone(), none]);
            fx.send(1, Message::Markers(mine.clone()));
            step(&mut fx, 3);
            // Nobody is sent player 1's markers.
            assert_eq!(markers_sent(&mut fx), [vec![], vec![]]);
            fx.server.save_now();
            fx.server.flush_store();
        }
        let mut fx = Fixture::new(config(f, &db));
        fx.join(2, &Client::new(2));
        fx.join(1, &Client::new(1));
        fx.send(2, caps());
        fx.send(1, caps());
        step(&mut fx, 3);
        // Player 1's own, as placed; player 2 not player 1's.
        assert_eq!(
            markers_sent(&mut fx),
            [vec![mine], vec![Markers::default()]]
        );
    }
);

#[test]
fn a_client_without_the_capability_is_never_sent_markers_and_cant_set_them() {
    // face-a-only: the capability exchange, no positions.
    let f = FaceFrame::A;
    let db = WorldDb::in_memory().expect("db");
    {
        let mut fx = Fixture::new(config(f, &db));
        fx.join(1, &Client::new(1));
        step(&mut fx, 3);
        fx.send(1, Message::Markers(Markers(vec![pin(f, 1, "x", [0.0; 3])])));
        step(&mut fx, 3);
        assert_eq!(markers_sent(&mut fx), [vec![], vec![]]);
        fx.server.save_now();
        fx.server.flush_store();
    }
    assert_eq!(db.load_markers(&Client::new(1).public_key), None);
}
