//! The Rust client's game loop end to end, natively (Phase 24e): a local world's server core, the
//! client session (with the signed handshake), the game with its jobs run inline and a recording
//! scene. The player joins, the terrain streams in and is meshed, the player walks, breaks a block
//! and sees it gone (the e2e suite's local-mode movement and edit cases, without a browser).

use std::cell::RefCell;
use std::rc::Rc;

use dwell_client::game::{Game, Scene, Send};
use dwell_client::jobs::Runner;
use dwell_client::local::{LOCAL_SESSION, LocalCore};
use dwell_client::net::session::{Out, Session, SessionState};
use dwell_client::pool::{InlineJobs, Pool};
use dwell_client::render::display::RenderStats;
use dwell_client::render::{Line, PlayerView, StandIn};
use dwell_game::input::{EditAction, KeyboardMouseInput};
use dwell_mesh::{ChunkMeshes, SectionMeshes};
use dwell_physics::config::PlayerControllerConfig;
use dwell_protocol::{Channel, TransportKind};
use dwell_sim::entropy::SeededEntropy;
use dwell_sim::server::OutKind;
use dwell_storage::StorageBackend;
use dwell_world::{ChunkCoord, GENERATOR_TERRAIN};
use ed25519_dalek::{Signer, SigningKey};

#[derive(Default)]
struct Recorder {
    chunks: std::collections::HashSet<ChunkCoord>,
    sections: usize,
    camera: Option<[f64; 3]>,
    outline: Option<[f64; 3]>,
}

impl Scene for Recorder {
    fn set_terrain_chunk(&mut self, coord: ChunkCoord, meshes: Option<&ChunkMeshes>) {
        if meshes.is_some() {
            self.chunks.insert(coord);
        } else {
            self.chunks.remove(&coord);
        }
    }
    fn set_lod_section(
        &mut self,
        _id: i64,
        _origin: [f64; 3],
        _cell: f64,
        meshes: Option<&SectionMeshes>,
    ) {
        self.sections += usize::from(meshes.is_some());
    }
    fn show_lod_sections(&mut self, _visible: &[(i64, u8)]) {}
    fn show_lod_stand_ins(&mut self, _stand_ins: &[StandIn]) {}
    fn set_player(&mut self, _id: u32, _view: Option<PlayerView>) {}
    fn set_camera(&mut self, eye: [f64; 3], _yaw: f64, _pitch: f64, _face: f64, _flip: f64) {
        self.camera = Some(eye);
    }
    fn set_block_outline(&mut self, cell: Option<[f64; 3]>, _height: f64) {
        self.outline = cell;
    }
    fn set_debug_lines(&mut self, _lines: Option<&[Line]>) {}
    fn stats(&self) -> RenderStats {
        RenderStats::default()
    }
}

struct Rig {
    core: LocalCore,
    session: Session,
    key: SigningKey,
    game: Option<Game<Recorder>>,
    scene: Rc<RefCell<Recorder>>,
    input: KeyboardMouseInput,
    now: f64,
}

const SEED: u64 = 1;

impl Rig {
    fn new() -> Self {
        let mut core = LocalCore::create(
            SEED,
            GENERATOR_TERRAIN,
            None::<NoStorage>,
            Box::new(SeededEntropy(1)),
        )
        .unwrap();
        core.connected(LOCAL_SESSION, TransportKind::Loopback, [0; 32]);
        let key = SigningKey::from_bytes(&[5; 32]);
        let mut session = Session::new([0; 32], key.verifying_key().to_bytes(), "Me", "t", 1000.0);
        session.start(0.0);
        Self {
            core,
            session,
            key,
            game: None,
            scene: Rc::default(),
            input: KeyboardMouseInput::new(),
            now: 0.0,
        }
    }

    /// One 60 Hz frame: the server steps, messages flow both ways, the game runs a frame.
    fn frame(&mut self) {
        self.now += 1000.0 / 60.0;
        let now = self.now;
        for out in self.session.take_out() {
            match out {
                Out::Reliable(b) => self.core.reliable(LOCAL_SESSION, Channel::Control, &b),
                Out::Datagram(b) => self.core.datagram(LOCAL_SESSION, &b),
                Out::Sign(t) => self.session.signed(self.key.sign(&t).to_bytes()),
                Out::Close => {}
            }
        }
        self.core.advance(1.0 / 60.0);
        for o in self.core.take_outbox() {
            match o.kind {
                OutKind::Reliable(channel) => self.session.on_reliable(channel, &o.bytes, now),
                OutKind::Datagram => self.session.on_datagram(&o.bytes, now),
                OutKind::Close => {}
            }
        }
        self.session.tick(now);
        if self.game.is_none()
            && let SessionState::Joined {
                player_id,
                verification_chunk,
                ..
            } = self.session.state().clone()
        {
            let pool = Pool::new(Box::new(InlineJobs::new(Runner::world(
                GENERATOR_TERRAIN,
                SEED,
            ))));
            let c = ChunkCoord::from_array(verification_chunk);
            self.game = Some(Game::new(
                player_id,
                pool,
                self.scene.clone(),
                Some(c),
                PlayerControllerConfig::default(),
            ));
        }
        let Some(game) = &mut self.game else { return };
        while let Some((m, bytes)) = self.session.next_game() {
            game.on_game_message(m, bytes.len(), now);
        }
        game.frame(now, &mut self.input, None, &mut || 0.0);
        for s in game.take_outbox() {
            match s {
                Send::Control(m) => self.session.send_control(&m),
                Send::Datagram(m) => self.session.send_game_datagram(&m),
            }
        }
    }

    fn run_until(&mut self, max_frames: usize, mut done: impl FnMut(&mut Self) -> bool) -> bool {
        for _ in 0..max_frames {
            self.frame();
            if done(self) {
                return true;
            }
        }
        false
    }
}

struct NoStorage;
impl StorageBackend for NoStorage {
    fn len(&self) -> Result<u64, std::io::Error> {
        unreachable!()
    }
    fn read(&self, _: u64, _: &mut [u8]) -> Result<(), std::io::Error> {
        unreachable!()
    }
    fn set_len(&self, _: u64) -> Result<(), std::io::Error> {
        unreachable!()
    }
    fn sync_data(&self) -> Result<(), std::io::Error> {
        unreachable!()
    }
    fn write(&self, _: u64, _: &[u8]) -> Result<(), std::io::Error> {
        unreachable!()
    }
}

impl std::fmt::Debug for NoStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NoStorage")
    }
}

#[test]
fn joins_streams_walks_and_breaks_a_block() {
    let mut rig = Rig::new();
    // Joined, placed, and the terrain around the player loaded (prediction runs).
    assert!(
        rig.run_until(60 * 30, |r| r
            .game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)),
        "the player is placed and the terrain around them arrives"
    );
    assert!(
        rig.run_until(60 * 10, |r| !r.scene.borrow().chunks.is_empty()),
        "chunks are meshed and handed to the scene"
    );
    let start = rig.game.as_ref().unwrap().debug_state().feet;
    assert!(rig.scene.borrow().camera.is_some());

    // Walk forward for a second and a half.
    rig.input.set_key("KeyW", true);
    rig.run_until(90, |_| false);
    rig.input.set_key("KeyW", false);
    rig.run_until(30, |_| false);
    let feet = rig.game.as_ref().unwrap().debug_state().feet;
    let moved = ((feet[0] - start[0]).powi(2) + (feet[2] - start[2]).powi(2)).sqrt();
    assert!(moved > 2.0, "walked {moved} m from {start:?} to {feet:?}");

    // Look at the ground in front and break the targeted block.
    rig.input.look.pitch = -60.0;
    assert!(
        rig.run_until(30, |r| r
            .game
            .as_ref()
            .unwrap()
            .debug_state()
            .target
            .is_some()),
        "a block is targeted"
    );
    let target = rig.game.as_ref().unwrap().debug_state().target.unwrap();
    assert!(rig.scene.borrow().outline.is_some(), "and outlined");
    let [x, y, z] = target.cell;
    assert_ne!(rig.game.as_mut().unwrap().voxel(x, y, z), 0);
    let now = rig.now + 1000.0;
    assert!(
        rig.game.as_mut().unwrap().edit(EditAction::Break, now),
        "the edit is sent"
    );
    assert!(
        rig.run_until(60, |r| r.game.as_mut().unwrap().voxel(x, y, z) == 0),
        "the server's modification arrives and the block is gone"
    );
}

#[test]
fn the_whole_world_view_generates_and_meshes_sections() {
    let mut rig = Rig::new();
    assert!(rig.run_until(60 * 30, |r| r.game.is_some()));
    rig.game
        .as_mut()
        .unwrap()
        .enable_lod(dwell_client::game::LodSettings {
            pixel_error: 2.0,
            cache_bytes: 256 << 20,
            detail_distance_m: None,
            full_mode: false,
        });
    assert!(
        rig.run_until(60 * 20, |r| r.scene.borrow().sections > 0),
        "LOD sections reach the scene: {:?}",
        rig.game.as_ref().unwrap().debug_state().lod
    );
    let stats = rig.game.as_ref().unwrap().debug_state().lod.unwrap();
    assert!(stats.nodes > 0);
}
