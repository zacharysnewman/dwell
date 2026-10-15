//! Background jobs (ARCHITECTURE.md §5.1, ADR 0007): chunk generation, LOD bounds and sections,
//! the debug map, and chunk and section meshing. A [`Runner`] executes them (in a Web Worker with
//! its own WASM instance, or inline natively and in tests); [`codec`] carries jobs and results
//! across `postMessage` as bytes. The main thread's [`Jobs`] trait hides where they run.

pub mod codec;

use dwell_mesh::{ChunkMeshes, MeshSectionOptions, SectionMeshes};
use dwell_world::lod::{self, LodBounds, LodCoord, LodGenerator, LodSurfaces};
use dwell_world::{Chunk, ChunkCoord, ChunkGenerator, GENERATOR_TERRAIN, chunk_hash};
use dwell_worldgen::BifacialTerrain;

/// One unit of background work.
#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    /// A chunk's voxels (and their hash, for the verification check, §6.3).
    Generate { coord: ChunkCoord },
    /// Height bounds of the column of LOD sections (level, i, ·, k).
    LodBounds { level: i32, i: i32, k: i32 },
    /// An LOD section's cells and column surfaces.
    Lod { coord: LodCoord },
    /// The F4 map: n × n samples of each column's height from (x0, z0), `step` apart.
    Map {
        x0: i32,
        z0: i32,
        step: i32,
        n: i32,
        face: u8,
    },
    /// Greedy-meshes a chunk from its padded voxels; `origin` is its minimum corner's world cell
    /// (plants are jittered by their world position).
    MeshChunk {
        padded: Vec<u16>,
        mirror: bool,
        origin: [i32; 3],
    },
    /// Meshes an LOD section.
    MeshSection {
        cells: Vec<u16>,
        surface: Option<Vec<f32>>,
        water_drop: f64,
        mirror: bool,
    },
}

impl Job {
    /// Meshing jobs need no world; the others need the generator.
    pub fn is_mesh(&self) -> bool {
        matches!(self, Job::MeshChunk { .. } | Job::MeshSection { .. })
    }
}

/// A job's result.
#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    Chunk {
        voxels: Vec<u16>,
        hash: u64,
    },
    Bounds(LodBounds),
    /// `surface` is face A's (world terms), `surface_b` face B's (face-local); `None` where the
    /// generator has none (flat worlds) or the section has no rows of that face. SURFACE_STRIDE
    /// floats per column: height (m), material, flags (1 valid, 2 wet), water level (m).
    Lod {
        cells: Vec<u16>,
        surface: Option<Vec<f32>>,
        surface_b: Option<Vec<f32>>,
    },
    /// Height (i16 little-endian) and an outside flag per sample; `None` without a terrain.
    Map(Option<Vec<u8>>),
    ChunkMesh(Box<ChunkMeshes>),
    SectionMesh(Box<SectionMeshes>),
    Failed(String),
}

/// Runs jobs for one world (a worker's state).
pub struct Runner {
    chunks: Option<ChunkGenerator>,
    terrain: Option<BifacialTerrain>,
    lod: Option<LodGenerator>,
    bounds: Option<lod::LodBoundsFn>,
}

impl Runner {
    /// A runner for meshing only.
    pub fn mesher() -> Self {
        Self {
            chunks: None,
            terrain: None,
            lod: None,
            bounds: None,
        }
    }

    /// A runner for a world (its generator version and seed, from Welcome), meshing included.
    pub fn world(generator_version: u32, world_seed: u64) -> Self {
        Self {
            chunks: Some(dwell_worldgen::generator_for(generator_version, world_seed)),
            terrain: (generator_version == GENERATOR_TERRAIN)
                .then(|| BifacialTerrain::new(world_seed)),
            lod: Some(dwell_worldgen::lod_generator_for(
                generator_version,
                world_seed,
            )),
            bounds: Some(dwell_worldgen::lod_bounds_for(
                generator_version,
                world_seed,
            )),
        }
    }

    pub fn run(&self, job: Job) -> Output {
        match job {
            Job::Generate { coord } => {
                let Some(g) = &self.chunks else {
                    return no_world();
                };
                let mut chunk = Chunk::default();
                g(coord, &mut chunk);
                Output::Chunk {
                    hash: chunk_hash(&chunk),
                    voxels: chunk.voxels().to_vec(),
                }
            }
            Job::LodBounds { level, i, k } => match &self.bounds {
                Some(b) => Output::Bounds(b(level, i, k)),
                None => no_world(),
            },
            Job::Lod { coord } => self.lod(coord),
            Job::Map {
                x0,
                z0,
                step,
                n,
                face,
            } => {
                let Some(t) = &self.terrain else {
                    return Output::Map(None);
                };
                if n <= 0 || n > 512 || step <= 0 {
                    return Output::Map(None);
                }
                let local = t.local(if face == 1 {
                    dwell_world::Face::B
                } else {
                    dwell_world::Face::A
                });
                let mut out = Vec::with_capacity((n * n * 3) as usize);
                for j in 0..n {
                    for k in 0..n {
                        let c = local.column_at(x0 + k * step, z0 + j * step);
                        let h = c.height.clamp(-32768.0, 32767.0) as i16;
                        out.extend_from_slice(&h.to_le_bytes());
                        out.push(u8::from(c.outside));
                    }
                }
                Output::Map(Some(out))
            }
            Job::MeshChunk {
                padded,
                mirror,
                origin,
            } => match dwell_mesh::mesh_chunk_at(&padded, mirror, origin) {
                Ok(m) => Output::ChunkMesh(Box::new(m)),
                Err(e) => Output::Failed(format!("{e:?}")),
            },
            Job::MeshSection {
                cells,
                surface,
                water_drop,
                mirror,
            } => {
                let options = MeshSectionOptions {
                    surface: surface.as_deref(),
                    water_drop,
                    mirror,
                };
                match dwell_mesh::mesh_section(&cells, &options) {
                    Ok(m) => Output::SectionMesh(Box::new(m)),
                    Err(e) => Output::Failed(format!("{e:?}")),
                }
            }
        }
    }

    fn lod(&self, c: LodCoord) -> Output {
        let mut cells = Vec::new();
        if let Some(t) = &self.terrain {
            let (mut a, mut b) = (LodSurfaces::new(), LodSurfaces::new());
            t.generate_lod(c, &mut cells, Some(&mut a), Some(&mut b));
            // Which faces' passes ran: face A's rows start at `first`, face B's end below it.
            let first = lod::first_face_a_row(c);
            let in_world = lod::in_world(c);
            let pack = |s: &LodSurfaces, ran: bool| {
                (ran && in_world && !s.is_empty()).then(|| surface_floats(s))
            };
            return Output::Lod {
                cells,
                surface: pack(&a, first <= lod::SECTION_CELLS),
                surface_b: pack(&b, first >= 0),
            };
        }
        let Some(g) = &self.lod else {
            return no_world();
        };
        g(c, &mut cells);
        Output::Lod {
            cells,
            surface: None,
            surface_b: None,
        }
    }
}

fn no_world() -> Output {
    Output::Failed("this worker has no world".into())
}

/// Column surfaces as the meshers read them (the C++ export's layout).
pub fn surface_floats(s: &LodSurfaces) -> Vec<f32> {
    s.iter()
        .flat_map(|c| {
            [
                c.height,
                f32::from(c.material),
                f32::from(u8::from(c.valid) | (u8::from(c.wet) << 1)),
                c.water,
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_world::GENERATOR_TERRAIN;

    #[test]
    fn generates_hashes_and_meshes_a_chunk() {
        let r = Runner::world(GENERATOR_TERRAIN, 1);
        let spawn = dwell_worldgen::spawn_point_for(GENERATOR_TERRAIN, 1);
        let coord = dwell_world::chunk_of(
            spawn[0].floor() as i32,
            (spawn[1] - 1.0).floor() as i32,
            spawn[2].floor() as i32,
        );
        let Output::Chunk { voxels, hash } = r.run(Job::Generate { coord }) else {
            panic!()
        };
        assert_eq!(voxels.len(), dwell_world::CHUNK_VOLUME);
        let chunk = Chunk::from_voxels(&voxels, 0).unwrap();
        assert_eq!(hash, chunk_hash(&chunk));
        assert!(voxels.iter().any(|&v| v != 0), "the spawn chunk has ground");
        let Output::Failed(_) = Runner::mesher().run(Job::Generate { coord }) else {
            panic!("a mesher has no world")
        };
    }

    #[test]
    fn lod_sections_carry_face_as_surface_where_face_a_has_rows() {
        let r = Runner::world(GENERATOR_TERRAIN, 1);
        let c = dwell_lod::grid::section_at(4, [0.0, 0.0, 0.0]);
        let Output::Lod { cells, surface, .. } = r.run(Job::Lod { coord: c }) else {
            panic!()
        };
        assert_eq!(cells.len(), lod::VOLUME);
        let s = surface.expect("face A's surface");
        assert_eq!(
            s.len(),
            (lod::PAD * lod::PAD) as usize * dwell_mesh::SURFACE_STRIDE
        );
        let flat = Runner::world(0, 1);
        let Output::Lod {
            surface: None,
            surface_b: None,
            ..
        } = flat.run(Job::Lod { coord: c })
        else {
            panic!("flat worlds have no surfaces")
        };
    }

    #[test]
    fn maps_heights_with_an_outside_flag() {
        let r = Runner::world(GENERATOR_TERRAIN, 1);
        let Output::Map(Some(bytes)) = r.run(Job::Map {
            x0: -64,
            z0: -64,
            step: 16,
            n: 8,
            face: 0,
        }) else {
            panic!()
        };
        assert_eq!(bytes.len(), 8 * 8 * 3);
        let Output::Map(None) = Runner::world(0, 1).run(Job::Map {
            x0: 0,
            z0: 0,
            step: 1,
            n: 4,
            face: 0,
        }) else {
            panic!()
        };
    }
}
