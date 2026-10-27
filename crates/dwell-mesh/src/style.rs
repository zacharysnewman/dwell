//! How block states are drawn (`client/src/world/materials.ts`): each state's look, colour,
//! opacity, render mode, textures, ladder face, shape, flooding, fluid and the cross look, read
//! from the generated registry
//! (`dwell-blocks`), with the TS fallback for ids the registry does not know (a magenta cube).

use std::sync::OnceLock;

use dwell_blocks::{
    Cross, FLUIDS, Facing, Fade, FluidId, FluidInfo, RenderMode, SHAPES, STATES, ShapeInfo,
    SideUnder, StateId, StateInfo, Textures, Wind,
};

/// The look a state is drawn with (the registry's `look` name).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Look {
    Cube,
    Shaped,
    Ladder,
    /// A fluid's look (`"look": "fluid"`): its parameters are the state's `fluid`.
    Fluid,
    /// Crossed vertical quads through the cell (plants).
    Cross,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MaterialStyle {
    pub look: Look,
    /// sRGB `0xRRGGBB`.
    pub color: u32,
    /// How its pixels are drawn: which of the chunk's meshes it goes in.
    pub render: RenderMode,
    pub textures: Option<Textures>,
    /// Its block's id (`dwell:grass`; the state's name without properties), as `side_under` names
    /// the block above.
    pub block: &'static str,
    /// Side tiles chosen by the block above (block data `sideUnder`).
    pub side_under: &'static [SideUnder],
    /// Ladders: the face whose side the plate faces (0 +X, 1 −X, 4 +Z, 5 −Z).
    pub ladder_face: Option<usize>,
    /// Index into `SHAPES`.
    pub shape: usize,
    pub flooded: bool,
    /// The cross look's geometry, wind and distance fade.
    pub cross: Option<Cross>,
    pub wind: Option<Wind>,
    pub fade: Option<Fade>,
    /// Whether the ink outline draws on it (block data `outline`): the cross geometry carries it
    /// to the frame in the cutout mesh's `bare` stream.
    pub outline: bool,
    /// Fluids: how far below the cell's top its surface is drawn, in cells (0 otherwise).
    pub surface_drop: f64,
    /// The fluid whose look it is drawn with (an index into the registry's fluid table).
    pub fluid: Option<FluidId>,
    /// The axis its ends lie along (0 x, 1 y, 2 z; block data `axisProperty`): its top and bottom
    /// tiles face along it.
    pub axis: usize,
}

/// The per-vertex fluid id of see-through faces that are not a fluid's ([`crate::NO_FLUID`]), or
/// a fluid's id, as the meshes' `fluids` stream holds it.
pub(crate) fn fluid_value(fluid: Option<FluidId>) -> f32 {
    fluid.map_or(crate::NO_FLUID, f32::from)
}

/// The full cube's shape: what a full face is tested against.
pub(crate) fn cube() -> &'static ShapeInfo {
    &SHAPES[cube_index()]
}

fn cube_index() -> usize {
    SHAPES
        .iter()
        .position(|s| !s.inverted && s.corners.iter().all(|h| *h == 2))
        .expect("the shape table has a full cube")
}

/// The style of a state row, with `fluids` the table its `fluid` indexes (the build's, or a test
/// registry's).
pub(crate) fn style_of(s: &StateInfo, fluids: &[FluidInfo]) -> MaterialStyle {
    if !drawable(s, fluids) {
        return fallback_style(usize::from(s.shape_index), s.flooded);
    }
    let look = match s.look {
        "shaped" => Look::Shaped,
        "ladder" => Look::Ladder,
        "cross" => Look::Cross,
        "fluid" => Look::Fluid,
        // 'cube', and any look the meshers do not know: a full cube.
        _ => Look::Cube,
    };
    // shared/blocks/gen.mjs LADDER_FACE.
    let ladder_face = match s.facing {
        Facing::East => Some(0),
        Facing::West => Some(1),
        Facing::South => Some(4),
        Facing::North => Some(5),
        Facing::None => None,
    };
    MaterialStyle {
        look,
        color: s.color,
        render: s.render,
        textures: s.textures,
        block: block_id(s.name),
        side_under: s.side_under,
        ladder_face: if look == Look::Ladder {
            ladder_face
        } else {
            None
        },
        shape: usize::from(s.shape_index),
        flooded: s.flooded,
        cross: s.cross,
        wind: s.wind,
        fade: s.fade,
        outline: s.outline,
        surface_drop: s
            .fluid
            .and_then(|f| fluids.get(usize::from(f)))
            .map_or(0.0, |f| f.surface_drop),
        fluid: s.fluid,
        axis: match s.axis {
            dwell_blocks::Axis::X => 0,
            dwell_blocks::Axis::Y => 1,
            dwell_blocks::Axis::Z => 2,
        },
    }
}

/// The block id of a canonical state name (`dwell:ladder[facing=north]` → `dwell:ladder`).
fn block_id(name: &'static str) -> &'static str {
    name.split_once('[').map_or(name, |(b, _)| b)
}

const UNKNOWN_COLOR: u32 = 0xff00ff;

/// Whether this build can draw a state as its data asks: a look the meshers know, texture tiles
/// the atlas has, and a fluid for the fluid look. Every build state can (a test pins it); a state
/// synced from a server may not (ADR 0038 decision 7).
pub(crate) fn drawable(s: &StateInfo, fluids: &[FluidInfo]) -> bool {
    let look = matches!(s.look, "cube" | "shaped" | "ladder" | "cross" | "fluid");
    let tiles = s.textures.is_none_or(|t| {
        crate::textures::has_tile(t.top)
            && crate::textures::has_tile(t.side)
            && crate::textures::has_tile(t.bottom)
    }) && s
        .side_under
        .iter()
        .all(|r| crate::textures::has_tile(r.side));
    let fluid = s.look != "fluid" || s.fluid.is_some_and(|f| usize::from(f) < fluids.len());
    look && tiles && fluid
}

/// The fallback look: the magenta and black checker on a cube over the state's own shape (its
/// collision is the state's, whatever it looks like).
fn fallback_style(shape: usize, flooded: bool) -> MaterialStyle {
    let missing = crate::textures::MISSING_TILE;
    MaterialStyle {
        look: Look::Cube,
        color: UNKNOWN_COLOR,
        render: RenderMode::Opaque,
        textures: Some(Textures {
            top: missing,
            side: missing,
            bottom: missing,
        }),
        block: "",
        side_under: &[],
        ladder_face: None,
        shape,
        flooded,
        cross: None,
        wind: None,
        fade: None,
        outline: true,
        surface_drop: 0.0,
        fluid: None,
        axis: 1,
    }
}

/// The style of state `id`.
pub(crate) fn material_style(id: StateId) -> MaterialStyle {
    static STYLES: OnceLock<Vec<MaterialStyle>> = OnceLock::new();
    let styles = STYLES.get_or_init(|| STATES.iter().map(|s| style_of(s, &FLUIDS)).collect());
    if let Some(s) = styles.get(usize::from(id)) {
        return *s;
    }
    // A state registered at run time (synced from a server, ADR 0038): its own row.
    if let Some(s) = dwell_blocks::try_info(id) {
        return style_of(s, &FLUIDS);
    }
    fallback_style(cube_index(), false)
}

/// The state turned upside down (slabs and slopes swap `half`); ids the registry does not know
/// are their own mirror (`client/src/world/face.ts` mirrorMaterial).
pub(crate) fn mirror_material(id: StateId) -> StateId {
    static TABLE: OnceLock<Vec<StateId>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        (0..STATES.len())
            .map(|i| dwell_blocks::mirror(i as StateId))
            .collect()
    });
    table
        .get(usize::from(id))
        .copied()
        .unwrap_or_else(|| dwell_blocks::mirror(id))
}
