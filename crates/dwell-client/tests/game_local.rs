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
use dwell_client::render::{Line, PlayerView, RegionBox, StandIn};
use dwell_game::brush_tool::{BrushAction, BrushOutline};
use dwell_game::input::{EditAction, KeyboardMouseInput};
use dwell_game::region_tools::ToolAction;
use dwell_mesh::{ChunkMeshes, SectionMeshes};
use dwell_physics::config::PlayerControllerConfig;
use dwell_protocol::{Channel, TransportKind};
use dwell_sim::entropy::SeededEntropy;
use dwell_sim::server::OutKind;
use dwell_storage::StorageBackend;
use dwell_world::{ChunkCoord, GENERATOR_SANDBOX, GENERATOR_TERRAIN};
use ed25519_dalek::{Signer, SigningKey};

#[derive(Default)]
struct Recorder {
    chunks: std::collections::HashSet<ChunkCoord>,
    sections: usize,
    /// Each section held: its origin and cell size.
    held: std::collections::HashMap<i64, ([f64; 3], f64)>,
    shown: Vec<i64>,
    camera: Option<[f64; 3]>,
    outline: Option<[f64; 3]>,
    region_boxes: Vec<RegionBox>,
    brush: Option<BrushOutline>,
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
        id: i64,
        origin: [f64; 3],
        cell: f64,
        meshes: Option<&SectionMeshes>,
    ) {
        self.sections += usize::from(meshes.is_some());
        if meshes.is_some() {
            self.held.insert(id, (origin, cell));
        } else {
            self.held.remove(&id);
        }
    }
    fn show_lod_sections(&mut self, visible: &[(i64, u8)]) {
        self.shown = visible.iter().map(|&(id, _)| id).collect();
    }
    fn show_lod_stand_ins(&mut self, _stand_ins: &[StandIn]) {}
    fn set_player(&mut self, _id: u32, _view: Option<PlayerView>) {}
    fn set_camera(&mut self, eye: [f64; 3], _yaw: f64, _pitch: f64, _face: f64, _flip: f64) {
        self.camera = Some(eye);
    }
    fn set_block_outline(&mut self, cell: Option<[f64; 3]>, _height: f64) {
        self.outline = cell;
    }
    fn set_region_boxes(&mut self, boxes: &[RegionBox]) {
        self.region_boxes = boxes.to_vec();
    }
    fn set_brush_outline(&mut self, outline: Option<&BrushOutline>) {
        self.brush = outline.cloned();
    }
    fn set_debug_lines(&mut self, _lines: Option<&[Line]>) {}
    fn stats(&self) -> RenderStats {
        RenderStats::default()
    }
}

struct Rig {
    generator: u32,
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
        Self::with_generator(GENERATOR_TERRAIN)
    }

    fn with_generator(generator: u32) -> Self {
        let mut core = LocalCore::create(
            SEED,
            generator,
            None::<NoStorage>,
            Box::new(SeededEntropy(1)),
        )
        .unwrap();
        core.connected(LOCAL_SESSION, TransportKind::Loopback, [0; 32]);
        let key = SigningKey::from_bytes(&[5; 32]);
        let mut session = Session::new([0; 32], key.verifying_key().to_bytes(), "Me", "t", 1000.0);
        session.start(0.0);
        Self {
            generator,
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
                self.generator,
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
    rig.input.look.pitch = -60.0;
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
fn a_brush_stroke_in_the_sandbox_comes_back_from_the_server_and_undoes_as_one_step() {
    let mut rig = Rig::with_generator(GENERATOR_SANDBOX);
    assert!(rig.run_until(60 * 30, |r| {
        r.game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)
    }));
    let SessionState::Joined {
        region_edits,
        brushes,
        generator_version,
        ..
    } = rig.session.state().clone()
    else {
        panic!("joined")
    };
    assert!(brushes, "a sandbox server announces brushes");
    let game = rig.game.as_mut().unwrap();
    game.set_capabilities(region_edits, brushes, generator_version);
    assert!(game.brush.available);
    assert!(game.brush.reach >= 128.0);
    game.brush_key(BrushAction::Toggle);
    let ball = dwell_world::brush::PRESETS
        .iter()
        .position(|p| p.name == "ball")
        .unwrap();
    game.brush_key(BrushAction::Preset(ball));
    game.interaction.select(0); // stone
    // Looking down at the floor a few metres ahead: the brush aims there and outlines itself.
    rig.input.look.pitch = -45.0;
    assert!(rig.run_until(30, |r| r.game.as_ref().unwrap().brush.target().is_some()));
    assert!(rig.scene.borrow().brush.is_some(), "the brush is outlined");
    let target = rig.game.as_ref().unwrap().brush.target().unwrap().cell;
    let above = [target[0], target[1] + 1, target[2]];
    let stone = dwell_blocks::require("dwell:stone");
    let now = rig.now;
    assert!(
        rig.game.as_mut().unwrap().brush_apply(false, now),
        "the stroke is queued"
    );
    assert!(
        rig.run_until(60, |r| r
            .game
            .as_mut()
            .unwrap()
            .voxel(above[0], above[1], above[2])
            == stone),
        "the server's modification arrives"
    );
    // The floor stays, and one undo takes the whole ball away.
    let floor = rig
        .game
        .as_mut()
        .unwrap()
        .voxel(target[0], target[1], target[2]);
    assert!(
        dwell_blocks::info(floor).indestructible,
        "the sandbox floor"
    );
    assert_eq!(rig.game.as_ref().unwrap().region.undo_depth(), (1, 0));
    let now = rig.now;
    rig.game.as_mut().unwrap().tool(ToolAction::Undo, now);
    assert!(
        rig.run_until(60, |r| r
            .game
            .as_mut()
            .unwrap()
            .voxel(above[0], above[1], above[2])
            == 0),
        "undo takes the stroke back"
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
