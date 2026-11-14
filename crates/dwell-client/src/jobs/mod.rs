//! Background jobs (ARCHITECTURE.md §5.1, ADR 0007): chunk generation, LOD bounds and sections,
//! the debug map, and chunk and section meshing. A [`Runner`] executes them (in a Web Worker with
//! its own WASM instance, or inline natively and in tests); [`codec`] carries jobs and results
//! across `postMessage` as bytes. The main thread's [`Jobs`] trait hides where they run.

pub mod codec;

use dwell_mesh::MeshSectionOptions;

use dwell_mesh::{ChunkMeshes, SectionMeshes};
use dwell_world::lod::{self, LodBounds, LodCoord, LodGenerator, LodSurfaces};
use dwell_world::{Chunk, ChunkCoord, ChunkGenerator, chunk_hash};
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
    /// The same of grid `grid` (its `grids.json` index), not home (`dwell_worldgen::grid_lod`,
    /// BUGS.md #156): the moon's and the sun's distant terrain.
    GridLod { grid: u8, coord: LodCoord },
    /// [`Job::LodBounds`] of grid `grid`.
    GridLodBounds {
        grid: u8,
        level: i32,
        i: i32,
        k: i32,
    },
    /// The F4 map: n × n samples of each column's height from (x0, z0), `step` apart; or, with
    /// `layer` above 0, the big map's debug layer of field `layer − 1`
    /// (`dwell_worldgen::map_layers`, REALISTIC_WORLD.md R6), its `LayerGrid::encode` bytes.
    Map {
        x0: i32,
        z0: i32,
        step: i32,
        n: i32,
        face: u8,
        layer: u8,
    },
    /// A chunk's greedy mesh from its padded voxels; `origin` is its minimum corner's world cell
    /// (plants are jittered by their world position).
    MeshChunk {
        padded: Vec<u16>,
        /// Its flood light with a one-cell apron (empty: open sky; docs/DAY_NIGHT.md §11).
        light: Vec<u8>,
        mirror: bool,
        origin: [i32; 3],
        /// Its cut wood cells, chunk-local (ADR 0067 §11): meshed carved.
        cuts: Vec<([i32; 3], dwell_protocol::chopping::Cut)>,
        /// Its damaged cells, chunk-local (docs/BLOCK_DAMAGE.md): drawn struck.
        damage: Vec<([i32; 3], dwell_protocol::damage::DamageView)>,
        /// Its ground items (ADR 0071): the world seed, its gone cells (chunk-local) and `padded`
        /// as generated (empty: as `padded`; the rules read the terrain as generated).
        ground: Option<(u64, Vec<[i32; 3]>, Vec<u16>)>,
        /// Liquid tops are meshed (false: the GPU heightfield draws them; `dwell_mesh::ChunkFlags`).
        water_tops: bool,
        /// Leaf cards are meshed (false on a CPU adapter; `dwell_mesh::ChunkFlags`).
        cards: bool,
        /// The share of cross plants meshed, percent (Graphics → Tall grass; `ChunkFlags::plants`).
        plants: u8,
    },
    /// An LOD section's meshes.
    MeshSection {
        cells: Vec<u16>,
        surface: Option<Vec<f32>>,
        water_drop: f64,
        mirror: bool,
        /// Its tree cells' leaf tints (ADR 0079; `dwell_mesh::mesh_section_tinted`).
        tints: Vec<(u32, dwell_world::tree_tint::Tint)>,
        /// Its trees are drawn (Graphics → Far trees; `dwell_mesh::mesh_section_trees`).
        trees: bool,
    },
    /// A strip of the LOD colour map (`crate::lod_map::raster`): world texels `x0..x0 + w` ×
    /// `z0..z0 + h` of `texel_m` m on face `face` (0 A, 1 B); with `heights`, each texel's height
    /// code after the colours (`crate::lod_map::raster_with_heights`).
    LodMap {
        face: u8,
        heights: bool,
        texel_m: i32,
        x0: i32,
        z0: i32,
        w: i32,
        h: i32,
    },
    /// A slab of the clouds' shape noise (docs/CLOUDS.md §9.3, `dwell_look::cloud_noise`): the
    /// detail texture's (else the base's) slices `z0..z1` at `size`³.
    CloudNoise {
        detail: bool,
        size: u32,
        z0: u32,
        z1: u32,
    },
    /// One baked far tree of the impostors' atlas (`dwell_worldgen::impostor::Config::bake_layer`,
    /// docs/proposals/TREE_IMPOSTORS.md): array layer `layer` at `px` texels a view.
    ImpostorBake { layer: u32, px: u32 },
    /// The species trees anchored in the square `x0..x0 + size` × `z0..z0 + size` of face `face`
    /// (0 A, 1 B), face-local (`TerrainGenerator::far_trees`): a tile of far tree impostors.
    FarTrees {
        face: u8,
        x0: i32,
        z0: i32,
        size: i32,
    },
    /// The block states the main thread registered at run time (synced from a server, ADR 0038),
    /// as `dwell_blocks::sync::extension_payload`: a worker adds them to its own table, so mesh
    /// jobs naming synced ids read the same rows. Sent to every worker ([`crate::pool::Jobs::broadcast`]).
    Registry { payload: Vec<u8> },
}

impl Job {
    /// Meshing jobs need no world; the others need the generator.
    pub fn is_mesh(&self) -> bool {
        matches!(self, Job::MeshChunk { .. } | Job::MeshSection { .. })
    }

    /// The column of terrain a generation job works on, as (level, x, z): a chunk's column (level
    /// −1) or an LOD section's; none for the others. The workers that generate keep the columns
    /// they work out, so a column's jobs go to one of them (`dwell_world::affinity`).
    pub fn column(&self) -> Option<(i32, i32, i32)> {
        match *self {
            Job::Generate { coord } => Some((-1, coord.x, coord.z)),
            Job::Lod { coord } => Some((coord.level, coord.i, coord.k)),
            Job::LodBounds { level, i, k } => Some((level, i, k)),
            Job::GridLod { coord, .. } => Some((coord.level, coord.i, coord.k)),
            Job::GridLodBounds { level, i, k, .. } => Some((level, i, k)),
            _ => None,
        }
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
        /// Its tree cells' leaf tints (ADR 0079; `BifacialTerrain::lod_tints`).
        tints: Vec<(u32, dwell_world::tree_tint::Tint)>,
    },
    /// Height (i16 little-endian) and an outside flag per sample; `None` without a terrain.
    Map(Option<Vec<u8>>),
    /// A colour map strip's rgba8 sRGB texels, rows of x; `None` without a terrain.
    LodMap(Option<Vec<u8>>),
    /// A cloud-noise slab's RGBA8 texels (x, then y, then z).
    CloudNoise(Vec<u8>),
    /// A baked impostor layer: the bounding sphere's centre height over the foot and radius (m),
    /// the albedo and the normal + depth atlases (RGBA8 rows); empty past the layers.
    Impostor {
        centre_y: f32,
        radius: f32,
        albedo: Vec<u8>,
        normal_depth: Vec<u8>,
    },
    /// A tile's far trees (`dwell_worldgen::impostor::FarTree`); empty without a terrain.
    FarTrees(Vec<dwell_worldgen::impostor::FarTree>),
    ChunkMesh(Box<ChunkMeshes>),
    SectionMesh(Box<SectionMeshes>),
    Failed(String),
    /// A job without a result (`Registry`).
    Done,
}

/// Runs jobs for one world (a worker's state).
pub struct Runner {
    chunks: Option<ChunkGenerator>,
    terrain: Option<std::sync::Arc<BifacialTerrain>>,
    lod: Option<LodGenerator>,
    bounds: Option<lod::LodBoundsFn>,
    /// The world's disc and build volume (BIG_MAP.md M2b: its size), for the LOD's in-world test.
    world: dwell_world::WorldBounds,
    /// The world's seed (the grids' generators derive theirs from it), `None` for a mesher.
    seed: Option<u64>,
    /// The other grids' generators, built at their first job.
    grids: std::sync::Mutex<GridGenerators>,
}

type GridGenerators = dwell_world::hash::HashMap<
    u8,
    Option<std::sync::Arc<dyn dwell_worldgen::grid_lod::GridColumns>>,
>;

impl Runner {
    /// A runner for meshing only.
    pub fn mesher() -> Self {
        Self {
            chunks: None,
            terrain: None,
            lod: None,
            bounds: None,
            world: dwell_world::WorldBounds::DEFAULT,
            seed: None,
            grids: Default::default(),
        }
    }

    /// A runner for a world (its generator version and seed, from Welcome), meshing included.
    pub fn world(generator_version: u32, world_seed: u64) -> Self {
        Self::world_with(generator_version, world_seed, None)
    }

    /// [`Runner::world`] with the LOD colour map's river valleys (`?lodmap=on`: their margin under
    /// the river's level, m; ARCHITECTURE.md §6.6 "Colour map") in its LOD sections and bounds.
    pub fn world_with(generator_version: u32, world_seed: u64, valleys: Option<f64>) -> Self {
        Self::world_sized(
            generator_version,
            world_seed,
            dwell_protocol::world_size::WorldSize::DEFAULT,
            valleys,
        )
    }

    /// [`Runner::world_with`] in a world of `size` (BIG_MAP.md M2b: the join's `WorldSettings`
    /// size): its generators, LOD and bounds over its own map, disc and build volume.
    pub fn world_sized(
        generator_version: u32,
        world_seed: u64,
        size: dwell_protocol::world_size::WorldSize,
        valleys: Option<f64>,
    ) -> Self {
        let terrain = BifacialTerrain::for_generator_sized(generator_version, world_seed, size)
            .map(|mut t| {
                t.set_lod_valleys(valleys);
                std::sync::Arc::new(t)
            });
        let bounds: lod::LodBoundsFn = match (&terrain, valleys) {
            // The bounds see the same lowered columns the sections are generated from.
            (Some(t), Some(_)) => {
                let t = t.clone();
                std::sync::Arc::new(move |level, i, k| t.lod_bounds_at(level, i, k))
            }
            _ => dwell_worldgen::lod_bounds_for_sized(generator_version, world_seed, size),
        };
        Self {
            chunks: Some(dwell_worldgen::generator_for_sized(
                generator_version,
                world_seed,
                size,
            )),
            terrain,
            lod: Some(dwell_worldgen::lod_generator_for_sized(
                generator_version,
                world_seed,
                size,
            )),
            bounds: Some(bounds),
            world: dwell_world::WorldBounds::of(size),
            seed: Some(world_seed),
            grids: Default::default(),
        }
    }

    pub fn run(&self, job: Job) -> Output {
        match job {
            Job::Registry { payload } => match dwell_blocks::sync::apply_extension(&payload) {
                Ok(()) => Output::Done,
                Err(e) => Output::Failed(e),
            },
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
            Job::GridLod { grid, coord } => self.grid_lod(grid, coord),
            Job::GridLodBounds { grid, level, i, k } => match self.grid(grid) {
                Some(g) => {
                    Output::Bounds(dwell_worldgen::grid_lod::grid_lod_bounds(&*g, level, i, k))
                }
                None => no_world(),
            },
            Job::Map {
                x0,
                z0,
                step,
                n,
                face,
                layer,
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
                if layer > 0 {
                    use dwell_worldgen::map_layers::{MapField, sample};
                    return Output::Map(MapField::of_index(layer - 1).and_then(|f| {
                        sample(
                            local,
                            f,
                            i64::from(x0),
                            i64::from(z0),
                            i64::from(step),
                            n as usize,
                        )
                        .map(|g| g.encode())
                    }));
                }
                let mut out = Vec::with_capacity((n * n * 3) as usize);
                for j in 0..n {
                    for k in 0..n {
                        let c = local.column_at(x0 + k * step, z0 + j * step);
                        // Above sea level (the map's colours): heights count from the midplane.
                        let h = (c.height - dwell_world::SEA_LEVEL_M as f32)
                            .clamp(-32768.0, 32767.0) as i16;
                        out.extend_from_slice(&h.to_le_bytes());
                        out.push(u8::from(c.outside));
                    }
                }
                Output::Map(Some(out))
            }
            Job::CloudNoise {
                detail,
                size,
                z0,
                z1,
            } => {
                // A size the data allows (a power of two the generator checked), else nothing.
                if !size.is_power_of_two() || !(8..=256).contains(&size) || z1 <= z0 {
                    return Output::CloudNoise(Vec::new());
                }
                let slab = if detail {
                    dwell_look::cloud_noise::bake_detail_slab(size, z0, z1)
                } else {
                    dwell_look::cloud_noise::bake_base_slab(size, z0, z1)
                };
                Output::CloudNoise(slab.into_iter().flatten().collect())
            }
            Job::ImpostorBake { layer, px } => {
                let config = dwell_worldgen::impostor::Config::get();
                match config.bake_layer(layer, px.clamp(4, 256)) {
                    Some(b) => Output::Impostor {
                        centre_y: b.centre[1],
                        radius: b.radius,
                        albedo: b.albedo.into_iter().flatten().collect(),
                        normal_depth: b.normal_depth.into_iter().flatten().collect(),
                    },
                    None => Output::Impostor {
                        centre_y: 0.0,
                        radius: 0.0,
                        albedo: Vec::new(),
                        normal_depth: Vec::new(),
                    },
                }
            }
            Job::FarTrees { face, x0, z0, size } => match &self.terrain {
                Some(t) if size > 0 => {
                    let face = if face == 1 {
                        dwell_world::Face::B
                    } else {
                        dwell_world::Face::A
                    };
                    Output::FarTrees(
                        t.local(face)
                            .far_trees((x0, z0), (x0 + size - 1, z0 + size - 1)),
                    )
                }
                _ => Output::FarTrees(Vec::new()),
            },
            Job::LodMap {
                face,
                heights,
                texel_m,
                x0,
                z0,
                w,
                h,
            } => {
                let Some(t) = &self.terrain else {
                    return Output::LodMap(None);
                };
                if w <= 0 || h <= 0 || i64::from(w) * i64::from(h) > 1 << 22 || texel_m <= 0 {
                    return Output::LodMap(None);
                }
                let local = t.local(if face == 1 {
                    dwell_world::Face::B
                } else {
                    dwell_world::Face::A
                });
                let rect = crate::lod_map::Rect {
                    x0: i64::from(x0),
                    z0: i64::from(z0),
                    x1: i64::from(x0) + i64::from(w),
                    z1: i64::from(z0) + i64::from(h),
                };
                let raster = if heights {
                    crate::lod_map::raster_with_heights
                } else {
                    crate::lod_map::raster
                };
                Output::LodMap(Some(raster(
                    local,
                    &dwell_look::lod_map::LOD_MAP,
                    i64::from(texel_m),
                    &rect,
                )))
            }
            Job::MeshChunk {
                padded,
                light,
                mirror,
                origin,
                cuts,
                damage,
                ground,
                water_tops,
                cards,
                plants,
            } => match dwell_mesh::mesh_chunk_trees(
                &padded,
                &light,
                mirror,
                origin,
                &cuts,
                ground
                    .map(|(seed, gone, generated)| dwell_mesh::GroundJob {
                        seed,
                        gone,
                        generated,
                        // The chunk's biomes, from the generator this worker runs (ADR 0074 B5).
                        density: self.ground_density(origin),
                    })
                    .as_ref(),
                dwell_mesh::ChunkFlags {
                    water_tops,
                    cards,
                    plants,
                },
                &damage,
                // Its species trees' leaf tints, from the generator this worker runs (ADR 0079).
                &self.tree_tints(origin, &padded),
            ) {
                Ok(m) => Output::ChunkMesh(Box::new(m)),
                Err(e) => Output::Failed(format!("{e:?}")),
            },
            Job::MeshSection {
                cells,
                surface,
                water_drop,
                mirror,
                tints,
                trees,
            } => {
                let options = MeshSectionOptions {
                    surface: surface.as_deref(),
                    water_drop,
                    mirror,
                };
                match dwell_mesh::mesh_section_trees(&cells, &options, &tints, trees) {
                    Ok(m) => Output::SectionMesh(Box::new(m)),
                    Err(e) => Output::Failed(format!("{e:?}")),
                }
            }
        }
    }

    /// The ground densities of the chunk at `origin` (world cells) from its face's biomes (ADR
    /// 0074 B5): the server's and the crosshair's, so all agree; empty with no terrain.
    fn ground_density(&self, origin: [i32; 3]) -> dwell_world::ground::GroundDensity {
        self.terrain.as_ref().map_or_else(Default::default, |t| {
            t.ground_density(dwell_world::ChunkCoord::from_array(
                origin.map(|v| v.div_euclid(dwell_world::CHUNK_SIZE)),
            ))
        })
    }

    /// The species trees reaching the chunk at `origin` (world cells) with their leaf tints, in the
    /// frame the chunk is meshed in (ADR 0079): none without terrain or leaves in `padded`.
    fn tree_tints(
        &self,
        origin: [i32; 3],
        padded: &[u16],
    ) -> Vec<dwell_world::tree_tint::TreeTint> {
        match &self.terrain {
            Some(t) if dwell_mesh::has_leaves(padded) => {
                t.tree_tints(dwell_world::ChunkCoord::from_array(
                    origin.map(|v| v.div_euclid(dwell_world::CHUNK_SIZE)),
                ))
            }
            _ => Vec::new(),
        }
    }

    /// Grid `grid`'s generator (`None` for home, a grid without columns or a mesher).
    fn grid(&self, grid: u8) -> Option<std::sync::Arc<dyn dwell_worldgen::grid_lod::GridColumns>> {
        let seed = self.seed?;
        let mut held = self.grids.lock().unwrap_or_else(|e| e.into_inner());
        held.entry(grid)
            .or_insert_with(|| {
                let def = dwell_worldgen::grids::grids().get(usize::from(grid))?;
                dwell_worldgen::grid_lod::grid_columns(def, seed)
            })
            .clone()
    }

    /// A section of grid `grid` (`Job::GridLod`): its cells and both faces' column surfaces.
    fn grid_lod(&self, grid: u8, c: LodCoord) -> Output {
        let Some(g) = self.grid(grid) else {
            return no_world();
        };
        let world = dwell_worldgen::grid_lod::grid_world(&*g);
        let mut cells = Vec::new();
        let (mut a, mut b) = (LodSurfaces::new(), LodSurfaces::new());
        dwell_worldgen::grid_lod::generate_grid_lod(
            &*g,
            &world,
            c,
            &mut cells,
            Some(&mut a),
            Some(&mut b),
        );
        let first = lod::first_face_a_row(c);
        let in_world = lod::in_world_of(c, &world);
        let pack = |s: &LodSurfaces, ran: bool| {
            (ran && in_world && !s.is_empty()).then(|| surface_floats(s))
        };
        Output::Lod {
            surface: pack(&a, first <= lod::SECTION_CELLS),
            surface_b: pack(&b, first >= 0),
            cells,
            tints: Vec::new(),
        }
    }

    fn lod(&self, c: LodCoord) -> Output {
        let mut cells = Vec::new();
        if let Some(t) = &self.terrain {
            let (mut a, mut b) = (LodSurfaces::new(), LodSurfaces::new());
            t.generate_lod(c, &mut cells, Some(&mut a), Some(&mut b));
            // Which faces' passes ran: face A's rows start at `first`, face B's end below it.
            let first = lod::first_face_a_row(c);
            let in_world = lod::in_world_of(c, &self.world);
            let pack = |s: &LodSurfaces, ran: bool| {
                (ran && in_world && !s.is_empty()).then(|| surface_floats(s))
            };
            let tints = t.lod_tints(c, &cells);
            return Output::Lod {
                cells,
                surface: pack(&a, first <= lod::SECTION_CELLS),
                surface_b: pack(&b, first >= 0),
                tints,
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
            tints: Vec::new(),
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

    #[test]
    fn a_generation_job_names_its_column_and_meshing_none() {
        // face-a-only: columns have no up.
        let c = dwell_world::ChunkCoord::new(3, -2, 9);
        assert_eq!(Job::Generate { coord: c }.column(), Some((-1, 3, 9)));
        let l = dwell_world::lod::LodCoord::new(2, 5, 1, -4);
        assert_eq!(Job::Lod { coord: l }.column(), Some((2, 5, -4)));
        assert_eq!(
            Job::LodBounds {
                level: 2,
                i: 5,
                k: -4
            }
            .column(),
            Some((2, 5, -4))
        );
        let mesh = Job::MeshSection {
            cells: Vec::new(),
            surface: None,
            water_drop: 0.0,
            mirror: false,
            tints: Vec::new(),
            trees: true,
        };
        assert_eq!(mesh.column(), None);
    }

    use super::*;
    use dwell_world::GENERATOR_TERRAIN;

    #[test]
    fn generates_hashes_and_meshes_a_chunk() {
        let r = Runner::world(GENERATOR_TERRAIN, 1);
        let spawn = dwell_worldgen::spawn_point_for(GENERATOR_TERRAIN, 1);
        // The chunk of the cell under the feet.
        let coord = dwell_world::chunk_of_pos([spawn[0], spawn[1] - dwell_world::CELL_H, spawn[2]]);
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
    fn maps_a_debug_layer_by_field() {
        // face-a-only: the job only picks the face's generator (map_layers.rs tests the fields).
        let r = Runner::world(GENERATOR_TERRAIN, 1);
        let rocks = 1 + dwell_worldgen::map_layers::MapField::Rocks.index();
        let Output::Map(Some(bytes)) = r.run(Job::Map {
            x0: -4000,
            z0: -4000,
            step: 1000,
            n: 8,
            face: 0,
            layer: rocks,
        }) else {
            panic!("the default world has rocks")
        };
        let names = usize::from(bytes[0]);
        assert!(names > 0);
        let mut o = 1;
        for _ in 0..names {
            o += 1 + usize::from(bytes[o]);
        }
        o += 4 * names; // their colours
        assert_eq!(u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap()), 8);
        assert_eq!(bytes.len(), o + 4 + 8 * 8 * 6);
        // No plates in the default world: nothing.
        let plates = 1 + dwell_worldgen::map_layers::MapField::Plates.index();
        let Output::Map(None) = r.run(Job::Map {
            x0: 0,
            z0: 0,
            step: 1000,
            n: 4,
            face: 0,
            layer: plates,
        }) else {
            panic!("no plates in the default world")
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
            layer: 0,
        }) else {
            panic!()
        };
        assert_eq!(bytes.len(), 8 * 8 * 3);
        // Heights above sea level (the map's colours: sea below 0), not face-local ones: the
        // generator's heights count from the midplane since Phase 16 (sea level 32).
        let t = dwell_worldgen::BifacialTerrain::new(1);
        let want = t.local(dwell_world::Face::A).column_at(-64, -64).height
            - dwell_world::SEA_LEVEL_M as f32;
        assert_eq!(i16::from_le_bytes([bytes[0], bytes[1]]), want as i16);
        let Output::Map(None) = Runner::world(0, 1).run(Job::Map {
            x0: 0,
            z0: 0,
            step: 1,
            n: 4,
            face: 0,
            layer: 0,
        }) else {
            panic!()
        };
    }
}
