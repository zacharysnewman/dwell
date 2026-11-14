//! The run-time plume list (docs/CLOUDS.md §8.7): columns of cloud — steam over a lava vent, a
//! volcano's ash, a weather system's towers — that the game adds, moves and removes while it runs,
//! drawn by the clouds pass's march from one uniform of `plume_slots` plumes (`clouds.json`), so a
//! new plume is a write to that uniform, not a new pipeline. The list starts with `clouds.json`'s
//! `plumes` (on face A); each plume is placed face-locally on its face ([`FaceFrame`]), and a viewer
//! sees only its own face's.

use crate::Rgb;
use crate::clouds::{CloudQuality, CloudTunables, Plume, PlumeSource};
use dwell_world::FaceFrame;

/// One plume of the list: its face, its axis at world `x`, `z`, its heights face-locally above
/// sea level on that face, and its look.
#[derive(Clone, Debug, PartialEq)]
pub struct PlumeSpec {
    pub face: FaceFrame,
    pub x_m: f64,
    pub z_m: f64,
    pub base_m: f64,
    pub top_m: f64,
    pub radius_m: f64,
    pub spread: f64,
    pub density: f64,
    /// sRGB, as the clouds' other colours.
    pub color: Rgb,
    pub rise_mps: f64,
}

impl PlumeSpec {
    /// A data plume (`clouds.json` `plumes`), on face A.
    pub fn from_data(p: &Plume) -> Self {
        Self {
            face: FaceFrame::A,
            x_m: p.x_m,
            z_m: p.z_m,
            base_m: p.base_m,
            top_m: p.top_m,
            radius_m: p.radius_m,
            spread: p.spread,
            density: p.density,
            color: p.color,
            rise_mps: p.rise_mps,
        }
    }

    /// A source's plume (`clouds.json` `sources`) over face-local point `at` (x, height above sea
    /// level, z) on face `face`.
    pub fn from_source(s: &PlumeSource, face: FaceFrame, at: [f64; 3]) -> Self {
        Self {
            face,
            x_m: at[0],
            z_m: at[2],
            base_m: at[1],
            top_m: at[1] + s.height_m,
            radius_m: s.radius_m,
            spread: s.spread,
            density: s.density,
            color: s.color,
            rise_mps: s.rise_mps,
        }
    }

    /// Why it can't be drawn, if it can't (non-finite numbers, a top not over its base, …).
    pub fn check(&self) -> Result<(), String> {
        let nums = [
            self.x_m,
            self.z_m,
            self.base_m,
            self.top_m,
            self.radius_m,
            self.spread,
            self.density,
            self.rise_mps,
        ];
        if nums.iter().any(|v| !v.is_finite()) || self.color.iter().any(|c| !c.is_finite()) {
            return Err("a plume's numbers must be finite".into());
        }
        if self.top_m <= self.base_m {
            return Err("a plume's top must be over its base".into());
        }
        if self.radius_m <= 0.0 || self.spread < 0.0 || self.density < 0.0 {
            return Err(
                "a plume's radius must be positive, its spread and density not negative".into(),
            );
        }
        Ok(())
    }
}

/// The floats of one packed plume (three vec4s) and of the list's head.
pub const PLUME_FLOATS: usize = 12;
pub const HEAD_FLOATS: usize = 4;

/// The run-time plumes, by name, at most `slots`.
#[derive(Clone, Debug)]
pub struct PlumeList {
    slots: usize,
    items: Vec<(String, PlumeSpec)>,
}

impl PlumeList {
    /// An empty list of `slots` plumes.
    pub fn empty(slots: u32) -> Self {
        Self {
            slots: slots as usize,
            items: Vec::new(),
        }
    }

    /// The list `t` starts with: its `plumes`, in `plume_slots` slots.
    pub fn new(t: &CloudTunables) -> Self {
        let mut list = Self::empty(t.plume_slots);
        for p in t.plumes {
            list.set(p.name, PlumeSpec::from_data(p))
                .expect("clouds.mjs checks the data plumes fit and are drawable");
        }
        list
    }

    pub fn slots(&self) -> usize {
        self.slots
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn get(&self, name: &str) -> Option<&PlumeSpec> {
        self.items.iter().find(|(n, _)| n == name).map(|(_, p)| p)
    }
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.items.iter().map(|(n, _)| n.as_str())
    }

    /// Adds plume `name`, or replaces it if the list has it. Fails, changing nothing, when the
    /// plume can't be drawn or the list is full.
    pub fn set(&mut self, name: &str, p: PlumeSpec) -> Result<(), String> {
        p.check()?;
        if let Some(slot) = self.items.iter_mut().find(|(n, _)| n == name) {
            slot.1 = p;
            return Ok(());
        }
        if self.items.len() >= self.slots {
            return Err(format!("the plume list is full ({} slots)", self.slots));
        }
        self.items.push((name.to_owned(), p));
        Ok(())
    }

    /// Moves plume `name`'s axis to world `x`, `z` and its base to `base_m` (its height kept).
    pub fn move_to(&mut self, name: &str, x_m: f64, z_m: f64, base_m: f64) -> Result<(), String> {
        let Some((_, p)) = self.items.iter_mut().find(|(n, _)| n == name) else {
            return Err(format!("no plume {name:?}"));
        };
        let moved = PlumeSpec {
            x_m,
            z_m,
            top_m: base_m + (p.top_m - p.base_m),
            base_m,
            ..p.clone()
        };
        moved.check()?;
        *p = moved;
        Ok(())
    }

    /// Removes plume `name`; whether it had it.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|(n, _)| n != name);
        self.items.len() != before
    }

    /// Removes every plume whose name starts with `prefix` (a source's own); how many.
    pub fn remove_prefixed(&mut self, prefix: &str) -> usize {
        let before = self.items.len();
        self.items.retain(|(n, _)| !n.starts_with(prefix));
        before - self.items.len()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// The clouds pass's plume uniform (`CloudPlumes`) for an eye at world `eye` on face `face` at
    /// clock `seconds`, drawn at setting `quality`: the head (plumes in use, samples through each)
    /// then `slots` plumes (axis offset from the eye, radius, spread; base, top, density, rise so
    /// far; colour). Only the eye's face's plumes, within `reach_m` of it across (past its edge
    /// the march can't reach), nearest first; none on a tier without plume samples (Low, Off).
    pub fn pack(
        &self,
        t: &CloudTunables,
        eye: [f64; 3],
        face: FaceFrame,
        seconds: f64,
        quality: CloudQuality,
    ) -> Vec<f32> {
        let mut out = vec![0.0_f32; HEAD_FLOATS + PLUME_FLOATS * self.slots];
        let tier = quality.volume(t);
        if tier.steps == 0 || tier.plume_steps == 0 {
            return out;
        }
        let reach = t.volume.max_distance_m;
        // No cloud past the atmosphere's top (docs/DAY_NIGHT.md §15): a plume is cut there.
        let ceiling = crate::atmosphere::ATMOSPHERE.top_m;
        let mut near: Vec<(f64, &PlumeSpec)> = self
            .items
            .iter()
            .map(|(_, p)| p)
            .filter(|p| p.face == face && p.base_m < ceiling)
            .filter_map(|p| {
                let r = p.radius_m * (1.0 + p.spread);
                let d = ((p.x_m - eye[0]).powi(2) + (p.z_m - eye[2]).powi(2)).sqrt();
                (d - r <= reach).then_some((d, p))
            })
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        out[0] = near.len() as f32;
        out[1] = tier.plume_steps as f32;
        for (i, (_, p)) in near.iter().enumerate() {
            let o = HEAD_FLOATS + i * PLUME_FLOATS;
            // The billows' rise so far, wrapped every 4096 base radii (value noise's tile is far
            // larger than any plume, so the wrap never shows) to keep the f32 small.
            let rise = (p.rise_mps * seconds).rem_euclid(4096.0 * p.radius_m);
            let v = [
                p.x_m - eye[0],
                p.z_m - eye[2],
                p.radius_m,
                p.spread,
                p.base_m,
                p.top_m.min(ceiling),
                p.density,
                rise,
                p.color[0],
                p.color[1],
                p.color[2],
                0.0,
            ];
            for (k, x) in v.iter().enumerate() {
                out[o + k] = *x as f32;
            }
        }
        out
    }
}

/// The plume list's dev tools (`__dwell.plume(op, args)`, docs/DEV_TOOLS.md), JSON back, for an
/// eye at world `eye`: `list`; `add` [name, x, y, z, radius?, height?, density?, riseMps?] a plume
/// of the lava vent's look (`sources.lavaVent`, its numbers overridden by the optional ones) based
/// at world point x, y, z (its face that point's); `near` [name, dx, dz] one 40 m under the eye,
/// dx, dz across from it; `move` [name, x, y, z]; `remove` [name]; `clear`.
pub fn dev(list: &mut PlumeList, op: &str, name: &str, args: &[f64], eye: [f64; 3]) -> String {
    let sea = dwell_protocol::world::SEA_LEVEL_M;
    let place = |at: [f64; 3]| {
        let f = FaceFrame::at_height(at[1]);
        (f, [at[0], f.y(at[1]) - sea, at[2]])
    };
    let made = |at: [f64; 3], extra: &[f64]| {
        let (f, local) = place(at);
        let mut s = crate::clouds::CLOUDS.sources.lava_vent;
        let fields = [
            &mut s.radius_m,
            &mut s.height_m,
            &mut s.density,
            &mut s.rise_mps,
        ];
        for (v, x) in fields.into_iter().zip(extra) {
            *v = *x;
        }
        PlumeSpec::from_source(&s, f, local)
    };
    let done = |list: &PlumeList, r: Result<(), String>| match r {
        Ok(()) => format!(r#"{{"plumes":{}}}"#, list.len()),
        Err(e) => format!(r#"{{"error":{e:?}}}"#),
    };
    match (op, args) {
        ("list", _) => list_json(list),
        ("add", [x, y, z, extra @ ..]) if !name.is_empty() => {
            let p = made([*x, *y, *z], extra);
            let r = list.set(name, p);
            done(list, r)
        }
        ("near", [dx, dz]) if !name.is_empty() => {
            let p = made(
                [
                    eye[0] + dx,
                    eye[1] - FaceFrame::at_height(eye[1]).sign_f64() * 40.0,
                    eye[2] + dz,
                ],
                &[],
            );
            let r = list.set(name, p);
            done(list, r)
        }
        ("move", [x, y, z]) => {
            let (f, local) = place([*x, *y, *z]);
            let r = match list.get(name) {
                Some(p) if p.face != f => Err("a plume moves on its own face".to_owned()),
                _ => list.move_to(name, local[0], local[2], local[1]),
            };
            done(list, r)
        }
        ("remove", _) => {
            let had = list.remove(name);
            format!(r#"{{"removed":{had}}}"#)
        }
        ("clear", _) => {
            list.clear();
            done(list, Ok(()))
        }
        _ => format!(
            r#"{{"error":"unknown plume op {op:?} with {} args"}}"#,
            args.len()
        ),
    }
}

/// The list as JSON: each plume's name, face, axis, heights and look.
pub fn list_json(list: &PlumeList) -> String {
    let items: Vec<String> = list
        .items
        .iter()
        .map(|(n, p)| {
            format!(
                r#"{{"name":{n:?},"face":"{}","x":{},"z":{},"baseM":{},"topM":{},"radiusM":{},"density":{},"riseMps":{}}}"#,
                if p.face.is_b() { "b" } else { "a" },
                p.x_m,
                p.z_m,
                p.base_m,
                p.top_m,
                p.radius_m,
                p.density,
                p.rise_mps
            )
        })
        .collect();
    format!(
        r#"{{"slots":{},"plumes":[{}]}}"#,
        list.slots,
        items.join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clouds::CLOUDS;
    use dwell_world::both_faces;

    fn steam(face: FaceFrame, x: f64) -> PlumeSpec {
        PlumeSpec::from_source(&CLOUDS.sources.lava_vent, face, [x, 200.0, 0.0])
    }

    #[test]
    fn the_list_starts_with_the_data_plumes_and_holds_its_slots() {
        let list = PlumeList::new(&CLOUDS);
        assert_eq!(list.len(), CLOUDS.plumes.len());
        assert_eq!(list.slots(), CLOUDS.plume_slots as usize);
        let packed = list.pack(&CLOUDS, [0.0; 3], FaceFrame::A, 0.0, CloudQuality::Medium);
        assert_eq!(
            packed.len(),
            HEAD_FLOATS + PLUME_FLOATS * CLOUDS.plume_slots as usize,
            "the uniform is the same size whatever the list holds"
        );
    }

    #[test]
    fn plumes_add_move_replace_and_remove_by_name_within_the_slots() {
        let mut list = PlumeList::empty(2);
        list.set("a", steam(FaceFrame::A, 10.0)).unwrap();
        list.set("b", steam(FaceFrame::A, 20.0)).unwrap();
        assert!(
            list.set("c", steam(FaceFrame::A, 30.0))
                .unwrap_err()
                .contains("full")
        );
        // Replacing one by name needs no slot.
        list.set("a", steam(FaceFrame::A, 15.0)).unwrap();
        assert_eq!(list.get("a").unwrap().x_m, 15.0);
        list.move_to("b", -5.0, 7.0, 400.0).unwrap();
        let b = list.get("b").unwrap();
        assert_eq!((b.x_m, b.z_m, b.base_m), (-5.0, 7.0, 400.0));
        assert_eq!(b.top_m - b.base_m, CLOUDS.sources.lava_vent.height_m);
        assert!(list.move_to("zz", 0.0, 0.0, 0.0).is_err());
        assert!(list.remove("a"));
        assert!(!list.remove("a"));
        list.set("c", steam(FaceFrame::A, 30.0)).unwrap();
        assert_eq!(list.names().collect::<Vec<_>>(), ["b", "c"]);
        list.set("lava:1", steam(FaceFrame::A, 1.0)).unwrap_err();
        list.clear();
        list.set("lava:1", steam(FaceFrame::A, 1.0)).unwrap();
        list.set("lava:2", steam(FaceFrame::A, 2.0)).unwrap();
        assert_eq!(list.remove_prefixed("lava:"), 2);
        assert!(list.is_empty());
    }

    #[test]
    fn a_plume_that_cant_be_drawn_is_refused() {
        let mut list = PlumeList::empty(4);
        let bad = [
            PlumeSpec {
                top_m: 100.0,
                ..steam(FaceFrame::A, 0.0)
            },
            PlumeSpec {
                radius_m: 0.0,
                ..steam(FaceFrame::A, 0.0)
            },
            PlumeSpec {
                x_m: f64::NAN,
                ..steam(FaceFrame::A, 0.0)
            },
        ];
        for p in bad {
            assert!(list.set("x", p).is_err());
        }
        assert!(list.is_empty());
        list.set("x", steam(FaceFrame::A, 0.0)).unwrap();
        assert!(list.move_to("x", f64::INFINITY, 0.0, 0.0).is_err());
        assert_eq!(
            list.get("x").unwrap().x_m,
            0.0,
            "a refused move changes nothing"
        );
    }

    both_faces!(
        fn no_plume_is_drawn_past_the_atmospheres_top(f: FaceFrame) {
            // A plume reaching past the top is cut at it; one based above it is not drawn.
            let top = crate::atmosphere::ATMOSPHERE.top_m;
            let mut list = PlumeList::empty(4);
            list.set(
                "tall",
                PlumeSpec {
                    top_m: top + 50_000.0,
                    ..steam(f, 0.0)
                },
            )
            .unwrap();
            list.set(
                "space",
                PlumeSpec {
                    base_m: top + 10.0,
                    top_m: top + 500.0,
                    ..steam(f, 10.0)
                },
            )
            .unwrap();
            let p = list.pack(&CLOUDS, [0.0, f.y(300.0), 0.0], f, 0.0, CloudQuality::High);
            assert_eq!(p[0], 1.0, "the one above the top is left out");
            assert_eq!(
                p[HEAD_FLOATS + 5],
                top as f32,
                "the tall one is cut at the top"
            );
        }
    );

    #[test]
    fn packing_puts_the_nearest_first_relative_to_the_eye_and_drops_the_out_of_reach() {
        let mut list = PlumeList::empty(4);
        list.set("far", steam(FaceFrame::A, 900.0)).unwrap();
        list.set("near", steam(FaceFrame::A, 100.0)).unwrap();
        let reach = CLOUDS.volume.max_distance_m;
        list.set("gone", steam(FaceFrame::A, 2.0 * reach)).unwrap();
        let eye = [50.0, 300.0, 0.0];
        let p = list.pack(&CLOUDS, eye, FaceFrame::A, 10.0, CloudQuality::High);
        assert_eq!(p[0], 2.0, "the one past the march's reach is left out");
        assert_eq!(p[1], CLOUDS.tiers.high.plume_steps as f32);
        let first = &p[HEAD_FLOATS..HEAD_FLOATS + PLUME_FLOATS];
        let s = CLOUDS.sources.lava_vent;
        assert_eq!(first[0], 50.0, "the near one, offset from the eye");
        assert_eq!(first[2], s.radius_m as f32);
        assert_eq!([first[4], first[5]], [200.0, (200.0 + s.height_m) as f32]);
        assert_eq!(first[6], s.density as f32);
        assert_eq!(first[7], (s.rise_mps * 10.0) as f32, "its billows' rise");
        assert_eq!(first[8], s.color[0] as f32);
        assert_eq!(p[HEAD_FLOATS + PLUME_FLOATS], 850.0, "then the far one");
        // The unused slots stay zero.
        assert!(
            p[HEAD_FLOATS + 2 * PLUME_FLOATS..]
                .iter()
                .all(|v| *v == 0.0)
        );
    }

    #[test]
    fn no_plumes_on_a_tier_without_plume_samples() {
        let mut list = PlumeList::empty(2);
        list.set("a", steam(FaceFrame::A, 10.0)).unwrap();
        for q in [CloudQuality::Off, CloudQuality::Low] {
            let p = list.pack(&CLOUDS, [0.0; 3], FaceFrame::A, 0.0, q);
            assert!(p.iter().all(|v| *v == 0.0), "{q:?} draws no plumes");
        }
        for q in [CloudQuality::Medium, CloudQuality::High] {
            let p = list.pack(&CLOUDS, [0.0; 3], FaceFrame::A, 0.0, q);
            assert_eq!(p[0], 1.0, "{q:?} draws it");
        }
    }

    both_faces! {
        fn a_viewer_sees_its_own_faces_plumes_only(f: FaceFrame) {
            let mut list = PlumeList::empty(4);
            // Placed face-locally: 200 m above sea level on face `f`.
            list.set("here", steam(f, 10.0)).unwrap();
            list.set("there", steam(f.other(), 20.0)).unwrap();
            let eye = f.point([0.0, 300.0, 0.0]);
            let p = list.pack(&CLOUDS, eye, f, 0.0, CloudQuality::Medium);
            assert_eq!(p[0], 1.0);
            assert_eq!(p[HEAD_FLOATS], 10.0);
            // Heights stay face-local: the same numbers on either face.
            assert_eq!(p[HEAD_FLOATS + 4], 200.0);
        }
    }

    both_faces! {
        fn the_dev_tools_add_move_and_remove_a_plume_on_the_points_face(f: FaceFrame) {
            let sea = dwell_protocol::world::SEA_LEVEL_M;
            let mut list = PlumeList::empty(4);
            let at = f.point([10.0, sea + 250.0, -4.0]);
            let r = dev(&mut list, "add", "s", &[at[0], at[1], at[2], 30.0], at);
            assert_eq!(r, r#"{"plumes":1}"#);
            let p = list.get("s").unwrap();
            assert_eq!(p.face, f);
            assert!((p.base_m - 250.0).abs() < 1e-9, "face-local over sea level");
            assert_eq!(p.radius_m, 30.0);
            let to = f.point([20.0, sea + 300.0, 0.0]);
            assert_eq!(dev(&mut list, "move", "s", &to, at), r#"{"plumes":1}"#);
            assert_eq!(list.get("s").unwrap().x_m, 20.0);
            let other = f.other().point([0.0, sea + 300.0, 0.0]);
            assert!(dev(&mut list, "move", "s", &other, at).contains("own face"));
            dev(&mut list, "near", "n", &[5.0, 0.0], to);
            let n = list.get("n").unwrap();
            assert!((n.base_m - 260.0).abs() < 1e-9, "40 m under the eye");
            assert!(dev(&mut list, "list", "", &[], at).contains(r#""name":"n""#));
            assert_eq!(dev(&mut list, "remove", "s", &[], at), r#"{"removed":true}"#);
            assert!(dev(&mut list, "fly", "", &[], at).contains("error"));
            dev(&mut list, "clear", "", &[], at);
            assert!(list.is_empty());
        }
    }

    #[test]
    fn a_ray_meets_a_plumes_column_where_the_shader_marches_it() {
        use crate::clouds::plume_span;
        // Level through the axis at 200 m, from 100 m away: in at 100 − r, out at 100 + r.
        let r = 50.0;
        let (t0, t1) =
            plume_span([1.0, 0.0], 0.0, 200.0, [100.0, 0.0], r, [100.0, 300.0], 1e4).unwrap();
        assert!((t0 - 50.0).abs() < 1e-9 && (t1 - 150.0).abs() < 1e-9);
        // Above its top, level: no hit; aimed away: none.
        assert!(plume_span([1.0, 0.0], 0.0, 400.0, [100.0, 0.0], r, [100.0, 300.0], 1e4).is_none());
        assert!(
            plume_span(
                [-1.0, 0.0],
                0.0,
                200.0,
                [100.0, 0.0],
                r,
                [100.0, 300.0],
                1e4
            )
            .is_none()
        );
        // Straight up from inside the column: from the base to the top.
        let (t0, t1) =
            plume_span([0.0, 0.0], 1.0, 0.0, [10.0, 0.0], r, [100.0, 300.0], 1e4).unwrap();
        assert!((t0 - 100.0).abs() < 1e-9 && (t1 - 300.0).abs() < 1e-9);
        // The density: full on the axis mid-way, none past the radius or out of the heights.
        let s = CLOUDS.sources.lava_vent;
        let d = |dx, u| crate::clouds::plume_density(s.radius_m, s.spread, s.density, dx, 0.0, u);
        assert!((d(0.0, 0.5) - s.density).abs() < 1e-9);
        assert_eq!(d(s.radius_m * (1.0 + s.spread) + 1.0, 1.0 - 1e-9), 0.0);
        assert_eq!(d(0.0, -0.1), 0.0);
        assert_eq!(d(0.0, 1.1), 0.0);
    }
}
