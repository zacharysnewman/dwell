//! The distant terrain of a grid while standing on it (docs/proposals/MOON_WORLD.md §7.3; BUGS.md
//! #143, #145, #156): the moon and the sun are drawn past their streamed chunks by an LOD tree of
//! their own, home's [`LodSystem`] (its pixel-error selection, graded levels, cross-fades and
//! stand-ins) over the grid's disc, its sections generated in the job workers from the grid's own
//! generator (`Job::GridLod`, `dwell_worldgen::grid_lod`) and meshed by the same LOD mesher as
//! home's, so they take the same colours from the block looks as the chunks next to them. A grid
//! has one when its `grids.json` entry says `lod`: no per-grid code.
//!
//! One instance at a time, of the grid the view is on: made on the first frame there, dropped
//! (its meshes off the view) at every crossing (#879's rule); every job carries the instance's
//! epoch, so a result for an instance gone is ignored. Its sections share the renderer's section
//! store with home's (whose are dropped while away), under ids with [`ID_BIT`] set.

use dwell_lod::fade::Fade;
use dwell_lod::{
    ChunkReadiness, LodMeshJob, LodNetwork, LodOptions, LodSystem, LodView, SectionMesher,
    SectionSource, StandInBox,
};
use dwell_world::lod::LodCoord;
use dwell_world::{ChunkCoord, WorldBounds};

/// The bit a grid's section ids carry in the renderer, apart from home's (`dwell_lod::grid::lod_id`
/// stays far under it).
pub const ID_BIT: i64 = 1 << 60;

/// The LOD tree of the grid the view is on.
pub struct GridLod {
    grid: u8,
    epoch: u64,
    pub lod: LodSystem,
}

impl GridLod {
    /// Grid `grid`'s (its `grids.json` index) with home's options and full-detail distance, as
    /// instance `epoch`; `None` for home or a grid without `lod`.
    pub fn for_grid(
        grid: u8,
        epoch: u64,
        options: LodOptions,
        detail_m: Option<f64>,
    ) -> Option<Self> {
        let def = dwell_worldgen::grids::grids()
            .get(usize::from(grid))
            .filter(|d| grid != 0 && d.lod)?;
        let mut options = options;
        // A grid has no server index or far chunks to ask for: every section is generated here.
        options.request_chunks = false;
        let mut lod = LodSystem::new(options);
        lod.set_detail_distance(detail_m);
        let radius = def.radius_m?;
        lod.set_world(WorldBounds::grid(radius.ceil() as i64));
        // Home's reach past the rim, scaled to the grid's disc.
        lod.set_rim_view(dwell_lod::RIM_VIEW_M * radius / WorldBounds::DEFAULT.radius() as f64);
        Some(Self { grid, epoch, lod })
    }

    pub fn grid(&self) -> u8 {
        self.grid
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
}

/// Where a grid's LOD jobs go (the job pool; a fake in tests). Each carries the instance's epoch.
pub trait GridJobs {
    fn bounds(&mut self, grid: u8, epoch: u64, level: i32, i: i32, k: i32);
    fn section(&mut self, grid: u8, epoch: u64, c: LodCoord, token: u64);
    fn mesh(&mut self, epoch: u64, job: LodMeshJob, token: u64);
}

/// The renderer seen by a grid's LOD: its ids with [`ID_BIT`] set.
pub struct GridView<'a, P>(pub &'a mut dyn LodView<P>);

impl<P> LodView<P> for GridView<'_, P> {
    fn set_lod_section(&mut self, id: i64, origin: [f64; 3], cell_size: f64, meshes: Option<P>) {
        self.0
            .set_lod_section(id | ID_BIT, origin, cell_size, meshes);
    }
    fn show_lod_sections(&mut self, visible: &[(i64, u8, Fade)]) {
        let v: Vec<_> = visible
            .iter()
            .map(|&(id, s, f)| (id | ID_BIT, s, f))
            .collect();
        self.0.show_lod_sections(&v);
    }
    fn show_lod_stand_ins(&mut self, stand_ins: &[StandInBox]) {
        let v: Vec<_> = stand_ins
            .iter()
            .map(|b| StandInBox {
                id: b.id | ID_BIT,
                ..*b
            })
            .collect();
        self.0.show_lod_stand_ins(&v);
    }
}

/// A grid LOD's collaborators for one update: its jobs, the renderer and the grid's chunks.
pub struct Host<'a, P> {
    pub grid: u8,
    pub epoch: u64,
    pub jobs: &'a mut dyn GridJobs,
    pub view: GridView<'a, P>,
    pub chunks: &'a dyn ChunkReadiness,
}

impl<P> SectionSource for Host<'_, P> {
    fn lod_bounds(&mut self, level: i32, i: i32, k: i32) {
        self.jobs.bounds(self.grid, self.epoch, level, i, k);
    }
    fn lod(&mut self, c: LodCoord, token: u64) {
        self.jobs.section(self.grid, self.epoch, c, token);
    }
}
impl<P> SectionMesher for Host<'_, P> {
    fn mesh_section(&mut self, job: LodMeshJob, token: u64) {
        self.jobs.mesh(self.epoch, job, token);
    }
}
impl<P> LodView<P> for Host<'_, P> {
    fn set_lod_section(&mut self, id: i64, origin: [f64; 3], cell_size: f64, meshes: Option<P>) {
        self.view.set_lod_section(id, origin, cell_size, meshes);
    }
    fn show_lod_sections(&mut self, visible: &[(i64, u8, Fade)]) {
        self.view.show_lod_sections(visible);
    }
    fn show_lod_stand_ins(&mut self, stand_ins: &[StandInBox]) {
        self.view.show_lod_stand_ins(stand_ins);
    }
}
impl<P> ChunkReadiness for Host<'_, P> {
    fn chunk_drawable(&self, coord: ChunkCoord) -> bool {
        self.chunks.chunk_drawable(coord)
    }
    fn chunk_loaded(&self, coord: ChunkCoord) -> bool {
        self.chunks.chunk_loaded(coord)
    }
    fn drawable_revision(&self) -> Option<u64> {
        self.chunks.drawable_revision()
    }
}
/// A grid has no LOD index and its far chunks aren't asked for: nothing goes to the server.
impl<P> LodNetwork for Host<'_, P> {
    fn request_sections(&mut self, _: Vec<dwell_protocol::LodSectionRequest>) {}
    fn request_chunks(&mut self, _: Vec<dwell_protocol::ChunkCoordNet>) {}
}
