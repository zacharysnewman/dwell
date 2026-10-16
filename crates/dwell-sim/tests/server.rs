//! Server core: status query, the join handshake with device-key identity (ADR 0004), and pings.

mod common;

use common::*;
use dwell_protocol::*;
use dwell_sim::app_version::APP_VERSION;
use dwell_sim::server::{OutKind, ServerConfig, SessionId};

/// Messages to `id` since the last call, and whether its transport was closed.
fn replies(f: &mut Fixture, id: SessionId) -> (Vec<Message>, bool) {
    let mut closed = false;
    let mut out = Vec::new();
    for o in f
        .server
        .take_outbox()
        .into_iter()
        .filter(|o| o.session == id)
    {
        match o.kind {
            OutKind::Close => closed = true,
            _ => out.push(Message::decode(&o.bytes).expect("decodes")),
        }
    }
    (out, closed)
}

fn hello(key: PublicKey, version: u16) -> Message {
    Message::ClientHello(ClientHello {
        protocol_version: version,
        client_version: "t".into(),
        public_key: key,
        display_name: "x".into(),
    })
}

fn reject_reason(m: &Message) -> RejectReason {
    match m {
        Message::Reject(r) => r.reason,
        other => panic!("expected Reject, got {other:?}"),
    }
}

fn challenge(f: &mut Fixture, id: SessionId, key: PublicKey) -> Nonce {
    f.server
        .on_connected(id, TransportKind::WebTransport, f.binding);
    f.send(id, hello(key, PROTOCOL_VERSION));
    match &replies(f, id).0[0] {
        Message::Challenge(n) => *n,
        other => panic!("expected a challenge, got {other:?}"),
    }
}

fn fixture(config: ServerConfig) -> Fixture {
    let mut f = Fixture::new(config);
    f.binding = [0x42; 32];
    f
}

#[test]
fn status_query_works_before_joining() {
    let mut f = fixture(ServerConfig {
        name: "Test".into(),
        motd: "hi".into(),
        max_players: 4,
        ..flat()
    });
    f.server
        .on_connected(1, TransportKind::WebTransport, f.binding);
    f.send(1, Message::StatusRequest);
    let (r, _) = replies(&mut f, 1);
    assert_eq!(r.len(), 1);
    let Message::StatusResponse(s) = &r[0] else {
        panic!("{r:?}")
    };
    assert_eq!(s.server_name, "Test");
    assert_eq!((s.max_players, s.players), (4, 0));
}

#[test]
fn a_signed_handshake_joins_and_assigns_a_player_id() {
    let mut f = fixture(ServerConfig {
        world_seed: 1234,
        ..flat()
    });
    let w = f.join(7, &Client::new(1));
    assert_eq!(w.player_id, 1);
    assert_eq!(w.world_seed, 1234);
    assert_eq!(f.server.joined_players(), 1);
}

#[test]
fn a_wrong_protocol_version_is_rejected_and_closed() {
    let mut f = fixture(flat());
    f.server
        .on_connected(1, TransportKind::WebTransport, f.binding);
    f.send(1, hello([0; 32], PROTOCOL_VERSION + 1));
    let (r, closed) = replies(&mut f, 1);
    assert_eq!(r.len(), 1);
    assert_eq!(reject_reason(&r[0]), RejectReason::ProtocolVersion);
    // The rejection names the server's app version, so the player knows what to run (RELEASES.md §7).
    let Message::Reject(rej) = &r[0] else {
        unreachable!()
    };
    assert!(
        rej.message.contains(&format!("Dwell {APP_VERSION}")),
        "{}",
        rej.message
    );
    assert!(closed);
}

#[test]
fn a_bad_signature_is_rejected() {
    let mut f = fixture(flat());
    let (who, other) = (Client::new(2), Client::new(3));
    let nonce = challenge(&mut f, 1, who.public_key);
    // Signed by a different key.
    f.send(
        1,
        Message::ClientAuth(other.sign(&auth_transcript(&nonce, &f.binding, &who.public_key))),
    );
    let (r, closed) = replies(&mut f, 1);
    assert_eq!(reject_reason(&r[0]), RejectReason::AuthFailed);
    assert!(closed);
}

#[test]
fn a_signature_for_another_transport_binding_is_rejected() {
    let mut f = fixture(flat());
    let who = Client::new(4);
    let nonce = challenge(&mut f, 1, who.public_key);
    f.send(
        1,
        Message::ClientAuth(who.sign(&auth_transcript(&nonce, &[0x99; 32], &who.public_key))),
    );
    assert_eq!(
        reject_reason(&replies(&mut f, 1).0[0]),
        RejectReason::AuthFailed
    );
}

#[test]
fn a_new_login_replaces_the_old_session_for_the_same_key() {
    let mut f = fixture(flat());
    let who = Client::new(5);
    f.join(1, &who);
    let nonce = challenge(&mut f, 2, who.public_key);
    f.send(
        2,
        Message::ClientAuth(who.sign(&auth_transcript(&nonce, &f.binding, &who.public_key))),
    );
    let (mut to_old, mut to_new, mut old_closed, mut new_closed) =
        (Vec::new(), Vec::new(), false, false);
    for o in f.server.take_outbox() {
        let old = o.session == 1;
        match o.kind {
            OutKind::Close => {
                *(if old {
                    &mut old_closed
                } else {
                    &mut new_closed
                }) = true
            }
            _ => (if old { &mut to_old } else { &mut to_new })
                .push(Message::decode(&o.bytes).unwrap()),
        }
    }
    assert_eq!(to_old.len(), 1);
    assert_eq!(reject_reason(&to_old[0]), RejectReason::Replaced);
    assert!(old_closed);
    // The new session gets the registry and its Welcome (then its first world messages: the
    // spawn's events).
    assert!(matches!(to_new[0], Message::BlockRegistry(_)), "{to_new:?}");
    assert!(matches!(to_new[1], Message::Welcome(_)), "{to_new:?}");
    assert!(!new_closed);
    assert_eq!(f.server.joined_players(), 1);
}

#[test]
fn joins_are_rejected_when_full() {
    let mut f = fixture(ServerConfig {
        max_players: 1,
        ..flat()
    });
    f.join(1, &Client::new(6));
    f.server
        .on_connected(2, TransportKind::WebTransport, f.binding);
    f.send(2, hello(Client::new(7).public_key, PROTOCOL_VERSION));
    assert_eq!(reject_reason(&replies(&mut f, 2).0[0]), RejectReason::Full);
}

#[test]
fn malformed_control_messages_close_the_session() {
    let mut f = fixture(flat());
    f.server
        .on_connected(1, TransportKind::WebTransport, f.binding);
    f.server.on_reliable(1, Channel::Control, &[0xEE, 0x01]);
    let (r, closed) = replies(&mut f, 1);
    assert_eq!(reject_reason(&r[0]), RejectReason::Malformed);
    assert!(closed);
}

#[test]
fn pings_report_the_server_tick_and_datagram_pings_need_a_joined_session() {
    let mut f = fixture(flat());
    f.server
        .on_connected(1, TransportKind::WebTransport, f.binding);
    for _ in 0..3 {
        f.server.step();
    }
    f.server.on_datagram(
        1,
        &Message::DatagramPing(DatagramPing {
            seq: 1,
            client_time_ms: 10.0,
        })
        .encode(),
    );
    assert!(replies(&mut f, 1).0.is_empty(), "not joined yet");

    f.send(
        1,
        Message::Ping(Ping {
            seq: 2,
            client_time_ms: 20.0,
        }),
    );
    let Message::Pong(pong) = replies(&mut f, 1).0.remove(0) else {
        panic!()
    };
    assert_eq!(
        (pong.seq, pong.client_time_ms, pong.server_tick),
        (2, 20.0, 3)
    );

    f.join(1, &Client::new(8));
    f.server.on_datagram(
        1,
        &Message::DatagramPing(DatagramPing {
            seq: 3,
            client_time_ms: 30.0,
        })
        .encode(),
    );
    let Message::DatagramPong(dpong) = replies(&mut f, 1).0.remove(0) else {
        panic!()
    };
    assert_eq!((dpong.seq, dpong.server_tick), (3, 3));
}

#[test]
fn the_default_terrain_world_starts_and_admits_a_player() {
    // The C++ suite ran on the default (terrain) generator; one case keeps that path covered.
    let mut f = fixture(ServerConfig::default());
    assert_eq!(f.join(1, &Client::new(9)).player_id, 1);
    f.server.step();
}
