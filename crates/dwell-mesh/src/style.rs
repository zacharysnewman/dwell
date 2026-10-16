//! How block states are drawn (`client/src/world/materials.ts`): each state's look, colour,
//! opacity, textures, ladder face, shape, flooding and fluid, read from the generated registry
//! (`dwell-blocks`), with the TS fallback for ids the registry does not know (a magenta cube).

use std::sync::OnceLock;

use dwell_blocks::{
    FLUIDS, Facing, FluidId, FluidInfo, SHAPES, STATES, ShapeInfo, StateId, StateInfo, Textures,
};

/// The look a state is drawn with (the registry's `look` name).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Look {
    Cube,
    Shaped,
    Ladder,
    /// A fluid's look (`"look": "fluid"`): its parameters are the state's `fluid`.
    Fluid,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MaterialStyle {
    pub look: Look,
    /// sRGB `0xRRGGBB`.
    pub color: u32,
    pub opacity: f32,
    pub textures: Option<Textures>,
    /// Ladders: the face whose side the plate faces (0 +X, 1 −X, 4 +Z, 5 −Z).
    pub ladder_face: Option<usize>,
    /// Index into `SHAPES`.
    pub shape: usize,
    pub flooded: bool,
    /// Fluids: how far below the cell's top its surface is drawn, in cells (0 otherwise).
    pub surface_drop: f64,
    /// The fluid whose look it is drawn with (an index into the registry's fluid table).
    pub fluid: Option<FluidId>,
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
    let look = match s.look {
        "shaped" => Look::Shaped,
        "ladder" => Look::Ladder,
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
        opacity: s.opacity,
        textures: s.textures,
        ladder_face: if look == Look::Ladder {
            ladder_face
        } else {
            None
        },
        shape: usize::from(s.shape_index),
        flooded: s.flooded,
        surface_drop: s
            .fluid
            .and_then(|f| fluids.get(usize::from(f)))
            .map_or(0.0, |f| f.surface_drop),
        fluid: s.fluid,
    }
}

const UNKNOWN_COLOR: u32 = 0xff00ff;

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
    {
        MaterialStyle {
            look: Look::Cube,
            color: UNKNOWN_COLOR,
            opacity: 1.0,
            textures: None,
            ladder_face: None,
            shape: cube_index(),
            flooded: false,
            surface_drop: 0.0,
            fluid: None,
        }
    }
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
