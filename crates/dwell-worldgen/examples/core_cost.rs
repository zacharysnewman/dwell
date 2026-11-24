//! What the stone core costs (WORK_QUEUE "Q&A with the owner, 2026-11-23": the 512 m underground,
//! "Measure first"): around seed 0's face A spawn search point, a 7 x 7 column area's chunks from
//! the midplane to 3 chunks over the surface generated and timed, and how many are all one block.
//!
//! `cargo run --release -p dwell-worldgen --example core_cost`

use std::time::Instant;

use dwell_world::{CHUNK_SIZE, Chunk, ChunkCoord, Face, MIDPLANE_Y, SEA_LEVEL};
use dwell_worldgen::BifacialTerrain;

fn main() {
    let t = BifacialTerrain::new(0);
    let [sx, sy, sz] = t.local(Face::A).spawn_search_point();
    let (cx, cz) = (
        (sx as i32).div_euclid(CHUNK_SIZE),
        (sz as i32).div_euclid(CHUNK_SIZE),
    );
    let top = ((sy / dwell_world::CELL_H) as i32).div_euclid(CHUNK_SIZE) + 3;
    let bottom = MIDPLANE_Y.div_euclid(CHUNK_SIZE);
    println!(
        "sea level row {SEA_LEVEL}, spawn ({sx:.0}, {sy:.1} m, {sz:.0}), chunk rows {bottom}..={top}"
    );
    let (mut n, mut uniform) = (0, 0);
    let start = Instant::now();
    let mut per_row = Vec::new();
    for cy in bottom..=top {
        let row_start = Instant::now();
        for dz in -3..=3 {
            for dx in -3..=3 {
                let mut c = Chunk::default();
                t.generate(ChunkCoord::new(cx + dx, cy, cz + dz), &mut c);
                n += 1;
                let v = c.voxels();
                if v.iter().all(|&m| m == v[0]) {
                    uniform += 1;
                }
            }
        }
        per_row.push((cy, row_start.elapsed().as_secs_f64() * 1000.0));
    }
    let total = start.elapsed().as_secs_f64() * 1000.0;
    println!(
        "{n} chunks ({} rows of 49) in {total:.0} ms, {uniform} all one block",
        per_row.len()
    );
    let deep: f64 = per_row
        .iter()
        .take(per_row.len().saturating_sub(6))
        .map(|r| r.1)
        .sum();
    println!(
        "the rows under the surface's 6: {deep:.0} ms ({:.2} ms a chunk)",
        deep / (49.0 * per_row.len().saturating_sub(6).max(1) as f64)
    );
}
