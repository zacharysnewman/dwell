//! Players' looks on the client (the Character painter, docs/CHARACTERS.md §9.4): the paint of each
//! other player (from the server's `Looks`) and your own, as the renderer draws them.

use dwell_models::paint::{PAINT_SLOTS, Paint};
use dwell_protocol::looks::PlayerLook;
use dwell_world::hash::HashMap;

/// A look as paint, if it fits the player model (its face count and ramps).
pub fn paint_of(look: &PlayerLook) -> Option<Paint> {
    let (_, model) = dwell_models::model(dwell_models::PLAYER)?;
    if look.slots.len() != PAINT_SLOTS {
        return None;
    }
    let p = Paint {
        slots: std::array::from_fn(|k| look.slots[k].clone()),
        faces: look.faces.clone(),
    };
    p.fits(model).then_some(p)
}

/// A paint as a look to send.
pub fn look_of(paint: &Paint) -> PlayerLook {
    PlayerLook {
        player_id: 0,
        slots: paint.slots.to_vec(),
        faces: paint.faces.clone(),
    }
}

/// A paint for the device's store: `{"slots": [...], "faces": "<slot digits>"}`.
pub fn to_json(paint: &Paint) -> serde_json::Value {
    let faces: String = paint.faces.iter().map(|&f| char::from(b'0' + f)).collect();
    serde_json::json!({ "slots": paint.slots, "faces": faces })
}

/// A stored paint back, if it fits the player model.
pub fn from_json(v: &serde_json::Value) -> Option<Paint> {
    let slots: Vec<String> = v["slots"]
        .as_array()?
        .iter()
        .map(|s| s.as_str().map(str::to_string))
        .collect::<Option<_>>()?;
    let faces = v["faces"]
        .as_str()?
        .bytes()
        .map(|b| b.checked_sub(b'0'))
        .collect::<Option<Vec<u8>>>()?;
    paint_of(&PlayerLook {
        player_id: 0,
        slots,
        faces,
    })
}

/// A paint's key: the renderer keeps one mesh a distinct paint.
pub fn key_of(paint: &Paint) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    paint.slots.hash(&mut h);
    paint.faces.hash(&mut h);
    h.finish() | 1
}

/// The Character painter's giant on its empty stage (docs/CHARACTERS.md §9.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StageGiant {
    /// Its feet (world space).
    pub feet: [f64; 3],
    /// Degrees; 0 faces +z.
    pub yaw: f64,
    pub face: dwell_world::FaceFrame,
    /// Its size over the model's (30 m tall).
    pub size: f32,
    /// The paint being painted's key.
    pub paint: u64,
}

#[derive(Default)]
pub struct Looks {
    /// Others' paint by player id, with its key.
    others: HashMap<u16, (u64, std::sync::Arc<Paint>)>,
    /// Your own paint (None: the model's own colours).
    own: Option<(u64, std::sync::Arc<Paint>)>,
    /// The server takes looks (`players.looks`).
    pub synced: bool,
    /// Yours changed since it last went.
    pub dirty: bool,
    /// Changed since the renderer last had them.
    pub drawn_dirty: bool,
    /// The Character painter, while it is open, and its paint as drawn on the giant.
    pub painter: Option<Painter>,
    painting: Option<(u64, std::sync::Arc<Paint>)>,
}

impl Looks {
    /// The server's looks: each replaces that player's (an empty or unfitting one clears it).
    pub fn apply(&mut self, list: Vec<PlayerLook>) {
        self.drawn_dirty = true;
        for l in list {
            match paint_of(&l) {
                Some(p) => {
                    self.others
                        .insert(l.player_id, (key_of(&p), std::sync::Arc::new(p)));
                }
                None => {
                    self.others.remove(&l.player_id);
                }
            }
        }
    }

    /// Your own paint (None: the model's colours); sent once the server takes looks.
    pub fn set_own(&mut self, paint: Option<Paint>) {
        self.own = paint.map(|p| (key_of(&p), std::sync::Arc::new(p)));
        self.dirty = true;
        self.drawn_dirty = true;
    }

    pub fn own(&self) -> Option<&(u64, std::sync::Arc<Paint>)> {
        self.own.as_ref()
    }

    pub fn of(&self, player_id: u16) -> Option<&(u64, std::sync::Arc<Paint>)> {
        self.others.get(&player_id)
    }

    /// Each other player's paint key.
    pub fn by_player(&self) -> impl Iterator<Item = (u16, u64)> + '_ {
        self.others.iter().map(|(&id, (k, _))| (id, *k))
    }

    /// Every paint drawn now (others', yours and the painter's), by key.
    pub fn paints(&self) -> impl Iterator<Item = &(u64, std::sync::Arc<Paint>)> {
        self.others
            .values()
            .chain(self.own.iter())
            .chain(self.painting.iter())
    }

    /// The painter's paint changed (or it opened or closed): drawn again.
    pub fn refresh_painting(&mut self) {
        self.painting = self
            .painter
            .as_ref()
            .map(|p| (p.giant.paint, std::sync::Arc::new(p.paint.clone())));
        self.drawn_dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_look_that_fits_is_paint_and_one_that_doesnt_clears() {
        // face-a-only: model space, no face frame.
        let (data, model) = dwell_models::model(dwell_models::PLAYER).unwrap();
        let paint = dwell_models::paint::default_paint(data, model);
        let mut look = look_of(&paint);
        look.player_id = 4;
        let mut looks = Looks::default();
        looks.apply(vec![look.clone()]);
        assert_eq!(looks.of(4).map(|(_, p)| &**p), Some(&paint));
        // Another build's model (a different face count) or an unknown ramp: drawn as the model.
        let mut short = look.clone();
        short.faces.pop();
        looks.apply(vec![short]);
        assert!(looks.of(4).is_none());
        looks.apply(vec![look.clone()]);
        look.slots[2] = "no_such".into();
        looks.apply(vec![look]);
        assert!(looks.of(4).is_none());
        // The device's store keeps it as JSON and reads it back.
        assert_eq!(from_json(&to_json(&paint)), Some(paint));
        assert_eq!(
            from_json(&serde_json::json!({"slots": [], "faces": "0"})),
            None
        );
    }
}

/// The giant's height on the painter's stage (owner, 2026-11-26: "as if the character model was a
/// giant 30m tall"), and how far its soles stand above where the painter was opened ("5m in the air,
/// so the feet bottoms are visible").
pub const STAGE_HEIGHT_M: f64 = 30.0;
pub const STAGE_LIFT_M: f64 = 5.0;

/// The Character painter (docs/CHARACTERS.md §9.4): the paint being painted on the stage's giant,
/// with its undo and redo.
pub struct Painter {
    pub paint: Paint,
    undo: Vec<Paint>,
    redo: Vec<Paint>,
    /// A stroke is open (its first change pushed the undo step).
    stroke: bool,
    pub giant: StageGiant,
}

/// The player model's height (m, life size): its rest mesh's top over its soles.
pub fn model_height_m() -> f64 {
    let Some((data, model)) = dwell_models::model(dwell_models::PLAYER) else {
        return 1.8;
    };
    let ys = model.level(0).positions.iter().map(|p| p[1]);
    let (lo, hi) = ys.fold((f32::MAX, f32::MIN), |(a, b), y| (a.min(y), b.max(y)));
    f64::from((hi - lo) * data.scale)
}

impl Painter {
    /// A painter over `paint`, its giant standing `STAGE_LIFT_M` above `feet` (the player's), turned
    /// to face `yaw` back toward where the camera starts.
    pub fn new(paint: Paint, feet: [f64; 3], face: dwell_world::FaceFrame, yaw: f64) -> Self {
        let up = face.up();
        let feet = std::array::from_fn(|k| feet[k] + up[k] * STAGE_LIFT_M);
        let size = (STAGE_HEIGHT_M / model_height_m()) as f32;
        let mut p = Self {
            paint,
            undo: Vec::new(),
            redo: Vec::new(),
            stroke: false,
            giant: StageGiant {
                feet,
                yaw,
                face,
                size,
                paint: 0,
            },
        };
        p.giant.paint = key_of(&p.paint);
        p
    }

    /// The painted level's triangles relative to `eye` (world axes), at the giant's rest pose.
    fn triangles(&self, eye: [f64; 3]) -> Vec<[[f32; 3]; 3]> {
        let Some((data, model)) = dwell_models::model(dwell_models::PLAYER) else {
            return Vec::new();
        };
        let g = &self.giant;
        let place = crate::render::characters::place_frame(g.feet, g.yaw, g.face, eye);
        let turn = dwell_models::math::axis_angle([0.0, 1.0, 0.0], data.yaw);
        let local = dwell_models::math::trs([0.0; 3], turn, [data.scale * g.size; 3]);
        let m = model.level(dwell_models::paint::PAINT_LEVEL);
        let world = |i: u32| {
            let p = dwell_models::math::point(&local, m.positions[i as usize]);
            std::array::from_fn(|k| {
                place.m[0][k] * p[0] + place.m[1][k] * p[1] + place.m[2][k] * p[2] + place.t[k]
            })
        };
        m.indices
            .chunks_exact(3)
            .map(|t| [world(t[0]), world(t[1]), world(t[2])])
            .collect()
    }

    /// The face a ray from `eye` along `dir` (world) hits first, and where (relative to `eye`).
    pub fn hit(&self, eye: [f64; 3], dir: [f64; 3]) -> Option<(usize, [f32; 3])> {
        let d = dir.map(|v| v as f32);
        let mut best: Option<(usize, f32)> = None;
        for (i, t) in self.triangles(eye).iter().enumerate() {
            if let Some(s) = ray_triangle(d, t)
                && best.is_none_or(|(_, b)| s < b)
            {
                best = Some((i, s));
            }
        }
        best.map(|(i, s)| (i, d.map(|v| v * s)))
    }

    fn begin_change(&mut self) {
        if !self.stroke {
            self.undo.push(self.paint.clone());
            self.redo.clear();
            self.stroke = true;
        }
    }

    /// Ends a stroke (the button released): the next change is a new undo step.
    pub fn end_stroke(&mut self) {
        self.stroke = false;
    }

    fn changed(&mut self) {
        self.giant.paint = key_of(&self.paint);
    }

    /// Paints with `slot` every face whose centre is within `radius_m` of where the ray hits.
    /// Whether any face changed.
    pub fn brush(&mut self, eye: [f64; 3], dir: [f64; 3], slot: u8, radius_m: f32) -> bool {
        let Some((_, at)) = self.hit(eye, dir) else {
            return false;
        };
        let near: Vec<usize> = self
            .triangles(eye)
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                let c: [f32; 3] = std::array::from_fn(|k| (t[0][k] + t[1][k] + t[2][k]) / 3.0);
                (0..3).map(|k| (c[k] - at[k]).powi(2)).sum::<f32>() <= radius_m * radius_m
            })
            .map(|(i, _)| i)
            .filter(|&i| self.paint.faces[i] != slot)
            .collect();
        if near.is_empty() {
            return false;
        }
        self.begin_change();
        for i in near {
            self.paint.faces[i] = slot;
        }
        self.changed();
        true
    }

    /// Fills with `slot` the hit face and every face joined to it by an edge in its slot.
    pub fn fill(&mut self, eye: [f64; 3], dir: [f64; 3], slot: u8) -> bool {
        let Some((start, _)) = self.hit(eye, dir) else {
            return false;
        };
        let from = self.paint.faces[start];
        if from == slot {
            return false;
        }
        let tris = self.triangles(eye);
        let key = |p: [f32; 3]| p.map(|v| (v * 64.0).round() as i64);
        let mut by_edge: HashMap<([i64; 3], [i64; 3]), Vec<usize>> = HashMap::default();
        for (i, t) in tris.iter().enumerate() {
            for e in 0..3 {
                let (a, b) = (key(t[e]), key(t[(e + 1) % 3]));
                by_edge.entry((a.min(b), a.max(b))).or_default().push(i);
            }
        }
        self.begin_change();
        let mut todo = vec![start];
        while let Some(i) = todo.pop() {
            if self.paint.faces[i] != from {
                continue;
            }
            self.paint.faces[i] = slot;
            let t = tris[i];
            for e in 0..3 {
                let (a, b) = (key(t[e]), key(t[(e + 1) % 3]));
                if let Some(n) = by_edge.get(&(a.min(b), a.max(b))) {
                    todo.extend(n.iter().copied().filter(|&j| self.paint.faces[j] == from));
                }
            }
        }
        self.end_stroke();
        self.changed();
        true
    }

    /// Slot `k`'s colour (a paint ramp): every face painted with it changes.
    pub fn set_slot(&mut self, k: usize, ramp: &str) -> bool {
        if k >= PAINT_SLOTS
            || !dwell_models::PAINT_RAMPS.contains(&ramp)
            || self.paint.slots[k] == ramp
        {
            return false;
        }
        self.end_stroke();
        self.begin_change();
        self.paint.slots[k] = ramp.to_string();
        self.end_stroke();
        self.changed();
        true
    }

    pub fn undo(&mut self) -> bool {
        self.end_stroke();
        let Some(p) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(&mut self.paint, p));
        self.changed();
        true
    }

    pub fn redo(&mut self) -> bool {
        self.end_stroke();
        let Some(p) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(&mut self.paint, p));
        self.changed();
        true
    }

    /// Back to the model's own colours (an undo step).
    pub fn reset(&mut self) -> bool {
        let Some((data, model)) = dwell_models::model(dwell_models::PLAYER) else {
            return false;
        };
        let p = dwell_models::paint::default_paint(data, model);
        if p == self.paint {
            return false;
        }
        self.end_stroke();
        self.begin_change();
        self.paint = p;
        self.end_stroke();
        self.changed();
        true
    }
}

/// The distance along `d` from the origin to triangle `t` (Möller–Trumbore), either side.
fn ray_triangle(d: [f32; 3], t: &[[f32; 3]; 3]) -> Option<f32> {
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let (e1, e2) = (sub(t[1], t[0]), sub(t[2], t[0]));
    let p = cross(d, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-9 {
        return None;
    }
    let inv = 1.0 / det;
    let s = sub([0.0; 3], t[0]);
    let u = dot(s, p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = cross(s, e1);
    let v = dot(d, q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let dist = dot(e2, q) * inv;
    (dist > 0.0).then_some(dist)
}

#[cfg(test)]
mod painter_tests {
    use super::*;

    fn painter(f: dwell_world::FaceFrame) -> Painter {
        let (data, model) = dwell_models::model(dwell_models::PLAYER).unwrap();
        let paint = dwell_models::paint::default_paint(data, model);
        Painter::new(paint, f.point([0.0, 100.0, 0.0]), f, 0.0)
    }

    dwell_world::both_faces!(
        /// Owner, 2026-11-26: the giant 30 m tall, 5 m in the air; aimed at its chest from in front,
        /// the brush paints the faces there and nothing else, the fill a connected patch, and undo,
        /// redo and reset step through them.
        fn the_painter_paints_the_giant_where_it_is_aimed(f: dwell_world::FaceFrame) {
            let mut p = painter(f);
            let before = p.paint.clone();
            // Its top: about 30 m over its soles, which are 5 m up.
            let tris = p.triangles(f.point([0.0, 0.0, 0.0]));
            let ys: Vec<f32> = tris
                .iter()
                .flatten()
                .map(|v| v[1] * f.up()[1] as f32)
                .collect();
            let (lo, hi) = ys
                .iter()
                .fold((f32::MAX, f32::MIN), |(a, b), &y| (a.min(y), b.max(y)));
            assert!(
                (lo - 105.0).abs() < 1.0 && (hi - lo - 30.0).abs() < 1.0,
                "{lo} {hi}"
            );
            // Aimed from 45 m in front (yaw 0 faces +z) at its chest, 20 m up its body.
            let eye = f.point([0.0, 125.0, 45.0]);
            let dir = f.vector([0.0, 0.0, -1.0]);
            let (face, at) = p.hit(eye, dir).expect("the chest is hit");
            assert!((at[2].abs() - 45.0).abs() < 8.0, "hit {at:?}");
            let slot = (p.paint.faces[face] + 1) % 7;
            assert!(p.brush(eye, dir, slot, 1.5));
            p.end_stroke();
            let painted: Vec<usize> = (0..p.paint.faces.len())
                .filter(|&i| p.paint.faces[i] != before.faces[i])
                .collect();
            assert!(
                painted.contains(&face) && painted.len() < 60,
                "{} faces",
                painted.len()
            );
            // A miss paints nothing.
            assert!(!p.brush(eye, f.vector([0.0, 0.0, 1.0]), slot, 1.5));
            // Fill the patch the brush made with another slot: exactly those faces (and their
            // joined same-slot neighbours) change.
            let other = (slot + 1) % 7;
            assert!(p.fill(eye, dir, other));
            assert!(
                painted
                    .iter()
                    .all(|&i| p.paint.faces[i] == other || before.faces[i] == other)
            );
            // Undo steps back through the fill and the stroke; redo forward; reset to the model's.
            assert!(p.undo() && p.undo());
            assert_eq!(p.paint, before);
            assert!(p.redo());
            assert_ne!(p.paint, before);
            assert!(p.reset());
            assert_eq!(p.paint, before);
            // A slot's colour recolours its faces (one change, one undo step).
            assert!(p.set_slot(0, "skin_deep"));
            assert_eq!(p.paint.slots[0], "skin_deep");
            assert!(p.undo());
            assert_eq!(p.paint, before);
        }
    );
}
