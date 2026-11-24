//! Prints the water around a spot of a seed's face (`water_spot <seed> <x> <z> [r] [a|b]
//! [water|ground]`): each column's water row (relative to the spot's, `.` dry), or its ground
//! height (dm over the spot's), and the river courses passing near.
use dwell_worldgen::BifacialTerrain;
use dwell_worldgen::terrain::Water;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let seed: u64 = a[1].parse().unwrap();
    let (x0, z0): (i32, i32) = (a[2].parse().unwrap(), a[3].parse().unwrap());
    let r: i32 = a.get(4).map_or(8, |v| v.parse().unwrap());
    let t = BifacialTerrain::height_field(seed);
    let face = if a.get(5).is_some_and(|f| f == "b") {
        dwell_world::Face::B
    } else {
        dwell_world::Face::A
    };
    let ground = a.get(6).is_some_and(|m| m == "ground");
    let g = t.local(face);
    let base = g.column_at(x0, z0).water_row();
    let h0 = g.column_at(x0, z0).height;
    for z in z0 - r..=z0 + r {
        let line: Vec<String> = (x0 - r..=x0 + r)
            .map(|x| {
                let c = g.column_at(x, z);
                if ground {
                    return format!("{:5}", ((c.height - h0) * 10.0).round() as i32);
                }
                if c.water == Water::None {
                    "  .".into()
                } else {
                    format!("{:3}", c.water_row() - base)
                }
            })
            .collect();
        println!("z{z:6} {}", line.join(""));
    }
    let ch = &g.map().channels;
    for (k, c) in ch.courses.iter().enumerate() {
        let near: Vec<_> = c
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                (p.x - f64::from(x0)).abs() < 40.0 && (p.z - f64::from(z0)).abs() < 40.0
            })
            .collect();
        if near.is_empty() {
            continue;
        }
        println!(
            "course {k} ({} pts, last at {:.0},{:.0} s {:.2}):",
            c.len(),
            c.last().unwrap().x,
            c.last().unwrap().z,
            c.last().unwrap().s
        );
        for (i, p) in near {
            println!("  #{i} {:.1},{:.1} s {:.2} w {:.1}", p.x, p.z, p.s, p.w);
        }
    }
}
