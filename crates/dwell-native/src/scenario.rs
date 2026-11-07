//! A bench scenario run natively (`--scenario NAME`): the steps of `client/bench/scenarios.json`
//! (scripts/scenarios.ts describes them) done from the frame loop, and a `report.json` in the
//! browser bench's shape (`client/scripts/bench.ts` `ScenarioReport`), so the two compare
//! directly. Steps that need the browser (screenshots, Chrome traces, `goto`'s finder, bursts) are
//! skipped and listed in the report's `native.skipped`.

use std::path::PathBuf;

use dwell_client::game::{DayOverride, ViewOverride};
use dwell_client::telemetry;
use serde_json::{Map, Value, json};

use crate::{Args, Harness};

pub enum Status {
    Running,
    Done,
    Failed(String),
}

pub struct Run {
    sc: Value,
    name: String,
    dir: PathBuf,
    steps: Vec<Value>,
    i: usize,
    /// When the current step began (ms), for the steps that take time.
    began: Option<f64>,
    quiet: Option<f64>,
    mark_ms: f64,
    /// The server seconds since the start, at the last mark.
    mark_server: usize,
    records: Map<String, Value>,
    servers: Map<String, Value>,
    settles: Vec<f64>,
    skipped: Vec<String>,
    join: Option<Value>,
}

/// The join's timeout (bench.ts: `terrainReady` within 120 s).
const JOIN_MS: f64 = 120_000.0;

impl Run {
    pub fn load(args: &Args, name: &str) -> Result<Self, String> {
        let text = std::fs::read_to_string(&args.scenarios)
            .map_err(|e| format!("{}: {e}", args.scenarios.display()))?;
        let file: Value = serde_json::from_str(&text).map_err(|e| format!("scenarios: {e}"))?;
        let sc = file["scenarios"]
            .as_array()
            .and_then(|l| l.iter().find(|s| s["name"] == name))
            .cloned()
            .ok_or_else(|| format!("no scenario named {name}"))?;
        let steps = sc["steps"].as_array().cloned().unwrap_or_default();
        let out = args.out.clone().unwrap_or_else(|| {
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            crate::repo_root().join(format!("client/bench/out/native-{t}"))
        });
        let dir = out.join(name);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(Self {
            sc,
            name: name.into(),
            dir,
            steps,
            i: 0,
            began: None,
            quiet: None,
            mark_ms: 0.0,
            mark_server: 0,
            records: Map::new(),
            servers: Map::new(),
            settles: Vec::new(),
            skipped: Vec::new(),
            join: None,
        })
    }

    pub fn window_size(&self) -> (u32, u32) {
        let w = &self.sc["window"];
        (
            w["width"].as_u64().unwrap_or(1280) as u32,
            w["height"].as_u64().unwrap_or(720) as u32,
        )
    }

    /// The scenario's world: its seed (the bench's default 7), a fresh world, its type.
    pub fn configure(&self, args: &mut Args) {
        args.seed = self.sc["seed"].as_u64().unwrap_or(7);
        args.fresh = true;
        args.flat = self.sc["world"] == "flat";
        args.size = self.window_size();
        if self.sc["face"] == "b" || self.sc["portal"] == true {
            eprintln!("dwell-native: face B and portal spawns are not wired; spawning on face A");
        }
    }

    /// The scenario's settings over the shell's defaults (bench.ts `viewStorage`).
    pub fn started(&mut self, h: &mut Harness) {
        let s = &self.sc["settings"];
        if let Some(d) = s.get("detail") {
            if let Some(m) = d["distanceM"].as_f64() {
                h.lod_settings.detail_distance_m = (m >= 0.0).then_some(m);
            }
            if let Some(p) = d["pixelError"].as_f64() {
                h.lod_settings.pixel_error = p;
            }
            if let Some(mb) = d["memoryMb"].as_f64() {
                h.lod_settings.cache_bytes = (mb * 1_048_576.0) as usize;
            }
        }
        if let Some(q) = s["shadows"]["quality"].as_str() {
            let level = ["off", "low", "high"].iter().position(|n| *n == q);
            if let Some(l) = level {
                h.renderer()
                    .set_shadow_quality(dwell_look::shadows::ShadowQuality::from_level(l as u32));
            }
        }
        if let Some(q) = s["water"]["quality"].as_str() {
            let level = ["low", "medium", "high"].iter().position(|n| *n == q);
            if let Some(l) = level {
                h.renderer().set_water_quality(
                    dwell_client::render::water::WaterQuality::from_level(l as u32),
                );
            }
        }
        if let Some(c) = s["clouds"].as_str()
            && let Some(q) = dwell_look::clouds::CloudQuality::from_name(c)
        {
            h.renderer().set_clouds(q);
        }
        println!(
            "dwell-native: scenario {} (seed {}, {}x{})",
            self.name, h.args.seed, h.args.size.0, h.args.size.1
        );
    }

    pub fn update(&mut self, h: &mut Harness) -> Status {
        let now = telemetry::now_ms();
        let Some(playing) = h.playing_ms else {
            if now - h.started_ms > JOIN_MS {
                return Status::Failed(format!("{}: the game did not start", self.name));
            }
            return Status::Running;
        };
        if self.join.is_none() {
            let s = (playing - h.started_ms) / 1000.0;
            println!("  playing {s:.2} s after start");
            self.join = Some(json!({ "playingS": s, "loading": null, "stages": [] }));
        }
        while self.i < self.steps.len() {
            let step = self.steps[self.i].clone();
            match self.step(h, &step, now) {
                Ok(true) => {
                    self.i += 1;
                    self.began = None;
                    self.quiet = None;
                }
                Ok(false) => return Status::Running,
                Err(e) => return Status::Failed(format!("{}: {e}", self.name)),
            }
        }
        match self.write(h) {
            Ok(()) => Status::Done,
            Err(e) => Status::Failed(e),
        }
    }

    /// Does `s` (or its part due now): whether it is finished.
    fn step(&mut self, h: &mut Harness, s: &Value, now: f64) -> Result<bool, String> {
        let began = *self.began.get_or_insert(now);
        let elapsed = now - began;
        let num = |k: &str| {
            s[k].as_f64()
                .ok_or_else(|| format!("{s}: {k} is not a number"))
        };
        let what = s["do"].as_str().unwrap_or("");
        match what {
            "wait" => Ok(elapsed >= num("s")? * 1000.0),
            "key" => {
                h.input
                    .set_key(s["code"].as_str().unwrap_or(""), s["down"] == true);
                Ok(true)
            }
            "hold" => {
                let code = s["code"].as_str().unwrap_or("");
                if elapsed == 0.0 {
                    h.input.set_key(code, true);
                }
                if elapsed >= num("s")? * 1000.0 {
                    h.input.set_key(code, false);
                    return Ok(true);
                }
                Ok(false)
            }
            "look" => {
                h.input.look.yaw = num("yaw")?;
                h.input.look.pitch = num("pitch")?;
                Ok(true)
            }
            "fly" => {
                let on = s["on"] == true;
                if h.input.flight.flying() != on {
                    h.key_down(dwell_game::input::FLY_KEY, false, false, false);
                    h.input.key_up(dwell_game::input::FLY_KEY);
                }
                Ok(true)
            }
            "flySpeed" => {
                h.input.flight.set_speed_level(num("level")?);
                Ok(true)
            }
            "mark" => {
                telemetry::with(telemetry::Telemetry::reset);
                self.mark_ms = now;
                self.mark_server = h.server_seconds.len();
                Ok(true)
            }
            "record" => {
                let name = s["name"].as_str().unwrap_or("record").to_string();
                self.record(h, &name, now);
                Ok(true)
            }
            "settle" => {
                let lod = h.game.as_ref().and_then(|g| g.debug_state().lod);
                let idle = lod.is_none_or(|l| l.generating == 0 && l.meshing == 0 && l.asking == 0);
                self.quiet = if idle { self.quiet.or(Some(now)) } else { None };
                let settled = self.quiet.is_some_and(|q| now - q >= 1000.0);
                if settled || elapsed >= num("maxS")? * 1000.0 {
                    self.settles.push(elapsed / 1000.0);
                    println!("  settled in {:.2} s", elapsed / 1000.0);
                    return Ok(true);
                }
                Ok(false)
            }
            "camera" => {
                let p: Vec<f64> = s["position"]
                    .as_array()
                    .map(|a| a.iter().filter_map(Value::as_f64).collect())
                    .unwrap_or_default();
                let [x, y, z] = <[f64; 3]>::try_from(p).map_err(|_| "camera: position x,y,z")?;
                let Some(g) = &mut h.game else {
                    return Ok(false);
                };
                let feet = g.debug_state().feet;
                let eye = if s["aroundPlayer"] == true {
                    let y = if s["aboveFeet"] == true {
                        feet[1] + y
                    } else {
                        y
                    };
                    [feet[0] + x, y, feet[2] + z]
                } else {
                    [x, y, z]
                };
                g.set_view(Some(ViewOverride {
                    eye,
                    yaw_deg: num("yaw")?,
                    pitch_deg: num("pitch")?,
                }));
                Ok(true)
            }
            "releaseCamera" => {
                if let Some(g) = &mut h.game {
                    g.set_view(None);
                }
                Ok(true)
            }
            "time" => {
                if let Some(g) = &mut h.game {
                    let mut q = format!("time={}", num("t")?);
                    if let Some(m) = s["moon"].as_f64() {
                        q.push_str(&format!("&moon={m}"));
                    }
                    g.day_override = DayOverride::from_query(&q);
                }
                Ok(true)
            }
            _ => {
                self.skipped.push(what.to_string());
                Ok(true)
            }
        }
    }

    /// bench.ts `record`: the telemetry summary, the raw frames, the ticks and the state.
    fn record(&mut self, h: &mut Harness, name: &str, now: f64) {
        let stats = |list: Vec<(&'static str, telemetry::Stats)>| -> Value {
            list.into_iter()
                .map(|(n, s)| {
                    (
                        n.to_string(),
                        json!({ "n": s.n, "mean": s.mean, "p50": s.p50, "p95": s.p95,
                                "p99": s.p99, "min": s.min, "max": s.max, "sum": s.sum }),
                    )
                })
                .collect::<Map<_, _>>()
                .into()
        };
        let data = telemetry::with(|t| {
            let counters: Map<_, _> = t
                .counters()
                .map(|(n, v)| (n.to_string(), v.into()))
                .collect();
            let frames: Vec<Value> = t
                .frames()
                .map(|f| {
                    std::iter::once(f.at_ms)
                        .chain(f.values.iter().copied())
                        .collect::<Vec<_>>()
                        .into()
                })
                .collect();
            let ticks: Vec<Value> = t
                .ticks()
                .map(|k| {
                    json!({ "seq": k.seq, "atMs": k.at_ms, "feet": k.feet, "velocity": k.velocity,
                            "eye": k.eye, "state": k.state, "grounded": k.grounded,
                            "crouched": k.crouched, "touchingWall": k.touching_wall,
                            "ceilingBlocked": k.ceiling_blocked, "move": k.move_input,
                            "jump": k.jump, "crouch": k.crouch })
                })
                .collect();
            json!({
                "telemetry": {
                    "frames": stats(t.frame_stats()),
                    "samples": stats(t.sample_stats()),
                    "counters": counters,
                    "frameCount": t.frames().count(),
                },
                "frames": { "names": t.names(), "frames": frames },
                "ticks": ticks,
            })
        });
        let state = h.game.as_ref().map(|g| {
            let d = g.debug_state();
            json!({
                "terrainReady": d.terrain_ready,
                "feet": d.feet,
                "flying": d.flying,
                "lod": d.lod.map(|l| json!({
                    "generating": l.generating, "meshing": l.meshing, "asking": l.asking,
                    "nodes": l.nodes, "drawn": l.drawn, "cacheBytes": l.cache_bytes,
                })),
            })
        });
        let render = format!("{:?}", h.renderer().stats());
        let seconds = (now - self.mark_ms) / 1000.0;
        let mut rec = data;
        rec["state"] = state.unwrap_or(Value::Null);
        rec["render"] = json!({ "debug": render });
        rec["memory"] = Value::Null;
        rec["now"] = now.into();
        rec["longTasks"] = json!([]);
        rec["seconds"] = seconds.into();
        let server: Vec<Value> = h.server_seconds[self.mark_server.min(h.server_seconds.len())..]
            .iter()
            .map(|s| s.json())
            .collect();
        summarise(name, &rec, &server, seconds);
        self.servers.insert(name.into(), server.into());
        self.records.insert(name.into(), rec);
    }

    fn write(&self, h: &Harness) -> Result<(), String> {
        let a = h.adapter();
        let report = json!({
            "scenario": self.sc,
            "gpu": { "vendor": a.vendor, "device": a.device, "description": a.name,
                     "architecture": format!("{:?}", a.device_type), "driver": a.driver,
                     "backend": format!("{:?}", a.backend) },
            "errors": [],
            "records": self.records,
            "settles": self.settles,
            "join": self.join,
            "untils": [],
            "respawns": [],
            "bursts": {},
            "gotos": [],
            "servers": self.servers,
            "url": "native",
            "native": {
                "skipped": self.skipped,
                "workers": h.args.workers,
                "serverThreads": h.args.server_threads,
                "pixelRatio": h.args.pixel_ratio,
                "present": format!("{:?}", h.args.present),
            },
        });
        let path = self.dir.join("report.json");
        let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
        std::fs::write(&path, format!("{text}\n"))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if !self.skipped.is_empty() {
            println!("  skipped (browser-only): {}", self.skipped.join(", "));
        }
        println!("  report: {}", path.display());
        Ok(())
    }
}

/// bench.ts `summarise`, the lines that compare.
fn summarise(name: &str, rec: &Value, server: &[Value], seconds: f64) {
    let t = &rec["telemetry"];
    println!(
        "  [{name}] {} frames in {seconds:.2} s",
        t["frameCount"].as_u64().unwrap_or(0)
    );
    let iv = &t["frames"]["interval"];
    let f = |v: &Value| v.as_f64().unwrap_or(f64::NAN);
    println!(
        "    frame interval ms  p50 {:.2}  p95 {:.2}  p99 {:.2}  max {:.2}  ({:.1} fps)",
        f(&iv["p50"]),
        f(&iv["p95"]),
        f(&iv["p99"]),
        f(&iv["max"]),
        1000.0 / f(&iv["mean"])
    );
    for k in [
        "frame.cpu",
        "game.draw",
        "draw.render",
        "draw.acquire",
        "jobs.apply",
        "ticks",
    ] {
        let s = &t["frames"][k];
        if s.is_object() {
            println!(
                "      {k:<16} mean {:>6.2}  p95 {:>6.2}  max {:>6.2}",
                f(&s["mean"]),
                f(&s["p95"]),
                f(&s["max"])
            );
        }
    }
    let g = &t["samples"]["gpu.frame"];
    if g.is_object() {
        println!(
            "      gpu.frame        mean {:>6.2}  p95 {:>6.2}  max {:>6.2}",
            f(&g["mean"]),
            f(&g["p95"]),
            f(&g["max"])
        );
    }
    if !server.is_empty() {
        let n = server.len() as f64;
        let ticks: f64 = server.iter().map(|s| f(&s["ticks"])).sum();
        let mean: f64 = server.iter().map(|s| f(&s["tickMeanMs"])).sum::<f64>() / n;
        let worst = server
            .iter()
            .map(|s| f(&s["advanceMaxMs"]))
            .fold(0.0, f64::max);
        println!(
            "    server: {:.1} ticks/s, tick mean {mean:.2} ms, worst advance {worst:.2} ms",
            ticks / n
        );
    }
}
