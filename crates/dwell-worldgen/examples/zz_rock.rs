use dwell_world::Face;
use dwell_worldgen::BifacialTerrain;
use dwell_worldgen::terrain::Band;
fn main() {
    let t = BifacialTerrain::new(7);
    let g = t.local(Face::A);
    let (x0, z0, step, w, l) = (17_000, 1_500, 8, 1000, 500);
    let mut grid = vec![false; (w * l) as usize];
    for j in 0..l { for i in 0..w { grid[(j * w + i) as usize] = g.column_at(x0 + i * step, z0 + j * step).band == Band::Rock; } }
    let mut seen = vec![false; grid.len()]; let mut sizes = Vec::new();
    for s in 0..grid.len() { if grid[s] && !seen[s] {
        let mut st = vec![s]; seen[s] = true; let mut n = 0;
        while let Some(c) = st.pop() { n += 1; let (i, j) = ((c as i32) % w, (c as i32) / w);
            for (di, dj) in [(1,0),(-1,0),(0,1),(0,-1)] { let (a, b) = (i + di, j + dj);
                if a >= 0 && b >= 0 && a < w && b < l { let k = (b * w + a) as usize; if grid[k] && !seen[k] { seen[k] = true; st.push(k); } } } }
        sizes.push(n);
    }}
    let small = sizes.iter().filter(|&&n| n * step * step < 50 * 50).count();
    println!("rock share {:.2} components {} smaller than 50 m sq: {}", grid.iter().filter(|&&b| b).count() as f64 / grid.len() as f64, sizes.len(), small);
}
