//! Scenes: test worlds built from data (docs/THEATER_MODE.md §6.3; owner, 2026-11-14: "We'll
//! probably do a bunch of these so if you need tools to generate a valley with a river/lake with a
//! dam, go ahead and make them"). A scene file (`shared/theater/scenes/*.json`) describes the
//! ground (a valley: length, width, depth, side slope, the floor's fall, materials), the water (a
//! lake held behind a structure, filled to a level) and the structures (a wall across the valley:
//! where, how high in blocks, how thick, of what), and timed events (at `atS` remove a structure
//! by name). [`Scene::parse`] validates it (a bad entry fails with its path and the reason);
//! [`Scene::cells`] gives the cells the world must change to hold it (only those that differ), and
//! `Server::build_scene` writes them as one build, the water joining **held** at its level (as a
//! generated lake does: still, at rest, from its first tick); `Server::scene_event` runs an event.
//!
//! The engine is the small fixed vocabulary here (the `valley` terrain, the `lake` water, the
//! `wall` structure, the `remove` event); a scene is data. Everything is face-local (+y up) and
//! crosses into the world through the scene's [`FaceFrame`].

use dwell_blocks::StateId;
use dwell_world::ramps::{RampFacing, ramp_state_flooded};
use dwell_world::{CELL_H, FaceFrame};
use serde_json::{Map, Value};

/// The scene format's version.
pub const FORMAT_VERSION: u64 = 1;

/// Which way the valley runs from its head (its `origin`), in the world's x and z.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    PosX,
    NegX,
    PosZ,
    NegZ,
}

impl Axis {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "+x" => Self::PosX,
            "-x" => Self::NegX,
            "+z" => Self::PosZ,
            "-z" => Self::NegZ,
            _ => return None,
        })
    }

    /// A column centre's (u along the valley, v across it) from its world x, z (m) and the origin.
    fn uv(self, [ox, oz]: [f64; 2], x: f64, z: f64) -> (f64, f64) {
        match self {
            Self::PosZ => (z - oz, x - ox),
            Self::NegZ => (oz - z, ox - x),
            Self::PosX => (x - ox, oz - z),
            Self::NegX => (ox - x, z - oz),
        }
    }

    /// The world x, z of (u, v): the inverse of [`Axis::uv`].
    fn xz(self, [ox, oz]: [f64; 2], u: f64, v: f64) -> (f64, f64) {
        match self {
            Self::PosZ => (ox + v, oz + u),
            Self::NegZ => (ox - v, oz - u),
            Self::PosX => (ox + u, oz - v),
            Self::NegX => (ox - u, oz + v),
        }
    }
}

/// How fast the water runs in a scene's world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaterClock {
    /// The world's own (game time × its water speed: a 20-minute day runs the water 72× real time).
    Game,
    /// Real time: the world's water speed set so a second of water is a second of play (a flood
    /// filmed as it would happen).
    Real,
}

/// The ground: a valley carved into level land, its floor falling downstream, its ends and
/// sides sloping up to the land.
#[derive(Clone, Debug, PartialEq)]
pub struct Valley {
    /// The land's top (face-local m).
    pub land_m: f64,
    pub length_m: f64,
    pub floor_width_m: f64,
    /// The floor's depth below the land at the head (m).
    pub depth_m: f64,
    /// The floor's fall downstream (m per m).
    pub floor_fall: f64,
    /// The sides' and ends' rise over run.
    pub side_slope: f64,
    /// Air cleared over the land (m): whatever stood there goes.
    pub clear_m: f64,
    pub surface: String,
    pub under: String,
    pub under_depth_m: f64,
    pub rock: String,
    /// The floor's top cell (None: the surface's).
    pub bed: Option<String>,
}

/// A wall across the valley (a dam): its upstream face `at_m` along the valley, `height_blocks`
/// blocks of `block_m` over the floor there, `thickness_m` downstream.
#[derive(Clone, Debug, PartialEq)]
pub struct Wall {
    pub name: String,
    pub at_m: f64,
    pub height_blocks: u32,
    pub block_m: f64,
    pub thickness_m: f64,
    pub material: String,
}

/// A lake: every open cell upstream of a structure filled up to a level.
#[derive(Clone, Debug, PartialEq)]
pub struct Lake {
    pub name: String,
    /// The structure it is held behind (a [`Wall`]'s name).
    pub behind: String,
    /// Its level: face-local m, or how far below the structure's top.
    pub level: Level,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Level {
    At(f64),
    BelowTop(f64),
}

/// What an event does.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// A structure deleted all at once (its cells to air).
    Remove(String),
    /// Explosives set into a structure and lit at once (owner, 2026-11-24: a dam "blown up with
    /// yellow dynamite"; a measurement's and a recording's).
    Blast(Blast),
}

/// A blast's charges: `explosive` (a name in `shared/explosives/explosives.json`) at each charge,
/// lit with a fuse of `fuse_s`. A charge is (across the valley m, over the floor m at the
/// structure, along the valley m from the structure's upstream face), of structure `target`.
#[derive(Clone, Debug, PartialEq)]
pub struct Blast {
    pub target: String,
    pub explosive: String,
    pub fuse_s: f64,
    pub charges: Vec<[f64; 3]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// The take's time (s) it fires at (the cinematic clock).
    pub at_s: f64,
    pub action: Action,
}

/// A parsed, validated scene.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub name: String,
    /// The world it is built on (`flat`: the flat world, `?world=flat`).
    pub world: String,
    pub frame: FaceFrame,
    /// The valley's head, on its axis (world x, z, m).
    pub origin: [f64; 2],
    pub axis: Axis,
    pub water_clock: WaterClock,
    pub valley: Valley,
    pub walls: Vec<Wall>,
    pub lakes: Vec<Lake>,
    pub events: Vec<Event>,
}

/// The validation's reader: every problem found, each with its path.
struct Reader {
    errs: Vec<String>,
}

impl Reader {
    fn obj<'a>(
        &mut self,
        v: Option<&'a Value>,
        at: &str,
        known: &[&str],
    ) -> Option<&'a Map<String, Value>> {
        let Some(Value::Object(m)) = v else {
            self.errs.push(format!("{at}: expected an object"));
            return None;
        };
        for k in m.keys() {
            if !k.starts_with('$') && !known.contains(&k.as_str()) {
                self.errs.push(format!(
                    "{at}: unknown key \"{k}\" (known: {})",
                    known.join(", ")
                ));
            }
        }
        Some(m)
    }

    fn num(&mut self, m: &Map<String, Value>, k: &str, at: &str, lo: f64, hi: f64) -> f64 {
        match m.get(k).and_then(Value::as_f64) {
            Some(v) if (lo..=hi).contains(&v) => v,
            Some(v) => {
                self.errs
                    .push(format!("{at}.{k}: {v} is out of range ({lo} to {hi})"));
                lo
            }
            None => {
                self.errs
                    .push(format!("{at}.{k}: expected a number ({lo} to {hi})"));
                lo
            }
        }
    }

    fn num_or(
        &mut self,
        m: &Map<String, Value>,
        k: &str,
        at: &str,
        lo: f64,
        hi: f64,
        d: f64,
    ) -> f64 {
        if m.contains_key(k) {
            self.num(m, k, at, lo, hi)
        } else {
            d
        }
    }

    fn text(&mut self, m: &Map<String, Value>, k: &str, at: &str) -> String {
        match m.get(k).and_then(Value::as_str) {
            Some(s) if !s.is_empty() => s.to_owned(),
            _ => {
                self.errs.push(format!("{at}.{k}: expected a string"));
                String::new()
            }
        }
    }

    /// A solid block's name (`dwell:stone`), looked up in the registry.
    fn block(&mut self, m: &Map<String, Value>, k: &str, at: &str) -> String {
        let name = self.text(m, k, at);
        if !name.is_empty() {
            match dwell_blocks::state(&name) {
                Some(s) if dwell_blocks::info(s).solid => {}
                Some(_) => self
                    .errs
                    .push(format!("{at}.{k}: \"{name}\" is not a solid block")),
                None => self.errs.push(format!("{at}.{k}: no block \"{name}\"")),
            }
        }
        name
    }

    fn list<'a>(&mut self, m: &'a Map<String, Value>, k: &str, at: &str) -> &'a [Value] {
        match m.get(k) {
            None => &[],
            Some(Value::Array(a)) => a,
            Some(_) => {
                self.errs.push(format!("{at}.{k}: expected a list"));
                &[]
            }
        }
    }
}

impl Scene {
    /// A scene from its file's text, validated: an error lists every problem, each with its path.
    pub fn parse(text: &str) -> Result<Self, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("scene: not JSON: {e}"))?;
        let mut r = Reader { errs: Vec::new() };
        let top = [
            "formatVersion",
            "name",
            "description",
            "world",
            "face",
            "origin",
            "axis",
            "waterClock",
            "terrain",
            "water",
            "structures",
            "events",
        ];
        let Some(m) = r.obj(Some(&v), "scene", &top) else {
            return Err(r.errs.join("\n"));
        };
        if m.get("formatVersion").and_then(Value::as_u64) != Some(FORMAT_VERSION) {
            r.errs
                .push(format!("scene.formatVersion: expected {FORMAT_VERSION}"));
        }
        let name = r.text(m, "name", "scene");
        if !name.is_empty()
            && !name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        {
            r.errs
                .push(format!("scene.name: \"{name}\" is not a lowercase id"));
        }
        let world = r.text(m, "world", "scene");
        if !world.is_empty() && world != "flat" {
            r.errs.push(format!(
                "scene.world: \"{world}\" is not a world a scene builds on (flat)"
            ));
        }
        let frame = match m.get("face").map(Value::as_str) {
            None => FaceFrame::A,
            Some(Some(s)) => FaceFrame::parse(s).unwrap_or_else(|| {
                r.errs
                    .push(format!("scene.face: \"{s}\" is not a face (a or b)"));
                FaceFrame::A
            }),
            Some(None) => {
                r.errs.push("scene.face: expected \"a\" or \"b\"".into());
                FaceFrame::A
            }
        };
        let origin = match m.get("origin").and_then(Value::as_array) {
            Some(a) if a.len() == 2 && a.iter().all(|x| x.as_f64().is_some_and(f64::is_finite)) => {
                [a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0)]
            }
            _ => {
                r.errs
                    .push("scene.origin: expected [x, z] (world m, the valley's head)".into());
                [0.0; 2]
            }
        };
        let axis_name = r.text(m, "axis", "scene");
        let axis = Axis::parse(&axis_name).unwrap_or_else(|| {
            if !axis_name.is_empty() {
                r.errs.push(format!(
                    "scene.axis: \"{axis_name}\" is not one of +x, -x, +z, -z"
                ));
            }
            Axis::PosZ
        });
        let water_clock = match m.get("waterClock").map(Value::as_str) {
            None | Some(Some("game")) => WaterClock::Game,
            Some(Some("real")) => WaterClock::Real,
            Some(other) => {
                r.errs.push(format!(
                    "scene.waterClock: {other:?} is not \"game\" or \"real\""
                ));
                WaterClock::Game
            }
        };
        let valley = Self::parse_valley(&mut r, m.get("terrain"));
        let mut walls: Vec<Wall> = Vec::new();
        for (i, s) in r.list(m, "structures", "scene").iter().enumerate() {
            let at = format!("scene.structures[{i}]");
            let known = [
                "name",
                "kind",
                "atM",
                "heightBlocks",
                "blockM",
                "thicknessM",
                "material",
            ];
            let Some(s) = r.obj(Some(s), &at, &known) else {
                continue;
            };
            let wname = r.text(s, "name", &at);
            if walls.iter().any(|w| w.name == wname) {
                r.errs.push(format!("{at}.name: \"{wname}\" is used twice"));
            }
            if s.get("kind").and_then(Value::as_str) != Some("wall") {
                r.errs.push(format!(
                    "{at}.kind: expected \"wall\" (the structures a scene builds)"
                ));
            }
            let at_m = r.num(s, "atM", &at, 0.0, valley.length_m);
            let height = r.num(s, "heightBlocks", &at, 1.0, 256.0);
            if height.fract() != 0.0 {
                r.errs.push(format!(
                    "{at}.heightBlocks: expected a whole number of blocks"
                ));
            }
            let block_m = r.num_or(s, "blockM", &at, 0.5, 1.0, 1.0);
            if block_m != 0.5 && block_m != 1.0 {
                r.errs.push(format!(
                    "{at}.blockM: expected 1 (1 m blocks) or 0.5 (½ m cells)"
                ));
            }
            let thickness_m = r.num(s, "thicknessM", &at, 1.0, 64.0);
            let material = r.block(s, "material", &at);
            let w = Wall {
                name: wname,
                at_m,
                height_blocks: height as u32,
                block_m,
                thickness_m,
                material,
            };
            let depth = valley.land_m - valley.floor_m(w.at_m);
            if w.height_m() > depth {
                r.errs.push(format!(
                    "{at}.heightBlocks: {} m stands over the land ({depth} m of valley there)",
                    w.height_m()
                ));
            }
            walls.push(w);
        }
        let mut lakes: Vec<Lake> = Vec::new();
        for (i, l) in r.list(m, "water", "scene").iter().enumerate() {
            let at = format!("scene.water[{i}]");
            let Some(l) = r.obj(
                Some(l),
                &at,
                &["name", "kind", "behind", "levelM", "belowTopM"],
            ) else {
                continue;
            };
            let lname = r.text(l, "name", &at);
            if l.get("kind").and_then(Value::as_str) != Some("lake") {
                r.errs.push(format!(
                    "{at}.kind: expected \"lake\" (the water a scene fills)"
                ));
            }
            let behind = r.text(l, "behind", &at);
            let wall = walls.iter().find(|w| w.name == behind);
            if wall.is_none() && !behind.is_empty() {
                r.errs
                    .push(format!("{at}.behind: no structure \"{behind}\""));
            }
            let level = match (l.contains_key("levelM"), l.contains_key("belowTopM")) {
                (true, false) => Level::At(r.num(l, "levelM", &at, -4096.0, 4096.0)),
                (false, true) => Level::BelowTop(r.num(l, "belowTopM", &at, 0.0, 256.0)),
                _ => {
                    r.errs
                        .push(format!("{at}: expected one of levelM or belowTopM"));
                    Level::BelowTop(0.0)
                }
            };
            let lake = Lake {
                name: lname,
                behind,
                level,
            };
            if let Some(w) = wall {
                let lv = lake.level_m(&valley, w);
                let floor = valley.floor_m(w.at_m);
                if lv > w.top_m(&valley) || lv > valley.land_m {
                    r.errs.push(format!(
                        "{at}: its level {lv} m is over {} (it would spill)",
                        if lv > valley.land_m {
                            "the land"
                        } else {
                            "its structure's top"
                        }
                    ));
                } else if lv <= floor {
                    r.errs.push(format!(
                        "{at}: its level {lv} m is under the floor at its structure ({floor} m)"
                    ));
                }
            }
            lakes.push(lake);
        }
        let mut events = Vec::new();
        for (i, e) in r.list(m, "events", "scene").iter().enumerate() {
            let at = format!("scene.events[{i}]");
            let Some(e) = r.obj(
                Some(e),
                &at,
                &["atS", "do", "target", "explosive", "fuseS", "charges"],
            ) else {
                continue;
            };
            let at_s = r.num(e, "atS", &at, 0.0, 86_400.0);
            let target = r.text(e, "target", &at);
            match e.get("do").and_then(Value::as_str) {
                Some("remove") => {
                    if !target.is_empty() && !walls.iter().any(|w| w.name == target) {
                        r.errs
                            .push(format!("{at}.target: no structure \"{target}\""));
                    }
                    events.push(Event {
                        at_s,
                        action: Action::Remove(target),
                    });
                }
                Some("blast") => {
                    if !walls.iter().any(|w| w.name == target) {
                        r.errs
                            .push(format!("{at}.target: no structure \"{target}\""));
                    }
                    let explosive = r.text(e, "explosive", &at);
                    if !dwell_world::explosives::EXPLOSIVES
                        .iter()
                        .any(|x| x.name == explosive)
                    {
                        r.errs.push(format!(
                            "{at}.explosive: {explosive:?} is no explosive (shared/explosives/explosives.json)"
                        ));
                    }
                    let fuse_s = r.num_or(e, "fuseS", &at, 0.0, 60.0, 0.0);
                    let mut charges = Vec::new();
                    for (k, c) in r.list(e, "charges", &at).iter().enumerate() {
                        let xyz: Option<Vec<f64>> = c
                            .as_array()
                            .filter(|a| a.len() == 3)
                            .and_then(|a| a.iter().map(Value::as_f64).collect());
                        match xyz.filter(|v| v.iter().all(|n| n.abs() <= 1000.0)) {
                            Some(v) => charges.push([v[0], v[1], v[2]]),
                            None => r.errs.push(format!(
                                "{at}.charges[{k}]: [across m, over the floor m, along m], each within 1000"
                            )),
                        }
                    }
                    if charges.is_empty() {
                        r.errs.push(format!("{at}.charges: at least one"));
                    }
                    events.push(Event {
                        at_s,
                        action: Action::Blast(Blast {
                            target,
                            explosive,
                            fuse_s,
                            charges,
                        }),
                    });
                }
                other => r.errs.push(format!(
                    "{at}.do: {other:?} is not an event a scene runs (remove, blast)"
                )),
            }
        }
        if !r.errs.is_empty() {
            return Err(format!("scene {name:?}:\n  {}", r.errs.join("\n  ")));
        }
        Ok(Self {
            name,
            world,
            frame,
            origin,
            axis,
            water_clock,
            valley,
            walls,
            lakes,
            events,
        })
    }

    fn parse_valley(r: &mut Reader, v: Option<&Value>) -> Valley {
        let known = [
            "kind",
            "landM",
            "lengthM",
            "floorWidthM",
            "depthM",
            "floorFall",
            "sideSlope",
            "clearM",
            "surface",
            "under",
            "underDepthM",
            "rock",
            "bed",
        ];
        let at = "scene.terrain";
        let Some(t) = r.obj(v, at, &known) else {
            return Valley::default_for_errors();
        };
        if t.get("kind").and_then(Value::as_str) != Some("valley") {
            r.errs.push(format!(
                "{at}.kind: expected \"valley\" (the terrain a scene builds)"
            ));
        }
        let valley = Valley {
            land_m: r.num(t, "landM", at, 2.0, 1000.0),
            length_m: r.num(t, "lengthM", at, 8.0, 2048.0),
            floor_width_m: r.num(t, "floorWidthM", at, 1.0, 512.0),
            depth_m: r.num(t, "depthM", at, 1.0, 256.0),
            floor_fall: r.num_or(t, "floorFall", at, 0.0, 0.5, 0.0),
            side_slope: r.num(t, "sideSlope", at, 0.05, 4.0),
            clear_m: r.num_or(t, "clearM", at, 0.0, 64.0, 2.0),
            surface: r.block(t, "surface", at),
            under: r.block(t, "under", at),
            under_depth_m: r.num_or(t, "underDepthM", at, 0.0, 64.0, 1.5),
            rock: r.block(t, "rock", at),
            bed: t.contains_key("bed").then(|| r.block(t, "bed", at)),
        };
        if valley.land_m - valley.floor_m(valley.length_m) < 1.0 {
            r.errs.push(format!(
                "{at}: the floor falls out of the ground by its end"
            ));
        }
        valley
    }
}

impl Valley {
    fn default_for_errors() -> Self {
        Self {
            land_m: 32.0,
            length_m: 8.0,
            floor_width_m: 1.0,
            depth_m: 1.0,
            floor_fall: 0.0,
            side_slope: 1.0,
            clear_m: 0.0,
            surface: String::new(),
            under: String::new(),
            under_depth_m: 0.0,
            rock: String::new(),
            bed: None,
        }
    }

    /// The floor's height (face-local m) `u` m along the valley (held at its ends beyond them).
    pub fn floor_m(&self, u: f64) -> f64 {
        self.land_m - self.depth_m - self.floor_fall * u.clamp(0.0, self.length_m)
    }

    /// The ground's height at (u, v), and whether it is the floor (no slope).
    fn ground_m(&self, u: f64, v: f64) -> (f64, bool) {
        let du = (-u).max(u - self.length_m).max(0.0);
        let dv = (v.abs() - self.floor_width_m / 2.0).max(0.0);
        let off = du.hypot(dv);
        let s = (self.floor_m(u) + self.side_slope * off).min(self.land_m);
        (s, off == 0.0)
    }

    /// How far the valley reaches past its floor's edge (m).
    fn reach_m(&self) -> f64 {
        (self.land_m - self.floor_m(self.length_m)) / self.side_slope + 2.0
    }
}

impl Wall {
    pub fn height_m(&self) -> f64 {
        f64::from(self.height_blocks) * self.block_m
    }

    /// Its top (face-local m): its height over the floor at its upstream face, on a row.
    pub fn top_m(&self, valley: &Valley) -> f64 {
        rows(valley.floor_m(self.at_m)) as f64 * CELL_H + self.height_m()
    }
}

impl Lake {
    pub fn level_m(&self, valley: &Valley, wall: &Wall) -> f64 {
        match self.level {
            Level::At(m) => m,
            Level::BelowTop(d) => wall.top_m(valley) - d,
        }
    }
}

/// A height (m) as the rows below it, to the nearest row.
fn rows(m: f64) -> i32 {
    (m / CELL_H).round() as i32
}

/// One face-local column's plan: its ground's top (rows), floor, steepness and what stands in it.
struct Column {
    /// Rows of ground (its top cell is row `t − 1`).
    t: i32,
    floor: bool,
    /// The ramp its top cell takes, and whether steep (a 1 m ramp: two pieces).
    ramp: Option<(RampFacing, bool)>,
    /// A wall's rows `[t, top)`, its material and its index.
    wall: Option<(i32, StateId, usize)>,
    /// A lake's water up to this row (exclusive).
    lake: Option<i32>,
}

/// What a build changes: the cells (world, with their new states) and its parts' cells.
#[derive(Debug, Default)]
pub struct Plan {
    pub cells: Vec<([i32; 3], StateId)>,
    /// Each structure's cells (world), by name: what its `remove` event takes.
    pub structures: Vec<(String, Vec<[i32; 3]>)>,
    /// Cells of water the build sets.
    pub water_cells: usize,
}

impl Scene {
    /// The world columns (x, z) the scene covers: its valley and a margin round it.
    pub fn columns(&self) -> ([i32; 2], [i32; 2]) {
        let v = &self.valley;
        let reach = v.reach_m();
        let half = v.floor_width_m / 2.0 + reach;
        let corners = [
            (-reach, -half),
            (-reach, half),
            (v.length_m + reach, -half),
            (v.length_m + reach, half),
        ];
        let (mut lo, mut hi) = ([i32::MAX; 2], [i32::MIN; 2]);
        for (u, w) in corners {
            let (x, z) = self.axis.xz(self.origin, u, w);
            for (i, c) in [x, z].into_iter().enumerate() {
                lo[i] = lo[i].min(c.floor() as i32);
                hi[i] = hi[i].max(c.ceil() as i32);
            }
        }
        (lo, hi)
    }

    /// The ground's rows at world column (x, z), and whether it is the floor.
    fn ground_rows(&self, x: i32, z: i32) -> (i32, bool, f64) {
        let (u, v) = self
            .axis
            .uv(self.origin, f64::from(x) + 0.5, f64::from(z) + 0.5);
        let (s, floor) = self.valley.ground_m(u, v);
        (rows(s), floor, u)
    }

    /// The ramp a top cell of `t` rows takes among its neighbours' (HALF_HEIGHT_GRID.md §4: a
    /// ramp descends toward its one lower edge neighbour, steep where that is a metre lower).
    fn ramp_of(&self, x: i32, z: i32, t: i32) -> Option<(RampFacing, bool)> {
        let h = |dx: i32, dz: i32| self.ground_rows(x + dx, z + dz).0;
        for d in RampFacing::ALL {
            let (dx, dz) = d.step();
            let (px, pz) = (dz, dx); // perpendicular
            let toward = h(dx, dz);
            let opp = h(-dx, -dz);
            if toward >= t || opp < t {
                continue;
            }
            let (s1, s2) = (h(px, pz), h(-px, -pz));
            let slope =
                s1 >= t && s2 >= t && h(-dx + px, -dz + pz) >= t && h(-dx - px, -dz - pz) >= t;
            let one_wide = opp > t && s1 < t && s2 < t;
            if slope || one_wide {
                return Some((d, toward <= t - 2));
            }
        }
        None
    }

    fn column(&self, x: i32, z: i32) -> Column {
        let (t, floor, u) = self.ground_rows(x, z);
        let land = rows(self.valley.land_m);
        let mut c = Column {
            t,
            floor,
            ramp: (t < land).then(|| self.ramp_of(x, z, t)).flatten(),
            wall: None,
            lake: None,
        };
        if t >= land {
            return c;
        }
        for (i, w) in self.walls.iter().enumerate() {
            if u >= w.at_m && u < w.at_m + w.thickness_m {
                let top = rows(w.top_m(&self.valley));
                if top > t {
                    c.wall = Some((top, dwell_blocks::require(&w.material), i));
                }
            }
        }
        for l in &self.lakes {
            let Some(w) = self.walls.iter().find(|w| w.name == l.behind) else {
                continue;
            };
            if u < w.at_m && c.wall.is_none() {
                let level = rows(l.level_m(&self.valley, w));
                if level > t {
                    c.lake = Some(level.max(c.lake.unwrap_or(0)));
                }
            }
        }
        c
    }

    /// The face-local state of local row `r` in column `c`.
    fn state_at(&self, c: &Column, r: i32) -> StateId {
        let v = &self.valley;
        if let Some((top, m, _)) = c.wall
            && r >= c.t
            && r < top
        {
            return m;
        }
        if let Some(level) = c.lake
            && r >= c.t
            && r < level
        {
            return dwell_blocks::require("dwell:water");
        }
        if r >= c.t {
            return dwell_blocks::AIR;
        }
        let top = match (&v.bed, c.floor) {
            (Some(b), true) => b.as_str(),
            _ => v.surface.as_str(),
        };
        let flooded = c.lake.is_some_and(|l| r < l);
        if let Some((facing, steep)) = c.ramp {
            let piece = match (steep, c.t - 1 - r) {
                (false, 0) => Some("ramp"),
                (true, 0) => Some("steep_upper"),
                (true, 1) => Some("steep_lower"),
                _ => None,
            };
            if let Some(p) = piece
                && let Some(s) = ramp_state_flooded(top, p, facing, false, flooded)
            {
                return s;
            }
        }
        let under_rows = rows(v.under_depth_m);
        let depth = c.t - 1 - r;
        let name = if depth == 0 || (depth == 1 && matches!(c.ramp, Some((_, true)))) {
            top
        } else if depth <= under_rows {
            v.under.as_str()
        } else {
            v.rock.as_str()
        };
        dwell_blocks::require(name)
    }

    /// The cells the world must change to hold the scene: every cell of its columns from a little
    /// under the ground to `clearM` over the land whose state `read` (world cell → state) differs
    /// from the scene's, in world cells and states (through the scene's face), with each
    /// structure's cells.
    /// A blast's charges' world cells (face `frame`'s): each across, over the floor at its
    /// structure and along the valley from the structure's upstream face.
    pub fn charge_cells(&self, b: &Blast) -> Vec<[i32; 3]> {
        let Some(w) = self.walls.iter().find(|w| w.name == b.target) else {
            return Vec::new();
        };
        let floor = rows(self.valley.floor_m(w.at_m));
        b.charges
            .iter()
            .map(|&[v, over, along]| {
                let (x, z) = self.axis.xz(self.origin, w.at_m + along, v);
                self.frame
                    .cell([x.floor() as i32, floor + rows(over), z.floor() as i32])
            })
            .collect()
    }

    pub fn cells(&self, mut read: impl FnMut([i32; 3]) -> StateId) -> Plan {
        let f = self.frame;
        let v = &self.valley;
        let ([x0, z0], [x1, z1]) = self.columns();
        let land = rows(v.land_m);
        let hi_rows = land + rows(v.clear_m);
        let mut plan = Plan {
            structures: self
                .walls
                .iter()
                .map(|w| (w.name.clone(), Vec::new()))
                .collect(),
            ..Plan::default()
        };
        let water = dwell_blocks::require("dwell:water");
        for z in z0..z1 {
            for x in x0..x1 {
                let c = self.column(x, z);
                let lo = c.t - 2 - rows(v.under_depth_m);
                let mut hi = hi_rows;
                if let Some((top, _, _)) = c.wall {
                    hi = hi.max(top);
                }
                for r in lo..hi {
                    let want = self.state_at(&c, r);
                    let cell = f.cell([x, r, z]);
                    let want = f.state(want);
                    if let Some((top, _, i)) = c.wall
                        && r >= c.t
                        && r < top
                    {
                        plan.structures[i].1.push(cell);
                    }
                    if want == water {
                        plan.water_cells += 1;
                    }
                    if read(cell) != want {
                        plan.cells.push((cell, want));
                    }
                }
            }
        }
        plan
    }

    /// How much faster than game time the water runs in this scene's world, given the world's day
    /// length (s): None keeps the world's own.
    pub fn water_speed(&self, day_length_s: f64) -> Option<f64> {
        match self.water_clock {
            WaterClock::Game => None,
            WaterClock::Real => Some(day_length_s / 86_400.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small valley with a dam and its lake, as data.
    pub(crate) fn fixture(face: &str) -> String {
        format!(
            r#"{{"formatVersion": 1, "name": "test", "world": "flat", "face": "{face}",
            "origin": [0, 0], "axis": "+z", "waterClock": "real",
            "terrain": {{"kind": "valley", "landM": 32, "lengthM": 64, "floorWidthM": 6,
              "depthM": 6, "floorFall": 0.0, "sideSlope": 0.5, "surface": "dwell:grass",
              "under": "dwell:dirt", "rock": "dwell:stone", "bed": "dwell:sand"}},
            "structures": [{{"name": "dam", "kind": "wall", "atM": 20, "heightBlocks": 4,
              "thicknessM": 2, "material": "dwell:stone"}}],
            "water": [{{"name": "lake", "kind": "lake", "behind": "dam", "belowTopM": 1}}],
            "events": [{{"atS": 3, "do": "remove", "target": "dam"}}]}}"#
        )
    }

    fn with(edit: impl FnOnce(&mut Value)) -> String {
        let mut v: Value = serde_json::from_str(&fixture("a")).unwrap();
        edit(&mut v);
        v.to_string()
    }

    #[test]
    fn the_fixture_parses() {
        let s = Scene::parse(&fixture("a")).expect("valid");
        assert_eq!(s.walls[0].height_m(), 4.0);
        assert_eq!(s.walls[0].top_m(&s.valley), 26.0 + 4.0);
        assert_eq!(s.lakes[0].level_m(&s.valley, &s.walls[0]), 29.0);
        assert_eq!(s.events[0].action, Action::Remove("dam".into()));
    }

    #[test]
    fn bad_entries_fail_with_their_path_and_reason() {
        let cases: Vec<(String, &str)> = vec![
            (
                with(|v| v["terrain"]["wobble"] = 1.into()),
                "scene.terrain: unknown key \"wobble\"",
            ),
            (
                with(|v| v["terrain"]["surface"] = "dwell:cheese".into()),
                "no block \"dwell:cheese\"",
            ),
            (
                with(|v| v["terrain"]["rock"] = "dwell:water".into()),
                "is not a solid block",
            ),
            (
                with(|v| v["structures"][0]["heightBlocks"] = 9.into()),
                "stands over the land",
            ),
            (
                with(|v| v["structures"][0]["blockM"] = 0.7.into()),
                "blockM",
            ),
            (
                with(|v| v["water"][0]["behind"] = "weir".into()),
                "no structure \"weir\"",
            ),
            (
                with(|v| v["water"][0]["levelM"] = 31.into()),
                "expected one of levelM or belowTopM",
            ),
            (
                with(|v| {
                    v["water"][0].as_object_mut().unwrap().remove("belowTopM");
                    v["water"][0]["levelM"] = 31.into();
                }),
                "would spill",
            ),
            (
                with(|v| v["events"][0]["do"] = "explode".into()),
                "is not an event a scene runs",
            ),
            (
                with(|v| v["events"][0]["target"] = "weir".into()),
                "events[0].target: no structure",
            ),
            (with(|v| v["axis"] = "up".into()), "scene.axis"),
            (with(|v| v["face"] = "c".into()), "scene.face"),
            (with(|v| v["formatVersion"] = 2.into()), "formatVersion"),
            (with(|v| v["world"] = "terrain".into()), "scene.world"),
        ];
        for (text, want) in cases {
            let err = Scene::parse(&text).expect_err(want);
            assert!(err.contains(want), "{want:?} not in:\n{err}");
        }
    }

    #[test]
    fn every_scene_file_parses() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../shared/theater/scenes");
        let mut n = 0;
        for e in std::fs::read_dir(&dir).expect("shared/theater/scenes") {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "json") {
                let text = std::fs::read_to_string(&p).unwrap();
                let s = Scene::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
                assert_eq!(
                    p.file_stem().and_then(|s| s.to_str()),
                    Some(s.name.as_str()),
                    "a scene's file is named after it"
                );
                n += 1;
            }
        }
        assert!(n > 0, "no scenes in {}", dir.display());
    }

    dwell_world::both_faces!(
        /// The fixture's cells on face `f` are face A's, put through the frame: the valley floor
        /// six metres under the land, the dam four blocks over the floor across the whole valley,
        /// and the lake one metre under the dam's top behind it.
        fn the_scene_is_built_face_local(f: FaceFrame) {
            let face = if f.is_b() { "b" } else { "a" };
            let s = Scene::parse(&fixture(face)).expect("valid");
            let a = Scene::parse(&fixture("a")).expect("valid");
            let air = |_: [i32; 3]| dwell_blocks::AIR;
            let (pf, pa) = (s.cells(air), a.cells(air));
            let want: Vec<_> = pa
                .cells
                .iter()
                .map(|&(c, m)| (f.cell(c), f.state(m)))
                .collect();
            assert_eq!(pf.cells, want);
            assert_eq!(pf.water_cells, pa.water_cells);
            let at = |p: &Plan, c: [i32; 3]| {
                Some(
                    p.cells
                        .iter()
                        .find(|(x, _)| *x == c)
                        .map_or(dwell_blocks::AIR, |x| x.1),
                )
            };
            let stone = dwell_blocks::require("dwell:stone");
            let sand = dwell_blocks::require("dwell:sand");
            let water = dwell_blocks::require("dwell:water");
            // the floor: 26 m (row 51 the sand on top), on the axis at z 10 (upstream of the dam)
            assert_eq!(at(&pa, [0, 51, 10]), Some(sand));
            assert_eq!(at(&pa, [0, 52, 10]), Some(water));
            // the lake's top row is 28.5–29 m (row 57), the cell over it air
            assert_eq!(at(&pa, [0, 57, 10]), Some(water));
            assert_eq!(at(&pa, [0, 58, 10]), Some(dwell_blocks::AIR));
            // the dam: rows 52–59 (26–30 m) at z 20 and 21, none at 22
            for z in [20, 21] {
                assert_eq!(at(&pa, [0, 52, z]), Some(stone));
                assert_eq!(at(&pa, [0, 59, z]), Some(stone));
                assert_eq!(at(&pa, [0, 60, z]), Some(dwell_blocks::AIR));
            }
            assert_eq!(at(&pa, [0, 52, 22]), Some(dwell_blocks::AIR));
            let dam = &pf.structures[0].1;
            assert!(dam.contains(&f.cell([0, 52, 20])) && !dam.contains(&f.cell([0, 52, 22])));
            // across the valley: the dam reaches each side's slope
            assert!(dam.contains(&f.cell([-6, 55, 20])), "keyed into the side");
            // a side is ramps: a grass ramp descending toward the axis
            let side = at(&pa, [6, rows(26.0 + 0.5 * 3.5) - 1, 40]).expect("a side cell");
            assert!(
                dwell_blocks::info(side)
                    .name
                    .starts_with("dwell:grass_ramp[facing=west"),
                "{}",
                dwell_blocks::info(side).name
            );
        }
    );

    #[test]
    fn a_world_that_already_holds_the_scene_changes_nothing() {
        let s = Scene::parse(&fixture("a")).unwrap();
        let first = s.cells(|_| dwell_blocks::AIR);
        let held: std::collections::HashMap<[i32; 3], StateId> =
            first.cells.iter().copied().collect();
        let again = s.cells(|c| held.get(&c).copied().unwrap_or(dwell_blocks::AIR));
        assert!(again.cells.is_empty(), "{} cells", again.cells.len());
        assert_eq!(again.structures, first.structures);
    }

    #[test]
    fn the_water_clock_runs_real_time_or_the_worlds() {
        let s = Scene::parse(&fixture("a")).unwrap();
        assert_eq!(s.water_speed(1200.0), Some(1200.0 / 86_400.0));
        let g = Scene::parse(&with(|v| v["waterClock"] = "game".into())).unwrap();
        assert_eq!(g.water_speed(1200.0), None);
    }
}
