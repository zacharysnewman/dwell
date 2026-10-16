//! Client side of the join handshake and connection health (ARCHITECTURE.md §8.3, ADR 0004), as a
//! poll-driven state machine: the host feeds it transport events and the time, and drains what to
//! send and the game messages. Signing is the host's (the device key is a non-extractable WebCrypto
//! key): on a Challenge the session asks for a signature and resumes with [`Session::signed`].

use std::collections::VecDeque;

use dwell_blocks::sync::{Differences, Mapping, Payload, definitions_hash, map};
use dwell_protocol::*;

const RTT_SMOOTHING: f64 = 0.2;

fn smooth(prev: Option<f64>, sample: f64) -> Option<f64> {
    Some(prev.map_or(sample, |p| p + (sample - p) * RTT_SMOOTHING))
}

#[derive(Clone, Debug, PartialEq)]
pub enum SessionState {
    Handshaking,
    /// The server's block registry differs from this build's (ADR 0036): waiting for the player to
    /// continue ([`Session::accept_registry`]) or cancel ([`Session::cancel_registry`]).
    Confirming {
        differences: Differences,
        /// The definitions hash ("don't ask again" is keyed by it).
        hash: u64,
    },
    Joined {
        player_id: u16,
        world_seed: u64,
        generator_version: u32,
        verification_chunk: [i32; 3],
        /// The server lets this player use creative flight (§8.3).
        may_fly: bool,
        /// The server takes `RegionEdit`s (`welcome_flags::REGION_EDITS`; builds before them
        /// reject an unknown message, so the client sends none without it).
        region_edits: bool,
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
    /// `BlockRegistry` parts received so far, and how many the server announced.
    registry_parts: Vec<u8>,
    registry_next: u16,
    registry_parts_total: u16,
    /// The complete payload, once every part arrived.
    registry: Option<Vec<u8>>,
    /// The server's ids ↔ this client's (ADR 0036), from Welcome on.
    mapping: Option<Mapping>,
    /// The Welcome waiting on the player's answer (Confirming).
    pending: Option<Welcome>,
    /// A definitions hash the player said not to ask about again for this server.
    trusted_registry: Option<u64>,
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
            registry_parts: Vec::new(),
            registry_next: 0,
            registry_parts_total: 0,
            registry: None,
            mapping: None,
            pending: None,
            trusted_registry: None,
        }
    }

    /// Joins without asking when the server's definitions hash is `hash` ("don't ask again for
    /// this server", remembered by the shell per server and hash).
    pub fn trust_registry(&mut self, hash: Option<u64>) {
        self.trusted_registry = hash;
    }

    /// The id translation, once Welcome has arrived.
    pub fn mapping(&self) -> Option<&Mapping> {
        self.mapping.as_ref()
    }

    /// The player chose Continue in the join prompt.
    pub fn accept_registry(&mut self) {
        if matches!(self.state, SessionState::Confirming { .. })
            && let Some(w) = self.pending.take()
        {
            self.join(w);
        }
    }

    /// The player chose Cancel in the join prompt: the connection closes.
    pub fn cancel_registry(&mut self) {
        if matches!(self.state, SessionState::Confirming { .. }) {
            self.pending = None;
            self.set_state(SessionState::Closed {
                message: "Cancelled: this server's blocks differ from this version's.".into(),
            });
            self.stop_pings();
            self.close();
        }
    }

    fn join(&mut self, w: Welcome) {
        self.stats.server_tick = w.server_tick;
        self.set_state(SessionState::Joined {
            player_id: w.player_id,
            world_seed: w.world_seed,
            generator_version: w.generator_version,
            verification_chunk: w.verification_chunk,
            may_fly: w.flags & welcome_flags::FLIGHT != 0,
            region_edits: w.flags & welcome_flags::REGION_EDITS != 0,
        });
    }

    fn fail(&mut self, message: String) {
        self.set_state(SessionState::Closed { message });
        self.stop_pings();
        self.close();
    }

    /// A `BlockRegistry` part; parts arrive in order on the control stream.
    fn on_registry_part(&mut self, b: BlockRegistry) {
        if b.part != self.registry_next
            || (b.part > 0 && b.parts != self.registry_parts_total)
            || self.registry.is_some()
        {
            self.fail("The server sent its block registry out of order.".into());
            return;
        }
        self.registry_parts_total = b.parts;
        self.registry_parts.extend_from_slice(&b.data);
        self.registry_next += 1;
        if self.registry_next == b.parts {
            self.registry = Some(std::mem::take(&mut self.registry_parts));
        }
    }

    /// Welcome: map the server's registry, then join, or ask the player first.
    fn on_welcome(&mut self, w: Welcome) {
        let Some(bytes) = self.registry.take() else {
            self.fail("The server sent no block registry.".into());
            return;
        };
        let hash = definitions_hash(&bytes);
        let mapping = match Payload::decode(&bytes).and_then(|p| map(&p, hash)) {
            Ok(m) => m,
            Err(e) => {
                self.fail(format!("The server's block registry can't be used: {e}"));
                return;
            }
        };
        let differences = mapping.differences.clone();
        self.mapping = Some(mapping);
        if differences.is_empty() || self.trusted_registry == Some(hash) {
            self.join(w);
        } else {
            self.pending = Some(w);
            self.set_state(SessionState::Confirming { differences, hash });
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
            ) && (self.joined() || matches!(self.state, SessionState::Confirming { .. }))
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
            Message::BlockRegistry(b) if self.state == SessionState::Handshaking => {
                self.on_registry_part(b);
            }
            Message::Welcome(w) if self.state == SessionState::Handshaking => self.on_welcome(w),
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

    /// Sends a registry payload as `BlockRegistry` parts of at most `part` bytes.
    fn send_registry(s: &mut Session, payload: &[u8], part: usize) {
        let parts: Vec<&[u8]> = payload.chunks(part).collect();
        for (i, data) in parts.iter().enumerate() {
            let m = Message::BlockRegistry(BlockRegistry {
                part: i as u16,
                parts: parts.len() as u16,
                data: data.to_vec(),
            });
            s.on_reliable(Channel::Control, &m.encode(), 1000.0);
        }
    }

    fn build_registry() -> Vec<u8> {
        Payload::served().encode()
    }

    /// This build's registry with sand's density changed and a new block.
    fn modded_registry() -> Vec<u8> {
        let mut p = Payload::served();
        let sand = p.blocks.iter_mut().find(|b| b.name == "dwell:sand").unwrap();
        for f in &mut sand.states[0] {
            if f.0 == dwell_blocks::sync::tag::DENSITY {
                f.1 = 5.0f32.to_le_bytes().to_vec();
            }
        }
        let mut glow = p.blocks.iter().find(|b| b.name == "dwell:stone").unwrap().clone();
        glow.name = "test:session_glow".into();
        glow.first_state = p.id_end() as u16;
        p.blocks.push(glow);
        p.encode()
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

        send_registry(&mut s, &build_registry(), 100);
        s.on_reliable(Channel::Control, &welcome(0).encode(), 1000.0);
        assert!(s.mapping().is_some_and(|m| m.identity));
        assert_eq!(
            s.state(),
            &SessionState::Joined {
                player_id: 3,
                world_seed: 5,
                generator_version: 0,
                verification_chunk: [0, 2, 0],
                may_fly: true,
                region_edits: false,
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
    fn a_different_registry_asks_and_continue_joins_with_the_servers_rules() {
        let (_, mut s) = setup();
        let payload = modded_registry();
        send_registry(&mut s, &payload, 64);
        s.on_reliable(Channel::Control, &welcome(0).encode(), 1000.0);
        let SessionState::Confirming { differences, hash } = s.state().clone() else {
            panic!("{:?}", s.state())
        };
        assert_eq!(hash, definitions_hash(&payload));
        assert_eq!(differences.different[0].name, "dwell:sand");
        assert_eq!(differences.added[0].name, "test:session_glow");
        assert!(s.next_game().is_none(), "nothing reaches the game while asking");
        s.accept_registry();
        assert!(matches!(s.state(), SessionState::Joined { .. }));
        let m = s.mapping().expect("mapping");
        assert!(!m.generated_ok);
    }

    #[test]
    fn cancel_closes_the_connection() {
        let (_, mut s) = setup();
        send_registry(&mut s, &modded_registry(), 1 << 16);
        s.on_reliable(Channel::Control, &welcome(0).encode(), 1000.0);
        s.take_out();
        s.cancel_registry();
        assert!(matches!(s.state(), SessionState::Closed { .. }));
        assert!(s.take_out().contains(&Out::Close));
    }

    #[test]
    fn dont_ask_again_skips_the_prompt_only_for_the_same_hash() {
        let payload = modded_registry();
        for (trusted, asks) in [
            (Some(definitions_hash(&payload)), false),
            (Some(definitions_hash(&payload) ^ 1), true),
            (None, true),
        ] {
            let (_, mut s) = setup();
            s.trust_registry(trusted);
            send_registry(&mut s, &payload, 1 << 16);
            s.on_reliable(Channel::Control, &welcome(0).encode(), 1000.0);
            assert_eq!(
                matches!(s.state(), SessionState::Confirming { .. }),
                asks,
                "{trusted:?}"
            );
            assert_eq!(!asks, matches!(s.state(), SessionState::Joined { .. }));
        }
    }

    #[test]
    fn a_missing_or_malformed_registry_closes() {
        let (_, mut s) = setup();
        s.on_reliable(Channel::Control, &welcome(0).encode(), 1000.0);
        assert!(matches!(s.state(), SessionState::Closed { .. }));
        let (_, mut s) = setup();
        send_registry(&mut s, &[1, 0, 0], 16);
        s.on_reliable(Channel::Control, &welcome(0).encode(), 1000.0);
        assert!(matches!(s.state(), SessionState::Closed { .. }));
    }

    /// Additions within a compatibility line are negotiated by Welcome flags (ARCHITECTURE.md
    /// §10.5): a flag this build doesn't know must not keep it from joining.
    #[test]
    fn reads_region_edits_from_the_welcome_and_ignores_unknown_flags() {
        let Message::Welcome(w) = welcome(0) else {
            unreachable!()
        };
        for (flags, region_edits) in [
            (welcome_flags::FLIGHT, false),
            (welcome_flags::REGION_EDITS | 0x80, true),
        ] {
            let (_, mut s) = setup();
            let m = Message::Welcome(Welcome { flags, ..w.clone() });
            send_registry(&mut s, &build_registry(), 1 << 16);
            s.on_reliable(Channel::Control, &m.encode(), 1000.0);
            let SessionState::Joined {
                region_edits: got, ..
            } = s.state()
            else {
                panic!("{:?}", s.state())
            };
            assert_eq!(*got, region_edits);
        }
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
