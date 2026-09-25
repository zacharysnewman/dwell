//! WebRTC fallback for dedicated servers (ADR 0008): an ICE-lite endpoint built on `str0m`.
//!
//! Clients connect without a signaling round-trip. The invite link carries the server address,
//! ICE credentials, and certificate fingerprint; the browser builds the server's answer locally
//! and starts ICE. The server learns each client's ICE username from its first STUN binding
//! request and creates a `str0m::Rtc` for it. Three pre-negotiated data channels mirror the
//! WebTransport channels: 0 = control (reliable, ordered), 1 = world (reliable, ordered),
//! 2 = datagrams (unordered, no retransmits). SCTP preserves message boundaries, so no framing.
//!
//! Security: the client pins the server's DTLS fingerprint (the same certificate as WebTransport).
//! The server does not verify client certificates; clients are authenticated by the device-key
//! handshake, whose signature is bound to the server fingerprint (ADR 0004).

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

use str0m::channel::{ChannelConfig, ChannelId, Reliability};
use str0m::config::{DtlsCert, Fingerprint};
use str0m::net::{Protocol, Receive};
use str0m::{
    Candidate, Event as RtcEvent, IceConnectionState, IceCreds, Input, Output, Rtc, RtcConfig,
};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use crate::server::{
    CHANNEL_CONTROL, CHANNEL_WORLD, Command, Event, Limits, Sessions, next_session_id,
};

pub const TRANSPORT_WEBRTC: u8 = 2;
const DATAGRAM_CHANNEL: u16 = 2;
/// Clients that haven't opened their data channels by then are dropped.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Bound on half-open clients, so STUN floods can't allocate unbounded state.
const MAX_CLIENTS: usize = 512;

pub struct RtcParams {
    /// Address advertised in invite links; used as the local ICE candidate.
    pub advertised: SocketAddr,
    pub cert: DtlsCert,
    pub binding: [u8; 32],
    pub ice: IceCreds,
    pub limits: Limits,
}

/// Random ICE credentials (RFC 8445 ice-chars; ufrag ≥ 4, pwd ≥ 22 characters).
pub fn random_ice_credentials() -> IceCreds {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("OS randomness");
    let s: String = bytes
        .iter()
        .map(|b| CHARS[*b as usize % CHARS.len()] as char)
        .collect();
    IceCreds {
        ufrag: s[..8].to_string(),
        pass: s[8..].to_string(),
    }
}

struct Client {
    rtc: Rtc,
    session: u32,
    channels: [ChannelId; 3],
    open: [bool; 3],
    connected: bool,
    created: Instant,
    /// Reliable messages waiting for SCTP buffer space, per channel (control, world).
    pending: [VecDeque<Vec<u8>>; 2],
}

pub async fn run(
    socket: std::net::UdpSocket,
    params: RtcParams,
    events: std_mpsc::Sender<Event>,
    sessions: Sessions,
) {
    socket.set_nonblocking(true).expect("nonblocking UDP");
    let socket = UdpSocket::from_std(socket).expect("tokio UDP");
    let (cmd_tx, mut commands) = mpsc::unbounded_channel::<(u32, Command)>();
    let mut clients: HashMap<SocketAddr, Client> = HashMap::new();
    let mut by_session: HashMap<u32, SocketAddr> = HashMap::new();
    let mut buf = vec![0u8; 2048];

    loop {
        // Drive every client until it reports its next timeout.
        let mut next = Instant::now() + Duration::from_millis(100);
        let mut dead = Vec::new();
        for (addr, c) in clients.iter_mut() {
            match drive(c, &socket, &events, &sessions, &cmd_tx, &params).await {
                Some(deadline) => next = next.min(deadline),
                None => dead.push(*addr),
            }
            if !c.connected && c.created.elapsed() > CONNECT_TIMEOUT {
                dead.push(*addr);
            }
        }
        for addr in dead {
            if let Some(c) = clients.remove(&addr) {
                by_session.remove(&c.session);
                sessions.lock().expect("sessions lock").remove(&c.session);
                if c.connected {
                    let _ = events.send(Event::Disconnected { session: c.session });
                }
            }
        }

        let sleep = tokio::time::sleep_until(tokio::time::Instant::from_std(next));
        tokio::select! {
            received = socket.recv_from(&mut buf) => {
                let Ok((n, source)) = received else { continue };
                let data = &buf[..n];
                if !clients.contains_key(&source) {
                    if clients.len() >= MAX_CLIENTS { continue; }
                    let Some(client) = new_client(data, &params) else { continue };
                    by_session.insert(client.session, source);
                    clients.insert(source, client);
                }
                let c = clients.get_mut(&source).expect("client");
                if let Ok(r) = Receive::new(Protocol::Udp, source, params.advertised, data) {
                    let _ = c.rtc.handle_input(Input::Receive(Instant::now(), r));
                }
            }
            command = commands.recv() => {
                let Some((session, command)) = command else { break };
                let Some(c) = by_session.get(&session).and_then(|a| clients.get_mut(a)) else { continue };
                match command {
                    Command::Reliable { channel, data } => {
                        let idx = if channel == CHANNEL_WORLD { 1 } else { 0 };
                        c.pending[idx].push_back(data);
                        flush_pending(c);
                    }
                    Command::Datagram(data) => {
                        if let Some(mut ch) = c.rtc.channel(c.channels[2]) {
                            let _ = ch.write(true, &data); // best-effort
                        }
                    }
                    Command::Close => c.rtc.disconnect(),
                }
            }
            _ = sleep => {}
        }
    }
}

/// Creates a client from the first STUN binding request addressed to our ICE username.
fn new_client(data: &[u8], params: &RtcParams) -> Option<Client> {
    let username = stun_username(data)?;
    let (ours, theirs) = username.split_once(':')?;
    if ours != params.ice.ufrag || theirs.is_empty() {
        return None;
    }
    let now = Instant::now();
    let mut rtc = RtcConfig::new()
        .set_ice_lite(true)
        .set_fingerprint_verification(false)
        .set_dtls_cert(params.cert.clone())
        .set_local_ice_credentials(params.ice.clone())
        .build(now);
    rtc.add_local_candidate(Candidate::host(params.advertised, "udp").ok()?);
    let channels = {
        let mut api = rtc.direct_api();
        // The client's ICE password is never needed: an ICE-lite agent only answers checks.
        api.set_remote_ice_credentials(IceCreds {
            ufrag: theirs.to_string(),
            pass: String::new(),
        });
        api.set_ice_controlling(false);
        // Verification is disabled; str0m still needs a remote fingerprint on record.
        api.set_remote_fingerprint(Fingerprint {
            hash_func: "sha-256".into(),
            bytes: vec![0; 32],
        });
        api.start_dtls(false).ok()?;
        api.start_sctp(false);
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
            make(CHANNEL_CONTROL as u16, true, Reliability::Reliable),
            make(CHANNEL_WORLD as u16, true, Reliability::Reliable),
            make(
                DATAGRAM_CHANNEL,
                false,
                Reliability::MaxRetransmits { retransmits: 0 },
            ),
        ]
    };
    Some(Client {
        rtc,
        session: next_session_id(),
        channels,
        open: [false; 3],
        connected: false,
        created: now,
        pending: [VecDeque::new(), VecDeque::new()],
    })
}

/// Writes queued reliable messages while SCTP has buffer space.
fn flush_pending(c: &mut Client) {
    for idx in 0..2 {
        while let Some(msg) = c.pending[idx].front() {
            let Some(mut ch) = c.rtc.channel(c.channels[idx]) else {
                break;
            };
            match ch.write(true, msg) {
                Ok(true) => {
                    c.pending[idx].pop_front();
                }
                _ => break, // retried on ChannelBufferedAmountLow
            }
        }
    }
}

/// Processes a client's outputs. Returns its next timeout, or None if it is finished.
async fn drive(
    c: &mut Client,
    socket: &UdpSocket,
    events: &std_mpsc::Sender<Event>,
    sessions: &Sessions,
    cmd_tx: &mpsc::UnboundedSender<(u32, Command)>,
    params: &RtcParams,
) -> Option<Instant> {
    if !c.rtc.is_alive() {
        return None;
    }
    let _ = c.rtc.handle_input(Input::Timeout(Instant::now()));
    loop {
        match c.rtc.poll_output() {
            Ok(Output::Timeout(t)) => return Some(t),
            Ok(Output::Transmit(t)) => {
                let _ = socket.send_to(&t.contents, t.destination).await;
            }
            Ok(Output::Event(e)) => match e {
                RtcEvent::ChannelOpen(id, _) => {
                    if let Some(idx) = c.channels.iter().position(|x| *x == id) {
                        c.open[idx] = true;
                    }
                    if !c.connected && c.open.iter().all(|o| *o) {
                        c.connected = true;
                        sessions
                            .lock()
                            .expect("sessions lock")
                            .insert(c.session, cmd_tx.clone());
                        let _ = events.send(Event::Connected {
                            session: c.session,
                            transport: TRANSPORT_WEBRTC,
                            binding: params.binding,
                        });
                        flush_pending(c);
                    }
                }
                RtcEvent::ChannelData(d) if c.connected => {
                    let idx = c.channels.iter().position(|x| *x == d.id);
                    let event = match idx {
                        Some(0)
                            if d.data.len()
                                <= params.limits.max_reliable_message_bytes as usize =>
                        {
                            Event::Reliable {
                                session: c.session,
                                channel: CHANNEL_CONTROL,
                                data: d.data,
                            }
                        }
                        Some(2) if d.data.len() <= params.limits.max_datagram_bytes as usize => {
                            Event::Datagram {
                                session: c.session,
                                data: d.data,
                            }
                        }
                        // Clients never send on the world channel; oversize messages are dropped.
                        _ => continue,
                    };
                    let _ = events.send(event);
                }
                RtcEvent::ChannelBufferedAmountLow(_) => flush_pending(c),
                RtcEvent::IceConnectionStateChange(IceConnectionState::Disconnected)
                | RtcEvent::ChannelClose(_) => {
                    c.rtc.disconnect();
                    return None;
                }
                _ => {}
            },
            Err(_) => return None,
        }
    }
}

/// Extracts the USERNAME attribute from a STUN message (RFC 5389), if `data` is one.
fn stun_username(data: &[u8]) -> Option<String> {
    const MAGIC_COOKIE: [u8; 4] = [0x21, 0x12, 0xA4, 0x42];
    if data.len() < 20 || data[0] > 3 || data[4..8] != MAGIC_COOKIE {
        return None;
    }
    let end = 20 + (u16::from_be_bytes([data[2], data[3]]) as usize).min(data.len() - 20);
    let mut pos = 20;
    while pos + 4 <= end {
        let kind = u16::from_be_bytes([data[pos], data[pos + 1]]);
        let len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        let value = data.get(pos + 4..pos + 4 + len)?;
        if kind == 0x0006 {
            return String::from_utf8(value.to_vec()).ok();
        }
        pos += 4 + len.div_ceil(4) * 4;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stun_username() {
        // Binding request with a single USERNAME attribute "abcd:xy".
        let mut msg = vec![0x00, 0x01, 0x00, 0x0C, 0x21, 0x12, 0xA4, 0x42];
        msg.extend_from_slice(&[0u8; 12]); // transaction id
        msg.extend_from_slice(&[0x00, 0x06, 0x00, 0x07]);
        msg.extend_from_slice(b"abcd:xy\0");
        assert_eq!(stun_username(&msg).as_deref(), Some("abcd:xy"));
        assert_eq!(stun_username(&msg[..10]), None);
        assert_eq!(stun_username(b"\x16\xfe\xfd not stun at all.."), None);
    }

    #[test]
    fn ice_credentials_meet_rfc_lengths() {
        let c = random_ice_credentials();
        assert!(c.ufrag.len() >= 4 && c.pass.len() >= 22);
        assert!(
            c.ufrag
                .chars()
                .chain(c.pass.chars())
                .all(|ch| ch.is_ascii_alphanumeric())
        );
    }
}
