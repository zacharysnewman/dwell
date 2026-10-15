//! The in-game loop (PLAYER_CONTROLLER.md §8, §9), ported from `client/src/game/game.ts`: fixed 60 Hz
//! ticks of local prediction, input datagrams, snapshot reconciliation, interpolated remote players,
//! the first-person camera, terrain streaming and the whole-world view, and block editing. Platform
//! free: time is passed in, the renderer is a [`Scene`] and the workers a [`Pool`], so it runs
//! natively under test as it does in the browser.

use dwell_world::hash::HashSet;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use dwell_game::block_interaction::{BlockInteraction, BlockTarget};
use dwell_game::chunk_stream::{ChunkMessage, ChunkStreamer, Coord, StreamStats, TerrainView};
use dwell_game::eye::{EyeCamera, EyeState};
use dwell_game::face_flip::FaceFlip;
use dwell_game::gate::prediction_may_run;
use dwell_game::hud_text::{HudTextInput, hud_text};
use dwell_game::input::{EditAction, KeyboardMouseInput, quantize_input};
use dwell_game::remotes::RemotePlayers;
use dwell_game::touch::TouchState;
use dwell_lod::{LodCamera, LodSystem, LodView, StandInBox, format_lod_stats};
use dwell_mesh::{ChunkMeshes, SectionMeshes};
use dwell_physics::config::PlayerControllerConfig;
use dwell_physics::net::to_net;
use dwell_physics::predictor::{Predictor, PredictorStats};
use dwell_physics::{DVec3, Vec3};
use dwell_protocol::{
    GroundKind, InputFrame, MAX_INPUTS_PER_DATAGRAM, Message, PlayerEventKind, PlayerInput,
    PlayerState, SIM_HZ, controller_flags, player_flags, players,
};
use dwell_world::{ChunkCoord, VoxelWorld};

use crate::jobs::Output;
use crate::pool::{ChunkJobs, Done, MeshJobs, PoolRef, Purpose};
use crate::render::display::{RenderStats, format_render_stats};
use crate::render::{Line, PlayerView, StandIn};
use crate::telemetry::{self, TickSample};

const TICK_MS: f64 = 1000.0 / SIM_HZ as f64;
/// Ticks one frame may catch up (250 ms): beyond it time is dropped, so the frame loop can't spiral.
/// It covers a slow device's frame (down to 4 fps): a client that drops time predicts fewer ticks
/// than the server's 60 Hz, the server fills the missing ones by repeating the last input, and the
/// replays drag the player (a snap once past RECONCILE_SNAP_DISTANCE; e2e/play.spec.ts on CI).
const MAX_TICKS_PER_FRAME: f64 = 15.0;
/// Main-thread time per frame for starting mesh jobs (copying chunks' voxels out of the world).
const MESH_BUDGET_MS: f64 = 4.0;
/// Server input buffer outside [LOW, HIGH] nudges the local tick rate by ±RATE_NUDGE.
const BUFFER_LOW: u8 = 1;
const BUFFER_HIGH: u8 = 4;
const RATE_NUDGE: f64 = 0.02;
const PLAYER_COLORS: [u32; 7] = [
    0xe0564a, 0x4a8fe0, 0xe0c24a, 0x5fc26b, 0xb15fe0, 0x4ad0c8, 0xe08a4a,
];

/// What the game draws through (the wgpu renderer, or a recorder in tests).
pub trait Scene {
    fn set_terrain_chunk(&mut self, coord: ChunkCoord, meshes: Option<&ChunkMeshes>);
    fn set_lod_section(
        &mut self,
        id: i64,
        origin: [f64; 3],
        cell_size: f64,
        meshes: Option<&SectionMeshes>,
    );
    fn show_lod_sections(&mut self, visible: &[(i64, u8)]);
    fn show_lod_stand_ins(&mut self, stand_ins: &[StandIn]);
    fn set_player(&mut self, id: u32, view: Option<PlayerView>);
    fn set_camera(&mut self, eye: [f64; 3], yaw_deg: f64, pitch_deg: f64, face: f64, flip: f64);
    /// The fluid whose drawn surface the camera's eye is under (`dwell_world::fluid::eye_fluid`).
    fn set_eye_fluid(&mut self, _eye_fluid: Option<dwell_world::fluid::EyeFluid>) {}
    fn set_block_outline(&mut self, cell: Option<[f64; 3]>, height: f64);
    fn set_debug_lines(&mut self, lines: Option<&[Line]>);
    fn stats(&self) -> RenderStats;
}

pub type SceneRef<S> = Rc<RefCell<S>>;

/// The streamer's and the LOD system's view of the scene.
pub struct View<S>(pub SceneRef<S>);

impl<S: Scene> TerrainView<ChunkMeshes> for View<S> {
    fn set_terrain_chunk(&mut self, coord: Coord, _origin: [i32; 3], meshes: Option<ChunkMeshes>) {
        self.0
            .borrow_mut()
            .set_terrain_chunk(ChunkCoord::from_array(coord), meshes.as_ref());
    }
}

impl<S: Scene> LodView for View<S> {
    fn set_lod_section(
        &mut self,
        id: i64,
        origin: [f64; 3],
        cell_size: f64,
        meshes: Option<SectionMeshes>,
    ) {
        self.0
            .borrow_mut()
            .set_lod_section(id, origin, cell_size, meshes.as_ref());
    }
    fn show_lod_sections(&mut self, visible: &[(i64, u8)]) {
        self.0.borrow_mut().show_lod_sections(visible);
    }
    fn show_lod_stand_ins(&mut self, stand_ins: &[StandInBox]) {
        let boxes: Vec<StandIn> = stand_ins
            .iter()
            .map(|b| StandIn {
                id: b.id,
                lo: b.lo,
                hi: b.hi,
            })
            .collect();
        self.0.borrow_mut().show_lod_stand_ins(&boxes);
    }
}

/// Messages for the session: reliable control messages and gameplay datagrams.
#[derive(Clone, Debug, PartialEq)]
pub enum Send {
    Control(Message),
    Datagram(Message),
}

pub type Outbox = Rc<RefCell<Vec<Send>>>;

type Streamer<S> = ChunkStreamer<VoxelWorld, ChunkJobs, MeshJobs, View<S>>;

/// The client sim's state after the latest tick (`ClientState` in the TS).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClientState {
    pub active: bool,
    /// Predicted capsule centre.
    pub position: [f64; 3],
    pub render_offset: [f64; 3],
    pub velocity: [f64; 3],
    pub half_height: f64,
    pub state: PlayerState,
    pub controller_flags: u8,
    pub face: i32,
    pub platform_yaw_delta: f64,
    pub ground_kind: GroundKind,
    pub ground_gap: f64,
    pub ceiling_blocked: bool,
    pub touching_wall: bool,
    pub submerged: f64,
    pub horizontal_current: [f64; 3],
    pub horizontal_external: [f64; 3],
    pub target_velocity: [f64; 3],
    pub stats: PredictorStats,
    pub next_seq: u32,
    pub eye_height: f64,
    pub crouch_eye_height: f64,
    pub max_step_height: f64,
    pub radius: f64,
    pub standing_height: f64,
}

/// The tick's controller state and input, for the telemetry's trace.
fn record_tick(c: &ClientState, input: &InputFrame) {
    use dwell_protocol::input_buttons;
    let flag = |f: u8| c.controller_flags & f != 0;
    let sample = TickSample {
        seq: input.seq,
        at_ms: telemetry::now_ms(),
        feet: [
            c.position[0],
            c.position[1] - f64::from(c.face) * c.half_height,
            c.position[2],
        ],
        velocity: c.velocity,
        state: player_state_name(c.state),
        grounded: flag(controller_flags::GROUNDED),
        crouched: flag(controller_flags::CROUCHING),
        touching_wall: c.touching_wall,
        ceiling_blocked: c.ceiling_blocked,
        move_input: [
            f64::from(input.move_x) / 127.0,
            f64::from(input.move_y) / 127.0,
        ],
        jump: input.buttons & input_buttons::JUMP != 0,
        crouch: input.buttons & input_buttons::CROUCH != 0,
    };
    telemetry::with(|t| t.tick(sample));
}

fn player_state_name(s: PlayerState) -> &'static str {
    match s {
        PlayerState::Idle => "idle",
        PlayerState::Walking => "walking",
        PlayerState::Running => "running",
        PlayerState::Crouching => "crouching",
        PlayerState::Sliding => "sliding",
        PlayerState::Jumping => "jumping",
        PlayerState::Falling => "falling",
        PlayerState::Climbing => "climbing",
        PlayerState::Swimming => "swimming",
        PlayerState::Flying => "flying",
    }
}

fn v3(v: Vec3) -> [f64; 3] {
    [f64::from(v.x), f64::from(v.y), f64::from(v.z)]
}

impl ClientState {
    fn of(p: &Predictor) -> Self {
        let c = p.controller();
        let pos = p.position();
        let config = p.config();
        let net = to_net(&c, |_| 0);
        Self {
            active: p.active(),
            position: [pos.x, pos.y, pos.z],
            render_offset: v3(p.render_offset()),
            velocity: v3(p.velocity()),
            half_height: f64::from(p.half_height()),
            state: PlayerState::from_u8(c.state as u8).unwrap_or(PlayerState::Idle),
            controller_flags: net.flags,
            face: i32::from(c.face),
            platform_yaw_delta: f64::from(c.platform.yaw_delta),
            ground_kind: net.ground_kind,
            ground_gap: f64::from(c.ground.gap),
            ceiling_blocked: c.ground.ceiling_blocked,
            touching_wall: c.ground.touching_wall,
            submerged: f64::from(c.swim.submerged),
            horizontal_current: v3(c.horizontal.current),
            horizontal_external: v3(c.horizontal.external),
            target_velocity: v3(c.target_velocity),
            stats: *p.stats(),
            next_seq: p.next_seq(),
            eye_height: f64::from(config.body.eye_height),
            crouch_eye_height: f64::from(config.body.crouch_eye_height),
            max_step_height: f64::from(config.movement.max_step_height),
            radius: f64::from(config.body.radius),
            standing_height: f64::from(config.body.standing_height),
        }
    }

    fn eye_state(&self) -> EyeState {
        EyeState {
            position: self.position,
            render_offset: self.render_offset,
            half_height: self.half_height,
            velocity: self.velocity,
            controller_flags: self.controller_flags,
            face: self.face,
            eye_height: self.eye_height,
            crouch_eye_height: self.crouch_eye_height,
            max_step_height: self.max_step_height,
        }
    }
}

/// What automated tests read through `window.__dwell.state()` (GameDebugState in the TS).
#[derive(Clone, Debug, PartialEq)]
pub struct DebugState {
    pub player_id: u16,
    pub active: bool,
    pub feet: [f64; 3],
    pub health: u8,
    pub dead: bool,
    /// (player id, feet, dead).
    pub remotes: Vec<(u16, [f64; 3], bool)>,
    pub stats: PredictorStats,
    pub terrain: StreamStats,
    pub terrain_ready: bool,
    pub target: Option<BlockTarget>,
    pub lod: Option<dwell_lod::LodStats>,
    pub flying: bool,
}

/// The HUD's contents after a frame (the TS shell draws them).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hud {
    pub message: String,
    pub status: String,
    pub health: u8,
    /// The F3 overlay's text, while it is shown.
    pub debug: Option<String>,
    /// Damage taken since the last frame (the HUD flashes).
    pub damaged: bool,
}

/// A camera a tool places (tests, the views runner, ARCHITECTURE.md §6.6): the renderer and the
/// LOD view from it while the player stays where it is. Full-detail chunks still stream around the
/// player only, so away from the player the view is the LOD's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewOverride {
    pub eye: [f64; 3],
    /// Degrees, as the player's look (yaw 0 = +Z; pitch toward the face's up).
    pub yaw_deg: f64,
    pub pitch_deg: f64,
}

impl ViewOverride {
    /// The face the eye is over (1: A above the midplane, −1: B).
    fn face(&self) -> f64 {
        if self.eye[1] >= f64::from(dwell_world::MIDPLANE_Y) {
            1.0
        } else {
            -1.0
        }
    }
}

/// The whole-world view's settings (§6.6).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodSettings {
    pub pixel_error: f64,
    pub cache_bytes: usize,
    pub detail_distance_m: Option<f64>,
    /// Full-chunk mode (the verification failed or was skipped): the LOD draws nothing generated.
    pub full_mode: bool,
}

pub struct Game<S: Scene> {
    pub player_id: u16,
    predictor: Predictor,
    streamer: Streamer<S>,
    pool: PoolRef,
    scene: SceneRef<S>,
    outbox: Outbox,
    remotes: RemotePlayers,
    proxies: HashSet<u16>,
    recent: VecDeque<InputFrame>,
    accumulator: f64,
    last_frame_ms: Option<f64>,
    last_snapshot_tick: u32,
    input_buffer: u8,
    tick_rate: f64,
    health: u8,
    dead: bool,
    death_feet: [f64; 3],
    previous: Option<ClientState>,
    current: ClientState,
    flip: FaceFlip,
    last_draw_ms: Option<f64>,
    eye: EyeCamera,
    pub interaction: BlockInteraction,
    lod: Option<LodSystem>,
    /// The drawing's size in CSS pixels, for LOD selection.
    pub viewport_css: (f64, f64),
    /// Extra lines for the debug overlay.
    pub debug_note: String,
    pub memory_note: String,
    pub hud: Hud,
    pub debug_visible: bool,
    /// The session's round-trip time, for the debug overlay (set by the shell).
    pub rtt_ms: Option<f64>,
    /// The verification hash sent (None until the verification chunk is generated).
    verified: Option<u64>,
    pending_lod: Option<LodSettings>,
    /// The latest F4 map (None until one arrives; an empty map without a terrain).
    map: Option<Vec<u8>>,
    /// A placed camera, if a tool set one.
    view: Option<ViewOverride>,
}

impl<S: Scene> Game<S> {
    /// A game for a joined session: `verification` is the Welcome's verification chunk, generated
    /// and hashed first (or skipped with `full_chunks`, sending 0: full-chunk mode, §6.3).
    pub fn new(
        player_id: u16,
        pool: PoolRef,
        scene: SceneRef<S>,
        verification: Option<ChunkCoord>,
        config: PlayerControllerConfig,
    ) -> Self {
        let outbox: Outbox = Rc::new(RefCell::new(Vec::new()));
        let resync_out = outbox.clone();
        let streamer = ChunkStreamer::with_resync(
            VoxelWorld::streamed(),
            ChunkJobs(pool.clone()),
            MeshJobs(pool.clone()),
            View(scene.clone()),
            move |coords: &[Coord]| {
                resync_out
                    .borrow_mut()
                    .push(Send::Control(Message::ChunkResync(coords.to_vec())));
            },
        );
        let predictor = Predictor::new(config);
        let current = ClientState::of(&predictor);
        let mut flip = FaceFlip::new();
        flip.tick(current.face); // joining on face B does not turn the player
        let mut eye = EyeCamera::new();
        eye.tick(&current.eye_state(), 1.0 / SIM_HZ as f64);
        let mut game = Self {
            player_id,
            predictor,
            streamer,
            pool,
            scene,
            outbox,
            remotes: RemotePlayers::default(),
            proxies: HashSet::default(),
            recent: VecDeque::new(),
            accumulator: 0.0,
            last_frame_ms: None,
            last_snapshot_tick: 0,
            input_buffer: 0,
            tick_rate: 1.0,
            health: 0,
            dead: false,
            death_feet: [0.0; 3],
            previous: None,
            current,
            flip,
            last_draw_ms: None,
            eye,
            interaction: BlockInteraction::new(),
            lod: None,
            viewport_css: (1280.0, 720.0),
            debug_note: String::new(),
            memory_note: String::new(),
            hud: Hud::default(),
            debug_visible: false,
            rtt_ms: None,
            verified: None,
            pending_lod: None,
            map: None,
            view: None,
        };
        match verification {
            Some(c) => game
                .pool
                .borrow_mut()
                .submit(Purpose::Verify, crate::jobs::Job::Generate { coord: c }),
            None => game.verify(0),
        }
        game
    }

    /// Places the camera (None: back to the player's eyes).
    pub fn set_view(&mut self, view: Option<ViewOverride>) {
        self.view = view;
    }
    pub fn view(&self) -> Option<ViewOverride> {
        self.view
    }

    /// Turns the whole-world view on once the verification is known (`?lod=0` leaves it off).
    pub fn enable_lod(&mut self, settings: LodSettings) {
        self.pending_lod = Some(settings);
        self.start_lod();
    }

    fn verify(&mut self, hash: u64) {
        self.verified = Some(hash);
        self.outbox
            .borrow_mut()
            .push(Send::Control(Message::WorldgenCheck(hash)));
        self.start_lod();
    }

    fn start_lod(&mut self) {
        let (Some(s), Some(hash)) = (self.pending_lod, self.verified) else {
            return;
        };
        let pool = self.pool.borrow();
        let mut options = dwell_lod::LodOptions::new(
            s.pixel_error,
            s.cache_bytes,
            pool.capacity(false),
            2 * pool.capacity(true),
        );
        options.request_chunks = true;
        drop(pool);
        let mut lod = LodSystem::new(options);
        lod.set_full_mode(s.full_mode || hash == 0);
        lod.set_detail_distance(s.detail_distance_m);
        self.lod = Some(lod);
        self.pending_lod = None;
    }

    /// Asks for the F4 map: n × n column samples from (x0, z0), `step` apart, of `face`.
    pub fn request_map(&mut self, x0: i32, z0: i32, step: i32, n: i32, face: u8) {
        self.pool.borrow_mut().submit(
            Purpose::Map,
            crate::jobs::Job::Map {
                x0,
                z0,
                step,
                n,
                face,
            },
        );
    }
    /// The latest map, once (height i16 LE and an outside flag per sample; empty without a terrain).
    pub fn take_map(&mut self) -> Option<Vec<u8>> {
        self.map.take()
    }
    /// Regenerates the chunk under the player and diffs it against the world (the F3 note).
    pub fn request_diff(&mut self, c: Coord) {
        if self.streamer.revision(c).is_none() {
            self.debug_note.clear();
            return;
        }
        self.pool.borrow_mut().submit(
            Purpose::Diff(c),
            crate::jobs::Job::Generate {
                coord: ChunkCoord::from_array(c),
            },
        );
    }

    /// The job workers' memory (bytes), for the F3 memory line.
    pub fn worker_memory(&self) -> Vec<f64> {
        self.pool.borrow().memory()
    }

    pub fn lod(&mut self) -> Option<&mut LodSystem> {
        self.lod.as_mut()
    }
    /// Which chunks the renderer draws (the LOD draws instead of the others).
    pub fn chunk_visible(&self, c: ChunkCoord) -> bool {
        self.lod.as_ref().is_none_or(|l| l.chunk_visible(c))
    }
    pub fn take_outbox(&mut self) -> Vec<Send> {
        std::mem::take(&mut self.outbox.borrow_mut())
    }
    pub fn world(&mut self) -> &mut VoxelWorld {
        self.streamer.store_mut()
    }
    pub fn voxel(&mut self, x: i32, y: i32, z: i32) -> u16 {
        self.streamer.store_mut().get_voxel(x, y, z)
    }
    pub fn state(&self) -> &ClientState {
        &self.current
    }
    pub fn terrain_stats(&self) -> StreamStats {
        self.streamer.stats()
    }
    pub fn chunk_revision(&self, c: Coord) -> Option<u32> {
        self.streamer.revision(c)
    }
    pub fn allow_flight(&mut self, input: &mut KeyboardMouseInput, allowed: bool) {
        input.flight.set_allowed(allowed);
    }

    /// Handles a snapshot, player event, chunk or LOD message from the session.
    pub fn on_game_message(&mut self, m: Message, byte_len: usize, now_ms: f64) {
        match m {
            Message::ChunkData(d) => self.streamer.on_chunk_data(ChunkMessage::from(d)),
            Message::ChunkUnload(u) => self.streamer.on_chunk_unload(&u.coords),
            Message::VoxelModification(v) => self.streamer.on_voxel_modification(&v.chunks),
            Message::LodIndex(_) | Message::LodIndexUpdate(_) | Message::LodData(_) => {
                if let Some(lod) = &mut self.lod {
                    lod.on_message(&m, byte_len, now_ms);
                }
            }
            Message::PhysicsSnapshot(s) => {
                if s.server_tick <= self.last_snapshot_tick {
                    return; // reordered datagram
                }
                self.last_snapshot_tick = s.server_tick;
                self.predictor.on_snapshot(self.streamer.store_mut(), &s);
                self.health = s.local.health;
                self.dead = s.local.flags & player_flags::DEAD != 0;
                if self.dead {
                    self.death_feet = s.local.position;
                }
                self.input_buffer = s.local.input_buffer;
                self.tick_rate = if self.input_buffer <= BUFFER_LOW {
                    1.0 + RATE_NUDGE
                } else if self.input_buffer >= BUFFER_HIGH {
                    1.0 - RATE_NUDGE
                } else {
                    1.0
                };
                self.remotes.push(s.server_tick, &s.remotes, now_ms);
                let mut seen = HashSet::default();
                for r in self.remotes.latest() {
                    if r.is_dead() {
                        continue;
                    }
                    seen.insert(r.player_id);
                    let feet = DVec3::new(r.feet[0], r.feet[1], r.feet[2]);
                    let velocity = Vec3::new(r.velocity[0], r.velocity[1], r.velocity[2]);
                    self.predictor.set_remote(
                        r.player_id,
                        feet,
                        velocity,
                        r.is_crouched(),
                        0.0,
                        r.is_face_b(),
                    );
                    self.proxies.insert(r.player_id);
                }
                for id in self
                    .proxies
                    .iter()
                    .copied()
                    .filter(|id| !seen.contains(id))
                    .collect::<Vec<_>>()
                {
                    self.predictor.remove_remote(id);
                    self.proxies.remove(&id);
                }
                if !self.current.active {
                    self.current = ClientState::of(&self.predictor);
                    self.flip.tick(self.current.face);
                }
            }
            Message::PlayerEvent(e) if e.player_id == self.player_id => match e.kind {
                PlayerEventKind::Knockback => {
                    let v = Vec3::new(e.vector[0], e.vector[1], e.vector[2]);
                    self.predictor
                        .on_knockback(self.streamer.store_mut(), e.input_seq, v);
                }
                PlayerEventKind::Damage => self.hud.damaged = true,
                _ => {}
            },
            _ => {}
        }
    }

    /// Applies finished background jobs.
    pub fn on_jobs(&mut self, done: Vec<Done>) {
        for (purpose, out) in done {
            match (purpose, out) {
                (Purpose::Chunk(c, token), Output::Chunk { voxels, .. }) => {
                    self.streamer.on_generated(c, token, &voxels)
                }
                (Purpose::Chunk(c, _), Output::Failed(e)) => self.streamer.on_generate_failed(c, e),
                (Purpose::ChunkMesh(c, token), Output::ChunkMesh(m)) => {
                    self.streamer.on_meshed(c, token, *m)
                }
                (Purpose::Verify, Output::Chunk { hash, .. }) => self.verify(hash),
                (Purpose::Map, Output::Map(bytes)) => self.map = Some(bytes.unwrap_or_default()),
                (Purpose::Diff(c), Output::Chunk { voxels, .. }) => {
                    let revision = self.streamer.revision(c);
                    let padded = dwell_game::chunk_stream::ChunkStore::padded_chunk(
                        self.streamer.store_mut(),
                        c,
                    );
                    self.debug_note = match (
                        revision,
                        dwell_game::chunk_diff::count_changed(&padded, &voxels),
                    ) {
                        (Some(rev), Ok(n)) => format!(
                            "chunk ({}, {}, {}) rev {rev} · {n} voxels differ from generation",
                            c[0], c[1], c[2]
                        ),
                        _ => String::new(),
                    };
                }
                (Purpose::Verify, _) => self.verify(0),
                (Purpose::Bounds(level, i, k), out) => {
                    if let Some(lod) = &mut self.lod {
                        lod.on_lod_bounds(
                            level,
                            i,
                            k,
                            if let Output::Bounds(b) = out {
                                Some(b)
                            } else {
                                None
                            },
                        );
                    }
                }
                (Purpose::Section(token), out) => {
                    if let Some(lod) = &mut self.lod {
                        let section = match out {
                            Output::Lod {
                                cells,
                                surface,
                                surface_b,
                            } => Some(dwell_lod::GeneratedSection {
                                cells,
                                surface,
                                surface_b,
                            }),
                            _ => None,
                        };
                        lod.on_generated(token, section);
                    }
                }
                (Purpose::SectionMesh(token), out) => {
                    if let Some(lod) = &mut self.lod {
                        match out {
                            Output::SectionMesh(m) => {
                                lod.on_meshed(token, *m, &mut View(self.scene.clone()))
                            }
                            _ => lod.on_mesh_failed(token),
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub fn debug_state(&self) -> DebugState {
        let c = &self.current;
        DebugState {
            player_id: self.player_id,
            active: c.active,
            feet: [
                c.position[0],
                c.position[1] - f64::from(c.face) * c.half_height,
                c.position[2],
            ],
            health: self.health,
            dead: self.dead,
            remotes: self
                .remotes
                .latest()
                .iter()
                .map(|r| (r.player_id, r.feet, r.is_dead()))
                .collect(),
            stats: c.stats,
            terrain: self.streamer.stats(),
            terrain_ready: self.terrain_ready(),
            target: self.interaction.target(),
            lod: self.lod.as_ref().map(LodSystem::debug_stats),
            flying: c.state == PlayerState::Flying,
        }
    }

    /// Breaks or places at the crosshair (§6.5): only while alive and playing.
    pub fn edit(&mut self, action: EditAction, now_ms: f64) -> bool {
        if self.dead || !self.terrain_ready() {
            return false;
        }
        match self.interaction.act(action, now_ms) {
            Some(m) => {
                self.outbox.borrow_mut().push(Send::Control(m));
                true
            }
            None => false,
        }
    }

    /// Prediction waits until the terrain around the player has arrived (collision needs it).
    fn terrain_ready(&self) -> bool {
        self.current.active && self.streamer.ready_around(self.current.position)
    }

    /// One animation frame: run due ticks, then draw. `now` is a clock in ms for budgets.
    pub fn frame(
        &mut self,
        now_ms: f64,
        input: &mut KeyboardMouseInput,
        touch: Option<&TouchState>,
        now: &mut dyn FnMut() -> f64,
    ) {
        let done = telemetry::span("jobs.poll", || self.pool.borrow_mut().poll());
        telemetry::add("jobs.done", done.len() as f64);
        telemetry::span("jobs.apply", || self.on_jobs(done));
        let elapsed = self.last_frame_ms.map_or(0.0, |l| now_ms - l);
        self.last_frame_ms = Some(now_ms);
        self.accumulator =
            (self.accumulator + elapsed * self.tick_rate).min(MAX_TICKS_PER_FRAME * TICK_MS);
        let t0 = telemetry::now_ms();
        while self.accumulator >= TICK_MS {
            self.accumulator -= TICK_MS;
            self.tick(input, touch);
            telemetry::add("ticks.count", 1.0);
        }
        telemetry::add("ticks", telemetry::now_ms() - t0);
        telemetry::span("game.draw", || self.draw(now_ms, input, now));
    }

    fn tick(&mut self, input: &mut KeyboardMouseInput, touch: Option<&TouchState>) {
        let c = self.current;
        if !prediction_may_run(c.active, c.state, self.terrain_ready()) {
            return;
        }
        let frame = quantize_input(&input.sample(touch), self.predictor.next_seq());
        self.predictor.tick(self.streamer.store_mut(), &frame);
        self.recent.push_back(frame);
        while self.recent.len() > MAX_INPUTS_PER_DATAGRAM {
            self.recent.pop_front();
        }
        self.outbox
            .borrow_mut()
            .push(Send::Datagram(Message::PlayerInput(PlayerInput {
                last_snapshot_tick: self.last_snapshot_tick,
                inputs: self.recent.iter().copied().collect(),
            })));
        self.previous = Some(self.current);
        self.current = ClientState::of(&self.predictor);
        record_tick(&self.current, &frame);
        // Crossing the midplane turns the heading round (a half somersault, face_flip.rs).
        let turn = self.flip.tick(self.current.face);
        if turn != 0.0 {
            input.look.yaw = (input.look.yaw + turn) % 360.0;
        }
        self.eye
            .tick(&self.current.eye_state(), 1.0 / SIM_HZ as f64);
        input.look.turn_sign = f64::from(self.current.face);
        // The camera turns with rotating ground (PPC yawDelta).
        input.look.yaw += self.current.platform_yaw_delta;
    }

    fn draw(&mut self, now_ms: f64, input: &KeyboardMouseInput, now: &mut dyn FnMut() -> f64) {
        let c = self.current;
        let face = c.face;
        let flip = self.flip.draw(
            face,
            self.last_draw_ms.map_or(0.0, |l| (now_ms - l) / 1000.0),
        );
        self.last_draw_ms = Some(now_ms);
        let p = self.previous.unwrap_or(c);
        let alpha = self.accumulator / TICK_MS;
        let center: [f64; 3] = std::array::from_fn(|i| {
            let a = p.position[i] + p.render_offset[i];
            a + (c.position[i] + c.render_offset[i] - a) * alpha
        });
        telemetry::span("mesh_dirty", || {
            self.streamer.mesh_dirty(
                if c.active { center } else { [0.5, 64.0, 0.5] },
                MESH_BUDGET_MS,
                &mut *now,
            );
        });

        // Remote players, interpolated.
        let views = self.remotes.sample(now_ms);
        let mut drawn = HashSet::default();
        for v in &views {
            drawn.insert(v.player_id);
            let view = self.player_view(
                v.player_id,
                v.feet,
                f64::from(v.yaw),
                v.is_crouched(),
                v.is_dead(),
                if v.is_face_b() { -1.0 } else { 1.0 },
            );
            self.scene
                .borrow_mut()
                .set_player(u32::from(v.player_id), Some(view));
        }
        for id in self.remotes.ids() {
            if !drawn.contains(&id) {
                self.scene.borrow_mut().set_player(u32::from(id), None);
            }
        }

        let (yaw, pitch) = (input.look.yaw, input.look.pitch);
        let camera_eye;
        if self.dead {
            // Third person over the body.
            let view = self.player_view(
                self.player_id,
                self.death_feet,
                yaw,
                false,
                true,
                f64::from(face),
            );
            let mut scene = self.scene.borrow_mut();
            scene.set_player(u32::from(self.player_id), Some(view));
            let r = yaw.to_radians();
            let [x, y, z] = self.death_feet;
            camera_eye = [
                x - r.sin() * 4.0,
                y + f64::from(face) * 2.5,
                z - r.cos() * 4.0,
            ];
            scene.set_camera(camera_eye, yaw, -25.0, f64::from(face), 0.0);
            drop(scene);
            self.show_hud_text();
            self.target(None, input);
        } else {
            self.scene
                .borrow_mut()
                .set_player(u32::from(self.player_id), None);
            self.show_hud_text();
            // Eye height is smoothed per tick (steps, crouching; eye.rs), then interpolated.
            let eye = [center[0], self.eye.draw(alpha), center[2]];
            camera_eye = eye;
            self.scene
                .borrow_mut()
                .set_camera(eye, yaw, pitch, f64::from(face), flip);
            let ready = self.terrain_ready();
            self.target(ready.then_some(eye), input);
        }
        let camera_eye = match self.view {
            Some(v) => {
                self.scene
                    .borrow_mut()
                    .set_camera(v.eye, v.yaw_deg, v.pitch_deg, v.face(), 0.0);
                self.scene.borrow_mut().set_block_outline(None, 1.0);
                v.eye
            }
            None => camera_eye,
        };
        let store = self.streamer.store_mut();
        let eye_fluid =
            dwell_world::fluid::eye_fluid(camera_eye, |x, y, z| store.get_voxel(x, y, z));
        self.scene.borrow_mut().set_eye_fluid(eye_fluid);
        telemetry::span("lod", || self.update_lod(&c, now_ms, input));

        self.hud.health = self.health;
        if self.debug_visible {
            let mut lines = vec![format_debug(
                &c,
                self.health,
                self.input_buffer,
                self.tick_rate,
                views.len(),
                self.rtt_ms,
            )];
            if let Some(lod) = &self.lod {
                lines.push(format_lod_stats(&lod.debug_stats()));
            }
            lines.push(format_render_stats(&self.scene.borrow().stats()));
            if !self.memory_note.is_empty() {
                lines.push(self.memory_note.clone());
            }
            if !self.debug_note.is_empty() {
                lines.push(self.debug_note.clone());
            }
            self.hud.debug = Some(lines.join("\n"));
            let probe = probe_lines(center, &c);
            self.scene.borrow_mut().set_debug_lines(Some(&probe));
        } else {
            self.hud.debug = None;
            self.scene.borrow_mut().set_debug_lines(None);
        }
    }

    fn show_hud_text(&mut self) {
        let text = hud_text(&HudTextInput {
            dead: self.dead,
            active: self.current.active,
            terrain_ready: self.terrain_ready(),
            terrain_error: self.streamer.error.clone(),
        });
        self.hud.message = text.center;
        self.hud.status = text.status;
    }

    /// The whole-world view around the camera.
    fn update_lod(&mut self, c: &ClientState, now_ms: f64, input: &KeyboardMouseInput) {
        if !c.active || self.lod.is_none() {
            return;
        }
        let viewport = dwell_lod::lod_viewport(self.viewport_css.0, self.viewport_css.1);
        let (position, yaw_deg, pitch_deg) = match self.view {
            Some(v) => (v.eye, v.yaw_deg, v.face() * v.pitch_deg),
            None => (
                if self.dead {
                    self.death_feet
                } else {
                    c.position
                },
                input.look.yaw,
                // The world pitch: a face-B player's up is −y.
                f64::from(c.face) * input.look.pitch,
            ),
        };
        let camera = LodCamera {
            position,
            yaw_deg,
            pitch_deg,
            fov_y_deg: viewport.fov_y_deg,
            aspect: viewport.aspect,
            height_px: viewport.height_px,
        };
        let mut host = LodHost {
            pool_gen: ChunkJobs(self.pool.clone()),
            pool_mesh: MeshJobs(self.pool.clone()),
            view: View(self.scene.clone()),
            streamer: &self.streamer,
            outbox: &self.outbox,
        };
        if let Some(lod) = &mut self.lod {
            lod.update(&camera, now_ms, &mut host);
        }
    }

    /// Targets the block under the crosshair from `eye` and outlines it (none while not playing).
    fn target(&mut self, eye: Option<[f64; 3]>, input: &KeyboardMouseInput) {
        let face = self.current.face;
        let t = self.interaction.update(
            self.streamer.store_mut(),
            eye,
            input.look.yaw,
            input.look.pitch,
            face,
        );
        let Some(t) = t else {
            self.scene.borrow_mut().set_block_outline(None, 1.0);
            return;
        };
        // The outline hugs the shape's top: slabs and low slopes are not a full cell tall.
        let shape = dwell_blocks::shape::shape_of(
            self.streamer
                .store_mut()
                .get_voxel(t.cell[0], t.cell[1], t.cell[2]),
        );
        let height = if shape.inverted {
            1.0
        } else {
            f64::from(shape.max_y)
        };
        self.scene
            .borrow_mut()
            .set_block_outline(Some(t.cell.map(f64::from)), height);
    }

    fn player_view(
        &self,
        id: u16,
        feet: [f64; 3],
        yaw: f64,
        crouched: bool,
        dead: bool,
        face: f64,
    ) -> PlayerView {
        PlayerView {
            feet,
            face,
            yaw,
            crouched,
            dead,
            color: PLAYER_COLORS[usize::from(id) % PLAYER_COLORS.len()],
            radius: self.current.radius as f32,
            height: self.current.standing_height as f32,
        }
    }
}

/// The LOD system's collaborators for one update.
struct LodHost<'a, S: Scene> {
    pool_gen: ChunkJobs,
    pool_mesh: MeshJobs,
    view: View<S>,
    streamer: &'a Streamer<S>,
    outbox: &'a Outbox,
}

impl<S: Scene> dwell_lod::SectionSource for LodHost<'_, S> {
    fn lod_bounds(&mut self, level: i32, i: i32, k: i32) {
        self.pool_gen.lod_bounds(level, i, k);
    }
    fn lod(&mut self, c: dwell_world::lod::LodCoord, token: u64) {
        self.pool_gen.lod(c, token);
    }
}
impl<S: Scene> dwell_lod::SectionMesher for LodHost<'_, S> {
    fn mesh_section(&mut self, job: dwell_lod::LodMeshJob, token: u64) {
        self.pool_mesh.mesh_section(job, token);
    }
}
impl<S: Scene> LodView for LodHost<'_, S> {
    fn set_lod_section(
        &mut self,
        id: i64,
        origin: [f64; 3],
        cell_size: f64,
        meshes: Option<SectionMeshes>,
    ) {
        self.view.set_lod_section(id, origin, cell_size, meshes);
    }
    fn show_lod_sections(&mut self, visible: &[(i64, u8)]) {
        self.view.show_lod_sections(visible);
    }
    fn show_lod_stand_ins(&mut self, stand_ins: &[StandInBox]) {
        self.view.show_lod_stand_ins(stand_ins);
    }
}
impl<S: Scene> dwell_lod::ChunkReadiness for LodHost<'_, S> {
    fn chunk_drawable(&self, coord: ChunkCoord) -> bool {
        self.streamer.drawable([coord.x, coord.y, coord.z])
    }
    fn chunk_loaded(&self, coord: ChunkCoord) -> bool {
        self.streamer.is_loaded([coord.x, coord.y, coord.z])
    }
}
impl<S: Scene> dwell_lod::LodNetwork for LodHost<'_, S> {
    fn request_sections(&mut self, sections: Vec<dwell_protocol::LodSectionRequest>) {
        self.outbox
            .borrow_mut()
            .push(Send::Control(Message::LodRequest(sections)));
    }
    fn request_chunks(&mut self, coords: Vec<dwell_protocol::ChunkCoordNet>) {
        self.outbox
            .borrow_mut()
            .push(Send::Control(Message::ChunkRequest(coords)));
    }
}

fn js_round(v: f64) -> i64 {
    (v + 0.5).floor() as i64
}

const STATE_NAMES: [&str; 10] = [
    "Idle",
    "Walking",
    "Running",
    "Crouching",
    "Sliding",
    "Jumping",
    "Falling",
    "Climbing",
    "Swimming",
    "Flying",
];

fn ground_name(k: GroundKind) -> &'static str {
    match k {
        GroundKind::None => "None",
        GroundKind::Terrain => "Terrain",
        GroundKind::Tier1Body => "Tier1Body",
        GroundKind::Player => "Player",
    }
}

/// Text of the debug overlay (`formatDebug` in ui/hud.ts).
pub fn format_debug(
    c: &ClientState,
    health: u8,
    input_buffer: u8,
    tick_rate: f64,
    remotes: usize,
    rtt_ms: Option<f64>,
) -> String {
    let f2 = |v: f64| format!("{v:.2}");
    let vec = |v: [f64; 3]| format!("({}, {}, {})", f2(v[0]), f2(v[1]), f2(v[2]));
    let s = &c.stats;
    let smoothing = c.render_offset.iter().map(|v| v * v).sum::<f64>().sqrt();
    [
        format!(
            "state {} · ground {} gap {}{}{}{}",
            STATE_NAMES.get(c.state as usize).copied().unwrap_or("?"),
            ground_name(c.ground_kind),
            f2(c.ground_gap),
            if c.touching_wall { " · wall" } else { "" },
            if c.ceiling_blocked { " · ceiling" } else { "" },
            if c.submerged > 0.0 { format!(" · submerged {}", f2(c.submerged)) } else { String::new() }
        ),
        format!("pos {} vel {}", vec(c.position), vec(c.velocity)),
        format!("layers current {} external {} target {}", vec(c.horizontal_current), vec(c.horizontal_external), vec(c.target_velocity)),
        format!(
            "prediction seq {} · snapshots {} · replays {} · snaps {} · knockback replays {}",
            c.next_seq.wrapping_sub(1),
            s.snapshots,
            s.replays,
            s.snaps,
            s.knockback_replays
        ),
        format!(
            "error at ack {} cm · last correction {} cm · smoothing {} cm",
            f2(f64::from(s.last_error) * 100.0),
            f2(f64::from(s.last_correction) * 100.0),
            f2(smoothing * 100.0)
        ),
        format!(
            "server input buffer {input_buffer} · tick rate ×{tick_rate:.3} · RTT {} · remote players {remotes} · health {health}/{}",
            rtt_ms.map_or("–".to_string(), |r| format!("{} ms", js_round(r))),
            players::MAX_HEALTH
        ),
    ]
    .join("\n")
}

/// Probe rays (ground ring, walls) and velocity, for the debug overlay.
fn probe_lines(center: [f64; 3], c: &ClientState) -> Vec<Line> {
    let mut lines = Vec::new();
    let grounded = c.controller_flags & controller_flags::GROUNDED != 0;
    let ring = c.radius * 0.9;
    let down =
        f64::from(c.face) * (c.half_height + if grounded { c.max_step_height } else { 0.15 });
    for i in -1..16 {
        let a = f64::from(i) * std::f64::consts::TAU / 16.0;
        let o = if i < 0 {
            center
        } else {
            [
                center[0] + a.cos() * ring,
                center[1],
                center[2] + a.sin() * ring,
            ]
        };
        lines.push(Line {
            from: o,
            to: [o[0], o[1] - down, o[2]],
            color: if grounded { 0x3ce05a } else { 0xe0463c },
        });
    }
    for (dx, dz) in [(0.0, 1.0), (0.0, -1.0), (-1.0, 0.0), (1.0, 0.0)] {
        let o = [center[0] + dx * ring, center[1], center[2] + dz * ring];
        lines.push(Line {
            from: o,
            to: [o[0] + dx * 0.16, o[1], o[2] + dz * 0.16],
            color: if c.touching_wall { 0xffd23c } else { 0x9aa3b5 },
        });
    }
    let v = c.velocity;
    lines.push(Line {
        from: center,
        to: [
            center[0] + v[0] * 0.25,
            center[1] + v[1] * 0.25,
            center[2] + v[2] * 0.25,
        ],
        color: 0x3cc8ff,
    });
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_debug_overlay_shows_state_layers_and_prediction_stats() {
        // ui/hud.test.ts's case.
        let mut c = ClientState::of(&Predictor::new(PlayerControllerConfig::default()));
        c.state = PlayerState::Running;
        c.ground_kind = GroundKind::Terrain;
        c.touching_wall = true;
        c.render_offset = [0.0, 0.0, 0.03];
        c.next_seq = 101;
        c.stats.replays = 2;
        let text = format_debug(&c, 83, 2, 1.0, 1, Some(151.4));
        assert!(
            text.contains("state Running · ground Terrain gap 0.00 · wall"),
            "{text}"
        );
        assert!(text.contains("replays 2"));
        assert!(text.contains("smoothing 3.00 cm"));
        assert!(text.contains("RTT 151 ms"));
        assert!(text.contains("health 83/100"));
    }
}
