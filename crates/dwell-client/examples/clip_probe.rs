//! The animation probe's CLI (docs/DEV_TOOLS.md "Clip probe"):
//!
//! ```text
//! cargo run -p dwell-client --example clip_probe -- <clip or arms role> [--item dwell:stone_axe|none]
//!     [--part tool_head[,grip,elbow,…]] [--samples 31] [--csv] [--svg out.svg]
//!     [--views fp,top,side,front,tp] [--face a|b] [--yaw 0] [--pitch 0]
//!     [--fit "t/gx,gy,gz/hx,hy,hz[/bx,by,bz]" …] [--body]
//! ```
//!
//! `--fit` (repeatable) fits a key instead: the root and arm turns putting the grip and the tool
//! head at those places (the player's frame: right, up, ahead of the eye), and the blade's edge
//! facing `b` if given (the strike's way), printed as the clip's JSON keys; with `--body` the
//! third-person body's key (the chest's twist and the arm; the frame right, up, ahead of the feet).
//!
//! Prints a table of each point's first-person screen place and body place over the clip, each
//! phase's direction of travel, the peak speed and the contact; `--csv` prints every sample in
//! every space instead; `--svg` writes the views' paths as a panel sheet.

use dwell_client::clip_probe::{Fit, Point, Probe, Target, VIEWS, csv, fit, probe, svg, table};

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
    let mut fits: Vec<(f32, Target)> = Vec::new();
    let mut body = false;
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
            "--body" => body = true,
            "--fit" => {
                // `t/gx,gy,gz/hx,hy,hz`: a key at t putting the grip and the head there.
                let v = it.next().ok_or("--fit needs t/grip/head")?;
                let f: Vec<&str> = v.split('/').collect();
                let xyz = |s: &str| -> Result<[f32; 3], String> {
                    let n: Vec<f32> = s
                        .split(',')
                        .map(|x| {
                            x.trim()
                                .parse::<f32>()
                                .map_err(|e| format!("--fit {s}: {e}"))
                        })
                        .collect::<Result<_, _>>()?;
                    n.try_into()
                        .map_err(|_| format!("--fit wants 3 numbers: {s}"))
                };
                if !(3..=4).contains(&f.len()) {
                    return Err(format!(
                        "--fit wants t/gx,gy,gz/hx,hy,hz[/bx,by,bz], not {v}"
                    ));
                }
                let t = f[0].parse::<f32>().map_err(|e| format!("--fit t: {e}"))?;
                let blade = f.get(3).map(|b| xyz(b)).transpose()?;
                fits.push((
                    t,
                    Target {
                        grip: xyz(f[1])?,
                        head: xyz(f[2])?,
                        blade,
                    },
                ));
            }
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
    if !fits.is_empty() {
        // Each key starts from the one before (the first from the hold), so the turns stay continuous.
        let role = p.clip.clone();
        let mut from = if body {
            Fit {
                arm: [-20.0, 0.0, -4.0],
                forearm: -65.0,
                ..Fit::default()
            }
        } else {
            Fit {
                arm: [-70.0, 22.0, 0.0],
                forearm: -40.0,
                hand: [10.0, 0.0, 0.0],
                ..Fit::default()
            }
        };
        for (t, w) in fits {
            let f = fit(&role, p.item, body, w, from);
            println!("{}", f.json(t));
            eprintln!(
                "  t {t}: grip {:+.2} {:+.2} {:+.2} (want {:?}), head {:+.2} {:+.2} {:+.2} (want {:?}), blade {:+.2} {:+.2} {:+.2}",
                f.grip[0],
                f.grip[1],
                f.grip[2],
                w.grip,
                f.head[0],
                f.head[1],
                f.head[2],
                w.head,
                f.blade[0],
                f.blade[1],
                f.blade[2]
            );
            from = f;
        }
        return Ok(());
    }
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
