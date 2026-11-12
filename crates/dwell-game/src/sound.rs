//! Sound events (docs/AUDIO.md §5): the game's movement and edits turned into compact events the
//! TS shell plays. A side channel: it reads the predicted controller state and the world, and
//! nothing here feeds back into the simulation. The event kinds, gaits and their strides and speed
//! ranges are data (shared/sounds/*.json, generated into `sound.gen.rs`).

use dwell_blocks::{StateId, info};
use dwell_protocol::{PlayerEventKind, PlayerState};
use dwell_world::FaceFrame;

include!("sound.gen.rs");

/// One sound for the shell to play.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoundEvent {
    pub kind: SoundKind,
    /// The block whose sound set plays it (0: none, the kind's own sound only).
    pub block: StateId,
    /// 0–1: how hard (a landing's impact, a splash's speed); 1 for most events.
    pub intensity: f32,
    /// Where, for spatial playback (other players, edits); None: at the listener.
    pub at: Option<[f64; 3]>,
    /// Footsteps: the gait (the shell's loudness per gait).
    pub gait: Option<Gait>,
    /// Continuous sounds: the voice the event starts, updates or stops — a mover's slide
    /// (`MoverSounds::voice`, its player id) or a fire ([`FIRE_VOICE`] on).
    pub voice: Option<u32>,
}

impl SoundEvent {
    pub fn new(kind: SoundKind, block: StateId, intensity: f64, at: Option<[f64; 3]>) -> Self {
        Self {
            kind,
            block,
            intensity: intensity.clamp(0.0, 1.0) as f32,
            at,
            gait: None,
            voice: None,
        }
    }
}

/// How far into an event's speed range `speed` is (0 at the start, 1 at full), or None below it.
pub fn speed_intensity(kind: SoundKind, speed: f64) -> Option<f64> {
    let (min, full) = kind.speed()?;
    (speed >= min).then(|| ((speed - min) / (full - min).max(1e-6)).clamp(0.0, 1.0))
}

/// What a mover's tracker reads each update: the local player's predicted state each tick, or a
/// remote player's interpolated state each frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoverSample {
    pub feet: [f64; 3],
    pub velocity: [f64; 3],
    pub state: PlayerState,
    pub grounded: bool,
    pub crouched: bool,
    /// The face it is on (its up).
    pub face: FaceFrame,
    /// 0–1: how much of the body is under water.
    pub submerged: f64,
    /// The velocity (world, m/s) of the moving ground carrying it (zero on the world's ground):
    /// strides are walked over that ground, so a player still on a raft takes no steps and one
    /// walking against its motion does (owner, 2026-11-09).
    pub carry: [f64; 3],
    /// The block just under the feet.
    pub ground: StateId,
    /// The block at the body (a ladder being climbed, the water swum in).
    pub body: StateId,
    /// The blocks just beside the body at its middle, face-local +x, −x, +z, −z (the walls it can
    /// hit), and just over its head (the ceiling).
    pub walls: [StateId; 4],
    pub ceiling: StateId,
    /// Seconds since the mover's previous sample (a tick for the local player, a frame for
    /// others).
    pub dt: f64,
}

/// How far past a body's side (and over its head) the impact sounds look for the surface it hits
/// (m): a drop of the speed toward a surface this close (touching, the controller's skin
/// included) is a hit (`ImpactSounds`); braking beside a wall a step away is not.
pub const IMPACT_PROBE_M: f64 = 0.06;

/// The blocks a mover's sounds read round a body (`MoverSample`'s `ground`, `body`, `walls`,
/// `ceiling`), and the block at its head line (`head`: how deep another player swims).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Surroundings {
    pub ground: StateId,
    pub body: StateId,
    pub walls: [StateId; 4],
    pub ceiling: StateId,
    /// At [`head_fraction`] of the body's height (the line drowning counts the head under at).
    pub head: StateId,
}

/// The share of the body under water at which its head is under (the vitals' `headFraction`,
/// shared/vitals/vitals.json: the same line drowning counts from). A swim stroke with the head
/// under plays the set's `swim_under` (BUGS.md #104).
pub fn head_fraction() -> f64 {
    f64::from(dwell_protocol::vitals::VITALS.head_fraction)
}

/// Whether a swimmer's head is under water (its stroke the deep, soft `swim_under`).
pub fn head_under(s: &MoverSample) -> bool {
    s.submerged >= head_fraction()
}

impl Surroundings {
    /// Reads them round a body with its feet at `feet` (world) on `face`, `radius` wide and
    /// `height` tall, from `voxel` (a world cell's state): the block just under the feet, at the
    /// body, just past its side at its middle (face-local ±x, ±z) and just over its head.
    pub fn read(
        feet: [f64; 3],
        face: FaceFrame,
        radius: f64,
        height: f64,
        mut voxel: impl FnMut([i32; 3]) -> StateId,
    ) -> Self {
        // The cell dx, dy, dz metres from the feet (face-local: dy toward the face's up; rows are
        // `CELL_H` tall).
        let mut at = |dx: f64, dy: f64, dz: f64| {
            voxel(dwell_world::cell_of_pos([
                feet[0] + dx,
                face.y(face.y(feet[1]) + dy),
                feet[2] + dz,
            ]))
        };
        let (side, mid) = (radius + IMPACT_PROBE_M, 0.5 * height);
        Self {
            ground: at(0.0, -0.2, 0.0),
            body: at(0.0, 0.5, 0.0),
            walls: [
                at(side, mid, 0.0),
                at(-side, mid, 0.0),
                at(0.0, mid, side),
                at(0.0, mid, -side),
            ],
            ceiling: at(0.0, height + IMPACT_PROBE_M, 0.0),
            head: at(0.0, head_fraction() * height, 0.0),
        }
    }

    /// How much of the body is under water, read from these blocks (another player, whose
    /// controller's share we don't have): all of it with the head line wet, afloat (the
    /// controller's `float_fraction`, 0.7) with only the body wet, else none.
    pub fn submerged(&self) -> f64 {
        if liquid(self.head) {
            1.0
        } else if liquid(self.body) {
            0.7
        } else {
            0.0
        }
    }
}

/// Submersion that counts as entering water, and as having left it (hysteresis).
const ENTER_SUBMERGED: f64 = 0.25;
const EXIT_SUBMERGED: f64 = 0.05;
/// Fraction of a stride already walked when a gait starts from standing (the first step comes
/// soon after setting off).
const FIRST_STEP_AT: f64 = 0.7;

fn gait_of(s: &MoverSample) -> Option<Gait> {
    match s.state {
        PlayerState::Walking if s.grounded => Some(Gait::Walk),
        PlayerState::Running if s.grounded => Some(Gait::Run),
        PlayerState::Crouching if s.grounded => Some(Gait::Crouch),
        PlayerState::Climbing => Some(Gait::Climb),
        PlayerState::Swimming => Some(Gait::Swim),
        _ => None,
    }
}

fn solid(id: StateId) -> bool {
    id != 0 && info(id).solid
}

fn liquid(id: StateId) -> bool {
    id != 0 && (info(id).liquid || info(id).flooded)
}

/// Seconds a slide may pause (the state flickering at the foot of a ramp, a bump) and go on as the
/// same slide; longer, and it has ended.
pub const SLIDE_HOLD: f64 = 0.15;
/// Speed updates of a slide: at most this often (s), and only for a change of intensity this
/// large (the shell glides between them).
pub const SLIDE_UPDATE_EVERY: f64 = 0.05;
pub const SLIDE_UPDATE_STEP: f64 = 0.02;

/// A slide in progress (one continuous voice in the shell).
#[derive(Clone, Copy, Debug)]
struct Slide {
    /// The surface it scrapes (its block's sound set).
    block: StateId,
    /// The intensity last sent.
    sent: f64,
    /// The intensity while last sliding (the stop sound's).
    last: f64,
    since_update: f64,
    /// Seconds not sliding.
    paused: f64,
}

/// The face-local directions a body hits a wall or a ceiling in: +x, −x, +z, −z (the walls, as
/// `MoverSample::walls`), up (the ceiling). Landings are `land`'s.
const HIT_DIRECTIONS: [[f64; 3]; 5] = [
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
    [0.0, 1.0, 0.0],
];
/// How long (s) the fastest speed toward a surface is remembered: a hit is a drop from it within
/// this long — a tick of our prediction, or one snapshot interval (50 ms) of another player's
/// snapshots. After a hit that direction is quiet as long (one hit per contact).
pub const IMPACT_HOLD: f64 = 0.07;
/// A hit keeps at most this share of the speed it had into the surface (the rest is lost to it):
/// a body that glances along a wall or turns away keeps more.
pub const IMPACT_KEEPS: f64 = 0.35;

/// The fastest a body went toward one direction lately.
#[derive(Clone, Copy, Debug, Default)]
struct Approach {
    peak: f64,
    age: f64,
    /// Seconds still quiet after a hit.
    quiet: f64,
}

/// Hits on walls and ceilings (docs/AUDIO.md §5), read from a body's sampled motion: the speed
/// toward a solid block touching its side (within `IMPACT_PROBE_M`; or over its head) dropping, in
/// a sample, to at most `IMPACT_KEEPS` of the fastest it went toward it lately — the hit's speed
/// that fastest, as the controller's `max(0, −v · n)` for the cell's face. The same reading for
/// every body: ours from the prediction's ticks, another player's from its snapshots (exact
/// positions, so the touch is exact too; dwell-client queues those by the interpolation delay).
/// Once per contact, the hardest of a sample's. Climbing and swimming hit nothing; flight does
/// (it never hurts). Landings are `land`'s (`MoverSounds`).
#[derive(Clone, Debug, Default)]
pub struct ImpactSounds {
    /// Toward each of `HIT_DIRECTIONS`.
    approach: [Approach; 5],
    feet: Option<[f64; 3]>,
}

impl ImpactSounds {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, s: &MoverSample, at: Option<[f64; 3]>, out: &mut Vec<SoundEvent>) {
        // A jump of more than a few metres between samples is a teleport (respawn, snap).
        if let Some(p) = self.feet.replace(s.feet)
            && (0..3).map(|k| (s.feet[k] - p[k]).abs()).sum::<f64>() > 4.0
        {
            self.approach = Default::default();
            return;
        }
        let v = s.face.vector(s.velocity);
        let exempt = s.submerged > ENTER_SUBMERGED
            || matches!(s.state, PlayerState::Climbing | PlayerState::Swimming);
        let mut hit: Option<(f64, StateId)> = None;
        for (k, n) in HIT_DIRECTIONS.iter().enumerate() {
            let toward = (v[0] * n[0] + v[1] * n[1] + v[2] * n[2]).max(0.0);
            let block = if k < 4 { s.walls[k] } else { s.ceiling };
            let a = &mut self.approach[k];
            a.quiet = (a.quiet - s.dt).max(0.0);
            let hits = !exempt && a.quiet == 0.0 && solid(block) && toward <= IMPACT_KEEPS * a.peak;
            if hits && speed_intensity(SoundKind::Impact, a.peak).is_some() {
                if hit.is_none_or(|(h, _)| a.peak > h) {
                    hit = Some((a.peak, block));
                }
                a.quiet = IMPACT_HOLD;
            }
            if hits || toward >= a.peak {
                (a.peak, a.age) = (toward, 0.0);
            } else {
                a.age += s.dt;
                if a.age > IMPACT_HOLD {
                    (a.peak, a.age) = (toward, 0.0);
                }
            }
        }
        if let Some((speed, block)) = hit
            && let Some(i) = speed_intensity(SoundKind::Impact, speed)
        {
            out.push(SoundEvent::new(SoundKind::Impact, block, i, at));
        }
    }
}

/// Turns one mover's motion into footsteps, landings, jumps, splashes, slides, hits and the like.
#[derive(Clone, Debug, Default)]
pub struct MoverSounds {
    /// The voice of this mover's continuous sounds (0: the local player; others: their id).
    voice: u32,
    /// Its hits (ours; another player's come from its snapshots, `ImpactSounds`).
    impacts: Option<ImpactSounds>,
    slide: Option<Slide>,
    prev: Option<MoverSample>,
    /// Distance walked toward the next footstep.
    travelled: f64,
    in_water: bool,
    last_ground: StateId,
    last_water: StateId,
    /// Flying: −1 diving, 0 level, +1 rising.
    flying_dir: i8,
}

impl MoverSounds {
    /// Our sounds, hits included.
    pub fn new() -> Self {
        Self {
            impacts: Some(ImpactSounds::new()),
            ..Self::default()
        }
    }

    /// Another player's sounds from its interpolated frames: `voice` is its player id. Its hits
    /// are read from its snapshots instead (`ImpactSounds`).
    pub fn with_voice(voice: u16) -> Self {
        Self {
            voice: u32::from(voice),
            ..Self::default()
        }
    }

    fn voiced(&self, mut e: SoundEvent) -> SoundEvent {
        e.voice = Some(self.voice);
        e
    }

    /// Ends what is still sounding (a slide), e.g. when the mover goes out of view.
    /// `at` None for another player's sounds: where it was last seen.
    pub fn finish(&mut self, at: Option<[f64; 3]>, out: &mut Vec<SoundEvent>) {
        let at = at.or_else(|| {
            (self.voice != 0)
                .then(|| self.prev.map(|p| p.feet))
                .flatten()
        });
        if let Some(sl) = self.slide.take() {
            out.push(self.voiced(SoundEvent::new(SoundKind::SlideEnd, sl.block, sl.last, at)));
        }
    }

    /// The slide's continuous scrape (docs/AUDIO.md §5): `slide` starts the mover's voice on
    /// the surface's set, `slide_speed` moves its intensity with the speed, `slide_end` stops it
    /// and plays the set's soft stop. A pause shorter than `SLIDE_HOLD` keeps the same slide
    /// (its voice falls silent meanwhile); a new surface restarts the voice on that set.
    fn slide(
        &mut self,
        s: &MoverSample,
        surface: StateId,
        at: Option<[f64; 3]>,
        out: &mut Vec<SoundEvent>,
    ) {
        let speed = (s.velocity[0].powi(2) + s.velocity[1].powi(2) + s.velocity[2].powi(2)).sqrt();
        let level = (s.state == PlayerState::Sliding)
            .then(|| speed_intensity(SoundKind::Slide, speed))
            .flatten();
        let Some(mut sl) = self.slide else {
            if let Some(i) = level {
                out.push(self.voiced(SoundEvent::new(SoundKind::Slide, surface, i, at)));
                self.slide = Some(Slide {
                    block: surface,
                    sent: i,
                    last: i,
                    since_update: 0.0,
                    paused: 0.0,
                });
            }
            return;
        };
        sl.since_update += s.dt;
        match level {
            Some(i) => {
                sl.paused = 0.0;
                sl.last = i;
                if surface != sl.block && solid(surface) {
                    // Onto another material: the voice restarts on its set.
                    sl.block = surface;
                    sl.sent = i;
                    sl.since_update = 0.0;
                    out.push(self.voiced(SoundEvent::new(SoundKind::Slide, surface, i, at)));
                }
            }
            None => sl.paused += s.dt,
        }
        if sl.paused > SLIDE_HOLD {
            self.slide = Some(sl);
            self.finish(at, out);
            return;
        }
        let target = level.unwrap_or(0.0);
        let moved =
            (target - sl.sent).abs() >= SLIDE_UPDATE_STEP || (target == 0.0) != (sl.sent == 0.0);
        if sl.since_update >= SLIDE_UPDATE_EVERY && moved {
            sl.sent = target;
            sl.since_update = 0.0;
            out.push(self.voiced(SoundEvent::new(SoundKind::SlideSpeed, sl.block, target, at)));
        }
        self.slide = Some(sl);
    }

    /// One update; events are pushed to `out`, placed at `at` (None for the local player).
    pub fn update(&mut self, s: &MoverSample, at: Option<[f64; 3]>, out: &mut Vec<SoundEvent>) {
        if solid(s.ground) {
            self.last_ground = s.ground;
        }
        if liquid(s.body) {
            self.last_water = s.body;
        } else if liquid(s.ground) {
            self.last_water = s.ground;
        }
        let Some(p) = self.prev.replace(*s) else {
            self.in_water = s.submerged > ENTER_SUBMERGED;
            return;
        };
        let surface = if solid(s.ground) {
            s.ground
        } else {
            self.last_ground
        };
        // Speeds along the mover's up (face-local).
        let up = |v: [f64; 3], face: FaceFrame| face.vector(v)[1];
        // Moved over the ground carrying it (its carry taken off).
        let d: [f64; 3] = std::array::from_fn(|k| s.feet[k] - p.feet[k] - s.carry[k] * s.dt);
        let horizontal = d[0].hypot(d[2]);
        // A jump of more than a few metres between samples is a teleport (respawn, snap): no sound.
        if horizontal + d[1].abs() > 4.0 {
            self.travelled = 0.0;
            self.finish(at, out);
            if let Some(i) = &mut self.impacts {
                i.update(s, at, out);
            }
            return;
        }
        if let Some(i) = &mut self.impacts {
            i.update(s, at, out);
        }
        let speed = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();

        // Water: entering and leaving, scaled by speed.
        // The speed it met the water at: the faster of this sample and the last.
        let water_speed = speed(p.velocity).max(speed(s.velocity));
        let was_in = self.in_water;
        if !was_in && s.submerged > ENTER_SUBMERGED {
            self.in_water = true;
            if let Some(i) = speed_intensity(SoundKind::Enter, water_speed) {
                out.push(SoundEvent::new(SoundKind::Enter, self.last_water, i, at));
            }
        } else if was_in && s.submerged < EXIT_SUBMERGED {
            self.in_water = false;
            if let Some(i) = speed_intensity(SoundKind::Exit, water_speed) {
                out.push(SoundEvent::new(SoundKind::Exit, self.last_water, i, at));
            }
        }

        // Landing: grounded again after falling, by the speed it hit at.
        if s.grounded && !p.grounded && !self.in_water {
            let impact = -up(p.velocity, p.face);
            if let Some(i) = speed_intensity(SoundKind::Land, impact) {
                out.push(SoundEvent::new(SoundKind::Land, surface, i, at));
            }
            self.travelled = 0.5 * Gait::Walk.stride();
        }
        if s.state == PlayerState::Jumping && p.state != PlayerState::Jumping && p.grounded {
            out.push(SoundEvent::new(SoundKind::Jump, surface, 1.0, at));
        }
        if s.state != PlayerState::Flying && p.state != PlayerState::Flying {
            if s.crouched && !p.crouched {
                out.push(SoundEvent::new(SoundKind::CrouchOn, 0, 1.0, at));
            } else if !s.crouched && p.crouched {
                out.push(SoundEvent::new(SoundKind::CrouchOff, 0, 1.0, at));
            }
        }
        let from_footing = matches!(
            p.state,
            PlayerState::Idle | PlayerState::Walking | PlayerState::Crouching
        );
        if s.state == PlayerState::Running && p.state != PlayerState::Running && from_footing {
            out.push(SoundEvent::new(SoundKind::Sprint, 0, 1.0, at));
        }
        self.slide(s, surface, at, out);

        // Flight: a whoosh when starting to rise or dive.
        if s.state == PlayerState::Flying {
            let v = up(s.velocity, s.face);
            let (min, _) = SoundKind::Rise.speed().unwrap_or((2.0, 30.0));
            let dir = if v > min {
                1
            } else if v < -min {
                -1
            } else if v.abs() < 0.5 * min {
                0
            } else {
                self.flying_dir
            };
            if dir != self.flying_dir && dir != 0 {
                let kind = if dir > 0 {
                    SoundKind::Rise
                } else {
                    SoundKind::Dive
                };
                let i = speed_intensity(kind, v.abs()).unwrap_or(0.0);
                out.push(SoundEvent::new(kind, 0, i, at));
            }
            self.flying_dir = dir;
        } else {
            self.flying_dir = 0;
        }

        // Footsteps, climbing rungs and swim strokes: one per stride of distance.
        let gait = gait_of(s);
        match (gait, gait_of(&p)) {
            (Some(g), None) => self.travelled = FIRST_STEP_AT * g.stride(),
            (Some(g), Some(was)) if g != was => {
                self.travelled *= g.stride() / was.stride();
            }
            _ => {}
        }
        if let Some(g) = gait {
            let (distance, kind, block) = match g {
                Gait::Climb => (
                    d[1].abs() + horizontal,
                    SoundKind::Climb,
                    if s.body != 0 { s.body } else { surface },
                ),
                // The head under: the soft, deep stroke; at the surface, the calm splash.
                Gait::Swim if head_under(s) => (
                    horizontal + d[1].abs(),
                    SoundKind::SwimUnder,
                    self.last_water,
                ),
                Gait::Swim => (horizontal + d[1].abs(), SoundKind::Swim, self.last_water),
                _ => (horizontal, SoundKind::Step, surface),
            };
            self.travelled += distance;
            if self.travelled >= g.stride() {
                self.travelled %= g.stride();
                let mut e = SoundEvent::new(kind, block, 1.0, at);
                e.gait = Some(g);
                out.push(e);
            }
        }
    }
}

/// How long the server's echo of our own edit is recognised (ms).
const ECHO_MS: f64 = 3000.0;
/// At most this many edit sounds per modification message (a big change would be a roar).
const MAX_EDIT_SOUNDS: usize = 4;

/// One voxel changed by the server: its cell, what it was and what it is now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellChange {
    pub cell: [i32; 3],
    pub old: StateId,
    pub new: StateId,
}

fn centre(cell: [i32; 3]) -> [f64; 3] {
    dwell_world::centre_of_cell(cell)
}

/// Break and place sounds: our own at once (the client does not predict edits), other players'
/// when their change arrives.
#[derive(Clone, Debug, Default)]
pub struct EditSounds {
    /// Cells we asked to change, and when: their echo is not played again.
    pending: Vec<([i32; 3], f64)>,
}

impl EditSounds {
    pub fn new() -> Self {
        Self::default()
    }

    /// Our break request on `block` at `cell` (an indestructible block only gives a light hit).
    pub fn local_break(
        &mut self,
        cell: [i32; 3],
        block: StateId,
        now_ms: f64,
        out: &mut Vec<SoundEvent>,
    ) {
        if info(block).indestructible {
            out.push(SoundEvent::new(
                SoundKind::Hit,
                block,
                1.0,
                Some(centre(cell)),
            ));
            return;
        }
        self.pending.push((cell, now_ms));
        out.push(SoundEvent::new(
            SoundKind::Break,
            block,
            1.0,
            Some(centre(cell)),
        ));
    }

    /// Our break request at `cell`, whose sound comes later (a tool's hit lands at its swing's
    /// contact, WORK_QUEUE item 25): the server's change of it is its echo, not played again.
    pub fn expect_break(&mut self, cell: [i32; 3], now_ms: f64) {
        self.pending.push((cell, now_ms));
    }

    /// Our place request of `block` into `cell`.
    pub fn local_place(
        &mut self,
        cell: [i32; 3],
        block: StateId,
        now_ms: f64,
        out: &mut Vec<SoundEvent>,
    ) {
        self.pending.push((cell, now_ms));
        out.push(SoundEvent::new(
            SoundKind::Place,
            block,
            1.0,
            Some(centre(cell)),
        ));
    }

    /// The server's edit changes: ours (recognised by cell) are skipped, the rest sound where they
    /// happened.
    pub fn changes(&mut self, changes: &[CellChange], now_ms: f64, out: &mut Vec<SoundEvent>) {
        self.pending.retain(|(_, at)| now_ms - at < ECHO_MS);
        let mut played = 0;
        for c in changes {
            if c.old == c.new {
                continue;
            }
            if let Some(i) = self.pending.iter().position(|(cell, _)| *cell == c.cell) {
                self.pending.swap_remove(i);
                continue;
            }
            if played == MAX_EDIT_SOUNDS {
                continue;
            }
            played += 1;
            let at = Some(centre(c.cell));
            out.push(if c.new == 0 {
                SoundEvent::new(SoundKind::Break, c.old, 1.0, at)
            } else {
                SoundEvent::new(SoundKind::Place, c.new, 1.0, at)
            });
        }
    }
}

/// A health sound (docs/AUDIO.md §5) for the server's player event: `hurt` by the damage taken
/// (the event's `speed` range is in health points), `death`, `respawn`; any player's, ours at the
/// listener (`at` None), another's where it is. None for events without a sound (knockback).
pub fn player_event_sound(
    kind: PlayerEventKind,
    amount: u8,
    at: Option<[f64; 3]>,
) -> Option<SoundEvent> {
    match kind {
        PlayerEventKind::Damage => {
            let i = speed_intensity(SoundKind::Hurt, f64::from(amount)).unwrap_or(0.0);
            Some(SoundEvent::new(SoundKind::Hurt, 0, i, at))
        }
        PlayerEventKind::Death => Some(SoundEvent::new(SoundKind::Death, 0, 1.0, at)),
        PlayerEventKind::Respawn => Some(SoundEvent::new(SoundKind::Respawn, 0, 1.0, at)),
        PlayerEventKind::Knockback => None,
    }
}

/// The first voice of the fire loops (one per slot, up to `ambient::FIRE_MAX`); clear of the
/// movers' voices (player ids, u16).
pub const FIRE_VOICE: u32 = 0x1_0000;
/// Loop updates: at most this often (s), and only for a change of intensity this large or a move
/// this far (m); the shell glides between them.
pub const LOOP_UPDATE_EVERY: f64 = 0.1;
pub const LOOP_UPDATE_STEP: f64 = 0.02;
pub const LOOP_UPDATE_MOVE: f64 = 0.25;
/// A loop starts above this level and stops at or below the lower one (hysteresis).
pub const LOOP_ON: f64 = 0.03;
pub const LOOP_OFF: f64 = 0.01;

/// One continuous sound on its own voice: started, moved and stopped by a level (0–1).
#[derive(Clone, Copy, Debug)]
struct LoopTrack {
    start: SoundKind,
    update: SoundKind,
    stop: SoundKind,
    voice: u32,
    playing: bool,
    sent: f64,
    sent_at: Option<[f64; 3]>,
    since: f64,
}

impl LoopTrack {
    const fn new(start: SoundKind, update: SoundKind, stop: SoundKind, voice: u32) -> Self {
        Self {
            start,
            update,
            stop,
            voice,
            playing: false,
            sent: 0.0,
            sent_at: None,
            since: 0.0,
        }
    }

    fn event(
        &self,
        kind: SoundKind,
        block: StateId,
        level: f64,
        at: Option<[f64; 3]>,
    ) -> SoundEvent {
        let mut e = SoundEvent::new(kind, block, level, at);
        e.voice = Some(self.voice);
        e
    }

    /// The level this update (`block`: its sound set, for set loops).
    fn set(
        &mut self,
        level: f64,
        block: StateId,
        at: Option<[f64; 3]>,
        dt: f64,
        out: &mut Vec<SoundEvent>,
    ) {
        self.since += dt;
        if !self.playing {
            if level > LOOP_ON {
                out.push(self.event(self.start, block, level, at));
                (self.playing, self.sent, self.sent_at, self.since) = (true, level, at, 0.0);
            }
            return;
        }
        if level <= LOOP_OFF {
            self.end(out);
            return;
        }
        let moved = match (self.sent_at, at) {
            (Some(a), Some(b)) => {
                (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f64>() >= LOOP_UPDATE_MOVE.powi(2)
            }
            _ => false,
        };
        if self.since >= LOOP_UPDATE_EVERY
            && ((level - self.sent).abs() >= LOOP_UPDATE_STEP || moved)
        {
            out.push(self.event(self.update, block, level, at));
            (self.sent, self.sent_at, self.since) = (level, at, 0.0);
        }
    }

    fn end(&mut self, out: &mut Vec<SoundEvent>) {
        if self.playing {
            out.push(self.event(self.stop, 0, 0.0, self.sent_at));
            self.playing = false;
        }
    }
}

/// `x`'s share of the way from `range.0` (0) to `range.1` (1), clamped.
pub fn ramp(x: f64, (from, full): (f64, f64)) -> f64 {
    ((x - from) / (full - from)).clamp(0.0, 1.0)
}

/// A burning light source that may crackle: a placed one's flame cell, or one in a hand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FireSource {
    /// Stable while it burns (a placed one's cell, a hand's player): it keeps its voice.
    pub key: u64,
    /// Where it is (the distance to the listener).
    pub pos: [f64; 3],
    /// Where it sounds: `pos`, or None at the listener (the torch in our own hand).
    pub at: Option<[f64; 3]>,
    /// Its block (its sound set's `burn`).
    pub block: StateId,
}

/// Whether a block burns audibly: a light source with a flame (docs/LIGHTS.md §6). Its sound is
/// its sound set's `burn` (shared/sounds), none if the set has none.
pub fn burns(block: StateId) -> bool {
    block != 0
        && dwell_blocks::try_info(block)
            .and_then(|i| i.light)
            .is_some_and(|l| l.flame.is_some())
}

/// A fire's level at `distance` m: full beside it, fading smoothly to nothing at the radius (the
/// shell's distance rolloff comes on top).
pub fn fire_level(distance: f64) -> f64 {
    let x = (distance / ambient::FIRE_RADIUS_M).clamp(0.0, 1.0);
    (1.0 - x * x).powi(2)
}

/// The crackle of burning light sources near the listener (docs/AUDIO.md §5): the nearest
/// `ambient::FIRE_MAX` within `ambient::FIRE_RADIUS_M`, each on its own voice while it stays
/// among them, its level by distance.
#[derive(Clone, Debug)]
pub struct FireSounds {
    slots: Vec<(Option<u64>, LoopTrack)>,
}

impl Default for FireSounds {
    fn default() -> Self {
        Self {
            slots: (0..ambient::FIRE_MAX)
                .map(|i| {
                    let t = LoopTrack::new(
                        SoundKind::Burn,
                        SoundKind::BurnLevel,
                        SoundKind::BurnEnd,
                        FIRE_VOICE + i as u32,
                    );
                    (None, t)
                })
                .collect(),
        }
    }
}

impl FireSounds {
    pub fn new() -> Self {
        Self::default()
    }

    /// One frame: `sources` near the listener at `listener` (any order, any number).
    pub fn update(
        &mut self,
        listener: [f64; 3],
        sources: &[FireSource],
        dt: f64,
        out: &mut Vec<SoundEvent>,
    ) {
        let dist = |p: [f64; 3]| {
            (0..3)
                .map(|k| (p[k] - listener[k]).powi(2))
                .sum::<f64>()
                .sqrt()
        };
        let mut near: Vec<(f64, &FireSource)> = sources
            .iter()
            .filter(|f| burns(f.block))
            .map(|f| (dist(f.pos), f))
            .filter(|(d, _)| *d < ambient::FIRE_RADIUS_M)
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        near.truncate(ambient::FIRE_MAX);
        // Slots whose source is no longer among the nearest fall silent.
        for (key, track) in &mut self.slots {
            if key.is_some_and(|k| !near.iter().any(|(_, f)| f.key == k)) {
                track.end(out);
                *key = None;
            }
        }
        for (d, f) in near {
            let slot = match self.slots.iter().position(|(k, _)| *k == Some(f.key)) {
                Some(i) => i,
                None => {
                    let Some(i) = self.slots.iter().position(|(k, _)| k.is_none()) else {
                        continue;
                    };
                    self.slots[i].0 = Some(f.key);
                    i
                }
            };
            let track = &mut self.slots[slot].1;
            track.set(fire_level(d), f.block, f.at, dt, out);
        }
    }

    /// Ends every fire's voice (leaving the world).
    pub fn finish(&mut self, out: &mut Vec<SoundEvent>) {
        for (key, track) in &mut self.slots {
            track.end(out);
            *key = None;
        }
    }

    /// The fires sounding now (their keys).
    pub fn playing(&self) -> Vec<u64> {
        self.slots
            .iter()
            .filter(|(_, t)| t.playing)
            .filter_map(|(k, _)| *k)
            .collect()
    }
}

/// The first voice of the waterfalls' loops (one per slot, up to `ambient::FALLS_MAX`); past the
/// fires' and the night's.
pub const FALL_VOICE: u32 = 0x3_0000;

/// A waterfall that may rush (docs/WATER_SURFACE_GRID.md §2e): `dwell_world::falls`' near the
/// listener.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FallSource {
    /// Stable while it falls (its lip's place and way): it keeps its voice.
    pub key: u64,
    /// Where it sounds (world m): its sheet's middle.
    pub pos: [f64; 3],
    /// The water it carries over (m³/s).
    pub discharge: f64,
}

/// A waterfall's level: by its flow (the square root of its share of `ambient::FALLS_FULL_M3S`, at
/// most 1) and by distance, full beside it and fading smoothly to nothing at `FALLS_RADIUS_M` (the
/// shell's distance rolloff comes on top).
pub fn fall_level(distance: f64, discharge: f64) -> f64 {
    let x = (distance / ambient::FALLS_RADIUS_M).clamp(0.0, 1.0);
    let flow = (discharge / ambient::FALLS_FULL_M3S).clamp(0.0, 1.0).sqrt();
    flow * (1.0 - x * x).powi(2)
}

/// The rush of the waterfalls near the listener: the loudest `ambient::FALLS_MAX` within
/// `ambient::FALLS_RADIUS_M`, each on its own voice while it stays among them.
#[derive(Clone, Debug)]
pub struct FallSounds {
    slots: Vec<(Option<u64>, LoopTrack)>,
}

impl Default for FallSounds {
    fn default() -> Self {
        Self {
            slots: (0..ambient::FALLS_MAX)
                .map(|i| {
                    let t = LoopTrack::new(
                        SoundKind::Fall,
                        SoundKind::FallLevel,
                        SoundKind::FallEnd,
                        FALL_VOICE + i as u32,
                    );
                    (None, t)
                })
                .collect(),
        }
    }
}

impl FallSounds {
    pub fn new() -> Self {
        Self::default()
    }

    /// One frame: the falls `sources` near the listener at `listener` (any order, any number).
    pub fn update(
        &mut self,
        listener: [f64; 3],
        sources: &[FallSource],
        dt: f64,
        out: &mut Vec<SoundEvent>,
    ) {
        let dist = |p: [f64; 3]| {
            (0..3)
                .map(|k| (p[k] - listener[k]).powi(2))
                .sum::<f64>()
                .sqrt()
        };
        let mut near: Vec<(f64, &FallSource)> = sources
            .iter()
            .map(|f| (fall_level(dist(f.pos), f.discharge), f))
            .filter(|(l, _)| *l > LOOP_OFF)
            .collect();
        near.sort_by(|a, b| b.0.total_cmp(&a.0));
        near.truncate(ambient::FALLS_MAX);
        for (key, track) in &mut self.slots {
            if key.is_some_and(|k| !near.iter().any(|(_, f)| f.key == k)) {
                track.end(out);
                *key = None;
            }
        }
        for (level, f) in near {
            let slot = match self.slots.iter().position(|(k, _)| *k == Some(f.key)) {
                Some(i) => i,
                None => {
                    let Some(i) = self.slots.iter().position(|(k, _)| k.is_none()) else {
                        continue;
                    };
                    self.slots[i].0 = Some(f.key);
                    i
                }
            };
            self.slots[slot].1.set(level, 0, Some(f.pos), dt, out);
        }
    }

    /// Ends every fall's voice (leaving the world).
    pub fn finish(&mut self, out: &mut Vec<SoundEvent>) {
        for (key, track) in &mut self.slots {
            track.end(out);
            *key = None;
        }
    }

    /// The falls sounding now (their keys).
    pub fn playing(&self) -> Vec<u64> {
        self.slots
            .iter()
            .filter(|(_, t)| t.playing)
            .filter_map(|(k, _)| *k)
            .collect()
    }
}

/// Events as the JS API hands them over: 7 numbers each (kind, block state, intensity, x, y, z,
/// flags: bit 0 spatial, bits 1–3 the gait + 1, bits 4 and up the voice + 1).
pub const FLOATS_PER_EVENT: usize = 7;

/// A creature as the sounds see it: its id, model, feet (world) and whether it runs.
#[derive(Clone, Copy, Debug)]
pub struct CreatureSample<'a> {
    pub id: u32,
    pub model: &'a str,
    pub feet: [f64; 3],
    pub running: bool,
}

#[derive(Clone, Debug)]
struct CreatureTrack {
    last: [f64; 3],
    walked: f64,
    running: bool,
    call_in: f64,
}

/// The creatures' sounds (docs/AUDIO.md §2d): a footstep every stride walked, a startle when one
/// breaks into a run, a call now and then; within `creature_sounds::RADIUS_M` of the listener, by
/// the model's data (`creature_sounds::MODELS`). Events carry the model's index as their `block`.
#[derive(Clone, Debug, Default)]
pub struct CreatureSounds {
    seen: std::collections::HashMap<u32, CreatureTrack>,
    rng: u64,
}

impl CreatureSounds {
    pub fn new(seed: u64) -> Self {
        Self {
            seen: Default::default(),
            rng: seed | 1,
        }
    }

    fn draw(&mut self, (lo, hi): (f64, f64)) -> f64 {
        // xorshift64*: the calls' spacing only.
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        let u = (self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64;
        lo + (hi - lo) * u
    }

    pub fn update(
        &mut self,
        eye: [f64; 3],
        creatures: &[CreatureSample<'_>],
        dt: f64,
        out: &mut Vec<SoundEvent>,
    ) {
        use creature_sounds::{MODELS, RADIUS_M};
        for c in creatures {
            let Some(index) = MODELS.iter().position(|m| m.0 == c.model) else {
                continue;
            };
            let (_, stride, call) = MODELS[index];
            let d: [f64; 3] = std::array::from_fn(|k| c.feet[k] - eye[k]);
            let near = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() <= RADIUS_M;
            let first_call = call.map_or(f64::INFINITY, |r| self.draw(r));
            let t = self.seen.entry(c.id).or_insert(CreatureTrack {
                last: c.feet,
                walked: 0.0,
                running: c.running,
                call_in: first_call,
            });
            let m: [f64; 3] = std::array::from_fn(|k| c.feet[k] - t.last[k]);
            let moved = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
            t.last = c.feet;
            // A jump of more than a few metres (a first sight, a teleport) is no walk.
            if moved < 4.0 {
                t.walked += moved;
            }
            let block = index as StateId;
            let mut events = Vec::new();
            if t.walked >= stride {
                t.walked %= stride;
                events.push(SoundKind::CreatureStep);
            }
            if c.running && !t.running {
                events.push(SoundKind::CreatureStartle);
            }
            t.running = c.running;
            t.call_in -= dt;
            let called = t.call_in <= 0.0;
            if called {
                events.push(SoundKind::CreatureCall);
            }
            if near {
                out.extend(
                    events
                        .into_iter()
                        .map(|k| SoundEvent::new(k, block, 1.0, Some(c.feet))),
                );
            }
            if called {
                let next = call.map_or(f64::INFINITY, |r| self.draw(r));
                if let Some(t) = self.seen.get_mut(&c.id) {
                    t.call_in = next;
                }
            }
        }
        self.seen
            .retain(|id, _| creatures.iter().any(|c| c.id == *id));
    }
}

/// The air share under which surfacing gasps (a breath held long), and under which the body strains.
pub const GASP_BELOW: f64 = 0.7;
pub const STRAIN_BELOW: f64 = 0.25;

/// The player's body (docs/AUDIO.md §5): heavy breathing as stamina runs out, a gasp on surfacing
/// short of air, a strain as the air runs low. Read from the vitals each frame (ADR 0069).
#[derive(Clone, Debug, Default)]
pub struct VitalSounds {
    exhausted: bool,
    last_air: Option<f64>,
    /// The air fell below `GASP_BELOW` and has not come back yet.
    held: bool,
    strained: bool,
}

impl VitalSounds {
    pub fn new() -> Self {
        Self::default()
    }

    /// `exhausted`: out of stamina; `air`: the oxygen's share of full (None where the mode has none).
    pub fn update(&mut self, exhausted: bool, air: Option<f64>, out: &mut Vec<SoundEvent>) {
        if exhausted && !self.exhausted {
            out.push(SoundEvent::new(SoundKind::Exhausted, 0, 1.0, None));
        }
        self.exhausted = exhausted;
        let Some(air) = air else {
            self.last_air = None;
            return;
        };
        if let Some(last) = self.last_air {
            if air < GASP_BELOW && air < last {
                self.held = true;
            }
            if self.held && air > last + 1e-6 {
                out.push(SoundEvent::new(SoundKind::Gasp, 0, 1.0, None));
                self.held = false;
            }
            if air < STRAIN_BELOW && last >= STRAIN_BELOW && !self.strained {
                out.push(SoundEvent::new(SoundKind::Strain, 0, 1.0, None));
                self.strained = true;
            }
        }
        if air >= STRAIN_BELOW {
            self.strained = false;
        }
        self.last_air = Some(air);
    }
}

pub fn encode(events: &[SoundEvent]) -> Vec<f64> {
    let mut v = Vec::with_capacity(events.len() * FLOATS_PER_EVENT);
    for e in events {
        let at = e.at.unwrap_or([0.0; 3]);
        let flags = u64::from(e.at.is_some())
            | (e.gait.map_or(0, |g| u64::from(g as u8) + 1) << 1)
            | (e.voice.map_or(0, |v| u64::from(v) + 1) << 4);
        v.extend_from_slice(&[
            f64::from(e.kind as u8),
            f64::from(e.block),
            f64::from(e.intensity),
            at[0],
            at[1],
            at[2],
            flags as f64,
        ]);
    }
    v
}

/// Moving through foliage (AUDIO.md §5 `brush`, sounds round 2b, BUGS.md #103): a passable block
/// the body is in (leaves, tall grass) rustles its set's `brush`, louder the faster (the event's
/// `speed`). On its feet a body rustles only with a footstep (`Stride::stepped`), at most one
/// rustle a step, layered on it; off its feet (falling, flying, climbing through leaves) one every
/// `BRUSH_EVERY_M` moved. Never closer than `BRUSH_MIN_S`, never on entering a cell, and standing
/// still in a bush is silent. Both are data (shared/sounds `brush`).
pub const BRUSH_EVERY_M: f64 = brush_sounds::EVERY_M;
pub const BRUSH_MIN_S: f64 = brush_sounds::MIN_S;

/// What a body's feet did this update, for the sounds layered on its footsteps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stride {
    /// On a ground gait (walking, running, crouching): its foliage rustles with its steps.
    pub on_feet: bool,
    /// A footstep sounded this update.
    pub stepped: bool,
}

impl Stride {
    /// The stride of a mover's sample whose update pushed `events`.
    pub fn of(s: &MoverSample, events: &[SoundEvent]) -> Self {
        Self {
            on_feet: matches!(gait_of(s), Some(Gait::Walk | Gait::Run | Gait::Crouch)),
            stepped: events.iter().any(|e| e.kind == SoundKind::Step),
        }
    }
}

/// Where up the body (m above the feet, face-local) foliage is looked for: the shins, the waist,
/// the head.
const BRUSH_AT_M: [f64; 3] = [0.2, 0.8, 1.5];

/// The foliage a body with its feet at `feet` (world) on `face`, `height` tall, is in: the first
/// passable, non-liquid, non-air block at its shins, waist or head (0: none).
pub fn foliage_at(
    feet: [f64; 3],
    face: FaceFrame,
    height: f64,
    mut voxel: impl FnMut([i32; 3]) -> StateId,
) -> StateId {
    for dy in BRUSH_AT_M.into_iter().filter(|&dy| dy < height) {
        let m = voxel(dwell_world::cell_of_pos([
            feet[0],
            face.y(face.y(feet[1]) + dy),
            feet[2],
        ]));
        if m == 0 {
            continue;
        }
        let Some(i) = dwell_blocks::try_info(m) else {
            continue;
        };
        if !i.liquid && (i.passable || !i.solid) {
            return m;
        }
    }
    0
}

#[derive(Clone, Copy, Debug)]
pub struct BrushSounds {
    /// Metres moved through foliage off its feet since the last rustle.
    moved: f64,
    /// Seconds since the last rustle.
    since: f64,
}

impl Default for BrushSounds {
    fn default() -> Self {
        Self {
            moved: 0.0,
            since: BRUSH_MIN_S,
        }
    }
}

impl BrushSounds {
    /// One update: the foliage the body is in (`foliage_at`; 0: none), its speed (m/s), the time
    /// since the last update (s) and its `stride`; a rustle at `at` (None: at the listener).
    pub fn update(
        &mut self,
        foliage: StateId,
        speed: f64,
        dt: f64,
        stride: Stride,
        at: Option<[f64; 3]>,
        out: &mut Vec<SoundEvent>,
    ) {
        self.since += dt;
        if foliage == 0 {
            self.moved = 0.0;
            return;
        }
        let due = if stride.on_feet {
            self.moved = 0.0;
            stride.stepped
        } else {
            self.moved += speed * dt;
            self.moved >= BRUSH_EVERY_M
        };
        if !due || self.since < BRUSH_MIN_S {
            return;
        }
        let Some(i) = speed_intensity(SoundKind::Brush, speed) else {
            return;
        };
        self.moved = 0.0;
        self.since = 0.0;
        out.push(SoundEvent::new(SoundKind::Brush, foliage, i, at));
    }
}

/// The ground items a player steps onto (AUDIO.md §5, sounds round 2b: a stick snaps): the cell
/// its feet are in while on the ground; a new one is stepped onto once.
#[derive(Clone, Copy, Debug, Default)]
pub struct GroundSteps {
    last: Option<[i32; 3]>,
}

impl GroundSteps {
    /// The feet's cell now (None: not on the ground): the cell stepped onto, if it is a new one.
    pub fn update(&mut self, feet_cell: Option<[i32; 3]>) -> Option<[i32; 3]> {
        let new = feet_cell.filter(|c| self.last != Some(*c));
        self.last = feet_cell;
        new
    }
}

/// At most one ground item's snap (a stick stepped onto) a body this often (s): a run over a
/// heap of sticks doesn't crackle on every cell.
pub const SNAP_MIN_S: f64 = 0.15;

/// A body brushing through foliage and stepping onto ground items (AUDIO.md §5 `brush`,
/// `stick_snap`): the local player's (at the listener) and each remote player's (at its feet,
/// `RemoteContacts`), one rule for both.
#[derive(Clone, Copy, Debug)]
pub struct ContactSounds {
    brush: BrushSounds,
    steps: GroundSteps,
    /// Seconds since the last snap.
    snap_since: f64,
}

impl Default for ContactSounds {
    fn default() -> Self {
        Self {
            brush: BrushSounds::default(),
            steps: GroundSteps::default(),
            snap_since: SNAP_MIN_S,
        }
    }
}

impl ContactSounds {
    /// One update: the foliage the body is in (`foliage_at`), its speed (m/s), the time since the
    /// last update (s), its `stride` (its footsteps this update), the open cell its feet stand in
    /// while on the ground (None: off it), and `ground_item(cell)`: the point and step event of a
    /// ground item there that has one. A rustle sounds at `at` (None: the listener), a step at
    /// its item.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        foliage: StateId,
        speed: f64,
        dt: f64,
        stride: Stride,
        feet_cell: Option<[i32; 3]>,
        at: Option<[f64; 3]>,
        ground_item: impl FnOnce([i32; 3]) -> Option<([f64; 3], SoundKind)>,
        out: &mut Vec<SoundEvent>,
    ) {
        self.brush.update(foliage, speed, dt, stride, at, out);
        self.snap_since += dt;
        if let Some(cell) = self.steps.update(feet_cell)
            && self.snap_since >= SNAP_MIN_S
            && let Some((point, kind)) = ground_item(cell)
        {
            self.snap_since = 0.0;
            out.push(SoundEvent::new(kind, 0, 1.0, Some(point)));
        }
    }
}

/// A remote player as its contact sounds read it (the replicated snapshot's, interpolated).
#[derive(Clone, Copy, Debug)]
pub struct ContactSample {
    pub player_id: u16,
    pub feet: [f64; 3],
    pub face: FaceFrame,
    /// m/s.
    pub speed: f64,
    pub grounded: bool,
    /// The body's height (m): foliage is looked for below it.
    pub height: f64,
    /// Its footsteps this update (its `MoverSounds`', `Stride::of`).
    pub stride: Stride,
}

/// The open cell a body's feet stand in (just above the ground's top) on `face`.
pub fn feet_cell(feet: [f64; 3], face: FaceFrame) -> [i32; 3] {
    dwell_world::cell_of_pos([feet[0], face.y(face.y(feet[1]) + 0.1), feet[2]])
}

/// Other players brushing through leaves and snapping sticks (AUDIO.md §2e), heard at their
/// positions from the replicated snapshots alone (no wire message): each player its own
/// `ContactSounds`, so each is rate-limited by itself. The local player is never one of them
/// (its own sounds come from its prediction): a sample with `local`'s id is passed over.
#[derive(Clone, Debug, Default)]
pub struct RemoteContacts {
    players: dwell_world::hash::HashMap<u16, ContactSounds>,
}

impl RemoteContacts {
    /// One update of every remote player drawn (`samples`; those gone are forgotten), `dt` (s)
    /// since the last; `voxel` reads the world, `ground_item` as `ContactSounds::update`'s.
    pub fn update(
        &mut self,
        local: u16,
        samples: &[ContactSample],
        dt: f64,
        mut voxel: impl FnMut([i32; 3]) -> StateId,
        mut ground_item: impl FnMut([i32; 3]) -> Option<([f64; 3], SoundKind)>,
        out: &mut Vec<SoundEvent>,
    ) {
        let samples = || samples.iter().filter(|s| s.player_id != local);
        self.players
            .retain(|id, _| samples().any(|s| s.player_id == *id));
        for s in samples() {
            let foliage = foliage_at(s.feet, s.face, s.height, &mut voxel);
            let cell = s.grounded.then(|| feet_cell(s.feet, s.face));
            self.players.entry(s.player_id).or_default().update(
                foliage,
                s.speed,
                dt,
                s.stride,
                cell,
                Some(s.feet),
                &mut ground_item,
                out,
            );
        }
    }

    /// The players being followed (for tests).
    pub fn len(&self) -> usize {
        self.players.len()
    }

    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_blocks::require;

    const DT: f64 = 1.0 / 60.0;

    fn stone() -> StateId {
        require("dwell:stone")
    }

    fn standing() -> MoverSample {
        MoverSample {
            feet: [0.0, 10.0, 0.0],
            velocity: [0.0; 3],
            state: PlayerState::Idle,
            grounded: true,
            crouched: false,
            face: FaceFrame::A,
            submerged: 0.0,
            carry: [0.0; 3],
            ground: stone(),
            body: 0,
            walls: [0; 4],
            ceiling: 0,
            dt: DT,
        }
    }

    /// Moves along +x at `speed` for `seconds` in `state`, returning the events.
    fn walk(
        m: &mut MoverSounds,
        s: &mut MoverSample,
        state: PlayerState,
        speed: f64,
        seconds: f64,
    ) -> Vec<SoundEvent> {
        let mut out = Vec::new();
        s.state = state;
        s.velocity = [speed, 0.0, 0.0];
        for _ in 0..(seconds / DT).round() as usize {
            s.feet[0] += speed * DT;
            m.update(s, None, &mut out);
        }
        out
    }

    fn kinds(events: &[SoundEvent]) -> Vec<SoundKind> {
        events.iter().map(|e| e.kind).collect()
    }

    dwell_world::both_faces!(
        /// Owner, 2026-11-09 (walking played while still on moving bodies): strides are walked over
        /// the ground carrying the mover. Walking at 5 m/s against a raft's 5 m/s, still in the
        /// world, steps as on the ground; carried while still, none. Red before: the steps
        /// counted the world's distance, none against the raft.
        fn footsteps_are_walked_over_the_moving_ground_carrying_the_mover(f: FaceFrame) {
            let steps = |state, world: f64, carry: f64| {
                let mut m = MoverSounds::new();
                let mut s = MoverSample {
                    feet: f.point([0.0, 10.0, 0.0]),
                    face: f,
                    carry: f.vector([carry, 0.0, 0.0]),
                    ..standing()
                };
                m.update(&s, None, &mut Vec::new());
                s.state = state;
                s.velocity = f.vector([world, 0.0, 0.0]);
                let mut out = Vec::new();
                for _ in 0..(10.0 / DT).round() as usize {
                    s.feet[0] += world * DT;
                    m.update(&s, None, &mut out);
                }
                out.iter().filter(|e| e.kind == SoundKind::Step).count()
            };
            let ground = steps(PlayerState::Walking, 5.0, 0.0);
            assert!(ground > 10, "{ground}");
            assert_eq!(steps(PlayerState::Walking, 0.0, -5.0), ground);
            assert_eq!(steps(PlayerState::Walking, 8.0, 3.0), ground);
            assert_eq!(steps(PlayerState::Idle, 3.0, 3.0), 0);
        }
    );

    #[test]
    fn footsteps_follow_distance_travelled_at_each_gaits_stride() {
        for (state, gait, speed) in [
            (PlayerState::Walking, Gait::Walk, 5.0),
            (PlayerState::Running, Gait::Run, 8.0),
            (PlayerState::Crouching, Gait::Crouch, 1.6),
        ] {
            let (mut m, mut s) = (MoverSounds::new(), standing());
            m.update(&s, None, &mut Vec::new());
            let seconds = 10.0;
            let steps: Vec<_> = walk(&mut m, &mut s, state, speed, seconds)
                .into_iter()
                .filter(|e| e.kind == SoundKind::Step)
                .collect();
            // The first step comes after (1 − FIRST_STEP_AT) of a stride, then one per stride.
            let distance = speed * seconds;
            let expected = ((distance - (1.0 - FIRST_STEP_AT) * gait.stride()) / gait.stride())
                .floor() as usize
                + 1;
            assert_eq!(steps.len(), expected, "{state:?}");
            assert!(
                steps
                    .iter()
                    .all(|e| e.gait == Some(gait) && e.block == stone() && e.at.is_none())
            );
        }
        // Faster gaits step more often in time, though each stride is longer.
        assert!(Gait::Run.stride() / 8.0 < Gait::Walk.stride() / 5.0);
    }

    #[test]
    fn standing_still_and_flying_make_no_footsteps() {
        let (mut m, mut s) = (MoverSounds::new(), standing());
        m.update(&s, None, &mut Vec::new());
        assert!(walk(&mut m, &mut s, PlayerState::Idle, 0.0, 3.0).is_empty());
        s.grounded = false;
        let flying = walk(&mut m, &mut s, PlayerState::Flying, 10.0, 3.0);
        assert!(
            flying.iter().all(|e| e.kind != SoundKind::Step),
            "{flying:?}"
        );
    }

    #[test]
    fn steps_sound_of_the_block_underfoot_and_keep_the_last_one_over_a_gap() {
        let (mut m, mut s) = (MoverSounds::new(), standing());
        s.ground = require("dwell:sand");
        m.update(&s, None, &mut Vec::new());
        let events = walk(&mut m, &mut s, PlayerState::Walking, 5.0, 2.0);
        assert!(events.iter().all(|e| e.block == require("dwell:sand")));
        s.ground = 0; // over an edge: still the sand
        let events = walk(&mut m, &mut s, PlayerState::Walking, 5.0, 2.0);
        assert!(!events.is_empty() && events.iter().all(|e| e.block == require("dwell:sand")));
    }

    fn fall_and_land(impact: f64) -> Vec<SoundEvent> {
        let (mut m, mut s) = (MoverSounds::new(), standing());
        m.update(&s, None, &mut Vec::new());
        s.grounded = false;
        s.state = PlayerState::Falling;
        s.velocity = [0.0, -impact, 0.0];
        m.update(&s, None, &mut Vec::new());
        s.grounded = true;
        s.state = PlayerState::Idle;
        s.velocity = [0.0; 3];
        let mut out = Vec::new();
        m.update(&s, None, &mut out);
        out
    }

    #[test]
    fn landing_intensity_follows_impact_speed() {
        let (min, full) = SoundKind::Land.speed().expect("land has a speed range");
        assert!(
            fall_and_land(min * 0.5).is_empty(),
            "a small drop is silent"
        );
        let soft = fall_and_land(min + 1.0);
        let hard = fall_and_land(full - 1.0);
        let huge = fall_and_land(full * 3.0);
        assert_eq!(kinds(&soft), [SoundKind::Land]);
        assert!(soft[0].intensity < hard[0].intensity, "{soft:?} {hard:?}");
        assert_eq!(huge[0].intensity, 1.0);
        assert_eq!(soft[0].block, stone());
    }

    /// A face-local session (BIFACIAL_WORLD.md §11): a walk, a jump and a landing, a crouch, a run
    /// into a slide down a slope, a dive into water and out, a flight up and down.
    fn session() -> Vec<MoverSample> {
        let mut out = Vec::new();
        let mut s = standing();
        let mut push = |s: &mut MoverSample, ticks: usize| {
            for _ in 0..ticks {
                for k in 0..3 {
                    s.feet[k] += s.velocity[k] * DT;
                }
                out.push(*s);
            }
        };
        push(&mut s, 1);
        (s.state, s.velocity) = (PlayerState::Walking, [4.0, 0.0, 0.0]);
        push(&mut s, 60);
        (s.state, s.grounded, s.velocity) = (PlayerState::Jumping, false, [4.0, 6.0, 0.0]);
        push(&mut s, 20);
        (s.state, s.velocity) = (PlayerState::Falling, [4.0, -12.0, 0.0]);
        push(&mut s, 10);
        (s.state, s.grounded, s.velocity) = (PlayerState::Idle, true, [0.0; 3]);
        push(&mut s, 5);
        s.crouched = true;
        push(&mut s, 5);
        s.crouched = false;
        (s.state, s.velocity) = (PlayerState::Running, [8.0, 0.0, 0.0]);
        push(&mut s, 30);
        (s.state, s.velocity) = (PlayerState::Sliding, [8.0, -5.0, 0.0]);
        push(&mut s, 30);
        // Into a wall at +x: a hit.
        (s.state, s.velocity, s.walls) =
            (PlayerState::Running, [9.0, 0.0, 0.0], [stone(), 0, 0, 0]);
        push(&mut s, 10);
        s.velocity = [0.0; 3];
        push(&mut s, 5);
        s.walls = [0; 4];
        (s.state, s.grounded, s.velocity) = (PlayerState::Falling, false, [0.0, -9.0, 0.0]);
        s.ground = 0;
        push(&mut s, 5);
        (s.state, s.submerged, s.body) = (PlayerState::Swimming, 1.0, require("dwell:water"));
        s.velocity = [2.0, -1.0, 0.0];
        push(&mut s, 40);
        (s.submerged, s.body, s.state) = (0.0, 0, PlayerState::Flying);
        s.velocity = [0.0, 12.0, 0.0];
        push(&mut s, 20);
        s.velocity = [0.0, -12.0, 0.0];
        push(&mut s, 20);
        out
    }

    dwell_world::both_faces!(
        /// Every sound a face-local session makes, it makes on either face: the session placed on
        /// face `f` through its frame sounds as it does face-local (a landing is toward the face's
        /// ground, a rise away from it).
        fn a_session_sounds_the_same_on_either_face(f: FaceFrame) {
            let run = |frame: FaceFrame| {
                let mut m = MoverSounds::new();
                let mut out = Vec::new();
                for s in session() {
                    let placed = MoverSample {
                        feet: frame.point(s.feet),
                        velocity: frame.vector(s.velocity),
                        face: frame,
                        ..s
                    };
                    m.update(&placed, None, &mut out);
                }
                out
            };
            let local = run(FaceFrame::A);
            let all = kinds(&local);
            for k in [
                SoundKind::Step,
                SoundKind::Jump,
                SoundKind::Land,
                SoundKind::CrouchOn,
                SoundKind::Sprint,
                SoundKind::Slide,
                SoundKind::Enter,
                SoundKind::Rise,
                SoundKind::Dive,
                SoundKind::Impact,
            ] {
                assert!(all.contains(&k), "the session sounds {k:?}: {all:?}");
            }
            assert_eq!(run(f), local, "{:?}", f.face);
        }
    );

    #[test]
    fn jump_crouch_sprint_and_slide_sound_once_on_their_transitions() {
        let (mut m, mut s) = (MoverSounds::new(), standing());
        m.update(&s, None, &mut Vec::new());
        let mut out = Vec::new();
        s.state = PlayerState::Jumping;
        m.update(&s, None, &mut out);
        s.grounded = false;
        m.update(&s, None, &mut out);
        assert_eq!(kinds(&out), [SoundKind::Jump]);
        assert_eq!(out[0].block, stone());

        let (mut m, mut s) = (MoverSounds::new(), standing());
        m.update(&s, None, &mut Vec::new());
        let mut out = Vec::new();
        s.crouched = true;
        m.update(&s, None, &mut out);
        m.update(&s, None, &mut out);
        s.crouched = false;
        m.update(&s, None, &mut out);
        assert_eq!(kinds(&out), [SoundKind::CrouchOn, SoundKind::CrouchOff]);

        let (mut m, mut s) = (MoverSounds::new(), standing());
        m.update(&s, None, &mut Vec::new());
        let run = walk(&mut m, &mut s, PlayerState::Running, 8.0, 0.1);
        assert_eq!(kinds(&run), [SoundKind::Sprint]);
        let slide = walk(&mut m, &mut s, PlayerState::Sliding, 8.0, 0.1);
        assert_eq!(kinds(&slide), [SoundKind::Slide]);
    }

    /// Slides along +x for `seconds` at a speed following `speed(t)`; returns the events.
    fn slide_for(
        m: &mut MoverSounds,
        s: &mut MoverSample,
        seconds: f64,
        speed: impl Fn(f64) -> f64,
    ) -> Vec<SoundEvent> {
        let mut out = Vec::new();
        s.state = PlayerState::Sliding;
        for k in 0..(seconds / DT).round() as usize {
            let v = speed(k as f64 * DT);
            s.velocity = [v * 0.7, -v * 0.7, 0.0];
            s.feet[0] += v * 0.7 * DT;
            s.feet[1] -= v * 0.7 * DT;
            m.update(s, None, &mut out);
        }
        out
    }

    #[test]
    fn a_slide_starts_a_voice_follows_its_speed_and_ends_with_a_stop() {
        let (mut m, mut s) = (MoverSounds::new(), standing());
        m.update(&s, None, &mut Vec::new());
        // Speeding up from 1 to 13 m/s over a second, then stepping off onto the flat.
        let events = slide_for(&mut m, &mut s, 1.0, |t| 1.0 + 12.0 * t);
        assert_eq!(events[0].kind, SoundKind::Slide);
        assert_eq!(events[0].block, stone());
        assert_eq!(events[0].voice, Some(0));
        let (min, full) = SoundKind::Slide.speed().unwrap();
        assert!((f64::from(events[0].intensity) - (1.0 - min) / (full - min)).abs() < 0.02);
        let speeds: Vec<f32> = events[1..]
            .iter()
            .inspect(|e| assert_eq!(e.kind, SoundKind::SlideSpeed))
            .map(|e| e.intensity)
            .collect();
        // Updates rise with the speed, at most every SLIDE_UPDATE_EVERY.
        assert!(speeds.len() >= 10 && speeds.len() <= 21, "{}", speeds.len());
        assert!(speeds.windows(2).all(|w| w[1] > w[0]));
        assert!(*speeds.last().unwrap() > 0.8);
        // Off the slope: the voice goes silent at once, and the slide ends after the hold with
        // the stop sound at the speed it slid at.
        s.state = PlayerState::Walking;
        s.velocity = [5.0, 0.0, 0.0];
        let mut end = Vec::new();
        for _ in 0..(0.3 / DT) as usize {
            m.update(&s, None, &mut end);
        }
        let end: Vec<_> = end.iter().filter(|e| e.kind != SoundKind::Step).collect();
        assert_eq!(
            end.iter().map(|e| e.kind).collect::<Vec<_>>(),
            [SoundKind::SlideSpeed, SoundKind::SlideEnd]
        );
        assert_eq!(end[0].intensity, 0.0);
        assert!(end[1].intensity > 0.8);
    }

    #[test]
    fn a_short_pause_keeps_the_slide_and_slow_creeping_makes_no_sound() {
        let (mut m, mut s) = (MoverSounds::new(), standing());
        m.update(&s, None, &mut Vec::new());
        let mut events = slide_for(&mut m, &mut s, 0.5, |_| 6.0);
        // A few ticks walking (the state flickers at a ramp's foot), then sliding again.
        s.state = PlayerState::Walking;
        for _ in 0..4 {
            m.update(&s, None, &mut events);
        }
        events.extend(slide_for(&mut m, &mut s, 0.5, |_| 6.0));
        let starts = events.iter().filter(|e| e.kind == SoundKind::Slide).count();
        assert_eq!(starts, 1, "{events:?}");
        assert!(events.iter().all(|e| e.kind != SoundKind::SlideEnd));
        // Below the slide's start speed: nothing.
        let (mut m, mut s) = (MoverSounds::new(), standing());
        m.update(&s, None, &mut Vec::new());
        assert!(slide_for(&mut m, &mut s, 1.0, |_| 0.2).is_empty());
    }

    #[test]
    fn a_slide_onto_another_surface_restarts_its_voice_on_that_set() {
        let (mut m, mut s) = (MoverSounds::new(), standing());
        m.update(&s, None, &mut Vec::new());
        slide_for(&mut m, &mut s, 0.3, |_| 6.0);
        s.ground = require("dwell:sand");
        let events = slide_for(&mut m, &mut s, 0.3, |_| 6.0);
        assert_eq!(events[0].kind, SoundKind::Slide);
        assert_eq!(events[0].block, require("dwell:sand"));
    }

    #[test]
    fn another_players_slide_is_spatial_on_its_own_voice_and_ends_when_it_leaves() {
        let mut m = MoverSounds::with_voice(7);
        let mut s = standing();
        let at = Some([1.0, 2.0, 3.0]);
        m.update(&s, at, &mut Vec::new());
        s.state = PlayerState::Sliding;
        s.velocity = [6.0, -6.0, 0.0];
        let mut out = Vec::new();
        m.update(&s, at, &mut out);
        assert_eq!(out[0].kind, SoundKind::Slide);
        assert_eq!((out[0].voice, out[0].at), (Some(7), at));
        let mut gone = Vec::new();
        m.finish(at, &mut gone);
        assert_eq!(gone.len(), 1);
        assert_eq!(
            (gone[0].kind, gone[0].voice),
            (SoundKind::SlideEnd, Some(7))
        );
        // The voice travels in the flags: bits 4 and up hold voice + 1.
        let v = encode(&out);
        assert_eq!(v[6] as u32 >> 4, 8);
        assert_eq!(v[6] as u32 & 1, 1);
    }

    #[test]
    fn entering_water_splashes_by_speed_and_swimming_strokes_by_distance() {
        let water = require("dwell:water");
        let (mut m, mut s) = (MoverSounds::new(), standing());
        m.update(&s, None, &mut Vec::new());
        s.grounded = false;
        s.state = PlayerState::Falling;
        s.velocity = [0.0, -8.0, 0.0];
        s.body = water;
        s.submerged = 0.6;
        let mut out = Vec::new();
        m.update(&s, None, &mut out);
        assert_eq!(kinds(&out), [SoundKind::Enter]);
        assert_eq!(out[0].block, water);
        assert!(out[0].intensity > 0.3);
        let strokes = walk(&mut m, &mut s, PlayerState::Swimming, 3.0, 5.0);
        let n = strokes.iter().filter(|e| e.kind == SoundKind::Swim).count();
        assert!(
            n >= (15.0 / Gait::Swim.stride()) as usize - 1
                && n <= (15.0 / Gait::Swim.stride()) as usize + 1,
            "{n}"
        );
        // Wading out slowly is quiet but heard; drifting out below the exit speed is silent.
        s.submerged = 0.0;
        s.velocity = [2.0, 0.0, 0.0];
        let mut out = Vec::new();
        m.update(&s, None, &mut out);
        assert!(kinds(&out).contains(&SoundKind::Exit));
    }

    dwell_world::both_faces!(
        /// BUGS.md #104 (owner, 2026-11-12: "swimming underwater sounds like swimming on the water
        /// surface"): a stroke with the head under plays the set's soft, deep `swim_under`; afloat
        /// at the surface (the controller's rest share, 0.7) the calm `swim`. Red before: every
        /// stroke was `Swim` whatever the depth.
        fn a_swim_stroke_is_chosen_by_how_deep_the_head_is(f: FaceFrame) {
            let water = require("dwell:water");
            let strokes = |submerged: f64| {
                let mut m = MoverSounds::new();
                let mut s = MoverSample {
                    feet: f.point([0.0, 10.0, 0.0]),
                    face: f,
                    grounded: false,
                    body: water,
                    ground: 0,
                    submerged,
                    ..standing()
                };
                m.update(&s, None, &mut Vec::new());
                let mut out = Vec::new();
                s.state = PlayerState::Swimming;
                s.velocity = f.vector([3.0, 0.0, 0.0]);
                for _ in 0..600 {
                    s.feet = std::array::from_fn(|k| s.feet[k] + s.velocity[k] * DT);
                    m.update(&s, None, &mut out);
                }
                let swim = out.iter().filter(|e| e.kind == SoundKind::Swim).count();
                let under = out
                    .iter()
                    .filter(|e| e.kind == SoundKind::SwimUnder)
                    .count();
                assert!(out.iter().all(|e| !matches!(
                    e.kind,
                    SoundKind::Swim | SoundKind::SwimUnder
                ) || e.block == water));
                (swim, under)
            };
            let (surface, deep) = (strokes(0.7), strokes(1.0));
            assert!(
                surface.0 > 0 && surface.1 == 0,
                "at the surface: {surface:?}"
            );
            assert!(deep.0 == 0 && deep.1 > 0, "head under: {deep:?}");
            assert_eq!(
                surface.0, deep.1,
                "the same strokes, only the sound differs"
            );
            // Just under the head line, and just over it.
            assert!(strokes(head_fraction()).1 > 0);
            assert!(strokes(head_fraction() - 0.01).1 == 0);
        }
    );

    dwell_world::both_faces!(
        /// Another player's depth is read from the blocks round it (`Surroundings::submerged`):
        /// water to its chest is afloat (a surface stroke), water over its head line is under.
        fn another_swimmers_depth_is_read_at_its_head_line(f: FaceFrame) {
            let water = require("dwell:water");
            // Water from face-local y 9.0 up to `top`; the body's feet at 10.0, 1.8 m tall.
            let read = |top: f64| {
                Surroundings::read(f.point([0.5, 10.0, 0.5]), f, 0.3, 1.8, |c| {
                    let y = f.y(dwell_world::centre_of_cell(c)[1]);
                    if (9.0..top).contains(&y) { water } else { 0 }
                })
                .submerged()
            };
            assert_eq!(read(10.0), 0.0, "dry");
            assert_eq!(read(11.0), 0.7, "chest deep: afloat");
            assert_eq!(read(12.5), 1.0, "over the head line");
        }
    );

    /// BUGS.md #104 (owner, 2026-11-12: "Swimming in the surface is too loud and high ... like a
    /// kid learning to swim splashing"): at the swim speed (3 m/s) a stroke sounds once a stroke
    /// of the data's stride — about once a second, never two within most of a second — not every
    /// tick. Red before: the 2.4 m stride gave a stroke every 0.8 s.
    #[test]
    fn surface_strokes_sound_once_a_stroke_about_once_a_second() {
        let water = require("dwell:water");
        let mut m = MoverSounds::new();
        let mut s = MoverSample {
            grounded: false,
            body: water,
            ground: 0,
            submerged: 0.7,
            ..standing()
        };
        m.update(&s, None, &mut Vec::new());
        s.state = PlayerState::Swimming;
        s.velocity = [3.0, 0.0, 0.0];
        let mut at = Vec::new();
        for i in 0..(20.0 / DT) as usize {
            s.feet[0] += 3.0 * DT;
            let mut out = Vec::new();
            m.update(&s, None, &mut out);
            at.extend(
                out.iter()
                    .filter(|e| e.kind == SoundKind::Swim)
                    .map(|_| i as f64 * DT),
            );
        }
        assert!(
            (19..=21).contains(&at.len()),
            "{} strokes in 20 s",
            at.len()
        );
        let closest = at.windows(2).map(|w| w[1] - w[0]).fold(f64::MAX, f64::min);
        assert!(closest >= 0.95, "strokes {closest} s apart");
    }

    #[test]
    fn climbing_a_ladder_sounds_its_rungs() {
        let ladder = require("dwell:ladder[facing=north,flooded=false]");
        let (mut m, mut s) = (MoverSounds::new(), standing());
        m.update(&s, None, &mut Vec::new());
        s.state = PlayerState::Climbing;
        s.grounded = false;
        s.body = ladder;
        let mut out = Vec::new();
        for _ in 0..120 {
            s.feet[1] += 2.0 * DT;
            m.update(&s, None, &mut out);
        }
        let rungs: Vec<_> = out.iter().filter(|e| e.kind == SoundKind::Climb).collect();
        assert!(
            rungs.len() >= 4
                && rungs
                    .iter()
                    .all(|e| e.block == ladder && e.gait == Some(Gait::Climb)),
            "{out:?}"
        );
    }

    #[test]
    fn a_teleport_makes_no_sound() {
        let (mut m, mut s) = (MoverSounds::new(), standing());
        m.update(&s, None, &mut Vec::new());
        s.state = PlayerState::Walking;
        s.feet[0] += 100.0;
        let mut out = Vec::new();
        m.update(&s, None, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn flight_whooshes_when_it_starts_rising_or_diving() {
        let (mut m, mut s) = (MoverSounds::new(), standing());
        s.state = PlayerState::Flying;
        s.grounded = false;
        m.update(&s, None, &mut Vec::new());
        let mut out = Vec::new();
        for v in [0.0, 5.0, 6.0, 6.0, 0.0, -8.0, -8.0] {
            s.velocity = [0.0, v, 0.0];
            m.update(&s, None, &mut out);
        }
        assert_eq!(kinds(&out), [SoundKind::Rise, SoundKind::Dive]);
    }

    #[test]
    fn our_edits_sound_at_once_and_their_echo_is_not_played_again() {
        let mut e = EditSounds::new();
        let mut out = Vec::new();
        e.local_break([1, 2, 3], stone(), 0.0, &mut out);
        e.local_place([4, 5, 6], require("dwell:dirt"), 10.0, &mut out);
        assert_eq!(kinds(&out), [SoundKind::Break, SoundKind::Place]);
        assert_eq!(out[0].block, stone());
        assert_eq!(out[0].at, Some(dwell_world::centre_of_cell([1, 2, 3])));
        let mut echo = Vec::new();
        e.changes(
            &[
                CellChange {
                    cell: [1, 2, 3],
                    old: stone(),
                    new: 0,
                },
                CellChange {
                    cell: [4, 5, 6],
                    old: 0,
                    new: require("dwell:dirt"),
                },
            ],
            200.0,
            &mut echo,
        );
        assert!(echo.is_empty(), "{echo:?}");
    }

    #[test]
    fn other_players_edits_sound_where_they_happen() {
        let mut e = EditSounds::new();
        let mut out = Vec::new();
        e.changes(
            &[
                CellChange {
                    cell: [10, 0, 0],
                    old: require("dwell:sand"),
                    new: 0,
                },
                CellChange {
                    cell: [0, 0, 10],
                    old: 0,
                    new: require("dwell:grass"),
                },
                CellChange {
                    cell: [0, 0, 0],
                    old: stone(),
                    new: stone(),
                },
            ],
            0.0,
            &mut out,
        );
        assert_eq!(kinds(&out), [SoundKind::Break, SoundKind::Place]);
        assert_eq!(out[0].block, require("dwell:sand"));
        assert_eq!(out[0].at, Some(dwell_world::centre_of_cell([10, 0, 0])));
        assert_eq!(out[1].block, require("dwell:grass"));
        // A stale pending edit no longer hides a later change in its cell.
        let mut e = EditSounds::new();
        e.local_break([1, 1, 1], stone(), 0.0, &mut Vec::new());
        let mut out = Vec::new();
        e.changes(
            &[CellChange {
                cell: [1, 1, 1],
                old: stone(),
                new: 0,
            }],
            ECHO_MS + 1.0,
            &mut out,
        );
        assert_eq!(kinds(&out), [SoundKind::Break]);
    }

    #[test]
    fn many_changes_at_once_are_capped() {
        let mut e = EditSounds::new();
        let changes: Vec<_> = (0..50)
            .map(|i| CellChange {
                cell: [i, 0, 0],
                old: stone(),
                new: 0,
            })
            .collect();
        let mut out = Vec::new();
        e.changes(&changes, 0.0, &mut out);
        assert_eq!(out.len(), MAX_EDIT_SOUNDS);
    }

    // --- impacts -------------------------------------------------------------------------------

    /// Toward `dir` (one of `HIT_DIRECTIONS`) at `speed` for a third of a second, sampled every
    /// `dt` (a tick: ours; a snapshot interval: another player's), with a stone surface touching
    /// there (`wall`), then stopped dead by it over `stop_over` samples. The hits.
    fn hit_every(dt: f64, dir: usize, speed: f64, stop_over: usize, wall: bool) -> Vec<SoundEvent> {
        let mut m = ImpactSounds::new();
        let mut s = MoverSample { dt, ..standing() };
        m.update(&s, None, &mut Vec::new());
        let n = HIT_DIRECTIONS[dir];
        if wall {
            if dir < 4 {
                s.walls[dir] = stone();
            } else {
                s.ceiling = stone();
            }
        }
        (s.state, s.grounded) = if dir < 4 {
            (PlayerState::Running, true)
        } else {
            (PlayerState::Jumping, false)
        };
        let v = n.map(|c| c * speed);
        s.velocity = v;
        for _ in 0..(0.33 / dt) as usize {
            m.update(&s, None, &mut Vec::new());
        }
        let mut out = Vec::new();
        for k in 1..=stop_over + 3 {
            let f = 1.0 - (k as f64 / stop_over as f64).min(1.0);
            s.velocity = v.map(|c| c * f);
            m.update(&s, None, &mut out);
        }
        out.retain(|e| e.kind == SoundKind::Impact);
        out
    }

    fn hit(dir: usize, speed: f64, stop_over: usize, wall: bool) -> Vec<SoundEvent> {
        hit_every(DT, dir, speed, stop_over, wall)
    }

    #[test]
    fn a_wall_hit_at_25_m_s_is_a_loud_impact_and_at_8_a_soft_thud() {
        // Feedback, not damage: walls never hurt (PLAYER_CONTROLLER.md §6.10), so no hurt follows
        // either (the hurt is the server's Damage event's, `player_event_sound`).
        let hard = hit(2, 25.0, 1, true);
        assert_eq!(kinds(&hard), [SoundKind::Impact], "{hard:?}");
        assert!(hard[0].intensity >= 0.99, "loud: {hard:?}");
        assert_eq!(hard[0].block, stone(), "the wall's sound set");
        // At sprint speed: a soft thud.
        let soft = hit(2, 8.0, 1, true);
        assert_eq!(kinds(&soft), [SoundKind::Impact], "{soft:?}");
        assert!(
            soft[0].intensity > 0.1 && soft[0].intensity < 0.4,
            "soft: {soft:?}"
        );
        // Louder by speed throughout.
        let level = |v: f64| hit(2, v, 1, true).first().map_or(0.0, |e| e.intensity);
        assert!(level(1.5) == 0.0, "a nudge is silent");
        assert!(level(5.0) < level(8.0) && level(8.0) < level(15.0) && level(15.0) < level(22.0));
    }

    #[test]
    fn every_side_and_the_ceiling_hit_once_and_another_players_snapshots_hit_as_ours() {
        for dir in 0..5 {
            let ours = hit(dir, 18.0, 1, true);
            assert_eq!(ours.len(), 1, "dir {dir}: {ours:?}");
            // Braking into it over two ticks is still one hit.
            assert_eq!(kinds(&hit(dir, 18.0, 2, true)), kinds(&ours), "dir {dir}");
            // Another player's snapshots, 20 a second.
            let theirs = hit_every(
                1.0 / f64::from(dwell_protocol::SNAPSHOT_HZ),
                dir,
                18.0,
                1,
                true,
            );
            assert_eq!(theirs, ours, "dir {dir}");
        }
        // Through MoverSounds: ours hit, another player's frames leave it to its snapshots.
        let (mut ours, mut theirs, mut s) =
            (MoverSounds::new(), MoverSounds::with_voice(3), standing());
        s.walls[2] = stone();
        let mut out = Vec::new();
        for v in [0.0, 15.0, 15.0, 15.0, 0.0] {
            s.velocity = [0.0, 0.0, v];
            ours.update(&s, None, &mut out);
            theirs.update(&s, Some(s.feet), &mut out);
        }
        assert_eq!(kinds(&out), [SoundKind::Impact], "{out:?}");
        assert_eq!(out[0].at, None);
    }

    #[test]
    fn stopping_turning_back_or_brushing_along_a_wall_is_no_hit() {
        // Stopping with nothing touching (in the open, or turning back a step from a wall: the
        // probe only sees a touching one, `a_body_probe_reads_…`).
        assert!(hit(2, 18.0, 1, false).is_empty());
        // Along a wall at +x, moving in z: nothing goes toward it.
        let (mut m, mut s) = (MoverSounds::new(), standing());
        s.walls[0] = stone();
        let along = walk(&mut m, &mut s, PlayerState::Running, 0.0, 0.1);
        s.velocity = [0.0, 0.0, 20.0];
        let mut out = along;
        for _ in 0..30 {
            m.update(&s, None, &mut out);
        }
        assert!(
            kinds(&out).iter().all(|k| *k != SoundKind::Impact),
            "{out:?}"
        );
        // Pressing on into the wall after the hit: once.
        let (mut m, mut s) = (MoverSounds::new(), standing());
        s.walls[2] = stone();
        s.velocity = [0.0, 0.0, 12.0];
        let mut out = Vec::new();
        for i in 0..60 {
            s.velocity[2] = if i < 20 || i % 2 == 0 {
                12.0 * f64::from(u8::from(i < 20))
            } else {
                0.3
            };
            m.update(&s, None, &mut out);
        }
        assert_eq!(
            kinds(&out)
                .iter()
                .filter(|k| **k == SoundKind::Impact)
                .count(),
            1,
            "{out:?}"
        );
    }

    #[test]
    fn swimming_and_climbing_hit_nothing_and_flight_does() {
        let (mut m, mut s) = (MoverSounds::new(), standing());
        s.walls[2] = stone();
        for (state, sounds) in [
            (PlayerState::Swimming, false),
            (PlayerState::Climbing, false),
            (PlayerState::Flying, true),
        ] {
            s.state = state;
            s.velocity = [0.0, 0.0, 15.0];
            for _ in 0..10 {
                m.update(&s, None, &mut Vec::new());
            }
            s.velocity = [0.0; 3];
            let mut out = Vec::new();
            m.update(&s, None, &mut out);
            assert_eq!(
                out.iter().any(|e| e.kind == SoundKind::Impact),
                sounds,
                "{state:?}"
            );
        }
    }

    #[test]
    fn a_body_probe_reads_its_walls_and_ceiling_face_locally() {
        // A body 0.31 m from a wall's face at z = 1 (just touching, past the skin) and 0.1 m under
        // a ceiling, placed on each face through its frame; then a step back from both.
        for f in FaceFrame::under_test() {
            let wall = dwell_world::cell_of_pos(f.point([0.5, 41.1, 1.2]));
            let roof = dwell_world::cell_of_pos(f.point([0.5, 42.25, 0.69]));
            let read = |feet_local: [f64; 3]| {
                Surroundings::read(f.point(feet_local), f, 0.3, 1.8, |c| {
                    if c == wall || c == roof { stone() } else { 0 }
                })
            };
            let near = read([0.5, 40.2, 0.69]);
            assert_eq!(near.walls, [0, 0, stone(), 0], "{:?}", f.face);
            assert_eq!(near.ceiling, stone(), "{:?}", f.face);
            assert_eq!(near.ground, 0);
            let apart = read([0.5, 39.5, 0.5]);
            assert_eq!((apart.walls, apart.ceiling), ([0; 4], 0), "{:?}", f.face);
        }
    }

    // --- health --------------------------------------------------------------------------------

    #[test]
    fn health_events_sound_hurt_by_the_damage_death_and_respawn_for_anyone() {
        let at = Some([1.0, 2.0, 3.0]);
        let small = player_event_sound(PlayerEventKind::Damage, 5, None).expect("hurt");
        let big = player_event_sound(PlayerEventKind::Damage, 100, at).expect("hurt");
        assert_eq!((small.kind, big.kind), (SoundKind::Hurt, SoundKind::Hurt));
        assert!(small.intensity < big.intensity && big.intensity == 1.0);
        assert_eq!(
            (small.at, big.at),
            (None, at),
            "ours at the listener, theirs where they are"
        );
        assert_eq!(
            player_event_sound(PlayerEventKind::Death, 0, at).map(|e| e.kind),
            Some(SoundKind::Death)
        );
        assert_eq!(
            player_event_sound(PlayerEventKind::Respawn, 0, None).map(|e| e.kind),
            Some(SoundKind::Respawn)
        );
        assert_eq!(
            player_event_sound(PlayerEventKind::Knockback, 0, None),
            None
        );
    }

    // --- fires ---------------------------------------------------------------------------------

    fn torch() -> StateId {
        require("dwell:torch_upper")
    }

    fn fire(key: u64, x: f64) -> FireSource {
        FireSource {
            key,
            pos: [x, 10.0, 0.0],
            at: Some([x, 10.0, 0.0]),
            block: torch(),
        }
    }

    /// The fires' voices after a second of frames: voice → (intensity, key's x).
    fn fires_after(
        fs: &mut FireSounds,
        listener: [f64; 3],
        sources: &[FireSource],
    ) -> Vec<SoundEvent> {
        let mut out = Vec::new();
        for _ in 0..60 {
            fs.update(listener, sources, 1.0 / 60.0, &mut out);
        }
        out
    }

    #[test]
    fn a_waterfall_rushes_louder_with_more_flow_and_nearer_and_only_the_loudest_few() {
        // docs/WATER_SURFACE_GRID.md §2e (owner, 2026-11-06).
        let r = ambient::FALLS_RADIUS_M;
        let full = ambient::FALLS_FULL_M3S;
        assert!(fall_level(0.0, full) == 1.0 && fall_level(r, full) == 0.0);
        assert!(
            fall_level(5.0, full) > fall_level(10.0, full),
            "nearer is louder"
        );
        assert!(
            fall_level(5.0, full) > fall_level(5.0, 0.1 * full),
            "more flow is louder"
        );
        let fall = |key: u64, x: f64, q: f64| FallSource {
            key,
            pos: [x, 10.0, 0.0],
            discharge: q,
        };
        let mut sources: Vec<FallSource> = (0..(ambient::FALLS_MAX as u64 + 2))
            .map(|i| fall(i, 2.0 + 3.0 * i as f64, full))
            .collect();
        sources.push(fall(99, r + 1.0, full));
        let mut fs = FallSounds::new();
        let mut out = Vec::new();
        for _ in 0..60 {
            fs.update([0.0, 10.0, 0.0], &sources, 1.0 / 60.0, &mut out);
        }
        let playing = fs.playing();
        assert_eq!(playing.len(), ambient::FALLS_MAX);
        assert!(
            playing.iter().all(|&k| k < ambient::FALLS_MAX as u64),
            "{playing:?}"
        );
        let starts: Vec<&SoundEvent> = out.iter().filter(|e| e.kind == SoundKind::Fall).collect();
        assert_eq!(starts.len(), ambient::FALLS_MAX);
        assert!(
            starts
                .iter()
                .all(|e| e.voice.is_some_and(|v| v >= FALL_VOICE))
        );
        // Gone (the falls dried up): their voices stop.
        fs.update([0.0, 10.0, 0.0], &[], 1.0 / 60.0, &mut out);
        assert!(fs.playing().is_empty());
        assert_eq!(
            out.iter().filter(|e| e.kind == SoundKind::FallEnd).count(),
            ambient::FALLS_MAX
        );
    }

    #[test]
    fn a_torch_loop_is_heard_within_its_radius_and_only_the_nearest_few() {
        let r = ambient::FIRE_RADIUS_M;
        let n = ambient::FIRE_MAX;
        assert!(n >= 1);
        // More torches than voices inside the radius, one outside, and a stone light-less block.
        let mut sources: Vec<FireSource> = (0..(n as u64 + 3))
            .map(|i| fire(i, 1.0 + i as f64 * (r - 2.0) / (n as f64 + 3.0)))
            .collect();
        sources.push(fire(100, r + 1.0));
        sources.push(FireSource {
            block: stone(),
            ..fire(101, 0.5)
        });
        let mut fs = FireSounds::new();
        let listener = [0.0, 10.0, 0.0];
        let out = fires_after(&mut fs, listener, &sources);
        let starts: Vec<_> = out.iter().filter(|e| e.kind == SoundKind::Burn).collect();
        assert_eq!(starts.len(), n, "capped at {n}: {out:?}");
        let mut playing = fs.playing();
        playing.sort_unstable();
        assert_eq!(playing, (0..n as u64).collect::<Vec<_>>(), "the nearest");
        assert!(starts.iter().all(|e| e.block == torch() && e.at.is_some()));
        // Each on its own voice, nearer louder.
        let mut voices: Vec<_> = starts.iter().map(|e| e.voice.expect("voiced")).collect();
        voices.dedup();
        assert_eq!(voices.len(), n);
        assert!(
            starts.windows(2).all(|w| w[0].intensity >= w[1].intensity),
            "{starts:?}"
        );
        assert!(fire_level(0.0) == 1.0 && fire_level(r) == 0.0 && fire_level(0.5 * r) > 0.3);
        // Walk away: every voice stops; nothing past the radius starts.
        let out = fires_after(&mut fs, [0.0, 10.0, 3.0 * r], &sources);
        assert_eq!(
            out.iter().filter(|e| e.kind == SoundKind::BurnEnd).count(),
            n,
            "{out:?}"
        );
        assert!(out.iter().all(|e| e.kind != SoundKind::Burn));
        assert!(fs.playing().is_empty());
    }

    #[test]
    fn a_nearer_torch_takes_a_far_ones_voice_and_a_held_one_sounds_at_the_listener() {
        let n = ambient::FIRE_MAX;
        let mut sources: Vec<FireSource> = (0..n as u64).map(|i| fire(i, 4.0 + i as f64)).collect();
        let mut fs = FireSounds::new();
        let listener = [0.0, 10.0, 0.0];
        fires_after(&mut fs, listener, &sources);
        // A torch in our hand: the nearest of all, at the listener.
        sources.push(FireSource {
            key: 1 << 63,
            pos: listener,
            at: None,
            block: require("dwell:torch"),
        });
        let out = fires_after(&mut fs, listener, &sources);
        let farthest = n as u64 - 1;
        assert!(
            !fs.playing().contains(&farthest),
            "the farthest gave up its voice"
        );
        let start = out
            .iter()
            .find(|e| e.kind == SoundKind::Burn)
            .expect("the hand's starts");
        assert_eq!((start.at, start.intensity), (None, 1.0));
        assert_eq!(
            out.iter().filter(|e| e.kind == SoundKind::BurnEnd).count(),
            1
        );
    }

    #[test]
    fn a_moving_torch_updates_its_voice_at_most_ten_times_a_second() {
        let mut fs = FireSounds::new();
        let mut out = Vec::new();
        for i in 0..120 {
            let x = 1.0 + 6.0 * f64::from(i) / 120.0;
            fs.update([0.0, 10.0, 0.0], &[fire(7, x)], 1.0 / 60.0, &mut out);
        }
        let updates = out
            .iter()
            .filter(|e| e.kind == SoundKind::BurnLevel)
            .count();
        assert!((10..=21).contains(&updates), "{updates}");
    }

    #[test]
    fn events_encode_as_seven_numbers() {
        let mut step = SoundEvent::new(SoundKind::Step, 3, 1.0, None);
        step.gait = Some(Gait::Run);
        let land = SoundEvent::new(SoundKind::Land, 2, 0.5, Some([1.0, 2.0, 3.0]));
        let v = encode(&[step, land]);
        assert_eq!(v.len(), 2 * FLOATS_PER_EVENT);
        assert_eq!(
            &v[..7],
            &[
                SoundKind::Step as u8 as f64,
                3.0,
                1.0,
                0.0,
                0.0,
                0.0,
                f64::from((Gait::Run as u8 + 1) << 1)
            ]
        );
        assert_eq!(
            &v[7..],
            &[SoundKind::Land as u8 as f64, 2.0, 0.5, 1.0, 2.0, 3.0, 1.0]
        );
        // A fire's voice, past the players' ids.
        let mut burn = SoundEvent::new(SoundKind::Burn, 3, 1.0, None);
        burn.voice = Some(FIRE_VOICE + 2);
        let v = encode(&[burn]);
        assert_eq!(v[6], f64::from((FIRE_VOICE + 3) << 4));
    }

    #[test]
    fn running_out_of_stamina_breathes_and_air_running_low_strains_then_gasps() {
        // face-a-only: the vitals have no up.
        let mut v = VitalSounds::new();
        let mut out = Vec::new();
        v.update(false, Some(1.0), &mut out);
        v.update(true, Some(1.0), &mut out);
        v.update(true, Some(1.0), &mut out);
        assert_eq!(kinds(&out), vec![SoundKind::Exhausted]);
        out.clear();
        // Under water: the air falls past the strain line once, then surfacing gasps once.
        for k in (10..=100).rev() {
            v.update(false, Some(f64::from(k) / 100.0), &mut out);
        }
        assert_eq!(kinds(&out), vec![SoundKind::Strain]);
        out.clear();
        v.update(false, Some(0.12), &mut out);
        v.update(false, Some(0.15), &mut out);
        assert_eq!(kinds(&out), vec![SoundKind::Gasp]);
        out.clear();
        // A short dip (never under the gasp line) is silent.
        for a in [1.0, 0.9, 0.8, 0.85, 0.9] {
            v.update(false, Some(a), &mut out);
        }
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn creatures_step_by_their_stride_startle_into_a_run_and_call_only_near() {
        // face-a-only: distances along x only; the creatures' events are not face-dependent.
        let mut s = CreatureSounds::new(7);
        let mut out = Vec::new();
        let wolf = |x: f64, running: bool| CreatureSample {
            id: 1,
            model: "wolf",
            feet: [x, 0.0, 0.0],
            running,
        };
        let stride = creature_sounds::MODELS[0].1;
        // 10 strides' walk at 1 cm a frame.
        let frames = (10.0 * stride / 0.01).round() as usize;
        for i in 0..=frames {
            s.update(
                [0.0; 3],
                &[wolf(i as f64 * 0.01, false)],
                1.0 / 60.0,
                &mut out,
            );
        }
        let steps = out
            .iter()
            .filter(|e| e.kind == SoundKind::CreatureStep)
            .count();
        assert!((9..=10).contains(&steps), "{steps}");
        assert!(out.iter().all(|e| e.block == 0 && e.at.is_some()));
        out.clear();
        let x = frames as f64 * 0.01;
        s.update([0.0; 3], &[wolf(x, true)], 0.0, &mut out);
        assert_eq!(kinds(&out), vec![SoundKind::CreatureStartle]);
        // It calls within its interval, but not heard from far away.
        out.clear();
        let far = [1000.0, 0.0, 0.0];
        s.update(far, &[wolf(x, true)], 200.0, &mut out);
        assert!(out.is_empty());
        let mut near = Vec::new();
        s.update([0.0; 3], &[wolf(x, true)], 200.0, &mut near);
        assert_eq!(kinds(&near), vec![SoundKind::CreatureCall]);
        // An unknown model is silent.
        out.clear();
        let fox = CreatureSample {
            id: 2,
            model: "fox",
            feet: [0.0; 3],
            running: true,
        };
        s.update([0.0; 3], &[fox], 500.0, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn moving_through_leaves_off_its_feet_rustles_by_distance_at_most_every_min_interval() {
        let leaves = dwell_blocks::parse_state("dwell:leaves").expect("leaves");
        let mut b = BrushSounds::default();
        let mut out = Vec::new();
        let dt = 1.0 / 60.0;
        let air = Stride::default();
        // Hanging in a bush: silent.
        for _ in 0..120 {
            b.update(leaves, 0.0, dt, air, None, &mut out);
        }
        assert!(out.is_empty());
        // Falling or flying 4 m/s for 5 s through it: one rustle per BRUSH_EVERY_M (20 m).
        for _ in 0..300 {
            b.update(leaves, 4.0, dt, air, None, &mut out);
        }
        let want = (20.0 / BRUSH_EVERY_M) as usize;
        assert!((want - 1..=want).contains(&out.len()), "{}", out.len());
        assert!(
            out.iter()
                .all(|e| e.kind == SoundKind::Brush && e.block == leaves)
        );
        // 20 m/s: capped by BRUSH_MIN_S (1 s → at most 1 / BRUSH_MIN_S), at full intensity.
        out.clear();
        for _ in 0..60 {
            b.update(leaves, 20.0, dt, air, None, &mut out);
        }
        assert!(
            out.len() <= (1.0 / BRUSH_MIN_S) as usize + 1,
            "{}",
            out.len()
        );
        assert!(out.iter().all(|e| e.intensity == 1.0));
        // Out of the leaves: nothing.
        out.clear();
        for _ in 0..60 {
            b.update(0, 4.0, dt, air, None, &mut out);
        }
        assert!(out.is_empty());
    }

    /// Walks `metres` at `speed` (m/s) in `state` over grass with tall grass at the shins, a tick
    /// at a time, through the local player's two trackers as the game runs them (`MoverSounds`,
    /// then `ContactSounds` with its `Stride`); with `patchy` the tall grass grows in every other
    /// cell only (a cell entered every metre). Returns every event.
    fn walk_through_tall_grass(
        state: PlayerState,
        speed: f64,
        metres: f64,
        patchy: bool,
    ) -> Vec<SoundEvent> {
        let grass = dwell_blocks::parse_state("dwell:grass").expect("grass");
        let tall = dwell_blocks::parse_state("dwell:tall_grass").expect("tall grass");
        let dt = 1.0 / 60.0;
        let mut m = MoverSounds::new();
        let mut c = ContactSounds::default();
        let mut out = Vec::new();
        let ticks = (metres / (speed * dt)).round() as usize;
        for i in 0..=ticks {
            let x = speed * dt * i as f64;
            let s = MoverSample {
                feet: [x, 40.0, 0.5],
                velocity: [speed, 0.0, 0.0],
                state,
                grounded: true,
                crouched: state == PlayerState::Crouching,
                ground: grass,
                dt,
                ..standing()
            };
            let from = out.len();
            m.update(&s, None, &mut out);
            let stride = Stride::of(&s, &out[from..]);
            let foliage = if !patchy || (x.floor() as i64) % 2 == 0 {
                tall
            } else {
                0
            };
            c.update(foliage, speed, dt, stride, None, None, |_| None, &mut out);
        }
        out
    }

    /// BUGS.md #103 (owner, 2026-11-12): "Walking through tall grass is really loud and
    /// obnoxious ... it's also playing really fast ... are there 3 SFX playing while walking on
    /// grass?" A rustle sounded every 0.8 m (~5 a second at the walk, two or three over each
    /// footstep, on entering each patch at once). Now: N steps are N footsteps on the grass and at
    /// most one rustle each, sounding with its step, at the gait's stride.
    #[test]
    fn walking_through_tall_grass_is_one_footstep_a_step_and_at_most_one_rustle_on_it() {
        let grass = dwell_blocks::parse_state("dwell:grass").expect("grass");
        for (state, gait, speed) in [
            (PlayerState::Walking, Gait::Walk, 5.0),
            (PlayerState::Running, Gait::Run, 8.0),
            (PlayerState::Crouching, Gait::Crouch, 1.6),
        ] {
            for patchy in [false, true] {
                let metres = 30.0;
                let out = walk_through_tall_grass(state, speed, metres, patchy);
                let steps: Vec<_> = out.iter().filter(|e| e.kind == SoundKind::Step).collect();
                let brushes = out.iter().filter(|e| e.kind == SoundKind::Brush).count();
                let n = steps.len();
                let want = metres / gait.stride();
                assert!(
                    (n as f64 - want).abs() <= 1.0,
                    "{state:?}: {n} steps over {metres} m, want {want}"
                );
                assert!(
                    steps
                        .iter()
                        .all(|e| e.block == grass && e.gait == Some(gait))
                );
                assert!(
                    brushes <= n,
                    "{state:?} patchy {patchy}: {brushes} rustles over {n} steps"
                );
                if !patchy && speed >= 0.4 {
                    assert!(brushes >= n - 1, "{state:?}: {brushes} rustles, {n} steps");
                }
                // Every rustle sounds in the same tick as a footstep (layered on it).
                let mut last_step = None;
                for (i, e) in out.iter().enumerate() {
                    if e.kind == SoundKind::Step {
                        last_step = Some(i);
                    }
                    if e.kind == SoundKind::Brush {
                        assert_eq!(
                            last_step.map(|s| i - s),
                            Some(1),
                            "{state:?}: a rustle not on a step: {out:?}"
                        );
                    }
                }
                let only = |k: SoundKind| matches!(k, SoundKind::Step | SoundKind::Brush);
                assert!(out.iter().all(|e| only(e.kind)), "{:?}", kinds(&out));
            }
        }
        // Natural cadences: ~2 steps a second at the walk, under 3 at the sprint.
        assert!((1.8..=2.2).contains(&(5.0 / Gait::Walk.stride())));
        assert!((2.5..=3.0).contains(&(8.0 / Gait::Run.stride())));
    }

    #[test]
    fn rustles_on_feet_are_never_closer_than_the_min_interval() {
        let tall = dwell_blocks::parse_state("dwell:tall_grass").expect("tall grass");
        let mut b = BrushSounds::default();
        let mut out = Vec::new();
        let dt = 1.0 / 60.0;
        // A step every tick (no gait does this; the limit holds anyway): at most one per
        // BRUSH_MIN_S, and none without a step.
        let step = Stride {
            on_feet: true,
            stepped: true,
        };
        for _ in 0..60 {
            b.update(tall, 5.0, dt, step, None, &mut out);
        }
        assert!(
            out.len() <= (1.0 / BRUSH_MIN_S) as usize + 1,
            "{}",
            out.len()
        );
        out.clear();
        let between = Stride {
            on_feet: true,
            stepped: false,
        };
        for _ in 0..120 {
            b.update(tall, 5.0, dt, between, None, &mut out);
        }
        assert!(out.is_empty(), "a rustle between steps: {out:?}");
    }

    dwell_world::both_faces! {
        fn foliage_is_found_at_the_body_face_locally(f: FaceFrame) {
            let leaves = dwell_blocks::parse_state("dwell:leaves").expect("leaves");
            let stone = dwell_blocks::parse_state("dwell:stone").expect("stone");
            // Leaves a metre up (the waist), stone under the feet.
            let feet = f.point([0.5, 40.0, 0.5]);
            let leaf_cell = dwell_world::cell_of_pos(f.point([0.5, 40.8, 0.5]));
            let ground = dwell_world::cell_of_pos(f.point([0.5, 39.8, 0.5]));
            let voxel = |c: [i32; 3]| {
                if c == leaf_cell {
                    leaves
                } else if c == ground {
                    stone
                } else {
                    0
                }
            };
            assert_eq!(foliage_at(feet, f, 1.8, voxel), leaves);
            // A solid block at the body is not foliage.
            assert_eq!(foliage_at(feet, f, 1.8, |c| if c == leaf_cell { stone } else { 0 }), 0);
        }
    }

    #[test]
    fn a_ground_item_cell_is_stepped_onto_once() {
        let mut g = GroundSteps::default();
        assert_eq!(g.update(Some([1, 2, 3])), Some([1, 2, 3]));
        assert_eq!(g.update(Some([1, 2, 3])), None);
        assert_eq!(g.update(Some([1, 2, 3])), None);
        // A jump off it and back onto it: stepped onto again.
        assert_eq!(g.update(None), None);
        assert_eq!(g.update(Some([1, 2, 3])), Some([1, 2, 3]));
        assert_eq!(g.update(Some([2, 2, 3])), Some([2, 2, 3]));
        // The stick's step and hit are a sound event by name (shared/ground/ground.json).
        let stick = dwell_world::ground::KINDS
            .iter()
            .find(|k| k.name == "dwell:ground_stick")
            .expect("stick");
        for e in [stick.step, stick.hit] {
            let e = e.expect("a stick sounds");
            assert!(SoundKind::ALL.iter().any(|k| k.name() == e), "{e}");
        }
    }
    dwell_world::both_faces! {
        /// WORK_QUEUE (block damage follow-ups): remote players' leaf brushing and stick snaps were
        /// silent. A remote player walking through leaves rustles at its own feet; one walking
        /// onto a stick snaps it there; the local player's id is never sounded as a remote one.
        fn a_remote_player_moving_through_leaves_rustles_at_its_position(f: FaceFrame) {
            let leaves = dwell_blocks::parse_state("dwell:leaves").expect("leaves");
            let dt = 1.0 / 60.0;
            // Leaves a metre deep at the waist everywhere x < 20; a stick in the cell at x = 25.
            let voxel = |c: [i32; 3]| {
                let p = dwell_world::pos_of_cell(c);
                let up = f.y(p[1] + 0.25) - 40.0;
                if p[0] < 20.0 && (0.5..1.0).contains(&up) { leaves } else { 0 }
            };
            let stick = feet_cell(f.point([25.25, 40.0, 0.25]), f);
            let item = |c: [i32; 3]| (c == stick).then(|| (dwell_world::pos_of_cell(c), SoundKind::StickSnap));
            // A footstep every 30 frames (2 m at 4 m/s).
            let sample = |id, x: f64, i: i32| ContactSample {
                player_id: id,
                feet: f.point([x, 40.0, 0.25]),
                face: f,
                speed: 4.0,
                grounded: true,
                height: 1.8,
                stride: Stride {
                    on_feet: true,
                    stepped: i % 30 == 29,
                },
            };
            let mut r = RemoteContacts::default();
            let mut out = Vec::new();
            // Players 2 (remote) and 1 (local) walk side by side through the leaves for 2 s.
            for i in 0..120 {
                let x = 10.0 + 4.0 * dt * f64::from(i);
                r.update(1, &[sample(2, x, i), sample(1, x, i)], dt, voxel, item, &mut out);
            }
            assert_eq!(out.len(), 4, "a rustle on each of its 4 steps: {out:?}");
            for e in &out {
                assert_eq!((e.kind, e.block), (SoundKind::Brush, leaves));
                let at = e.at.expect("at the remote player, not the listener");
                assert!(at[0] >= 10.0 && at[0] < 18.1, "{at:?}");
                assert!((f.y(at[1]) - 40.0).abs() < 1e-9, "at its feet: {at:?}");
            }
            assert_eq!(r.len(), 1, "the local player is not followed as a remote one");
            // Out of the leaves and onto the stick: one snap at the stick, none again standing on it.
            out.clear();
            for i in 0..90 {
                let x = 22.0 + 4.0 * dt * f64::from(i);
                r.update(1, &[sample(2, x.min(25.25), 0)], dt, voxel, item, &mut out);
            }
            assert_eq!(kinds(&out), vec![SoundKind::StickSnap], "{out:?}");
            assert_eq!(out[0].at, Some(dwell_world::pos_of_cell(stick)));
            // Gone from view: forgotten.
            r.update(1, &[], dt, voxel, item, &mut out);
            assert!(r.is_empty());
        }
    }

    #[test]
    fn stick_snaps_are_rate_limited_per_body() {
        let mut c = ContactSounds::default();
        let mut out = Vec::new();
        let snap = |cell: [i32; 3]| Some(([f64::from(cell[0]), 0.0, 0.0], SoundKind::StickSnap));
        // A new stick cell every tick for half a second: at most one per SNAP_MIN_S.
        for i in 0..30 {
            c.update(
                0,
                10.0,
                1.0 / 60.0,
                Stride::default(),
                Some([i, 0, 0]),
                None,
                snap,
                &mut out,
            );
        }
        let n = out.len();
        assert!(n >= 3 && n <= (0.5 / SNAP_MIN_S) as usize + 1, "{n}");
    }
}
