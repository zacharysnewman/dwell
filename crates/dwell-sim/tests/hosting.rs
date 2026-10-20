//! Friend-world hosting in the integrated server (ARCHITECTURE.md §10.2, Phase 5c): a local world
//! admits guests once its player starts hosting, with the host's limits and policies; guests hear
//! when the host pauses; stopping hosting (or shutting down) ends sessions with a reason.

mod common;

use common::*;
use dwell_protocol::*;
use dwell_sim::server::{EditPolicy, OutKind, ServerConfig, SessionId};

/// A local world before hosting: one player, like the local-world create call.
fn local_world() -> ServerConfig {
    ServerConfig {
        max_players: 1,
        ..flat()
    }
}

/// Connects over `kind` and runs the handshake; the server's last reply (Welcome or Reject).
fn handshake(f: &mut Fixture, id: SessionId, who: &Client, kind: TransportKind) -> Message {
    f.server.on_connected(id, kind, f.binding);
    f.send(
        id,
        Message::ClientHello(ClientHello {
            protocol_version: PROTOCOL_VERSION,
            client_version: "test".into(),
            public_key: who.public_key,
            display_name: "Tester".into(),
        }),
    );
    let reply = f.take(id).remove(0).0;
    let Message::Challenge(nonce) = reply else {
        return reply;
    };
    let signature = who.sign(&auth_transcript(&nonce, &f.binding, &who.public_key));
    f.send(id, Message::ClientAuth(signature));
    // The registry's parts precede Welcome (ADR 0038).
    f.take(id)
        .into_iter()
        .map(|(m, _)| m)
        .find(|m| !matches!(m, Message::BlockRegistry(_)))
        .expect("a reply")
}

fn reason_of(m: &Message) -> RejectReason {
    match m {
        Message::Reject(r) => r.reason,
        other => panic!("expected Reject, got {other:?}"),
    }
}

fn may_fly(m: &Message) -> bool {
    match m {
        Message::Welcome(w) => w.flags & welcome_flags::FLIGHT != 0,
        other => panic!("expected Welcome, got {other:?}"),
    }
}

fn is_welcome(m: &Message) -> bool {
    matches!(m, Message::Welcome(_))
}

#[test]
fn a_local_world_admits_guests_only_once_its_host_starts_hosting_up_to_its_cap() {
    let mut f = Fixture::new(local_world());
    let (host, guest, third) = (Client::new(1), Client::new(2), Client::new(3));
    assert!(is_welcome(&handshake(
        &mut f,
        1,
        &host,
        TransportKind::Loopback
    )));
    assert_eq!(
        reason_of(&handshake(&mut f, 2, &guest, TransportKind::WebRtc)),
        RejectReason::Full
    );

    f.server.set_hosting(
        2,
        EditPolicy::Everyone,
        EditPolicy::Everyone,
        host.public_key,
    );
    assert!(is_welcome(&handshake(
        &mut f,
        3,
        &guest,
        TransportKind::WebRtc
    )));
    assert_eq!(f.server.joined_players(), 2);
    assert_eq!(
        reason_of(&handshake(&mut f, 4, &third, TransportKind::WebRtc)),
        RejectReason::Full
    );
}

#[test]
fn with_ops_only_policies_the_host_an_op_may_fly_guests_may_not() {
    let mut f = Fixture::new(local_world());
    let (host, guest) = (Client::new(1), Client::new(2));
    assert!(is_welcome(&handshake(
        &mut f,
        1,
        &host,
        TransportKind::Loopback
    )));
    f.server
        .set_hosting(4, EditPolicy::Ops, EditPolicy::Ops, host.public_key);
    assert!(!may_fly(&handshake(
        &mut f,
        2,
        &guest,
        TransportKind::WebRtc
    )));
    // The host rejoining (e.g. after a reload) is still an op.
    assert!(may_fly(&handshake(
        &mut f,
        3,
        &host,
        TransportKind::Loopback
    )));
}

#[test]
fn every_joined_client_hears_that_the_host_paused_and_resumed() {
    let mut f = Fixture::new(local_world());
    let (host, guest) = (Client::new(1), Client::new(2));
    assert!(is_welcome(&handshake(
        &mut f,
        1,
        &host,
        TransportKind::Loopback
    )));
    f.server.set_hosting(
        4,
        EditPolicy::Everyone,
        EditPolicy::Everyone,
        host.public_key,
    );
    assert!(is_welcome(&handshake(
        &mut f,
        2,
        &guest,
        TransportKind::WebRtc
    )));
    f.take_all();

    f.server.broadcast_host_status(HostState::Paused);
    let sent = f.take_all();
    for id in [1, 2] {
        let messages = sent.get(&id).cloned().unwrap_or_default();
        assert_eq!(messages.len(), 1, "session {id}: {messages:?}");
        assert_eq!(messages[0], Message::HostStatus(HostState::Paused));
    }
    f.server.broadcast_host_status(HostState::Resumed);
    let sent = f.take_all();
    assert_eq!(sent[&2][0], Message::HostStatus(HostState::Resumed));
}

#[test]
fn stopping_ends_the_guests_sessions_with_a_reason_and_keeps_the_host_playing() {
    let mut f = Fixture::new(local_world());
    let (host, a, b) = (Client::new(1), Client::new(2), Client::new(3));
    assert!(is_welcome(&handshake(
        &mut f,
        1,
        &host,
        TransportKind::Loopback
    )));
    f.server.set_hosting(
        4,
        EditPolicy::Everyone,
        EditPolicy::Everyone,
        host.public_key,
    );
    assert!(is_welcome(&handshake(&mut f, 2, &a, TransportKind::WebRtc)));
    assert!(is_welcome(&handshake(&mut f, 3, &b, TransportKind::WebRtc)));
    f.take_all();

    f.server.close_sessions("The host stopped hosting.", true);
    let out = f.server.take_outbox();
    for guest in [2, 3] {
        let reject = out
            .iter()
            .find(|o| o.session == guest && matches!(o.kind, OutKind::Reliable(_)))
            .expect("a reject");
        let m = Message::decode(&reject.bytes).expect("decodes");
        let Message::Reject(r) = m else {
            panic!("expected Reject, got {m:?}")
        };
        assert_eq!(r.reason, RejectReason::ServerClosing);
        assert_eq!(r.message, "The host stopped hosting.");
        assert!(
            out.iter()
                .any(|o| o.session == guest && o.kind == OutKind::Close)
        );
    }
    assert!(out.iter().all(|o| o.session != 1));
    assert_eq!(f.server.joined_players(), 1);

    // A dedicated server shutting down closes everyone.
    f.server
        .close_sessions("The server is shutting down.", false);
    assert_eq!(f.server.joined_players(), 0);
}
