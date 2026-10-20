//! End-to-end test of the WebRTC fallback (ADR 0008) with a real WebRTC peer: a `str0m` client
//! that connects as the browser does (ICE controlling, DTLS client, the four pre-negotiated data
//! channels) and, like the browser's, accepts SCTP messages of at most 64 KiB from the server.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use dwell_net::{Command, Event, Limits, NetServer, StartOptions};
use dwell_protocol::fragment::{Reassembler, fragment};
use dwell_protocol::{
    CHUNK_VOLUME, ChunkData, ChunkForm, MAX_RELIABLE_MESSAGE_BYTES, MAX_WEBRTC_MESSAGE_BYTES,
    Message,
};
use str0m::channel::{ChannelConfig, ChannelId, Reliability};
use str0m::config::Fingerprint;
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event as RtcEvent, IceCreds, Input, Output, Rtc, RtcConfig};

fn start() -> NetServer {
    NetServer::start(StartOptions {
        port: 0,
        rtc_port: 0,
        advertised_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
        limits: Limits {
            max_reliable_message_bytes: MAX_RELIABLE_MESSAGE_BYTES as u32,
            max_datagram_bytes: 1200,
        },
    })
    .expect("the front-end starts")
}

/// Polls until an event arrives (or panics after 10 s).
fn next(net: &NetServer, peer: &mut Peer) -> Event {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(e) = net.poll() {
            return e;
        }
        assert!(Instant::now() < deadline, "timed out waiting for an event");
        peer.drive(Duration::from_millis(5));
    }
}

/// A sans-I/O str0m peer on a local UDP socket.
struct Peer {
    rtc: Rtc,
    socket: UdpSocket,
    local: SocketAddr,
    channels: [ChannelId; 4],
    open: [bool; 4],
    /// Reliable SCTP messages received, per channel.
    received: [Vec<Vec<u8>>; 4],
    closed: bool,
}

impl Peer {
    fn connect(net: &NetServer) -> Self {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let local = socket.local_addr().unwrap();
        let server = SocketAddr::from((Ipv4Addr::LOCALHOST, net.rtc_port()));
        let (ufrag, pass) = net.ice_credentials();
        let mut rtc = RtcConfig::new().build(Instant::now());
        rtc.add_local_candidate(Candidate::host(local, "udp").unwrap());
        rtc.add_remote_candidate(Candidate::host(server, "udp").unwrap());
        let channels = {
            let mut api = rtc.direct_api();
            api.set_remote_ice_credentials(IceCreds {
                ufrag: ufrag.into(),
                pass: pass.into(),
            });
            api.set_ice_controlling(true);
            api.set_remote_fingerprint(Fingerprint {
                hash_func: "sha-256".into(),
                bytes: net.cert_hash().to_vec(),
            });
            api.start_dtls(true).unwrap();
            api.start_sctp(true);
            let mut make = |id: u16, ordered: bool, reliability: Reliability| {
                api.create_data_channel(ChannelConfig {
                    label: String::new(),
                    ordered,
                    reliability,
                    negotiated: Some(id),
                    protocol: String::new(),
                })
            };
            [
                make(0, true, Reliability::Reliable),
                make(1, true, Reliability::Reliable),
                make(2, false, Reliability::MaxRetransmits { retransmits: 0 }),
                make(3, true, Reliability::Reliable),
            ]
        };
        Self {
            rtc,
            socket,
            local,
            channels,
            open: [false; 4],
            received: Default::default(),
            closed: false,
        }
    }

    /// Runs the peer for up to `wait` (less if a packet arrives).
    fn drive(&mut self, wait: Duration) {
        let _ = self.rtc.handle_input(Input::Timeout(Instant::now()));
        let timeout = loop {
            match self.rtc.poll_output() {
                Ok(Output::Timeout(t)) => break t,
                Ok(Output::Transmit(t)) => {
                    let _ = self.socket.send_to(&t.contents, t.destination);
                }
                Ok(Output::Event(e)) => match e {
                    RtcEvent::ChannelOpen(id, _) => {
                        if let Some(i) = self.channels.iter().position(|c| *c == id) {
                            self.open[i] = true;
                        }
                    }
                    RtcEvent::ChannelData(d) => {
                        if let Some(i) = self.channels.iter().position(|c| *c == d.id) {
                            self.received[i].push(d.data);
                        }
                    }
                    RtcEvent::ChannelClose(_) => self.closed = true,
                    _ => {}
                },
                Err(_) => {
                    self.closed = true;
                    return;
                }
            }
        };
        let now = Instant::now();
        let wait = wait
            .min(timeout.saturating_duration_since(now))
            .max(Duration::from_millis(1));
        self.socket.set_read_timeout(Some(wait)).unwrap();
        let mut buf = [0u8; 2048];
        if let Ok((n, source)) = self.socket.recv_from(&mut buf)
            && let Ok(r) = Receive::new(Protocol::Udp, source, self.local, &buf[..n])
        {
            let _ = self.rtc.handle_input(Input::Receive(Instant::now(), r));
        }
    }

    /// Writes one SCTP message, waiting while the channel's buffer is full.
    fn write(&mut self, channel: usize, data: &[u8]) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let mut ch = self.rtc.channel(self.channels[channel]).expect("channel");
            if ch.write(true, data).expect("the write is accepted") {
                return;
            }
            assert!(Instant::now() < deadline, "the channel stayed full");
            self.drive(Duration::from_millis(5));
        }
    }
}

/// A chunk no two of whose voxels in wire order (x fastest, then z, then y) share a block, almost a
/// checkerboard: palette + RLE spends two bytes on each of its 32 768 voxels, over 64 KiB.
fn checkerboard_chunk() -> Vec<u8> {
    let voxels = (0..CHUNK_VOLUME)
        .map(|i| {
            let wire = (i & 31) | (((i >> 10) & 31) << 5) | (((i >> 5) & 31) << 10);
            1 + (wire % 3) as u16
        })
        .collect();
    Message::ChunkData(ChunkData {
        form: ChunkForm::Explicit,
        coord: [3, -1, 7],
        revision: 1,
        voxels,
    })
    .encode()
}

/// Regression (0.14.5): str0m's direct API sends SCTP messages of at most 64 KiB, and the server
/// closed the connection on any larger reliable message, so a chunk with a checkerboard pattern
/// (65 562 bytes encoded) disconnected a WebRTC player. Messages over the limit now travel as
/// fragments (`dwell_protocol::fragment`) on every reliable channel, in both directions, and
/// arrive intact; messages that fit are sent unchanged.
#[test]
fn messages_over_64_kib_cross_every_reliable_webrtc_channel_intact() {
    let net = start();
    let mut peer = Peer::connect(&net);
    let Event::Connected {
        session, transport, ..
    } = next(&net, &mut peer)
    else {
        panic!("expected Connected")
    };
    assert_eq!(transport, 2);

    let chunk = checkerboard_chunk();
    assert!(chunk.len() > MAX_WEBRTC_MESSAGE_BYTES, "{}", chunk.len());
    let big: Vec<u8> = (0..MAX_RELIABLE_MESSAGE_BYTES)
        .map(|i| (i % 251) as u8)
        .collect();
    let small = Message::ChunkUnload(dwell_protocol::ChunkUnload {
        coords: vec![[1, 2, 3]],
    })
    .encode();
    // (net channel, data channel index): control, world, lod.
    let sends: [(u8, usize, Vec<Vec<u8>>); 3] = [
        (0, 0, vec![big.clone(), small.clone()]),
        (1, 1, vec![chunk.clone(), small.clone(), chunk.clone()]),
        (2, 3, vec![small.clone(), big.clone()]),
    ];
    for (channel, _, messages) in &sends {
        for data in messages {
            assert!(net.send(
                session,
                Command::Reliable {
                    channel: *channel,
                    data: data.clone()
                }
            ));
        }
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut got: [Vec<Vec<u8>>; 4] = Default::default();
    let mut reassemblers: Vec<Reassembler> = (0..4)
        .map(|_| Reassembler::new(MAX_RELIABLE_MESSAGE_BYTES))
        .collect();
    loop {
        for (i, received) in peer.received.iter_mut().enumerate() {
            for sctp in received.drain(..) {
                assert!(sctp.len() <= MAX_WEBRTC_MESSAGE_BYTES);
                if let Some(m) = reassemblers[i].push(&sctp).expect("well-formed fragments") {
                    got[i].push(m);
                }
            }
        }
        if sends.iter().all(|(_, i, m)| got[*i].len() == m.len()) {
            break;
        }
        assert!(
            !peer.closed,
            "the server closed the connection; got {:?}",
            got.iter().map(Vec::len).collect::<Vec<_>>()
        );
        assert!(
            Instant::now() < deadline,
            "timed out; got {:?}",
            got.iter().map(Vec::len).collect::<Vec<_>>()
        );
        peer.drive(Duration::from_millis(5));
    }
    for (_, i, messages) in &sends {
        assert!(got[*i] == *messages, "channel {i} delivered other bytes");
    }
    // A message that fits is one unchanged SCTP message.
    assert!(got[1][1] == small);

    // Up: the client fragments the same way; the server reassembles control messages.
    for part in fragment(&chunk, MAX_WEBRTC_MESSAGE_BYTES) {
        peer.write(0, &part);
    }
    peer.write(0, &small);
    let mut up = Vec::new();
    while up.len() < 2 {
        match next(&net, &mut peer) {
            Event::Reliable { channel, data, .. } => {
                assert_eq!(channel, 0);
                up.push(data);
            }
            Event::Disconnected { .. } => panic!("the server dropped the client"),
            _ => {}
        }
    }
    assert!(
        up[0] == chunk && up[1] == small,
        "the server reassembled other bytes"
    );
    net.shutdown();
}
