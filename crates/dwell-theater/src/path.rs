//! A camera path (THEATER_MODE.md §3): keyframes and the segments between them, read from JSON
//! with a message naming the bad entry, and written back to the same JSON.

use serde_json::{Map, Value, json};

use dwell_world::FaceFrame;

use crate::easing::Easing;

/// The path format this build reads and writes.
pub const FORMAT_VERSION: u64 = 1;

/// Something a camera can look at, orbit or follow.
#[derive(Clone, Debug, PartialEq)]
pub enum TargetRef {
    /// A fixed point (world metres).
    Point([f64; 3]),
    /// The viewer's own character.
    Player,
    /// A creature by its id.
    Creature(u64),
    /// An NPC by its id.
    Npc(u64),
}

impl TargetRef {
    /// Reads a target (`at` names it in the message).
    pub fn from_json(v: &Value, at: &str) -> Result<Self, String> {
        if let Some(p) = vec3(v) {
            return Ok(Self::Point(p));
        }
        if v.as_str() == Some("player") {
            return Ok(Self::Player);
        }
        if let Some(o) = v.as_object() {
            let id = |k: &str| o.get(k).and_then(Value::as_u64);
            if o.len() == 1 {
                if let Some(n) = id("creature") {
                    return Ok(Self::Creature(n));
                }
                if let Some(n) = id("npc") {
                    return Ok(Self::Npc(n));
                }
            }
        }
        Err(format!(
            "{at} must be a point [x, y, z], \"player\", {{\"creature\": id}} or {{\"npc\": id}}"
        ))
    }
    pub fn to_json(&self) -> Value {
        match self {
            Self::Point(p) => json!(p),
            Self::Player => json!("player"),
            Self::Creature(n) => json!({ "creature": n }),
            Self::Npc(n) => json!({ "npc": n }),
        }
    }
}

/// Where a keyframe looks.
#[derive(Clone, Debug, PartialEq)]
pub enum Aim {
    /// A look (degrees, face-local to the keyframe's position: yaw 0 = +Z, 90 = +X, pitch toward
    /// the face's up), as `placeCamera` takes it.
    Look { yaw_deg: f64, pitch_deg: f64 },
    /// At a target.
    At(TargetRef),
}

/// A pose the camera passes through at path time `t`.
#[derive(Clone, Debug, PartialEq)]
pub struct Keyframe {
    /// Path seconds (increasing).
    pub t: f64,
    /// World metres.
    pub position: [f64; 3],
    pub aim: Aim,
    /// Vertical field of view (degrees); `None`: the viewer's setting.
    pub fov_deg: Option<f64>,
    /// The time of day the viewer sees (0–1, 0 midnight); `None`: the world's.
    pub time_of_day: Option<f64>,
    /// Playback rate here (path seconds per real second; > 1 a time-lapse of the move).
    pub speed: f64,
    /// The face whose up the camera holds (`up`: `"a"`/`"b"`), wherever it is: its up, its roll
    /// and this keyframe's `look` are that face's frame, so from face B's side in face A's frame
    /// the world reads upside down. `None` (`"auto"`, the default): the face the camera is over.
    pub up: Option<FaceFrame>,
}

impl Keyframe {
    /// The frame this keyframe's `look` is in: its `up` face's, else its position's face.
    pub fn look_frame(&self) -> FaceFrame {
        self.up
            .unwrap_or_else(|| FaceFrame::at_height(self.position[1]))
    }
}

/// An `up` value's data name (`"a"`, `"b"`, `"auto"`).
pub fn up_name(up: Option<FaceFrame>) -> &'static str {
    match up {
        None => "auto",
        Some(f) if f.is_b() => "b",
        Some(_) => "a",
    }
}
/// Reads an `up` value (`"a"`, `"b"` or `"auto"`; `at` names it in the message).
pub fn parse_up(v: &Value, at: &str) -> Result<Option<FaceFrame>, String> {
    match v.as_str() {
        Some("a") => Ok(Some(FaceFrame::A)),
        Some("b") => Ok(Some(FaceFrame::B)),
        Some("auto") => Ok(None),
        _ => Err(format!("{at} must be \"a\", \"b\" or \"auto\"")),
    }
}

/// How the camera moves between two keyframes: a small fixed vocabulary (§3.2).
#[derive(Clone, Debug, PartialEq)]
pub enum Move {
    /// Hold the first keyframe, then jump to the next.
    Cut,
    /// Straight between the keyframes (the segment's easing, `linear` by default).
    Linear,
    /// Straight, `inOutCubic` by default.
    Ease,
    /// Around a target: angle, radius and height (face-local) eased from the first keyframe's to
    /// the next's, looking at it. Without `degrees` the shortest way round; with it (signed, may
    /// exceed 360) the way round nearest it, whole turns included.
    Orbit {
        around: TargetRef,
        degrees: Option<f64>,
    },
    /// Follow a target at an offset (face-local metres from it), smoothed over `smoothing_s`,
    /// looking at it. The keyframes' positions are only where it is meant to start and end.
    Track {
        target: TargetRef,
        offset: [f64; 3],
        smoothing_s: f64,
    },
    /// Straight, looking at `at` when set; `vertigo` holds the subject's size on screen by
    /// widening or narrowing the field of view as the camera moves (the dolly zoom).
    Dolly {
        at: Option<TargetRef>,
        vertigo: bool,
    },
    /// Crane / rise: across with the segment's easing, up (world y) with `up_easing`.
    Crane { up_easing: Easing },
    /// Turn in place: the camera stays at the first keyframe and the look turns.
    Pan,
}

impl Move {
    /// The data names, in the order of the vocabulary.
    pub const NAMES: [&'static str; 8] = [
        "cut", "linear", "ease", "orbit", "track", "dolly", "crane", "pan",
    ];
    pub fn name(&self) -> &'static str {
        match self {
            Move::Cut => "cut",
            Move::Linear => "linear",
            Move::Ease => "ease",
            Move::Orbit { .. } => "orbit",
            Move::Track { .. } => "track",
            Move::Dolly { .. } => "dolly",
            Move::Crane { .. } => "crane",
            Move::Pan => "pan",
        }
    }
    fn default_easing(&self) -> Easing {
        match self {
            Move::Ease | Move::Dolly { .. } | Move::Crane { .. } => Easing::InOutCubic,
            Move::Pan => Easing::InOutSine,
            _ => Easing::Linear,
        }
    }
}

/// The move from keyframe `i` to `i + 1`.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub mv: Move,
    pub easing: Easing,
    /// Whole days the time of day runs on through, besides the way to the next keyframe's.
    pub days: u32,
}

/// A camera path.
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub name: String,
    pub keyframes: Vec<Keyframe>,
    /// One per pair of keyframes.
    pub segments: Vec<Segment>,
    /// Start over at the end.
    pub looped: bool,
}

impl Path {
    /// The path's length in path seconds.
    pub fn duration(&self) -> f64 {
        self.keyframes.last().map_or(0.0, |k| k.t) - self.keyframes[0].t
    }

    /// Reads a path from JSON text.
    pub fn parse_str(text: &str) -> Result<Self, String> {
        let v: Value =
            serde_json::from_str(text).map_err(|e| format!("camera path: not JSON: {e}"))?;
        Self::parse(&v)
    }

    /// Reads a path, failing with a message that names the bad entry.
    pub fn parse(v: &Value) -> Result<Self, String> {
        let o = object(v, "camera path", &PATH_FIELDS)?;
        match o.get("formatVersion").and_then(Value::as_u64) {
            Some(FORMAT_VERSION) => {}
            _ => {
                return Err(format!(
                    "camera path: formatVersion must be {FORMAT_VERSION}"
                ));
            }
        }
        let name = match o.get("name") {
            None => String::new(),
            Some(n) => n
                .as_str()
                .ok_or("camera path: name must be a string")?
                .to_owned(),
        };
        let looped = match o.get("loop") {
            None => false,
            Some(b) => b
                .as_bool()
                .ok_or("camera path: loop must be true or false")?,
        };
        let kfs = o
            .get("keyframes")
            .and_then(Value::as_array)
            .ok_or("camera path: keyframes must be a list")?;
        if kfs.len() < 2 {
            return Err("camera path: keyframes needs at least two".into());
        }
        let mut keyframes: Vec<Keyframe> = Vec::with_capacity(kfs.len());
        for (i, k) in kfs.iter().enumerate() {
            let at = format!("keyframes[{i}]");
            keyframes.push(Keyframe::parse(k, &at, keyframes.last())?);
        }
        let segs = match o.get("segments") {
            None => Vec::new(),
            Some(s) => s
                .as_array()
                .ok_or("camera path: segments must be a list")?
                .clone(),
        };
        if segs.len() > keyframes.len() - 1 {
            return Err(format!(
                "camera path: {} segments for {} keyframes (at most one between each pair)",
                segs.len(),
                keyframes.len()
            ));
        }
        let mut segments = Vec::with_capacity(keyframes.len() - 1);
        for i in 0..keyframes.len() - 1 {
            let at = format!("segments[{i}]");
            let s = match segs.get(i) {
                Some(s) => Segment::parse(s, &at)?,
                // A missing segment eases.
                None => Segment {
                    mv: Move::Ease,
                    easing: Move::Ease.default_easing(),
                    days: 0,
                },
            };
            if matches!(s.mv, Move::Pan)
                && dist(keyframes[i].position, keyframes[i + 1].position) > 1e-6
            {
                return Err(format!(
                    "{at}: a pan turns in place, but keyframes[{}] moves the camera (leave its position out, or use linear)",
                    i + 1
                ));
            }
            if let Move::Dolly {
                vertigo: true,
                at: a,
            } = &s.mv
            {
                if keyframes[i].fov_deg.is_none() {
                    return Err(format!(
                        "{at}: a vertigo dolly needs keyframes[{i}].fov to start from"
                    ));
                }
                let subject = a.is_some() || matches!(keyframes[i].aim, Aim::At(_));
                if !subject {
                    return Err(format!(
                        "{at}: a vertigo dolly needs a subject (`at`, or keyframes[{i}].target)"
                    ));
                }
            }
            segments.push(s);
        }
        Ok(Self {
            name,
            keyframes,
            segments,
            looped,
        })
    }

    /// The path as JSON (what [`Path::parse`] reads back to the same path).
    pub fn to_json(&self) -> Value {
        let keyframes: Vec<Value> = self
            .keyframes
            .iter()
            .enumerate()
            .map(|(i, k)| k.to_json(i.checked_sub(1).map(|j| &self.keyframes[j])))
            .collect();
        let segments: Vec<Value> = self.segments.iter().map(Segment::to_json).collect();
        let mut o = Map::new();
        o.insert("formatVersion".into(), json!(FORMAT_VERSION));
        if !self.name.is_empty() {
            o.insert("name".into(), json!(self.name));
        }
        if self.looped {
            o.insert("loop".into(), json!(true));
        }
        o.insert("keyframes".into(), Value::Array(keyframes));
        o.insert("segments".into(), Value::Array(segments));
        Value::Object(o)
    }
}

const PATH_FIELDS: [&str; 5] = ["formatVersion", "name", "loop", "keyframes", "segments"];
const KEYFRAME_FIELDS: [&str; 8] = [
    "t",
    "position",
    "look",
    "target",
    "fov",
    "timeOfDay",
    "speed",
    "up",
];
const SEGMENT_FIELDS: [&str; 11] = [
    "upEasing",
    "move",
    "easing",
    "days",
    "around",
    "degrees",
    "target",
    "offset",
    "smoothingS",
    "at",
    "vertigo",
];

impl Keyframe {
    /// A keyframe; a field left out keeps `prev`'s (the first's position is required).
    fn parse(v: &Value, at: &str, prev: Option<&Keyframe>) -> Result<Self, String> {
        let o = object(v, at, &KEYFRAME_FIELDS)?;
        let t = o
            .get("t")
            .and_then(Value::as_f64)
            .filter(|t| t.is_finite())
            .ok_or_else(|| format!("{at}.t must be a number (path seconds)"))?;
        if let Some(p) = prev
            && t <= p.t
        {
            return Err(format!(
                "{at}.t must be after the keyframe before ({})",
                p.t
            ));
        }
        let position = match o.get("position") {
            Some(p) => vec3(p).ok_or_else(|| format!("{at}.position must be [x, y, z]"))?,
            None => prev
                .map(|p| p.position)
                .ok_or_else(|| format!("{at}.position is required on the first keyframe"))?,
        };
        let up = match o.get("up") {
            Some(u) => parse_up(u, &format!("{at}.up"))?,
            None => prev.and_then(|p| p.up),
        };
        let look_frame = up.unwrap_or_else(|| FaceFrame::at_height(position[1]));
        let aim = match (o.get("look"), o.get("target")) {
            (Some(_), Some(_)) => return Err(format!("{at}: look or target, not both")),
            (Some(l), None) => match l.as_array().map(Vec::as_slice) {
                Some([y, p]) if finite(y) && finite(p) => Aim::Look {
                    yaw_deg: y.as_f64().unwrap_or(0.0),
                    pitch_deg: p.as_f64().unwrap_or(0.0).clamp(-90.0, 90.0),
                },
                _ => return Err(format!("{at}.look must be [yaw, pitch] (degrees)")),
            },
            (None, Some(t)) => Aim::At(TargetRef::from_json(t, &format!("{at}.target"))?),
            // Kept from the keyframe before; a look carried into another frame (over the
            // midplane, or to another `up`) keeps its world direction (`FaceFrame::look_across`).
            // A held `up` keeps one frame across the midplane: nothing to carry.
            (None, None) => match prev.map(|p| (p.aim.clone(), p.look_frame())) {
                Some((Aim::Look { yaw_deg, pitch_deg }, before)) if before != look_frame => {
                    let (yaw_deg, pitch_deg) = FaceFrame::look_across(yaw_deg, pitch_deg);
                    Aim::Look { yaw_deg, pitch_deg }
                }
                Some((aim, _)) => aim,
                None => Aim::Look {
                    yaw_deg: 0.0,
                    pitch_deg: 0.0,
                },
            },
        };
        let fov_deg = match o.get("fov") {
            Some(f) => Some(
                f.as_f64()
                    .filter(|f| (5.0..=150.0).contains(f))
                    .ok_or_else(|| format!("{at}.fov must be a number in [5, 150] (degrees)"))?,
            ),
            None => prev.and_then(|p| p.fov_deg),
        };
        let time_of_day = match o.get("timeOfDay") {
            Some(d) => Some(
                d.as_f64()
                    .filter(|d| (0.0..=1.0).contains(d))
                    .ok_or_else(|| format!("{at}.timeOfDay must be a number in [0, 1]"))?
                    .rem_euclid(1.0),
            ),
            None => prev.and_then(|p| p.time_of_day),
        };
        let speed = match o.get("speed") {
            Some(s) => s
                .as_f64()
                .filter(|s| *s > 0.0 && *s <= 1000.0)
                .ok_or_else(|| format!("{at}.speed must be a number in (0, 1000]"))?,
            None => prev.map_or(1.0, |p| p.speed),
        };
        Ok(Self {
            t,
            position,
            aim,
            fov_deg,
            time_of_day,
            speed,
            up,
        })
    }
    /// The keyframe as JSON; `prev` (the keyframe before) decides whether an `"auto"` up has to be
    /// written (after a held one, which a left-out `up` would carry on).
    fn to_json(&self, prev: Option<&Keyframe>) -> Value {
        let mut o = Map::new();
        o.insert("t".into(), json!(self.t));
        o.insert("position".into(), json!(self.position));
        match &self.aim {
            Aim::Look { yaw_deg, pitch_deg } => {
                o.insert("look".into(), json!([yaw_deg, pitch_deg]))
            }
            Aim::At(t) => o.insert("target".into(), t.to_json()),
        };
        if let Some(f) = self.fov_deg {
            o.insert("fov".into(), json!(f));
        }
        if let Some(d) = self.time_of_day {
            o.insert("timeOfDay".into(), json!(d));
        }
        o.insert("speed".into(), json!(self.speed));
        if self.up.is_some() || prev.is_some_and(|p| p.up.is_some()) {
            o.insert("up".into(), json!(up_name(self.up)));
        }
        Value::Object(o)
    }
}

impl Segment {
    fn parse(v: &Value, at: &str) -> Result<Self, String> {
        let o = object(v, at, &SEGMENT_FIELDS)?;
        let kind = o
            .get("move")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{at}.move must be one of {}", Move::NAMES.join(", ")))?;
        let only = |allowed: &[&str]| -> Result<(), String> {
            for k in o.keys() {
                if !["move", "easing", "days"].contains(&k.as_str())
                    && !allowed.contains(&k.as_str())
                    && !k.starts_with('$')
                {
                    return Err(format!("{at}: a {kind} move has no field {k}"));
                }
            }
            Ok(())
        };
        let target = |k: &str| -> Result<TargetRef, String> {
            TargetRef::from_json(
                o.get(k)
                    .ok_or_else(|| format!("{at}.{k} is required for a {kind} move"))?,
                &format!("{at}.{k}"),
            )
        };
        let mv = match kind {
            "cut" => only(&[]).map(|_| Move::Cut)?,
            "linear" => only(&[]).map(|_| Move::Linear)?,
            "ease" => only(&[]).map(|_| Move::Ease)?,
            "pan" => only(&[]).map(|_| Move::Pan)?,
            "orbit" => {
                only(&["around", "degrees"])?;
                let degrees = match o.get("degrees") {
                    None => None,
                    Some(d) => Some(
                        d.as_f64()
                            .filter(|d| d.is_finite() && d.abs() <= 36_000.0)
                            .ok_or_else(|| format!("{at}.degrees must be a number (signed)"))?,
                    ),
                };
                Move::Orbit {
                    around: target("around")?,
                    degrees,
                }
            }
            "track" => {
                only(&["target", "offset", "smoothingS"])?;
                let offset = match o.get("offset") {
                    None => [0.0, 2.0, -6.0],
                    Some(v) => {
                        vec3(v).ok_or_else(|| format!("{at}.offset must be [x, y, z] (metres)"))?
                    }
                };
                let smoothing_s = match o.get("smoothingS") {
                    None => 0.5,
                    Some(v) => v
                        .as_f64()
                        .filter(|s| (0.0..=30.0).contains(s))
                        .ok_or_else(|| format!("{at}.smoothingS must be a number in [0, 30]"))?,
                };
                Move::Track {
                    target: target("target")?,
                    offset,
                    smoothing_s,
                }
            }
            "dolly" => {
                only(&["at", "vertigo"])?;
                let at_ = match o.get("at") {
                    None => None,
                    Some(_) => Some(target("at")?),
                };
                let vertigo = match o.get("vertigo") {
                    None => false,
                    Some(b) => b
                        .as_bool()
                        .ok_or_else(|| format!("{at}.vertigo must be true or false"))?,
                };
                Move::Dolly { at: at_, vertigo }
            }
            "crane" => {
                only(&["upEasing"])?;
                let up_easing = match o.get("upEasing") {
                    None => Easing::OutCubic,
                    Some(e) => e.as_str().and_then(Easing::by_name).ok_or_else(|| {
                        format!("{at}.upEasing must be one of {}", Easing::names())
                    })?,
                };
                Move::Crane { up_easing }
            }
            other => {
                return Err(format!(
                    "{at}.move: unknown move {other} ({})",
                    Move::NAMES.join(", ")
                ));
            }
        };
        let easing = match o.get("easing") {
            None => mv.default_easing(),
            Some(e) => e
                .as_str()
                .and_then(Easing::by_name)
                .ok_or_else(|| format!("{at}.easing must be one of {}", Easing::names()))?,
        };
        let days = match o.get("days") {
            None => 0,
            Some(d) => d
                .as_u64()
                .filter(|d| *d <= 365)
                .ok_or_else(|| format!("{at}.days must be a whole number in [0, 365]"))?
                as u32,
        };
        Ok(Self { mv, easing, days })
    }
    fn to_json(&self) -> Value {
        let mut o = Map::new();
        o.insert("move".into(), json!(self.mv.name()));
        o.insert("easing".into(), json!(self.easing.name()));
        if self.days > 0 {
            o.insert("days".into(), json!(self.days));
        }
        match &self.mv {
            Move::Orbit { around, degrees } => {
                o.insert("around".into(), around.to_json());
                if let Some(d) = degrees {
                    o.insert("degrees".into(), json!(d));
                }
            }
            Move::Track {
                target,
                offset,
                smoothing_s,
            } => {
                o.insert("target".into(), target.to_json());
                o.insert("offset".into(), json!(offset));
                o.insert("smoothingS".into(), json!(smoothing_s));
            }
            Move::Dolly { at, vertigo } => {
                if let Some(a) = at {
                    o.insert("at".into(), a.to_json());
                }
                if *vertigo {
                    o.insert("vertigo".into(), json!(true));
                }
            }
            Move::Crane { up_easing } => {
                o.insert("upEasing".into(), json!(up_easing.name()));
            }
            _ => {}
        }
        Value::Object(o)
    }
}

fn object<'a>(v: &'a Value, at: &str, fields: &[&str]) -> Result<&'a Map<String, Value>, String> {
    let o = v
        .as_object()
        .ok_or_else(|| format!("{at} must be an object"))?;
    for k in o.keys() {
        if !fields.contains(&k.as_str()) && !k.starts_with('$') {
            return Err(format!("{at}: unknown field {k} ({})", fields.join(", ")));
        }
    }
    Ok(o)
}
fn finite(v: &Value) -> bool {
    v.as_f64().is_some_and(f64::is_finite)
}
pub(crate) fn vec3(v: &Value) -> Option<[f64; 3]> {
    match v.as_array().map(Vec::as_slice) {
        Some([x, y, z]) if finite(x) && finite(y) && finite(z) => {
            Some([x.as_f64()?, y.as_f64()?, z.as_f64()?])
        }
        _ => None,
    }
}
pub(crate) fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}
