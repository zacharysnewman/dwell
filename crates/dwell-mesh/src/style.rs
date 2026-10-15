//! How block states are drawn (`client/src/world/materials.ts`): each state's look, colour,
//! opacity, textures, ladder face, shape and flooding, read from the generated registry
//! (`dwell-blocks`), with the TS fallback for ids the registry does not know (a magenta cube).

use std::sync::OnceLock;

use dwell_blocks::{Facing, SHAPES, STATES, ShapeInfo, StateId, Textures};

/// The look a state is drawn with (the registry's `look` name).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Look {
    Cube,
    Shaped,
    Ladder,
    Water,
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

fn style_of_state(i: usize) -> MaterialStyle {
    let s = &STATES[i];
    let look = match s.look {
        "shaped" => Look::Shaped,
        "ladder" => Look::Ladder,
        "water" => Look::Water,
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
    }
}

const UNKNOWN_COLOR: u32 = 0xff00ff;

/// The style of state `id`.
pub(crate) fn material_style(id: StateId) -> MaterialStyle {
    static STYLES: OnceLock<Vec<MaterialStyle>> = OnceLock::new();
    let styles = STYLES.get_or_init(|| (0..STATES.len()).map(style_of_state).collect());
    styles
        .get(usize::from(id))
        .copied()
        .unwrap_or_else(|| MaterialStyle {
            look: Look::Cube,
            color: UNKNOWN_COLOR,
            opacity: 1.0,
            textures: None,
            ladder_face: None,
            shape: cube_index(),
            flooded: false,
        })
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
    table.get(usize::from(id)).copied().unwrap_or(id)
}
