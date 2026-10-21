//! Voxel storage (ARCHITECTURE.md §6.1): 32³ chunks, the world's disc and two faces, and the
//! voxel grid that generates, loads or streams chunks. What bounds building and storage is the
//! build volume (`bounds.rs`).

use crate::FaceFrame;
use crate::bounds::{BUILD_VOLUME, Overlap};
use crate::hash::HashMap;
use std::sync::{Arc, LazyLock};

use dwell_blocks::{StateId, require};
use dwell_protocol::world;

pub const CHUNK_SIZE: i32 = dwell_protocol::CHUNK_SIZE;
pub const CHUNK_VOLUME: usize = (CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE) as usize;

/// The apron (cells per side: x, y, z) of the block a chunk is meshed from: deep enough for the
/// baked sky-visibility rays (ARCHITECTURE.md §6.1, shared/look/ambient.json), whose reach, the
/// ray plus the probe straight up from its end, must stay inside it (dwell-look checks it). It
/// stays inside the 26 neighbours, so an edit's re-mesh of those (`neighbours_changed`) refreshes
/// every chunk whose visibility it can change.
pub const SKY_APRON: [i32; 3] = [6, 16, 6];
/// The sky volume's edges (cells): the chunk and [`SKY_APRON`] on each side.
pub const SKY_EXTENT: [i32; 3] = [
    CHUNK_SIZE + 2 * SKY_APRON[0],
    CHUNK_SIZE + 2 * SKY_APRON[1],
    CHUNK_SIZE + 2 * SKY_APRON[2],
];
/// Cells in the sky volume.
pub const SKY_VOLUME: usize = (SKY_EXTENT[0] * SKY_EXTENT[1] * SKY_EXTENT[2]) as usize;
const _: () = assert!(
    SKY_APRON[0] >= 1
        && SKY_APRON[1] >= 1
        && SKY_APRON[2] >= 1
        && SKY_APRON[0] <= CHUNK_SIZE
        && SKY_APRON[1] <= CHUNK_SIZE
        && SKY_APRON[2] <= CHUNK_SIZE,
    "the sky apron lies inside the 26 neighbours"
);

/// Face A starts here: the halfway plane of the bifacial world (BIFACIAL_WORLD.md), the origin of
/// generation and the build sphere's centre (ADR 0025).
pub const MIDPLANE_Y: i32 = world::MIDPLANE_Y as i32;
/// Terrain: water fills open space below this face-local height (on face A, y). Interim: Phase 17
/// moves it into the terrain definition.
pub const SEA_LEVEL: i32 = world::SEA_LEVEL as i32;
/// The stone core's layers per face, face-local heights 0..CORE_LAYERS, never carved by generation
/// (ADR 0025 D1). Interim: Phase 17 moves it into the terrain definition.
pub const CORE_LAYERS: i32 = world::CORE_LAYERS as i32;
/// The flat worlds' (the flat test world's, the structure sandbox's) surface: their ground fills
/// the midplane up to here.
pub const FLAT_TOP_Y: i32 = world::FLAT_TOP_Y as i32;
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

/// Chunk rows the build volume reaches; outside them everything is air.
pub const MIN_CHUNK_Y: i32 = BUILD_VOLUME.y_range().0.div_euclid(CHUNK_SIZE as i64) as i32;
pub const MAX_CHUNK_Y: i32 = (BUILD_VOLUME.y_range().1 - 1).div_euclid(CHUNK_SIZE as i64) as i32;

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
const _: () = assert!(
    world::CORE_ANCHOR_LAYERS <= world::CORE_LAYERS,
    "the core anchor zone lies within the never-carved core"
);
const _: () = assert!(
    FLAT_TOP_Y > MIDPLANE_Y && FLAT_TOP_Y % CHUNK_SIZE == 0,
    "the flat worlds' surface lies above the midplane, on a chunk boundary"
);

// Shorthands for the face frame's maps (crate::FaceFrame, BIFACIAL_WORLD.md §11).
pub fn face_of_y(y: i64) -> Face {
    if y >= i64::from(MIDPLANE_Y) {
        Face::A
    } else {
        Face::B
    }
}
pub fn face_of_chunk_y(cy: i32) -> Face {
    FaceFrame::at_chunk_y(cy).face
}
/// The direction of up for a voxel row: +1 on face A, −1 on face B (its terrain faces −y).
pub fn up_of_y(y: i32) -> i32 {
    FaceFrame::at_row(y).sign()
}
/// A voxel row mirrored in the midplane (face B's row ↔ its face-local row).
pub const fn mirror_y(y: i32) -> i32 {
    FaceFrame::B.row(y)
}
/// A chunk row mirrored in the midplane.
pub const fn mirror_chunk_y(cy: i32) -> i32 {
    FaceFrame::B.chunk_y(cy)
}
/// The face-local chunk a chunk of face B is the flip of (face A's are their own).
pub fn face_local_chunk(c: ChunkCoord) -> ChunkCoord {
    FaceFrame::at_chunk_y(c.y).chunk(c)
}

/// Chebyshev distance between chunk coordinates.
pub fn chunk_distance(a: ChunkCoord, b: ChunkCoord) -> i32 {
    (a.x - b.x)
        .abs()
        .max((a.y - b.y).abs())
        .max((a.z - b.z).abs())
}

/// What a flat world is made of, by depth below its surface at `FLAT_TOP_Y`: the top layer, the
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

    /// The block `depth` cells below the surface (0: the top layer, at `FLAT_TOP_Y − 1`).
    pub fn at_depth(&self, depth: i64) -> StateId {
        match depth {
            0 => self.top,
            1..=3 => self.under,
            _ => self.base,
        }
    }

    fn at(&self, y: i32) -> StateId {
        if !(MIDPLANE_Y..FLAT_TOP_Y).contains(&y) {
            M.air
        } else {
            self.at_depth(i64::from(FLAT_TOP_Y - 1 - y))
        }
    }
}

/// The flat test world: stone from the midplane up, three layers of dirt, grass topped at
/// `FLAT_TOP_Y`.
pub fn generate_flat_chunk(coord: ChunkCoord, chunk: &mut Chunk) {
    generate_flat_chunk_of(&FlatLayers::standard(), coord, chunk);
}

/// A flat world of `layers`, topped at `FLAT_TOP_Y` from the midplane up (the shape of the flat
/// world), within the disc and the build volume.
pub fn generate_flat_chunk_of(layers: &FlatLayers, coord: ChunkCoord, chunk: &mut Chunk) {
    let disc = chunk_disc_overlap(coord.x, coord.z);
    let volume = BUILD_VOLUME.chunk_overlap(coord);
    if disc == DiscOverlap::Outside || volume == Overlap::Outside {
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
                if volume == Overlap::Partial
                    && !BUILD_VOLUME.contains_voxel(
                        i64::from(coord.x * CHUNK_SIZE + lx),
                        i64::from(coord.y * CHUNK_SIZE + ly),
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
    y0 >= FLAT_TOP_Y
        || y0 + CHUNK_SIZE <= MIDPLANE_Y
        || chunk_disc_overlap(c.x, c.z) == DiscOverlap::Outside
        || !BUILD_VOLUME.chunk_may_hold(c)
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
    /// The flood light of the chunks the holder says are loaded (the client's;
    /// [`Self::enable_light`]).
    light: Option<Box<crate::light::LightMap>>,
    /// The held chunks' placed light sources (docs/LIGHTS.md §4): the cells whose blocks give off a
    /// light source and block light (a torch's flame), kept with the flood light.
    sources: HashMap<ChunkCoord, Vec<[i32; 3]>>,
}

/// Whether a state is a placed light source: it gives off a light (`light`) and block light (the
/// cell of its flame).
pub fn is_light_source(m: StateId) -> bool {
    m != 0
        && dwell_blocks::try_info(m).is_some_and(|s| s.light.is_some() && s.emits > 0)
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
            light: None,
            sources: HashMap::default(),
        }
    }

    /// Keeps a flood light ([`crate::light`]) from now on, of the chunks [`Self::light_load`]
    /// names (the client's streamer: every chunk it holds, all-air ones included).
    pub fn enable_light(&mut self) {
        self.light.get_or_insert_with(Default::default);
    }

    /// The stored chunks (the flood light's tests read them).
    #[cfg(test)]
    pub(crate) fn chunk_table(&self) -> &crate::light::Voxels {
        &self.chunks
    }

    /// The flood light, if kept.
    pub fn light(&self) -> Option<&crate::light::LightMap> {
        self.light.as_deref()
    }

    pub fn light_mut(&mut self) -> Option<&mut crate::light::LightMap> {
        self.light.as_deref_mut()
    }

    /// The chunk is held (its voxels stored, or all air): lit, and its surroundings relit.
    pub fn light_load(&mut self, coord: ChunkCoord) {
        if let Some(l) = self.light.as_deref_mut() {
            l.load(&self.chunks, coord);
            self.index_sources(coord);
        }
    }

    /// Indexes the chunk's light sources (`is_light_source`).
    fn index_sources(&mut self, coord: ChunkCoord) {
        let found: Vec<[i32; 3]> = self.chunks.get(&coord).map_or_else(Vec::new, |c| {
            c.voxels()
                .iter()
                .enumerate()
                .filter(|&(_, &id)| is_light_source(id))
                .map(|(i, _)| {
                    // `local_index`: x | y << 5 | z << 10.
                    let (x, y, z) = (i as i32 & 31, (i as i32 >> 5) & 31, i as i32 >> 10);
                    [
                        coord.x * CHUNK_SIZE + x,
                        coord.y * CHUNK_SIZE + y,
                        coord.z * CHUNK_SIZE + z,
                    ]
                })
                .collect()
        });
        if found.is_empty() {
            self.sources.remove(&coord);
        } else {
            self.sources.insert(coord, found);
        }
    }

    /// The placed light sources within `radius` (m) of `centre` (world metres), nearest first:
    /// each cell and its state (`is_light_source`; the client's, with the flood light).
    pub fn light_sources_near(&self, centre: [f64; 3], radius: f64) -> Vec<([i32; 3], StateId)> {
        let mut out: Vec<(f64, [i32; 3], StateId)> = Vec::new();
        for (coord, cells) in &self.sources {
            let lo = crate::pos_of_cell([
                coord.x * CHUNK_SIZE,
                coord.y * CHUNK_SIZE,
                coord.z * CHUNK_SIZE,
            ]);
            let hi = crate::pos_of_cell([
                (coord.x + 1) * CHUNK_SIZE,
                (coord.y + 1) * CHUNK_SIZE,
                (coord.z + 1) * CHUNK_SIZE,
            ]);
            let gap: f64 = (0..3)
                .map(|k| {
                    let (a, b) = (lo[k].min(hi[k]), lo[k].max(hi[k]));
                    (a - centre[k]).max(centre[k] - b).max(0.0).powi(2)
                })
                .sum();
            if gap > radius * radius {
                continue;
            }
            for &c in cells {
                let p = crate::pos_of_cell(c);
                let d2: f64 = (0..3)
                    .map(|k| (p[k] + 0.5 * crate::CELL[k] - centre[k]).powi(2))
                    .sum();
                if d2 <= radius * radius {
                    out.push((d2, c, self.get_voxel_if_loaded(c)));
                }
            }
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        out.into_iter().map(|(_, c, m)| (c, m)).collect()
    }

    /// The chunk is no longer held.
    pub fn light_unload(&mut self, coord: ChunkCoord) {
        if let Some(l) = self.light.as_deref_mut() {
            l.unload(&self.chunks, coord);
        }
        self.sources.remove(&coord);
    }

    /// The chunks are no longer held, relit in one update (`LightMap::unload_many`).
    pub fn light_unload_many(&mut self, coords: &[ChunkCoord]) {
        if let Some(l) = self.light.as_deref_mut() {
            l.unload_many(&self.chunks, coords);
        }
        for c in coords {
            self.sources.remove(c);
        }
    }

    /// These world cells' blocks changed.
    pub fn light_edited(&mut self, cells: &[[i32; 3]]) {
        if let Some(l) = self.light.as_deref_mut() {
            l.edited(&self.chunks, cells);
            for &c in cells {
                let coord = crate::chunk_of(c[0], c[1], c[2]);
                let source = is_light_source(self.get_voxel_if_loaded(c));
                let list = self.sources.entry(coord).or_default();
                list.retain(|&o| o != c);
                if source {
                    list.push(c);
                }
                if list.is_empty() {
                    self.sources.remove(&coord);
                }
            }
        }
    }

    /// The chunk's light in the mesher's padded layout (empty without a flood light: meshed as
    /// open sky).
    pub fn padded_light(&self, coord: ChunkCoord) -> Vec<u8> {
        self.light
            .as_deref()
            .map_or_else(Vec::new, |l| l.padded(coord))
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

    /// The voxel of a stored chunk (air elsewhere): a read that never generates or loads.
    pub fn get_voxel_if_loaded(&self, [x, y, z]: [i32; 3]) -> StateId {
        let c = chunk_of(x, y, z);
        self.chunks.get(&c).map_or(0, |ch| {
            ch.get(x - c.x * CHUNK_SIZE, y - c.y * CHUNK_SIZE, z - c.z * CHUNK_SIZE)
        })
    }

    pub fn get_voxel(&mut self, x: i32, y: i32, z: i32) -> StateId {
        let c = chunk_of(x, y, z);
        self.read(c).get(
            x - c.x * CHUNK_SIZE,
            y - c.y * CHUNK_SIZE,
            z - c.z * CHUNK_SIZE,
        )
    }

    /// The shape the voxel is drawn, collided and targeted as: its state's own (no block is shaped
    /// by its neighbours, ADR 0042; a face-B state is stored turned over).
    pub fn shape_at(&mut self, x: i32, y: i32, z: i32) -> &'static dwell_blocks::ShapeInfo {
        dwell_blocks::shape::shape_of(self.get_voxel(x, y, z))
    }

    /// Whether the voxel holds water in its open part: a flooded state (a ramp under the sea).
    pub fn flooded_at(&mut self, x: i32, y: i32, z: i32) -> bool {
        dwell_blocks::try_info(self.get_voxel(x, y, z)).is_some_and(|s| s.flooded)
    }

    /// The chunk's voxels with an apron of `pad` (1..=CHUNK_SIZE) voxels from its 26 neighbours,
    /// x fastest, then z, then y: index `(x + pad) + e·((z + pad) + e·(y + pad))` with
    /// e = CHUNK_SIZE + 2·pad. Each neighbour is read once (generated if needed; air when missing
    /// from a streamed world).
    pub fn padded_block(&mut self, coord: ChunkCoord, pad: i32) -> Vec<StateId> {
        self.padded_box(coord, [pad; 3])
    }

    /// [`Self::padded_block`] with its own apron per axis (`apron[a]` in 1..=CHUNK_SIZE cells):
    /// the block is `CHUNK_SIZE + 2·apron[a]` cells along axis `a`, x fastest, then z, then y
    /// (the meshers' sky volume, [`SKY_APRON`]).
    pub fn padded_box(&mut self, coord: ChunkCoord, apron: [i32; 3]) -> Vec<StateId> {
        let n = CHUNK_SIZE;
        let [ax, ay, az] = apron;
        let (ex, ey, ez) = (n + 2 * ax, n + 2 * ay, n + 2 * az);
        let mut out = vec![0; (ex * ey * ez) as usize];
        for dy in -1..=1 {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let c = ChunkCoord::new(coord.x + dx, coord.y + dy, coord.z + dz);
                    let chunk = self.read(c);
                    // The part of this neighbour inside the padded block, in its local coordinates.
                    let range = |d: i32, pad: i32| match d {
                        -1 => (n - pad, n),
                        0 => (0, n),
                        _ => (0, pad),
                    };
                    let (x0, x1) = range(dx, ax);
                    let (y0, y1) = range(dy, ay);
                    let (z0, z1) = range(dz, az);
                    for ly in y0..y1 {
                        let py = ly + dy * n + ay;
                        for lz in z0..z1 {
                            let pz = lz + dz * n + az;
                            let row = (pz + ez * py) * ex;
                            let px0 = x0 + dx * n + ax;
                            let src = local_index(x0, ly, lz);
                            let len = (x1 - x0) as usize;
                            out[(row + px0) as usize..(row + px0) as usize + len]
                                .copy_from_slice(&chunk.voxels()[src..src + len]);
                        }
                    }
                }
            }
        }
        out
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
