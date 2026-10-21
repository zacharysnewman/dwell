//! The hydrology prototype's viewer (docs/proposals/TERRAIN_HYDROLOGY_AND_VOLUMES.md):
//!
//! `cargo run --release -p dwell-worldgen --example hydrology -- --seed 7 --face a --out dir
//! [--profile medium] [--seed …] [--face …] [--profile …] [--time-only]`
//!
//! For each seed, face and profile it computes the map, prints its timing and statistics, and
//! writes PNG layers named `<seed>-<face>-<profile>-<layer>.png`: `original` (the sampled height
//! field, hillshaded), `eroded` (hillshaded over a sea-level colour ramp), `flow` (log discharge),
//! `rivers` (drawn by width over the hillshade), `lakes` and `map` (the composite). Images are
//! cropped to the face's land, one pixel per grid cell. `--time-only` writes nothing. For tuning,
//! `--set key=value` overrides a tunable of every profile given after it (`--label NAME` names the
//! result's files instead of the profile): cell, iterations, timeStep, erodibility, areaExponent,
//! slopeExponent, uplift, upliftReference, talusSlope, talusPasses, lakeFill, rainfall, riverThreshold,
//! lakeMinArea (km²), lakeMinDepth, maxBreachDepth.

mod png;

use std::time::Instant;

use dwell_world::FaceFrame;
use dwell_worldgen::hydrology::{self, HydrologyMap, MouthKind, NONE};

fn main() {
    let mut seeds = Vec::new();
    let mut faces = Vec::new();
    let mut profiles = Vec::new();
    let mut out = ".".to_string();
    let mut write = true;
    let mut sets: Vec<(String, f64)> = Vec::new();
    let mut label: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--seed" => seeds.push(args.next().and_then(|s| s.parse().ok()).expect("--seed N")),
            "--face" => faces.push(
                args.next()
                    .and_then(|s| FaceFrame::parse(&s))
                    .expect("--face a|b"),
            ),
            "--profile" => {
                let name = args.next().expect("--profile NAME");
                let mut p = *hydrology::profile(&name).unwrap_or_else(|| {
                    panic!(
                        "no profile {name} (hydrology.json has {:?})",
                        hydrology::PROFILES.map(|p| p.name)
                    )
                });
                for (k, v) in &sets {
                    set(&mut p, k, *v);
                }
                if let Some(l) = &label {
                    p.name = Box::leak(l.clone().into_boxed_str());
                }
                profiles.push(p);
            }
            "--set" => {
                let kv = args.next().expect("--set key=value");
                let (k, v) = kv.split_once('=').expect("--set key=value");
                sets.push((k.to_string(), v.parse().expect("a number")));
            }
            "--label" => label = Some(args.next().expect("--label NAME")),
            "--out" => out = args.next().expect("--out DIR"),
            "--time-only" => write = false,
            other => panic!("unknown argument {other}"),
        }
    }
    if seeds.is_empty() {
        seeds.push(7);
    }
    if faces.is_empty() {
        faces.push(FaceFrame::A);
    }
    if profiles.is_empty() {
        profiles.push(*hydrology::default_profile());
    }
    if write {
        std::fs::create_dir_all(&out).expect("create the output directory");
    }
    println!(
        "seed face profile grid seconds(sampling) land_km2 lakes lake_km2 rivers river_cells mouths(delta/estuary) max_q_m3s relief_before relief_after digest | largest: q_m3s length_km lakes_on_stem | systems>=5/>=10/>=20 m3s | big_lakes_on_stems(>=10) | delta_share"
    );
    for &seed in &seeds {
        let terrain = dwell_worldgen::BifacialTerrain::new(seed);
        for &face in &faces {
            for p in &profiles {
                let t = Instant::now();
                let (grid, x0, h) = hydrology::sample(&terrain, face, p.cell);
                let sampled = t.elapsed().as_secs_f64();
                let seed_of_face = terrain.local(face.face).seed();
                let map = hydrology::from_heights(grid, f64::from(p.cell), x0, &h, seed_of_face, p);
                let secs = t.elapsed().as_secs_f64();
                stats(seed, face, p.name, &map, secs, sampled);
                if write {
                    let name = format!(
                        "{out}/{seed}-{}-{}",
                        if face.is_b() { "b" } else { "a" },
                        p.name
                    );
                    render(&map, &name);
                }
            }
        }
    }
}

fn set(p: &mut hydrology::HydrologyParams, key: &str, v: f64) {
    match key {
        "cell" => p.cell = v as i32,
        "iterations" => p.iterations = v as u32,
        "timeStep" => p.time_step = v,
        "erodibility" => p.erodibility = v,
        "areaExponent" => p.area_exponent = v,
        "slopeExponent" => p.slope_exponent = v,
        "uplift" => p.uplift = v,
        "upliftReference" => p.uplift_reference = v,
        "talusSlope" => p.talus_slope = v,
        "talusPasses" => p.talus_passes = v as u32,
        "lakeFill" => p.lake_fill = v,
        "rainfall" => p.rainfall = v,
        "riverThreshold" => p.river_threshold = v,
        "lakeMinArea" => p.lake_min_area = v * 1e6,
        "mouthMinDischarge" => p.mouth_min_discharge = v,
        "mouthReach" => p.mouth_reach = v,
        "coastRelief" => {
            p.mouth_rule = if v > 0.0 {
                hydrology::MouthRule::CoastRelief
            } else {
                hydrology::MouthRule::RiverSlope
            }
        }
        "deltaMaxSlope" => p.delta_max_slope = v,
        "upliftRate" | "baseHeight" | "keepRelief" | "plains" | "shelfMin" | "shelfMax"
        | "rangeCount" | "rangeWidth" | "rangeLengthMin" | "rangeLengthMax" | "rangeWander"
        | "rangeStrength" | "highlands" | "plateCount" | "plateWidth" | "rift" => {
            let t = p.tectonics.as_mut().expect("the profile has an uplift layer");
            match key {
                "upliftRate" => t.rate = v,
                "baseHeight" => t.base_height = v,
                "keepRelief" => t.keep_relief = v,
                "plains" => t.plains = v,
                "shelfMin" => t.shelf_min = v,
                "shelfMax" => t.shelf_max = v,
                "rangeCount" => t.ranges.count = v as u32,
                "rangeWidth" => t.ranges.width = v,
                "rangeLengthMin" => t.ranges.length_min = v,
                "rangeLengthMax" => t.ranges.length_max = v,
                "rangeWander" => t.ranges.wander = v,
                "rangeStrength" => t.ranges.strength = v,
                "highlands" => t.highlands.strength = v,
                "plateCount" => t.plates.count = v as u32,
                "plateWidth" => t.plates.width = v,
                _ => t.plates.rift = v,
            }
        }
        "lakeMinDepth" => p.lake_min_depth = v,
        "maxBreachDepth" => p.max_breach_depth = v,
        _ => panic!("unknown tunable {key}"),
    }
}

fn stats(seed: u64, face: FaceFrame, profile: &str, m: &HydrologyMap, secs: f64, sampled: f64) {
    let a = m.cell * m.cell / 1e6;
    let land = m.sea.iter().filter(|s| !**s).count() as f64 * a;
    let lake_km2: f64 = m.lakes.iter().map(|l| l.area / 1e6).sum();
    let river_cells: usize = m.rivers.iter().map(|r| r.cells.len()).sum();
    let deltas = m
        .mouths
        .iter()
        .filter(|x| x.kind == MouthKind::Delta)
        .count();
    let max_q = m.discharge.iter().fold(0f32, |a, &b| a.max(b));
    let max = |v: &[f32]| v.iter().fold(0f32, |a, &b| a.max(b));
    let outlets = m.outlets();
    let (big, stem) = match outlets.first() {
        Some(&(c, q)) => (q, m.main_stem(c)),
        None => (
            0.0,
            hydrology::Stem {
                cells: Vec::new(),
                length: 0.0,
                lakes: Vec::new(),
            },
        ),
    };
    let over = |t: f64| outlets.iter().filter(|o| o.1 >= t).count();
    let stem_lakes: usize = outlets
        .iter()
        .filter(|o| o.1 >= 10.0)
        .map(|o| m.main_stem(o.0).lakes.len())
        .sum();
    let share = if m.mouths.is_empty() {
        0.0
    } else {
        deltas as f64 / m.mouths.len() as f64
    };
    print!(
        "{seed} {} {profile} {n}x{n} {secs:.3}({sampled:.3}) {land:.0} {} {lake_km2:.1} {} {river_cells} {}({deltas}/{}) {max_q:.0} {:.0} {:.0} {:016x}",
        if face.is_b() { "b" } else { "a" },
        m.lakes.len(),
        m.rivers.len(),
        m.mouths.len(),
        m.mouths.len() - deltas,
        max(&m.original),
        max(&m.height),
        m.digest(),
        n = m.grid.n,
    );
    println!(
        " | {big:.1} {:.1} {} | {}/{}/{} | {stem_lakes} | {:.0}%",
        stem.length / 1000.0,
        stem.lakes.len(),
        over(5.0),
        over(10.0),
        over(20.0),
        share * 100.0,
    );
    if std::env::var_os("HYDROLOGY_MOUTHS").is_some() {
        for mo in &m.mouths {
            println!("  mouth q {:.1} slope {:.4} {:?}", mo.discharge, mo.slope, mo.kind);
        }
    }
}

type Rgb = [f32; 3];

struct Canvas {
    w: usize,
    h: usize,
    x0: usize,
    z0: usize,
    px: Vec<Rgb>,
}

impl Canvas {
    fn put(&mut self, x: usize, z: usize, c: Rgb) {
        self.px[(z - self.z0) * self.w + (x - self.x0)] = c;
    }
    fn blend(&mut self, x: f64, z: f64, c: Rgb, a: f32) {
        let (xi, zi) = (
            x.floor() as i64 - self.x0 as i64,
            z.floor() as i64 - self.z0 as i64,
        );
        if xi < 0 || zi < 0 || xi >= self.w as i64 || zi >= self.h as i64 {
            return;
        }
        let p = &mut self.px[zi as usize * self.w + xi as usize];
        for k in 0..3 {
            p[k] = p[k] * (1.0 - a) + c[k] * a;
        }
    }
    /// A disc of radius r (px) at (x, z) (grid units, cell centres at integers + ½).
    fn disc(&mut self, x: f64, z: f64, r: f64, c: Rgb) {
        let reach = r.ceil() as i64 + 1;
        for dz in -reach..=reach {
            for dx in -reach..=reach {
                let (px, pz) = (x.floor() + dx as f64 + 0.5, z.floor() + dz as f64 + 0.5);
                let d = ((px - x).powi(2) + (pz - z).powi(2)).sqrt();
                let cover = (r + 0.5 - d).clamp(0.0, 1.0) as f32;
                if cover > 0.0 {
                    self.blend(px, pz, c, cover);
                }
            }
        }
    }
    fn line(&mut self, a: (f64, f64), b: (f64, f64), r: (f64, f64), c: Rgb) {
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let steps = (len * 3.0).ceil().max(1.0) as usize;
        for s in 0..=steps {
            let t = s as f64 / steps as f64;
            self.disc(
                a.0 + (b.0 - a.0) * t,
                a.1 + (b.1 - a.1) * t,
                r.0 + (r.1 - r.0) * t,
                c,
            );
        }
    }
    fn save(&self, path: &str) {
        let mut rgb = Vec::with_capacity(self.w * self.h * 3);
        for p in &self.px {
            for v in p {
                rgb.push((v.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
        }
        std::fs::write(path, png::encode(self.w as u32, self.h as u32, &rgb)).expect("write png");
    }
}

fn lerp3(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn ramp(stops: &[(f32, Rgb)], v: f32) -> Rgb {
    if v <= stops[0].0 {
        return stops[0].1;
    }
    for w in stops.windows(2) {
        if v <= w[1].0 {
            return lerp3(w[0].1, w[1].1, (v - w[0].0) / (w[1].0 - w[0].0));
        }
    }
    stops[stops.len() - 1].1
}

fn hex(c: u32) -> Rgb {
    [
        ((c >> 16) & 255) as f32 / 255.0,
        ((c >> 8) & 255) as f32 / 255.0,
        (c & 255) as f32 / 255.0,
    ]
}

/// The land's colours by height above sea level (m).
fn land_colour(h: f32) -> Rgb {
    ramp(
        &[
            (0.0, hex(0xC9B98A)),
            (8.0, hex(0x8DB45A)),
            (120.0, hex(0x5E9447)),
            (260.0, hex(0xA79A6B)),
            (420.0, hex(0x9A8576)),
            (560.0, hex(0xF2EEE8)),
        ],
        h,
    )
}

fn sea_colour(depth: f32) -> Rgb {
    lerp3(hex(0x7FB6D9), hex(0x2F5181), (depth / 25.0).clamp(0.0, 1.0))
}

/// Lambert shading of a height field (sun from the upper left, 45° up), vertical × `exaggerate`.
fn hillshade(m: &HydrologyMap, h: &[f32], i: usize, exaggerate: f32) -> f32 {
    let n = m.grid.n;
    let (x, z) = (i % n, i / n);
    let at = |x: usize, z: usize| h[z * n + x].max(0.0);
    let (xl, xr) = (x.saturating_sub(1), (x + 1).min(n - 1));
    let (zu, zd) = (z.saturating_sub(1), (z + 1).min(n - 1));
    let c = m.cell as f32;
    let dx = (at(xr, z) - at(xl, z)) * exaggerate / (c * (xr - xl) as f32);
    let dz = (at(x, zd) - at(x, zu)) * exaggerate / (c * (zd - zu) as f32);
    let nrm = [-dx, 1.0, -dz];
    let len = (nrm[0] * nrm[0] + 1.0 + nrm[2] * nrm[2]).sqrt();
    let l = [-0.5f32, std::f32::consts::FRAC_1_SQRT_2, -0.5];
    ((nrm[0] * l[0] + nrm[1] * l[1] + nrm[2] * l[2]) / len).clamp(0.0, 1.0)
}

fn shade(c: Rgb, s: f32) -> Rgb {
    let k = 0.35 + 0.9 * s;
    [c[0] * k, c[1] * k, c[2] * k]
}

fn render(m: &HydrologyMap, name: &str) {
    let n = m.grid.n;
    // Crop to the land, with a margin of 20 cells.
    let (mut x0, mut z0, mut x1, mut z1) = (n, n, 0, 0);
    for i in 0..m.grid.len() {
        if !m.sea[i] {
            let (x, z) = (i % n, i / n);
            (x0, z0, x1, z1) = (x0.min(x), z0.min(z), x1.max(x), z1.max(z));
        }
    }
    if x0 > x1 {
        (x0, z0, x1, z1) = (0, 0, n - 1, n - 1);
    }
    let pad = 20;
    let (x0, z0) = (x0.saturating_sub(pad), z0.saturating_sub(pad));
    let (x1, z1) = ((x1 + pad).min(n - 1), (z1 + pad).min(n - 1));
    let new = || Canvas {
        w: x1 - x0 + 1,
        h: z1 - z0 + 1,
        x0,
        z0,
        px: vec![[0.0; 3]; (x1 - x0 + 1) * (z1 - z0 + 1)],
    };
    let cells = || (z0..=z1).flat_map(move |z| (x0..=x1).map(move |x| (x, z, z * n + x)));
    let ex = 3.0;

    // The original and the eroded height, hillshaded over the colour ramp.
    for (layer, h) in [("original", &m.original), ("eroded", &m.height)] {
        let mut c = new();
        for (x, z, i) in cells() {
            let col = if m.sea[i] {
                sea_colour(-h[i])
            } else {
                shade(land_colour(h[i]), hillshade(m, h, i, ex))
            };
            c.put(x, z, col);
        }
        c.save(&format!("{name}-{layer}.png"));
    }

    // Flow accumulation, log scale.
    let mut c = new();
    let qmax = m.discharge.iter().fold(1e-6f32, |a, &b| a.max(b)).log10();
    let qmin = (m.cell * m.cell / 31_557_600.0) as f32;
    let qmin = qmin.log10();
    for (x, z, i) in cells() {
        let col = if m.sea[i] {
            hex(0x101820)
        } else {
            let t = ((m.discharge[i].max(1e-9).log10() - qmin) / (qmax - qmin)).clamp(0.0, 1.0);
            ramp(
                &[
                    (0.0, hex(0x1B1B2F)),
                    (0.3, hex(0x3B4A8C)),
                    (0.55, hex(0x2C9FC9)),
                    (0.8, hex(0x8FE3E0)),
                    (1.0, hex(0xFFFFFF)),
                ],
                t,
            )
        };
        c.put(x, z, col);
    }
    c.save(&format!("{name}-flow.png"));

    // Grey hillshade base, for the rivers and lakes layers.
    let grey = |c: &mut Canvas| {
        for (x, z, i) in cells() {
            let col = if m.sea[i] {
                hex(0x9FB4C4)
            } else {
                let s = hillshade(m, &m.height, i, ex);
                let g = 0.45 + 0.5 * s;
                [g, g, g * 0.97]
            };
            c.put(x, z, col);
        }
    };
    let lake_blue = hex(0x3D7FD1);
    let river_blue = hex(0x1F5FBF);
    let draw_lakes = |c: &mut Canvas, outlets: bool| {
        for (x, z, i) in cells() {
            if m.lake_of[i] != NONE {
                let l = &m.lakes[m.lake_of[i] as usize];
                let depth = (l.level - f64::from(m.height[i])) as f32;
                c.put(
                    x,
                    z,
                    lerp3(hex(0x8EC5F0), lake_blue, (depth / 20.0).clamp(0.0, 1.0)),
                );
            }
        }
        if outlets {
            for l in &m.lakes {
                if l.outlet != NONE {
                    let o = l.outlet as usize;
                    c.disc(
                        (o % n) as f64 + 0.5,
                        (o / n) as f64 + 0.5,
                        1.2,
                        hex(0xE0402A),
                    );
                }
            }
        }
    };
    let draw_rivers = |c: &mut Canvas, colour: Rgb| {
        let to_px =
            |v: &hydrology::Vertex| ((v.x - m.x0) / m.cell + 0.5, (v.z - m.x0) / m.cell + 0.5);
        // Widths are exaggerated to read at one pixel per cell: radius ½ px + width ÷ 40 m.
        let radius = |w: f64| 0.3 + w / 40.0;
        for r in &m.rivers {
            for w in r.path.windows(2) {
                c.line(
                    to_px(&w[0]),
                    to_px(&w[1]),
                    (radius(w[0].width), radius(w[1].width)),
                    colour,
                );
            }
        }
    };
    let draw_mouths = |c: &mut Canvas| {
        for mo in &m.mouths {
            let (x, z) = ((mo.x - m.x0) / m.cell + 0.5, (mo.z - m.x0) / m.cell + 0.5);
            let col = match mo.kind {
                MouthKind::Delta => hex(0xF29A2E),
                MouthKind::Estuary => hex(0xC23B9B),
            };
            c.disc(x, z, 2.5, col);
        }
    };

    let mut c = new();
    grey(&mut c);
    draw_rivers(&mut c, river_blue);
    c.save(&format!("{name}-rivers.png"));

    // River widths: every river by its width, coloured by discharge (log), so the big ones stand
    // out; lakes under them.
    let mut c = new();
    grey(&mut c);
    draw_lakes(&mut c, false);
    {
        let to_px =
            |v: &hydrology::Vertex| ((v.x - m.x0) / m.cell + 0.5, (v.z - m.x0) / m.cell + 0.5);
        let colour = |q: f64| {
            let t = ((q.max(0.1).log10() + 0.5) / 2.0).clamp(0.0, 1.0) as f32;
            ramp(
                &[
                    (0.0, hex(0x9CC9E8)),
                    (0.5, hex(0x2E7FD0)),
                    (0.8, hex(0x1840A0)),
                    (1.0, hex(0x9B1D5A)),
                ],
                t,
            )
        };
        // Draw small rivers first so the big ones lie on top.
        let mut segs: Vec<(&hydrology::Vertex, &hydrology::Vertex)> = m
            .rivers
            .iter()
            .flat_map(|r| r.path.windows(2).map(|w| (&w[0], &w[1])))
            .collect();
        segs.sort_by(|a, b| a.1.discharge.total_cmp(&b.1.discharge));
        for (a, b) in segs {
            let r = |w: f64| 0.25 + w / 22.0;
            c.line(to_px(a), to_px(b), (r(a.width), r(b.width)), colour(b.discharge));
        }
    }
    c.save(&format!("{name}-widths.png"));

    // The uplift layer: the uplift rate over the hillshade, with the ranges' spines and the plates'
    // points; and the rain and the rock where they vary.
    let umax = m.uplift.iter().fold(1e-12f32, |a, &b| a.max(b));
    let mut c = new();
    for (x, z, i) in cells() {
        let col = if m.sea[i] {
            hex(0x22303C)
        } else {
            let t = (m.uplift[i] / umax).clamp(-1.0, 1.0);
            let base = if t < 0.0 {
                lerp3(hex(0x3B6E5A), hex(0x2A4F9E), -t)
            } else {
                ramp(
                    &[
                        (0.0, hex(0x3B6E5A)),
                        (0.25, hex(0xA9B55B)),
                        (0.55, hex(0xD9893B)),
                        (0.8, hex(0xB8412E)),
                        (1.0, hex(0xFFF0E0)),
                    ],
                    t,
                )
            };
            shade(base, 0.35 + 0.5 * hillshade(m, &m.height, i, ex))
        };
        c.put(x, z, col);
    }
    for sp in &m.spines {
        for w in sp.points.windows(2) {
            let px = |p: (f64, f64)| ((p.0 - m.x0) / m.cell + 0.5, (p.1 - m.x0) / m.cell + 0.5);
            c.line(px(w[0]), px(w[1]), (0.6, 0.6), hex(0x101010));
        }
    }
    for &(x, z) in &m.plates {
        c.disc((x - m.x0) / m.cell + 0.5, (z - m.x0) / m.cell + 0.5, 3.0, hex(0xFFFFFF));
    }
    c.save(&format!("{name}-uplift.png"));
    for (layer, v, lo, hi) in [("rain", &m.rain, 0.3f32, 2.5f32), ("rock", &m.erodibility, 0.0, 0.0)] {
        let (vmin, vmax) = m
            .sea
            .iter()
            .zip(v.iter())
            .filter(|(s, _)| !**s)
            .fold((f32::MAX, f32::MIN), |(a, b), (_, &x)| (a.min(x), b.max(x)));
        if vmax - vmin < 1e-9 {
            continue;
        }
        let (lo, hi) = if lo < hi { (lo, hi) } else { (vmin, vmax) };
        let mut c = new();
        for (x, z, i) in cells() {
            let col = if m.sea[i] {
                hex(0x22303C)
            } else {
                let t = ((v[i] - lo) / (hi - lo)).clamp(0.0, 1.0);
                shade(
                    ramp(&[(0.0, hex(0xC8A26A)), (0.5, hex(0xE8E4D0)), (1.0, hex(0x2E6FB0))], t),
                    0.4 + 0.5 * hillshade(m, &m.height, i, ex),
                )
            };
            c.put(x, z, col);
        }
        c.save(&format!("{name}-{layer}.png"));
    }

    let mut c = new();
    grey(&mut c);
    draw_lakes(&mut c, true);
    c.save(&format!("{name}-lakes.png"));

    // The composite: the eroded colours, lakes, rivers and mouths.
    let mut c = new();
    for (x, z, i) in cells() {
        let col = if m.sea[i] {
            sea_colour(-m.height[i])
        } else {
            shade(land_colour(m.height[i]), hillshade(m, &m.height, i, ex))
        };
        c.put(x, z, col);
    }
    draw_lakes(&mut c, false);
    draw_rivers(&mut c, hex(0x2A6FD6));
    draw_mouths(&mut c);
    c.save(&format!("{name}-map.png"));
}
