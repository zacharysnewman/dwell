//! Probes a seam on face B of a seed (`seam_probe <seed> <x0> <x1> <z>`): per column, its height
//! and the nearest river hit the chunks carve by.
use dwell_worldgen::BifacialTerrain;

fn main() {
    let a: Vec<i64> = std::env::args().skip(1).map(|v| v.parse().unwrap()).collect();
    let t = BifacialTerrain::height_field(a[0] as u64);
    let g = t.local(dwell_world::Face::B);
    for x in a[1]..=a[2] {
        let c = g.column_at(x as i32, a[3] as i32);
        let hit = g.river_hit(x, a[3], 0.0);
        let mut near = Vec::new();
        g.map().channels.each_near(x as f64, a[3] as f64, 0.0, f64::INFINITY, |id, h| {
            near.push(format!("#{id} d{:.1} half{:.1} lvl{:.2} valley{:.1}", h.d, h.half, h.level, h.valley))
        });
        if a.len() > 4 {
            println!("x {x}: {c:?}");
            continue;
        }
        println!(
            "x {x}: h {:.2} band {:?} wall {} | hit {:?} | {}",
            c.height,
            c.band,
            c.wall,
            hit.map(|h| format!("d{:.1} half{:.1} lvl{:.2} valley{:.1}", h.d, h.half, h.level, h.valley)),
            near.join("; ")
        );
    }
}
