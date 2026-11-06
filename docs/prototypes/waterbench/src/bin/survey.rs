use dwell_worldgen::worldmap;
fn main() {
    for seed in [1u64, 42, 0xD3E11] {
        let m = worldmap::for_seed(seed);
        let mut len = 0.0f64; let mut area = 0.0f64; let mut qmax = 0.0f32; let mut wmax = 0.0f32;
        let mut wsum_len = 0.0; let mut big = 0usize;
        for r in &m.rivers {
            for w in r.path.windows(2) {
                let l = ((w[1].x - w[0].x) as f64).hypot((w[1].z - w[0].z) as f64);
                let wd = 0.5 * (w[0].width + w[1].width) as f64;
                len += l; area += l * wd; wsum_len += l * wd;
                qmax = qmax.max(w[0].discharge); wmax = wmax.max(w[0].width);
            }
            if r.path.iter().any(|v| v.discharge > 5.0) { big += 1; }
        }
        let lake_cells = m.lake.iter().filter(|&&l| l != worldmap::NO_LAKE).count();
        let lake_area = lake_cells as f64 * m.cell * m.cell;
        let mut q: Vec<f32> = m.rivers.iter().filter_map(|r| r.path.last().map(|v| v.discharge)).collect();
        q.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = q.len();
        println!("seed {seed}: map {}x{} cells of {} m; rivers {} ({} carry > 5 m3/s); total length {:.0} km; mean width {:.1} m, max {:.1} m; channel area {:.1} km2 = {:.2e} columns of 1 m2; max discharge {:.1} m3/s; end discharge median {:.2} p90 {:.2}; lakes {} covering {:.1} km2 ({:.2e} columns)",
            m.n, m.n, m.cell, m.rivers.len(), big, len / 1000.0, wsum_len / len, wmax, area / 1e6, area, qmax, q[n / 2], q[n * 9 / 10], m.lakes.len(), lake_area / 1e6, lake_area);
        let mut lens: Vec<f64> = m.rivers.iter().map(|r| r.path.windows(2).map(|w| ((w[1].x - w[0].x) as f64).hypot((w[1].z - w[0].z) as f64)).sum()).collect();
        lens.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("   river length median {:.1} km, max {:.1} km", lens[lens.len() / 2] / 1000.0, lens[lens.len() - 1] / 1000.0);
    }
}
