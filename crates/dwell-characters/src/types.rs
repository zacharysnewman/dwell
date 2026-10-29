//! The generated tables' types (`characters.gen.rs`, written by `shared/characters/gen.mjs`): the
//! engine's fixed vocabulary of CHARACTERS.md §3, as data the game reads by name.

/// A part's shape; every kind fills its `size` box, its axis along y.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Box,
    RoundBox,
    Ellipsoid,
    Cylinder,
    Capsule,
    Cone,
}

/// Which view draws a part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum View {
    Both,
    Body,
    Arms,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    Body,
    Arms,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    Base,
    Hold,
    Action,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ease {
    Linear,
    Smooth,
    Spline,
    Step,
}

/// A bone with no parent (the rig's root).
pub const NO_PARENT: usize = usize::MAX;

/// `$block`'s material index: the held block's own look (its atlas tiles), supplied at run time.
pub const BLOCK_MATERIAL: u16 = u16::MAX;

#[derive(Clone, Copy, Debug)]
pub struct Bone {
    pub name: &'static str,
    /// An earlier bone's index, or `NO_PARENT`.
    pub parent: usize,
    /// The rest offset from the parent's pivot (metres): the proportions.
    pub rest: [f32; 3],
}

/// A composed part (rig.mjs `compose`): on a bone, `at` in the bone's frame, turned by `rot`
/// (Euler XYZ degrees), filling `size`.
#[derive(Clone, Copy, Debug)]
pub struct Part {
    pub id: &'static str,
    pub bone: usize,
    pub kind: Kind,
    pub size: [f32; 3],
    pub at: [f32; 3],
    pub rot: [f32; 3],
    /// `round_box`: the rounding, 0–0.5 of the smallest side.
    pub round: f32,
    /// `cone`: the top radius, 0–1.
    pub tip: f32,
    /// A `$block` box's sides show this part of its tile, bottom (0) to top (1): a ½ m held piece
    /// the top half, as the world draws a ½ m cell (data `sideV`; default the whole tile).
    pub side_v: [f32; 2],
    pub view: View,
    /// An index into `MATERIALS`, or `BLOCK_MATERIAL`.
    pub material: u16,
}

/// A look (a variant): every part it draws, accessories included and hidden parts left out.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub name: &'static str,
    pub label: &'static str,
    pub parts: &'static [Part],
}

/// A held item, composed onto the character's socket.
#[derive(Clone, Copy, Debug)]
pub struct Item {
    pub name: &'static str,
    /// Where the light a held block gives off sits (a torch's flame; docs/LIGHTS.md): a bone and
    /// the point in its frame (metres), the item's socket resolved. None: no light point.
    pub light: Option<(usize, [f32; 3])>,
    /// Turns (degrees, Euler XYZ) added to bones while it is held, on top of any clip: a block
    /// carried on an upturned palm (data `pose`; [`crate::anim::with_item`]).
    pub pose: &'static [(usize, [f32; 3])],
    pub parts: &'static [Part],
}

#[derive(Clone, Copy, Debug)]
pub struct FirstPerson {
    /// The arm chain, root first (bone indices).
    pub bones: &'static [usize],
    /// The chain's first bone in view space (metres from the eye; +z ahead, +x left).
    pub anchor: [f32; 3],
    /// The arms pass's vertical FOV (degrees).
    pub fov: f32,
    /// Role → clip index.
    pub clips: &'static [(&'static str, usize)],
    /// Your own body in first person (CHARACTERS.md §5.1), if the character draws it.
    pub body: Option<FirstPersonBody>,
}

/// Your own body in first person (CHARACTERS.md §5.1, `firstPerson.body`): the full body from the
/// neck down, kept clear of the eye (`own`).
#[derive(Clone, Copy, Debug)]
pub struct FirstPersonBody {
    /// Bones whose parts (and accessories) are left out: the head, the arms (bone indices).
    pub hide_bones: &'static [usize],
    /// Parts left out by id (the neck).
    pub hide_parts: &'static [&'static str],
    /// No vertex is drawn nearer the eye than this (m): the body moves back until none is.
    pub clear: f32,
    /// How far behind the eye the body is drawn (m, along its back).
    pub back: f32,
    /// How much further back looking straight down (m, by the sine of the downward pitch).
    pub look_down: f32,
    /// The margin past the camera's near plane no part may come within (m).
    pub near: f32,
    /// The most of the screen the body may cover (0–1).
    pub fill: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Character {
    /// The data's key (`player`).
    pub id: &'static str,
    /// Namespaced (`dwell:player`).
    pub name: &'static str,
    pub rig: &'static str,
    pub height: f32,
    pub bones: &'static [Bone],
    /// Role → clip index.
    pub clips: &'static [(&'static str, usize)],
    pub looks: &'static [Look],
    pub items: &'static [Item],
    pub first_person: Option<FirstPerson>,
}

#[derive(Clone, Copy, Debug)]
pub struct Material {
    pub name: &'static str,
    /// sRGB `0xRRGGBB`.
    pub shadow: u32,
    pub base: u32,
    pub light: u32,
    /// How strongly the brush tile shows (0 clean).
    pub overlay: f32,
    /// How much of its colour it gives off unlit (a flame; shared/textures `emissive`).
    pub emissive: f32,
    /// Index into `TILES`, or −1.
    pub tile: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct Key {
    pub t: f32,
    pub v: [f32; 3],
}

/// One bone's keys, in time order.
#[derive(Clone, Copy, Debug)]
pub struct Channel {
    pub bone: usize,
    pub keys: &'static [Key],
}

#[derive(Clone, Copy, Debug)]
pub struct Clip {
    pub name: &'static str,
    pub rig: &'static str,
    pub space: Space,
    pub layer: Layer,
    pub duration: f32,
    pub looping: bool,
    pub ease: Ease,
    /// Metres travelled per loop (0: not a gait).
    pub cycle: f32,
    pub grounded: bool,
    /// The cross-fade into it (s).
    pub blend: f32,
    pub channels: &'static [Channel],
    /// The root offset's keys (metres), if it keys one.
    pub root: &'static [Key],
}

impl Character {
    /// The clip a role names (`walk`), if the character has it.
    pub fn clip(&self, role: &str) -> Option<usize> {
        self.clips.iter().find(|(r, _)| *r == role).map(|(_, c)| *c)
    }
    /// The first-person clip a role names.
    pub fn arms_clip(&self, role: &str) -> Option<usize> {
        self.first_person?
            .clips
            .iter()
            .find(|(r, _)| *r == role)
            .map(|(_, c)| *c)
    }
    pub fn look(&self, name: &str) -> Option<&'static Look> {
        self.looks.iter().find(|l| l.name == name)
    }
    pub fn item(&self, name: &str) -> Option<&'static Item> {
        self.items.iter().find(|i| i.name == name)
    }
    pub fn bone(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.name == name)
    }
}
