//! Presets (THEATER_MODE.md §4): a named move with parameters and their ranges, as data in
//! `shared/theater/presets.json`, expanded into a [`Path`] around a subject from where the camera
//! is. Each move kind knows a fixed set of parameter names (the code); a preset picks a move, an
//! easing and its parameters' defaults and ranges (the data), so a new preset is a data entry.

use serde_json::{Value, json};

use dwell_world::FaceFrame;

use crate::easing::Easing;
use crate::eval::{Pose, add, sub};
use crate::path::{Path, TargetRef};

/// The game's presets.
pub const PRESETS_JSON: &str = include_str!("../../../shared/theater/presets.json");

/// The parameters every move takes: the length, the field of view and the viewer's time of day
/// (`timeFrom` → `timeTo`, plus whole `days`, for a time-lapse; left out, the world's).
const COMMON: [&str; 5] = ["seconds", "fov", "timeFrom", "timeTo", "days"];

/// The move kinds a preset can use, with the parameters each understands besides [`COMMON`].
pub const PRESET_MOVES: [(&str, &[&str]); 5] = [
    ("orbit", &["radiusM", "heightM", "degrees"]),
    ("dolly", &["fromM", "toM", "heightM", "vertigo"]),
    ("track", &["distanceM", "sideM", "heightM", "smoothingS"]),
    ("crane", &["backM", "fromM", "toM"]),
    ("pan", &["degrees", "pitch"]),
];

/// A parameter's default and range.
#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: String,
    pub default: Option<f64>,
    pub min: f64,
    pub max: f64,
}

/// A preset.
#[derive(Clone, Debug, PartialEq)]
pub struct Preset {
    pub name: String,
    pub label: String,
    pub mv: String,
    pub easing: Easing,
    pub params: Vec<Param>,
    /// The face whose up the camera holds (the preset's `up`: `"a"`, `"b"` or `"auto"`, the
    /// default); `None`: the face the camera is over.
    pub up: Option<FaceFrame>,
}

/// What a preset is expanded around.
#[derive(Clone, Debug)]
pub struct Context {
    /// The subject and where it is now (a pan needs none).
    pub subject: Option<(TargetRef, [f64; 3])>,
    /// Where the camera is: an orbit, dolly, track or rise starts on its side of the subject.
    pub camera: Pose,
    /// The face whose up the shot holds, over the preset's own `up` (`None`: the preset's).
    pub up: Option<FaceFrame>,
}

/// Reads a presets file, failing with a message that names the bad entry.
pub fn parse_presets(text: &str) -> Result<Vec<Preset>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("presets: not JSON: {e}"))?;
    let o = v.as_object().ok_or("presets must be an object")?;
    for k in o.keys() {
        if !["formatVersion", "presets"].contains(&k.as_str()) && !k.starts_with('$') {
            return Err(format!("presets: unknown field {k}"));
        }
    }
    if o.get("formatVersion").and_then(Value::as_u64) != Some(1) {
        return Err("presets: formatVersion must be 1".into());
    }
    let list = o
        .get("presets")
        .and_then(Value::as_array)
        .ok_or("presets: presets must be a list")?;
    let mut out: Vec<Preset> = Vec::new();
    for (i, p) in list.iter().enumerate() {
        let at = format!("presets[{i}]");
        let p = p
            .as_object()
            .ok_or_else(|| format!("{at} must be an object"))?;
        for k in p.keys() {
            if !["name", "label", "move", "easing", "params", "up"].contains(&k.as_str())
                && !k.starts_with('$')
            {
                return Err(format!("{at}: unknown field {k}"));
            }
        }
        let name = p
            .get("name")
            .and_then(Value::as_str)
            .filter(|n| {
                !n.is_empty()
                    && n.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            })
            .ok_or_else(|| format!("{at}.name must be lower_snake_case"))?
            .to_owned();
        let at = format!("preset {name}");
        if out.iter().any(|q| q.name == name) {
            return Err(format!("{at}: a second preset of that name"));
        }
        let label = p
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or(&name)
            .to_owned();
        let mv = p.get("move").and_then(Value::as_str).unwrap_or("");
        let Some(&(_, known)) = PRESET_MOVES.iter().find(|(m, _)| *m == mv) else {
            let names: Vec<&str> = PRESET_MOVES.iter().map(|(m, _)| *m).collect();
            return Err(format!("{at}.move must be one of {}", names.join(", ")));
        };
        let easing = match p.get("easing") {
            None => Easing::InOutSine,
            Some(e) => e
                .as_str()
                .and_then(Easing::by_name)
                .ok_or_else(|| format!("{at}.easing must be one of {}", Easing::names()))?,
        };
        let up = match p.get("up") {
            None => None,
            Some(u) => crate::path::parse_up(u, &format!("{at}.up"))?,
        };
        let mut params = Vec::new();
        if let Some(ps) = p.get("params") {
            let ps = ps
                .as_object()
                .ok_or_else(|| format!("{at}.params must be an object"))?;
            for (k, v) in ps {
                if !COMMON.contains(&k.as_str()) && !known.contains(&k.as_str()) {
                    return Err(format!(
                        "{at}.params: a {mv} preset has no parameter {k} ({}, {})",
                        known.join(", "),
                        COMMON.join(", ")
                    ));
                }
                let num = |f: &str| v.get(f).and_then(Value::as_f64);
                let (min, max) = (num("min"), num("max"));
                let (Some(min), Some(max)) = (min, max) else {
                    return Err(format!("{at}.params.{k} needs min and max"));
                };
                let default = num("default");
                if min > max || default.is_some_and(|d| d < min || d > max) {
                    return Err(format!(
                        "{at}.params.{k}: default must lie in [min, max] ({min}, {max})"
                    ));
                }
                params.push(Param {
                    name: k.clone(),
                    default,
                    min,
                    max,
                });
            }
        }
        if !params
            .iter()
            .any(|q| q.name == "seconds" && q.default.is_some())
        {
            return Err(format!("{at}.params.seconds needs a default"));
        }
        out.push(Preset {
            name,
            label,
            mv: mv.to_owned(),
            easing,
            params,
            up,
        });
    }
    Ok(out)
}

/// The game's presets (`shared/theater/presets.json`; a test checks it reads).
pub fn presets() -> Vec<Preset> {
    parse_presets(PRESETS_JSON).unwrap_or_default()
}

impl Preset {
    /// A parameter's value: the override (clamped to its range) or its default.
    fn value(&self, name: &str, overrides: &[(String, f64)]) -> Option<f64> {
        let p = self.params.iter().find(|p| p.name == name);
        let o = overrides.iter().find(|(n, _)| n == name).map(|&(_, v)| v);
        match (p, o) {
            (Some(p), Some(v)) => Some(v.clamp(p.min, p.max)),
            (Some(p), None) => p.default,
            (None, v) => v,
        }
    }

    /// The preset as a path around `ctx`'s subject, with `overrides` (name, value) of its
    /// parameters; fails for an override it has no parameter for, or a missing subject.
    pub fn expand(&self, ctx: &Context, overrides: &[(String, f64)]) -> Result<Path, String> {
        let known = PRESET_MOVES
            .iter()
            .find(|(m, _)| *m == self.mv)
            .map_or(&[][..], |&(_, k)| k);
        for (n, _) in overrides {
            if !COMMON.contains(&n.as_str()) && !known.contains(&n.as_str()) {
                return Err(format!("preset {}: no parameter {n}", self.name));
            }
        }
        let v = |n: &str, d: f64| self.value(n, overrides).unwrap_or(d);
        let seconds = v("seconds", 10.0).max(0.1);
        let up = ctx.up.or(self.up);
        // The camera's look in the shot's frame (a pan's turn starts from it).
        let cam = ctx.camera.with_up(up);
        let subject = || {
            ctx.subject
                .clone()
                .ok_or_else(|| format!("preset {}: needs a subject", self.name))
        };
        // The heading from the subject toward the camera (face-local, the subject's face).
        let heading = |s: [f64; 3]| {
            let l = FaceFrame::at_height(s[1]).vector(sub(cam.position, s));
            if l[0].hypot(l[2]) < 1e-6 {
                (cam.yaw_deg + 180.0).to_radians()
            } else {
                l[0].atan2(l[2])
            }
        };
        let around = |s: [f64; 3], h: f64, r: f64, up: f64| {
            add(
                s,
                FaceFrame::at_height(s[1]).vector([r * h.sin(), up, r * h.cos()]),
            )
        };
        let easing = self.easing.name();
        let (k0, k1, seg) = match self.mv.as_str() {
            "orbit" => {
                let (t, s) = subject()?;
                let h = heading(s);
                let (r, up, deg) = (v("radiusM", 12.0), v("heightM", 4.0), v("degrees", 360.0));
                let a = around(s, h, r, up);
                let b = around(s, h + deg.to_radians(), r, up);
                (
                    json!({ "position": a, "target": t.to_json() }),
                    json!({ "position": b }),
                    json!({ "move": "orbit", "around": t.to_json(), "degrees": deg, "easing": easing }),
                )
            }
            "dolly" => {
                let (t, s) = subject()?;
                let h = heading(s);
                let up = v("heightM", 1.6);
                let a = around(s, h, v("fromM", 20.0), up);
                let b = around(s, h, v("toM", 6.0), up);
                (
                    json!({ "position": a, "target": t.to_json() }),
                    json!({ "position": b }),
                    json!({ "move": "dolly", "at": t.to_json(), "vertigo": v("vertigo", 0.0) >= 0.5, "easing": easing }),
                )
            }
            "track" => {
                let (t, s) = subject()?;
                let h = heading(s);
                let (d, side, up) = (v("distanceM", 6.0), v("sideM", 1.5), v("heightM", 2.0));
                // Behind the subject on the camera's side, `side` to the right of the line.
                let offset = [
                    d * h.sin() + side * h.cos(),
                    up,
                    d * h.cos() - side * h.sin(),
                ];
                let p = add(s, FaceFrame::at_height(s[1]).vector(offset));
                (
                    json!({ "position": p, "target": t.to_json() }),
                    json!({ "position": p }),
                    json!({ "move": "track", "target": t.to_json(), "offset": offset,
                            "smoothingS": v("smoothingS", 0.6), "easing": easing }),
                )
            }
            "crane" => {
                let (t, s) = subject()?;
                let h = heading(s);
                let back = v("backM", 10.0);
                let a = around(s, h, back, v("fromM", 2.0));
                let b = around(s, h, back, v("toM", 60.0));
                (
                    json!({ "position": a, "target": t.to_json() }),
                    json!({ "position": b }),
                    json!({ "move": "crane", "easing": easing, "upEasing": easing }),
                )
            }
            _ => {
                // pan: turn in place from the camera's look.
                let pitch = v("pitch", cam.pitch_deg);
                (
                    json!({ "position": cam.position, "look": [cam.yaw_deg, pitch] }),
                    json!({ "look": [cam.yaw_deg + v("degrees", 90.0), pitch] }),
                    json!({ "move": "pan", "easing": easing }),
                )
            }
        };
        let (mut k0, mut k1) = (k0, k1);
        k0["t"] = json!(0.0);
        if up.is_some() {
            k0["up"] = json!(crate::path::up_name(up));
        }
        k1["t"] = json!(seconds);
        if let Some(f) = self.value("fov", overrides) {
            k0["fov"] = json!(f);
        }
        if let Some(d) = self.value("timeFrom", overrides) {
            k0["timeOfDay"] = json!(d);
            k1["timeOfDay"] = json!(self.value("timeTo", overrides).unwrap_or(d));
        }
        let mut seg = seg;
        if let Some(d) = self.value("days", overrides) {
            seg["days"] = json!(d.round().max(0.0) as u64);
        }
        Path::parse(&json!({
            "formatVersion": crate::path::FORMAT_VERSION,
            "name": self.name,
            "keyframes": [k0, k1],
            "segments": [seg],
        }))
        .map_err(|e| format!("preset {}: {e}", self.name))
    }
}
