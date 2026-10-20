//! The bifacial world's terrain (docs/BIFACIAL_WORLD.md §5, ADR 0023): two face-local generators —
//! face A from the world seed, face B from its own seed streams — and chunks of face B are
//! face-local chunks flipped about the midplane. Still a pure function of (seed, chunk).

use dwell_blocks::mirror;
use dwell_world::lod::{self, LodBounds, LodCells, LodCoord, LodKind, LodSurfaces};
use dwell_world::{
    CHUNK_SIZE, Chunk, ChunkCoord, Face, M, face_local_chunk, face_of_chunk_y, face_of_y,
    local_index, mirror_y,
};

use crate::noise::seed_word;
use crate::terrain::TerrainGenerator;

/// The seed of a face's generator: face A keeps the world seed, face B's is derived from it.
pub fn face_seed(world_seed: u64, face: Face) -> u64 {
    match face {
        Face::A => world_seed,
        Face::B => {
            let lo = u64::from(seed_word(world_seed, 0xFACE_B001));
            let hi = u64::from(seed_word(world_seed, 0xFACE_B002));
            (hi << 32) | lo
        }
    }
}

#[derive(Clone, Debug)]
pub struct BifacialTerrain {
    a: TerrainGenerator,
    b: TerrainGenerator,
}

impl BifacialTerrain {
    pub fn new(world_seed: u64) -> Self {
        Self {
            a: TerrainGenerator::new(face_seed(world_seed, Face::A)),
            b: TerrainGenerator::new(face_seed(world_seed, Face::B)),
        }
    }

    /// The mirror world (a test tool for face parity, BIFACIAL_WORLD.md §11): face B generated
    /// from face A's seed, so it is face A's exact mirror image and anything drawn, collided or
    /// edited on one face can be compared with the other. Never a real world's generator.
    pub fn mirrored(world_seed: u64) -> Self {
        let a = TerrainGenerator::new(face_seed(world_seed, Face::A));
        Self { b: a.clone(), a }
    }

    /// A face's generator, in face-local coordinates.
    pub fn local(&self, face: Face) -> &TerrainGenerator {
        match face {
            Face::A => &self.a,
            Face::B => &self.b,
        }
    }

    /// A chunk of either face: face A's directly, face B's as the vertical flip of the face-local
    /// chunk with shaped blocks turned over.
    pub fn generate(&self, coord: ChunkCoord, chunk: &mut Chunk) {
        if face_of_chunk_y(coord.y) == Face::A {
            self.a.generate(coord, chunk);
            return;
        }
        let mut scratch = Chunk::default();
        self.b.generate(face_local_chunk(coord), &mut scratch);
        let src = scratch.voxels();
        let dst = chunk.voxels_mut();
        const S: i32 = CHUNK_SIZE;
        for z in 0..S {
            for y in 0..S {
                for x in 0..S {
                    dst[local_index(x, S - 1 - y, z)] = mirror(src[local_index(x, y, z)]);
                }
            }
        }
    }

    pub fn is_air_chunk(&self, coord: ChunkCoord) -> bool {
        self.local(face_of_chunk_y(coord.y))
            .is_air_chunk(face_local_chunk(coord))
    }

    /// The sky test's height for a chunk column of a face (face-local).
    pub fn sky_floor_at(&self, cx: i32, cz: i32, face: Face) -> f32 {
        self.local(face).sky_floor_at(cx, cz)
    }

    /// `TerrainGenerator::is_air_chunk_with` for a chunk in the world.
    pub fn is_air_chunk_with(coord: ChunkCoord, sky_floor: f32) -> bool {
        TerrainGenerator::is_air_chunk_with(face_local_chunk(coord), sky_floor)
    }

    /// The column bounds of both faces.
    pub fn lod_bounds_at(&self, level: i32, i: i32, k: i32) -> LodBounds {
        let a = self.a.lod_bounds_at(level, i, k);
        let b = self.b.lod_bounds_at(level, i, k);
        LodBounds {
            lo: a.lo,
            hi: a.hi,
            lo_b: b.lo,
            hi_b: b.hi,
            any_inside: a.any_inside || b.any_inside,
            bifacial: true,
        }
    }

    /// A section of the world, its rows in world order. Rows belong to the face of their centre:
    /// face A's come from its generator, face B's from the mirror image of the section in face B's.
    /// `surface` receives face A's column surfaces, `surface_b` face B's (face-local).
    pub fn generate_lod(
        &self,
        c: LodCoord,
        cells: &mut LodCells,
        surface: Option<&mut LodSurfaces>,
        surface_b: Option<&mut LodSurfaces>,
    ) -> LodKind {
        let columns = (lod::PAD * lod::PAD) as usize;
        cells.clear();
        cells.resize(lod::VOLUME, M.air);
        let mut surface = surface;
        let mut surface_b = surface_b;
        for s in [surface.as_deref_mut(), surface_b.as_deref_mut()]
            .into_iter()
            .flatten()
        {
            s.clear();
            s.resize(columns, Default::default());
        }
        if !lod::in_world(c) {
            return LodKind::Empty;
        }
        let y0 = lod::section_origin(c).y;
        let first_a = lod::first_face_a_row(c);
        let mut bounds = LodBounds::default();
        let mut local = LodCells::new();
        let copy_rows = |cells: &mut LodCells,
                         local: &LodCells,
                         rows: std::ops::RangeInclusive<i32>,
                         row_of: &dyn Fn(i32) -> i32| {
            for r in rows {
                let from = row_of(r);
                for z in -1..=lod::SECTION_CELLS {
                    for x in -1..=lod::SECTION_CELLS {
                        cells[lod::cell_index(x, r, z)] = local[lod::cell_index(x, from, z)];
                    }
                }
            }
        };
        if first_a <= lod::SECTION_CELLS {
            let (_, b) = self.a.generate_lod_at(c, y0, &mut local, surface);
            bounds.lo = b.lo;
            bounds.hi = b.hi;
            bounds.any_inside = b.any_inside;
            copy_rows(cells, &local, first_a..=lod::SECTION_CELLS, &|r| r);
        }
        if first_a >= 0 {
            // Face-local row r' is world row 31 − r': the section's mirror image starts at the
            // mirror of the section's top.
            let origin_l = lod::mirror_row_bottom(y0, lod::section_size(c.level));
            let (_, b) = self.b.generate_lod_at(c, origin_l, &mut local, surface_b);
            bounds.lo_b = b.lo;
            bounds.hi_b = b.hi;
            bounds.any_inside = bounds.any_inside || b.any_inside;
            copy_rows(
                cells,
                &local,
                -1..=(first_a - 1).min(lod::SECTION_CELLS),
                &|r| lod::SECTION_CELLS - 1 - r,
            );
        }
        let kind = lod::kind_from_bounds(c, &bounds);
        // A section the bounds decide is canonical whatever the passes generated.
        match kind {
            LodKind::Empty => cells.fill(M.air),
            LodKind::Buried => cells.fill(M.stone),
            LodKind::Content => {}
        }
        kind
    }

    /// Feet position players spawn at: always on face A.
    pub fn spawn_point(&self) -> [f64; 3] {
        self.a.spawn_point()
    }

    /// Face B's spawn point (a dev tool's: `?spawn=b` local worlds), its generator's face-local
    /// feet mirrored into the world: the player stands on face B's ground, upside down.
    pub fn spawn_point_b(&self) -> [f64; 3] {
        let [x, y, z] = self.b.spawn_point();
        [x, dwell_world::FaceFrame::B.y(y), z]
    }

    /// Face B's voxel (x, y, z) is the face-local voxel (x, mirror_y(y), z) of face B's generator.
    pub fn solid_at(&self, x: i32, y: i32, z: i32) -> bool {
        match face_of_y(i64::from(y)) {
            Face::A => self.a.solid_at(x, y, z),
            Face::B => self.b.solid_at(x, mirror_y(y), z),
        }
    }
}
