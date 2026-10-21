//! Rivers: the dry land cells whose discharge reaches `river_threshold`, traced as polylines from
//! each source (a river cell no river flows into, or a lake's outlet) down the receivers to the sea,
//! a lake or the river it joins. Width and depth come from the discharge through the profile's
//! curve (`river_geometry`, piecewise linear, held beyond its ends). Where a river of at least
//! `mouth_min_discharge` meets the sea its mouth is marked: a delta where the slope `mouth_rule`
//! measures over `mouth_reach` m is at most `delta_max_slope`, an estuary otherwise. The rules: the
//! river's own fall over its last `mouth_reach` m, or the coast's relief (the highest land within
//! `mouth_reach` m of the mouth), so a valley cut into a steep coast is an estuary and a mouth on a
//! low plain a delta.

use super::flood::{Grid, NONE};
use super::{End, HydrologyParams, Mouth, MouthKind, MouthRule, River, RiverPoint, Vertex};

/// Width and depth (m) for a discharge (m³/s) on the curve.
pub fn geometry(curve: &[RiverPoint], q: f64) -> (f64, f64) {
    let first = curve[0];
    if q <= first.discharge {
        return (first.width, first.depth);
    }
    for w in curve.windows(2) {
        let (a, b) = (w[0], w[1]);
        if q <= b.discharge {
            let t = (q - a.discharge) / (b.discharge - a.discharge);
            return (
                a.width + (b.width - a.width) * t,
                a.depth + (b.depth - a.depth) * t,
            );
        }
    }
    let last = curve[curve.len() - 1];
    (last.width, last.depth)
}

pub struct Inputs<'a> {
    pub grid: Grid,
    pub cell: f64,
    pub x0: f64,
    pub sea: &'a [bool],
    pub lake_of: &'a [u32],
    pub outlets: &'a [u32],
    pub receiver: &'a [u32],
    pub discharge: &'a [f64],
    pub surface: &'a [f64],
}

pub fn trace(inp: &Inputs, p: &HydrologyParams) -> (Vec<River>, Vec<Mouth>) {
    let len = inp.grid.len();
    let n = inp.grid.n;
    let river =
        |i: usize| !inp.sea[i] && inp.lake_of[i] == NONE && inp.discharge[i] >= p.river_threshold;
    // A river cell with a river donor is not a source.
    let mut fed = vec![false; len];
    for i in 0..len {
        if river(i) {
            let r = inp.receiver[i];
            if r != NONE {
                fed[r as usize] = true;
            }
        }
    }
    let mut starts: Vec<(u32, Option<u32>)> = Vec::new();
    for (id, &o) in inp.outlets.iter().enumerate() {
        if o != NONE && river(o as usize) {
            starts.push((o, Some(id as u32)));
        }
    }
    for (i, &f) in fed.iter().enumerate() {
        if river(i) && !f {
            starts.push((i as u32, None));
        }
    }
    let at = |i: usize| {
        (
            inp.x0 + (i % n) as f64 * inp.cell,
            inp.x0 + (i / n) as f64 * inp.cell,
        )
    };
    let vertex = |i: usize, surface: f64| {
        let (x, z) = at(i);
        let q = inp.discharge[i];
        let (width, depth) = geometry(p.river_geometry, q);
        Vertex {
            x,
            z,
            surface,
            discharge: q,
            width,
            depth,
        }
    };
    let mut visited = vec![false; len];
    let mut rivers = Vec::new();
    let mut mouths = Vec::new();
    for (start, from_lake) in starts {
        let mut c = start as usize;
        if visited[c] {
            continue;
        }
        let mut path = Vec::new();
        let mut cells = Vec::new();
        let end;
        loop {
            visited[c] = true;
            path.push(vertex(c, inp.surface[c]));
            cells.push(c as u32);
            let r = inp.receiver[c] as usize;
            if inp.sea[r] {
                let mut v = vertex(c, 0.0);
                let (x, z) = at(r);
                (v.x, v.z) = (x, z);
                path.push(v);
                end = End::Sea;
                break;
            }
            if inp.lake_of[r] != NONE {
                path.push(vertex(r, inp.surface[r]));
                end = End::Lake(inp.lake_of[r]);
                break;
            }
            if visited[r] {
                path.push(vertex(r, inp.surface[r]));
                end = End::Joins;
                break;
            }
            c = r;
        }
        if end == End::Sea {
            let last = cells[cells.len() - 1] as usize;
            let q = inp.discharge[last];
            if q >= p.mouth_min_discharge {
                let reach = ((p.mouth_reach / inp.cell).round() as usize).max(1);
                let slope = match p.mouth_rule {
                    MouthRule::RiverSlope => {
                        // The mean fall over the river's last `mouth_reach` m.
                        let k = cells.len().min(reach);
                        let from = cells[cells.len() - k] as usize;
                        inp.surface[from] / ((k as f64) * inp.cell)
                    }
                    MouthRule::CoastRelief => {
                        // The highest land within `mouth_reach` m of the mouth, over that distance:
                        // a valley cut into high ground (a ria) or a low coastal plain.
                        let r = reach as i64;
                        let (cx, cz) = ((last % n) as i64, (last / n) as i64);
                        let mut top = 0.0f64;
                        for dz in -r..=r {
                            for dx in -r..=r {
                                let (x, z) = (cx + dx, cz + dz);
                                if dx * dx + dz * dz > r * r
                                    || x < 0
                                    || z < 0
                                    || x >= n as i64
                                    || z >= n as i64
                                {
                                    continue;
                                }
                                let j = z as usize * n + x as usize;
                                if !inp.sea[j] {
                                    top = top.max(inp.surface[j]);
                                }
                            }
                        }
                        top / p.mouth_reach
                    }
                };
                let (x, z) = at(last);
                mouths.push(Mouth {
                    x,
                    z,
                    discharge: q,
                    slope,
                    river: rivers.len() as u32,
                    kind: if slope <= p.delta_max_slope {
                        MouthKind::Delta
                    } else {
                        MouthKind::Estuary
                    },
                });
            }
        }
        rivers.push(River {
            from_lake,
            end,
            path,
            cells,
        });
    }
    (rivers, mouths)
}
