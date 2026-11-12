//! The animation probe (docs/DEV_TOOLS.md "Clip probe"; owner, 2026-11-11: "make yourself some tools
//! to properly observe animations, movement directions, positions over time"): samples a clip at N
//! times through the same posing the game draws with and reports where a point (the held tool's
//! head, tip or grip, a bone's pivot, an item part) is over time, in five views of the same swing:
//!
//! - `fp`: the first-person screen (what the player sees): NDC x right, y up, and the depth, from
//!   the arms' animator (`Animator` in `Space::Arms`, the swing on its action layer driven to clip
//!   time as `Bodies::drive_arms` does), the held item's turns (`with_item`), `arm_transforms` and
//!   the arms pass's projection (the first person `fov`), the tool drawn as its model on the hold
//!   (`prop_mesh`, `held_item`).
//! - `top`, `side`, `front`: orthographic views in the player's frame (right, up, ahead; the eye at
//!   the origin, looking level): from above (right vs ahead), from the player's right (ahead vs
//!   up), and facing the player (right vs up, mirrored as the player's own right on the left).
//! - `tp`: the whole body from behind and to the right: the body's animator (`Space::Body`, the
//!   rig pose the skinned player model retargets), `bone_transforms`, the same model on the hand.
//!
//! [`probe`] gives the samples, the phases (from the tool's `swing` timings for a chop clip, else
//! thirds), each phase's direction of travel in each view, the peak speed and when; [`table`],
//! [`csv`] and [`svg`] print them. `examples/clip_probe.rs` is the CLI. Tests (`chop_swing_tests`)
//! pin the axe swings' directions with it.
//!
//! face-independent: the arms are posed in view space and turned into the world by the camera's
//! basis (`arms_frame`); the world space output does that for a chosen face (`FaceFrame`).

use crate::render::camera::{Basis, aim};
use crate::render::characters::{Held, arms_frame, held_item, prop_mesh};
use dwell_characters::anim::{MAX_BONES, arm_transforms, bone_transforms, with_item};
use dwell_characters::animator::{Animator, MoveInput, Space, Swing};
use dwell_characters::math::Affine;
use dwell_characters::{TABLES, View, mesh, player};
use dwell_world::FaceFrame;

/// The views a probe reports, by name (`--views`).
pub const VIEWS: [&str; 5] = ["fp", "top", "side", "front", "tp"];

/// What point is followed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Point {
    /// The held tool's head: the far tenth of its model along the handle.
    ToolHead,
    /// Its far end.
    ToolTip,
    /// The hand's grip (where the handle sits in the fist).
    Grip,
    /// A bone's pivot (`elbow` is `forearm_r`'s, `shoulder` `arm_r`'s).
    Bone(&'static str),
    /// A part of the held characters item (`axe_blade`), its centre.
    Part(&'static str),
}

impl Point {
    pub fn parse(s: &str) -> Result<Point, String> {
        Ok(match s {
            "tool_head" | "head" => Point::ToolHead,
            "tool_tip" | "tip" => Point::ToolTip,
            "grip" | "hand" => Point::Grip,
            "elbow" => Point::Bone("forearm_r"),
            "shoulder" => Point::Bone("arm_r"),
            other => {
                let name: &'static str = Box::leak(other.to_string().into_boxed_str());
                if player().bone(name).is_some() {
                    Point::Bone(name)
                } else {
                    Point::Part(name)
                }
            }
        })
    }

    pub fn name(self) -> String {
        match self {
            Point::ToolHead => "tool_head".into(),
            Point::ToolTip => "tool_tip".into(),
            Point::Grip => "grip".into(),
            Point::Bone("forearm_r") => "elbow".into(),
            Point::Bone(b) | Point::Part(b) => b.into(),
        }
    }
}

/// What to probe.
#[derive(Clone, Debug)]
pub struct Probe {
    /// The arms role (`chop_level`) or a clip's name (`fp_chop_level`).
    pub clip: String,
    /// The inventory item in hand (`dwell:stone_axe`); none: empty-handed.
    pub item: Option<&'static str>,
    pub points: Vec<Point>,
    pub samples: usize,
    /// The screen's width over its height.
    pub aspect: f32,
    /// The world space output's face, heading and pitch (degrees).
    pub face: FaceFrame,
    pub yaw: f64,
    pub pitch: f64,
}

impl Probe {
    pub fn new(clip: &str) -> Self {
        Self {
            clip: clip.into(),
            item: Some("dwell:stone_axe"),
            points: vec![Point::ToolHead, Point::Grip, Point::Bone("forearm_r")],
            samples: 31,
            aspect: 16.0 / 9.0,
            face: FaceFrame::A,
            yaw: 0.0,
            pitch: 0.0,
        }
    }
}

/// One point at one time.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct At {
    /// The player's frame: right, up, ahead (metres from the eye, looking level).
    pub local: [f32; 3],
    /// The first-person screen: NDC x right, y up (−1..1), and the depth (m ahead).
    pub screen: [f32; 3],
    /// Eye-relative world space on the probe's face, heading and pitch.
    pub world: [f64; 3],
    /// The body (third person): right, up, ahead from the feet (metres).
    pub body: [f32; 3],
}

/// One sample: its clip time and each point's place.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub t: f32,
    pub at: Vec<At>,
}

/// A stretch of the clip and where the first point went over it.
#[derive(Clone, Debug, PartialEq)]
pub struct Phase {
    pub name: &'static str,
    pub from: f32,
    pub to: f32,
    /// The first point's travel in the first-person screen (NDC).
    pub screen: [f32; 2],
    /// Its travel in the player's frame (right, up, ahead; m).
    pub local: [f32; 3],
    /// Its travel on the body (right, up, ahead; m).
    pub body: [f32; 3],
}

impl Phase {
    /// The screen travel's dominant way: "right→left", "top→bottom", ….
    pub fn screen_way(&self) -> &'static str {
        way(self.screen[0], self.screen[1])
    }
    pub fn body_way(&self) -> &'static str {
        way(self.body[0], self.body[1])
    }
}

fn way(dx: f32, dy: f32) -> &'static str {
    if dx.abs().max(dy.abs()) < 1e-4 {
        "still"
    } else if dx.abs() >= dy.abs() {
        if dx < 0.0 {
            "right→left"
        } else {
            "left→right"
        }
    } else if dy < 0.0 {
        "top→bottom"
    } else {
        "bottom→top"
    }
}

/// A probe's result.
#[derive(Clone, Debug)]
pub struct Report {
    pub clip: &'static str,
    pub duration: f32,
    pub points: Vec<Point>,
    pub samples: Vec<Sample>,
    pub phases: Vec<Phase>,
    /// The first point's fastest screen speed (NDC/s) and when.
    pub peak_speed: f32,
    pub peak_t: f32,
    /// When a centred hit lands (s): the arc's middle (the swing's `time_of_step`), for a chop.
    pub contact_t: Option<f32>,
    /// A rest pose of the body's bones (pivots, body frame) and their parents, for the `tp` view.
    pub skeleton: Vec<([f32; 3], Option<[f32; 3]>)>,
}

impl Report {
    /// The phase named `name`.
    pub fn phase(&self, name: &str) -> Option<&Phase> {
        self.phases.iter().find(|p| p.name == name)
    }
    /// Point `k` at the sample nearest time `t`.
    pub fn at(&self, k: usize, t: f32) -> At {
        let s = self
            .samples
            .iter()
            .min_by(|a, b| (a.t - t).abs().total_cmp(&(b.t - t).abs()))
            .expect("samples");
        s.at[k]
    }
}

/// The swing the arms and body play for a role (`chop_level` → the level chop).
fn swing_of(role: &str) -> Option<Swing> {
    match role {
        "chop_level" => Some(Swing::ChopLevel),
        "chop_vertical" => Some(Swing::ChopVertical),
        "break" => Some(Swing::Break),
        "place" => Some(Swing::Place),
        _ => None,
    }
}

/// The arms role and clip a probe names (a role, or a clip the arms or body play).
fn resolve(name: &str) -> Result<(&'static str, &'static dwell_characters::Clip), String> {
    let ch = player();
    let fp = ch.first_person.ok_or("no first person")?;
    if let Some(i) = ch.arms_clip(name) {
        let role = fp
            .clips
            .iter()
            .find(|(r, _)| *r == name)
            .map(|(r, _)| *r)
            .unwrap_or("idle");
        return Ok((role, TABLES.clip(i)));
    }
    for &(role, _) in fp.clips {
        if let Some(i) = ch.arms_clip(role)
            && TABLES.clip(i).name == name
        {
            return Ok((role, TABLES.clip(i)));
        }
    }
    Err(format!(
        "no arms role or clip `{name}` (roles: {})",
        fp.clips
            .iter()
            .map(|(r, _)| *r)
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// The tool's swing timings for an axe role.
fn swing_def(item: Option<&str>, role: &str) -> Option<dwell_protocol::chopping::SwingDef> {
    if swing_of(role).is_none_or(|s| !matches!(s, Swing::ChopLevel | Swing::ChopVertical)) {
        return None;
    }
    let tool = dwell_protocol::chopping::tool(item.unwrap_or("dwell:stone_axe"))
        .or_else(|| dwell_protocol::chopping::tool("dwell:stone_axe"))?;
    Some(tool.swing)
}

/// The hold (characters item) the hand shows for `item` swinging `role`.
pub fn held_for(item: Option<&'static str>, role: &str) -> Held {
    let Some(item) = item else {
        return Held::Empty;
    };
    let tool = dwell_protocol::chopping::tool(item);
    let hold = tool.and_then(|t| {
        if role == "chop_vertical" {
            t.held_vertical.or(t.held)
        } else {
            t.held
        }
    });
    held_item(item, hold)
}

/// Point `p`'s place in its bone's frame, and that bone; None: the held thing has no such point.
pub fn local_point(held: Held, p: Point) -> Option<(usize, [f32; 3])> {
    let ch = player();
    let hand = ch.bone(dwell_models::HAND_GRIP.bone)?;
    match p {
        Point::Grip => Some((hand, dwell_models::HAND_GRIP.at)),
        Point::Bone(b) => Some((ch.bone(b)?, [0.0; 3])),
        Point::ToolHead | Point::ToolTip => {
            let (bone, verts) = held_vertices(held)?;
            // Along the grip's axis from the fist: the head is the far tenth, the tip the end.
            let g = dwell_models::HAND_GRIP;
            let along = |v: &[f32; 3]| (0..3).map(|k| (v[k] - g.at[k]) * g.axis[k]).sum::<f32>();
            let max = verts.iter().map(along).fold(f32::MIN, f32::max);
            let min = verts.iter().map(along).fold(f32::MAX, f32::min);
            let cut = if p == Point::ToolTip {
                max - 0.01 * (max - min)
            } else {
                max - 0.1 * (max - min)
            };
            Some((bone, centroid(verts.iter().filter(|v| along(v) >= cut))))
        }
        Point::Part(id) => {
            let (name, _) = held.item()?;
            let it = ch.item(name)?;
            let part = it.parts.iter().find(|q| q.id == id)?;
            let fp = ch.first_person?;
            let m = mesh::build(std::slice::from_ref(part), View::Arms, fp.bones);
            Some((usize::from(*m.bones.first()?), centroid(m.positions.iter())))
        }
    }
}

/// The held thing's vertices in its bone's frame: the model on the hold, or the item's parts.
fn held_vertices(held: Held) -> Option<(usize, Vec<[f32; 3]>)> {
    let ch = player();
    let m = match held {
        Held::Model { hold, model } => prop_mesh(ch, hold, model),
        _ => {
            let (name, _) = held.item()?;
            let it = ch.item(name)?;
            mesh::build(it.parts, View::Arms, ch.first_person?.bones)
        }
    };
    let bone = usize::from(*m.bones.first()?);
    Some((bone, m.positions))
}

fn centroid<'a>(it: impl Iterator<Item = &'a [f32; 3]>) -> [f32; 3] {
    let (mut c, mut n) = ([0.0f32; 3], 0.0f32);
    for v in it {
        for k in 0..3 {
            c[k] += v[k];
        }
        n += 1.0;
    }
    c.map(|v| v / n.max(1.0))
}

/// The arms' bone transforms (view space: +x left, +y up, +z ahead) `t` s into `role`'s swing, as
/// the game poses them: the arms' animator at idle, the swing on its action layer driven to `t`.
fn arms_bones(role: &str, held: Held, t: f32) -> [Affine; MAX_BONES] {
    let ch = player();
    let mut a = Animator::new(&TABLES, ch, Space::Arms);
    let input = MoveInput {
        holding: held != Held::Empty,
        ..Default::default()
    };
    a.update(&input, 0.0);
    if let Some(s) = swing_of(role) {
        a.swing(s);
        a.drive_action(Some(t));
    }
    let pose = a.update(&input, 0.0);
    let hold = held.item().and_then(|(n, _)| ch.item(n));
    arm_transforms(ch, &with_item(&pose, hold))
}

/// A fitted key: the arms' root offset (view space, as the clip keys it: +x left) or the body's
/// chest twist (degrees about up), the swing's three arm bones (degrees), and where it puts the
/// grip, the tool head and which way the blade's edge faces (the player's frame: right, up, ahead
/// of the eye; the body's: right, up, ahead of the feet).
#[derive(Clone, Copy, Debug, Default)]
pub struct Fit {
    pub body: bool,
    pub root: [f32; 3],
    pub chest: f32,
    pub arm: [f32; 3],
    pub forearm: f32,
    pub hand: [f32; 3],
    pub grip: [f32; 3],
    pub head: [f32; 3],
    pub blade: [f32; 3],
}

/// What a fitted key should do: the grip and the tool head there, and (if given) the blade's edge
/// facing that way (a direction; the strike's way, so the edge leads the head).
#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub grip: [f32; 3],
    pub head: [f32; 3],
    pub blade: Option<[f32; 3]>,
}

impl Fit {
    /// The key as the clip's JSON (`{ "t": …, "root": …, "bones": {…} }`; a body key keys the
    /// chest instead of a root).
    pub fn json(&self, t: f32) -> String {
        let r = |v: f32| (v * 1000.0).round() / 1000.0 + 0.0;
        let d = |v: f32| v.round() + 0.0;
        let arms = format!(
            r#""arm_r": [{}, {}, {}], "forearm_r": [{}, 0, 0], "hand_r": [{}, {}, {}]"#,
            d(self.arm[0]),
            d(self.arm[1]),
            d(self.arm[2]),
            d(self.forearm),
            d(self.hand[0]),
            d(self.hand[1]),
            d(self.hand[2]),
        );
        if self.body {
            format!(
                r#"{{ "t": {t}, "bones": {{ "chest": [0, {}, 0], {arms} }} }}"#,
                d(self.chest)
            )
        } else {
            format!(
                r#"{{ "t": {t}, "root": [{}, {}, {}], "bones": {{ {arms} }} }}"#,
                r(self.root[0]),
                r(self.root[1]),
                r(self.root[2]),
            )
        }
    }
}

/// Fits a key for `role`'s swing (the clip-authoring aid behind `clip_probe --fit`): the arm,
/// elbow and wrist turns, with the arms' root offset (first person; a torso turn, kept small) or
/// the chest's twist (`body`: the third-person rig, its frame from the feet), that put the grip
/// and the tool head at `target`'s places and turn the blade's edge its way, within the rig's
/// joint limits, starting from `from`. Coordinate descent on the game's own posing
/// (`arm_transforms` / `bone_transforms` with the hold's turns), so the key plays where it was
/// fitted.
pub fn fit(role: &str, item: Option<&'static str>, body: bool, target: Target, from: Fit) -> Fit {
    let ch = player();
    let held = held_for(item, role);
    let hold = held.item().and_then(|(n, _)| ch.item(n));
    let (gb, gp) = local_point(held, Point::Grip).expect("grip");
    let (hb, hp) = local_point(held, Point::ToolHead).unwrap_or((gb, gp));
    let edge = local_point(held, Point::Part("axe_blade"));
    let lash = local_point(held, Point::Part("axe_lashing"));
    let bone = |n: &str| ch.bone(n).expect("bone");
    let (arm, fore, hand, chest) = (
        bone("arm_r"),
        bone("forearm_r"),
        bone("hand_r"),
        bone("chest"),
    );
    let place = |x: &[f32; 10]| {
        let mut pose = dwell_characters::anim::Pose::REST;
        pose.rot[arm] = [x[0], x[1], x[2]];
        pose.rot[fore] = [x[3], 0.0, 0.0];
        pose.rot[hand] = [x[4], x[5], x[6]];
        let b = if body {
            pose.rot[chest] = [0.0, x[7], 0.0];
            bone_transforms(ch, &with_item(&pose, hold))
        } else {
            pose.root = [x[7], x[8], x[9]];
            arm_transforms(ch, &with_item(&pose, hold))
        };
        let at = |(i, p): (usize, [f32; 3])| to_local(b[i].apply(p));
        let blade = match (edge, lash) {
            (Some(e), Some(l)) => norm3(sub3(at(e), at(l))),
            _ => [0.0; 3],
        };
        (at((gb, gp)), at((hb, hp)), blade)
    };
    // The rig's joint limits, and a torso turn of at most 0.25 m (arms) or 50° (body).
    let (lo, hi) = if body {
        (
            [
                -180.0, -90.0, -180.0, -150.0, -80.0, -60.0, -40.0, -50.0, 0.0, 0.0,
            ],
            [70.0, 90.0, 30.0, 0.0, 80.0, 60.0, 40.0, 50.0, 0.0, 0.0],
        )
    } else {
        (
            [
                -180.0, -90.0, -180.0, -150.0, -80.0, -60.0, -40.0, -0.25, -0.2, -0.2,
            ],
            [70.0, 90.0, 30.0, 0.0, 80.0, 60.0, 40.0, 0.25, 0.2, 0.2],
        )
    };
    let cost = |x: &[f32; 10]| {
        let (g, h, b) = place(x);
        let d2 = |a: [f32; 3], b: [f32; 3]| (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f32>();
        // The root costs a little (the arm does the work), the wrist's turn a little too.
        let reg = if body {
            1e-5 * x[7] * x[7]
        } else {
            0.3 * (x[7] * x[7] + x[8] * x[8] + x[9] * x[9])
        } + 2e-6 * (x[4] * x[4] + x[5] * x[5] + x[6] * x[6]);
        let behind = if !body && h[2] < 0.2 {
            10.0 * (0.2 - h[2])
        } else {
            0.0
        };
        let facing = target
            .blade
            .map_or(0.0, |w| 0.05 * (1.0 - dot3(b, norm3(w))));
        d2(g, target.grip) + d2(h, target.head) + reg + behind + facing
    };
    let mut x = [
        from.arm[0],
        from.arm[1],
        from.arm[2],
        from.forearm,
        from.hand[0],
        from.hand[1],
        from.hand[2],
        if body { from.chest } else { from.root[0] },
        if body { 0.0 } else { from.root[1] },
        if body { 0.0 } else { from.root[2] },
    ];
    let mut best = cost(&x);
    let mut step = if body {
        [8.0f32, 8.0, 8.0, 8.0, 8.0, 8.0, 8.0, 8.0, 0.0, 0.0]
    } else {
        [8.0f32, 8.0, 8.0, 8.0, 8.0, 8.0, 8.0, 0.03, 0.03, 0.03]
    };
    for _ in 0..4000 {
        let mut moved = false;
        for k in 0..10 {
            for dir in [1.0f32, -1.0] {
                let mut y = x;
                y[k] = (y[k] + dir * step[k]).clamp(lo[k], hi[k]);
                let c = cost(&y);
                if c < best {
                    (x, best, moved) = (y, c, true);
                    break;
                }
            }
        }
        if !moved {
            step = step.map(|s| s * 0.5);
            if step[0] < 0.05 {
                break;
            }
        }
    }
    let (g, h, b) = place(&x);
    Fit {
        body,
        root: if body { [0.0; 3] } else { [x[7], x[8], x[9]] },
        chest: if body { x[7] } else { 0.0 },
        arm: [x[0], x[1], x[2]],
        forearm: x[3],
        hand: [x[4], x[5], x[6]],
        grip: g,
        head: h,
        blade: b,
    }
}

/// The body's bone transforms (its frame: feet at the origin, +x left, +z ahead) `t` s into the
/// swing, from the body's animator.
fn body_bones(role: &str, held: Held, t: f32) -> [Affine; MAX_BONES] {
    let ch = player();
    let mut a = Animator::new(&TABLES, ch, Space::Body);
    let input = MoveInput {
        holding: held != Held::Empty,
        ..Default::default()
    };
    a.update(&input, 0.0);
    if let Some(s) = swing_of(role) {
        a.swing(s);
        a.drive_action(Some(t));
    }
    let pose = a.update(&input, 0.0);
    let hold = held.item().and_then(|(n, _)| ch.item(n));
    bone_transforms(ch, &with_item(&pose, hold))
}

/// View space (+x left) to the player's frame (+x right).
fn to_local(v: [f32; 3]) -> [f32; 3] {
    [-v[0], v[1], v[2]]
}

/// Runs a probe.
pub fn probe(p: &Probe) -> Result<Report, String> {
    let ch = player();
    let fp = ch.first_person.ok_or("no first person")?;
    let (role, clip) = resolve(&p.clip)?;
    let held = held_for(p.item, role);
    let mut points = Vec::new();
    for &pt in &p.points {
        let lp = local_point(held, pt)
            .ok_or_else(|| format!("no point `{}` on {:?}", pt.name(), held))?;
        points.push((pt, lp));
    }
    let tan = (f64::from(fp.fov).to_radians() / 2.0).tan() as f32;
    let basis: Basis = aim(p.yaw, p.pitch, p.face, 0.0);
    let frame = arms_frame(&basis);
    let n = p.samples.max(2);
    let samples: Vec<Sample> = (0..n)
        .map(|i| {
            let t = clip.duration * i as f32 / (n - 1) as f32;
            let arms = arms_bones(role, held, t);
            let body = body_bones(role, held, t);
            let at = points
                .iter()
                .map(|&(_, (bone, q))| {
                    let v = arms[bone].apply(q);
                    let z = v[2].max(1e-3);
                    let w = frame.apply(v);
                    let b = body[bone].apply(q);
                    At {
                        local: to_local(v),
                        screen: [-v[0] / (z * tan * p.aspect), v[1] / (z * tan), v[2]],
                        world: w.map(f64::from),
                        body: to_local(b),
                    }
                })
                .collect();
            Sample { t, at }
        })
        .collect();
    let swing = swing_def(p.item, role);
    let cuts: Vec<(&'static str, f32, f32)> = match swing {
        Some(s) => {
            let a = s.windup_s as f32;
            let b = (s.windup_s + s.active_s) as f32;
            vec![
                ("wind-up", 0.0, a),
                ("strike", a, b),
                ("recovery", b, clip.duration),
            ]
        }
        None => {
            let d = clip.duration;
            vec![
                ("first", 0.0, d / 3.0),
                ("middle", d / 3.0, 2.0 * d / 3.0),
                ("last", 2.0 * d / 3.0, d),
            ]
        }
    };
    let (mut peak_speed, mut peak_t) = (0.0f32, 0.0f32);
    for w in samples.windows(2) {
        let (a, b) = (w[0].at[0].screen, w[1].at[0].screen);
        let s = ((b[0] - a[0]).hypot(b[1] - a[1])) / (w[1].t - w[0].t).max(1e-6);
        if s > peak_speed {
            (peak_speed, peak_t) = (s, (w[0].t + w[1].t) / 2.0);
        }
    }
    let mut report = Report {
        clip: clip.name,
        duration: clip.duration,
        points: points.iter().map(|(p, _)| *p).collect(),
        samples,
        phases: Vec::new(),
        peak_speed,
        peak_t,
        contact_t: swing.map(|s| s.time_of_step(s.samples.max(2).saturating_sub(1) / 2) as f32),
        skeleton: Vec::new(),
    };
    // The phases' travel, from exact samples at their ends (not the table's nearest).
    let point0 = points[0].1;
    let place = |t: f32| {
        let arms = arms_bones(role, held, t);
        let body = body_bones(role, held, t);
        let v = arms[point0.0].apply(point0.1);
        let z = v[2].max(1e-3);
        (
            [-v[0] / (z * tan * p.aspect), v[1] / (z * tan)],
            to_local(v),
            to_local(body[point0.0].apply(point0.1)),
        )
    };
    report.phases = cuts
        .into_iter()
        .map(|(name, from, to)| {
            let (s0, l0, b0) = place(from);
            let (s1, l1, b1) = place(to);
            Phase {
                name,
                from,
                to,
                screen: [s1[0] - s0[0], s1[1] - s0[1]],
                local: std::array::from_fn(|k| l1[k] - l0[k]),
                body: std::array::from_fn(|k| b1[k] - b0[k]),
            }
        })
        .collect();
    let rest = body_bones("idle", held, 0.0);
    report.skeleton = ch
        .bones
        .iter()
        .enumerate()
        .take(MAX_BONES)
        .map(|(i, b)| {
            let parent =
                (b.parent != dwell_characters::NO_PARENT).then(|| to_local(rest[b.parent].t));
            (to_local(rest[i].t), parent)
        })
        .collect();
    Ok(report)
}

/// The report as a text table: each sample's time and the first-person screen place of each
/// point, the phases and the peak.
pub fn table(r: &Report) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let _ = writeln!(s, "clip {} ({:.2} s)", r.clip, r.duration);
    let _ = write!(s, "{:>6}", "t");
    for p in &r.points {
        let _ = write!(
            s,
            " | {:^34}",
            format!("{} fp x,y,depth / body r,u,a", p.name())
        );
    }
    let _ = writeln!(s);
    for smp in &r.samples {
        let _ = write!(s, "{:>6.3}", smp.t);
        for a in &smp.at {
            let _ = write!(
                s,
                " | {:+.2} {:+.2} {:.2} / {:+.2} {:+.2} {:+.2}",
                a.screen[0], a.screen[1], a.screen[2], a.body[0], a.body[1], a.body[2]
            );
        }
        let _ = writeln!(s);
    }
    let _ = writeln!(s, "phases of {} :", r.points[0].name());
    for ph in &r.phases {
        let _ = writeln!(
            s,
            "  {:<9} {:.2}–{:.2} s  screen {:+.2},{:+.2} ({})  body r{:+.2} u{:+.2} a{:+.2} ({})",
            ph.name,
            ph.from,
            ph.to,
            ph.screen[0],
            ph.screen[1],
            ph.screen_way(),
            ph.body[0],
            ph.body[1],
            ph.body[2],
            ph.body_way()
        );
    }
    let _ = writeln!(
        s,
        "peak screen speed {:.1} NDC/s at {:.3} s",
        r.peak_speed, r.peak_t
    );
    if let Some(c) = r.contact_t {
        let a = r.at(0, c);
        let _ = writeln!(
            s,
            "contact {c:.3} s: {} at screen {:+.2},{:+.2} (the crosshair is 0,0)",
            r.points[0].name(),
            a.screen[0],
            a.screen[1]
        );
    }
    s
}

/// The samples as CSV: one row per sample and point.
pub fn csv(r: &Report) -> String {
    let mut s = String::from(
        "t,point,screen_x,screen_y,depth,right,up,ahead,world_x,world_y,world_z,body_right,body_up,body_ahead\n",
    );
    for smp in &r.samples {
        for (p, a) in r.points.iter().zip(&smp.at) {
            s.push_str(&format!(
                "{:.4},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4}\n",
                smp.t,
                p.name(),
                a.screen[0],
                a.screen[1],
                a.screen[2],
                a.local[0],
                a.local[1],
                a.local[2],
                a.world[0],
                a.world[1],
                a.world[2],
                a.body[0],
                a.body[1],
                a.body[2]
            ));
        }
    }
    s
}

/// A view's 2D place of a point (x right, y up on the panel) and its axes' labels.
fn view_xy(view: &str, a: &At) -> [f32; 2] {
    // The third-person camera: behind and to the right of the body, a little above, looking at the
    // chest (perspective).
    match view {
        // Clamped: a point behind the eye projects far off; it is drawn at the sheet's edge.
        "fp" if a.screen[2] <= 0.02 => [a.screen[0].signum() * 3.0, a.screen[1].signum() * 3.0],
        "fp" => [a.screen[0].clamp(-3.0, 3.0), a.screen[1].clamp(-3.0, 3.0)],
        "top" => [a.local[0], a.local[2]],
        "side" => [a.local[2], a.local[1]],
        "front" => [-a.local[0], a.local[1]],
        _ => tp_project(a.body),
    }
}

/// The third-person view: the body seen from behind and to the right (2.4 m back, 1.4 m right,
/// 1.9 m up), looking at the chest; x right, y up on the panel (perspective, divided by depth).
pub fn tp_project(p: [f32; 3]) -> [f32; 2] {
    let eye = [1.4f32, 1.9, -2.4];
    let target = [0.0f32, 1.2, 0.3];
    let f = norm3(sub3(target, eye));
    // The body frame here is right, up, ahead (a left-handed view of a right-handed rig): the
    // camera's right is up × forward in it.
    let r = norm3(cross3([0.0, 1.0, 0.0], f));
    let u = cross3(f, r);
    let d = sub3(p, eye);
    let z = dot3(d, f).max(0.05);
    [dot3(d, r) / z, dot3(d, u) / z]
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|k| a[k] - b[k])
}
fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm3(a: [f32; 3]) -> [f32; 3] {
    let l = dot3(a, a).sqrt().max(1e-9);
    a.map(|v| v / l)
}

fn view_title(view: &str) -> &'static str {
    match view {
        "fp" => "first person (screen)",
        "top" => "top (right → / ahead ↑)",
        "side" => "side, from the right (ahead → / up ↑)",
        "front" => "front, facing the player (player's right ←)",
        _ => "third person, behind-right",
    }
}

/// The report as an SVG sheet: one panel per view (`views`, from [`VIEWS`]), each point's path with
/// time ticks (dots every sample, a label every fifth), arrows along it, the phases coloured
/// (wind-up amber, strike red, recovery blue), the crosshair and contact in `fp`, the body's rest
/// skeleton in `tp`.
pub fn svg(r: &Report, views: &[&str]) -> String {
    use std::fmt::Write;
    // A square sheet (five views: three columns of two rows, 1080 × 1080) with a viewBox, so a
    // thumbnailer that renders a square (`qlmanage -t`) shows every panel (it cropped the side
    // panel of a wide sheet and the last of a tall one).
    let cols = (views.len() as f32).sqrt().ceil().max(1.0) as usize;
    let rows = views.len().div_ceil(cols).max(1);
    let side = 360.0 * cols as f32;
    let (pw, ph) = (side / cols as f32, (side - 24.0) / rows as f32);
    let (w, h) = (side, side);
    let mut s = String::new();
    let _ = writeln!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" font-family="sans-serif" font-size="11">"#,
    );
    let _ = writeln!(
        s,
        r##"<rect width="100%" height="100%" fill="#fbfaf7"/><text x="8" y="16" font-size="13">{} — {} (dots every {:.0} ms; amber wind-up, red strike, blue recovery)</text>"##,
        r.clip,
        r.points
            .iter()
            .map(|p| p.name())
            .collect::<Vec<_>>()
            .join(", "),
        1000.0 * r.duration / (r.samples.len() - 1).max(1) as f32
    );
    let colour = |t: f32| match r.phases.iter().find(|p| t >= p.from && t <= p.to) {
        Some(p) if p.name == "wind-up" || p.name == "first" => "#d08a1c",
        Some(p) if p.name == "strike" || p.name == "middle" => "#c8322a",
        _ => "#2f6db5",
    };
    for (vi, &view) in views.iter().enumerate() {
        let (ox, oy) = ((vi % cols) as f32 * pw, 24.0 + (vi / cols) as f32 * ph);
        // The panel's bounds: every point of every sample (and the origin), padded.
        let mut pts: Vec<[f32; 2]> = r
            .samples
            .iter()
            .flat_map(|smp| smp.at.iter().map(|a| view_xy(view, a)))
            .collect();
        if view == "tp" {
            for (b, _) in &r.skeleton {
                pts.push(tp_project(*b));
            }
        }
        pts.push([0.0, 0.0]);
        if view == "fp" {
            pts.extend([[-1.0, -1.0], [1.0, 1.0]]);
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in &pts {
            (x0, y0, x1, y1) = (x0.min(p[0]), y0.min(p[1]), x1.max(p[0]), y1.max(p[1]));
        }
        let pad = 0.08 * (x1 - x0).max(y1 - y0).max(1e-3);
        (x0, y0, x1, y1) = (x0 - pad, y0 - pad, x1 + pad, y1 + pad);
        let k = ((pw - 20.0) / (x1 - x0)).min((ph - 40.0) / (y1 - y0));
        let map = |p: [f32; 2]| {
            (
                ox + 10.0 + (p[0] - x0) * k,
                oy + ph - 10.0 - (p[1] - y0) * k,
            )
        };
        let _ = writeln!(
            s,
            r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="none" stroke="#bbb"/><text x="{:.1}" y="{:.1}">{}</text>"##,
            ox + 4.0,
            oy + 4.0,
            pw - 8.0,
            ph - 8.0,
            ox + 10.0,
            oy + 20.0,
            view_title(view)
        );
        // The crosshair (fp) or the eye / feet (others).
        let (cx, cy) = map([0.0, 0.0]);
        let _ = writeln!(
            s,
            r##"<path d="M{:.1} {cy:.1}h12M{cx:.1} {:.1}v12" stroke="#555"/>"##,
            cx - 6.0,
            cy - 6.0
        );
        if view == "fp" {
            let (a, b) = (map([-1.0, 1.0]), map([1.0, -1.0]));
            let _ = writeln!(
                s,
                r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="none" stroke="#999" stroke-dasharray="4 3"/>"##,
                a.0,
                a.1,
                b.0 - a.0,
                b.1 - a.1
            );
        }
        if view == "tp" {
            for (b, parent) in &r.skeleton {
                if let Some(pa) = parent {
                    let (a, c) = (map(tp_project(*b)), map(tp_project(*pa)));
                    let _ = writeln!(
                        s,
                        r##"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="#c9c4ba" stroke-width="3"/>"##,
                        a.0, a.1, c.0, c.1
                    );
                }
            }
        }
        for (pi, p) in r.points.iter().enumerate() {
            let width = if pi == 0 { 2.2 } else { 1.0 };
            let path: Vec<(f32, (f32, f32))> = r
                .samples
                .iter()
                .map(|smp| (smp.t, map(view_xy(view, &smp.at[pi]))))
                .collect();
            for w in path.windows(2) {
                let ((t, a), (_, b)) = (w[0], w[1]);
                let _ = writeln!(
                    s,
                    r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{}" stroke-width="{width}" opacity="{}"/>"#,
                    a.0,
                    a.1,
                    b.0,
                    b.1,
                    colour(t),
                    if pi == 0 { 1.0 } else { 0.55 }
                );
            }
            for (i, &(t, (x, y))) in path.iter().enumerate() {
                let _ = writeln!(
                    s,
                    r#"<circle cx="{x:.1}" cy="{y:.1}" r="{}" fill="{}"/>"#,
                    if pi == 0 { 2.4 } else { 1.4 },
                    colour(t)
                );
                if pi == 0 && i % 5 == 0 {
                    let _ = writeln!(
                        s,
                        r##"<text x="{:.1}" y="{:.1}" fill="#333" font-size="9">{:.2}</text>"##,
                        x + 4.0,
                        y - 4.0,
                        t
                    );
                }
                // An arrowhead every third segment, pointing along the travel.
                if i % 3 == 1 && i + 1 < path.len() {
                    let (nx, ny) = path[i + 1].1;
                    let (dx, dy) = (nx - x, ny - y);
                    let l = dx.hypot(dy);
                    if l > 2.0 {
                        let (ux, uy) = (dx / l, dy / l);
                        let (tx, ty) = (x + dx * 0.5, y + dy * 0.5);
                        let _ = writeln!(
                            s,
                            r#"<path d="M{:.1} {:.1}L{:.1} {:.1}L{:.1} {:.1}Z" fill="{}"/>"#,
                            tx + ux * 5.0,
                            ty + uy * 5.0,
                            tx - ux * 3.0 - uy * 3.5,
                            ty - uy * 3.0 + ux * 3.5,
                            tx - ux * 3.0 + uy * 3.5,
                            ty - uy * 3.0 - ux * 3.5,
                            colour(path[i].0)
                        );
                    }
                }
            }
            // The path's name at its start.
            let (x, y) = path[0].1;
            let _ = writeln!(
                s,
                r##"<text x="{:.1}" y="{:.1}" fill="#666" font-size="9">{}</text>"##,
                x + 4.0,
                y + 10.0,
                p.name()
            );
        }
        if let Some(c) = r.contact_t {
            let (x, y) = map(view_xy(view, &r.at(0, c)));
            let _ = writeln!(
                s,
                r##"<circle cx="{x:.1}" cy="{y:.1}" r="6" fill="none" stroke="#111" stroke-width="1.5"/><text x="{:.1}" y="{:.1}" font-size="9">contact {c:.2}s</text>"##,
                x + 8.0,
                y + 3.0
            );
        }
    }
    s.push_str("</svg>\n");
    s
}

#[cfg(test)]
mod chop_swing_tests {
    //! The axe's swings, observed with the probe (owner, 2026-11-11: "Should swing right to left for
    //! horizontal and up to down for vertical"). Before the 0.52.x refit the level strike ran
    //! bottom → top and left → right on screen and its wind-up took the head behind the eye; the
    //! vertical one's head went behind the eye too (docs/clip_probe/*_before.txt).
    use super::*;

    fn run(role: &str) -> Report {
        let mut p = Probe::new(role);
        p.samples = 63;
        probe(&p).expect("probe")
    }

    /// The tool head's samples within a phase.
    fn within<'a>(r: &'a Report, name: &str) -> Vec<&'a Sample> {
        let ph = r.phase(name).expect("phase");
        r.samples
            .iter()
            .filter(|s| s.t >= ph.from - 1e-4 && s.t <= ph.to + 1e-4)
            .collect()
    }

    #[test]
    fn the_level_chop_strikes_right_to_left_across_the_screen() {
        // face-a-only: the first-person screen is view space, the same on both faces (the world
        // space test below checks both).
        let r = run("chop_level");
        let s = within(&r, "strike");
        let (first, last) = (s[0].at[0].screen, s[s.len() - 1].at[0].screen);
        assert!(
            first[0] > 0.3,
            "the strike starts right of centre: {first:?}\n{}",
            table(&r)
        );
        assert!(
            last[0] < -0.2,
            "the strike ends left of centre: {last:?}\n{}",
            table(&r)
        );
        let ph = r.phase("strike").unwrap();
        assert!(
            ph.screen[0] < -0.8 && ph.screen[0].abs() > 2.0 * ph.screen[1].abs(),
            "the strike is mostly leftward: {ph:?}"
        );
        for w in s.windows(2) {
            let dx = w[1].at[0].screen[0] - w[0].at[0].screen[0];
            assert!(
                dx < 0.02,
                "the head never goes back right during the strike: {:.3}→{:.3} s, dx {dx:+.3}\n{}",
                w[0].t,
                w[1].t,
                table(&r)
            );
        }
    }

    #[test]
    fn the_vertical_chop_strikes_top_to_bottom_across_the_screen() {
        // face-a-only: view space, as above.
        let r = run("chop_vertical");
        let s = within(&r, "strike");
        let (first, last) = (s[0].at[0].screen, s[s.len() - 1].at[0].screen);
        assert!(
            first[1] > 0.4,
            "the strike starts high: {first:?}\n{}",
            table(&r)
        );
        assert!(
            last[1] < -0.4,
            "the strike ends low: {last:?}\n{}",
            table(&r)
        );
        let ph = r.phase("strike").unwrap();
        assert!(
            ph.screen[1] < -1.0 && ph.screen[1].abs() > 2.0 * ph.screen[0].abs(),
            "the strike is mostly downward: {ph:?}"
        );
        for w in s.windows(2) {
            let dy = w[1].at[0].screen[1] - w[0].at[0].screen[1];
            assert!(
                dy < 0.02,
                "the head never goes back up during the strike: {:.3}→{:.3} s, dy {dy:+.3}\n{}",
                w[0].t,
                w[1].t,
                table(&r)
            );
        }
    }

    #[test]
    fn the_head_meets_the_crosshair_at_contact_in_both_swings() {
        // face-a-only: view space.
        for role in ["chop_level", "chop_vertical"] {
            let r = run(role);
            let c = r.contact_t.expect("a chop has a contact time");
            let a = r.at(0, c);
            assert!(
                a.screen[0].abs() < 0.12 && a.screen[1].abs() < 0.12 && a.screen[2] > 0.3,
                "{role}: at contact ({c:.3} s) the head is on the crosshair, in front: {a:?}"
            );
            // And the strike is the swing's fastest stretch, peaking within it.
            let ph = r.phase("strike").unwrap();
            assert!(
                r.peak_t >= ph.from && r.peak_t <= ph.to,
                "{role}: the peak speed ({:.1} at {:.3} s) is in the strike {:.2}–{:.2}",
                r.peak_speed,
                r.peak_t,
                ph.from,
                ph.to
            );
        }
    }

    #[test]
    fn the_wind_up_goes_the_other_way_and_the_recovery_returns_to_rest() {
        // face-a-only: view space.
        for (role, axis) in [("chop_level", 0), ("chop_vertical", 1)] {
            let r = run(role);
            let w = r.phase("wind-up").unwrap();
            let s = r.phase("strike").unwrap();
            assert!(
                w.screen[axis] * s.screen[axis] < 0.0 && w.screen[axis].abs() > 0.15,
                "{role}: the wind-up draws back against the strike: {w:?} vs {s:?}"
            );
            let (a, b) = (r.samples[0].at[0], r.samples[r.samples.len() - 1].at[0]);
            for k in 0..3 {
                assert!(
                    (a.local[k] - b.local[k]).abs() < 1e-3,
                    "{role}: the recovery ends where the swing began: {a:?} vs {b:?}"
                );
            }
            // Never behind or inside the eye: everything held stays in front.
            for smp in &r.samples {
                assert!(
                    smp.at[0].screen[2] > 0.15,
                    "{role}: the head at {:.3} s is {:.3} m ahead",
                    smp.t,
                    smp.at[0].screen[2]
                );
            }
        }
    }

    /// A point's path length (m, the player's frame) over samples.
    fn travel(s: &[&Sample], k: usize) -> f32 {
        s.windows(2)
            .map(|w| {
                let (a, b) = (w[0].at[k].local, w[1].at[k].local);
                (0..3).map(|i| (b[i] - a[i]).powi(2)).sum::<f32>().sqrt()
            })
            .sum()
    }

    #[test]
    fn the_arms_drive_the_strike_not_the_wrist() {
        // face-a-only: view space. Owner review 2026-11-11: the head's arc was mostly a wrist
        // turn, the grip and elbow nearly still (the grip went 0.2 m to the head's 1.1 m).
        for role in ["chop_level", "chop_vertical"] {
            let mut p = Probe::new(role);
            p.samples = 121;
            p.points = vec![Point::ToolHead, Point::Grip];
            let r = probe(&p).expect("probe");
            let s = within(&r, "strike");
            let (head, grip) = (travel(&s, 0), travel(&s, 1));
            assert!(
                grip >= 0.4 * head && grip > 0.4,
                "{role}: the hands travel the arc with the head: grip {grip:.2} m, head {head:.2} m"
            );
        }
    }

    #[test]
    fn the_level_strike_stays_level_on_screen() {
        // face-a-only: view space. Round 2's refit drew the head back high off the top right, so
        // the strike came down diagonally to the crosshair.
        let mut p = Probe::new("chop_level");
        p.samples = 121;
        let r = probe(&p).expect("probe");
        let s = within(&r, "strike");
        let (mut dx, mut dy) = (0.0f32, 0.0f32);
        for w in s.windows(2) {
            let (a, b) = (w[0].at[0].screen, w[1].at[0].screen);
            dx += (b[0] - a[0]).abs();
            dy += (b[1] - a[1]).abs();
        }
        let high = s
            .iter()
            .map(|x| x.at[0].screen[1].abs())
            .fold(0.0, f32::max);
        assert!(
            dy < 0.3 * dx && high < 0.45,
            "the level strike runs across, not down: across {dx:.2}, up/down {dy:.2}, \
             furthest off the crosshair's height {high:.2}\n{}",
            table(&r)
        );
    }

    #[test]
    fn the_wind_up_draws_back_far_and_long_enough() {
        // face-a-only: view space. Owner review: a 0.1 s flick with no draw back over the shoulder.
        for role in ["chop_level", "chop_vertical"] {
            let mut p = Probe::new(role);
            p.samples = 121;
            p.points = vec![Point::ToolHead, Point::Grip];
            let r = probe(&p).expect("probe");
            let w = r.phase("wind-up").unwrap();
            let d = w.to - w.from;
            assert!(
                (0.25..=0.35).contains(&d),
                "{role}: the wind-up takes {d:.2} s (want 0.25–0.35)"
            );
            let (a, b) = (r.at(0, w.from).local, r.at(0, w.to).local);
            let head = (0..3).map(|i| (b[i] - a[i]).powi(2)).sum::<f32>().sqrt();
            let (a, b) = (r.at(1, w.from).local, r.at(1, w.to).local);
            let grip = (0..3).map(|i| (b[i] - a[i]).powi(2)).sum::<f32>().sqrt();
            assert!(
                head >= 0.45 && grip >= 0.35,
                "{role}: the wind-up draws the head {head:.2} m and the hands {grip:.2} m back"
            );
        }
    }

    dwell_world::both_faces!(
        fn the_swings_go_the_same_way_in_the_world_on_either_face(f: FaceFrame) {
            // The arms are turned into the world by the camera's basis: the level strike moves
            // the head along the view's left, the vertical one along its down, on either face and
            // any heading.
            for yaw in [0.0, 70.0, 200.0] {
                for (role, way) in [("chop_level", 0usize), ("chop_vertical", 1)] {
                    let mut p = Probe::new(role);
                    (p.face, p.yaw, p.pitch) = (f, yaw, -10.0);
                    let r = probe(&p).expect("probe");
                    let s = within(&r, "strike");
                    let (a, b) = (s[0].at[0].world, s[s.len() - 1].at[0].world);
                    let d: [f64; 3] = std::array::from_fn(|k| b[k] - a[k]);
                    let basis = aim(yaw, -10.0, f, 0.0);
                    let axis = if way == 0 { basis.right } else { basis.up };
                    let along = (0..3).map(|k| d[k] * axis[k]).sum::<f64>();
                    assert!(
                        along < -0.2,
                        "{role} at yaw {yaw}: the head travels {along:.3} m along the view's \
                         {} (want left/down)",
                        if way == 0 { "right" } else { "up" }
                    );
                }
            }
        }
    );

    #[test]
    fn others_see_the_same_swing_on_the_body() {
        // face-a-only: the body's rig is posed face-locally (right, up, ahead of its feet); its
        // frame (`body_frame`) turns it onto either face like every other body.
        let r = run("chop_level");
        let s = r.phase("strike").unwrap();
        assert!(
            s.body[0] < -0.6 && s.body[0].abs() > 2.0 * s.body[1].abs(),
            "the body's level strike sweeps from its right to its left: {s:?}"
        );
        let c = r.at(0, r.contact_t.unwrap());
        assert!(
            c.body[2] > 0.5 && c.body[0].abs() < 0.15,
            "at contact the head is in front of the body: {c:?}"
        );
        let r = run("chop_vertical");
        let s = r.phase("strike").unwrap();
        assert!(
            s.body[1] < -0.8 && s.body[1].abs() > 2.0 * s.body[0].abs(),
            "the body's vertical strike comes down: {s:?}"
        );
        let w = r.phase("wind-up").unwrap();
        assert!(w.body[1] > 0.2, "it is raised first: {w:?}");
        // Both swings play their own body clips, not the generic break.
        let ch = player();
        for role in ["chop_level", "chop_vertical"] {
            let clip = ch.clip(role).map(|i| TABLES.clip(i).name);
            assert_eq!(clip, Some(&*format!("biped_{role}")), "{role}");
        }
    }

    #[test]
    fn the_sheet_draws_every_view_and_the_table_names_the_phases() {
        let r = run("chop_level");
        let s = svg(&r, &VIEWS);
        for v in VIEWS {
            assert!(s.contains(view_title(v)), "{v}");
        }
        assert!(s.starts_with("<svg") && s.trim_end().ends_with("</svg>"));
        let t = table(&r);
        assert!(t.contains("strike") && t.contains("right→left"), "{t}");
        let c = csv(&r);
        assert_eq!(c.lines().count(), 1 + r.samples.len() * r.points.len());
        assert!(Point::parse("elbow").unwrap() == Point::Bone("forearm_r"));
    }
}
