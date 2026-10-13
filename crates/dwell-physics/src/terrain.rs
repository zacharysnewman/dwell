//! Terrain collision (PLAYER_CONTROLLER.md §5): one static `Voxels` collider holding the solid
//! cells of the chunks around players. Parry classifies each voxel by its neighbours, so the
//! capsule slides across voxel edges and chunk borders without catching (the internal-edge problem
//! the Jolt build solved with per-chunk meshes in regions); with f64 positions it is exact anywhere
//! in the world, so it needs no regions or anchors. Chunks are added on demand, re-synced when the
//! world changes them (same tick), and dropped when far from every player.
//!
//! Every solid state collides as its full cell: the baseline has only cubes (Phase 15); shaped
//! blocks (Phase 18) will need their own colliders.

use std::collections::HashMap;

use dwell_blocks::info;
use dwell_world::{CHUNK_SIZE, CHUNK_VOLUME, ChunkCoord, MAX_CHUNK_Y, MIN_CHUNK_Y, VoxelWorld, chunk_of};
use rapier3d_f64::math::IVector;
use rapier3d_f64::parry::shape::Voxels;
use rapier3d_f64::prelude::*;

use crate::physics::{DVec3, Layer, Physics, groups};

/// What the collider holds of one chunk: which stored chunk and revision, and its solid cells.
struct Loaded {
    stamp: u64,
    revision: u32,
    solid: Box<[u64; CHUNK_VOLUME / 64]>,
}

pub struct TerrainCollision {
    collider: ColliderHandle,
    chunks: HashMap<ChunkCoord, Loaded>,
}

fn solid_mask(world: &mut VoxelWorld, c: ChunkCoord) -> (u64, u32, Box<[u64; CHUNK_VOLUME / 64]>) {
    let chunk = world.read(c);
    let mut mask = Box::new([0u64; CHUNK_VOLUME / 64]);
    for (i, &m) in chunk.voxels().iter().enumerate() {
        if info(m).solid {
            mask[i / 64] |= 1 << (i % 64);
        }
    }
    (chunk.stamp(), chunk.revision(), mask)
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
        let shape = SharedShape::new(Voxels::new(DVec3::ONE, &[]));
        let collider = ColliderBuilder::new(shape).collision_groups(groups(Layer::Terrain)).friction(0.0).build();
        let collider = physics.w.colliders.insert_with_parent(collider, body, &mut physics.w.bodies);
        Self { collider, chunks: HashMap::new() }
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
    fn apply(&self, physics: &mut Physics, c: ChunkCoord, old: Option<&[u64; CHUNK_VOLUME / 64]>, new: &[u64; CHUNK_VOLUME / 64]) {
        let voxels = self.voxels(physics);
        for (w, &bits) in new.iter().enumerate() {
            let mut changed = bits ^ old.map_or(0, |o| o[w]);
            while changed != 0 {
                let b = changed.trailing_zeros() as usize;
                changed &= changed - 1;
                let i = w * 64 + b;
                voxels.set_voxel(key(c, i), bits & (1 << b) != 0);
            }
        }
    }

    /// Makes sure the chunks overlapping the box (world metres) are in the collider.
    pub fn ensure_box(&mut self, physics: &mut Physics, world: &mut VoxelWorld, lo: DVec3, hi: DVec3) {
        let a = chunk_of(lo.x.floor() as i32, lo.y.floor() as i32, lo.z.floor() as i32);
        let b = chunk_of(hi.x.floor() as i32, hi.y.floor() as i32, hi.z.floor() as i32);
        for cy in a.y.max(MIN_CHUNK_Y)..=b.y.min(MAX_CHUNK_Y) {
            for cz in a.z..=b.z {
                for cx in a.x..=b.x {
                    let c = ChunkCoord::new(cx, cy, cz);
                    if self.chunks.contains_key(&c) {
                        continue;
                    }
                    let (stamp, revision, solid) = solid_mask(world, c);
                    self.apply(physics, c, None, &solid);
                    self.chunks.insert(c, Loaded { stamp, revision, solid });
                }
            }
        }
    }

    /// Re-syncs loaded chunks the world has changed (edits, streamed or replaced chunks).
    pub fn sync(&mut self, physics: &mut Physics, world: &mut VoxelWorld) {
        let coords: Vec<ChunkCoord> = self.chunks.keys().copied().collect();
        for c in coords {
            let (stamp, revision) = world.find(c).map_or((0, 0), |ch| (ch.stamp(), ch.revision()));
            let loaded = &self.chunks[&c];
            if loaded.stamp == stamp && loaded.revision == revision {
                continue;
            }
            // An unmodified chunk the world evicted reads the same when regenerated; reading it
            // anyway is the simple, safe path.
            let (stamp, revision, solid) = solid_mask(world, c);
            let old = self.chunks.remove(&c).expect("loaded");
            self.apply(physics, c, Some(&old.solid), &solid);
            self.chunks.insert(c, Loaded { stamp, revision, solid });
        }
    }

    /// Drops the chunks `keep` rejects.
    pub fn retain(&mut self, physics: &mut Physics, keep: impl Fn(ChunkCoord) -> bool) {
        let drop: Vec<ChunkCoord> = self.chunks.keys().copied().filter(|&c| !keep(c)).collect();
        let empty = Box::new([0u64; CHUNK_VOLUME / 64]);
        for c in drop {
            let old = self.chunks.remove(&c).expect("loaded");
            self.apply(physics, c, Some(&old.solid), &empty);
        }
    }

    pub fn loaded_chunks(&self) -> usize {
        self.chunks.len()
    }
    pub fn is_loaded(&self, c: ChunkCoord) -> bool {
        self.chunks.contains_key(&c)
    }
}
