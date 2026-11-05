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
    let valleys = std::env::var("GROUND").is_ok_and(|v| v == "valleys");
    let cols: Vec<(f32, f32, f32, u8)> = (0..n).into_par_iter().map(|i| { // ground, today's depth, today's height, rock(1)/outside(9)
        let (c, l) = (i / CA, i % CA);
        let (kx, kz) = keys[c];
        let (x, z) = (kx * cs + (l % CS) as i32, kz * cs + (l / CS) as i32);
        let col = tg.column_at(x, z);
        let today = if col.water == Water::None { 0.0 } else { (col.level - col.height).max(0.0) };
        let g = if valleys { tg.ground_with_valleys(x, z) } else { tg.ground_without_water(x, z) };
        match g {
            None => (VOID, 0.0, 0.0, 9),
            Some(h) => (top(h), today, col.height, (col.band == dwell_worldgen::terrain::Band::Rock) as u8),
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
    for &i in &src { let (x, z) = xy(i); let on_sink = sinks.contains(&i); let q = sim.inflow[i]; if q > 1.0 || on_sink { println!("spring at ({x}, {z}): {q:.2} m3/s{}", if on_sink { ", ON AN OUTLET" } else { "" }); } }

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
    // each spring's way down (whole, for the routed flow) and the flow it adds to every column on it
    let mut qcol = vec![0.0f32; n];
    let mut chains: Vec<Vec<usize>> = Vec::new();
    for &s in &src {
        let q = sim.inflow[s];
        let mut chain = Vec::new();
        let mut i = s;
        while i != usize::MAX { qcol[i] += q; chain.push(i); if sim.hold[i] > 0.0 || sinks.contains(&i) { break; } i = parent[i]; }
        chains.push(chain);
    }
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
        vol += sim.depth[i] as f64;
    }
    // the river at the depth its flow needs: at each column of a way down (outside the filled hollows),
    // a cut across the local direction; the level whose wet section carries the routed flow by
    // Manning on the local slope (`STEADY=0`: a 10 cm film instead, as first tried)
    let steady = std::env::var("STEADY").map_or(true, |v| v != "0");
    let mut done = vec![false; n];
    let mut level = vec![f32::NEG_INFINITY; n];
    for chain in &chains {
        for (k, &i) in chain.iter().enumerate() {
            if done[i] || flooded[i] || sim.hold[i] > 0.0 || !wet[i] { continue; }
            done[i] = true;
            if !steady { level[i] = level[i].max(sim.ground[i] + 0.1); continue; }
            let a = chain[k.saturating_sub(8)]; let b = chain[(k + 8).min(chain.len() - 1)];
            let ((ax, az), (bx, bz)) = (xy(a), xy(b));
            let (dx, dz) = ((bx - ax) as f32, (bz - az) as f32);
            let len = (dx * dx + dz * dz).sqrt().max(1.0);
            let slope = ((fill[a] - fill[b]) / len).max(1e-3);
            let (nx, nz) = (-dz / len, dx / len);
            let (cx, cz) = xy(i);
            let line: Vec<(i32, Option<usize>)> = (-60..=60).map(|t| (t, idx((cx as f32 + nx * t as f32).round() as i32, (cz as f32 + nz * t as f32).round() as i32))).collect();
            let carry = |l: f32| -> f32 {
                let mut q = 0.0f32;
                for side in [1i32, -1] {
                    let mut t: i32 = if side == 1 { 0 } else { -1 };
                    while t.abs() <= 60 {
                        let Some(j) = line[(t + 60) as usize].1 else { break };
                        let h = l - sim.ground[j];
                        if sim.ground[j] >= VOID || h <= 0.0 { break; }
                        q += h.powf(5.0 / 3.0) * slope.sqrt() / 0.03;
                        t += side;
                    }
                }
                q
            };
            let g = sim.ground[i];
            let (mut lo, mut hi) = (g, g + 6.0);
            for _ in 0..24 { let mid = 0.5 * (lo + hi); if carry(mid) < qcol[i] { lo = mid } else { hi = mid } }
            for side in [1i32, -1] {
                let mut t: i32 = if side == 1 { 0 } else { -1 };
                while t.abs() <= 60 {
                    let Some(j) = line[(t + 60) as usize].1 else { break };
                    if sim.ground[j] >= VOID || hi - sim.ground[j] <= 0.0 { break; }
                    level[j] = level[j].max(hi);
                    t += side;
                }
            }
        }
    }
    for i in 0..n {
        if sim.hold[i] > 0.0 || flooded[i] || level[i] == f32::NEG_INFINITY { continue; }
        let d = (level[i] - sim.ground[i]).max(0.0);
        vol += (d - sim.depth[i]) as f64;
        sim.depth[i] = d;
    }
    let fill_s = t1.elapsed().as_secs_f64();
    {
        let mut d: Vec<f32> = (0..n).filter(|&i| flooded[i]).map(|i| fill[i] - sim.ground[i]).collect();
        d.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if !d.is_empty() { let p = |f: f64| d[((d.len() - 1) as f64 * f) as usize]; println!("hollows' depth below their spill level: p50 {:.2} m, p90 {:.2} m, max {:.2} m", p(0.5), p(0.9), p(1.0)); }
    }
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

    // erosion: where water runs faster than the soil holds, the ground wears down, round by round,
    // never below the stone (soil: a top block over dirt to SOIL m; none where the band is rock)
    let rounds: usize = std::env::var("ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    if rounds == 0 { return; }
    let soil: f32 = std::env::var("SOIL").ok().and_then(|v| v.parse().ok()).unwrap_or(3.5);
    let vc: f32 = std::env::var("VC").ok().and_then(|v| v.parse().ok()).unwrap_or(0.3);
    let k: f32 = std::env::var("ERODE_K").ok().and_then(|v| v.parse().ok()).unwrap_or(0.005);
    let round_s = 10.0;
    let stone: Vec<f32> = (0..n).map(|i| if cols[i].3 == 1 { sim.ground[i] } else { sim.ground[i] - soil }).collect();
    let g0 = sim.ground.clone();
    let t3 = Instant::now();
    // erosion with transport (as hydraulic-erosion simulations on a water grid do): each column's
    // water carries a load (m of sediment); it can carry KC × its speed; carrying less it picks up
    // KE of the shortfall (soil only, never below the stone), carrying more it drops KD of the excess;
    // the load moves with the water across the faces (conserved)
    let kc: f32 = std::env::var("KC").ok().and_then(|v| v.parse().ok()).unwrap_or(0.05);
    let ke: f32 = std::env::var("KE").ok().and_then(|v| v.parse().ok()).unwrap_or(0.05);
    let kd: f32 = std::env::var("KD").ok().and_then(|v| v.parse().ok()).unwrap_or(0.2);
    let _ = (k, vc);
    let mut load = vec![0.0f32; n];
    let mut flux = vec![0.0f32; n];
    for r in 0..rounds {
        let mut worn = 0.0f64;
        let mut dropped = 0.0f64;
        for _ in 0..round_s as usize {
            sim.step(1.0);
            // pick up or drop
            for i in 0..n {
                if sim.ground[i] >= VOID || sim.hold[i] > 0.0 { continue; }
                let d = sim.depth[i];
                if d < 0.01 { if load[i] > 0.0 { sim.ground[i] += load[i]; dropped += load[i] as f64; load[i] = 0.0; } continue; }
                let (x, z) = xy(i);
                let w = idx(x - 1, z).map_or(0.0, |j| sim.qx[j]);
                let nn = idx(x, z - 1).map_or(0.0, |j| sim.qz[j]);
                let vx = 0.5 * (sim.qx[i] + w); let vz = 0.5 * (sim.qz[i] + nn);
                let v = (vx * vx + vz * vz).sqrt() / d;
                let cap = kc * v;
                if load[i] < cap {
                    let e = (ke * (cap - load[i])).min((sim.ground[i] - stone[i]).max(0.0));
                    sim.ground[i] -= e; load[i] += e; worn += e as f64;
                } else {
                    let dep = kd * (load[i] - cap);
                    sim.ground[i] += dep; load[i] -= dep; dropped += dep as f64;
                }
            }
            // the load moves with the water: across each face, the upwind column's share of its water
            flux.iter_mut().for_each(|f| *f = 0.0);
            for i in 0..n {
                if sim.ground[i] >= VOID { continue; }
                let (x, z) = xy(i);
                for (j, q) in [(idx(x + 1, z), sim.qx[i]), (idx(x, z + 1), sim.qz[i])] {
                    let Some(j) = j else { continue };
                    if q == 0.0 || sim.ground[j] >= VOID { continue; }
                    let (from, to) = if q > 0.0 { (i, j) } else { (j, i) };
                    let d = sim.depth[from];
                    if d < 0.01 { continue; }
                    let moved = (load[from] * (q.abs() * 1.0 / d)).min(load[from] * 0.25);
                    flux[from] -= moved; flux[to] += moved;
                }
            }
            for i in 0..n { load[i] = (load[i] + flux[i]).max(0.0); if sim.hold[i] > 0.0 { load[i] = 0.0; } }
        }
        sim.wake_all();
        if r % 6 == 5 || r + 1 == rounds {
            sim.totals();
            let carved = (0..n).filter(|&i| g0[i] < VOID && g0[i] - sim.ground[i] > 0.25).count();
            let raised = (0..n).filter(|&i| g0[i] < VOID && sim.ground[i] - g0[i] > 0.25).count();
            let at_stone = (0..n).filter(|&i| g0[i] < VOID && cols[i].3 == 0 && sim.ground[i] <= stone[i] + 1e-3).count();
            println!("  erosion round {:>3} ({:>5.0} water s): worn {:.0} m3, dropped {:.0} m3 this round; {} columns carved over 25 cm, {} raised over 25 cm, {} down to stone; out {:.2} m3/s; wall {:.1} s",
                r + 1, (r + 1) as f64 * round_s, worn, dropped, carved, raised, at_stone, (sim.taken - last_out) / round_s, t3.elapsed().as_secs_f64());
        }
        sim.totals(); last_out = sim.taken;
    }
    // the carved ground in ½ m rows, as blocks would hold it
    for i in 0..n { if sim.ground[i] < VOID { sim.ground[i] = top(sim.ground[i]); } }
    for _ in 0..120 { sim.step(1.0); }
    let rock: Vec<bool> = (0..n).map(|i| g0[i] < VOID && sim.ground[i] <= stone[i] + 1e-3 && cols[i].3 == 0).collect();
    {
        // where the water is: held in the square against what the springs put in; the deepest places
        let held: f64 = (0..n).filter(|&i| sim.hold[i] == 0.0 && !sinks.contains(&i)).map(|i| sim.depth[i] as f64).sum();
        let added = sim.src_total * (sim.t);
        let mut deep: Vec<usize> = (0..n).filter(|&i| sim.ground[i] < VOID && sim.hold[i] == 0.0).collect();
        deep.sort_by(|&a, &b| sim.depth[b].partial_cmp(&sim.depth[a]).unwrap());
        println!("water held in the square {held:.0} m3 (placed by the fill {vol:.0}); springs put in {added:.0} m3 over {:.0} s", sim.t);
        let mut shown: Vec<(i32, i32)> = Vec::new();
        for &i in &deep {
            let (x, z) = xy(i);
            if shown.iter().any(|&(a, b)| (a - x).abs() < 60 && (b - z).abs() < 60) { continue; }
            // how big the wet patch around it is
            let mut cnt = 0; let mut v = 0.0f64;
            for dz in -60..=60 { for dx in -60..=60 { if let Some(j) = idx(x + dx, z + dz) { if sim.depth[j] > 0.05 { cnt += 1; v += sim.depth[j] as f64; } } } }
            println!("   deep water at ({x}, {z}): {:.2} m over ground {:.1} (carved {:+.2} m); wet over 5 cm within 60 m: {cnt} columns, {v:.0} m3", sim.depth[i], sim.ground[i], sim.ground[i] - g0[i]);
            shown.push((x, z));
            if shown.len() >= 6 { break; }
        }
    }
    // along each river's course (the map's), every 25 m: a cut across it (±40 m) — the deepest water,
    // the flow across the cut, the fastest water, the floor's height
    for (ri, r) in m.rivers.iter().enumerate() {
        let p = &r.path;
        let mut pts = Vec::new();
        for k in 0..p.len().saturating_sub(1) {
            let (a, b) = (p[k], p[k + 1]);
            let len = (b.x - a.x).hypot(b.z - a.z);
            let steps = (len / 25.0).ceil().max(1.0) as usize;
            for s in 0..steps {
                let t = s as f32 / steps as f32;
                let (x, z) = (a.x + (b.x - a.x) * t, a.z + (b.z - a.z) * t);
                if inside(x, z) { pts.push((x, z, (b.x - a.x) / len, (b.z - a.z) / len, a.discharge + (b.discharge - a.discharge) * t)); }
            }
        }
        if pts.len() < 4 { continue; }
        let mut line = Vec::new();
        let mut dry = 0;
        for (k, &(x, z, dx, dz, qmap)) in pts.iter().enumerate() {
            let (nx, nz) = (-dz, dx);
            let (mut dmax, mut flow, mut vmax, mut floor) = (0.0f32, 0.0f32, 0.0f32, f32::INFINITY);
            for s in -40..=40 {
                let Some(i) = idx((x + nx * s as f32).floor() as i32, (z + nz * s as f32).floor() as i32) else { continue };
                if sim.ground[i] >= VOID { continue; }
                floor = floor.min(sim.ground[i]);
                let d = sim.depth[i];
                dmax = dmax.max(d);
                let (cx, cz) = xy(i);
                let w = idx(cx - 1, cz).map_or(0.0, |j| sim.qx[j]);
                let nn = idx(cx, cz - 1).map_or(0.0, |j| sim.qz[j]);
                let (qx, qz) = (0.5 * (sim.qx[i] + w), 0.5 * (sim.qz[i] + nn));
                flow += qx * dx + qz * dz; // discharge per m across the cut, summed over its 1 m columns
                if d > 0.01 { vmax = vmax.max((qx * qx + qz * qz).sqrt() / d); }
            }
            if dmax < 0.01 { dry += 1; }
            if k >= 32 && k <= 44 { println!("     point {k} ({:.0}, {:.0}): depth {:.2}, flow {:.1}, floor {:.1}; edge distance {:.0} m", x, z, dmax, flow, floor, (x - x0 as f32).min(z - z0 as f32).min((x0 + size) as f32 - x).min((z0 + size) as f32 - z)); }
            if k % 4 == 0 { line.push(format!("{:>4}m d{:.2} Q{:.1}/{:.1} v{:.1} f{:.1}", k * 25, dmax, flow, qmap, vmax, floor)); }
        }
        println!("river {ri}: {} points every 25 m, {} with no water over 1 cm in ±40 m; every 100 m: depth, flow across / the map's, fastest, floor", pts.len(), dry);
        for c in line.chunks(4) { println!("   {}", c.join(" | ")); }
    }
    image_rock(&format!("{out}/eroded.ppm"), &sim.depth, &sim.ground, &rock, nc as usize);
    println!("erosion {} rounds in {:.1} s wall", rounds, t3.elapsed().as_secs_f64());
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

/// As `image`, with the ground carved down to stone shown grey.
fn image_rock(path: &str, depth: &[f32], ground: &[f32], rock: &[bool], nc: usize) {
    let size = nc * CS;
    let at = |x: usize, z: usize| ((z / CS) * nc + x / CS) * CA + (z % CS) * CS + x % CS;
    let mut px = Vec::with_capacity(size * size * 3);
    for z in 0..size {
        for x in 0..size {
            let i = at(x, z);
            let g = ground[i];
            if g >= VOID { px.extend_from_slice(&[40, 40, 40]); continue; }
            let gx = if x + 1 < size { ground[at(x + 1, z)] - g } else { 0.0 };
            let gz = if z + 1 < size { ground[at(x, z + 1)] - g } else { 0.0 };
            let shade = (0.75 - 0.35 * (gx + gz)).clamp(0.25, 1.0);
            let d = depth[i];
            let c = if d > 0.01 {
                let k = (d / 2.0).min(1.0);
                [(40.0 * (1.0 - k)) as u8, (120.0 - 60.0 * k) as u8, (200.0 - 40.0 * k) as u8]
            } else if rock[i] {
                [(140.0 * shade) as u8, (140.0 * shade) as u8, (140.0 * shade) as u8]
            } else {
                [(120.0 * shade) as u8, (150.0 * shade) as u8, (95.0 * shade) as u8]
            };
            px.extend_from_slice(&c);
        }
    }
    let mut f = std::fs::File::create(path).unwrap();
    use std::io::Write;
    write!(f, "P6\n{size} {size}\n255\n").unwrap();
    f.write_all(&px).unwrap();
}
