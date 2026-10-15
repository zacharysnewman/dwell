//! The Rust client's game loop end to end, natively (Phase 24e): a local world's server core, the
//! client session (with the signed handshake), the game with its jobs run inline and a recording
//! scene. The player joins, the terrain streams in and is meshed, the player walks, breaks a block
//! and sees it gone (the e2e suite's local-mode movement and edit cases, without a browser).

use std::cell::RefCell;
use std::rc::Rc;

use dwell_client::game::{Game, Scene, Send, ViewOverride};
use dwell_client::jobs::Runner;
use dwell_client::local::{LOCAL_SESSION, LocalCore};
use dwell_client::net::session::{Out, Session, SessionState};
use dwell_client::pool::{InlineJobs, Pool};
use dwell_client::render::display::RenderStats;
use dwell_client::render::march::{ChunkBricks, SectionBricks};
use dwell_client::render::seam::{
    ChunkTerrain, Recording, Routed, SectionTerrain, Selection, TerrainKind, TerrainRenderer,
};
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

type Renderers =
    Routed<Recording<ChunkMeshes, SectionMeshes>, Recording<ChunkBricks, SectionBricks>>;

/// A recording scene: what reaches the scene, and (behind the renderer seam) what each terrain
/// renderer holds.
struct Recorder {
    terrain: Renderers,
    chunks: std::collections::HashSet<ChunkCoord>,
    sections: usize,
    /// Each section held: its origin and cell size.
    held: std::collections::HashMap<i64, ([f64; 3], f64)>,
    shown: Vec<i64>,
    camera: Option<[f64; 3]>,
    outline: Option<[f64; 3]>,
}

impl Default for Recorder {
    fn default() -> Self {
        Self {
            terrain: Routed::new(
                Recording::default(),
                Recording::default(),
                Selection::default(),
            ),
            chunks: Default::default(),
            sections: 0,
            held: Default::default(),
            shown: Vec::new(),
            camera: None,
            outline: None,
        }
    }
}

impl TerrainRenderer for Recorder {
    type Chunk = ChunkTerrain;
    type Section = SectionTerrain;
    fn set_chunk(&mut self, coord: ChunkCoord, payload: Option<&ChunkTerrain>) {
        self.terrain.set_chunk(coord, payload);
        if payload.is_some() {
            self.chunks.insert(coord);
        } else {
            self.chunks.remove(&coord);
        }
    }
    fn set_section(
        &mut self,
        id: i64,
        origin: [f64; 3],
        cell: f64,
        payload: Option<&SectionTerrain>,
    ) {
        self.terrain.set_section(id, origin, cell, payload);
        self.sections += usize::from(payload.is_some());
        if payload.is_some() {
            self.held.insert(id, (origin, cell));
        } else {
            self.held.remove(&id);
        }
    }
    fn show_sections(&mut self, visible: &[(i64, u8)]) {
        self.terrain.show_sections(visible);
        self.shown = visible.iter().map(|&(id, _)| id).collect();
    }
    fn show_stand_ins(&mut self, _stand_ins: &[StandIn]) {}
    fn clear(&mut self) {
        self.terrain.clear();
    }
    fn gpu_bytes(&self) -> u64 {
        self.terrain.gpu_bytes()
    }
}

impl Scene for Recorder {
    fn selection(&self) -> Selection {
        self.terrain.selection()
    }
    fn select(&mut self, selection: Selection) -> bool {
        self.terrain.select(selection)
    }
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

#[test]
fn a_placed_camera_views_the_world_from_there() {
    let mut rig = Rig::new();
    assert!(rig.run_until(60 * 30, |r| {
        r.game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)
    }));
    rig.game
        .as_mut()
        .unwrap()
        .enable_lod(dwell_client::game::LodSettings {
            pixel_error: 2.0,
            cache_bytes: 256 << 20,
            detail_distance_m: None,
            full_mode: false,
        });
    let feet = rig.game.as_ref().unwrap().debug_state().feet;
    // 6 km from the player, 600 m up, looking down: the LOD refines around the placed camera.
    let eye = [feet[0] + 6000.0, 600.0, feet[2]];
    let view = ViewOverride {
        eye,
        yaw_deg: 0.0,
        pitch_deg: -60.0,
    };
    rig.game.as_mut().unwrap().set_view(Some(view));
    let fine_below = |r: &Rig| {
        let scene = r.scene.borrow();
        scene.shown.iter().any(|id| {
            scene.held.get(id).is_some_and(|&(o, cell)| {
                let size = cell * 32.0;
                let near = |k: usize| o[k] - 200.0 <= eye[k] && eye[k] <= o[k] + size + 200.0;
                cell <= 4.0 && near(0) && near(2)
            })
        })
    };
    assert!(
        rig.run_until(60 * 60, |r| fine_below(r)),
        "fine sections drawn under the placed camera: {:?}",
        rig.game.as_ref().unwrap().debug_state().lod
    );
    assert_eq!(rig.scene.borrow().camera, Some(eye));
    assert_eq!(rig.game.as_ref().unwrap().view(), Some(view));
    // The player stayed where it was.
    let now = rig.game.as_ref().unwrap().debug_state().feet;
    assert!((now[0] - feet[0]).abs() < 1.0 && (now[2] - feet[2]).abs() < 1.0);

    // Released: the camera is back at the player's eyes.
    rig.game.as_mut().unwrap().set_view(None);
    rig.run_until(2, |_| false);
    let camera = rig.scene.borrow().camera.unwrap();
    assert!((camera[0] - feet[0]).abs() < 2.0 && (camera[2] - feet[2]).abs() < 2.0);
}

#[test]
fn switching_renderer_rebuilds_the_terrain_for_it_without_a_reload() {
    // The renderer seam (RAY_MARCHED_RENDERER.md §3): the jobs make what the drawing renderers
    // take, and a switch rebuilds every loaded chunk and LOD section from the client's own data.
    let mut rig = Rig::new();
    assert!(rig.run_until(60 * 30, |r| {
        r.game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)
    }));
    rig.game
        .as_mut()
        .unwrap()
        .enable_lod(dwell_client::game::LodSettings {
            pixel_error: 2.0,
            cache_bytes: 256 << 20,
            detail_distance_m: None,
            full_mode: false,
        });
    assert!(rig.run_until(60 * 20, |r| {
        let s = r.scene.borrow();
        s.terrain.raster.chunks.len() > 4 && !s.terrain.raster.sections.is_empty()
    }));
    assert!(
        rig.scene.borrow().terrain.march.chunks.is_empty(),
        "raster only: no bricks are made"
    );
    let loaded: Vec<ChunkCoord> = rig
        .scene
        .borrow()
        .terrain
        .raster
        .chunks
        .keys()
        .copied()
        .collect();

    // Compare mode: bricks for every chunk already loaded (and the meshes kept meanwhile).
    let compare = Selection::parse("compare", None).unwrap();
    rig.game.as_mut().unwrap().set_renderer(compare);
    assert!(
        rig.scene.borrow().terrain.raster.chunks.len() >= loaded.len(),
        "raster kept"
    );
    assert!(
        rig.run_until(60 * 10, |r| {
            let s = r.scene.borrow();
            loaded
                .iter()
                .all(|c| s.terrain.march.chunks.contains_key(c))
                && !s.terrain.march.sections.is_empty()
        }),
        "every loaded chunk and some sections rebuilt as bricks"
    );

    // March only: the meshes go (their GPU memory with them) and are not made again.
    rig.game
        .as_mut()
        .unwrap()
        .set_renderer(Selection::single(TerrainKind::March));
    assert!(rig.scene.borrow().terrain.raster.chunks.is_empty());
    assert!(rig.scene.borrow().terrain.raster.sections.is_empty());
    rig.run_until(60, |_| false);
    let s = rig.scene.borrow();
    assert!(s.terrain.raster.chunks.is_empty() && s.terrain.raster.sections.is_empty());
    assert!(
        loaded
            .iter()
            .all(|c| s.terrain.march.chunks.contains_key(c))
    );
}
