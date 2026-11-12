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
//! server runs it too: its worker installs both faces' maps in its own instance before the world
//! starts (`installWorldMap`), and the islands are made from those (docs/ECOLOGY.md §6).
//!
//! **Saving**: each face's forage is written as runs, a cell at its most not written
//! (`dwell_ecology::codec`), so a fresh or regrown world's record is a few hundred bytes; a save
//! still writes it only when it changed since the store last held it ([`Ecology::record_to_save`]):
//! a local world saves every few seconds, the ecology steps once a world day.

use super::{Server, SessionId};
use dwell_ecology::{DATA, Ecosystem, Island};
use dwell_protocol::{
    Channel, EcologyGroupNet, EcologyMessage, MAX_ECOLOGY_GROUPS, Message, SIM_HZ,
};
use dwell_world::Face;
use std::sync::Arc;

/// The most ecology steps in one tick (a catch-up is spread over ticks). One since the real
/// densities (WORK_QUEUE item 46): a step of both faces' ~600 herds is a few ms natively.
pub const MAX_STEPS_PER_TICK: u32 = 1;
/// Behind by this many days or more, the ecology catches up in big steps.
pub const CATCH_UP_DAYS: f64 = 10.0;
/// With nobody online the ecology waits until it is this far behind, then catches up.
pub const IDLE_DAYS: f64 = 10.0;

/// Ticks between sends of the far groups to the clients that said `EcologyHello` (every 2 s: the
/// far groups move over days, so a low rate is plenty).
pub const ECOLOGY_SEND_EVERY: u32 = 2 * SIM_HZ;

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

/// The grass's climate share at (face index, face-local x, z) (docs/ECOLOGY.md §10).
pub type GrassShare = Arc<dyn Fn(usize, f64, f64) -> f64 + Send + Sync>;

/// The server's ecology: both faces' ecosystems (once their islands are known), the world's
/// switch and speed, and the last tick's figures.
pub struct Ecology {
    pub enabled: bool,
    /// Dev (`animalsIgnorePlayers`, docs/DEV_TOOLS.md): the near animals see no player.
    pub ignore_players: bool,
    /// Dev (`animalPose`): every near animal is sent in this pose (its index in
    /// `CREATURE_POSES`), whatever its idle, to judge a pose by eye.
    pub force_pose: Option<u8>,
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
    /// The herds and packs near players, as individuals (docs/ECOLOGY.md §8).
    pub near: crate::herds::Near,
    /// The grass's climate share of a cell's most forage at (face index, face-local x, z), applied
    /// to the world maps' islands as they are made (docs/ECOLOGY.md §10; none: as the ground's).
    pub grass_share: Option<GrassShare>,
    /// A hash of the record the store holds (last restored or handed to a save), so a save whose
    /// record is unchanged writes nothing: a local world saves every few seconds while the ecology
    /// steps once a world day.
    written: Option<u64>,
    /// Records handed to saves since the server started.
    pub writes: u64,
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
            near: crate::herds::Near::default(),
            grass_share: None,
            written: None,
            writes: 0,
            ignore_players: false,
            force_pose: None,
        }
    }

    /// Restores a saved record (`init_storage`).
    pub(super) fn restore(&mut self, bytes: Vec<u8>) {
        self.written = Some(record_hash(&bytes));
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
        let faces: [Vec<u8>; 2] = match (&self.faces, &self.islands, &self.saved) {
            (Some(f), Some(islands), _) => {
                // The groups near players are saved as they would fold back now.
                let mut f = f.clone();
                for g in &self.near.groups {
                    if let Some(r) = g.folded() {
                        let eco = &mut f[g.face];
                        let at = eco.groups.partition_point(|o| o.id < r.id);
                        eco.groups.insert(at, r);
                    }
                }
                [
                    f[0].to_bytes(&islands[0], &DATA),
                    f[1].to_bytes(&islands[1], &DATA),
                ]
            }
            (_, _, Some(s)) => s.clone(),
            _ => [Vec::new(), Vec::new()],
        };
        encode_record(self.enabled, self.speed, &faces)
    }

    /// The record to save, or None when the store already holds it (nothing stepped, nothing near
    /// changed since the last save).
    pub(super) fn record_to_save(&mut self) -> Option<Vec<u8>> {
        let r = self.record();
        let h = record_hash(&r);
        if self.written == Some(h) {
            return None;
        }
        self.written = Some(h);
        self.writes += 1;
        Some(r)
    }

    /// A save failed: the next one writes the record whatever it holds.
    pub(super) fn save_failed(&mut self) {
        self.written = None;
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
                            let mut k = 0;
                            Arc::new(seeds.map(|s| {
                                let mut island = Island::from_world_map(
                                    &dwell_worldgen::worldmap::for_seed(s),
                                    &DATA,
                                );
                                if let Some(share) = &self.grass_share {
                                    island.scale_grazing(|x, z| share(k, x, z));
                                }
                                k += 1;
                                island
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

/// Whether it is night on `face` at `world_s` under `clock`: face A's sun is up for `day_s` of each
/// day, centred on noon (docs/DAY_NIGHT.md); face B's day is face A's night.
pub fn night_on(clock: crate::server::WorldClock, world_s: f64, face: Face) -> bool {
    let len = clock.day_length_s();
    let t = (world_s / len + f64::from(clock.start_hour) / 24.0).rem_euclid(1.0);
    let day_a = (t - 0.5).abs() < f64::from(clock.day_s) / len / 2.0;
    // face-boundary: the two faces' days are opposite (one sun between them).
    day_a == (face == Face::B)
}

fn record_hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
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

    /// The near ecology (docs/ECOLOGY.md §8): records near players become individuals, those far
    /// from all fold back, and the individuals decide; a grazer's eaten block is written here (the
    /// only world change, near players only).
    /// Dev tools (the local world's `spawnHerd`, docs/DEV_TOOLS.md): a far record of `species`
    /// (`"dwell:deer"`, `"dwell:wolf"`: the ecology's names) of `size` head, fed and rested, put
    /// `offset_m` along +x (face-local) from `feet` (world) on its face, where the near ecology
    /// brings it in as individuals on its next tick, as a herd or pack walking up would be. Its
    /// record's id, or an error (the ecology off, no such species).
    pub fn spawn_group_dev(
        &mut self,
        species: &str,
        feet: [f64; 3],
        size: f64,
        offset_m: f64,
    ) -> Result<u32, String> {
        let s = DATA
            .find(species)
            .ok_or_else(|| format!("no species {species:?} in shared/ecology"))?;
        let f = dwell_world::FaceFrame::at_height(feet[1]);
        let local = f.point(feet);
        let k = usize::from(f.is_b());
        let faces = self
            .ecology
            .faces
            .as_mut()
            .ok_or_else(|| "the ecology is not running".to_owned())?;
        Ok(faces[k].push(s, local[0] + offset_m, local[2], size.max(1.0), 0.0, 1.0))
    }

    /// Dev tools (`animalPose`): every near animal sent in pose `pose` (`CREATURE_POSES`), or as
    /// it idles (None).
    pub fn force_animal_pose(&mut self, pose: Option<u8>) {
        self.ecology.force_pose = pose;
    }

    /// Dev tools (`animalsIgnorePlayers`): the near animals see no player while `on`.
    pub fn set_animals_ignore_players(&mut self, on: bool) {
        self.ecology.ignore_players = on;
    }

    pub(super) fn step_herds(&mut self) {
        let running =
            self.ecology.enabled && self.ecology.unread.is_none() && self.ecology.faces.is_some();
        if !running {
            self.creatures.reserved.clear();
            return;
        }
        if self.creatures.reserved.is_empty() {
            self.creatures.reserved = DATA
                .species
                .iter()
                .filter_map(|s| s.near)
                .filter_map(|n| self.creatures.kind_of(n.creature))
                .collect();
        }
        let players = self.living_points();
        let near = self.presence_points(&players);
        let stances = self.living_stances();
        let world_s = f64::from(self.tick) / f64::from(SIM_HZ);
        let night = FACES.map(|f| night_on(self.settings.clock, world_s, f));
        let Some(islands) = self.ecology.islands.clone() else {
            return;
        };
        let eaten = {
            let world = &self.world;
            let get = |c: [i32; 3]| {
                world.find(dwell_world::chunk_of(c[0], c[1], c[2]))?;
                Some(world.get_voxel_if_loaded(c))
            };
            let e = &mut self.ecology;
            let Some(faces) = e.faces.as_mut() else {
                return;
            };
            let mut w = crate::herds::World {
                data: &DATA,
                islands: &islands[..],
                ecos: &mut faces[..],
                players: &players,
                near: &near,
                stances: &stances,
                get: &get,
                night,
                seed: self.config.world_seed,
                ignore_players: e.ignore_players,
            };
            e.near.tick(&mut w, &mut self.creatures)
        };
        if eaten.is_empty() {
            return;
        }
        let mut changes = super::TickChanges::default();
        for c in eaten {
            let to = self.eaten_to(c);
            if self.world.get_voxel_if_loaded(c) != to {
                changes.set(&mut self.world, c, to);
            }
        }
        self.broadcast_changes(changes, dwell_protocol::VoxelModificationReason::Edit);
    }

    /// What a grazer's eaten tuft at world cell `c` becomes (docs/ECOLOGY.md §10): in a world that
    /// grows grass, a sprout over the rule's ground (the roots stay and it regrows stage by stage);
    /// else air, as before. Public for tests and tools.
    pub fn eaten_to(&self, c: [i32; 3]) -> dwell_blocks::StateId {
        if !self.grass.enabled {
            return dwell_blocks::AIR;
        }
        let f = dwell_world::FaceFrame::at_row(c[1]);
        let [x, y, z] = f.cell(c);
        let below = f.state(self.world.get_voxel_if_loaded(f.cell([x, y - 1, z])));
        f.state(self.grass_rule.eaten(below))
    }

    /// A client reads the far ecology (`EcologyHello`): the far groups now, then every
    /// [`ECOLOGY_SEND_EVERY`] ticks while the ecology runs.
    pub(super) fn ecology_hello(&mut self, id: SessionId) {
        let Some(s) = self.sessions.get_mut(&id) else {
            return;
        };
        s.ecology_hello = true;
        if let Some(m) = self.ecology_message_from(self.session_feet(id)) {
            self.send_reliable(id, &Message::Ecology(m), Channel::World);
        }
    }

    /// The far groups for the clients (docs/ECOLOGY.md §9), None while the ecology doesn't run.
    pub fn ecology_message(&self) -> Option<EcologyMessage> {
        self.ecology_message_from(None)
    }

    /// [`Self::ecology_message`] for a player at `eye` (world feet): its face's groups nearest
    /// first, then the other face's, so the [`MAX_ECOLOGY_GROUPS`] sent are the ones it could see
    /// or find (WORK_QUEUE item 46: ~600 herds a face at real densities); None: in id order.
    pub fn ecology_message_from(&self, eye: Option<[f64; 3]>) -> Option<EcologyMessage> {
        let snap = self.ecology_snapshot();
        if !snap.enabled || !snap.running {
            return None;
        }
        let mut order: Vec<&GroupView> = snap.groups.iter().collect();
        if let Some(eye) = eye {
            let f = dwell_world::FaceFrame::at_height(eye[1]);
            let local = f.point(eye);
            let key = |g: &GroupView| {
                (
                    dwell_world::FaceFrame::of(g.face) != f,
                    (g.x - local[0]).hypot(g.z - local[2]),
                )
            };
            order.sort_by(|a, b| {
                let (ka, kb) = (key(a), key(b));
                ka.0.cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
            });
        }
        let groups = order
            .into_iter()
            .take(MAX_ECOLOGY_GROUPS)
            .map(|g| EcologyGroupNet {
                id: g.id,
                face: dwell_physics::net::face_bit(dwell_world::FaceFrame::of(g.face), 1),
                species: g.species.to_string(),
                x: g.x as f32,
                z: g.z as f32,
                size: g.size as f32,
            })
            .collect();
        Some(EcologyMessage {
            day: snap.day as f32,
            groups,
        })
    }

    /// The world feet of session `id`'s living player, if it has one.
    fn session_feet(&self, id: SessionId) -> Option<[f64; 3]> {
        let h = self.sessions.get(&id)?.handle?;
        Some(self.players.position(&self.physics, h).to_array())
    }

    /// Every [`ECOLOGY_SEND_EVERY`] ticks, the far groups to the joined clients that asked, each
    /// its own nearest first.
    pub(super) fn send_ecology(&mut self) {
        if !self.tick.is_multiple_of(ECOLOGY_SEND_EVERY) {
            return;
        }
        let to: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.joined() && s.ecology_hello)
            .map(|(&id, _)| id)
            .collect();
        for id in to {
            let Some(m) = self.ecology_message_from(self.session_feet(id)) else {
                return;
            };
            self.send_reliable(id, &Message::Ecology(m), Channel::World);
        }
    }

    /// Tests, tools: the herds and packs near players.
    pub fn herds(&self) -> &crate::herds::Near {
        &self.ecology.near
    }

    /// Tests, tools: face `face`'s ecosystem, to move or change its records.
    pub fn ecosystem_mut(&mut self, face: Face) -> Option<&mut Ecosystem> {
        let k = FACES.iter().position(|&f| f == face)?;
        self.ecology.faces.as_mut().map(|f| &mut f[k])
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

    /// The heap the ecology's islands and ecosystems hold on both faces (bytes; the world maps they
    /// were made from are not counted: those are the installed ones, shared).
    pub fn ecology_heap_bytes(&self) -> usize {
        let e = &self.ecology;
        e.islands
            .as_ref()
            .map_or(0, |i| i.iter().map(Island::heap_bytes).sum())
            + e.faces
                .as_ref()
                .map_or(0, |f| f.iter().map(Ecosystem::heap_bytes).sum())
    }

    /// Tests, tools: the ecology record a save writes now (the `ecology` table's bytes).
    pub fn ecology_record(&self) -> Vec<u8> {
        self.ecology.record()
    }

    /// Tests, tools: face `face`'s ecosystem, once it exists.
    pub fn ecosystem(&self, face: Face) -> Option<&Ecosystem> {
        let k = FACES.iter().position(|&f| f == face)?;
        self.ecology.faces.as_ref().map(|f| &f[k])
    }

    /// Ecology records handed to saves since the server started (an unchanged one is not written).
    pub fn ecology_writes(&self) -> u64 {
        self.ecology.writes
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
