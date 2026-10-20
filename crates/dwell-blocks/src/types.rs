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

/// A side-face tile rule (block data `sideUnder`, docs/PAINTERLY_TEXTURES.md §2): a side face of
/// the state whose cell above holds block `above` (any state, any derived shape) takes tile `side`,
/// unless that block shows a full side there itself. A look (registry sync tag `SIDE_UNDER`, which
/// older clients ignore).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SideUnder {
    /// Namespaced block id, e.g. `dwell:grass`.
    pub above: &'static str,
    /// The texture tile its side faces take.
    pub side: &'static str,
}

/// World-space grid lines the terrain shader draws on a state's faces (block data `grid`, a look
/// option): a line every `every` blocks (a divisor of the chunk size), darkening by `strength`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub every: u8,
    pub strength: f32,
}

/// How a state's pixels are drawn (block data `render`, docs/TRANSPARENT_BLOCKS.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderMode {
    /// Covers every pixel it draws (cubes).
    Opaque,
    /// Alpha-tested (alpha-to-coverage under MSAA), depth-writing, order-independent: foliage,
    /// ladders.
    Cutout,
    /// Alpha-blended, no depth writes, sorted far to near: water, later glass and ice.
    Translucent,
}

/// The `cross` look's geometry (block data `cross`): `quads` vertical quads through the cell's
/// centre, evenly turned, `height` tall, each cell's copy jittered from its world position by up
/// to `offset` (m, in x and z), `rotation_deg` (about y) and a height scale in
/// [`height_min`, `height_max`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cross {
    pub height: f32,
    pub quads: u8,
    pub offset: f32,
    pub rotation_deg: f32,
    pub height_min: f32,
    pub height_max: f32,
}

/// Wind sway of a cross's top (block data `wind`): `strength` metres at the top, gusts at `speed`
/// (1 = the default gust field's pace).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wind {
    pub strength: f32,
    pub speed: f32,
}

/// Distance fade of a cross (block data `fade`, metres): fully drawn to `start`, gone at `end`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fade {
    pub start: f32,
    pub end: f32,
}

/// Which neighbour a block needs to stay (block data `needsSupport`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Support {
    None,
    /// The cell below it, toward its face's ground (−y on face A, +y on face B).
    Below,
}

/// What a block may stand in (block data `medium`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Medium {
    /// Anything (no rule).
    Any,
    /// Air only: placing it into a liquid is refused, and it never stays flooded.
    Air,
    /// Water only: placed into water cells, holding the water (its `flooded` property true).
    Water,
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
    /// Shaped from its neighbours at use (docs/ADAPTIVE_BLOCKS.md): stored as a cube, its shape as
    /// a top block comes from the adaptive table (`crate::adaptive`).
    pub adaptive: bool,
    /// Blocks with the `fluid` look: index into `FLUIDS` (their opacity is the fluid's).
    pub fluid: Option<FluidId>,
    pub placeable: bool,
    /// The look (meshing and material) the client draws it with, by name.
    pub look: &'static str,
    /// sRGB `0xRRGGBB`.
    pub color: u32,
    pub opacity: f32,
    pub textures: Option<Textures>,
    /// Side tiles chosen by the block above (first match wins); empty: always `textures.side`.
    pub side_under: &'static [SideUnder],
    pub grid: Option<Grid>,
    pub render: RenderMode,
    /// The `cross` look's geometry (other looks: none).
    pub cross: Option<Cross>,
    pub wind: Option<Wind>,
    pub fade: Option<Fade>,
    /// A smaller box (min x, y, z, max x, y, z in the cell) for targeting, line of sight and the
    /// outline; none: its shape, or the whole cell for a block without one.
    pub selection: Option<[f32; 6]>,
    /// Block ids it may be placed on (the cell below it); empty: anywhere.
    pub place_on: &'static [&'static str],
    pub needs_support: Support,
    /// Placing a block into its cell (or against it) replaces it.
    pub replaceable: bool,
    pub medium: Medium,
}

/// A fluid's index in `FLUIDS`, valid for this build's registry.
pub type FluidId = u8;

/// A named fluid look (`fluids` in `shared/blocks/*.json`; docs/BLOCK_REGISTRY.md, "Fluid looks"): looks only.
/// Colours are sRGB `0xRRGGBB`; lengths in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FluidInfo {
    /// Namespaced name, e.g. `dwell:water`.
    pub name: &'static str,
    /// The fixed alpha its blocks are drawn with on the Low water tier (no scene copies).
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
    /// The caustics' brightness (0: none), their cell size (m), and the depth (m) over which
    /// they fade to 1/e.
    pub caustics: f32,
    pub caustic_scale: f32,
    pub caustic_depth: f32,
    /// σ per metre of the light the fluid scatters toward the eye: over a ray's length its lit
    /// body (the scatter colour) replaces what is behind, at the rate of absorption + scattering.
    pub scattering: f32,
    /// How far (m) the scene behind shifts per unit of ripple slope (refraction).
    pub refraction: f32,
    /// The sun's glitter off the ripples' facets.
    pub sparkle: f32,
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
    /// The top as 3 × 3 surface points in quarters of a cell (rows north to south, columns west
    /// to east): corners, edge midpoints and the centre.
    pub points: [[u8; 3]; 3],
    /// The top is the fan of 8 triangles around the centre (a ridge shape), each quadrant split
    /// along its centre–corner diagonal; otherwise two triangles split along `diagonal`.
    pub fan: bool,
    pub inverted: bool,
    /// Top-surface split: 0 NW–SE, 1 NE–SW.
    pub diagonal: u8,
    pub convex: bool,
    /// m³ (a full cell is 1).
    pub volume: f32,
    /// Side profiles for culling: heights (quarters of a cell) at the lower end, the middle and
    /// the higher end of the running coordinate of faces +X, −X, +Z, −Z.
    pub sides: [[u8; 3]; 4],
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
        points: [[0; 3]; 3],
        fan: false,
        inverted: false,
        diagonal: 0,
        convex: true,
        volume: 0.0,
        sides: [[0; 3]; 4],
        full_top: false,
        full_bottom: false,
        min_y: 0.0,
        max_y: 0.0,
    };
}
