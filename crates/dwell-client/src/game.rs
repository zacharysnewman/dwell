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
use dwell_game::block_interaction::{BlockInteraction, BlockTarget, view_direction};
use dwell_game::brush_tool::{BrushAction, BrushOutline, BrushPanel, BrushTool};
use dwell_game::chunk_stream::{
    ChunkMessage, ChunkStreamer, Coord, SecondCentre, StreamStats, TerrainView,
};
use dwell_game::edit_prediction::EditPredictor;
use dwell_game::eye::{EyeCamera, EyeState, StepRamp};
use dwell_game::face_flip::FaceFlip;
use dwell_game::gate::prediction_may_run;
use dwell_game::hud_text::{HudTextInput, hud_text};
use dwell_game::input::{EditAction, KeyboardMouseInput, quantize_input};
use dwell_game::loading::{
    LOADING, LoadingGate, LoadingScreen, RESPAWN_PRELOAD_RADIUS_CHUNKS, near_ring_offsets,
};
use dwell_game::region_tools::{RegionTools, ToolAction, ToolBoxKind, ToolContext};
use dwell_game::remotes::RemotePlayers;
use dwell_game::sound::{
    CellChange, EditSounds, FireSounds, FireSource, ImpactSounds, MoverSample, MoverSounds,
    SoundEvent, SoundKind, Surroundings, ambient, player_event_sound,
};
use dwell_game::stream_budget::StreamBudget;
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
use dwell_protocol::chopping::CameraOffset;
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
/// Server input buffer outside [LOW, HIGH] nudges the local tick rate by ±RATE_NUDGE.
const BUFFER_LOW: u8 = 1;
const BUFFER_HIGH: u8 = 4;
const RATE_NUDGE: f64 = 0.02;
/// Prediction holds once the server reports this many of our inputs queued, until it reports no more
/// than `BUFFER_RESUME`: a backlog that size is a server stalled (generating terrain for a fast flyer,
/// docs/BUGS.md 1), not a slow frame's burst of ticks. Predicting on would only pile up inputs the
/// server reaches seconds later; past its backstop (`MAX_BUFFERED_INPUTS`, 60) it skips them, and a
/// skipped input is movement it never simulates, so every later snapshot would snap us back.
const BUFFER_HOLD: u8 = 30;
/// ... and resumes here: lower, and the snapshots' lag lets the queue run dry, when the server
/// repeats our last input (movement we never predicted).
const BUFFER_RESUME: u8 = 6;
/// ... and holds, too, while this many of our inputs are unacknowledged: a stalled server sends no
/// snapshots to report its queue, and a longer burst overflows its queue (96) and input rate limit.
/// It is above a slow frame's whole catch-up (`MAX_TICKS_PER_FRAME`) plus half a second of round
/// trip and snapshot lag: at 45 a frame of 48 ticks (CI's 800 ms frames) held its last ticks every
/// frame, the server repeated our last input for them, and a walk's correction depended on how the
/// frames lined up with the server's ticks (up to 3 ticks of walking, 0.25 m; docs/BUGS.md 58).
const UNACKED_HOLD: u32 = MAX_TICKS_PER_FRAME as u32 + 30;
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
    /// The creatures to draw this frame, each with its flood light (ADR 0073).
    fn set_creatures(&mut self, _creatures: Vec<(crate::creatures::CreatureView, [f32; 2])>) {}
    /// The near water's surface is drawn from the GPU heightfield (its tier's `heightfield`):
    /// chunks are meshed without liquid tops (`dwell_mesh::ChunkFlags`).
    fn draws_heightfield(&self) -> bool {
        false
    }
    /// Leaf cards are drawn (shared/look/leaves.json `cards`; not on a CPU adapter, the cheapest
    /// tier): chunks are meshed with them (`dwell_mesh::ChunkFlags`).
    fn draws_cards(&self) -> bool {
        true
    }
    /// How many bytes of finished chunk meshes a frame uploads (shared/render/streaming.json by
    /// tier; the first mesh of a frame always goes).
    fn upload_budget_bytes(&self) -> u64 {
        u64::MAX
    }
    /// A chunk mesh is still being written a slice a frame (`render/staged.rs`): no more are
    /// handed over until it is done.
    fn uploads_waiting(&self) -> bool {
        false
    }
    /// Terrain collider cells a prediction tick puts in outside the player's reach
    /// (shared/render/streaming.json `collisionCellsPerTick` by tier; BUGS.md #72).
    fn collision_cells_per_tick(&self) -> usize {
        dwell_physics::terrain::COLLISION_CELLS_PER_TICK
    }
    /// Rewrites chunk `coord`'s heightfield tile alone (none: its water is gone), its meshes kept.
    fn set_water_tile(
        &mut self,
        _coord: ChunkCoord,
        _tile: Option<&dwell_mesh::heightfield::WaterTile>,
    ) {
    }
    /// Your first-person arms this frame (docs/CHARACTERS.md §5), or none.
    fn set_arms(&mut self, _arms: Option<ArmsView>) {}
    /// A source's run-time plumes (docs/CLOUDS.md §8.7): every plume whose name starts with
    /// `prefix` replaced by `plumes`.
    fn set_plumes(&mut self, _prefix: &str, _plumes: Vec<(String, dwell_look::plumes::PlumeSpec)>) {
    }
    /// Your own body this frame, cast into the shadow maps only (none while it is drawn).
    fn set_own_shadow(&mut self, _body: Option<PlayerView>) {}
    /// Your own body drawn in first person from the neck down (docs/CHARACTERS.md §5.1), or none.
    fn set_own_body(&mut self, _body: Option<PlayerView>) {}
    /// The placed light sources near the eye this frame (a torch's flame; docs/LIGHTS.md §4):
    /// drawn as shadowed dynamic lights with Torch shadows on.
    fn set_placed_lights(&mut self, _lights: Vec<crate::render::lights::PlacedLight>) {}
    /// The placed light sources' flames near the eye this frame (a torch's; docs/LIGHTS.md §6).
    fn set_placed_flames(&mut self, _flames: Vec<crate::render::flames::PlacedFlame>) {}
    /// The water's currents around the eye (render/flow.rs; `Currents::window`'s texels), when they
    /// changed.
    fn set_water_flow(&mut self, _texels: Vec<i8>) {}
    /// The flecks drifting on moving water this frame (crate::flecks).
    fn set_flecks(&mut self, _flecks: Vec<crate::flecks::Fleck>) {}
    /// The waterfalls near the eye to draw this frame (render/waterfalls.rs).
    fn set_falls(&mut self, _falls: Vec<dwell_world::falls::Fall>) {}
    /// The blasts' puffs and flash lights this frame (crate::blasts; docs/EXPLOSIVES.md §4).
    fn set_blasts(
        &mut self,
        _puffs: Vec<crate::blasts::Puff>,
        _lights: Vec<dwell_look::lights::PointLight>,
    ) {
    }
    /// The portals near the eye whose motes draw this frame (BIFACIAL_WORLD.md §12.1).
    fn set_portal_motes(&mut self, _portals: Vec<crate::render::flames::PortalMotes>) {}
    /// A falling piece's meshes (docs/FELLING.md §3), relative to its reference cell's centre,
    /// reaching that far (m); `None` removes it.
    fn set_piece(&mut self, _id: u32, _meshes: Option<(&[dwell_mesh::ChunkMeshes], f64)>) {}
    /// Where the falling pieces are this frame: id, reference cell's centre, rotation (x, y, z, w).
    fn show_pieces(&mut self, _poses: Vec<(u32, [f64; 3], [f32; 4])>) {}
    fn set_camera(
        &mut self,
        eye: [f64; 3],
        yaw_deg: f64,
        pitch_deg: f64,
        face: FaceFrame,
        roll: f64,
    );
    /// The fluid whose drawn surface the camera's eye is under (`dwell_world::fluid::eye_fluid`).
    fn set_eye_fluid(&mut self, _eye_fluid: Option<dwell_world::fluid::EyeFluid>) {}
    /// The tree being chopped leans (crate::chop `TreeLean`; none: nothing leans).
    fn set_tree_lean(&mut self, _lean: Option<crate::chop::TreeLean>) {}
    /// Outlines the targeted block: its cell's minimum corner and the box in the cell (min, max
    /// corner; `dwell_world::edit::target_box`), or none.
    fn set_block_outline(&mut self, cell: Option<[f64; 3]>, bounds: ([f32; 3], [f32; 3]));
    /// Outlines a cell of a moving body: the box (min, max) about `origin` (the body's reference
    /// cell's centre), turned by `rotation` (a unit quaternion x, y, z, w) about it.
    fn set_block_outline_turned(
        &mut self,
        origin: Option<[f64; 3]>,
        bounds: ([f32; 3], [f32; 3]),
        _rotation: [f32; 4],
    ) {
        self.set_block_outline(origin, bounds);
    }
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
    /// The LOD colour map's rings this frame (ARCHITECTURE.md §6.6 "Colour map"), or none (off).
    fn set_lod_map(&mut self, _map: Option<&crate::lod_map::LodMap>) {}
    /// A strip of ring `ring` rasterised: world texels `rect`, rgba8 sRGB rows of x.
    fn lod_map_tile(&mut self, _ring: usize, _rect: crate::lod_map::Rect, _rgba: &[u8]) {}
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
    fn set_water_levels(
        &mut self,
        coord: Coord,
        padded: &[u16],
        light: &[u8],
        mirror: bool,
    ) -> bool {
        let mut scene = self.0.borrow_mut();
        let coord = ChunkCoord::from_array(coord);
        // A water-only change (levels, or the wet front: W1, proposals/WATER_GPU_SURFACE.md §3)
        // rewrites the tile, making one for the chunk's first water or freeing it on drying: the
        // chunk's shown mesh (current, the streamer checks) was made without the water's tops
        // and sides while the heightfield draws.
        if !scene.draws_heightfield() {
            return false;
        }
        let tile = dwell_mesh::heightfield::water_tile(padded, light, mirror);
        scene.set_water_tile(coord, tile.as_ref());
        true
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
    /// The velocity of the ground carrying the player (world, m/s; the controller's
    /// `base_velocity`): its motion over that ground is `velocity - carry`, what the arms, the
    /// body and the footsteps read.
    pub carry: [f64; 3],
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
    pub crouch_height: f64,
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
            carry: c.face.vector(v3(c.platform.base_velocity)),
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
            crouch_height: f64::from(config.crouch.height),
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
    /// The moving body's cell aimed at (docs/BUILT_BODIES.md §4): body, cell (its grid), face.
    pub body_target: Option<(u32, [i32; 3], u8)>,
    pub lod: Option<dwell_lod::LodStats>,
    pub flying: bool,
    /// The bodies drawn for remote players: their roles now and lately (docs/CHARACTERS.md §6).
    pub bodies: Vec<crate::bodies::BodyState>,
    /// Your arms' base role and swing.
    pub arms: (&'static str, Option<&'static str>),
    /// The loading screen: whether it is on, what it shows now, how its last wait ended.
    pub loading_enabled: bool,
    pub loading: Option<LoadingScreen>,
    pub loading_opened: Option<(dwell_game::loading::Opened, f64)>,
}

/// The HUD's contents after a frame (the TS shell draws them).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hud {
    pub message: String,
    pub status: String,
    pub health: u8,
    /// Stamina and air as shares of full, and exhaustion (ADR 0069): None in a mode without them.
    pub vitals: Option<HudVitals>,
    /// The F3 overlay's text, while it is shown.
    pub debug: Option<String>,
    /// Damage taken since the last frame (the HUD flashes).
    pub damaged: bool,
    /// The burning edge's strength, 0–1 (shared/fluids/lava.json `burnEdge`; VOLCANOES.md §6.3).
    pub burn: f64,
}

/// The burning edge's strength at `now_ms` for a burn that shows until `until_ms`: 1 at a burn
/// hit, fading to 0 over `burnEdge.seconds`.
pub fn burn_edge_strength(until_ms: f64, now_ms: f64) -> f64 {
    let span = f64::from(dwell_lava::DATA.burn_edge.seconds) * 1000.0;
    ((until_ms - now_ms) / span).clamp(0.0, 1.0)
}

/// The HUD's stamina and air bars (ADR 0069): each a share of full, None where the mode has no
/// such rule.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HudVitals {
    pub stamina: Option<f32>,
    pub oxygen: Option<f32>,
    pub exhausted: bool,
}

impl HudVitals {
    pub fn of(
        v: &dwell_protocol::vitals::Vitals,
        rules: dwell_protocol::vitals::VitalsRules,
    ) -> Option<Self> {
        use dwell_protocol::vitals::VITALS;
        rules.any().then(|| HudVitals {
            stamina: rules
                .stamina
                .then(|| (v.stamina / VITALS.max_stamina).clamp(0.0, 1.0)),
            oxygen: rules
                .oxygen
                .then(|| (v.oxygen / VITALS.oxygen_seconds).clamp(0.0, 1.0)),
            exhausted: rules.stamina && v.exhausted,
        })
    }
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

pub mod streaming_report;

pub struct Game<S: Scene> {
    pub player_id: u16,
    /// The server's chunk unloads of the last 30 s (when, chunk, distance from the feet, m): the
    /// streaming report's (`streaming_report.rs`).
    unloads: std::collections::VecDeque<(f64, [i32; 3], f64)>,
    /// The world's portals: their axes (world x, z) and mote look (`set_world`).
    portals: (Vec<[f64; 2]>, &'static str),
    predictor: Predictor,
    streamer: Streamer<S>,
    pool: PoolRef,
    scene: SceneRef<S>,
    outbox: Outbox,
    remotes: RemotePlayers,
    proxies: HashSet<u16>,
    /// The falling pieces (docs/FELLING.md §3).
    pieces: crate::pieces::Pieces,
    /// The inventory and loose items the server sent (ADR 0066), and the hotbar slot held when
    /// the world uses items.
    pub loose: crate::loose_items::LooseItems,
    /// The creatures the server tells of (ADR 0073), and whether it was told this client draws them.
    pub creatures: crate::creatures::ClientCreatures,
    creatures_on: bool,
    /// The far ecology's herds and packs the server last told of (`Ecology`, docs/ECOLOGY.md §9;
    /// `__dwell.ecology()`; for far rendering later), and whether `EcologyHello` was sent.
    pub ecology: Option<dwell_protocol::EcologyMessage>,
    /// The far herds' heads laid out on the ground from the last `ecology` (crate::far_herds),
    /// and whether it is to be laid out again (a new snapshot, or heads whose ground wasn't loaded).
    far_heads: Vec<crate::far_herds::Head>,
    far_stale: bool,
    ecology_on: bool,
    /// The server keeps block damage (`inventory_flags::DAMAGE`): `DamageHello` sent, struck
    /// points sent with breaks.
    damage_on: bool,
    /// The server takes `CausesHello` (`inventory_flags::CAUSES`): said once, so it sends the
    /// damage causes this build knows (`Burn`, …).
    causes_on: bool,
    /// The last `Inventory`'s flags (`inventory_flags`): whether the server takes `Combine`.
    inventory_flags: u8,
    /// Other players' hits seen in the damage updates (docs/BLOCK_DAMAGE.md §6).
    remote_hits: crate::hit_feedback::RemoteHits,
    /// Our tool hits on their way to their swing's contact (WORK_QUEUE item 25).
    pending_hits: crate::hit_feedback::PendingHits,
    /// Foliage brushed through and ground items stepped onto (AUDIO.md §5): ours, and other
    /// players' from their replicated positions (AUDIO.md §2e).
    contact_sounds: dwell_game::sound::ContactSounds,
    remote_contacts: dwell_game::sound::RemoteContacts,
    /// The world seed (Welcome), the ground items' scatter's (ADR 0071).
    world_seed: u64,
    /// The server keeps ground items and was told this client draws them (`GroundHello`).
    ground_on: bool,
    /// The ground item under the crosshair (ADR 0071): E or X picks it up.
    pub ground_target: Option<dwell_world::ground::Placed>,
    /// The held axe's swing and the server's cuts (ADR 0067).
    pub chop: crate::chop::Chopping,
    /// The axe's chips in flight (crate::chips; draw-only).
    pub chips: crate::chips::Chips,
    /// Steam where lava boils water away (crate::steam; the server's `Steam`, VOLCANOES.md §6.3).
    pub steam: crate::steam::Steam,
    /// The steam plumes over the world's lava vents (crate::vent_plumes, docs/CLOUDS.md §8.7).
    pub vent_plumes: crate::vent_plumes::VentPlumes,
    /// When the burning edge has faded (ms; a `Burn` hit sets it).
    burn_until_ms: f64,
    /// Said `SteamHello` (the server has `inventory_flags::STEAM`).
    steam_on: bool,
    /// Asking for the others' swing kinds (`SwingHello`: their chops by direction).
    swing_hello: dwell_game::remotes::SwingHello,
    pub inventory_slot: usize,
    /// The creative hotbar's picks (docs/ITEMS.md §6a): its slots' palette items.
    pub creative: dwell_game::creative_hotbar::CreativeHotbar,
    /// The moving body's cell under the crosshair, when nearer than the block aimed at
    /// (docs/BUILT_BODIES.md §4).
    body_target: Option<crate::pieces::BodyHit>,
    /// When the last body edit went (ms), for the edit interval.
    body_edit_ms: f64,
    /// Settled pieces drawn gliding into their blocks' places (docs/FELLING.md §4.1).
    glides: crate::settle_glide::Glides,
    recent: VecDeque<InputFrame>,
    accumulator: f64,
    last_frame_ms: Option<f64>,
    /// When this frame started, on the budgets' clock: the stream's budget is what is left of
    /// the frame's (`dwell_game::stream_budget`).
    frame_start_ms: f64,
    last_snapshot_tick: u32,
    /// The server's clock, from its snapshots: world time for the day/night cycle and the clouds'
    /// drift.
    pub clock: crate::world_clock::WorldClock,
    input_buffer: u8,
    /// Prediction is holding for the server to drain our queued inputs (`BUFFER_HOLD`).
    holding: bool,
    tick_rate: f64,
    health: u8,
    /// The world's stamina and oxygen rules the prediction runs (ADR 0069).
    vitals_rules: dwell_protocol::vitals::VitalsRules,
    /// The world's mode breaks every block in one hit without a tool (`instantBreaks`,
    /// creative): the break prediction asks it as the server does.
    instant_breaks: bool,
    dead: bool,
    death_feet: [f64; 3],
    /// The streaming probe's last feet and heading (`probe_stream`).
    probe: ([f64; 3], [f64; 2]),
    /// Fixed points the streaming probe also reports (`probe.target`, `probe.control`).
    pub probe_points: Vec<[f64; 3]>,
    previous: Option<ClientState>,
    current: ClientState,
    flip: FaceFlip,
    last_draw_ms: Option<f64>,
    eye: EyeCamera,
    pub interaction: BlockInteraction,
    /// Holding Place builds on in the build mode (docs/ITEMS.md §5).
    pub drag: dwell_game::build_drag::BuildDrag,
    /// The look's unit direction at the last frame (bridging help's aim).
    view_dir: [f64; 3],
    /// Drag fills sent and not yet answered (prediction id → the fill and its tries so far).
    drag_sent: std::collections::HashMap<u32, (dwell_protocol::RegionEdit, u8)>,
    /// When each water chunk's currents last came (docs/WATER_SURFACE_GRID.md §2c): forgotten
    /// `CURRENTS_KEEP_MS` after.
    currents_seen: dwell_world::hash::HashMap<dwell_world::currents::CurrentKey, f64>,
    /// What the renderer's flow window was last built from: the table's revision, the eye's face
    /// and column (`update_flow`).
    flow_sent: Option<(u64, bool, i32, i32)>,
    /// The Drifting flecks setting: particles on moving water (render/flames.rs's flecks).
    flow_particles: bool,
    flecks: crate::flecks::Flecks,
    /// The waterfalls near the eye (docs/WATER_SURFACE_GRID.md §2e; `dwell_world::falls`, found
    /// again every `WATERFALLS.refresh_s`), drawn while the Waterfalls setting is on and heard always.
    falls: Vec<dwell_world::falls::Fall>,
    falls_age: f64,
    /// The Waterfalls setting: the falls' sheets, foam and spray drawn.
    falls_shown: bool,
    /// The blasts going off near us (docs/EXPLOSIVES.md §4): their flash, smoke, dust and shake.
    blasts: crate::blasts::Blasts,
    fall_sounds: dwell_game::sound::FallSounds,
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
    /// The Camera sway setting (Settings → Motion): the swing's sway on the view.
    pub camera_sway: bool,
    /// Stand-in players for the dev tools (`__dwell.puppets`): the frame cost of N bodies and
    /// their clips without N clients.
    pub puppets: Option<Puppets>,
    /// The distant terrain is drawn (a debug switch: `?lod=off`, `__dwell.setLod`; the sections
    /// still stream, so it comes back at once).
    pub lod_drawn: bool,
    /// The eye the crosshair targets from (None while not playing).
    target_eye: Option<[f64; 3]>,
    lod: Option<LodSystem>,
    /// The LOD colour map's rings (`?lodmap=on`), once turned on.
    lod_map: Option<crate::lod_map::LodMap>,
    lod_map_on: bool,
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
    /// Finished chunk meshes waiting for their upload, taken under the frame's byte budget
    /// (`Scene::upload_budget_bytes`, `dwell_game::upload_budget`; BUGS.md #72).
    mesh_uploads: dwell_game::upload_budget::UploadQueue<(
        dwell_game::chunk_stream::Coord,
        u64,
        Box<ChunkMeshes>,
    )>,
    /// The revealed big map's finished tiles, not yet taken (`crate::map_reveal::encode_tile`).
    reveal: std::collections::VecDeque<Vec<u8>>,
    /// The explored cells' generated tiles, not yet taken: (face, ti, tk), RGBA (None: no
    /// terrain to generate from).
    explore_tiles: std::collections::VecDeque<(dwell_game::minimap::TileKey, Option<Vec<u8>>)>,
    /// A placed camera, if a tool set one.
    view: Option<ViewOverride>,
    /// Sound events since the shell last took them (docs/AUDIO.md §5): a side channel.
    sounds: Vec<SoundEvent>,
    mover_sounds: MoverSounds,
    remote_sounds: dwell_world::hash::HashMap<u16, MoverSounds>,
    /// The tools others hold, by player id (`HeldTools`, ADR 0067): its item name and hold.
    remote_tools: dwell_world::hash::HashMap<u16, (&'static str, bool)>,
    /// The ground densities the chunk meshes were scattered with (ADR 0074 B5), by chunk, where
    /// the biomes vary them: the crosshair's rolls are the mesh's.
    ground_density: dwell_world::hash::HashMap<[i32; 3], dwell_world::ground::GroundDensity>,
    /// The tool and hold last told the server (`ChopAction::Held`).
    held_tool_sent: Option<(&'static str, bool)>,
    /// The standing tree the axe last bit, and when (s): it sways as it is cut (`TreeLean`).
    lean_at: Option<([i32; 3], f64)>,
    /// The crackle of torches near the listener, and the wind and the night (docs/AUDIO.md §5).
    fire_sounds: FireSounds,
    /// The body's breathing, gasps and strain from the vitals (docs/AUDIO.md §5).
    vital_sounds: dwell_game::sound::VitalSounds,
    /// The creatures' footsteps, startles and calls (docs/AUDIO.md §2d).
    creature_sounds: dwell_game::sound::CreatureSounds,
    /// The listener's surroundings for the ambience beds (AUDIO.md §2c), read a few times a second.
    ambience: dwell_game::ambience::Listening,
    ambience_due_s: f64,
    ambience_trees: Vec<usize>,
    /// Other players' health events, held until their bodies are drawn at that moment (the
    /// interpolation delay), so a hurt sounds with the hit that caused it: (local ms, event).
    pending_player_events: Vec<(f64, dwell_protocol::PlayerEvent)>,
    /// Other players' hits, read from their snapshots (exact positions) and held likewise.
    remote_impacts: dwell_world::hash::HashMap<u16, ImpactSounds>,
    pending_sounds: Vec<(f64, SoundEvent)>,
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
    /// The world's clock (the server's world settings; this build's until they arrive).
    world_clock: dwell_look::daynight::Clock,
    day: DayState,
    first_draw_ms: Option<f64>,
    /// The loading screen (ARCHITECTURE.md §6.3): off unless `set_loading_screen` turns it on.
    loading: LoadingGate,
    /// Its near ring's chunk offsets, nearest first.
    near_ring: Vec<Coord>,
    /// The Welcome's verification chunk: the cell under the world's spawn, where a respawn is.
    spawn_chunk: Option<ChunkCoord>,
    /// The respawn preload's priorities (§6.3; off with `?respawnPreload=off`).
    respawn_preload_on: bool,
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
        let mut predictor = Predictor::new(config);
        // Its stand-ins slow in water as the server's bodies do (docs/BUILT_BODIES.md §6).
        predictor.set_water_drag(dwell_sim::felling::water_drag_with);
        let current = ClientState::of(&predictor);
        let mut flip = FaceFlip::new();
        flip.reset(current.face); // joining on face B does not turn the player
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
            pieces: crate::pieces::Pieces::default(),
            body_target: None,
            body_edit_ms: f64::NEG_INFINITY,
            glides: crate::settle_glide::Glides::default(),
            recent: VecDeque::new(),
            accumulator: 0.0,
            last_frame_ms: None,
            frame_start_ms: 0.0,
            last_snapshot_tick: 0,
            clock: Default::default(),
            input_buffer: 0,
            holding: false,
            tick_rate: 1.0,
            health: 0,
            vitals_rules: Default::default(),
            instant_breaks: dwell_protocol::modes::default_mode()
                .allows(dwell_protocol::modes::Ability::InstantBreaks),
            dead: false,
            death_feet: [0.0; 3],
            probe: ([0.0; 3], [0.0, 1.0]),
            probe_points: Vec::new(),
            previous: None,
            current,
            flip,
            last_draw_ms: None,
            eye,
            interaction: BlockInteraction::new(),
            drag: dwell_game::build_drag::BuildDrag::new(),
            view_dir: [0.0, 0.0, 1.0],
            drag_sent: std::collections::HashMap::new(),
            currents_seen: Default::default(),
            flow_sent: None,
            flow_particles: true,
            flecks: crate::flecks::Flecks::default(),
            falls: Vec::new(),
            falls_age: f64::INFINITY,
            falls_shown: true,
            blasts: crate::blasts::Blasts::default(),
            fall_sounds: dwell_game::sound::FallSounds::new(),
            region: RegionTools::new(),
            brush: BrushTool::new(),
            portals: (Vec::new(), ""),
            prediction: EditPredictor::new(),
            character_lights: Default::default(),
            target_eye: None,
            lod: None,
            unloads: Default::default(),
            lod_map: None,
            lod_map_on: false,
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
            mesh_uploads: Default::default(),
            reveal: std::collections::VecDeque::new(),
            explore_tiles: std::collections::VecDeque::new(),
            view: None,
            sounds: Vec::new(),
            mover_sounds: MoverSounds::new(),
            remote_sounds: Default::default(),
            remote_tools: Default::default(),
            ground_density: Default::default(),
            held_tool_sent: None,
            lean_at: None,
            fire_sounds: FireSounds::new(),
            vital_sounds: dwell_game::sound::VitalSounds::new(),
            creature_sounds: dwell_game::sound::CreatureSounds::new(0x5eed),
            ambience: dwell_game::ambience::Listening {
                fire_m: dwell_game::ambience::FAR_M,
                water_m: dwell_game::ambience::FAR_M,
                ..Default::default()
            },
            ambience_due_s: 0.0,
            ambience_trees: dwell_game::ambience::tree_blocks(),
            pending_player_events: Vec::new(),
            remote_impacts: Default::default(),
            pending_sounds: Vec::new(),
            edit_sounds: EditSounds::new(),
            listener: [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0],
            motion: MotionFx::new(),
            motion_settings: MotionSettings::default(),
            menu_open: false,
            fov_setting: dwell_lod::frustum::VERTICAL_FOV,
            speeds,
            frame_fov: dwell_lod::vertical_fov(1280.0 / 720.0),
            spyglass: Spyglass::new(),
            spyglass_view: SpyglassView::NONE,
            reduced_motion: false,
            lod_camera: None,
            day_override: DayOverride::default(),
            world_clock: DAY_NIGHT.clock,
            day: DayState::default(),
            first_draw_ms: None,
            loading: LoadingGate::new(LOADING, false),
            near_ring: near_ring_offsets(LOADING.radius_chunks),
            spawn_chunk: verification,
            respawn_preload_on: true,
            bodies: Bodies::default(),
            held_sent: None,
            loose: Default::default(),
            creatures: Default::default(),
            creatures_on: false,
            ecology: None,
            far_heads: Vec::new(),
            far_stale: false,
            ecology_on: false,
            damage_on: false,
            causes_on: false,
            inventory_flags: 0,
            remote_hits: Default::default(),
            pending_hits: Default::default(),
            contact_sounds: Default::default(),
            remote_contacts: Default::default(),
            world_seed: 0,
            ground_on: false,
            ground_target: None,
            chop: Default::default(),
            chips: Default::default(),
            steam: Default::default(),
            vent_plumes: Default::default(),
            burn_until_ms: 0.0,
            steam_on: false,
            swing_hello: dwell_game::remotes::SwingHello::default(),
            inventory_slot: 0,
            creative: dwell_game::creative_hotbar::CreativeHotbar::new(
                &dwell_game::block_interaction::palette(),
            ),
            show_arms: true,
            camera_sway: true,
            lod_drawn: true,
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

    /// How long the camera rolls upright after crossing the midplane (s; 0 snaps): the
    /// experiment switch `?crossingroll=` / `__dwell.setCrossingRoll` (face_flip.rs). False for a
    /// value it refuses.
    pub fn set_crossing_roll(&mut self, seconds: f64) -> bool {
        self.flip.set_seconds(seconds)
    }

    /// The respawn preload's lighting, meshing and LOD priorities (§6.3) on or off.
    pub fn set_respawn_preload(&mut self, on: bool) {
        self.respawn_preload_on = on;
    }

    /// The loading screen on joining and respawning (off for `?loading=off` and the tests).
    pub fn set_loading_screen(&mut self, enabled: bool) {
        self.loading = LoadingGate::new(LOADING, enabled);
    }

    /// The loading screen to show, while the near ring loads.
    pub fn loading_screen(&self) -> Option<LoadingScreen> {
        self.loading.screen()
    }

    /// The player's centre (the prediction's), where the loading screen's near ring is centred.
    pub fn position(&self) -> [f64; 3] {
        self.current.position
    }

    /// The other players' feet (world), their latest snapshots: the minimap's dots (ADR 0068).
    pub fn remote_feet(&self) -> Vec<[f64; 3]> {
        self.remotes.latest().iter().map(|v| v.feet).collect()
    }

    /// The loading gate's state, for the bench and the tests.
    pub fn loading_gate(&self) -> &LoadingGate {
        &self.loading
    }

    /// The respawn preload (§6.3): while dead, the server streams the ring around the respawn
    /// point, and its chunks are lit and meshed right after the near ring around the body. The
    /// respawn is at the world's spawn, whose chunk is predicted from the Welcome's verification
    /// chunk (the cell under it): the ring is widened by one chunk to cover the body's centre
    /// above it and the players' spread around it, and by one more for the LOD: a level-1
    /// section the near ring straddles is drawn as chunks only once all 8 of its chunks are, and
    /// they reach a chunk beyond the ring (in a valley under generator 21's mountains those
    /// sections hold ground, not air, and waited 14 frames for their outer chunks, meshed last).
    fn respawn_preload(&self) -> Option<SecondCentre> {
        let radius = RESPAWN_PRELOAD_RADIUS_CHUNKS.filter(|_| self.respawn_preload_on)?;
        let at = self.spawn_chunk.filter(|_| self.dead)?;
        Some(SecondCentre {
            near: LOADING.radius_chunks,
            at: at.to_array(),
            radius: radius + 2,
        })
    }

    /// Opens the loading gate once the near ring around the player is drawn (in every direction,
    /// wherever the camera looks) or its timeout passes.
    fn update_loading(&mut self, now_ms: f64) {
        if self.loading.is_open() || !self.current.active {
            return;
        }
        let lod = self.lod.as_ref();
        let progress = self
            .streamer
            .near_progress(self.current.position, &self.near_ring, |c| {
                // Drawn as chunks, or where the LOD knows there is nothing to draw (buried rock).
                let c = ChunkCoord::from_array(c);
                lod.is_none_or(|l| l.chunk_visible(c) || l.shows_nothing_at(c))
            });
        self.loading.update(now_ms, progress);
    }

    /// Places the camera (None: back to the player's eyes).
    /// Holds the axe's level or vertical swing at clip time `t` (s) on your arms and every remote
    /// body (puppets included), or releases it (none): the bench's `clip-strip` (DEV_TOOLS.md).
    pub fn hold_swing(&mut self, held: Option<(bool, f64)>) {
        use dwell_characters::animator::Swing;
        // The hand takes that swing's hold (the vertical swing's grip differs), as a toggle would:
        // without it the vertical clip played on the level hold, its head off the crosshair.
        if let Some((vertical, _)) = held {
            self.chop.vertical = vertical;
        }
        self.bodies.hold_swing(held.map(|(vertical, t)| {
            let s = if vertical {
                Swing::ChopVertical
            } else {
                Swing::ChopLevel
            };
            (s, t.max(0.0) as f32)
        }));
    }

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

    /// Turns the LOD colour map on or off (`?lodmap=on`, `__dwell.setLodMap`; ARCHITECTURE.md §6.6
    /// "Colour map"). Its rings are kept while off, so turning it back on shows them at once.
    pub fn set_lod_map(&mut self, on: bool) {
        if on && self.lod_map.is_none() {
            self.lod_map = Some(crate::lod_map::LodMap::default());
        }
        self.lod_map_on = on;
        if !on {
            self.scene.borrow_mut().set_lod_map(None);
        }
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
    /// Asks for tile (`ti`, `tk`) of the big map's zoom level of `cell_m` cells on `face` (0 A,
    /// 1 B; `crate::map_reveal`).
    pub fn request_reveal(&mut self, face: u8, ti: i32, tk: i32, cell_m: i32) {
        let f = if face == 1 {
            FaceFrame::B
        } else {
            FaceFrame::A
        };
        self.pool.borrow_mut().submit(
            Purpose::Reveal(face, ti, tk, cell_m),
            crate::map_reveal::reveal_job(f, ti, tk, cell_m, &dwell_game::minimap::MAP),
        );
    }
    /// Asks for the generated colours of the explored cells in tile `key` (face, ti, tk;
    /// `dwell_game::minimap::Explored::ask`), in the job workers.
    pub fn request_explore_tile(&mut self, key: dwell_game::minimap::TileKey) {
        let (face, ti, tk) = key;
        let f = if face == 1 {
            FaceFrame::B
        } else {
            FaceFrame::A
        };
        self.pool.borrow_mut().submit(
            Purpose::ExploreTile(face, ti, tk),
            crate::map_reveal::reveal_job(
                f,
                ti,
                tk,
                dwell_game::minimap::MAP.explored_cell_m as i32,
                &dwell_game::minimap::MAP,
            ),
        );
    }
    /// The oldest finished explored-cell tile, once.
    pub fn take_explore_tile(&mut self) -> Option<(dwell_game::minimap::TileKey, Option<Vec<u8>>)> {
        self.explore_tiles.pop_front()
    }
    /// The oldest finished tile of the revealed big map, once.
    pub fn take_reveal(&mut self) -> Option<Vec<u8>> {
        self.reveal.pop_front()
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
    /// How a chunk is drawn, for the streaming probe: 0 as a chunk, 1 loaded but not drawn as one
    /// (waiting for its light or mesh, or the LOD shows its section), 2 not loaded.
    pub fn chunk_detail(&self, c: ChunkCoord) -> u8 {
        let a = c.to_array();
        if !self.streamer.is_loaded(a) {
            2
        } else if self.streamer.drawable(a) && self.chunk_visible(c) {
            0
        } else {
            1
        }
    }

    /// The streaming probe (the bench's per-frame series, docs/DEV_TOOLS.md): how the chunk under
    /// the feet (a metre down) and the one 10 m ahead along the way the player last moved are
    /// drawn ([`Self::chunk_detail`]), where the feet are, and the queues behind them.
    fn probe_stream(&mut self, feet: [f64; 3]) {
        let (last, mut heading) = self.probe;
        let (dx, dz) = (feet[0] - last[0], feet[2] - last[2]);
        let moved = dx.hypot(dz);
        if moved > 1e-3 && moved < 50.0 {
            heading = [dx / moved, dz / moved];
        }
        self.probe = (feet, heading);
        let f = dwell_world::FaceFrame::at_height(feet[1]);
        let local = f.point(feet);
        let at = |ahead: f64| {
            dwell_world::chunk_of_pos(f.point([
                local[0] + heading[0] * ahead,
                local[1] - 1.0,
                local[2] + heading[1] * ahead,
            ]))
        };
        telemetry::add("probe.under", f64::from(self.chunk_detail(at(0.0))));
        telemetry::add("probe.ahead", f64::from(self.chunk_detail(at(10.0))));
        const POINTS: [(&str, &str); 2] = [
            ("probe.target", "probe.target_m"),
            ("probe.control", "probe.control_m"),
        ];
        for (p, (detail, distance)) in self.probe_points.iter().zip(POINTS) {
            let c = dwell_world::chunk_of_pos(*p);
            telemetry::add(detail, f64::from(self.chunk_detail(c)));
            telemetry::add(distance, (p[0] - feet[0]).hypot(p[2] - feet[2]));
        }
        telemetry::add("probe.x", feet[0]);
        telemetry::add("probe.z", feet[2]);
        let [generating, mesh] = self.pool.borrow().pending();
        telemetry::add("pool.gen", generating as f64);
        telemetry::add("pool.mesh", mesh as f64);
        let s = self.streamer.stats();
        telemetry::add("stream.loaded", s.loaded as f64);
        telemetry::add("stream.dirty", s.dirty as f64);
        telemetry::add("stream.generating", s.generating as f64);
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
    /// The damage drawn on `cell` (its marks and cracks), if any.
    pub fn damage_at(&self, cell: [i32; 3]) -> Option<&dwell_protocol::damage::DamageView> {
        self.streamer.damage_at(cell)
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
    /// Loaded and meshed (or all air): drawable as full detail.
    pub fn chunk_drawable(&self, c: Coord) -> bool {
        self.streamer.drawable(c)
    }
    pub fn allow_flight(&mut self, input: &mut KeyboardMouseInput, allowed: bool) {
        input.flight.set_allowed(allowed);
    }

    /// Handles a snapshot, player event, chunk or LOD message from the session.
    pub fn on_game_message(&mut self, m: Message, byte_len: usize, now_ms: f64) {
        match m {
            Message::WaterCurrents(c) => self.on_currents(&c, now_ms),
            // The server's vitals as of an input (ADR 0069): the prediction is corrected to them.
            Message::Vitals(v) => {
                let vitals = dwell_protocol::vitals::Vitals {
                    stamina: v.stamina,
                    oxygen: v.oxygen,
                    exhausted: v.flags & dwell_protocol::vitals_flags::EXHAUSTED != 0,
                    rest_ticks: self.predictor.controller().vitals.rest_ticks,
                };
                self.predictor
                    .on_vitals(self.streamer.store_mut(), v.seq, vitals);
            }
            Message::ChunkData(d) => {
                // The server's data for the chunk: predictions in it are its business now.
                self.prediction.forget_chunk(d.coord);
                self.streamer.on_chunk_data(ChunkMessage::from(d))
            }
            Message::ChunkUnload(u) => {
                for &c in &u.coords {
                    self.prediction.forget_chunk(c);
                }
                self.note_unloads(&u.coords, now_ms);
                self.streamer.on_chunk_unload(&u.coords)
            }
            Message::VoxelModification(mut v) => {
                // Cells showing a prediction keep it until their edits' ack (§6.5).
                self.prediction.filter(&mut v.chunks);
                if v.reason == dwell_protocol::VoxelModificationReason::Edit {
                    let changes = self.cell_changes(&v.chunks);
                    self.edit_sounds.changes(&changes, now_ms, &mut self.sounds);
                }
                // A lit explosive's cell gone in an `Explosion` is its blast going off: its
                // effects and its boom (docs/EXPLOSIVES.md §4).
                if v.reason == dwell_protocol::VoxelModificationReason::Explosion {
                    let changes = self.cell_changes(&v.chunks);
                    self.blasts_from(&changes, now_ms);
                }
                // A settling piece's blocks: hidden from the terrain while its glide draws them.
                let settled = (v.reason == dwell_protocol::VoxelModificationReason::Rebake)
                    .then(|| self.cell_changes(&v.chunks));
                self.streamer.on_voxel_modification(&v.chunks);
                if let Some(changes) = settled {
                    let cells: Vec<_> = changes.iter().map(|c| (c.cell, c.new)).collect();
                    let pieces = &self.pieces;
                    let hide = self.glides.rebaked(&cells, now_ms, |c| pieces.near_cell(c));
                    self.streamer.hide_cells(&hide, true);
                }
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
                if self.swing_hello.on_snapshot(s.swing_kinds, now_ms) {
                    self.outbox
                        .borrow_mut()
                        .push(Send::Datagram(Message::SwingHello));
                }
                // The pieces' poses first: drawn interpolated, and stood in for in the prediction
                // at their latest carried on to the present, so a replay meets them where the
                // server had them (docs/FELLING.md §3, BUILT_BODIES.md §6).
                self.pieces.on_snapshot(
                    &s.pieces,
                    s.ack_input_seq,
                    s.server_tick,
                    now_ms,
                    &mut self.sounds,
                );
                for (id, cells, position, rotation, velocity, seq) in self.pieces.latest() {
                    self.predictor.set_piece(
                        id,
                        cells,
                        position,
                        rotation,
                        velocity.as_vec3(),
                        seq,
                    );
                }
                // Only the pieces that could touch the player soon are stood in for.
                telemetry::sample("pieces.standIns", self.predictor.stand_ins() as f64);
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
                let latest = self.remotes.latest();
                self.remote_impacts(&latest, now_ms);
                for r in latest {
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
                if self.current.active && !self.predictor.active() {
                    // Died: the player is no longer placed, even where the prediction never
                    // ticked (the terrain around had not arrived), so the respawn is a placement
                    // the loading screen waits for.
                    self.current = ClientState::of(&self.predictor);
                }
                if !self.current.active {
                    self.current = ClientState::of(&self.predictor);
                    // Placed: the loading screen waits for the near ring (not in flight, which
                    // outruns the streaming, gate.rs).
                    if self.current.active && self.current.state != PlayerState::Flying {
                        self.loading.start(now_ms);
                    } else if self.current.active {
                        self.loading.skip();
                    }
                    self.flip.reset(self.current.face);
                    // Placed (joined or respawned): the camera starts where the player is, not
                    // interpolated from the state before, whose eye was elsewhere (the default
                    // state's at the origin, inside the ground) and aimed the crosshair from there.
                    self.previous = None;
                    self.eye = EyeCamera::new();
                    self.eye
                        .tick(&self.current.eye_state(), 1.0 / SIM_HZ as f64);
                } else if !prediction_may_run(
                    self.current.active,
                    self.current.state,
                    self.terrain_ready(),
                ) && self.moved_by_server()
                {
                    // Moved by the server while the prediction waits for terrain (the dev tools'
                    // teleport, docs/DEV_TOOLS.md): the player is where the server has it, so
                    // the terrain the prediction waits for is the terrain there, not where the
                    // player was (which the server stops streaming, so it never arrives).
                    self.current = ClientState::of(&self.predictor);
                    self.flip.reset(self.current.face);
                    self.previous = None;
                    self.eye = EyeCamera::new();
                    self.eye
                        .tick(&self.current.eye_state(), 1.0 / SIM_HZ as f64);
                }
            }
            Message::EntitySpawn(e) => {
                let (meshes, radius) = self.pieces.spawn(&e, now_ms, &mut self.sounds);
                self.scene
                    .borrow_mut()
                    .set_piece(e.id, Some((&meshes, radius)));
            }
            Message::BodyEdit(e) => {
                // Its cells changed (docs/BUILT_BODIES.md §4): drawn anew, and stood in for anew
                // from the next snapshot.
                if let Some((meshes, radius)) = self.pieces.edit(&e) {
                    self.scene
                        .borrow_mut()
                        .set_piece(e.body, Some((&meshes, radius)));
                    self.predictor.remove_piece(e.body);
                }
            }
            Message::Cuts(c) => {
                self.chop.on = true;
                // Carved into the terrain mesh (ADR 0067 §11): each changed cell's chunk again.
                for cell in &c.gone {
                    self.streamer.set_cut(*cell, None);
                }
                for (cell, cut) in &c.cuts {
                    self.streamer.set_cut(*cell, Some(*cut));
                }
                self.chop.apply(c);
            }
            Message::Steam(cells) => {
                // Lava boiled water away (VOLCANOES.md §6.3): a puff over each cell.
                self.steam.add(&cells, now_ms / 1000.0);
                // And its hiss, once a message, at the first cell (shared/sounds `steam_hiss`).
                if let Some(c) = cells.first() {
                    let at = [
                        f64::from(c[0]) + 0.5,
                        dwell_world::m_of_row(c[1]) + 0.25,
                        f64::from(c[2]) + 0.5,
                    ];
                    self.sounds
                        .push(SoundEvent::new(SoundKind::SteamHiss, 0, 1.0, Some(at)));
                }
            }
            Message::Damage(d) => {
                // Another player's hit: its material's hit and chips at the struck point.
                for (cell, mark) in self.remote_hits.on_damage(&d, now_ms) {
                    let [x, y, z] = cell;
                    let block = self.voxel(x, y, z);
                    let at = point_in_cell(cell, mark.point);
                    self.sounds
                        .push(SoundEvent::new(SoundKind::Hit, block, 1.0, Some(at)));
                    self.hit_chips(block, cell, at, now_ms);
                }
                // Drawn on the struck faces (docs/BLOCK_DAMAGE.md): each changed cell's chunk again.
                for cell in &d.gone {
                    self.streamer.set_damage(*cell, None);
                }
                for (cell, view) in &d.cells {
                    self.streamer.set_damage(*cell, Some(view.clone()));
                }
            }
            Message::HeldTools(list) => {
                // The server sweeps swings along their path (its answer to our first `Held`).
                self.chop.sweep = true;
                for h in list {
                    match dwell_protocol::chopping::tool(&h.tool) {
                        Some(t) => {
                            self.remote_tools.insert(h.player_id, (t.name, h.vertical));
                        }
                        None => {
                            self.remote_tools.remove(&h.player_id);
                        }
                    }
                }
            }
            Message::Inventory(i) => {
                let uses = i.flags & dwell_protocol::inventory_flags::USES_ITEMS != 0;
                self.inventory_flags = i.flags;
                // The server keeps ground items (ADR 0071): say so once, and draw them.
                if i.flags & dwell_protocol::inventory_flags::GROUND != 0 && !self.ground_on {
                    self.ground_on = true;
                    self.outbox
                        .borrow_mut()
                        .push(Send::Control(Message::InventoryAction(
                            dwell_protocol::InventoryAction::GroundHello,
                        )));
                    self.streamer.set_ground(self.world_seed);
                }
                // The server keeps block damage (docs/BLOCK_DAMAGE.md): say so once, and draw it.
                if i.flags & dwell_protocol::inventory_flags::DAMAGE != 0 && !self.damage_on {
                    self.damage_on = true;
                    self.remote_hits.hello(now_ms);
                    self.outbox
                        .borrow_mut()
                        .push(Send::Control(Message::DamageHello));
                }
                // The server takes `CausesHello`: say once which damage causes this build knows.
                if i.flags & dwell_protocol::inventory_flags::CAUSES != 0 && !self.causes_on {
                    self.causes_on = true;
                    self.outbox
                        .borrow_mut()
                        .push(Send::Control(Message::CausesHello(
                            dwell_protocol::DamageCause::MAX,
                        )));
                }
                // The server has lava (VOLCANOES.md §6.3): ask once for its steam.
                if i.flags & dwell_protocol::inventory_flags::STEAM != 0 && !self.steam_on {
                    self.steam_on = true;
                    self.outbox
                        .borrow_mut()
                        .push(Send::Control(Message::SteamHello));
                }
                // The server keeps creatures (ADR 0073): say so once, and draw them.
                if i.flags & dwell_protocol::inventory_flags::CREATURES != 0 && !self.creatures_on {
                    self.creatures_on = true;
                    self.outbox
                        .borrow_mut()
                        .push(Send::Control(Message::CreaturesHello));
                }
                // The server runs a far ecology (docs/ECOLOGY.md §9): ask for its far groups once.
                if i.flags & dwell_protocol::inventory_flags::ECOLOGY != 0 && !self.ecology_on {
                    self.ecology_on = true;
                    self.outbox
                        .borrow_mut()
                        .push(Send::Control(Message::EcologyHello));
                }
                // A tool that broke from wear (owner, 2026-11-11): its break sound.
                if let Some((_, old)) = &self.loose.inventory
                    && old.tool_broke(&i.inventory, dwell_protocol::chopping::durability)
                    && let Some(ev) = dwell_protocol::chopping::WEAR_BREAK_SOUND
                        .and_then(|n| SoundKind::ALL.into_iter().find(|k| k.name() == n))
                {
                    self.sounds.push(SoundEvent::new(ev, 0, 1.0, None));
                }
                self.loose.inventory = Some((uses, i.inventory));
                if uses {
                    self.select_inventory_slot(self.inventory_slot);
                }
            }
            Message::Creatures(m) => self.creatures.apply(m),
            Message::Ecology(m) => {
                self.ecology = Some(m);
                self.far_stale = true;
            }
            Message::GroundGone(cells) => {
                self.streamer.ground_gone(&cells);
            }
            Message::ItemDrops(d) => {
                let changed = self.loose.apply(d);
                let mut scene = self.scene.borrow_mut();
                for id in changed.removed {
                    scene.set_piece(crate::loose_items::ID_BASE | id, None);
                }
                for (id, item, face) in changed.added {
                    let meshes = crate::loose_items::item_meshes(&item, face);
                    scene.set_piece(
                        crate::loose_items::ID_BASE | id,
                        meshes.as_ref().map(|(m, r)| (m.as_slice(), *r)),
                    );
                }
            }
            Message::EntityDespawn(id) => {
                let settled = self.pieces.despawn(id, now_ms);
                self.predictor.remove_piece(id);
                let mut scene = self.scene.borrow_mut();
                scene.set_piece(id, None);
                if let Some(piece) = settled {
                    for (part, cells, mirror) in self.glides.settled(&piece, now_ms) {
                        let (meshes, radius) = crate::settle_glide::part_meshes(&cells, mirror);
                        scene.set_piece(part, Some((&meshes, radius)));
                    }
                }
            }
            Message::PlayerEvent(e) if e.player_id == self.player_id => {
                // Ours: hurt, death and respawn sound at once, at the listener.
                self.sounds
                    .extend(player_event_sound(e.kind, e.amount, None));
                match e.kind {
                    PlayerEventKind::Knockback => {
                        let v = Vec3::new(e.vector[0], e.vector[1], e.vector[2]);
                        self.predictor
                            .on_knockback(self.streamer.store_mut(), e.input_seq, v);
                    }
                    PlayerEventKind::Damage => {
                        self.hud.damaged = true;
                        // Burning (lava, VOLCANOES.md §6.3): the screen's red-orange edge for
                        // `burnEdge.seconds` and a sizzle.
                        if e.cause == dwell_protocol::DamageCause::Burn {
                            self.burn_until_ms =
                                now_ms + f64::from(dwell_lava::DATA.burn_edge.seconds) * 1000.0;
                            self.sounds
                                .push(SoundEvent::new(SoundKind::Sizzle, 0, 1.0, None));
                        }
                    }
                    _ => {}
                }
            }
            Message::PlayerEvent(e) => {
                // Another player's: when its body is drawn at that moment.
                let due = now_ms + players::INTERP_DELAY_MS as f64;
                self.pending_player_events.push((due, e));
            }
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
                (Purpose::Base(c), Output::Chunk { voxels, .. }) => {
                    self.streamer.on_generated_base(c, &voxels)
                }
                (Purpose::ChunkMesh(c, token), Output::ChunkMesh(m)) => {
                    let bytes = (m.opaque.byte_len()
                        + m.transparent.byte_len()
                        + m.cutout.byte_len()) as u64;
                    self.mesh_uploads.push((c, token, m), bytes);
                }
                (Purpose::Verify, Output::Chunk { hash, voxels }) => {
                    let hash = self
                        .registry
                        .as_ref()
                        .map_or(hash, |m| m.verification_hash(&voxels, hash));
                    self.verify(hash)
                }
                (Purpose::Map, Output::Map(bytes)) => self.map = Some(bytes.unwrap_or_default()),
                (Purpose::Reveal(face, ti, tk, cell_m), out) => {
                    let rgba = match &out {
                        Output::LodMap(Some(p)) => Some(p.as_slice()),
                        _ => None,
                    };
                    self.reveal
                        .push_back(crate::map_reveal::encode_tile(face, ti, tk, cell_m, rgba));
                }
                (Purpose::ExploreTile(face, ti, tk), out) => {
                    let rgba = match out {
                        Output::LodMap(p) => p,
                        _ => None,
                    };
                    self.explore_tiles.push_back(((face, ti, tk), rgba));
                }
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
                (Purpose::LodMap(job), out) => {
                    if let Some(map) = &mut self.lod_map
                        && map.on_done(&job)
                        && let Output::LodMap(Some(rgba)) = out
                    {
                        self.scene
                            .borrow_mut()
                            .lod_map_tile(job.ring, job.rect, &rgba);
                    }
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
                                tints,
                            } => Some(dwell_lod::GeneratedSection {
                                cells,
                                surface,
                                surface_b,
                                tints,
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
        self.upload_meshes();
    }

    /// Hands this frame's share of the finished chunk meshes to the scene: oldest first, as many
    /// as fit the scene's byte budget (the first always); the rest wait for the next frame.
    fn upload_meshes(&mut self) {
        let (budget, waiting) = {
            let s = self.scene.borrow();
            (s.upload_budget_bytes(), s.uploads_waiting())
        };
        let take = if waiting {
            Vec::new()
        } else {
            self.mesh_uploads.take(budget)
        };
        for (c, token, mut m) in take {
            let ground = std::mem::take(&mut m.ground);
            self.keep_ground_density(c, ground);
            self.streamer.on_meshed(c, token, *m);
        }
        telemetry::add("upload.waiting", self.mesh_uploads.len() as f64);
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
            body_target: self.body_target.map(|b| (b.body, b.cell, b.face)),
            lod: self.lod.as_ref().map(LodSystem::debug_stats),
            flying: c.state == PlayerState::Flying,
            loading_enabled: self.loading.enabled(),
            loading: self.loading.screen(),
            loading_opened: self.loading.last_opened(),
        }
    }

    /// Breaks or places at the crosshair (§6.5): only while alive and playing. Shown and sounded at
    /// once (edit prediction); nothing targeted sounds a refusal.
    pub fn edit(&mut self, action: EditAction, now_ms: f64) -> bool {
        self.edit_with(action, None, now_ms)
    }

    /// [`edit`](Self::edit), held by `holder` if any (a place with nothing targeted then starts a
    /// drag stroke from the edge it builds off).
    fn edit_with(
        &mut self,
        action: EditAction,
        holder: Option<dwell_game::build_drag::Holder>,
        now_ms: f64,
    ) -> bool {
        if self.dead || !self.terrain_ready() {
            return false;
        }
        // A ground item aimed at, nearer than the block behind it: the break (a click, the touch
        // Break button, the controller's, an axe's swing) knocks it loose (ADR 0071).
        if action == EditAction::Break
            && !self.brush.active()
            && !self.region.pasting()
            && self.hit_ground()
        {
            return true;
        }
        // The axe held (ADR 0067): Use turns the swing, Break swings at wood aimed at.
        if let Some(tool) = self.held_tool() {
            return self.chop_with(tool, action, now_ms);
        }
        // In a world that uses items (ADR 0066) a place needs the held slot to hold a block; a
        // hotbar tool that does not chop (a pickaxe, a shovel) places nothing either.
        if action == EditAction::Place
            && ((self.loose.uses_items() && self.held_palette_slot().is_none())
                || self.palette_tool().is_some())
        {
            self.sounds.push(sound_event(SoundKind::Deny));
            return false;
        }
        // The brush tool: left click its arrow, right click its gunpowder (BRUSHES.md).
        if self.brush.active() {
            return self.brush_apply(action == EditAction::Place, now_ms);
        }
        // A moving body aimed at, nearer than any block (docs/BUILT_BODIES.md §4): break its cell.
        if let Some(b) = self.body_target.filter(|_| !self.region.pasting()) {
            return self.edit_body(action, b, now_ms);
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
            if action == EditAction::Place && self.edge_place(holder, now_ms) {
                return true;
            }
            self.sounds.push(sound_event(SoundKind::Deny));
            return false;
        };
        let placement = self.interaction.placement();
        // A tool's hit lands at its swing's contact (`contactS`; WORK_QUEUE item 25): one swing at
        // a time, and its effects (and a one-hit break's) wait for the contact.
        let tool_item = (action == EditAction::Break)
            .then(|| self.held_break_item())
            .flatten();
        let contact_s = dwell_protocol::chopping::break_contact_s(tool_item);
        if contact_s > 0.0 && self.pending_hits.busy(now_ms) {
            return false;
        }
        // A steep slope's upper half goes with it (`act_upper`), read before the place's
        // prediction changes the target.
        let upper = (action == EditAction::Place)
            .then(|| self.interaction.act_upper())
            .flatten();
        match self.interaction.act(action, now_ms) {
            Some(m) => {
                // The sound first: the prediction changes the block at once.
                let mut predict = true;
                match (action, placement) {
                    (EditAction::Break, _) if contact_s > 0.0 => {
                        // Sent now (the server lands it at the contact); shown at the contact,
                        // a one-hit break by the server's change then (not predicted).
                        let [x, y, z] = target.cell;
                        let block = self.voxel(x, y, z);
                        let name = dwell_blocks::block_of(block).name;
                        let breaks = self.hits_to_break(name) == Some(1);
                        let at = self
                            .struck_point(target.cell, target.face)
                            .map(|p| point_in_cell(target.cell, p));
                        if breaks {
                            self.edit_sounds.expect_break(target.cell, now_ms);
                        } else {
                            self.remote_hits.own_hit(target.cell, now_ms);
                        }
                        self.pending_hits.push(crate::hit_feedback::PendingHit {
                            due_ms: crate::hit_feedback::PendingHits::due(now_ms, contact_s),
                            item: tool_item.expect("a tool with a contact"),
                            cell: target.cell,
                            block,
                            at,
                            breaks,
                        });
                        predict = false;
                    }
                    (EditAction::Break, _) => {
                        let [x, y, z] = target.cell;
                        let block = self.voxel(x, y, z);
                        let name = dwell_blocks::block_of(block).name;
                        if self.hits_to_break(name) == Some(1) {
                            self.edit_sounds.local_break(
                                target.cell,
                                block,
                                now_ms,
                                &mut self.sounds,
                            );
                        } else {
                            // A hit short of breaking (docs/BLOCK_DAMAGE.md): the material's hit
                            // at the struck point; the block stays until the server breaks it.
                            let at = self
                                .struck_point(target.cell, target.face)
                                .map(|p| point_in_cell(target.cell, p));
                            self.sounds
                                .push(SoundEvent::new(SoundKind::Hit, block, 1.0, at));
                            // Its chips; the server's echo of the hit is not shown again.
                            self.remote_hits.own_hit(target.cell, now_ms);
                            if let Some(at) = at {
                                self.hit_chips(block, target.cell, at, now_ms);
                            }
                        }
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
                // The struck point, to a server that keeps damage (an older one reads none).
                let mut m = m;
                if let Message::BlockEditRequest(e) = &mut m
                    && e.action == dwell_protocol::BlockEditAction::Break
                    && self.damage_on
                {
                    e.point = self.struck_point(target.cell, target.face);
                }
                let id = (!predict).then(|| self.prediction.next_id());
                self.send_edit(m, id);
                if let Some(u) = upper {
                    self.send_edit(u, None);
                }
                true
            }
            None => false,
        }
    }

    /// Breaks the cell of a moving body aimed at, or places the selected block against the face
    /// aimed at (a body its kind lets be built on: a built one; else refused), as `BodyEditRequest`
    /// (docs/BUILT_BODIES.md §4). Shown when the server answers.
    fn edit_body(&mut self, action: EditAction, b: crate::pieces::BodyHit, now_ms: f64) -> bool {
        let builds = self
            .pieces
            .kind_of(b.body)
            .is_some_and(|k| dwell_sim::felling::body_kind(k).place);
        let material = self.selected_material();
        if action == EditAction::Place && (!builds || material == 0) {
            self.sounds.push(sound_event(SoundKind::Deny));
            return false;
        }
        if now_ms - self.body_edit_ms < players::BLOCK_EDIT_INTERVAL_MS as f64 {
            return false;
        }
        self.body_edit_ms = now_ms;
        if action == EditAction::Place {
            self.bodies
                .swing_arms(dwell_characters::animator::Swing::Place);
            self.outbox
                .borrow_mut()
                .push(Send::Control(Message::BodyEditRequest(
                    dwell_protocol::BodyEditRequest {
                        body: b.body,
                        cell: b.cell.map(|v| v as i16),
                        face: b.face,
                        action: dwell_protocol::BlockEditAction::Place,
                        material,
                    },
                )));
            return true;
        }
        // Its block's break sound where it was hit.
        if let (Some(eye), Some(block)) = (self.target_eye, self.pieces.block_at(b.body, b.cell)) {
            let at: [f64; 3] = std::array::from_fn(|k| eye[k] + self.view_dir[k] * b.distance);
            let cell = dwell_world::cell_of_pos(at);
            self.edit_sounds
                .local_break(cell, block, now_ms, &mut self.sounds);
        }
        self.bodies
            .swing_arms(dwell_characters::animator::Swing::Break);
        self.outbox
            .borrow_mut()
            .push(Send::Control(Message::BodyEditRequest(
                dwell_protocol::BodyEditRequest {
                    body: b.body,
                    cell: b.cell.map(|v| v as i16),
                    face: b.face,
                    action: dwell_protocol::BlockEditAction::Break,
                    material: 0,
                },
            )));
        true
    }

    /// Breaks or places as [`edit`](Self::edit), with the button held by `holder`: a place
    /// starts a drag stroke from the block placed (docs/ITEMS.md §5), built on each frame until
    /// `holder` lets go ([`release_place`](Self::release_place)).
    pub fn edit_held(
        &mut self,
        action: EditAction,
        holder: dwell_game::build_drag::Holder,
        now_ms: f64,
    ) -> bool {
        // On a held body (built on a build platform, so lined up with the world's grid): the click
        // places against it, and the drag builds on in the world's cells next to it, which the
        // server puts into the body (docs/BUILT_BODIES.md §3).
        if action == EditAction::Place
            && let Some(b) = self.body_target
            && self.region.enabled
            && !self.brush.active()
            && !self.region.pasting()
        {
            let lined_up = b.rotation.angle_between(dwell_physics::DQuat::IDENTITY) < 1e-3;
            let placed = self.edit(action, now_ms);
            if placed && lined_up {
                let dir = dwell_world::edit::FACE_DIRS[usize::from(b.face)];
                let new = [b.cell[0] + dir[0], b.cell[1] + dir[1], b.cell[2] + dir[2]];
                let at = b.position
                    + DVec3::new(
                        f64::from(new[0]),
                        f64::from(new[1]) * dwell_world::CELL_H,
                        f64::from(new[2]),
                    );
                let cell = dwell_world::cell_of_pos(at.to_array());
                let material = self.selected_material();
                self.drag.start(
                    holder,
                    &[cell],
                    dir,
                    material,
                    dwell_game::items::ItemDrag::Modes,
                    now_ms,
                );
            }
            return placed;
        }
        let start = (action == EditAction::Place
            && self.region.enabled
            && !self.brush.active()
            && !self.region.pasting())
        .then(|| {
            let t = self.interaction.target()?;
            let (_, material) = self.interaction.placement()?;
            let slot = self
                .interaction
                .palette()
                .get(self.interaction.selected())?;
            // A piece with its own upper state (a steep slope) is placed as two single cells.
            let rows = if slot.upper.is_empty() { slot.rows } else { 1 };
            Some((t, material, rows, slot.drag))
        })
        .flatten();
        let upper = self.interaction.upper_placement();
        // The piece the place will fill, as predicted, before the edit predicts it.
        let drag = start.map_or(dwell_game::items::ItemDrag::None, |s| s.3);
        let piece = start.and_then(|(t, material, rows, _)| {
            let m = Message::BlockEditRequest(dwell_protocol::BlockEditRequest {
                action: dwell_protocol::BlockEditAction::Place,
                cell: t.cell,
                face: t.face,
                material,
                rows,
                down: rows > 1 && t.face != 2 && t.face != 3 && t.aim_up < 0.5,
                point: None,
            });
            let cells: Option<Vec<[i32; 3]>> = self
                .predicted_changes(&m)
                .map(|c| c.iter().map(|(c, _)| *c).collect());
            let dir = dwell_world::edit::FACE_DIRS
                .get(usize::from(t.face))
                .copied()?;
            Some((m, cells, dir, material))
        });
        let Some((m, piece, dir, material)) = piece else {
            return self.edit_with(action, Some(holder), now_ms);
        };
        // Standing where the block goes (towering): it waits, held, until a jump lifts the body
        // clear (docs/ITEMS.md §5). Not a two-state piece (a steep slope): it is placed or not.
        if piece.is_none()
            && upper.is_none()
            && self.predicted_changes_with(&m, false).is_some()
            && let Message::BlockEditRequest(request) = m
        {
            self.drag.wait(dwell_game::build_drag::Waiting {
                holder,
                request,
                dir,
                material,
                pressed_ms: now_ms,
            });
            return self.drag.waiting().is_some() || self.edit(action, now_ms);
        }
        let placed = self.edit(action, now_ms);
        if placed && let Some(mut cells) = piece {
            // Each row's state from the lowest up: a steep slope's halves (face B's upper half is
            // below its lower).
            let mut rows: Vec<([i32; 3], StateId)> = cells.iter().map(|&c| (c, material)).collect();
            if let Some((c, m)) = upper {
                rows.push((c, m));
                cells.push(c);
            }
            rows.sort_by_key(|(c, _)| c[1]);
            cells.sort_by_key(|c| c[1]);
            let materials: Vec<StateId> = rows.iter().map(|&(_, m)| m).collect();
            self.drag
                .start_with(holder, &cells, dir, &materials, drag, now_ms);
        }
        placed
    }

    /// Bridging help (docs/ITEMS.md §5): Place with nothing targeted builds off the block stood
    /// on, against its side faced (`build_drag::edge_target`), level with it; held, the stroke
    /// builds on from there. A region fill: the server refuses a place against a face the eye
    /// can't see, as the side of the block underfoot is. Plain blocks only (`build_drag::drags`).
    fn edge_place(&mut self, holder: Option<dwell_game::build_drag::Holder>, now_ms: f64) -> bool {
        // Not a slope: it faces by the look and a target (`BlockInteraction::placement`).
        if self
            .interaction
            .palette()
            .get(self.interaction.selected())
            .is_some_and(|s| s.facing != dwell_game::items::ItemFacing::None || !s.upper.is_empty())
        {
            return false;
        }
        let Some((cells, dir, material)) = self.edge_spot() else {
            return false;
        };
        let rows = cells.len();
        let min = [
            cells[0][0],
            cells.iter().map(|c| c[1]).min().unwrap_or(cells[0][1]),
            cells[0][2],
        ];
        let fill = dwell_protocol::RegionEdit {
            op: dwell_protocol::RegionEditOp::Fill,
            min,
            size: [1, rows as u8, 1],
            material,
            from: 0,
            cells: Vec::new(),
        };
        self.edit_sounds
            .local_place(min, material, now_ms, &mut self.sounds);
        self.bodies
            .swing_arms(dwell_characters::animator::Swing::Place);
        self.send_drag_fill(fill, 0);
        if let Some(holder) = holder {
            self.drag.start(
                holder,
                &cells,
                dir,
                material,
                dwell_game::items::ItemDrag::Modes,
                now_ms,
            );
            // Held, it builds on only while the look stays below the horizon (owner, 2026-11-02).
            self.drag.bridging();
        }
        true
    }

    /// Where bridging help would build now (`build_drag::edge_target`: the edge looked toward, at
    /// the level stood on): the piece's cells (its top cell level with the block stood on, down
    /// from it), its direction out from the platform and the block; None when it wouldn't (no
    /// plain block selected, nothing under the body, no edge in reach).
    fn edge_spot(&mut self) -> Option<(Vec<[i32; 3]>, [i32; 3], StateId)> {
        use dwell_game::build_drag::{DragWorld, drags, edge_target};
        let slot = self
            .interaction
            .palette()
            .get(self.interaction.selected())?;
        let (material, rows) = (slot.material, i32::from(slot.rows));
        if !self.region.enabled
            || !drags(material)
            || slot.drag != dwell_game::items::ItemDrag::Modes
            || self.brush.active()
            || self.region.pasting()
        {
            return None;
        }
        let body = self.own_capsule()?;
        let eye = self.target_eye?;
        let feet = self.current.feet();
        let view = self.view_dir;
        let me = self.drag_body();
        let settled = self.drag_sent.is_empty();
        let mut world = DragCells {
            world: self.streamer.store_mut(),
            me,
            settled,
        };
        let reach = f64::from(dwell_protocol::players::REACH_DISTANCE);
        let (cell, dir) = edge_target(
            feet,
            f64::from(body.radius),
            eye,
            view,
            reach,
            material,
            &mut world,
        )?;
        // Level with the block stood on: down from it (up would be where the body is).
        let up = dwell_world::FaceFrame::at_row(cell[1]).sign();
        let cells: Vec<[i32; 3]> = (0..rows)
            .map(|r| [cell[0], cell[1] - r * up, cell[2]])
            .collect();
        cells
            .iter()
            .all(|&c| world.free(c, material))
            .then_some((cells, dir, material))
    }

    /// `holder` let go of Place: its drag stroke ends.
    pub fn release_place(&mut self, holder: dwell_game::build_drag::Holder) {
        self.drag.release(holder);
    }

    /// The next build mode (the key, the controller's button, the touch Build button): its label,
    /// for the HUD.
    pub fn cycle_build_mode(&mut self) -> &'static str {
        self.sounds.push(sound_event(SoundKind::Tool));
        self.drag.cycle().label
    }

    /// A drag stroke's next pieces at the aim from `eye` along `dir`, and the fills the server's
    /// rate takes now, shown at once (edit prediction).
    fn drag_build(&mut self, eye: Option<[f64; 3]>, dir: [f64; 3], now_ms: f64) {
        if self.dead || !self.region.enabled {
            self.drag.cancel();
        }
        // A first place waiting for the body: it goes once the body is clear of it.
        if let Some(w) = self.drag.waiting() {
            let m = Message::BlockEditRequest(w.request.clone());
            let me = self.drag_body();
            let material = w.material;
            let clear = |world: &mut dwell_world::VoxelWorld, cells: &[[i32; 3]]| {
                let mut free = DragCells {
                    world,
                    me,
                    settled: true,
                };
                cells
                    .iter()
                    .all(|&c| dwell_game::build_drag::DragWorld::free(&mut free, c, material))
            };
            if let Some(changes) = self.predicted_changes(&m)
                && clear(
                    self.streamer.store_mut(),
                    &changes.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
                )
                && let Some(w) = self.drag.take_waiting()
            {
                let cells: Vec<[i32; 3]> = changes.iter().map(|(c, _)| *c).collect();
                if let Some(&at) = cells.first() {
                    self.edit_sounds
                        .local_place(at, w.material, now_ms, &mut self.sounds);
                }
                self.bodies
                    .swing_arms(dwell_characters::animator::Swing::Place);
                self.send_edit(m, None);
                self.drag.start(
                    w.holder,
                    &cells,
                    w.dir,
                    w.material,
                    dwell_game::items::ItemDrag::Modes,
                    w.pressed_ms,
                );
            }
        }
        // It builds on once Place has been held a moment (a click places one block).
        if let Some(eye) = eye
            && self.drag.armed(now_ms)
        {
            let me = self.drag_body();
            let settled = self.drag_sent.is_empty();
            let mut world = DragCells {
                world: self.streamer.store_mut(),
                me,
                settled,
            };
            self.drag.aim(
                eye,
                dir,
                f64::from(dwell_protocol::players::REACH_DISTANCE),
                &mut world,
            );
        }
        for (fill, tries) in self.drag.pump_tries(now_ms) {
            let at = fill.min;
            let material = fill.material;
            // A retry sounds no second time.
            if tries == 0 {
                self.edit_sounds
                    .local_place(at, material, now_ms, &mut self.sounds);
            }
            self.send_drag_fill(fill, tries);
        }
    }

    /// The world's portals (BIFACIAL_WORLD.md §12): their axes and mote look, from the generator
    /// and seed the server announced, for the motes drawn in their shafts.
    pub fn set_world(&mut self, generator_version: u32, world_seed: u64) {
        self.portals = dwell_worldgen::portal_axes_for(generator_version, world_seed);
        self.world_seed = world_seed;
    }

    /// What the server announced it takes (Welcome flags, and the world's game mode when the host
    /// changes it): region edits, and brush strokes with the brush reach where the mode allows
    /// brushes (BRUSHES.md, ADR 0065).
    /// The world's stamina and oxygen rules (ADR 0069), from its mode on a server with them.
    pub fn set_vitals_rules(&mut self, rules: dwell_protocol::vitals::VitalsRules) {
        self.vitals_rules = rules;
        self.predictor.set_vitals_rules(rules);
    }

    /// Whether the world's mode breaks every block in one hit without a tool (`instantBreaks`,
    /// creative; owner 2026-11-11), so the break prediction matches the server's.
    pub fn set_instant_breaks(&mut self, on: bool) {
        self.instant_breaks = on;
    }

    /// The hits the server will take to break `block` with what is held now.
    fn hits_to_break(&self, block: &str) -> Option<u32> {
        dwell_protocol::damage::hits_to_break_for(
            block,
            self.held_break_item(),
            self.instant_breaks,
        )
    }

    pub fn set_capabilities(&mut self, region_edits: bool, brushes: bool) {
        self.region.enabled = region_edits;
        self.region.keep = brushes;
        self.region.reach = if brushes {
            dwell_world::brush::REACH.max(dwell_protocol::region_edit::REACH)
        } else {
            dwell_protocol::region_edit::REACH
        };
        self.brush.available = region_edits && brushes;
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
    fn send_edit(&mut self, m: Message, id: Option<u32>) -> u32 {
        let id = id.unwrap_or_else(|| {
            let id = self.prediction.next_id();
            if let Some(changes) = self.predicted_changes(&m) {
                self.prediction.predict(id, &changes, &mut self.streamer);
            }
            id
        });
        self.outbox.borrow_mut().push(Send::Edit(m, id));
        id
    }

    /// Sends a drag building fill, kept until the server answers so a refused one is tried again
    /// (`build_drag::RETRIES`; the server's per-tick region queue refuses a bunch of them).
    fn send_drag_fill(&mut self, fill: dwell_protocol::RegionEdit, tries: u8) {
        let id = self.send_edit(Message::RegionEdit(fill.clone()), None);
        self.drag_sent.insert(id, (fill, tries));
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
    /// the crosshair targets from, with our own body but not the other players'): `None` when it would be
    /// refused or its box isn't loaded. The server's later changes (blocks left unsupported) come
    /// with its modification.
    fn predicted_changes(&mut self, m: &Message) -> Option<Vec<([i32; 3], StateId)>> {
        self.predicted_changes_with(m, true)
    }

    /// [`predicted_changes`](Self::predicted_changes), without our own body (`body` false): what
    /// the edit would do were we out of the way.
    fn predicted_changes_with(
        &mut self,
        m: &Message,
        body: bool,
    ) -> Option<Vec<([i32; 3], StateId)>> {
        use dwell_game::region_tools::BoxSource;
        let eye = self.target_eye?;
        // Our own body, as the server sees it (its `EditCapsule`s): a block placed into it is
        // refused there, so predicting it would show it for a moment (docs/BUGS.md 7).
        let me = self.own_capsule().filter(|_| body);
        let players: &[dwell_world::edit::EditCapsule] = me.as_slice();
        match m {
            Message::BlockEditRequest(e) => {
                // A hit short of breaking leaves the block (docs/BLOCK_DAMAGE.md): only a break
                // the data says takes one hit is shown at once (the server's damage so far isn't
                // the client's; a last hit shows when the server's change arrives).
                if e.action == dwell_protocol::BlockEditAction::Break {
                    let [x, y, z] = e.cell;
                    let block =
                        dwell_blocks::block_of(self.streamer.store_mut().get_voxel(x, y, z)).name;
                    if self.hits_to_break(block) != Some(1) {
                        return None;
                    }
                }
                let world = self.streamer.store_mut();
                let out = dwell_world::edit::check_block_edit(
                    world,
                    e,
                    eye,
                    players,
                    dwell_world::edit::DEFAULT_REACH,
                );
                // With a multi-cell block's other cells (a torch's upper part), as the server
                // writes them.
                (out.check == dwell_world::edit::EditCheck::Ok)
                    .then(|| out.writes(world, dwell_world::edit::medium_kept))
            }
            Message::RegionEdit(e) => {
                // Every chunk of the box loaded (Air ones too), so the client's world reads it right.
                self.streamer.read_box(e.min, e.size.map(i32::from))?;
                let reach = self.region.reach + 1.0;
                let out = dwell_world::region::check_region_edit(
                    self.streamer.store_mut(),
                    e,
                    eye,
                    players,
                    reach,
                );
                (out.check == dwell_world::region::RegionCheck::Ok).then_some(out.changes)
            }
            _ => None,
        }
    }

    /// Our body as drag building keeps clear of it (docs/ITEMS.md §5): where it is, and back
    /// along its motion by `buildModes.clearance` and its input buffer's two ticks
    /// (`build_drag::lagged_offset`), not the round trip: the server holds a piece its own view of
    /// the body is still in until that view catches up (`EDIT_HOLD_TICKS`), and keeping clear of
    /// where it may see the body narrowed a jump's moment clear of the next piece to nothing at a
    /// few frames a second (BUGS.md 30).
    fn drag_body(&self) -> Vec<dwell_world::edit::EditCapsule> {
        let Some(c) = self.own_capsule() else {
            return Vec::new();
        };
        let v = self.predictor.velocity();
        let back = dwell_game::build_drag::lagged_offset(
            [f64::from(v.x), f64::from(v.y), f64::from(v.z)],
            None,
        );
        let lagged = dwell_world::edit::EditCapsule {
            center: std::array::from_fn(|i| c.center[i] + back[i]),
            ..c
        };
        vec![c, lagged]
    }

    /// Our predicted body as an edit capsule (the server builds its players' the same way), or
    /// none while we have no body (dead, not yet placed).
    fn own_capsule(&self) -> Option<dwell_world::edit::EditCapsule> {
        if !self.current.active || self.dead {
            return None;
        }
        let half = self.predictor.half_height();
        let radius = self.predictor.config().body.radius;
        Some(dwell_world::edit::EditCapsule {
            center: self.predictor.position().to_array(),
            radius,
            half_cylinder: half - radius,
        })
    }

    /// The server is done with these edits (`(id, refused)`: from its `EditAck` through the session,
    /// or refused unsent): their cells show the server's state again (§6.5).
    pub fn on_edit_results(&mut self, results: &[(u32, bool)]) {
        if !results.is_empty() {
            self.prediction.resolve(results, &mut self.streamer);
        }
        // A drag fill the server refused, or took but left as it was (it skips a cell its view of
        // the body is in without refusing; the prediction then shows the server's state): a line
        // goes back to it; any other goes again while its cells are still free (docs/ITEMS.md §5,
        // `BuildDrag::answered`). One put into water comes back flooded (BUGS.md 37).
        for &(id, refused) in results {
            let Some((fill, tries)) = self.drag_sent.remove(&id) else {
                continue;
            };
            let store = self.streamer.store_mut();
            let placed =
                dwell_game::build_drag::fill_placed(&fill, |[x, y, z]| store.get_voxel(x, y, z));
            let me = self.drag_body();
            let settled = self.drag_sent.is_empty();
            let mut world = DragCells {
                world: self.streamer.store_mut(),
                me,
                settled,
            };
            self.drag.answered(fill, tries, refused, placed, &mut world);
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

    /// The listener's surroundings for the ambience beds (AUDIO.md §2c), in
    /// `dwell_game::ambience::INPUTS` order.
    pub fn ambience(&self) -> [f64; 9] {
        self.ambience.to_array()
    }

    /// The camera at the last draw, for the audio listener: eye, forward, up.
    pub fn listener(&self) -> [f64; 9] {
        self.listener
    }

    /// A modification's changes with what each cell held before (read before it is applied).
    /// The blasts among an `Explosion` modification's changes (a lit explosive's cell turned to
    /// air): each starts its effects, and its boom plays as loud as its distance from the
    /// listener allows (`boom_level`; at the listener, so a far blast is heard past the panner's
    /// reach).
    fn blasts_from(&mut self, changes: &[CellChange], now_ms: f64) {
        use dwell_world::explosives::{EXPLOSIVES, boom_level, explosive_of};
        for c in changes {
            let Some((i, true)) = explosive_of(EXPLOSIVES, c.old) else {
                continue;
            };
            if dwell_blocks::info(c.new).solid {
                continue;
            }
            let at = dwell_world::centre_of_cell(c.cell);
            // The way out, from the cells round it before the blast clears them (this runs
            // before the modification is applied): its smoke follows it (§4).
            let store = self.streamer.store_mut();
            let open = crate::blasts::open_way(&EXPLOSIVES[i], c.cell, |w| {
                let m = store.get_voxel(w[0], w[1], w[2]);
                m != dwell_world::M.air && !dwell_blocks::info(m).liquid
            });
            self.blasts.start(at, i, now_ms / 1000.0, open);
            let d = (0..3)
                .map(|k| (at[k] - self.listener[k]).powi(2))
                .sum::<f64>()
                .sqrt();
            let level = boom_level(&EXPLOSIVES[i], d);
            if level > 0.0 {
                self.sounds
                    .push(SoundEvent::new(SoundKind::Explode, c.old, level, None));
            }
        }
    }

    /// The blasts' effects this frame (docs/EXPLOSIVES.md §4): their puffs and flash lights to the
    /// scene, and the camera's shake at `eye` (yaw, pitch degrees).
    /// The axe's chips (crate::chips) ride the same billboard pass.
    fn update_blasts(&mut self, eye: [f64; 3], now_ms: f64) -> (CameraOffset, (f64, f64)) {
        let sway = self.update_swing(now_ms);
        (sway, self.update_blast_effects(eye, now_ms))
    }

    /// The view drawn from the aim's pose this frame: the swing's sway (with the Camera sway
    /// setting) and a blast's shake added (crate::chop::swayed_view); the aim keeps `aim`.
    fn view_pose(
        &self,
        aim: crate::chop::ViewPose,
        effects: (CameraOffset, (f64, f64)),
        face: FaceFrame,
    ) -> crate::chop::ViewPose {
        crate::chop::swayed_view(aim, effects.0, self.camera_sway, effects.1, face)
    }

    /// The swing under way this frame (crate::chop `SwingRun`): the arms held through the
    /// hit-stop and pulled back, the impact's effects at its moment, and the view's sway.
    fn update_swing(&mut self, now_ms: f64) -> CameraOffset {
        self.land_hits(now_ms);
        // The axe put away mid-swing: the swing stops, landing nothing (the server's too).
        if self
            .chop
            .run
            .is_some_and(|r| !r.impacted && self.held_tool().is_none_or(|t| t.name != r.tool.name))
        {
            self.chop.run = None;
            self.chop.cleared.clear();
            self.bodies.drive_arms(None);
        }
        let Some(mut run) = self.chop.run else {
            return CameraOffset::ZERO;
        };
        if run.done(now_ms) {
            self.chop.run = None;
            self.bodies.drive_arms(None);
            return CameraOffset::ZERO;
        }
        let t = run.elapsed(now_ms);
        self.bodies.drive_arms(Some(run.clip_time(t)));
        if run.take_clear(now_ms) {
            self.swing_clear(run.tool, now_ms);
        }
        if let Some(hit) = run.take_impact(now_ms) {
            self.swing_impact(run.tool, hit, now_ms);
        }
        self.chop.run = Some(run);
        run.camera(t)
    }

    /// Our tool hits whose swing's contact has come (WORK_QUEUE item 25): a hit short of breaking
    /// sounds the material's hit and bursts its chips at the struck point, a one-hit break its
    /// break sound; a hit whose tool was put away first shows nothing.
    fn land_hits(&mut self, now_ms: f64) {
        let held = self.held_break_item();
        for h in self.pending_hits.take_due(now_ms, held) {
            if h.breaks {
                self.sounds.push(SoundEvent::new(
                    SoundKind::Break,
                    h.block,
                    1.0,
                    Some(dwell_world::centre_of_cell(h.cell)),
                ));
                continue;
            }
            self.sounds
                .push(SoundEvent::new(SoundKind::Hit, h.block, 1.0, h.at));
            if let Some(at) = h.at {
                self.hit_chips(h.block, h.cell, at, now_ms);
            }
        }
    }

    /// What a swing's impact shows: into a wood its chips, its hit's sound by how far the wood is
    /// cut, the tree's leaves shaken down and its sway; off anything else a clank and a jolt, no
    /// effect (the server cuts; this is the feel).
    fn swing_impact(
        &mut self,
        tool: &'static dwell_protocol::chopping::ToolDef,
        hit: crate::chop::Impact,
        now_ms: f64,
    ) {
        let [x, y, z] = hit.cell;
        let block = self.voxel(x, y, z);
        let name = dwell_blocks::block_of(block).name;
        let wood = dwell_protocol::chopping::WOODS
            .iter()
            .position(|w| w.block == name);
        let at = Some(hit.point);
        let event = |n: &str| SoundKind::ALL.into_iter().find(|k| k.name() == n);
        match wood.filter(|_| hit.wood) {
            Some(w) => {
                let def = &dwell_protocol::chopping::WOODS[w];
                self.chips_burst(block, hit.face, hit.point.map(|v| v as f32), now_ms);
                let grain = crate::chop::grain_of(block);
                let cross = self
                    .chop
                    .cuts
                    .get(&hit.cell)
                    .map_or(0.0, |c| c.cross(grain) / def.fell_fraction);
                if let Some(k) = def.sound_at(cross).and_then(event) {
                    self.sounds.push(SoundEvent::new(k, block, 1.0, at));
                }
                let store = self.streamer.store_mut();
                let cells =
                    crate::chips::leaf_cells(hit.cell, w, |c| store.get_voxel(c[0], c[1], c[2]));
                self.chips
                    .shake_leaves(w, &cells, now_ms / 1000.0, now_ms as u32 ^ 0x5bd1_e995);
                if grain == 1 {
                    self.lean_at = Some((hit.cell, now_ms / 1000.0));
                }
            }
            None => {
                if let Some(k) = tool.swing.bounce.and_then(event) {
                    self.sounds.push(SoundEvent::new(k, block, 1.0, at));
                }
            }
        }
    }

    /// What a swing going through the cells it clears (its tool's `clears`: leaves) shows: the
    /// tool's rustle once, at the first, and its `rustleLeaves` falling from each (the server
    /// breaks them; no hit-stop, no bounce).
    fn swing_clear(&mut self, tool: &'static dwell_protocol::chopping::ToolDef, now_ms: f64) {
        let cells = std::mem::take(&mut self.chop.cleared);
        let Some(&first) = cells.first() else {
            return;
        };
        let event = |n: &str| SoundKind::ALL.into_iter().find(|k| k.name() == n);
        let block = self.voxel(first[0], first[1], first[2]);
        if let Some(k) = tool.swing.rustle.and_then(event) {
            let at = dwell_world::centre_of_cell(first);
            self.sounds.push(SoundEvent::new(k, block, 1.0, Some(at)));
        }
        for (i, &c) in cells.iter().enumerate() {
            let m = self.voxel(c[0], c[1], c[2]);
            let colour = crate::chips::leaf_colour_of(dwell_blocks::block_of(m).name);
            let seed = (now_ms as u32) ^ (i as u32).wrapping_mul(0x9e37_79b9);
            self.chips
                .fall_leaves(colour, c, tool.swing.rustle_leaves, now_ms / 1000.0, seed);
        }
    }

    /// The tree being cut leans toward its notch as it nears the fall (the wood's `sway`), with
    /// a shiver after each hit: none when no standing wood near was cut lately.
    pub fn tree_lean(&mut self, now_ms: f64) -> Option<crate::chop::TreeLean> {
        let (cell, at) = self.lean_at?;
        let cut = *self.chop.cuts.get(&cell)?;
        let m = self.voxel(cell[0], cell[1], cell[2]);
        let w = dwell_protocol::chopping::wood(dwell_blocks::block_of(m).name)?;
        crate::chop::TreeLean::of(cell, &cut, w, now_ms / 1000.0 - at)
    }

    /// The far herds' heads (docs/ECOLOGY.md §11), laid out again on a new snapshot; a head's
    /// ground is the loaded terrain's (one past the loaded chunks waits for them).
    fn far_herd_puffs(&mut self, eye: [f64; 3]) -> Vec<crate::blasts::Puff> {
        if self.chips.cpu {
            return Vec::new();
        }
        if self.far_stale
            && let Some(snapshot) = self.ecology.as_ref()
        {
            // Only the groups that could show (within their fade's end, on the eye's face): the
            // ground scan is per head.
            let face = dwell_world::FaceFrame::at_height(eye[1]);
            let local = face.point(eye);
            let mut near = snapshot.clone();
            near.groups.retain(|g| {
                let end = crate::far_herds::FAR_LOOKS
                    .iter()
                    .find(|l| l.species == g.species)
                    .map_or(0.0, |l| l.fade_end_m);
                u8::from(face.is_b()) == g.face
                    && (f64::from(g.x) - local[0]).hypot(f64::from(g.z) - local[2]) < end + 32.0
            });
            let world = self.streamer.store_mut();
            self.far_heads =
                crate::far_herds::lay_out(&near, crate::far_herds::FAR_LOOKS, &mut |f, x, z| {
                    crate::bodies::ground_at(world, f, [x, local[1], z], 48.0)
                });
            self.far_stale = false;
        }
        crate::far_herds::puffs(&self.far_heads, eye, false)
    }

    /// The steam plumes over the world's lava vents, every few seconds: over each vent column's
    /// top molten cell, gone with the lava (crate::vent_plumes).
    fn update_vent_plumes(&mut self, now_s: f64) {
        if !self.vent_plumes.due(now_s) {
            return;
        }
        let world = self.streamer.store_mut();
        let plumes = self.vent_plumes.plumes(|f, c| {
            crate::vent_plumes::top_lava_in(
                f,
                c,
                |cell| world.get_voxel_if_loaded(cell),
                crate::vent_plumes::is_lava,
            )
        });
        self.scene
            .borrow_mut()
            .set_plumes(crate::vent_plumes::PREFIX, plumes);
    }

    fn update_blast_effects(&mut self, eye: [f64; 3], now_ms: f64) -> (f64, f64) {
        let now = now_ms / 1000.0;
        self.blasts.prune(now);
        self.chips.prune(now);
        self.steam.prune(now);
        self.update_vent_plumes(now);
        let mut extra = self.chips.puffs(now);
        extra.extend(self.steam.puffs(now));
        extra.extend(self.far_herd_puffs(eye));
        if self.blasts.list.is_empty() {
            self.scene.borrow_mut().set_blasts(extra, Vec::new());
            return (0.0, 0.0);
        }
        let mut puffs = self.blasts.puffs(now);
        puffs.extend(extra);
        let lights = self.blasts.lights(now);
        self.scene.borrow_mut().set_blasts(puffs, lights);
        self.blasts.shake(eye, now)
    }

    /// A hit's chips out of `cell` (the block `block`) at the struck point `at` (world), when its
    /// data has `damage.chips` (docs/BLOCK_DAMAGE.md §6).
    fn hit_chips(&mut self, block: StateId, cell: [i32; 3], at: [f64; 3], now_ms: f64) {
        if dwell_blocks::try_info(block).is_none() {
            return;
        }
        if let Some(look) = crate::chips::hit_chips_of(dwell_blocks::block_of(block).name) {
            let n = crate::chips::struck_normal(cell, at);
            self.chips.hit_burst(look, n, at, now_ms / 1000.0);
        }
    }

    /// A swing's chips out of `face` at `point` of the wood `block` (the server cuts; these show).
    fn chips_burst(&mut self, block: StateId, face: u8, point: [f32; 3], now_ms: f64) {
        let name = dwell_blocks::block_of(block).name;
        if let Some(wood) = dwell_protocol::chopping::WOODS
            .iter()
            .position(|w| w.block == name)
        {
            self.chips.burst(
                wood,
                face,
                point.map(f64::from),
                self.chop.vertical,
                self.view_dir,
                now_ms / 1000.0,
            );
        }
    }

    /// Which blast effects show (Settings → Explosions), and whether the adapter is a CPU's.
    pub fn set_blast_settings(&mut self, s: crate::blasts::BlastSettings) {
        self.blasts.settings = s;
        self.chips.cpu = s.cpu;
        self.steam.cpu = s.cpu;
    }

    /// The blasts whose effects are still going (the dev hook): each one's place and age (s).
    pub fn blasts(&self, now_ms: f64) -> Vec<([f64; 3], f64)> {
        self.blasts
            .list
            .iter()
            .map(|b| (b.at, now_ms / 1000.0 - b.start_s))
            .collect()
    }

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

    /// The local player brushing through foliage and stepping onto ground items this tick
    /// (AUDIO.md §5): a rustle by speed, a stick's snap.
    fn foliage_and_ground_sounds(&mut self, c: &ClientState, grounded: bool) {
        let feet = c.feet();
        let foliage = {
            let store = self.streamer.store_mut();
            dwell_game::sound::foliage_at(
                feet,
                c.face,
                self.current.standing_height,
                |[x, y, z]| store.get_voxel(x, y, z),
            )
        };
        let speed = c.velocity.iter().map(|v| v * v).sum::<f64>().sqrt();
        // The open cell the feet stand in (just above the ground's top), while on the ground.
        let cell = grounded.then(|| dwell_game::sound::feet_cell(feet, c.face));
        let mut contact = self.contact_sounds;
        let mut out = Vec::new();
        contact.update(
            foliage,
            speed,
            1.0 / SIM_HZ as f64,
            cell,
            None,
            |cell| self.ground_step_at(cell),
            &mut out,
        );
        self.contact_sounds = contact;
        self.sounds.extend(out);
    }

    /// The point and step sound of the ground item in `cell`, if it has one.
    fn ground_step_at(&self, cell: [i32; 3]) -> Option<([f64; 3], SoundKind)> {
        self.ground_item_sound(cell, |k| k.step)
            .map(|(it, ev)| (it.point(), ev))
    }

    /// Other players brushing through foliage and stepping onto ground items (AUDIO.md §2e), from
    /// their drawn (interpolated) positions: no wire message of their own.
    fn remote_contact_sounds(&mut self, views: &[dwell_game::remotes::RemoteView], dt: f64) {
        let height = self.current.standing_height;
        let samples: Vec<_> = views
            .iter()
            .filter(|v| !v.is_dead())
            .map(|v| dwell_game::sound::ContactSample {
                player_id: v.player_id,
                feet: v.feet,
                face: v.face(),
                speed: v
                    .velocity
                    .map(f64::from)
                    .iter()
                    .map(|v| v * v)
                    .sum::<f64>()
                    .sqrt(),
                grounded: on_ground(v.state),
                height,
            })
            .collect();
        let local = self.player_id;
        let mut contacts = std::mem::take(&mut self.remote_contacts);
        let mut out = Vec::new();
        {
            let world = self.streamer.store();
            contacts.update(
                local,
                &samples,
                dt,
                |c| world.get_voxel_if_loaded(c),
                |cell| self.ground_step_at(cell),
                &mut out,
            );
        }
        self.remote_contacts = contacts;
        self.sounds.extend(out);
    }

    /// The ground item in `cell` (not gone) and its sound event `which` (its `step` or `hit`).
    fn ground_item_sound(
        &self,
        cell: [i32; 3],
        which: impl Fn(&dwell_world::ground::GroundKind) -> Option<&'static str>,
    ) -> Option<(dwell_world::ground::Placed, SoundKind)> {
        let (seed, gone) = self.streamer.ground()?;
        if gone.cells.contains(&cell) {
            return None;
        }
        let world = self.streamer.store();
        let mut get = |c: [i32; 3], generated: bool| {
            if generated && let Some(m) = self.streamer.generated_voxel(c) {
                return Some(m);
            }
            world.find(dwell_world::chunk_of(c[0], c[1], c[2]))?;
            Some(world.get_voxel_if_loaded(c))
        };
        let density = |k: usize, cell: [i32; 3]| self.ground_density_at(k, cell);
        let rules = dwell_world::ground::Rules::shipped();
        let rules = if self.ground_density.is_empty() {
            rules
        } else {
            rules.with_density(&density)
        };
        let it = rules.item_over(&mut get, *seed, cell)?;
        let name = which(rules.kind(it.kind)?)?;
        let ev = SoundKind::ALL.into_iter().find(|k| k.name() == name)?;
        Some((it, ev))
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
        let height = if crouched {
            self.current.crouch_height
        } else {
            self.current.standing_height
        };
        let store = self.streamer.store_mut();
        let Surroundings {
            ground,
            body,
            walls,
            ceiling,
        } = Surroundings::read(feet, face, self.current.radius, height, |[x, y, z]| {
            store.get_voxel(x, y, z)
        });
        let liquid = |id: u16| id != 0 && dwell_blocks::info(id).liquid;
        MoverSample {
            feet,
            velocity,
            state,
            grounded,
            crouched,
            face,
            submerged: submerged.unwrap_or(if liquid(body) { 1.0 } else { 0.0 }),
            carry: [0.0; 3],
            ground,
            body,
            walls,
            ceiling,
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

    /// The predictor's player (the server's, while no tick runs) is more than a snap away from
    /// the state the game shows.
    fn moved_by_server(&self) -> bool {
        let p = self.predictor.position();
        let d = (0..3)
            .map(|k| (p[k] - self.current.position[k]).powi(2))
            .sum::<f64>()
            .sqrt();
        d > f64::from(dwell_protocol::players::RECONCILE_SNAP_DISTANCE)
    }

    /// Prediction waits until the terrain around the player has arrived (collision needs it), and
    /// while the loading screen shows.
    fn terrain_ready(&self) -> bool {
        self.current.active
            && self.loading.is_open()
            && self.streamer.ready_around(self.current.position)
    }

    /// One animation frame: run due ticks, then draw. `now` is a clock in ms for budgets.
    pub fn frame(
        &mut self,
        now_ms: f64,
        input: &mut KeyboardMouseInput,
        touch: Option<&TouchState>,
        now: &mut dyn FnMut() -> f64,
    ) {
        self.frame_start_ms = now();
        input.brushes = self.brush.available;
        input.brush_active = self.brush.active();
        // Queued region edits (a brush stroke) as the server's cell rate allows.
        self.pump_region(now_ms);
        self.expire_currents(now_ms);
        // Chunks meshed from now on leave their water's tops to the heightfield where it draws.
        let water_tops = !self.scene.borrow().draws_heightfield();
        self.pool.borrow_mut().water_tops = water_tops;
        let cards = self.scene.borrow().draws_cards();
        self.pool.borrow_mut().cards = cards;
        let done = telemetry::span("jobs.poll", || self.pool.borrow_mut().poll());
        telemetry::add("jobs.done", done.len() as f64);
        telemetry::span("jobs.apply", || self.on_jobs(done));
        self.update_loading(now_ms);
        let elapsed = self.last_frame_ms.map_or(0.0, |l| now_ms - l);
        self.last_frame_ms = Some(now_ms);
        self.accumulator =
            (self.accumulator + elapsed * self.tick_rate).min(MAX_TICKS_PER_FRAME * TICK_MS);
        let cells = self.scene.borrow().collision_cells_per_tick();
        self.predictor.set_collision_cells_per_tick(cells);
        let t0 = telemetry::now_ms();
        while self.accumulator >= TICK_MS {
            self.accumulator -= TICK_MS;
            self.tick(input, touch);
            let predictor = &self.predictor;
            self.pieces.tick_present(|id| predictor.piece_pose(id));
            self.drag_tick(input, now_ms - self.accumulator / self.tick_rate);
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
        // The server is behind: let it drain our queued inputs first (hysteresis, BUFFER_HOLD).
        if self.input_buffer >= BUFFER_HOLD {
            self.holding = true;
        } else if self.input_buffer <= BUFFER_RESUME {
            self.holding = false;
        }
        if self.holding || self.predictor.unacked() >= UNACKED_HOLD {
            return;
        }
        let frame = quantize_input(&input.sample(touch), self.predictor.next_seq());
        input.took();
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
        let mut sample = self.mover_sample(
            c.feet(),
            c.velocity,
            c.state,
            c.controller_flags & controller_flags::GROUNDED != 0,
            c.controller_flags & controller_flags::CROUCHING != 0,
            c.face,
            Some(c.submerged),
            1.0 / SIM_HZ as f64,
        );
        sample.carry = c.carry;
        self.mover_sounds.update(&sample, None, &mut self.sounds);
        self.foliage_and_ground_sounds(&c, sample.grounded);
        // Across the midplane the body turns over and the look goes on across: the heading kept,
        // the pitch mirrored, the camera pointing the same way and rolling upright (face_flip.rs).
        (input.look.yaw, input.look.pitch) =
            self.flip
                .tick(self.current.face, input.look.yaw, input.look.pitch);
        self.eye
            .tick(&self.current.eye_state(), 1.0 / SIM_HZ as f64);
        record_tick(&self.current, &frame, self.eye.draw(1.0));
        input.look.face = self.current.face;
        // The camera turns with rotating ground (PPC yawDelta).
        input.look.yaw += self.current.platform_yaw_delta;
    }

    /// A water chunk's currents from the server (docs/WATER_SURFACE_GRID.md §2c): the swimmers'
    /// prediction and the floating bodies' stand-ins read them from the world.
    fn on_currents(&mut self, c: &dwell_protocol::WaterCurrents, now_ms: f64) {
        let key = (c.face_b, c.cx, c.cz);
        let still = c.cells.iter().all(|q| *q == [0, 0]);
        let cells = (!still).then(|| {
            let mut out: dwell_world::currents::ChunkCurrents =
                Box::new([[0.0; 2]; dwell_world::currents::CA]);
            for (i, v) in out.iter_mut().enumerate() {
                *v = c.current(i);
            }
            out
        });
        self.streamer.store_mut().currents.set(key, cells);
        if still {
            self.currents_seen.remove(&key);
        } else {
            self.currents_seen.insert(key, now_ms);
        }
    }

    /// The water's current (m/s along x and z) at world point `p`, as this client has it.
    pub fn current_at(&mut self, p: [f64; 3]) -> [f32; 2] {
        self.streamer.store_mut().currents.at(p)
    }

    /// The flecks drifting near the eye now.
    pub fn fleck_count(&self) -> usize {
        self.flecks.list.len()
    }

    /// Where the flecks near the eye are (world m).
    pub fn fleck_positions(&self) -> Vec<[f64; 3]> {
        self.flecks.list.iter().map(|p| p.at).collect()
    }

    /// The Drifting flecks setting (docs/WATER_SURFACE_GRID.md §2c).
    pub fn set_flow_particles(&mut self, on: bool) {
        self.flow_particles = on;
    }

    /// The Waterfalls setting (docs/WATER_SURFACE_GRID.md §2e): the falls' sheets, foam and spray.
    pub fn set_falls_shown(&mut self, on: bool) {
        self.falls_shown = on;
    }

    /// The waterfalls near the eye now (as last found).
    pub fn falls(&self) -> &[dwell_world::falls::Fall] {
        &self.falls
    }

    /// The renderer's flow window around `eye` (render/flow.rs), built again when the current table
    /// changed or the eye is over another column.
    fn update_flow(&mut self, eye: [f64; 3]) {
        let f = FaceFrame::at_height(eye[1]);
        let (cx, cz) = (eye[0].floor() as i32, eye[2].floor() as i32);
        let currents = &self.streamer.store().currents;
        let key = (currents.revision(), f.is_b(), cx, cz);
        if self.flow_sent == Some(key) {
            return;
        }
        self.flow_sent = Some(key);
        let t = dwell_look::waterflow::WATER_FLOW;
        let texels = currents.window(f, cx, cz, t.window_columns as i32, t.range_ms as f32);
        self.scene.borrow_mut().set_water_flow(texels);
    }

    /// Forgets the currents of water chunks the server stopped sending (`CURRENTS_KEEP_MS`).
    fn expire_currents(&mut self, now_ms: f64) {
        let old: Vec<_> = self
            .currents_seen
            .iter()
            .filter(|&(_, &t)| now_ms - t > dwell_protocol::CURRENTS_KEEP_MS)
            .map(|(&k, _)| k)
            .collect();
        for k in old {
            self.currents_seen.remove(&k);
            self.streamer.store_mut().currents.set(k, None);
        }
    }

    /// A held Place's stroke at this tick's body and eye, at `tick_ms` (docs/BUGS.md 30): a slow
    /// device's frame covers many ticks, and a jump's moment clear of the next piece (towering) can
    /// fall between two frames; the frame's own `drag_build` goes on at the drawn eye.
    fn drag_tick(&mut self, input: &KeyboardMouseInput, tick_ms: f64) {
        let c = self.current;
        if !(self.drag.active() || self.drag.waiting().is_some())
            || !c.active
            || self.dead
            || !self.terrain_ready()
        {
            return;
        }
        let eye = [c.position[0], self.eye.draw(1.0), c.position[2]];
        let dir = view_direction(input.look.yaw, input.look.pitch, c.face);
        self.drag_build(Some(eye), dir, tick_ms);
    }

    fn draw(&mut self, now_ms: f64, input: &KeyboardMouseInput, now: &mut dyn FnMut() -> f64) {
        let c = self.current;
        let face = c.face;
        let dt = self.last_draw_ms.map_or(0.0, |l| (now_ms - l) / 1000.0);
        let roll = self.flip.draw(dt);
        self.last_draw_ms = Some(now_ms);
        let p = self.previous.unwrap_or(c);
        let alpha = self.accumulator / TICK_MS;
        let center: [f64; 3] = std::array::from_fn(|i| {
            let a = p.position[i] + p.render_offset[i];
            a + (c.position[i] + c.render_offset[i] - a) * alpha
        });
        let second = self.respawn_preload();
        self.streamer.set_second_centre(second);
        let around = if c.active {
            center
        } else if self.dead {
            self.death_feet // the view around the body
        } else {
            [0.5, 64.0, 0.5]
        };
        // The pool starts the jobs near the player first; with a second centre (the respawn
        // preload) the streamer's and the LOD's own order rules, as queued.
        {
            let mut pool = self.pool.borrow_mut();
            pool.set_focus(second.is_none().then_some(around));
            pool.set_frame_ms(dt * 1000.0);
        }
        // What is left of the frame's loading budget after the jobs taken in and the ticks.
        let budget = StreamBudget::for_frame(now() - self.frame_start_ms, dt * 1000.0);
        telemetry::span("mesh_dirty", || {
            self.streamer.mesh_dirty(around, budget, &mut *now);
        });

        // Remote players, interpolated.
        let views = self.remotes.sample(now_ms);
        let mut drawn = HashSet::default();
        for v in &views {
            drawn.insert(v.player_id);
            // The moving body under its feet, if any: its motion is read over it (standing still
            // on a raft is idle; owner, 2026-11-09).
            let carry = if v.flags & player_flags::GROUNDED != 0 && !v.is_dead() {
                self.pieces
                    .ground_velocity(DVec3::from(v.feet), v.face(), now_ms)
                    .map_or([0.0; 3], |c| c.to_array())
            } else {
                [0.0; 3]
            };
            if !v.is_dead() {
                let grounded = on_ground(v.state);
                let face = v.face();
                let mut sample = self.mover_sample(
                    v.feet,
                    v.velocity.map(f64::from),
                    v.state,
                    grounded,
                    v.is_crouched(),
                    face,
                    None,
                    dt,
                );
                sample.carry = carry;
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
            let held = crate::bodies::with_tool(held, self.remote_tools.get(&v.player_id).copied());
            let world = self.streamer.store_mut();
            let reach = self.current.max_step_lift;
            let ridden = crate::bodies::over_its_ground(v, carry);
            let mut view = self.bodies.remote(&ridden, held, dt, &mut |f, p| {
                crate::bodies::ground_at(world, f, p, reach)
            });
            view.light = self.body_light(u32::from(v.player_id), view.feet, view.face, dt);
            self.scene
                .borrow_mut()
                .set_player(u32::from(v.player_id), Some(view));
        }
        self.remote_contact_sounds(&views, dt);
        for id in self.remotes.ids() {
            if !drawn.contains(&id) {
                self.scene.borrow_mut().set_player(u32::from(id), None);
            }
        }
        // The creatures, smoothed toward the server's points, lit where they stand (ADR 0073).
        let creatures: Vec<_> = self
            .creatures
            .advance(dt)
            .into_iter()
            .map(|v| {
                let light = self.body_light(0x4000_0000 | v.id, v.feet, v.face, dt);
                (v, light)
            })
            .collect();
        // Their footsteps, startles and calls (docs/AUDIO.md §2d), at the listener.
        let samples: Vec<_> = creatures
            .iter()
            .map(|(v, _)| dwell_game::sound::CreatureSample {
                id: v.id,
                model: v.model,
                feet: v.feet,
                running: v.role == "sprint",
            })
            .collect();
        let ear = [self.listener[0], self.listener[1], self.listener[2]];
        self.creature_sounds
            .update(ear, &samples, dt, &mut self.sounds);
        self.scene.borrow_mut().set_creatures(creatures);
        if let Some(mut p) = self.puppets {
            // Holding the hotbar's: a tool (the axe) as your hand shows it, else its block.
            let tool = (p.held == Puppets::HOTBAR)
                .then(|| self.held())
                .filter(|h| matches!(h, Held::Tool(_) | Held::Model { .. }));
            if p.held == Puppets::HOTBAR {
                p.held = self.selected_material();
            }
            let held_swing = self.bodies.swing_held();
            for mut v in p.views(now_ms / 1000.0) {
                drawn.insert(v.player_id);
                // A held swing (the bench's `clip-strip`): they stand still facing +Z, so a
                // placed camera frames them from behind or the side.
                if held_swing {
                    use dwell_protocol::player_flags as pf;
                    (v.yaw, v.velocity, v.state) = (0.0, [0.0; 3], PlayerState::Idle);
                    let face = dwell_physics::net::face_of_flags(v.flags, pf::FACE_B);
                    v.flags =
                        (v.flags & pf::GROUNDED) | dwell_physics::net::face_bit(face, pf::FACE_B);
                    v.feet[1] = p.origin[1];
                }
                // Holding a tool, they swing it (a break swing every 0.8 s): the swing clips seen
                // on a body (docs/DEV_TOOLS.md `puppets`).
                if tool.is_some() {
                    use dwell_protocol::swing_flags as sf;
                    v.swing = sf::BREAK | ((now_ms / 800.0) as u64 as u8 & sf::COUNT_MASK);
                    // The chop your hand would swing, as another player's arrives (`SwingHello`).
                    v.swing_kind = if self.chop.vertical {
                        dwell_protocol::SwingKind::ChopVertical
                    } else {
                        dwell_protocol::SwingKind::ChopLevel
                    } as u8;
                }
                let held = tool.unwrap_or_else(|| crate::bodies::held_from_wire(v.held, Some));
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
        // Other players' hits and health events whose moment is drawn now, where they are.
        let sounds = &mut self.sounds;
        self.pending_sounds.retain(|(t, e)| {
            let wait = *t > now_ms;
            if !wait {
                sounds.push(*e);
            }
            wait
        });
        let mut due = std::mem::take(&mut self.pending_player_events);
        due.retain(|(t, e)| {
            if *t > now_ms {
                return true;
            }
            let at = views
                .iter()
                .find(|v| v.player_id == e.player_id)
                .map(|v| v.feet)
                .or((e.kind == PlayerEventKind::Respawn).then_some(e.position));
            if let Some(at) = at {
                self.sounds
                    .extend(player_event_sound(e.kind, e.amount, Some(at)));
            }
            false
        });
        self.pending_player_events = due;
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
            // On the face the body lies on, not the controller's (it has no body while dead).
            let cam = death_camera(self.death_feet, yaw);
            let mut view = self.bodies.own_dead(self.death_feet, yaw, cam.face, dt);
            view.light = self.body_light(u32::from(self.player_id), view.feet, view.face, dt);
            let mut scene = self.scene.borrow_mut();
            scene.set_player(u32::from(self.player_id), Some(view));
            scene.set_arms(None);
            scene.set_own_shadow(None);
            scene.set_own_body(None);
            camera_eye = cam.eye;
            drop(scene);
            // Third person: a blast's shake, no swing's sway (the swing is the body's).
            let (_, (sy, sp)) = self.update_blasts(cam.eye, now_ms);
            self.scene
                .borrow_mut()
                .set_camera(cam.eye, yaw + sy, cam.pitch + sp, cam.face, 0.0);
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
            // A blast's shake and the swing's sway turn the view drawn, never the aim: the
            // targeting below, the listener and the facing keep `eye`, `yaw` and `pitch`.
            let effects = self.update_blasts(eye, now_ms);
            let aim = crate::chop::ViewPose {
                eye,
                yaw,
                pitch,
                roll,
            };
            let v = self.view_pose(aim, effects, face);
            self.scene
                .borrow_mut()
                .set_camera(v.eye, v.yaw, v.pitch, face, v.roll);
            self.listener = listener(eye, yaw, pitch, face);
            let ready = self.terrain_ready();
            self.target(ready.then_some(eye), input, now_ms);
            self.update_motion(eye, dt);
            // Your arms in their own pass (CHARACTERS.md §5): not while zoomed in the spyglass, in
            // a placed camera, or with the setting off.
            let move_input = crate::bodies::local_input(
                c.state,
                c.velocity,
                c.carry,
                c.controller_flags,
                c.face,
            );
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
        // Under the surface, or above it within the waterline's reach (WATER_RENDERING.md §10).
        let eye_fluid = dwell_world::fluid::eye_fluid(
            camera_eye,
            dwell_look::underwater::line_reach_m(),
            |x, y, z| store.get_voxel(x, y, z),
        );
        self.scene.borrow_mut().set_eye_fluid(eye_fluid);
        let lean = self.tree_lean(now_ms);
        self.scene.borrow_mut().set_tree_lean(lean);
        telemetry::span("lod", || self.update_lod(&c, now_ms, input));
        self.probe_stream(c.feet());
        if let Some(lod) = &self.lod {
            // The cross-fade's extra draws this frame (dwell-lod fade.rs): the bench reads them.
            let f = lod.debug_stats().fades;
            telemetry::add("lod.fading", f.swaps as f64);
            telemetry::add("lod.retiring", f.retiring as f64);
        }
        // The placed torches' flames near the eye (docs/LIGHTS.md §6).
        let flames = crate::render::flames::placed_flames(self.streamer.store(), camera_eye);
        self.scene.borrow_mut().set_placed_flames(flames);
        // The portals' motes near the eye (BIFACIAL_WORLD.md §12.1).
        let motes =
            crate::render::flames::portals_near(&self.portals.0, self.portals.1, camera_eye);
        self.scene.borrow_mut().set_portal_motes(motes);
        // The falling pieces, where they are now (interpolated).
        // And the settled ones gliding into place, handed to the terrain once it draws them.
        let step = self
            .glides
            .step(now_ms, |cells| self.streamer.cells_drawn(cells));
        self.streamer.hide_cells(&step.show, false);
        for id in step.remove {
            self.scene.borrow_mut().set_piece(id, None);
        }
        // Pieces near the player drawn at the present it is drawn in (docs/BUILT_BODIES.md §6).
        // Between the stand-ins' last two ticks, as the player is drawn.
        let me = self.predictor.active().then(|| self.predictor.position());
        self.pieces.set_present(self.accumulator / TICK_MS, me, dt);
        let mut poses = self.pieces.poses(now_ms);
        poses.extend(self.glides.poses(now_ms));
        poses.extend(self.loose.poses(now_ms));
        self.scene.borrow_mut().show_pieces(poses);
        // The look's time (render_clock.rs): the sky and the flecks stand still while it is paused.
        let look_dt = if crate::render_clock::paused() {
            0.0
        } else {
            dt
        };
        self.update_day(crate::render_clock::at(now_ms));
        self.ambient_sounds(&views, dt);
        // The placed light sources near the eye (docs/LIGHTS.md §4): Torch shadows draws the
        // nearest as shadowed lights.
        let placed = placed_lights(
            self.streamer.store(),
            camera_eye,
            dwell_look::lights::LIGHTS.shadows.placed_m,
        );
        self.scene.borrow_mut().set_placed_lights(placed);
        self.update_flow(camera_eye);
        // The flecks on moving water (the Drifting flecks setting).
        self.flecks.update(
            self.streamer.store(),
            camera_eye,
            look_dt,
            self.flow_particles,
        );
        self.scene.borrow_mut().set_flecks(self.flecks.list.clone());
        // The waterfalls near the eye (docs/WATER_SURFACE_GRID.md §2e), found again now and then.
        self.falls_age += dt;
        if self.falls_age >= dwell_look::waterfalls::WATERFALLS.refresh_s {
            self.falls_age = 0.0;
            self.falls = crate::telemetry::span("falls.find", || {
                dwell_world::falls::falls_near(
                    self.streamer.store(),
                    camera_eye,
                    dwell_look::waterfalls::WATERFALLS.draw_m,
                )
            });
        }
        let shown = if self.falls_shown {
            self.falls.clone()
        } else {
            Vec::new()
        };
        self.scene.borrow_mut().set_falls(shown);

        self.hud.health = self.health;
        self.hud.burn = burn_edge_strength(self.burn_until_ms, now_ms);
        self.hud.vitals = HudVitals::of(&self.predictor.controller().vitals, self.vitals_rules)
            .filter(|_| !self.dead);
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

    /// Uses the world's clock (its settings, DAY_NIGHT.md §2) for the sky.
    pub fn set_world_clock(&mut self, c: dwell_protocol::world_settings::WorldClock) {
        self.world_clock = dwell_look::daynight::Clock {
            day_s: f64::from(c.day_s),
            night_s: f64::from(c.night_s),
            start_hour: f64::from(c.start_hour),
        };
    }

    /// The day/night cycle at `now_ms`: world time from the server (none before the first
    /// snapshot: the world's start), sped up or replaced by the dev switches.
    fn update_day(&mut self, now_ms: f64) {
        let first = *self.first_draw_ms.get_or_insert(now_ms);
        let o = self.day_override;
        let world_s =
            self.clock.seconds(now_ms).unwrap_or(0.0) + (o.speed - 1.0) * (now_ms - first) / 1000.0;
        let d = dwell_look::daynight::DayNight {
            clock: self.world_clock,
            ..DAY_NIGHT
        };
        self.day = d.state(world_s, o.time, o.moon);
        self.scene
            .borrow_mut()
            .set_day(&self.day, (now_ms - first) / 1000.0);
    }

    /// Other players' hits on walls and ceilings (sound.rs `ImpactSounds`), from each snapshot's
    /// latest states: their positions are exact there (an interpolated body reaches a wall late),
    /// and they sound when the bodies are drawn at that moment.
    fn remote_impacts(&mut self, latest: &[dwell_game::remotes::RemoteView], now_ms: f64) {
        let dt = 1.0 / f64::from(dwell_protocol::SNAPSHOT_HZ);
        let due = now_ms + players::INTERP_DELAY_MS as f64;
        let mut out = Vec::new();
        for r in latest.iter().filter(|r| !r.is_dead()) {
            let grounded = on_ground(r.state);
            let sample = self.mover_sample(
                r.feet,
                r.velocity.map(f64::from),
                r.state,
                grounded,
                r.is_crouched(),
                r.face(),
                None,
                dt,
            );
            self.remote_impacts.entry(r.player_id).or_default().update(
                &sample,
                Some(r.feet),
                &mut out,
            );
        }
        self.remote_impacts
            .retain(|id, _| latest.iter().any(|r| r.player_id == *id && !r.is_dead()));
        self.pending_sounds
            .extend(out.into_iter().map(|e| (due, e)));
    }

    /// The fires near the listener and the ambience (docs/AUDIO.md §5): burning light sources
    /// placed in the world, a torch in our hand or another player's, the wind and the night.
    fn ambient_sounds(&mut self, views: &[dwell_game::remotes::RemoteView], dt: f64) {
        let eye = [self.listener[0], self.listener[1], self.listener[2]];
        let mut fires: Vec<FireSource> = self
            .streamer
            .store()
            .light_sources_near(eye, ambient::FIRE_RADIUS_M)
            .into_iter()
            .map(|(c, block)| {
                let lo = dwell_world::pos_of_cell(c);
                let pos = std::array::from_fn(|k| lo[k] + 0.5 * dwell_world::CELL[k]);
                FireSource {
                    key: fire_key(c),
                    pos,
                    at: Some(pos),
                    block,
                }
            })
            .collect();
        // A torch in a hand: ours at the listener, another player's at its hand.
        const HAND: u64 = 1 << 63;
        let held_block = |h: crate::render::characters::Held| match h {
            crate::render::characters::Held::Block(b) => b,
            _ => 0,
        };
        if self.current.active && !self.dead {
            fires.push(FireSource {
                key: HAND | u64::from(self.player_id),
                pos: eye,
                at: None,
                block: held_block(self.held()),
            });
        }
        for v in views.iter().filter(|v| !v.is_dead()) {
            let registry = &self.registry;
            let held = crate::bodies::held_from_wire(v.held, |b| match registry {
                Some(m) => m.client(b),
                None => Some(b),
            });
            let up = v.face().up();
            let pos = std::array::from_fn(|k| v.feet[k] + 1.2 * up[k]);
            fires.push(FireSource {
                key: HAND | u64::from(v.player_id),
                pos,
                at: Some(pos),
                block: held_block(held),
            });
        }
        self.fire_sounds.update(eye, &fires, dt, &mut self.sounds);
        // The ambience beds' surroundings (AUDIO.md §2c), four times a second.
        self.ambience_due_s -= dt;
        if self.ambience_due_s <= 0.0 {
            self.ambience_due_s = 0.25;
            let fire_m = fires
                .iter()
                .filter(|f| dwell_game::sound::burns(f.block))
                .map(|f| {
                    let d: [f64; 3] = std::array::from_fn(|k| f.pos[k] - eye[k]);
                    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
                })
                .fold(dwell_game::ambience::FAR_M, f64::min);
            let face = FaceFrame::at_row(dwell_world::cell_of_pos(eye)[1]);
            let store = self.streamer.store();
            let trees = &self.ambience_trees;
            let mut l = dwell_game::ambience::probe(
                eye,
                face,
                ambient::AMBIENCE_RADIUS_M,
                |c| store.get_voxel_if_loaded(c),
                |id| trees.contains(&dwell_blocks::info(id).block),
                |p| {
                    let c = store.currents.at(p);
                    f64::from(c[0].hypot(c[1]))
                },
            );
            l.day = dwell_game::ambience::daylight(self.day.sun, face);
            l.fire_m = fire_m;
            self.ambience = l;
        }
        let vitals = HudVitals::of(&self.predictor.controller().vitals, self.vitals_rules)
            .filter(|_| !self.dead);
        self.vital_sounds.update(
            vitals.is_some_and(|v| v.exhausted),
            vitals.and_then(|v| v.oxygen).map(f64::from),
            &mut self.sounds,
        );
        // Each waterfall near the listener rushes, louder with more flow and nearer.
        let falls: Vec<dwell_game::sound::FallSource> = self
            .falls
            .iter()
            .map(|f| dwell_game::sound::FallSource {
                key: (u64::from(f.x as u32) << 32)
                    ^ (u64::from(f.z as u32) << 2)
                    ^ u64::from(f.dir)
                    ^ (u64::from(f.face.is_b()) << 63),
                pos: f
                    .face
                    .point([f.lip_mid()[0], 0.5 * (f.lip + f.land), f.lip_mid()[2]]),
                discharge: f.discharge(),
            })
            .collect();
        self.fall_sounds.update(eye, &falls, dt, &mut self.sounds);
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
        let out = self.motion.frame(
            &MotionInput {
                eye,
                velocity: c.velocity,
                state: c.state,
                dt,
                first_person: c.active && !self.dead && self.view.is_none(),
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
        // Dead, the view around the body keeps loading, and the respawn's ring is readied.
        if !(c.active || self.dead) || self.lod.is_none() {
            return;
        }
        let second = self.respawn_preload().map(|s| {
            let at = ChunkCoord::from_array(s.at);
            let r = s.radius;
            let lo = dwell_world::chunk_origin_m(ChunkCoord::new(at.x - r, at.y - r, at.z - r));
            let hi = dwell_world::chunk_origin_m(ChunkCoord::new(
                at.x + r + 1,
                at.y + r + 1,
                at.z + r + 1,
            ));
            (lo, hi)
        });
        if let Some(lod) = &mut self.lod {
            lod.set_second_area(second);
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
        if !self.lod_drawn {
            let mut view = View(self.scene.clone());
            LodView::<SectionMeshes>::show_lod_sections(&mut view, &[]);
            LodView::<SectionMeshes>::show_lod_stand_ins(&mut view, &[]);
        }
        self.update_lod_map(position);
    }

    /// The LOD colour map's rings around the eye (`position`, world): starts the strips its moves
    /// uncover, within its jobs budget, and hands the rings to the renderer.
    fn update_lod_map(&mut self, position: [f64; 3]) {
        let Some(map) = &mut self.lod_map else {
            return;
        };
        let face = FaceFrame::at_height(position[1]);
        let mut pool = self.pool.borrow_mut();
        for job in map.update([position[0], position[2]], face) {
            let r = job.rect;
            pool.submit(
                Purpose::LodMap(job),
                crate::jobs::Job::LodMap {
                    face: u8::from(face.is_b()),
                    heights: false,
                    texel_m: job.texel_m as i32,
                    x0: r.x0 as i32,
                    z0: r.z0 as i32,
                    w: r.width() as i32,
                    h: r.height() as i32,
                },
            );
        }
        drop(pool);
        if self.lod_map_on {
            self.scene.borrow_mut().set_lod_map(Some(map));
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
        let dir = view_direction(input.look.yaw, input.look.pitch, face);
        self.view_dir = dir;
        self.ground_target = eye.and_then(|e| self.ground_under_crosshair(e, dir));
        // A moving body's cell nearer than the block aimed at (docs/BUILT_BODIES.md §4).
        self.body_target = eye.filter(|_| !self.brush.active()).and_then(|e| {
            let (o, d) = (DVec3::from(e), DVec3::from(dir));
            let reach = f64::from(players::REACH_DISTANCE);
            let hit = self.pieces.raycast(o, d, reach, now_ms)?;
            let block = t.and_then(|t| {
                let corner = DVec3::from(dwell_world::pos_of_cell(t.cell));
                let centre = corner + DVec3::new(0.5, 0.5 * dwell_world::CELL_H, 0.5);
                crate::pieces::ray_cells(o - centre, d, &[([0, 0, 0], 0)], reach).map(|h| h.2)
            });
            block.is_none_or(|b| hit.distance < b).then_some(hit)
        });
        self.drag_build(eye, dir, now_ms);
        self.region
            .aim(self.interaction.placement().map(|(c, _)| c));
        self.show_region_boxes();
        // The brush aims much further along the same ray (its mode's brush reach), and where the
        // ray meets nothing editable in reach, at the farthest editable cell along it: it always
        // shows where it acts (BRUSHES.md §3).
        let t = if self.brush.active() {
            let dir = view_direction(input.look.yaw, input.look.pitch, face).map(|v| v as f32);
            let reach = self.brush.reach;
            let aim = eye.and_then(|e| {
                let hit = dwell_game::block_interaction::target_hit(
                    self.streamer.store_mut(),
                    e,
                    dir,
                    reach,
                );
                // Editable: in the world, with every chunk a stroke there reads loaded.
                let extent = self.brush.read_extent(dwell_world::cell_of_pos(e));
                let streamer = &self.streamer;
                dwell_game::brush_tool::clamp_aim(e, dir, reach, hit, |c| {
                    dwell_world::region::box_in_world(c, [1, 1, 1])
                        && extent.is_none_or(|(lo, size)| {
                            streamer.box_loaded(std::array::from_fn(|i| c[i] + lo[i]), size)
                        })
                })
            });
            self.brush.aim_clamped(aim);
            aim.map(|a| a.0)
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
        // A moving body's cell aimed at: its box outlined, turned with the body.
        if let Some(b) = self.body_target {
            let half = [0.5f32, 0.5 * dwell_world::CELL_H as f32, 0.5];
            let c = [
                b.cell[0] as f32,
                b.cell[1] as f32 * dwell_world::CELL_H as f32,
                b.cell[2] as f32,
            ];
            let r = b.rotation;
            self.scene.borrow_mut().set_block_outline_turned(
                Some(b.position.to_array()),
                (
                    std::array::from_fn(|k| c[k] - half[k]),
                    std::array::from_fn(|k| c[k] + half[k]),
                ),
                [r.x as f32, r.y as f32, r.z as f32, r.w as f32],
            );
            return;
        }
        let Some(t) = t else {
            // Bridging help's ghost: where Place would build, the edge looked toward.
            let ghost = self.edge_spot().map(|(cells, _, _)| {
                let lowest = cells
                    .iter()
                    .copied()
                    .min_by_key(|c| c[1])
                    .unwrap_or(cells[0]);
                let h = dwell_world::CELL_H as f32 * cells.len() as f32;
                (dwell_world::pos_of_cell(lowest), ([0.0; 3], [1.0, h, 1.0]))
            });
            let (at, bounds) = ghost.map_or((None, FULL_CELL), |(p, b)| (Some(p), b));
            self.scene.borrow_mut().set_block_outline(at, bounds);
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
        if let Some(t) = self.held_tool() {
            let hold = t.held_for(self.chop.vertical).unwrap_or("block");
            return crate::render::characters::held_item(t.name, Some(hold));
        }
        // In a world that uses items the hand shows the held inventory slot's block, so an empty
        // slot shows an empty hand (owner, 2026-11-08: "When you run out of a block in adventure it
        // shouldn't be shown in your hand anymore"); the palette's selection otherwise.
        // An item with a shaped look (a stick, a rock) shows that look in the hand.
        if let Some(item) = self.held_look_item().or_else(|| self.palette_tool()) {
            return crate::render::characters::held_item(item, None);
        }
        let slot_empty = self.loose.uses_items() && self.held_palette_slot().is_none();
        held_of(slot_empty, self.selected_material(), self.brush.active())
    }

    /// The creative hotbar's selected tool when it does not chop (a pickaxe, a shovel: a breaking
    /// tool of shared/tools `breaking`); None in a world that uses items (its slots say).
    fn palette_tool(&self) -> Option<&'static str> {
        if self.loose.uses_items() {
            return None;
        }
        let slot = self
            .interaction
            .palette()
            .get(self.interaction.selected())?;
        slot.tool
            .filter(|t| dwell_protocol::chopping::tool(t).is_none() || !self.chop.on)
    }

    /// The held inventory slot's item when it has a shaped look (a world that uses items).
    fn held_look_item(&self) -> Option<&'static str> {
        if !self.loose.uses_items() {
            return None;
        }
        let (_, inv) = self.loose.inventory.as_ref()?;
        let s = inv.slots.get(self.inventory_slot)?.as_ref()?;
        dwell_protocol::inventory::item(&s.item)
            .filter(|d| d.look.is_some())
            .map(|d| d.name)
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
        // The tool by name, to a server that passes it on (ADR 0067: the wire above carries
        // blocks only).
        // A pickaxe or a shovel too: the server's hits to break go by it (docs/BLOCK_DAMAGE.md).
        let tool = self
            .held_tool()
            .map(|t| (t.name, self.chop.vertical))
            .or_else(|| self.held_break_item().map(|n| (n, false)));
        if self.chop.sweep && self.held_tool_sent != Some(tool.unwrap_or(("", false))) {
            let (name, vertical) = tool.unwrap_or(("", false));
            self.held_tool_sent = Some((name, vertical));
            self.outbox.borrow_mut().push(Send::Control(Message::Chop(
                dwell_protocol::ChopAction::Held {
                    tool: name.to_owned(),
                    vertical,
                },
            )));
        }
    }

    /// The palette slot that places what the held inventory slot holds (ADR 0066).
    /// The tool in the held inventory slot (ADR 0067), when the server chops.
    /// In creative, the palette's selected slot when it is a tool (items.json `tool`).
    pub fn held_tool(&self) -> Option<&'static dwell_protocol::chopping::ToolDef> {
        if !self.chop.on {
            return None;
        }
        if !self.loose.uses_items() {
            let slot = self
                .interaction
                .palette()
                .get(self.interaction.selected())?;
            return dwell_protocol::chopping::tool(slot.tool?);
        }
        let (_, inv) = self.loose.inventory.as_ref()?;
        let held = inv.slots.get(self.inventory_slot)?.as_ref()?;
        dwell_protocol::chopping::tool(&held.item)
    }

    /// The held item a break is made with, when it is a tool (`breaking.tools`: an axe, a
    /// pickaxe, a shovel; docs/BLOCK_DAMAGE.md).
    pub fn held_break_item(&self) -> Option<&'static str> {
        let name: &str = if !self.loose.uses_items() {
            self.interaction
                .palette()
                .get(self.interaction.selected())?
                .tool?
        } else {
            let (_, inv) = self.loose.inventory.as_ref()?;
            &inv.slots.get(self.inventory_slot)?.as_ref()?.item
        };
        dwell_protocol::chopping::BREAK_TOOLS
            .iter()
            .find(|t| t.item == name)
            .map(|t| t.item)
    }

    /// Where the aim meets `face` of `cell` (world-oriented), each axis in 1/255ths of the cell: a
    /// break's struck point (docs/BLOCK_DAMAGE.md).
    fn struck_point(&self, cell: [i32; 3], face: u8) -> Option<[u8; 3]> {
        let eye = self.target_eye?;
        let dir = self.view_dir;
        let centre = dwell_world::centre_of_cell(cell);
        let size = [1.0, dwell_world::CELL_H, 1.0];
        let axis = usize::from(face / 2).min(2);
        let sign = if face.is_multiple_of(2) { 1.0 } else { -1.0 };
        let plane = centre[axis] + sign * size[axis] / 2.0;
        if dir[axis].abs() < 1e-9 {
            return None;
        }
        let t = (plane - eye[axis]) / dir[axis];
        Some(std::array::from_fn(|k| {
            let p = eye[k] + dir[k] * t;
            let f = ((p - centre[k]) / size[k] + 0.5).clamp(0.0, 1.0);
            (f * 255.0).round() as u8
        }))
    }

    /// The swing hint while an axe is held.
    pub fn chop_hint(&self) -> Option<&'static str> {
        self.held_tool().map(|_| self.chop.hint())
    }

    /// The swing line the reticle shows while a tool is held: where its path will pass through
    /// the crosshair, `level` or `vertical`.
    pub fn swing_line(&self) -> Option<&'static str> {
        self.held_tool().map(|_| {
            if self.chop.vertical {
                "vertical"
            } else {
                "level"
            }
        })
    }

    /// Use with the axe: the swing turns; Break: a swing at the wood aimed at.
    fn chop_with(
        &mut self,
        tool: &'static dwell_protocol::chopping::ToolDef,
        action: EditAction,
        now_ms: f64,
    ) -> bool {
        if action == EditAction::Place {
            self.chop.toggle();
            return true;
        }
        // A swing goes at any time (a free swing at nothing too): its path is what hits.
        let Some(eye) = self.target_eye else {
            return false;
        };
        if !self.chop.ready(tool, now_ms) {
            return false;
        }
        // Exhausted (ADR 0069 §5): no swing until stamina is back to the recover share.
        if !self
            .predictor
            .controller()
            .vitals
            .can_exert(self.vitals_rules)
        {
            return false;
        }
        let look = self.view_dir;
        let arc = dwell_world::swing::Arc::of(tool);
        let store = self.streamer.store_mut();
        let swept = dwell_world::swing::sweep(store, eye, look, self.chop.vertical, &arc);
        // The chopping prototype (`?chop=v2`): the bite lands at the crosshair.
        let aimed = crate::chop::crosshair_aim()
            .then(|| crate::chop::crosshair_impact(store, tool, eye, look))
            .flatten();
        let hit = aimed.or_else(|| {
            swept.hit.map(|h| {
                let m = store.get_voxel(h.hit.cell[0], h.hit.cell[1], h.hit.cell[2]);
                crate::chop::Impact {
                    cell: h.hit.cell,
                    face: h.axis_face(),
                    point: h.point,
                    step: h.step,
                    wood: dwell_protocol::chopping::wood(dwell_blocks::block_of(m).name).is_some(),
                }
            })
        });
        let Some(message) = self.chop.start(tool, look, hit, now_ms) else {
            return false;
        };
        self.chop.set_cleared(&swept.cleared);
        self.bodies.swing_arms(if self.chop.vertical {
            dwell_characters::animator::Swing::ChopVertical
        } else {
            dwell_characters::animator::Swing::ChopLevel
        });
        let event = |n: &str| SoundKind::ALL.into_iter().find(|k| k.name() == n);
        if let Some(k) = tool.swing.swish.and_then(event) {
            self.sounds.push(SoundEvent::new(k, 0, 1.0, None));
        }
        if let Some(m) = message {
            self.outbox
                .borrow_mut()
                .push(Send::Control(Message::Chop(m)));
        }
        true
    }

    fn held_palette_slot(&self) -> Option<usize> {
        self.held_palette_slot_at(self.inventory_slot)
    }

    /// Holds hotbar slot `slot` of the inventory (a world that uses items): the palette follows
    /// what it holds, and the server is told (it takes placed blocks from it first).
    pub fn select_inventory_slot(&mut self, slot: usize) -> usize {
        let slot = slot.min(dwell_protocol::inventory::HOTBAR_SLOTS - 1);
        if let Some(p) = self.held_palette_slot_at(slot) {
            self.interaction.select(p as i64);
        }
        if self.inventory_slot != slot
            || self
                .loose
                .inventory
                .as_ref()
                .is_some_and(|(_, i)| usize::from(i.held) != slot)
        {
            if let Some((_, i)) = &mut self.loose.inventory {
                i.held = slot as u8;
            }
            self.outbox
                .borrow_mut()
                .push(Send::Control(Message::InventoryAction(
                    dwell_protocol::InventoryAction::Select { slot: slot as u8 },
                )));
        }
        self.inventory_slot = slot;
        slot
    }

    fn held_palette_slot_at(&self, slot: usize) -> Option<usize> {
        let (_, inv) = self.loose.inventory.as_ref()?;
        let names: Vec<&'static str> = self.interaction.palette().iter().map(|p| p.item).collect();
        crate::loose_items::palette_slot_for(inv, slot, &names)
    }

    /// The hotbar's slots (`inventory.json` `hotbarSlots`, both modes): the inventory's hotbar in
    /// a world that uses items, else the creative hotbar's picks.
    pub fn hotbar_len(&self) -> usize {
        dwell_protocol::inventory::HOTBAR_SLOTS
    }

    /// Selects hotbar slot `slot` (wrapping): an inventory slot, or a creative slot (its pick
    /// becomes the palette's selection).
    pub fn select_hotbar(&mut self, slot: i64) -> usize {
        if self.loose.uses_items() {
            let n = dwell_protocol::inventory::HOTBAR_SLOTS as i64;
            self.select_inventory_slot(slot.rem_euclid(n) as usize)
        } else {
            let (slot, pick) = self.creative.select(slot);
            self.interaction.select(pick as i64);
            slot
        }
    }

    /// Puts palette item `palette_index` in creative hotbar slot `slot` (the creative inventory
    /// view); the selected slot's pick is held at once. False if either is out of range.
    pub fn set_creative_slot(&mut self, slot: usize, palette_index: usize) -> bool {
        let n = self.interaction.palette().len();
        if !self.creative.set(slot, palette_index, n) {
            return false;
        }
        if !self.loose.uses_items() && slot == self.creative.slot() {
            self.interaction.select(palette_index as i64);
        }
        true
    }

    /// The scroll wheel or a controller's bumper: the next (step > 0) or previous hotbar slot.
    pub fn scroll_hotbar(&mut self, step: i32) -> Option<usize> {
        let at = if self.loose.uses_items() {
            self.inventory_slot
        } else {
            self.creative.slot()
        };
        (step != 0).then(|| self.select_hotbar(at as i64 + i64::from(step.signum())))
    }

    /// The ground item a ray from `eye` along `dir` meets within reach (ADR 0071), if the server
    /// keeps them.
    /// Keeps chunk `c`'s ground densities from its mesh (only where they vary; bounded).
    fn keep_ground_density(&mut self, c: [i32; 3], d: dwell_world::ground::GroundDensity) {
        if d.is_uniform() {
            self.ground_density.remove(&c);
            return;
        }
        if self.ground_density.len() >= 16_384 {
            self.ground_density.clear();
        }
        self.ground_density.insert(c, d);
    }

    /// Kind `kind`'s ground density in world cell `cell` (1 where its chunk's mesh had none).
    fn ground_density_at(&self, kind: usize, cell: [i32; 3]) -> f64 {
        let c = cell.map(|v| v.div_euclid(dwell_world::CHUNK_SIZE));
        self.ground_density
            .get(&c)
            .map_or(1.0, |d| d.at(kind, cell))
    }

    fn ground_under_crosshair(
        &self,
        eye: [f64; 3],
        dir: [f64; 3],
    ) -> Option<dwell_world::ground::Placed> {
        let (seed, gone) = self.streamer.ground()?;
        let world = self.streamer.store();
        // The rules read the terrain as generated (an edited chunk's kept generation).
        let mut get = |c: [i32; 3], generated: bool| {
            if generated && let Some(m) = self.streamer.generated_voxel(c) {
                return Some(m);
            }
            world.find(dwell_world::chunk_of(c[0], c[1], c[2]))?;
            Some(world.get_voxel_if_loaded(c))
        };
        let density = |k: usize, cell: [i32; 3]| self.ground_density_at(k, cell);
        let rules = dwell_world::ground::Rules::shipped();
        let rules = if self.ground_density.is_empty() {
            rules
        } else {
            rules.with_density(&density)
        };
        rules.target(
            &mut get,
            *seed,
            eye,
            dir,
            f64::from(players::REACH_DISTANCE),
            &gone.cells,
        )
    }

    /// The break action on the ground item under the crosshair (ADR 0071, amended 2026-11-09:
    /// "if you hit the rock or stick they drop as the spinning item that auto picks up"), when it
    /// is nearer than the block the crosshair is on (the ground under it): the server knocks it
    /// loose (`InventoryAction::Pick`). False when the break goes to the block as before.
    fn hit_ground(&mut self) -> bool {
        let (Some(it), Some(eye)) = (self.ground_target, self.target_eye) else {
            return false;
        };
        let block = self.interaction.target().map(|t| t.cell);
        if !dwell_world::ground::Rules::shipped().hit_before(&it, eye, self.view_dir, block) {
            return false;
        }
        self.outbox
            .borrow_mut()
            .push(Send::Control(Message::InventoryAction(
                dwell_protocol::InventoryAction::Pick { cell: it.cell },
            )));
        self.bodies
            .swing_arms(dwell_characters::animator::Swing::Break);
        self.sound(dwell_game::sound::SoundKind::Tool);
        // Its own sound where it lies (a stick's snap).
        if let Some(ev) = dwell_world::ground::Rules::shipped()
            .kind(it.kind)
            .and_then(|k| k.hit)
            .and_then(|n| SoundKind::ALL.into_iter().find(|k| k.name() == n))
        {
            self.sounds
                .push(SoundEvent::new(ev, 0, 1.0, Some(it.point())));
        }
        true
    }

    /// Moves items between inventory slots (the backpack screen's drag): the server decides.
    pub fn move_items(&mut self, from: u8, to: u8, count: u16) {
        self.outbox
            .borrow_mut()
            .push(Send::Control(Message::InventoryAction(
                dwell_protocol::InventoryAction::Move { from, to, count },
            )));
    }

    /// Makes recipe `name` once (the backpack's Combine section): the server decides. False, sending
    /// nothing, to a server that doesn't make recipes (`inventory_flags::COMBINE`).
    pub fn combine(&mut self, name: &str) -> bool {
        let Some(a) = dwell_protocol::inventory::combine_request(self.inventory_flags, name) else {
            return false;
        };
        self.outbox
            .borrow_mut()
            .push(Send::Control(Message::InventoryAction(a)));
        true
    }

    /// Whether the server makes recipes (`inventory_flags::COMBINE`).
    pub fn combines(&self) -> bool {
        self.inventory_flags & dwell_protocol::inventory_flags::COMBINE != 0
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
    fn drawable_revision(&self) -> Option<u64> {
        Some(self.streamer.drawable_revision())
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

/// Whether another player's state is on the ground (its snapshot carries no grounded flag).
fn on_ground(state: PlayerState) -> bool {
    matches!(
        state,
        PlayerState::Idle
            | PlayerState::Walking
            | PlayerState::Running
            | PlayerState::Crouching
            | PlayerState::Sliding
    )
}

/// A placed fire's key: its cell (stable while it burns there).
fn fire_key([x, y, z]: [i32; 3]) -> u64 {
    // 21 bits a coordinate, enough for the world's span (well under ±1M cells).
    let b = |v: i32| u64::from(v as u32) & 0x1f_ffff;
    b(x) | (b(y) << 21) | (b(z) << 42)
}

fn sound_event(kind: SoundKind) -> SoundEvent {
    SoundEvent::new(kind, 0, 1.0, None)
}

/// The audio listener at an eye looking along yaw/pitch on `face`: eye, forward, up.
/// The third-person camera over your dead body.
struct DeathCamera {
    eye: [f64; 3],
    pitch: f64,
    face: FaceFrame,
}

/// The camera over a dead body at `feet`, looking along `yaw`: on the face the body lies on (the
/// predictor's controller has no body while dead and falls back to face A's frame: owner,
/// 2026-11-06, the camera was upside down on face B), 4 m behind it along that face's look and
/// 2.5 m along its up, 25° down.
fn death_camera(feet: [f64; 3], yaw: f64) -> DeathCamera {
    let face = FaceFrame::at_height(feet[1]);
    let ahead = face.look(yaw, 0.0);
    let up = face.up();
    DeathCamera {
        eye: std::array::from_fn(|k| feet[k] - ahead[k] * 4.0 + up[k] * 2.5),
        pitch: -25.0,
        face,
    }
}

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

/// What the hand holds when it holds no tool: nothing for an empty inventory slot (a world that
/// uses items) or no block, else the block, as a brush while the brush tool is on.
fn held_of(inventory_slot_empty: bool, block: StateId, brush: bool) -> Held {
    if inventory_slot_empty || block == 0 {
        Held::Empty
    } else if brush {
        Held::Brush(block)
    } else {
        Held::Block(block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_inventory_slot_shows_an_empty_hand_whatever_the_palette_held_last() {
        // Owner (2026-11-08): run out of a block in adventure and it was still in your hand: the
        // palette kept the last block selected. face-a-only: no face involved.
        let dirt = dwell_blocks::require("dwell:dirt");
        assert_eq!(held_of(true, dirt, false), Held::Empty);
        assert_eq!(held_of(false, dirt, false), Held::Block(dirt));
        assert_eq!(held_of(false, dirt, true), Held::Brush(dirt));
        assert_eq!(held_of(false, 0, false), Held::Empty);
    }

    dwell_world::both_faces!(
        /// Owner (2026-11-06): dead on face B, the third-person camera was upside down. The body
        /// leaves the physics world when it dies and the predictor's controller falls back to face
        /// A's frame, which the camera used; the camera takes the face the body lies on, sits over
        /// it along that face's up and looks down at it.
        fn the_death_camera_is_upright_on_the_face_the_body_lies_on(f: FaceFrame) {
            let feet = f.point([3.5, 40.0, 7.5]);
            let cam = death_camera(feet, 30.0);
            assert_eq!(cam.face, f, "the face the body lies on");
            let up = f.up();
            let rise: f64 = (0..3).map(|k| (cam.eye[k] - feet[k]) * up[k]).sum();
            assert!(
                (rise - 2.5).abs() < 1e-9,
                "2.5 m over the body along its up: {rise}"
            );
            let look = f.look(30.0, cam.pitch);
            let toward: f64 = (0..3).map(|k| (feet[k] - cam.eye[k]) * look[k]).sum();
            assert!(toward > 3.0, "looking at the body: {toward}");
        }
    );

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
                    swing_kind: 0,
                    jumped: false,
                }
            })
            .collect()
    }
}

/// The client's world as drag building reads it (docs/ITEMS.md §5): a cell is free for air, a
/// replaceable plant or water, unless one of `me` is in it: our body where it is and where the
/// server may still see it (`Game::drag_body`).
struct DragCells<'a> {
    world: &'a mut dwell_world::VoxelWorld,
    me: Vec<dwell_world::edit::EditCapsule>,
    /// No drag fill awaits the server's answer.
    settled: bool,
}

impl dwell_game::build_drag::DragWorld for DragCells<'_> {
    fn occupied(&mut self, [x, y, z]: [i32; 3]) -> bool {
        !dwell_game::build_drag::open_cell(self.world.get_voxel(x, y, z))
    }
    fn holds(&mut self, cell: [i32; 3], material: StateId) -> bool {
        dwell_world::edit::held_in_place(self.world, cell, material)
    }
    fn settled(&mut self) -> bool {
        self.settled
    }

    fn free(&mut self, [x, y, z]: [i32; 3], material: StateId) -> bool {
        dwell_game::build_drag::cell_free(
            self.world.get_voxel(x, y, z),
            material,
            [x, y, z],
            &self.me,
        )
    }
}

/// The placed light sources within `radius` of `eye` (`VoxelWorld::light_sources_near`), as the
/// renderer's placed lights: each flame cell's centre, its light source and a flicker phase of
/// its cell.
pub fn placed_lights(
    world: &dwell_world::VoxelWorld,
    eye: [f64; 3],
    radius: f64,
) -> Vec<crate::render::lights::PlacedLight> {
    world
        .light_sources_near(eye, radius)
        .into_iter()
        .filter_map(|(c, m)| {
            let source = dwell_blocks::try_info(m)?.light?;
            let lo = dwell_world::pos_of_cell(c);
            let pos = std::array::from_fn(|k| lo[k] + 0.5 * dwell_world::CELL[k]);
            let h = (c[0] as u32).wrapping_mul(0x9e37_79b1)
                ^ (c[1] as u32).wrapping_mul(0x85eb_ca77)
                ^ (c[2] as u32).wrapping_mul(0xc2b2_ae3d);
            Some(crate::render::lights::PlacedLight {
                pos,
                source,
                seed: h,
            })
        })
        .collect()
}

#[cfg(test)]
mod placed_light_tests {
    use super::placed_lights;

    dwell_world::both_faces!(
        /// A placed torch near the eye becomes the renderer's placed light at its flame cell's
        /// centre (docs/LIGHTS.md §4); none past the radius.
        fn a_placed_torch_is_a_placed_light_at_its_flame(f: dwell_world::FaceFrame) {
            let mut w = dwell_world::VoxelWorld::new(dwell_world::empty_generator(), None);
            w.enable_light();
            let ground = f.cell([3, 30, 3]);
            w.set_voxel(
                ground[0],
                ground[1],
                ground[2],
                dwell_blocks::require("dwell:stone"),
            );
            let at = f.cell([3, 31, 3]);
            let put =
                dwell_world::edit::edit_writes(&mut w, at, dwell_blocks::require("dwell:torch"));
            for &([x, y, z], m) in &put {
                w.set_voxel(x, y, z, m);
            }
            // Both cells' chunks (the flame may be in the chunk above).
            for &([x, y, z], _) in &put {
                w.light_load(dwell_world::chunk_of(x, y, z));
            }
            let flame = dwell_world::edit::above(at);
            let eye = f.point([3.5, dwell_world::m_of_row(34), 0.5]);
            let found = placed_lights(&w, eye, 8.0);
            assert_eq!(found.len(), 1);
            let lo = dwell_world::pos_of_cell(flame);
            for (k, l) in lo.iter().enumerate() {
                let c = l + 0.5 * dwell_world::CELL[k];
                assert!((found[0].pos[k] - c).abs() < 1e-9, "{:?}", found[0].pos);
            }
            assert_eq!(found[0].source.name, "dwell:torch");
            assert!(placed_lights(&w, eye, 1.0).is_empty());
        }
    );
}

/// A struck point (1/255ths of `cell`, world-oriented) in world metres.
fn point_in_cell(cell: [i32; 3], p: [u8; 3]) -> [f64; 3] {
    let centre = dwell_world::centre_of_cell(cell);
    let size = [1.0, dwell_world::CELL_H, 1.0];
    std::array::from_fn(|k| centre[k] + (f64::from(p[k]) / 255.0 - 0.5) * size[k])
}
