//! End-to-end test of the front-end with a real WebTransport client (wtransport's client side).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use dwell_net::{Command, Event, Limits, NetServer, StartOptions};
use wtransport::tls::Sha256Digest;
use wtransport::{ClientConfig, Endpoint};

fn start() -> NetServer {
    NetServer::start(StartOptions {
        port: 0,
        rtc_port: 0,
        advertised_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
        limits: Limits {
            max_reliable_message_bytes: 1 << 20,
            max_datagram_bytes: 1200,
        },
    })
    .expect("the front-end starts")
}

/// Polls until an event arrives (or panics after 5 s).
fn next(net: &NetServer) -> Event {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(e) = net.poll() {
            return e;
        }
        assert!(Instant::now() < deadline, "timed out waiting for an event");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn frame(payload: &[u8]) -> Vec<u8> {
    let mut v = (payload.len() as u32).to_le_bytes().to_vec();
    v.extend_from_slice(payload);
    v
}

#[test]
fn control_stream_world_stream_and_datagrams_round_trip() {
    let net = start();
    let (hash, port) = (net.cert_hash(), net.port());
    let rt = tokio::runtime::Runtime::new().unwrap();
    let client = rt.block_on(async move {
        let config = ClientConfig::builder()
            .with_bind_address(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
            .with_server_certificate_hashes([Sha256Digest::new(hash)])
            .build();
        let conn = Endpoint::client(config)
            .unwrap()
            .connect(format!("https://127.0.0.1:{port}/dwell"))
            .await
            .unwrap();
        let (mut tx, rx) = conn.open_bi().await.unwrap().await.unwrap();
        tx.write_all(&[0]).await.unwrap(); // control channel id
        tx.write_all(&frame(b"hello")).await.unwrap();
        conn.send_datagram(b"dgram-up").unwrap();
        (conn, tx, rx)
    });

    let Event::Connected {
        session,
        transport,
        binding,
    } = next(&net)
    else {
        panic!("expected Connected")
    };
    assert_eq!(transport, 1);
    assert_eq!(binding, hash);

    let (mut got_reliable, mut got_datagram) = (false, false);
    while !(got_reliable && got_datagram) {
        match next(&net) {
            Event::Reliable { channel, data, .. } => {
                assert_eq!((channel, data.as_slice()), (0, &b"hello"[..]));
                got_reliable = true;
            }
            Event::Datagram { data, .. } => {
                assert_eq!(data, b"dgram-up");
                got_datagram = true;
            }
            _ => panic!("unexpected event"),
        }
    }

    assert!(net.send(
        session,
        Command::Reliable {
            channel: 0,
            data: b"reply".to_vec()
        }
    ));
    assert!(net.send(
        session,
        Command::Reliable {
            channel: 1,
            data: b"world".to_vec()
        }
    ));
    assert!(net.send(session, Command::Datagram(b"dgram-down".to_vec())));

    // The client stays connected until the server closes the session: dropped earlier, the server
    // could see it leave first and the session would be gone before Close.
    let (conn, _tx, mut rx) = client;
    rt.block_on(async {
        let mut buf = [0u8; 9];
        rx.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, &frame(b"reply")[..]);
        let mut world = conn.accept_uni().await.unwrap();
        let mut wbuf = [0u8; 10];
        world.read_exact(&mut wbuf).await.unwrap();
        assert_eq!(wbuf[0], 1); // world channel id
        assert_eq!(&wbuf[1..], &frame(b"world")[..]);
        let d = tokio::time::timeout(Duration::from_secs(5), conn.receive_datagram())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(d.payload().as_ref(), b"dgram-down");
    });

    // A pause before closing, as a host's next tick would give: the session must still be there.
    std::thread::sleep(Duration::from_millis(300));
    assert!(net.send(session, Command::Close));
    let Event::Disconnected { session: closed } = next(&net) else {
        panic!("expected Disconnected")
    };
    assert_eq!(closed, session);
    drop(conn);
}
