//! Terrain collision (PLAYER_CONTROLLER.md §5): one static body holding the solid cells of the
//! chunks around players. Cells that collide as their full cube — every solid block but a shaped
//! one — go into one Rapier (Parry) `Voxels` collider: Parry classifies each voxel by its
//! neighbours, so the capsule slides across voxel edges and chunk borders without catching (the
//! internal-edge problem the Jolt build solved with per-chunk meshes in regions). Shaped cells —
//! the generated ramps (HALF_HEIGHT_GRID.md §3) — go into one
//! triangle mesh per chunk (`TriMeshFlags::FIX_INTERNAL_EDGES`) of their faces that no neighbour
//! covers. With f64 positions it is exact anywhere in the world, so it needs no regions or anchors.
//! Chunks are added on demand, rebuilt when the world changes them or any chunk their shapes read
//! (same tick), and dropped when far from every player.

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
}

type Mask = Box<[u64; CHUNK_VOLUME / 64]>;

/// A chunk some loaded chunk's collision was built from: which stored chunk and revision it was,
/// and how many loaded chunks read it.
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
}

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
        .map_or((0, 0), |ch| (ch.stamp(), ch.revision()))
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
        vertices: Vec::new(),
        triangles: Vec::new(),
    };
    let up = up_of_y(c.y * n);
    // Most chunks are all air or all buried: no neighbour needs reading for them.
    let (any_solid, all_solid) = {
        let v = world.read(c).voxels();
        (
            v.iter().any(|&m| collides(m)),
            v.iter().all(|&m| collides(m)),
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
            .friction(0.0)
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

    /// Sets the cells whose bit differs between `old` and `new`.
    fn apply(&self, physics: &mut Physics, c: ChunkCoord, old: Option<&Mask>, new: Option<&Mask>) {
        if old.is_none() && new.is_none() {
            return;
        }
        let voxels = self.voxels(physics);
        for w in 0..CHUNK_VOLUME / 64 {
            let bits = new.map_or(0, |n| n[w]);
            let mut changed = bits ^ old.map_or(0, |o| o[w]);
            while changed != 0 {
                let b = changed.trailing_zeros() as usize;
                changed &= changed - 1;
                let i = w * 64 + b;
                voxels.set_voxel(key(c, i), bits & (1 << b) != 0);
            }
        }
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
            .friction(0.0)
            .mass_properties(MassProperties::default())
            .build();
        Some(physics.w.insert_collider(collider, Some(self.body)))
    }

    /// (Re)builds a chunk's collision from the world, replacing what it held.
    fn load(&mut self, physics: &mut Physics, world: &mut VoxelWorld, c: ChunkCoord) {
        let built = build(world, c);
        let mesh_hash = built.mesh_hash();
        let wide = built.solid.is_some() || !built.triangles.is_empty();
        let old = self.chunks.remove(&c);
        let fresh = old.is_none();
        let old_wide = old.as_ref().map(|o| o.wide);
        let (old_solid, old_mesh, old_hash) =
            old.map_or((None, None, 0), |o| (o.solid, o.mesh, o.mesh_hash));
        self.apply(physics, c, old_solid.as_ref(), built.solid.as_ref());
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
        self.apply(physics, c, old.solid.as_ref(), None);
        if let Some(h) = old.mesh {
            physics.w.remove_collider(h);
        }
        self.unwatch(c, old.wide);
    }

    /// Makes sure the chunks overlapping the box (world metres) are in the collider.
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
    }

    /// Rebuilds the loaded chunks whose collision the world has changed: the chunk itself or any
    /// chunk around it (edits, streamed or replaced chunks). An unmodified chunk the world evicted
    /// reads the same when regenerated; rebuilding anyway is the simple, safe path.
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
            .flat_map(|&c| around(c))
            .filter(|c| self.chunks.contains_key(c))
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
