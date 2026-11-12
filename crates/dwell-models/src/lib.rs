//! Dwell's skinned models (docs/CHARACTERS.md §11, ADR 0070): imported from the packs' FBX into
//! GLBs with the palette remap (shared/models/import.mjs), listed in shared/models/models.json, and
//! generated into this crate's table by shared/models/gen.mjs. The player's model is built in
//! ([`PLAYER_GLB`]); the others (the animals) are data for the viewer until they spawn.

pub mod anim;
pub mod first_person;
pub mod glb;
pub mod grip;
pub mod math;
pub mod retarget;

#[rustfmt::skip]
mod table {
    include!("models.gen.rs");
}
pub use table::{
    DETAIL_DEFAULT, DETAIL_LEVELS, GLBS, HAND_GRIP, MODELS, PLAYER, PLAYER_GLB, PROP_GLBS, PROPS,
    RAMP_BASES,
};

/// The most colours one body's remap swaps (the skinned shader's `remap` rows).
pub const MAX_REMAP: usize = 6;

/// A palette ramp's base colour (sRGB `0xRRGGBB`), by name: what a model's faces carry (§11.2).
pub fn ramp_base(name: &str) -> Option<u32> {
    RAMP_BASES.iter().find(|(n, _)| *n == name).map(|&(_, c)| c)
}

/// The sRGB curve's inverse (an sRGB channel 0..1 to linear).
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// A model's part map (shared/models/parts.mjs, docs/CHARACTERS.md §11.11, BUGS.md #96): its part
/// names and, a string a detail level, each vertex's part (`'0'` none, `'1'`.. the parts in order).
/// An NPC's colours recolour by part, not by colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parts {
    pub names: &'static [&'static str],
    pub levels: &'static [&'static str],
}

impl Parts {
    /// Part `name`'s number (1-based: what its vertices carry); None: not a part of the model.
    pub fn number(&self, name: &str) -> Option<u8> {
        self.names
            .iter()
            .position(|n| *n == name)
            .map(|k| k as u8 + 1)
    }

    /// Each vertex's part number (0 none) at detail level `level` (past the last: the last's).
    pub fn level(&self, level: usize) -> Vec<u8> {
        let Some(s) = self
            .levels
            .get(level.min(self.levels.len().saturating_sub(1)))
        else {
            return Vec::new();
        };
        s.bytes().map(|b| b - b'0').collect()
    }
}

/// A body's colour remap as drawn, by part (docs/CHARACTERS.md §11.11, BUGS.md #96): slot `k` is
/// part `k + 1`'s (the model's ramp, the ramp drawn in its place), their base colours sRGB 0..1;
/// None: the part as modelled. `colours` are (part, from, to); a part the model lacks or a ramp the
/// palette lacks is left out. A ramp kept (`from` = `to`) still recolours its part, so every shade of
/// the part takes the ramp's hue as in every other variant (the Peasant's neck is skin_tan).
pub fn part_remap(
    parts: Option<&Parts>,
    colours: &[(&str, &str, &str)],
) -> [Option<([f32; 3], [f32; 3])>; MAX_REMAP] {
    let rgb = |c: u32| [16, 8, 0].map(|s| ((c >> s) & 0xff) as f32 / 255.0);
    let mut out = [None; MAX_REMAP];
    let Some(parts) = parts else { return out };
    for (part, from, to) in colours {
        let Some(n) = parts.number(part) else {
            continue;
        };
        if usize::from(n) > MAX_REMAP {
            continue;
        }
        if let (Some(f), Some(t)) = (ramp_base(from), ramp_base(to)) {
            out[usize::from(n) - 1] = Some((rgb(f), rgb(t)));
        }
    }
    out
}

/// A palette colour's luminance (sRGB 0..1, Rec. 709 weights): the recolour keeps a vertex's
/// shade of its part (its luminance over the part's ramp base's).
pub fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// A vertex's colour (sRGB 0..1) after its body's remap (the skinned shader's `remap_colour`): a
/// vertex of a remapped part takes the part's new base, at the vertex's shade of the old one
/// (luminance over the old base's), at most 1; any other as modelled.
pub fn recolour(c: [f32; 3], part: u8, remap: &[Option<([f32; 3], [f32; 3])>]) -> [f32; 3] {
    let Some(Some((from, to))) = (part as usize).checked_sub(1).and_then(|k| remap.get(k)) else {
        return c;
    };
    let shade = luma(c) / luma(*from).max(1e-3);
    to.map(|t| (t * shade).min(1.0))
}

/// The sRGB curve (a linear channel 0..1 to sRGB): the GLBs' `COLOR_0` is linear, the skinned
/// vertices carry sRGB bytes (BUGS.md #96: read as sRGB, the linear bytes drew dark colours black).
pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// The fixture table (shared/models/fixtures): a model added by data alone, for tests.
#[rustfmt::skip]
#[allow(unused_imports)]
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

/// One rig bone of a retarget map (models.json `retarget.bones`, docs/CHARACTERS.md §11.9).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RetargetBone {
    /// The characters rig's bone (`arm_r`).
    pub bone: &'static str,
    /// The skin joint it turns (an index into the skin's joints).
    pub joint: usize,
    /// Degrees, Euler XYZ in the model's space: the turn from the joint's rest frame to the bone's.
    pub rest: [f32; 3],
}

/// A model driven by the characters rig's pose (models.json `retarget`, docs/CHARACTERS.md §11.9).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Retarget {
    /// The characters rig (`biped`).
    pub rig: &'static str,
    /// The mapped bone whose joint takes the pose's root offset (`hips`).
    pub root: &'static str,
    pub bones: &'static [RetargetBone],
}

/// Your first-person arms drawn from a model (models.json `firstPerson`, docs/CHARACTERS.md §11.10).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FirstPerson {
    /// The arm's skin joints (indices into the skin's joints).
    pub joints: &'static [usize],
    /// The least share of a vertex's skin weight on `joints` that puts it in the arm.
    pub weight: f32,
    /// Degrees, Euler XYZ in view space: the whole arm turned about its hand (the sleeve's cuff
    /// brought into view, the hand and item kept).
    pub turn: [f32; 3],
    /// Your own body in first person from this model (`firstPerson.body`); None: none drawn.
    pub body: Option<FirstPersonBody>,
}

/// Your own body seen in first person (models.json `firstPerson.body`, docs/CHARACTERS.md §5.1,
/// §11.12): the body mesh less every triangle with a vertex that has at least `weight` of its skin
/// weight on `hide` or on the first-person arm's `joints` (drawn by the arms view).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FirstPersonBody {
    /// The skin joints left out besides the arm's: the head (its hat and hair), the other arm.
    pub hide: &'static [usize],
    pub weight: f32,
}

/// A pose of the hand (models.json `grips.poses`, docs/CHARACTERS.md §11.10): turns of skin joints
/// on top of their own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grip {
    pub name: &'static str,
    /// (skin joint index, degrees Euler XYZ in the joint's frame).
    pub turns: &'static [(usize, [f32; 3])],
    /// Metres the whole body is lowered while posed (an idle pose's `$drop`; 0 for a grip).
    pub drop: f32,
}

impl ModelData {
    /// Its idle pose of that name (models.json `poses`), if it has one.
    pub fn pose(&self, name: &str) -> Option<&'static Grip> {
        self.poses.iter().find(|p| p.name == name)
    }
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
    /// Your first-person arms drawn from it; None: it draws none.
    pub first_person: Option<FirstPerson>,
    /// Its hand's poses, and which each held item (by name) takes; an item not listed: the open
    /// hand.
    pub grips: &'static [Grip],
    pub grip_items: &'static [(&'static str, &'static str)],
    /// Idle poses as data (models.json `poses`, WORK_QUEUE item 39): per pose name its joints'
    /// turns on top of the clips, blended in and out over `pose_blend_s` seconds.
    pub poses: &'static [Grip],
    pub pose_blend_s: f32,
    /// Its part map (models.json `parts`): what an NPC's colours recolour; None: none.
    pub parts: Option<Parts>,
}

impl ModelData {
    /// The hand's pose for held item `item` (a characters.json item's name); None: the open hand.
    pub fn grip(&self, item: &str) -> Option<&'static Grip> {
        let (_, name) = self.grip_items.iter().find(|(i, _)| *i == item)?;
        self.grips.iter().find(|g| g.name == *name)
    }
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
    /// Where a hand holds it (m from its origin): on its handle's measured axis when it has one.
    pub grip: [f32; 3],
    /// Its handle's axis (unit, GLB frame) from the grip toward its head, measured from the GLB
    /// by shared/models/gen.mjs (props.json `handle`); +y when it names no handle.
    pub axis: [f32; 3],
    /// How the hand holds it (props.json `hold`; [`PropHold::DEFAULT`] when it names none).
    pub hold: PropHold,
}

/// A held prop's pose about its handle (props.json `hold`, docs/ITEMS.md §1a): scaled about its
/// grip and rolled about its handle. The handle itself lies along the hand's grip axis
/// ([`HandGrip`]); a lean comes from the hand's pose, not the prop's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PropHold {
    /// Uniform scale about the grip.
    pub scale: f32,
    /// Degrees about the handle, from the prop's +x side facing the hand's `edge`.
    pub roll: f32,
}

impl PropHold {
    /// As modelled, unrolled.
    pub const DEFAULT: PropHold = PropHold {
        scale: 1.0,
        roll: 0.0,
    };
}

/// The hand that holds a prop (props.json `hand`), in the characters rig's hand bone's frame (the
/// Peasant's hand joint maps to it, models.json `hand`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HandGrip {
    /// The characters rig's hand bone (`hand_r`).
    pub bone: &'static str,
    /// The closed fist's centre (m).
    pub at: [f32; 3],
    /// The line through the fist a handle lies along (unit).
    pub axis: [f32; 3],
    /// The way a tool's working side faces across the fist (unit, perpendicular to `axis`).
    pub edge: [f32; 3],
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt().max(1e-12);
    a.map(|v| v / l)
}

impl PropData {
    /// A point `p` of its GLB (m) in the hand's bone frame held by `hand`: its grip at the fist,
    /// its handle's axis along the hand's grip axis, its +x side (square to the handle) toward
    /// the hand's edge turned by its `roll` about the handle, scaled about the grip.
    pub fn in_hand(&self, hand: &HandGrip, p: [f32; 3]) -> [f32; 3] {
        let a = unit(self.axis);
        // The prop's frame: its handle, its +x side square to it, and the third way.
        let side = [1.0, 0.0, 0.0];
        let u = unit([0, 1, 2].map(|k| side[k] - a[k] * dot(side, a)));
        let w = cross(a, u);
        // The hand's: its grip axis, its edge rolled about it, and the third way.
        let ax = unit(hand.axis);
        let e = unit(hand.edge);
        let f = cross(ax, e);
        let (s, c) = self.hold.roll.to_radians().sin_cos();
        let e = [0, 1, 2].map(|k| e[k] * c + f[k] * s);
        let f = cross(ax, e);
        let q = [0, 1, 2].map(|k| (p[k] - self.grip[k]) * self.hold.scale);
        let (qa, qu, qw) = (dot(q, a), dot(q, u), dot(q, w));
        [0, 1, 2].map(|k| hand.at[k] + ax[k] * qa + e[k] * qu + f[k] * qw)
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
        assert_eq!(m.level(1).indices.len(), 6);
        // A level past the model's last draws its coarsest.
        assert_eq!(m.level(9).positions.len(), m.level(1).positions.len());
    }
}

#[cfg(test)]
mod remap_tests {
    use super::*;

    const PARTS: Parts = Parts {
        names: &["skin", "hair"],
        levels: &["0121", "12"],
    };

    #[test]
    fn a_remap_swaps_a_parts_ramp_bases_by_name() {
        assert_eq!(ramp_base("wool_green"), Some(0x4f8a4a));
        assert_eq!(ramp_base("nope"), None);
        let r = part_remap(
            Some(&PARTS),
            &[
                ("hair", "hair_brown", "hair_black"),
                ("hat", "wool_yellow", "leather"),
                ("skin", "skin_fair", "nope"),
            ],
        );
        assert_eq!(r[0], None, "an unknown ramp is left out");
        let srgb = |c: u32| [16, 8, 0].map(|s| ((c >> s) & 0xff) as f32 / 255.0);
        assert_eq!(r[1], Some((srgb(0x6b4329), srgb(0x302a3a))), "hair, slot 2");
        assert!(
            r[2..].iter().all(Option::is_none),
            "a part the model lacks: none"
        );
        assert_eq!(
            part_remap(None, &[("hair", "hair_brown", "hair_black")]),
            [None; MAX_REMAP]
        );
        assert_eq!(PARTS.level(0), vec![0, 1, 2, 1]);
        assert_eq!(
            PARTS.level(7),
            vec![1, 2],
            "past the last level: the last's"
        );
    }

    #[test]
    fn a_part_map_is_added_by_data_alone() {
        // The fixture model's part map (shared/models/fixtures/parts/two_bone.json): the vertex on
        // the tip joint is the `tip`, the rest the `base`.
        let parts = find(fixture::MODELS, "two_bone").unwrap().parts.unwrap();
        assert_eq!(parts.number("tip"), Some(1));
        assert_eq!(parts.level(0), vec![2, 1, 2, 2]);
        let r = part_remap(Some(&parts), &[("tip", "wool_green", "wool_red")]);
        assert!(r[0].is_some() && r[1].is_none());
    }

    #[test]
    fn a_recolour_keeps_the_vertexs_shade_of_its_part() {
        let r = part_remap(Some(&PARTS), &[("skin", "skin_fair", "skin_deep")]);
        let srgb = |c: u32| [16, 8, 0].map(|s| ((c >> s) & 0xff) as f32 / 255.0);
        let (fair, deep) = (srgb(0xf0c6a6), srgb(0x6b4334));
        assert_eq!(recolour(fair, 1, &r), deep, "the base takes the new base");
        let tan = srgb(0xd9a27a);
        let neck = recolour(tan, 1, &r);
        assert!(
            luma(neck) < luma(deep) && luma(neck) > 0.6 * luma(deep),
            "{neck:?}"
        );
        assert_eq!(recolour(fair, 0, &r), fair, "no part: as modelled");
        assert_eq!(
            recolour(fair, 2, &r),
            fair,
            "a part not remapped: as modelled"
        );
    }

    #[test]
    fn linear_to_srgb_inverts_srgb_to_linear() {
        for b in [0u8, 3, 37, 107, 200, 255] {
            let c = f32::from(b) / 255.0;
            assert!((linear_to_srgb(srgb_to_linear(c)) - c).abs() < 1e-4, "{b}");
        }
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
        // Its hold, from the fixture's data alone: half size, rolled a quarter turn.
        assert_eq!(
            data.hold,
            PropHold {
                scale: 0.5,
                roll: 90.0
            }
        );
        let hand = fixture::HAND_GRIP;
        // Its grip lands on the fist; a point 1 m up its axis lies 0.5 m along the grip axis.
        let g = data.in_hand(&hand, data.grip);
        assert!((0..3).all(|k| (g[k] - hand.at[k]).abs() < 1e-6), "{g:?}");
        let up = [0, 1, 2].map(|k| data.grip[k] + data.axis[k]);
        let p = data.in_hand(&hand, up);
        assert!(
            (0..3).all(|k| (p[k] - hand.at[k] - 0.5 * hand.axis[k]).abs() < 1e-5),
            "{p:?}"
        );
        // A prop that names no hold is held as modelled.
        let (chest, _) = (PROPS.iter().find(|p| p.name == "chest").unwrap(), ());
        assert_eq!(chest.hold, PropHold::DEFAULT);
    }

    #[test]
    fn a_tilted_off_centre_handle_is_laid_along_the_fist_by_data_alone() {
        // A fixture prop by data alone: its handle 3 cm off its origin and leaning 10 degrees.
        let t = 10f32.to_radians();
        let data = PropData {
            name: "stick",
            category: "dishes",
            glb: "stick.glb",
            length: 0.5,
            size: [0.05, 0.5, 0.05],
            grip: [0.03, -0.1, 0.0],
            axis: [t.sin(), t.cos(), 0.0],
            hold: PropHold {
                scale: 1.0,
                roll: 30.0,
            },
        };
        let hand = HAND_GRIP;
        for s in [-0.1f32, 0.0, 0.2] {
            let p = [0, 1, 2].map(|k| data.grip[k] + data.axis[k] * s);
            let h = data.in_hand(&hand, p);
            let want = [0, 1, 2].map(|k| hand.at[k] + hand.axis[k] * s);
            assert!(
                (0..3).all(|k| (h[k] - want[k]).abs() < 1e-5),
                "{s}: {h:?} {want:?}"
            );
        }
        // Rolled 30 degrees: its +x side (square to the handle) faces 30 degrees from the edge.
        let side = [t.cos(), -t.sin(), 0.0];
        let p = data.in_hand(&hand, [0, 1, 2].map(|k| data.grip[k] + side[k]));
        let d = [0, 1, 2].map(|k| p[k] - hand.at[k]);
        let c = d[0] * hand.edge[0] + d[1] * hand.edge[1] + d[2] * hand.edge[2];
        assert!((c - 30f32.to_radians().cos()).abs() < 1e-4, "{c}");
    }

    #[test]
    fn a_skinned_glb_is_not_a_prop() {
        assert!(glb::parse_static(PLAYER_GLB).is_err());
    }
}
