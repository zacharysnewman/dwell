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
    /// The face the camera was drawn on (its up), and the last day the scene was given.
    camera_face: Option<dwell_world::FaceFrame>,
    day: Option<dwell_look::daynight::DayState>,
    outline: Option<[f64; 3]>,
    fov: Option<f64>,
    wind: WindView,
    region_boxes: Vec<RegionBox>,
    brush: Option<BrushOutline>,
    /// The players drawn (the full bodies, with their clips), by id.
    players: std::collections::HashMap<u32, PlayerView>,
    /// Whether your arms were drawn last frame.
    arms: bool,
    /// The scene's upload budget a frame (None: unlimited), and the chunk mesh bytes handed to it
    /// since `frame_bytes` was last cleared, with the count of chunks.
    upload_budget: Option<u64>,
    /// A chunk mesh is still being written (the renderer's staged writes): none handed over.
    uploads_waiting: bool,
    frame_bytes: u64,
    frame_chunks: usize,
}

impl TerrainRenderer for Recorder {
    fn set_chunk(&mut self, coord: ChunkCoord, payload: Option<&ChunkMeshes>) {
        self.terrain.set_chunk(coord, payload);
        if let Some(m) = payload {
            self.frame_bytes +=
                (m.opaque.byte_len() + m.transparent.byte_len() + m.cutout.byte_len()) as u64;
            self.frame_chunks += 1;
        }
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
        face: dwell_world::FaceFrame,
        _roll: f64,
    ) {
        self.camera = Some(eye);
        self.camera_face = Some(face);
    }
    fn set_day(&mut self, day: &dwell_look::daynight::DayState, _seconds: f64) {
        self.day = Some(*day);
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
    fn upload_budget_bytes(&self) -> u64 {
        self.upload_budget.unwrap_or(u64::MAX)
    }
    fn uploads_waiting(&self) -> bool {
        self.uploads_waiting
    }
    fn stats(&self) -> RenderStats {
        RenderStats::default()
    }
}

struct Rig {
    generator: u32,
    seed: u64,
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
    /// Jobs the workers finish per poll (usize::MAX: all at once; a few: a device's workers).
    jobs_per_poll: usize,
}

/// The guest's session id on the local core.
const GUEST: u32 = 2;

const SEED: u64 = 1;

impl Rig {
    fn new() -> Self {
        Self::with_generator(GENERATOR_TERRAIN)
    }

    fn with_generator(generator: u32) -> Self {
        Self::with_seed(generator, SEED)
    }

    fn with_seed(generator: u32, seed: u64) -> Self {
        let core = LocalCore::create(
            seed,
            generator,
            None::<NoStorage>,
            Box::new(SeededEntropy(1)),
        )
        .unwrap();
        Self::with_core(generator, seed, core)
    }

    /// A rig over a local core made by the test (a sized or saved world).
    fn with_core(generator: u32, seed: u64, mut core: LocalCore) -> Self {
        core.connected(LOCAL_SESSION, TransportKind::Loopback, [0; 32]);
        let key = SigningKey::from_bytes(&[5; 32]);
        let mut session = Session::new([0; 32], key.verifying_key().to_bytes(), "Me", "t", 1000.0);
        session.start(0.0);
        Self {
            generator,
            seed,
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
            jobs_per_poll: usize::MAX,
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
            // As the web app does: the workers and the game at the world's size (BIG_MAP.md M2b).
            let size = self.session.world_size();
            let mut jobs =
                InlineJobs::new(Runner::world_sized(self.generator, self.seed, size, None));
            jobs.per_poll = self.jobs_per_poll;
            let pool = Pool::new(Box::new(jobs));
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
            // As the web app does (`app.rs`: always, the portals' shafts with the size).
            game.set_world(self.generator, self.seed, size);
            // As the web app does: the moon's travel on a server that carries players there.
            game.set_grid_frames(
                self.session
                    .capability(dwell_protocol::capability_names::GRID_FRAMES),
            );
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

/// BUGS.md #72: finished chunk meshes arrive in bursts, and a frame that uploaded them all at once
/// spent up to 15 ms on it on the phone page (CPU 4×). Under a byte budget no frame hands the
/// scene more than the budget (one mesh over it alone), and the view still finishes loading.
// face-a-only: the upload budget counts bytes, whichever face the chunks are on.
#[test]
fn no_frame_uploads_more_chunk_mesh_bytes_than_the_scenes_budget() {
    let mut rig = Rig::new();
    let budget = 256 * 1024;
    rig.scene.borrow_mut().upload_budget = Some(budget);
    let (mut frames, mut over, mut most, mut uploads) = (0, 0, 0u64, 0usize);
    let mut drained = false;
    for _ in 0..60 * 40 {
        {
            let mut s = rig.scene.borrow_mut();
            s.frame_bytes = 0;
            s.frame_chunks = 0;
        }
        rig.frame();
        frames += 1;
        let s = rig.scene.borrow();
        if s.frame_chunks > 1 && s.frame_bytes > budget {
            over += 1;
        }
        most = most.max(s.frame_bytes);
        uploads += s.frame_chunks;
        drop(s);
        let g = rig.game.as_ref();
        if uploads > 0
            && g.is_some_and(|g| {
                let t = g.debug_state().terrain;
                g.debug_state().terrain_ready && t.dirty == 0 && t.meshing == 0
            })
        {
            drained = true;
            break;
        }
    }
    assert_eq!(
        over, 0,
        "{over} of {frames} frames uploaded more than {budget} B of several meshes (most {most} B)"
    );
    assert!(
        drained,
        "the view finished meshing ({uploads} meshes in {frames} frames)"
    );
}

/// BUGS.md #72: a chunk mesh over the phone's budget is written a slice a frame
/// (render/staged.rs); while it is, no more meshes are handed over, so the writes can't pile up.
// face-a-only: the upload budget counts bytes, whichever face the chunks are on.
#[test]
fn no_mesh_is_handed_over_while_the_scene_is_still_writing_one() {
    let mut rig = Rig::new();
    assert!(
        rig.run_until(60 * 30, |r| !r.scene.borrow().chunks.is_empty()),
        "chunks arrive"
    );
    rig.scene.borrow_mut().uploads_waiting = true;
    let mut handed = 0;
    for _ in 0..120 {
        rig.scene.borrow_mut().frame_chunks = 0;
        rig.frame();
        handed += rig.scene.borrow().frame_chunks;
    }
    assert_eq!(
        handed, 0,
        "{handed} meshes handed over while one was being written"
    );
    rig.scene.borrow_mut().uploads_waiting = false;
    assert!(
        rig.run_until(60 * 10, |r| r.scene.borrow().frame_chunks > 0),
        "handed over again once written"
    );
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
    let mut now = rig.now + 1000.0;
    assert!(
        rig.game.as_mut().unwrap().edit(EditAction::Break, now),
        "the edit is sent"
    );
    // The ground takes several hits by hand (docs/BLOCK_DAMAGE.md): hit until it breaks.
    for _ in 0..12 {
        if rig.run_until(10, |r| r.game.as_mut().unwrap().voxel(x, y, z) == 0) {
            break;
        }
        now += 200.0;
        rig.game.as_mut().unwrap().edit(EditAction::Break, now);
    }
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

/// ARCHITECTURE.md §8.3 "Capabilities", STRUCTURES.md §9: the local server announces
/// `structures.place` at join, the client answers with its own, and a saved structure's paste
/// goes as a structure placement, which the creative world places.
#[test]
fn a_saved_structure_goes_as_a_structure_placement_to_a_server_that_announced_it() {
    use dwell_protocol::capability_names::STRUCTURES_PLACE;
    let mut rig = Rig::new();
    assert!(rig.run_until(60 * 30, |r| {
        r.game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)
    }));
    let SessionState::Joined {
        region_edits,
        player_id,
        ..
    } = rig.session.state().clone()
    else {
        panic!("joined")
    };
    assert!(
        rig.session.capability(STRUCTURES_PLACE),
        "the server announced it"
    );
    let theirs = rig
        .core
        .server()
        .client_capabilities(LOCAL_SESSION)
        .cloned()
        .expect("joined");
    assert!(theirs.has(STRUCTURES_PLACE), "the client answered");
    let structures = rig.session.structures();
    assert!(structures, "a creative world places structures");
    let game = rig.game.as_mut().unwrap();
    game.region.enabled = region_edits;
    game.set_structures(structures);
    game.set_structure_ops(rig.session.capability(STRUCTURES_PLACE));
    rig.input.look.pitch = -40.0;
    assert!(rig.run_until(30, |r| {
        r.game.as_ref().unwrap().debug_state().target.is_some()
    }));
    let stone = dwell_blocks::require("dwell:stone");
    let game = rig.game.as_mut().unwrap();
    game.region
        .load_clipboard([1, 1, 1], vec![stone], Vec::new(), "Test")
        .expect("loads in creative");
    assert!(game.tool(ToolAction::Paste, rig.now), "the paste is sent");
    let placed = |r: &mut Rig| {
        r.core
            .server()
            .stats_of(player_id)
            .is_some_and(|s| s.structure_edits_applied == 1)
    };
    assert!(rig.run_until(60, placed), "placed as a structure");
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
        generator_version: _,
        ..
    } = rig.session.state().clone()
    else {
        panic!("joined")
    };
    assert!(brushes, "a sandbox server announces brushes");
    let game = rig.game.as_mut().unwrap();
    game.set_capabilities(region_edits, brushes);
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

/// Theater mode (docs/THEATER_MODE.md): a camera path playing places the view, its field of view
/// and the viewer's time of day by the frame clock; stopping returns to the player's eyes.
#[test]
fn a_theater_path_drives_the_view() {
    let mut rig = Rig::new();
    assert!(rig.run_until(60 * 30, |r| r.game.is_some()));
    let feet = rig.game.as_ref().unwrap().debug_state().feet;
    let at = |dx: f64| [feet[0] + dx, feet[1] + 10.0, feet[2]];
    let path = dwell_theater::Path::parse(&serde_json::json!({
        "formatVersion": 1,
        "keyframes": [
            { "t": 0, "position": at(0.0), "look": [90, -10], "fov": 50, "timeOfDay": 0.25 },
            { "t": 2, "position": at(20.0), "timeOfDay": 0.75 }
        ],
        "segments": [{ "move": "linear" }]
    }))
    .unwrap();
    let g = rig.game.as_mut().unwrap();
    g.play_theater(path);
    g.step_theater(1000.0);
    g.step_theater(2000.0);
    let v = g.view().unwrap();
    assert!(
        (v.eye[0] - at(10.0)[0]).abs() < 1e-6,
        "half way at 1 s: {:?}",
        v.eye
    );
    assert_eq!(v.fov_deg, Some(50.0));
    assert!((v.yaw_deg - 90.0).abs() < 1e-6 && (v.pitch_deg + 10.0).abs() < 1e-6);
    assert_eq!(g.theater_state(), Some((1.0, 2.0, false)));
    g.step_theater(4000.0);
    assert_eq!(g.theater_state(), Some((2.0, 2.0, true)), "held at the end");
    g.stop_theater();
    assert_eq!(g.view(), None);
    assert_eq!(g.theater_state(), None);
}

/// The simulation viewpoint (docs/THEATER_MODE.md §7, T-4): a playing path's eye goes to the
/// server as `TheaterViewpoint` only where it is one (the capability and creative), at the rate,
/// and one clear follows when the path stops; elsewhere (adventure, an older server) none is sent.
#[test]
fn a_playing_path_sends_its_viewpoint_only_where_the_server_takes_it() {
    let mut rig = Rig::new();
    assert!(rig.run_until(60 * 30, |r| r.game.is_some()));
    let feet = rig.game.as_ref().unwrap().debug_state().feet;
    let at = |dx: f64| [feet[0] + dx, feet[1] + 10.0, feet[2]];
    let path = dwell_theater::Path::parse(&serde_json::json!({
        "formatVersion": 1,
        "keyframes": [
            { "t": 0, "position": at(0.0), "look": [90, -10] },
            { "t": 4, "position": at(400.0) }
        ],
        "segments": [{ "move": "linear" }]
    }))
    .unwrap();
    let viewpoints = |g: &mut Game<_>| -> Vec<Option<[f64; 3]>> {
        g.take_outbox()
            .into_iter()
            .filter_map(|m| match m {
                Send::Control(dwell_protocol::Message::TheaterViewpoint(v)) => Some(v),
                _ => None,
            })
            .collect()
    };
    let g = rig.game.as_mut().unwrap();
    viewpoints(g);
    // Off (adventure, or a server without `theater.viewpoint`): nothing on the wire.
    g.play_theater(path.clone());
    for i in 0..60 {
        g.step_theater(f64::from(i) * 1000.0 / 60.0);
    }
    g.stop_theater();
    g.step_theater(1100.0);
    assert!(viewpoints(g).is_empty(), "never sent where it is off");
    // On: four a second while playing, then one clear.
    g.set_theater_viewpoint(true);
    g.play_theater(path);
    for i in 0..60 {
        g.step_theater(10_000.0 + f64::from(i) * 1000.0 / 60.0);
    }
    let sent = viewpoints(g);
    assert_eq!(sent.len(), 4, "{sent:?}");
    assert!(sent.iter().all(Option::is_some));
    g.stop_theater();
    g.step_theater(11_100.0);
    g.step_theater(11_200.0);
    assert_eq!(viewpoints(g), vec![None], "one clear when it ends");
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
        fov_deg: None,
        up: None,
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
    slow_device_keeps_the_servers_time(11, 5, 0);
}

#[test]
fn a_device_at_under_two_frames_a_second_walks_and_jumps_without_snapping() {
    // CI's software renderer drew e2e/touch.spec.ts's phone page at 440–810 ms a frame
    // (2026-10-19): up to 48 of the server's ticks a frame, more than the 15 the client used to
    // catch up. It dropped the rest, the starved server repeated the held input (a held jump flew
    // and landed there), and the snapshot replays put the player back on the ground one predicted
    // tick into the jump (the test read a 0.112 m peak).
    slow_device_keeps_the_servers_time(48, 0, 0);
}

// face-a-only: the input queue's timing on the default spawn; nothing in it depends on the face.
#[test]
fn a_slow_devices_prediction_matches_whatever_the_phase_of_its_frames() {
    // The test above passed only for some lengths of the wait before its slow frames (found
    // 2026-11-10): how a slow device's frames line up with the server's ticks must not matter.
    // Each wait puts the walk's first input at a different point of the frames' bursts.
    for extra in 0..=8 {
        slow_device_keeps_the_servers_time(48, 0, extra);
    }
}

/// A device whose frames each cover `steps` of the server's 60 Hz ticks walks, then holds jump.
/// Its prediction must keep the server's time, or the server fills the missing ticks by repeating
/// the last input (walking further, or jumping earlier, than the player did) and the replays drag
/// the player along, snapping it once a correction passes RECONCILE_SNAP_DISTANCE.
fn slow_device_keeps_the_servers_time(steps: usize, latency_steps: u32, extra_frames: usize) {
    let mut rig = Rig::new();
    assert!(
        rig.run_until(60 * 30, |r| r
            .game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)),
        "the player is placed and the terrain around them arrives"
    );
    // Settle on the ground at full speed, then slow down.
    rig.run_until(60 + extra_frames, |_| false);
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
    let pid = rig.game.as_ref().unwrap().debug_state().player_id;
    let server = rig.core.server().stats_of(pid).unwrap();
    // The prediction matches the server: no correction to speak of, so no snap.
    assert!(
        worst < 0.2,
        "a {worst} m correction after {extra_frames} extra frames: {before:?} -> {after:?}; \
         server {server:?}"
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
        generator_version: _,
        ..
    } = rig.session.state().clone()
    else {
        panic!("joined")
    };
    rig.game
        .as_mut()
        .unwrap()
        .set_capabilities(region_edits, brushes);
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
    // The ½ m stone: one cell an edit (docs/ITEMS.md).
    let half = dwell_game::block_interaction::palette()
        .iter()
        .position(|s| s.item == "dwell:stone")
        .expect("a ½ m stone");
    rig.game.as_mut().unwrap().interaction.select(half as i64); // stone
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

/// Regression (owner, 2026-11-11): in creative, stone couldn't be broken without a pickaxe. Holding
/// a block (not a tool) it breaks in one hit, the client predicts it at once, and the server agrees
/// (no refusal to roll back, no flicker).
#[test]
fn in_creative_a_block_in_hand_breaks_stone_at_once_as_predicted() {
    let mut rig = predicting_rig(8);
    let stone = dwell_blocks::require("dwell:stone");
    let half = dwell_game::block_interaction::palette()
        .iter()
        .position(|s| s.item == "dwell:stone")
        .expect("a ½ m stone");
    rig.game.as_mut().unwrap().interaction.select(half as i64);
    let floor = target_at(&mut rig, 0.0, -40.0);
    let p = [floor[0], floor[1] + 1, floor[2]];
    let mut now = rig.now + 1000.0;
    assert!(rig.game.as_mut().unwrap().edit(EditAction::Place, now));
    assert!(rig.run_until(120, |r| r.game.as_ref().unwrap().prediction.pending() == 0));
    assert_eq!(server_voxel(&mut rig, p), stone, "placed");
    rig.frame();
    let aimed = rig
        .game
        .as_ref()
        .unwrap()
        .debug_state()
        .target
        .unwrap()
        .cell;
    assert_eq!(aimed, p, "the crosshair sees the stone");
    now += 200.0;
    assert!(rig.game.as_mut().unwrap().edit(EditAction::Break, now));
    assert_eq!(host_voxel(&mut rig, p), 0, "predicted at once");
    assert!(rig.run_until(120, |r| r.game.as_ref().unwrap().prediction.pending() == 0));
    assert_eq!(
        server_voxel(&mut rig, p),
        0,
        "the server broke it in one hit"
    );
    assert_eq!(host_voxel(&mut rig, p), 0);
    let stats = rig.game.as_ref().unwrap().prediction.stats;
    assert_eq!((stats.refused, stats.corrected), (0, 0), "no flicker");
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
    // The ½ m stone: one cell an edit (docs/ITEMS.md).
    let half = dwell_game::block_interaction::palette()
        .iter()
        .position(|s| s.item == "dwell:stone")
        .expect("a ½ m stone");
    rig.game.as_mut().unwrap().interaction.select(half as i64);
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
    // Leaves: one cell an edit (docs/ITEMS.md) and one hit to break (no `damage`, so the break
    // is predicted; stone takes a pickaxe, docs/BLOCK_DAMAGE.md).
    let stone = dwell_blocks::require("dwell:leaves");
    let half = dwell_game::block_interaction::palette()
        .iter()
        .position(|s| s.item == "dwell:leaves")
        .expect("½ m leaves");
    rig.game.as_mut().unwrap().interaction.select(half as i64);
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
    // The ½ m stone: one cell an edit (docs/ITEMS.md).
    let half = dwell_game::block_interaction::palette()
        .iter()
        .position(|s| s.item == "dwell:stone")
        .expect("a ½ m stone");
    rig.game.as_mut().unwrap().interaction.select(half as i64);
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
            rows: 1,
            down: false,

            point: None,
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
    // The ½ m stone: one cell an edit (docs/ITEMS.md).
    let half = dwell_game::block_interaction::palette()
        .iter()
        .position(|s| s.item == "dwell:stone")
        .expect("a ½ m stone");
    rig.game.as_mut().unwrap().interaction.select(half as i64);
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

// face-a-only: the edit check's own-body capsule on the default spawn; the overlap rule itself is face-local and tested in dwell-world.
#[test]
fn placing_a_block_into_your_own_body_is_not_predicted() {
    // docs/BUGS.md 7: the client predicted edits without its own body, so a block placed where the
    // player stands showed until the server refused it (into a player), then vanished.
    let mut rig = Rig::new();
    rig.input.look.pitch = -90.0;
    assert!(
        rig.run_until(60 * 20, |r| r
            .game
            .as_ref()
            .is_some_and(
                |g| g.debug_state().terrain_ready && g.debug_state().target.is_some()
            )),
        "terrain loads and the crosshair has a target"
    );
    rig.run_until(30, |_| false);
    rig.latency_steps = 20;
    let target = rig
        .game
        .as_ref()
        .unwrap()
        .debug_state()
        .target
        .expect("the ground below");
    let feet = rig.game.as_ref().unwrap().debug_state().feet;
    let [x, _, z] = target.cell;
    assert_eq!(
        [x, z],
        [feet[0].floor() as i32, feet[2].floor() as i32],
        "aimed at the ground under the player"
    );
    let before: Vec<u16> = (0..8)
        .map(|dy| rig.game.as_mut().unwrap().voxel(x, target.cell[1] + dy, z))
        .collect();
    let now = rig.now + 1000.0;
    rig.game.as_mut().unwrap().edit(EditAction::Place, now);
    for _ in 0..30 {
        rig.frame();
        let shown: Vec<u16> = (0..8)
            .map(|dy| rig.game.as_mut().unwrap().voxel(x, target.cell[1] + dy, z))
            .collect();
        assert_eq!(
            shown, before,
            "a block placed into the player's own cell was shown"
        );
    }
}

/// Towering (docs/ITEMS.md §5) on a device whose frames each cover `steps` of the server's ticks:
/// Place held looking straight down, four jumps of a 150 ms press and 750 ms after. The feet rise
/// by the blocks built under them.
fn towering_rise(steps: usize) -> f64 {
    use dwell_game::build_drag::Holder;
    let mut rig = Rig::with_generator(GENERATOR_SANDBOX);
    assert!(rig.run_until(60 * 30, |r| {
        r.game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)
    }));
    let SessionState::Joined {
        region_edits,
        brushes,
        generator_version: _,
        ..
    } = rig.session.state().clone()
    else {
        panic!("joined")
    };
    rig.game
        .as_mut()
        .unwrap()
        .set_capabilities(region_edits, brushes);
    target_at(&mut rig, 0.0, -89.0);
    rig.run_until(60, |_| false);
    let feet0 = rig.game.as_ref().unwrap().debug_state().feet[1];
    let now = rig.now;
    rig.game
        .as_mut()
        .unwrap()
        .edit_held(EditAction::Place, Holder::Mouse, now);
    // Frames of `steps` ticks covering at least `ms`.
    let frames = |ms: f64| ((ms * 60.0 / 1000.0) / steps as f64).ceil() as usize;
    for _ in 0..4 {
        rig.input.set_key("Space", true);
        for _ in 0..frames(150.0) {
            rig.slow_frame(steps);
        }
        rig.input.set_key("Space", false);
        for _ in 0..frames(750.0) {
            rig.slow_frame(steps);
        }
    }
    rig.game.as_mut().unwrap().release_place(Holder::Mouse);
    for _ in 0..frames(1000.0) {
        rig.slow_frame(steps);
    }
    rig.game.as_ref().unwrap().debug_state().feet[1] - feet0
}

#[test]
fn towering_builds_a_metre_a_jump_at_sixty_frames_a_second() {
    let rise = towering_rise(1);
    assert!((rise - 4.0).abs() < 0.01, "rose {rise} m in four jumps");
}

#[test]
fn towering_builds_a_metre_a_jump_at_five_and_three_frames_a_second() {
    // docs/BUGS.md 30: under the software renderer (CI ~5 fps, a Mac's ~3 fps) a held Place built
    // the first piece and no more: the server checked each piece against its view of the body, a
    // slow frame of inputs behind (refused), and the client looked for the jump's moment clear of
    // the next piece once a frame, which fell between frames.
    for steps in [12, 20] {
        let rise = towering_rise(steps);
        assert!(
            (rise - 4.0).abs() < 0.01,
            "rose {rise} m in four jumps at {steps} ticks a frame"
        );
    }
}

#[test]
fn a_jump_pressed_and_let_go_between_two_ticks_still_jumps() {
    // docs/BUGS.md 30: under the e2e software renderer, a jump press shorter than the gap between
    // ticks the client predicts (a slow frame, or the prediction held while the server catches up)
    // was never sampled, and the jump was lost; a tower then built nothing that jump.
    let mut rig = Rig::with_generator(GENERATOR_SANDBOX);
    assert!(rig.run_until(60 * 30, |r| {
        r.game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)
    }));
    rig.run_until(60, |_| false);
    let ground = rig.game.as_ref().unwrap().debug_state().feet[1];
    let last = telemetry::with(|t| t.ticks().last().map_or(0, |s| s.seq));
    rig.input.set_key("Space", true);
    rig.input.set_key("Space", false);
    rig.run_until(60, |_| false);
    let peak = telemetry::with(|t| {
        t.ticks()
            .filter(|s| s.seq > last)
            .map(|s| s.feet[1])
            .fold(ground, f64::max)
    });
    assert!(peak - ground > 1.0, "jumped {} m", peak - ground);
}

#[test]
fn the_waters_currents_near_the_player_reach_the_client_and_clear() {
    // Phase 53 G3a (docs/WATER_SURFACE_GRID.md §2c): the server sends the currents of the water
    // chunks near a player (`WaterCurrents`), which the client's prediction reads; a chunk gone
    // still is sent as zeros.
    let mut rig = Rig::with_generator(GENERATOR_SANDBOX);
    assert!(rig.run_until(60 * 30, |r| {
        r.game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)
    }));
    let feet = rig.game.as_ref().unwrap().debug_state().feet;
    let face = dwell_world::FaceFrame::at_height(feet[1]);
    let (key, i) =
        dwell_world::currents::current_key(face, feet[0].floor() as i32, feet[2].floor() as i32);
    let mut cells = Box::new([[0.0f32; 2]; dwell_world::currents::CA]);
    cells[i] = [0.6, -0.3];
    rig.core.server().world().currents.set(key, Some(cells));
    // None go out in a session's first 5 s (its terrain first): past that, within a pass.
    rig.run_until(360, |_| false);
    let at = rig.game.as_mut().unwrap().current_at(feet);
    assert!(
        (at[0] - 0.6).abs() < 0.03 && (at[1] + 0.3).abs() < 0.03,
        "the client has {at:?}"
    );
    rig.core.server().world().currents.set(key, None);
    rig.run_until(60, |_| false);
    assert_eq!(rig.game.as_mut().unwrap().current_at(feet), [0.0, 0.0]);
}

/// The dev tools' `goto` (docs/DEV_TOOLS.md "Finding places"): the world map's nearest water found
/// by the local core, the local player moved over it (the client's prediction follows the server's
/// body), and the water's cells found among the chunks that stream in there. Regression: the
/// client waited for the terrain round where the player had been, which the server stopped
/// streaming once it had moved the player, so it never followed.
// face-a-only: the generated world's face A spawn; the search and the move are face-local
// (find.rs's both-faces tests).
#[test]
fn the_map_finds_water_the_player_is_moved_over_it_and_its_cells_are_found() {
    use dwell_client::find::{Matcher, Query, find};
    let mut rig = Rig::new();
    assert!(rig.run_until(60 * 30, |r| {
        r.game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)
    }));
    let feet = rig.game.as_ref().unwrap().debug_state().feet;
    let json = rig.core.find_on_map("water", feet, 20_000.0, None, 0.5);
    assert!(json.starts_with("{\"at\":["), "{json}");
    assert!(
        rig.core
            .find_on_map("lava", feet, 100.0, None, 0.0)
            .contains("unknown water kind")
    );
    let nums: Vec<f64> = json["{\"at\":[".len()..]
        .split(']')
        .next()
        .unwrap()
        .split(',')
        .map(|v| v.parse().unwrap())
        .collect();
    let over = [nums[0], nums[1] + 3.0, nums[2]];
    // Not across the faces.
    assert!(!rig.core.teleport_local_player([over[0], -50.0, over[2]]));
    // Held there (a fall would take it under) while the client follows and the chunks stream.
    let mut found = None;
    for _ in 0..60 * 20 {
        assert!(rig.core.teleport_local_player(over));
        rig.frame();
        let g = rig.game.as_mut().unwrap();
        let at = g.debug_state().feet;
        if (at[0] - over[0]).hypot(at[2] - over[2]) < 1.0 {
            let q = Query {
                min_depth_m: 0.5,
                ..Query::new(Matcher::parse("water").unwrap())
            };
            found = find(g.world(), at, &q);
            if found.is_some() {
                break;
            }
        }
    }
    let g = rig.game.as_mut().unwrap();
    let at = g.debug_state().feet;
    let found = found.unwrap_or_else(|| {
        panic!(
            "the client follows to the water and finds its cells: {json}, at {at:?}, {} chunks",
            g.world().loaded_chunks()
        )
    });
    assert!(
        (f64::from(found.cell[0]) - over[0]).hypot(f64::from(found.cell[2]) - over[2]) < 64.0,
        "{found:?} near {over:?}"
    );
}

/// A sandbox world with a pool beside the spawn: water 3 m deep (rows `FLAT_TOP_Y` − 6 to − 1)
/// over x −2..=2, z 2..=6, its top level with the ground, and the player standing on the shore at
/// (0.5, 1.5), or `in_water` on the pool's bed at (0.5, 4.5).
fn pool_rig(in_water: bool) -> Rig {
    let mut rig = Rig::with_generator(GENERATOR_SANDBOX);
    let g = dwell_world::FLAT_TOP_Y;
    for x in -2..=2 {
        for z in 2..=6 {
            for y in g - 6..g {
                let w = rig.core.server().world();
                w.set_voxel(x, y, z, dwell_world::M.water);
            }
        }
    }
    assert!(rig.run_until(60 * 30, |r| {
        r.game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)
    }));
    let SessionState::Joined {
        region_edits,
        brushes,
        generator_version: _,
        ..
    } = rig.session.state().clone()
    else {
        panic!("joined")
    };
    rig.game
        .as_mut()
        .unwrap()
        .set_capabilities(region_edits, brushes);
    let feet = if in_water {
        [0.5, f64::from(g - 6) * dwell_world::CELL_H, 4.5]
    } else {
        [0.5, f64::from(g) * dwell_world::CELL_H, 1.5]
    };
    for _ in 0..60 {
        assert!(rig.core.teleport_local_player(feet));
        rig.frame();
    }
    rig.run_until(30, |_| false);
    assert_eq!(host_voxel(&mut rig, [0, g - 3, 4]), dwell_world::M.water);
    rig
}

/// The block in a cell, its `flooded` property aside (a block put into water is written flooded).
fn dry(m: u16) -> u16 {
    dwell_world::edit::flooded_for(m, dwell_world::M.air)
}

/// Drag building into water (docs/BUGS.md 37): Place held on the pool's bed, aimed at `yaw` and
/// `pitch`, in build mode `mode` (the modes' index), with the look set by `aim` each of 150 frames
/// after: the bed's cell placed on, and the pool's cells holding a block after (the server and
/// this client agreeing).
fn drag_into_pool(
    in_water: bool,
    mode: usize,
    (yaw, pitch): (f64, f64),
    mut aim: impl FnMut(usize, &mut KeyboardMouseInput),
) -> ([i32; 3], Vec<[i32; 3]>) {
    use dwell_game::build_drag::Holder;
    let mut rig = pool_rig(in_water);
    for _ in 0..mode {
        rig.game.as_mut().unwrap().cycle_build_mode();
    }
    let g = dwell_world::FLAT_TOP_Y;
    let bed = target_at(&mut rig, yaw, pitch);
    assert_eq!(bed[1], g - 7, "aimed at the pool's bed: {bed:?}");
    let now = rig.now;
    assert!(
        rig.game
            .as_mut()
            .unwrap()
            .edit_held(EditAction::Place, Holder::Mouse, now)
    );
    for i in 0..150 {
        aim(i, &mut rig.input);
        rig.frame();
    }
    rig.game.as_mut().unwrap().release_place(Holder::Mouse);
    rig.run_until(60, |_| false);
    let stone = dwell_blocks::require("dwell:stone");
    let mut out = Vec::new();
    for x in -2..=2 {
        for z in 2..=6 {
            for y in g - 6..g {
                let c = [x, y, z];
                let (client, server) = (host_voxel(&mut rig, c), server_voxel(&mut rig, c));
                assert_eq!(client, server, "the client and the server agree at {c:?}");
                if !dwell_blocks::info(server).liquid {
                    assert_eq!(dry(server), stone, "at {c:?}");
                    out.push(c);
                }
            }
        }
    }
    (bed, out)
}

// face-a-only: the sandbox's spawn is on face A and a rig's player can't cross; drag building's
// answers for fills into water on both faces: `build_drag`'s `a_stroke_builds_on_into_water`.
#[test]
fn a_line_drags_up_through_water_from_the_shore_and_from_in_it() {
    // docs/BUGS.md 37 (owner, 2026-11-06): a held Place built one piece into water and no more.
    // The server writes a block put into water flooded (`flooded_for`); the client took a fill
    // that came back flooded for one the server had left out, sent the line back to it, and the
    // line waited on its own block for good.
    for in_water in [false, true] {
        let start = if in_water { -55.0 } else { -70.0 };
        let (bed, built) = drag_into_pool(in_water, 0, (0.0, start), |i, input| {
            input.look.pitch = (start + i as f64).min(20.0);
        });
        let [x, y, z] = bed;
        assert_eq!(
            built,
            (1..=6).map(|r| [x, y + r, z]).collect::<Vec<_>>(),
            "six ½ m pieces up through the water (in the water: {in_water})"
        );
    }
}

// face-a-only: as `a_line_drags_up_through_water_from_the_shore_and_from_in_it`.
#[test]
fn a_flat_drags_a_floor_across_a_pools_bed_from_the_shore_and_from_in_it() {
    // docs/BUGS.md 37: Flat, the aim swept to and fro across the bed.
    for in_water in [false, true] {
        let pitch = if in_water { -55.0 } else { -70.0 };
        let (bed, built) = drag_into_pool(in_water, 1, (0.0, pitch), |i, input| {
            input.look.yaw = 40.0 * (i as f64 / 12.0).sin();
        });
        assert!(
            built.iter().all(|c| c[1] == bed[1] + 1),
            "a floor on the bed: {built:?}"
        );
        assert!(
            built.len() >= 3,
            "the floor is built on past its first piece (in the water: {in_water}): {built:?}"
        );
    }
}

/// Riding a raft (owner, 2026-11-09: "close chunks aren't being correctly prioritized. And
/// previously loaded chunks are not being unloaded behind you", on a raft): a 4 × 4 log raft
/// carries the player 4 m/s along a 400 m channel, the workers finishing one job a frame. The
/// LOD's camera follows the rider, the chunk 10 m ahead is drawn as chunks (not a LOD section)
/// every frame, and the chunks left behind past the requested radius are dropped. Not red on
/// main: kept as the guard for the report, which this does not reproduce natively.
/// face-a-only: the flat test world is face A's; the streaming centre is a world point (the
/// server's `view_center`, the client's `LodCamera`), no face-local logic.
#[test]
fn riding_a_raft_streams_the_terrain_ahead_and_drops_it_behind() {
    let mut rig = Rig::with_generator(dwell_world::GENERATOR_FLAT);
    rig.jobs_per_poll = 1;
    let g = dwell_world::FLAT_TOP_Y;
    for x in -8..700 {
        for z in -8..16 {
            for y in g - 6..g {
                rig.core
                    .server()
                    .world()
                    .set_voxel(x, y, z, dwell_world::M.water);
            }
        }
    }
    assert!(rig.run_until(60 * 30, |r| {
        r.game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)
    }));
    rig.game
        .as_mut()
        .unwrap()
        .enable_lod(dwell_client::game::LodSettings {
            pixel_error: 4.0,
            cache_bytes: 256 << 20,
            detail_distance_m: Some(256.0),
            full_mode: false,
        });
    let surface = f64::from(g) * dwell_world::CELL_H;
    let log = dwell_blocks::require("dwell:log[axis=y]");
    let cells: Vec<([i32; 3], u16)> = (0..4)
        .flat_map(|x| (0..4).map(move |z| ([x, 0, z], log)))
        .collect();
    let raft = rig.core.server().add_body(1, cells, [0.5, surface, 2.5]);
    rig.run_until(240, |_| false);
    for _ in 0..30 {
        assert!(rig.core.teleport_local_player([1.5, surface + 0.3, 3.5]));
        rig.frame();
    }
    rig.run_until(120, |_| false);
    rig.input.look.yaw = 90.0;
    let body = rig.core.server().body_handle(raft).expect("the raft");
    let mut coarse = 0;
    let mut coarse_run = 0;
    let mut worst_run = 0;
    for t in 0..60 * 85 {
        {
            let (_, physics, _) = rig.core.server().sim();
            let v = physics.velocity(body);
            physics.set_velocity(body, dwell_physics::DVec3::new(4.0, v.y, 0.0));
        }
        rig.frame();
        let game = rig.game.as_ref().unwrap();
        let feet = game.debug_state().feet;
        let camera = game.lod_camera().expect("the LOD selects").position;
        assert!(
            (camera[0] - feet[0]).abs() < 1.0,
            "the LOD's camera {camera:?} follows the rider {feet:?} (t {t})"
        );
        let ahead = dwell_world::chunk_of_pos([feet[0] + 10.0, feet[1] - 1.0, feet[2]]);
        let drawn = rig.scene.borrow().chunks.contains(&ahead) && game.chunk_visible(ahead);
        if drawn {
            coarse_run = 0;
        } else {
            coarse += 1;
            coarse_run += 1;
            worst_run = worst_run.max(coarse_run);
        }
    }
    let feet = rig.game.as_ref().unwrap().debug_state().feet;
    assert!(feet[0] > 300.0, "the raft carried the rider to {feet:?}");
    assert!(
        worst_run <= 30,
        "the chunk 10 m ahead was coarse {coarse} frames, at most {worst_run} in a row"
    );
    // Past the requested radius and its margin (13 chunks): gone from the client.
    let me = dwell_world::chunk_of_pos(feet);
    let behind: Vec<_> = rig
        .scene
        .borrow()
        .chunks
        .iter()
        .filter(|c| c.x < me.x - 14)
        .copied()
        .collect();
    assert!(behind.is_empty(), "left behind and still drawn: {behind:?}");
}

/// BUGS #54's long trip (owner, 2026-11-09: "a long ways down a river at or greater than walking
/// speed … for several minutes"): the player flies 8 m/s in a straight line for `seconds`, the
/// workers finishing one job a poll, with the LOD on (the default desktop settings). Returns the
/// longest wait (frames) of a chunk of the near ring (the 3 × 3 chunks of the ground's row around
/// the player) to be drawn as a chunk after the first 20 s (the start's load), the frame and chunk, the distance
/// flown, the chunks still drawn 14 or more behind, and a log every 30 s.
fn long_trip(rig: Rig, seconds: u32) -> ((u32, u32, ChunkCoord), f64, usize, String) {
    trip(rig, seconds, None)
}

/// [`long_trip`], the player carried at `carry` m/s along +x by setting its position every frame
/// (40 m over its start, so no hill stops it) when given, instead of flying under its own power.
fn trip(
    mut rig: Rig,
    seconds: u32,
    carry: Option<f64>,
) -> ((u32, u32, ChunkCoord), f64, usize, String) {
    rig.jobs_per_poll = 1;
    assert!(rig.run_until(60 * 30, |r| {
        r.game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)
    }));
    rig.game
        .as_mut()
        .unwrap()
        .enable_lod(dwell_client::game::LodSettings {
            pixel_error: 4.0,
            cache_bytes: 256 << 20,
            detail_distance_m: Some(256.0),
            full_mode: false,
        });
    rig.input.flight.set_allowed(true);
    rig.input.flight.set(true);
    rig.input.look.yaw = 90.0;
    rig.input.look.pitch = 0.0;
    // Up clear of the hills and trees, then on at that height.
    rig.input.set_key("Space", true);
    rig.run_until(90, |_| false);
    rig.input.set_key("Space", false);
    if carry.is_none() {
        rig.input.set_key("KeyW", true);
    }
    let start = rig.game.as_ref().unwrap().debug_state().feet;
    let mut waited: dwell_world::hash::HashMap<ChunkCoord, u32> = Default::default();
    let mut worst = (0, 0, ChunkCoord::new(0, 0, 0));
    let mut lines = Vec::new();
    for t in 1..=60 * seconds {
        if let Some(v) = carry {
            let x = start[0] + v * f64::from(t) / 60.0;
            assert!(
                rig.core
                    .teleport_local_player([x, start[1] + 40.0, start[2]])
            );
        }
        rig.frame();
        // The ground under the player (the first solid cell down, within 64 m).
        let feet = rig.game.as_ref().unwrap().debug_state().feet;
        let [x, z] = [feet[0].floor() as i32, feet[2].floor() as i32];
        let top = dwell_world::row_of_m(feet[1]);
        let air = dwell_world::M.air;
        let Some(ground) = (top - 128..=top)
            .rev()
            .find(|&y| rig.game.as_mut().unwrap().voxel(x, y, z) != air)
        else {
            continue;
        };
        let game = rig.game.as_ref().unwrap();
        let me = dwell_world::chunk_of(x, ground, z);
        let ring: Vec<ChunkCoord> = (-1..=1)
            .flat_map(|dx| (-1..=1).map(move |dz| ChunkCoord::new(me.x + dx, me.y, me.z + dz)))
            .collect();
        waited.retain(|c, _| ring.contains(c));
        for c in ring {
            if game.chunk_detail(c) == 0
                || !rig.scene.borrow().chunks.contains(&c) && game.chunk_drawable(c.to_array())
            {
                // Drawn as a chunk (or all air: nothing to draw).
                waited.remove(&c);
            } else {
                let n = waited.entry(c).or_insert(0);
                *n += 1;
                if t > 1200 && *n > worst.0 {
                    worst = (*n, t, c);
                }
            }
        }
        if t % (60 * 30) == 0 {
            let s = game.terrain_stats();
            lines.push(format!(
                "t {:>3}s x {:>6.0} loaded {} generating {} dirty {} meshing {} drawn {} worst {:?}",
                t / 60,
                feet[0],
                s.loaded,
                s.generating,
                s.dirty,
                s.meshing,
                rig.scene.borrow().chunks.len(),
                worst
            ));
        }
    }
    let feet = rig.game.as_ref().unwrap().debug_state().feet;
    let me = dwell_world::chunk_of_pos(feet);
    let behind = rig
        .scene
        .borrow()
        .chunks
        .iter()
        .filter(|c| c.x < me.x - 14)
        .count();
    (worst, feet[0] - start[0], behind, lines.join("\n"))
}

/// The long trip over the flat test world: past ~1.1 km from the axis its LOD bounds read every
/// column as outside the world (`flat_lod_bounds`), so the ground's sections were Empty and the
/// chunks under the player were never drawn (red before the fix: a near chunk waited 720 frames).
/// face-a-only: the flat test world is face A's; the streaming centre is a world point.
#[test]
fn a_long_trip_over_the_flat_world_keeps_the_near_ring_at_full_detail() {
    let (worst, flown, behind, log) =
        long_trip(Rig::with_generator(dwell_world::GENERATOR_FLAT), 180);
    assert!(flown > 1_300.0, "flew {flown} m\n{log}");
    assert!(
        worst.0 <= 120,
        "a near chunk waited {} frames to be drawn as a chunk (frame {}, {:?})\n{log}",
        worst.0,
        worst.1,
        worst.2
    );
    assert_eq!(behind, 0, "left behind and still drawn\n{log}");
}

/// The long trip over generated terrain (seed 7, the owner's river world), the player carried
/// 5 m/s in a straight line 40 m over its start (BUGS #54): every near chunk is drawn as a chunk
/// within 2 s of coming into the ring.
/// face-a-only: the spawn is on face A; the trip's path is a world line.
#[test]
fn a_long_trip_over_the_terrain_keeps_the_near_ring_at_full_detail() {
    let (worst, flown, behind, log) = trip(Rig::with_seed(GENERATOR_TERRAIN, 7), 90, Some(5.0));
    eprintln!("worst near-chunk wait {worst:?}\n{log}");
    assert!(flown > 400.0, "carried {flown} m\n{log}");
    assert!(
        worst.0 <= 120,
        "a near chunk waited {} frames to be drawn as a chunk (frame {}, {:?})\n{log}",
        worst.0,
        worst.1,
        worst.2
    );
    assert_eq!(behind, 0, "left behind and still drawn\n{log}");
}

#[test]
fn with_the_lod_the_loading_screen_opens_on_the_terrain_at_a_slow_pages_frame_rate() {
    // CI's software renderer draws ~5 fps, a slow phone not much more. On the flat world with the
    // LOD the screen opened on the near ring in ~150 frames at 60 fps (2.5 s), but at 5 fps it
    // waited out its 20 s timeout (e2e join.spec.ts:55 ran into its 30 s), with 168 of 179 shown:
    // (1) air chunks above the player whose level-1 sections were made, unclassified, under an
    // empty level-2 section on the way down to the forced chunks (`shows_nothing_at` stopped at
    // them), and (2) ground chunks whose level-1 sections' partners beyond the ring were not yet
    // meshed: a few mesh starts a frame, the workers idle most of each 200 ms frame.
    use dwell_game::loading::{LOADING, near_ring_offsets};
    let mut rig = Rig::with_generator(dwell_world::GENERATOR_FLAT);
    rig.loading_screen = true;
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
    let mut frames = 0;
    while rig
        .game
        .as_ref()
        .unwrap()
        .debug_state()
        .loading_opened
        .is_none()
        && frames < 200
    {
        rig.slow_frame(12);
        frames += 1;
    }
    let pos = rig.game.as_ref().unwrap().position();
    let me = dwell_world::chunk_of_pos(pos);
    let mut missing = Vec::new();
    for o in near_ring_offsets(LOADING.radius_chunks) {
        let k = [me.x + o[0], me.y + o[1], me.z + o[2]];
        let c = ChunkCoord::from_array(k);
        if !dwell_world::BUILD_VOLUME.chunk_may_hold(c) {
            continue;
        }
        let g = rig.game.as_mut().unwrap();
        let drawable = g.chunk_drawable(k);
        let shown = g
            .lod()
            .is_none_or(|l| l.chunk_visible(c) || l.shows_nothing_at(c));
        if !(drawable && shown) {
            missing.push((o, drawable));
        }
    }
    let d = rig.game.as_ref().unwrap().debug_state();
    let (how, after) = d.loading_opened.expect("opened");
    assert_eq!(
        how,
        dwell_game::loading::Opened::Ready,
        "after {frames} frames ({after} s); not shown (offset, drawable): {missing:?}; lod {:?}",
        d.lod
    );
    assert!(after < 10.0, "opened after {after} s ({frames} frames)");
}

/// The streaming report (docs/DEV_TOOLS.md, BUGS.md #54) of a joined client with the LOD: every
/// part is there, the near ring lists each chunk within ±3 chunks (in the world's rows), a chunk
/// not drawn in full says why, and the JSON reads back as it was written.
// face-a-only: the shape of the report; the per-chunk wait reasons are tested on both faces in
// dwell-game's `a_chunk_status_names_the_neighbour_it_waits_on`.
#[test]
fn the_streaming_report_describes_the_near_ring_pool_streamer_and_lod() {
    use dwell_client::game::streaming_report::NEAR_RING_CHUNKS;
    let mut rig = Rig::new();
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
    assert!(rig.run_until(60 * 30, |r| {
        r.game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)
    }));
    let report = rig
        .game
        .as_ref()
        .unwrap()
        .streaming_report(dwell_client::telemetry::now_ms());
    for key in [
        "player",
        "nearRing",
        "pool",
        "streamer",
        "lod",
        "frames",
        "creatures",
    ] {
        assert!(report.get(key).is_some(), "{key} missing");
    }
    for key in ["feet", "velocity", "speed", "face", "flying"] {
        assert!(report["player"].get(key).is_some(), "player.{key} missing");
    }
    assert_eq!(report["player"]["face"], "A");
    for key in ["queues", "focus", "nearJobsM", "slowFrameScale", "inFlight"] {
        assert!(report["pool"].get(key).is_some(), "pool.{key} missing");
    }
    for key in [
        "cacheBytes",
        "cacheBudget",
        "drawnByLevel",
        "selectionAgeMs",
    ] {
        assert!(report["lod"].get(key).is_some(), "lod.{key} missing");
    }
    assert!(report["streamer"]["loaded"].as_u64().unwrap() > 0);
    // Every chunk of the ±3 cube in the world's rows.
    let me = report["player"]["chunk"].as_array().unwrap()[1]
        .as_i64()
        .unwrap() as i32;
    let rows = (me - NEAR_RING_CHUNKS..=me + NEAR_RING_CHUNKS)
        .filter(|y| (dwell_world::MIN_CHUNK_Y..=dwell_world::MAX_CHUNK_Y).contains(y))
        .count();
    let side = (2 * NEAR_RING_CHUNKS + 1) as usize;
    let chunks = report["nearRing"]["chunks"].as_array().unwrap();
    assert_eq!(chunks.len(), rows * side * side);
    for c in chunks {
        let flags: Vec<&str> = c["flags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f.as_str().unwrap())
            .collect();
        assert!(c.get("lod").is_some(), "the LOD's view of {c}");
        if flags.contains(&"drawn") || flags.contains(&"outsideVolume") {
            assert!(c.get("waits").is_none(), "{c}");
        } else {
            let reasons = c["waits"]["reasons"].as_array().expect("why it waits");
            assert!(!reasons.is_empty(), "a chunk not drawn says why: {c}");
        }
    }
    let text = report.to_string();
    assert!(text.len() < 200_000, "{} bytes", text.len());
    let back: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(back, report);
}

// The far ecology in a local world (docs/ECOLOGY.md §6, §9): the browser's local server runs the
// same `Server` with the world maps the worker installed (`installWorldMap`), so its ecology runs,
// is saved in the world file's `ecology` table and reaches its own client and its guests.

/// Installs both faces' maps from their bytes, as the local worker does before the world starts
/// (worker.ts: `installWorldMap`, the bytes the page built or kept).
fn install_maps_as_the_worker_does(seed: u64) {
    for f in [dwell_world::Face::A, dwell_world::Face::B] {
        let bytes =
            dwell_worldgen::worldmap::build_bytes(dwell_worldgen::bifacial::face_seed(seed, f));
        dwell_worldgen::worldmap::install(&bytes).expect("the map installs");
    }
}

/// A local world as the worker opens it (jobs in the server-jobs worker, so nothing on the tick
/// computes a map), its ecology a world day a tick.
fn ecology_core(file: dwell_storage::BytesFile) -> LocalCore {
    let day_ticks = dwell_sim::server::default_settings(1.0)
        .clock
        .day_length_s()
        * f64::from(dwell_protocol::SIM_HZ);
    LocalCore::create_tuned(
        dwell_world::Face::A,
        false,
        None,
        false,
        false,
        SEED,
        GENERATOR_TERRAIN,
        Some(file),
        Box::new(SeededEntropy(1)),
        dwell_sim::water::host::Placement::Inline,
        true,
        |c| c.ecology_speed = day_ticks,
    )
    .unwrap()
}

#[test]
fn a_local_world_runs_the_far_ecology_on_both_faces_saves_and_reloads_it() {
    // both-faces: the ecology holds both faces' ecosystems, each checked here.
    install_maps_as_the_worker_does(SEED);
    let file = dwell_storage::BytesFile::default();
    let mut core = ecology_core(file.clone());
    assert!(core.persisted);
    // Nobody online: it waits until 10 days behind, then catches up in big steps (§6). Run until
    // it has stepped and is under 8 days behind (so the reload's first tick steps nothing).
    let (mut ticks, mut slowest) = (0, 0.0f64);
    loop {
        core.advance(1.0 / 60.0);
        ticks += 1;
        slowest = slowest.max(core.server().ecology_stats().1);
        let s = core.server().ecology_snapshot();
        if s.running && s.day >= 30.0 && s.clock_day - s.day < 8.0 {
            break;
        }
        assert!(ticks < 200, "the ecology steps by the world's days: {s:?}");
    }
    let before = core.server().ecology_snapshot();
    assert!(before.enabled && before.running);
    for face in [dwell_world::Face::A, dwell_world::Face::B] {
        let herds = before.groups.iter().filter(|g| g.face == face);
        assert!(
            herds.clone().any(|g| g.species == "dwell:deer"),
            "deer on face {face:?}"
        );
        assert!(
            herds.clone().any(|g| g.species == "dwell:wolf"),
            "wolves on face {face:?}"
        );
        assert_eq!(core.server().ecosystem(face).unwrap().day, before.day);
    }

    // Memory: the islands (the land cells' most forage and ground class) and the ecosystems
    // (forage per land cell, the groups) of both faces, measured; the world maps themselves are
    // the installed ones, shared.
    let bytes = core.server().ecology_heap_bytes();
    let record = core.server().ecology_record().len();
    println!(
        "ecology: {bytes} bytes for both faces ({} groups), a {record}-byte record; slowest tick \
         {slowest:.1} ms",
        before.groups.len()
    );
    // 12.5 MB before the land-only island with f32 forage (docs/ECOLOGY.md §6, "Cost"); an iOS
    // Safari tab is tight on memory.
    assert!(
        bytes <= 3 << 20,
        "the ecology adds at most 3 MB: {bytes} bytes"
    );
    assert!(
        record <= 200 << 10,
        "the ecology's saved record is at most 200 KB: {record} bytes"
    );

    // Saved in the world file's `ecology` table; a save with nothing stepped since writes nothing
    // (the record is megabytes and a local world saves every 5 s); a reload continues exactly.
    assert!(core.save());
    let writes = core.server().ecology_writes();
    assert!(writes >= 1);
    assert!(core.save());
    assert_eq!(
        core.server().ecology_writes(),
        writes,
        "an unchanged record is not written again"
    );
    drop(core);
    let mut reloaded = ecology_core(dwell_storage::BytesFile::new(file.bytes()));
    reloaded.advance(1.0 / 60.0);
    let after = reloaded.server().ecology_snapshot();
    assert!(after.running);
    assert_eq!(after.day, before.day);
    assert_eq!(
        after.groups, before.groups,
        "the herds and packs reload as saved"
    );
    // And it goes on stepping by the world's days.
    for _ in 0..30 {
        reloaded.advance(1.0 / 60.0);
    }
    assert!(reloaded.server().ecology_snapshot().day > before.day);
    assert_eq!(reloaded.server().ecology_writes(), 0);
    assert!(reloaded.save());
    assert_eq!(
        reloaded.server().ecology_writes(),
        1,
        "a stepped ecology is saved"
    );
}

#[test]
fn a_local_worlds_far_herds_reach_its_own_client_and_a_guest() {
    // both-faces: one snapshot carries both faces' groups, each checked here.
    install_maps_as_the_worker_does(SEED);
    let mut rig = Rig::new();
    rig.add_guest();
    let mut asked = false;
    let told = rig.run_until(60 * 30, |r| {
        if !asked
            && let Some((g, _, _)) = &mut r.guest
            && matches!(g.state(), SessionState::Joined { .. })
        {
            // A guest's client asks as the host's does (on the Inventory's ECOLOGY flag).
            g.send_control(&dwell_protocol::Message::EcologyHello);
            asked = true;
        }
        let host = r.game.as_ref().is_some_and(|g| g.ecology.is_some());
        let guest = r.guest.as_ref().is_some_and(|(_, _, got)| {
            got.iter()
                .any(|m| matches!(m, dwell_protocol::Message::Ecology(_)))
        });
        host && guest
    });
    assert!(told, "the local client and the guest get the far groups");
    let host = rig.game.as_ref().unwrap().ecology.clone().unwrap();
    for face in [0u8, 1] {
        assert!(
            host.groups
                .iter()
                .any(|g| g.face == face && g.species == "dwell:deer"),
            "the host's client sees deer on face {face}"
        );
        assert!(
            host.groups
                .iter()
                .any(|g| g.face == face && g.species == "dwell:wolf"),
            "and wolves on face {face}"
        );
    }
    // The client's read-out lists the server's far groups (the near ones are creatures).
    let snap = rig.core.server().ecology_snapshot();
    assert!(snap.running);
    assert!(!host.groups.is_empty() && host.groups.len() <= snap.groups.len());
}

#[test]
fn a_test_generator_local_world_has_no_ecology() {
    // face-a-only: a world with no ecology has none on either face; nothing face-local to compare.
    let mut rig = Rig::with_generator(GENERATOR_SANDBOX);
    assert!(rig.run_until(60 * 5, |r| r.game.is_some()));
    for _ in 0..(60 * 3) {
        rig.frame();
    }
    let snap = rig.core.server().ecology_snapshot();
    assert!(!snap.running && snap.groups.is_empty());
    assert!(
        rig.game.as_ref().unwrap().ecology.is_none(),
        "no far groups are sent"
    );
}

/// WORK_QUEUE item 25 (owner, 2026-11-12: "the gashes in blocks from tools show up on click instead
/// of when the animation lines up with the block. We want that timing."): a pickaxe's hit on stone
/// shows no mark, no hit sound and no chips before its swing's contact (`contactS`), and its mark
/// once the server lands it there.
#[test]
fn a_pickaxe_hit_marks_the_stone_at_its_contact_not_on_the_click() {
    use dwell_game::sound::SoundKind;
    let mut rig = predicting_rig(2);
    let stone = dwell_blocks::require("dwell:stone");
    let palette = dwell_game::block_interaction::palette();
    let half = palette
        .iter()
        .position(|s| s.item == "dwell:stone")
        .expect("a ½ m stone");
    let pick = palette
        .iter()
        .position(|s| s.tool == Some("dwell:stone_pickaxe"))
        .expect("the stone pickaxe on the palette");
    rig.game.as_mut().unwrap().interaction.select(half as i64);
    let floor = target_at(&mut rig, 0.0, -40.0);
    let p = [floor[0], floor[1] + 1, floor[2]];
    assert!(rig.game.as_mut().unwrap().edit(EditAction::Place, rig.now));
    assert!(rig.run_until(120, |r| r.game.as_ref().unwrap().prediction.pending() == 0));
    assert_eq!(server_voxel(&mut rig, p), stone, "placed");
    rig.game.as_mut().unwrap().interaction.select(pick as i64);
    for _ in 0..10 {
        rig.frame();
    }
    rig.game.as_mut().unwrap().take_sounds();
    let contact = dwell_protocol::chopping::break_contact_s(Some("dwell:stone_pickaxe")) * 1000.0;
    assert!(contact > 100.0, "the pickaxe strikes after the click");
    let click = rig.now;
    assert!(rig.game.as_mut().unwrap().edit(EditAction::Break, click));
    let hit = |s: &[dwell_game::sound::SoundEvent]| s.iter().any(|e| e.kind == SoundKind::Hit);
    assert!(
        !hit(&rig.game.as_mut().unwrap().take_sounds()),
        "no hit sound on the click"
    );
    while rig.now + 1000.0 / 60.0 < click + contact {
        rig.frame();
        let g = rig.game.as_mut().unwrap();
        assert!(
            g.damage_at(p).is_none(),
            "no mark {:.0} ms into the swing (contact {contact:.0} ms)",
            rig.now - click
        );
        assert!(!hit(&g.take_sounds()), "no hit sound before contact");
    }
    // One more swing waits for this one's contact.
    assert!(
        !rig.game.as_mut().unwrap().edit(EditAction::Break, rig.now),
        "a swing at a time"
    );
    let mut heard = false;
    assert!(rig.run_until(60, |r| {
        let g = r.game.as_mut().unwrap();
        heard |= hit(&g.take_sounds());
        g.damage_at(p).is_some()
    }));
    assert!(heard, "the hit sounds at contact");
    assert!(rig.now >= click + contact, "marked at contact");
    assert_eq!(host_voxel(&mut rig, p), stone, "one hit of five: it stands");
}

/// Two ½ m stones on the sandbox's floor, `a` ahead (yaw 0°) and `b` beside it (yaw 35°), both 35° down, the
/// stone pickaxe in hand and `hits.at_contact` on as the session announced it; the player's id.
fn two_stones_and_a_pickaxe(rig: &mut Rig) -> ([i32; 3], [i32; 3], u16) {
    use dwell_protocol::capability_names::HITS_AT_CONTACT;
    let SessionState::Joined { player_id, .. } = rig.session.state().clone() else {
        panic!("joined")
    };
    let on = rig.session.capability(HITS_AT_CONTACT);
    assert!(on, "the local server announces hits.at_contact");
    let stone = dwell_blocks::require("dwell:stone");
    let palette = dwell_game::block_interaction::palette();
    let half = palette
        .iter()
        .position(|s| s.item == "dwell:stone")
        .expect("a ½ m stone");
    let pick = palette
        .iter()
        .position(|s| s.tool == Some("dwell:stone_pickaxe"))
        .expect("the stone pickaxe on the palette");
    let g = rig.game.as_mut().unwrap();
    g.set_hits_at_contact(on);
    g.interaction.select(half as i64);
    let mut placed = Vec::new();
    for yaw in [0.0, B_YAW] {
        let floor = target_at(rig, yaw, -35.0);
        let p = [floor[0], floor[1] + 1, floor[2]];
        assert!(rig.game.as_mut().unwrap().edit(EditAction::Place, rig.now));
        assert!(rig.run_until(120, |r| r.game.as_ref().unwrap().prediction.pending() == 0));
        assert_eq!(
            server_voxel(rig, p),
            stone,
            "placed at {yaw}° {floor:?} → {p:?}"
        );
        placed.push(p);
    }
    let (a, b) = (placed[0], placed[1]);
    assert_ne!(a, b);
    rig.game.as_mut().unwrap().interaction.select(pick as i64);
    for _ in 0..10 {
        rig.frame();
    }
    assert_eq!(target_at(rig, 0.0, -35.0), a, "a at yaw 0°");
    assert_eq!(target_at(rig, B_YAW, -35.0), b, "b at yaw 35°");
    (a, b, player_id)
}

/// Where stone B is from the player.
const B_YAW: f64 = 35.0;

fn pickaxe_contact_ms() -> f64 {
    dwell_protocol::chopping::break_contact_s(Some("dwell:stone_pickaxe")) * 1000.0
}

fn hits_heard(rig: &mut Rig) -> usize {
    rig.game
        .as_mut()
        .unwrap()
        .take_sounds()
        .iter()
        .filter(|e| e.kind == dwell_game::sound::SoundKind::Hit)
        .count()
}

/// BUGS.md #111 (owner, 2026-11-12: "if I click on a block, and move my cursor to aim at the block
/// below it, I'll see and hear the tool hit, but it won't be on the block I'm aiming at, it'll be
/// the one I clicked on first before the hit played"): clicked on stone A, aimed at stone B by the
/// swing's contact, the hit (the server's damage, the mark, the sound) is B's and A is untouched;
/// the server takes it as sent (nothing refused, nothing replayed).
// face-a-only: the sandbox's floor and aim; the server's side is both faces' (dwell-sim damage).
#[test]
fn a_pickaxe_hit_lands_on_what_is_aimed_at_its_contact_not_the_block_clicked() {
    let mut rig = predicting_rig(2);
    let (a, b, player_id) = two_stones_and_a_pickaxe(&mut rig);
    assert_eq!(target_at(&mut rig, 0.0, -35.0), a);
    hits_heard(&mut rig);
    let before = rig.core.server().stats_of(player_id).unwrap();
    let click = rig.now;
    assert!(rig.game.as_mut().unwrap().edit(EditAction::Break, click));
    // The aim moves to B before the contact.
    rig.input.look.yaw = B_YAW;
    let mut heard = 0;
    while rig.now + 1000.0 / 60.0 < click + pickaxe_contact_ms() {
        rig.frame();
        heard += hits_heard(&mut rig);
    }
    assert_eq!(heard, 0, "no hit before the contact");
    assert!(rig.run_until(60, |r| r.game.as_ref().unwrap().damage_at(b).is_some()));
    heard += hits_heard(&mut rig);
    for _ in 0..30 {
        rig.frame();
        heard += hits_heard(&mut rig);
    }
    assert_eq!(heard, 1, "one hit sound, at the contact");
    let server = rig.core.server();
    assert_eq!(
        server.damage_of(b).map(|d| d.hits),
        Some(1),
        "B took the hit"
    );
    assert!(server.damage_of(a).is_none(), "A untouched (server)");
    let after = server.stats_of(player_id).unwrap();
    assert_eq!(
        after.edits_applied,
        before.edits_applied + 1,
        "one hit landed"
    );
    assert_eq!(after.edits_rejected, before.edits_rejected, "none refused");
    let g = rig.game.as_ref().unwrap();
    assert!(g.damage_at(a).is_none(), "A untouched (client)");
    assert_eq!(
        g.prediction.pending(),
        0,
        "nothing left waiting on the server"
    );
}

/// A swing whose contact finds only air (the aim moved off the stone clicked) is a miss: nothing
/// sent, no mark, no hit sound (and so no wear).
// face-a-only: the sandbox's floor and aim; the server's side is both faces' (dwell-sim damage).
#[test]
fn a_pickaxe_swing_aimed_at_air_by_its_contact_is_a_miss() {
    let mut rig = predicting_rig(2);
    let (a, b, player_id) = two_stones_and_a_pickaxe(&mut rig);
    assert_eq!(target_at(&mut rig, 0.0, -35.0), a);
    hits_heard(&mut rig);
    let before = rig.core.server().stats_of(player_id).unwrap();
    let click = rig.now;
    assert!(rig.game.as_mut().unwrap().edit(EditAction::Break, click));
    rig.input.look.pitch = 30.0; // the sky
    let mut heard = 0;
    while rig.now < click + pickaxe_contact_ms() + 1000.0 {
        rig.frame();
        heard += hits_heard(&mut rig);
    }
    assert!(
        rig.game.as_ref().unwrap().debug_state().target.is_none(),
        "aimed at air"
    );
    assert_eq!(heard, 0, "no hit sound");
    let server = rig.core.server();
    assert!(
        server.damage_of(a).is_none() && server.damage_of(b).is_none(),
        "no mark"
    );
    let after = server.stats_of(player_id).unwrap();
    assert_eq!(
        (after.edits_applied, after.edits_rejected),
        (before.edits_applied, before.edits_rejected),
        "nothing sent"
    );
}

/// Break held with the pickaxe (`repeatOnHold`): each swing strikes what is aimed at its own
/// contact (WORK_QUEUE item 17: "each swing hitting what is targeted then"). Each swing starts at one
/// stone and is turned to the other before its contact: the first lands on B, the second on A.
// face-a-only: the sandbox's floor and aim; the server's side is both faces' (dwell-sim damage).
#[test]
fn a_held_pickaxe_swing_strikes_what_is_aimed_at_each_contact() {
    let mut rig = predicting_rig(2);
    let (a, b, _) = two_stones_and_a_pickaxe(&mut rig);
    let dwell_client::held_use::Repeat::Every(repeat_s) =
        dwell_client::held_use::Repeat::of_break(Some("dwell:stone_pickaxe"))
    else {
        panic!("the pickaxe repeats on hold")
    };
    let contact = pickaxe_contact_ms();
    assert!(
        repeat_s * 1000.0 > contact + 50.0,
        "a gap between contact and the next swing"
    );
    assert_eq!(target_at(&mut rig, 0.0, -35.0), a);
    let click = rig.now;
    let holder = dwell_game::build_drag::Holder::Mouse;
    assert!(
        rig.game
            .as_mut()
            .unwrap()
            .edit_held(EditAction::Break, holder, click)
    );
    // The first swing, started at A, turned to B.
    rig.input.look.yaw = B_YAW;
    assert!(rig.run_until(60, |r| r.core.server().damage_of(b).is_some()
        || r.core.server().damage_of(a).is_some()));
    assert!(
        rig.core.server().damage_of(a).is_none(),
        "the first swing lands on B, not A where it started"
    );
    // The second starts at B (`repeatS` after the first) and is turned back to A before its contact.
    while rig.now < click + repeat_s * 1000.0 + 1000.0 / 30.0 {
        rig.frame();
    }
    assert!(
        rig.now < click + repeat_s * 1000.0 + contact - 50.0,
        "before the contact"
    );
    rig.input.look.yaw = 0.0;
    assert!(rig.run_until(60, |r| r.core.server().damage_of(a).is_some()));
    rig.game.as_mut().unwrap().release_use(holder);
    for _ in 0..60 {
        rig.frame();
    }
    let server = rig.core.server();
    assert_eq!(
        server.damage_of(b).map(|d| d.hits),
        Some(1),
        "B by the first swing"
    );
    assert_eq!(
        server.damage_of(a).map(|d| d.hits),
        Some(1),
        "A by the second"
    );
}

/// Break held with an empty hand (owner, 2026-11-13: "You should also be able to hold 'punch' to dig
/// with your hand."), by the controller's trigger: a punch every `breaking.hand.repeatS`, each a hit
/// the server takes (none refused), stopping when let go, and the dirt breaks after its hits by hand.
// face-a-only: the sandbox's floor and aim; the server's rate check is both faces' (dwell-sim damage).
#[test]
fn a_held_punch_digs_dirt_at_the_hands_rate_and_stops_when_let_go() {
    let mut rig = predicting_rig(2);
    let SessionState::Joined { player_id, .. } = rig.session.state().clone() else {
        panic!("joined")
    };
    // A dirt block on the floor (creative's palette), then adventure: no instant breaks.
    let dirt = dwell_blocks::require("dwell:dirt");
    let palette = dwell_game::block_interaction::palette();
    let pick = palette
        .iter()
        .position(|s| s.item == "dwell:dirt")
        .expect("dirt on the palette");
    rig.game.as_mut().unwrap().interaction.select(pick as i64);
    let floor = target_at(&mut rig, 0.0, -35.0);
    let a = [floor[0], floor[1] + 1, floor[2]];
    assert!(rig.game.as_mut().unwrap().edit(EditAction::Place, rig.now));
    assert!(rig.run_until(120, |r| r.game.as_ref().unwrap().prediction.pending() == 0));
    assert_eq!(server_voxel(&mut rig, a), dirt);
    assert!(rig.core.server().set_mode("adventure"));
    rig.run_until(60, |_| false);
    let empty = rig
        .core
        .server()
        .inventory_of(player_id)
        .unwrap()
        .slots
        .iter()
        .take(dwell_protocol::inventory::HOTBAR_SLOTS)
        .position(|s| s.is_none())
        .expect("an empty hotbar slot");
    rig.game.as_mut().unwrap().select_hotbar(empty as i64);
    rig.run_until(30, |_| false);
    assert_eq!(target_at(&mut rig, 0.0, -35.0), a);
    let g = rig.game.as_ref().unwrap();
    assert!(g.use_repeats(), "an empty hand repeats on hold");
    let need = dwell_protocol::damage::hits_to_break("dwell:dirt", None).unwrap();
    assert!(need > 4);
    let r_ms = dwell_protocol::chopping::HAND_REPEAT_S.unwrap() * 1000.0;
    let hits = |rig: &mut Rig| {
        rig.core
            .server()
            .damage_of(a)
            .map_or(0, |d| u32::from(d.hits))
    };
    let before = rig.core.server().stats_of(player_id).unwrap();
    let pad = dwell_game::build_drag::Holder::Gamepad;
    // Held for 3 punches' worth: the press and a punch every `repeatS`.
    let press = rig.now;
    assert!(
        rig.game
            .as_mut()
            .unwrap()
            .edit_held(EditAction::Break, pad, press)
    );
    let hold_ms = 3.0 * r_ms + 100.0;
    while rig.now < press + hold_ms {
        rig.frame();
    }
    rig.game.as_mut().unwrap().release_use(pad);
    for _ in 0..90 {
        rig.frame();
    }
    let n = hits(&mut rig);
    assert_eq!(
        n, 4,
        "the press and three repeats in {hold_ms} ms at {r_ms} ms"
    );
    // Let go: no more.
    for _ in 0..60 {
        rig.frame();
    }
    assert_eq!(hits(&mut rig), n, "none after the release");
    // Held again until it breaks: the rest of its hits by hand, at the same rate.
    let press = rig.now;
    assert!(
        rig.game
            .as_mut()
            .unwrap()
            .edit_held(EditAction::Break, pad, press)
    );
    assert!(rig.run_until(
        60 * 10,
        |r| r.core.server().world().get_voxel(a[0], a[1], a[2]) != dirt
    ));
    let took = rig.now - press;
    rig.game.as_mut().unwrap().release_use(pad);
    let rest = f64::from(need - n - 1);
    assert!(
        took >= rest * r_ms && took <= rest * (r_ms + 1000.0 / 60.0) + 200.0,
        "{} more punches took {took} ms",
        need - n
    );
    let after = rig.core.server().stats_of(player_id).unwrap();
    assert_eq!(
        after.edits_rejected, before.edits_rejected,
        "the server took every punch of the hold"
    );
}

/// A local world as the browser's worker opens it at `size` (BIG_MAP.md M2b): the page's maps for
/// that size installed first (`buildWorldMap` with the scale), its jobs in the server-jobs worker.
fn sized_core(
    file: dwell_storage::BytesFile,
    size: dwell_protocol::world_size::WorldSize,
) -> LocalCore {
    LocalCore::create_tuned(
        dwell_world::Face::A,
        false,
        None,
        false,
        false,
        SEED,
        GENERATOR_TERRAIN,
        Some(file),
        Box::new(SeededEntropy(1)),
        dwell_sim::water::host::Placement::Inline,
        true,
        // As `LocalWorld::new` does with the page's `radiusScale`.
        |c| c.world_size = size,
    )
    .unwrap()
}

#[test]
fn a_local_world_created_at_2x_saves_and_reopens_at_2x_and_its_client_follows_the_size() {
    // both-faces: the maps of both faces are built and installed, the size is the world's.
    use dwell_protocol::world_size::WorldSize;
    let two = WorldSize::named("2x").unwrap();
    // The page's maps for a 2× world (`worldMapKeys` and `buildWorldMap` with the scale): keyed
    // apart from the seed's 1× maps, and built over the 2× disc.
    let keys = dwell_client::local::world_map_keys(SEED, "", two);
    let plain = dwell_client::local::world_map_keys(SEED, "", WorldSize::DEFAULT);
    assert_eq!(plain.len(), 2);
    for (k, p) in keys.iter().zip(&plain) {
        assert_eq!(*k, format!("{p}-x2"), "a 2× world's map keys are its own");
    }
    for f in [dwell_world::Face::A, dwell_world::Face::B] {
        let seed = dwell_worldgen::bifacial::face_seed(SEED, f);
        let bytes = dwell_worldgen::config::worldgen_map_bytes_sized(seed, "", two).unwrap();
        assert_eq!(dwell_worldgen::worldmap::install(&bytes), Ok(seed));
        assert!(dwell_worldgen::worldmap::cache::is_ready_sized(seed, two));
    }

    // Created at 2×: its settings say so, and the file keeps it.
    let file = dwell_storage::BytesFile::default();
    let mut core = sized_core(file.clone(), two);
    assert!(core.persisted);
    assert_eq!(core.world_size(), two);
    core.advance(1.0 / 60.0);
    assert!(core.save());
    drop(core);
    // Reopened as the page opens a saved world, whatever size it passes: the file's own.
    let mut reopened = sized_core(
        dwell_storage::BytesFile::new(file.bytes()),
        WorldSize::DEFAULT,
    );
    assert_eq!(reopened.world_size(), two, "a saved 2× world reopens at 2×");
    assert_eq!(
        reopened.server().bounds().radius(),
        2 * i64::from(dwell_world::WORLD_RADIUS)
    );

    // Its client learns the size at the join (`WORLD_SIZE`) and follows it: the bounds, the
    // streamer's volume, the LOD's rim and the big map's levels.
    let mut rig = Rig::with_core(GENERATOR_TERRAIN, SEED, reopened);
    assert!(
        rig.run_until(60 * 30, |r| r.game.is_some()),
        "the client joins the 2× world"
    );
    assert_eq!(rig.session.world_size(), two);
    let g = rig.game.as_mut().unwrap();
    assert_eq!(g.world_bounds(), dwell_world::WorldBounds::of(two));
    // What bounds an edit (BUGS.md #109): the Place and region pre-checks and the prediction's
    // world, the world's size's, not the 1× build's.
    assert_eq!(g.interaction.bounds(), dwell_world::WorldBounds::of(two));
    assert_eq!(g.region.bounds, dwell_world::WorldBounds::of(two));
    assert_eq!(g.world().bounds, dwell_world::WorldBounds::of(two));
    assert_eq!(
        g.big_map_levels(),
        dwell_game::minimap::MAP.big_map_levels_sized(two)
    );
    assert_ne!(
        g.big_map_levels(),
        dwell_game::minimap::MAP.big_map_levels.to_vec(),
        "a 2× world's big map has its own coarse levels"
    );
    assert!(dwell_client::map_reveal::is_level_sized(
        g.big_map_levels()[2].0 as i32,
        &dwell_game::minimap::MAP,
        two
    ));
    assert!(
        rig.run_until(60 * 30, |r| r
            .game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)),
        "the terrain round the spawn arrives in a 2× world"
    );
}

#[test]
fn a_world_saved_without_a_size_opens_at_1x_and_its_client_too() {
    // face-a-only: the size is the world's, not a face's; the default world's maps are 1×'s.
    use dwell_protocol::world_size::WorldSize;
    install_maps_as_the_worker_does(SEED);
    let file = dwell_storage::BytesFile::default();
    let mut core = sized_core(file.clone(), WorldSize::DEFAULT);
    core.advance(1.0 / 60.0);
    assert!(core.save());
    drop(core);
    let reopened = sized_core(
        dwell_storage::BytesFile::new(file.bytes()),
        WorldSize::DEFAULT,
    );
    assert_eq!(reopened.world_size(), WorldSize::DEFAULT);
    let mut rig = Rig::with_core(GENERATOR_TERRAIN, SEED, reopened);
    assert!(rig.run_until(60 * 30, |r| r.game.is_some()));
    assert_eq!(rig.session.world_size(), WorldSize::DEFAULT);
    let g = rig.game.as_ref().unwrap();
    assert_eq!(g.world_bounds(), dwell_world::WorldBounds::DEFAULT);
    assert_eq!(
        g.big_map_levels(),
        dwell_game::minimap::MAP.big_map_levels.to_vec()
    );
}

/// A single dev teleport far off (5 km, where nothing is loaded; BUGS.md #83): the player is moved
/// there and stays, the client follows and its terrain arrives. Regression: it did not move.
// face-a-only: the generated world's face A spawn; the move is face-local (local.rs).
#[test]
fn a_single_teleport_five_km_off_moves_the_player_and_its_terrain_arrives() {
    let mut rig = Rig::new();
    assert!(rig.run_until(60 * 30, |r| {
        r.game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)
    }));
    let feet = rig.game.as_ref().unwrap().debug_state().feet;
    let to = [feet[0] + 5000.0, feet[1] + 60.0, feet[2] + 300.0];
    assert!(rig.core.teleport_local_player(to));
    let pid = rig.game.as_ref().unwrap().debug_state().player_id;
    let arrived = rig.run_until(60 * 40, |r| {
        let g = r.game.as_ref().unwrap().debug_state();
        (g.feet[0] - to[0]).hypot(g.feet[2] - to[2]) < 2.0 && g.terrain_ready
    });
    let g = rig.game.as_ref().unwrap().debug_state();
    let server = rig.core.server().feet_of(pid);
    assert!(
        arrived,
        "moved to {to:?}: the client at {:?} (terrain ready {}), the server at {server:?}",
        g.feet, g.terrain_ready
    );
}

/// The dev link `?at=` at a far spot (5 km off, BUGS.md #83), in the air and underground: the
/// player starts there and the game's terrain arrives (the loading ends). Regression: it hung.
// face-a-only: a face A point; `?at=`'s spawn is face-local (local.rs).
#[test]
fn a_spawn_link_five_km_off_loads() {
    for y in [60.0, 0.5, -0.0] {
        let at = [5000.5, y + 20.0, 300.5];
        let core = LocalCore::create_spawning(
            dwell_world::Face::A,
            false,
            Some(at),
            false,
            false,
            SEED,
            GENERATOR_TERRAIN,
            None::<NoStorage>,
            Box::new(SeededEntropy(1)),
            dwell_sim::water::host::Placement::Inline,
            false,
        )
        .unwrap();
        let mut rig = Rig::with_core(GENERATOR_TERRAIN, SEED, core);
        let ready = rig.run_until(60 * 40, |r| {
            r.game
                .as_ref()
                .is_some_and(|g| g.debug_state().terrain_ready)
        });
        let feet = rig.game.as_ref().map(|g| g.debug_state().feet);
        assert!(
            ready,
            "?at={at:?}: the terrain never arrived (feet {feet:?})"
        );
        let feet = feet.unwrap();
        assert!(
            (feet[0] - at[0]).hypot(feet[2] - at[2]) < 2.0,
            "{feet:?} at {at:?}"
        );
    }
}

/// The dev tools' `spawnHerd` (DEV_TOOLS.md): a deer herd and a wolf pack put by the local player
/// come in as creatures near it, by the near ecology (WORK_QUEUE item 39's poses, filmed by it).
// face-a-only: the generated world's face A spawn; the record is placed face-locally.
#[test]
fn spawn_herd_puts_a_herd_and_a_pack_by_the_player() {
    let mut rig = Rig::new();
    assert!(rig.run_until(60 * 30, |r| {
        r.game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)
    }));
    let feet = rig.game.as_ref().unwrap().debug_state().feet;
    let deer = rig.core.lava_dev("spawnHerd", &[0.0, 5.0, 10.0]);
    assert!(deer.starts_with("{\"record\":"), "{deer}");
    let wolves = rig.core.lava_dev("spawnHerd", &[1.0, 3.0, -10.0]);
    assert!(wolves.starts_with("{\"record\":"), "{wolves}");
    let near = |rig: &mut Rig, name: &str| {
        let server = rig.core.server();
        let table = server.creature_table();
        server
            .creatures()
            .iter()
            .filter(|c| table.creatures[c.kind].name == name)
            .filter(|c| (c.position[0] - feet[0]).hypot(c.position[2] - feet[2]) < 25.0)
            .count()
    };
    assert!(
        rig.run_until(60 * 10, |r| near(r, "dwell:wolf") >= 3
            && near(r, "dwell:deer") + near(r, "dwell:doe") >= 5),
        "deer {} does {} wolves {}",
        near(&mut rig, "dwell:deer"),
        near(&mut rig, "dwell:doe"),
        near(&mut rig, "dwell:wolf")
    );
}

/// The finder's `goToHerd` (WORK_QUEUE item 46; DEV_TOOLS.md): the local player is stood 40 m short
/// of the nearest far herd on its face, on the ground, and the herd is the finder's nearest.
// face-a-only: the generated world's face A spawn; the finder is face-local (far_herds tests run both).
#[test]
fn go_to_herd_stands_the_player_by_the_nearest_herd() {
    let mut rig = Rig::new();
    assert!(rig.run_until(60 * 30, |r| {
        r.game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)
            && r.core.server().ecology_message().is_some()
    }));
    let feet = rig.game.as_ref().unwrap().debug_state().feet;
    let snapshot = rig.core.server().ecology_message().unwrap();
    let nearest = dwell_client::far_herds::find(&snapshot, feet, Some("dwell:deer"))
        .into_iter()
        .next()
        .expect("a deer herd on face A at the data's densities");
    let r = rig.core.lava_dev("goToHerd", &[0.0, 40.0]);
    assert!(
        r.starts_with(&format!("{{\"id\":{},", nearest.id)),
        "the nearest herd: {r} vs {nearest:?}"
    );
    rig.run_until(30, |_| false);
    let server = rig.core.server();
    let p = server.player_of_session(LOCAL_SESSION).unwrap();
    let now = server.feet_of(p).unwrap();
    assert_eq!(
        dwell_world::FaceFrame::at_height(now[1]),
        dwell_world::FaceFrame::A,
        "on its face"
    );
    let d = (now[0] - nearest.x).hypot(now[2] - nearest.z);
    assert!((d - 40.0).abs() < 3.0, "40 m short of the herd: {d}");
}

/// The duel through the game (WORK_QUEUE item 35): a player holding the sword swings at a villager
/// that fights back. Its block rises in the player's wind-up, the player's level slash across its
/// upright blade is blocked (a clash), and in the player's recovery it strikes back.
// face-a-only: the sandbox's face A spawn; the fight is face-local (sim weapons tests run both).
#[test]
fn a_sword_swing_through_the_game_meets_a_villagers_block_and_it_strikes_back() {
    let mut rig = Rig::with_generator(GENERATOR_SANDBOX);
    assert!(rig.run_until(60 * 30, |r| {
        r.game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)
    }));
    rig.run_until(60, |_| false);
    let ids = rig.core.lava_dev("spawnNpcs", &[1.0, 1.8, 0.0]);
    assert!(ids.contains("[1]") || ids.contains("\"ids\":["), "{ids}");
    rig.run_until(30, |_| false);
    assert!(
        rig.core
            .lava_dev("npcsFight", &[])
            .contains("\"fighting\":1")
    );
    {
        let g = rig.game.as_mut().unwrap();
        g.set_swordplay(true);
        let i = g
            .interaction
            .palette()
            .iter()
            .position(|s| s.tool == Some("dwell:sword"))
            .expect("the sword in the palette");
        let slot = g.creative.slot();
        assert!(g.set_creative_slot(slot, i));
        assert!(g.held_tool().is_some(), "the sword is held");
    }
    // The hold reaches the server (and its answer turns sweeping on).
    rig.run_until(60, |_| false);
    let raised = |rig: &mut Rig| {
        rig.core
            .server()
            .creatures()
            .iter()
            .find_map(|c| c.guard)
            .is_some_and(|g| g.raised)
    };
    assert!(!raised(&mut rig), "lowered before a blade comes");
    let now = rig.now;
    rig.game.as_mut().unwrap().edit(EditAction::Break, now);
    let rose = rig.run_until(20, raised);
    let strikes = rig.core.server().strikes().to_vec();
    assert!(
        rose,
        "the villager raises its block in the wind-up; strikes {strikes:?}"
    );
    rig.run_until(90, |_| false);
    let strikes = rig.core.server().strikes().to_vec();
    assert_eq!(
        strikes.last().and_then(|s| s.clash),
        Some(dwell_protocol::chopping::Clash::Blocked),
        "{strikes:?}"
    );
    let n = rig.core.server().npc_strikes().to_vec();
    assert!(!n.is_empty(), "it struck back in the recovery");
}

/// BUGS.md #107: a player who walks into a portal and drops comes out of the far face's opening
/// and lands beside it, through the browser's path: the local world's server core (`LocalCore`,
/// as the worker opens it), the client session and the game's prediction, the look continued
/// across the midplane by the game itself, as the page does. As the showcase take walked it
/// (`portaltake.js`): from 9 m out on the diagonal (yaw 45), W held until 1.2 m from the axis,
/// looking down, so it falls with the walk's momentum and slides into the shaft's wall.
/// Returns the face it ended on, its distance from the axis (m), the crossings and a log.
fn portal_walk_in(face: dwell_world::Face) -> (dwell_world::FaceFrame, f64, u32, Vec<String>) {
    use dwell_world::FaceFrame;
    const SEED: u64 = 7;
    let shafts = dwell_worldgen::portal_shafts_for_sized(
        GENERATOR_TERRAIN,
        SEED,
        dwell_protocol::world_size::WorldSize::DEFAULT,
    );
    // The portal `?spawn=portal` goes to: nearest the disc's centre (1779.5, 20690.5).
    let shaft = *shafts
        .shafts
        .iter()
        .min_by(|a, b| {
            a.axis[0]
                .hypot(a.axis[1])
                .total_cmp(&b.axis[0].hypot(b.axis[1]))
        })
        .unwrap();
    let f = FaceFrame::of(face);
    let (cx, cz) = (shaft.axis[0], shaft.axis[1]);
    // On the ground 9 m out on the diagonal (the take's teleport), dropped from 2 m over it.
    let at = [cx - 9.0, f.y(shaft.top(face) + 2.0), cz - 9.0];
    let core = LocalCore::create_tuned(
        face,
        false,
        Some(at),
        false,
        false,
        SEED,
        GENERATOR_TERRAIN,
        None::<NoStorage>,
        Box::new(SeededEntropy(1)),
        dwell_sim::water::host::Placement::Inline,
        false,
        |_| {},
    )
    .unwrap();
    let mut rig = Rig::with_core(GENERATOR_TERRAIN, SEED, core);
    rig.input.look.yaw = 45.0;
    rig.input.look.pitch = -4.0;
    assert!(
        rig.run_until(60 * 60, |r| r
            .game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)),
        "the terrain round the portal arrives"
    );
    rig.run_until(120, |_| false);
    rig.input.set_key("KeyW", true);
    let (mut crossings, mut on, mut log) = (0, f, Vec::new());
    let (mut walking, mut still, mut last) = (true, 0, [0.0; 3]);
    for t in 0..60 * 25 {
        rig.frame();
        let g = rig.game.as_ref().unwrap();
        let feet = g.debug_state().feet;
        let from_axis = (feet[0] - cx).hypot(feet[2] - cz);
        if walking {
            let p = if from_axis > 7.0 {
                -4.0
            } else {
                -4.0 - (7.0 - from_axis) / 6.0 * 76.0
            };
            rig.input.look.pitch = p.max(-80.0);
            if from_axis < 1.2 {
                rig.input.set_key("KeyW", false);
                walking = false;
            }
        }
        let now_on = FaceFrame::at_height(feet[1]);
        if now_on != on {
            crossings += 1;
            on = now_on;
        }
        if t % 10 == 0 {
            log.push(format!(
                "{t}: feet {:.2},{:.2},{:.2} look {:.1},{:.1}",
                feet[0] - cx,
                feet[1],
                feet[2] - cz,
                rig.input.look.yaw,
                rig.input.look.pitch
            ));
        }
        let moved = (0..3)
            .map(|i| (feet[i] - last[i]).abs())
            .fold(0.0, f64::max);
        still = if moved < 1e-3 { still + 1 } else { 0 };
        last = feet;
        if !walking && crossings > 0 && still > 30 {
            break;
        }
    }
    let from_axis = (last[0] - cx).hypot(last[2] - cz);
    (on, from_axis, crossings, log)
}

#[test]
fn bug_107_a_player_who_walks_into_a_portal_lands_beside_the_far_hole() {
    // face-a-only: seed 7's portal as the take dropped in, from face A (face B's opening is lower:
    // a drop from there comes out on a later swing, `portal_exit.rs` covers both faces).
    let (on, from_axis, crossings, log) = portal_walk_in(dwell_world::Face::A);
    let log = log.join("\n");
    assert_eq!(on, dwell_world::FaceFrame::B, "{log}");
    assert_eq!(crossings, 1, "{log}");
    assert!(from_axis > 4.0, "{from_axis:.2} m from the axis:\n{log}");
}

/// The player's id on the local core (the game's).
fn my_id(rig: &Rig) -> u16 {
    rig.game.as_ref().unwrap().debug_state().player_id
}

fn moon_ground(seed: u64, f: dwell_world::FaceFrame, up: f64) -> [f64; 3] {
    let moon = dwell_worldgen::grids::CrateredTerrain::for_grid("dwell:moon", seed).unwrap();
    let s = moon.spawn_point(f.face);
    f.point([s[0], s[1] + up, s[2]])
}

dwell_world::both_faces!(
    /// MOON_WORLD.md §7.3 (M-7c): the game client's scripted flight home → moon → home. Flying over
    /// the moon's face `f` the predictor and the server capture the player on the same input; the
    /// game drops home's chunks, asks for and meshes the moon's in the moon's own coordinates (its
    /// dust where the server's moon has it), draws the view on the moon's face (its up) with the sun
    /// turned into the moon's frame and no far moon disc, and falls at ⅙ g; flying out past the
    /// release sphere it comes home on the same input, with no snap or grid miss at either
    /// crossing, and home's chunks stream again.
    fn a_scripted_flight_to_the_moon_and_back_follows_its_grid(f: FaceFrame) {
        use dwell_sim::travel::GRID_CENTRE;
        let mut rig = Rig::with_generator(GENERATOR_SANDBOX);
        assert!(rig.run_until(60 * 20, |r| {
            r.game
                .as_ref()
                .is_some_and(|g| g.debug_state().terrain_ready)
        }));
        let home_feet = rig.game.as_ref().unwrap().debug_state().feet;
        let home_chunk = dwell_world::chunk_of(
            home_feet[0].floor() as i32,
            dwell_world::row_of_m(home_feet[1]) - 1,
            home_feet[2].floor() as i32,
        );
        let id = my_id(&rig);
        rig.input.flight.set_allowed(true);
        rig.input.flight.set(true);
        rig.run_until(30, |_| false);

        // At home, 200 m over the moon's ground, moving with it: a dev move (one snap).
        let t = f64::from(rig.core.server().tick()) / 60.0;
        let fr = rig.core.server().grids().frame(0, t);
        let (wp, wv) = fr.release(moon_ground(rig.seed, f, 200.0), [0.0; 3]);
        assert!(rig.core.server().place_on_grid(id, wp, wv));
        let captured = rig.run_until(120, |r| {
            r.game.as_ref().unwrap().grid() == 1
                && r.core.server().grid_state_of(id).is_some_and(|g| g.0 == 1)
        });
        assert!(captured, "captured by the client and the server");
        let stats = rig.game.as_ref().unwrap().debug_state().stats;
        rig.run_until(30, |_| false);

        // The moon's chunks, in the moon's coordinates: the dust under its spawn, meshed.
        let ground = moon_ground(rig.seed, f, 0.0);
        let cell = dwell_world::GridFrame::HOME.cell_of(moon_ground(
            rig.seed,
            f,
            -0.5 * dwell_world::CELL_H,
        ));
        let chunk = dwell_world::chunk_of(cell[0], cell[1], cell[2]);
        // Down onto it, slowly: flight on, a few m over it.
        assert!(
            rig.core
                .server()
                .place_on_grid(id, moon_ground(rig.seed, f, 6.0), [0.0; 3])
        );
        let meshed = rig.run_until(60 * 30, |r| {
            let feet = r.game.as_ref().unwrap().debug_state().feet;
            let over = f.vector([0, 1, 2].map(|k| feet[k] - ground[k]))[1];
            r.scene.borrow().chunks.contains(&chunk) && over.abs() < 10.0
        });
        assert!(
            meshed,
            "the moon's chunk under its spawn meshed, the player over it"
        );
        // The eye eases down after the dev move's 200 m snap.
        let settled = rig.run_until(60 * 10, |r| {
            let eye = r.scene.borrow().camera.unwrap();
            let feet = r.game.as_ref().unwrap().debug_state().feet;
            (f.vector([0, 1, 2].map(|k| eye[k] - feet[k]))[1] - 1.62).abs() < 0.5
        });
        assert!(settled, "the eye over the feet on the moon's face");
        let server = rig.core.server().grid_voxel(1, cell).unwrap();
        let client = rig.game.as_mut().unwrap().voxel(cell[0], cell[1], cell[2]);
        assert_eq!(
            client, server,
            "the moon's cell as the server's moon has it"
        );
        let name = dwell_blocks::block_of(client).name;
        assert!(name.contains("moon"), "moon dust under the spawn: {name}");

        // The view on the moon's face, in its frame: the sun turned into it, no far moon disc.
        let face = rig.scene.borrow().camera_face;
        assert_eq!(face, Some(f), "the view's up is the moon's face's");
        let day = rig.scene.borrow().day.unwrap();
        assert!(!day.moon_shown, "no far moon disc on the moon");
        let eye = rig.scene.borrow().camera.unwrap();
        let d = [0, 1, 2].map(|k| eye[k] - ground[k]);
        assert!(
            d.iter().map(|x| x * x).sum::<f64>().sqrt() < 30.0,
            "eye by the moon's ground (grid-local): {eye:?}"
        );

        // ⅙ g: flight off, 40 m over the ground at rest, falling toward the moon's midplane.
        rig.input.flight.set(false);
        assert!(
            rig.core
                .server()
                .place_on_grid(id, moon_ground(rig.seed, f, 40.0), [0.0; 3])
        );
        rig.run_until(20, |_| false);
        let up = |r: &Rig| f.vector(r.game.as_ref().unwrap().state().velocity)[1];
        let v0 = up(&rig);
        rig.run_until(30, |_| false);
        let v1 = up(&rig);
        let g = (v0 - v1) / 0.5;
        let home_g = f64::from(PlayerControllerConfig::default().body.gravity);
        assert!(
            (g / home_g - 0.1667).abs() < 0.03,
            "falls at {g} m/s² on the moon ({home_g} at home)"
        );
        let mid = rig.game.as_ref().unwrap().debug_state().stats;
        assert_eq!(
            mid.grid_misses, stats.grid_misses,
            "the capture as the server made it"
        );

        // Home: hovering inside the release sphere, then flying up and out (far flight).
        let moon = dwell_worldgen::grids::grid("dwell:moon").unwrap();
        let r = moon.capture_m.unwrap() * dwell_worldgen::grids::travel().release_factor - 200.0;
        let upv = f.vector([0.0, 1.0, 0.0]);
        rig.input.flight.set(true);
        rig.run_until(5, |_| false);
        let at = [0, 1, 2].map(|k| GRID_CENTRE[k] + upv[k] * r);
        assert!(rig.core.server().place_on_grid(id, at, [0.0; 3]));
        rig.run_until(30, |_| false);
        assert_eq!(rig.game.as_ref().unwrap().grid(), 1, "still on the moon");
        let before = rig.game.as_ref().unwrap().debug_state().stats;
        rig.input.set_key("Space", true);
        let released = rig.run_until(120, |r| {
            r.game.as_ref().unwrap().grid() == 0
                && r.core.server().grid_state_of(id).is_some_and(|g| g.0 == 0)
        });
        rig.input.set_key("Space", false);
        assert!(released, "released by the client and the server");
        rig.run_until(10, |_| false);
        let after = rig.game.as_ref().unwrap().debug_state().stats;
        assert_eq!(
            after.grid_misses, before.grid_misses,
            "the release as the server made it"
        );
        assert_eq!(after.snaps, before.snaps, "no snap through the release");
        assert!(
            rig.scene.borrow().day.unwrap().moon_shown,
            "the moon in home's sky again"
        );

        // Home's chunks stream again: back at the spawn (a dev move), its ground arrives.
        let centre = [home_feet[0], home_feet[1] + 1.0, home_feet[2]];
        assert!(rig.core.server().place_on_grid(id, centre, [0.0; 3]));
        let back = rig.run_until(60 * 20, |r| {
            r.scene.borrow().chunks.contains(&home_chunk)
                || r.game
                    .as_ref()
                    .unwrap()
                    .chunk_revision(home_chunk.to_array())
                    .is_some()
        });
        assert!(back, "home's chunks stream again after the release");
    }
);

dwell_world::both_faces!(
    /// BUGS.md #123, #124, #136 (MOON_WORLD.md §7.3): teleported onto the moon (`moon_land`) with the LOD
    /// on, home's section meshes are dropped at the capture (they stayed uploaded while away, the mesh memory growing each trip), and the
    /// moon's chunks under the player are drawn: `Game::chunk_fade` (the renderer's draw decision
    /// for a chunk) is not asked of home's LOD selection, made in home's coordinates, for the
    /// moon's grid-local chunks (the browser drew 16 of 232).
    fn on_the_moon_its_chunks_are_drawn_and_homes_lod_meshes_dropped(f: FaceFrame) {
        let mut rig = Rig::with_generator(GENERATOR_SANDBOX);
        assert!(rig.run_until(60 * 20, |r| {
            r.game
                .as_ref()
                .is_some_and(|g| g.debug_state().terrain_ready)
        }));
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
            rig.run_until(60 * 30, |r| r.scene.borrow().held.len() > 4),
            "home's LOD sections meshed: {:?}",
            rig.game.as_ref().unwrap().debug_state().lod
        );
        let feet = rig.game.as_ref().unwrap().debug_state().feet;
        let home = [feet[0], feet[1] + 1.0, feet[2]];
        let id = my_id(&rig);
        rig.input.flight.set_allowed(true);
        rig.input.flight.set(true);
        rig.run_until(30, |_| false);

        // A dev teleport onto the moon's ground on face `f`, no flight (owner, 2026-11-13: "We
        // should just teleport to the moon to see if ground draws").
        let face = if f == dwell_world::FaceFrame::A {
            1.0
        } else {
            -1.0
        };
        let r = rig.core.moon_land(face);
        assert!(!r.contains("error"), "{r}");
        assert!(
            rig.run_until(120, |r| r.game.as_ref().unwrap().grid() == 1),
            "captured"
        );
        let held_at_capture = rig.scene.borrow().held.len();

        // The moon's chunk under the player meshed and drawn.
        let cell = dwell_world::GridFrame::HOME.cell_of(moon_ground(
            rig.seed,
            f,
            -0.5 * dwell_world::CELL_H,
        ));
        let chunk = dwell_world::chunk_of(cell[0], cell[1], cell[2]);
        assert!(
            rig.run_until(60 * 30, |r| r.scene.borrow().chunks.len() > 8
                && r.scene.borrow().chunks.contains(&chunk)),
            "the moon's chunks meshed"
        );
        rig.run_until(30, |_| false);
        let g = rig.game.as_ref().unwrap();
        assert!(
            g.chunk_fade(chunk).is_some(),
            "the moon's chunk under the player is drawn"
        );
        let scene = rig.scene.borrow();
        let hidden = scene
            .chunks
            .iter()
            .filter(|&&c| g.chunk_fade(c).is_none())
            .count();
        assert_eq!(
            hidden,
            0,
            "every moon chunk meshed is drawn ({} meshed)",
            scene.chunks.len()
        );
        assert_eq!(
            held_at_capture, 0,
            "home's LOD meshes dropped at the capture"
        );
        assert!(
            scene.held.is_empty(),
            "no home section uploaded on the moon"
        );
        assert!(scene.shown.is_empty(), "no home section shown on the moon");
        drop(scene);

        // Home again (out past the release sphere, then a dev move to the spawn): its LOD builds
        // again from nothing.
        let moon = dwell_worldgen::grids::grid("dwell:moon").unwrap();
        let r = moon.capture_m.unwrap() * dwell_worldgen::grids::travel().release_factor - 200.0;
        let upv = f.vector([0.0, 1.0, 0.0]);
        let at = [0, 1, 2].map(|k| dwell_sim::travel::GRID_CENTRE[k] + upv[k] * r);
        assert!(rig.core.server().place_on_grid(id, at, [0.0; 3]));
        rig.run_until(30, |_| false);
        rig.input.set_key("Space", true);
        assert!(
            rig.run_until(120, |r| r.game.as_ref().unwrap().grid() == 0
                && r.core.server().grid_state_of(id).is_some_and(|g| g.0 == 0)),
            "released by the client and the server"
        );
        rig.input.set_key("Space", false);
        assert!(rig.core.server().place_on_grid(id, home, [0.0; 3]));
        assert!(
            rig.run_until(60 * 30, |r| r.scene.borrow().held.len() > 4
                && !r.scene.borrow().shown.is_empty()),
            "home's LOD sections meshed and shown again: {:?}",
            rig.game.as_ref().unwrap().debug_state().lod
        );
    }
);

/// BUGS.md #113: a clean recording can hide the targeted block's outline (`setView({ targetOutline:
/// false })`); the block stays targeted, and the outline comes back when switched on again.
#[test]
fn the_target_outline_can_be_hidden_for_recordings() {
    let mut rig = Rig::new();
    assert!(
        rig.run_until(60 * 30, |r| r
            .game
            .as_ref()
            .is_some_and(|g| g.debug_state().terrain_ready)),
        "the player is placed and the terrain around them arrives"
    );
    rig.input.look.pitch = -60.0;
    assert!(
        rig.run_until(60, |r| r.scene.borrow().outline.is_some()),
        "the ground in front is targeted and outlined"
    );
    rig.game.as_mut().unwrap().show_outline = false;
    rig.run_until(5, |_| false);
    assert!(
        rig.game.as_ref().unwrap().debug_state().target.is_some(),
        "still targeted"
    );
    assert!(rig.scene.borrow().outline.is_none(), "but not outlined");
    rig.game.as_mut().unwrap().show_outline = true;
    assert!(
        rig.run_until(5, |r| r.scene.borrow().outline.is_some()),
        "outlined again"
    );
}

dwell_world::both_faces!(
    /// DEV_TOOLS.md "The moon": the bench's moon trip natively. `__dwell.moon()`'s report
    /// (`Game::moon_report`) from home after the dev move (`LocalCore::moon_approach`) puts the
    /// player just outside the capture sphere over the moon's face `f`: its grid home, the moon's
    /// centre the approach's distance off, home's gravity now and the moon's ⅙; flying along the
    /// report's look (as `flyTo` steers: level by the yaw, crouch or Space for the pitch) is
    /// captured, and then the report says the moon, its gravity, and a height over its ground on
    /// that face.
    fn the_moon_report_steers_a_flight_into_its_capture(f: FaceFrame) {
        let mut rig = Rig::with_generator(GENERATOR_SANDBOX);
        assert!(rig.run_until(60 * 20, |r| {
            r.game
                .as_ref()
                .is_some_and(|g| g.debug_state().terrain_ready)
        }));
        rig.input.flight.set_allowed(true);
        rig.input.flight.set(true);
        rig.run_until(30, |_| false);
        let at_home = rig.game.as_ref().unwrap().moon_report().expect("a moon");
        assert_eq!(at_home.grid, "home");
        assert!(at_home.ground_held, "home's chunk under the spawn held");

        let face = if f == dwell_world::FaceFrame::A {
            1.0
        } else {
            -1.0
        };
        let r = rig.core.moon_approach(2000.0, face);
        let j: serde_json::Value = serde_json::from_str(&r).unwrap();
        assert!(j.get("error").is_none(), "{r}");
        rig.run_until(30, |_| false);
        let m = rig.game.as_ref().unwrap().moon_report().unwrap();
        assert_eq!(m.grid, "home", "outside the capture sphere");
        assert!(
            (m.capture_m..m.capture_m + 4000.0).contains(&m.distance),
            "just outside the capture ({} m): {} m",
            m.capture_m,
            m.distance
        );
        let home_g = f64::from(PlayerControllerConfig::default().body.gravity);
        assert!((m.gravity_now - home_g).abs() < 1e-6);
        assert!((m.moon_gravity / home_g - 0.1667).abs() < 1e-4);
        assert_eq!(m.ground_m, None);

        // Steer by the report, as `flyTo` does (moonTrip.ts `aim`).
        let mut captured = false;
        for _ in 0..60 * 60 {
            let m = rig.game.as_ref().unwrap().moon_report().unwrap();
            if m.grid == "moon" {
                captured = true;
                break;
            }
            let (yaw, pitch) = m.look;
            rig.input.look.yaw = yaw;
            rig.input.look.pitch = pitch;
            rig.input.set_key("KeyW", pitch.abs() < 70.0);
            rig.input.set_key("Space", pitch > 20.0);
            rig.input.set_key("KeyC", pitch < -20.0);
            rig.run_until(6, |_| false);
        }
        for k in ["KeyW", "Space", "KeyC"] {
            rig.input.set_key(k, false);
        }
        assert!(captured, "captured flying along the report's look");
        let m = rig.game.as_ref().unwrap().moon_report().unwrap();
        assert_eq!(rig.game.as_ref().unwrap().grid(), m.index);
        assert!(
            (m.gravity_now - m.moon_gravity).abs() < 1e-9,
            "the moon's gravity now"
        );
        assert!(m.distance <= m.capture_m + 50.0, "{} m", m.distance);
        let g = m.ground_m.expect("a height over the moon's ground");
        assert!(
            (0.0..m.capture_m).contains(&g),
            "over the moon's ground on this face: {g} m"
        );
        assert_eq!(
            rig.scene.borrow().camera_face,
            Some(f),
            "on the moon's face {f:?}"
        );

        // Down as `flyTo` goes (crouch held, slower under 600 m) to 40 m, then the fall at ⅙ g.
        rig.input.look.pitch = 0.0;
        rig.input.set_key("KeyC", true);
        let mut down = false;
        for _ in 0..60 * 60 {
            let g = rig.game.as_ref().unwrap().moon_report().unwrap().ground_m;
            let g = g.unwrap();
            if g < 40.0 {
                down = true;
                break;
            }
            rig.input
                .flight
                .set_speed_level(if g < 600.0 { 6.0 } else { 11.0 });
            rig.run_until(6, |_| false);
        }
        rig.input.set_key("KeyC", false);
        assert!(down, "down to 40 m over the moon's ground");
        rig.run_until(60, |_| false);
        rig.input.flight.set(false);
        rig.run_until(15, |_| false);
        let v0 = rig.game.as_ref().unwrap().moon_report().unwrap().up_mps;
        rig.run_until(60, |_| false);
        let m = rig.game.as_ref().unwrap().moon_report().unwrap();
        let fall = v0 - m.up_mps;
        assert!(
            (fall / m.moon_gravity - 1.0).abs() < 0.1,
            "fell {fall} m/s in 1 s (the moon's {} m/s²)",
            m.moon_gravity
        );
        let landed = rig.run_until(60 * 30, |r| {
            let m = r.game.as_ref().unwrap().moon_report().unwrap();
            m.ground_m.unwrap() < 0.3 && m.up_mps.abs() < 0.1
        });
        assert!(landed, "landed on the moon's ground");
    }
);

dwell_world::both_faces!(
    /// SUN_WORLD.md §5's capture speed: the sun goes round its path at 3.7–7.5 km/s (the moon's
    /// ~1 km/s), so the carry's clock slip on a jittery, laggy device turns positions by several
    /// times the moon's distance. A far flier steered at the sun from just outside its capture
    /// sphere (`sunApproach`, moving with it) is captured, flown down and landed on its dry crust
    /// with no snap (#879's carry smoothing).
    fn a_sun_trip_on_a_jittery_device_makes_no_snap(f: FaceFrame) {
        let mut rig = Rig::with_generator(GENERATOR_SANDBOX);
        rig.latency_steps = 6;
        assert!(rig.run_until(60 * 20, |r| {
            r.game
                .as_ref()
                .is_some_and(|g| g.debug_state().terrain_ready)
        }));
        rig.input.flight.set_allowed(true);
        rig.input.flight.set(true);
        rig.run_until(30, |_| false);
        let face = if f == dwell_world::FaceFrame::A {
            1.0
        } else {
            -1.0
        };
        let r = rig.core.grid_approach("dwell:sun", 300.0, face);
        assert!(!r.contains("error"), "{r}");
        rig.run_until(60, |_| false);
        let snaps = |r: &Rig| r.game.as_ref().unwrap().debug_state().stats.snaps;
        let report = |r: &Rig| r.game.as_ref().unwrap().grid_report("dwell:sun").unwrap();
        let before = snaps(&rig);
        let mut n = 0usize;
        let mut step = |r: &mut Rig| {
            n += 1;
            if n.is_multiple_of(7) {
                r.slow_frame(4);
            } else {
                r.frame();
            }
        };
        let mut captured = false;
        for _ in 0..60 * 60 {
            let m = report(&rig);
            if m.grid == "sun" {
                captured = true;
                break;
            }
            rig.input.look.yaw = m.look.0;
            rig.input.look.pitch = m.look.1;
            rig.input.set_key("KeyW", m.look.1.abs() < 70.0);
            rig.input.set_key("Space", m.look.1 > 20.0);
            rig.input.set_key("KeyC", m.look.1 < -20.0);
            step(&mut rig);
        }
        for k in ["KeyW", "Space", "KeyC"] {
            rig.input.set_key(k, false);
        }
        assert!(captured, "captured by the sun");
        // The sun's speed on its path at the capture: several times the moon's.
        let index = report(&rig).index;
        let t = f64::from(rig.core.server().tick()) / 60.0;
        let grids = rig.core.server().grids();
        let slot = grids.slots.iter().position(|s| s.index == index).unwrap();
        let frame = grids.frame(slot, t);
        let v = frame.point_velocity(frame.to_world(dwell_sim::travel::GRID_CENTRE));
        let speed = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        assert!(speed > 3_000.0, "the sun moves {speed} m/s");
        assert_eq!(
            snaps(&rig),
            before,
            "no snap at a capture at {speed:.0} m/s"
        );
        // Down to 40 m, then flight off and the landing.
        rig.input.look.pitch = 0.0;
        rig.input.set_key("KeyC", true);
        for _ in 0..60 * 120 {
            let g = report(&rig).ground_m.unwrap();
            if g < 40.0 {
                break;
            }
            rig.input
                .flight
                .set_speed_level(if g < 600.0 { 6.0 } else { 11.0 });
            step(&mut rig);
        }
        rig.input.set_key("KeyC", false);
        rig.input.flight.set(false);
        let mut landed = false;
        for _ in 0..60 * 30 {
            let m = report(&rig);
            if m.ground_m.unwrap() < 0.3 && m.up_mps.abs() < 0.1 {
                landed = true;
                break;
            }
            step(&mut rig);
        }
        assert!(landed, "landed: {:?}", report(&rig).ground_m);
        assert_eq!(
            snaps(&rig),
            before,
            "no snap through the capture and landing on the sun"
        );
    }
);

dwell_world::both_faces!(
    /// BUGS.md #125: a capture, the descent, the landing and the release on a device with uneven
    /// frames and a laggy link make no snap. The server runs an input a few ticks off the client's
    /// guess (`server_tick − 1 − ack`), so the carry turned positions at another moment of the
    /// moon's ~1 km/s orbit: 45–93 m off at the first snapshot on the moon, snapped (the browser's
    /// 34 m and 18.7 m). The predictor now smooths a correction its carry's clock slip explains.
    fn a_moon_trip_on_a_jittery_device_makes_no_snap(f: FaceFrame) {
        let mut rig = Rig::with_generator(GENERATOR_SANDBOX);
        rig.latency_steps = 6;
        assert!(rig.run_until(60 * 20, |r| {
            r.game
                .as_ref()
                .is_some_and(|g| g.debug_state().terrain_ready)
        }));
        rig.input.flight.set_allowed(true);
        rig.input.flight.set(true);
        rig.run_until(30, |_| false);
        let face = if f == dwell_world::FaceFrame::A {
            1.0
        } else {
            -1.0
        };
        let r = rig.core.moon_approach(300.0, face);
        assert!(!r.contains("error"), "{r}");
        rig.run_until(60, |_| false);
        let snaps = |r: &Rig| r.game.as_ref().unwrap().debug_state().stats.snaps;
        let report = |r: &Rig| r.game.as_ref().unwrap().moon_report().unwrap();
        let before = snaps(&rig);
        // Every 7th frame takes 4 ticks (a hitch), as `flyTo` steers.
        let mut n = 0usize;
        let mut step = |r: &mut Rig| {
            n += 1;
            if n.is_multiple_of(7) {
                r.slow_frame(4);
            } else {
                r.frame();
            }
        };
        let mut captured = false;
        for _ in 0..60 * 60 {
            let m = report(&rig);
            if m.grid == "moon" {
                captured = true;
                break;
            }
            rig.input.look.yaw = m.look.0;
            rig.input.look.pitch = m.look.1;
            rig.input.set_key("KeyW", m.look.1.abs() < 70.0);
            rig.input.set_key("Space", m.look.1 > 20.0);
            rig.input.set_key("KeyC", m.look.1 < -20.0);
            step(&mut rig);
        }
        for k in ["KeyW", "Space", "KeyC"] {
            rig.input.set_key(k, false);
        }
        assert!(captured, "captured");
        // Down to 40 m, then flight off and the landing.
        rig.input.look.pitch = 0.0;
        rig.input.set_key("KeyC", true);
        for _ in 0..60 * 120 {
            let g = report(&rig).ground_m.unwrap();
            if g < 40.0 {
                break;
            }
            rig.input
                .flight
                .set_speed_level(if g < 600.0 { 6.0 } else { 11.0 });
            step(&mut rig);
        }
        rig.input.set_key("KeyC", false);
        rig.input.flight.set(false);
        let mut landed = false;
        for _ in 0..60 * 30 {
            let m = report(&rig);
            if m.ground_m.unwrap() < 0.3 && m.up_mps.abs() < 0.1 {
                landed = true;
                break;
            }
            step(&mut rig);
        }
        assert!(landed, "landed: {:?}", report(&rig).ground_m);
        assert_eq!(
            snaps(&rig),
            before,
            "no snap through the capture and landing"
        );

        // Out: inside the release sphere (a dev move: its own snap), then up past it.
        let id = my_id(&rig);
        let moon = dwell_worldgen::grids::grid("dwell:moon").unwrap();
        let r = moon.capture_m.unwrap() * dwell_worldgen::grids::travel().release_factor - 200.0;
        let upv = f.vector([0.0, 1.0, 0.0]);
        let at = [0, 1, 2].map(|k| dwell_sim::travel::GRID_CENTRE[k] + upv[k] * r);
        rig.input.flight.set(true);
        rig.run_until(5, |_| false);
        assert!(rig.core.server().place_on_grid(id, at, [0.0; 3]));
        rig.run_until(60, |_| false);
        let before = snaps(&rig);
        rig.input.set_key("Space", true);
        let mut released = false;
        for _ in 0..60 * 10 {
            if rig.game.as_ref().unwrap().grid() == 0
                && rig
                    .core
                    .server()
                    .grid_state_of(id)
                    .is_some_and(|g| g.0 == 0)
            {
                released = true;
                break;
            }
            step(&mut rig);
        }
        rig.input.set_key("Space", false);
        assert!(released, "released");
        for _ in 0..120 {
            step(&mut rig);
        }
        assert_eq!(snaps(&rig), before, "no snap through the release");
    }
);
