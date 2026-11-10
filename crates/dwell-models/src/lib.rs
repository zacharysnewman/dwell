//! Dwell's skinned models (docs/CHARACTERS.md §11, ADR 0070): imported from the packs' FBX into
//! GLBs with the palette remap (shared/models/import.mjs), listed in shared/models/models.json, and
//! generated into this crate's table by shared/models/gen.mjs. The player's model is built in
//! ([`PLAYER_GLB`]); the others (the animals) are data for the viewer until they spawn.

pub mod anim;
pub mod glb;
pub mod math;

#[rustfmt::skip]
mod table {
    include!("models.gen.rs");
}
pub use table::{GLBS, MODELS, PLAYER, PLAYER_GLB};

/// The fixture table (shared/models/fixtures): a model added by data alone, for tests.
#[rustfmt::skip]
pub mod fixture {
    include!("fixture.gen.rs");
}

/// The most joints a skin may have (the shader's joint block; shared/models/gen.mjs `MAX_JOINTS`).
pub const MAX_JOINTS: usize = 64;

/// Which clip plays a role, at what speed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoleClip {
    pub role: &'static str,
    pub clip: &'static str,
    pub speed: f32,
}

/// Where a model's hand holds an item (docs/CHARACTERS.md §11.5): the skin joint the item follows
/// and the turn from that joint's frame to the characters rig's hand bone's (the frame the held
/// items are authored in, shared/characters/characters.json).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hand {
    /// The joint: an index into the skin's joints.
    pub joint: usize,
    /// The characters rig's bone the items are authored on (`hand_r`).
    pub bone: &'static str,
    /// Metres in the joint's frame (unscaled).
    pub at: [f32; 3],
    /// Degrees, Euler XYZ (as characters.json).
    pub rot: [f32; 3],
}

/// One model's data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelData {
    pub name: &'static str,
    /// Its GLB, relative to its models.json.
    pub glb: &'static str,
    pub joints: usize,
    /// Metres per model unit.
    pub scale: f32,
    /// Degrees turning its front to +z.
    pub yaw: f32,
    /// The role played for a role it has no clip for.
    pub fallback: &'static str,
    pub roles: &'static [RoleClip],
    /// Where it holds an item; None: it holds none.
    pub hand: Option<Hand>,
}

impl ModelData {
    /// The clip for `role`, else the fallback's.
    pub fn role(&self, role: &str) -> RoleClip {
        let find = |r: &str| self.roles.iter().find(|c| c.role == r).copied();
        find(role)
            .or_else(|| find(self.fallback))
            .unwrap_or(self.roles[0])
    }
}

/// A model's data by name in `table`.
pub fn find(table: &'static [ModelData], name: &str) -> Option<&'static ModelData> {
    table.iter().find(|m| m.name == name)
}

/// A model's data and its GLB read once, by name; None: not in the table.
pub fn model(name: &str) -> Option<(&'static ModelData, &'static glb::Model)> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static READ: OnceLock<Mutex<HashMap<&'static str, &'static glb::Model>>> = OnceLock::new();
    let data = find(MODELS, name)?;
    let mut read = READ.get_or_init(Default::default).lock().ok()?;
    if let Some(m) = read.get(data.name) {
        return Some((data, m));
    }
    let bytes = GLBS.iter().find(|(n, _)| *n == data.name)?.1;
    let model: &'static glb::Model = Box::leak(Box::new(glb::parse(bytes).ok()?));
    read.insert(data.name, model);
    Some((data, model))
}

/// The player's model: its data and its GLB read once.
pub fn player() -> (&'static ModelData, &'static glb::Model) {
    static MODEL: std::sync::OnceLock<glb::Model> = std::sync::OnceLock::new();
    let data = find(MODELS, PLAYER).expect("models.json names a player model");
    let model = MODEL.get_or_init(|| glb::parse(PLAYER_GLB).expect("the player's GLB reads"));
    (data, model)
}
