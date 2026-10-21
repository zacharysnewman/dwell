//! The in-game loop (PLAYER_CONTROLLER.md §8, §9), ported from `client/src/game/game.ts`: fixed 60 Hz
//! ticks of local prediction, input datagrams, snapshot reconciliation, interpolated remote players,
//! the first-person camera, terrain streaming and the whole-world view, and block editing. Platform
//! free: time is passed in, the renderer is a [`Scene`] and the workers a [`Pool`], so it runs
//! natively under test as it does in the browser.

use dwell_world::hash::HashSet;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use dwell_blocks::StateId;
use dwell_game::block_interaction::{BlockInteraction, BlockTarget, TargetSource, view_direction};
use dwell_game::brush_tool::{BrushAction, BrushOutline, BrushPanel, BrushTool};
use dwell_game::chunk_stream::{ChunkMessage, ChunkStreamer, Coord, StreamStats, TerrainView};
use dwell_game::edit_prediction::EditPredictor;
use dwell_game::eye::{EyeCamera, EyeState, StepRamp};
use dwell_game::face_flip::FaceFlip;
use dwell_game::gate::prediction_may_run;
use dwell_game::hud_text::{HudTextInput, hud_text};
use dwell_game::input::{EditAction, KeyboardMouseInput, quantize_input};
use dwell_game::region_tools::{RegionTools, ToolAction, ToolBoxKind, ToolContext};
use dwell_game::remotes::RemotePlayers;
use dwell_game::sound::{CellChange, EditSounds, MoverSample, MoverSounds, SoundEvent, SoundKind};
use dwell_game::touch::TouchState;
use dwell_lod::fade::Fade;
use dwell_lod::{LodCamera, LodSystem, LodView, StandInBox, format_lod_stats};
use dwell_look::daynight::{DAY_NIGHT, DayState};
use dwell_look::lod_blend::LOD_BLEND;
use dwell_look::motion::{MOTION, MotionFx, MotionInput, MotionSettings, Speeds, effective_fov};
use dwell_look::spyglass::{SPYGLASS, Spyglass, SpyglassView, zoom_fov};
use dwell_mesh::{ChunkMeshes, SectionMeshes};
use dwell_physics::config::PlayerControllerConfig;
use dwell_physics::net::to_net;
use dwell_physics::predictor::{Predictor, PredictorStats};
use dwell_physics::{DVec3, Vec3};
use dwell_protocol::{
    GroundKind, InputFrame, MAX_INPUTS_PER_DATAGRAM, Message, PlayerEventKind, PlayerInput,
    PlayerState, SIM_HZ, controller_flags, player_flags, players,
};
use dwell_world::{ChunkCoord, FaceFrame, VoxelWorld};

use crate::bodies::Bodies;
use crate::jobs::Output;
use crate::pool::{ChunkJobs, Done, MeshJobs, PoolRef, Purpose};
use crate::render::display::{RenderStats, format_render_stats};
use crate::render::seam::TerrainRenderer;
use crate::render::{ArmsView, Held, Line, PlayerView, RegionBox, StandIn, WindView};
use crate::telemetry::{self, TickSample};

const TICK_MS: f64 = 1000.0 / SIM_HZ as f64;
/// Ticks one frame may catch up (1 s): beyond it time is dropped, so the frame loop can't spiral.
/// The key of your first-person arms' light in `Game::character_lights` (not a player id).
const ARMS_LIGHT: u32 = u32::MAX;

/// It covers a slow device's frame (down to 1 fps): a client that drops time predicts fewer ticks
/// than the server's 60 Hz, the server fills the missing ones by repeating the last input, and the
/// replays drag the player (a snap once past RECONCILE_SNAP_DISTANCE; e2e/play.spec.ts on CI) or
/// land a held jump one tick into it (e2e/touch.spec.ts at CI's 440–810 ms frames). The server's
/// input queue and latency window (dwell-sim `MAX_QUEUED_INPUTS`, `LATENCY_WINDOW_TICKS`) hold a
/// frame of this many inputs.
const MAX_TICKS_PER_FRAME: f64 = 60.0;
/// Main-thread time per frame for starting mesh jobs (copying chunks' voxels out of the world)
/// and, apart from it, for lighting chunks that arrived (the flood light, docs/DAY_NIGHT.md §11):
/// each at least `STREAM_BUDGET_MS`, or `STREAM_FRAME_SHARE` of the last frame's interval on a
/// slow page, up to the cap. A slow page's frames are long anyway, and a fixed 4 ms there started
/// one or two jobs a frame: at CI's ~5 fps the view took 20 s to mesh (e2e/play.spec.ts). The
/// shares feed back into the next frame's interval, at most by 1/(1 − 2 × share).
const STREAM_BUDGET_MS: f64 = 4.0;
const STREAM_FRAME_SHARE: f64 = 0.1;
const STREAM_BUDGET_MAX_MS: f64 = 25.0;

/// This frame's light budget, and its mesh budget (ms each), after a frame interval of
/// `frame_ms`.
pub fn stream_budget_ms(frame_ms: f64) -> f64 {
    (frame_ms * STREAM_FRAME_SHARE).clamp(STREAM_BUDGET_MS, STREAM_BUDGET_MAX_MS)
}
/// Server input buffer outside [LOW, HIGH] nudges the local tick rate by ±RATE_NUDGE.
const BUFFER_LOW: u8 = 1;
const BUFFER_HIGH: u8 = 4;
const RATE_NUDGE: f64 = 0.02;
/// The region tools' outlines: the selection, and where a paste would go.
const REGION_SELECTION_COLOR: u32 = 0xffffff;
const REGION_GHOST_COLOR: u32 = 0x6fd3ff;

/// The whole cell, as an outline box.
pub const FULL_CELL: ([f32; 3], [f32; 3]) = ([0.0; 3], [1.0; 3]);

/// What the game draws through (the wgpu renderer, or a recorder in tests). The terrain part is
/// [`TerrainRenderer`]'s: each chunk's and section's meshes; the rest is the camera, players,
/// outline, lines and stats.
pub trait Scene: TerrainRenderer {
    fn set_player(&mut self, id: u32, view: Option<PlayerView>);
    /// Your first-person arms this frame (docs/CHARACTERS.md §5), or none.
    fn set_arms(&mut self, _arms: Option<ArmsView>) {}
    /// Your own body this frame, cast into the shadow maps only (none while it is drawn).
    fn set_own_shadow(&mut self, _body: Option<PlayerView>) {}
    /// Your own body drawn in first person from the neck down (docs/CHARACTERS.md §5.1), or none.
    fn set_own_body(&mut self, _body: Option<PlayerView>) {}
    fn set_camera(
        &mut self,
        eye: [f64; 3],
        yaw_deg: f64,
        pitch_deg: f64,
        face: FaceFrame,
        flip: f64,
    );
    /// The fluid whose drawn surface the camera's eye is under (`dwell_world::fluid::eye_fluid`).
    fn set_eye_fluid(&mut self, _eye_fluid: Option<dwell_world::fluid::EyeFluid>) {}
    /// Outlines the targeted block: its cell's minimum corner and the box in the cell (min, max
    /// corner; `dwell_world::edit::target_box`), or none.
    fn set_block_outline(&mut self, cell: Option<[f64; 3]>, bounds: ([f32; 3], [f32; 3]));
    /// The frame's vertical FOV (degrees; the speed boost, docs/MOTION_EFFECTS.md): the same value
    /// the LOD selection uses. `None`: the screen's own.
    fn set_fov(&mut self, fov_y_deg: Option<f64>);
    /// The wind streaks to draw this frame.
    fn set_wind(&mut self, wind: WindView);
    /// The spyglass's magnification this frame (1 unzoomed): the ink outline's fade follows it.
    fn set_magnification(&mut self, _magnification: f64) {}
    fn set_region_boxes(&mut self, boxes: &[RegionBox]);
    /// The brush tool's wireframe at its target (BRUSHES.md), or none.
    fn set_brush_outline(&mut self, _outline: Option<&BrushOutline>) {}
    fn set_debug_lines(&mut self, lines: Option<&[Line]>);
    fn stats(&self) -> RenderStats;
    /// The day/night cycle's state this frame, and seconds for the stars' twinkle.
    fn set_day(&mut self, _day: &DayState, _seconds: f64) {}
}

/// Dev switches over the day/night cycle (`?time=`, `?moon=`, `?timespeed=`, the bench's `time`
/// step): they change this client's view only, never the world's time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DayOverride {
    /// The time of day (0–1, 0 midnight) instead of the world's.
    pub time: Option<f64>,
    /// The moon's phase (0 new, 0.5 full) instead of the day count's.
    pub moon: Option<f64>,
    /// World time runs this many times faster here (1 = as the server's).
    pub speed: f64,
}

impl Default for DayOverride {
    fn default() -> Self {
        Self {
            time: None,
            moon: None,
            speed: 1.0,
        }
    }
}

impl DayOverride {
    /// Reads `?time=`, `?moon=` and `?timespeed=` from a query string; anything missing, not a
    /// number or out of range is left alone.
    pub fn from_query(search: &str) -> Self {
        let read = |key: &str| {
            search
                .trim_start_matches('?')
                .split('&')
                .filter_map(|kv| kv.split_once('='))
                .find(|(k, _)| *k == key)
                .and_then(|(_, v)| v.parse::<f64>().ok())
                .filter(|v| v.is_finite())
        };
        let unit = |v: Option<f64>| v.filter(|v| (0.0..=1.0).contains(v));
        Self {
            time: unit(read("time")),
            moon: unit(read("moon")),
            speed: read("timespeed")
                .filter(|v| *v > 0.0 && *v <= 10_000.0)
                .unwrap_or(1.0),
        }
    }
}

pub type SceneRef<S> = Rc<RefCell<S>>;

/// The streamer's and the LOD system's view of the scene.
pub struct View<S>(pub SceneRef<S>);

impl<S: Scene> TerrainView<ChunkMeshes> for View<S> {
    fn set_terrain_chunk(&mut self, coord: Coord, _origin: [i32; 3], meshes: Option<ChunkMeshes>) {
        self.0
            .borrow_mut()
            .set_chunk(ChunkCoord::from_array(coord), meshes.as_ref());
    }
}

impl<S: Scene> LodView<SectionMeshes> for View<S> {
    fn set_lod_section(
        &mut self,
        id: i64,
        origin: [f64; 3],
        cell_size: f64,
        meshes: Option<SectionMeshes>,
    ) {
        self.0
            .borrow_mut()
            .set_section(id, origin, cell_size, meshes.as_ref());
    }
    fn show_lod_sections(&mut self, visible: &[(i64, u8, Fade)]) {
        self.0.borrow_mut().show_sections(visible);
    }
    fn show_lod_stand_ins(&mut self, stand_ins: &[StandInBox]) {
        let boxes: Vec<StandIn> = stand_ins
            .iter()
            .map(|b| StandIn {
                id: b.id,
                lo: b.lo,
                hi: b.hi,
                fade: b.fade,
            })
            .collect();
        self.0.borrow_mut().show_stand_ins(&boxes);
    }
}

/// Messages for the session: reliable control messages and gameplay datagrams.
#[derive(Clone, Debug, PartialEq)]
pub enum Send {
    Control(Message),
    Datagram(Message),
    /// An edit (`BlockEditRequest`, `RegionEdit`) with its prediction id: the session reports it
    /// done when the server's `EditAck` covers it (`Session::send_edit`, `Game::on_edit_results`).
    Edit(Message, u32),
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
    pub face: FaceFrame,
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
    /// The most one step-up lifts the player (a ramp's rise to the step included).
    pub max_step_lift: f64,
    /// The camera's virtual ramp over steps (`step_view`).
    pub step_ramp: StepRamp,
    pub radius: f64,
    pub standing_height: f64,
}

/// The tick's controller state and input, for the telemetry's trace.
fn record_tick(c: &ClientState, input: &InputFrame, eye: f64) {
    use dwell_protocol::input_buttons;
    let flag = |f: u8| c.controller_flags & f != 0;
    let sample = TickSample {
        seq: input.seq,
        at_ms: telemetry::now_ms(),
        feet: c.feet(),
        velocity: c.velocity,
        eye,
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
            face: c.face,
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
            max_step_lift: f64::from(config.max_step_lift()),
            step_ramp: config.step_view.into(),
            radius: f64::from(config.body.radius),
            standing_height: f64::from(config.body.standing_height),
        }
    }

    /// The feet: half a body below the capsule's centre along the face's up.
    fn feet(&self) -> [f64; 3] {
        let up = self.face.up();
        std::array::from_fn(|k| self.position[k] - up[k] * self.half_height)
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
            // The eye smooths every lift a step-up makes.
            max_step_height: self.max_step_lift,
            ramp: self.step_ramp,
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
    /// The bodies drawn for remote players: their roles now and lately (docs/CHARACTERS.md §6).
    pub bodies: Vec<crate::bodies::BodyState>,
    /// Your arms' base role and swing.
    pub arms: (&'static str, Option<&'static str>),
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
    /// The face the eye is over.
    fn face(&self) -> FaceFrame {
        FaceFrame::at_height(self.eye[1])
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
    /// The server's clock, from its snapshots: world time for the day/night cycle and the clouds'
    /// drift.
    pub clock: crate::world_clock::WorldClock,
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
    /// Box select, fill, copy/paste, undo (§6.5); `enabled` once the server announces region edits.
    pub region: RegionTools,
    /// VoxelSniper-style brushes (BRUSHES.md); `available` where the server announces them.
    pub brush: BrushTool,
    /// Every edit shown at once and reconciled with the server's acks (§6.5).
    pub prediction: EditPredictor,
    /// Each drawn character's eased flood light (`eased_light`), by player id; your arms'
    /// under `ARMS_LIGHT`.
    character_lights: std::collections::HashMap<u32, dwell_world::light::EasedLight>,
    /// The characters' animators: the remote players' bodies and your arms (bodies.rs).
    bodies: Bodies,
    /// What you hold as last sent to the server (`Message::HeldItem`, protocol v17).
    held_sent: Option<u16>,
    /// The Show your arms setting (the settings menu; on by default).
    pub show_arms: bool,
    /// Stand-in players for the dev tools (`__dwell.puppets`): the frame cost of N bodies and
    /// their clips without N clients.
    pub puppets: Option<Puppets>,
    /// The eye the crosshair targets from (None while not playing).
    target_eye: Option<[f64; 3]>,
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
    /// The session's id translation (ADR 0038): the worldgen check hashes server ids.
    registry: Option<dwell_blocks::sync::Mapping>,
    pending_lod: Option<LodSettings>,
    /// The latest F4 map (None until one arrives; an empty map without a terrain).
    map: Option<Vec<u8>>,
    /// A placed camera, if a tool set one.
    view: Option<ViewOverride>,
    /// Sound events since the shell last took them (docs/AUDIO.md §5): a side channel.
    sounds: Vec<SoundEvent>,
    mover_sounds: MoverSounds,
    remote_sounds: dwell_world::hash::HashMap<u16, MoverSounds>,
    edit_sounds: EditSounds,
    /// The camera at the last draw: eye, forward, up (the audio listener).
    listener: [f64; 9],
    /// The motion effects (docs/MOTION_EFFECTS.md): read the camera's state, never write it.
    motion: MotionFx,
    /// The player's choices for them (the settings menu).
    pub motion_settings: MotionSettings,
    /// The game menu is open: the effects hold still.
    pub menu_open: bool,
    /// The Field of view setting (degrees, vertical; `dwell_lod::base_fov` narrows it on wide
    /// screens): the base of the projection, the LOD's pixel error and the culling.
    pub fov_setting: f64,
    /// The controller's walk and run speeds the effects scale to.
    speeds: Speeds,
    /// The face the last drawn frame's camera was on (a change is the turn-over).
    motion_face: Option<FaceFrame>,
    /// The vertical FOV of the last drawn frame, given to the renderer and the LOD alike.
    frame_fov: f64,
    /// The spyglass (docs/SPYGLASS.md): its zoom, and the last frame's view of it.
    pub spyglass: Spyglass,
    spyglass_view: SpyglassView,
    /// The browser asks for reduced motion: the spyglass zooms at once.
    pub reduced_motion: bool,
    /// The camera the LOD selection last used.
    lod_camera: Option<LodCamera>,
    /// The dev switches over the day/night cycle, and its state at the last frame (world time is
    /// `clock`'s).
    pub day_override: DayOverride,
    day: DayState,
    first_draw_ms: Option<f64>,
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
        // The client keeps the flood light of what it holds (docs/DAY_NIGHT.md §11).
        let mut world = VoxelWorld::streamed();
        world.enable_light();
        let streamer = ChunkStreamer::with_resync(
            world,
            ChunkJobs(pool.clone()),
            MeshJobs(pool.clone()),
            View(scene.clone()),
            move |coords: &[Coord]| {
                resync_out
                    .borrow_mut()
                    .push(Send::Control(Message::ChunkResync(coords.to_vec())));
            },
        );
        let speeds = Speeds {
            walk: f64::from(config.movement.walk_speed),
            run: f64::from(config.movement.run_speed),
        };
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
            clock: Default::default(),
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
            region: RegionTools::new(),
            brush: BrushTool::new(),
            prediction: EditPredictor::new(),
            character_lights: Default::default(),
            target_eye: None,
            lod: None,
            viewport_css: (1280.0, 720.0),
            debug_note: String::new(),
            memory_note: String::new(),
            hud: Hud::default(),
            debug_visible: false,
            rtt_ms: None,
            verified: None,
            registry: None,
            pending_lod: None,
            map: None,
            view: None,
            sounds: Vec::new(),
            mover_sounds: MoverSounds::new(),
            remote_sounds: Default::default(),
            edit_sounds: EditSounds::new(),
            listener: [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0],
            motion: MotionFx::new(),
            motion_settings: MotionSettings::default(),
            menu_open: false,
            fov_setting: dwell_lod::frustum::VERTICAL_FOV,
            speeds,
            motion_face: None,
            frame_fov: dwell_lod::vertical_fov(1280.0 / 720.0),
            spyglass: Spyglass::new(),
            spyglass_view: SpyglassView::NONE,
            reduced_motion: false,
            lod_camera: None,
            day_override: DayOverride::default(),
            day: DayState::default(),
            first_draw_ms: None,
            bodies: Bodies::default(),
            held_sent: None,
            show_arms: true,
            puppets: None,
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
    /// The vertical FOV the last frame drew with (and the LOD selected with).
    pub fn frame_fov(&self) -> f64 {
        self.frame_fov
    }
    /// The spyglass's view at the last frame (its magnification, the LOD's detail, the vignette).
    pub fn spyglass_view(&self) -> SpyglassView {
        self.spyglass_view
    }
    /// The camera the LOD selection last used (tests: it has the frame's FOV).
    pub fn lod_camera(&self) -> Option<LodCamera> {
        self.lod_camera
    }

    /// Turns the whole-world view on once the verification is known (`?lod=0` leaves it off).
    pub fn enable_lod(&mut self, settings: LodSettings) {
        self.pending_lod = Some(settings);
        self.start_lod();
    }

    /// The session's registry mapping (ADR 0038), right after joining: the job workers learn the
    /// states synced from the server (they are separate WASM instances with tables of their own),
    /// and the worldgen check hashes the server's ids.
    pub fn set_registry(&mut self, mapping: Option<dwell_blocks::sync::Mapping>) {
        if dwell_blocks::state_count() > dwell_blocks::STATES.len() {
            self.pool
                .borrow_mut()
                .broadcast(crate::jobs::Job::Registry {
                    payload: dwell_blocks::sync::extension_payload(),
                });
        }
        self.registry = mapping;
    }

    /// The registry differences the session found (the F3 overlay).
    pub fn registry_summary(&self) -> Option<String> {
        self.registry
            .as_ref()
            .filter(|m| !m.differences.is_empty())
            .map(|m| m.differences.summary())
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
        // Smooth swaps between levels (shared/look/lodblend.json): the cross-fade and the
        // selection's hysteresis.
        options.fade_ms = LOD_BLEND.fade_s * 1000.0;
        options.coarsen_ratio = LOD_BLEND.coarsen_ratio;
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
        self.chunk_fade(c).is_some()
    }
    /// How the renderer draws a chunk: in full, cross-fading with its level-1 section (dwell-lod
    /// fade.rs), or (None) not at all.
    pub fn chunk_fade(&self, c: ChunkCoord) -> Option<Fade> {
        self.lod
            .as_ref()
            .map_or(Some(Fade::Full), |l| l.chunk_fade(c))
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
    /// The client's states in a box, in layer order; empty where a chunk of it isn't loaded (Air
    /// chunks are loaded; the streamer's `BoxSource`): saving a structure.
    pub fn voxels(&mut self, min: [i32; 3], size: [u32; 3]) -> Vec<u16> {
        use dwell_game::region_tools::BoxSource;
        self.streamer
            .read_box(min, size.map(|n| n as i32))
            .unwrap_or_default()
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
            Message::ChunkData(d) => {
                // The server's data for the chunk: predictions in it are its business now.
                self.prediction.forget_chunk(d.coord);
                self.streamer.on_chunk_data(ChunkMessage::from(d))
            }
            Message::ChunkUnload(u) => {
                for &c in &u.coords {
                    self.prediction.forget_chunk(c);
                }
                self.streamer.on_chunk_unload(&u.coords)
            }
            Message::VoxelModification(mut v) => {
                // Cells showing a prediction keep it until their edits' ack (§6.5).
                self.prediction.filter(&mut v.chunks);
                if v.reason == dwell_protocol::VoxelModificationReason::Edit {
                    let changes = self.cell_changes(&v.chunks);
                    self.edit_sounds.changes(&changes, now_ms, &mut self.sounds);
                }
                self.streamer.on_voxel_modification(&v.chunks)
            }
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
                self.clock.observe(s.server_tick, now_ms);
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
                        r.face(),
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
                    // Placed (joined or respawned): the camera starts where the player is, not
                    // interpolated from the state before, whose eye was elsewhere (the default
                    // state's at the origin, inside the ground) and aimed the crosshair from there.
                    self.previous = None;
                    self.eye = EyeCamera::new();
                    self.eye
                        .tick(&self.current.eye_state(), 1.0 / SIM_HZ as f64);
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

    /// The flood light a character takes at world point `at` (DAY_NIGHT.md §11): blended
    /// between the cells around it and eased over time per character (`characters.easeS`); open
    /// sky without a flood light.
    fn eased_light(&mut self, id: u32, at: [f64; 3], dt: f64) -> [f32; 2] {
        let target = self
            .streamer
            .store()
            .light()
            .map_or([1.0, 0.0], |l| l.sample(at));
        self.character_lights.entry(id).or_default().update(
            target,
            dt,
            dwell_world::light::LIGHT.character_ease_s,
        )
    }

    /// [`Self::eased_light`] at a body's middle, a metre over its feet toward its face's up.
    fn body_light(&mut self, id: u32, feet: [f64; 3], face: FaceFrame, dt: f64) -> [f32; 2] {
        let up = face.up();
        self.eased_light(id, std::array::from_fn(|k| feet[k] + up[k]), dt)
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
                (Purpose::Verify, Output::Chunk { hash, voxels }) => {
                    let hash = self
                        .registry
                        .as_ref()
                        .map_or(hash, |m| m.verification_hash(&voxels, hash));
                    self.verify(hash)
                }
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
            feet: c.feet(),
            health: self.health,
            dead: self.dead,
            remotes: self
                .remotes
                .latest()
                .iter()
                .map(|r| (r.player_id, r.feet, r.is_dead()))
                .collect(),
            bodies: self.bodies.roles(),
            arms: self.bodies.arms_state(),
            stats: c.stats,
            terrain: self.streamer.stats(),
            terrain_ready: self.terrain_ready(),
            target: self.interaction.target(),
            lod: self.lod.as_ref().map(LodSystem::debug_stats),
            flying: c.state == PlayerState::Flying,
        }
    }

    /// Breaks or places at the crosshair (§6.5): only while alive and playing. Shown and sounded at
    /// once (edit prediction); nothing targeted sounds a refusal.
    pub fn edit(&mut self, action: EditAction, now_ms: f64) -> bool {
        if self.dead || !self.terrain_ready() {
            return false;
        }
        // The brush tool: left click its arrow, right click its gunpowder (BRUSHES.md).
        if self.brush.active() {
            return self.brush_apply(action == EditAction::Place, now_ms);
        }
        // In paste mode the buttons paste (right) or leave paste mode (left).
        if self.region.pasting() {
            let tool = match action {
                EditAction::Place => ToolAction::Paste,
                EditAction::Break => ToolAction::PasteMode,
            };
            return self.tool(tool, now_ms);
        }
        let Some(target) = self.interaction.target() else {
            self.sounds.push(sound_event(SoundKind::Deny));
            return false;
        };
        let placement = self.interaction.placement();
        match self.interaction.act(action, now_ms) {
            Some(m) => {
                // The sound first: the prediction changes the block at once.
                match (action, placement) {
                    (EditAction::Break, _) => {
                        let [x, y, z] = target.cell;
                        let block = self.voxel(x, y, z);
                        self.edit_sounds
                            .local_break(target.cell, block, now_ms, &mut self.sounds);
                    }
                    (EditAction::Place, Some((cell, block))) => {
                        self.edit_sounds
                            .local_place(cell, block, now_ms, &mut self.sounds);
                    }
                    (EditAction::Place, None) => {}
                }
                self.bodies.swing_arms(match action {
                    EditAction::Break => dwell_characters::animator::Swing::Break,
                    EditAction::Place => dwell_characters::animator::Swing::Place,
                });
                self.send_edit(m, None);
                true
            }
            None => false,
        }
    }

    /// What the server announced it takes (Welcome flags) in a world of this generator: region
    /// edits, and brush strokes with the world type's brush reach (BRUSHES.md).
    pub fn set_capabilities(&mut self, region_edits: bool, brushes: bool, generator_version: u32) {
        let policy = dwell_world::brush::brush_policy(generator_version).filter(|_| brushes);
        self.region.enabled = region_edits;
        self.region.keep = brushes;
        self.region.reach = policy.map_or(dwell_protocol::region_edit::REACH, |p| {
            p.reach.max(dwell_protocol::region_edit::REACH)
        });
        self.brush.available = region_edits && policy.is_some();
        self.brush.reach = self.region.reach;
    }

    /// A brush tool key (BRUSHES.md).
    pub fn brush_key(&mut self, action: BrushAction) {
        // The region tools' click, as for their keys (AUDIO.md).
        self.sounds.push(sound_event(SoundKind::Tool));
        self.brush.key(action);
        if !self.brush.active() {
            self.scene.borrow_mut().set_brush_outline(None);
        }
    }

    /// The wheel with Alt held while the brush tool is on: its size. True if it took the step.
    pub fn brush_wheel(&mut self, step: i32) -> bool {
        if !self.brush.active() {
            return false;
        }
        self.brush.resize(-step);
        self.sounds.push(sound_event(SoundKind::Tool));
        true
    }

    /// Middle or Alt click with the brush tool on: the aimed-at block becomes the replace target.
    pub fn brush_pick(&mut self) -> bool {
        if !self.brush.active() {
            return false;
        }
        let state = self.brush.target().map(|t| {
            self.streamer
                .store_mut()
                .get_voxel(t.cell[0], t.cell[1], t.cell[2])
        });
        self.brush.pick(state);
        self.sounds.push(sound_event(SoundKind::Tool));
        true
    }

    /// Applies the brush at its target with the hotbar's block: arrow (the aimed-at block) or
    /// gunpowder (the cell in front of it). True if it queued a stroke.
    pub fn brush_apply(&mut self, gunpowder: bool, now_ms: f64) -> bool {
        if self.dead || !self.terrain_ready() {
            return false;
        }
        let target = self.brush.target().map(|t| t.cell);
        let ctx = ToolContext {
            target,
            target_state: target
                .map_or(0, |c| self.streamer.store_mut().get_voxel(c[0], c[1], c[2])),
            placement: self.brush.center(true),
            material: self.selected_material(),
            eye: self.target_eye,
            now_ms,
        };
        let sent = self
            .brush
            .apply(gunpowder, &ctx, &mut self.region, &mut self.streamer);
        // The region tools' click as a stroke goes out; its blocks sound as they arrive.
        if sent {
            self.sounds.push(sound_event(SoundKind::Tool));
            self.bodies
                .swing_arms(dwell_characters::animator::Swing::Place);
        }
        // The stroke shows at once; its first edits go out now, the rest as the rate allows (each
        // frame).
        self.predict_queued();
        self.pump_region(now_ms);
        sent
    }

    /// Sends an edit, shown at once (edit prediction, §6.5): `id` when it was predicted already
    /// (a queued region edit), else predicted now.
    fn send_edit(&mut self, m: Message, id: Option<u32>) {
        let id = id.unwrap_or_else(|| {
            let id = self.prediction.next_id();
            if let Some(changes) = self.predicted_changes(&m) {
                self.prediction.predict(id, &changes, &mut self.streamer);
            }
            id
        });
        self.outbox.borrow_mut().push(Send::Edit(m, id));
    }

    /// Queued region edits (a brush stroke, the undo of one) shown at once, before they are sent.
    fn predict_queued(&mut self) {
        for (i, edit) in self.region.untagged() {
            let id = self.prediction.next_id();
            if let Some(changes) = self.predicted_changes(&Message::RegionEdit(edit)) {
                self.prediction.predict(id, &changes, &mut self.streamer);
            }
            self.region.tag(i, id);
        }
    }

    /// The queued region edits the server's rate allows now.
    fn pump_region(&mut self, now_ms: f64) {
        for (m, id) in self.region.pump_tagged(now_ms) {
            self.send_edit(m, id);
        }
    }

    /// What an edit will do, by the server's own checks run on the client's world (from the eye
    /// the crosshair targets from, without the other players' bodies): `None` when it would be
    /// refused or its box isn't loaded. The server's later changes (blocks left unsupported) come
    /// with its modification.
    fn predicted_changes(&mut self, m: &Message) -> Option<Vec<([i32; 3], StateId)>> {
        use dwell_game::region_tools::BoxSource;
        let eye = self.target_eye?;
        match m {
            Message::BlockEditRequest(e) => {
                let world = self.streamer.store_mut();
                let out = dwell_world::edit::check_block_edit(
                    world,
                    e,
                    eye,
                    &[],
                    dwell_world::edit::DEFAULT_REACH,
                );
                // With a multi-cell block's other cells (a torch's upper part), as the server
                // writes them.
                (out.check == dwell_world::edit::EditCheck::Ok).then(|| {
                    dwell_world::edit::edit_writes(
                        world,
                        out.cell,
                        dwell_world::edit::medium_kept(out.material),
                    )
                })
            }
            Message::RegionEdit(e) => {
                // Every chunk of the box loaded (Air ones too), so the client's world reads it right.
                self.streamer.read_box(e.min, e.size.map(i32::from))?;
                let reach = self.region.reach + 1.0;
                let out = dwell_world::region::check_region_edit(
                    self.streamer.store_mut(),
                    e,
                    eye,
                    &[],
                    reach,
                );
                (out.check == dwell_world::region::RegionCheck::Ok).then_some(out.changes)
            }
            _ => None,
        }
    }

    /// The server is done with these edits (`(id, refused)`: from its `EditAck` through the session,
    /// or refused unsent): their cells show the server's state again (§6.5).
    pub fn on_edit_results(&mut self, results: &[(u32, bool)]) {
        if !results.is_empty() {
            self.prediction.resolve(results, &mut self.streamer);
        }
    }

    fn selected_material(&self) -> StateId {
        self.interaction
            .palette()
            .get(self.interaction.selected())
            .map_or(0, |s| s.material)
    }

    /// The brush tool's HUD panel (none while it is off); its note shows a stroke's progress.
    pub fn brush_panel(&self) -> Option<BrushPanel> {
        let mut p = self.brush.panel(self.selected_material())?;
        if let Some((sent, all)) = self.region.progress() {
            p.note = format!("Sending {sent} of {all} edits");
        }
        Some(p)
    }

    /// A sound of the shell's or a debug tool's own (e.g. compare mode's keys), heard with the
    /// game's events.
    pub fn sound(&mut self, kind: SoundKind) {
        self.sounds.push(sound_event(kind));
    }

    /// The sound events since the last call (docs/AUDIO.md §5).
    pub fn take_sounds(&mut self) -> Vec<SoundEvent> {
        std::mem::take(&mut self.sounds)
    }

    /// The camera at the last draw, for the audio listener: eye, forward, up.
    pub fn listener(&self) -> [f64; 9] {
        self.listener
    }

    /// A modification's changes with what each cell held before (read before it is applied).
    fn cell_changes(&mut self, chunks: &[dwell_protocol::ChunkChanges]) -> Vec<CellChange> {
        let n = dwell_world::CHUNK_SIZE;
        let mut out = Vec::new();
        for c in chunks {
            let [cx, cy, cz] = c.coord;
            for ch in &c.changes {
                let i = i32::from(ch.index);
                let cell = [
                    cx * n + (i & 31),
                    cy * n + ((i >> 5) & 31),
                    cz * n + ((i >> 10) & 31),
                ];
                let old = self.voxel(cell[0], cell[1], cell[2]);
                out.push(CellChange {
                    cell,
                    old,
                    new: ch.material,
                });
            }
        }
        out
    }

    /// What the movement sounds read of a body with its feet at `feet`.
    #[allow(clippy::too_many_arguments)]
    fn mover_sample(
        &mut self,
        feet: [f64; 3],
        velocity: [f64; 3],
        state: PlayerState,
        grounded: bool,
        crouched: bool,
        face: FaceFrame,
        submerged: Option<f64>,
        dt: f64,
    ) -> MoverSample {
        // The cell dy metres above the feet (toward the face's up; rows are `CELL_H` tall).
        let at =
            |dy: f64| dwell_world::cell_of_pos([feet[0], face.y(face.y(feet[1]) + dy), feet[2]]);
        let [gx, gy, gz] = at(-0.2);
        let [bx, by, bz] = at(0.5);
        let ground = self.voxel(gx, gy, gz);
        let body = self.voxel(bx, by, bz);
        let liquid = |id: u16| id != 0 && dwell_blocks::info(id).liquid;
        MoverSample {
            feet,
            velocity,
            state,
            grounded,
            crouched,
            face,
            submerged: submerged.unwrap_or(if liquid(body) { 1.0 } else { 0.0 }),
            ground,
            body,
            dt,
        }
    }

    /// Runs a region tool (§6.5) on the crosshair's target and the hotbar's block: only while alive
    /// and playing. True if it sent a region edit.
    pub fn tool(&mut self, action: ToolAction, now_ms: f64) -> bool {
        self.tool_at(action, None, now_ms)
    }

    /// [`Self::tool`] as if aimed at world cell `at` (the bench's `region` step, DEV_TOOLS.md;
    /// None: the crosshair's target).
    pub fn tool_at(&mut self, action: ToolAction, at: Option<[i32; 3]>, now_ms: f64) -> bool {
        if self.dead || !self.terrain_ready() {
            return false;
        }
        // A click as the tool takes the key; the blocks an edit changes sound as they arrive.
        self.sounds.push(sound_event(SoundKind::Tool));
        let target = at.or_else(|| self.interaction.target().map(|t| t.cell));
        let target_state =
            target.map_or(0, |c| self.streamer.store_mut().get_voxel(c[0], c[1], c[2]));
        let material = self
            .interaction
            .palette()
            .get(self.interaction.selected())
            .map_or(0, |s| s.material);
        let ctx = ToolContext {
            target,
            target_state,
            placement: self.interaction.placement().map(|(c, _)| c),
            material,
            eye: self.target_eye,
            now_ms,
        };
        // Through the streamer: it knows the Air chunks the client never stores.
        let sent = match self.region.act(action, &ctx, &mut self.streamer) {
            Some(m) => {
                self.send_edit(m, None);
                true
            }
            None => false,
        };
        // An undo of a stroke queues its edits: shown at once too.
        self.predict_queued();
        self.show_region_boxes();
        sent
    }

    /// The region tools' boxes to the renderer.
    fn show_region_boxes(&self) {
        let boxes: Vec<RegionBox> = self
            .region
            .boxes()
            .iter()
            .map(|b| RegionBox {
                min: dwell_world::pos_of_cell(b.min),
                size: std::array::from_fn(|i| f64::from(b.size[i]) * dwell_world::CELL[i]),
                color: match b.kind {
                    ToolBoxKind::Selection => REGION_SELECTION_COLOR,
                    ToolBoxKind::Ghost => REGION_GHOST_COLOR,
                },
            })
            .collect();
        self.scene.borrow_mut().set_region_boxes(&boxes);
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
        input.brushes = self.brush.available;
        input.brush_active = self.brush.active();
        // Queued region edits (a brush stroke) as the server's cell rate allows.
        self.pump_region(now_ms);
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
        let c = self.current;
        let sample = self.mover_sample(
            c.feet(),
            c.velocity,
            c.state,
            c.controller_flags & controller_flags::GROUNDED != 0,
            c.controller_flags & controller_flags::CROUCHING != 0,
            c.face,
            Some(c.submerged),
            1.0 / SIM_HZ as f64,
        );
        self.mover_sounds.update(&sample, None, &mut self.sounds);
        // Crossing the midplane turns the heading round (a half somersault, face_flip.rs).
        let turn = self.flip.tick(self.current.face);
        if turn != 0.0 {
            input.look.yaw = (input.look.yaw + turn) % 360.0;
        }
        self.eye
            .tick(&self.current.eye_state(), 1.0 / SIM_HZ as f64);
        record_tick(&self.current, &frame, self.eye.draw(1.0));
        input.look.face = self.current.face;
        // The camera turns with rotating ground (PPC yawDelta).
        input.look.yaw += self.current.platform_yaw_delta;
    }

    fn draw(&mut self, now_ms: f64, input: &KeyboardMouseInput, now: &mut dyn FnMut() -> f64) {
        let c = self.current;
        let face = c.face;
        let dt = self.last_draw_ms.map_or(0.0, |l| (now_ms - l) / 1000.0);
        let flip = self.flip.draw(face, dt);
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
                stream_budget_ms(dt * 1000.0),
                stream_budget_ms(dt * 1000.0),
                &mut *now,
            );
        });

        // Remote players, interpolated.
        let views = self.remotes.sample(now_ms);
        let mut drawn = HashSet::default();
        for v in &views {
            drawn.insert(v.player_id);
            if !v.is_dead() {
                let grounded = matches!(
                    v.state,
                    PlayerState::Idle
                        | PlayerState::Walking
                        | PlayerState::Running
                        | PlayerState::Crouching
                        | PlayerState::Sliding
                );
                let face = v.face();
                let sample = self.mover_sample(
                    v.feet,
                    v.velocity.map(f64::from),
                    v.state,
                    grounded,
                    v.is_crouched(),
                    face,
                    None,
                    dt,
                );
                self.remote_sounds
                    .entry(v.player_id)
                    .or_insert_with(|| MoverSounds::with_voice(v.player_id))
                    .update(&sample, Some(v.feet), &mut self.sounds);
            }
            // The full body, animated from the snapshot's controller state (bodies.rs).
            let registry = &self.registry;
            let held = crate::bodies::held_from_wire(v.held, |b| match registry {
                Some(m) => m.client(b),
                None => Some(b),
            });
            let world = self.streamer.store_mut();
            let reach = self.current.max_step_lift;
            let mut view = self.bodies.remote(v, held, dt, &mut |f, p| {
                crate::bodies::ground_at(world, f, p, reach)
            });
            view.light = self.body_light(u32::from(v.player_id), view.feet, view.face, dt);
            self.scene
                .borrow_mut()
                .set_player(u32::from(v.player_id), Some(view));
        }
        for id in self.remotes.ids() {
            if !drawn.contains(&id) {
                self.scene.borrow_mut().set_player(u32::from(id), None);
            }
        }
        if let Some(mut p) = self.puppets {
            if p.held == Puppets::HOTBAR {
                p.held = self.selected_material();
            }
            for v in p.views(now_ms / 1000.0) {
                drawn.insert(v.player_id);
                let held = crate::bodies::held_from_wire(v.held, Some);
                let world = self.streamer.store_mut();
                let reach = self.current.max_step_lift;
                let mut view = self.bodies.remote(&v, held, dt, &mut |f, p| {
                    crate::bodies::ground_at(world, f, p, reach)
                });
                view.light = self.body_light(u32::from(v.player_id), view.feet, view.face, dt);
                self.scene
                    .borrow_mut()
                    .set_player(u32::from(v.player_id), Some(view));
            }
        }
        for id in Puppets::ID_BASE..Puppets::ID_BASE + Puppets::MAX {
            if !drawn.contains(&id) {
                self.scene.borrow_mut().set_player(u32::from(id), None);
            }
        }
        self.bodies.retain(|id| drawn.contains(&id));
        self.character_lights.retain(|id, _| {
            *id == ARMS_LIGHT || *id == u32::from(self.player_id) || drawn.contains(&(*id as u16))
        });
        // A player gone from view (or dead) stops sounding: its slide's voice ends.
        let sounds = &mut self.sounds;
        self.remote_sounds.retain(|id, m| {
            let keep = drawn.contains(id);
            if !keep {
                m.finish(None, sounds);
            }
            keep
        });

        let (yaw, pitch) = (input.look.yaw, input.look.pitch);
        let camera_eye;
        let held = self.held();
        self.send_held(held);
        if self.dead {
            // Third person over the body.
            let mut view = self.bodies.own_dead(self.death_feet, yaw, face, dt);
            view.light = self.body_light(u32::from(self.player_id), view.feet, view.face, dt);
            let mut scene = self.scene.borrow_mut();
            scene.set_player(u32::from(self.player_id), Some(view));
            scene.set_arms(None);
            scene.set_own_shadow(None);
            scene.set_own_body(None);
            let r = yaw.to_radians();
            let [x, y, z] = self.death_feet;
            camera_eye = [
                x - r.sin() * 4.0,
                face.y(face.y(y) + 2.5),
                z - r.cos() * 4.0,
            ];
            scene.set_camera(camera_eye, yaw, -25.0, face, 0.0);
            drop(scene);
            self.show_hud_text();
            self.target(None, input, now_ms);
            self.update_motion(c.position, dt);
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
                .set_camera(eye, yaw, pitch, face, flip);
            self.listener = listener(eye, yaw, pitch, face);
            let ready = self.terrain_ready();
            self.target(ready.then_some(eye), input, now_ms);
            self.update_motion(eye, dt);
            // Your arms in their own pass (CHARACTERS.md §5): not while zoomed in the spyglass, in
            // a placed camera, or with the setting off.
            let move_input =
                crate::bodies::local_input(c.state, c.velocity, c.controller_flags, c.face);
            let mut arms = self.bodies.arms(&move_input, held, dt);
            arms.light = self.eased_light(ARMS_LIGHT, eye, dt);
            let shown = self.show_arms
                && c.active
                && self.view.is_none()
                && self.spyglass_view.magnification <= 1.001;
            self.scene.borrow_mut().set_arms(shown.then_some(arms));
            // Your body casts a shadow (drawn into the shadow maps only), first person or not.
            let up = face.up();
            let feet = std::array::from_fn(|k| center[k] - up[k] * c.half_height);
            let own = self.bodies.own(feet, yaw, face, &move_input, held, dt);
            let mut scene = self.scene.borrow_mut();
            scene.set_own_shadow(c.active.then_some(own));
            // And you see it from the neck down, in first person (CHARACTERS.md §5.1): not from a
            // placed camera.
            scene.set_own_body((c.active && self.view.is_none()).then_some(own));
        }
        let camera_eye = match self.view {
            Some(v) => {
                self.scene
                    .borrow_mut()
                    .set_camera(v.eye, v.yaw_deg, v.pitch_deg, v.face(), 0.0);
                self.scene.borrow_mut().set_block_outline(None, FULL_CELL);
                v.eye
            }
            None => camera_eye,
        };
        let store = self.streamer.store_mut();
        let eye_fluid =
            dwell_world::fluid::eye_fluid(camera_eye, |x, y, z| store.get_voxel(x, y, z));
        self.scene.borrow_mut().set_eye_fluid(eye_fluid);
        telemetry::span("lod", || self.update_lod(&c, now_ms, input));
        if let Some(lod) = &self.lod {
            // The cross-fade's extra draws this frame (dwell-lod fade.rs): the bench reads them.
            let f = lod.debug_stats().fades;
            telemetry::add("lod.fading", f.swaps as f64);
            telemetry::add("lod.retiring", f.retiring as f64);
        }
        self.update_day(now_ms);

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
            let scene = self.scene.borrow();
            lines.push(format_render_stats(&scene.stats()));
            drop(scene);
            lines.push(self.day.debug_line());
            if let Some(r) = self.registry_summary() {
                lines.push(r);
            }
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

    /// The day/night cycle at `now_ms`: world time from the server (none before the first
    /// snapshot: the world's start), sped up or replaced by the dev switches.
    fn update_day(&mut self, now_ms: f64) {
        let first = *self.first_draw_ms.get_or_insert(now_ms);
        let o = self.day_override;
        let world_s =
            self.clock.seconds(now_ms).unwrap_or(0.0) + (o.speed - 1.0) * (now_ms - first) / 1000.0;
        self.day = DAY_NIGHT.state(world_s, o.time, o.moon);
        self.scene
            .borrow_mut()
            .set_day(&self.day, (now_ms - first) / 1000.0);
    }

    /// The day/night cycle's state at the last frame.
    pub fn day(&self) -> &DayState {
        &self.day
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

    /// The motion effects for this frame (docs/MOTION_EFFECTS.md): the FOV boost from the camera's
    /// velocity and state, and the wind. They only read the predicted state. The frame's FOV goes to
    /// the renderer and, in `update_lod`, to the LOD selection.
    fn update_motion(&mut self, eye: [f64; 3], dt: f64) {
        let c = &self.current;
        let turned_over = self.motion_face.is_some_and(|f| f != c.face);
        self.motion_face = Some(c.face);
        let out = self.motion.frame(
            &MotionInput {
                eye,
                velocity: c.velocity,
                state: c.state,
                dt,
                first_person: c.active && !self.dead && self.view.is_none(),
                turned_over,
                paused: self.menu_open,
            },
            self.motion_settings,
            self.speeds,
            &MOTION,
        );
        let viewport = self.viewport();
        // The spyglass narrows the frame's FOV, speed boost included (docs/SPYGLASS.md).
        self.spyglass_view = self.spyglass.frame(dt, self.reduced_motion, &SPYGLASS);
        self.frame_fov = zoom_fov(
            effective_fov(viewport.fov_y_deg, out.fov_boost_pct),
            self.spyglass_view.magnification,
        );
        let mut scene = self.scene.borrow_mut();
        scene.set_fov(Some(self.frame_fov));
        scene.set_magnification(self.spyglass_view.magnification);
        scene.set_wind(WindView {
            intensity: out.wind,
            flight: out.flight,
            direction: out.direction,
            time: out.time,
        });
    }

    /// The drawing's viewport at the player's Field of view setting (its base FOV, before the
    /// speed boost).
    fn viewport(&self) -> dwell_lod::LodViewport {
        dwell_lod::lod_viewport(self.fov_setting, self.viewport_css.0, self.viewport_css.1)
    }

    /// The whole-world view around the camera.
    fn update_lod(&mut self, c: &ClientState, now_ms: f64, input: &KeyboardMouseInput) {
        if !c.active || self.lod.is_none() {
            return;
        }
        let viewport = self.viewport();
        let (position, yaw_deg, pitch_deg) = match self.view {
            Some(v) => (v.eye, v.yaw_deg, v.face().pitch(v.pitch_deg)),
            None => (
                if self.dead {
                    self.death_feet
                } else {
                    c.position
                },
                input.look.yaw,
                // The world pitch: a face-B player's up is −y.
                c.face.pitch(input.look.pitch),
            ),
        };
        let camera = LodCamera {
            position,
            yaw_deg,
            pitch_deg,
            // The frame's FOV, speed boost and spyglass included: what the renderer draws with.
            fov_y_deg: self.frame_fov,
            aspect: viewport.aspect,
            height_px: viewport.height_px,
            // Inside the zoomed view the sections refine for its magnification (up to the
            // spyglass's detail boost), outside it for the unzoomed FOV.
            magnification: self.spyglass_view.magnification,
            detail_magnification: self.spyglass_view.detail,
        };
        self.lod_camera = Some(camera);
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
    fn target(&mut self, eye: Option<[f64; 3]>, input: &KeyboardMouseInput, now_ms: f64) {
        let face = self.current.face;
        let t = self.interaction.update(
            self.streamer.store_mut(),
            eye,
            input.look.yaw,
            input.look.pitch,
            face,
        );
        self.target_eye = eye;
        self.region
            .aim(self.interaction.placement().map(|(c, _)| c));
        self.show_region_boxes();
        // The brush aims much further along the same ray (its world's brush reach).
        let t = if self.brush.active() {
            let dir = view_direction(input.look.yaw, input.look.pitch, face).map(|v| v as f32);
            let far = eye.and_then(|e| self.streamer.store_mut().target(e, dir, self.brush.reach));
            self.brush.aim(far);
            far
        } else {
            t
        };
        // The brush's preview: the ghost of the cells a stroke there would change (BRUSHES.md §3).
        let target_state = self.brush.target().map_or(0, |t| {
            self.streamer
                .store_mut()
                .get_voxel(t.cell[0], t.cell[1], t.cell[2])
        });
        let materials = self.brush.materials(self.selected_material(), target_state);
        self.brush
            .update_preview(materials, &mut self.streamer, now_ms);
        self.scene
            .borrow_mut()
            .set_brush_outline(self.brush.outline());
        let Some(t) = t else {
            self.scene.borrow_mut().set_block_outline(None, FULL_CELL);
            return;
        };
        // The outline is the box the block is aimed at: a plant's selection box, else its shape's
        // extent (an adaptive block's derived shape is often not a full cell tall).
        // The box is in fractions of the cell; the outline is in metres (HALF_HEIGHT_GRID.md §2).
        // A multi-cell block (a 1 m torch) is outlined as one, over its cells.
        let (lo, hi) = dwell_world::edit::outline_box(self.streamer.store_mut(), t.cell);
        let h = dwell_world::CELL_H as f32;
        let bounds = ([lo[0], lo[1] * h, lo[2]], [hi[0], hi[1] * h, hi[2]]);
        self.scene
            .borrow_mut()
            .set_block_outline(Some(dwell_world::pos_of_cell(t.cell)), bounds);
    }

    /// What your hand holds: the hotbar's block, or the brush painting with it while the brush tool
    /// is on (an empty slot: an empty hand).
    pub fn held(&self) -> Held {
        let block = self.selected_material();
        if block == 0 {
            Held::Empty
        } else if self.brush.active() {
            Held::Brush(block)
        } else {
            Held::Block(block)
        }
    }

    /// Tells the server what you hold when it changes (`Message::HeldItem`, protocol v17), so the
    /// others see it in their snapshots.
    fn send_held(&mut self, held: Held) {
        let registry = &self.registry;
        let wire = crate::bodies::held_to_wire(held, |b| match registry {
            Some(m) => m.server(b),
            None => Some(b),
        });
        if self.held_sent != Some(wire) {
            self.held_sent = Some(wire);
            self.outbox
                .borrow_mut()
                .push(Send::Control(Message::HeldItem(wire)));
        }
    }

    /// The arms' base role and swing (tests, telemetry).
    pub fn arms_state(&self) -> (&'static str, Option<&'static str>) {
        self.bodies.arms_state()
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
impl<S: Scene> LodView<SectionMeshes> for LodHost<'_, S> {
    fn set_lod_section(
        &mut self,
        id: i64,
        origin: [f64; 3],
        cell_size: f64,
        meshes: Option<SectionMeshes>,
    ) {
        self.view.set_lod_section(id, origin, cell_size, meshes);
    }
    fn show_lod_sections(&mut self, visible: &[(i64, u8, Fade)]) {
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

fn sound_event(kind: SoundKind) -> SoundEvent {
    SoundEvent::new(kind, 0, 1.0, None)
}

/// The audio listener at an eye looking along yaw/pitch on `face`: eye, forward, up.
fn listener(eye: [f64; 3], yaw: f64, pitch: f64, face: FaceFrame) -> [f64; 9] {
    // The camera's axes (its up perpendicular to forward, toward the face's up).
    let b = crate::render::camera::aim(yaw, pitch, face, 0.0);
    let (f, up) = (b.forward(), b.up);
    [
        eye[0], eye[1], eye[2], f[0], f[1], f[2], up[0], up[1], up[2],
    ]
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
        c.face.sign_f64() * (c.half_height + if grounded { c.max_step_height } else { 0.15 });
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
    fn the_stream_budgets_are_4_ms_or_a_tenth_of_a_slow_frame() {
        assert_eq!(stream_budget_ms(16.7), 4.0);
        assert_eq!(stream_budget_ms(0.0), 4.0);
        assert!(
            (stream_budget_ms(200.0) - 20.0).abs() < 1e-9,
            "CI's ~5 fps page"
        );
        assert_eq!(stream_budget_ms(1000.0), 25.0);
    }

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

/// Stand-in players for the dev tools (`__dwell.puppets`, docs/DEV_TOOLS.md): `count` bodies in
/// rows ahead of `origin` (feet), on `face`, each walking a small circle with a clip of its own
/// (the controller's states in turn) and a held block, so the bench can time N bodies and a
/// screenshot can show every clip, on either face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Puppets {
    pub count: u16,
    pub origin: [f64; 3],
    /// The face they stand on.
    pub face: FaceFrame,
    /// A block every puppet holds (0: none).
    pub held: u16,
}

impl Puppets {
    pub const ID_BASE: u16 = 60000;
    pub const MAX: u16 = 256;
    /// `held`: the hotbar's block, whatever it is.
    pub const HOTBAR: u16 = u16::MAX;

    /// The puppets at time `t` (s), as remote snapshots would carry them.
    pub fn views(&self, t: f64) -> Vec<dwell_game::remotes::RemoteView> {
        use dwell_protocol::player_flags as pf;
        let states = [
            (PlayerState::Walking, 2.5, 0u8),
            (PlayerState::Running, 6.0, 0),
            (PlayerState::Sliding, 7.0, pf::CROUCHED),
            (PlayerState::Crouching, 1.2, pf::CROUCHED),
            (PlayerState::Idle, 0.0, 0),
            (PlayerState::Jumping, 2.0, 0),
            (PlayerState::Swimming, 1.5, pf::SWIMMING),
            (PlayerState::Climbing, 0.0, pf::CLIMBING),
        ];
        let n = self.count.min(Self::MAX);
        let cols = 8.0;
        (0..n)
            .map(|i| {
                let (state, speed, flags) = states[usize::from(i) % states.len()];
                let (row, col) = ((f64::from(i) / cols).floor(), f64::from(i) % cols);
                let yaw = (t * 40.0 + f64::from(i) * 45.0) % 360.0;
                let r = yaw.to_radians();
                let up = if state == PlayerState::Jumping {
                    3.0 * (t * 2.0 + f64::from(i)).sin()
                } else {
                    0.0
                };
                let face_b = dwell_physics::net::face_bit(self.face, pf::FACE_B);
                let grounded = if state == PlayerState::Jumping {
                    0
                } else {
                    pf::GROUNDED
                };
                dwell_game::remotes::RemoteView {
                    player_id: Self::ID_BASE + i,
                    feet: [
                        self.origin[0] + (col - cols / 2.0) * 2.5,
                        self.origin[1],
                        self.origin[2] + 4.0 + row * 2.5,
                    ],
                    velocity: [
                        (r.sin() * speed) as f32,
                        (up * self.face.sign_f64()) as f32,
                        (r.cos() * speed) as f32,
                    ],
                    yaw: yaw as f32,
                    pitch: 0.0,
                    state,
                    flags: flags | face_b | grounded,
                    held: self.held,
                    swing: 0,
                    jumped: false,
                }
            })
            .collect()
    }
}
