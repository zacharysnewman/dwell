#[path = "../sim.rs"]
mod sim;
use sim::*;

// Does a wave lose force as it bounces? A clean river along x (0.4%, 8 m wide, banks), settled with
// everything stepped; then a 15 cm bump of water dropped mid-river, and the wave's energy (the sum of
// squared level differences from the settled state) followed for 300 s: everything stepped, then
// with sleep (settled asleep first), with the band and smooth waking.
fn build() -> Sim {
    let n = 16;
    let mut keys = Vec::new();
    for cz in 0..n as i32 { for cx in 0..n as i32 { keys.push((cx, cz)); } }
    let mut s = Sim::new(keys);
    s.theta = 0.95;
    let at = |x: usize, z: usize| -> usize { ((z / CS) * n + x / CS) * CA + (z % CS) * CS + x % CS };
    let q5 = |v: f32| (v * 2.0).floor() / 2.0;
    let size = n * CS;
    let (slope, width, q) = (0.004f32, 8.0f32, 5.0f32);
    let mid = size as f32 / 2.0;
    let mut head = Vec::new(); let mut foot = Vec::new();
    for z in 0..size { for x in 2..size - 2 {
        let across = (z as f32 + 0.5 - mid).abs();
        let surf = 20.0 + slope * (size - x) as f32;
        let i = at(x, z);
        if across <= width / 2.0 {
            s.ground[i] = q5(surf - 1.0); s.depth[i] = 0.5;
            if x == 2 { head.push(i); } if x == size - 3 { foot.push(i); }
        } else if across <= width / 2.0 + 3.0 { s.ground[i] = q5(surf + 0.5) + 0.5; }
    } }
    for &i in &head { s.sources[i / CA].push(((i % CA) as u16, q / head.len() as f32)); }
    for &i in &foot { s.sinks[i / CA].push((i % CA) as u16); }
    s.rebuild_inflow();
    s
}

fn energy(a: &[f32], b: &[f32]) -> (f64, f32) {
    let mut e = 0.0f64; let mut mx = 0.0f32;
    for (x, y) in a.iter().zip(b) { let d = x - y; e += (d * d) as f64; mx = mx.max(d.abs()); }
    (e, mx)
}

fn main() {
    let band: f32 = std::env::var("BAND").ok().and_then(|v| v.parse().ok()).unwrap_or(0.05);
    unsafe { sim::BAND = band; sim::SMOOTH = true; sim::WIN_SECS = 10.0; }
    let sponges: Vec<f32> = std::env::var("SPONGES").map(|v| v.split(',').map(|t| t.parse().unwrap()).collect()).unwrap_or(vec![0.0]);
    let mut s = build();
    s.ema_secs = 5.0;
    s.sleep = false;
    for _ in 0..1500 { s.step(1.0); }
    let settled = s.depth.clone();
    let (q0x, q0z) = (s.qx.clone(), s.qz.clone());
    let size = 16 * CS;
    let bump: Vec<usize> = { let mut v = Vec::new(); for z in size / 2 - 3..size / 2 + 3 { for x in 120..128 { v.push(((z / CS) * 16 + x / CS) * CA + (z % CS) * CS + x % CS); } } v };
    let mut runs = vec![("everything stepped".to_string(), false, 0.0f32)];
    for &sp in &sponges { runs.push((format!("sleeping, absorbing edge {sp}"), true, sp)); }
    runs.push(("sleeping at the average".to_string(), true, -1.0));
    for (label, sleep, sp) in runs {
        unsafe { sim::SPONGE = sp.max(0.0); }
        s.mean_sleep = sp < 0.0;
        s.ed.copy_from_slice(&settled);
        s.depth.copy_from_slice(&settled); s.qx.copy_from_slice(&q0x); s.qz.copy_from_slice(&q0z);
        s.ex.copy_from_slice(&q0x); s.ez.copy_from_slice(&q0z);
        s.awake.iter_mut().for_each(|a| *a = 1); s.calm.iter_mut().for_each(|c| *c = 0); s.dirty = true;
        s.owed.iter_mut().for_each(|v| *v = 0.0); s.rate.iter_mut().for_each(|v| *v = 0.0); s.rel_left.iter_mut().for_each(|v| *v = 0.0);
        s.sleep = sleep;
        if sleep { for _ in 0..120 { s.step(1.0); } } // fall asleep on the settled river
        let asleep_before = s.nch - s.awake_count();
        let base = s.depth.clone();
        for &i in &bump { s.depth[i] += 0.15; }
        let mut chunks: Vec<usize> = bump.iter().map(|&i| i / CA).collect(); chunks.sort(); chunks.dedup();
        for c in chunks { s.wake_around(c); }
        let mut line = Vec::new();
        for t in 0..=300 {
            if t % 30 == 0 {
                let (e, mx) = energy(&s.depth, &base);
                line.push(format!("{t}s:{:.3}/{:.1}cm/{}", e, mx * 100.0, s.awake_count()));
            }
            s.step(1.0);
        }
        println!("{label} (asleep before the bump: {asleep_before} of {}): energy / largest difference / awake every 30 s:\n   {}", s.nch, line.join("  "));
    }
}
