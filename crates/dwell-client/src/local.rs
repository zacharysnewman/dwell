//! Local mode's integrated server (ARCHITECTURE.md §2.1): the server core with its world file,
//! run in a worker and fed the same protocol bytes as a network server. Pure Rust: the worker glue
//! (web/local_worker.rs) and the native tests drive it the same way.

use std::time::Duration;

use dwell_protocol::{Channel, HostState, TransportKind};
use dwell_sim::app_version::{APP_VERSION, world_open_error};
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
    /// The world's terrain for searches over its map (`find_on_map`), made on the first one.
    map: Option<Option<dwell_worldgen::BifacialTerrain>>,
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
        Self::create_on(
            dwell_world::Face::A,
            world_seed,
            generator_version,
            storage,
            entropy,
        )
    }

    /// [`LocalCore::create`] with a new world's spawn on `face` (the dev link `?spawn=b`: face B's
    /// spawn point, to play and check face B; a saved world keeps its own spawn).
    pub fn create_on(
        face: dwell_world::Face,
        world_seed: u64,
        generator_version: u32,
        storage: Option<impl StorageBackend>,
        entropy: Box<dyn Entropy>,
    ) -> Result<Self, LocalError> {
        Self::create_spawning(
            face,
            false,
            None,
            false,
            false,
            world_seed,
            generator_version,
            storage,
            entropy,
            dwell_sim::water::host::Placement::Inline,
            false,
        )
    }

    /// [`LocalCore::create_on`], with `portal` by the opening of the portal nearest the centre on
    /// `face` (the dev link `?spawn=portal`, `portal-b`; BIFACIAL_WORLD.md §6), with `at` a spawn
    /// at that world point (feet, m; the dev link `?at=x,y,z`, its player starting there however
    /// the world was left), and with `at_spawn` a saved world's player starting at that spawn, not
    /// where it left (a spawn link). With `upgrade` (the player confirmed the warning, owner
    /// 2026-11-06) a world file from an older version line opens too. `water`: where the water
    /// grid runs (ADR 0062; the browser's water worker is `Remote`, its messages carried by the
    /// worker script: `take_water_outgoing`, `deliver_water`). With `jobs_worker` the chunks to
    /// generate and the LOD's sections are done in the server-jobs worker (`Server::take_job`,
    /// `deliver_job`), not on the tick.
    #[allow(clippy::too_many_arguments)]
    pub fn create_spawning(
        face: dwell_world::Face,
        portal: bool,
        at: Option<[f64; 3]>,
        at_spawn: bool,
        upgrade: bool,
        world_seed: u64,
        generator_version: u32,
        storage: Option<impl StorageBackend>,
        entropy: Box<dyn Entropy>,
        water: dwell_sim::water::host::Placement,
        jobs_worker: bool,
    ) -> Result<Self, LocalError> {
        Self::create_tuned(
            face,
            portal,
            at,
            at_spawn,
            upgrade,
            world_seed,
            generator_version,
            storage,
            entropy,
            water,
            jobs_worker,
            |_| {},
        )
    }

    /// [`LocalCore::create_spawning`] with `tune` given the server's configuration last: the
    /// native harness (`dwell-native`, docs/DEV_TOOLS.md "Native harness") gives generation and
    /// the LOD native threads where the browser has its server-jobs worker.
    #[allow(clippy::too_many_arguments)]
    pub fn create_tuned(
        face: dwell_world::Face,
        portal: bool,
        at: Option<[f64; 3]>,
        at_spawn: bool,
        upgrade: bool,
        world_seed: u64,
        generator_version: u32,
        storage: Option<impl StorageBackend>,
        entropy: Box<dyn Entropy>,
        water: dwell_sim::water::host::Placement,
        jobs_worker: bool,
        tune: impl FnOnce(&mut ServerConfig),
    ) -> Result<Self, LocalError> {
        // The local player (player 1) lands on the point itself: its spawn spread taken back.
        let at_portal = portal
            .then(|| dwell_worldgen::portal_spawn_for(generator_version, world_seed, face))
            .flatten()
            .map(|[x, y, z]| {
                let [dx, dz] = dwell_sim::server::spawn_spread(1);
                [x - dx, y, z - dz]
            });
        // A point given: the local player's feet there (its spawn spread taken back too).
        let at_point = at.map(|[x, y, z]| {
            let [dx, dz] = dwell_sim::server::spawn_spread(1);
            [x - dx, y, z - dz]
        });
        let mut config = ServerConfig {
            spawn: at_point.or(at_portal).or_else(|| {
                (face == dwell_world::Face::B)
                    .then(|| dwell_worldgen::spawn_point_b_for(generator_version, world_seed))
            }),
            // A spawn link on a saved world: its player starts at the spawn (ServerConfig).
            join_at_spawn: at_spawn || at.is_some(),
            world_seed,
            generator_version,
            name: "Local world".into(),
            max_players: 1,
            // Single-threaded in the browser: generation and LOD run on the tick within budgets.
            worldgen_threads: 0,
            lod_threads: Some(0),
            jobs_remote: jobs_worker,
            // A browser tab can close at any moment: save often (and on hide or close, §6.4).
            autosave_seconds: LOCAL_AUTOSAVE_SECONDS,
            water,
            ..ServerConfig::default()
        };
        tune(&mut config);
        let (mut persisted, mut storage_error) = (false, String::new());
        if let Some(backend) = storage {
            match WorldDb::open_backend(backend) {
                Ok(db) => {
                    if let Some(meta) = db.load_meta() {
                        let locked = world_open_error(APP_VERSION, &meta.app_version_last, upgrade);
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
            map: None,
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
    /// The host changes the world's game mode (ADR 0065): at once for everyone in it, saved with
    /// the world. False for a name not in `shared/modes/modes.json`.
    pub fn set_mode(&mut self, name: &str) -> bool {
        self.server.set_mode(name)
    }
    /// The world's game mode's name.
    pub fn mode(&self) -> &'static str {
        self.server.world_settings().mode
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

    /// Kills the local player (the bench's `die` step; dev tools).
    pub fn kill_local_player(&mut self) {
        if let Some(p) = self.server.player_of_session(LOCAL_SESSION) {
            self.server.kill_player(p);
        }
    }

    /// Moves the local player's feet to world point `feet` (m), at rest (the dev tools' `goto`,
    /// docs/DEV_TOOLS.md): the server moves the body, the client's prediction snaps to it and the
    /// chunks stream around it. Only on the face the player is on (crossing has its own path,
    /// BIFACIAL_WORLD.md §6). False without a living local player or across the faces.
    pub fn teleport_local_player(&mut self, feet: [f64; 3]) -> bool {
        let Some(p) = self.server.player_of_session(LOCAL_SESSION) else {
            return false;
        };
        let (Some(h), Some(now)) = (self.server.player_handle_of(p), self.server.feet_of(p)) else {
            return false;
        };
        let f = dwell_world::FaceFrame::at_height(feet[1]);
        if dwell_world::FaceFrame::at_height(now[1]) != f || !feet.iter().all(|v| v.is_finite()) {
            return false;
        }
        let (_, physics, players) = self.server.sim();
        let half = f64::from(players.half_height(h));
        let centre = [feet[0], f.y(f.y(feet[1]) + half), feet[2]];
        players.teleport(
            physics,
            h,
            dwell_physics::DVec3::from_array(centre),
            dwell_physics::Vec3::ZERO,
        );
        true
    }

    /// The nearest water of `kind` (`water` any, `sea`, `river`, `lake`) on the world map (the
    /// generator's columns, loaded or not) from world point `from` (m), within `radius_m`, on
    /// `face` (None: `from`'s), at least `min_depth_m` deep (the map's level over its ground; a
    /// sea's first column off the beach is a few centimetres): JSON `{"at":[x,y,z],"kind","face","ground","distance"}` with `at`
    /// its column's centre at the water's level (world m), or `null`; an unknown kind is an error
    /// (`{"error":…}`). For the dev tools' `goto` (docs/DEV_TOOLS.md): a coarse search to move the
    /// player near, then a search of the cells (`find`).
    pub fn find_on_map(
        &mut self,
        kind: &str,
        from: [f64; 3],
        radius_m: f64,
        face: Option<dwell_world::FaceFrame>,
        min_depth_m: f64,
    ) -> String {
        use dwell_worldgen::terrain::Water;
        let want: &[Water] = match kind {
            "water" => &[Water::Sea, Water::River, Water::Lake],
            "sea" => &[Water::Sea],
            "river" => &[Water::River],
            "lake" => &[Water::Lake],
            _ => {
                return format!(
                    "{{\"error\":\"unknown water kind {kind:?} (water, sea, river or lake)\"}}"
                );
            }
        };
        let (version, seed) = {
            let c = self.server.config();
            (c.generator_version, c.world_seed)
        };
        let terrain = self
            .map
            .get_or_insert_with(|| dwell_worldgen::BifacialTerrain::for_generator(version, seed));
        let Some(terrain) = terrain else {
            return "{\"error\":\"this world has no map (not the terrain generator)\"}".into();
        };
        let f = face.unwrap_or_else(|| dwell_world::FaceFrame::at_height(from[1]));
        let local = terrain.local(f.face);
        let hit = crate::find::nearest_column([from[0], from[2]], radius_m, |x, z| {
            let c = local.column_at(x, z);
            (!c.outside && want.contains(&c.water) && f64::from(c.level - c.height) >= min_depth_m)
                .then_some(c)
        });
        let Some(([x, z], c)) = hit else {
            return "null".into();
        };
        let name = match c.water {
            Water::Sea => "sea",
            Water::River => "river",
            Water::Lake => "lake",
            Water::None => "none",
        };
        let (cx, cz) = (f64::from(x) + 0.5, f64::from(z) + 0.5);
        let distance = (cx - from[0]).hypot(cz - from[2]);
        format!(
            "{{\"at\":[{cx},{},{cz}],\"kind\":\"{name}\",\"face\":\"{}\",\"ground\":{},\"distance\":{distance:.1}}}",
            f.y(f64::from(c.level)),
            if f.is_b() { "b" } else { "a" },
            f.y(f64::from(c.height)),
        )
    }

    /// The respawn preload (ARCHITECTURE.md §6.3) on (its radius from `shared/look/loading.json`)
    /// or off (`?respawnPreload=off`, the bench's comparison).
    pub fn set_respawn_preload(&mut self, on: bool) {
        self.server
            .set_respawn_preload(dwell_sim::server::RESPAWN_PRELOAD_RADIUS_CHUNKS.filter(|_| on));
    }

    /// The water near cell `at` as the local server has it (a diagnostic, `Water::report`), with
    /// the last block edits (`edits`, `Server::edit_log_json`) so a dump can be replayed.
    /// None while a remote water's report is on its way (`take_water_report`).
    pub fn water_report(&mut self, at: [i32; 3], radius: i32) -> Option<String> {
        let water = self.server.water_report(at, radius)?;
        Some(self.with_edits(water))
    }

    /// A remote water's report come back (with the edits, as `water_report`).
    pub fn take_water_report(&mut self) -> Option<String> {
        let water = self.server.take_water_report()?;
        Some(self.with_edits(water))
    }

    fn with_edits(&self, water: String) -> String {
        let edits = self.server.edit_log_json();
        match water.strip_suffix('}') {
            Some(body) => format!("{body},\"edits\":{edits}}}"),
            None => water,
        }
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
