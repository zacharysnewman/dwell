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
use dwell_client::render::seam::{Recording, TerrainRenderer};
use dwell_client::render::{Line, PlayerView, RegionBox, StandIn, WindView};
use dwell_client::telemetry;
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

/// A recording scene: what reaches the scene, and the terrain it holds.
#[derive(Default)]
struct Recorder {
    terrain: Recording,
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
    brush: Option<BrushOutline>,
    /// The players drawn (the full bodies, with their clips), by id.
    players: std::collections::HashMap<u32, PlayerView>,
    /// Whether your arms were drawn last frame.
    arms: bool,
}

impl TerrainRenderer for Recorder {
    fn set_chunk(&mut self, coord: ChunkCoord, payload: Option<&ChunkMeshes>) {
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
        payload: Option<&SectionMeshes>,
    ) {
        self.terrain.set_section(id, origin, cell, payload);
        self.sections += usize::from(payload.is_some());
        if payload.is_some() {
            self.held.insert(id, (origin, cell));
        } else {
            self.held.remove(&id);
        }
    }
    fn show_sections(&mut self, visible: &[(i64, u8, dwell_lod::fade::Fade)]) {
        self.terrain.show_sections(visible);
        self.shown = visible.iter().map(|&(id, _, _)| id).collect();
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
    fn set_player(&mut self, id: u32, view: Option<PlayerView>) {
        match view {
            Some(v) => self.players.insert(id, v),
            None => self.players.remove(&id),
        };
    }
    fn set_arms(&mut self, arms: Option<dwell_client::render::ArmsView>) {
        self.arms = arms.is_some();
    }
    fn set_camera(
        &mut self,
        eye: [f64; 3],
        _yaw: f64,
        _pitch: f64,
        _face: dwell_world::FaceFrame,
        _roll: f64,
    ) {
        self.camera = Some(eye);
    }
    fn set_block_outline(&mut self, cell: Option<[f64; 3]>, _bounds: ([f32; 3], [f32; 3])) {
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
    /// A second client, session-level only (no game): its key, and the game messages it got.
    guest: Option<(Session, SigningKey, Vec<dwell_protocol::Message>)>,
    /// The game's loading screen on joining (ARCHITECTURE.md §6.3; off, as in the e2e suite).
    loading_screen: bool,
}

/// The guest's session id on the local core.
const GUEST: u32 = 2;

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
            guest: None,
            loading_screen: false,
        }
    }

    /// Opens the world to a guest client (session-level: it joins, takes the registry prompt's
    /// Continue, asks for explicit chunks and keeps what arrives).
    fn add_guest(&mut self) {
        use dwell_sim::server::EditPolicy;
        let host = self.key.verifying_key().to_bytes();
        self.core
            .host(4, EditPolicy::Everyone, EditPolicy::Everyone, host);
        self.core.connected(GUEST, TransportKind::WebRtc, [0; 32]);
        let key = SigningKey::from_bytes(&[6; 32]);
        let mut session = Session::new(
            [0; 32],
            key.verifying_key().to_bytes(),
            "Guest",
            "t",
            1000.0,
        );
        session.start(self.now);
        self.guest = Some((session, key, Vec::new()));
    }

    fn pump_guest_up(&mut self) {
        let Some((session, key, got)) = &mut self.guest else {
            return;
        };
        if matches!(session.state(), SessionState::Confirming { .. }) {
            session.accept_registry();
            session.send_control(&dwell_protocol::Message::WorldgenCheck(0));
        }
        while let Some((m, _)) = session.next_game() {
            got.push(m);
        }
        for out in session.take_out() {
            match out {
                Out::Reliable(b) => self.core.reliable(GUEST, Channel::Control, &b),
                Out::Datagram(b) => self.core.datagram(GUEST, &b),
                Out::Sign(t) => session.signed(key.sign(&t).to_bytes()),
                Out::Close => {}
            }
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
        self.pump_guest_up();
        self.core.advance(1.0 / 60.0);
        for o in self.core.take_outbox() {
            if o.session == GUEST {
                match (&mut self.guest, o.kind) {
                    (Some((guest, _, _)), OutKind::Reliable(channel)) => {
                        guest.on_reliable(channel, &o.bytes, now);
                    }
                    (Some((guest, _, _)), OutKind::Datagram) => guest.on_datagram(&o.bytes, now),
                    _ => {}
                }
                continue;
            }
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
            let mut game = Game::new(
                player_id,
                pool,
                self.scene.clone(),
                Some(c),
                PlayerControllerConfig::default(),
            );
            game.set_registry(self.session.mapping().cloned());
            game.set_loading_screen(self.loading_screen);
            self.game = Some(game);
        }
        let Some(game) = &mut self.game else { return };
        while let Some((m, bytes)) = self.session.next_game() {
            // As the web app does: an edit ack is translated by the session (edit prediction).
            if let dwell_protocol::Message::EditAck(a) = &m {
                let done = self.session.edit_results(a);
                game.on_edit_results(&done);
                continue;
            }
            game.on_game_message(m, bytes.len(), now);
        }
        game.frame(now, &mut self.input, None, &mut || 0.0);
        for s in game.take_outbox() {
            match s {
                Send::Control(m) => self.session.send_control(&m),
                Send::Datagram(m) => self.session.send_game_datagram(&m),
                Send::Edit(m, id) => {
                    self.session.send_edit(&m, id);
                }
            }
        }
        let dropped: Vec<(u32, bool)> = self
            .session
            .take_dropped_edits()
            .into_iter()
            .map(|id| (id, true))
            .collect();
        game.on_edit_results(&dropped);
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
    // Local mode goes through registry sync too (ADR 0038): the same build serves the same
    // definitions, so the map is the identity and nothing is asked.
    let m = rig.session.mapping().expect("mapped at Welcome");
    assert!(m.identity && m.generated_ok && m.differences.is_empty());
    assert_eq!(
        m.hash,
        dwell_blocks::sync::definitions_hash(&dwell_blocks::sync::Payload::served().encode())
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

/// Regression (e2e touch.spec on CI, 0.17.1): the eye camera kept the height it was given before
/// the player was placed (the default state at the origin), so the first frames after the spawn
/// drew the eye from there, inside the ground, and aimed the crosshair from it: on a slow page the
/// e2e test read the target of that frame (a cell under the stone) and found stone above it.
#[test]
fn the_eye_is_at_the_players_head_from_the_first_frame_after_the_spawn() {
    let mut rig = Rig::new();
    rig.input.look.pitch = -45.0;
    let mut checked = 0;
    for _ in 0..60 * 30 {
        rig.frame();
        let Some(game) = rig.game.as_ref() else {
            continue;
        };
        let s = game.debug_state();
        if !s.active {
            continue;
        }
        let eye = rig.scene.borrow().camera.expect("a camera");
        let head = s.feet[1] + 1.62;
        assert!(
            (eye[1] - head).abs() < 0.6,
            "frame {checked} after the spawn: the eye at y {:.2}, the head at {head:.2}",
            eye[1]
        );
        checked += 1;
        if s.terrain_ready && checked > 120 {
            break;
        }
    }
    assert!(
        checked > 120,
        "the player was placed and the terrain arrived"
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
    // A cell clear of the player, ahead of the feet.
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
    // One cell: 1 m across, ½ m tall (HALF_HEIGHT_GRID.md).
    assert_eq!(
        rig.scene.borrow().region_boxes[0].size,
        [1.0, dwell_world::CELL_H, 1.0]
    );
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
    rig.game.as_mut().unwrap().take_sounds();
    assert!(
        rig.game.as_mut().unwrap().brush_apply(false, now),
        "the stroke is queued"
    );
    assert!(
        rig.game
            .as_mut()
            .unwrap()
            .take_sounds()
            .iter()
            .any(|s| s.kind == dwell_game::sound::SoundKind::Tool),
        "a stroke clicks like a region tool"
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
fn the_loading_screen_holds_the_player_until_the_terrain_all_round_is_drawn() {
    // Owner (2026-10-21): a loading screen until the chunks around the player load on joining,
    // and terrain that loads without turning round. Facing +X from the start, the chunks at −X
    // within the near ring are meshed when the screen opens, and the player did not move before.
    use dwell_game::loading::{LOADING, near_ring_offsets};
    let mut rig = Rig::new();
    rig.loading_screen = true;
    rig.input.look.yaw = 90.0;
    rig.input.set_key("KeyW", true); // held all along: no step until the screen opens
    assert!(rig.run_until(60 * 30, |r| r.game.is_some()));
    let mut placed = None;
    let mut waited = 0;
    assert!(
        rig.run_until(60 * 60, |r| {
            let d = r.game.as_ref().unwrap().debug_state();
            if !d.active {
                // Before the join places the player the screen is already up (no view or HUD
                // first, owner 2026-10-21).
                assert!(d.loading.is_some_and(|l| !l.placed), "shown while joining");
                return false;
            }
            if d.loading.is_some() {
                waited += 1;
                assert!(
                    !d.terrain_ready,
                    "the prediction waits while the screen shows"
                );
                let at = *placed.get_or_insert(d.feet);
                assert_eq!(
                    d.feet, at,
                    "the player does not move before the screen opens"
                );
                return false;
            }
            true
        }),
        "the screen opens"
    );
    assert!(waited > 0, "the screen showed");
    let d = rig.game.as_ref().unwrap().debug_state();
    assert_eq!(
        d.loading_opened.map(|o| o.0),
        Some(dwell_game::loading::Opened::Ready),
        "opened on the near ring, not the timeout"
    );
    // Every chunk of the near ring that holds anything is meshed: behind (−X) as ahead.
    let me = dwell_world::chunk_of_pos(rig.game.as_ref().unwrap().position());
    let scene = rig.scene.borrow();
    let mut behind = 0;
    for o in near_ring_offsets(LOADING.radius_chunks) {
        let c = ChunkCoord::new(me.x + o[0], me.y + o[1], me.z + o[2]);
        let revision = rig.game.as_ref().unwrap().chunk_revision(c.to_array());
        assert!(revision.is_some(), "{c:?} loaded");
        if o[0] < 0 && scene.chunks.contains(&c) {
            behind += 1;
        }
    }
    assert!(behind > 0, "chunks behind the camera are meshed");
    drop(scene);
    // Then the held key walks.
    let start = rig.game.as_ref().unwrap().debug_state().feet;
    rig.run_until(60, |_| false);
    let feet = rig.game.as_ref().unwrap().debug_state().feet;
    assert!(((feet[0] - start[0]).powi(2) + (feet[2] - start[2]).powi(2)).sqrt() > 1.0);
}

#[test]
fn with_the_lod_the_loading_screen_opens_on_the_terrain_and_nothing_behind_is_left_waiting() {
    // The near ring counts as drawn where the LOD shows it as chunks or knows there is nothing to
    // draw (buried rock), so the screen opens on the terrain, not its timeout (the bench's first
    // run timed out at 20 s: buried sections straddling the ring never show their chunks). Then
    // the view waits for nothing behind the camera (stats.waiting[1]) once settled: sections whose
    // chunks are past the request radius (up or down) are not waited for.
    let mut rig = Rig::new();
    rig.loading_screen = true;
    rig.input.look.yaw = 90.0;
    assert!(rig.run_until(60 * 30, |r| r.game.is_some()));
    rig.game
        .as_mut()
        .unwrap()
        .enable_lod(dwell_client::game::LodSettings {
            pixel_error: 4.0,
            cache_bytes: 256 << 20,
            detail_distance_m: Some(256.0),
            full_mode: false,
        });
    assert!(
        rig.run_until(60 * 60, |r| r
            .game
            .as_ref()
            .unwrap()
            .debug_state()
            .loading_opened
            .is_some()),
        "the screen opens"
    );
    let d = rig.game.as_ref().unwrap().debug_state();
    let (how, after) = d.loading_opened.unwrap();
    assert_eq!(how, dwell_game::loading::Opened::Ready, "after {after} s");
    assert!(
        rig.run_until(60 * 120, |r| {
            let l = r.game.as_ref().unwrap().debug_state().lod.unwrap();
            l.generating == 0 && l.meshing == 0 && l.asking == 0 && l.waiting == [0, 0, 0]
        }),
        "settles with nothing waited for: {:?}",
        rig.game.as_ref().unwrap().debug_state().lod
    );
}

/// What a respawn after dying 20 chunks from the spawn costs: whether the respawn point's ring
/// was drawable before the respawn, and the frames from the respawn to the loading screen opening.
struct Respawned {
    ring: usize,
    ring_drawable_before: usize,
    frames_to_open: usize,
    opened: Option<(dwell_game::loading::Opened, f64)>,
}

fn respawn_after_dying_far_away(preload: Option<i32>, lod: bool) -> Respawned {
    use dwell_game::loading::{LOADING, near_ring_offsets};
    let mut rig = Rig::new();
    rig.loading_screen = true;
    assert!(rig.run_until(60 * 30, |r| r.game.is_some()));
    if lod {
        rig.game
            .as_mut()
            .unwrap()
            .enable_lod(dwell_client::game::LodSettings {
                pixel_error: 4.0,
                cache_bytes: 256 << 20,
                detail_distance_m: Some(256.0),
                full_mode: false,
            });
    }
    assert!(
        rig.run_until(60 * 60, |r| r
            .game
            .as_ref()
            .unwrap()
            .debug_state()
            .loading_opened
            .is_some()),
        "the join's screen opens"
    );
    rig.core.server().set_respawn_preload(preload);
    // Off 20 chunks east (the server's copy; the client follows), the view there loads, then the
    // player dies there.
    let feet = rig.game.as_ref().unwrap().debug_state().feet;
    let far = [feet[0] + 640.0, feet[1] + 40.0, feet[2]];
    let hover = |rig: &mut Rig| {
        let server = rig.core.server();
        let h = server.player_handle_of(1).unwrap();
        let (_, physics, players) = server.sim();
        let half = f64::from(players.half_height(h));
        players.teleport(
            physics,
            h,
            dwell_physics::DVec3::new(far[0], far[1] + half, far[2]),
            dwell_physics::Vec3::ZERO,
        );
    };
    // Held there for four seconds (a fall would kill it too soon) while the view there streams.
    for _ in 0..60 * 4 {
        hover(&mut rig);
        rig.frame();
    }
    rig.core.server().kill_player(1);
    assert!(rig.run_until(60, |r| r.game.as_ref().unwrap().debug_state().dead));
    // Four of the five seconds dead.
    rig.run_until(60 * 4, |_| false);
    assert!(rig.game.as_ref().unwrap().debug_state().dead, "still dead");
    let respawn = rig.core.server().config().spawn.unwrap();
    let at = dwell_world::chunk_of_pos([respawn[0], respawn[1] + 0.9, respawn[2]]);
    let game = rig.game.as_ref().unwrap();
    let ring: Vec<[i32; 3]> = near_ring_offsets(LOADING.radius_chunks)
        .into_iter()
        .map(|o| [at.x + o[0], at.y + o[1], at.z + o[2]])
        .filter(|c| dwell_world::BUILD_VOLUME.chunk_may_hold(ChunkCoord::from_array(*c)))
        .collect();
    let ring_drawable_before = ring.iter().filter(|c| game.chunk_drawable(**c)).count();
    // The respawn, then the screen.
    assert!(rig.run_until(60 * 3, |r| {
        let d = r.game.as_ref().unwrap().debug_state();
        !d.dead && d.active
    }));
    let mut frames_to_open = 0;
    rig.run_until(60 * 30, |r| {
        frames_to_open += 1;
        r.game.as_ref().unwrap().debug_state().loading.is_none()
    });
    Respawned {
        ring: ring.len(),
        ring_drawable_before,
        frames_to_open,
        opened: rig.game.as_ref().unwrap().debug_state().loading_opened,
    }
}

#[test]
fn dying_before_the_terrain_around_arrived_still_waits_behind_the_loading_screen_on_respawning() {
    // Found while testing the respawn preload: killed the moment it was moved far away (its
    // prediction waiting for the terrain there, so never ticking), the player stayed "placed"
    // while dead, and the respawn did not show the loading screen.
    let mut rig = Rig::new();
    rig.loading_screen = true;
    assert!(rig.run_until(60 * 30, |r| r.game.is_some()));
    assert!(rig.run_until(60 * 60, |r| {
        r.game
            .as_ref()
            .unwrap()
            .debug_state()
            .loading_opened
            .is_some()
    }));
    rig.core.server().set_respawn_preload(None);
    let feet = rig.game.as_ref().unwrap().debug_state().feet;
    let server = rig.core.server();
    let h = server.player_handle_of(1).unwrap();
    let (_, physics, players) = server.sim();
    players.teleport(
        physics,
        h,
        dwell_physics::DVec3::new(feet[0] + 640.0, feet[1] + 41.0, feet[2]),
        dwell_physics::Vec3::ZERO,
    );
    rig.frame();
    rig.core.server().kill_player(1);
    assert!(rig.run_until(60, |r| r.game.as_ref().unwrap().debug_state().dead));
    rig.frame();
    assert!(
        !rig.game.as_ref().unwrap().debug_state().active,
        "dead: not placed"
    );
    let mut shown = false;
    assert!(rig.run_until(60 * 30, |r| {
        let d = r.game.as_ref().unwrap().debug_state();
        shown |= !d.dead && d.loading.is_some();
        !d.dead && d.active && d.loading.is_none() && shown
    }));
    assert!(shown, "the respawn waited behind the screen");
}

#[test]
fn dying_far_away_preloads_the_respawn_so_its_loading_screen_opens_at_once() {
    // Owner (2026-10-21): "We could preload terrain at spawn once you die too?" Four seconds
    // into the five-second wait the respawn point's near ring is drawable, and the loading screen
    // on respawning opens within a few frames; without the preload it waits for the streaming.
    let with = respawn_after_dying_far_away(Some(3), false);
    assert_eq!(
        with.ring_drawable_before, with.ring,
        "the respawn ring is drawable before the respawn"
    );
    assert_eq!(
        with.opened.map(|o| o.0),
        Some(dwell_game::loading::Opened::Ready)
    );
    assert!(
        with.frames_to_open <= 3,
        "opened after {} frames",
        with.frames_to_open
    );
    let without = respawn_after_dying_far_away(None, false);
    assert!(
        without.ring_drawable_before < without.ring / 2,
        "{}",
        without.ring_drawable_before
    );
    assert!(
        without.frames_to_open > 3 * with.frames_to_open.max(1),
        "{} frames",
        without.frames_to_open
    );
    eprintln!(
        "respawn → screen open: {} frames with the preload, {} without",
        with.frames_to_open, without.frames_to_open
    );
}

#[test]
fn with_the_lod_a_preloaded_respawn_opens_its_loading_screen_at_once() {
    // The LOD readies the respawn area's sections down to level 1 and asks for the chunks of
    // those the ring straddles, so the view there refines to its chunks on the first frame.
    let with = respawn_after_dying_far_away(Some(3), true);
    let without = respawn_after_dying_far_away(None, true);
    eprintln!(
        "with the LOD, respawn → screen open: {} frames with the preload ({} / {} drawable before), {} without",
        with.frames_to_open, with.ring_drawable_before, with.ring, without.frames_to_open
    );
    assert_eq!(
        with.opened.map(|o| o.0),
        Some(dwell_game::loading::Opened::Ready)
    );
    assert_eq!(with.ring_drawable_before, with.ring);
    // Generator 21 (ADR 0049) put the spawn in a valley, where the level-1 sections the ring
    // straddles hold ground: they wait for their chunks a chunk beyond the ring, which took 14
    // frames while those were meshed after the far view's; with them in the preload's ring, 1.
    assert!(
        with.frames_to_open <= 3,
        "opened after {} frames",
        with.frames_to_open
    );
    assert!(
        without.frames_to_open > 3 * with.frames_to_open.max(1),
        "{} frames",
        without.frames_to_open
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
fn the_spyglass_narrows_the_view_for_the_renderer_and_the_lod_alike() {
    // The spyglass (docs/SPYGLASS.md): the zoomed FOV is the frame's, the LOD camera has it and
    // the magnification its detail follows, and zooming out restores both.
    use dwell_look::spyglass::{SPYGLASS, zoom_fov};
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
    rig.run_until(30, |_| false);
    let base = rig.scene.borrow().fov.unwrap();
    let m = SPYGLASS.steps[0].magnification;
    rig.game.as_mut().unwrap().spyglass.toggle(&SPYGLASS);
    rig.run_until(2, |_| false);
    let easing = rig.scene.borrow().fov.unwrap();
    assert!(
        easing < base && easing > zoom_fov(base, m),
        "eases in: {easing}"
    );
    rig.run_until(60, |_| false);
    let zoomed = rig.scene.borrow().fov.unwrap();
    assert!(
        (zoomed - zoom_fov(base, m)).abs() < 1e-9,
        "{zoomed} vs {base}"
    );
    let lod = rig.game.as_ref().unwrap().lod_camera().unwrap();
    assert_eq!(lod.fov_y_deg, zoomed, "the LOD selects with the zoomed FOV");
    assert!((lod.magnification - m).abs() < 1e-9);
    assert!((lod.detail_magnification - m.min(SPYGLASS.detail_boost)).abs() < 1e-9);
    // Your arms hide while zoomed (docs/CHARACTERS.md §9.1).
    assert!(!rig.scene.borrow().arms, "no arms in the spyglass");

    // Reduced motion: out at once.
    rig.game.as_mut().unwrap().reduced_motion = true;
    rig.game.as_mut().unwrap().spyglass.toggle(&SPYGLASS);
    rig.run_until(1, |_| false);
    assert_eq!(rig.scene.borrow().fov.unwrap(), base);
    assert!(rig.scene.borrow().arms, "the arms are back");
    // The Show your arms setting off hides them.
    rig.game.as_mut().unwrap().show_arms = false;
    rig.run_until(1, |_| false);
    assert!(!rig.scene.borrow().arms);
    let lod = rig.game.as_ref().unwrap().lod_camera().unwrap();
    assert_eq!((lod.fov_y_deg, lod.magnification), (base, 1.0));
}

#[test]
fn a_slow_device_walks_without_snapping() {
    // A device drawing ~5.5 frames a second (CI's software renderer with two pages,
    // e2e/play.spec.ts): each frame covers 11 of the server's 60 Hz ticks, over a 150 ms round
    // trip (as the e2e test's B page: `?netsim=150,…`).
    slow_device_keeps_the_servers_time(11, 5);
}

#[test]
fn a_device_at_under_two_frames_a_second_walks_and_jumps_without_snapping() {
    // CI's software renderer drew e2e/touch.spec.ts's phone page at 440–810 ms a frame
    // (2026-10-19): up to 48 of the server's ticks a frame, more than the 15 the client used to
    // catch up. It dropped the rest, the starved server repeated the held input (a held jump flew
    // and landed there), and the snapshot replays put the player back on the ground one predicted
    // tick into the jump (the test read a 0.112 m peak).
    slow_device_keeps_the_servers_time(48, 0);
}

/// A device whose frames each cover `steps` of the server's 60 Hz ticks walks, then holds jump.
/// Its prediction must keep the server's time, or the server fills the missing ticks by repeating
/// the last input (walking further, or jumping earlier, than the player did) and the replays drag
/// the player along, snapping it once a correction passes RECONCILE_SNAP_DISTANCE.
fn slow_device_keeps_the_servers_time(steps: usize, latency_steps: u32) {
    let mut rig = Rig::new();
    assert!(
        rig.run_until(60 * 30, |r| r
            .game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)),
        "the player is placed and the terrain around them arrives"
    );
    // Settle on the ground at full speed, then slow down.
    rig.run_until(60, |_| false);
    rig.latency_steps = latency_steps;
    let stats = |r: &Rig| r.game.as_ref().unwrap().debug_state().stats;
    let mut worst = 0.0f32;
    let mut slow_frames = |rig: &mut Rig, n: usize| {
        for _ in 0..n {
            let replays = stats(rig).replays;
            rig.slow_frame(steps);
            let s = stats(rig);
            if s.replays > replays {
                worst = worst.max(s.last_correction);
            }
        }
    };
    // Each phase lasts about a second of frames.
    let second = 60usize.div_ceil(steps);
    slow_frames(&mut rig, second.max(10));
    let before = stats(&rig);
    let start = rig.game.as_ref().unwrap().debug_state().feet;
    rig.input.set_key("KeyW", true);
    slow_frames(&mut rig, second);
    rig.input.set_key("KeyW", false);
    slow_frames(&mut rig, second);
    let feet = rig.game.as_ref().unwrap().debug_state().feet;
    let moved = ((feet[0] - start[0]).powi(2) + (feet[2] - start[2]).powi(2)).sqrt();
    assert!(moved > 2.0, "walked {moved} m");
    // Hold jump through the jump (as the touch button is held), then land: the predicted ticks
    // rise the whole jump (about 1.2 m), not one tick of it before a replay lands the player.
    let ground = feet[1];
    let last = telemetry::with(|t| t.ticks().last().map_or(0, |s| s.seq));
    rig.input.set_key("Space", true);
    slow_frames(&mut rig, second);
    rig.input.set_key("Space", false);
    slow_frames(&mut rig, second);
    let peak = telemetry::with(|t| {
        t.ticks()
            .filter(|s| s.seq > last)
            .map(|s| s.feet[1])
            .fold(ground, f64::max)
    });
    assert!(peak - ground > 1.0, "jumped {} m", peak - ground);
    let after = stats(&rig);
    // The prediction matches the server: no correction to speak of, so no snap.
    assert!(
        worst < 0.2,
        "a {worst} m correction: {before:?} -> {after:?}"
    );
    assert_eq!(after.snaps, before.snaps);
    // Predicted ticks keep up with the server's.
    let frames = 4 * second;
    let predicted = after.ticks - before.ticks;
    assert!(
        predicted as usize >= frames * steps - steps,
        "{predicted} ticks predicted for {} of the server's",
        frames * steps
    );
}

/// The fixture's server-only block (`shared/blocks/fixtures/test.json` `test:glow`), as a server
/// mod registers it from data (ADR 0038): this build's client has never heard of it.
fn glow_block() -> dwell_blocks::NewBlock {
    let mut row = *dwell_blocks::fixture::STATES
        .iter()
        .find(|s| s.name == "test:glow")
        .expect("the fixture has test:glow");
    row.shape_index = dwell_blocks::info(dwell_blocks::require("dwell:stone")).shape_index;
    dwell_blocks::NewBlock {
        name: "test:glow".into(),
        properties: Vec::new(),
        states: vec![row],
    }
}

/// The extensibility test of registry sync (ADR 0038): a block only the server has arrives by
/// sync, after the join prompt; the client draws it with the fallback look, stands on it exactly
/// where the server does, targets and breaks it, and another client sees it and its removal.
#[test]
fn a_block_only_the_server_has_arrives_by_sync_and_plays_like_any_other() {
    let mut rig = Rig::new();
    let server_glow = rig
        .core
        .server()
        .register_block(glow_block())
        .expect("registers");
    // A 5 × 5 pad of it under the spawn, in the server's world only (modified chunks are sent
    // explicitly), with room to stand above.
    let spawn = rig.core.server().config().spawn.expect("spawn");
    let (sx, sz) = (spawn[0].floor() as i32, spawn[2].floor() as i32);
    let world = rig.core.server().world();
    let top = (-160..dwell_world::row_of_m(spawn[1]) + 8)
        .rev()
        .find(|&y| dwell_blocks::info(world.get_voxel(sx, y, sz)).solid)
        .expect("ground under the spawn");
    for dx in -2..=2 {
        for dz in -2..=2 {
            world.set_voxel(sx + dx, top, sz + dz, server_glow);
            for up in 1..=3 {
                world.set_voxel(sx + dx, top + up, sz + dz, dwell_blocks::AIR);
            }
        }
    }
    for c in [
        dwell_world::chunk_of(sx - 2, top, sz - 2),
        dwell_world::chunk_of(sx + 2, top, sz + 2),
        dwell_world::chunk_of(sx - 2, top, sz + 2),
        dwell_world::chunk_of(sx + 2, top, sz - 2),
        dwell_world::chunk_of(sx, top + 3, sz),
    ] {
        if let Some(chunk) = world.find_mut(c) {
            chunk.bump_revision();
        }
    }

    // The join prompt lists it as new; Continue.
    assert!(rig.run_until(600, |r| matches!(
        r.session.state(),
        SessionState::Confirming { .. }
    )));
    let SessionState::Confirming { differences, .. } = rig.session.state().clone() else {
        unreachable!()
    };
    assert_eq!(differences.added.len(), 1);
    assert_eq!(differences.added[0].name, "test:glow");
    assert!(differences.different.is_empty() && differences.client_only.is_empty());
    rig.session.accept_registry();
    assert!(
        rig.run_until(60 * 30, |r| r
            .game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)),
        "joined, and the terrain around the player arrives"
    );

    // The client holds its own synced copy of the state, with the server's rules.
    let glow = rig.game.as_mut().unwrap().voxel(sx, top, sz);
    assert_eq!(dwell_blocks::info(glow).name, "test:glow");
    assert!(!dwell_blocks::is_build_state(glow));
    assert_ne!(
        glow, server_glow,
        "the client's id is its own, mapped at the boundary"
    );
    assert!(dwell_blocks::info(glow).solid);
    assert!(
        dwell_mesh::uses_fallback_look(glow),
        "its texture is in no atlas here"
    );

    // It collides as on the server: the player lands on its top, and prediction agrees.
    let pad = dwell_world::chunk_of(sx, top, sz);
    assert!(
        rig.run_until(600, |r| r.scene.borrow().chunks.contains(&pad)),
        "its chunk is meshed"
    );
    rig.run_until(120, |_| false);
    let state = rig.game.as_ref().unwrap().debug_state();
    let pid = state.player_id;
    let server_feet = rig.core.server().feet_of(pid).expect("server player");
    assert!(
        (state.feet[1] - dwell_world::m_of_row(top + 1)).abs() < 0.01,
        "stands on the pad: {:?} (top {top})",
        state.feet
    );
    assert!(
        (server_feet[1] - state.feet[1]).abs() < 0.01,
        "{server_feet:?} {:?}",
        state.feet
    );
    assert_eq!(state.stats.snaps, 0, "no prediction snaps");

    // A second client sees it too.
    rig.add_guest();
    assert!(
        rig.run_until(600, |r| {
            r.guest.as_ref().is_some_and(|(_, _, got)| {
                got.iter().any(|m| match m {
                    dwell_protocol::Message::ChunkData(c) => c
                        .voxels
                        .iter()
                        .any(|&v| dwell_blocks::try_info(v).is_some_and(|s| s.name == "test:glow")),
                    _ => false,
                })
            })
        }),
        "the guest gets the pad's chunk, glow mapped to its ids"
    );

    // Target it (looking straight down) and break it; both clients see it gone.
    rig.input.look.pitch = -89.0;
    let on_pad = |c: [i32; 3]| c[1] == top && (c[0] - sx).abs() <= 2 && (c[2] - sz).abs() <= 2;
    assert!(
        rig.run_until(30, |r| {
            r.game
                .as_ref()
                .unwrap()
                .debug_state()
                .target
                .is_some_and(|t| on_pad(t.cell))
        }),
        "the glow under the player is targeted"
    );
    let [x, y, z] = rig
        .game
        .as_ref()
        .unwrap()
        .debug_state()
        .target
        .unwrap()
        .cell;
    assert_eq!(rig.game.as_mut().unwrap().voxel(x, y, z), glow);
    let now = rig.now + 1000.0;
    assert!(rig.game.as_mut().unwrap().edit(EditAction::Break, now));
    // Predicted: gone at once; then the server's ack settles it.
    assert_eq!(rig.game.as_mut().unwrap().voxel(x, y, z), 0);
    assert!(
        rig.run_until(60, |r| r.game.as_ref().unwrap().prediction.pending() == 0),
        "the server's ack arrives"
    );
    assert_eq!(rig.game.as_mut().unwrap().voxel(x, y, z), 0);
    assert_eq!(rig.core.server().world().get_voxel(x, y, z), 0);
    assert!(
        rig.run_until(60, |r| r.guest.as_ref().is_some_and(|(_, _, got)| {
            got.iter().any(|m| {
                matches!(m, dwell_protocol::Message::VoxelModification(v)
            if v.chunks.iter().any(|c| c.changes.iter().any(|ch| ch.material == 0)))
            })
        })),
        "the guest sees it broken"
    );
}

/// A sandbox rig joined with its capabilities set, a guest watching, and `latency` server ticks
/// each way: edits take a while to come back, as over a network.
fn predicting_rig(latency: u32) -> Rig {
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
    rig.game
        .as_mut()
        .unwrap()
        .set_capabilities(region_edits, brushes, generator_version);
    rig.add_guest();
    // The guest joins (the same registry: no prompt) and asks for explicit chunks.
    assert!(rig.run_until(120, |r| {
        r.guest
            .as_ref()
            .is_some_and(|(s, _, _)| matches!(s.state(), SessionState::Joined { .. }))
    }));
    let (guest, _, _) = rig.guest.as_mut().unwrap();
    guest.send_control(&dwell_protocol::Message::WorldgenCheck(0));
    assert!(
        rig.run_until(300, |r| r.guest.as_ref().is_some_and(|(_, _, got)| got
            .iter()
            .any(|m| matches!(m, dwell_protocol::Message::ChunkData(_)))))
    );
    rig.run_until(60, |_| false);
    rig.latency_steps = latency;
    rig
}

/// Looks `pitch` down and `yaw` round until a floor or block cell is targeted; returns it.
fn target_at(rig: &mut Rig, yaw: f64, pitch: f64) -> [i32; 3] {
    rig.input.look.yaw = yaw;
    rig.input.look.pitch = pitch;
    assert!(rig.run_until(30, |r| {
        r.game.as_ref().unwrap().debug_state().target.is_some()
    }));
    rig.game
        .as_ref()
        .unwrap()
        .debug_state()
        .target
        .unwrap()
        .cell
}

fn host_voxel(rig: &mut Rig, c: [i32; 3]) -> u16 {
    rig.game.as_mut().unwrap().voxel(c[0], c[1], c[2])
}

fn server_voxel(rig: &mut Rig, c: [i32; 3]) -> u16 {
    rig.core.server().world().get_voxel(c[0], c[1], c[2])
}

/// The guest's view of a cell: the last modification it got that changes it, else air.
fn guest_voxel(rig: &Rig, c: [i32; 3]) -> u16 {
    let (_, _, got) = rig.guest.as_ref().unwrap();
    let mut out = 0;
    for m in got {
        if let dwell_protocol::Message::VoxelModification(v) = m {
            for ch in &v.chunks {
                for x in &ch.changes {
                    let i = i32::from(x.index);
                    let cell = [
                        ch.coord[0] * 32 + (i & 31),
                        ch.coord[1] * 32 + ((i >> 5) & 31),
                        ch.coord[2] * 32 + ((i >> 10) & 31),
                    ];
                    if cell == c {
                        out = x.material;
                    }
                }
            }
        }
    }
    out
}

#[test]
fn an_edit_shows_at_once_and_the_servers_ack_settles_it() {
    let mut rig = predicting_rig(8);
    let stone = dwell_blocks::require("dwell:stone");
    rig.game.as_mut().unwrap().interaction.select(0); // stone
    let floor = target_at(&mut rig, 0.0, -40.0);
    let p = [floor[0], floor[1] + 1, floor[2]];
    assert_eq!(host_voxel(&mut rig, p), 0);
    let now = rig.now + 1000.0;
    assert!(rig.game.as_mut().unwrap().edit(EditAction::Place, now));
    // Predicted: on the host at once, before the server has even received it.
    assert_eq!(host_voxel(&mut rig, p), stone, "shown before the echo");
    assert_eq!(server_voxel(&mut rig, p), 0);
    assert_eq!(rig.game.as_ref().unwrap().prediction.pending(), 1);
    // The ack settles it: the server, the host and the guest agree.
    assert!(rig.run_until(120, |r| r.game.as_ref().unwrap().prediction.pending() == 0));
    assert_eq!(server_voxel(&mut rig, p), stone);
    assert_eq!(host_voxel(&mut rig, p), stone);
    assert!(
        rig.run_until(30, |r| guest_voxel(r, p) == stone),
        "two clients agree"
    );
    let stats = rig.game.as_ref().unwrap().prediction.stats;
    assert_eq!((stats.refused, stats.corrected), (0, 0));

    // A brush stroke shows whole at once, every queued edit of it.
    let game = rig.game.as_mut().unwrap();
    game.brush_key(BrushAction::Toggle);
    let ball = dwell_world::brush::PRESETS
        .iter()
        .position(|p| p.name == "ball")
        .unwrap();
    game.brush_key(BrushAction::Preset(ball));
    game.brush.resize(4);
    // Far enough that the ball misses the player (the server leaves cells in a body alone).
    rig.input.look.pitch = -10.0;
    assert!(rig.run_until(30, |r| r.game.as_ref().unwrap().brush.target().is_some()));
    let t = rig.game.as_ref().unwrap().brush.target().unwrap().cell;
    let top = [t[0], t[1] + 7, t[2]];
    let now = rig.now;
    assert!(rig.game.as_mut().unwrap().brush_apply(false, now));
    assert_eq!(
        host_voxel(&mut rig, top),
        stone,
        "the whole stroke, at once"
    );
    assert_eq!(server_voxel(&mut rig, top), 0);
    assert!(rig.run_until(600, |r| r.game.as_ref().unwrap().prediction.pending() == 0));
    assert_eq!(server_voxel(&mut rig, top), stone);
    assert_eq!(host_voxel(&mut rig, top), stone);
    assert!(rig.run_until(30, |r| guest_voxel(r, top) == stone));
    assert_eq!(rig.game.as_ref().unwrap().prediction.stats.corrected, 0);
}

#[test]
fn a_refused_edit_rolls_back() {
    use dwell_sim::server::EditPolicy;
    let mut rig = predicting_rig(8);
    let stone = dwell_blocks::require("dwell:stone");
    // Nobody may edit now; the host's client doesn't know it.
    let host = rig.key.verifying_key().to_bytes();
    rig.core
        .host(4, EditPolicy::Nobody, EditPolicy::Everyone, host);
    rig.game.as_mut().unwrap().interaction.select(0);
    let floor = target_at(&mut rig, 0.0, -40.0);
    let p = [floor[0], floor[1] + 1, floor[2]];
    let now = rig.now + 1000.0;
    assert!(rig.game.as_mut().unwrap().edit(EditAction::Place, now));
    assert_eq!(host_voxel(&mut rig, p), stone, "predicted");
    assert!(rig.run_until(120, |r| r.game.as_ref().unwrap().prediction.pending() == 0));
    assert_eq!(host_voxel(&mut rig, p), 0, "rolled back");
    assert_eq!(server_voxel(&mut rig, p), 0);
    let stats = rig.game.as_ref().unwrap().prediction.stats;
    assert_eq!((stats.refused, stats.corrected), (1, 1));
}

#[test]
fn rapid_edits_and_a_refusal_reconcile_with_the_server() {
    use dwell_sim::server::EditPolicy;
    let mut rig = predicting_rig(8);
    let stone = dwell_blocks::require("dwell:stone");
    rig.game.as_mut().unwrap().interaction.select(0);
    let floor = target_at(&mut rig, 0.0, -40.0);
    let p = [floor[0], floor[1] + 1, floor[2]];
    // 1: place P, accepted.
    let mut now = rig.now + 1000.0;
    assert!(rig.game.as_mut().unwrap().edit(EditAction::Place, now));
    assert!(rig.run_until(30, |r| server_voxel(r, p) == stone));
    assert_eq!(
        rig.game.as_ref().unwrap().prediction.pending(),
        1,
        "its ack is on its way"
    );
    // The server stops taking edits; two more go out before the first's ack is back.
    let host = rig.key.verifying_key().to_bytes();
    rig.core
        .host(4, EditPolicy::Nobody, EditPolicy::Everyone, host);
    // 2: break P (aimed at the predicted block), refused.
    rig.frame();
    let aimed = rig
        .game
        .as_ref()
        .unwrap()
        .debug_state()
        .target
        .unwrap()
        .cell;
    assert_eq!(aimed, p, "the crosshair sees the predicted block");
    now += 200.0;
    assert!(rig.game.as_mut().unwrap().edit(EditAction::Break, now));
    assert_eq!(host_voxel(&mut rig, p), 0);
    // 3: place at another cell, refused.
    let other_floor = target_at(&mut rig, 120.0, -40.0);
    let q = [other_floor[0], other_floor[1] + 1, other_floor[2]];
    assert_ne!(q, p);
    now += 200.0;
    assert!(rig.game.as_mut().unwrap().edit(EditAction::Place, now));
    assert_eq!(host_voxel(&mut rig, q), stone);
    assert!(rig.game.as_ref().unwrap().prediction.pending() >= 2);
    // All acks back: the host shows exactly what the server has.
    assert!(rig.run_until(120, |r| r.game.as_ref().unwrap().prediction.pending() == 0));
    rig.run_until(30, |_| false);
    for c in [p, q] {
        assert_eq!(host_voxel(&mut rig, c), server_voxel(&mut rig, c), "{c:?}");
        assert_eq!(guest_voxel(&rig, c), server_voxel(&mut rig, c), "{c:?}");
    }
    assert_eq!(
        host_voxel(&mut rig, p),
        stone,
        "the refused break is undone"
    );
    assert_eq!(
        host_voxel(&mut rig, q),
        0,
        "the refused place is taken back"
    );
    let stats = rig.game.as_ref().unwrap().prediction.stats;
    assert_eq!(stats.refused, 2);
}

/// The guest's input on its next server step: `move_y` forward, `buttons` held.
fn guest_input(rig: &mut Rig, seq: u32, move_y: i8, buttons: u16) {
    let (guest, _, _) = rig.guest.as_mut().unwrap();
    guest.send_game_datagram(&dwell_protocol::Message::PlayerInput(
        dwell_protocol::PlayerInput {
            last_snapshot_tick: 0,
            inputs: vec![dwell_protocol::InputFrame {
                seq,
                move_x: 0,
                move_y,
                buttons,
                yaw: 0,
                pitch: 0,
            }],
        },
    ));
}

/// The guest's body as the host draws it.
fn guest_body(rig: &Rig) -> Option<PlayerView> {
    let host = rig.game.as_ref().unwrap().debug_state().player_id;
    let scene = rig.scene.borrow();
    scene
        .players
        .iter()
        .find(|(id, _)| **id != u32::from(host))
        .map(|(_, v)| *v)
}

/// Phase 40 C1 (docs/CHARACTERS.md §6, §9.2–9.3): client B (the host's game) draws client A (a
/// guest sending its own inputs) as the full body, playing the clip its controller state calls
/// for, while A walks, sprints, slides and jumps; and what each holds reaches the other.
#[test]
fn another_client_sees_your_body_walk_sprint_slide_and_jump_and_what_you_hold() {
    use dwell_protocol::{Message, held_item, input_buttons};
    let mut rig = predicting_rig(0);
    let mut seq = 1u32;
    // Plays `frames` frames of the guest's input; true once B draws A playing `role`.
    let mut play = |rig: &mut Rig, frames: usize, move_y: i8, buttons: u16, role: &str| {
        let mut seen = Vec::new();
        for _ in 0..frames {
            guest_input(rig, seq, move_y, buttons);
            seq += 1;
            rig.frame();
            if let Some(v) = guest_body(rig) {
                if seen.last() != Some(&v.role) {
                    seen.push(v.role);
                }
                if v.role == role {
                    return Ok(());
                }
            }
        }
        Err(seen)
    };
    play(&mut rig, 120, 0, 0, "idle").expect("A stands idle");
    play(&mut rig, 180, 127, 0, "walk").expect("A walks");
    play(&mut rig, 180, 127, input_buttons::RUN, "sprint").expect("A sprints");
    play(
        &mut rig,
        60,
        127,
        input_buttons::RUN | input_buttons::CROUCH,
        "slide",
    )
    .expect("A slides");
    play(&mut rig, 120, 0, 0, "idle").expect("A stops");
    play(&mut rig, 60, 0, input_buttons::JUMP, "jump").expect("A jumps");
    play(&mut rig, 120, 0, 0, "fall").expect("then falls");

    // What A holds reaches B: a block, then the brush (server ids; one registry here).
    let stone = dwell_blocks::require("dwell:stone");
    let (guest, _, _) = rig.guest.as_mut().unwrap();
    guest.send_control(&Message::HeldItem(stone));
    assert!(rig.run_until(60, |r| {
        guest_body(r).is_some_and(|v| v.held == dwell_client::render::Held::Block(stone))
    }));
    let (guest, _, _) = rig.guest.as_mut().unwrap();
    guest.send_control(&Message::HeldItem(held_item::BRUSH | stone));
    assert!(rig.run_until(60, |r| {
        guest_body(r).is_some_and(|v| v.held == dwell_client::render::Held::Brush(stone))
    }));
    // And what B holds reaches A: the host's game sends its hotbar block when it changes.
    rig.game.as_mut().unwrap().interaction.select(0);
    let want = rig.game.as_ref().unwrap().held();
    let dwell_client::render::Held::Block(block) = want else {
        panic!("the hotbar's first slot holds a block: {want:?}")
    };
    let host = rig.game.as_ref().unwrap().debug_state().player_id;
    assert!(rig.run_until(60, |r| {
        r.guest.as_ref().unwrap().2.iter().rev().any(|m| match m {
            Message::PhysicsSnapshot(s) => s
                .remotes
                .iter()
                .any(|p| p.player_id == host && p.held == block),
            _ => false,
        })
    }));
    // A's edit swings its arm on B's screen, even one the server refuses (out of reach here).
    let (guest, _, _) = rig.guest.as_mut().unwrap();
    guest.send_control(&Message::BlockEditRequest(
        dwell_protocol::BlockEditRequest {
            action: dwell_protocol::BlockEditAction::Break,
            cell: [1000, 0, 1000],
            face: 0,
            material: 0,
        },
    ));
    assert!(rig.run_until(60, |r| {
        guest_body(r).is_some_and(|v| v.action == Some("break"))
    }));
    // Your own arms are drawn (first person, nothing zoomed).
    assert!(rig.scene.borrow().arms);
}

/// Sounds for everything (docs/AUDIO.md §5): the torch in our hand crackles at the listener; one
/// placed in front of us crackles where it stands, on its own voice; putting the torch away stops
/// the hand's.
#[test]
fn torches_in_the_hand_and_placed_crackle_and_stop() {
    use dwell_game::sound::{FIRE_VOICE, SoundKind};
    let mut rig = predicting_rig(0);
    let torch = rig
        .game
        .as_ref()
        .unwrap()
        .interaction
        .palette()
        .iter()
        .position(|s| s.block == "dwell:torch")
        .expect("the torch is in the palette");
    let floor = target_at(&mut rig, 0.0, -40.0);
    let sounds = |rig: &mut Rig, frames: usize| {
        let mut all = Vec::new();
        for _ in 0..frames {
            rig.frame();
            all.extend(rig.game.as_mut().unwrap().take_sounds());
        }
        all
    };
    sounds(&mut rig, 10);
    rig.game.as_mut().unwrap().interaction.select(torch as i64);
    let held = sounds(&mut rig, 10);
    let hand = held
        .iter()
        .find(|e| e.kind == SoundKind::Burn)
        .unwrap_or_else(|| panic!("the held torch crackles: {held:?}"));
    assert_eq!(hand.at, None, "at the listener");
    assert!(hand.voice.is_some_and(|v| v >= FIRE_VOICE));
    // Placed on the floor ahead: a second fire, where it stands.
    let now = rig.now + 1000.0;
    assert!(rig.game.as_mut().unwrap().edit(EditAction::Place, now));
    let placed = sounds(&mut rig, 60);
    let at = placed
        .iter()
        .filter(|e| e.kind == SoundKind::Burn)
        .find_map(|e| e.at)
        .unwrap_or_else(|| panic!("the placed torch crackles: {placed:?}"));
    let d = ((at[0] - f64::from(floor[0]) - 0.5).powi(2)
        + (at[2] - f64::from(floor[2]) - 0.5).powi(2))
    .sqrt();
    assert!(d < 0.01, "at the torch's cell: {at:?} over {floor:?}");
    // Back to stone in the hand: the hand's voice stops, the placed one burns on.
    rig.game.as_mut().unwrap().interaction.select(0);
    let away = sounds(&mut rig, 10);
    assert_eq!(
        away.iter().filter(|e| e.kind == SoundKind::BurnEnd).count(),
        1,
        "{away:?}"
    );
}

// face-a-only: a client/server input-queue sync check on the default spawn; nothing in it depends on the face.
#[test]
fn a_stalled_server_catches_up_without_dropping_the_players_inputs() {
    // docs/BUGS.md 1: a server busy generating terrain for a fast flyer falls behind real time; the
    // client kept predicting, the server's queue grew past MAX_BUFFERED_INPUTS and it skipped the
    // excess, so its copy fell behind and every snapshot snapped the player back (rubber-banding).
    let mut rig = Rig::new();
    assert!(
        rig.run_until(60 * 20, |r| r
            .game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)),
        "terrain around the spawn loads"
    );
    rig.input.flight.set_allowed(true);
    rig.input.flight.set(true);
    rig.input.set_key("Space", true);
    rig.run_until(60, |_| false);
    rig.input.set_key("Space", false);
    rig.input.look.pitch = 0.0;
    rig.input.set_key("KeyW", true);
    rig.run_until(30, |_| false);
    let before = rig.game.as_ref().unwrap().debug_state().stats;
    // The server stalls for 1.5 s three times (its fixed step drops the backlog), the game keeps
    // running its frames.
    for _ in 0..3 {
        for _ in 0..90 {
            rig.game_frame();
            rig.now += 1000.0 / 60.0;
        }
        rig.run_until(120, |_| false);
    }
    rig.input.set_key("KeyW", false);
    rig.run_until(120, |_| false);
    let after = rig.game.as_ref().unwrap().debug_state().stats;
    let pid = rig.game.as_ref().unwrap().debug_state().player_id;
    let skipped = rig.core.server().stats_of(pid).unwrap().inputs_skipped;
    assert_eq!(
        skipped, 0,
        "the server skipped {skipped} of the player's inputs"
    );
    assert_eq!(
        after.snaps - before.snaps,
        0,
        "the player snapped back {} times (last correction {:.1} m)",
        after.snaps - before.snaps,
        after.last_correction
    );
}
