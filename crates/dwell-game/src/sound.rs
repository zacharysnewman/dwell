//! Sound events (docs/AUDIO.md §5): the game's movement and edits turned into compact events the
//! TS shell plays. A side channel: it reads the predicted controller state and the world, and
//! nothing here feeds back into the simulation. The event kinds, gaits and their strides and speed
//! ranges are data (shared/sounds/*.json, generated into `sound.gen.rs`).

use dwell_blocks::{StateId, info};
use dwell_protocol::PlayerState;

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
    /// Continuous sounds (the slide): the voice the event starts, updates or stops — the mover's
    /// (`MoverSounds::voice`).
    pub voice: Option<u16>,
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
    /// +1 face A (up is +y), −1 face B.
    pub face: f64,
    /// 0–1: how much of the body is under water.
    pub submerged: f64,
    /// The block just under the feet.
    pub ground: StateId,
    /// The block at the body (a ladder being climbed, the water swum in).
    pub body: StateId,
    /// Seconds since the mover's previous sample (a tick for the local player, a frame for
    /// others).
    pub dt: f64,
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

/// Turns one mover's motion into footsteps, landings, jumps, splashes, slides and the like.
#[derive(Clone, Debug, Default)]
pub struct MoverSounds {
    /// The voice of this mover's continuous sounds (0: the local player; others: their id).
    voice: u16,
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
    pub fn new() -> Self {
        Self::default()
    }

    /// Another player's sounds: `voice` is its player id.
    pub fn with_voice(voice: u16) -> Self {
        Self {
            voice,
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
        let up = |v: [f64; 3], face: f64| v[1] * face;
        let d = [
            s.feet[0] - p.feet[0],
            s.feet[1] - p.feet[1],
            s.feet[2] - p.feet[2],
        ];
        let horizontal = d[0].hypot(d[2]);
        // A jump of more than a few metres between samples is a teleport (respawn, snap): no sound.
        if horizontal + d[1].abs() > 4.0 {
            self.travelled = 0.0;
            self.finish(at, out);
            return;
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
    cell.map(|v| f64::from(v) + 0.5)
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

/// Events as the JS API hands them over: 7 numbers each (kind, block state, intensity, x, y, z,
/// flags: bit 0 spatial, bits 1–3 the gait + 1, bits 4–20 the voice + 1).
pub const FLOATS_PER_EVENT: usize = 7;

pub fn encode(events: &[SoundEvent]) -> Vec<f64> {
    let mut v = Vec::with_capacity(events.len() * FLOATS_PER_EVENT);
    for e in events {
        let at = e.at.unwrap_or([0.0; 3]);
        let flags = u32::from(e.at.is_some())
            | (e.gait.map_or(0, |g| g as u32 + 1) << 1)
            | (e.voice.map_or(0, |v| u32::from(v) + 1) << 4);
        v.extend_from_slice(&[
            f64::from(e.kind as u8),
            f64::from(e.block),
            f64::from(e.intensity),
            at[0],
            at[1],
            at[2],
            f64::from(flags),
        ]);
    }
    v
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
            face: 1.0,
            submerged: 0.0,
            ground: stone(),
            body: 0,
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

    #[test]
    fn landing_on_face_b_reads_impact_toward_its_ground() {
        let (mut m, mut s) = (MoverSounds::new(), standing());
        s.face = -1.0;
        m.update(&s, None, &mut Vec::new());
        s.grounded = false;
        s.velocity = [0.0, 10.0, 0.0]; // falling toward the midplane: +y on face B
        m.update(&s, None, &mut Vec::new());
        s.grounded = true;
        let mut out = Vec::new();
        m.update(&s, None, &mut out);
        assert_eq!(kinds(&out), [SoundKind::Land]);
    }

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
        assert_eq!(out[0].at, Some([1.5, 2.5, 3.5]));
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
        assert_eq!(out[0].at, Some([10.5, 0.5, 0.5]));
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
    }
}
