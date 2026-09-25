//! End-to-end test of the C ABI with a real WebTransport client (the same crate's client side).

use std::net::{Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use dwell_net::*;
use wtransport::tls::Sha256Digest;
use wtransport::{ClientConfig, Endpoint};

struct Net(*mut DwellNet);
// SAFETY: the handle is only used from the test thread; the crate's internals are thread-safe.
unsafe impl Send for Net {}

impl Net {
    fn start() -> Net {
        let config = DwellNetConfig {
            port: 0,
            rtc_port: 0,
            advertised_ip: c"127.0.0.1".as_ptr(),
            max_reliable_message_bytes: 1 << 20,
            max_datagram_bytes: 1200,
        };
        // SAFETY: config is valid.
        let net = unsafe { dwell_net_start(&config) };
        assert!(!net.is_null());
        Net(net)
    }

    /// Polls until an event arrives (or panics after 5 s).
    fn next(&self) -> (DwellNetEvent, Vec<u8>) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let mut ev = std::mem::MaybeUninit::<DwellNetEvent>::uninit();
            // SAFETY: live handle and writable event.
            if unsafe { dwell_net_poll(self.0, ev.as_mut_ptr()) } {
                // SAFETY: poll initialized it; data is valid until the next poll.
                let ev = unsafe { ev.assume_init() };
                let data = if ev.data.is_null() {
                    Vec::new()
                } else {
                    unsafe { std::slice::from_raw_parts(ev.data, ev.len) }.to_vec()
                };
                return (ev, data);
            }
            assert!(Instant::now() < deadline, "timed out waiting for an event");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for Net {
    fn drop(&mut self) {
        // SAFETY: handle from dwell_net_start, stopped once.
        unsafe { dwell_net_stop(self.0) };
    }
}

fn frame(payload: &[u8]) -> Vec<u8> {
    let mut v = (payload.len() as u32).to_le_bytes().to_vec();
    v.extend_from_slice(payload);
    v
}

#[test]
fn control_stream_world_stream_and_datagrams_round_trip() {
    let net = Net::start();
    let mut hash = [0u8; 32];
    // SAFETY: live handle, 32-byte buffer.
    let port = unsafe {
        dwell_net_cert_hash(net.0, hash.as_mut_ptr());
        dwell_net_port(net.0)
    };

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

    let (ev, _) = net.next();
    assert_eq!(ev.kind, DwellNetEventKind::Connected);
    assert_eq!(ev.transport, 1);
    assert_eq!(ev.binding, hash);
    let session = ev.session;

    let mut got_reliable = false;
    let mut got_datagram = false;
    while !(got_reliable && got_datagram) {
        let (ev, data) = net.next();
        match ev.kind {
            DwellNetEventKind::Reliable => {
                assert_eq!((ev.channel, data.as_slice()), (0, &b"hello"[..]));
                got_reliable = true;
            }
            DwellNetEventKind::Datagram => {
                assert_eq!(data, b"dgram-up");
                got_datagram = true;
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    // SAFETY: live handle, valid buffers.
    unsafe {
        assert!(dwell_net_send_reliable(
            net.0,
            session,
            0,
            b"reply".as_ptr(),
            5
        ));
        assert!(dwell_net_send_reliable(
            net.0,
            session,
            1,
            b"world".as_ptr(),
            5
        ));
        assert!(dwell_net_send_datagram(
            net.0,
            session,
            b"dgram-down".as_ptr(),
            10
        ));
    }

    let (conn, _tx, mut rx) = client;
    rt.block_on(async move {
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

    // SAFETY: live handle.
    unsafe { dwell_net_close(net.0, session) };
    let (ev, _) = net.next();
    assert_eq!(ev.kind, DwellNetEventKind::Disconnected);
    assert_eq!(ev.session, session);
}
