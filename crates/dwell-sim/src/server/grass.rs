//! The server's side of tall grass growing and spreading (ADR 0075 T3c, docs/ECOLOGY.md §10): every
//! tick a couple of the loaded chunks that hasn't grown up to today's whole world day grows
//! the days it missed (at most the data's `catchUpDays`: on its first load since day 0, after an
//! idle stretch), by `dwell_worldgen::grass::Rule::grow_chunk` over the face terrain's envelope. The
//! writes go out as one modification. The world's switch and each grown chunk's last day are saved
//! in an optional `grass_growth` table that older builds ignore.

use super::{Server, TickChanges};
use dwell_protocol::VoxelModificationReason;
use dwell_world::{ChunkCoord, Face, FaceFrame, VoxelWorld, chunk_of};
use dwell_worldgen::bifacial::BifacialTerrain;
use dwell_worldgen::grass::{Cells, Envelope, TerrainEnvelope};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

/// The most chunks grown a tick (the rest on the next ticks): a few every tick, not 64 every
/// 5 s, whose tick and re-meshing stalled the game (BUGS.md #194). 60 chunks a second.
const CHUNKS_PER_TICK: usize = 1;
/// The chunks behind today looked at a tick for one ready to grow.
const CANDIDATES_PER_TICK: usize = 8;
/// The saved form's version.
const VERSION: u8 = 1;

/// A test's or tool's envelope: (face, face-local x, z) → (roll, climate share).
pub type GrassEnvelope = Arc<dyn Fn(Face, i32, i32) -> (f32, f64) + Send + Sync>;

/// The face terrains grass grows over, built once the world maps are in the process.
pub type GrassTerrain = Arc<OnceLock<Option<Arc<BifacialTerrain>>>>;

/// The world's grass growth: its switch and each grown chunk's last whole world day.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Grass {
    pub enabled: bool,
    pub days: HashMap<ChunkCoord, u32>,
}

impl Grass {
    /// The saved form: a version byte, the switch, the count, then each chunk (x, y, z, day; LE).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = vec![VERSION, u8::from(self.enabled)];
        out.extend((self.days.len() as u32).to_le_bytes());
        let mut days: Vec<_> = self.days.iter().collect();
        days.sort_unstable_by_key(|(c, _)| (c.x, c.y, c.z));
        for (c, d) in days {
            for v in [c.x, c.y, c.z] {
                out.extend(v.to_le_bytes());
            }
            out.extend(d.to_le_bytes());
        }
        out
    }

    /// None: another version's (kept unread) or malformed.
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        let (&version, rest) = b.split_first()?;
        if version != VERSION {
            return None;
        }
        let (&enabled, rest) = rest.split_first()?;
        let n = u32::from_le_bytes(rest.get(..4)?.try_into().ok()?) as usize;
        let rest = &rest[4..];
        if rest.len() != n * 16 {
            return None;
        }
        let word = |c: &[u8], i: usize| -> [u8; 4] { c[i * 4..i * 4 + 4].try_into().unwrap() };
        let days = rest
            .chunks_exact(16)
            .map(|c| {
                let i = |k| i32::from_le_bytes(word(c, k));
                (
                    ChunkCoord {
                        x: i(0),
                        y: i(1),
                        z: i(2),
                    },
                    u32::from_le_bytes(word(c, 3)),
                )
            })
            .collect();
        Some(Self {
            enabled: enabled != 0,
            days,
        })
    }
}

/// The world's cells for the step: loaded chunks only, generated cells regenerated for edited chunks.
struct WorldCells<'a> {
    world: &'a mut VoxelWorld,
    /// Edited chunks as generated (the jobs' copies), read before regenerating one here.
    pristine: &'a dwell_world::hash::HashMap<ChunkCoord, Box<dwell_world::Chunk>>,
    cache: dwell_world::hash::HashMap<ChunkCoord, Box<dwell_world::Chunk>>,
    /// The chunks read, copied once each (a chunk's 32³ cells read one by one through the world
    /// cost ~5 ms: priority 0); `None` for one not loaded.
    copies: std::cell::RefCell<dwell_world::hash::HashMap<ChunkCoord, Option<Box<[u16]>>>>,
}

impl Cells for WorldCells<'_> {
    fn now(&self, c: [i32; 3]) -> Option<u16> {
        let k = chunk_of(c[0], c[1], c[2]);
        let mut copies = self.copies.borrow_mut();
        let v = copies.entry(k).or_insert_with(|| {
            self.world
                .find(k)
                .map(|ch| ch.voxels().to_vec().into_boxed_slice())
        });
        let v = v.as_ref()?;
        let s = dwell_world::CHUNK_SIZE;
        Some(v[dwell_world::local_index(c[0] - k.x * s, c[1] - k.y * s, c[2] - k.z * s)])
    }
    fn generated(&mut self, c: [i32; 3]) -> Option<u16> {
        let k = chunk_of(c[0], c[1], c[2]);
        let chunk = self.world.find(k)?;
        if chunk.revision() == 0 {
            return Some(self.world.get_voxel_if_loaded(c));
        }
        if let Some(p) = self.pristine.get(&k) {
            return Some(p.get(
                c[0] - k.x * dwell_world::CHUNK_SIZE,
                c[1] - k.y * dwell_world::CHUNK_SIZE,
                c[2] - k.z * dwell_world::CHUNK_SIZE,
            ));
        }
        Some(self.world.generated_voxel(c, &mut self.cache))
    }
}

/// The grass's climate share for the far ecology at face `k`'s face-local (x, z) (docs/ECOLOGY.md
/// §10), from the same terrain envelope the near step uses (1 without one). Asked only once the
/// world maps are in the process (as the ecology's islands are made), so it may build the terrain.
pub fn far_share(terrain: &GrassTerrain, of: Option<(u32, u64)>, k: usize, x: f64, z: f64) -> f64 {
    let t = terrain.get_or_init(|| {
        of.and_then(|(version, seed)| BifacialTerrain::for_generator(version, seed))
            .map(Arc::new)
    });
    let Some(t) = t else {
        return 1.0;
    };
    let face = if k == 0 { Face::A } else { Face::B };
    let env = TerrainEnvelope {
        terrain: t.local(face),
        decoration: dwell_worldgen::grass::GRASS.decoration,
    };
    dwell_worldgen::grass::far_share(&env, x, z)
}

impl Server {
    /// The grass growth (tests, tools).
    pub fn grass(&self) -> &Grass {
        &self.grass
    }

    /// The chunk the tick's grass step is part way through and its next quarter (tests, tools).
    pub fn grass_in_hand(&self) -> Option<(ChunkCoord, u8)> {
        self.grass_part
    }

    /// How many chunks the grass step has regenerated to read them as generated (tests, tools:
    /// none on the tick, BUGS.md #194).
    pub fn grass_regenerated(&self) -> usize {
        self.grass_regenerated
    }

    /// Tests, tools: the envelope in place of the terrain's, and the switch.
    pub fn set_grass_envelope(&mut self, env: Option<GrassEnvelope>) {
        self.grass_envelope = env;
    }

    /// Tests, tools: turns the world's grass growth on or off.
    pub fn set_grass_enabled(&mut self, on: bool) {
        self.grass.enabled = on;
        self.grass_forced = on;
        self.share_grass_with_ecology();
    }

    /// The far ecology's forage follows the grass's climate cap while the world grows grass
    /// (docs/ECOLOGY.md §10); a world without grass growth keeps its forage as before.
    pub(super) fn share_grass_with_ecology(&mut self) {
        let of = (self.config.generator_override.is_none())
            .then_some((self.config.generator_version, self.config.world_seed));
        self.ecology.grass_share = self.grass.enabled.then(|| {
            let t = self.grass_terrain.clone();
            Arc::new(move |k: usize, x: f64, z: f64| far_share(&t, of, k, x, z))
                as super::ecology::GrassShare
        });
    }

    /// The face terrains, once both faces' world maps are in the process (never computed on the
    /// tick); none for a world that isn't the terrain generator's.
    pub(super) fn grass_terrain(&mut self) -> Option<Arc<BifacialTerrain>> {
        if let Some(t) = self.grass_terrain.get() {
            return t.clone();
        }
        if self.config.generator_override.is_some()
            || self.config.generator_version != dwell_world::GENERATOR_TERRAIN
        {
            let _ = self.grass_terrain.set(None);
            return None;
        }
        let seed = self.config.world_seed;
        let ready = [Face::A, Face::B].into_iter().all(|f| {
            dwell_worldgen::worldmap::cache::is_ready(dwell_worldgen::bifacial::face_seed(seed, f))
        });
        if !ready {
            return None;
        }
        let t = BifacialTerrain::for_generator(self.config.generator_version, seed).map(Arc::new);
        let _ = self.grass_terrain.set(t.clone());
        t
    }

    /// The grass step, every tick: an edited chunk waits for its pristine copy.
    pub(super) fn grow_grass(&mut self) {
        self.grass_step(true);
    }

    /// The grass step now (tests, tools): up to `CHUNKS_PER_TICK` loaded chunks behind today grow
    /// their days, an edited one regenerated here if its pristine copy isn't at hand.
    pub fn grow_grass_now(&mut self) {
        self.grass_step(false);
    }

    /// Chunk `c`'s and the ones under and over it that are edited without a pristine copy (the
    /// step would regenerate them to read them as generated).
    fn grass_missing(&self, c: ChunkCoord) -> Vec<ChunkCoord> {
        Self::grass_chunks(c)
            .into_iter()
            .filter(|k| {
                self.world.find(*k).is_some_and(|ch| ch.revision() > 0)
                    && !self.pristine.contains_key(k)
            })
            .collect()
    }

    fn grass_chunks(c: ChunkCoord) -> [ChunkCoord; 3] {
        let f = FaceFrame::at_chunk_y(c.y);
        let l = f.chunk(c);
        [
            c,
            f.chunk(ChunkCoord::new(l.x, l.y - 1, l.z)),
            f.chunk(ChunkCoord::new(l.x, l.y + 1, l.z)),
        ]
    }

    /// Before a chunk's first quarter: its and its neighbours' cells kept as their generated ones
    /// where unedited (a copy), so the later quarters read them so after its own grass edits them
    /// (BUGS.md #194: they regenerated the chunk, 3–12 ms natively a quarter).
    fn keep_unedited_as_pristine(&mut self, c: ChunkCoord) {
        // Room for all three first, the chunk's own kept: a clear dropped the copy that let it be
        // picked, and its first quarter regenerated it.
        let mine = Self::grass_chunks(c);
        if self.pristine.len() + 3 > super::PRISTINE_KEPT {
            self.pristine.retain(|k, _| mine.contains(k));
        }
        for k in mine {
            if let Some(ch) = self.world.find(k)
                && ch.revision() == 0
                && !self.pristine.contains_key(&k)
            {
                let copy = Box::new(ch.clone());
                self.pristine.insert(k, copy);
            }
        }
    }

    fn want_pristine(&mut self, missing: Vec<ChunkCoord>) {
        for k in missing {
            if !self.pristine_wanted.contains(&k) && self.pristine_wanted.len() < 32 {
                self.pristine_wanted.push(k);
            }
        }
    }

    fn grass_step(&mut self, wait: bool) {
        // Off in this build (grass.json `grows`, owner 2026-11-24): no world grows, its own switch
        // kept for when it is on again; a test or tool that turns it on still grows it.
        if !self.grass.enabled || !(dwell_worldgen::grass::GRASS.grows || self.grass_forced) {
            return;
        }
        let env = self.grass_envelope.clone();
        let terrain = if env.is_none() {
            match self.grass_terrain() {
                Some(t) => Some(t),
                None => return,
            }
        } else {
            None
        };
        let today = self.world_days().floor().max(0.0) as u32;
        // A chunk grows once its own and the ones under and over it, where edited, are at hand as generated
        // (the jobs' pristine copies): never regenerated on the tick (priority 0, BUGS.md #194).
        let candidates: Vec<ChunkCoord> = self
            .world
            .chunks()
            .map(|(c, _)| *c)
            .filter(|c| self.grass.days.get(c).copied().unwrap_or(0) < today)
            .take(CANDIDATES_PER_TICK)
            .collect();
        // On the tick a chunk grows a quarter of its columns a tick (`grow_chunk_part`), the one in
        // hand first; its day once the fourth is done.
        let mut behind: Vec<(ChunkCoord, Option<u8>)> = Vec::new();
        let in_hand = wait.then_some(self.grass_part).flatten();
        if let Some((c, k)) = in_hand {
            // Part way through: on once its chunks still read as generated without regenerating
            // (else it waits for their pristine copies).
            let missing = self.grass_missing(c);
            if missing.is_empty() {
                behind.push((c, Some(k)));
            } else {
                self.want_pristine(missing);
            }
        }
        for c in candidates.into_iter().filter(|_| in_hand.is_none()) {
            let missing = self.grass_missing(c);
            if missing.is_empty() || !wait {
                if behind.len() < CHUNKS_PER_TICK {
                    if wait {
                        self.keep_unedited_as_pristine(c);
                    }
                    behind.push((c, wait.then_some(0)));
                }
            } else {
                self.want_pristine(missing);
            }
        }
        let rule = &self.grass_rule;
        let mut changes = TickChanges::default();
        let mut writes = Vec::new();
        let regenerated;
        {
            let mut cells = WorldCells {
                world: &mut self.world,
                pristine: &self.pristine,
                cache: Default::default(),
                copies: Default::default(),
            };
            for (c, part) in behind.iter().copied() {
                let last = self.grass.days.get(&c).copied().unwrap_or(0);
                let (first, days) = rule.catch_up(last, today);
                let f = FaceFrame::at_chunk_y(c.y);
                let w = match (&env, &terrain) {
                    (Some(e), _) => {
                        let face = f.face;
                        let e = |x: i32, z: i32| e(face, x, z);
                        rule.grow_chunk_part(f, c, part, first, days, &e, &mut cells)
                    }
                    (None, Some(t)) => {
                        let e = TerrainEnvelope {
                            terrain: t.local(f.face),
                            decoration: rule.data.decoration,
                        };
                        rule.grow_chunk_part(
                            f,
                            c,
                            part,
                            first,
                            days,
                            &e as &dyn Envelope,
                            &mut cells,
                        )
                    }
                    (None, None) => Vec::new(),
                };
                writes.extend(w);
                match part {
                    Some(k) if k < 3 => self.grass_part = Some((c, k + 1)),
                    _ => {
                        self.grass_part = None;
                        self.grass.days.insert(c, today);
                    }
                }
            }
            regenerated = cells.cache.len();
        }
        for (cell, s) in writes {
            changes.set(&mut self.world, cell, s);
        }
        self.grass_regenerated += regenerated;
        if !changes.chunks.is_empty() {
            self.broadcast_changes(changes, VoxelModificationReason::Rebake);
        }
    }
}
