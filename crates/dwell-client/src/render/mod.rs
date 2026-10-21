//! Dwell's wgpu renderer (ARCHITECTURE.md §5, ADR 0028; WebGPU only): terrain chunks, LOD sections
//! with their skirts and stand-ins, players, the block outline, debug lines, the sky, height fog
//! and tone mapping. The scene draws in two passes split at LOD_NEAR_SPLIT_M (§6.6) into an
//! offscreen frame (`frame.rs`: linear colour and view depth); a composite pass tone-maps, fogs and
//! puts the sky behind it into the canvas, and the lines draw over that. Everything is drawn
//! relative to the camera (origins in f64, offsets in f32), so the world stays precise far from
//! the origin. It is the Three.js renderer's interface (render/Renderer.ts) in Rust.

pub mod arena;
pub mod camera;
pub mod characters;
pub mod display;
pub mod face_shading;
pub mod frame;
pub mod geometry;
pub mod pacing;
pub mod reflection;
pub mod seam;
pub mod shaders;
pub mod timing;
pub mod water;
pub mod water_tiles;

use dwell_world::hash::HashMap;

use dwell_game::brush_tool::{BrushOutline, GhostFace, GhostKind};
use dwell_look::clouds::{CLOUDS, CloudQuality, cloud_uniforms};
use dwell_look::daynight::{DAY_SKY_FLOATS, DayState};
use dwell_look::fog::{DEFAULT_FOG, FogSettings, fog_sigma};
use dwell_look::motion::{MOTION, vanishing_point};
use dwell_look::outline::{OUTLINE, OutlineMode, outline_uniforms};
use dwell_look::reflections::{REFLECTIONS, reflection_uniforms};
use dwell_look::{DEFAULT_EXPOSURE, hex, sanitize_exposure, srgb_to_linear};
use dwell_mesh::{ChunkMeshes, FlatMesh, MeshArrays, SectionMeshes};
use dwell_world::fluid::EyeFluid;
use dwell_world::{CELL_H, CHUNK_EXTENT_M, ChunkCoord, FaceFrame, chunk_of, chunk_origin_m};

use arena::{Arena, ArenaRef, Region};
use camera::{Basis, V3, aim, mul, perspective, vertical_fov, view_matrix};
pub use display::RenderStats;
use frame::{DEPTH, HDR, Targets, VIEW_DEPTH};
use seam::TerrainRenderer;
use timing::GpuTimer;
use water::WaterQuality;
use water_tiles::WaterTiles;

/// Beyond this the far pass draws (§6.6); the near pass's far plane.
pub const NEAR_SPLIT: f64 = dwell_protocol::lod::NEAR_SPLIT_M as f64;
pub const NEAR_PLANE: f64 = 0.05;
pub const FAR_PLANE: f64 = 5e7;
/// Face-local height no terrain reaches (m): above it, nothing is nearer than the altitude over it,
/// so the far pass's near plane moves out with it.
const TERRAIN_TOP_M: f64 = 1024.0;
pub const SAMPLES: u32 = 4;

// A CPU (software) adapter — SwiftShader: CI, machines without a GPU — draws the cheapest tier of
// everything, whatever the settings: there every pixel and sample is the CPU's time, shared with
// the game and its workers, and a frame of the full look takes whole seconds (CI's e2e suite timed
// out). The functions below are that policy, one per cost; a GPU keeps the settings.

fn is_cpu(device_type: wgpu::DeviceType) -> bool {
    device_type == wgpu::DeviceType::Cpu
}

/// Multisampling for an adapter: 4× on a GPU; none on a CPU adapter.
pub fn samples_for(device_type: wgpu::DeviceType) -> u32 {
    if is_cpu(device_type) { 1 } else { SAMPLES }
}

/// The clouds a frame draws with the Clouds setting `quality`: none on a CPU adapter (the deck's
/// per-pixel noise).
pub fn clouds_for(device_type: wgpu::DeviceType, quality: CloudQuality) -> CloudQuality {
    if is_cpu(device_type) {
        CloudQuality::Off
    } else {
        quality
    }
}

/// The water a frame draws with the Water setting `quality`: Low on a CPU adapter (one layer of
/// ripples).
pub fn water_for(device_type: wgpu::DeviceType, quality: WaterQuality) -> WaterQuality {
    if is_cpu(device_type) {
        WaterQuality::Low
    } else {
        quality
    }
}

/// Whether the wind's edge blur draws (four more loads of the finished image per pixel): not on a
/// CPU adapter. The streaks and vignette still do.
pub fn motion_blur_for(device_type: wgpu::DeviceType) -> bool {
    !is_cpu(device_type)
}

/// Whether this frame draws the wind (docs/MOTION_EFFECTS.md): then the composite writes the
/// finished image (`finished_fs`) and the motion pass (`motion_fs`) reads it, blurs it and draws
/// the streaks over it into the canvas; otherwise the composite draws into the canvas
/// (`composite_fs`). The shader's own test of the intensity (`frame.wind_point.z`) is the same.
pub fn motion_pass(wind_intensity: f64) -> bool {
    wind_intensity > 1e-3
}

/// Whether the night sky draws its stars (a hashed grid per pixel): not on a CPU adapter.
pub fn stars_for(device_type: wgpu::DeviceType) -> bool {
    !is_cpu(device_type)
}

/// The stars' visibility in the day sky block (`DaySky.stars.x`, dwell-look's `pack`: the ninth
/// vec4).
const DAY_STARS: usize = 8 * 4;

/// The drawing buffer's share of the screen's pixels on a CPU adapter (each axis): the frame's
/// cost there is per pixel (measured on SwiftShader: 0.75 frames/s at full size, 1.5 at half).
pub const CPU_RESOLUTION: f64 = 0.5;

/// The drawing buffer's pixels per CSS pixel on an adapter, for a screen of `pixel_ratio`
/// (the device's, capped, times `?scale=`): that ratio on a GPU, `CPU_RESOLUTION` of it on a CPU.
pub fn pixel_ratio_for(device_type: wgpu::DeviceType, pixel_ratio: f64) -> f64 {
    if is_cpu(device_type) {
        pixel_ratio * CPU_RESOLUTION
    } else {
        pixel_ratio
    }
}

/// The frame uniform's `water` vec4 (shaders.rs `Frame`): x 1 when the eye is under a fluid's
/// drawn surface, y its fluid id, z the surface's world y (zeros when it isn't); w the water
/// quality.
fn water_uniform(eye_fluid: Option<EyeFluid>, quality: WaterQuality) -> [f32; 4] {
    match eye_fluid {
        Some(e) => [1.0, f32::from(e.fluid), e.surface_y as f32, quality.level()],
        None => [0.0, 0.0, 0.0, quality.level()],
    }
}
/// Draw uniforms live at this stride (WebGPU's minimum uniform offset alignment).
/// Bytes per draw in the draws storage array (`Draw` in the shaders: five vec4f).
const DRAW_STRIDE: u64 = 80;
const MAX_STAND_INS: usize = 64;
/// Debug tints of LOD levels (setLodLevelColors), sRGB.
const LEVEL_TINTS: [u32; 20] = [
    0xffffff, 0xff6060, 0xffb060, 0xffff60, 0x80ff60, 0x60ffd0, 0x60a0ff, 0x9060ff, 0xff60e0,
    0xff8080, 0xffd080, 0xffff90, 0xa0ff90, 0x90ffe0, 0x90c0ff, 0xb090ff, 0xff90f0, 0xc0c0c0,
    0x808080, 0x404040,
];

/// A player drawn by the renderer as the full body (docs/CHARACTERS.md §9.2): remote players,
/// and the local one when dead. The game animates it (`dwell_characters::animator`); the renderer
/// draws the pose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerView {
    /// Feet position.
    pub feet: V3,
    /// The face the player stands on: face B draws it upside down.
    pub face: FaceFrame,
    /// Degrees; 0 = +Z.
    pub yaw: f64,
    /// Lying on the ground (cosmetic death pose).
    pub dead: bool,
    /// The look it wears (a variant of the player character's data; `DEFAULT_LOOK` for everyone
    /// until looks are chosen, CHARACTERS.md §9.4).
    pub look: &'static str,
    /// The bones' rotations this frame.
    pub pose: dwell_characters::anim::Pose,
    /// The base role playing (`walk`, `sprint`: telemetry and tests).
    pub role: &'static str,
    /// The swing playing, if any (`break`, `place`).
    pub action: Option<&'static str>,
    pub held: Held,
}

/// Your own first-person arms (docs/CHARACTERS.md §5), drawn in their own pass; none while dead,
/// zoomed in the spyglass, or hidden in the settings.
pub type ArmsView = characters::ArmsDraw;
pub use characters::Held;

/// The wind to draw this frame (docs/MOTION_EFFECTS.md): none when `intensity` is 0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WindView {
    /// 0–1.
    pub intensity: f64,
    /// The flight layer, 0–1.
    pub flight: f64,
    /// Unit world direction of travel.
    pub direction: V3,
    /// Seconds of animation.
    pub time: f64,
}

/// A debug line segment, sRGB colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub from: V3,
    pub to: V3,
    pub color: u32,
}

/// A box of cells outlined over everything (the region tools' selection and paste ghost, §6.5):
/// its min corner and size in metres, and an sRGB colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RegionBox {
    pub min: V3,
    pub size: V3,
    pub color: u32,
}

/// An ancestor section drawn clipped to a box (world metres) where the traversal has nothing ready.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StandIn {
    pub id: i64,
    pub lo: V3,
    pub hi: V3,
}

/// A mesh in the arena's pages (arena.rs): its vertices with their attributes interleaved (one
/// format per `Kind`) and its indices, drawn from `first_index` with `base_vertex`.
struct GpuMesh {
    region: Region,
    base_vertex: i32,
    first_index: u32,
    /// Index ranges: [0] the surface; for LOD sections, [1 + face] each side's skirt.
    parts: Vec<std::ops::Range<u32>>,
    bytes: u64,
    arena: ArenaRef,
}

/// Bytes per vertex of each `Kind`'s interleaved format (the pipelines' vertex layouts), in the
/// enum's order: `Kind as usize` indexes it.
const STRIDES: [u64; 6] = [
    4 * (3 + 3 + 3 + 2 + 4), // Terrain: position, normal, colour, uv, tile
    4 * (3 + 3 + 3),         // Flat: position, normal, colour
    4 * (3 + 3),             // Line: position, colour
    4 * (3 + 3),             // Ghost: the lines' format (triangles)
    4 * (3 + 3 + 3 + 1),     // Fluid: position, normal, colour, fluid id
    4 * (3 + 3 + 3 + 2 + 4 + 4 + 1), // Cutout: the terrain's, plant (sway, wind speed, fade), bare
];

fn floats(v: &[f32]) -> &[u8] {
    bytemuck_cast(v)
}
fn u32s(v: &[u32]) -> &[u8] {
    bytemuck_cast(v)
}
/// Plain-old-data as bytes (f32 and u32 slices only; `unsafe` is forbidden, so this copies through
/// a safe path when the platform's byte order is the GPU's, little-endian everywhere we run).
fn bytemuck_cast<T: Copy + ToLeBytes>(v: &[T]) -> &[u8] {
    T::as_bytes(v)
}
trait ToLeBytes: Sized {
    fn as_bytes(v: &[Self]) -> &[u8];
}
impl ToLeBytes for f32 {
    fn as_bytes(v: &[Self]) -> &[u8] {
        bytemuck::cast_slice(v)
    }
}
impl ToLeBytes for u32 {
    fn as_bytes(v: &[Self]) -> &[u8] {
        bytemuck::cast_slice(v)
    }
}

/// Index ranges with the empty ones dropped and touching ones joined: a section's surface and the
/// skirts drawn with it are consecutive in its index buffer, so most draw as one call.
fn merge_ranges(ranges: Vec<std::ops::Range<u32>>) -> Vec<std::ops::Range<u32>> {
    let mut out: Vec<std::ops::Range<u32>> = Vec::with_capacity(ranges.len());
    for r in ranges.into_iter().filter(|r| !r.is_empty()) {
        match out.last_mut() {
            Some(last) if last.end == r.start => last.end = r.end,
            _ => out.push(r),
        }
    }
    out
}

/// Streams of `n` floats per vertex, interleaved vertex by vertex (in the pipelines' attribute
/// order, `vertex_layout`).
fn interleave(streams: &[(&[f32], usize)]) -> Vec<f32> {
    let count = streams.first().map_or(0, |(s, n)| s.len() / n);
    let stride: usize = streams.iter().map(|(_, n)| n).sum();
    let mut out = Vec::with_capacity(count * stride);
    for v in 0..count {
        for (s, n) in streams {
            out.extend_from_slice(&s[v * n..(v + 1) * n]);
        }
    }
    out
}

impl GpuMesh {
    fn upload(
        arena: &ArenaRef,
        kind: Kind,
        vertices: &[f32],
        indices: &[u32],
        parts: Vec<std::ops::Range<u32>>,
        bytes: u64,
    ) -> Self {
        let mut a = arena.borrow_mut();
        let region = a.upload(kind as usize, floats(vertices), u32s(indices));
        Self {
            base_vertex: a.base_vertex(&region),
            first_index: (region.index_offset / 4) as u32,
            region,
            parts,
            bytes,
            arena: arena.clone(),
        }
    }

    fn textured(arena: &ArenaRef, m: &MeshArrays) -> Option<Self> {
        Self::textured_as(arena, m, Kind::Terrain)
    }

    /// A chunk's cutout mesh: the terrain format and its `plants` stream.
    fn cutout(arena: &ArenaRef, m: &MeshArrays) -> Option<Self> {
        Self::textured_as(arena, m, Kind::Cutout)
    }

    fn textured_as(arena: &ArenaRef, m: &MeshArrays, kind: Kind) -> Option<Self> {
        if m.indices.is_empty() {
            return None;
        }
        let mut streams: Vec<(&[f32], usize)> = vec![
            (&m.positions, 3),
            (&m.normals, 3),
            (&m.colors, 3),
            (&m.uvs, 2),
            (&m.tiles, 4),
        ];
        if kind == Kind::Cutout {
            streams.push((&m.plants, 4));
            streams.push((&m.bare, 1));
        }
        let vertices = interleave(&streams);
        Some(Self::upload(
            arena,
            kind,
            &vertices,
            &m.indices,
            std::iter::once(0..m.indices.len() as u32).collect(),
            m.byte_len() as u64,
        ))
    }

    /// A fluid mesh (a chunk's transparent mesh, a section's water): position, normal, colour and
    /// fluid id per vertex (`dwell_mesh::NO_FLUID` where the stream is short). The water shader
    /// draws its ripples procedurally, so the chunks' uvs and tiles are left behind.
    fn fluid(
        arena: &ArenaRef,
        positions: &[f32],
        normals: &[f32],
        colors: &[f32],
        fluids: &[f32],
        indices: &[u32],
    ) -> Option<Self> {
        if indices.is_empty() {
            return None;
        }
        let count = positions.len() / 3;
        let mut ids = fluids.to_vec();
        ids.resize(count, dwell_mesh::NO_FLUID);
        let vertices = interleave(&[(positions, 3), (normals, 3), (colors, 3), (&ids, 1)]);
        let bytes = 4 * (vertices.len() + indices.len()) as u64;
        Some(Self::upload(
            arena,
            Kind::Fluid,
            &vertices,
            indices,
            std::iter::once(0..indices.len() as u32).collect(),
            bytes,
        ))
    }

    /// Flat meshes concatenated into one (an LOD section's surface and its six skirts).
    fn flat(arena: &ArenaRef, meshes: &[&FlatMesh]) -> Option<Self> {
        if meshes.iter().all(|m| m.indices.is_empty()) {
            return None;
        }
        let (mut p, mut n, mut c, mut idx, mut parts) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for m in meshes {
            let base = (p.len() / 3) as u32;
            let start = idx.len() as u32;
            p.extend_from_slice(&m.positions);
            n.extend_from_slice(&m.normals);
            c.extend_from_slice(&m.colors);
            idx.extend(m.indices.iter().map(|i| i + base));
            parts.push(start..idx.len() as u32);
        }
        let bytes = 4 * (p.len() + n.len() + c.len() + idx.len()) as u64;
        let vertices = interleave(&[(&p, 3), (&n, 3), (&c, 3)]);
        Some(Self::upload(
            arena,
            Kind::Flat,
            &vertices,
            &idx,
            parts,
            bytes,
        ))
    }

    fn lines(arena: &ArenaRef, points: &[([f32; 3], [f32; 3])]) -> Self {
        let p: Vec<f32> = points.iter().flat_map(|(p, _)| *p).collect();
        let c: Vec<f32> = points.iter().flat_map(|(_, c)| *c).collect();
        let idx: Vec<u32> = (0..points.len() as u32).collect();
        let bytes = 4 * (p.len() + c.len() + idx.len()) as u64;
        let vertices = interleave(&[(&p, 3), (&c, 3)]);
        Self::upload(
            arena,
            Kind::Line,
            &vertices,
            &idx,
            std::iter::once(0..idx.len() as u32).collect(),
            bytes,
        )
    }

    /// A brush preview's ghost (BRUSHES.md §3): two triangles per face, in the lines' vertex
    /// format (position, colour), drawn by the ghost pipeline.
    fn ghost(arena: &ArenaRef, faces: &[GhostFace]) -> Self {
        let mut p: Vec<f32> = Vec::with_capacity(faces.len() * 12);
        let mut c: Vec<f32> = Vec::with_capacity(faces.len() * 12);
        let mut idx: Vec<u32> = Vec::with_capacity(faces.len() * 6);
        for (k, (quad, kind)) in faces.iter().enumerate() {
            let colour = hex(ghost_color(*kind)).map(|v| v as f32);
            for v in quad {
                p.extend_from_slice(v);
                c.extend_from_slice(&colour);
            }
            let b = 4 * k as u32;
            idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        }
        let bytes = 4 * (p.len() + c.len() + idx.len()) as u64;
        let vertices = interleave(&[(&p, 3), (&c, 3)]);
        Self::upload(
            arena,
            Kind::Line,
            &vertices,
            &idx,
            std::iter::once(0..idx.len() as u32).collect(),
            bytes,
        )
    }
}

/// A ghost cell's colour by what it becomes: blocks put down in the brush's orange, removals red,
/// repaints a pale yellow.
fn ghost_color(kind: GhostKind) -> u32 {
    match kind {
        GhostKind::Put => BRUSH_COLOR,
        GhostKind::Remove => 0xff5040,
        GhostKind::Paint => 0xfff0a0,
    }
}

/// A mesh's regions go back to its page when it goes (the pages themselves are destroyed with the
/// arena: wgpu's WebGPU backend frees nothing on drop).
impl Drop for GpuMesh {
    fn drop(&mut self) {
        self.arena.borrow_mut().free(&self.region);
    }
}

struct Chunk {
    origin: V3,
    opaque: Option<GpuMesh>,
    water: Option<GpuMesh>,
    cutout: Option<GpuMesh>,
}

/// Which of a chunk's meshes a draw is (one per render mode).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Opaque,
    Water,
    Cutout,
}

struct Section {
    origin: V3,
    cell: f64,
    level: usize,
    opaque: Option<GpuMesh>,
    water: Option<GpuMesh>,
}

/// The mesher's terrain renderer behind the seam (`seam.rs`): chunk and section meshes in the
/// arena's pages, drawn by `Renderer::render`'s raster passes (with the players and water).
pub struct RasterTerrain {
    arena: ArenaRef,
    chunks: HashMap<ChunkCoord, Chunk>,
    sections: HashMap<i64, Section>,
    shown: Vec<(i64, u8)>,
    stand_ins: Vec<StandIn>,
}

impl RasterTerrain {
    fn new(arena: ArenaRef) -> Self {
        Self {
            arena,
            chunks: HashMap::default(),
            sections: HashMap::default(),
            shown: Vec::new(),
            stand_ins: Vec::new(),
        }
    }
}

impl TerrainRenderer for RasterTerrain {
    /// Replaces (or, with `None`, removes) a terrain chunk; meshes are chunk-local.
    fn set_chunk(&mut self, coord: ChunkCoord, meshes: Option<&ChunkMeshes>) {
        self.chunks.remove(&coord);
        let Some(m) = meshes else { return };
        let t0 = crate::telemetry::now_ms();
        let t = &m.transparent;
        let cutout = GpuMesh::cutout(&self.arena, &m.cutout);
        let (opaque, water) = (
            GpuMesh::textured(&self.arena, &m.opaque),
            GpuMesh::fluid(
                &self.arena,
                &t.positions,
                &t.normals,
                &t.colors,
                &t.fluids,
                &t.indices,
            ),
        );
        crate::telemetry::add("upload.chunk", crate::telemetry::now_ms() - t0);
        crate::telemetry::add("upload.chunks", 1.0);
        crate::telemetry::add(
            "upload.bytes",
            [&opaque, &water, &cutout]
                .iter()
                .map(|m| m.as_ref().map_or(0, |m| m.bytes))
                .sum::<u64>() as f64,
        );
        if opaque.is_none() && water.is_none() && cutout.is_none() {
            return;
        }
        let origin = chunk_origin_m(coord);
        self.chunks.insert(
            coord,
            Chunk {
                origin,
                opaque,
                water,
                cutout,
            },
        );
    }

    /// LOD sections (§6.6): adds, replaces or removes a section's meshes (in cells from `origin`,
    /// scaled by `cell_size`). Nothing shows until `show_lod_sections` lists it.
    fn set_section(&mut self, id: i64, origin: V3, cell_size: f64, meshes: Option<&SectionMeshes>) {
        self.sections.remove(&id);
        let Some(m) = meshes else { return };
        let mut parts: Vec<&FlatMesh> = vec![&m.opaque];
        parts.extend(m.skirts.iter());
        let t0 = crate::telemetry::now_ms();
        let opaque = GpuMesh::flat(&self.arena, &parts);
        let w = &m.water;
        let water = GpuMesh::fluid(
            &self.arena,
            &w.positions,
            &w.normals,
            &w.colors,
            &w.fluids,
            &w.indices,
        );
        crate::telemetry::add("upload.section", crate::telemetry::now_ms() - t0);
        crate::telemetry::add("upload.sections", 1.0);
        crate::telemetry::add(
            "upload.bytes",
            (opaque.as_ref().map_or(0, |m| m.bytes) + water.as_ref().map_or(0, |m| m.bytes)) as f64,
        );
        let level = cell_size.log2().round().max(0.0) as usize;
        self.sections.insert(
            id,
            Section {
                origin,
                cell: cell_size,
                level,
                opaque,
                water,
            },
        );
    }
    fn show_sections(&mut self, visible: &[(i64, u8)]) {
        self.shown = visible.to_vec();
    }
    fn show_stand_ins(&mut self, stand_ins: &[StandIn]) {
        self.stand_ins = stand_ins.to_vec();
    }
    fn clear(&mut self) {
        self.chunks.clear();
        self.sections.clear();
        self.shown.clear();
        self.stand_ins.clear();
    }
    fn gpu_bytes(&self) -> u64 {
        self.chunks
            .values()
            .flat_map(|c| [&c.opaque, &c.water, &c.cutout])
            .chain(self.sections.values().flat_map(|s| [&s.opaque, &s.water]))
            .flatten()
            .map(|m| m.bytes)
            .sum()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    Terrain,
    Flat,
    Line,
    /// A brush preview's see-through faces: triangles in the lines' format, drawn with them over
    /// the composite, fainter where the scene hides them.
    Ghost,
    /// Fluids (chunk and LOD water): the water shader.
    Fluid,
    /// Cutout terrain (ladders, plants): alpha-tested, depth-writing, both sides, wind sway.
    Cutout,
}

impl Kind {
    /// Drawn over the composite, into the canvas (lines and the brush's ghost).
    fn overlay(self) -> bool {
        matches!(self, Self::Line | Self::Ghost)
    }
}

/// One draw, collected before the passes record.
#[derive(Clone)]
struct DrawItem {
    kind: Kind,
    water: bool,
    no_depth: bool,
    offset: [f32; 4],
    clip: Option<([f32; 3], [f32; 3])>,
    tint: [f32; 4],
    /// Turn about +y (cos, sin of the yaw): players face their heading; fluids carry their origin's
    /// x and z wrapped for the normal detail in zw (`water::wrapped_origin`).
    rotation: [f32; 4],
    mesh: DrawMesh,
    /// The arena page the mesh is in (filled in by `collect`): draws from a page bind it once.
    page: usize,
    /// The depth passes it is drawn in (bit per pass).
    passes: u8,
    /// Its place in the draw order (`draw_order`, filled in by `collect`).
    key: u128,
}

/// The draw order as one integer. Opaque first, then the see-through, then lines; the see-through
/// far to near; the opaque by pipeline and page (each bound once, `render`), then near to far, so
/// the GPU's depth test rejects what nearer meshes hide before shading it (their order does not
/// change the picture: depth decides). Distances compare by their f32 bits (non-negative floats
/// order as their bits do).
fn draw_order(d: &DrawItem) -> u128 {
    let rank = u128::from(u8::from(d.water) + 2 * u8::from(d.kind.overlay()));
    let dist = (d.offset[0] * d.offset[0] + d.offset[1] * d.offset[1] + d.offset[2] * d.offset[2])
        .to_bits();
    let rest: u128 = if d.kind.overlay() {
        // The ghost before the lines over it.
        u128::from(d.kind == Kind::Line)
    } else if d.water {
        u128::from(u32::MAX - dist)
    } else {
        (u128::from(d.kind as u8) << 72)
            | (u128::from(u8::from(d.no_depth)) << 64)
            | ((d.page as u128 & 0xffff_ffff) << 32)
            | u128::from(dist)
    };
    rank << 96 | rest
}

/// No turn.
const NO_ROTATION: [f32; 4] = [1.0, 0.0, 0.0, 0.0];

/// A fluid draw's `rotation`: no turn, and its origin wrapped for the normal detail.
fn fluid_rotation(origin: V3) -> [f32; 4] {
    let [x, z] = water::wrapped_origin(origin);
    [1.0, 0.0, x, z]
}

#[derive(Clone)]
enum DrawMesh {
    Chunk(ChunkCoord, Part),
    Section(i64, bool, u8),
    Outline,
    Debug,
    Region(usize),
    /// The brush preview's ghost (true) or lines.
    Brush(bool),
}

/// The brush outline's colour (BRUSHES.md): warm, apart from the region tools' white and blue.
const BRUSH_COLOR: u32 = 0xffb347;
/// How far a brush outline reaches from its centre at most (m): a line's length aside, the
/// largest brush stretched, for the passes it is drawn in.
const MAX_BRUSH_EXTENT: f32 = 80.0;
/// A brush ghost's opacity where nothing hides it (the shader draws hidden parts at a third).
const GHOST_ALPHA: f32 = 0.38;

/// A brush preview as uploaded.
struct BrushDraw {
    version: u64,
    origin: [f64; 3],
    lines: Option<GpuMesh>,
    ghost: Option<GpuMesh>,
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    size: (u32, u32),
    pixel_ratio: f64,
    /// The ink outline's mode (`?outline=`; docs/ART_DIRECTION.md §7), none by default.
    ink_outline: Option<&'static OutlineMode>,
    /// The spyglass's magnification this frame (1 unzoomed): the outline's fade follows it.
    magnification: f64,
    /// The frame's targets (made when first needed).
    targets: Option<Targets>,
    samples: u32,
    draw_layout: wgpu::BindGroupLayout,
    pipelines: HashMap<(Kind, bool, bool), wgpu::RenderPipeline>,
    /// The scene targets as the composite and lines read them (`frame::scene_layout`).
    scene_layout: wgpu::BindGroupLayout,
    composite_pipeline: wgpu::RenderPipeline,
    /// While the wind is on (`motion_pass`): the composite into the finished image, the motion
    /// pass from it into the canvas, the image's layout, and the image (made when first needed).
    finished_pipeline: wgpu::RenderPipeline,
    motion_pipeline: wgpu::RenderPipeline,
    finished_layout: wgpu::BindGroupLayout,
    finished: Option<frame::Finished>,
    /// The water that reads the scene (W3, Medium and High): its pipeline, drawn in each raster
    /// scene pass's water half, and its view of the scene (`frame::under_layout`) with the sampler.
    water_scene_pipeline: wgpu::RenderPipeline,
    under_layout: wgpu::BindGroupLayout,
    under_sampler: wgpu::Sampler,
    /// The terrain's chunks and LOD sections (seam.rs).
    terrain: RasterTerrain,
    /// The vertical FOV the game chose for this frame (the speed boost, `set_fov`); none: the
    /// screen's own (`vertical_fov`).
    fov: Option<f64>,
    /// The wind streaks and edge blur, drawn by the composite.
    wind: WindView,
    /// GPU pass timing, when the device has timestamp queries (`timing.rs`).
    timer: Option<GpuTimer>,
    /// The frame uniforms: the far pass's, the near pass's and the first-person arms' (their own
    /// projection, characters.rs).
    frame_buffers: [wgpu::Buffer; 3],
    frame_groups: [wgpu::BindGroup; 3],
    draw_buffer: wgpu::Buffer,
    draw_group: wgpu::BindGroup,
    draw_capacity: u64,
    level_colors: bool,
    players: HashMap<u32, PlayerView>,
    /// The first-person arms this frame, if drawn.
    arms: Option<ArmsView>,
    /// The characters' pipelines, meshes and instances (characters.rs).
    characters: characters::CharacterRenderer,
    outline: Option<V3>,
    /// The outline's box in its cell, and its lines.
    outline_box: ([f32; 3], [f32; 3]),
    outline_mesh: GpuMesh,
    debug: Option<GpuMesh>,
    /// The region tools' boxes, each with its edges (rebuilt only when the boxes change).
    region_boxes: Vec<(RegionBox, GpuMesh)>,
    /// The brush tool's preview (rebuilt only when its version changes): its origin, lines and
    /// ghost.
    brush: Option<BrushDraw>,
    /// The GPU pages every mesh lives in (arena.rs).
    arena: ArenaRef,
    /// The plants' wind scale (block data `wind`; 1 by default, 0 keeps them still).
    plant_wind: f64,
    /// Whether the cutout meshes (ladders, plants) are drawn (the bench's A/B of their cost).
    cutout_shown: bool,
    eye: V3,
    yaw: f64,
    pitch: f64,
    face: FaceFrame,
    flip: f64,
    fog: FogSettings,
    /// The fluid the eye is under (`set_eye_fluid`), for the frame uniform.
    eye_fluid: Option<EyeFluid>,
    /// The water quality setting (`set_water_quality`).
    water_quality: WaterQuality,
    /// The water's cost-isolation switches (`water::debug_mask`, `?waterdbg=`; 0 in play).
    water_debug: u32,
    /// The water's noise and caustic tiles (water_tiles.rs).
    water_tiles: WaterTiles,
    /// Frames in flight on the GPU (pacing.rs).
    pacer: pacing::FramePacer,
    /// When the renderer started (ms, telemetry's clock): the water's scroll time.
    start_ms: f64,
    exposure: f64,
    /// The day/night cycle's state (the game sets it each frame) and seconds for the stars' twinkle.
    day: DayState,
    day_seconds: f64,
    /// The sky's clouds: the setting, the coverage (the data's, or a weather system's) and their
    /// clock (s).
    clouds: CloudQuality,
    /// The adapter's kind (`clouds_for`): a CPU adapter draws no clouds.
    adapter: wgpu::DeviceType,
    cloud_coverage: f64,
    cloud_seconds: f64,
    stats: RenderStats,
}

fn linear(c: u32) -> [f32; 3] {
    hex(c).map(|v| srgb_to_linear(v) as f32)
}

impl Renderer {
    /// A renderer drawing into `format` targets (a canvas's, or an offscreen texture's).
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self::with_samples(device, queue, format, SAMPLES)
    }

    /// A renderer with `samples` per pixel (1 or 4, `samples_for`).
    pub fn with_samples(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        samples: u32,
    ) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("dwell"),
            source: wgpu::ShaderSource::Wgsl(shaders::source(samples).into()),
        });
        let frame_entries = [
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            // Each atlas tile's grid lines (dwell_mesh::tile_grids), written once.
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // The fluid looks (water.rs), indexed by fluid id.
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(water::FLUID_STRIDE),
                },
                count: None,
            },
        ];
        // The water's tiles (water_tiles.rs), bindings 5–7.
        let frame_entries: Vec<_> = frame_entries
            .into_iter()
            .chain(WaterTiles::layout_entries())
            .collect();
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame"),
            entries: &frame_entries,
        });
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("draw"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(80),
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("dwell"),
            bind_group_layouts: &[Some(&frame_layout), Some(&draw_layout)],
            immediate_size: 0,
        });
        // The composite and the lines also read the scene targets (they draw after the scene
        // passes, into the canvas).
        let scene_layout = frame::scene_layout(&device, samples);
        let overlay_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("overlay"),
            bind_group_layouts: &[Some(&frame_layout), Some(&draw_layout), Some(&scene_layout)],
            immediate_size: 0,
        });
        // The water that reads the scene behind it (W3): the under-water copy and the view depth.
        let under_layout = frame::under_layout(&device, samples);
        let water_scene_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("water (scene)"),
            bind_group_layouts: &[Some(&frame_layout), Some(&draw_layout), Some(&under_layout)],
            immediate_size: 0,
        });
        let under_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("under"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        // One interleaved buffer per mesh (`interleave`): (stride, attributes at their offsets).
        let attrs = |locations: &[(u32, u32)]| -> (u64, Vec<wgpu::VertexAttribute>) {
            let mut offset = 0u64;
            let attributes = locations
                .iter()
                .map(|&(location, n)| {
                    let format = match n {
                        1 => wgpu::VertexFormat::Float32,
                        2 => wgpu::VertexFormat::Float32x2,
                        3 => wgpu::VertexFormat::Float32x3,
                        _ => wgpu::VertexFormat::Float32x4,
                    };
                    let a = wgpu::VertexAttribute {
                        format,
                        offset,
                        shader_location: location,
                    };
                    offset += u64::from(n) * 4;
                    a
                })
                .collect();
            (offset, attributes)
        };
        let terrain_attrs = attrs(&[(0, 3), (1, 3), (2, 3), (3, 2), (4, 4)]);
        let cutout_attrs = attrs(&[(0, 3), (1, 3), (2, 3), (3, 2), (4, 4), (5, 4), (6, 1)]);
        let flat_attrs = attrs(&[(0, 3), (1, 3), (2, 3)]);
        let line_attrs = attrs(&[(0, 3), (1, 3)]);
        let fluid_attrs = attrs(&[(0, 3), (1, 3), (2, 3), (3, 1)]);
        let mut pipelines = HashMap::default();
        // Opaque terrain, LOD sections and players; lines (depth-tested or over everything); and
        // fluids, the only see-through draws (blended, both sides, no depth writes).
        let variants = [Kind::Terrain, Kind::Flat, Kind::Line]
            .into_iter()
            .flat_map(|k| [(k, false, false), (k, false, true)])
            .chain([
                (Kind::Fluid, true, false),
                (Kind::Cutout, false, false),
                (Kind::Ghost, false, false),
            ]);
        for (kind, water, no_depth) in variants {
            let line = kind.overlay();
            let cutout = kind == Kind::Cutout;
            let (vs, fs, a) = match kind {
                Kind::Terrain => ("terrain_vs", "terrain_fs", &terrain_attrs),
                Kind::Flat => ("flat_vs", "flat_fs", &flat_attrs),
                Kind::Line if no_depth => ("line_vs", "line_top_fs", &line_attrs),
                Kind::Line => ("line_vs", "line_fs", &line_attrs),
                Kind::Ghost => ("line_vs", "ghost_fs", &line_attrs),
                Kind::Fluid => ("fluid_vs", "water_fs", &fluid_attrs),
                Kind::Cutout => ("cutout_vs", "cutout_fs", &cutout_attrs),
            };
            let buffers = [Some(wgpu::VertexBufferLayout {
                array_stride: a.0,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &a.1,
            })];
            let blend = (water || line).then_some(wgpu::BlendState::ALPHA_BLENDING);
            // The scene's draws write colour and (opaque only) view depth into the frame;
            // lines draw into the canvas over the composite, depth-tested in the shader.
            let scene_targets = [
                Some(wgpu::ColorTargetState {
                    format: HDR,
                    blend,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                Some(wgpu::ColorTargetState {
                    format: VIEW_DEPTH,
                    blend: None,
                    write_mask: if water {
                        wgpu::ColorWrites::empty()
                    } else {
                        wgpu::ColorWrites::ALL
                    },
                }),
            ];
            let line_targets = [Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })];
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fs),
                layout: Some(if line { &overlay_layout } else { &layout }),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: &buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: if line { &line_targets } else { &scene_targets },
                }),
                primitive: wgpu::PrimitiveState {
                    topology: if kind == Kind::Line {
                        wgpu::PrimitiveTopology::LineList
                    } else {
                        wgpu::PrimitiveTopology::TriangleList
                    },
                    front_face: wgpu::FrontFace::Ccw,
                    // Water and cutout faces are seen from both sides; opaque faces are culled
                    // from behind.
                    cull_mode: (!water && !cutout && kind != Kind::Line)
                        .then_some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: (!line).then(|| wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(!water),
                    depth_compare: Some(if no_depth {
                        wgpu::CompareFunction::Always
                    } else {
                        wgpu::CompareFunction::LessEqual
                    }),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: if line { 1 } else { samples },
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            });
            pipelines.insert((kind, water, no_depth), pipeline);
        }
        // The water that reads the scene: drawn in a pass's water half, which attaches the colour
        // and the depth buffer only (the view depth is read, not written). Its output is the whole
        // view through the surface (alpha 1, except seen from below).
        let fluid_buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: fluid_attrs.0,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &fluid_attrs.1,
        })];
        let water_scene_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("water_scene_fs"),
            layout: Some(&water_scene_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("fluid_vs"),
                compilation_options: Default::default(),
                buffers: &fluid_buffers,
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("water_scene_fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: samples,
                ..Default::default()
            },
            multiview_mask: None,
            cache: None,
        });
        // The composite: a full-screen triangle into the canvas.
        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("composite"),
            layout: Some(&overlay_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("sky_vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("composite_fs"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        // With the wind on: the composite into the finished image, and the motion pass from it
        // (group 3) into the canvas, where the lines follow it.
        let full_screen = |label, layout, entry, target: wgpu::ColorTargetState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("sky_vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(target)],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let finished_pipeline = full_screen("finished", &overlay_layout, "finished_fs", HDR.into());
        let finished_layout = frame::finished_layout(&device);
        let motion_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("motion"),
            bind_group_layouts: &[
                Some(&frame_layout),
                Some(&draw_layout),
                Some(&scene_layout),
                Some(&finished_layout),
            ],
            immediate_size: 0,
        });
        let motion_pipeline = full_screen("motion", &motion_layout, "motion_fs", format.into());
        let timer = GpuTimer::new(&device, &queue);
        let (atlas_view, sampler) = Self::atlas(&device, &queue);
        let (tile_grid_buffer, fluid_buffer) = frame_extras(&device, &queue);
        let frame_buffers = std::array::from_fn(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("frame"),
                size: FRAME_FLOATS as u64 * 4,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        // The tiles read the near pass's uniform (the time is the frame's).
        let water_tiles = WaterTiles::new(
            &device,
            &module,
            &frame_buffers[1],
            &fluid_buffer,
            &dwell_blocks::FLUIDS,
        );
        let frame_groups = std::array::from_fn(|i| {
            let entries = [
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame_buffers[i].as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: tile_grid_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: fluid_buffer.as_entire_binding(),
                },
            ];
            let entries: Vec<_> = entries
                .into_iter()
                .chain(water_tiles.group_entries())
                .collect();
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("frame"),
                layout: &frame_layout,
                entries: &entries,
            })
        });
        let characters =
            characters::CharacterRenderer::new(&device, &queue, &frame_layout, samples);
        let (draw_buffer, draw_group) = Self::draw_buffer(&device, &draw_layout, 1024);
        let arena = Arena::new(device.clone(), queue.clone(), &STRIDES);
        let outline_box = ([0.0; 3], [1.0; 3]);
        let outline_mesh = Self::outline_lines(&arena, outline_box);
        Self {
            device,
            queue,
            format,
            size: (1, 1),
            pixel_ratio: 1.0,
            ink_outline: None,
            magnification: 1.0,
            targets: None,
            samples,
            draw_layout,
            pipelines,
            scene_layout,
            composite_pipeline,
            finished_pipeline,
            motion_pipeline,
            finished_layout,
            finished: None,
            water_scene_pipeline,
            under_layout,
            under_sampler,
            terrain: RasterTerrain::new(arena.clone()),
            fov: None,
            wind: WindView::default(),
            timer,
            frame_buffers,
            frame_groups,
            draw_buffer,
            draw_group,
            draw_capacity: 1024,
            level_colors: false,
            players: HashMap::default(),
            arms: None,
            characters,
            outline: None,
            outline_box,
            outline_mesh,
            debug: None,
            region_boxes: Vec::new(),
            brush: None,
            arena,
            plant_wind: 1.0,
            cutout_shown: true,
            eye: [0.0, 6.0, 14.0],
            yaw: 180.0,
            pitch: -20.0,
            face: FaceFrame::A,
            flip: 0.0,
            fog: DEFAULT_FOG,
            eye_fluid: None,
            water_quality: WaterQuality::default(),
            water_debug: 0,
            water_tiles,
            pacer: pacing::FramePacer::default(),
            start_ms: crate::telemetry::now_ms(),
            exposure: DEFAULT_EXPOSURE,
            day: DayState::default(),
            day_seconds: 0.0,
            clouds: CloudQuality::High,
            adapter: wgpu::DeviceType::Other,
            cloud_coverage: CLOUDS.shape.coverage,
            cloud_seconds: 0.0,
            stats: RenderStats::default(),
        }
    }

    /// Block textures: the tiled-noise atlas, mipmapped (averaged in linear light), sampled crisp
    /// up close and filtered far away.
    pub(crate) fn atlas(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> (wgpu::TextureView, wgpu::Sampler) {
        let size = dwell_mesh::ATLAS_SIZE as u32;
        let atlas = atlas_levels(dwell_mesh::texture_set());
        let levels = atlas.len() as u32;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("atlas"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (mip, (level, s)) in atlas.iter().enumerate() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: mip as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                level,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * *s as u32),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: *s as u32,
                    height: *s as u32,
                    depth_or_array_layers: 1,
                },
            );
        }
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        (texture.create_view(&Default::default()), sampler)
    }

    fn draw_buffer(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        slots: u64,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("draws"),
            size: slots * DRAW_STRIDE,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("draws"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: None,
                }),
            }],
        });
        (buffer, group)
    }

    /// Resizes the drawing buffer to CSS pixels × device pixel ratio.
    pub fn resize(&mut self, width: u32, height: u32, pixel_ratio: f64) {
        self.size = (width.max(1), height.max(1));
        self.pixel_ratio = pixel_ratio;
        self.targets = None;
        self.finished = None;
    }
    pub fn size(&self) -> (u32, u32) {
        self.size
    }
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Replaces (or, with `None`, removes) a raster terrain chunk; meshes are chunk-local.
    pub fn set_terrain_chunk(&mut self, coord: ChunkCoord, meshes: Option<&ChunkMeshes>) {
        self.terrain.set_chunk(coord, meshes);
    }
    /// LOD sections' raster meshes (§6.6), in cells from `origin` scaled by `cell_size`. Nothing
    /// shows until `show_lod_sections` lists it.
    pub fn set_lod_section(
        &mut self,
        id: i64,
        origin: V3,
        cell_size: f64,
        meshes: Option<&SectionMeshes>,
    ) {
        self.terrain.set_section(id, origin, cell_size, meshes);
    }
    /// The sections to draw from now on, each with the sides (bit per face) whose skirt shows.
    pub fn show_lod_sections(&mut self, visible: &[(i64, u8)]) {
        self.terrain.show_sections(visible);
    }
    pub fn show_lod_stand_ins(&mut self, stand_ins: &[StandIn]) {
        self.terrain.show_stand_ins(stand_ins);
    }
    pub fn set_lod_level_colors(&mut self, on: bool) {
        self.level_colors = on;
    }
    /// The time of day: the sun, moon and stars, each face's sky and lights (`seconds` drives the
    /// stars' twinkle).
    pub fn set_day(&mut self, day: &DayState, seconds: f64) {
        self.day = *day;
        self.day_seconds = seconds;
    }
    /// How far the viewer is toward face A (1) from face B (0): eased across the flip band so the
    /// sky's colours don't jump as the camera turns over at the midplane.
    fn viewer_weight_a(&self) -> f64 {
        let band = dwell_protocol::world::FLIP_BAND as f64;
        // face-boundary: a blend across the midplane, between the two faces' looks.
        let over = self.eye[1] - dwell_world::MIDPLANE_M;
        let t = (over / band * 0.5 + 0.5).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
    /// The plants' wind scale (block data `wind`): 1 by default, 0 keeps them still (low effects).
    /// Draws the cutout meshes or not (dev tools: `__dwell.plants`, measuring what they cost).
    pub fn set_cutout_shown(&mut self, shown: bool) {
        self.cutout_shown = shown;
    }
    pub fn set_plant_wind(&mut self, scale: f64) {
        self.plant_wind = if scale.is_finite() {
            scale.clamp(0.0, 4.0)
        } else {
            1.0
        };
    }
    pub fn set_fog(&mut self, fog: FogSettings) {
        self.fog = fog;
    }
    /// The adapter the frames draw on (its kind: a CPU adapter draws the cheapest tiers,
    /// `clouds_for` and its neighbours).
    pub fn set_adapter(&mut self, device_type: wgpu::DeviceType) {
        self.adapter = device_type;
    }
    pub fn adapter(&self) -> wgpu::DeviceType {
        self.adapter
    }
    /// The Clouds setting (Off, Low, High).
    pub fn set_clouds(&mut self, quality: CloudQuality) {
        self.clouds = quality;
    }
    /// The clouds' coverage, 0 (clear) to 1 (overcast): the data's by default; a weather system's
    /// to set.
    pub fn set_cloud_coverage(&mut self, coverage: f64) {
        self.cloud_coverage = coverage.clamp(0.0, 1.0);
    }
    /// The clouds' clock (s): world time where there is one, so clients roughly agree.
    pub fn set_cloud_time(&mut self, seconds: f64) {
        self.cloud_seconds = seconds;
    }
    /// The fluid whose drawn surface the eye is under (`dwell_world::fluid::eye_fluid`), or none:
    /// written to the frame uniform, where the composite fogs by it (the W4 stub).
    pub fn set_eye_fluid(&mut self, eye_fluid: Option<EyeFluid>) {
        self.eye_fluid = eye_fluid;
    }
    /// The water quality setting (the settings menu; WATER_RENDERING.md §4).
    pub fn set_water_quality(&mut self, quality: WaterQuality) {
        self.water_quality = quality;
    }
    /// Whether a frame may be drawn at `now_ms` (pacing.rs): not while `pacing::MAX_IN_FLIGHT`
    /// frames are still on the GPU.
    pub fn ready_to_draw(&mut self, now_ms: f64) -> bool {
        self.pacer.ready(now_ms)
    }
    /// The water's cost-isolation switches (a dev tool: `water::debug_mask`).
    pub fn set_water_debug(&mut self, mask: u32) {
        self.water_debug = mask;
    }
    /// The water this adapter draws: the setting's, Low on a CPU adapter (`water_for`).
    fn water(&self) -> WaterQuality {
        water_for(self.adapter, self.water_quality)
    }
    pub fn set_exposure(&mut self, exposure: f64) {
        self.exposure = sanitize_exposure(Some(exposure));
    }
    /// Outlines the targeted block: its cell's minimum corner and the box in the cell (min, max),
    /// or none.
    pub fn set_block_outline(&mut self, cell: Option<V3>, bounds: ([f32; 3], [f32; 3])) {
        self.outline = cell;
        if cell.is_some() && bounds != self.outline_box {
            self.outline_box = bounds;
            self.outline_mesh = Self::outline_lines(&self.arena, bounds);
        }
    }

    /// The outline's twelve edges about a box in the cell, slightly enlarged (2 mm each side).
    fn outline_lines(arena: &ArenaRef, (lo, hi): ([f32; 3], [f32; 3])) -> GpuMesh {
        let edges = geometry::box_edges_about(lo.map(|v| v - 0.002), hi.map(|v| v + 0.002));
        GpuMesh::lines(
            arena,
            &edges
                .iter()
                .map(|p| (*p, [1.0, 1.0, 1.0]))
                .collect::<Vec<_>>(),
        )
    }
    /// The region tools' boxes (§6.5), outlined over the terrain.
    pub fn set_region_boxes(&mut self, boxes: &[RegionBox]) {
        if self.region_boxes.len() == boxes.len()
            && self
                .region_boxes
                .iter()
                .zip(boxes)
                .all(|((a, _), b)| a == b)
        {
            return;
        }
        self.region_boxes = boxes
            .iter()
            .map(|b| {
                let c = hex(b.color).map(|v| v as f32);
                let size = b.size.map(|s| s as f32);
                let pts: Vec<([f32; 3], [f32; 3])> = geometry::box_edges()
                    .into_iter()
                    .map(|p| (std::array::from_fn(|k| p[k] / 1.004 * size[k]), c))
                    .collect();
                (*b, GpuMesh::lines(&self.arena, &pts))
            })
            .collect();
    }
    /// The brush tool's preview (BRUSHES.md §3): the ghost of the cells a stroke would change and
    /// its lines, over the terrain, or none. Uploaded only when its version changes.
    pub fn set_brush_outline(&mut self, outline: Option<&BrushOutline>) {
        if self.brush.as_ref().map(|b| b.version) == outline.map(|o| o.version) {
            return;
        }
        self.brush = outline.map(|o| {
            let c = hex(BRUSH_COLOR).map(|v| v as f32);
            let pts: Vec<([f32; 3], [f32; 3])> = o
                .segments
                .iter()
                .flat_map(|&(a, b)| [(a, c), (b, c)])
                .collect();
            BrushDraw {
                version: o.version,
                origin: o.origin,
                lines: (!pts.is_empty()).then(|| GpuMesh::lines(&self.arena, &pts)),
                ghost: (!o.faces.is_empty()).then(|| GpuMesh::ghost(&self.arena, &o.faces)),
            }
        });
    }
    pub fn set_player(&mut self, id: u32, view: Option<PlayerView>) {
        match view {
            Some(v) => {
                self.players.insert(id, v);
            }
            None => {
                self.players.remove(&id);
            }
        }
    }
    /// Places the camera at `eye`, looking along yaw/pitch (degrees) for `face`; `flip` turns the
    /// view over about its right axis across the midplane.
    /// Your first-person arms this frame, or none (dead, zoomed, hidden).
    pub fn set_arms(&mut self, arms: Option<ArmsView>) {
        self.arms = arms;
    }
    /// The characters drawn last frame and their GPU bytes (telemetry, the bench).
    pub fn character_stats(&self) -> characters::CharacterStats {
        self.characters.stats
    }
    pub fn set_camera(
        &mut self,
        eye: V3,
        yaw_deg: f64,
        pitch_deg: f64,
        face: FaceFrame,
        flip: f64,
    ) {
        (self.eye, self.yaw, self.pitch, self.face, self.flip) =
            (eye, yaw_deg, pitch_deg, face, flip);
    }
    /// The frame's vertical FOV (degrees), as the game chose it for the speed boost and gave the
    /// LOD selection too; `None` draws with the screen's own (`vertical_fov`).
    pub fn set_fov(&mut self, fov_y_deg: Option<f64>) {
        self.fov = fov_y_deg;
    }
    /// The wind for this frame (the composite draws it).
    pub fn set_wind(&mut self, wind: WindView) {
        self.wind = wind;
    }
    /// The spyglass's magnification this frame (1 unzoomed; docs/SPYGLASS.md).
    pub fn set_magnification(&mut self, magnification: f64) {
        self.magnification = magnification;
    }
    /// The ink outline's mode by name (`shared/look/outline.json`; docs/ART_DIRECTION.md §7);
    /// `off` or an unknown name turns it off. True when a mode is on.
    pub fn set_outline(&mut self, name: &str) -> bool {
        self.ink_outline = OUTLINE.mode(name);
        self.ink_outline.is_some()
    }
    /// The ink outline's mode from the page's query (`?outline=<mode>`); none without it.
    pub fn set_outline_query(&mut self, search: &str) {
        self.ink_outline = dwell_look::outline::mode_from_query(&OUTLINE, search);
    }
    /// The ink outline's mode (its name), or none.
    pub fn outline(&self) -> Option<&'static str> {
        self.ink_outline.map(|m| m.name)
    }
    /// The vertical FOV this frame draws with.
    pub fn frame_fov(&self) -> f64 {
        let aspect = f64::from(self.size.0) / f64::from(self.size.1);
        self.fov.unwrap_or_else(|| vertical_fov(aspect))
    }
    /// Debug line segments, or `None` to clear.
    pub fn set_debug_lines(&mut self, lines: Option<&[Line]>) {
        self.debug = lines.filter(|l| !l.is_empty()).map(|lines| {
            let pts: Vec<([f32; 3], [f32; 3])> = lines
                .iter()
                .flat_map(|l| {
                    let c = hex(l.color).map(|v| v as f32);
                    let rel = |p: V3| {
                        [
                            (p[0] - self.eye[0]) as f32,
                            (p[1] - self.eye[1]) as f32,
                            (p[2] - self.eye[2]) as f32,
                        ]
                    };
                    [(rel(l.from), c), (rel(l.to), c)]
                })
                .collect();
            GpuMesh::lines(&self.arena, &pts)
        });
    }
    pub fn stats(&self) -> RenderStats {
        let pixels = u64::from(self.size.0) * u64::from(self.size.1);
        let per_pixel = frame::bytes_per_pixel(self.samples, self.water().reads_scene());
        RenderStats {
            mesh_bytes: self.terrain.gpu_bytes(),
            screen_bytes: pixels
                * (per_pixel
                    + if self.finished.is_some() {
                        frame::FINISHED_BYTES
                    } else {
                        0
                    }),
            pixel_ratio: self.pixel_ratio,
            ..self.stats
        }
    }

    fn rel(&self, p: V3) -> [f32; 3] {
        [
            (p[0] - self.eye[0]) as f32,
            (p[1] - self.eye[1]) as f32,
            (p[2] - self.eye[2]) as f32,
        ]
    }

    fn frame_uniforms(&self, basis: &Basis, near: f64, far: f64) -> [f32; FRAME_FLOATS] {
        self.frame_uniforms_at(basis, near, far, self.frame_fov())
    }

    /// The frame uniforms with a vertical FOV of their own (the first-person arms').
    fn frame_uniforms_at(
        &self,
        basis: &Basis,
        near: f64,
        far: f64,
        fov: f64,
    ) -> [f32; FRAME_FLOATS] {
        let aspect = f64::from(self.size.0) / f64::from(self.size.1);
        let vp = mul(&perspective(fov, aspect, near, far), &view_matrix(basis));
        let mut u = [0.0f32; FRAME_FLOATS];
        let mut i = 0;
        let mut put = |v: &[f32]| {
            u[i..i + v.len()].copy_from_slice(v);
            i += 4 * v.len().div_ceil(4);
        };
        for col in &vp {
            put(col);
        }
        for v in outline_uniforms(
            &OUTLINE,
            self.ink_outline,
            self.pixel_ratio,
            self.magnification,
        ) {
            put(&v);
        }
        let weight_a = self.viewer_weight_a();
        let viewer = self.day.viewer(weight_a);
        put(&[
            self.eye[1] as f32,
            self.face.sign_f32(),
            (self.exposure * viewer.exposure) as f32,
            (self.fog.density * viewer.fog_density).min(1.0) as f32,
        ]);
        // The water's scroll time, seconds since the renderer started.
        let seconds = (crate::telemetry::now_ms() - self.start_ms) / 1000.0;
        put(&[
            fog_sigma(self.fog.distance_m) as f32,
            self.fog.height_m as f32,
            seconds as f32,
            self.plant_wind as f32,
        ]);
        let f3 = |v: [f64; 3]| [v[0] as f32, v[1] as f32, v[2] as f32];
        put(&f3(basis.right));
        put(&f3(basis.up));
        put(&f3(basis.forward()));
        let tan_y = (fov.to_radians() / 2.0).tan();
        put(&[
            (tan_y * aspect) as f32,
            tan_y as f32,
            self.eye[0].rem_euclid(WIND_PERIOD) as f32,
            self.eye[2].rem_euclid(WIND_PERIOD) as f32,
        ]);
        let clouds = cloud_uniforms(
            &CLOUDS,
            self.eye,
            self.face,
            self.cloud_seconds,
            clouds_for(self.adapter, self.clouds),
            self.cloud_coverage,
        );
        put(&clouds[0]);
        put(&clouds[1]);
        put(&clouds[2]);
        put(&water_uniform(self.eye_fluid, self.water()));
        for v in self.wind_uniforms(basis, aspect, tan_y) {
            put(&v);
        }
        // The eye's x and z wrapped for the water's detail (the composite's caustics under it).
        let [wx, wz] = water::wrapped_origin(self.eye);
        // z: the water's cost-isolation switches (`water::debug_mask`; 0 in play).
        put(&[wx, wz, self.water_debug as f32, 0.0]);
        // The water's reflections for its quality, under the Clouds setting (WATER_RENDERING.md §8).
        for v in reflection_uniforms(
            &REFLECTIONS,
            self.water().level() as usize,
            self.clouds.octaves(&CLOUDS),
            self.clouds.self_shadow(),
        ) {
            put(&v);
        }
        let pixel_angle = 2.0 * tan_y / f64::from(self.size.1.max(1));
        let mut day = self.day.pack(weight_a, self.day_seconds, pixel_angle);
        if !stars_for(self.adapter) {
            day[DAY_STARS] = 0.0;
        }
        put(&day);
        u
    }

    /// The wind's part of the frame block (`wind_*` in shaders.rs): the vanishing point of the
    /// travel in this frame's view, the intensity, and the look from the tunables (dwell-look's
    /// MOTION).
    fn wind_uniforms(&self, basis: &Basis, aspect: f64, tan_y: f64) -> [[f32; 4]; 6] {
        let w = &self.wind;
        let d = w.direction;
        let view_dir = [
            camera::dot(d, basis.right),
            camera::dot(d, basis.up),
            camera::dot(d, basis.forward()),
        ];
        let (point, sign) = vanishing_point(view_dir, tan_y * aspect, tan_y);
        let t = &MOTION.wind;
        let ratio = self.pixel_ratio.max(0.01);
        let f = |v: f64| v as f32;
        [
            [f(point[0]), f(point[1]), f(w.intensity), f(w.time)],
            [f(aspect), f(w.flight), self.size.1 as f32, f(sign)],
            [f(t.color[0]), f(t.color[1]), f(t.color[2]), f(t.opacity)],
            [
                t.streaks as f32,
                t.flight_streaks as f32,
                f(t.length),
                f(t.width_px * ratio),
            ],
            [
                f(t.inner_radius),
                f(t.outer_radius),
                f(t.vignette),
                f(t.flow_speed),
            ],
            [
                if motion_blur_for(self.adapter) {
                    f(t.blur)
                } else {
                    0.0
                },
                0.0,
                0.0,
                0.0,
            ],
        ]
    }

    /// The draws of one pass: everything whose distance range meets [near, far].
    /// A draw's mesh and the index ranges it draws (a section's surface and its shown skirts).
    fn mesh_of(&self, m: &DrawMesh) -> (&GpuMesh, Vec<std::ops::Range<u32>>) {
        match m {
            DrawMesh::Chunk(c, part) => {
                let chunk = &self.terrain.chunks[c];
                let m = match part {
                    Part::Opaque => chunk.opaque.as_ref(),
                    Part::Water => chunk.water.as_ref(),
                    Part::Cutout => chunk.cutout.as_ref(),
                }
                .expect("collected");
                (m, vec![m.parts[0].clone()])
            }
            DrawMesh::Section(id, w, mask) => {
                let s = &self.terrain.sections[id];
                let m = if *w {
                    s.water.as_ref()
                } else {
                    s.opaque.as_ref()
                }
                .expect("collected");
                let mut r = vec![m.parts[0].clone()];
                r.extend(
                    (0..6)
                        .filter(|f| mask & (1 << f) != 0)
                        .filter_map(|f| m.parts.get(1 + f).cloned()),
                );
                (m, r)
            }
            DrawMesh::Outline => (&self.outline_mesh, vec![self.outline_mesh.parts[0].clone()]),
            DrawMesh::Region(i) => {
                let m = &self.region_boxes[*i].1;
                (m, vec![m.parts[0].clone()])
            }
            DrawMesh::Brush(ghost) => {
                let b = self.brush.as_ref().expect("collected");
                let m = if *ghost { &b.ghost } else { &b.lines };
                let m = m.as_ref().expect("collected");
                (m, vec![m.parts[0].clone()])
            }
            DrawMesh::Debug => {
                let m = self.debug.as_ref().expect("collected");
                (m, vec![m.parts[0].clone()])
            }
        }
    }

    /// The draws of both depth passes (`passes`: each pass's near and far view depth), in one sweep
    /// over the chunks and sections (gathering each pass on its own walked everything twice).
    fn collect(
        &self,
        passes: [(f64, f64); 2],
        chunk_visible: &dyn Fn(ChunkCoord) -> bool,
    ) -> [Vec<DrawItem>; 3] {
        let mut out = Vec::new();
        let plain = [1.0, 1.0, 1.0, 1.0];
        let aspect = f64::from(self.size.0) / f64::from(self.size.1);
        let forward = aim(self.yaw, self.pitch, self.face, self.flip).forward();
        let view = PassView::new(self.eye, forward, self.frame_fov(), aspect);
        // Which passes (bit per pass) a box reaches into.
        let reaches = |origin: V3, size: V3| -> u8 {
            u8::from(view.holds_box(origin, size, passes[0].0, passes[0].1))
                | u8::from(view.holds_box(origin, size, passes[1].0, passes[1].1)) << 1
        };
        for (&coord, c) in &self.terrain.chunks {
            let passes = if chunk_visible(coord) {
                reaches(c.origin, CHUNK_EXTENT_M)
            } else {
                0
            };
            if passes == 0 {
                continue;
            }
            let r = self.rel(c.origin);
            let offset = [r[0], r[1], r[2], 1.0];
            if c.opaque.is_some() {
                out.push(DrawItem {
                    kind: Kind::Terrain,
                    water: false,
                    no_depth: false,
                    offset,
                    clip: None,
                    tint: plain,
                    rotation: NO_ROTATION,
                    page: 0,
                    passes,
                    key: 0,
                    mesh: DrawMesh::Chunk(coord, Part::Opaque),
                });
            }
            if c.cutout.is_some() && self.cutout_shown {
                out.push(DrawItem {
                    kind: Kind::Cutout,
                    water: false,
                    no_depth: false,
                    offset,
                    clip: None,
                    tint: plain,
                    rotation: NO_ROTATION,
                    page: 0,
                    passes,
                    key: 0,
                    mesh: DrawMesh::Chunk(coord, Part::Cutout),
                });
            }
            if c.water.is_some() {
                out.push(DrawItem {
                    kind: Kind::Fluid,
                    water: true,
                    no_depth: false,
                    offset,
                    clip: None,
                    tint: plain,
                    rotation: fluid_rotation(c.origin),
                    page: 0,
                    passes,
                    key: 0,
                    mesh: DrawMesh::Chunk(coord, Part::Water),
                });
            }
        }
        let level_tint = |s: &Section, alpha: f32| {
            let t = if self.level_colors {
                linear(LEVEL_TINTS[s.level.min(LEVEL_TINTS.len() - 1)])
            } else {
                [1.0; 3]
            };
            [t[0], t[1], t[2], alpha]
        };
        for &(id, mask) in &self.terrain.shown {
            let Some(s) = self.terrain.sections.get(&id) else {
                continue;
            };
            let side = s.cell * 32.0;
            let passes = reaches(s.origin, [side, side * CELL_H, side]);
            if passes == 0 {
                continue;
            }
            let r = self.rel(s.origin);
            let offset = [r[0], r[1], r[2], s.cell as f32];
            if s.opaque.is_some() {
                out.push(DrawItem {
                    kind: Kind::Flat,
                    water: false,
                    no_depth: false,
                    offset,
                    clip: None,
                    tint: level_tint(s, 1.0),
                    rotation: NO_ROTATION,
                    page: 0,
                    passes,
                    key: 0,
                    mesh: DrawMesh::Section(id, false, mask),
                });
            }
            if s.water.is_some() {
                out.push(DrawItem {
                    kind: Kind::Fluid,
                    water: true,
                    no_depth: false,
                    offset,
                    clip: None,
                    tint: level_tint(s, 1.0),
                    rotation: fluid_rotation(s.origin),
                    page: 0,
                    passes,
                    key: 0,
                    mesh: DrawMesh::Section(id, true, 0),
                });
            }
        }
        // Stand-ins: the source section's surface, clipped to the hole's box (its own skirts off).
        for st in self.terrain.stand_ins.iter().take(MAX_STAND_INS) {
            let passes = 3;
            let Some(s) = self
                .terrain
                .sections
                .get(&st.id)
                .filter(|s| s.opaque.is_some())
            else {
                continue;
            };
            let r = self.rel(s.origin);
            let clip = Some((self.rel(st.lo), self.rel(st.hi)));
            out.push(DrawItem {
                kind: Kind::Flat,
                water: false,
                no_depth: false,
                offset: [r[0], r[1], r[2], s.cell as f32],
                clip,
                tint: level_tint(s, 1.0),
                rotation: NO_ROTATION,
                page: 0,
                passes,
                key: 0,
                mesh: DrawMesh::Section(st.id, false, 0),
            });
        }
        if let Some(cell) = self.outline.filter(|c| reaches(*c, [2.0; 3]) != 0) {
            let passes = reaches(cell, [2.0; 3]);
            let r = self.rel(cell);
            let c = hex(0x101418).map(|v| v as f32);
            out.push(DrawItem {
                kind: Kind::Line,
                water: false,
                no_depth: false,
                offset: [r[0], r[1], r[2], 1.0],
                clip: None,
                tint: [c[0], c[1], c[2], 0.8],
                rotation: NO_ROTATION,
                page: 0,
                passes,
                key: 0,
                mesh: DrawMesh::Outline,
            });
        }
        // The region tools' boxes: over everything, so a box around buried cells still shows.
        for (i, (b, _)) in self.region_boxes.iter().enumerate() {
            let side = b.size.iter().copied().fold(0.0, f64::max);
            let passes = reaches(b.min, [side; 3]);
            if passes == 0 {
                continue;
            }
            let r = self.rel(b.min);
            out.push(DrawItem {
                kind: Kind::Line,
                water: false,
                no_depth: true,
                offset: [r[0], r[1], r[2], 1.0],
                clip: None,
                tint: [1.0, 1.0, 1.0, 0.9],
                rotation: NO_ROTATION,
                page: 0,
                passes,
                key: 0,
                mesh: DrawMesh::Region(i),
            });
        }
        // The brush preview (BRUSHES.md §3): its lines over everything, like the region boxes; its
        // ghost see-through, fainter where the terrain hides it.
        if let Some(b) = &self.brush {
            let side = 2.0 * f64::from(MAX_BRUSH_EXTENT);
            let corner = b.origin.map(|v| v - f64::from(MAX_BRUSH_EXTENT));
            let passes = reaches(corner, [side; 3]);
            if passes != 0 {
                let r = self.rel(b.origin);
                for (ghost, present) in [(true, b.ghost.is_some()), (false, b.lines.is_some())] {
                    if !present {
                        continue;
                    }
                    out.push(DrawItem {
                        kind: if ghost { Kind::Ghost } else { Kind::Line },
                        water: false,
                        no_depth: !ghost,
                        offset: [r[0], r[1], r[2], 1.0],
                        clip: None,
                        tint: if ghost {
                            [1.0, 1.0, 1.0, GHOST_ALPHA]
                        } else {
                            [1.0, 1.0, 1.0, 0.95]
                        },
                        rotation: NO_ROTATION,
                        page: 0,
                        passes,
                        key: 0,
                        mesh: DrawMesh::Brush(ghost),
                    });
                }
            }
        }
        // Debug lines: in the pass that draws from the near plane.
        let passes = u8::from(passes[0].0 < NEAR_PLANE * 2.0)
            | u8::from(passes[1].0 < NEAR_PLANE * 2.0) << 1;
        if self.debug.is_some() && passes != 0 {
            out.push(DrawItem {
                kind: Kind::Line,
                water: false,
                no_depth: true,
                offset: [0.0, 0.0, 0.0, 1.0],
                clip: None,
                tint: [1.0, 1.0, 1.0, 1.0],
                rotation: NO_ROTATION,
                page: 0,
                passes,
                key: 0,
                mesh: DrawMesh::Debug,
            });
        }
        for d in &mut out {
            d.page = self.mesh_of(&d.mesh).0.region.page;
            d.key = draw_order(d);
        }
        // One stable sort by a precomputed key (draw_order), then each pass takes its draws in that
        // order. Lines draw once, over the composite (the third list), with the near pass's
        // projection (the outline is in reach, debug lines are near-pass draws).
        out.sort_by_key(|d| d.key);
        let mut split: [Vec<DrawItem>; 3] = [Vec::with_capacity(out.len()), Vec::new(), Vec::new()];
        for d in out {
            if d.kind.overlay() {
                split[2].push(d);
            } else if d.passes == 3 {
                split[1].push(d.clone());
                split[0].push(d);
            } else if d.passes & 1 != 0 {
                split[0].push(d);
            } else {
                split[1].push(d);
            }
        }
        split
    }

    /// The frame's targets, made again when the size changes or the water starts or stops
    /// reading the scene.
    fn ensure_targets(&mut self) {
        // The under-water copy only where the water reads the scene (Medium and High).
        let under = self.water().reads_scene();
        if self
            .targets
            .as_ref()
            .is_some_and(|t| t.under.is_some() == under)
        {
            return;
        }
        // Dropped first: their textures are destroyed before the new ones are made.
        self.targets = None;
        self.targets = Some(Targets::new(
            &self.device,
            self.size,
            self.samples,
            &self.scene_layout,
            under.then_some((&self.under_layout, &self.under_sampler)),
        ));
    }

    /// Draws one frame into `target` (a canvas texture or an offscreen one, `format`, the size
    /// given to `resize`).
    /// `chunk_visible` says which terrain chunks to draw (the LOD draws instead of the others).
    ///
    /// The terrain's far and near passes (players and water with them) draw into the frame's
    /// targets; then the composite draws them into `target` with the lines over it.
    pub fn render(
        &mut self,
        target: &wgpu::TextureView,
        chunk_visible: &dyn Fn(ChunkCoord) -> bool,
    ) {
        self.ensure_targets();
        // GPU pass times of earlier frames the GPU has finished; then whether this one is timed.
        if let Some(timer) = &mut self.timer {
            for (name, ms) in timer.collect() {
                crate::telemetry::sample(name, ms);
            }
            timer.begin();
        }
        let collect_start = crate::telemetry::now_ms();
        let basis = aim(self.yaw, self.pitch, self.face, self.flip);
        // Two passes (§6.6): the far one beyond the split (its near plane pushed out with altitude,
        // where nothing is closer), then a depth clear and the near one. Heights are face-local.
        let altitude = self.face.y(self.eye[1]) - TERRAIN_TOP_M;
        let far_near = (NEAR_SPLIT * 0.95).max(altitude * 0.8);
        let passes = [(far_near, FAR_PLANE), (NEAR_PLANE, NEAR_SPLIT)];
        let draws = self.collect(passes, chunk_visible);
        for (i, (near, far)) in passes.iter().enumerate() {
            let u = self.frame_uniforms(&basis, *near, *far);
            self.queue
                .write_buffer(&self.frame_buffers[i], 0, floats(&u));
        }
        // The characters: other players' bodies in the scene passes, your arms after them with
        // their own projection (characters.rs).
        let bodies: Vec<characters::BodyDraw> = self
            .players
            .values()
            .map(|v| characters::BodyDraw {
                feet: v.feet,
                yaw: v.yaw,
                face: v.face,
                dead: v.dead,
                look: v.look,
                pose: v.pose,
                held: v.held,
            })
            .collect();
        let strokes = dwell_mesh::texture_set() == dwell_mesh::TextureSet::Painterly;
        self.characters.prepare(
            &self.device,
            &self.queue,
            self.eye,
            &basis,
            &passes,
            &bodies,
            self.arms.as_ref(),
            strokes,
        );
        let arms = self.characters.has_arms();
        if arms {
            let fov = dwell_characters::player()
                .first_person
                .map_or(62.0, |f| f64::from(f.fov));
            let u =
                self.frame_uniforms_at(&basis, characters::ARMS_NEAR, characters::ARMS_FAR, fov);
            self.queue
                .write_buffer(&self.frame_buffers[2], 0, floats(&u));
        }
        // With the arms, the world's last pass keeps its colour and the arms' pass resolves.
        let planned = passes.len() + usize::from(arms);
        // Per-draw uniforms, every pass's in one buffer.
        let total = draws.iter().map(Vec::len).sum::<usize>() as u64;
        if total > self.draw_capacity {
            self.draw_capacity = total.next_power_of_two();
            self.draw_buffer.destroy();
            (self.draw_buffer, self.draw_group) =
                Self::draw_buffer(&self.device, &self.draw_layout, self.draw_capacity);
        }
        let mut data = vec![0.0f32; (total * DRAW_STRIDE / 4) as usize];
        for (k, d) in draws.iter().flatten().enumerate() {
            let at = k * (DRAW_STRIDE / 4) as usize;
            data[at..at + 4].copy_from_slice(&d.offset);
            if let Some((lo, hi)) = d.clip {
                data[at + 4..at + 8].copy_from_slice(&[lo[0], lo[1], lo[2], 1.0]);
                data[at + 8..at + 12].copy_from_slice(&[hi[0], hi[1], hi[2], 0.0]);
            }
            data[at + 12..at + 16].copy_from_slice(&d.tint);
            data[at + 16..at + 20].copy_from_slice(&d.rotation);
        }
        if !data.is_empty() {
            self.queue.write_buffer(&self.draw_buffer, 0, floats(&data));
        }
        // The draws' first slots in the draw buffer: each list follows the one before.
        let first = [
            0,
            draws[0].len() as u64,
            (draws[0].len() + draws[1].len()) as u64,
        ];
        let collect_ms = crate::telemetry::now_ms() - collect_start;
        let mut encoder = self.device.create_command_encoder(&Default::default());
        // The water's tiles first: the noise once, and this frame's caustics where the water reads
        // the scene and some is drawn or the eye is under it.
        let caustics = self.water().reads_scene()
            && (self.eye_fluid.is_some() || draws.iter().flatten().any(|d| d.water));
        if self.water_tiles.pending(caustics) {
            let query = self
                .timer
                .as_mut()
                .and_then(|t| t.pass(timing::WATER_TILES));
            self.water_tiles.draw(
                &mut encoder,
                caustics,
                self.timer.as_ref().and_then(|t| t.writes(query)),
            );
        }
        let arena = self.arena.borrow();
        let (mut calls, mut triangles) = (0u32, 0u64);
        let scene_passes = passes.len();
        let targets = self.targets.as_ref().expect("ensure_targets made them");
        let t0 = crate::telemetry::now_ms();
        // With water that reads the scene (W3), a pass with water is split: its opaque half
        // (colour resolved, depth kept), a copy of the colour, then its water half, which samples
        // the copy and the view depth.
        let reads_scene = self.water().reads_scene();
        let names = [timing::FAR, timing::NEAR];
        let water_names = [timing::FAR_WATER, timing::NEAR_WATER];
        for (i, pass_draws) in draws[..scene_passes].iter().enumerate() {
            let query = self.timer.as_mut().and_then(|t| t.pass(names[i]));
            let split = reads_scene
                .then(|| pass_draws.iter().position(|d| d.water))
                .flatten();
            let (ops, water_ops) = match split {
                Some(_) => {
                    let (o, w) = frame::split_pass_ops(i, planned, self.samples);
                    (o, Some(w))
                }
                None => (frame::scene_pass_ops(i, planned, self.samples), None),
            };
            let opaque = &pass_draws[..split.unwrap_or(pass_draws.len())];
            // The characters draw with the opaque draws, before any see-through water.
            let solid = opaque.iter().position(|d| d.water).unwrap_or(opaque.len());
            {
                let mut pass = Self::scene_pass(
                    &mut encoder,
                    targets,
                    i,
                    ops,
                    split.is_some(),
                    self.timer.as_ref().and_then(|t| t.writes(query)),
                );
                pass.set_bind_group(0, &self.frame_groups[i], &[]);
                pass.set_bind_group(1, &self.draw_group, &[]);
                self.record(
                    &mut pass,
                    &opaque[..solid],
                    first[i],
                    &arena,
                    false,
                    &mut calls,
                    &mut triangles,
                );
                if self
                    .characters
                    .draw_bodies(&mut pass, i, &mut calls, &mut triangles)
                {
                    pass.set_bind_group(1, &self.draw_group, &[]);
                }
                self.record(
                    &mut pass,
                    &opaque[solid..],
                    first[i] + solid as u64,
                    &arena,
                    false,
                    &mut calls,
                    &mut triangles,
                );
            }
            let (Some(at), Some(water_ops)) = (split, water_ops) else {
                continue;
            };
            let (copy, under) = targets.under.as_ref().expect("reads_scene targets");
            encoder.copy_texture_to_texture(
                targets.colour.0.as_image_copy(),
                copy.0.as_image_copy(),
                copy.0.size(),
            );
            let query = self.timer.as_mut().and_then(|t| t.pass(water_names[i]));
            let mut pass = Self::water_pass(
                &mut encoder,
                targets,
                water_ops,
                self.timer.as_ref().and_then(|t| t.writes(query)),
            );
            pass.set_bind_group(0, &self.frame_groups[i], &[]);
            pass.set_bind_group(1, &self.draw_group, &[]);
            pass.set_bind_group(2, under, &[]);
            self.record(
                &mut pass,
                &pass_draws[at..],
                first[i] + at as u64,
                &arena,
                true,
                &mut calls,
                &mut triangles,
            );
        }
        // Your arms over the world: the same targets, the depth buffer cleared (characters.rs
        // `arms_pass`), their own projection.
        if arms {
            let plan = characters::arms_pass(scene_passes, self.samples);
            let query = self.timer.as_mut().and_then(|t| t.pass(timing::ARMS));
            let mut pass = Self::begin_scene_pass(
                &mut encoder,
                targets,
                "arms",
                plan.ops,
                plan.depth,
                false,
                self.timer.as_ref().and_then(|t| t.writes(query)),
            );
            pass.set_bind_group(0, &self.frame_groups[2], &[]);
            self.characters
                .draw_arms(&mut pass, &mut calls, &mut triangles);
        }
        let cpu = crate::telemetry::now_ms() - t0 + collect_ms;
        // With the wind on, the composite draws the finished image and the motion pass reads it
        // (docs/MOTION_EFFECTS.md: post effects that sample neighbours read the finished image).
        let motion = motion_pass(self.wind.intensity);
        if motion && self.finished.is_none() {
            self.finished = Some(frame::Finished::new(
                &self.device,
                self.size,
                &self.finished_layout,
            ));
        }
        if let (true, Some(finished)) = (motion, &self.finished) {
            let query = self.timer.as_mut().and_then(|t| t.pass(timing::COMPOSITE));
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("finished"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &finished.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                timestamp_writes: self.timer.as_ref().and_then(|t| t.writes(query)),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.frame_groups[scene_passes - 1], &[]);
            pass.set_bind_group(1, &self.draw_group, &[]);
            pass.set_pipeline(&self.finished_pipeline);
            pass.set_bind_group(2, &targets.scene, &[]);
            pass.draw(0..3, 0..1);
            calls += 1;
        }
        {
            let name = if motion {
                timing::MOTION
            } else {
                timing::COMPOSITE
            };
            let query = self.timer.as_mut().and_then(|t| t.pass(name));
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("composite"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                timestamp_writes: self.timer.as_ref().and_then(|t| t.writes(query)),
                ..Default::default()
            });
            // The composite and lines use the near pass's uniforms.
            pass.set_bind_group(0, &self.frame_groups[scene_passes - 1], &[]);
            pass.set_bind_group(1, &self.draw_group, &[]);
            pass.set_bind_group(2, &targets.scene, &[]);
            if let (true, Some(finished)) = (motion, &self.finished) {
                // The wind over the finished image.
                pass.set_bind_group(3, &finished.group, &[]);
                pass.set_pipeline(&self.motion_pipeline);
            } else {
                pass.set_pipeline(&self.composite_pipeline);
            }
            pass.draw(0..3, 0..1);
            calls += 1;
            self.record(
                &mut pass,
                &draws[scene_passes],
                first[scene_passes],
                &arena,
                false,
                &mut calls,
                &mut triangles,
            );
        }
        drop(arena);
        if let Some(timer) = &self.timer {
            timer.resolve(&mut encoder);
        }
        self.queue.submit([encoder.finish()]);
        self.queue.on_submitted_work_done(self.pacer.submitted());
        if let Some(timer) = &mut self.timer {
            timer.submitted();
        }
        crate::telemetry::add("draw.raster", cpu);
        self.stats.calls = calls;
        self.stats.triangles = triangles;
    }

    /// Records `draws` (taking draw-buffer slots from `first_slot`) into a pass; fluids with the
    /// water that reads the scene when `water_scene` (a split pass's water half).
    #[allow(clippy::too_many_arguments)]
    fn record(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        draws: &[DrawItem],
        first_slot: u64,
        arena: &Arena,
        water_scene: bool,
        calls: &mut u32,
        triangles: &mut u64,
    ) {
        let mut bound: Option<(Kind, bool, bool)> = None;
        let mut bound_page: Option<(usize, usize)> = None;
        for (k, d) in draws.iter().enumerate() {
            let instance = (first_slot + k as u64) as u32;
            let (mesh, ranges) = self.mesh_of(&d.mesh);
            // Each WebGPU call crosses from WASM to JS: set only what changes.
            let key = (d.kind, d.water, d.no_depth);
            if bound != Some(key) {
                pass.set_pipeline(if water_scene && d.water {
                    &self.water_scene_pipeline
                } else {
                    &self.pipelines[&key]
                });
                bound = Some(key);
            }
            // A page holds many meshes: bound once for the draws from it in a row.
            let page = (mesh.region.format, mesh.region.page);
            if bound_page != Some(page) {
                let buffer = arena.buffer(page.0, page.1);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.set_index_buffer(buffer.slice(..), wgpu::IndexFormat::Uint32);
                bound_page = Some(page);
            }
            for r in merge_ranges(ranges) {
                *triangles += u64::from(r.len() as u32 / 3);
                let first = mesh.first_index;
                pass.draw_indexed(
                    first + r.start..first + r.end,
                    mesh.base_vertex,
                    instance..instance + 1,
                );
                *calls += 1;
            }
        }
    }

    /// A raster scene pass (or a split pass's opaque half, `keep_depth`) into a side's targets.
    fn scene_pass<'a>(
        encoder: &'a mut wgpu::CommandEncoder,
        targets: &'a Targets,
        i: usize,
        ops: frame::PassOps,
        keep_depth: bool,
        timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'a>>,
    ) -> wgpu::RenderPass<'a> {
        Self::begin_scene_pass(
            encoder,
            targets,
            if i == 0 { "far" } else { "near" },
            ops,
            wgpu::LoadOp::Clear(1.0),
            keep_depth,
            timestamp_writes,
        )
    }

    /// A pass into a side's scene targets: `ops` for the colour and view depth, `depth` for the
    /// depth buffer (every scene pass clears it: each has its own projection).
    fn begin_scene_pass<'a>(
        encoder: &'a mut wgpu::CommandEncoder,
        targets: &'a Targets,
        label: &'static str,
        ops: frame::PassOps,
        depth: wgpu::LoadOp<f32>,
        keep_depth: bool,
        timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'a>>,
    ) -> wgpu::RenderPass<'a> {
        let clear_or_load = if ops.clear {
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
        } else {
            wgpu::LoadOp::Load
        };
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment {
                    view: targets.scene_colour(),
                    depth_slice: None,
                    resolve_target: ops.resolve.then_some(&targets.colour.1),
                    ops: wgpu::Operations {
                        load: clear_or_load,
                        store: if ops.store_colour {
                            wgpu::StoreOp::Store
                        } else {
                            wgpu::StoreOp::Discard
                        },
                    },
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &targets.view_depth.1,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: clear_or_load,
                        store: wgpu::StoreOp::Store,
                    },
                }),
            ],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &targets.depth.1,
                depth_ops: Some(wgpu::Operations {
                    load: depth,
                    store: if keep_depth {
                        wgpu::StoreOp::Store
                    } else {
                        wgpu::StoreOp::Discard
                    },
                }),
                stencil_ops: None,
            }),
            timestamp_writes,
            ..Default::default()
        })
    }

    /// A split scene pass's water half (W3): over its opaque half's colour and depth, reading the
    /// view depth (so not attaching it).
    fn water_pass<'a>(
        encoder: &'a mut wgpu::CommandEncoder,
        targets: &'a Targets,
        ops: frame::PassOps,
        timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'a>>,
    ) -> wgpu::RenderPass<'a> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("water"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: targets.scene_colour(),
                depth_slice: None,
                resolve_target: ops.resolve.then_some(&targets.colour.1),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: if ops.store_colour {
                        wgpu::StoreOp::Store
                    } else {
                        wgpu::StoreOp::Discard
                    },
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &targets.depth.1,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes,
            ..Default::default()
        })
    }

    /// The chunk at a world position (tests and tools).
    pub fn has_chunk_at(&self, x: i32, y: i32, z: i32) -> bool {
        self.terrain.chunks.contains_key(&chunk_of(x, y, z))
    }
}

/// The camera as a pass's culling sees it: where it is, its view axis, and how much farther than
/// its depth a point in the view can be (1 / the cosine of the angle out to a corner of the view).
#[derive(Clone, Copy, Debug)]
struct PassView {
    eye: V3,
    forward: V3,
    stretch: f64,
}

impl PassView {
    fn new(eye: V3, forward: V3, fov_y_deg: f64, aspect: f64) -> Self {
        let tan_y = (fov_y_deg.to_radians() / 2.0).tan();
        let tan_x = tan_y * aspect;
        Self {
            eye,
            forward,
            stretch: (1.0 + tan_x * tan_x + tan_y * tan_y).sqrt(),
        }
    }

    /// Whether the box [origin, origin + size]³ has anything a pass drawing view depths [near, far]
    /// draws. The passes clip by depth along the view axis, not by distance: a box beside the view
    /// can be farther than `far` and still in front of the far plane.
    #[cfg(test)]
    fn holds(&self, origin: V3, size: f64, near: f64, far: f64) -> bool {
        self.holds_box(origin, [size; 3], near, far)
    }

    /// [`Self::holds`] for the box [origin, origin + size] (m), sides per axis.
    fn holds_box(&self, origin: V3, size: V3, near: f64, far: f64) -> bool {
        let (mut d2, mut z_lo, mut z_hi) = (0.0, 0.0, 0.0);
        for (((o, e), f), s) in origin.iter().zip(self.eye).zip(self.forward).zip(size) {
            let lo = o - e;
            let hi = lo + s;
            let d = if lo > 0.0 {
                lo
            } else if hi < 0.0 {
                -hi
            } else {
                0.0
            };
            d2 += d * d;
            let (a, b) = (lo * f, hi * f);
            z_lo += a.min(b);
            z_hi += a.max(b);
        }
        // Nothing in the view at depth ≤ far is farther than far × stretch.
        z_lo <= far && z_hi >= near && d2.sqrt() <= far * self.stretch
    }
}

/// The frame group's buffers 3 and 4, written once: the tile grids and the fluid looks
/// (generated from the registry, `water::pack_fluids`: fixed per build).
pub(crate) fn frame_extras(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::Buffer, wgpu::Buffer) {
    let tile_grids: Vec<f32> = dwell_mesh::tile_grids().into_iter().flatten().collect();
    let tile_grid_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("tile grids"),
        size: tile_grids.len() as u64 * 4,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&tile_grid_buffer, 0, floats(&tile_grids));
    let fluid_data = water::pack_fluids(&dwell_blocks::FLUIDS);
    let fluid_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fluids"),
        size: (fluid_data.len() * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&fluid_buffer, 0, floats(&fluid_data));
    (tile_grid_buffer, fluid_buffer)
}

/// The plants' gust field repeats every this many metres (`WIND_PERIOD` in the shader): the eye's
/// position is passed modulo it.
const WIND_PERIOD: f64 = 1024.0;

/// Floats in the frame uniform block (`Frame` in shaders.rs): a matrix, 22 vec4s (the outline's
/// three; after `tan` the clouds' three, then the water's, the wind's six, the wrap and the
/// reflections' two) and the day/night cycle's block.
pub(crate) const FRAME_FLOATS: usize =
    16 + dwell_look::outline::OUTLINE_FLOATS + 19 * 4 + DAY_SKY_FLOATS;

/// The atlas and its mipmaps, each with its side in texels: down to the level where a tile's gutter
/// is one texel wide. Filtering a smaller level at a tile's edge would reach past the gutter into
/// the neighbouring tiles and the atlas's empty (transparent) space, which a grazing view of
/// distant ground samples: a grey, see-through grid along the block edges.
fn atlas_levels(set: dwell_mesh::TextureSet) -> Vec<(Vec<u8>, usize)> {
    let mut levels = vec![(dwell_mesh::build_atlas_in(set), dwell_mesh::ATLAS_SIZE)];
    while let Some(&(ref level, s)) = levels.last()
        && s > dwell_mesh::ATLAS_SIZE / dwell_mesh::GUTTER
    {
        levels.push((downsample(level, s), s / 2));
    }
    preserve_coverage(set, &mut levels);
    levels
}

/// The alpha a cutout texel must reach to be drawn (the shader's edge, 0.5).
const CUTOUT_EDGE: f64 = 127.5;

/// The share of a tile's texels at mip level (`texels`, `size`) that a cutout draw covers (alpha
/// at least the edge) with the alpha scaled by `scale`.
fn tile_coverage(texels: &[u8], size: usize, tile: usize, scale: f64) -> f64 {
    let k = dwell_mesh::ATLAS_SIZE / size;
    let cell = dwell_mesh::CELL;
    let (x0, y0) = (
        ((tile % dwell_mesh::ATLAS_CELLS) * cell + dwell_mesh::GUTTER) / k,
        ((tile / dwell_mesh::ATLAS_CELLS) * cell + dwell_mesh::GUTTER) / k,
    );
    let n = (dwell_mesh::TILE / k).max(1);
    let mut covered = 0usize;
    for y in y0..y0 + n {
        for x in x0..x0 + n {
            if f64::from(texels[(y * size + x) * 4 + 3]) * scale >= CUTOUT_EDGE {
                covered += 1;
            }
        }
    }
    covered as f64 / (n * n) as f64
}

/// Averaging alpha down the mips thins cutout texels out (a blade a texel wide averages to half
/// alpha and fails the test), so distant grass and ladders would vanish. Each level of a tile with
/// transparent texels has its alpha scaled so the share it covers matches the full-size tile's
/// (the coverage-preserving mipmaps of Castaño, 2010); opaque tiles stay untouched.
fn preserve_coverage(set: dwell_mesh::TextureSet, levels: &mut [(Vec<u8>, usize)]) {
    for (tile, name) in dwell_mesh::TILE_ORDER.iter().enumerate() {
        if !dwell_mesh::tile_has_alpha_in(set, name) {
            continue;
        }
        let want = tile_coverage(&levels[0].0, levels[0].1, tile, 1.0);
        for (texels, size) in levels.iter_mut().skip(1) {
            // Coverage grows with the scale: the smallest scale that reaches the full-size share
            // (up to 255, which lifts any texel with alpha to full).
            let (mut lo, mut hi) = (0.0f64, 255.0f64);
            for _ in 0..24 {
                let mid = 0.5 * (lo + hi);
                if tile_coverage(texels, *size, tile, mid) >= want {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            // Coverage moves in steps: take whichever side of the step lands nearer, but never the
            // side that empties the tile. At the last levels a tile is 2 × 2 texels whose alphas
            // can be all alike (the painterly ladder's), so the step goes from nothing to all:
            // "nearer" would be nothing, and distant ladders would vanish.
            let below = tile_coverage(texels, *size, tile, lo);
            let above = tile_coverage(texels, *size, tile, hi);
            let empties = below == 0.0 && want > 0.0;
            let scale = if !empties && want - below < above - want {
                lo
            } else {
                hi
            };
            let k = dwell_mesh::ATLAS_SIZE / *size;
            let cell = dwell_mesh::CELL / k;
            let (cx, cy) = (
                (tile % dwell_mesh::ATLAS_CELLS) * cell,
                (tile / dwell_mesh::ATLAS_CELLS) * cell,
            );
            for y in cy..cy + cell {
                for x in cx..cx + cell {
                    let a = &mut texels[(y * *size + x) * 4 + 3];
                    *a = (f64::from(*a) * scale).round().min(255.0) as u8;
                }
            }
        }
    }
}

/// Halves an RGBA8 sRGB image, averaging in linear light.
fn downsample(src: &[u8], size: usize) -> Vec<u8> {
    let half = (size / 2).max(1);
    let to_linear = |b: u8| srgb_to_linear(f64::from(b) / 255.0);
    let to_srgb = |l: f64| {
        let s = if l <= 0.0031308 {
            l * 12.92
        } else {
            1.055 * l.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round().clamp(0.0, 255.0) as u8
    };
    let mut out = vec![0u8; half * half * 4];
    for y in 0..half {
        for x in 0..half {
            for c in 0..4 {
                let at = |dx: usize, dy: usize| {
                    src[((2 * y + dy).min(size - 1) * size + (2 * x + dx).min(size - 1)) * 4 + c]
                };
                let px = [at(0, 0), at(1, 0), at(0, 1), at(1, 1)];
                out[(y * half + x) * 4 + c] = if c == 3 {
                    (px.iter().map(|&p| u32::from(p)).sum::<u32>() / 4) as u8
                } else {
                    to_srgb(px.iter().map(|&p| to_linear(p)).sum::<f64>() / 4.0)
                };
            }
        }
    }
    out
}

impl TerrainRenderer for Renderer {
    fn set_chunk(&mut self, coord: ChunkCoord, payload: Option<&ChunkMeshes>) {
        self.terrain.set_chunk(coord, payload);
    }
    fn set_section(
        &mut self,
        id: i64,
        origin: V3,
        cell_size: f64,
        payload: Option<&SectionMeshes>,
    ) {
        self.terrain.set_section(id, origin, cell_size, payload);
    }
    fn show_sections(&mut self, visible: &[(i64, u8)]) {
        self.terrain.show_sections(visible);
    }
    fn show_stand_ins(&mut self, stand_ins: &[StandIn]) {
        self.terrain.show_stand_ins(stand_ins);
    }
    fn clear(&mut self) {
        self.terrain.clear();
    }
    fn gpu_bytes(&self) -> u64 {
        self.terrain.gpu_bytes()
    }
}

impl crate::game::Scene for Renderer {
    fn set_player(&mut self, id: u32, view: Option<PlayerView>) {
        Renderer::set_player(self, id, view);
    }
    fn set_arms(&mut self, arms: Option<ArmsView>) {
        Renderer::set_arms(self, arms);
    }
    fn set_camera(&mut self, eye: V3, yaw_deg: f64, pitch_deg: f64, face: FaceFrame, flip: f64) {
        Renderer::set_camera(self, eye, yaw_deg, pitch_deg, face, flip);
    }
    fn set_eye_fluid(&mut self, eye_fluid: Option<EyeFluid>) {
        Renderer::set_eye_fluid(self, eye_fluid);
    }
    fn set_day(&mut self, day: &DayState, seconds: f64) {
        Renderer::set_day(self, day, seconds);
    }
    fn set_block_outline(&mut self, cell: Option<V3>, bounds: ([f32; 3], [f32; 3])) {
        Renderer::set_block_outline(self, cell, bounds);
    }
    fn set_region_boxes(&mut self, boxes: &[RegionBox]) {
        Renderer::set_region_boxes(self, boxes);
    }
    fn set_brush_outline(&mut self, outline: Option<&BrushOutline>) {
        Renderer::set_brush_outline(self, outline);
    }
    fn set_debug_lines(&mut self, lines: Option<&[Line]>) {
        Renderer::set_debug_lines(self, lines);
    }
    fn set_fov(&mut self, fov_y_deg: Option<f64>) {
        Renderer::set_fov(self, fov_y_deg);
    }
    fn set_wind(&mut self, wind: WindView) {
        Renderer::set_wind(self, wind);
    }
    fn set_magnification(&mut self, magnification: f64) {
        Renderer::set_magnification(self, magnification);
    }
    fn stats(&self) -> RenderStats {
        Renderer::stats(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `Kind` indexes `STRIDES` (the arena's formats) with its own format's stride: a kind
    /// inserted without its stride shifted the later kinds (fluids read the cutout's stride, and
    /// cutout meshes indexed past the end and panicked in `Arena::upload`).
    #[test]
    fn every_kind_has_its_own_stride() {
        let line = 4 * (3 + 3);
        for (kind, stride) in [
            (Kind::Terrain, 4 * (3 + 3 + 3 + 2 + 4)),
            (Kind::Flat, 4 * (3 + 3 + 3)),
            (Kind::Line, line),
            (Kind::Ghost, line),
            (Kind::Fluid, 4 * (3 + 3 + 3 + 1)),
            (Kind::Cutout, 4 * (3 + 3 + 3 + 2 + 4 + 4 + 1)),
        ] {
            assert_eq!(STRIDES.get(kind as usize), Some(&stride), "{kind:?}");
        }
    }

    #[test]
    fn the_water_uniform_carries_the_eye_fluid_and_the_water_quality() {
        assert_eq!(water_uniform(None, WaterQuality::Low), [0.0; 4]);
        let e = EyeFluid {
            fluid: 0,
            surface_y: -12.125,
        };
        assert_eq!(
            water_uniform(Some(e), WaterQuality::High),
            [1.0, 0.0, -12.125, 2.0]
        );
        assert_eq!(water_uniform(None, WaterQuality::Medium)[3], 1.0);
    }

    #[test]
    fn draw_order_opaque_near_to_far_by_pipeline_then_water_far_to_near_then_lines() {
        let item = |kind: Kind, water: bool, dist: f32, page: usize| DrawItem {
            kind,
            water,
            no_depth: false,
            offset: [dist, 0.0, 0.0, 1.0],
            clip: None,
            tint: [1.0; 4],
            rotation: NO_ROTATION,
            mesh: DrawMesh::Outline,
            page,
            passes: 1,
            key: 0,
        };
        let mut items = [
            item(Kind::Line, false, 1.0, 0),
            item(Kind::Flat, true, 5.0, 0),
            item(Kind::Flat, false, 50.0, 1),
            item(Kind::Terrain, false, 30.0, 0),
            item(Kind::Flat, true, 50.0, 0),
            item(Kind::Flat, false, 20.0, 1),
            item(Kind::Terrain, false, 10.0, 0),
            item(Kind::Flat, false, 90.0, 0),
        ];
        for d in &mut items {
            d.key = draw_order(d);
        }
        items.sort_by_key(|d| d.key);
        let got: Vec<(bool, f32, usize)> = items
            .iter()
            .map(|d| (d.water, d.offset[0], d.page))
            .collect();
        assert_eq!(
            got,
            vec![
                (false, 10.0, 0), // terrain, near to far
                (false, 30.0, 0),
                (false, 90.0, 0), // flat, page 0 then page 1, each near to far
                (false, 20.0, 1),
                (false, 50.0, 1),
                (true, 50.0, 0), // water far to near
                (true, 5.0, 0),
                (false, 1.0, 0), // lines last
            ]
        );
    }

    #[test]
    fn touching_index_ranges_draw_as_one() {
        assert_eq!(
            merge_ranges(vec![0..6, 6..9, 9..9, 12..15, 15..18]),
            vec![0..9, 12..18]
        );
        assert!(merge_ranges(vec![3..3, 5..5]).is_empty());
    }

    #[test]
    fn streams_interleave_vertex_by_vertex() {
        let p = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let uv = [7.0, 8.0, 9.0, 10.0];
        assert_eq!(
            interleave(&[(&p, 3), (&uv, 2)]),
            vec![1.0, 2.0, 3.0, 7.0, 8.0, 4.0, 5.0, 6.0, 9.0, 10.0]
        );
    }

    #[test]
    fn software_adapters_draw_without_multisampling() {
        assert_eq!(samples_for(wgpu::DeviceType::Cpu), 1);
        assert_eq!(samples_for(wgpu::DeviceType::DiscreteGpu), SAMPLES);
        assert_eq!(samples_for(wgpu::DeviceType::IntegratedGpu), SAMPLES);
    }

    #[test]
    fn a_cpu_adapter_draws_no_clouds() {
        // Main's e2e suite (SwiftShader) slowed until two-client and walking tests timed out once
        // the clouds landed (0.11.10): their noise ran per pixel on the CPU.
        for q in [CloudQuality::Off, CloudQuality::Low, CloudQuality::High] {
            assert_eq!(clouds_for(wgpu::DeviceType::Cpu, q), CloudQuality::Off);
            assert_eq!(clouds_for(wgpu::DeviceType::DiscreteGpu, q), q);
            assert_eq!(clouds_for(wgpu::DeviceType::IntegratedGpu, q), q);
        }
        // And the renderer on one sends the shader zero octaves (the deck off).
        let mut r = noop_renderer();
        r.set_adapter(wgpu::DeviceType::Cpu);
        r.set_clouds(CloudQuality::High);
        let u = r.frame_uniforms(&aim(0.0, 0.0, FaceFrame::A, 0.0), 1.0, 100.0);
        // The clouds' second vec4 (`cloud_cfg`: coverage, octaves, …) follows `clouds`, after tan.
        // (Then the clouds' third, the water's, the wind's six, the wrap and the reflections' two.)
        let cfg = FRAME_FLOATS - DAY_SKY_FLOATS - 8 - 4 - 6 * 4 - 4 - 4 - 4;
        assert_eq!(u[cfg + 1], 0.0, "no octaves on a CPU adapter");
        r.set_adapter(wgpu::DeviceType::DiscreteGpu);
        let u = r.frame_uniforms(&aim(0.0, 0.0, FaceFrame::A, 0.0), 1.0, 100.0);
        assert!(u[cfg + 1] > 0.0, "the setting's octaves on a GPU");
    }

    #[test]
    fn a_cpu_adapter_draws_the_cheapest_water_and_no_motion_blur() {
        // CI's SwiftShader: with clouds off the two-client and friend-world tests still timed out
        // on slower runners; every per-pixel cost there is the main thread's time.
        let mut r = noop_renderer();
        r.set_water_quality(WaterQuality::High);
        r.set_wind(WindView {
            intensity: 1.0,
            flight: 1.0,
            direction: [0.0, 0.0, 1.0],
            time: 1.0,
        });
        let basis = aim(0.0, 0.0, FaceFrame::A, 0.0);
        // After the water's vec4 (w: the quality) come the wind's six (the last's x: the blur),
        // the wrapped eye, the reflections' two and the day sky.
        let water_w = FRAME_FLOATS - DAY_SKY_FLOATS - 8 - 4 - 6 * 4 - 1;
        let blur = FRAME_FLOATS - DAY_SKY_FLOATS - 8 - 4 - 4;
        let u = r.frame_uniforms(&basis, 1.0, 100.0);
        assert_eq!(u[water_w], 2.0, "the setting's water on a GPU");
        assert!(u[blur] > 0.0, "the edge blur on a GPU");
        r.set_adapter(wgpu::DeviceType::Cpu);
        let u = r.frame_uniforms(&basis, 1.0, 100.0);
        assert_eq!(u[water_w], 0.0, "Low water on a CPU adapter");
        assert_eq!(u[blur], 0.0, "no edge blur on a CPU adapter");
    }

    #[test]
    fn a_cpu_adapter_draws_no_stars() {
        let mut r = noop_renderer();
        // Midnight: the stars show.
        r.set_day(
            &dwell_look::daynight::DAY_NIGHT.state(0.0, Some(0.0), None),
            1.0,
        );
        let stars = FRAME_FLOATS - DAY_SKY_FLOATS + DAY_STARS;
        let u = r.frame_uniforms(&aim(0.0, 0.0, FaceFrame::A, 0.0), 1.0, 100.0);
        assert!(u[stars] > 0.0, "stars at midnight on a GPU");
        r.set_adapter(wgpu::DeviceType::Cpu);
        let u = r.frame_uniforms(&aim(0.0, 0.0, FaceFrame::A, 0.0), 1.0, 100.0);
        assert_eq!(u[stars], 0.0, "no stars on a CPU adapter");
    }

    #[test]
    fn a_cpu_adapter_draws_a_smaller_buffer() {
        assert_eq!(pixel_ratio_for(wgpu::DeviceType::DiscreteGpu, 2.0), 2.0);
        assert_eq!(pixel_ratio_for(wgpu::DeviceType::Cpu, 1.0), CPU_RESOLUTION);
        assert_eq!(
            pixel_ratio_for(wgpu::DeviceType::Cpu, 2.0),
            2.0 * CPU_RESOLUTION
        );
        assert_eq!(
            water_for(wgpu::DeviceType::Cpu, WaterQuality::High),
            WaterQuality::Low
        );
        assert_eq!(
            water_for(wgpu::DeviceType::IntegratedGpu, WaterQuality::Medium),
            WaterQuality::Medium
        );
    }

    #[test]
    fn mipmaps_average_in_linear_light() {
        // Black and white pixels average to linear mid-grey (sRGB ~188), not 128.
        let src = vec![
            0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255,
        ];
        let out = downsample(&src, 2);
        assert_eq!(out.len(), 4);
        assert!((186..=190).contains(&out[0]), "{}", out[0]);
        assert_eq!(out[3], 255);
    }

    #[test]
    fn cutout_tiles_keep_their_coverage_down_the_mips() {
        // A cutout tile's share of drawn texels stays the full-size tile's at every level (within a
        // texel's worth at the smallest), so ladders, grass and leaves don't thin out or vanish with
        // distance, in every texture set (the painterly ladder vanished at mip 4, 0.20.6).
        for set in dwell_mesh::TextureSet::ALL {
            let levels = atlas_levels(set);
            let mut checked = 0;
            for (tile, name) in dwell_mesh::TILE_ORDER.iter().enumerate() {
                if !dwell_mesh::tile_has_alpha_in(set, name) {
                    continue;
                }
                checked += 1;
                let want = tile_coverage(&levels[0].0, levels[0].1, tile, 1.0);
                assert!(want > 0.05 && want < 0.95, "{set:?} {name}: {want}");
                for (level, (texels, size)) in levels.iter().enumerate() {
                    let n = (dwell_mesh::TILE * size / dwell_mesh::ATLAS_SIZE).max(1);
                    let got = tile_coverage(texels, *size, tile, 1.0);
                    // Within the step a regular pattern (rungs) moves in; at the last levels, a
                    // tile of 2 × 2 texels is all or nothing, and must not be nothing.
                    if n >= 4 {
                        assert!(
                            (got - want).abs() <= 0.15,
                            "{set:?} {name} at mip {level}: {got} vs {want}"
                        );
                    } else {
                        assert!(got > 0.0, "{set:?} {name} vanishes at mip {level}");
                    }
                }
                // Plain averaging would have lost a sparse tile (the ladder): the unscaled level 2
                // covers much less. A mostly solid tile with scattered holes (the leaves) fills in.
                let plain = downsample(&downsample(&levels[0].0, levels[0].1), levels[0].1 / 2);
                let lost = tile_coverage(&plain, levels[0].1 / 4, tile, 1.0);
                if want < 0.5 {
                    assert!(
                        lost < want,
                        "{set:?} {name}: plain mips keep {lost} of {want}"
                    );
                }
            }
            assert!(
                checked >= 3,
                "{set:?}: the ladder, grass and leaves are cutout"
            );
        }
    }

    /// Bilinear filtering of mip level (`texels`, `size`) at atlas coordinates (u, v), as the GPU
    /// samples it (texel centres at half-integers, clamped at the edge), in 0–255 per channel.
    fn bilinear(texels: &[u8], size: usize, u: f64, v: f64) -> [f64; 4] {
        let (x, y) = (u * size as f64 - 0.5, v * size as f64 - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |x: f64, y: f64, c: usize| {
            let x = (x.max(0.0) as usize).min(size - 1);
            let y = (y.max(0.0) as usize).min(size - 1);
            f64::from(texels[(y * size + x) * 4 + c])
        };
        std::array::from_fn(|c| {
            let top = at(x0, y0, c) * (1.0 - fx) + at(x0 + 1.0, y0, c) * fx;
            let bottom = at(x0, y0 + 1.0, c) * (1.0 - fx) + at(x0 + 1.0, y0 + 1.0, c) * fx;
            top * (1.0 - fy) + bottom * fy
        })
    }

    #[test]
    fn filtering_any_mip_level_at_a_tiles_edge_stays_on_that_tile() {
        // A block's texture is sampled up to its tile's edges (fract(uv)), and the bilinear filter
        // reaches half a texel past the sample: at every level the gutter must cover that, or
        // distant ground picks up the neighbouring tiles and the atlas's empty space along every
        // block edge (the grey, see-through grid near the horizon, playtest 2026-10-14).
        for set in dwell_mesh::TextureSet::ALL {
            let levels = atlas_levels(set);
            for (t, name) in dwell_mesh::TILE_ORDER.iter().enumerate() {
                let r = dwell_mesh::tile_rect(name);
                let cell = [
                    (t % dwell_mesh::ATLAS_CELLS) * dwell_mesh::CELL,
                    (t / dwell_mesh::ATLAS_CELLS) * dwell_mesh::CELL,
                ];
                for (level, (texels, size)) in levels.iter().enumerate() {
                    let texel = dwell_mesh::ATLAS_SIZE / size;
                    for (u, v) in [(r.u0, r.v0), (r.u1, r.v0), (r.u0, r.v1), (r.u1, r.v1)] {
                        // The filter's 2 × 2 texels lie in the tile's cell (the tile and its gutter).
                        for (k, w) in [u, v].into_iter().enumerate() {
                            let first = (w * *size as f64 - 0.5).floor();
                            let (lo, hi) = (first as usize * texel, (first as usize + 2) * texel);
                            assert!(
                                first >= 0.0 && lo >= cell[k] && hi <= cell[k] + dwell_mesh::CELL,
                                "{name} at mip {level}: filtering ({u}, {v}) reaches past its cell"
                            );
                        }
                        let got = bilinear(texels, *size, u, v);
                        if !dwell_mesh::tile_has_alpha_in(set, name) {
                            assert_eq!(got[3], 255.0, "{set:?} {name} at mip {level}, ({u}, {v})");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_pass_draws_what_its_depth_range_holds_at_the_edge_of_the_view() {
        // Looking along +z, a box 900 m ahead and 600 m to the side is nearer than the split in
        // view depth (where the near pass draws it and the far pass clips it), though more than the
        // split away in a straight line: it must be in the near pass, or neither pass draws it (the
        // gaps in the ground seen from high up, playtest 2026-10-14).
        let view = PassView::new([0.0; 3], [0.0, 0.0, 1.0], vertical_fov(2.0), 2.0);
        let origin = [600.0, -16.0, 884.0];
        assert!(view.holds(origin, 32.0, NEAR_PLANE, NEAR_SPLIT));
        assert!(!view.holds(origin, 32.0, NEAR_SPLIT * 0.95, FAR_PLANE));
        // Beyond the split in depth: only the far pass.
        let beyond = [600.0, -16.0, 1100.0];
        assert!(!view.holds(beyond, 32.0, NEAR_PLANE, NEAR_SPLIT));
        assert!(view.holds(beyond, 32.0, NEAR_SPLIT * 0.95, FAR_PLANE));
        // Behind the eye, or too far aside to be in the view at all: neither.
        assert!(!view.holds([0.0, 0.0, -100.0], 32.0, NEAR_PLANE, NEAR_SPLIT));
        assert!(!view.holds([5000.0, 0.0, 500.0], 32.0, NEAR_PLANE, NEAR_SPLIT));
    }

    fn noop_renderer() -> Renderer {
        noop_renderer_with(1, wgpu::Features::empty())
    }

    fn noop_renderer_with(samples: u32, features: wgpu::Features) -> Renderer {
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor {
            required_features: features,
            ..Default::default()
        });
        Renderer::with_samples(device, queue, wgpu::TextureFormat::Rgba8Unorm, samples)
    }

    /// Whether submitting a pass that binds `b` (a vertex or index buffer) fails because it was
    /// destroyed.
    fn destroyed_on(device: &wgpu::Device, queue: &wgpu::Queue, b: &wgpu::Buffer) -> bool {
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations::default(),
                })],
                ..Default::default()
            });
            if b.usage().contains(wgpu::BufferUsages::INDEX) {
                pass.set_index_buffer(b.slice(..), wgpu::IndexFormat::Uint32);
            } else {
                pass.set_vertex_buffer(0, b.slice(..));
            }
        }
        queue.submit([encoder.finish()]);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        match std::pin::pin!(scope.pop()).as_mut().poll(&mut cx) {
            std::task::Poll::Ready(e) => e.is_some_and(|e| e.to_string().contains("destroyed")),
            std::task::Poll::Pending => panic!("error scope pending"),
        }
    }

    #[test]
    fn replaced_meshes_free_their_gpu_memory() {
        // Every replaced or removed chunk, section and debug-lines mesh must give its GPU memory
        // back, or memory grows and the frame rate falls over a session (playtest 2026-10-14).
        // Meshes live in arena pages (arena.rs): replacing them over and over reuses the same
        // pages, and removing them all leaves nothing in use.
        let mut r = noop_renderer();
        let outline = r.arena.borrow().used_bytes();
        let mut padded = vec![0u16; dwell_mesh::PADDED_VOLUME];
        for x in -1..=32 {
            for z in -1..=32 {
                padded[dwell_mesh::padded_index(x, 0, z)] = dwell_blocks::require("dwell:stone");
            }
        }
        let meshes = dwell_mesh::mesh_chunk(&padded, false).expect("meshed");
        let quad = FlatMesh {
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            normals: vec![0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0],
            colors: vec![1.0; 9],
            fluids: Vec::new(),
            indices: vec![0, 1, 2],
        };
        let section = SectionMeshes {
            opaque: quad.clone(),
            water: quad,
            skirts: Default::default(),
        };
        let line = Line {
            from: [0.0; 3],
            to: [1.0; 3],
            color: 0xffffff,
        };
        let replace = |r: &mut Renderer| {
            for x in 0..4 {
                r.set_terrain_chunk(ChunkCoord { x, y: 0, z: 0 }, Some(&meshes));
                r.set_lod_section(i64::from(x), [0.0; 3], 2.0, Some(&section));
            }
            r.set_debug_lines(Some(&[line]));
        };
        replace(&mut r);
        let (pages, used) = (r.arena.borrow().page_bytes(), r.arena.borrow().used_bytes());
        for _ in 0..500 {
            replace(&mut r);
        }
        assert_eq!(
            r.arena.borrow().page_bytes(),
            pages,
            "replacing reuses the pages"
        );
        assert_eq!(
            r.arena.borrow().used_bytes(),
            used,
            "replaced meshes are freed"
        );
        for x in 0..4 {
            r.set_terrain_chunk(ChunkCoord { x, y: 0, z: 0 }, None);
            r.set_lod_section(i64::from(x), [0.0; 3], 2.0, None);
        }
        r.set_debug_lines(None);
        assert_eq!(
            r.arena.borrow().used_bytes(),
            outline,
            "only the block outline is left"
        );
        // The pages themselves are destroyed with the renderer (wgpu's WebGPU backend does not
        // destroy a buffer on drop).
        let pages = r.arena.borrow().buffers();
        let (device, queue) = (r.device.clone(), r.queue.clone());
        drop(r);
        for p in &pages {
            assert!(
                destroyed_on(&device, &queue, p),
                "a page outlived the renderer"
            );
        }
    }

    #[test]
    fn frames_draw_cleanly_across_resizes_and_replaced_meshes() {
        // Destroying the old targets, draw uniforms and meshes between frames is valid WebGPU; so
        // is the shared frame (offscreen colour and view depth, the composite, lines over it) with
        // and without MSAA, timed or not.
        for (samples, features) in [
            (1, wgpu::Features::empty()),
            (4, wgpu::Features::empty()),
            (4, wgpu::Features::TIMESTAMP_QUERY),
        ] {
            draws_cleanly(noop_renderer_with(samples, features));
        }
    }

    fn draws_cleanly(mut r: Renderer) {
        let scope = r.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let target = r.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let mut padded = vec![0u16; dwell_mesh::PADDED_VOLUME];
        for x in -1..=32 {
            for z in -1..=32 {
                padded[dwell_mesh::padded_index(x, 0, z)] = dwell_blocks::require("dwell:grass");
                padded[dwell_mesh::padded_index(x, 1, z)] = dwell_blocks::require("dwell:water");
            }
        }
        let meshes = dwell_mesh::mesh_chunk(&padded, false).expect("meshed");
        assert!(!meshes.transparent.indices.is_empty(), "a water surface");
        // Chunk water and a section's water (the fluid pipeline: its fluid ids missing, so drawn as
        // no fluid's), the outline and debug lines (the overlay).
        let quad = FlatMesh {
            positions: vec![0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0],
            normals: vec![0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0],
            colors: vec![1.0; 9],
            fluids: Vec::new(),
            indices: vec![0, 1, 2],
        };
        let section = SectionMeshes {
            opaque: quad.clone(),
            water: quad,
            skirts: Default::default(),
        };
        r.set_lod_section(7, [0.0; 3], 1.0, Some(&section));
        r.show_lod_sections(&[(7, 0)]);
        r.set_block_outline(Some([16.0, 0.0, 16.0]), ([0.0; 3], [1.0; 3]));
        r.set_debug_lines(Some(&[Line {
            from: [16.0, 1.0, 16.0],
            to: [20.0, 1.0, 20.0],
            color: 0xff0000,
        }]));
        r.set_camera([16.0, 4.0, 16.0], 0.0, -30.0, FaceFrame::A, 0.0);
        // Characters: a body holding the brush, one on face B holding a block, a dead one, and
        // your arms (their own pass).
        let grass = dwell_blocks::require("dwell:grass");
        for (id, feet, face, dead, held) in [
            (
                1,
                [16.0, 1.0, 20.0],
                FaceFrame::A,
                false,
                Held::Brush(grass),
            ),
            (
                2,
                [18.0, -3.0, 20.0],
                FaceFrame::B,
                false,
                Held::Block(grass),
            ),
            (3, [14.0, 1.0, 20.0], FaceFrame::A, true, Held::Empty),
        ] {
            r.set_player(
                id,
                Some(PlayerView {
                    feet,
                    face,
                    yaw: 30.0,
                    dead,
                    look: dwell_characters::DEFAULT_LOOK,
                    pose: Default::default(),
                    role: "idle",
                    action: None,
                    held,
                }),
            );
        }
        r.set_arms(Some(ArmsView {
            pose: Default::default(),
            held: Held::Block(grass),
        }));
        // A boosted FOV and the wind (streaks, vignette, edge blur in the composite) too.
        r.set_fov(Some(85.0));
        r.set_wind(WindView {
            intensity: 0.8,
            flight: 0.5,
            direction: [0.0, 0.0, 1.0],
            time: 1.0,
        });
        for frame in 0..4 {
            r.resize(64, 64, 1.0 + f64::from(frame));
            // Under water on odd frames (the composite's underwater branch), every quality.
            r.set_eye_fluid((frame % 2 == 1).then_some(EyeFluid {
                fluid: 0,
                surface_y: 5.0,
            }));
            r.set_water_quality(water::WaterQuality::from_level(frame as u32));
            for x in 0..=frame {
                r.set_terrain_chunk(ChunkCoord { x, y: 0, z: 0 }, Some(&meshes));
            }
            r.render(&view, &|_| true);
            r.device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("polled");
        }
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        match std::pin::pin!(scope.pop()).as_mut().poll(&mut cx) {
            std::task::Poll::Ready(e) => assert!(e.is_none(), "{e:?}"),
            std::task::Poll::Pending => panic!("error scope pending"),
        }
    }

    #[test]
    fn timed_frames_recycle_their_readback_buffers() {
        // A frame's readback buffer is in use until a later frame reads it (with all of them in
        // use a frame goes untimed rather than wait: `timing::Ring`'s test); frame after frame
        // they are read and reused, so timing keeps going.
        let mut r = noop_renderer_with(4, wgpu::Features::TIMESTAMP_QUERY);
        let target = r.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 8,
                height: 8,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        r.resize(8, 8, 1.0);
        for frame in 0..4 * timing::RING {
            r.render(&view, &|_| true);
            r.device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("polled");
            assert!(
                r.timer.as_ref().expect("timed").in_use() < timing::RING,
                "frame {frame}"
            );
        }
        assert!(noop_renderer().timer.is_none(), "no timestamps, no timer");
    }

    #[test]
    fn the_frame_uses_the_fov_the_game_chose() {
        // The projection, the sky's rays and each pass's culling all use the frame's FOV.
        let mut r = noop_renderer();
        r.resize(1600, 900, 1.0);
        assert_eq!(r.frame_fov(), vertical_fov(16.0 / 9.0));
        r.set_fov(Some(80.0));
        assert_eq!(r.frame_fov(), 80.0);
        let basis = aim(0.0, 0.0, FaceFrame::A, 0.0);
        let u = r.frame_uniforms(&basis, 1.0, 100.0);
        let f = 1.0 / (80f64.to_radians() / 2.0).tan();
        assert!((f64::from(u[5]) - f).abs() < 1e-5, "projection's y scale");
        // `tan` is the vec4 before the clouds' three, the water's, the wind's six, the wrap and the
        // reflections' two (then the day's block).
        let tan_y = f64::from(u[FRAME_FLOATS - DAY_SKY_FLOATS - 13 * 4 - 3]);
        assert!((tan_y - 1.0 / f).abs() < 1e-5, "the sky's rays");
        // The culling's corner stretch widens with the view.
        let aspect = 16.0 / 9.0;
        let narrow = PassView::new([0.0; 3], [0.0, 0.0, 1.0], vertical_fov(aspect), aspect);
        let wide = PassView::new([0.0; 3], [0.0, 0.0, 1.0], 80.0, aspect);
        assert!(wide.stretch > narrow.stretch);
    }

    #[test]
    fn the_wind_reaches_the_composite_through_the_frame_block() {
        let mut r = noop_renderer();
        r.resize(1600, 900, 2.0);
        let basis = aim(0.0, 0.0, FaceFrame::A, 0.0);
        let wind = |r: &Renderer| {
            r.frame_uniforms(&basis, 1.0, 100.0)
                [FRAME_FLOATS - DAY_SKY_FLOATS - 36..FRAME_FLOATS - DAY_SKY_FLOATS - 12]
                .to_vec()
        };
        assert_eq!(wind(&r)[2], 0.0, "no wind: intensity 0");
        // Falling forward: the vanishing point drops below the centre, the intensity is passed.
        r.set_wind(WindView {
            intensity: 0.5,
            flight: 0.0,
            direction: camera::normalize([0.0, -0.5, 1.0]),
            time: 2.0,
        });
        let w = wind(&r);
        assert!(w[1] < -0.3 && w[0].abs() < 1e-6 && w[2] == 0.5 && w[7] == 1.0);
        // Widths are in CSS pixels: doubled on a 2× screen.
        assert!((f64::from(w[15]) - 2.0 * MOTION.wind.width_px).abs() < 1e-5);
    }

    #[test]
    fn the_ink_outline_reaches_the_composite_through_the_frame_block_and_is_off_by_default() {
        use dwell_look::outline::{EDGE_BLOCK, EDGE_DEPTH, EDGE_NORMAL, tap_px};
        let mut r = noop_renderer();
        r.resize(1600, 900, 2.0);
        let basis = aim(0.0, 0.0, FaceFrame::A, 0.0);
        // Right after the matrix: `outline`, `outline_ink`, `outline_cfg`.
        let outline = |r: &Renderer| r.frame_uniforms(&basis, 1.0, 100.0)[16..28].to_vec();
        assert_eq!(r.outline(), None);
        assert_eq!(outline(&r), vec![0.0; 12], "off: the composite skips it");
        r.set_outline_query("?local=1&outline=blocks");
        let m = OUTLINE.mode("blocks").unwrap();
        let u = outline(&r);
        assert_eq!(u[0], (EDGE_DEPTH | EDGE_NORMAL | EDGE_BLOCK) as f32);
        assert_eq!(
            f64::from(u[1]),
            tap_px(m.width_px, 2.0),
            "CSS pixels on a 2x screen"
        );
        assert_eq!(
            (f64::from(u[2]), f64::from(u[3])),
            (m.fade_start_m, m.fade_end_m)
        );
        assert_eq!(
            u[4..8],
            [
                OUTLINE.ink[0] as f32,
                OUTLINE.ink[1] as f32,
                OUTLINE.ink[2] as f32,
                m.strength as f32
            ]
        );
        assert_eq!(
            u[8..10],
            [m.depth_threshold as f32, m.crease_threshold as f32]
        );
        // The spyglass carries the fade out with its magnification.
        r.set_magnification(4.0);
        assert_eq!(f64::from(outline(&r)[3]), m.fade_end_m * 4.0);
        assert!(r.set_outline("silhouette"));
        assert_eq!(outline(&r)[0], EDGE_DEPTH as f32);
        assert!(!r.set_outline("off"));
        assert_eq!(outline(&r), vec![0.0; 12]);
        r.set_outline_query("?outline=unknown");
        assert_eq!(r.outline(), None);
    }

    #[test]
    fn the_reflections_follow_the_water_quality_and_the_clouds_setting() {
        let mut r = noop_renderer();
        r.resize(1280, 720, 1.0);
        let basis = aim(0.0, 0.0, FaceFrame::A, 0.0);
        let refl = |r: &Renderer| {
            r.frame_uniforms(&basis, 1.0, 100.0)
                [FRAME_FLOATS - DAY_SKY_FLOATS - 8..FRAME_FLOATS - DAY_SKY_FLOATS]
                .to_vec()
        };
        r.set_water_quality(WaterQuality::Low);
        let low = refl(&r);
        assert_eq!((low[0], low[4]), (0.0, 0.0), "Low: the sky alone");
        r.set_water_quality(WaterQuality::Medium);
        let m = refl(&r);
        assert_eq!(m[0], REFLECTIONS.tier(1).ssr_steps as f32);
        assert!(m[4] > 0.0, "Medium reflects clouds");
        r.set_water_quality(WaterQuality::High);
        assert_eq!(refl(&r)[0], REFLECTIONS.tier(2).ssr_steps as f32);
        r.set_clouds(CloudQuality::Off);
        assert_eq!(refl(&r)[4], 0.0, "no clouds in the sky, none reflected");
    }

    #[test]
    fn the_frame_block_matches_the_shader_struct() {
        // mat4x4f + the vec4fs + the `DaySky` block in `Frame`.
        let fields = shaders::source(1)
            .split("struct Frame {")
            .nth(1)
            .unwrap()
            .split("};")
            .next()
            .unwrap()
            .matches("vec4f,")
            .count();
        assert_eq!(16 + 4 * fields + DAY_SKY_FLOATS, FRAME_FLOATS);
    }
}
