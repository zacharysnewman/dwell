//! A Server with test clients that join through the real handshake (streaming, edit, LOD and
//! persistence suites). Messages are exchanged directly, without links.

#![allow(dead_code)]

use std::collections::BTreeMap;

use dwell_physics::{DVec3, Vec3};
use dwell_protocol::*;
use dwell_sim::entropy::Entropy;
use dwell_sim::server::{OutKind, Server, ServerConfig, SessionId};
use dwell_world::{ChunkCoord, GENERATOR_FLAT};
use ed25519_dalek::{Signer, SigningKey};

/// Nonces 1, 2, 3, … (deterministic handshakes).
pub struct CountingEntropy(pub u8);

impl Entropy for CountingEntropy {
    fn fill(&mut self, out: &mut [u8]) {
        for b in out {
            *b = self.0;
            self.0 = self.0.wrapping_add(1);
        }
    }
}

/// A device identity: an Ed25519 key pair from a repeated seed byte.
pub struct Client {
    pub key: SigningKey,
    pub public_key: PublicKey,
}

impl Client {
    pub fn new(seed_byte: u8) -> Self {
        let key = SigningKey::from_bytes(&[seed_byte; 32]);
        let public_key = key.verifying_key().to_bytes();
        Self { key, public_key }
    }
    pub fn sign(&self, message: &[u8]) -> Signature {
        self.key.sign(message).to_bytes()
    }
}

pub struct Fixture {
    pub server: Server,
    pub binding: [u8; 32],
}

impl Fixture {
    pub fn new(config: ServerConfig) -> Self {
        Self {
            server: Server::new(config, Box::new(CountingEntropy(1))),
            binding: [0; 32],
        }
    }

    pub fn send(&mut self, id: SessionId, m: Message) {
        self.server.on_reliable(id, Channel::Control, &m.encode());
    }

    /// Reliable messages per session since the last call (outbox drained).
    pub fn take_all(&mut self) -> BTreeMap<SessionId, Vec<Message>> {
        let mut out: BTreeMap<SessionId, Vec<Message>> = BTreeMap::new();
        for o in self.server.take_outbox() {
            if matches!(o.kind, OutKind::Reliable(_)) {
                out.entry(o.session)
                    .or_default()
                    .push(Message::decode(&o.bytes).expect("decodes"));
            }
        }
        out
    }

    /// Reliable messages to `id` since the last call (outbox drained), with their encoded sizes.
    pub fn take(&mut self, id: SessionId) -> Vec<(Message, usize)> {
        self.server
            .take_outbox()
            .into_iter()
            .filter(|o| o.session == id && matches!(o.kind, OutKind::Reliable(_)))
            .map(|o| (Message::decode(&o.bytes).expect("decodes"), o.bytes.len()))
            .collect()
    }

    pub fn join(&mut self, id: SessionId, who: &Client) -> Welcome {
        self.server
            .on_connected(id, TransportKind::WebTransport, self.binding);
        self.send(
            id,
            Message::ClientHello(ClientHello {
                protocol_version: PROTOCOL_VERSION,
                client_version: "test".into(),
                public_key: who.public_key,
                display_name: "Tester".into(),
            }),
        );
        let Message::Challenge(nonce) = self.take(id).remove(0).0 else {
            panic!("expected a challenge")
        };
        let signature = who.sign(&auth_transcript(&nonce, &self.binding, &who.public_key));
        self.send(id, Message::ClientAuth(signature));
        // The registry's parts come first (ADR 0038), then Welcome.
        let mut out = self.take(id).into_iter().map(|(m, _)| m);
        let mut registry = Vec::new();
        loop {
            match out.next() {
                Some(Message::BlockRegistry(b)) => registry.extend_from_slice(&b.data),
                Some(Message::Welcome(w)) => {
                    assert_eq!(
                        w.registry_hash,
                        dwell_blocks::sync::definitions_hash(&registry),
                        "Welcome carries the hash of the registry sent before it"
                    );
                    return w;
                }
                other => panic!("expected BlockRegistry or Welcome, got {other:?}"),
            }
        }
    }

    /// Chunk messages over `ticks` ticks (and the unloads, when asked).
    pub fn chunks(
        &mut self,
        id: SessionId,
        ticks: i32,
        mut unloads: Option<&mut Vec<ChunkUnload>>,
    ) -> Vec<ChunkData> {
        let mut out = Vec::new();
        for _ in 0..ticks {
            self.server.step();
            for (m, _) in self.take(id) {
                match m {
                    Message::ChunkData(c) => out.push(c),
                    Message::ChunkUnload(u) => {
                        if let Some(u2) = unloads.as_deref_mut() {
                            u2.push(u);
                        }
                    }
                    _ => {}
                }
            }
        }
        out
    }

    /// Puts a joined player's feet at (x, feet_y, z), at rest.
    pub fn move_to(&mut self, player_id: u16, x: f64, feet_y: f64, z: f64) {
        let h = self
            .server
            .player_handle_of(player_id)
            .expect("a live player");
        let (_, physics, players) = self.server.sim();
        let half = f64::from(players.half_height(h));
        players.teleport(physics, h, DVec3::new(x, feet_y + half, z), Vec3::ZERO);
    }
}

/// The flat world with the spawn at (0.5, 0, 0.5).
pub fn flat() -> ServerConfig {
    ServerConfig {
        generator_version: GENERATOR_FLAT,
        spawn: Some([0.5, 0.0, 0.5]),
        ..ServerConfig::default()
    }
}

pub fn coord_of(m: &ChunkData) -> ChunkCoord {
    ChunkCoord::from_array(m.coord)
}
