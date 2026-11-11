//! The far ecology on the server (docs/ECOLOGY.md §6, WORK_QUEUE item 11 slice 2): one
//! [`Ecosystem`] per face, on that face's island ([`Island::from_world_map`] of its world map),
//! stepped by the world's clock in whole world days, saved with the world as an optional table
//! (`ecology`, which older builds ignore; a world without it starts a fresh ecology from its seed
//! and catches up its days).
//!
//! **The schedule** is a pure function of how far behind the clock the ecology is and whether
//! anyone is online ([`next_step`]), so a save → load → continue runs the very steps an
//! uninterrupted run does:
//! - online and less than [`CATCH_UP_DAYS`] behind: one day at a time;
//! - nobody online: nothing until [`IDLE_DAYS`] behind, then big steps (the data's `stepDays`);
//! - [`CATCH_UP_DAYS`] or more behind (after a load of an older world, or an idle stretch): big
//!   steps, at most [`MAX_STEPS_PER_TICK`] a tick, so a catch-up of years is spread over ticks and
//!   never stalls one.
//!
//! **Per world**: `enabled` (a world can opt out: no ecology is made or stepped) and `speed` (×
//! the world's days: a per-world tuning hook) are saved in the record; a new world takes them from
//! [`crate::server::ServerConfig`]'s `ecology` and `ecology_speed`.
//!
//! Worlds without a world map (the flat world, test generators) have no island and no ecology,
//! unless a test hands the server islands (`ServerConfig::ecology_islands`). The browser's local
//! server waits for the map to be computed in its own instance (it is computed in the server-jobs
//! worker today, so a local world's ecology does not run yet: docs/ECOLOGY.md §7).

use super::Server;
use dwell_ecology::{DATA, Ecosystem, Island};
use dwell_protocol::SIM_HZ;
use dwell_world::Face;
use std::sync::Arc;

/// The most ecology steps in one tick (a catch-up is spread over ticks).
pub const MAX_STEPS_PER_TICK: u32 = 4;
/// Behind by this many days or more, the ecology catches up in big steps.
pub const CATCH_UP_DAYS: f64 = 10.0;
/// With nobody online the ecology waits until it is this far behind, then catches up.
pub const IDLE_DAYS: f64 = 10.0;

const RECORD_VERSION: u8 = 1;
const FACES: [Face; 2] = [Face::A, Face::B];

/// The next step (days) for an ecology `behind` days behind the clock, or None to wait. Whole
/// days only, so the ecology's day stays a whole number and every run steps the same.
pub fn next_step(behind: f64, online: bool, step_days: f64) -> Option<f64> {
    if behind < 1.0 || (!online && behind < IDLE_DAYS) {
        return None;
    }
    if online && behind < CATCH_UP_DAYS {
        return Some(1.0);
    }
    Some(step_days.max(1.0).floor().min(behind.floor()))
}

/// The server's ecology: both faces' ecosystems (once their islands are known), the world's
/// switch and speed, and the last tick's figures.
pub struct Ecology {
    pub enabled: bool,
    pub speed: f64,
    /// Where the islands come from: a test's, or the world maps of this seed (None: no map).
    given: Option<Arc<[Island; 2]>>,
    map_seed: Option<u64>,
    islands: Option<Arc<[Island; 2]>>,
    faces: Option<[Ecosystem; 2]>,
    /// The saved faces' bytes, until the islands are known (kept as they were if never).
    saved: Option<[Vec<u8>; 2]>,
    /// A saved record this build can't read: kept, not written over, not stepped.
    unread: Option<Vec<u8>>,
    /// The last tick's steps and their time (ms).
    pub last_steps: u32,
    pub last_ms: f64,
    /// Every step since the server started.
    pub steps: u64,
    world_seed: u64,
}

/// One herd or pack in the dev read-out.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupView {
    pub face: Face,
    pub id: u32,
    pub species: &'static str,
    /// Face-local metres.
    pub x: f64,
    pub z: f64,
    pub size: f64,
    pub hunger: f64,
    pub energy: f64,
    pub activity: &'static str,
}

/// The dev read-out of the ecology ([`Server::ecology_snapshot`]).
#[derive(Clone, Debug, PartialEq)]
pub struct EcologySnapshot {
    pub enabled: bool,
    /// Whether the ecosystems exist (the islands are known).
    pub running: bool,
    /// The ecology's day and the clock's (× the world's speed).
    pub day: f64,
    pub clock_day: f64,
    pub groups: Vec<GroupView>,
}

impl Ecology {
    pub(super) fn new(
        enabled: bool,
        speed: f64,
        world_seed: u64,
        map: bool,
        given: Option<Arc<[Island; 2]>>,
    ) -> Self {
        Self {
            enabled,
            speed,
            map_seed: (map && given.is_none()).then_some(world_seed),
            given,
            islands: None,
            faces: None,
            saved: None,
            unread: None,
            last_steps: 0,
            last_ms: 0.0,
            steps: 0,
            world_seed,
        }
    }

    /// Restores a saved record (`init_storage`).
    pub(super) fn restore(&mut self, bytes: Vec<u8>) {
        match decode_record(&bytes) {
            Some((enabled, speed, faces)) => {
                self.enabled = enabled;
                self.speed = speed;
                self.saved = Some(faces);
            }
            None => self.unread = Some(bytes),
        }
    }

    /// The record to save.
    pub(super) fn record(&self) -> Vec<u8> {
        if let Some(b) = &self.unread {
            return b.clone();
        }
        let faces: [Vec<u8>; 2] = match (&self.faces, &self.saved) {
            (Some(f), _) => [f[0].to_bytes(&DATA), f[1].to_bytes(&DATA)],
            (None, Some(s)) => s.clone(),
            (None, None) => [Vec::new(), Vec::new()],
        };
        encode_record(self.enabled, self.speed, &faces)
    }

    /// The islands, once known: a test's, or both faces' world maps once this process has them
    /// (never computed on the tick).
    fn islands(&mut self) -> Option<Arc<[Island; 2]>> {
        if self.islands.is_none() {
            self.islands = match (&self.given, self.map_seed) {
                (Some(g), _) => Some(g.clone()),
                (None, Some(seed)) => {
                    let seeds = FACES.map(|f| dwell_worldgen::bifacial::face_seed(seed, f));
                    seeds
                        .iter()
                        .all(|&s| dwell_worldgen::worldmap::cache::is_ready(s))
                        .then(|| {
                            Arc::new(seeds.map(|s| {
                                Island::from_world_map(
                                    &dwell_worldgen::worldmap::for_seed(s),
                                    &DATA,
                                )
                            }))
                        })
                }
                (None, None) => None,
            };
        }
        self.islands.clone()
    }

    /// The ecosystems, made once the islands are known: the saved ones, else fresh from the seed.
    fn faces(&mut self) -> Option<&mut [Ecosystem; 2]> {
        if self.faces.is_none() {
            let islands = self.islands()?;
            let saved = self.saved.take().unwrap_or_default();
            let mut made = Vec::with_capacity(2);
            for (k, face) in FACES.into_iter().enumerate() {
                let fresh = || {
                    Ecosystem::new(
                        &islands[k],
                        &DATA,
                        dwell_worldgen::bifacial::face_seed(self.world_seed, face),
                    )
                };
                if saved[k].is_empty() {
                    made.push(fresh());
                    continue;
                }
                match Ecosystem::from_bytes(&saved[k], &islands[k], &DATA) {
                    Some(e) => made.push(e),
                    None => {
                        // Unreadable: keep the whole record as it was, step nothing.
                        self.unread = Some(encode_record(self.enabled, self.speed, &saved));
                        return None;
                    }
                }
            }
            let [a, b]: [Ecosystem; 2] = made.try_into().ok()?;
            self.faces = Some([a, b]);
        }
        self.faces.as_mut()
    }
}

fn encode_record(enabled: bool, speed: f64, faces: &[Vec<u8>; 2]) -> Vec<u8> {
    let mut b = vec![RECORD_VERSION, u8::from(enabled)];
    b.extend(speed.to_bits().to_le_bytes());
    for f in faces {
        b.extend((f.len() as u32).to_le_bytes());
        b.extend(f);
    }
    b
}

fn decode_record(b: &[u8]) -> Option<(bool, f64, [Vec<u8>; 2])> {
    if b.len() < 10 || b[0] != RECORD_VERSION {
        return None;
    }
    let enabled = b[1] != 0;
    let speed = f64::from_bits(u64::from_le_bytes(b[2..10].try_into().ok()?));
    let mut at = 10;
    let mut faces: [Vec<u8>; 2] = Default::default();
    for f in &mut faces {
        let len = u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?) as usize;
        at += 4;
        *f = b.get(at..at + len)?.to_vec();
        at += len;
    }
    (at == b.len() && speed.is_finite() && speed >= 0.0).then_some((enabled, speed, faces))
}

impl Server {
    /// The ecology's clock: the world's days so far (its clock's day length) × the world's
    /// ecology speed.
    pub fn ecology_clock_days(&self) -> f64 {
        f64::from(self.tick) / f64::from(SIM_HZ) / self.settings.clock.day_length_s()
            * self.ecology.speed
    }

    /// Steps the ecology toward the clock ([`next_step`]), at most [`MAX_STEPS_PER_TICK`] steps.
    pub(super) fn step_ecology(&mut self) {
        self.ecology.last_steps = 0;
        self.ecology.last_ms = 0.0;
        if !self.ecology.enabled || self.ecology.unread.is_some() {
            return;
        }
        let clock = self.ecology_clock_days();
        let online = self.sessions.values().any(|s| s.joined());
        let watch = crate::clock::Stopwatch::start();
        let Some(islands) = self.ecology.islands() else {
            return;
        };
        let Some(faces) = self.ecology.faces() else {
            return;
        };
        let mut steps = 0;
        while steps < MAX_STEPS_PER_TICK {
            let Some(dt) = next_step(clock - faces[0].day, online, DATA.island.step_days) else {
                break;
            };
            for (k, eco) in faces.iter_mut().enumerate() {
                eco.step(&islands[k], &DATA, dt);
            }
            steps += 1;
        }
        self.ecology.last_steps = steps;
        self.ecology.steps += u64::from(steps);
        self.ecology.last_ms = watch.elapsed_ms();
    }

    /// The dev read-out: every herd and pack on both faces (tests; a debug channel later).
    pub fn ecology_snapshot(&self) -> EcologySnapshot {
        let e = &self.ecology;
        let mut groups = Vec::new();
        if let Some(faces) = &e.faces {
            for (face, eco) in FACES.into_iter().zip(faces) {
                groups.extend(eco.groups.iter().map(|g| GroupView {
                    face,
                    id: g.id,
                    species: DATA.species[g.species].name,
                    x: g.x,
                    z: g.z,
                    size: g.size,
                    hunger: g.hunger,
                    energy: g.energy,
                    activity: g.activity.name(),
                }));
            }
        }
        EcologySnapshot {
            enabled: e.enabled,
            running: e.faces.is_some(),
            day: e.faces.as_ref().map_or(0.0, |f| f[0].day),
            clock_day: self.ecology_clock_days(),
            groups,
        }
    }

    /// Tests, tools: face `face`'s ecosystem, once it exists.
    pub fn ecosystem(&self, face: Face) -> Option<&Ecosystem> {
        let k = FACES.iter().position(|&f| f == face)?;
        self.ecology.faces.as_ref().map(|f| &f[k])
    }

    /// The last tick's ecology steps and their time (ms), and the steps since the start.
    pub fn ecology_stats(&self) -> (u32, f64, u64) {
        (
            self.ecology.last_steps,
            self.ecology.last_ms,
            self.ecology.steps,
        )
    }
}
