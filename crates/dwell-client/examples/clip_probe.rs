//! The animation probe's CLI (docs/DEV_TOOLS.md "Clip probe"):
//!
//! ```text
//! cargo run -p dwell-client --example clip_probe -- <clip or arms role> [--item dwell:stone_axe|none]
//!     [--part tool_head[,grip,elbow,…]] [--samples 31] [--csv] [--svg out.svg]
//!     [--views fp,top,side,front,tp] [--face a|b] [--yaw 0] [--pitch 0]
//! ```
//!
//! Prints a table of each point's first-person screen place and body place over the clip, each
//! phase's direction of travel, the peak speed and the contact; `--csv` prints every sample in
//! every space instead; `--svg` writes the views' paths as a panel sheet.

use dwell_client::clip_probe::{Point, Probe, VIEWS, csv, probe, svg, table};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("clip_probe: {e}");
        std::process::exit(2);
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let mut it = args.iter();
    let mut p: Option<Probe> = None;
    let (mut want_csv, mut svg_out, mut views) = (false, None::<String>, VIEWS.to_vec());
    let mut parts: Option<Vec<Point>> = None;
    let (mut face, mut yaw, mut pitch, mut samples, mut item) =
        (None, None, None, None, None::<Option<&'static str>>);
    let num = |v: Option<&String>, f: &str| -> Result<f64, String> {
        v.ok_or(format!("{f} needs a value"))?
            .parse()
            .map_err(|e| format!("{f}: {e}"))
    };
    while let Some(a) = it.next() {
        match a.as_str() {
            "--csv" => want_csv = true,
            "--svg" => svg_out = Some(it.next().ok_or("--svg needs a path")?.clone()),
            "--part" | "--parts" => {
                let v = it.next().ok_or("--part needs a name")?;
                parts = Some(v.split(',').map(Point::parse).collect::<Result<_, _>>()?);
            }
            "--views" => {
                let v = it.next().ok_or("--views needs a list")?;
                views = Vec::new();
                for name in v.split(',') {
                    let known = VIEWS.iter().find(|k| **k == name).ok_or(format!(
                        "unknown view `{name}` (views: {})",
                        VIEWS.join(",")
                    ))?;
                    views.push(*known);
                }
            }
            "--item" => {
                let v = it.next().ok_or("--item needs a name")?;
                item = Some(if v == "none" {
                    None
                } else {
                    Some(Box::leak(v.clone().into_boxed_str()) as &'static str)
                });
            }
            "--face" => {
                face = Some(match it.next().map(String::as_str) {
                    Some("a") => dwell_world::FaceFrame::A,
                    Some("b") => dwell_world::FaceFrame::B,
                    other => return Err(format!("--face a|b, not {other:?}")),
                })
            }
            "--yaw" => yaw = Some(num(it.next(), "--yaw")?),
            "--pitch" => pitch = Some(num(it.next(), "--pitch")?),
            "--samples" => samples = Some(num(it.next(), "--samples")? as usize),
            "-h" | "--help" => {
                println!(
                    "clip_probe <clip|role> [--item NAME|none] [--part tool_head,grip,elbow] \
                     [--samples N] [--csv] [--svg OUT] [--views {}] [--face a|b] [--yaw D] \
                     [--pitch D]",
                    VIEWS.join(",")
                );
                return Ok(());
            }
            clip if p.is_none() && !clip.starts_with("--") => p = Some(Probe::new(clip)),
            other => return Err(format!("unexpected argument `{other}`")),
        }
    }
    let mut p = p.ok_or("name a clip or arms role, e.g. chop_level")?;
    if let Some(v) = parts {
        p.points = v;
    }
    if let Some(i) = item {
        p.item = i;
    }
    p.face = face.unwrap_or(p.face);
    p.yaw = yaw.unwrap_or(p.yaw);
    p.pitch = pitch.unwrap_or(p.pitch);
    p.samples = samples.unwrap_or(p.samples);
    let r = probe(&p)?;
    if want_csv {
        print!("{}", csv(&r));
    } else {
        print!("{}", table(&r));
    }
    if let Some(out) = svg_out {
        std::fs::write(&out, svg(&r, &views)).map_err(|e| format!("{out}: {e}"))?;
        eprintln!("wrote {out}");
    }
    Ok(())
}
