use dwell_worldgen::BifacialTerrain;
use dwell_world::Face;

#[test]
fn explore() {
    for seed in [0u64, 7, 20_260_925, 1, 2, 3] {
        let t0 = std::time::Instant::now();
        let t = BifacialTerrain::new(seed);
        let dt = t0.elapsed();
        let sites = t.portal_sites().to_vec();
        let want = dwell_worldgen::portals::wanted(seed, &t.portals().rules);
        eprintln!("seed {seed}: {} of {want} in {dt:?}", sites.len());
        for p in sites {
            let ha = t.local(Face::A).column_at(p.x, p.z).height;
            let hb = t.local(Face::B).column_at(p.x, p.z).height;
            eprintln!("  {p:?} a {ha:.1} b {hb:.1}");
        }
    }
}

#[test]
fn explore_heights() {
    for seed in [0u64, 7, 3] {
        let t = BifacialTerrain::new(seed);
        let mut hs = Vec::new();
        let mut land = 0;
        for i in 0..4000 {
            let x = ((i * 7919) % 40001) as i32 - 20000;
            let z = ((i * 104729 + 13) % 40001) as i32 - 20000;
            if x as i64 * x as i64 + z as i64 * z as i64 > 20000i64 * 20000 { continue; }
            let a = t.local(Face::A).column_at(x, z);
            let b = t.local(Face::B).column_at(x, z);
            if a.height > 33.0 && b.height > 33.0 && a.coast > 150.0 && b.coast > 150.0 {
                land += 1;
                hs.push(a.height.max(b.height) - 32.0);
            }
        }
        hs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let q = |f: f32| hs[((hs.len() as f32 - 1.0) * f) as usize];
        eprintln!("seed {seed}: both land {land}: max-height above sea p5 {:.0} p10 {:.0} p25 {:.0} p50 {:.0}", q(0.05), q(0.1), q(0.25), q(0.5));
    }
}
