//! Local mode's integrated server (ARCHITECTURE.md §2.1): the server core with its world file,
//! run in a worker and fed the same protocol bytes as a network server. Pure Rust: the worker glue
//! (web/local_worker.rs) and the native tests drive it the same way.

use std::time::Duration;

use dwell_protocol::{Channel, HostState, TransportKind};
use dwell_sim::app_version::{APP_VERSION, world_version_error};
use dwell_sim::entropy::Entropy;
use dwell_sim::fixed_step::FixedStep;
use dwell_sim::server::{EditPolicy, OutKind, Outgoing, Server, ServerConfig};
use dwell_storage::{StorageBackend, WorldDb, WorldStore};

/// How often a local world saves (a dedicated server saves less often).
pub const LOCAL_AUTOSAVE_SECONDS: u32 = 5;

/// The local player's session; friend-world guests get others.
pub const LOCAL_SESSION: u32 = 1;

#[derive(Debug, PartialEq, Eq)]
pub enum LocalError {
    /// The world file is locked to another version (RELEASES.md §6): no world was started.
    VersionLocked(String),
}

pub struct LocalCore {
    server: Server,
    fixed: FixedStep,
    /// The world is saved to a file (§6.4).
    pub persisted: bool,
    /// Why it is not, when storage was given.
    pub storage_error: String,
}

impl LocalCore {
    /// Starts the local server. With `storage` the world is loaded from and saved to it, and a
    /// saved world's seed and generator win over the arguments.
    pub fn create(
        world_seed: u64,
        generator_version: u32,
        storage: Option<impl StorageBackend>,
        entropy: Box<dyn Entropy>,
    ) -> Result<Self, LocalError> {
        let mut config = ServerConfig {
            world_seed,
            generator_version,
            name: "Local world".into(),
            max_players: 1,
            // Single-threaded in the browser: generation and LOD run on the tick within budgets.
            worldgen_threads: 0,
            lod_threads: Some(0),
            // A browser tab can close at any moment: save often (and on hide or close, §6.4).
            autosave_seconds: LOCAL_AUTOSAVE_SECONDS,
            ..ServerConfig::default()
        };
        let (mut persisted, mut storage_error) = (false, String::new());
        if let Some(backend) = storage {
            match WorldDb::open_backend(backend) {
                Ok(db) => {
                    if let Some(meta) = db.load_meta() {
                        let locked = world_version_error(APP_VERSION, &meta.app_version_last);
                        if !locked.is_empty() {
                            return Err(LocalError::VersionLocked(locked));
                        }
                    }
                    config.store = Some(WorldStore::new(db, false));
                    persisted = true;
                }
                Err(e) => storage_error = e,
            }
        }
        Ok(Self {
            server: Server::new(config, entropy),
            fixed: FixedStep::new(dwell_protocol::SIM_HZ),
            persisted,
            storage_error,
        })
    }

    pub fn connected(&mut self, session: u32, kind: TransportKind, binding: [u8; 32]) {
        self.server.on_connected(session, kind, binding);
    }
    pub fn disconnected(&mut self, session: u32) {
        self.server.on_disconnected(session);
    }
    pub fn reliable(&mut self, session: u32, channel: Channel, bytes: &[u8]) {
        self.server.on_reliable(session, channel, bytes);
    }
    pub fn datagram(&mut self, session: u32, bytes: &[u8]) {
        self.server.on_datagram(session, bytes);
    }

    /// Starts hosting (§10.2): up to `max_players` players (the host included), the edit and
    /// flight policies, and the host's device key as an op.
    pub fn host(
        &mut self,
        max_players: u16,
        edits: EditPolicy,
        flight: EditPolicy,
        host_key: [u8; 32],
    ) {
        self.server
            .set_hosting(max_players, edits, flight, host_key);
    }
    /// Tells the guests the host paused or resumed.
    pub fn host_status(&mut self, state: HostState) {
        self.server.broadcast_host_status(state);
    }
    /// Stops hosting: guests get Reject(ServerClosing); the host keeps playing.
    pub fn close_guests(&mut self) {
        self.server
            .close_sessions("The host stopped hosting.", true);
    }

    /// Saves the world now (the page is closing). True if the save committed.
    pub fn save(&mut self) -> bool {
        if !self.persisted {
            return false;
        }
        let failed = self.server.save_stats().failed;
        self.server.save_now();
        self.server.flush_store();
        self.server.save_stats().failed == failed
    }

    /// Runs the simulation steps due for `elapsed_seconds` of real time; returns the tick.
    pub fn advance(&mut self, elapsed_seconds: f64) -> u32 {
        for _ in 0..self.fixed.advance(elapsed_seconds) {
            self.server.step();
        }
        self.server.tick()
    }

    pub fn take_outbox(&mut self) -> Vec<Outgoing> {
        self.server.take_outbox()
    }

    pub fn server(&mut self) -> &mut Server {
        &mut self.server
    }
}

/// Encodes an outbox for a worker message: count, then per message session (u32), kind (u8:
/// 0 reliable, 1 datagram, 2 close), channel (u8) and length-prefixed bytes (all little-endian).
pub fn encode_outbox(out: &[Outgoing]) -> Vec<u8> {
    let mut b = (out.len() as u32).to_le_bytes().to_vec();
    for o in out {
        let (kind, channel) = match o.kind {
            OutKind::Reliable(c) => (0u8, c as u8),
            OutKind::Datagram => (1, 0),
            OutKind::Close => (2, 0),
        };
        b.extend_from_slice(&o.session.to_le_bytes());
        b.extend_from_slice(&[kind, channel]);
        b.extend_from_slice(&(o.bytes.len() as u32).to_le_bytes());
        b.extend_from_slice(&o.bytes);
    }
    b
}

/// The app version that last saved a world file (its bytes), or None (not a world file, no meta).
pub fn world_file_version(bytes: Vec<u8>) -> Option<String> {
    let db = WorldDb::open_backend(dwell_storage::BytesFile::new(bytes)).ok()?;
    db.load_meta().map(|m| m.app_version_last)
}

/// A world file's bytes rewritten to say `version` last saved it (debug tools and the e2e suite: a
/// world "saved by another version", RELEASES.md §6). Errors when the bytes are not a world file.
pub fn with_world_file_version(bytes: Vec<u8>, version: &str) -> Result<Vec<u8>, String> {
    let file = dwell_storage::BytesFile::new(bytes);
    let db = WorldDb::open_backend(file.clone())?;
    if db.load_meta().is_none() {
        return Err("not a saved world (no meta)".into());
    }
    db.set_app_version_last(version)?;
    drop(db);
    Ok(file.bytes())
}

/// Wall-clock budget hint for hosts that cannot measure time (see dwell_sim::clock).
pub const TICK_BUDGET: Duration = Duration::from_millis(4);

#[cfg(test)]
mod tests {
    use super::*;

    use dwell_protocol::*;
    use dwell_sim::entropy::SeededEntropy;
    use ed25519_dalek::{Signer, SigningKey};

    /// A world "file" that outlives the core using it.
    type SharedBytes = dwell_storage::BytesFile;

    fn join(core: &mut LocalCore) -> u16 {
        let key = SigningKey::from_bytes(&[3; 32]);
        let public_key = key.verifying_key().to_bytes();
        core.connected(LOCAL_SESSION, TransportKind::Loopback, [0; 32]);
        let hello = ClientHello {
            protocol_version: PROTOCOL_VERSION,
            client_version: "t".into(),
            public_key,
            display_name: "Me".into(),
        };
        core.reliable(
            LOCAL_SESSION,
            Channel::Control,
            &Message::ClientHello(hello).encode(),
        );
        let nonce = core
            .take_outbox()
            .iter()
            .find_map(|o| match Message::decode(&o.bytes) {
                Some(Message::Challenge(n)) => Some(n),
                _ => None,
            });
        let sig = key
            .sign(&auth_transcript(
                &nonce.expect("challenge"),
                &[0; 32],
                &public_key,
            ))
            .to_bytes();
        core.reliable(
            LOCAL_SESSION,
            Channel::Control,
            &Message::ClientAuth(sig).encode(),
        );
        core.take_outbox()
            .iter()
            .find_map(|o| match Message::decode(&o.bytes) {
                Some(Message::Welcome(w)) => Some(w.player_id),
                _ => None,
            })
            .expect("welcome")
    }

    #[test]
    fn a_local_world_starts_admits_its_player_and_steps_in_real_time() {
        let mut core = LocalCore::create(
            0,
            dwell_world::GENERATOR_FLAT,
            None::<SharedBytes>,
            Box::new(SeededEntropy(1)),
        )
        .unwrap();
        assert!(!core.persisted);
        assert_eq!(join(&mut core), 1);
        // The worker advances every few milliseconds; a long gap is capped at 8 steps.
        let tick = (0..30).map(|_| core.advance(1.0 / 60.0)).last().unwrap();
        assert_eq!(tick, 30);
        assert_eq!(core.advance(10.0), 38);
        let out = core.take_outbox();
        assert!(
            out.iter().any(|o| o.kind == OutKind::Datagram),
            "snapshots flow"
        );
        // The outbox encoding round-trips its header.
        let bytes = encode_outbox(&out);
        assert_eq!(
            u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize,
            out.len()
        );
    }

    #[test]
    fn a_local_world_autosaves_every_few_seconds() {
        // Local mode saves often (a browser tab can close at any moment): within 6 s of play.
        let file = SharedBytes::default();
        let mut core = LocalCore::create(
            1,
            dwell_world::GENERATOR_FLAT,
            Some(file),
            Box::new(SeededEntropy(1)),
        )
        .unwrap();
        join(&mut core);
        let before = core.server().save_stats().saves;
        for _ in 0..6 * 60 {
            core.advance(1.0 / 60.0);
        }
        assert!(
            core.server().save_stats().saves > before,
            "no autosave within 6 s"
        );
    }

    #[test]
    fn a_world_files_version_is_read_and_rewritten_through_its_format() {
        let file = SharedBytes::default();
        let mut core = LocalCore::create(
            1,
            dwell_world::GENERATOR_FLAT,
            Some(file.clone()),
            Box::new(SeededEntropy(1)),
        )
        .unwrap();
        join(&mut core);
        core.advance(0.1);
        assert!(core.save());
        drop(core);
        assert_eq!(
            world_file_version(file.bytes()).as_deref(),
            Some(APP_VERSION)
        );
        let rewritten = with_world_file_version(file.bytes(), "9.9.9").unwrap();
        assert_eq!(
            world_file_version(rewritten.clone()).as_deref(),
            Some("9.9.9")
        );
        let refused = LocalCore::create(
            1,
            dwell_world::GENERATOR_FLAT,
            Some(SharedBytes::new(rewritten)),
            Box::new(SeededEntropy(1)),
        );
        assert!(matches!(refused, Err(LocalError::VersionLocked(m)) if m.contains("9.9.9")));
        // Bytes that are not a world file are refused, not rewritten.
        assert!(with_world_file_version(vec![0; 64], "9.9.9").is_err());
        assert_eq!(world_file_version(Vec::new()), None);
    }

    #[test]
    fn an_edited_world_survives_a_restart_and_another_line_is_refused() {
        let file = SharedBytes::default();
        let mut core = LocalCore::create(
            42,
            dwell_world::GENERATOR_FLAT,
            Some(file.clone()),
            Box::new(SeededEntropy(1)),
        )
        .unwrap();
        assert!(core.persisted);
        join(&mut core);
        core.advance(0.2);
        core.server()
            .world()
            .set_voxel(3, 5, 3, dwell_blocks::require("dwell:stone"));
        assert!(core.save());
        drop(core);
        // Reopened with other arguments: the saved seed wins.
        let mut again = LocalCore::create(
            7,
            dwell_world::GENERATOR_FLAT,
            Some(file.clone()),
            Box::new(SeededEntropy(2)),
        )
        .unwrap();
        assert_eq!(again.server().config().world_seed, 42);
        drop(again);
        // A world last saved by another compatibility line is refused, naming the version.
        let db = WorldDb::open_backend(file.clone()).unwrap();
        let mut meta = db.load_meta().unwrap();
        meta.app_version_last = "99.0.0".into();
        db.save(&dwell_storage::SaveBatch {
            meta: Some(meta),
            ..Default::default()
        })
        .unwrap();
        drop(db);
        let refused = LocalCore::create(
            42,
            dwell_world::GENERATOR_FLAT,
            Some(file),
            Box::new(SeededEntropy(3)),
        );
        assert!(matches!(refused, Err(LocalError::VersionLocked(m)) if m.contains("99.0.0")));
    }

    #[test]
    fn hosting_raises_the_player_limit_and_closing_keeps_the_host() {
        let mut core = LocalCore::create(
            0,
            dwell_world::GENERATOR_FLAT,
            None::<SharedBytes>,
            Box::new(SeededEntropy(1)),
        )
        .unwrap();
        join(&mut core);
        core.host(4, EditPolicy::Everyone, EditPolicy::Ops, [3; 32]);
        assert_eq!(core.server().config().max_players, 4);
        core.close_guests();
        assert_eq!(core.server().joined_players(), 1, "the loopback host stays");
    }
}
