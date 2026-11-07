//! Terrain collision (PLAYER_CONTROLLER.md §5): one static body holding the solid cells of the
//! chunks around players. Cells that collide as their full cube — every solid block but a shaped
//! one — go into one Rapier (Parry) `Voxels` collider: Parry classifies each voxel by its
//! neighbours, so the capsule slides across voxel edges and chunk borders without catching (the
//! internal-edge problem the Jolt build solved with per-chunk meshes in regions). Shaped cells —
//! the generated ramps (HALF_HEIGHT_GRID.md §3) — go into one
//! triangle mesh per chunk (`TriMeshFlags::FIX_INTERNAL_EDGES`) of their faces that no neighbour
//! covers. With f64 positions it is exact anywhere in the world, so it needs no regions or anchors.
//! Chunks are added on demand, rebuilt when the world changes them (a chunk with shaped cells also
//! when any chunk its shapes read changes; same tick), and dropped when far from every player.

use dwell_world::hash::HashMap;

use dwell_blocks::shape::{SURFACE_TAG, face_covered, faces_of};
use dwell_blocks::{ShapeInfo, info};
use dwell_world::{
    CELL, CELL_H, CHUNK_SIZE, CHUNK_VOLUME, ChunkCoord, MAX_CHUNK_Y, MIN_CHUNK_Y, VoxelWorld,
    chunk_of_pos, chunk_origin_m, local_index, up_of_y,
};
use rapier3d_f64::math::IVector;
use rapier3d_f64::parry::shape::{TriMeshFlags, Voxels};
use rapier3d_f64::prelude::*;

use crate::physics::{DVec3, Layer, Physics, groups};

/// What the collider holds of one chunk.
struct Loaded {
    /// The cells that collide as full cubes; `None` when there are none (most chunks above the
    /// ground), so passing through the sky costs no memory per chunk.
    solid: Option<Mask>,
    /// The shaped cells' triangle mesh, if any.
    mesh: Option<ColliderHandle>,
    /// What the mesh was built from (its vertices and triangles), so a rebuild that changes
    /// nothing keeps the collider — and the body's contacts with it.
    mesh_hash: u64,
    /// It has solid cells, so its collision reads the chunks around it (they are watched); an
    /// empty chunk's collision is empty whatever its neighbours, so only it is watched.
    wide: bool,
    /// It has shaped cells, whose faces are culled against the chunks around it: only such a
    /// chunk is rebuilt when a neighbour changes (its full cubes are its own cells alone).
    shaped: bool,
}

type Mask = Box<[u64; CHUNK_VOLUME / 64]>;

/// A chunk some loaded chunk's collision was built from: which stored chunk and collision revision
/// it was (water, and other writes that collide alike, don't count), and how many loaded chunks
/// read it.
struct Watch {
    stamp: u64,
    revision: u32,
    users: u32,
}

pub struct TerrainCollision {
    body: RigidBodyHandle,
    collider: ColliderHandle,
    chunks: HashMap<ChunkCoord, Loaded>,
    /// Every chunk a loaded chunk's collision reads (`deps`: a shape reads up to two cells
    /// across its chunk's border).
    watched: HashMap<ChunkCoord, Watch>,
    /// Chunk builds so far (tests).
    loads: u64,
    /// Full-cube cells set in or taken out of the collider so far (`collisionCells`).
    cells: u64,
    /// The full-cube cells the `Voxels` collider holds, per chunk: a chunk's `solid` is what it
    /// should hold, and Parry takes a cell at a time (~130 ns natively, a whole buried chunk
    /// 3–4 ms; several times that in the browser), so a chunk's cells go in a little a tick
    /// ([`Self::catch_up`]) — the cells near players and pieces at once.
    applied: HashMap<ChunkCoord, Mask>,
    /// The chunks whose `applied` cells differ from their `solid` ones.
    behind: std::collections::BTreeSet<(i32, i32, i32)>,
}

/// The full-cube cells a tick puts into (or takes out of) the collider beyond those near players
/// and pieces ([`TerrainCollision::catch_up`]): a buried chunk's 32 768 cells in four ticks,
/// ~1 ms a tick natively.
pub const COLLISION_CELLS_PER_TICK: usize = 8192;

/// The apron a chunk's collision reads: a cell's shape reads its neighbours one cell out, and its
/// faces are culled against its neighbours' shapes.
const PAD: i32 = 2;
const E: i32 = CHUNK_SIZE + 2 * PAD;

fn padded(x: i32, y: i32, z: i32) -> usize {
    ((x + PAD) + E * ((z + PAD) + E * (y + PAD))) as usize
}

fn is_cube(s: &ShapeInfo) -> bool {
    s.face_count > 0 && !s.fan && s.corners == [2, 2, 2, 2]
}

fn signature(world: &VoxelWorld, c: ChunkCoord) -> (u64, u32) {
    world
        .find(c)
        .map_or((0, 0), |ch| (ch.stamp(), ch.collision_revision()))
}

/// The chunks a loaded chunk's collision reads: the 27 around it, or (empty) itself.
fn deps(c: ChunkCoord, wide: bool) -> Vec<ChunkCoord> {
    if wide { around(c).collect() } else { vec![c] }
}

fn around(c: ChunkCoord) -> impl Iterator<Item = ChunkCoord> {
    (-1..=1).flat_map(move |dy| {
        (-1..=1).flat_map(move |dz| {
            (-1..=1).map(move |dx| ChunkCoord::new(c.x + dx, c.y + dy, c.z + dz))
        })
    })
}

/// A chunk's collision: its full-cube cells and its shaped cells' uncovered faces as triangles
/// (vertices relative to the chunk's minimum corner).
struct Built {
    solid: Option<Mask>,
    /// Any colliding cell is shaped (not a full cube), its faces culled against its neighbours.
    shaped: bool,
    vertices: Vec<DVec3>,
    triangles: Vec<[u32; 3]>,
}

impl Built {
    /// FNV-1a 64 over the vertices' bits and the triangles.
    fn mesh_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |v: u64| {
            for b in v.to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        for v in &self.vertices {
            eat(v.x.to_bits());
            eat(v.y.to_bits());
            eat(v.z.to_bits());
        }
        for t in &self.triangles {
            eat(u64::from(t[0]) | u64::from(t[1]) << 21 | u64::from(t[2]) << 42);
        }
        h
    }
}

/// Whether a cell stops players and bodies: solid, and not passable (leaves, docs/FELLING.md).
fn collides(m: dwell_blocks::StateId) -> bool {
    let s = info(m);
    s.solid && !s.passable
}

fn build(world: &mut VoxelWorld, c: ChunkCoord) -> Built {
    let n = CHUNK_SIZE;
    let mut out = Built {
        solid: None,
        shaped: false,
        vertices: Vec::new(),
        triangles: Vec::new(),
    };
    let up = up_of_y(c.y * n);
    // Most chunks are all air or all buried: no neighbour needs reading for them.
    let (any_solid, all_solid) = {
        let v = world.read(c).voxels();
        (
            v.iter().any(|&m| collides(m)),
            // Full cubes only: a shaped cell's faces are culled against its neighbours, which a
            // buried chunk doesn't read (a ramp placed in a dug cell deep down panicked here).
            v.iter()
                .all(|&m| collides(m) && is_cube(dwell_blocks::shape::shape_of(m))),
        )
    };
    if !any_solid {
        return out;
    }
    // A solid chunk under a solid layer has no top block: every cell is its own shape.
    let buried = all_solid && {
        let above = world.read(ChunkCoord::new(c.x, c.y + up, c.z)).voxels();
        let row = if up > 0 { 0 } else { n - 1 };
        (0..n).all(|z| (0..n).all(|x| collides(above[local_index(x, row, z)])))
    };
    let block = if buried {
        Vec::new()
    } else {
        world.padded_block(c, PAD)
    };
    let at = |x: i32, y: i32, z: i32| block[padded(x, y, z)];
    // A passable neighbour covers nothing: the face beside it is collided with.
    let shape_at = |x: i32, y: i32, z: i32| {
        let m = at(x, y, z);
        dwell_blocks::shape::shape_of(if collides(m) { m } else { dwell_world::M.air })
    };
    let voxels = world.read(c).voxels(); // borrowed: a 64 KiB copy overflows a WASM stack
    for (i, &m) in voxels.iter().enumerate() {
        if !collides(m) {
            continue;
        }
        let (x, y, z) = ((i & 31) as i32, ((i >> 5) & 31) as i32, (i >> 10) as i32);
        let shape = if buried {
            dwell_blocks::shape::shape_of(m)
        } else {
            shape_at(x, y, z)
        };
        if is_cube(shape) {
            out.solid
                .get_or_insert_with(|| Box::new([0u64; CHUNK_VOLUME / 64]))[i / 64] |=
                1 << (i % 64);
            continue;
        }
        out.shaped = true;
        for f in faces_of(shape) {
            if f.tag < SURFACE_TAG {
                let d = dwell_world::edit::FACE_DIRS[usize::from(f.tag)];
                let (nx, ny, nz) = (x + d[0], y + d[1], z + d[2]);
                // (A buried chunk's cells are cubes: they have no faces to cull here.)
                let neighbour = shape_at(nx, ny, nz);
                if face_covered(shape, usize::from(f.tag), neighbour) {
                    continue;
                }
            }
            let base = out.vertices.len() as u32;
            // Chunk-local metres: the shape is in fractions of its cell (HALF_HEIGHT_GRID.md §2).
            for p in &f.v[..usize::from(f.count)] {
                out.vertices.push(DVec3::new(
                    f64::from(x) + f64::from(p[0]),
                    (f64::from(y) + f64::from(p[1])) * CELL_H,
                    f64::from(z) + f64::from(p[2]),
                ));
            }
            for k in 1..u32::from(f.count) - 1 {
                out.triangles.push([base, base + k, base + k + 1]);
            }
        }
    }
    out
}

fn key(c: ChunkCoord, i: usize) -> IVector {
    let (x, y, z) = ((i & 31) as i32, ((i >> 5) & 31) as i32, (i >> 10) as i32);
    IVector::new(
        i64::from(c.x * CHUNK_SIZE + x),
        i64::from(c.y * CHUNK_SIZE + y),
        i64::from(c.z * CHUNK_SIZE + z),
    )
}

impl TerrainCollision {
    pub fn new(physics: &mut Physics) -> Self {
        let body = physics.w.bodies.insert(RigidBodyBuilder::fixed().build());
        // One voxel per cell: 1 m × the cell height × 1 m.
        let shape = SharedShape::new(Voxels::new(DVec3::from_array(CELL), &[]));
        // Static terrain has no mass: explicit (zero) mass properties keep rapier from recomputing
        // them from every voxel of the world each time a chunk's cells change (docs/BUGS.md 1: that
        // stalled the server's tick for a fast flyer streaming new terrain).
        let collider = ColliderBuilder::new(shape)
            .collision_groups(groups(Layer::Terrain))
            .friction(crate::physics::TERRAIN_FRICTION)
            .mass_properties(MassProperties::default())
            .build();
        let collider =
            physics
                .w
                .colliders
                .insert_with_parent(collider, body, &mut physics.w.bodies);
        Self {
            body,
            collider,
            chunks: HashMap::default(),
            watched: HashMap::default(),
            loads: 0,
            cells: 0,
            applied: HashMap::default(),
            behind: std::collections::BTreeSet::new(),
        }
    }

    fn voxels<'a>(&self, physics: &'a mut Physics) -> &'a mut Voxels {
        physics
            .w
            .colliders
            .get_mut(self.collider)
            .expect("terrain collider")
            .shape_mut()
            .as_voxels_mut()
            .expect("a voxel shape")
    }

    /// Puts the cells of chunk `c` whose collider bit differs from its `solid` one in line, those
    /// within cells `lo..=hi` (global cell keys) only, at most `budget` of them; returns how many.
    fn apply_cells(
        &mut self,
        physics: &mut Physics,
        c: ChunkCoord,
        lo: IVector,
        hi: IVector,
        budget: usize,
    ) -> usize {
        let want = self.chunks.get(&c).and_then(|l| l.solid.as_ref());
        let have = self.applied.get(&c);
        if want.is_none() && have.is_none() {
            self.behind.remove(&(c.y, c.z, c.x));
            return 0;
        }
        let origin = key(c, 0);
        let n = i64::from(CHUNK_SIZE);
        let clamp = |v: i64, o: i64| (v - o).clamp(0, n - 1) as usize;
        let (x0, x1) = (clamp(lo.x, origin.x), clamp(hi.x, origin.x));
        let (y0, y1) = (clamp(lo.y, origin.y), clamp(hi.y, origin.y));
        let (z0, z1) = (clamp(lo.z, origin.z), clamp(hi.z, origin.z));
        let whole = (x0, y0, z0, x1, y1, z1) == (0, 0, 0, 31, 31, 31);
        let mut set: Vec<(usize, bool)> = Vec::new();
        'words: for w in 0..CHUNK_VOLUME / 64 {
            let bits = want.map_or(0, |m| m[w]);
            let mut changed = bits ^ have.map_or(0, |m| m[w]);
            while changed != 0 {
                let b = changed.trailing_zeros() as usize;
                changed &= changed - 1;
                let i = w * 64 + b;
                let (x, y, z) = (i & 31, (i >> 5) & 31, i >> 10);
                if !whole
                    && !((x0..=x1).contains(&x) && (y0..=y1).contains(&y) && (z0..=z1).contains(&z))
                {
                    continue;
                }
                if set.len() >= budget {
                    break 'words;
                }
                set.push((i, bits & (1 << b) != 0));
            }
        }
        let voxels = self.voxels(physics);
        for &(i, on) in &set {
            voxels.set_voxel(key(c, i), on);
        }
        self.cells += set.len() as u64;
        let mask = self
            .applied
            .entry(c)
            .or_insert_with(|| Box::new([0u64; CHUNK_VOLUME / 64]));
        for &(i, on) in &set {
            if on {
                mask[i / 64] |= 1 << (i % 64);
            } else {
                mask[i / 64] &= !(1 << (i % 64));
            }
        }
        let want = self.chunks.get(&c).and_then(|l| l.solid.as_ref());
        let done = match want {
            Some(m) => m[..] == mask[..],
            None => mask.iter().all(|&w| w == 0),
        };
        if done {
            self.behind.remove(&(c.y, c.z, c.x));
        } else {
            self.behind.insert((c.y, c.z, c.x));
        }
        if mask.iter().all(|&w| w == 0) {
            self.applied.remove(&c);
        }
        set.len()
    }

    /// Puts the collider's cells in line with the chunks' (`behind`): all of those inside the
    /// `boxes` (world metres: the players' and pieces' reach) now, then up to `budget` more,
    /// in a fixed order (so the client's prediction builds the same). Returns the cells set.
    pub fn catch_up(
        &mut self,
        physics: &mut Physics,
        boxes: &[(DVec3, DVec3)],
        budget: usize,
    ) -> usize {
        let mut n = 0;
        for &(lo, hi) in boxes {
            n += self.catch_up_box(physics, lo, hi);
        }
        let behind: Vec<_> = self.behind.iter().copied().collect();
        let all = (IVector::splat(i64::MIN / 2), IVector::splat(i64::MAX / 2));
        for (y, z, x) in behind {
            if n >= budget {
                break;
            }
            n += self.apply_cells(physics, ChunkCoord::new(x, y, z), all.0, all.1, budget - n);
        }
        n
    }

    /// Puts in line every cell of the box (world metres) the collider is behind on.
    fn catch_up_box(&mut self, physics: &mut Physics, lo: DVec3, hi: DVec3) -> usize {
        if self.behind.is_empty() {
            return 0;
        }
        let cell = |p: DVec3| {
            let [x, y, z] = dwell_world::cell_of_pos(p.to_array());
            IVector::new(i64::from(x), i64::from(y), i64::from(z))
        };
        let (a, b) = (chunk_of_pos(lo.to_array()), chunk_of_pos(hi.to_array()));
        let (klo, khi) = (cell(lo), cell(hi));
        let mut n = 0;
        for cy in a.y.max(MIN_CHUNK_Y)..=b.y.min(MAX_CHUNK_Y) {
            for cz in a.z..=b.z {
                for cx in a.x..=b.x {
                    if self.behind.contains(&(cy, cz, cx)) {
                        n += self.apply_cells(
                            physics,
                            ChunkCoord::new(cx, cy, cz),
                            klo,
                            khi,
                            usize::MAX,
                        );
                    }
                }
            }
        }
        n
    }

    /// Whether every chunk's cells are in the collider (tests).
    pub fn caught_up(&self) -> bool {
        self.behind.is_empty()
    }

    fn insert_mesh(
        &self,
        physics: &mut Physics,
        c: ChunkCoord,
        b: Built,
    ) -> Option<ColliderHandle> {
        if b.triangles.is_empty() {
            return None;
        }
        let shape = SharedShape::trimesh_with_flags(
            b.vertices.clone(),
            b.triangles.clone(),
            TriMeshFlags::FIX_INTERNAL_EDGES,
        )
        .or_else(|_| SharedShape::trimesh(b.vertices, b.triangles))
        .ok()?;
        let origin = DVec3::from_array(chunk_origin_m(c));
        let collider = ColliderBuilder::new(shape)
            .translation(origin)
            .collision_groups(groups(Layer::Terrain))
            .friction(crate::physics::TERRAIN_FRICTION)
            .mass_properties(MassProperties::default())
            .build();
        Some(physics.w.insert_collider(collider, Some(self.body)))
    }

    /// (Re)builds a chunk's collision from the world, replacing what it held.
    fn load(&mut self, physics: &mut Physics, world: &mut VoxelWorld, c: ChunkCoord) {
        let built = build(world, c);
        let mesh_hash = built.mesh_hash();
        self.loads += 1;
        let wide = built.solid.is_some() || !built.triangles.is_empty();
        let shaped = built.shaped;
        let old = self.chunks.remove(&c);
        let fresh = old.is_none();
        let old_wide = old.as_ref().map(|o| o.wide);
        let (old_solid, old_mesh, old_hash) =
            old.map_or((None, None, 0), |o| (o.solid, o.mesh, o.mesh_hash));
        if old_solid != built.solid {
            self.behind.insert((c.y, c.z, c.x));
        }
        let solid = built.solid.clone();
        let mesh = if !fresh && old_hash == mesh_hash {
            old_mesh // the same triangles: keep the collider and its contacts
        } else {
            if let Some(h) = old_mesh {
                physics.w.remove_collider(h);
            }
            self.insert_mesh(physics, c, built)
        };
        self.chunks.insert(
            c,
            Loaded {
                solid,
                mesh,
                mesh_hash,
                wide,
                shaped,
            },
        );
        if old_wide != Some(wide) {
            if let Some(w) = old_wide {
                self.unwatch(c, w);
            }
            for d in deps(c, wide) {
                let (stamp, revision) = signature(world, d);
                self.watched
                    .entry(d)
                    .or_insert(Watch {
                        stamp,
                        revision,
                        users: 0,
                    })
                    .users += 1;
            }
        }
    }

    fn unwatch(&mut self, c: ChunkCoord, wide: bool) {
        for d in deps(c, wide) {
            if let Some(w) = self.watched.get_mut(&d) {
                w.users -= 1;
                if w.users == 0 {
                    self.watched.remove(&d);
                }
            }
        }
    }

    fn unload(&mut self, physics: &mut Physics, c: ChunkCoord) {
        let Some(old) = self.chunks.remove(&c) else {
            return;
        };
        if old.solid.is_some() || self.applied.contains_key(&c) {
            self.behind.insert((c.y, c.z, c.x));
        }
        if let Some(h) = old.mesh {
            physics.w.remove_collider(h);
        }
        self.unwatch(c, old.wide);
    }

    /// Makes sure the chunks overlapping the box (world metres) are in the collider, and the
    /// box's cells (the rest of a chunk's go in by [`Self::catch_up`]).
    pub fn ensure_box(
        &mut self,
        physics: &mut Physics,
        world: &mut VoxelWorld,
        lo: DVec3,
        hi: DVec3,
    ) {
        let a = chunk_of_pos(lo.to_array());
        let b = chunk_of_pos(hi.to_array());
        for cy in a.y.max(MIN_CHUNK_Y)..=b.y.min(MAX_CHUNK_Y) {
            for cz in a.z..=b.z {
                for cx in a.x..=b.x {
                    let c = ChunkCoord::new(cx, cy, cz);
                    if !self.chunks.contains_key(&c) {
                        self.load(physics, world, c);
                    }
                }
            }
        }
        self.catch_up_box(physics, lo, hi);
    }

    /// Loads the chunks the `boxes` (world metres) overlap, at most `budget` new ones, nearest their
    /// box's centre first (then in a fixed order, so every run builds the same); the rest wait for
    /// the next call. Returns how many it built.
    pub fn ensure_boxes_within(
        &mut self,
        physics: &mut Physics,
        world: &mut VoxelWorld,
        boxes: &[(DVec3, DVec3)],
        budget: usize,
    ) -> usize {
        let mut missing: Vec<(f64, ChunkCoord)> = Vec::new();
        for &(lo, hi) in boxes {
            let centre = (lo + hi) * 0.5;
            let a = chunk_of_pos(lo.to_array());
            let b = chunk_of_pos(hi.to_array());
            for cy in a.y.max(MIN_CHUNK_Y)..=b.y.min(MAX_CHUNK_Y) {
                for cz in a.z..=b.z {
                    for cx in a.x..=b.x {
                        let c = ChunkCoord::new(cx, cy, cz);
                        if self.chunks.contains_key(&c) {
                            continue;
                        }
                        let m = DVec3::from_array(dwell_world::chunk_centre_m(c));
                        missing.push(((m - centre).length_squared(), c));
                    }
                }
            }
        }
        missing.sort_by(|a, b| {
            a.0.total_cmp(&b.0)
                .then((a.1.y, a.1.z, a.1.x).cmp(&(b.1.y, b.1.z, b.1.x)))
        });
        let mut built = 0;
        for (_, c) in missing {
            if built >= budget {
                break;
            }
            if !self.chunks.contains_key(&c) {
                self.load(physics, world, c);
                built += 1;
            }
        }
        built
    }

    /// Rebuilds the loaded chunks whose collision the world has changed: the chunk itself, and the
    /// shaped chunks around it, whose faces are culled against it (edits, streamed or replaced
    /// chunks). A chunk of full cubes only is its own cells alone (Parry joins voxels across chunk
    /// borders itself), so it isn't rebuilt for a neighbour: a settling piece's cells rebuilt the
    /// 27 chunks round them, ~10 ms of a tick natively for a blast's pieces (2026-11-06). An
    /// unmodified chunk the world evicted reads the same when regenerated; rebuilding anyway is
    /// the simple, safe path.
    pub fn sync(&mut self, physics: &mut Physics, world: &mut VoxelWorld) {
        let mut changed: Vec<ChunkCoord> = Vec::new();
        for (&c, w) in &mut self.watched {
            let now = signature(world, c);
            if now != (w.stamp, w.revision) {
                (w.stamp, w.revision) = now;
                changed.push(c);
            }
        }
        if changed.is_empty() {
            return;
        }
        let mut rebuild: Vec<ChunkCoord> = changed
            .iter()
            .flat_map(|&c| around(c).map(move |n| (c, n)))
            .filter(|&(c, n)| self.chunks.get(&n).is_some_and(|l| n == c || l.shaped))
            .map(|(_, n)| n)
            .collect();
        // In a fixed order, so every run (and the client's prediction) builds the same colliders.
        rebuild.sort_by_key(|c| (c.y, c.z, c.x));
        rebuild.dedup();
        for c in rebuild {
            self.load(physics, world, c);
        }
        // Building may have generated chunks some watch saw as missing: take their stamps now.
        for (&c, w) in &mut self.watched {
            (w.stamp, w.revision) = signature(world, c);
        }
    }

    /// Drops the chunks `keep` rejects.
    pub fn retain(&mut self, physics: &mut Physics, keep: impl Fn(ChunkCoord) -> bool) {
        let mut drop: Vec<ChunkCoord> = self.chunks.keys().copied().filter(|&c| !keep(c)).collect();
        drop.sort_by_key(|c| (c.y, c.z, c.x));
        for c in drop {
            self.unload(physics, c);
        }
    }

    /// How many full-cube cells have been set in or taken out of the collider.
    pub fn cells(&self) -> u64 {
        self.cells
    }

    /// How many chunk builds there have been (tests).
    pub fn loads(&self) -> u64 {
        self.loads
    }

    pub fn loaded_chunks(&self) -> usize {
        self.chunks.len()
    }
    pub fn is_loaded(&self, c: ChunkCoord) -> bool {
        self.chunks.contains_key(&c)
    }
}

#[cfg(test)]
mod mass_tests {
    use super::*;
    use dwell_world::{Chunk, ChunkCoord, VoxelWorld};

    // face-a-only: whether the terrain collider carries mass doesn't depend on the face; one chunk of stone at the origin.
    #[test]
    fn terrain_colliders_carry_no_mass_so_loading_chunks_never_recomputes_it() {
        // docs/BUGS.md 1: with the default density, every chunk loaded made rapier recompute the
        // world-sized voxel collider's mass properties from all its voxels, stalling the server's tick
        // while a fast flyer streamed new terrain.
        let stone = dwell_blocks::find_block("dwell:stone")
            .expect("stone")
            .first_state;
        let generator: dwell_world::ChunkGenerator =
            std::sync::Arc::new(move |c: ChunkCoord, chunk: &mut Chunk| {
                if c == ChunkCoord::new(0, 0, 0) {
                    for x in 0..16 {
                        for z in 0..16 {
                            for y in 0..8 {
                                chunk.set(x, y, z, stone);
                            }
                        }
                    }
                }
            });
        let mut world = VoxelWorld::new(generator, None);
        let mut physics = Physics::default();
        let mut terrain = TerrainCollision::new(&mut physics);
        terrain.ensure_box(
            &mut physics,
            &mut world,
            DVec3::splat(1.0),
            DVec3::splat(2.0),
        );
        physics.step(1.0 / 60.0);
        let collider = physics.w.colliders.get(terrain.collider).expect("collider");
        assert_eq!(
            collider.mass(),
            0.0,
            "the terrain collider has mass {}",
            collider.mass()
        );
    }
}

#[cfg(test)]
mod rebuild_tests {
    use super::*;
    use dwell_world::{Chunk, ChunkCoord, VoxelWorld};

    /// Stone below y = 0 and air above, the 3 × 3 × 3 chunks round the chunk at the origin loaded.
    fn loaded() -> (VoxelWorld, Physics, TerrainCollision) {
        let stone = dwell_blocks::require("dwell:stone");
        let generator: dwell_world::ChunkGenerator =
            std::sync::Arc::new(move |c: ChunkCoord, chunk: &mut Chunk| {
                if c.y < 0 {
                    chunk.voxels_mut().fill(stone);
                }
            });
        let mut world = VoxelWorld::new(generator, None);
        let mut physics = Physics::default();
        let mut terrain = TerrainCollision::new(&mut physics);
        let lo = DVec3::from_array(chunk_origin_m(ChunkCoord::new(-1, -1, -1)));
        let hi = DVec3::from_array(chunk_origin_m(ChunkCoord::new(2, 2, 2)));
        terrain.ensure_box(
            &mut physics,
            &mut world,
            lo + DVec3::splat(0.1),
            hi - DVec3::splat(0.1),
        );
        assert_eq!(terrain.loaded_chunks(), 27);
        // The chunks the loads generated round the block are watched from now.
        terrain.sync(&mut physics, &mut world);
        (world, physics, terrain)
    }

    // face-a-only: which chunks are rebuilt has no up; one block of chunks round the origin.
    #[test]
    fn a_cell_changed_rebuilds_its_chunk_not_the_full_cube_chunks_round_it() {
        // 2026-11-06: each settling piece's cells rebuilt the 27 chunks round them (a blast's
        // pieces settling spiked the tick ~10 ms natively); a chunk of full cubes reads only its
        // own cells. Red before: 27 builds.
        let (mut world, mut physics, mut terrain) = loaded();
        let before = terrain.loads();
        world.set_voxel(5, -1, 5, dwell_world::M.air);
        terrain.sync(&mut physics, &mut world);
        assert_eq!(
            terrain.loads() - before,
            1,
            "only the edited chunk is built again"
        );
    }

    // face-a-only: which chunks are rebuilt has no up; one block of chunks round the origin.
    #[test]
    fn water_coming_and_going_rebuilds_no_collision() {
        // 2026-11-06: every write bumped the chunk's revision, so the water the host let into a
        // blast's crater rebuilt the chunks round it: 54 of the dam blast's first second's
        // builds (natively), terrain collision 150–600 ms a second in the browser. Water
        // collides as air does. Red before: 5 builds.
        let water = dwell_world::M.water;
        let ramp = |flooded: bool| {
            dwell_blocks::parse_state(&format!(
                "dwell:grass_ramp[facing=west,flooded={flooded},half=bottom,shape=ramp]"
            ))
            .expect("a ramp")
        };
        let (mut world, mut physics, mut terrain) = loaded();
        world.set_voxel(-1, -1, 5, ramp(false));
        world.set_voxel(5, -1, 5, dwell_world::M.air);
        terrain.sync(&mut physics, &mut world);
        let before = terrain.loads();
        // Water fills the dug cell and stands on the ground, in a modification (no bumps) and
        // cell by cell; the ramp floods; then it drains.
        world.write_voxel(5, -1, 5, water);
        world.set_voxel(6, 0, 5, water);
        world.set_voxel(31, 0, 31, water); // a chunk corner: the 8 chunks round it are read
        world.set_voxel(-1, -1, 5, ramp(true));
        terrain.sync(&mut physics, &mut world);
        world.set_voxel(5, -1, 5, dwell_world::M.air);
        world.set_voxel(6, 0, 5, dwell_world::M.air);
        world.set_voxel(-1, -1, 5, ramp(false));
        terrain.sync(&mut physics, &mut world);
        assert_eq!(terrain.loads() - before, 0, "water collides as air");
        // A block placed in the water does.
        world.set_voxel(40, 0, 40, dwell_world::M.stone); // away from the ramp's chunk
        terrain.sync(&mut physics, &mut world);
        assert_eq!(terrain.loads() - before, 1, "the placed block's chunk");
    }

    // face-a-only: as above.
    #[test]
    fn a_shaped_chunk_beside_an_edit_is_rebuilt_as_its_faces_may_uncover() {
        // A ramp on the border of the chunk at x = -1, its east face against the stone at x = 0
        // (the next chunk): digging that stone uncovers the face, so the ramp's chunk is rebuilt.
        let ramp = dwell_blocks::parse_state(
            "dwell:grass_ramp[facing=west,flooded=false,half=bottom,shape=ramp]",
        )
        .expect("a ramp");
        let (mut world, mut physics, mut terrain) = loaded();
        world.set_voxel(-1, -1, 5, ramp);
        terrain.sync(&mut physics, &mut world);
        let mesh = |t: &TerrainCollision| t.chunks[&ChunkCoord::new(-1, -1, 0)].mesh_hash;
        let covered = mesh(&terrain);
        let before = terrain.loads();
        world.set_voxel(0, -1, 5, dwell_world::M.air);
        terrain.sync(&mut physics, &mut world);
        assert_eq!(
            terrain.loads() - before,
            2,
            "the edited chunk and the ramp's"
        );
        assert_ne!(
            mesh(&terrain),
            covered,
            "the ramp's face by the dug cell collides"
        );
    }

    // face-a-only: a buried chunk is one under solid ground; the rule has no up of its own.
    #[test]
    fn a_ramp_in_a_chunk_otherwise_solid_under_solid_ground_builds() {
        // Found 2026-11-06: a chunk all solid under a solid chunk was built as cubes without
        // reading its neighbours, and a ramp in it (placed in a dug cell deep down) read them
        // to cull its faces: an index out of bounds, the server's tick panicking.
        let ramp = dwell_blocks::parse_state(
            "dwell:grass_ramp[facing=east,flooded=false,half=bottom,shape=ramp]",
        )
        .expect("a ramp");
        let (mut world, mut physics, mut terrain) = loaded();
        world.set_voxel(5, -40, 5, ramp); // in the chunk under (0, -1, 0)
        let at = DVec3::new(5.5, -19.9, 5.5);
        terrain.ensure_box(&mut physics, &mut world, at, at);
        assert!(terrain.is_loaded(ChunkCoord::new(0, -2, 0)));
        assert!(terrain.chunks[&ChunkCoord::new(0, -2, 0)].shaped);
    }
}
