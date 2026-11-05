#[path = "../sim.rs"]
mod sim;
use dwell_worldgen::terrain::{TerrainGenerator, Water};
use dwell_worldgen::worldmap::{self, NO_LAKE};
use rayon::prelude::*;
use sim::*;
use std::time::Instant;

// Rivers without beds (Phase 53 G0, ADR 0060): a square of the game's terrain with no water shaped
// into the ground (`ground_without_water`: the map's eroded height and detail), water placed by
// fill-and-spill from the map's springs (a priority flood from the outlets; each spring's water
// traced down the flood's tree; only the hollows it passes through filled, to their spill level),
// then settled by the grid. Images: today's water beside the bed-free water.
//
// nobeds X0 Z0 SIZE SETTLE_S OUTDIR

const SEA: f32 = 32.0; // sea level, face-local m (SEA_LEVEL_M)

fn top(h: f32) -> f32 { (h / 0.5).ceil() * 0.5 } // the top of a column's solid cells (½ m rows)

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (x0, z0): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let size: i32 = a[3].parse().unwrap();
    let settle: f64 = a[4].parse().unwrap();
    let out = a[5].clone();
    let seed = 42u64;
    let t0 = Instant::now();
    let mut tg = TerrainGenerator::new(seed);
    if let Some(def) = dwell_worldgen::volume::terrain_volume() { tg = tg.with_volume(def); }
    let m = worldmap::for_seed(seed);
    let cs = CS as i32;
    let (cx0, cz0, nc) = (x0.div_euclid(cs), z0.div_euclid(cs), size / cs);
    let mut keys = Vec::new();
    for cz in 0..nc { for cx in 0..nc { keys.push((cx0 + cx, cz0 + cz)); } }
    let mut sim = Sim::new(keys.clone());
    sim.theta = 0.95;
    let n = sim.nch * CA;
    // columns: the bed-free ground, and today's water for comparison
    let cols: Vec<(f32, f32, f32, u8)> = (0..n).into_par_iter().map(|i| {
        let (c, l) = (i / CA, i % CA);
        let (kx, kz) = keys[c];
        let (x, z) = (kx * cs + (l % CS) as i32, kz * cs + (l / CS) as i32);
        let col = tg.column_at(x, z);
        let today = if col.water == Water::None { 0.0 } else { (col.level - col.height).max(0.0) };
        match tg.ground_without_water(x, z) {
            None => (VOID, 0.0, 0.0, 9),
            Some(h) => (top(h), today, col.height, 0),
        }
    }).collect();
    let gen_s = t0.elapsed().as_secs_f64();
    let idx = |x: i32, z: i32| -> Option<usize> {
        let (cx, cz) = (x.div_euclid(cs) - cx0, z.div_euclid(cs) - cz0);
        if cx < 0 || cz < 0 || cx >= nc || cz >= nc { return None; }
        Some(((cz * nc + cx) as usize) * CA + (z.rem_euclid(cs) as usize) * CS + x.rem_euclid(cs) as usize)
    };
    let xy = |i: usize| -> (i32, i32) { let (c, l) = (i / CA, i % CA); let (kx, kz) = keys[c]; (kx * cs + (l % CS) as i32, kz * cs + (l / CS) as i32) };
    for i in 0..n { sim.ground[i] = cols[i].0; }
    // outlets: the sea (held at sea level) and the square's edge (open)
    let mut sinks = Vec::new();
    for i in 0..n {
        if sim.ground[i] >= VOID { continue; }
        let (x, z) = xy(i);
        let edge = x == x0 || z == z0 || x == x0 + size - 1 || z == z0 + size - 1;
        if sim.ground[i] < SEA {
            sim.hold[i] = SEA - sim.ground[i];
            sim.depth[i] = sim.hold[i];
            sinks.push(i);
        } else if edge {
            sim.hold[i] = 0.0;
            sinks.push(i);
        }
    }
    sim.hold_sea = true;
    for &i in &sinks { sim.sinks[i / CA].push((i % CA) as u16); }
    // springs: the map's river heads and growth inside the square
    let inside = |x: f32, z: f32| x >= x0 as f32 && z >= z0 as f32 && x < (x0 + size) as f32 && z < (z0 + size) as f32;
    let mut springs = Vec::new();
    for r in &m.rivers {
        let p = &r.path;
        if r.from_lake == NO_LAKE && inside(p[0].x, p[0].z) { springs.push((p[0].x, p[0].z, p[0].discharge.max(0.05))); }
        for k in 1..p.len() {
            let (was, is) = (inside(p[k - 1].x, p[k - 1].z), inside(p[k].x, p[k].z));
            if is && !was { springs.push((p[k].x, p[k].z, p[k].discharge.max(0.05))); }
            else if is { let inc = p[k].discharge - p[k - 1].discharge; if inc > 0.0 && inc < 1.0 { springs.push((p[k].x, p[k].z, inc)); } }
        }
    }
    let mut src = Vec::new();
    for &(x, z, q) in &springs {
        if let Some(i) = idx(x as i32, z as i32) { if sim.ground[i] < VOID && sim.hold[i] == 0.0 { sim.sources[i / CA].push(((i % CA) as u16, q)); src.push(i); } }
    }
    sim.rebuild_inflow();
    let total_q: f64 = sim.src_total;

    // fill-and-spill: a priority flood from the outlets (each column's spill level and its way down)
    let t1 = Instant::now();
    let mut fill = vec![f32::INFINITY; n];
    let mut parent = vec![usize::MAX; n];
    let mut heap = std::collections::BinaryHeap::new();
    let key = |v: f32| std::cmp::Reverse((v * 1000.0) as i64);
    for &i in &sinks { fill[i] = sim.ground[i] + sim.hold[i]; heap.push((key(fill[i]), i)); }
    let nb4 = |i: usize| -> [Option<usize>; 4] { let (x, z) = xy(i); [idx(x + 1, z), idx(x - 1, z), idx(x, z + 1), idx(x, z - 1)] };
    while let Some((_, i)) = heap.pop() {
        for j in nb4(i).into_iter().flatten() {
            if sim.ground[j] >= VOID || fill[j] < f32::INFINITY { continue; }
            fill[j] = fill[i].max(sim.ground[j]);
            parent[j] = i;
            heap.push((key(fill[j]), j));
        }
    }
    // each spring's water down the tree: the hollows it passes through fill to their spill level
    let mut wet = vec![false; n];
    let mut flooded = vec![false; n];
    let mut path_cols = 0usize;
    for &s in &src {
        let mut i = s;
        while i != usize::MAX && !wet[i] {
            wet[i] = true; path_cols += 1;
            if fill[i] > sim.ground[i] + 1e-3 && !flooded[i] {
                // the whole hollow at this level
                let lev = fill[i];
                let mut stack = vec![i];
                flooded[i] = true;
                while let Some(k) = stack.pop() {
                    for j in nb4(k).into_iter().flatten() {
                        if !flooded[j] && sim.ground[j] < VOID && (fill[j] - lev).abs() < 1e-3 && fill[j] > sim.ground[j] + 1e-3 { flooded[j] = true; stack.push(j); }
                    }
                }
            }
            i = parent[i];
        }
    }
    let mut lake_cols = 0usize;
    let mut vol = 0.0f64;
    for i in 0..n {
        if sim.hold[i] > 0.0 { continue; }
        if flooded[i] { sim.depth[i] = fill[i] - sim.ground[i]; lake_cols += 1; }
        else if wet[i] { sim.depth[i] = 0.1; }
        vol += sim.depth[i] as f64;
    }
    let fill_s = t1.elapsed().as_secs_f64();
    println!("square ({x0}, {z0}) {size} m: {} columns; generated in {gen_s:.1} s; {} springs, {total_q:.1} m3/s; fill-and-spill {fill_s:.2} s: {path_cols} columns on the water's way, {lake_cols} in filled hollows, {:.0} m3 placed", n, src.len(), vol);
    image(&format!("{out}/today.ppm"), &sim, &cols.iter().map(|c| c.1).collect::<Vec<_>>(), &cols.iter().map(|c| c.2).collect::<Vec<_>>(), nc as usize);
    image(&format!("{out}/fill.ppm"), &sim, &sim.depth.clone(), &sim.ground.clone(), nc as usize);

    // the settle: everything stepped, in/out and the largest level change each 60 s
    let t2 = Instant::now();
    let mut prev = sim.depth.clone();
    let mut last_out = 0.0;
    let mut tt = 0.0;
    while tt < settle {
        let w = Instant::now();
        for _ in 0..60 { sim.step(1.0); }
        tt += 60.0;
        sim.totals();
        let mx = sim.depth.iter().zip(&prev).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
        let moving = sim.depth.iter().zip(&prev).filter(|(a, b)| (*a - *b).abs() > 0.05).count();
        prev.copy_from_slice(&sim.depth);
        println!("  settle t {tt:>5.0} s: out {:.2} m3/s of {total_q:.2} in; columns moved more than 5 cm in the last 60 s: {moving}, the most {:.2} m; wall {:.1} s", (sim.taken - last_out) / 60.0, mx, w.elapsed().as_secs_f64());
        last_out = sim.taken;
    }
    println!("settle {:.0} water s in {:.1} s wall", tt, t2.elapsed().as_secs_f64());
    image(&format!("{out}/settled.ppm"), &sim, &sim.depth.clone(), &sim.ground.clone(), nc as usize);
}

/// A top-down picture, one pixel a column: ground shaded by its slope, water blue by depth.
fn image(path: &str, sim: &Sim, depth: &[f32], ground: &[f32], nc: usize) {
    let size = nc * CS;
    let at = |x: usize, z: usize| ((z / CS) * nc + x / CS) * CA + (z % CS) * CS + x % CS;
    let mut px = Vec::with_capacity(size * size * 3);
    for z in 0..size {
        for x in 0..size {
            let i = at(x, z);
            let g = ground[i];
            if g >= VOID || g > 1e3 { px.extend_from_slice(&[40, 40, 40]); continue; }
            let gx = if x + 1 < size { ground[at(x + 1, z)] - g } else { 0.0 };
            let gz = if z + 1 < size { ground[at(x, z + 1)] - g } else { 0.0 };
            let shade = (0.75 - 0.35 * (gx + gz)).clamp(0.25, 1.0);
            let d = depth[i];
            let c = if d > 0.02 {
                let k = (d / 2.0).min(1.0);
                [(40.0 * (1.0 - k)) as u8, (120.0 - 60.0 * k) as u8, (200.0 - 40.0 * k) as u8]
            } else {
                [(120.0 * shade) as u8, (150.0 * shade) as u8, (95.0 * shade) as u8]
            };
            px.extend_from_slice(&c);
        }
    }
    let _ = sim;
    let mut f = std::fs::File::create(path).unwrap();
    use std::io::Write;
    write!(f, "P6\n{size} {size}\n255\n").unwrap();
    f.write_all(&px).unwrap();
}
