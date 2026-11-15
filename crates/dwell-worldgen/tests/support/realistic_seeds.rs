//! Seed fixtures for the Realistic world's tests at real-world scale (0.71, WORK_QUEUE item 15): a
//! 1× face is usually one plate's interior with no boundary, often low or all ocean, so a test that
//! needs land, a range or a volcano takes the first seeds whose face has it, read off the plates
//! alone (cheap: no world map), instead of a fixed list whose faces may have none.
#![allow(dead_code)]

use dwell_world::Face;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::plate_tectonics::PlateTectonics;
use dwell_worldgen::plates::PLATES;

/// A face's land share on the disc and its plates' highest uplift (m).
pub fn survey(world: u64, face: Face) -> (f64, f64) {
    let seed = face_seed(world, face);
    let t = PlateTectonics::for_seed(seed);
    let reach = PLATES.reach;
    let n = 40;
    let (mut land, mut all, mut top) = (0, 0, 0.0f64);
    for j in 0..n {
        for i in 0..n {
            let x = -reach + (f64::from(i) + 0.5) * 2.0 * reach / f64::from(n);
            let z = -reach + (f64::from(j) + 0.5) * 2.0 * reach / f64::from(n);
            if x.hypot(z) > reach - PLATES.rim.edge_sea {
                continue;
            }
            all += 1;
            let h = t.height(x, z);
            land += usize::from(t.layout.is_land(x, z) || h > 0.0);
            top = top.max(h);
        }
    }
    (f64::from(land) / f64::from(all.max(1)), top)
}

/// The first `k` worlds (from 0) whose `face` is at least `min_land` land with an uplift of at
/// least `min_top` m.
pub fn seeds_with(face: Face, k: usize, min_land: f64, min_top: f64) -> Vec<u64> {
    (0..200)
        .filter(|&w| {
            let (land, top) = survey(w, face);
            land >= min_land && top >= min_top
        })
        .take(k)
        .collect()
}
