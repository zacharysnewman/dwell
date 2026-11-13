//! The distant terrain of a grid while standing on it (docs/proposals/MOON_WORLD.md §5; BUGS.md
//! #143, #145): the moon and the sun draw past their streamed chunks as `lod.levels` square rings
//! of their real surface (`grids.json` `lod`), centred on the viewer, each `lod.cells` quads a side
//! with level k's quads `cellM` × 2^k wide and the next finer ring in its middle, out past the
//! disc's rim (its horizon). Every vertex is its ground from the grid's [`FarSource`] (the same
//! generator its chunks and far bake come from, sampled at the quad's scale) less `sinkM` +
//! `sinkPerCell` × the quad's width, so the chunks cover the rings where they are. A ring is
//! rebuilt when the viewer has moved a quad of it from its centre (its centre snaps to two of its
//! quads, so the finer ring's square always lies on its quads' edges), coarsest first, a few a
//! frame. Any grid with a `lod` and a generator with a far source has one: no per-grid code.
//!
//! This extends M-5's single coarse mesh rather than porting home's octree LOD (`dwell-lod`
//! `LodSystem`), which is home's: its sections come from the worldgen workers, its index from
//! the server and its tree from home's world bounds.

use dwell_mesh::{ChunkMeshes, tile_rect};
use dwell_world::{Face, FaceFrame};
use dwell_worldgen::grids::{FarSource, GridLodRule};

/// The piece id of ring `level` (the drawn grid's; in the grid meshes' range, after their ids).
pub fn id(level: usize) -> u32 {
    crate::grid_bodies::ID_BASE | 0x80 | level as u32
}

/// A ring's centre: the viewer's (x, z) snapped to two of its quads, in quads.
pub fn centre(rule: &GridLodRule, level: usize, x: f64, z: f64) -> (i64, i64) {
    let two = 2.0 * rule.cell(level);
    (2 * (x / two).round() as i64, 2 * (z / two).round() as i64)
}

/// Ring `level`'s square (face-local x and z, m): its low and high corners round `c`.
pub fn square(rule: &GridLodRule, level: usize, c: (i64, i64)) -> ([f64; 2], [f64; 2]) {
    let cell = rule.cell(level);
    let half = (rule.cells / 2) as i64;
    (
        [((c.0 - half) as f64) * cell, ((c.1 - half) as f64) * cell],
        [((c.0 + half) as f64) * cell, ((c.1 + half) as f64) * cell],
    )
}

/// Ring `level` of face `face` round centre `c` (face-local, in the grid's coordinates through the
/// face's frame), the quads inside `hole` (the finer ring's square) left out, and those with a
/// corner off the disc. Coloured by the ground's albedo, normals by central differences; an outer
/// skirt hangs the sink and a quarter quad down (over the midplane) so no gap shows against the next ring. Returns the mesh and its
/// radius round the grid's origin.
pub fn ring(
    src: &(impl FarSource + ?Sized),
    face: Face,
    rule: &GridLodRule,
    level: usize,
    c: (i64, i64),
    hole: Option<([f64; 2], [f64; 2])>,
) -> (ChunkMeshes, f64) {
    let f = FaceFrame::of(face);
    let cell = rule.cell(level);
    let n = rule.cells;
    let side = n + 1;
    let r = src.far_radius_m();
    let (lo, _) = square(rule, level, c);
    let at = |i: usize, j: usize| (lo[0] + i as f64 * cell, lo[1] + j as f64 * cell);
    let sink = rule.sink_m + rule.sink_per_cell * cell;
    let heights: Vec<f64> = (0..side * side)
        .map(|k| {
            let (x, z) = at(k % side, k / side);
            // Off the disc: the rim's ground, so a quad there is left out but its neighbour's
            // normal stays sane.
            let s = (r / x.hypot(z).max(1e-9)).min(1.0);
            src.far_height(face, x * s, z * s, cell) - sink
        })
        .collect();
    let h = |i: usize, j: usize| heights[j.min(n) * side + i.min(n)];
    let tile = tile_rect("plain");
    let mut m = ChunkMeshes::default();
    let o = &mut m.opaque;
    let mut radius: f64 = 0.0;
    let mut vertex =
        |o: &mut dwell_mesh::MeshArrays, x: f64, y: f64, z: f64, nrm: [f64; 3], col: [f64; 3]| {
            let p = f.point([x, y, z]);
            radius = radius.max(p.iter().map(|c| c * c).sum::<f64>().sqrt());
            o.positions.extend(p.map(|v| v as f32));
            o.normals.extend(f.vector(nrm).map(|v| v as f32));
            o.colors.extend(col.map(|v| v as f32));
            o.uvs.extend([x as f32, z as f32]);
            o.tiles.extend([
                tile.u0 as f32,
                tile.v0 as f32,
                (tile.u1 - tile.u0) as f32,
                (tile.v1 - tile.v0) as f32,
            ]);
            o.light.push(1.0);
            o.flood.extend([1.0, 0.0]);
            (o.positions.len() / 3 - 1) as u32
        };
    let mut colour = vec![[0.0; 3]; side * side];
    for j in 0..side {
        for i in 0..side {
            let (x, z) = at(i, j);
            let dx = (h(i + 1, j) - h(i.saturating_sub(1), j)) / (2.0 * cell);
            let dz = (h(i, j + 1) - h(i, j.saturating_sub(1))) / (2.0 * cell);
            let l = (dx * dx + 1.0 + dz * dz).sqrt();
            let col = src.far_albedo(face, x, z);
            colour[j * side + i] = col;
            vertex(o, x, h(i, j), z, [-dx / l, 1.0 / l, -dz / l], col);
        }
    }
    let inside = |i: usize, j: usize| {
        let (x, z) = at(i, j);
        x.hypot(z) <= r
    };
    let in_hole = |i: usize, j: usize| {
        hole.is_some_and(|(a, b)| {
            let (x0, z0) = at(i, j);
            let (x1, z1) = at(i + 1, j + 1);
            x0 >= a[0] - 1e-6 && x1 <= b[0] + 1e-6 && z0 >= a[1] - 1e-6 && z1 <= b[1] + 1e-6
        })
    };
    let v = |i: usize, j: usize| (j * side + i) as u32;
    let quad = |o: &mut dwell_mesh::MeshArrays, a: u32, b: u32, c: u32, d: u32| {
        // Counter-clockwise seen from over the face; a face B point is mirrored, so its winding
        // turns.
        if f.is_b() {
            o.indices.extend([a, c, b, a, d, c]);
        } else {
            o.indices.extend([a, b, c, a, c, d]);
        }
    };
    for j in 0..n {
        for i in 0..n {
            if in_hole(i, j)
                || !(inside(i, j) && inside(i + 1, j) && inside(i, j + 1) && inside(i + 1, j + 1))
            {
                continue;
            }
            quad(o, v(i, j), v(i, j + 1), v(i + 1, j + 1), v(i + 1, j));
        }
    }
    // The skirt round the outer edge: each edge's vertices again lower, facing out.
    let edge: Vec<(usize, usize)> = (0..n)
        .map(|i| (i, 0))
        .chain((0..n).map(|j| (n, j)))
        .chain((0..n).map(|i| (n - i, n)))
        .chain((0..n).map(|j| (0, n - j)))
        .collect();
    for w in 0..edge.len() {
        let (a, b) = (edge[w], edge[(w + 1) % edge.len()]);
        if !(inside(a.0, a.1) && inside(b.0, b.1)) {
            continue;
        }
        let mut low = |o: &mut dwell_mesh::MeshArrays, (i, j): (usize, usize)| {
            let (x, z) = at(i, j);
            let out = [
                x - (lo[0] + n as f64 * cell / 2.0),
                0.0,
                z - (lo[1] + n as f64 * cell / 2.0),
            ];
            let l = out[0].hypot(out[2]).max(1e-9);
            vertex(
                o,
                x,
                (h(i, j) - sink - 0.25 * cell).max(1.0),
                z,
                [out[0] / l, 0.0, out[2] / l],
                colour[j * side + i],
            )
        };
        let (la, lb) = (low(o, a), low(o, b));
        // Seen from outside, a is on the right and b on the left: a, b, then b's and a's feet,
        // counter-clockwise.
        quad(o, v(a.0, a.1), v(b.0, b.1), lb, la);
    }
    (m, radius)
}

/// The rings of the grid the view is on: which centres are built, and the grid.
#[derive(Clone, Debug, Default)]
pub struct GridLod {
    /// The grid (its `grids.json` index) and face the rings are of.
    on: Option<(u8, Face)>,
    centres: Vec<Option<(i64, i64)>>,
}

/// A change to a ring's piece: its level and its new mesh (and radius), or `None` to drop it.
pub type RingChange = (usize, Option<(ChunkMeshes, f64)>);

impl GridLod {
    /// The rings for a view on grid `grid`'s face `face` with the eye at face-local (x, z): drops
    /// every ring when the grid or face changes (a crossing) and rebuilds at most `budget` stale
    /// rings, coarsest first. `src` is the grid's far source and `rule` its `lod`; with none (home,
    /// or a grid without a `lod`) every ring held is dropped.
    pub fn update(
        &mut self,
        on: Option<(u8, Face, &GridLodRule, &(dyn FarSource + Sync))>,
        x: f64,
        z: f64,
        budget: usize,
    ) -> Vec<RingChange> {
        let mut out = Vec::new();
        let key = on.map(|(g, f, ..)| (g, f));
        if key != self.on {
            out.extend(
                self.centres
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.is_some())
                    .map(|(k, _)| (k, None)),
            );
            self.centres.clear();
            self.on = key;
        }
        let Some((_, face, rule, src)) = on else {
            return out;
        };
        self.centres.resize(rule.levels, None);
        let want: Vec<(i64, i64)> = (0..rule.levels).map(|k| centre(rule, k, x, z)).collect();
        let mut built = 0;
        for k in (0..rule.levels).rev() {
            if built >= budget {
                break;
            }
            // A ring is stale when its centre moved, or when the finer ring's square moved (its
            // hole follows it).
            let fine = (k > 0).then(|| want[k - 1]);
            let held = self.centres[k];
            let hole_moved = k > 0 && self.centres[k - 1] != fine;
            if held == Some(want[k]) && !hole_moved {
                continue;
            }
            let hole = fine.map(|c| square(rule, k - 1, c));
            out.push((k, Some(ring(src, face, rule, k, want[k], hole))));
            self.centres[k] = Some(want[k]);
            built += 1;
        }
        out
    }

    /// The levels whose rings are held.
    pub fn held(&self) -> impl Iterator<Item = usize> + '_ {
        self.centres
            .iter()
            .enumerate()
            .filter_map(|(k, c)| c.map(|_| k))
    }

    /// Whether every ring is built round the eye at (x, z) (none stale).
    pub fn settled(&self, rule: &GridLodRule, x: f64, z: f64) -> bool {
        self.centres.len() == rule.levels
            && (0..rule.levels).all(|k| self.centres[k] == Some(centre(rule, k, x, z)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_worldgen::grids::{CrateredTerrain, grid};
    use dwell_worldgen::volcanic::VolcanicTerrain;

    fn tri_centres(m: &ChunkMeshes) -> Vec<[f64; 3]> {
        let o = &m.opaque;
        let p = |i: u32| {
            let i = i as usize * 3;
            [o.positions[i], o.positions[i + 1], o.positions[i + 2]].map(f64::from)
        };
        o.indices
            .chunks(3)
            .map(|t| {
                let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
                std::array::from_fn(|k| (a[k] + b[k] + c[k]) / 3.0)
            })
            .collect()
    }

    fn settle(
        lod: &mut GridLod,
        g: u8,
        f: FaceFrame,
        rule: &GridLodRule,
        src: &(dyn FarSource + Sync),
        x: f64,
        z: f64,
    ) -> Vec<Option<(ChunkMeshes, f64)>> {
        let mut rings: Vec<Option<(ChunkMeshes, f64)>> = vec![None; rule.levels];
        for _ in 0..rule.levels + 1 {
            for (k, m) in lod.update(Some((g, f.face, rule, src)), x, z, 3) {
                rings[k] = m;
            }
        }
        assert!(lod.settled(rule, x, z));
        rings
    }

    /// On grid `name` from (x, z) on face `f`: the rings reach the disc's rim (its horizon) in
    /// every direction, past the chunks, and lie under the ground near the eye.
    fn reaches_the_horizon(name: &str, src: &(dyn FarSource + Sync), f: FaceFrame, x: f64, z: f64) {
        let def = grid(name).unwrap();
        let rule = def.lod.expect("a lod");
        let r = def.radius_m.unwrap();
        let mut lod = GridLod::default();
        let rings = settle(&mut lod, 1, f, &rule, src, x, z);
        // The farthest drawn ground in 16 directions from the eye.
        let mut far = [0.0f64; 16];
        let mut beyond_chunks = 0;
        for (mesh, _) in rings.iter().flatten() {
            for c in tri_centres(mesh) {
                assert!(f.point(c)[1] > 0.0, "on face {:?}'s side: {c:?}", f.face);
                let (dx, dz) = (c[0] - x, c[2] - z);
                let d = dx.hypot(dz);
                let a = ((dz.atan2(dx) + std::f64::consts::PI) / std::f64::consts::TAU * 16.0)
                    as usize
                    % 16;
                far[a] = far[a].max(d);
                beyond_chunks += usize::from(d > 300.0);
            }
        }
        assert!(
            beyond_chunks > 1000,
            "{name}: terrain past the chunks ({beyond_chunks})"
        );
        for (a, d) in far.iter().enumerate() {
            let th = (a as f64 + 0.5) / 16.0 * std::f64::consts::TAU - std::f64::consts::PI;
            // The rim along this direction from the eye.
            let (ux, uz) = (th.cos(), th.sin());
            let b = x * ux + z * uz;
            let rim = -b + (b * b - (x * x + z * z - r * r)).sqrt();
            assert!(
                *d > rim - 3.0 * rule.cell(rule.levels - 1),
                "{name} face {:?}: toward {a}/16 the ground ends {d:.0} m out, the rim {rim:.0} m",
                f.face
            );
        }
        // Under the ground near the eye (the chunks cover it): the finest ring's vertices.
        let (fine, _) = rings[0].as_ref().unwrap();
        let o = &fine.opaque;
        for v in o.positions.chunks(3).step_by(7) {
            let p = f.point([f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]);
            let ground = src.far_height(f.face, p[0], p[2], 0.0);
            assert!(
                p[1] <= ground + 0.5,
                "{name}: a ring vertex {:.1} m over the ground",
                p[1] - ground
            );
        }
    }

    dwell_world::both_faces!(
        /// BUGS.md #143: standing on the moon, its ground is drawn past the chunks out to its
        /// horizon (the disc's rim) every way, under the ground near the eye.
        fn on_the_moon_its_terrain_is_drawn_to_its_horizon(f: FaceFrame) {
            let t = CrateredTerrain::for_grid("dwell:moon", 7).unwrap();
            let [x, _, z] = t.spawn_point(f.face);
            reaches_the_horizon("dwell:moon", &t, f, x, z);
            reaches_the_horizon("dwell:moon", &t, f, 9000.0, -4000.0);
        }
    );

    dwell_world::both_faces!(
        /// BUGS.md #145: the same on the sun, from its middle and near its rim.
        fn on_the_sun_its_terrain_is_drawn_to_its_horizon(f: FaceFrame) {
            let t = VolcanicTerrain::new(grid("dwell:sun").unwrap(), 7).unwrap();
            reaches_the_horizon("dwell:sun", &t, f, 0.0, 0.0);
            reaches_the_horizon("dwell:sun", &t, f, -20_000.0, 6000.0);
        }
    );

    dwell_world::both_faces!(
        /// The rings nest without gaps or overlaps: each quad of a coarser ring is outside the
        /// finer ring's square, and the finer square lies on the coarser ring's quad edges, after
        /// walks that move each ring a different number of times; moving rebuilds only the rings
        /// whose centres moved; a crossing drops every ring.
        fn the_rings_nest_and_follow_the_eye(f: FaceFrame) {
            let t = CrateredTerrain::for_grid("dwell:moon", 3).unwrap();
            let rule = grid("dwell:moon").unwrap().lod.unwrap();
            let mut lod = GridLod::default();
            settle(&mut lod, 1, f, &rule, &t, 10.0, 20.0);
            for (x, z) in [(13.0, 21.0), (120.0, -40.0), (3000.0, 2500.0)] {
                let changes = lod.update(Some((1, f.face, &rule, &t)), x, z, 99);
                let moved: Vec<usize> = changes.iter().map(|c| c.0).collect();
                for k in 0..rule.levels {
                    let c = centre(&rule, k, x, z);
                    if k > 0 {
                        let (a, b) = square(&rule, k - 1, centre(&rule, k - 1, x, z));
                        let cell = rule.cell(k);
                        for e in [a[0], a[1], b[0], b[1]] {
                            assert!(
                                (e / cell - (e / cell).round()).abs() < 1e-9,
                                "on quad edges"
                            );
                        }
                        let (oa, ob) = square(&rule, k, c);
                        assert!(
                            oa[0] <= a[0] && oa[1] <= a[1] && ob[0] >= b[0] && ob[1] >= b[1],
                            "nested"
                        );
                    }
                }
                assert!(lod.settled(&rule, x, z));
                if (x, z) == (13.0, 21.0) {
                    assert!(
                        moved.len() <= 2,
                        "a small step moves at most the finest ring and the one round it: {moved:?}"
                    );
                }
            }
            let dropped = lod.update(None, 0.0, 0.0, 99);
            assert_eq!(dropped.len(), rule.levels);
            assert!(dropped.iter().all(|c| c.1.is_none()));
        }
    );
}
