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

#[derive(Debug, Default)]
pub struct Shores {
    pub fields: Vec<Shore>,
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
                (e && s) || (s && w) || (w && nn) || (nn && e)
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
        Self { fields }
    }

    /// The lake whose field covers face-local (x, z) most deeply, if any.
    pub fn query(&self, map: &WorldMap, x: f64, z: f64) -> Option<LakeHit> {
        let u = (x - map.x0) / map.cell;
        let v = (z - map.x0) / map.cell;
        let mut best: Option<LakeHit> = None;
        for f in &self.fields {
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
