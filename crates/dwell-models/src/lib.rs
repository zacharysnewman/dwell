//! Dwell's skinned models (docs/CHARACTERS.md §11, ADR 0070): imported from the packs' FBX into
//! GLBs with the palette remap (shared/models/import.mjs), listed in shared/models/models.json, and
//! generated into this crate's table by shared/models/gen.mjs. The player's model is built in
//! ([`PLAYER_GLB`]); the others (the animals) are data for the viewer until they spawn.

pub mod anim;
pub mod glb;
pub mod math;
pub mod retarget;

#[rustfmt::skip]
mod table {
    include!("models.gen.rs");
}
pub use table::{
    DETAIL_DEFAULT, DETAIL_LEVELS, GLBS, MODELS, PLAYER, PLAYER_GLB, PROP_GLBS, PROPS,
};

/// The fixture table (shared/models/fixtures): a model added by data alone, for tests.
#[rustfmt::skip]
pub mod fixture {
    include!("fixture.gen.rs");
}

/// The most joints a skin may have (the shader's joint block; shared/models/gen.mjs `MAX_JOINTS`).
pub const MAX_JOINTS: usize = 64;

/// One model-detail level (models.json `detail.levels`, ADR 0070 §10): every skinned GLB holds a
/// mesh for it (`detail_<k>`, decimated to `ratio` of the faces and flat shaded at import).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DetailLevel {
    pub id: &'static str,
    pub label: &'static str,
    pub ratio: f32,
}

/// The level named `id` in `levels` (its index, the mesh it draws); an unknown name: `fallback`.
pub fn detail_level(levels: &[DetailLevel], id: &str, fallback: usize) -> usize {
    levels.iter().position(|l| l.id == id).unwrap_or(fallback)
}

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

/// One rig bone of a retarget map (models.json `retarget.bones`, docs/CHARACTERS.md §11.6).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RetargetBone {
    /// The characters rig's bone (`arm_r`).
    pub bone: &'static str,
    /// The skin joint it turns (an index into the skin's joints).
    pub joint: usize,
    /// Degrees, Euler XYZ in the model's space: the turn from the joint's rest frame to the bone's.
    pub rest: [f32; 3],
}

/// A model driven by the characters rig's pose (models.json `retarget`, docs/CHARACTERS.md §11.6).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Retarget {
    /// The characters rig (`biped`).
    pub rig: &'static str,
    /// The mapped bone whose joint takes the pose's root offset (`hips`).
    pub root: &'static str,
    pub bones: &'static [RetargetBone],
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
    /// Its joints driven by the characters rig's pose; None: it plays its own clips only.
    pub retarget: Option<Retarget>,
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

/// One static model, a prop (shared/models/props.json, ADR 0070 §8): no skin, no clips.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PropData {
    pub name: &'static str,
    /// Its category in props.json (`tools`, `weapons`, …).
    pub category: &'static str,
    /// Its GLB, relative to its props.json.
    pub glb: &'static str,
    /// Metres along its longest side (the import scales it to this).
    pub length: f32,
    /// Its bounds' size (m, x y z; +y up in the GLB), centred on its origin.
    pub size: [f32; 3],
    /// Where a hand holds it (m from its origin).
    pub grip: [f32; 3],
    /// How the hand holds it (props.json `hold`; [`PropHold::DEFAULT`] when it names none).
    pub hold: PropHold,
}

/// A held prop's pose in the hand (props.json `hold`, docs/ITEMS.md §8): scaled about its grip,
/// turned there and moved, in the hand's frame (+z the prop's length ahead of the grip, +x its
/// side, +y the other way across).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PropHold {
    /// Uniform scale about the grip.
    pub scale: f32,
    /// Degrees: yaw (about +y), pitch (about +x), roll (about +z, the length), applied roll first.
    pub rotation: [f32; 3],
    /// Metres, moved after the turn.
    pub offset: [f32; 3],
}

impl PropHold {
    /// As modelled, at its grip.
    pub const DEFAULT: PropHold = PropHold {
        scale: 1.0,
        rotation: [0.0; 3],
        offset: [0.0; 3],
    };

    /// A point `q` (metres from the grip, in the hand's frame) placed by the pose.
    pub fn apply(&self, q: [f32; 3]) -> [f32; 3] {
        let [yaw, pitch, roll] = self.rotation.map(f32::to_radians);
        let (s, c) = roll.sin_cos();
        let q = [q[0] * c - q[1] * s, q[0] * s + q[1] * c, q[2]];
        let (s, c) = pitch.sin_cos();
        let q = [q[0], q[1] * c - q[2] * s, q[1] * s + q[2] * c];
        let (s, c) = yaw.sin_cos();
        let q = [q[0] * c + q[2] * s, q[1], -q[0] * s + q[2] * c];
        [0, 1, 2].map(|i| q[i] * self.scale + self.offset[i])
    }
}

/// A prop built into the game (one an item's look names) and its GLB read once, by name; None:
/// not a prop, or not built in.
pub fn prop(name: &str) -> Option<(&'static PropData, &'static glb::Model)> {
    prop_in(PROPS, PROP_GLBS, name)
}

/// [`prop`] over a table (the data's, or the fixture's).
pub fn prop_in(
    table: &'static [PropData],
    glbs: &'static [(&'static str, &'static [u8])],
    name: &str,
) -> Option<(&'static PropData, &'static glb::Model)> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static READ: OnceLock<Mutex<HashMap<usize, &'static glb::Model>>> = OnceLock::new();
    let data = table.iter().find(|p| p.name == name)?;
    let bytes = glbs.iter().find(|(n, _)| *n == data.name)?.1;
    let mut read = READ.get_or_init(Default::default).lock().ok()?;
    let key = bytes.as_ptr() as usize;
    if let Some(m) = read.get(&key) {
        return Some((data, m));
    }
    let model: &'static glb::Model = Box::leak(Box::new(glb::parse_static(bytes).ok()?));
    read.insert(key, model);
    Some((data, model))
}

#[cfg(test)]
mod detail_tests {
    use super::*;

    #[test]
    fn the_setting_picks_a_level_by_name_from_the_data() {
        assert_eq!(
            DETAIL_LEVELS[0].ratio, 1.0,
            "the first level is the full mesh"
        );
        // Owner, 2026-11-10: "make 60% the default".
        assert_eq!(DETAIL_LEVELS[DETAIL_DEFAULT].id, "high");
        assert_eq!(DETAIL_LEVELS[DETAIL_DEFAULT].ratio, 0.6);
        let low = detail_level(DETAIL_LEVELS, "low", DETAIL_DEFAULT);
        assert_eq!(DETAIL_LEVELS[low].id, "low");
        assert_eq!(
            detail_level(DETAIL_LEVELS, "nonsense", DETAIL_DEFAULT),
            DETAIL_DEFAULT
        );
    }

    #[test]
    fn a_level_is_added_by_data_alone() {
        // The fixture's models.json lists a level of its own and makes it the default.
        let levels = fixture::DETAIL_LEVELS;
        assert_eq!(levels[fixture::DETAIL_DEFAULT].id, "coarse");
        assert_eq!(detail_level(levels, "coarse", 0), 1);
        let m = glb::parse(fixture::PLAYER_GLB).unwrap();
        assert_eq!(m.level_count(), levels.len());
        assert_eq!(m.level(1).indices.len(), 3);
        // A level past the model's last draws its coarsest.
        assert_eq!(m.level(9).positions.len(), m.level(1).positions.len());
    }
}

#[cfg(test)]
mod props_tests {
    use super::*;

    #[test]
    fn every_prop_glb_reads_static_with_colours_and_its_size() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../shared/models/");
        assert!(PROPS.len() >= 50, "the tools pack's props");
        for p in PROPS {
            let bytes = std::fs::read(format!("{dir}{}", p.glb)).unwrap();
            let m = glb::parse_static(&bytes).unwrap_or_else(|e| panic!("{}: {e}", p.name));
            assert!(!m.indices.is_empty(), "{}: no triangles", p.name);
            let mut colours: Vec<[u8; 3]> = m
                .colors
                .iter()
                .map(|c| c.map(|v| (v * 255.0).round() as u8))
                .collect();
            colours.sort_unstable();
            colours.dedup();
            assert!(colours.len() > 1, "{}: one colour (COLOR_0 lost?)", p.name);
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for q in &m.positions {
                for k in 0..3 {
                    lo[k] = lo[k].min(q[k]);
                    hi[k] = hi[k].max(q[k]);
                }
            }
            let longest = (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f32::max);
            assert!(
                (longest - p.length).abs() < 0.02 * p.length + 0.005,
                "{}: {longest} m",
                p.name
            );
            assert!((0.05..=1.8).contains(&longest), "{}: {longest} m", p.name);
        }
    }

    #[test]
    fn a_prop_named_by_an_item_look_is_built_in_and_reads() {
        let (data, m) = prop("stone_pickaxes").expect("the stone pickaxe's look is built in");
        assert_eq!(data.category, "tools");
        assert!(m.positions.len() > 100);
        // A prop no look names is listed but not built in.
        assert!(PROPS.iter().any(|p| p.name == "chest"));
        assert!(prop("chest").is_none());
    }

    #[test]
    fn a_prop_is_added_by_data_alone() {
        let (data, m) = prop_in(fixture::PROPS, fixture::PROP_GLBS, "plate").unwrap();
        assert_eq!(data.category, "dishes");
        assert_eq!(data.grip, [0.0, 0.01, 0.0]);
        assert!(!m.indices.is_empty());
        // Its hold pose, from the fixture's data alone.
        assert_eq!(
            data.hold,
            PropHold {
                scale: 0.5,
                rotation: [0.0, 90.0, 0.0],
                offset: [0.0, 0.0, 0.1]
            }
        );
        // Half size, its length (+z) pitched to −y, then moved 0.1 m ahead.
        let p = data.hold.apply([0.0, 0.0, 1.0]);
        assert!(
            (p[0]).abs() < 1e-6 && (p[1] + 0.5).abs() < 1e-6 && (p[2] - 0.1).abs() < 1e-6,
            "{p:?}"
        );
        // A prop that names no hold is held as modelled.
        assert_eq!(PropHold::DEFAULT.apply([0.1, 0.2, 0.3]), [0.1, 0.2, 0.3]);
        let (chest, _) = (PROPS.iter().find(|p| p.name == "chest").unwrap(), ());
        assert_eq!(chest.hold, PropHold::DEFAULT);
    }

    #[test]
    fn a_skinned_glb_is_not_a_prop() {
        assert!(glb::parse_static(PLAYER_GLB).is_err());
    }
}
