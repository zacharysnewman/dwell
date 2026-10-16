//! Types of the generated tables (`blocks.gen.rs`).

/// How a voxel's cell is occupied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoxelShape {
    Empty,
    Full,
    Shaped,
}

/// Compass facing of directional blocks (ladders). North = −Z, east = +X.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facing {
    None,
    North,
    East,
    South,
    West,
}

/// A typed block property: its values are `PROPERTY_VALUES[first_value..][..value_count]`.
#[derive(Clone, Copy, Debug)]
pub struct PropertyDef {
    pub name: &'static str,
    pub first_value: usize,
    pub value_count: usize,
}

/// A block: a namespaced id and the run of runtime state ids it owns.
#[derive(Clone, Copy, Debug)]
pub struct BlockDef {
    pub name: &'static str,
    pub first_state: u16,
    pub state_count: u16,
    pub first_property: usize,
    pub property_count: usize,
}

/// Texture names per face group (the client resolves them); untextured states use `color`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Textures {
    pub top: &'static str,
    pub side: &'static str,
    pub bottom: &'static str,
}

/// World-space grid lines the terrain shader draws on a state's faces (block data `grid`, a look
/// option): a line every `every` blocks (a divisor of the chunk size), darkening by `strength`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub every: u8,
    pub strength: f32,
}

/// One block state.
#[derive(Clone, Copy, Debug)]
pub struct StateInfo {
    /// Canonical string, e.g. `dwell:ladder[facing=north,flooded=false]`.
    pub name: &'static str,
    /// Index into `BLOCKS`.
    pub block: usize,
    pub density_kg_m3: f32,
    pub solid: bool,
    pub indestructible: bool,
    pub shape: VoxelShape,
    /// Index into `SHAPES`.
    pub shape_index: u16,
    pub flooded: bool,
    pub climbable: bool,
    pub facing: Facing,
    pub climb_speed_scale: f32,
    pub liquid: bool,
    /// Blocks with the `fluid` look: index into `FLUIDS` (their opacity is the fluid's).
    pub fluid: Option<FluidId>,
    pub placeable: bool,
    /// The look (meshing and material) the client draws it with, by name.
    pub look: &'static str,
    /// sRGB `0xRRGGBB`.
    pub color: u32,
    pub opacity: f32,
    pub textures: Option<Textures>,
    pub grid: Option<Grid>,
}

/// A fluid's index in `FLUIDS`, valid for this build's registry.
pub type FluidId = u8;

/// A named fluid look (`fluids` in `shared/blocks/*.json`; docs/BLOCK_REGISTRY.md, "Fluid looks"): looks only.
/// Colours are sRGB `0xRRGGBB`; lengths in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FluidInfo {
    /// Namespaced name, e.g. `dwell:water`.
    pub name: &'static str,
    /// The fixed alpha its blocks are drawn with (until absorption replaces it).
    pub opacity: f32,
    /// How far below its cell's top the surface is drawn, in cells (when the cell above is not
    /// the same fluid); on face B, above its cell's bottom.
    pub surface_drop: f64,
    /// Beer–Lambert σ per metre, linear red, green, blue.
    pub absorption: [f32; 3],
    pub scatter: u32,
    pub roughness: f32,
    pub ior: f32,
    pub normal_scale: f32,
    pub wave_scale: f32,
    /// Metres per second.
    pub flow_speed: f32,
    pub foam_depth: f32,
    pub foam_color: u32,
    pub caustics: f32,
}

/// One polygon of a baked shape: `count` points of `v` (metres within the cell).
#[derive(Clone, Copy, Debug)]
pub struct ShapeFace {
    pub tag: u8,
    pub count: u8,
    pub v: [[f32; 3]; 4],
}

/// Baked geometry of a voxel shape (`shared/blocks/shapes.mjs`).
#[derive(Clone, Copy, Debug)]
pub struct ShapeInfo {
    pub first_face: u16,
    pub face_count: u16,
    pub corners: [u8; 4],
    pub inverted: bool,
    /// Top-surface split: 0 NW–SE, 1 NE–SW.
    pub diagonal: u8,
    pub convex: bool,
    /// m³ (a full cell is 1).
    pub volume: f32,
    /// Side profiles for culling: heights (halves) at the lower and higher running coordinate of
    /// faces +X, −X, +Z, −Z.
    pub sides: [[u8; 2]; 4],
    pub full_top: bool,
    pub full_bottom: bool,
    pub min_y: f32,
    pub max_y: f32,
}

impl ShapeInfo {
    pub const EMPTY: Self = Self {
        first_face: 0,
        face_count: 0,
        corners: [0; 4],
        inverted: false,
        diagonal: 0,
        convex: true,
        volume: 0.0,
        sides: [[0; 2]; 4],
        full_top: false,
        full_bottom: false,
        min_y: 0.0,
        max_y: 0.0,
    };
}
