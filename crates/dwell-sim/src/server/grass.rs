//! The server's side of tall grass growing and spreading (ADR 0075 T3c, docs/ECOLOGY.md §10): every
//! few seconds of the tick, each loaded chunk that hasn't grown up to today's whole world day grows
//! the days it missed (at most the data's `catchUpDays`: on its first load since day 0, after an
//! idle stretch), by `dwell_worldgen::grass::Rule::grow_chunk` over the face terrain's envelope. The
//! writes go out as one modification. The world's switch and each grown chunk's last day are saved
//! in an optional `grass_growth` table that older builds ignore.

use super::{Server, TickChanges};
use dwell_protocol::{SIM_HZ, VoxelModificationReason};
use dwell_world::{ChunkCoord, Face, FaceFrame, VoxelWorld, chunk_of};
use dwell_worldgen::bifacial::BifacialTerrain;
use dwell_worldgen::grass::{Cells, Envelope, TerrainEnvelope};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

/// How often the grass step runs (s of the tick).
const EVERY_S: u32 = 5;
/// The most chunks grown per step (the rest on the next).
const CHUNKS_PER_STEP: usize = 64;
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
    cache: dwell_world::hash::HashMap<ChunkCoord, Box<dwell_world::Chunk>>,
}

impl Cells for WorldCells<'_> {
    fn now(&self, c: [i32; 3]) -> Option<u16> {
        self.world
            .find(chunk_of(c[0], c[1], c[2]))
            .is_some()
            .then(|| self.world.get_voxel_if_loaded(c))
    }
    fn generated(&mut self, c: [i32; 3]) -> Option<u16> {
        self.world
            .find(chunk_of(c[0], c[1], c[2]))
            .is_some()
            .then(|| self.world.generated_voxel(c, &mut self.cache))
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

    /// Tests, tools: the envelope in place of the terrain's, and the switch.
    pub fn set_grass_envelope(&mut self, env: Option<GrassEnvelope>) {
        self.grass_envelope = env;
    }

    /// Tests, tools: turns the world's grass growth on or off.
    pub fn set_grass_enabled(&mut self, on: bool) {
        self.grass.enabled = on;
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
    fn grass_terrain(&mut self) -> Option<Arc<BifacialTerrain>> {
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

    /// The grass step (every `EVERY_S` s).
    pub(super) fn grow_grass(&mut self) {
        if !self.tick.is_multiple_of(EVERY_S * SIM_HZ) {
            return;
        }
        self.grow_grass_now();
    }

    /// The grass step now: up to `CHUNKS_PER_STEP` loaded chunks behind today grow their days.
    pub fn grow_grass_now(&mut self) {
        if !self.grass.enabled {
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
        let rule = &self.grass_rule;
        let behind: Vec<ChunkCoord> = self
            .world
            .chunks()
            .map(|(c, _)| *c)
            .filter(|c| self.grass.days.get(c).copied().unwrap_or(0) < today)
            .take(CHUNKS_PER_STEP)
            .collect();
        let mut changes = TickChanges::default();
        let mut writes = Vec::new();
        {
            let mut cells = WorldCells {
                world: &mut self.world,
                cache: Default::default(),
            };
            for c in behind {
                let last = self.grass.days.get(&c).copied().unwrap_or(0);
                let (first, days) = rule.catch_up(last, today);
                let f = FaceFrame::at_chunk_y(c.y);
                let w = match (&env, &terrain) {
                    (Some(e), _) => {
                        let face = f.face;
                        let e = |x: i32, z: i32| e(face, x, z);
                        rule.grow_chunk(f, c, first, days, &e, &mut cells)
                    }
                    (None, Some(t)) => {
                        let e = TerrainEnvelope {
                            terrain: t.local(f.face),
                            decoration: rule.data.decoration,
                        };
                        rule.grow_chunk(f, c, first, days, &e as &dyn Envelope, &mut cells)
                    }
                    (None, None) => Vec::new(),
                };
                writes.extend(w);
                self.grass.days.insert(c, today);
            }
        }
        for (cell, s) in writes {
            changes.set(&mut self.world, cell, s);
        }
        if !changes.chunks.is_empty() {
            self.broadcast_changes(changes, VoxelModificationReason::Rebake);
        }
    }
}
