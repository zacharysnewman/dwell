//! The client's side of terrain streaming (ARCHITECTURE.md §6.3) and voxel edits (§6.5): turns the
//! server's ChunkData and ChunkUnload messages into chunks in the client sim (Generated ones via the
//! worldgen workers, Explicit ones from the message), applies VoxelModifications in revision order
//! (a gap asks the server to resync the chunk), and keeps render meshes of the loaded chunks current
//! through the meshing workers.
//!
//! The TS awaited promises from the worldgen and mesher pools; here a job is started through
//! [`ChunkSource::generate`] / [`Mesher::mesh`] with a token, and the shell reports its result with
//! [`ChunkStreamer::on_generated`] (or [`ChunkStreamer::on_generate_failed`]) and
//! [`ChunkStreamer::on_meshed`]. A result whose token is no longer current is ignored.

use dwell_world::hash::{HashMap, HashSet};

use dwell_protocol::{CHUNK_SIZE, CHUNK_VOLUME, ChunkChanges, ChunkData, ChunkForm};
use dwell_world::edit::apply_chunk_changes;
use dwell_world::{
    BUILD_VOLUME, Chunk, ChunkCoord, Face, VoxelWorld, face_of_chunk_y, local_index,
};
pub use dwell_world::{MAX_CHUNK_Y, MIN_CHUNK_Y};

use crate::chunk_diff::{PADDED_VOLUME, padded_index};

/// A chunk coordinate as on the wire.
pub type Coord = [i32; 3];

/// Edit prediction (`edit_prediction.rs`) reads and writes the client's world through the
/// streamer: a write is a local change of loaded chunks, remeshed like a modification.
impl<G, M, V> crate::edit_prediction::PredictionWorld for ChunkStreamer<VoxelWorld, G, M, V>
where
    G: ChunkSource,
    M: Mesher,
    V: TerrainView<M::Meshes>,
{
    fn state(&mut self, c: [i32; 3]) -> u16 {
        self.store_mut().get_voxel(c[0], c[1], c[2])
    }
    fn show(&mut self, cells: &[([i32; 3], u16)]) {
        self.set_local(cells);
    }
}

/// The region tools read the client's world through the streamer (structures too): a box is loaded
/// when every chunk it covers is, Air chunks included — the client never stores those, and its
/// world reads them as air. (`region::read_box` on the bare world counts them as missing.) Chunks
/// wholly outside the build volume are never streamed: they read as air.
impl<G, M, V> crate::region_tools::BoxSource for ChunkStreamer<VoxelWorld, G, M, V>
where
    G: ChunkSource,
    M: Mesher,
    V: TerrainView<M::Meshes>,
{
    fn read_box(&mut self, min: [i32; 3], size: [i32; 3]) -> Option<Vec<u16>> {
        if size.iter().any(|&n| n < 1) {
            return Some(Vec::new());
        }
        let lo = min.map(|v| v.div_euclid(CHUNK_SIZE));
        let hi: [i32; 3] = std::array::from_fn(|i| (min[i] + size[i] - 1).div_euclid(CHUNK_SIZE));
        for y in lo[1]..=hi[1] {
            for z in lo[2]..=hi[2] {
                for x in lo[0]..=hi[0] {
                    if !self.is_loaded([x, y, z])
                        && BUILD_VOLUME.chunk_may_hold(ChunkCoord::new(x, y, z))
                    {
                        return None;
                    }
                }
            }
        }
        let world = self.store_mut();
        let mut out = Vec::with_capacity(size.iter().map(|&n| n as usize).product());
        for y in 0..size[1] {
            for z in 0..size[2] {
                for x in 0..size[0] {
                    out.push(world.get_voxel(min[0] + x, min[1] + y, min[2] + z));
                }
            }
        }
        Some(out)
    }
}

/// What the streamer needs from the client sim.
pub trait ChunkStore {
    fn set_chunk(&mut self, coord: Coord, revision: u32, voxels: &[u16]);
    fn remove_chunk(&mut self, coord: Coord);
    /// Applies the changes; the chunk takes their revision.
    fn edit_chunk(&mut self, changes: &ChunkChanges);
    /// The chunk's voxels with a one-voxel apron (the mesher's layout), for meshing.
    fn padded_chunk(&mut self, coord: Coord) -> Vec<u16>;
}

/// The client sim's voxel world is the store.
impl ChunkStore for VoxelWorld {
    fn set_chunk(&mut self, coord: Coord, revision: u32, voxels: &[u16]) {
        if let Some(chunk) = Chunk::from_voxels(voxels, revision) {
            self.put(ChunkCoord::from_array(coord), chunk);
        }
    }
    fn remove_chunk(&mut self, coord: Coord) {
        self.remove(ChunkCoord::from_array(coord));
    }
    fn edit_chunk(&mut self, changes: &ChunkChanges) {
        apply_chunk_changes(self, changes);
    }
    fn padded_chunk(&mut self, coord: Coord) -> Vec<u16> {
        padded_from_neighbours(self, coord)
    }
}

/// The chunk at `coord` with its one-voxel apron from the 26 neighbours. Each of the 27 chunks is
/// looked up once (`read` first, so a world with a generator makes them, then `find`), and rows
/// are copied: a hash lookup per voxel (34³ of them) was most of the main thread's chunk meshing
/// time (docs/DEV_TOOLS.md profile, 2026-10-14). Equal to `get_voxel` per voxel.
pub fn padded_from_neighbours(world: &mut VoxelWorld, coord: Coord) -> Vec<u16> {
    let n = CHUNK_SIZE;
    let mut near: [Option<&[u16; CHUNK_VOLUME]>; 27] = [None; 27];
    let key = |dx: i32, dy: i32, dz: i32| ((dx + 1) + 3 * (dy + 1) + 9 * (dz + 1)) as usize;
    let coord_of =
        |dx: i32, dy: i32, dz: i32| ChunkCoord::new(coord[0] + dx, coord[1] + dy, coord[2] + dz);
    for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                world.read(coord_of(dx, dy, dz));
            }
        }
    }
    for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                near[key(dx, dy, dz)] = world.find(coord_of(dx, dy, dz)).map(|c| c.voxels());
            }
        }
    }
    // Which chunk and local coordinate a padded coordinate (−1..=n) falls in.
    let split = |v: i32| -> (i32, i32) {
        if v < 0 {
            (-1, v + n)
        } else if v >= n {
            (1, v - n)
        } else {
            (0, v)
        }
    };
    let mut out = vec![0; PADDED_VOLUME];
    for y in -1..=n {
        let (dy, ly) = split(y);
        for z in -1..=n {
            let (dz, lz) = split(z);
            let row = padded_index(0, y, z);
            let local = |lx: i32| local_index(lx, ly, lz);
            // The row's middle from the chunk at (0, dy, dz), its two ends from the x neighbours.
            if let Some(c) = near[key(0, dy, dz)] {
                let start = local(0);
                out[row..row + n as usize].copy_from_slice(&c[start..start + n as usize]);
            }
            if let Some(c) = near[key(-1, dy, dz)] {
                out[row - 1] = c[local(n - 1)];
            }
            if let Some(c) = near[key(1, dy, dz)] {
                out[row + n as usize] = c[local(0)];
            }
        }
    }
    out
}

/// The worldgen workers: generate a Generated chunk's voxels.
pub trait ChunkSource {
    /// Starts generating `coord`; the shell answers with [`ChunkStreamer::on_generated`] and `token`.
    fn generate(&mut self, coord: Coord, token: u64);
    /// Drops a queued job; true if it was still queued.
    fn cancel(&mut self, coord: Coord) -> bool;
}

/// A mesh job: the padded voxels of `coord`, mirrored for face B's chunks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeshJob {
    pub coord: Coord,
    pub token: u64,
    pub padded: Vec<u16>,
    pub mirror: bool,
}

/// The meshing workers.
pub trait Mesher {
    /// What a mesh job makes (handed to the [`TerrainView`]).
    type Meshes;
    /// Jobs in flight at once.
    fn capacity(&self) -> usize {
        DEFAULT_MESH_JOBS
    }
    /// Starts a job; the shell answers with [`ChunkStreamer::on_meshed`] and the job's coord and token.
    fn mesh(&mut self, job: MeshJob);
}

/// What the streamer needs from the renderer.
pub trait TerrainView<M> {
    /// Shows a chunk's meshes at `origin`, or removes the chunk (`None`).
    fn set_terrain_chunk(&mut self, coord: Coord, origin: [i32; 3], meshes: Option<M>);
}

/// A ChunkData message's parts the streamer reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkMessage {
    pub form: ChunkForm,
    pub coord: Coord,
    pub revision: u32,
    /// Explicit chunks' voxels.
    pub voxels: Option<Vec<u16>>,
}

impl From<ChunkData> for ChunkMessage {
    fn from(m: ChunkData) -> Self {
        let voxels = (m.form == ChunkForm::Explicit).then_some(m.voxels);
        Self {
            form: m.form,
            coord: m.coord,
            revision: m.revision,
            voxels,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StreamStats {
    pub loaded: usize,
    pub generating: usize,
    pub meshed: usize,
    pub dirty: usize,
    /// Chunks with a mesh job in a worker.
    pub meshing: usize,
    /// Chunks re-requested after a revision gap (§6.3).
    pub resyncs: u32,
}

/// The 26 chunks around a chunk.
const AROUND: [Coord; 26] = {
    let mut out = [[0; 3]; 26];
    let mut i = 0;
    let mut k = 0;
    while k < 27 {
        let d = [k % 3 - 1, (k / 3) % 3 - 1, k / 9 - 1];
        if d[0] != 0 || d[1] != 0 || d[2] != 0 {
            out[i] = d;
            i += 1;
        }
        k += 1;
    }
    out
};

const NEIGHBOURS: [Coord; 6] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
];

/// Mesh jobs in flight at once when the mesher does not say.
pub const DEFAULT_MESH_JOBS: usize = 8;

fn add(a: Coord, b: Coord) -> Coord {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// Chunks waiting to be meshed, in the order they were first marked (a JS `Map`'s order: marking
/// again keeps the place), so ties in distance go as they did in the TS.
#[derive(Clone, Debug, Default)]
struct DirtySet {
    order: HashMap<Coord, u64>,
    next: u64,
}

impl DirtySet {
    fn mark(&mut self, c: Coord) {
        if !self.order.contains_key(&c) {
            self.order.insert(c, self.next);
            self.next += 1;
        }
    }
    fn remove(&mut self, c: &Coord) {
        self.order.remove(c);
    }
    fn len(&self) -> usize {
        self.order.len()
    }
    fn in_order(&self) -> Vec<Coord> {
        let mut v: Vec<_> = self.order.iter().map(|(&c, &i)| (i, c)).collect();
        v.sort_unstable_by_key(|&(i, _)| i);
        v.into_iter().map(|(_, c)| c).collect()
    }
}

type ResyncFn = Box<dyn FnMut(&[Coord])>;

/// The client's chunk store and streaming.
pub struct ChunkStreamer<S, G, M: Mesher, V> {
    store: S,
    source: G,
    mesher: M,
    view: V,
    /// Asks the server to send chunks again (ChunkResync).
    resync: ResyncFn,
    /// Loaded chunks and their revisions (Air chunks: revision 0).
    loaded: HashMap<Coord, u32>,
    /// Loaded chunks the server sent as Air: known empty, never generated or stored.
    air: HashSet<Coord>,
    /// Generated chunks announced by the server and not yet generated → (request token, revision).
    generating: HashMap<Coord, (u64, u32)>,
    /// Modifications of chunks still generating, applied once they arrive.
    held_edits: HashMap<Coord, Vec<ChunkChanges>>,
    dirty: DirtySet,
    meshed: HashSet<Coord>,
    /// Mesh jobs in flight → token (a newer job or an unload supersedes).
    meshing: HashMap<Coord, u64>,
    /// Chunks asked for again after a revision gap, until they arrive.
    resyncing: HashSet<Coord>,
    resyncs: u32,
    token: u64,
    /// First worldgen failure (the workers are unusable).
    pub error: Option<String>,
}

impl<S, G, M, V> ChunkStreamer<S, G, M, V>
where
    S: ChunkStore,
    G: ChunkSource,
    M: Mesher,
    V: TerrainView<M::Meshes>,
{
    pub fn new(store: S, source: G, mesher: M, view: V) -> Self {
        Self::with_resync(store, source, mesher, view, |_| {})
    }

    /// `resync` asks the server to send chunks again (ChunkResync).
    pub fn with_resync(
        store: S,
        source: G,
        mesher: M,
        view: V,
        resync: impl FnMut(&[Coord]) + 'static,
    ) -> Self {
        Self {
            store,
            source,
            mesher,
            view,
            resync: Box::new(resync),
            loaded: HashMap::default(),
            air: HashSet::default(),
            generating: HashMap::default(),
            held_edits: HashMap::default(),
            dirty: DirtySet::default(),
            meshed: HashSet::default(),
            meshing: HashMap::default(),
            resyncing: HashSet::default(),
            resyncs: 0,
            token: 0,
            error: None,
        }
    }

    pub fn store(&self) -> &S {
        &self.store
    }
    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }
    pub fn source(&self) -> &G {
        &self.source
    }
    pub fn source_mut(&mut self) -> &mut G {
        &mut self.source
    }
    pub fn mesher(&self) -> &M {
        &self.mesher
    }
    pub fn mesher_mut(&mut self) -> &mut M {
        &mut self.mesher
    }
    pub fn view(&self) -> &V {
        &self.view
    }
    pub fn view_mut(&mut self) -> &mut V {
        &mut self.view
    }

    pub fn on_chunk_data(&mut self, m: ChunkMessage) {
        let key = m.coord;
        self.forget(key);
        self.resyncing.remove(&key);
        match m.form {
            ChunkForm::Air => self.put_air(key),
            ChunkForm::Explicit => {
                if let Some(voxels) = &m.voxels {
                    self.put(key, m.revision, voxels);
                }
            }
            ChunkForm::Generated => {
                self.token += 1;
                self.generating.insert(key, (self.token, m.revision));
                self.source.generate(key, self.token);
            }
        }
    }

    /// A generation job finished.
    pub fn on_generated(&mut self, coord: Coord, token: u64, voxels: &[u16]) {
        let Some(&(current, revision)) = self.generating.get(&coord) else {
            return; // unloaded meanwhile
        };
        if current != token {
            return; // replaced meanwhile
        }
        self.generating.remove(&coord);
        self.put(coord, revision, voxels);
        for changes in self.held_edits.remove(&coord).unwrap_or_default() {
            self.apply(coord, &changes);
        }
    }

    /// A generation job failed: the workers are unusable. As in the TS, this drops the chunk's
    /// pending generation whatever its token.
    pub fn on_generate_failed(&mut self, coord: Coord, error: impl Into<String>) {
        self.generating.remove(&coord);
        self.held_edits.remove(&coord);
        if self.error.is_none() {
            self.error = Some(error.into());
        }
    }

    pub fn on_chunk_unload(&mut self, coords: &[Coord]) {
        for &key in coords {
            self.forget(key);
            self.resyncing.remove(&key);
            if self.loaded.remove(&key).is_some() && !self.air.remove(&key) {
                self.store.remove_chunk(key);
            }
            self.dirty.remove(&key);
            self.meshing.remove(&key);
            if self.meshed.remove(&key) {
                self.view.set_terrain_chunk(key, [0, 0, 0], None);
            }
        }
    }

    /// A VoxelModification's chunks (§6.5): applied in revision order.
    pub fn on_voxel_modification(&mut self, chunks: &[ChunkChanges]) {
        for changes in chunks {
            let key = changes.coord;
            if self.generating.contains_key(&key) {
                self.held_edits
                    .entry(key)
                    .or_default()
                    .push(changes.clone());
            } else if self.loaded.contains_key(&key) {
                self.apply(key, changes);
            }
            // Otherwise the chunk is not ours (unloaded meanwhile): the server streams it anew.
        }
    }

    pub fn is_loaded(&self, coord: Coord) -> bool {
        self.loaded.contains_key(&coord)
    }

    /// Loaded and drawable (meshed, or all air): it can stand in for level 0 of the LOD (§6.6).
    pub fn drawable(&self, coord: Coord) -> bool {
        self.loaded.contains_key(&coord)
            && (self.air.contains(&coord) || self.meshed.contains(&coord))
    }

    /// Revision of a loaded chunk (Air: 0), or None.
    pub fn revision(&self, coord: Coord) -> Option<u32> {
        self.loaded.get(&coord).copied()
    }

    /// True when the chunks around `position` (±1 chunk, not wholly outside the build volume) are
    /// loaded.
    pub fn ready_around(&self, position: [f64; 3]) -> bool {
        let c = dwell_world::chunk_of_pos(position);
        let [cx, cy, cz] = [c.x, c.y, c.z];
        for y in (cy - 1).max(MIN_CHUNK_Y)..=(cy + 1).min(MAX_CHUNK_Y) {
            for z in cz - 1..=cz + 1 {
                for x in cx - 1..=cx + 1 {
                    if !self.loaded.contains_key(&[x, y, z])
                        && BUILD_VOLUME.chunk_may_hold(ChunkCoord::new(x, y, z))
                    {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Starts mesh jobs for changed chunks, nearest to `center` first, while the mesher has room and
    /// for up to `budget_ms` of `now()` (at least one). A chunk waits while a neighbour is still
    /// being generated, so it is meshed once with its borders known rather than again when each
    /// neighbour arrives. Returns the number of jobs started.
    pub fn mesh_dirty(
        &mut self,
        center: [f64; 3],
        budget_ms: f64,
        mut now: impl FnMut() -> f64,
    ) -> usize {
        let room = self.mesher.capacity().saturating_sub(self.meshing.len());
        if self.dirty.len() == 0 || room == 0 {
            return 0;
        }
        let deadline = now() + budget_ms;
        let distance = |c: &Coord| -> f64 {
            let m = dwell_world::chunk_centre_m(ChunkCoord::from_array(*c));
            (0..3).map(|i| (m[i] - center[i]).powi(2)).sum()
        };
        let mut ready: Vec<Coord> = self
            .dirty
            .in_order()
            .into_iter()
            .filter(|c| {
                !self.meshing.contains_key(c)
                    && !NEIGHBOURS
                        .iter()
                        .any(|d| self.generating.contains_key(&add(*c, *d)))
            })
            .collect();
        // Stable: equal distances keep their order.
        ready.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
        let mut count = 0;
        for c in ready {
            if count >= room {
                break;
            }
            self.dirty.remove(&c);
            self.token += 1;
            self.meshing.insert(c, self.token);
            let padded = self.store.padded_chunk(c);
            self.mesher.mesh(MeshJob {
                coord: c,
                token: self.token,
                padded,
                mirror: face_of_chunk_y(c[1]) == Face::B,
            });
            count += 1;
            if now() >= deadline {
                break;
            }
        }
        count
    }

    /// A mesh job finished.
    pub fn on_meshed(&mut self, coord: Coord, token: u64, meshes: M::Meshes) {
        if self.meshing.get(&coord) != Some(&token) {
            return; // unloaded or superseded meanwhile
        }
        self.meshing.remove(&coord);
        if !self.loaded.contains_key(&coord) || self.air.contains(&coord) {
            return;
        }
        self.view
            .set_terrain_chunk(coord, coord.map(|c| c * CHUNK_SIZE), Some(meshes));
        self.meshed.insert(coord);
    }

    pub fn stats(&self) -> StreamStats {
        StreamStats {
            loaded: self.loaded.len(),
            generating: self.generating.len(),
            meshed: self.meshed.len(),
            dirty: self.dirty.len(),
            meshing: self.meshing.len(),
            resyncs: self.resyncs,
        }
    }

    fn put(&mut self, key: Coord, revision: u32, voxels: &[u16]) {
        self.store.set_chunk(key, revision, voxels);
        self.air.remove(&key);
        self.loaded.insert(key, revision);
        self.dirty.mark(key);
        self.neighbours_changed(key);
    }

    /// An all-air chunk: the client sim reads a missing chunk as air, so nothing is stored.
    fn put_air(&mut self, key: Coord) {
        let replaced = self.loaded.contains_key(&key) && !self.air.contains(&key);
        if replaced {
            self.store.remove_chunk(key);
        }
        self.loaded.insert(key, 0);
        self.air.insert(key);
        self.dirty.remove(&key);
        self.meshing.remove(&key);
        if self.meshed.remove(&key) {
            self.view.set_terrain_chunk(key, [0, 0, 0], None);
        }
        if replaced {
            self.neighbours_changed(key);
        }
    }

    /// Writes cells on the client alone (edit prediction, §6.5): in loaded chunks only (an Air
    /// chunk is stored from then on), each chunk keeping its revision, remeshed like a
    /// modification. Cells of chunks not loaded, or still generating, are left. The cells written.
    pub fn set_local(&mut self, cells: &[([i32; 3], u16)]) -> usize {
        let mut by_chunk: Vec<ChunkChanges> = Vec::new();
        for &(c, m) in cells {
            let key = c.map(|v| v.div_euclid(CHUNK_SIZE));
            let Some(&revision) = self.loaded.get(&key) else {
                continue;
            };
            if self.generating.contains_key(&key) {
                continue;
            }
            let l = c.map(|v| v.rem_euclid(CHUNK_SIZE));
            let change = dwell_protocol::VoxelChange {
                index: (l[0] | l[1] << 5 | l[2] << 10) as u16,
                material: m,
            };
            match by_chunk.iter_mut().find(|ch| ch.coord == key) {
                Some(ch) => ch.changes.push(change),
                None => by_chunk.push(ChunkChanges {
                    coord: key,
                    revision,
                    changes: vec![change],
                }),
            }
        }
        let mut n = 0;
        for changes in by_chunk {
            n += changes.changes.len();
            self.write(changes.coord, &changes);
        }
        n
    }

    /// Applies one chunk's changes if they are the next revision; a gap asks for a resync.
    fn apply(&mut self, key: Coord, changes: &ChunkChanges) {
        let Some(&revision) = self.loaded.get(&key) else {
            return;
        };
        if self.resyncing.contains(&key) {
            return; // a resync brings the current state
        }
        if changes.revision <= revision {
            return; // stale
        }
        if changes.revision != revision + 1 {
            self.resyncing.insert(key);
            self.resyncs += 1;
            (self.resync)(&[changes.coord]);
            return;
        }
        self.write(key, changes);
    }

    /// Writes a loaded chunk's changes (it takes their revision) and marks it and the neighbours
    /// they touch for meshing.
    fn write(&mut self, key: Coord, changes: &ChunkChanges) {
        if self.air.remove(&key) {
            self.store.set_chunk(changes.coord, 0, &[0; CHUNK_VOLUME]);
        }
        self.store.edit_chunk(changes);
        self.loaded.insert(key, changes.revision);
        self.dirty.mark(changes.coord);
        // Neighbours a changed voxel is next to, diagonals included: their border faces and their
        // adaptive blocks' shapes (ADAPTIVE_BLOCKS.md: a shape reads the 8 columns around it and
        // the layer above) may change. In the order first touched.
        let mut touched: Vec<Coord> = Vec::with_capacity(7);
        for v in &changes.changes {
            let index = i32::from(v.index);
            let local = [index & 31, (index >> 5) & 31, (index >> 10) & 31];
            let side = |l: i32, d: i32| {
                if l + d < 0 {
                    -1
                } else if l + d >= CHUNK_SIZE {
                    1
                } else {
                    0
                }
            };
            for dz in -1..=1 {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let n = [side(local[0], dx), side(local[1], dy), side(local[2], dz)];
                        if n != [0, 0, 0] && !touched.contains(&n) {
                            touched.push(n);
                        }
                    }
                }
            }
        }
        for n in touched {
            let c = add(changes.coord, n);
            if self.loaded.contains_key(&c) && !self.air.contains(&c) {
                self.dirty.mark(c);
            }
        }
    }

    /// Neighbours' border faces may now be hidden (or exposed) by this chunk, and their adaptive
    /// blocks' shapes changed (diagonal neighbours too).
    fn neighbours_changed(&mut self, coord: Coord) {
        for d in AROUND {
            let n = add(coord, d);
            if self.loaded.contains_key(&n) && !self.air.contains(&n) {
                self.dirty.mark(n);
            }
        }
    }

    /// Drops a pending generation of the chunk (the new message or an unload supersedes it).
    fn forget(&mut self, key: Coord) {
        self.held_edits.remove(&key);
        if self.generating.remove(&key).is_some() {
            self.source.cancel(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk_diff::PAD;

    #[test]
    fn padding_from_neighbours_equals_reading_each_voxel() {
        // A generated world (flat terrain: ground at the chunk border) with edits scattered over
        // the chunk and all its neighbours, and a streamed one with chunks missing (air).
        let mut flat = VoxelWorld::flat();
        let mut seed = 0x9e37_79b9_u32;
        let mut rand = move |n: i32| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed % n as u32) as i32
        };
        for _ in 0..4000 {
            let (x, y, z) = (rand(96) - 48, rand(96) - 48, rand(96) - 48);
            flat.set_voxel(x, y, z, (1 + rand(5)) as u16);
        }
        let mut streamed = VoxelWorld::streamed();
        let mut chunk = Chunk::default();
        for i in 0..CHUNK_VOLUME {
            chunk.set_at(i, (i % 7) as u16);
        }
        streamed.put(ChunkCoord::new(0, 0, 0), chunk.clone());
        streamed.put(ChunkCoord::new(1, 0, 0), chunk.clone());
        streamed.put(ChunkCoord::new(0, -1, 1), chunk);
        check_padding(&mut flat, [0, 0, 0]);
        check_padding(&mut flat, [-1, -1, 0]);
        check_padding(&mut streamed, [0, 0, 0]);
    }

    fn check_padding(world: &mut VoxelWorld, coord: Coord) {
        let fast = padded_from_neighbours(world, coord);
        let origin = coord.map(|c| c * CHUNK_SIZE);
        let p = PAD as i32;
        for y in -1..p - 1 {
            for z in -1..p - 1 {
                for x in -1..p - 1 {
                    assert_eq!(
                        fast[padded_index(x, y, z)],
                        world.get_voxel(origin[0] + x, origin[1] + y, origin[2] + z),
                        "{coord:?} at {x},{y},{z}"
                    );
                }
            }
        }
    }
    use dwell_protocol::VoxelChange;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Chunk source whose jobs the test completes by hand.
    #[derive(Default)]
    struct ManualSource {
        jobs: HashMap<Coord, u64>,
        cancelled: Vec<Coord>,
    }
    impl ManualSource {
        fn pending(&self) -> usize {
            self.jobs.len()
        }
    }
    impl ChunkSource for ManualSource {
        fn generate(&mut self, coord: Coord, token: u64) {
            self.jobs.insert(coord, token);
        }
        fn cancel(&mut self, coord: Coord) -> bool {
            self.cancelled.push(coord);
            self.jobs.remove(&coord).is_some()
        }
    }

    #[derive(Default)]
    struct FakeStore {
        /// coord → (revision, first voxel)
        chunks: HashMap<Coord, (u32, u16)>,
        edits: Vec<(Coord, u32, Vec<u16>)>,
        meshed_from: Vec<Coord>,
    }
    impl ChunkStore for FakeStore {
        fn set_chunk(&mut self, coord: Coord, revision: u32, voxels: &[u16]) {
            self.chunks.insert(coord, (revision, voxels[0]));
        }
        fn remove_chunk(&mut self, coord: Coord) {
            self.chunks.remove(&coord);
        }
        fn edit_chunk(&mut self, c: &ChunkChanges) {
            let pairs = c
                .changes
                .iter()
                .flat_map(|v| [v.index, v.material])
                .collect();
            self.edits.push((c.coord, c.revision, pairs));
            if let Some(chunk) = self.chunks.get_mut(&c.coord) {
                chunk.0 = c.revision;
            }
        }
        fn padded_chunk(&mut self, coord: Coord) -> Vec<u16> {
            self.meshed_from.push(coord);
            vec![0; PADDED_VOLUME]
        }
    }

    /// Mesher whose jobs finish on [`flush`] (as the TS InlineMesher's promises did).
    #[derive(Default)]
    struct FakeMesher {
        capacity: Option<usize>,
        jobs: Vec<MeshJob>,
        received: Vec<bool>,
    }
    impl Mesher for FakeMesher {
        type Meshes = ();
        fn capacity(&self) -> usize {
            self.capacity.unwrap_or(DEFAULT_MESH_JOBS)
        }
        fn mesh(&mut self, job: MeshJob) {
            self.received.push(job.mirror);
            self.jobs.push(job);
        }
    }

    #[derive(Default)]
    struct FakeView {
        chunks: HashMap<Coord, [i32; 3]>,
    }
    impl TerrainView<()> for FakeView {
        fn set_terrain_chunk(&mut self, coord: Coord, origin: [i32; 3], meshes: Option<()>) {
            match meshes {
                Some(()) => self.chunks.insert(coord, origin),
                None => self.chunks.remove(&coord),
            };
        }
    }

    type Streamer = ChunkStreamer<FakeStore, ManualSource, FakeMesher, FakeView>;

    /// Completes the job for `coord` with voxels filled with the chunk's x coordinate + 1.
    fn finish(s: &mut Streamer, coord: Coord) {
        let token = s
            .source_mut()
            .jobs
            .remove(&coord)
            .unwrap_or_else(|| panic!("no job for {coord:?}"));
        s.on_generated(coord, token, &vec![(coord[0] + 1) as u16; CHUNK_VOLUME]);
    }

    /// Finishes every mesh job in flight.
    fn flush(s: &mut Streamer) {
        for job in std::mem::take(&mut s.mesher_mut().jobs) {
            s.on_meshed(job.coord, job.token, ());
        }
    }

    fn setup() -> (Streamer, Rc<RefCell<Vec<Coord>>>) {
        let resyncs = Rc::new(RefCell::new(Vec::new()));
        let sink = resyncs.clone();
        let streamer = ChunkStreamer::with_resync(
            FakeStore::default(),
            ManualSource::default(),
            FakeMesher::default(),
            FakeView::default(),
            move |coords| sink.borrow_mut().extend_from_slice(coords),
        );
        (streamer, resyncs)
    }

    fn no_clock() -> f64 {
        0.0
    }

    fn modification(coord: Coord, revision: u32, pairs: &[u16]) -> ChunkChanges {
        ChunkChanges {
            coord,
            revision,
            changes: pairs
                .chunks(2)
                .map(|p| VoxelChange {
                    index: p[0],
                    material: p[1],
                })
                .collect(),
        }
    }

    fn generated(coord: Coord) -> ChunkMessage {
        ChunkMessage {
            form: ChunkForm::Generated,
            coord,
            revision: 0,
            voxels: None,
        }
    }

    fn air(coord: Coord) -> ChunkMessage {
        ChunkMessage {
            form: ChunkForm::Air,
            coord,
            revision: 0,
            voxels: None,
        }
    }

    fn explicit(coord: Coord, revision: u32, fill: u16) -> ChunkMessage {
        ChunkMessage {
            form: ChunkForm::Explicit,
            coord,
            revision,
            voxels: Some(vec![fill; CHUNK_VOLUME]),
        }
    }

    fn sorted(mut v: Vec<Coord>) -> Vec<Coord> {
        v.sort();
        v
    }

    /// The region tools and the structures store read a box through the streamer: an Air chunk the
    /// server sent is loaded (all air), though the client never stores it; a chunk not sent is not.
    #[test]
    fn a_box_over_an_air_chunk_reads_as_loaded_air() {
        use crate::region_tools::BoxSource;
        let mut s = ChunkStreamer::new(
            VoxelWorld::streamed(),
            ManualSource::default(),
            FakeMesher::default(),
            FakeView::default(),
        );
        s.on_chunk_data(explicit([0, -1, 0], 1, 7));
        s.on_chunk_data(air([0, 0, 0]));
        // A 1 × 2 × 1 box across the border: the top of the stored chunk and the bottom of the Air one.
        assert_eq!(s.read_box([3, -1, 3], [1, 2, 1]), Some(vec![7, 0]));
        // Into a chunk the server hasn't sent: not loaded.
        assert_eq!(s.read_box([3, 31, 3], [1, 2, 1]), None);
    }

    #[test]
    fn treats_air_chunks_as_loaded_without_generating_storing_or_meshing_them() {
        let (mut s, _) = setup();
        for y in -1..=1 {
            for z in -1..=1 {
                for x in -1..=1 {
                    s.on_chunk_data(air([x, y, z]));
                }
            }
        }
        assert_eq!(s.source().pending(), 0);
        assert_eq!(s.store().chunks.len(), 0);
        assert!(s.ready_around([16.0, dwell_world::m_of_row(16), 16.0]));
        assert_eq!(s.mesh_dirty([0.0; 3], 100.0, no_clock), 0);
        assert_eq!(s.view().chunks.len(), 0);
        // Unloading an Air chunk removes nothing from the store; a stored chunk replaced by Air is
        // dropped from the store and the view.
        s.on_chunk_data(explicit([5, 0, 0], 1, 2));
        s.mesh_dirty([0.0; 3], 100.0, no_clock);
        flush(&mut s);
        assert!(s.view().chunks.contains_key(&[5, 0, 0]));
        s.on_chunk_data(air([5, 0, 0]));
        assert!(!s.store().chunks.contains_key(&[5, 0, 0]));
        assert!(!s.view().chunks.contains_key(&[5, 0, 0]));
        s.on_chunk_unload(&[[0, 0, 0]]);
        assert!(!s.is_loaded([0, 0, 0]));
        assert!(!s.ready_around([16.0, 16.0, 16.0]));
    }

    #[test]
    fn stores_explicit_chunks_directly_and_generated_ones_once_generated() {
        let (mut s, _) = setup();
        s.on_chunk_data(explicit([5, 0, 0], 3, 9));
        assert_eq!(s.store().chunks.get(&[5, 0, 0]), Some(&(3, 9)));

        s.on_chunk_data(generated([1, 0, 0]));
        assert!(!s.store().chunks.contains_key(&[1, 0, 0]));
        assert_eq!(s.stats().generating, 1);
        finish(&mut s, [1, 0, 0]);
        assert_eq!(s.store().chunks.get(&[1, 0, 0]), Some(&(0, 2)));
        assert!(s.is_loaded([1, 0, 0]));
    }

    #[test]
    fn unloads_chunks_from_the_sim_from_the_renderer_and_cancels_pending_generation() {
        let (mut s, _) = setup();
        s.on_chunk_data(generated([0, 0, 0]));
        finish(&mut s, [0, 0, 0]);
        s.mesh_dirty([0.0; 3], 10.0, no_clock);
        flush(&mut s);
        assert!(s.view().chunks.contains_key(&[0, 0, 0]));
        s.on_chunk_data(generated([3, 0, 0]));

        s.on_chunk_unload(&[[0, 0, 0], [3, 0, 0]]);
        assert_eq!(s.store().chunks.len(), 0);
        assert_eq!(s.view().chunks.len(), 0);
        assert!(s.source().cancelled.contains(&[3, 0, 0]));
        assert_eq!(s.stats(), StreamStats::default());
    }

    #[test]
    fn ignores_a_generation_that_finishes_after_its_chunk_was_unloaded() {
        let (mut s, _) = setup();
        s.on_chunk_data(generated([2, 0, 0]));
        let token = s.source().jobs[&[2, 0, 0]];
        s.on_chunk_unload(&[[2, 0, 0]]);
        s.on_generated([2, 0, 0], token, &[0; CHUNK_VOLUME]);
        assert_eq!(s.store().chunks.len(), 0);
        // Nor one superseded by a newer message for the chunk.
        s.on_chunk_data(generated([2, 0, 0]));
        s.on_chunk_data(generated([2, 0, 0]));
        s.on_generated([2, 0, 0], token + 1, &[0; CHUNK_VOLUME]);
        assert_eq!(s.store().chunks.len(), 0);
    }

    #[test]
    fn meshes_nearest_first_within_the_budget_after_neighbours_being_generated_arrive() {
        let (mut s, _) = setup();
        for c in [[0, 0, 0], [4, 0, 0], [8, 0, 0]] {
            s.on_chunk_data(generated(c));
            finish(&mut s, c);
        }
        s.on_chunk_data(generated([5, 0, 0])); // a neighbour of [4, 0, 0], still generating
        // A clock that runs out after every chunk: one chunk per call.
        let mut t = 0.0;
        let center = [8.0 * 32.0 + 16.0, 16.0, 16.0];
        let mut clock = || {
            t += 10.0;
            t
        };
        assert_eq!(s.mesh_dirty(center, 5.0, &mut clock), 1);
        assert_eq!(s.store().meshed_from, vec![[8, 0, 0]]);
        assert_eq!(s.mesh_dirty(center, 5.0, &mut clock), 1);
        // [4, 0, 0] waits for [5, 0, 0].
        assert_eq!(s.store().meshed_from, vec![[8, 0, 0], [0, 0, 0]]);
        assert_eq!(s.mesh_dirty(center, 5.0, &mut clock), 0);
        finish(&mut s, [5, 0, 0]);
        flush(&mut s);
        s.mesh_dirty(center, 100.0, no_clock);
        assert_eq!(
            sorted(s.store().meshed_from[2..].to_vec()),
            vec![[4, 0, 0], [5, 0, 0]]
        );
    }

    #[test]
    fn re_meshes_loaded_neighbours_when_a_chunk_arrives() {
        let (mut s, _) = setup();
        s.on_chunk_data(generated([0, 0, 0]));
        finish(&mut s, [0, 0, 0]);
        s.mesh_dirty([0.0; 3], 100.0, no_clock);
        s.on_chunk_data(generated([0, 1, 0]));
        finish(&mut s, [0, 1, 0]);
        flush(&mut s);
        s.mesh_dirty([0.0; 3], 100.0, no_clock);
        assert_eq!(
            sorted(s.store().meshed_from.clone()),
            vec![[0, 0, 0], [0, 0, 0], [0, 1, 0]]
        );
    }

    #[test]
    fn an_edit_beside_a_plants_support_across_a_chunk_border_re_meshes_the_plants_chunk() {
        // A plant at the bottom row of chunk (0, 1, 0) sits on its support at the top row of
        // (0, 0, 0) (TRANSPARENT_BLOCKS.md §4); a block put beside the support (in the lower
        // chunk, one cell over) reshapes it, so the plant's chunk re-meshes to follow it.
        let (mut s, _) = setup();
        s.on_chunk_data(explicit([0, 0, 0], 3, 0));
        s.on_chunk_data(explicit([0, 1, 0], 0, 0));
        s.mesh_dirty([0.0; 3], 100.0, no_clock);
        flush(&mut s);
        s.store_mut().meshed_from.clear();
        let beside_support = 6 | (31 << 5) | (5 << 10);
        s.on_voxel_modification(&[modification([0, 0, 0], 4, &[beside_support, 2])]);
        s.mesh_dirty([0.0; 3], 100.0, no_clock);
        assert_eq!(
            sorted(s.store().meshed_from.clone()),
            vec![[0, 0, 0], [0, 1, 0]]
        );
    }

    #[test]
    fn keeps_at_most_the_meshers_capacity_of_jobs_in_flight() {
        let mesher = FakeMesher {
            capacity: Some(2),
            ..FakeMesher::default()
        };
        let mut s = ChunkStreamer::new(
            FakeStore::default(),
            ManualSource::default(),
            mesher,
            FakeView::default(),
        );
        for x in [0, 2, 4] {
            s.on_chunk_data(explicit([x, 0, 0], 0, 0));
        }
        assert_eq!(s.mesh_dirty([0.0; 3], 100.0, no_clock), 2);
        assert_eq!(s.mesh_dirty([0.0; 3], 100.0, no_clock), 0);
        flush(&mut s);
        assert_eq!(s.view().chunks.len(), 2);
        assert_eq!(s.mesh_dirty([0.0; 3], 100.0, no_clock), 1);
    }

    #[test]
    fn applies_voxel_modifications_in_revision_order_and_re_meshes_the_chunk_and_its_neighbours() {
        let (mut s, resyncs) = setup();
        s.on_chunk_data(explicit([0, 0, 0], 3, 0));
        s.on_chunk_data(explicit([1, 0, 0], 0, 0));
        s.on_chunk_data(explicit([0, 1, 0], 0, 0));
        s.mesh_dirty([0.0; 3], 100.0, no_clock);
        flush(&mut s);
        s.store_mut().meshed_from.clear();
        // A voxel on the +X border (x = 31): the chunk and its +X neighbour re-mesh.
        let border = 31 | (5 << 5);
        s.on_voxel_modification(&[modification([0, 0, 0], 4, &[border, 2])]);
        assert_eq!(s.store().edits, vec![([0, 0, 0], 4, vec![border, 2])]);
        assert_eq!(s.revision([0, 0, 0]), Some(4));
        s.mesh_dirty([0.0; 3], 100.0, no_clock);
        assert_eq!(
            sorted(s.store().meshed_from.clone()),
            vec![[0, 0, 0], [1, 0, 0]]
        );
        // Stale revisions are ignored; unknown chunks too.
        s.on_voxel_modification(&[modification([0, 0, 0], 4, &[0, 3])]);
        s.on_voxel_modification(&[modification([9, 9, 9], 1, &[0, 3])]);
        assert_eq!(s.store().edits.len(), 1);
        // A gap asks for a resync (once), and nothing is applied until the chunk comes again.
        s.on_voxel_modification(&[modification([0, 0, 0], 6, &[0, 3])]);
        s.on_voxel_modification(&[modification([0, 0, 0], 7, &[0, 3])]);
        assert_eq!(*resyncs.borrow(), vec![[0, 0, 0]]);
        assert_eq!(s.store().edits.len(), 1);
        assert_eq!(s.stats().resyncs, 1);
        s.on_chunk_data(explicit([0, 0, 0], 7, 0));
        s.on_voxel_modification(&[modification([0, 0, 0], 8, &[0, 3])]);
        assert_eq!(s.store().edits.len(), 2);
        assert_eq!(s.revision([0, 0, 0]), Some(8));
    }

    #[test]
    fn holds_modifications_of_a_chunk_still_generating_and_edits_air_chunks_into_stored_ones() {
        let (mut s, resyncs) = setup();
        s.on_chunk_data(generated([2, 0, 0]));
        s.on_voxel_modification(&[modification([2, 0, 0], 1, &[7, 2])]);
        s.on_voxel_modification(&[modification([2, 0, 0], 2, &[8, 2])]);
        assert!(s.store().edits.is_empty());
        finish(&mut s, [2, 0, 0]);
        let revisions: Vec<_> = s.store().edits.iter().map(|e| e.1).collect();
        assert_eq!(revisions, vec![1, 2]);
        assert!(resyncs.borrow().is_empty());

        s.on_chunk_data(air([0, 5, 0]));
        s.on_voxel_modification(&[modification([0, 5, 0], 1, &[0, 2])]);
        assert_eq!(s.store().chunks.get(&[0, 5, 0]), Some(&(1, 0)));
        s.mesh_dirty([0.0, 160.0, 0.0], 100.0, no_clock);
        assert!(s.store().meshed_from.contains(&[0, 5, 0]));
    }

    #[test]
    fn is_ready_once_the_chunks_around_the_player_are_loaded() {
        let (mut s, _) = setup();
        let at = [10.0, dwell_world::m_of_row(70), -5.0]; // chunk (0, 2, -1)
        assert!(!s.ready_around(at));
        for y in 1..=3 {
            for z in -2..=0 {
                for x in -1..=1 {
                    s.on_chunk_data(explicit([x, y, z], 0, 0));
                }
            }
        }
        assert!(s.ready_around(at));
    }

    #[test]
    fn only_needs_rows_inside_the_world_at_its_bottom() {
        let (mut s, _) = setup();
        let at = [
            0.0,
            dwell_world::m_of_row(MIN_CHUNK_Y * CHUNK_SIZE + 8),
            0.0,
        ]; // the lowest chunk row
        for y in MIN_CHUNK_Y..=MIN_CHUNK_Y + 1 {
            for z in -1..=1 {
                for x in -1..=1 {
                    s.on_chunk_data(explicit([x, y, z], 0, 0));
                }
            }
        }
        assert!(s.ready_around(at));
    }

    #[test]
    fn meshes_face_bs_chunks_as_mirror_images_face_as_as_they_are() {
        let mut s = ChunkStreamer::new(
            FakeStore::default(),
            ManualSource::default(),
            FakeMesher::default(),
            FakeView::default(),
        );
        s.on_chunk_data(explicit([3, 0, 4], 1, 2));
        assert_eq!(s.mesh_dirty([0.0; 3], 100.0, no_clock), 1);
        // A chunk row below the midplane (face B).
        s.on_chunk_data(explicit([3, dwell_world::MIDPLANE_CHUNK_Y - 6, 4], 1, 2));
        assert_eq!(s.mesh_dirty([0.0; 3], 100.0, no_clock), 1);
        assert_eq!(s.mesher().received, vec![false, true]);
    }

    // Beyond the TS suite: the client's voxel world as the store, and a worldgen failure.
    #[test]
    fn the_voxel_world_stores_edits_and_pads_chunks() {
        let stone = dwell_blocks::require("dwell:stone");
        let mut world = VoxelWorld::streamed();
        world.set_chunk([0, 0, 0], 2, &[stone; CHUNK_VOLUME]);
        world.edit_chunk(&modification([0, 0, 0], 3, &[0, 0]));
        assert_eq!(
            world.find(ChunkCoord::new(0, 0, 0)).map(Chunk::revision),
            Some(3)
        );
        let padded = world.padded_chunk([0, 0, 0]);
        assert_eq!(padded[padded_index(0, 0, 0)], 0);
        assert_eq!(padded[padded_index(1, 0, 0)], stone);
        assert_eq!(padded[padded_index(-1, 0, 0)], 0); // apron: a missing chunk reads as air
        world.remove_chunk([0, 0, 0]);
        assert_eq!(world.loaded_chunks(), 0);
    }

    #[test]
    fn a_worldgen_failure_is_kept_and_drops_the_pending_chunk() {
        let (mut s, _) = setup();
        s.on_chunk_data(generated([1, 0, 0]));
        s.on_generate_failed([1, 0, 0], "worker failed");
        s.on_generate_failed([1, 0, 0], "later");
        assert_eq!(s.error.as_deref(), Some("worker failed"));
        assert_eq!(s.stats().generating, 0);
    }
}
