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
    /// The server's block registry differs from this build's (ADR 0038): waiting for the player to
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
        /// The server takes brush strokes in this world: `KEEP` paste cells and the world type's
        /// brush reach (`welcome_flags::BRUSHES`, BRUSHES.md).
        brushes: bool,
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
    /// Edits not sent because they named a state the server does not have (ADR 0038).
    pub edits_unmapped: u32,
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
    /// The server's ids ↔ this client's (ADR 0038), from Welcome on.
    mapping: Option<Mapping>,
    /// The Welcome waiting on the player's answer (Confirming).
    pending: Option<Welcome>,
    /// A definitions hash the player said not to ask about again for this server.
    trusted_registry: Option<u64>,
    /// The game's ids of the edits sent and not yet acknowledged, in the order sent: the server
    /// numbers them 1, 2, … in that order (`EditAck`, edit prediction).
    sent_edits: VecDeque<u32>,
    /// The server's number of the last edit acknowledged.
    edits_acked: u32,
    /// The game's ids of edits that were never sent (not joined, or naming a state the server
    /// lacks): refused here.
    dropped_edits: Vec<u32>,
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
            sent_edits: VecDeque::new(),
            edits_acked: 0,
            dropped_edits: Vec::new(),
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
            brushes: w.flags & welcome_flags::BRUSHES != 0,
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
        if !self.joined() {
            return;
        }
        match self.to_server(m) {
            Some(m) => self.send(&m),
            None => {
                self.stats.edits_unmapped += 1;
                self.changed = true;
            }
        }
    }

    /// Sends an edit (a `BlockEditRequest` or `RegionEdit`) the game knows by `id`, for its
    /// prediction: [`edit_results`](Self::edit_results) reports it when the server's `EditAck`
    /// covers it. An edit that can't be sent is refused at once
    /// ([`take_dropped_edits`](Self::take_dropped_edits)). True if it was sent.
    pub fn send_edit(&mut self, m: &Message, id: u32) -> bool {
        let mapped = self.joined().then(|| self.to_server(m)).flatten();
        match mapped {
            Some(m) => {
                self.send(&m);
                self.sent_edits.push_back(id);
                true
            }
            None => {
                if self.joined() {
                    self.stats.edits_unmapped += 1;
                    self.changed = true;
                }
                self.dropped_edits.push(id);
                false
            }
        }
    }

    /// The game's ids of the edits an `EditAck` covers, each with whether the server refused it,
    /// in the order they were sent. Call it as the ack comes out of [`next_game`](Self::next_game),
    /// after the modifications before it.
    pub fn edit_results(&mut self, ack: &EditAck) -> Vec<(u32, bool)> {
        let mut out = Vec::new();
        while self.edits_acked < ack.through {
            let Some(id) = self.sent_edits.pop_front() else {
                break;
            };
            self.edits_acked += 1;
            out.push((id, ack.rejected.binary_search(&self.edits_acked).is_ok()));
        }
        self.edits_acked = self.edits_acked.max(ack.through);
        out
    }

    /// The game's ids of edits refused without being sent.
    pub fn take_dropped_edits(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.dropped_edits)
    }

    /// An outgoing message with its state ids in the server's terms (ADR 0038); `None` if it
    /// names a state the server does not have (a client-only block), which is not sent.
    fn to_server(&self, m: &Message) -> Option<Message> {
        let Some(map) = self.mapping.as_ref().filter(|m| !m.identity) else {
            return Some(m.clone());
        };
        let id = |v: u16| map.server(v);
        Some(match m {
            Message::BlockEditRequest(e) if e.action == BlockEditAction::Place => {
                Message::BlockEditRequest(BlockEditRequest {
                    material: id(e.material)?,
                    ..e.clone()
                })
            }
            Message::RegionEdit(e) => {
                let mut e = e.clone();
                match e.op {
                    RegionEditOp::Fill | RegionEditOp::Hollow => e.material = id(e.material)?,
                    RegionEditOp::Replace => {
                        e.material = id(e.material)?;
                        e.from = id(e.from)?;
                    }
                    RegionEditOp::Paste => {
                        // Brushes' KEEP cells name no state.
                        for c in e
                            .cells
                            .iter_mut()
                            .filter(|c| usize::from(**c) != region_edit::KEEP)
                        {
                            *c = id(*c)?;
                        }
                    }
                }
                Message::RegionEdit(e)
            }
            other => other.clone(),
        })
    }

    /// An incoming message with its state ids in this client's terms; false if one is not in
    /// the server's registry (a malformed message).
    fn to_client(&self, m: &mut Message) -> bool {
        let Some(map) = self.mapping.as_ref() else {
            return true;
        };
        match m {
            Message::ChunkData(c) => map.to_client_ids(&mut c.voxels),
            Message::LodData(l) => map.to_client_ids(&mut l.cells),
            Message::EntitySpawn(e) => e.cells.iter_mut().all(|c| {
                let mut one = [c.state];
                let ok = map.to_client_ids(&mut one);
                c.state = one[0];
                ok
            }),
            Message::VoxelModification(v) => v.chunks.iter_mut().all(|c| {
                c.changes.iter_mut().all(|ch| {
                    let mut one = [ch.material];
                    let ok = map.to_client_ids(&mut one);
                    ch.material = one[0];
                    ok
                })
            }),
            _ => true,
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
        let Some(mut m) = Message::decode(bytes) else {
            self.malformed();
            return;
        };
        if !self.to_client(&mut m) {
            self.malformed();
            return;
        }
        if channel != Channel::Control {
            if matches!(
                m,
                Message::PlayerEvent(_)
                    | Message::ChunkData(_)
                    | Message::ChunkUnload(_)
                    | Message::VoxelModification(_)
                    | Message::EntitySpawn(_)
                    | Message::EntityDespawn(_)
                    | Message::EditAck(_)
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
        let sand = p
            .blocks
            .iter_mut()
            .find(|b| b.name == "dwell:sand")
            .unwrap();
        for f in &mut sand.states[0] {
            if f.0 == dwell_blocks::sync::tag::DENSITY {
                f.1 = 5.0f32.to_le_bytes().to_vec();
            }
        }
        let mut glow = p
            .blocks
            .iter()
            .find(|b| b.name == "dwell:stone")
            .unwrap()
            .clone();
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
                brushes: false,
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
        assert_eq!(differences.different[0].name, "dwell:sand[flooded=false]");
        assert_eq!(
            differences.added[0].name,
            "test:session_glow[flooded=false]"
        );
        assert!(
            s.next_game().is_none(),
            "nothing reaches the game while asking"
        );
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

    /// A server without `dwell:ladder` (its later ids shift down) whose sand differs.
    fn ladderless_registry() -> (Vec<u8>, Payload) {
        let mut p = Payload::served();
        let at = p
            .blocks
            .iter()
            .position(|b| b.name == "dwell:ladder")
            .unwrap();
        let removed = p.blocks.remove(at).states.len() as u16;
        for b in &mut p.blocks[at..] {
            b.first_state -= removed;
        }
        let sand = p
            .blocks
            .iter_mut()
            .find(|b| b.name == "dwell:sand")
            .unwrap();
        for f in &mut sand.states[0] {
            if f.0 == dwell_blocks::sync::tag::DENSITY {
                f.1 = 5.0f32.to_le_bytes().to_vec();
            }
        }
        (p.encode(), p)
    }

    fn server_id(p: &Payload, name: &str) -> u16 {
        p.states().find(|(_, n, _)| n == name).unwrap().0
    }

    fn joined_with(payload: &[u8]) -> Session {
        let (_, mut s) = setup();
        send_registry(&mut s, payload, 1 << 16);
        s.on_reliable(Channel::Control, &welcome(0).encode(), 1000.0);
        s.accept_registry();
        assert!(matches!(s.state(), SessionState::Joined { .. }));
        s.take_out();
        s
    }

    #[test]
    fn incoming_ids_map_to_the_clients_in_chunks_lod_and_modifications() {
        use dwell_blocks::{info, require};
        let (payload, p) = ladderless_registry();
        let mut s = joined_with(&payload);
        let (water, sand) = (
            server_id(&p, "dwell:water"),
            server_id(&p, "dwell:sand[flooded=false]"),
        );
        assert_ne!(water, require("dwell:water"), "shifted on the server");
        let mut voxels = vec![0u16; CHUNK_VOLUME];
        voxels[1] = water;
        voxels[2] = sand;
        let chunk = ChunkData {
            form: ChunkForm::Explicit,
            coord: [0, 0, 0],
            revision: 1,
            voxels,
        };
        s.on_reliable(Channel::World, &Message::ChunkData(chunk).encode(), 1000.0);
        let modification = Message::VoxelModification(VoxelModification {
            reason: VoxelModificationReason::Edit,
            server_tick: 1,
            chunks: vec![ChunkChanges {
                coord: [0, 0, 0],
                revision: 2,
                changes: vec![VoxelChange {
                    index: 5,
                    material: water,
                }],
            }],
        });
        s.on_reliable(Channel::World, &modification.encode(), 1000.0);
        let mut cells = vec![0u16; LOD_CELL_COUNT];
        cells[7] = sand;
        let lod = Message::LodData(LodData {
            form: LodForm::Explicit,
            level: 1,
            section: [0, 0, 0],
            revision: 1,
            cells,
        });
        s.on_reliable(Channel::Lod, &lod.encode(), 1000.0);
        let got: Vec<Message> = std::iter::from_fn(|| s.next_game())
            .map(|(m, _)| m)
            .collect();
        let Message::ChunkData(c) = &got[0] else {
            panic!()
        };
        assert_eq!(c.voxels[1], require("dwell:water"));
        assert_eq!(info(c.voxels[2]).name, "dwell:sand[flooded=false]");
        assert_eq!(info(c.voxels[2]).density_kg_m3, 5.0, "the server's sand");
        let Message::VoxelModification(v) = &got[1] else {
            panic!()
        };
        assert_eq!(v.chunks[0].changes[0].material, require("dwell:water"));
        let Message::LodData(l) = &got[2] else {
            panic!()
        };
        assert_eq!(l.cells[7], c.voxels[2]);
        // An id outside the server's registry is a malformed message.
        let bad = ChunkData {
            form: ChunkForm::Explicit,
            coord: [0, 0, 0],
            revision: 3,
            voxels: vec![p.id_end() as u16; CHUNK_VOLUME],
        };
        s.on_reliable(Channel::World, &Message::ChunkData(bad).encode(), 1000.0);
        assert!(matches!(s.state(), SessionState::Closed { .. }));
    }

    #[test]
    fn outgoing_edits_map_to_the_servers_ids_and_client_only_states_are_not_sent() {
        use dwell_blocks::require;
        let (payload, p) = ladderless_registry();
        let mut s = joined_with(&payload);
        let (water, sand) = (
            server_id(&p, "dwell:water"),
            server_id(&p, "dwell:sand[flooded=false]"),
        );
        let place = |material| {
            Message::BlockEditRequest(BlockEditRequest {
                action: BlockEditAction::Place,
                cell: [1, 2, 3],
                face: 2,
                material,
                rows: 1,
                down: false,
            })
        };
        s.send_control(&place(require("dwell:sand")));
        let Some(Message::BlockEditRequest(e)) = sent(&s.take_out()).pop() else {
            panic!("sent")
        };
        assert_eq!(e.material, sand, "the palette's sand is the server's sand");
        let synced_sand = s.mapping().unwrap().client(sand).unwrap();
        s.send_control(&place(synced_sand));
        let Some(Message::BlockEditRequest(e)) = sent(&s.take_out()).pop() else {
            panic!("sent")
        };
        assert_eq!(e.material, sand);
        let paste = Message::RegionEdit(RegionEdit {
            op: RegionEditOp::Paste,
            min: [0, 0, 0],
            size: [2, 1, 1],
            material: 0,
            from: 0,
            cells: vec![require("dwell:water"), require("dwell:sand")],
        });
        s.send_control(&paste);
        let Some(Message::RegionEdit(r)) = sent(&s.take_out()).pop() else {
            panic!("sent")
        };
        assert_eq!(r.cells, [water, sand]);
        let replace = Message::RegionEdit(RegionEdit {
            op: RegionEditOp::Replace,
            min: [0, 0, 0],
            size: [1, 1, 1],
            material: require("dwell:water"),
            from: require("dwell:stone"),
            cells: Vec::new(),
        });
        s.send_control(&replace);
        let Some(Message::RegionEdit(r)) = sent(&s.take_out()).pop() else {
            panic!("sent")
        };
        assert_eq!((r.from, r.material), (require("dwell:stone"), water));
        // A ladder: this client's only.
        s.send_control(&place(require("dwell:ladder")));
        assert!(sent(&s.take_out()).is_empty());
        assert_eq!(s.stats().edits_unmapped, 1);
        // A break names no state.
        s.send_control(&Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Break,
            cell: [1, 2, 3],
            face: 2,
            material: 0,
            rows: 1,
            down: false,
        }));
        assert_eq!(sent(&s.take_out()).len(), 1);
    }

    #[test]
    fn edit_acks_name_the_games_edits_in_the_order_sent_and_unsent_ones_are_refused() {
        use dwell_blocks::require;
        let (payload, _) = ladderless_registry();
        let mut s = joined_with(&payload);
        let place = |material| {
            Message::BlockEditRequest(BlockEditRequest {
                action: BlockEditAction::Place,
                cell: [1, 2, 3],
                face: 2,
                material,
                rows: 1,
                down: false,
            })
        };
        // The game's ids need not be in order (a stroke's queued edits were numbered first).
        assert!(s.send_edit(&place(require("dwell:sand")), 40));
        assert!(
            !s.send_edit(&place(require("dwell:ladder")), 41),
            "client-only: not sent"
        );
        assert!(s.send_edit(&place(require("dwell:stone")), 7));
        assert!(s.send_edit(&place(require("dwell:dirt")), 8));
        assert_eq!(sent(&s.take_out()).len(), 3);
        assert_eq!(s.take_dropped_edits(), vec![41]);
        // The acks reach the game through its message queue, in order with the modifications.
        s.on_reliable(
            Channel::World,
            &Message::EditAck(EditAck {
                through: 2,
                rejected: vec![2],
            })
            .encode(),
            1000.0,
        );
        let Some((Message::EditAck(a), _)) = s.next_game() else {
            panic!("queued for the game")
        };
        assert_eq!(s.edit_results(&a), vec![(40, false), (7, true)]);
        let a = EditAck {
            through: 3,
            rejected: vec![],
        };
        assert_eq!(s.edit_results(&a), vec![(8, false)]);
        assert!(s.edit_results(&a).is_empty(), "an ack again names nothing");
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
        for (flags, region_edits, brushes) in [
            (welcome_flags::FLIGHT, false, false),
            (welcome_flags::REGION_EDITS | 0x80, true, false),
            (
                welcome_flags::REGION_EDITS | welcome_flags::BRUSHES,
                true,
                true,
            ),
        ] {
            let (_, mut s) = setup();
            let m = Message::Welcome(Welcome { flags, ..w.clone() });
            send_registry(&mut s, &build_registry(), 1 << 16);
            s.on_reliable(Channel::Control, &m.encode(), 1000.0);
            let SessionState::Joined {
                region_edits: got,
                brushes: got_brushes,
                ..
            } = s.state()
            else {
                panic!("{:?}", s.state())
            };
            assert_eq!((*got, *got_brushes), (region_edits, brushes));
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
