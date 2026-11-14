//! The lakes' shores, derived from the map on load (ADR 0049; WORLD_GENERATION.md §3.4): per lake,
//! over its cells' box and a margin, the signed distance (m) from each cell's centre to the lake's
//! edge (midway between a lake cell and its dry neighbour): positive inside. Columns read it
//! bilinearly, so a shore is a smooth line through the map's cells rather than their squares.

use super::WorldMap;
use crate::terrain::SURFACE as S;

/// One lake's distance field.
#[derive(Clone, Debug, PartialEq)]
pub struct Shore {
    /// The lake (index into `WorldMap::lakes`).
    pub lake: u16,
    /// The field's first cell and size (grid cells).
    pub i0: i64,
    pub j0: i64,
    pub w: i64,
    pub h: i64,
    pub sdf: Vec<f32>,
}

/// A column's lake: its index, level (m above sea level) and signed distance to its shore (m).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LakeHit {
    pub lake: u16,
    pub level: f64,
    pub sdf: f64,
}

/// The lakes' index: map cells per bucket side (a bucket lists the fields whose box reaches it).
const BUCKET: i64 = 16;

#[derive(Debug, Default)]
pub struct Shores {
    pub fields: Vec<Shore>,
    /// Buckets per side, and per bucket (row-major) the fields reaching it, in `fields`' order, so
    /// a query reads only the lakes near its column and breaks ties as a scan of all would.
    side: i64,
    buckets: Vec<Vec<u32>>,
}

impl Shores {
    pub fn build(map: &WorldMap) -> Self {
        let n = map.n as i64;
        let mut fields = Vec::new();
        // Each lake's box of cells.
        let mut boxes: Vec<Option<(i64, i64, i64, i64)>> = vec![None; map.lakes.len()];
        for (c, &l) in map.lake.iter().enumerate() {
            let Some(b) = boxes.get_mut(usize::from(l).wrapping_sub(1)) else {
                continue;
            };
            let (i, j) = ((c % map.n) as i64, (c / map.n) as i64);
            *b = Some(match *b {
                None => (i, j, i, j),
                Some((a, b2, c2, d)) => (a.min(i), b2.min(j), c2.max(i), d.max(j)),
            });
        }
        let reach = ((S.lake_levee + S.lake_shore_noise) / map.cell).ceil() as i64 + 2;
        for (k, b) in boxes.iter().enumerate() {
            let Some((bi0, bj0, bi1, bj1)) = *b else {
                continue;
            };
            let id = (k + 1) as u16;
            let (i0, j0) = ((bi0 - reach).max(0), (bj0 - reach).max(0));
            let (i1, j1) = ((bi1 + reach).min(n - 1), (bj1 + reach).min(n - 1));
            let (w, h) = (i1 - i0 + 1, j1 - j0 + 1);
            let lake = |i: i64, j: i64| {
                (0..n).contains(&i) && (0..n).contains(&j) && map.lake[(j * n + i) as usize] == id
            };
            // Closed over diagonal steps: a dry cell whose two neighbours round one corner are the
            // lake's (and not far above its level) is the lake's too, so a lake one
            // cell wide that runs diagonally is one stretch of water, not a string of ponds.
            let level = map.lakes[k].level;
            let inside = |i: i64, j: i64| {
                if lake(i, j) {
                    return true;
                }
                if !(0..n).contains(&i)
                    || !(0..n).contains(&j)
                    || map.height[(j * n + i) as usize] > level + 25.0
                {
                    return false;
                }
                let (e, w, s, nn) = (
                    lake(i + 1, j),
                    lake(i - 1, j),
                    lake(i, j + 1),
                    lake(i, j - 1),
                );
                (e || w) && (s || nn)
            };
            let mut sdf = Vec::with_capacity((w * h) as usize);
            let far = (reach as f64) * map.cell;
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let me = inside(i, j);
                    let mut best = i64::MAX;
                    for dj in -reach..=reach {
                        for di in -reach..=reach {
                            if inside(i + di, j + dj) != me {
                                best = best.min(di * di + dj * dj);
                            }
                        }
                    }
                    let d = if best == i64::MAX {
                        far
                    } else {
                        (best as f64).sqrt() * map.cell - 0.5 * map.cell
                    };
                    sdf.push((if me { d } else { -d }) as f32);
                }
            }
            fields.push(Shore {
                lake: k as u16,
                i0,
                j0,
                w,
                h,
                sdf,
            });
        }
        Self::indexed(fields, n)
    }

    /// The fields with their index over a map `n` cells wide.
    fn indexed(fields: Vec<Shore>, n: i64) -> Self {
        let side = (n + BUCKET - 1) / BUCKET;
        let mut buckets = vec![Vec::new(); (side * side) as usize];
        for (k, f) in fields.iter().enumerate() {
            let (b0, c0) = (f.i0 / BUCKET, f.j0 / BUCKET);
            let b1 = ((f.i0 + f.w - 1) / BUCKET).min(side - 1);
            let c1 = ((f.j0 + f.h - 1) / BUCKET).min(side - 1);
            for c in c0..=c1 {
                for b in b0..=b1 {
                    buckets[(c * side + b) as usize].push(k as u32);
                }
            }
        }
        Self {
            fields,
            side,
            buckets,
        }
    }

    /// The lake whose field covers face-local (x, z) most deeply, if any.
    pub fn query(&self, map: &WorldMap, x: f64, z: f64) -> Option<LakeHit> {
        let u = (x - map.x0) / map.cell;
        let v = (z - map.x0) / map.cell;
        let (b, c) = ((u / BUCKET as f64).floor(), (v / BUCKET as f64).floor());
        let side = self.side as f64;
        if !(0.0..side).contains(&b) || !(0.0..side).contains(&c) {
            return None;
        }
        let near = &self.buckets[(c as i64 * self.side + b as i64) as usize];
        Self::best_of(map, near.iter().map(|&k| &self.fields[k as usize]), u, v)
    }

    /// `query` by a scan of every lake (the index's reference in tests).
    #[cfg(test)]
    fn query_all(&self, map: &WorldMap, x: f64, z: f64) -> Option<LakeHit> {
        let u = (x - map.x0) / map.cell;
        let v = (z - map.x0) / map.cell;
        Self::best_of(map, self.fields.iter(), u, v)
    }

    /// The deepest of `fields` covering map-grid point (u, v), the first on a tie.
    fn best_of<'a>(
        map: &WorldMap,
        fields: impl Iterator<Item = &'a Shore>,
        u: f64,
        v: f64,
    ) -> Option<LakeHit> {
        let mut best: Option<LakeHit> = None;
        for f in fields {
            let (fu, fv) = (u - f.i0 as f64, v - f.j0 as f64);
            if fu < 0.0 || fv < 0.0 || fu > (f.w - 1) as f64 || fv > (f.h - 1) as f64 {
                continue;
            }
            let (i, j) = (fu.floor() as i64, fv.floor() as i64);
            let (tx, tz) = (fu - i as f64, fv - j as f64);
            let at =
                |a: i64, b: i64| f64::from(f.sdf[(b.min(f.h - 1) * f.w + a.min(f.w - 1)) as usize]);
            let top = at(i, j) + (at(i + 1, j) - at(i, j)) * tx;
            let bottom = at(i, j + 1) + (at(i + 1, j + 1) - at(i, j + 1)) * tx;
            let sdf = top + (bottom - top) * tz;
            if best.is_none_or(|b| sdf > b.sdf) {
                best = Some(LakeHit {
                    lake: f.lake,
                    level: f64::from(map.lakes[usize::from(f.lake)].level),
                    sdf,
                });
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The points to compare at: a lattice over each field's box and a little past it (its edges,
    /// where buckets split it, included), and a coarse one over the whole map.
    fn points(map: &WorldMap, fields: &[Shore]) -> Vec<(f64, f64)> {
        let mut out = Vec::new();
        let at = |u: f64| map.x0 + u * map.cell;
        for f in fields {
            for t in 0..=24 {
                for s in 0..=24 {
                    let u = f.i0 as f64 - 1.0 + (f.w + 1) as f64 * f64::from(s) / 24.0;
                    let v = f.j0 as f64 - 1.0 + (f.h + 1) as f64 * f64::from(t) / 24.0;
                    out.push((at(u), at(v)));
                }
            }
        }
        let n = map.n as f64;
        for t in -2..=202 {
            for s in -2..=202 {
                out.push((at(n * f64::from(s) / 200.0), at(n * f64::from(t) / 200.0)));
            }
        }
        out
    }

    dwell_world::both_faces!(
        /// WORLD_SCALE.md §3.1: the lakes' index answers as the scan of every lake did, on each
        /// face's map (seeds 0, 1 and 7).
        fn the_lake_index_matches_a_scan_of_every_lake(f: dwell_world::FaceFrame) {
            let mut hits = 0;
            for seed in [0u64, 1, 7] {
                let map = crate::worldmap::for_seed(crate::bifacial::face_seed(seed, f.face));
                let shores = &map.shores;
                let pts = points(&map, &shores.fields);
                for &(x, z) in &pts {
                    let (a, b) = (shores.query(&map, x, z), shores.query_all(&map, x, z));
                    assert_eq!(a, b, "seed {seed} at ({x}, {z})");
                    hits += usize::from(a.is_some());
                }
            }
            assert!(hits > 100, "{hits} points hit a lake");
        }
    );

    dwell_world::both_faces!(
        /// Many overlapping lakes (a fixture of 600 fields, ties included, over a face's map): the
        /// index finds the deepest, the first on a tie, as the scan does.
        fn the_lake_index_matches_a_scan_over_many_overlapping_lakes(f: dwell_world::FaceFrame) {
            let map = crate::worldmap::for_seed(crate::bifacial::face_seed(0, f.face));
            let n = map.n as i64;
            let mut rng = 0x9E37_79B9_7F4A_7C15u64;
            let mut next = |m: i64| {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                (rng % m as u64) as i64
            };
            let fields: Vec<Shore> = (0..600)
                .map(|k| {
                    let (w, h) = (1 + next(60), 1 + next(60));
                    let (i0, j0) = (next(n - w), next(n - h));
                    let sdf = (0..w * h).map(|_| (next(9) - 4) as f32).collect();
                    Shore {
                        lake: (k % map.lakes.len()) as u16,
                        i0,
                        j0,
                        w,
                        h,
                        sdf,
                    }
                })
                .collect();
            let shores = Shores::indexed(fields, n);
            let mut hits = 0;
            for (x, z) in points(&map, &shores.fields) {
                let (a, b) = (shores.query(&map, x, z), shores.query_all(&map, x, z));
                assert_eq!(a, b, "at ({x}, {z})");
                hits += usize::from(a.is_some());
            }
            assert!(hits > 10_000, "{hits} hits");
        }
    );
}
