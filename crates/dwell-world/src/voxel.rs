//! Voxel storage (ARCHITECTURE.md §6.1): 32³ chunks, the world's bounds and two faces, and the
//! voxel grid that generates, loads or streams chunks.

use crate::hash::HashMap;
use std::sync::{Arc, LazyLock};

use dwell_blocks::{StateId, require};
use dwell_protocol::world;

pub const CHUNK_SIZE: i32 = dwell_protocol::CHUNK_SIZE;
pub const CHUNK_VOLUME: usize = (CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE) as usize;

/// Face A starts here: the halfway plane of the bifacial world (BIFACIAL_WORLD.md).
pub const MIDPLANE_Y: i32 = world::MIDPLANE_Y as i32;
/// Face B's lowest voxel row.
pub const WORLD_BOTTOM_Y: i32 = world::WORLD_BOTTOM_Y as i32;
/// Generated terrain stays below this.
pub const WORLD_MAX_Y: i32 = world::WORLD_MAX_Y as i32;
/// Terrain: water fills open space below.
pub const SEA_LEVEL: i32 = world::SEA_LEVEL as i32;
/// The world disc's radius (m); beyond it, the void.
pub const WORLD_RADIUS: i32 = world::WORLD_RADIUS as i32;

/// The built-in states the engine itself refers to, resolved by name once.
pub struct Materials {
    pub air: StateId,
    pub stone: StateId,
    pub dirt: StateId,
    pub grass: StateId,
    pub sand: StateId,
    pub water: StateId,
}

pub static M: LazyLock<Materials> = LazyLock::new(|| Materials {
    air: require("dwell:air"),
    stone: require("dwell:stone"),
    dirt: require("dwell:dirt"),
    grass: require("dwell:grass"),
    sand: require("dwell:sand"),
    water: require("dwell:water"),
});

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkCoord {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl ChunkCoord {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
    pub fn to_array(self) -> [i32; 3] {
        [self.x, self.y, self.z]
    }
    pub fn from_array(a: [i32; 3]) -> Self {
        Self::new(a[0], a[1], a[2])
    }
}

pub const fn floor_div(a: i32, b: i32) -> i32 {
    let q = a / b;
    if a % b != 0 && ((a < 0) != (b < 0)) {
        q - 1
    } else {
        q
    }
}

/// World voxel → its chunk (floor division).
pub fn chunk_of(x: i32, y: i32, z: i32) -> ChunkCoord {
    ChunkCoord::new(
        floor_div(x, CHUNK_SIZE),
        floor_div(y, CHUNK_SIZE),
        floor_div(z, CHUNK_SIZE),
    )
}

/// Chunk-local voxel index: `x | y << 5 | z << 10`.
pub const fn local_index(lx: i32, ly: i32, lz: i32) -> usize {
    (lx | (ly << 5) | (lz << 10)) as usize
}

#[derive(Clone)]
pub struct Chunk {
    voxels: Box<[StateId; CHUNK_VOLUME]>,
    revision: u32,
    /// Set by the world each time it stores a chunk object (0: never stored, e.g. the shared air
    /// chunk): with the revision, it tells caches (terrain collision) the content may have changed.
    stamp: u64,
}

impl Default for Chunk {
    fn default() -> Self {
        Self {
            voxels: Box::new([0; CHUNK_VOLUME]),
            revision: 0,
            stamp: 0,
        }
    }
}

impl Chunk {
    /// Which stored chunk object this is: changes whenever the world stores a chunk here.
    pub fn stamp(&self) -> u64 {
        self.stamp
    }
    pub fn get(&self, lx: i32, ly: i32, lz: i32) -> StateId {
        self.voxels[local_index(lx, ly, lz)]
    }
    pub fn set(&mut self, lx: i32, ly: i32, lz: i32, m: StateId) {
        self.voxels[local_index(lx, ly, lz)] = m;
        self.revision += 1;
    }
    pub fn revision(&self) -> u32 {
        self.revision
    }
    /// Edits (§6.5): voxels changed without per-voxel bumps, then one bump per modification.
    pub fn set_at(&mut self, index: usize, m: StateId) {
        self.voxels[index] = m;
    }
    pub fn bump_revision(&mut self) {
        self.revision += 1;
    }
    /// An unmodified generated chunk is revision 0.
    pub fn reset_revision(&mut self) {
        self.revision = 0;
    }
    pub fn set_revision(&mut self, revision: u32) {
        self.revision = revision;
    }
    pub fn voxels(&self) -> &[StateId; CHUNK_VOLUME] {
        &self.voxels
    }
    /// Generators write here directly (no revision bumps).
    pub fn voxels_mut(&mut self) -> &mut [StateId; CHUNK_VOLUME] {
        &mut self.voxels
    }
    pub fn from_voxels(voxels: &[StateId], revision: u32) -> Option<Self> {
        let mut c = Self {
            revision,
            ..Self::default()
        };
        if voxels.len() != CHUNK_VOLUME {
            return None;
        }
        c.voxels.copy_from_slice(voxels);
        Some(c)
    }
}

/// Fills a fresh (all-air) chunk; a pure function of the coordinate (§6.3).
pub type ChunkGenerator = Arc<dyn Fn(ChunkCoord, &mut Chunk) + Send + Sync>;
/// True for chunks a generator leaves all air, known without generating them.
pub type AirChunkTest = Arc<dyn Fn(ChunkCoord) -> bool + Send + Sync>;

/// Generator versions announced in Welcome (§6.3): 0 the flat test world, 8 procedural terrain,
/// 100 the structure sandbox (the flat world in its floor block; numbered away from the terrain's
/// versions, which grow). 1–7 are retired and load as the flat world, as any unknown version does.
pub const GENERATOR_FLAT: u32 = 0;
pub const GENERATOR_TERRAIN: u32 = 8;
pub const GENERATOR_SANDBOX: u32 = 100;

/// A world's type by its generator (the names `client/src/local/worldIndex.ts` and data files use,
/// e.g. `shared/brushes/brushes.json`): unknown and retired versions load as the flat world.
pub fn world_type(generator_version: u32) -> &'static str {
    match generator_version {
        GENERATOR_TERRAIN => "terrain",
        GENERATOR_SANDBOX => "sandbox",
        _ => "flat",
    }
}

/// FNV-1a 64 over a chunk's ids (u16 little-endian, index order): goldens and `WorldgenCheck`.
pub fn chunk_hash(chunk: &Chunk) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &m in chunk.voxels().iter() {
        for b in m.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// A voxel where `current` differs from the chunk as generated ("regenerate and diff").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoxelDiff {
    pub index: usize,
    pub generated: StateId,
    pub current: StateId,
}

pub fn diff_chunk(generated: &Chunk, current: &Chunk) -> Vec<VoxelDiff> {
    generated
        .voxels()
        .iter()
        .zip(current.voxels().iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(index, (&generated, &current))| VoxelDiff {
            index,
            generated,
            current,
        })
        .collect()
}

/// A column (x, z) is inside the world's disc when x² + z² < R² (ADR 0011).
pub fn inside_world_disc(x: i64, z: i64) -> bool {
    let r = i64::from(WORLD_RADIUS);
    x * x + z * z < r * r
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscOverlap {
    Inside,
    Partial,
    Outside,
}

/// How a chunk column's 32 × 32 voxel columns lie relative to the disc.
pub fn chunk_disc_overlap(cx: i32, cz: i32) -> DiscOverlap {
    let s = i64::from(CHUNK_SIZE);
    let nearest = |c: i32| {
        let lo = i64::from(c) * s;
        let hi = lo + s - 1;
        if lo > 0 {
            lo
        } else if hi < 0 {
            hi
        } else {
            0
        }
    };
    let farthest = |c: i32| {
        let lo = i64::from(c) * s;
        let hi = lo + s - 1;
        if -lo > hi { lo } else { hi }
    };
    let r2 = i64::from(WORLD_RADIUS) * i64::from(WORLD_RADIUS);
    let (nx, nz) = (nearest(cx), nearest(cz));
    if nx * nx + nz * nz >= r2 {
        return DiscOverlap::Outside;
    }
    let (fx, fz) = (farthest(cx), farthest(cz));
    if fx * fx + fz * fz < r2 {
        DiscOverlap::Inside
    } else {
        DiscOverlap::Partial
    }
}

/// Chunk rows the world can hold anything in; outside is air.
pub const MIN_CHUNK_Y: i32 = WORLD_BOTTOM_Y / CHUNK_SIZE;
pub const MAX_CHUNK_Y: i32 = WORLD_MAX_Y / CHUNK_SIZE - 1;

/// The two faces of the disc (BIFACIAL_WORLD.md §2): A at or above the midplane, B its mirror image
/// below. Face B's voxel y is face-local height `MIRROR_SUM − y`; its chunk row cy is
/// `MIRROR_CHUNK_SUM − cy`. The mirror is its own inverse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    A = 0,
    B = 1,
}

pub const MIRROR_SUM: i32 = 2 * MIDPLANE_Y - 1;
pub const MIDPLANE_CHUNK_Y: i32 = MIDPLANE_Y / CHUNK_SIZE;
pub const MIRROR_CHUNK_SUM: i32 = 2 * MIDPLANE_CHUNK_Y - 1;
const _: () = assert!(
    MIDPLANE_Y % CHUNK_SIZE == 0,
    "the midplane lies on a chunk boundary"
);

pub fn face_of_y(y: i64) -> Face {
    if y >= i64::from(MIDPLANE_Y) {
        Face::A
    } else {
        Face::B
    }
}
pub fn face_of_chunk_y(cy: i32) -> Face {
    if cy >= MIDPLANE_CHUNK_Y {
        Face::A
    } else {
        Face::B
    }
}
pub const fn mirror_y(y: i32) -> i32 {
    MIRROR_SUM - y
}
pub const fn mirror_chunk_y(cy: i32) -> i32 {
    MIRROR_CHUNK_SUM - cy
}
/// The face-local chunk a chunk of face B is the flip of (face A's are their own).
pub fn face_local_chunk(c: ChunkCoord) -> ChunkCoord {
    match face_of_chunk_y(c.y) {
        Face::A => c,
        Face::B => ChunkCoord::new(c.x, mirror_chunk_y(c.y), c.z),
    }
}

/// Chebyshev distance between chunk coordinates.
pub fn chunk_distance(a: ChunkCoord, b: ChunkCoord) -> i32 {
    (a.x - b.x)
        .abs()
        .max((a.y - b.y).abs())
        .max((a.z - b.z).abs())
}

/// What a flat world is made of, by depth below its surface at y = 0: the top layer (y = −1), the
/// three below it, and the rest down to the midplane. The flat test world is grass over dirt over
/// stone; the structure sandbox is its floor block throughout ([`FlatLayers::of`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlatLayers {
    pub top: StateId,
    pub under: StateId,
    pub base: StateId,
}

impl FlatLayers {
    /// The flat test world's layers.
    pub fn standard() -> Self {
        Self {
            top: M.grass,
            under: M.dirt,
            base: M.stone,
        }
    }

    /// One block throughout.
    pub fn of(m: StateId) -> Self {
        Self {
            top: m,
            under: m,
            base: m,
        }
    }

    /// The block `depth` cells below the surface (0: the top layer, at y = −1).
    pub fn at_depth(&self, depth: i64) -> StateId {
        match depth {
            0 => self.top,
            1..=3 => self.under,
            _ => self.base,
        }
    }

    fn at(&self, y: i32) -> StateId {
        if !(MIDPLANE_Y..0).contains(&y) {
            M.air
        } else {
            self.at_depth(i64::from(-1 - y))
        }
    }
}

/// The flat test world: stone from the midplane up, three layers of dirt, grass topped at y = 0.
pub fn generate_flat_chunk(coord: ChunkCoord, chunk: &mut Chunk) {
    generate_flat_chunk_of(&FlatLayers::standard(), coord, chunk);
}

/// A flat world of `layers`, topped at y = 0 from the midplane up (the shape of the flat world).
pub fn generate_flat_chunk_of(layers: &FlatLayers, coord: ChunkCoord, chunk: &mut Chunk) {
    let disc = chunk_disc_overlap(coord.x, coord.z);
    if disc == DiscOverlap::Outside {
        return;
    }
    for ly in 0..CHUNK_SIZE {
        let m = layers.at(coord.y * CHUNK_SIZE + ly);
        if m == M.air {
            continue;
        }
        for lz in 0..CHUNK_SIZE {
            for lx in 0..CHUNK_SIZE {
                if disc == DiscOverlap::Partial
                    && !inside_world_disc(
                        i64::from(coord.x * CHUNK_SIZE + lx),
                        i64::from(coord.z * CHUNK_SIZE + lz),
                    )
                {
                    continue;
                }
                chunk.voxels_mut()[local_index(lx, ly, lz)] = m;
            }
        }
    }
}

pub fn flat_is_air(c: ChunkCoord) -> bool {
    let y0 = c.y * CHUNK_SIZE;
    y0 >= 0 || y0 + CHUNK_SIZE <= MIDPLANE_Y || chunk_disc_overlap(c.x, c.z) == DiscOverlap::Outside
}

pub fn flat_generator() -> ChunkGenerator {
    Arc::new(generate_flat_chunk)
}

/// The flat world's generator made of `layers` (its all-air test is [`flat_is_air`]).
pub fn flat_generator_of(layers: FlatLayers) -> ChunkGenerator {
    Arc::new(move |coord, chunk| generate_flat_chunk_of(&layers, coord, chunk))
}

/// All air (tests build their geometry voxel by voxel).
pub fn empty_generator() -> ChunkGenerator {
    Arc::new(|_, _| {})
}

/// Chunks kept outside memory — the world file (§6.4). A world loads them rather than generating
/// them, and never reads them as air.
pub trait SavedChunks: Send {
    fn has(&self, c: ChunkCoord) -> bool;
    /// `None` on failure: the chunk is generated instead.
    fn load(&mut self, c: ChunkCoord) -> Option<Chunk>;
}

static AIR_CHUNK: LazyLock<Chunk> = LazyLock::new(Chunk::default);

/// The voxel grid. With a generator (the server, tests) missing chunks are generated on first
/// access, and unmodified ones can be evicted and regenerated. Without one (a *streamed* world: the
/// client) chunks arrive through `put` and missing chunks read as air.
pub struct VoxelWorld {
    generator: Option<ChunkGenerator>,
    air: Option<AirChunkTest>,
    saved: Option<Box<dyn SavedChunks>>,
    chunks: HashMap<ChunkCoord, Box<Chunk>>,
    epoch: u64,
    next_stamp: u64,
    generated_on_access: u64,
    loaded_saved: u64,
}

impl VoxelWorld {
    pub fn new(generator: ChunkGenerator, air: Option<AirChunkTest>) -> Self {
        Self {
            generator: Some(generator),
            air,
            saved: None,
            chunks: HashMap::default(),
            epoch: 0,
            next_stamp: 1,
            generated_on_access: 0,
            loaded_saved: 0,
        }
    }
    pub fn flat() -> Self {
        Self::new(flat_generator(), Some(Arc::new(flat_is_air)))
    }
    pub fn streamed() -> Self {
        Self {
            generator: None,
            ..Self::new(empty_generator(), None)
        }
    }
    pub fn is_streamed(&self) -> bool {
        self.generator.is_none()
    }
    pub fn set_saved(&mut self, saved: Box<dyn SavedChunks>) {
        self.saved = Some(saved);
    }

    fn saved_has(&self, c: ChunkCoord) -> bool {
        self.saved.as_ref().is_some_and(|s| s.has(c))
    }

    /// Generates a missing chunk (streamed worlds: creates an all-air one).
    pub fn get_or_create(&mut self, coord: ChunkCoord) -> &mut Chunk {
        if !self.chunks.contains_key(&coord) {
            let mut chunk = None;
            if self.saved_has(coord)
                && let Some(c) = self.saved.as_mut().and_then(|s| s.load(coord))
            {
                self.loaded_saved += 1;
                chunk = Some(c);
            }
            let chunk = chunk.unwrap_or_else(|| {
                let mut c = Chunk::default();
                if let Some(g) = &self.generator {
                    g(coord, &mut c);
                    self.generated_on_access += 1;
                }
                c.reset_revision();
                c
            });
            self.store(coord, chunk);
        }
        self.chunks.get_mut(&coord).expect("inserted")
    }

    pub fn find(&self, coord: ChunkCoord) -> Option<&Chunk> {
        self.chunks.get(&coord).map(|c| &**c)
    }
    pub fn find_mut(&mut self, coord: ChunkCoord) -> Option<&mut Chunk> {
        self.chunks.get_mut(&coord).map(|c| &mut **c)
    }

    /// A chunk to read: generated on demand, or all air while missing in a streamed world or when
    /// the air test says the generator would leave it empty (not stored).
    pub fn read(&mut self, coord: ChunkCoord) -> &Chunk {
        if self.generator.is_some() {
            if let Some(air) = &self.air
                && !self.chunks.contains_key(&coord)
                && !self.saved_has(coord)
                && air(coord)
            {
                return &AIR_CHUNK;
            }
            return self.get_or_create(coord);
        }
        self.chunks.get(&coord).map_or(&AIR_CHUNK, |c| c)
    }

    pub fn get_voxel(&mut self, x: i32, y: i32, z: i32) -> StateId {
        let c = chunk_of(x, y, z);
        self.read(c).get(
            x - c.x * CHUNK_SIZE,
            y - c.y * CHUNK_SIZE,
            z - c.z * CHUNK_SIZE,
        )
    }

    pub fn set_voxel(&mut self, x: i32, y: i32, z: i32, m: StateId) {
        let c = chunk_of(x, y, z);
        self.get_or_create(c).set(
            x - c.x * CHUNK_SIZE,
            y - c.y * CHUNK_SIZE,
            z - c.z * CHUNK_SIZE,
            m,
        );
    }

    /// Inserts or replaces a chunk (streamed chunks; chunks generated off-thread).
    pub fn put(&mut self, coord: ChunkCoord, chunk: Chunk) {
        self.store(coord, chunk);
    }

    fn store(&mut self, coord: ChunkCoord, mut chunk: Chunk) {
        chunk.stamp = self.next_stamp;
        self.next_stamp += 1;
        self.chunks.insert(coord, Box::new(chunk));
        self.epoch += 1;
    }
    pub fn remove(&mut self, coord: ChunkCoord) {
        if self.chunks.remove(&coord).is_some() {
            self.epoch += 1;
        }
    }
    /// Drops unmodified (revision 0) chunks that `keep` rejects; returns how many.
    pub fn evict_unmodified(&mut self, keep: impl Fn(ChunkCoord) -> bool) -> usize {
        self.evict(|c, chunk| chunk.revision() == 0 && !keep(c))
    }
    /// Drops the chunks `evict` selects.
    pub fn evict(&mut self, evict: impl Fn(ChunkCoord, &Chunk) -> bool) -> usize {
        let before = self.chunks.len();
        self.chunks.retain(|&c, chunk| !evict(c, chunk));
        let n = before - self.chunks.len();
        if n > 0 {
            self.epoch += 1;
        }
        n
    }
    pub fn chunks(&self) -> impl Iterator<Item = (&ChunkCoord, &Chunk)> {
        self.chunks.iter().map(|(c, ch)| (c, &**ch))
    }
    /// Changes whenever a chunk is replaced or removed (or appears): references may be stale.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn loaded_chunks(&self) -> usize {
        self.chunks.len()
    }
    pub fn generated_on_access(&self) -> u64 {
        self.generated_on_access
    }
    pub fn loaded_saved(&self) -> u64 {
        self.loaded_saved
    }
}
