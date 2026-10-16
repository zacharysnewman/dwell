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
use dwell_client::render::{Line, PlayerView, RegionBox, StandIn, WindView};
use dwell_game::input::{EditAction, KeyboardMouseInput};
use dwell_game::region_tools::ToolAction;
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
    fov: Option<f64>,
    wind: WindView,
    region_boxes: Vec<RegionBox>,
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
            region_boxes: Vec::new(),
            fov: None,
            wind: WindView::default(),
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
    fn set_fov(&mut self, fov_y_deg: Option<f64>) {
        self.fov = fov_y_deg;
    }
    fn set_wind(&mut self, wind: WindView) {
        self.wind = wind;
    }
    fn set_region_boxes(&mut self, boxes: &[RegionBox]) {
        self.region_boxes = boxes.to_vec();
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
    /// One-way delay of the link, in server ticks (0: none), and the messages on it.
    latency_steps: u32,
    step: u32,
    up: std::collections::VecDeque<(u32, bool, Vec<u8>)>,
    down: std::collections::VecDeque<(u32, Option<Channel>, Vec<u8>)>,
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
            latency_steps: 0,
            step: 0,
            up: std::collections::VecDeque::new(),
            down: std::collections::VecDeque::new(),
        }
    }

    /// One 60 Hz frame: the server steps, messages flow both ways, the game runs a frame.
    fn frame(&mut self) {
        self.server_step();
        self.game_frame();
    }

    /// A frame that took `steps` server ticks (a slow device: `steps` / 60 s per frame).
    fn slow_frame(&mut self, steps: usize) {
        for _ in 0..steps {
            self.server_step();
        }
        self.game_frame();
    }

    /// 1/60 s of the server's time: the client's messages reach it, it steps, its answers return.
    fn server_step(&mut self) {
        self.now += 1000.0 / 60.0;
        self.step += 1;
        let now = self.now;
        let due = self.step + self.latency_steps;
        for out in self.session.take_out() {
            match out {
                Out::Reliable(b) => self.up.push_back((due, true, b)),
                Out::Datagram(b) => self.up.push_back((due, false, b)),
                Out::Sign(t) => self.session.signed(self.key.sign(&t).to_bytes()),
                Out::Close => {}
            }
        }
        while self.up.front().is_some_and(|m| m.0 <= self.step) {
            let (_, reliable, b) = self.up.pop_front().unwrap();
            if reliable {
                self.core.reliable(LOCAL_SESSION, Channel::Control, &b);
            } else {
                self.core.datagram(LOCAL_SESSION, &b);
            }
        }
        self.core.advance(1.0 / 60.0);
        for o in self.core.take_outbox() {
            match o.kind {
                OutKind::Reliable(channel) => self.down.push_back((due, Some(channel), o.bytes)),
                OutKind::Datagram => self.down.push_back((due, None, o.bytes)),
                OutKind::Close => {}
            }
        }
        while self.down.front().is_some_and(|m| m.0 <= self.step) {
            let (_, channel, b) = self.down.pop_front().unwrap();
            match channel {
                Some(channel) => self.session.on_reliable(channel, &b, now),
                None => self.session.on_datagram(&b, now),
            }
        }
        self.session.tick(now);
    }

    /// One frame of the game at the current time: it reads what arrived, ticks, draws and sends.
    fn game_frame(&mut self) {
        let now = self.now;
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
fn a_region_fill_comes_back_from_the_server_and_undo_restores_it() {
    let mut rig = Rig::new();
    assert!(rig.run_until(60 * 30, |r| {
        r.game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)
    }));
    let SessionState::Joined { region_edits, .. } = rig.session.state().clone() else {
        panic!("joined")
    };
    assert!(region_edits, "the local server announces region edits");
    rig.game.as_mut().unwrap().region.enabled = region_edits;
    // A cell clear of the player: natural ground rests at half height (ADAPTIVE_BLOCKS.md), so the
    // top cell right under the feet holds part of the capsule, and filling it is refused.
    rig.input.look.pitch = -40.0;
    assert!(rig.run_until(30, |r| {
        r.game.as_ref().unwrap().debug_state().target.is_some()
    }));
    let [x, y, z] = rig
        .game
        .as_ref()
        .unwrap()
        .debug_state()
        .target
        .unwrap()
        .cell;
    let game = rig.game.as_mut().unwrap();
    let before = game.voxel(x, y, z);
    let stone = dwell_blocks::require("dwell:stone");
    assert_ne!(before, stone);
    game.interaction.select(0); // stone
    assert!(!game.tool(ToolAction::CornerA, rig.now));
    assert_eq!(
        rig.scene.borrow().region_boxes.len(),
        1,
        "the selection is outlined"
    );
    assert_eq!(rig.scene.borrow().region_boxes[0].size, [1.0; 3]);
    assert!(game.tool(ToolAction::Fill, rig.now), "the fill is sent");
    assert!(
        rig.run_until(60, |r| r.game.as_mut().unwrap().voxel(x, y, z) == stone),
        "the server's modification arrives"
    );
    let now = rig.now;
    assert!(rig.game.as_mut().unwrap().tool(ToolAction::Undo, now));
    assert!(
        rig.run_until(60, |r| r.game.as_mut().unwrap().voxel(x, y, z) == before),
        "undo puts the block back"
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

#[test]
fn sprinting_widens_the_view_for_the_renderer_and_the_lod_alike() {
    // Motion effects (Phase 29): the camera's velocity widens the FOV, and the frame's FOV is the
    // one the LOD selects with; standing still settles it back, and the menu holds it.
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
    rig.run_until(60, |_| false);
    let base = rig.scene.borrow().fov.expect("the game sets the FOV");
    assert_eq!(
        base,
        dwell_lod::vertical_fov(1280.0 / 720.0),
        "standing: no boost"
    );
    assert_eq!(rig.scene.borrow().wind.intensity, 0.0);

    rig.input.set_key("ShiftLeft", true);
    rig.input.set_key("KeyW", true);
    rig.run_until(90, |_| false);
    let fov = rig.scene.borrow().fov.unwrap();
    let state = rig.game.as_ref().unwrap().state().state;
    assert!(
        fov > base + 3.0,
        "sprinting widens the view: {fov} vs {base} ({state:?})"
    );
    assert!(
        rig.scene.borrow().wind.intensity > 0.1,
        "and the wind blows"
    );
    let lod = rig
        .game
        .as_ref()
        .unwrap()
        .lod_camera()
        .expect("the LOD ran");
    assert_eq!(lod.fov_y_deg, fov, "the LOD selects with the frame's FOV");
    assert_eq!(rig.game.as_ref().unwrap().frame_fov(), fov);

    // The menu opens: nothing changes, even standing still.
    rig.input.set_key("ShiftLeft", false);
    rig.input.set_key("KeyW", false);
    rig.game.as_mut().unwrap().menu_open = true;
    rig.run_until(60, |_| false);
    assert_eq!(rig.scene.borrow().fov.unwrap(), fov);
    rig.game.as_mut().unwrap().menu_open = false;
    rig.run_until(180, |_| false);
    let settled = rig.scene.borrow().fov.unwrap();
    assert!(settled < base + 0.5, "standing settles back: {settled}");

    // The Field of view setting is the base for the renderer and the LOD alike.
    rig.game.as_mut().unwrap().fov_setting = 90.0;
    rig.run_until(240, |_| false);
    let wider = dwell_lod::base_fov(90.0, 1280.0 / 720.0);
    assert!((rig.scene.borrow().fov.unwrap() - wider).abs() < 1e-6);
    assert!(wider > base + 10.0);
    let lod = rig.game.as_ref().unwrap().lod_camera().unwrap();
    assert!((lod.fov_y_deg - wider).abs() < 1e-6);
}

#[test]
fn a_slow_device_walks_without_snapping() {
    // A device drawing ~5.5 frames a second (CI's software renderer with two pages,
    // e2e/play.spec.ts): each frame covers 11 of the server's 60 Hz ticks. Its prediction must
    // keep the server's time, or the server fills the missing ticks by repeating the last input
    // (walking further than the player did) and the replays drag the player along, snapping it
    // once a correction passes RECONCILE_SNAP_DISTANCE.
    let mut rig = Rig::new();
    assert!(
        rig.run_until(60 * 30, |r| r
            .game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)),
        "the player is placed and the terrain around them arrives"
    );
    // Settle on the ground at full speed, then slow down, over a 150 ms round trip (as the e2e
    // test's B page: `?netsim=150,…`).
    rig.run_until(60, |_| false);
    rig.latency_steps = 5;
    let stats = |r: &Rig| r.game.as_ref().unwrap().debug_state().stats;
    let mut worst = 0.0f32;
    let mut slow_frames = |rig: &mut Rig, n: usize| {
        for _ in 0..n {
            let replays = stats(rig).replays;
            rig.slow_frame(11);
            let s = stats(rig);
            if s.replays > replays {
                worst = worst.max(s.last_correction);
            }
        }
    };
    slow_frames(&mut rig, 10);
    let before = stats(&rig);
    let start = rig.game.as_ref().unwrap().debug_state().feet;
    rig.input.set_key("KeyW", true);
    slow_frames(&mut rig, 6);
    rig.input.set_key("KeyW", false);
    slow_frames(&mut rig, 10);
    let after = stats(&rig);
    let feet = rig.game.as_ref().unwrap().debug_state().feet;
    let moved = ((feet[0] - start[0]).powi(2) + (feet[2] - start[2]).powi(2)).sqrt();
    assert!(moved > 2.0, "walked {moved} m");
    // The prediction matches the server: no correction to speak of, so no snap.
    assert!(
        worst < 0.2,
        "a {worst} m correction: {before:?} -> {after:?}"
    );
    assert_eq!(after.snaps, before.snaps);
    // Predicted ticks keep up with the server's (16 frames of 11 ticks).
    let predicted = after.ticks - before.ticks;
    assert!(
        predicted >= 16 * 11 - 11,
        "{predicted} ticks predicted for {} of the server's",
        16 * 11
    );
}
