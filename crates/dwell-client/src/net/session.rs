//! Client side of the join handshake and connection health (ARCHITECTURE.md §8.3, ADR 0004), as a
//! poll-driven state machine: the host feeds it transport events and the time, and drains what to
//! send and the game messages. Signing is the host's (the device key is a non-extractable WebCrypto
//! key): on a Challenge the session asks for a signature and resumes with [`Session::signed`].

use std::collections::VecDeque;

use dwell_blocks::REGISTRY_HASH;
use dwell_protocol::*;

const RTT_SMOOTHING: f64 = 0.2;

fn smooth(prev: Option<f64>, sample: f64) -> Option<f64> {
    Some(prev.map_or(sample, |p| p + (sample - p) * RTT_SMOOTHING))
}

#[derive(Clone, Debug, PartialEq)]
pub enum SessionState {
    Handshaking,
    Joined {
        player_id: u16,
        world_seed: u64,
        generator_version: u32,
        verification_chunk: [i32; 3],
        /// The server lets this player use creative flight (§8.3).
        may_fly: bool,
    },
    Rejected {
        reason: RejectReason,
        message: String,
    },
    Closed {
        message: String,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SessionStats {
    /// Smoothed round-trip time over the reliable control stream, ms.
    pub rtt_ms: Option<f64>,
    /// Smoothed round-trip time over datagrams, ms.
    pub datagram_rtt_ms: Option<f64>,
    /// Latest server tick seen.
    pub server_tick: u32,
    /// A friend world's host has paused it (their page is hidden, HostStatus, §10.2).
    pub host_paused: bool,
}

/// What the host must do for the session.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    /// A message on the control channel.
    Reliable(Vec<u8>),
    Datagram(Vec<u8>),
    /// Sign this transcript with the device key, then call `signed`.
    Sign(Vec<u8>),
    /// Close the transport.
    Close,
}

pub struct Session {
    binding: [u8; 32],
    public_key: PublicKey,
    display_name: String,
    client_version: String,
    ping_interval_ms: f64,
    next_ping_ms: Option<f64>,
    ping_seq: u32,
    state: SessionState,
    stats: SessionStats,
    out: Vec<Out>,
    /// Gameplay messages (snapshots, player events, chunks, LOD), with their bytes, in arrival
    /// order: reliable ones wait here until the game drains them, so none is lost while it loads.
    game: VecDeque<(Message, Vec<u8>)>,
    changed: bool,
}

impl Session {
    pub fn new(
        binding: [u8; 32],
        public_key: PublicKey,
        display_name: &str,
        client_version: &str,
        ping_interval_ms: f64,
    ) -> Self {
        Self {
            binding,
            public_key,
            display_name: display_name.into(),
            client_version: client_version.into(),
            ping_interval_ms,
            next_ping_ms: None,
            ping_seq: 0,
            state: SessionState::Handshaking,
            stats: SessionStats::default(),
            out: Vec::new(),
            game: VecDeque::new(),
            changed: true,
        }
    }

    /// Sends ClientHello and starts pinging.
    pub fn start(&mut self, now: f64) {
        self.send(&Message::ClientHello(ClientHello {
            protocol_version: PROTOCOL_VERSION,
            client_version: self.client_version.clone(),
            public_key: self.public_key,
            display_name: self.display_name.clone(),
        }));
        self.ping(now);
    }

    /// Runs the ping timer.
    pub fn tick(&mut self, now: f64) {
        if self.next_ping_ms.is_some_and(|at| now >= at) {
            self.ping(now);
        }
    }

    pub fn state(&self) -> &SessionState {
        &self.state
    }
    pub fn stats(&self) -> SessionStats {
        self.stats
    }
    fn joined(&self) -> bool {
        matches!(self.state, SessionState::Joined { .. })
    }
    /// Whether the state or stats changed since the last call (for the UI).
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// What to send or do, in order.
    pub fn take_out(&mut self) -> Vec<Out> {
        std::mem::take(&mut self.out)
    }
    /// The next gameplay message (only while joined).
    pub fn next_game(&mut self) -> Option<(Message, Vec<u8>)> {
        if self.joined() {
            self.game.pop_front()
        } else {
            None
        }
    }

    /// Sends a reliable control message while joined (WorldgenCheck, edits, resyncs, LodRequest).
    pub fn send_control(&mut self, m: &Message) {
        if self.joined() {
            self.send(m);
        }
    }
    /// Sends a gameplay datagram (player input); dropped unless joined.
    pub fn send_game_datagram(&mut self, m: &Message) {
        if self.joined() {
            self.out.push(Out::Datagram(m.encode()));
        }
    }
    pub fn close(&mut self) {
        self.next_ping_ms = None;
        self.out.push(Out::Close);
    }

    fn send(&mut self, m: &Message) {
        self.out.push(Out::Reliable(m.encode()));
    }
    fn set_state(&mut self, s: SessionState) {
        self.state = s;
        self.changed = true;
    }
    fn stop_pings(&mut self) {
        self.next_ping_ms = None;
    }

    fn ping(&mut self, now: f64) {
        self.ping_seq += 1;
        let (seq, client_time_ms) = (self.ping_seq, now);
        self.send(&Message::Ping(Ping {
            seq,
            client_time_ms,
        }));
        if self.joined() {
            self.out.push(Out::Datagram(
                Message::DatagramPing(DatagramPing {
                    seq,
                    client_time_ms,
                })
                .encode(),
            ));
        }
        self.next_ping_ms = Some(now + self.ping_interval_ms);
    }

    /// The device key's signature of the transcript asked for by `Out::Sign`.
    pub fn signed(&mut self, signature: Signature) {
        self.send(&Message::ClientAuth(signature));
    }

    /// The transport closed.
    pub fn on_close(&mut self, message: &str) {
        if !matches!(self.state, SessionState::Rejected { .. }) {
            self.set_state(SessionState::Closed {
                message: message.into(),
            });
        }
        self.stop_pings();
    }

    fn malformed(&mut self) {
        self.set_state(SessionState::Closed {
            message: "Server sent a malformed message.".into(),
        });
        self.close();
    }

    pub fn on_reliable(&mut self, channel: Channel, bytes: &[u8], now: f64) {
        let Some(m) = Message::decode(bytes) else {
            self.malformed();
            return;
        };
        if channel != Channel::Control {
            if matches!(
                m,
                Message::PlayerEvent(_)
                    | Message::ChunkData(_)
                    | Message::ChunkUnload(_)
                    | Message::VoxelModification(_)
                    | Message::LodIndex(_)
                    | Message::LodIndexUpdate(_)
                    | Message::LodData(_)
            ) && self.joined()
            {
                self.game.push_back((m, bytes.to_vec()));
            }
            return;
        }
        match m {
            Message::Challenge(nonce) => {
                self.out.push(Out::Sign(auth_transcript(
                    &nonce,
                    &self.binding,
                    &self.public_key,
                )));
            }
            Message::Welcome(w) => {
                if w.registry_hash != REGISTRY_HASH {
                    // Runtime state ids on the wire would name different blocks (BLOCK_REGISTRY.md).
                    self.set_state(SessionState::Rejected {
                        reason: RejectReason::ProtocolVersion,
                        message: "The server runs a different block registry (another build)."
                            .into(),
                    });
                    self.stop_pings();
                    self.close();
                    return;
                }
                self.stats.server_tick = w.server_tick;
                self.set_state(SessionState::Joined {
                    player_id: w.player_id,
                    world_seed: w.world_seed,
                    generator_version: w.generator_version,
                    verification_chunk: w.verification_chunk,
                    may_fly: w.flags & welcome_flags::FLIGHT != 0,
                });
            }
            Message::Reject(r) => {
                self.set_state(SessionState::Rejected {
                    reason: r.reason,
                    message: r.message,
                });
                self.stop_pings();
            }
            Message::HostStatus(state) => {
                self.stats.host_paused = state == HostState::Paused;
                self.changed = true;
            }
            Message::Pong(p) => {
                self.stats.rtt_ms = smooth(self.stats.rtt_ms, now - p.client_time_ms);
                self.stats.server_tick = self.stats.server_tick.max(p.server_tick);
                self.changed = true;
            }
            _ => {}
        }
    }

    pub fn on_datagram(&mut self, bytes: &[u8], now: f64) {
        match Message::decode(bytes) {
            Some(Message::PhysicsSnapshot(s)) => {
                self.stats.server_tick = self.stats.server_tick.max(s.server_tick);
                if self.joined() {
                    self.game
                        .push_back((Message::PhysicsSnapshot(s), bytes.to_vec()));
                }
            }
            Some(Message::DatagramPong(p)) => {
                self.stats.datagram_rtt_ms =
                    smooth(self.stats.datagram_rtt_ms, now - p.client_time_ms);
                self.stats.server_tick = self.stats.server_tick.max(p.server_tick);
                self.changed = true;
            }
            _ => {} // unreliable traffic: drop malformed datagrams
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};

    fn sent(out: &[Out]) -> Vec<Message> {
        out.iter()
            .filter_map(|o| match o {
                Out::Reliable(b) | Out::Datagram(b) => Message::decode(b),
                _ => None,
            })
            .collect()
    }

    fn welcome(registry_hash: u64) -> Message {
        Message::Welcome(Welcome {
            player_id: 3,
            world_seed: 5,
            generator_version: 0,
            server_tick: 10,
            verification_chunk: [0, 2, 0],
            flags: welcome_flags::FLIGHT,
            registry_hash,
        })
    }

    fn setup() -> (SigningKey, Session) {
        let key = SigningKey::from_bytes(&[4; 32]);
        let mut s = Session::new(
            [7; 32],
            key.verifying_key().to_bytes(),
            "Tester",
            "test",
            60_000.0,
        );
        s.start(1000.0);
        (key, s)
    }

    #[test]
    fn signs_the_challenge_bound_to_the_transport_and_joins() {
        let (key, mut s) = setup();
        assert!(
            sent(&s.take_out())
                .iter()
                .any(|m| matches!(m, Message::ClientHello(_)))
        );
        let nonce = [9u8; 32];
        s.on_reliable(
            Channel::Control,
            &Message::Challenge(nonce).encode(),
            1000.0,
        );
        let Some(Out::Sign(transcript)) = s.take_out().pop() else {
            panic!("asked to sign")
        };
        assert_eq!(
            transcript,
            auth_transcript(&nonce, &[7; 32], &key.verifying_key().to_bytes())
        );
        s.signed(key.sign(&transcript).to_bytes());
        let Some(Message::ClientAuth(sig)) = sent(&s.take_out()).pop() else {
            panic!("ClientAuth")
        };
        let vk = VerifyingKey::from_bytes(&key.verifying_key().to_bytes()).unwrap();
        assert!(
            vk.verify(&transcript, &ed25519_dalek::Signature::from_bytes(&sig))
                .is_ok()
        );

        s.on_reliable(Channel::Control, &welcome(REGISTRY_HASH).encode(), 1000.0);
        assert_eq!(
            s.state(),
            &SessionState::Joined {
                player_id: 3,
                world_seed: 5,
                generator_version: 0,
                verification_chunk: [0, 2, 0],
                may_fly: true
            }
        );
        // World messages wait for the game, in order.
        s.on_reliable(
            Channel::World,
            &Message::ChunkUnload(ChunkUnload {
                coords: vec![[1, 2, 3]],
            })
            .encode(),
            1000.0,
        );
        let chunk = ChunkData {
            form: ChunkForm::Generated,
            coord: [4, 5, 6],
            revision: 0,
            voxels: Vec::new(),
        };
        s.on_reliable(Channel::World, &Message::ChunkData(chunk).encode(), 1000.0);
        let got: Vec<MessageType> = std::iter::from_fn(|| s.next_game())
            .map(|(m, _)| m.message_type())
            .collect();
        assert_eq!(got, vec![MessageType::ChunkUnload, MessageType::ChunkData]);
        s.send_control(&Message::WorldgenCheck(7));
        assert_eq!(sent(&s.take_out()).pop(), Some(Message::WorldgenCheck(7)));
    }

    #[test]
    fn rejects_a_server_whose_block_registry_differs() {
        let (_, mut s) = setup();
        s.on_reliable(
            Channel::Control,
            &welcome(REGISTRY_HASH ^ 1).encode(),
            1000.0,
        );
        assert!(matches!(
            s.state(),
            SessionState::Rejected {
                reason: RejectReason::ProtocolVersion,
                ..
            }
        ));
        assert!(s.take_out().contains(&Out::Close));
    }

    #[test]
    fn reports_rejections() {
        let (_, mut s) = setup();
        let reject = Message::Reject(Reject {
            reason: RejectReason::Full,
            message: "Full.".into(),
        });
        s.on_reliable(Channel::Control, &reject.encode(), 1000.0);
        assert_eq!(
            s.state(),
            &SessionState::Rejected {
                reason: RejectReason::Full,
                message: "Full.".into()
            }
        );
        s.on_close("closed");
        assert!(
            matches!(s.state(), SessionState::Rejected { .. }),
            "a rejection is kept over the close"
        );
    }

    #[test]
    fn measures_rtt_from_pongs_and_pings_on_its_interval() {
        let (_, mut s) = setup();
        let pong = Message::Pong(Pong {
            seq: 1,
            client_time_ms: 960.0,
            server_tick: 5,
            server_time_ms: 0.0,
        });
        s.on_reliable(Channel::Control, &pong.encode(), 1000.0);
        assert_eq!(s.stats().rtt_ms, Some(40.0));
        s.take_out();
        s.tick(30_000.0);
        assert!(s.take_out().is_empty());
        s.tick(61_000.0);
        assert!(
            sent(&s.take_out())
                .iter()
                .any(|m| matches!(m, Message::Ping(_)))
        );
    }

    #[test]
    fn malformed_control_messages_close_and_malformed_datagrams_are_dropped() {
        let (_, mut s) = setup();
        s.on_datagram(&[0xEE], 1000.0);
        assert_eq!(s.state(), &SessionState::Handshaking);
        s.on_reliable(Channel::Control, &[0xEE], 1000.0);
        assert!(matches!(s.state(), SessionState::Closed { .. }));
    }
}
