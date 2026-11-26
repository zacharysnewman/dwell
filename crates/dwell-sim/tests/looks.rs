//! Players' looks through the server (the Character painter, docs/CHARACTERS.md §9.4): a client's
//! paint kept and told to every other client with `players.looks`, everyone's sent to a client once
//! it says the capability, a change replacing it, and a departed player's look cleared.
mod common;
use common::*;
use dwell_protocol::looks::PlayerLook;
use dwell_protocol::*;
use dwell_sim::server::SessionId;

fn caps() -> Message {
    Message::Capabilities(capabilities::Capabilities::this_client())
}

fn look(slot: u8) -> PlayerLook {
    PlayerLook {
        player_id: 0,
        slots: vec!["skin_fair".into(), "wool_blue".into()],
        faces: vec![slot; 1251],
    }
}

/// The looks session `id` was sent since the last call.
fn looks_to(fx: &mut Fixture, id: SessionId) -> Vec<Vec<PlayerLook>> {
    fx.take_all()
        .remove(&id)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|m| match m {
            Message::Looks(l) => Some(l),
            _ => None,
        })
        .collect()
}

#[test]
fn a_players_look_reaches_the_others_and_clears_when_they_leave() {
    // both-faces: a look has no geometry on the server (faces are the model's, not the world's).
    let mut fx = Fixture::new(flat());
    let p1 = fx.join(1, &Client::new(1)).player_id;
    fx.join(2, &Client::new(2));
    fx.send(2, caps());
    assert_eq!(
        looks_to(&mut fx, 2),
        vec![vec![]],
        "everyone's (nobody's yet), once"
    );
    // Player 1 says the capability and its look: kept under its id and told to player 2.
    fx.send(1, caps());
    fx.send(1, Message::Looks(vec![look(1)]));
    let kept = fx.server.look_of(p1).expect("kept").clone();
    assert_eq!((kept.player_id, kept.faces[0]), (p1, 1));
    let mut all = fx.take_all();
    let got: Vec<_> = all
        .remove(&2)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|m| match m {
            Message::Looks(l) => Some(l),
            _ => None,
        })
        .collect();
    assert_eq!(got, vec![vec![kept.clone()]]);
    // A changed look replaces it (a malformed one fails to decode: `looks.rs`'s tests).
    fx.send(1, Message::Looks(vec![look(0)]));
    let kept = fx.server.look_of(p1).expect("kept").clone();
    assert_eq!(kept.faces[0], 0);
    assert_eq!(looks_to(&mut fx, 2), vec![vec![kept.clone()]]);
    // A third client gets everyone's once it says the capability.
    fx.join(3, &Client::new(3));
    fx.send(3, caps());
    assert_eq!(looks_to(&mut fx, 3), vec![vec![kept]]);
    // Player 1 leaves: the others hear its look cleared.
    fx.server.on_disconnected(1);
    let cleared = PlayerLook {
        player_id: p1,
        ..PlayerLook::default()
    };
    let mut all = fx.take_all();
    for id in [2, 3] {
        let got: Vec<_> = all
            .remove(&id)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|m| match m {
                Message::Looks(l) => Some(l),
                _ => None,
            })
            .collect();
        assert_eq!(got, vec![vec![cleared.clone()]], "session {id}");
    }
}
