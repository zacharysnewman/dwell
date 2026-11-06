#[path = "../sim.rs"]
mod sim;
use sim::*;

// One clean river: straight, `width` wide, falling `slope`, banks ½ m over its surface, on ½ m steps,
// along x or diagonal; inflow `q` at its head, an outlet at its foot. Does it settle?
fn run(slope: f32, width: f32, diag: bool, q: f32) {
    let n = 16; // chunks a side
    let size = (n * CS) as f32;
    let mut keys = Vec::new();
    for cz in 0..n as i32 { for cx in 0..n as i32 { keys.push((cx, cz)); } }
    let mut s = Sim::new(keys);
    s.theta = 0.95;
    s.window_test = std::env::var("OLD_TEST").is_err();
    unsafe { sim::VECTOR = std::env::var("FACE_ONLY").is_err(); }
    let at = |x: usize, z: usize| -> usize { ((z / CS) * n + x / CS) * CA + (z % CS) * CS + x % CS };
    let q5 = |v: f32| (v * 2.0).floor() / 2.0;
    let (dx, dz) = if diag { (std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2) } else { (1.0, 0.0) };
    let (ox, oz) = if diag { (8.0, 8.0) } else { (2.0, size / 2.0) };
    let len = if diag { (size - 16.0) * std::f32::consts::SQRT_2 } else { size - 4.0 };
    let top = 20.0 + slope * len;
    let mut head = Vec::new();
    let mut foot = Vec::new();
    for z in 0..n * CS { for x in 0..n * CS {
        let (px, pz) = (x as f32 + 0.5 - ox, z as f32 + 0.5 - oz);
        let along = px * dx + pz * dz;
        let across = (-px * dz + pz * dx).abs();
        if along < 0.0 || along > len { continue; }
        let surf = top - slope * along;
        let i = at(x, z);
        if across <= width / 2.0 {
            s.ground[i] = q5(surf - 1.0);
            s.depth[i] = 0.3;
            if along < 1.5 { head.push(i); }
            if along > len - 1.5 { foot.push(i); }
        } else if across <= width / 2.0 + 3.0 {
            s.ground[i] = q5(surf + 0.5) + 0.5;
        }
    } }
    for &i in &head { s.sources[i / CA].push(((i % CA) as u16, q / head.len() as f32)); }
    for &i in &foot { s.sinks[i / CA].push((i % CA) as u16); }
    s.rebuild_inflow();
    let mut lo = vec![f32::MAX; s.depth.len()]; let mut hi = vec![f32::MIN; s.depth.len()];
    let mut aw = Vec::new();
    for t in 0..1800 {
        s.step(1.0);
        if t >= 1680 { for i in 0..s.depth.len() { lo[i] = lo[i].min(s.depth[i]); hi[i] = hi[i].max(s.depth[i]); } }
        if t % 300 == 299 { aw.push(s.awake_count()); }
    }
    // why the wet chunks are awake: their largest face change a substep and fastest level change,
    // and where (depth there)
    if std::env::var("WHY").is_ok() {
        let (mut nq, mut nd, mut both) = (0, 0, 0);
        let mut ex = Vec::new();
        for c in 0..s.nch {
            if s.awake[c] == 0 || !s.depth[c * CA..c * CA + CA].iter().any(|&d| d > 0.0) { continue; }
            let a = s.dq[c] >= SLEEP_DQ; let b = s.div[c] >= SLEEP_DIV;
            if a && b { both += 1 } else if a { nq += 1 } else if b { nd += 1 }
            if ex.len() < 4 { ex.push((c, s.dq[c], s.div[c])); }
        }
        println!("   awake wet chunks: by face change only {nq}, by level change only {nd}, both {both}");
        for (c, dq, dv) in ex {
            // the column with the fastest level change, and its depth and neighbours' depths
            let b = c * CA;
            let mut best = (0usize, 0.0f32);
            for l in 0..CA {
                let i = b + l; let (x, z) = (l % CS, l / CS);
                let mut net = -s.qx[i] - s.qz[i];
                if x > 0 { net += s.qx[i - 1]; } if z > 0 { net += s.qz[i - CS]; }
                if x > 0 && z > 0 && x < CS - 1 && z < CS - 1 && net.abs() > best.1 { best = (l, net.abs()); }
            }
            let i = b + best.0;
            println!("   chunk {c}: dq {dq:.1e} div {dv:.1e}; interior column ({}, {}) net {:.1e} m/s, depth {:.4}, neighbours E {:.4} W {:.4} S {:.4} N {:.4}",
                best.0 % CS, best.0 / CS, best.1, s.depth[i], s.depth[i + 1], s.depth[i - 1], s.depth[i + CS], s.depth[i - CS]);
        }
    }
    s.totals();
    let mut sw: Vec<f32> = (0..s.depth.len()).filter(|&i| hi[i] > 0.01).map(|i| hi[i] - lo[i]).collect();
    sw.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p = |f: f64| sw[((sw.len() - 1) as f64 * f) as usize] * 1000.0;
    let wet_chunks = (0..s.nch).filter(|&c| s.depth[c * CA..c * CA + CA].iter().any(|&d| d > 0.0)).count();
    println!("slope {:>5.1}% width {:>4.0} m {:<8} Q {:>4.1}: awake every 300 s {:?} of {} wet chunks; swing p50 {:.1} p90 {:.1} p99 {:.1} mm; out {:.2} of {:.2} m3/s",
        slope * 100.0, width, if diag { "diagonal" } else { "along x" }, q, aw, wet_chunks, p(0.5), p(0.9), p(0.99), s.taken / s.t, q);
}

fn main() {
    if std::env::var("WHY").is_ok() { run(0.01, 8.0, true, 5.0); run(0.004, 3.0, true, 1.5); return; }
    for &(slope, width, q) in &[(0.04f32, 3.0f32, 1.5f32), (0.004, 3.0, 1.5), (0.04, 25.0, 25.0), (0.004, 25.0, 25.0), (0.01, 8.0, 5.0)] {
        for diag in [false, true] { run(slope, width, diag, q); }
    }
}
