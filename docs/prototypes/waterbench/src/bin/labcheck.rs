#[path = "../sim.rs"]
mod sim;
use sim::*;

// The lab's River scene: 96 × 48, valley(0.04, 1.0), three springs at x = 0 (here 0.5 m³/s each),
// the far edge removing water. Chunks 6 × 3.
fn build(theta: f32, sleep: bool) -> Sim {
    let (w, h) = (96i32, 48i32);
    let mut keys = Vec::new();
    for cz in 0..3 { for cx in 0..6 { keys.push((cx, cz)); } }
    let mut s = Sim::new(keys);
    s.theta = theta; s.sleep = sleep;
    let q5 = |v: f32| (v * 2.0).round() / 2.0;
    let mid = h / 2;
    let at = |x: i32, z: i32| -> usize { let c = ((z / 16) * 6 + x / 16) as usize; c * CA + ((z % 16) * 16 + x % 16) as usize };
    for z in 0..h { for x in 0..w {
        let off = (z - mid).abs();
        let mut g = 12.0 - 0.04 * x as f32;
        if off > 1 { g += 1.0 + (off - 4).max(0) as f32 * 0.35; }
        s.ground[at(x, z)] = q5(g);
    } }
    for z in [mid - 1, mid, mid + 1] { let i = at(0, z); s.sources[i / CA].push(((i % CA) as u16, 0.5)); }
    for z in 0..h { let i = at(w - 1, z); s.sinks[i / CA].push((i % CA) as u16); }
    s.rebuild_inflow();
    s
}

fn main() {
    let theta: f32 = std::env::args().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0.95);
    let at = |x: i32, z: i32| -> usize { let c = ((z / 16) * 6 + x / 16) as usize; c * CA + ((z % 16) * 16 + x % 16) as usize };
    for sleep in [false, true] {
        let mut s = build(theta, sleep);
        let mut lo = vec![f32::MAX; s.depth.len()]; let mut hi = vec![f32::MIN; s.depth.len()];
        let mut awake = Vec::new();
        for t in 0..1500 {
            s.step(1.0);
            if t >= 1380 { for i in 0..s.depth.len() { lo[i] = lo[i].min(s.depth[i]); hi[i] = hi[i].max(s.depth[i]); } }
            if t % 150 == 149 { awake.push(s.awake_count()); }
        }
        s.totals();
        let o0 = s.taken; let t0 = s.t;
        for _ in 0..100 { s.step(1.0); }
        s.totals();
        let q = (s.taken - o0) / (s.t - t0);
        let mut sw = 0.0f32; for i in 0..s.depth.len() { if hi[i] > lo[i] { sw = sw.max(hi[i] - lo[i]); } }
        let prof: Vec<String> = [5, 20, 44, 60, 80, 94].iter().map(|&x| format!("x{}:{:.3}", x, s.ground[at(x, 24)] + s.depth[at(x, 24)])).collect();
        println!("theta {theta} sleep {sleep}: awake every 150 s {:?}; outflow {:.3} m3/s (in 1.5); biggest swing over the last 120 s {:.1} mm; levels at mid {}", awake, q, sw * 1000.0, prof.join(" "));
    }
}
