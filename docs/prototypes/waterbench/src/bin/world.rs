#[path = "../sim.rs"]
mod sim;
use dwell_worldgen::worldmap::{self, RiverEnd, NO_LAKE};
use dwell_worldgen::terrain::{TerrainGenerator, Water};
use rayon::prelude::*;
use sim::*;
use std::collections::HashMap;
use std::time::Instant;

const BANK_W: f32 = 3.0; // bank columns beside a channel
const BANK_H: f32 = 0.5; // a bank's ground over the river's surface
const LAKE_DEPTH: f32 = 3.0;

fn q5(h: f32) -> f32 {
    (h * 2.0).floor() / 2.0
}

struct Raster {
    // column -> (ground, initial depth); channel/lake columns lower the ground, banks only fill void
    cols: HashMap<(i32, i32), (f32, f32)>,
    sources: Vec<((i32, i32), f32)>,
    sinks: Vec<(i32, i32)>,
    /// a channel column's level for its flow: the smooth bed plus the normal depth (Manning)
    target: HashMap<(i32, i32), f32>,
    /// a channel column's map slope, width and discharge
    info: HashMap<(i32, i32), (f32, f32, f32)>,
}

fn build(seed: u64, with_lakes: bool) -> (Sim, usize) {
    let m = worldmap::for_seed(seed);
    let mut r = Raster { cols: HashMap::new(), sources: Vec::new(), sinks: Vec::new(), target: HashMap::new(), info: HashMap::new() };
    let wet = |r: &mut Raster, x: i32, z: i32, g: f32, s: f32| {
        let e = r.cols.entry((x, z)).or_insert((VOID, 0.0));
        if g < e.0 || e.0 >= VOID || e.1 == 0.0 {
            if g < e.0 {
                *e = (g, (s - g).max(0.0));
            }
        }
    };
    let bank = |r: &mut Raster, x: i32, z: i32, g: f32| {
        let e = r.cols.entry((x, z)).or_insert((VOID, 0.0));
        if e.0 >= VOID {
            *e = (g, 0.0);
        }
    };
    // lakes first: 100 m map cells
    if with_lakes {
        for j in 0..m.n {
            for i in 0..m.n {
                let l = m.lake[j * m.n + i];
                if l == NO_LAKE {
                    continue;
                }
                let level = m.lakes[l as usize - 1].level;
                let cx = m.x0 + i as f64 * m.cell;
                let cz = m.x0 + j as f64 * m.cell;
                let h = (m.cell / 2.0) as i32;
                for z in (cz as i32 - h)..(cz as i32 + h) {
                    for x in (cx as i32 - h)..(cx as i32 + h) {
                        wet(&mut r, x, z, q5(level - LAKE_DEPTH), level);
                    }
                }
            }
        }
    }
    for river in &m.rivers {
        let p = &river.path;
        for k in 0..p.len().saturating_sub(1) {
            let (a, b) = (p[k], p[k + 1]);
            let (dx, dz) = (b.x - a.x, b.z - a.z);
            let len2 = dx * dx + dz * dz;
            let half = 0.5 * a.width.max(b.width).max(1.0);
            let reach = half + BANK_W + 1.0;
            let (x0, x1) = ((a.x.min(b.x) - reach).floor() as i32, (a.x.max(b.x) + reach).ceil() as i32);
            let (z0, z1) = ((a.z.min(b.z) - reach).floor() as i32, (a.z.max(b.z) + reach).ceil() as i32);
            for z in z0..=z1 {
                for x in x0..=x1 {
                    let (px, pz) = (x as f32 + 0.5, z as f32 + 0.5);
                    let t = if len2 > 0.0 { (((px - a.x) * dx + (pz - a.z) * dz) / len2).clamp(0.0, 1.0) } else { 0.0 };
                    let (cx, cz) = (a.x + dx * t, a.z + dz * t);
                    let d = (px - cx).hypot(pz - cz);
                    let hw = 0.5 * (a.width + (b.width - a.width) * t).max(1.0);
                    let surf = a.surface + (b.surface - a.surface) * t;
                    let dep = (a.depth + (b.depth - a.depth) * t).max(0.3);
                    if d <= hw {
                        wet(&mut r, x, z, q5(surf - dep), surf);
                        let q = (a.discharge + (b.discharge - a.discharge) * t).max(0.05);
                        let slope = ((a.surface - b.surface) / len2.sqrt().max(1.0)).clamp(1e-4, 0.05);
                        let hn = (q * unsafe { sim::MANNING } / (2.0 * hw * slope.sqrt())).powf(0.6).min(dep);
                        let lt = surf - dep + hn;
                        let e = r.target.entry((x, z)).or_insert(lt);
                        if lt > *e { *e = lt; }
                        r.info.insert((x, z), (slope, 2.0 * hw, q));
                    } else if d <= hw + BANK_W {
                        bank(&mut r, x, z, q5(surf + BANK_H) + 0.5);
                    }
                }
            }
        }
        // inflow: the head's discharge (unless it flows out of a lake), then the map's growth along it
        let head = p[0];
        if river.from_lake == NO_LAKE || !with_lakes {
            r.sources.push((((head.x) as i32, (head.z) as i32), head.discharge.max(0.05)));
        }
        for k in 1..p.len() {
            let inc = p[k].discharge - p[k - 1].discharge;
            if inc > 0.0 && inc < 1.0 {
                r.sources.push(((p[k].x as i32, p[k].z as i32), inc));
            }
        }
        if river.end == RiverEnd::Sea || (!with_lakes && matches!(river.end, RiverEnd::Lake(_))) {
            let last = p[p.len() - 1];
            let hw = (0.5 * last.width).max(1.0) as i32 + 1;
            for z in -hw..=hw {
                for x in -hw..=hw {
                    r.sinks.push((last.x as i32 + x, last.z as i32 + z));
                }
            }
        }
    }
    // chunks
    let mut keys: Vec<(i32, i32)> = r.cols.keys().map(|&(x, z)| (x.div_euclid(CS as i32), z.div_euclid(CS as i32))).collect();
    keys.sort_unstable();
    keys.dedup();
    let mut sim = Sim::new(keys.clone());
    let idx: HashMap<(i32, i32), usize> = keys.iter().enumerate().map(|(i, &k)| (k, i)).collect();
    let loc = |x: i32, z: i32| -> Option<(usize, usize)> {
        let c = *idx.get(&(x.div_euclid(CS as i32), z.div_euclid(CS as i32)))?;
        Some((c, (z.rem_euclid(CS as i32) as usize) * CS + x.rem_euclid(CS as i32) as usize))
    };
    let mut wetn = 0;
    for (&(x, z), &(g, d)) in &r.cols {
        let (c, l) = loc(x, z).unwrap();
        sim.ground[c * CA + l] = g;
        sim.depth[c * CA + l] = d;
        if d > 0.0 {
            wetn += 1;
        }
    }
    // sources onto the nearest wet column (within 3 m)
    for ((x, z), q) in r.sources {
        let mut best = None;
        'o: for rad in 0..=3 {
            for dz in -rad..=rad {
                for dx in -rad..=rad {
                    if let Some((c, l)) = loc(x + dx, z + dz) {
                        if sim.depth[c * CA + l] > 0.0 {
                            best = Some((c, l));
                            break 'o;
                        }
                    }
                }
            }
        }
        if let Some((c, l)) = best {
            sim.sources[c].push((l as u16, q));
        }
    }
    for (x, z) in r.sinks {
        if let Some((c, l)) = loc(x, z) {
            if sim.ground[c * CA + l] < VOID {
                sim.sinks[c].push(l as u16);
            }
        }
    }
    let mut kind = vec![0u8; sim.nch];
    for (&(x, z), &(_, d)) in &r.cols {
        let (c, _) = loc(x, z).unwrap();
        if d >= LAKE_DEPTH - 0.6 { kind[c] = 2; } else if d > 0.0 && kind[c] == 0 { kind[c] = 1; }
    }
    unsafe { KIND = kind; }
    let mut info = vec![(0.0f32, 0.0f32, 0.0f32); sim.nch * CA];
    for (&(x, z), &v) in &r.info { let (c, l) = loc(x, z).unwrap(); info[c * CA + l] = v; }
    unsafe { INFO = info; }
    sim.rebuild_inflow();
    if std::env::var("MAP_LEVELS").is_err() {
        let mut tgt = vec![f32::NEG_INFINITY; sim.nch * CA];
        for (&(x, z), &v) in &r.target { let (c, l) = loc(x, z).unwrap(); tgt[c * CA + l] = v; }
        fill_start(&mut sim, &tgt);
    }
    (sim, wetn)
}

/// The start: each channel column filled to the level its lips hold back (a priority flood from the
/// outlets over the sparse grid; walls stop it), plus a thin overflow sheet.
fn fill_start(sim: &mut Sim, tgt: &[f32]) {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let n = sim.nch * CA;
    let mut fill = vec![f32::INFINITY; n];
    let mut heap = BinaryHeap::new();
    let key = |v: f32| Reverse((v * 1000.0) as i64);
    for c in 0..sim.nch {
        for &l in &sim.sinks[c] {
            let i = c * CA + l as usize;
            fill[i] = sim.ground[i];
            heap.push((key(fill[i]), i as u32));
        }
    }
    let nb = |sim: &Sim, i: usize, k: usize| -> Option<usize> {
        let (c, l) = (i / CA, i % CA);
        let (x, z) = (l % CS, l / CS);
        match k {
            0 => if x + 1 < CS { Some(i + 1) } else { let e = sim.nbr[c][0]; (e != NONE).then(|| e as usize * CA + z * CS) },
            1 => if x > 0 { Some(i - 1) } else { let w = sim.nbr[c][1]; (w != NONE).then(|| w as usize * CA + z * CS + CS - 1) },
            2 => if z + 1 < CS { Some(i + CS) } else { let s = sim.nbr[c][2]; (s != NONE).then(|| s as usize * CA + x) },
            _ => if z > 0 { Some(i - CS) } else { let nn = sim.nbr[c][3]; (nn != NONE).then(|| nn as usize * CA + (CS - 1) * CS + x) },
        }
    };
    while let Some((_, i)) = heap.pop() {
        let i = i as usize;
        let f = fill[i];
        for k in 0..4 {
            if let Some(j) = nb(sim, i, k) {
                if sim.ground[j] >= VOID || fill[j] < f32::INFINITY { continue; }
                fill[j] = f.max(sim.ground[j]);
                heap.push((key(fill[j]), j as u32));
            }
        }
    }
    let (mut changed, mut before, mut after) = (0usize, 0.0f64, 0.0f64);
    for i in 0..n {
        if sim.depth[i] > 0.0 {
            before += sim.depth[i] as f64;
            let d = if fill[i].is_finite() { (fill[i] + 0.15).max(tgt[i]) - sim.ground[i] } else { sim.depth[i] };
            let d = d.max(0.0);
            sim.depth[i] = d;
            after += d as f64;
            changed += 1;
        }
    }
    println!("start: {changed} channel columns at their normal depth, at least their lips + 0.15 m: {:.2} M m3 (the map's levels: {:.2} M m3)", after / 1e6, before / 1e6);
}
static mut KIND: Vec<u8> = Vec::new();
static mut INFO: Vec<(f32, f32, f32)> = Vec::new();

fn breakdown(sim: &Sim) -> String {
    let kind = unsafe { &*std::ptr::addr_of!(KIND) };
    let mut a = [[0usize; 3]; 3]; // [kind][0 total,1 awake,2 awake by dq]
    let mut why = [0usize; 3]; // awake river chunks: dq only, div only, both
    for c in 0..sim.nch {
        let k = kind[c] as usize;
        a[k][0] += 1;
        if sim.awake[c] != 0 {
            a[k][1] += 1;
            let dqb = sim.dq[c] >= SLEEP_DQ; let dvb = sim.div[c] >= SLEEP_DIV;
            if k == 1 { if dqb && dvb { why[2] += 1 } else if dqb { why[0] += 1 } else if dvb { why[1] += 1 } }
        }
    }
    format!("lake {}/{} river {}/{} dry {}/{} awake; river awake by dq {}, div {}, both {}", a[2][1], a[2][0], a[1][1], a[1][0], a[0][1], a[0][0], why[0], why[1], why[2])
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seed: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(42);
    let water_time: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3600.0);
    let theta: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.95);
    let t0 = Instant::now();
    let lakes = args.get(4).map_or(true, |s| s != "nolakes");
    let skip_a = args.get(5).is_some_and(|s| s == "skipA");
    let real = std::env::var("REAL").is_ok();
    let only_build = args.get(7).is_some_and(|m| m == "build");
    let (mut sim, wetn) = if real { build_real(seed) } else { build(seed, lakes) };
    if std::env::var("HOLD_SEA").is_ok() { sim.hold_sea = true; for c in 0..sim.nch { for &l in &sim.sinks[c] { let i = c * CA + l as usize; sim.depth[i] = sim.hold[i]; } } }
    sim.theta = theta;
    if let Ok(v) = std::env::var("BAND") { unsafe { sim::BAND = v.parse().unwrap(); } }
    if let Ok(v) = std::env::var("BAND_DQ") { unsafe { sim::BAND_DQ = v.parse().unwrap(); } }
    if let Ok(v) = std::env::var("WAKE_BAND") { unsafe { sim::WAKE_BAND = v.parse().unwrap(); } }
    if let Ok(v) = std::env::var("WIN_SECS") { unsafe { sim::WIN_SECS = v.parse().unwrap(); } }
    if let Ok(v) = std::env::var("EMA_SECS") { sim.ema_secs = v.parse().unwrap(); }
    sim.mean_sleep = std::env::var("MEAN_SLEEP").is_ok();
    unsafe { sim::SMOOTH = std::env::var("SMOOTH").is_ok(); sim::DRIFT = std::env::var("NO_DRIFT").is_err(); }
    sim.pools.on = std::env::var("POOLS").is_ok();
    if let Ok(v) = std::env::var("POOL_V") { unsafe { sim::POOL_V = v.parse().unwrap(); } }
    if let Ok(v) = std::env::var("POOL_BAND") { unsafe { sim::POOL_BAND = v.parse().unwrap(); } }
    unsafe { sim::VECTOR = std::env::var("FACE_ONLY").is_err(); }
    if let Ok(v) = std::env::var("MANNING") { unsafe { sim::MANNING = v.parse().unwrap(); } }
    if let Ok(v) = std::env::var("CFL") { unsafe { sim::CFL = v.parse().unwrap(); } }
    sim.spring_sleep = std::env::var("OLD_SPRINGS").is_err();
    let cols = sim.nch * CA;
    let srcs: usize = sim.sources.iter().map(|v| v.len()).sum();
    let snks: usize = sim.sinks.iter().map(|v| v.len()).sum();
    println!(
        "seed {seed}: built in {:.1}s: {} chunks ({} columns, {:.0} MB of fields), {} wet, {} springs, {} outlet columns, threads {}",
        t0.elapsed().as_secs_f64(), sim.nch, cols, (cols * 7 * 4) as f64 / 1e6, wetn, srcs, snks, rayon::current_num_threads()
    );

    if only_build { return; }
    // A. everything awake: the raw cost per column per substep
    sim.sleep = false;
    let s0 = sim.stored();
    if !skip_a {
    for _ in 0..3 { sim.substep(0.05); }
    let n = 30;
    let t = Instant::now();
    for _ in 0..n { sim.substep(0.05); }
    let el = t.elapsed().as_secs_f64();
    println!(
        "A. all awake: {:.1} ms a substep, {:.2} ns a column-substep ({} threads)",
        el / n as f64 * 1e3, el * 1e9 / (n * cols) as f64, rayon::current_num_threads()
    );
    let pool1 = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap();
    pool1.install(|| {
        let t = Instant::now();
        for _ in 0..5 { sim.substep(0.05); }
        let el = t.elapsed().as_secs_f64();
        println!("   one thread: {:.1} ms a substep, {:.2} ns a column-substep", el / 5.0 * 1e3, el * 1e9 / (5 * cols) as f64);
    });
    }

    // B. from the map's levels (or a saved state), with sleep: water time vs wall time, awake chunks
    sim.sleep = true;
    if let Some(p) = args.get(6) { if sim.load(p) { println!("resumed {p} at t {:.0}", sim.t); } }
    let s0 = if sim.t > 0.0 { sim.totals(); sim.stored() - (sim.brought - sim.taken) } else { s0 };
    sim.wake_all();
    let wall = Instant::now();
    let mut last = Instant::now();
    let mut sub0 = sim.substeps;
    let mut tl = 0.0;
    let report = (water_time / 12.0).max(60.0);
    while sim.t < water_time {
        sim.step(1.0);
        if sim.t - tl >= report {
            if let Some(p) = args.get(6) { sim.save(p); }
            let el = last.elapsed().as_secs_f64();
            sim.totals();
            let led = sim.brought - sim.taken - (sim.stored() - s0) - sim.owed_total() + sim.owed_lost;
            println!(
                "B. t {:>6.0} s: awake {:>6} / {} chunks; {:>5.1} substeps a water second; wall {:>6.2} s for {:.0} water s ({:>7.1}x real time); in {:.0} out {:.0} m3; ledger {:+.2} m3; wakes {} sleeps {}",
                sim.t, sim.awake_count(), sim.nch, (sim.substeps - sub0) as f64 / (sim.t - tl), el, sim.t - tl, (sim.t - tl) / el,
                sim.brought, sim.taken, led, sim.wakes, sim.sleeps
            );
            println!("   {}", breakdown(&sim));
            tl = sim.t;
            sub0 = sim.substeps;
            last = Instant::now();
        }
    }
    println!("B. total wall {:.1} s for {:.0} water s", wall.elapsed().as_secs_f64(), sim.t);
    if let Some(p) = args.get(6) { sim.save(p); println!("saved {p} at t {:.0}", sim.t); }

    if args.get(7).is_some_and(|m| m == "spot") {
        let x: i32 = args[8].parse().unwrap(); let z: i32 = args[9].parse().unwrap();
        spot(&mut sim, x, z);
        return;
    }
    if args.get(7).is_some_and(|m| m == "probe") {
        probe(&mut sim);
        return;
    }
    if args.get(7).is_some_and(|m| m == "stuck") {
        let secs: f64 = args.get(8).and_then(|v| v.parse().ok()).unwrap_or(300.0);
        stuck(&mut sim, secs);
        return;
    }
    if args.get(7).is_some_and(|m| m == "who") {
        who(&mut sim);
        return;
    }
    if args.get(7).is_some_and(|m| m == "short") {
        let secs: f64 = args.get(8).and_then(|v| v.parse().ok()).unwrap_or(120.0);
        short(&mut sim, secs);
        return;
    }
    if args.get(7).is_some_and(|m| m == "diag") {
        diag(&mut sim);
        return;
    }
    if args.get(7).is_some_and(|m| m == "edit") {
        edit_bench(&mut sim, seed);
        return;
    }
    // C. settled cost: 100 water seconds
    let t = Instant::now();
    let tt = sim.t;
    let sb = sim.substeps;
    while sim.t < tt + 100.0 { sim.step(1.0); }
    let el = t.elapsed().as_secs_f64();
    println!(
        "C. now: {:.2} ms per water second ({:.0}x real time), {:.1} substeps a water second, awake {}",
        el / (sim.t - tt) * 1e3, (sim.t - tt) / el, (sim.substeps - sb) as f64 / (sim.t - tt), sim.awake_count()
    );
}

fn run_for(sim: &mut Sim, secs: f64, every: f64, label: &str) -> f64 {
    let start = sim.t;
    let wall = Instant::now();
    let mut lt = sim.t;
    let mut lw = Instant::now();
    let mut sb = sim.substeps;
    let mut cs = sim.col_steps;
    while sim.t < start + secs {
        sim.step(1.0);
        if sim.t - lt >= every {
            let el = lw.elapsed().as_secs_f64();
            println!(
                "{label} +{:>5.0} s: awake {:>6} chunks; {:>8.2} ms a water second ({:>7.1}x real time); {:>4.1} substeps a water second; {:.1} M column-substeps a water second; wakes {} sleeps {}",
                sim.t - start, sim.awake_count(), el / (sim.t - lt) * 1e3, (sim.t - lt) / el, (sim.substeps - sb) as f64 / (sim.t - lt),
                (sim.col_steps - cs) as f64 / (sim.t - lt) / 1e6, sim.wakes, sim.sleeps
            );
            println!("   {}", breakdown(sim));
            // per-chunk time steps: what each awake chunk needs vs the world's deepest
            let need = |c: usize| (0.7 / (G * sim.dmax[c].max(0.01)).sqrt()).recip();
            let mut world = 0.0f32; let mut sum = 0.0f64; let mut n = 0usize;
            for c in 0..sim.nch { if sim.awake[c] != 0 { world = world.max(need(c)); sum += need(c) as f64; n += 1; } }
            if n > 0 { println!("   substeps: the world's {:.1}/s (deepest awake {:.2} m); per-chunk steps would average {:.1}/s: {:.0}% of the work", world, (world * 0.7).powi(2) / G, sum / n as f64, sum / n as f64 / world as f64 * 100.0); }
            lt = sim.t; lw = Instant::now(); sb = sim.substeps; cs = sim.col_steps;
        }
    }
    wall.elapsed().as_secs_f64()
}

fn edit_bench(sim: &mut Sim, seed: u64) {
    let m = worldmap::for_seed(seed);
    // 1. settled
    let w = run_for(sim, 300.0, 100.0, "settled");
    println!("1. settled: {:.2} ms a water second over 300 s", w / 300.0 * 1e3);
    // 2. a dam across the biggest river at mid-course: its top at the river's surface (½ m steps)
    let (ri, _) = m.rivers.iter().enumerate().max_by(|a, b| {
        let qa = a.1.path.last().map_or(0.0, |v| v.discharge);
        let qb = b.1.path.last().map_or(0.0, |v| v.discharge);
        qa.partial_cmp(&qb).unwrap()
    }).unwrap();
    let p = &m.rivers[ri].path;
    let k = p.len() / 2;
    let (a, b) = (p[k], p[k + 1]);
    let (dx, dz) = (b.x - a.x, b.z - a.z);
    let l = (dx * dx + dz * dz).sqrt().max(1e-3);
    let (nx, nz) = (-dz / l, dx / l);
    let hw = 0.5 * a.width + BANK_W + 1.0;
    let top = q5(a.surface) + 0.5;
    let mut cols = 0;
    let mut chunks = Vec::new();
    for s in -((hw * 4.0) as i32)..=((hw * 4.0) as i32) {
        for t in 0..3 {
            let (x, z) = ((a.x + nx * s as f32 / 4.0 + dx / l * t as f32).floor() as i32, (a.z + nz * s as f32 / 4.0 + dz / l * t as f32).floor() as i32);
            let key = (x.div_euclid(CS as i32), z.div_euclid(CS as i32));
            if let Some(c) = sim.key.iter().position(|&kk| kk == key) {
                let i = c * CA + (z.rem_euclid(CS as i32) as usize) * CS + x.rem_euclid(CS as i32) as usize;
                if sim.ground[i] < VOID && sim.ground[i] < top {
                    // raise the ground to the dam's top; the water on it is lifted with it (kept)
                    sim.ground[i] = top;
                    cols += 1;
                    chunks.push(c);
                }
            }
        }
    }
    chunks.sort(); chunks.dedup();
    for &c in &chunks { sim.wake_around(c); }
    println!("2. dam on river {ri} ({:.1} m3/s, {:.1} m wide) at ({:.0}, {:.0}): {cols} columns raised to {top} m", p.last().unwrap().discharge, a.width, a.x, a.z);
    let w = run_for(sim, 1800.0, 120.0, "dam");
    println!("2. the dam's 1800 s: {:.2} ms a water second", w / 1800.0 * 1e3);
    // 3. a flood everywhere: every spring doubled
    for v in sim.sources.iter_mut() { for s in v.iter_mut() { s.1 *= 2.0; } }
    sim.rebuild_inflow();
    sim.wake_all();
    let w = run_for(sim, 600.0, 120.0, "flood");
    println!("3. flood 600 s: {:.2} ms a water second", w / 600.0 * 1e3);
}

/// What keeps chunks awake: over 120 s with everything stepped, each wet column's swing (max − min)
/// and its drift (end − start), and each chunk's fastest level change against looser thresholds.
fn diag(sim: &mut Sim) {
    sim.sleep = false;
    sim.wake_all();
    let n = sim.nch * CA;
    let start = sim.depth.clone();
    let mut lo = start.clone();
    let mut hi = start.clone();
    let mut divmax = vec![0.0f32; sim.nch];
    let t0 = sim.t;
    while sim.t < t0 + 120.0 {
        sim.step(1.0);
        for i in 0..n {
            let d = sim.depth[i];
            if d < lo[i] { lo[i] = d; }
            if d > hi[i] { hi[i] = d; }
        }
        for c in 0..sim.nch { divmax[c] = divmax[c].max(sim.div[c]); }
    }
    let mut swing = Vec::new();
    let mut drift = Vec::new();
    for i in 0..n {
        if start[i] > 0.01 {
            swing.push(hi[i] - lo[i]);
            drift.push((sim.depth[i] - start[i]).abs());
        }
    }
    let pct = |v: &mut Vec<f32>, p: f64| { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[((v.len() - 1) as f64 * p) as usize] };
    let (mut s2, mut d2) = (swing.clone(), drift.clone());
    println!("diag over 120 s, {} wet columns:", swing.len());
    for p in [0.5, 0.9, 0.99, 0.999] {
        println!("  p{:>5}: swing {:>8.3} mm, drift {:>8.3} mm", p * 100.0, pct(&mut s2, p) * 1000.0, pct(&mut d2, p) * 1000.0);
    }
    // the biggest swings: where, how deep, and the ground around them
    let mut idx: Vec<usize> = (0..n).filter(|&i| start[i] > 0.01).collect();
    idx.sort_by(|&a, &b| (hi[b] - lo[b]).partial_cmp(&(hi[a] - lo[a])).unwrap());
    let col = |i: usize| { let (c, l) = (i / CA, i % CA); let (cx, cz) = sim.key[c]; (cx * CS as i32 + (l % CS) as i32, cz * CS as i32 + (l / CS) as i32) };
    for &i in idx.iter().take(6) {
        let (x, z) = col(i);
        let c = i / CA; let l = i % CA; let (lx, lz) = (l % CS, l / CS);
        let g = |dx: i32, dz: i32| -> String {
            let (nx, nz) = (lx as i32 + dx, lz as i32 + dz);
            if nx < 0 || nz < 0 || nx >= CS as i32 || nz >= CS as i32 { return "  edge".into(); }
            let j = c * CA + nz as usize * CS + nx as usize;
            if sim.ground[j] >= VOID { "  wall".into() } else { format!("{:6.1}", sim.ground[j]) }
        };
        println!("  swing {:.2} m at ({x}, {z}): ground {:.1}, depth {:.2} → min {:.2} max {:.2} now {:.2}; ground E {} W {} S {} N {}; spring {:.2} m3/s",
            hi[i] - lo[i], sim.ground[i], start[i], lo[i], hi[i], sim.depth[i], g(1, 0), g(-1, 0), g(0, 1), g(0, -1), sim.inflow[i]);
    }
    // swing against the local bed slope: the steepest drop to a neighbour
    let mut by = [(0usize, 0.0f64); 4];
    for &i in &idx {
        let c = i / CA; let l = i % CA; let (lx, lz) = (l % CS, l / CS);
        let mut drop = 0.0f32;
        for (dx, dz) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
            let (nx, nz) = (lx as i32 + dx, lz as i32 + dz);
            if nx < 0 || nz < 0 || nx >= CS as i32 || nz >= CS as i32 { continue; }
            let j = c * CA + nz as usize * CS + nx as usize;
            if sim.ground[j] < VOID { drop = drop.max((sim.ground[i] - sim.ground[j]).abs()); }
        }
        let k = if drop == 0.0 { 0 } else if drop <= 0.5 { 1 } else if drop <= 1.0 { 2 } else { 3 };
        by[k].0 += 1; by[k].1 += (hi[i] - lo[i]) as f64;
    }
    for (k, name) in ["flat", "a 1/2 m step", "a 1 m step", "a bigger drop"].iter().enumerate() {
        if by[k].0 > 0 { println!("  columns beside {name}: {} , mean swing {:.0} mm", by[k].0, by[k].1 / by[k].0 as f64 * 1000.0); }
    }
    // swing against distance to the nearest outlet column
    let mut sinkxy = Vec::new();
    for c in 0..sim.nch { for &l in &sim.sinks[c] { sinkxy.push(col(c * CA + l as usize)); } }
    let mut bins = [(0usize, 0.0f64); 6];
    for &i in idx.iter().step_by(7) {
        let (x, z) = col(i);
        let d = sinkxy.iter().map(|&(sx, sz)| (((sx - x) as f64).powi(2) + ((sz - z) as f64).powi(2)).sqrt()).fold(f64::INFINITY, f64::min);
        let k = if d < 100.0 { 0 } else if d < 500.0 { 1 } else if d < 2000.0 { 2 } else if d < 5000.0 { 3 } else if d < 15000.0 { 4 } else { 5 };
        bins[k].0 += 1; bins[k].1 += (hi[i] - lo[i]) as f64;
    }
    for (k, name) in ["< 100 m", "100-500 m", "0.5-2 km", "2-5 km", "5-15 km", "> 15 km"].iter().enumerate() {
        if bins[k].0 > 0 { println!("  {name} from an outlet: {} sampled columns, mean swing {:.0} mm", bins[k].0, bins[k].1 / bins[k].0 as f64 * 1000.0); }
    }
    let info = unsafe { &*std::ptr::addr_of!(INFO) };
    let mut sb = [(0usize, 0.0f64); 5];
    let mut wb = [(0usize, 0.0f64); 4];
    for &i in &idx {
        let (sl, w, _) = info[i];
        if w == 0.0 { continue; }
        let k = if sl < 0.0005 { 0 } else if sl < 0.002 { 1 } else if sl < 0.01 { 2 } else if sl < 0.03 { 3 } else { 4 };
        sb[k].0 += 1; sb[k].1 += (hi[i] - lo[i]) as f64;
        let k = if w < 4.0 { 0 } else if w < 8.0 { 1 } else if w < 15.0 { 2 } else { 3 };
        wb[k].0 += 1; wb[k].1 += (hi[i] - lo[i]) as f64;
    }
    for (k, name) in ["< 0.05%", "0.05-0.2%", "0.2-1%", "1-3%", "> 3%"].iter().enumerate() {
        if sb[k].0 > 0 { println!("  map slope {name}: {} columns, mean swing {:.0} mm", sb[k].0, sb[k].1 / sb[k].0 as f64 * 1000.0); }
    }
    for (k, name) in ["< 4 m", "4-8 m", "8-15 m", "> 15 m"].iter().enumerate() {
        if wb[k].0 > 0 { println!("  width {name}: {} columns, mean swing {:.0} mm", wb[k].0, wb[k].1 / wb[k].0 as f64 * 1000.0); }
    }
    let kind = unsafe { &*std::ptr::addr_of!(KIND) };
    let wetc: Vec<usize> = (0..sim.nch).filter(|&c| kind[c] != 0).collect();
    for th in [1e-5f32, 1e-4, 1e-3, 1e-2] {
        let pass = wetc.iter().filter(|&&c| divmax[c] < th).count();
        println!("  chunks whose fastest level change stays under {:.0e} m/s: {} / {} wet chunks", th, pass, wetc.len());
    }
}

fn spot(sim: &mut Sim, x0: i32, z0: i32) {
    sim.sleep = false;
    sim.wake_all();
    let find = |sim: &Sim, x: i32, z: i32| -> Option<usize> {
        let key = (x.div_euclid(CS as i32), z.div_euclid(CS as i32));
        let c = sim.key.iter().position(|&k| k == key)?;
        Some(c * CA + (z.rem_euclid(CS as i32) as usize) * CS + x.rem_euclid(CS as i32) as usize)
    };
    let r = 12;
    println!("ground around ({x0}, {z0}) (x across, z down; ## wall):");
    for z in z0 - r..=z0 + r {
        let mut line = String::new();
        for x in x0 - r..=x0 + r {
            match find(sim, x, z) {
                Some(i) if sim.ground[i] < VOID => line += &format!("{:5.1}", sim.ground[i]),
                _ => line += "   ##",
            }
        }
        println!("{line}");
    }
    let me = find(sim, x0, z0).unwrap();
    let mut springs = 0.0;
    for z in z0 - r..=z0 + r { for x in x0 - r..=x0 + r { if let Some(i) = find(sim, x, z) { springs += sim.inflow[i]; } } }
    println!("springs within {r} m: {springs:.2} m3/s");
    for k in 0..60 {
        let mut tot = 0.0f64; let mut mx = 0.0f32;
        for z in z0 - r..=z0 + r { for x in x0 - r..=x0 + r { if let Some(i) = find(sim, x, z) { tot += sim.depth[i] as f64; mx = mx.max(sim.depth[i]); } } }
        if k % 2 == 0 { println!("t+{k:>3}: depth here {:.2} (qx {:+.3} qz {:+.3}); patch volume {:.1} m3, deepest {:.2}", sim.depth[me], sim.qx[me], sim.qz[me], tot, mx); }
        sim.step(1.0);
    }
}

/// The game's own terrain for face A (seed `seed`): the generator's height field with the terrain's
/// volume, its columns' ground, water level and kind, in every chunk near a river course or a lake;
/// sea columns are outlets; inflow at river heads and as the map's discharge grows along them.
fn build_real(seed: u64) -> (Sim, usize) {
    let t0 = Instant::now();
    let mut tg = TerrainGenerator::new(seed);
    if let Some(def) = dwell_worldgen::volume::terrain_volume() { tg = tg.with_volume(def); }
    let m = worldmap::for_seed(seed);
    // candidate chunks: within 24 m of a river course, or over a lake cell
    let mut cand = std::collections::HashSet::new();
    for r in &m.rivers {
        for w in r.path.windows(2) {
            let (a, b) = (w[0], w[1]);
            let reach = 0.5 * a.width.max(b.width) + 24.0;
            let x0 = ((a.x.min(b.x) - reach) / CS as f32).floor() as i32;
            let x1 = ((a.x.max(b.x) + reach) / CS as f32).floor() as i32;
            let z0 = ((a.z.min(b.z) - reach) / CS as f32).floor() as i32;
            let z1 = ((a.z.max(b.z) + reach) / CS as f32).floor() as i32;
            let (dx, dz) = (b.x - a.x, b.z - a.z);
            let len2 = (dx * dx + dz * dz).max(1e-6);
            for cz in z0..=z1 { for cx in x0..=x1 {
                let (px, pz) = ((cx as f32 + 0.5) * CS as f32, (cz as f32 + 0.5) * CS as f32);
                let t = (((px - a.x) * dx + (pz - a.z) * dz) / len2).clamp(0.0, 1.0);
                let d = (px - a.x - dx * t).hypot(pz - a.z - dz * t);
                if d <= reach + CS as f32 { cand.insert((cx, cz)); }
            } }
        }
    }
    for j in 0..m.n { for i in 0..m.n {
        if m.lake[j * m.n + i] == NO_LAKE { continue; }
        let (cx, cz) = (m.x0 + i as f64 * m.cell, m.x0 + j as f64 * m.cell);
        let h = m.cell / 2.0 + 16.0;
        for kz in ((cz - h) / CS as f64).floor() as i32..=((cz + h) / CS as f64).floor() as i32 {
            for kx in ((cx - h) / CS as f64).floor() as i32..=((cx + h) / CS as f64).floor() as i32 { cand.insert((kx, kz)); }
        }
    } }
    let mut cand: Vec<(i32, i32)> = cand.into_iter().collect();
    // a window of the real world (REGION=x0,z0,x1,z1 in m): only its chunks
    let region: Option<[f32; 4]> = std::env::var("REGION").ok().map(|v| { let p: Vec<f32> = v.split(',').map(|t| t.parse().unwrap()).collect(); [p[0], p[1], p[2], p[3]] });
    let inside = |x: f32, z: f32| region.map_or(true, |r| x >= r[0] && x < r[2] && z >= r[1] && z < r[3]);
    if let Some(r) = region {
        cand.retain(|&(cx, cz)| { let (x, z) = (cx as f32 * CS as f32, cz as f32 * CS as f32); x >= r[0] && x + CS as f32 <= r[2] && z >= r[1] && z + CS as f32 <= r[3] });
    }
    cand.sort_unstable();
    // generate them
    let cols: Vec<Vec<(f32, f32, u8)>> = cand.par_iter().map(|&(cx, cz)| {
        let mut v = Vec::with_capacity(CA);
        for lz in 0..CS as i32 { for lx in 0..CS as i32 {
            let c = tg.column_at(cx * CS as i32 + lx, cz * CS as i32 + lz);
            let k = if c.outside { 9 } else { match c.water { Water::None => 0, Water::River => 1, Water::Lake => 2, Water::Sea => 3 } };
            v.push((c.height, if k == 1 || k == 2 || k == 3 { (c.level - c.height).max(0.0) } else { 0.0 }, k));
        } }
        v
    }).collect();
    let gen_s = t0.elapsed().as_secs_f64();
    // keep chunks with water, and their neighbours (the land water can spill onto)
    let wet: std::collections::HashSet<(i32, i32)> = cand.iter().zip(&cols).filter(|(_, v)| v.iter().any(|&(_, d, k)| d > 0.0 && k != 9)).map(|(&c, _)| c).collect();
    let keep: Vec<usize> = (0..cand.len()).filter(|&i| {
        let (cx, cz) = cand[i];
        (-1..=1).any(|dz| (-1..=1).any(|dx| wet.contains(&(cx + dx, cz + dz))))
    }).collect();
    let keys: Vec<(i32, i32)> = keep.iter().map(|&i| cand[i]).collect();
    let mut sim = Sim::new(keys.clone());
    let mut kind = vec![0u8; sim.nch];
    let mut wetn = 0;
    let (mut nriver, mut nlake, mut nsea) = (0, 0, 0);
    for (c, &i) in keep.iter().enumerate() {
        for (l, &(g, d, k)) in cols[i].iter().enumerate() {
            if k == 9 { continue; } // outside the disc: a wall
            let idx = c * CA + l;
            sim.ground[idx] = g;
            if k == 3 {
                sim.depth[idx] = if sim.hold_sea { d } else { 0.0 };
                sim.hold[idx] = d;
                sim.sinks[c].push(l as u16);
                nsea += 1;
            } else if d > 0.0 {
                sim.depth[idx] = d;
                wetn += 1;
                if k == 2 { nlake += 1; kind[c] = 2; } else { nriver += 1; if kind[c] == 0 { kind[c] = 1; } }
            }
        }
    }
    unsafe { KIND = kind; INFO = vec![(0.0, 0.0, 0.0); sim.nch * CA]; }
    // inflow: each river's head (rivers out of a lake get theirs from it), and the map's growth
    let idx: HashMap<(i32, i32), usize> = keys.iter().enumerate().map(|(i, &k)| (k, i)).collect();
    let find_wet = |sim: &Sim, x: i32, z: i32| -> Option<(usize, usize)> {
        for rad in 0i32..=48 { for dz in -rad..=rad { for dx in -rad..=rad {
            if dz.abs() != rad && dx.abs() != rad { continue; } // the ring at this radius only
            let (px, pz) = (x + dx, z + dz);
            if let Some(&c) = idx.get(&(px.div_euclid(CS as i32), pz.div_euclid(CS as i32))) {
                let l = (pz.rem_euclid(CS as i32) as usize) * CS + px.rem_euclid(CS as i32) as usize;
                if sim.depth[c * CA + l] > 0.0 && unsafe { (&*std::ptr::addr_of!(KIND))[c] } == 1 { return Some((c, l)); }
            }
        } } }
        None
    };
    let (mut placed, mut missed) = (0, 0);
    let mut entries = 0;
    let mut exits: Vec<(f32, f32)> = Vec::new();
    for r in &m.rivers {
        let p = &r.path;
        let mut add = |sim: &mut Sim, x: f32, z: f32, q: f32| match find_wet(sim, x as i32, z as i32) {
            Some((c, l)) => { sim.sources[c].push((l as u16, q)); placed += 1; }
            None => missed += 1,
        };
        if r.from_lake == NO_LAKE && inside(p[0].x, p[0].z) { add(&mut sim, p[0].x, p[0].z, p[0].discharge.max(0.05)); }
        for k in 1..p.len() {
            let (was, is) = (inside(p[k - 1].x, p[k - 1].z), inside(p[k].x, p[k].z));
            if is && !was {
                // a river entering the window: its discharge there, a little inside
                add(&mut sim, p[k].x, p[k].z, p[k].discharge.max(0.05));
                entries += 1;
            } else if is {
                let inc = p[k].discharge - p[k - 1].discharge;
                if inc > 0.0 && inc < 1.0 { add(&mut sim, p[k].x, p[k].z, inc); }
            } else if was && !is {
                exits.push((p[k - 1].x, p[k - 1].z));
            }
        }
    }
    // a river leaving the window: its wet columns on the window's edge near the crossing hold their
    // generated level (an outlet like the sea)
    let mut held = 0;
    if let Some(rg0) = region {
        // the kept chunks' own extent (the window rounded in to whole chunks)
        let rg = [keys.iter().map(|k| k.0).min().unwrap() as f32 * CS as f32, keys.iter().map(|k| k.1).min().unwrap() as f32 * CS as f32,
                  (keys.iter().map(|k| k.0).max().unwrap() + 1) as f32 * CS as f32, (keys.iter().map(|k| k.1).max().unwrap() + 1) as f32 * CS as f32];
        let _ = rg0;
        for c in 0..sim.nch {
            let (cx, cz) = keys[c];
            for l in 0..CA {
                let i = c * CA + l;
                if sim.depth[i] <= 0.0 || sim.ground[i] >= VOID { continue; }
                let (x, z) = ((cx * CS as i32) as f32 + (l % CS) as f32, (cz * CS as i32) as f32 + (l / CS) as f32);
                let edge = x < rg[0] + 2.0 || z < rg[1] + 2.0 || x >= rg[2] - 2.0 || z >= rg[3] - 2.0;
                if edge && exits.iter().any(|&(ex, ez)| (ex - x).hypot(ez - z) < 160.0) {
                    sim.sinks[c].push(l as u16);
                    sim.hold[i] = sim.depth[i];
                    held += 1;
                }
            }
        }
        println!("window {:?}: {entries} rivers enter, {} leave ({held} edge columns held at their level)", rg, exits.len());
    }
    sim.rebuild_inflow();
    println!("real terrain: generated {} candidate chunks in {:.1} s; kept {} ({} river, {} lake, {} sea columns); springs placed {placed}, missed {missed}; total inflow {:.1} m3/s",
        cand.len(), gen_s, sim.nch, nriver, nlake, nsea, sim.src_total);
    (sim, wetn)
}

/// What keeps rivers moving: six river columns (three typical, three of the worst) followed for 60 s
/// with everything stepped: their level each second, the swing's period, and the ground around them.
fn probe(sim: &mut Sim) {
    sim.sleep = false;
    sim.wake_all();
    let kind = unsafe { &*std::ptr::addr_of!(KIND) };
    let n = sim.nch * CA;
    // 20 s to rank columns by swing
    let start = sim.depth.clone();
    let (mut lo, mut hi) = (start.clone(), start.clone());
    let t0 = sim.t;
    while sim.t < t0 + 20.0 { sim.step(1.0); for i in 0..n { lo[i] = lo[i].min(sim.depth[i]); hi[i] = hi[i].max(sim.depth[i]); } }
    let mut riv: Vec<usize> = (0..n).filter(|&i| kind[i / CA] == 1 && start[i] > 0.05).collect();
    riv.sort_by(|&a, &b| (hi[a] - lo[a]).partial_cmp(&(hi[b] - lo[b])).unwrap());
    let len = riv.len();
    let pick = [riv[len / 2], riv[len * 3 / 4], riv[len * 9 / 10], riv[len * 99 / 100], riv[len * 999 / 1000], riv[len - 1]];
    let names = ["median", "p75", "p90", "p99", "p99.9", "worst"];
    let mut series: Vec<Vec<f32>> = vec![Vec::new(); pick.len()];
    let mut flow: Vec<(f32, f32)> = vec![(0.0, 0.0); pick.len()];
    for _ in 0..60 {
        sim.step(1.0);
        for (k, &i) in pick.iter().enumerate() { series[k].push(sim.ground[i] + sim.depth[i]); flow[k].0 += sim.qx[i] / 60.0; flow[k].1 += sim.qz[i] / 60.0; }
    }
    let col = |i: usize| { let (c, l) = (i / CA, i % CA); let (cx, cz) = sim.key[c]; (cx * CS as i32 + (l % CS) as i32, cz * CS as i32 + (l / CS) as i32) };
    let find = |x: i32, z: i32| -> Option<usize> {
        let key = (x.div_euclid(CS as i32), z.div_euclid(CS as i32));
        let c = sim.key.binary_search(&key).ok()?;
        Some(c * CA + (z.rem_euclid(CS as i32) as usize) * CS + x.rem_euclid(CS as i32) as usize)
    };
    for (k, &i) in pick.iter().enumerate() {
        let s = &series[k];
        let mean = s.iter().sum::<f32>() / s.len() as f32;
        let crossings = s.windows(2).filter(|w| (w[0] - mean) * (w[1] - mean) < 0.0).count();
        let (mn, mx) = s.iter().fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
        let (x, z) = col(i);
        let (fx, fz) = flow[k];
        let dir = fz.atan2(fx).to_degrees();
        println!("{} column ({x}, {z}): depth {:.2} m, level swings {:.1} cm over 60 s, {} mean crossings (period ~{:.0} s); mean flow ({:+.2}, {:+.2}) m2/s, heading {:.0} deg",
            names[k], sim.depth[i], (mx - mn) * 100.0, crossings, if crossings > 1 { 120.0 / crossings as f32 } else { f32::NAN }, fx, fz, dir);
        println!("   level, cm from the mean, each 5 s: {}", s.iter().step_by(5).map(|v| format!("{:+.1}", (v - mean) * 100.0)).collect::<Vec<_>>().join(" "));
        println!("   ground (water level {:.2}); * water, ## outside:", mean);
        for dz in -4..=4 {
            let mut line = String::from("     ");
            for dx in -6..=6 {
                line += &match find(x + dx, z + dz) {
                    Some(j) if sim.ground[j] < VOID => format!("{:>6.1}{}", sim.ground[j], if sim.depth[j] > 0.01 { "*" } else { " " }),
                    _ => "    ## ".into(),
                };
            }
            println!("{line}");
        }
    }
}

/// A short run from a saved state: awake chunks over time, cost, and how far the levels end from the
/// same run with everything stepped (the band's error).
fn short(sim: &mut Sim, secs: f64) {
    let kind = unsafe { &*std::ptr::addr_of!(KIND) };
    let n = sim.nch * CA;
    let (d0, qx0, qz0, t0) = (sim.depth.clone(), sim.qx.clone(), sim.qz.clone(), sim.t);
    let mut out = Vec::new();
    let refp = format!("ref_{}{}_{}_{}.bin", secs as i64, if sim.hold_sea { "_sea" } else { "" }, sim.nch, sim.theta);
    for (label, sleep) in [("all stepped", false), ("sleeping", true)] {
        if !sleep {
            if let Ok(b) = std::fs::read(&refp) {
                let v: Vec<f32> = b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
                println!("all stepped: cached ({refp})");
                out.push(v);
                continue;
            }
        }
        sim.depth.copy_from_slice(&d0); sim.qx.copy_from_slice(&qx0); sim.qz.copy_from_slice(&qz0); sim.t = t0;
        sim.ex.copy_from_slice(&qx0); sim.ez.copy_from_slice(&qz0); sim.ed.copy_from_slice(&d0);
        sim.rel_left.iter_mut().for_each(|v| *v = 0.0);
        sim.owed.iter_mut().for_each(|v| *v = 0.0); sim.rate.iter_mut().for_each(|v| *v = 0.0);
        sim.awake.iter_mut().for_each(|a| *a = 1); sim.calm.iter_mut().for_each(|c| *c = 0); sim.dirty = true;
        sim.sleep = sleep;
        // the reference steps everything, with no pools; each run starts with none
        let pools_wanted = std::env::var("POOLS").is_ok();
        sim.pools.list.clear(); sim.rebuild_pools(); sim.pools.clock = 0.0;
        sim.pools.on = sleep && pools_wanted;
        let (w0, s0) = (sim.wakes, sim.sleeps);
        let wall = std::time::Instant::now();
        let mut tl = Vec::new();
        // TRACK="x,z;x,z": those columns' depth every 15 s, and their chunk's state (A awake, s asleep, P pooled)
        let track: Vec<(i32, i32)> = std::env::var("TRACK").map(|v| v.split(';').map(|p| { let t: Vec<i32> = p.split(',').map(|n| n.parse().unwrap()).collect(); (t[0], t[1]) }).collect()).unwrap_or_default();
        let find = |sim: &Sim, x: i32, z: i32| -> usize { let c = sim.key.binary_search(&(x.div_euclid(CS as i32), z.div_euclid(CS as i32))).unwrap(); c * CA + (z.rem_euclid(CS as i32) as usize) * CS + x.rem_euclid(CS as i32) as usize };
        let mut trace: Vec<String> = vec![String::new(); track.len()];
        let mut last = -1i64;
        while sim.t < t0 + secs {
            sim.step(1.0);
            let el = (sim.t - t0).round() as i64;
            if el % 20 == 0 { tl.push(sim.awake_count()); }
            if el % 15 == 0 && el != last {
                last = el;
                for (k, &(x, z)) in track.iter().enumerate() {
                    let i = find(sim, x, z); let c = i / CA;
                    let st = if sim.pools.pooled[c] == 1 { 'P' } else if sim.awake[c] != 0 { 'A' } else { 's' };
                    trace[k] += &format!(" {:.2}{}", sim.depth[i], st);
                }
            }
        }
        if let Some(&(x0, z0)) = track.first() {
            println!("   {label} around ({x0}, {z0}): level (ground+depth) where wet, ground in brackets where dry");
            for dz in -4..=4 {
                let mut line = String::from("     ");
                for dx in -7..=7 {
                    let ok = sim.key.binary_search(&((x0 + dx).div_euclid(CS as i32), (z0 + dz).div_euclid(CS as i32))).is_ok();
                    if !ok { line += "      ##  "; continue; }
                    let j = find(sim, x0 + dx, z0 + dz);
                    line += &if sim.ground[j] >= VOID { "      ##  ".to_string() } else if sim.depth[j] > 0.005 { format!(" {:8.3} ", sim.ground[j] + sim.depth[j]) } else { format!("({:7.2})", sim.ground[j]) };
                }
                println!("{line}");
            }
        }
        for (k, &(x, z)) in track.iter().enumerate() {
            let i = find(sim, x, z);
            let nb: Vec<String> = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().map(|&(dx, dz)| { let j = find(sim, x + dx, z + dz); format!("{:.1}/{:.2}", sim.ground[j], sim.depth[j]) }).collect();
            println!("   {label} ({x}, {z}) ground {:.1}, pool {}: every 15 s{}; neighbours ground/depth E W S N {}", sim.ground[i], sim.pools.of[i], trace[k], nb.join(" "));
        }
        let el = wall.elapsed().as_secs_f64();
        if sleep { let tm = sim.pools.tm; println!("   pools: {} ({} columns, {} faces, {} inside faces kept 0); time: detect {:.2} s, zero {:.2} s, limit {:.2} s, update {:.2} s", sim.pools.list.len(), sim.pools.list.iter().map(|p| p.members.len()).sum::<usize>(), sim.pools.faces.len(), sim.pools.inside.len(), tm[0], tm[1], tm[2], tm[3]); }
        if sleep { println!("   wakes by a shared face jumping {}, by owed water reaching the band {}, by the neighbours' flow drifting {}", sim.wake_face, sim.wake_owed, sim.wake_rate); }
        // owed water back into the columns before comparing
        let asleep: Vec<usize> = (0..sim.nch).filter(|&c| sim.awake[c] == 0).collect();
        let still = asleep.len();
        let riv_awake = (0..sim.nch).filter(|&c| sim.awake[c] != 0 && kind[c] == 1).count();
        for c in asleep { sim.wake(c); }
        println!("{label}: {:.2} s wall for {secs:.0} water s ({:.1}x real time); awake every 20 s {:?}; at the end {} asleep, {} river chunks awake; wakes {} sleeps {}",
            el, secs / el, tl, still, riv_awake, sim.wakes - w0, sim.sleeps - s0);
        if !sleep { let b: Vec<u8> = sim.depth.iter().flat_map(|v| v.to_le_bytes()).collect(); std::fs::write(&refp, b).unwrap(); }
        out.push(sim.depth.clone());
    }
    let mut diffs: Vec<f32> = (0..n).filter(|&i| out[0][i] > 0.01 || out[1][i] > 0.01).map(|i| (out[0][i] - out[1][i]).abs()).collect();
    diffs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p = |f: f64| diffs[((diffs.len() - 1) as f64 * f) as usize] * 100.0;
    // where the big differences are: depth there, chunk kind, and how fast the water runs
    let mut big: Vec<usize> = (0..n).filter(|&i| (out[0][i] - out[1][i]).abs() > 0.1).collect();
    big.sort_by(|&a, &b| (out[1][b] - out[0][b]).abs().partial_cmp(&(out[1][a] - out[0][a]).abs()).unwrap());
    for &i in big.iter().take(5) {
        let (c, l) = (i / CA, i % CA); let (cx, cz) = sim.key[c];
        let sp = (sim.qx[i].powi(2) + sim.qz[i].powi(2)).sqrt() / out[0][i].max(0.05);
        println!("   off {:.1} cm at ({}, {}): depth stepped {:.2}, sleeping {:.2}; chunk kind {}; speed {:.2} m/s",
            (out[1][i] - out[0][i]) * 100.0, cx * CS as i32 + (l % CS) as i32, cz * CS as i32 + (l / CS) as i32, out[0][i], out[1][i], kind[c], sp);
    }
    println!("levels, sleeping vs all stepped: p50 {:.2} cm, p90 {:.2} cm, p99 {:.2} cm, p99.9 {:.1} cm, max {:.1} cm; over 10 cm: {} columns", p(0.5), p(0.9), p(0.99), p(0.999), diffs.last().unwrap() * 100.0, diffs.iter().filter(|&&d| d > 0.1).count());
}

/// Which chunks keep waking, and should they? 60 s with everything stepped (each chunk's real
/// swing, fastest water), then 60 s sleeping (each chunk's wakes by reason, and time awake).
fn who(sim: &mut Sim) {
    let kind = unsafe { &*std::ptr::addr_of!(KIND) };
    let n = sim.nch * CA;
    let band = unsafe { sim::BAND };
    let (d0, qx0, qz0, t0) = (sim.depth.clone(), sim.qx.clone(), sim.qz.clone(), sim.t);
    // 1. stepped: per chunk, the largest level swing among its wet columns, and the fastest water
    sim.sleep = false; sim.wake_all();
    let (mut lo, mut hi) = (d0.clone(), d0.clone());
    let mut fast = vec![0.0f32; sim.nch];
    while sim.t < t0 + 60.0 {
        sim.step(1.0);
        for i in 0..n { let d = sim.depth[i]; if d < lo[i] { lo[i] = d; } if d > hi[i] { hi[i] = d; } }
        for c in 0..sim.nch { for l in 0..CA { let i = c * CA + l; if sim.depth[i] > 0.05 { let v = (sim.qx[i].powi(2) + sim.qz[i].powi(2)).sqrt() / sim.depth[i]; if v > fast[c] { fast[c] = v; } } } }
    }
    let swing: Vec<f32> = (0..sim.nch).map(|c| (0..CA).map(|l| { let i = c * CA + l; if d0[i] > 0.0 || hi[i] > 0.0 { hi[i] - lo[i] } else { 0.0 } }).fold(0.0, f32::max)).collect();
    let real_drift: Vec<f32> = (0..sim.nch).map(|c| (0..CA).map(|l| { let i = c * CA + l; (sim.depth[i] - d0[i]).abs() }).fold(0.0, f32::max)).collect();
    // 2. sleeping
    sim.depth.copy_from_slice(&d0); sim.qx.copy_from_slice(&qx0); sim.qz.copy_from_slice(&qz0); sim.t = t0;
    sim.ex.copy_from_slice(&qx0); sim.ez.copy_from_slice(&qz0);
    sim.rel_left.iter_mut().for_each(|v| *v = 0.0);
    sim.owed.iter_mut().for_each(|v| *v = 0.0); sim.rate.iter_mut().for_each(|v| *v = 0.0);
    sim.awake.iter_mut().for_each(|a| *a = 1); sim.calm.iter_mut().for_each(|c| *c = 0); sim.dirty = true;
    sim.sleep = true;
    // let it settle in for 20 s, then count over 60 s
    while sim.t < t0 + 20.0 { sim.step(1.0); }
    sim.why.iter_mut().for_each(|w| *w = [0; 4]);
    let s0 = sim.substeps;
    while sim.t < t0 + 80.0 { sim.step(1.0); }
    let subs = (sim.substeps - s0) as f64;
    let wet: Vec<usize> = (0..sim.nch).filter(|&c| kind[c] != 0 || d0[c * CA..c * CA + CA].iter().any(|&d| d > 0.0)).collect();
    let wakes = |c: usize| sim.why[c][0] + sim.why[c][1] + sim.why[c][2];
    let awake_frac = |c: usize| sim.why[c][3] as f64 / subs;
    let mut busy: Vec<usize> = wet.iter().copied().filter(|&c| awake_frac(c) > 0.5).collect();
    busy.sort_by(|&a, &b| awake_frac(b).partial_cmp(&awake_frac(a)).unwrap());
    let total_wakes: u64 = wet.iter().map(|&c| wakes(c) as u64).sum();
    let mut by_w: Vec<usize> = wet.clone(); by_w.sort_by_key(|&c| std::cmp::Reverse(wakes(c)));
    let top = |f: f64| -> f64 { let k = (by_w.len() as f64 * f) as usize; by_w[..k].iter().map(|&c| wakes(c) as u64).sum::<u64>() as f64 / total_wakes.max(1) as f64 * 100.0 };
    println!("band {:.0} cm: {} wet chunks; over 60 s: {} wakes; the top 1% of chunks make {:.0}% of them, the top 10% {:.0}%", band * 100.0, wet.len(), total_wakes, top(0.01), top(0.1));
    println!("awake more than half the time: {} chunks", busy.len());
    let should = busy.iter().filter(|&&c| swing[c] > band).count();
    println!("  of them, {} really swing more than the band when everything is stepped (should be awake), {} don't", should, busy.len() - should);
    // what the ones that shouldn't be awake have
    let not: Vec<usize> = busy.iter().copied().filter(|&c| swing[c] <= band).collect();
    let has_sink = |c: usize| !sim.sinks[c].is_empty();
    let has_spring = |c: usize| sim.inflow[c * CA..c * CA + CA].iter().any(|&q| q > 0.0);
    let nb_sink = |c: usize| sim.nbr[c].iter().any(|&nn| nn != NONE && !sim.sinks[nn as usize].is_empty());
    let count = |v: &[usize], f: &dyn Fn(usize) -> bool| v.iter().filter(|&&c| f(c)).count();
    for (name, v) in [("should be awake", busy.iter().copied().filter(|&c| swing[c] > band).collect::<Vec<_>>()), ("shouldn't", not.clone())] {
        if v.is_empty() { continue; }
        let mean = |f: &dyn Fn(usize) -> f64| v.iter().map(|&c| f(c)).sum::<f64>() / v.len() as f64;
        println!("  {name} ({}): river {}, lake {}, other {}; with an outlet (sea) {}, beside one {}; with a spring {}; fastest water mean {:.2} m/s; real swing mean {:.1} cm, real drift over 60 s mean {:.1} cm; wakes by face/owed/drift {:.1}/{:.1}/{:.1} each",
            v.len(), count(&v, &|c| kind[c] == 1), count(&v, &|c| kind[c] == 2), count(&v, &|c| kind[c] == 0), count(&v, &has_sink), count(&v, &|c| !has_sink(c) && nb_sink(c)), count(&v, &has_spring),
            mean(&|c| fast[c] as f64), mean(&|c| swing[c] as f64 * 100.0), mean(&|c| real_drift[c] as f64 * 100.0),
            mean(&|c| sim.why[c][0] as f64), mean(&|c| sim.why[c][1] as f64), mean(&|c| sim.why[c][2] as f64));
    }
    println!("  ten that shouldn't be awake and have no outlet, the busiest:");
    for &c in not.iter().filter(|&&c| !has_sink(c)).take(10) {
        let (cx, cz) = sim.key[c];
        let wetc = d0[c * CA..c * CA + CA].iter().filter(|&&d| d > 0.0).count();
        println!("    chunk at ({}, {}): kind {}, {} wet columns, awake {:.0}%, wakes face/owed/drift {}/{}/{}, real swing {:.1} cm, fastest {:.2} m/s, sink {} spring {}",
            cx * CS as i32, cz * CS as i32, kind[c], wetc, awake_frac(c) * 100.0, sim.why[c][0], sim.why[c][1], sim.why[c][2], swing[c] * 100.0, fast[c], has_sink(c), has_spring(c));
    }
}

/// Why the chunks still awake after `secs` of sleeping aren't asleep.
fn stuck(sim: &mut Sim, secs: f64) {
    let band = unsafe { sim::BAND };
    sim.sleep = true;
    sim.pools.on = std::env::var("POOLS").is_ok();
    sim.ex.copy_from_slice(&sim.qx.clone()); sim.ez.copy_from_slice(&sim.qz.clone());
    let t0 = sim.t;
    // when each chunk last woke
    let mut woke_at = vec![t0; sim.nch];
    let mut was = sim.awake.clone();
    while sim.t < t0 + secs {
        sim.step(1.0);
        for c in 0..sim.nch { if sim.awake[c] != 0 && was[c] == 0 { woke_at[c] = sim.t; } }
        was.copy_from_slice(&sim.awake);
    }
    let kind = unsafe { &*std::ptr::addr_of!(KIND) };
    let awake: Vec<usize> = (0..sim.nch).filter(|&c| sim.awake[c] != 0).collect();
    let (mut shore, mut fixed, mut dq, mut moved, mut fresh, mut other) = (0, 0, 0, 0, 0, 0);
    let mut moved_v = Vec::new();
    let mut since = Vec::new();
    for &c in &awake {
        let b = c * CA;
        let mv = sim.depth[b..b + CA].iter().zip(&sim.win_depth[b..b + CA]).fold(0.0f32, |m, (a, w)| m.max((a - w).abs()));
        since.push(sim.t - woke_at[c]);
        if sim.pools.shore[c] == 1 { shore += 1; }
        else if sim.div[c] >= 1.0 { fixed += 1; }
        else if sim.dq[c] >= unsafe { sim::BIG_DQ } { dq += 1; }
        else if sim.calm[c] > 0 && sim.t - sim.win_t[c] < unsafe { sim::WIN_SECS } { fresh += 1; }
        else if mv > band { moved += 1; moved_v.push(mv); }
        else { other += 1; }
    }
    since.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p = |v: &Vec<f64>, f: f64| if v.is_empty() { 0.0 } else { v[((v.len() - 1) as f64 * f) as usize] };
    println!("after {secs:.0} s: {} awake (river {}, lake {}, other {})", awake.len(), awake.iter().filter(|&&c| kind[c] == 1).count(), awake.iter().filter(|&&c| kind[c] == 2).count(), awake.iter().filter(|&&c| kind[c] == 0).count());
    println!("  why not asleep: a pool's shore {shore}, a spring or outlet {fixed}, a face jumping {dq}, in its calm window {fresh}, levels moved more than the band {moved}, none of these {other}");
    println!("  awake since their last waking: median {:.0} s, p90 {:.0} s, max {:.0} s", p(&since, 0.5), p(&since, 0.9), p(&since, 1.0));
    let mut mvs: Vec<f64> = moved_v.iter().map(|&v| v as f64).collect(); mvs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if !mvs.is_empty() { println!("  of those over the band, their level moved over the window: median {:.1} cm, p90 {:.1} cm", p(&mvs, 0.5) * 100.0, p(&mvs, 0.9) * 100.0); }
    // a few of the 'none' and 'fresh' kind
    let mut shown = 0;
    for &c in &awake {
        if shown >= 6 || sim.pools.shore[c] == 1 || sim.div[c] >= 1.0 { continue; }
        let (cx, cz) = sim.key[c];
        let b = c * CA;
        let mv = sim.depth[b..b + CA].iter().zip(&sim.win_depth[b..b + CA]).fold(0.0f32, |m, (a, w)| m.max((a - w).abs()));
        println!("    chunk ({}, {}): kind {}, calm {}, window {:.1} s, moved {:.2} cm, dq {:.1e}, div {:.1e}, wakes face/owed/drift {:?}, rel_left {:.2} m3",
            cx * CS as i32, cz * CS as i32, kind[c], sim.calm[c], sim.t - sim.win_t[c], mv * 100.0, sim.dq[c], sim.div[c], &sim.why[c][..3], sim.rel_left[c]);
        shown += 1;
    }
}
