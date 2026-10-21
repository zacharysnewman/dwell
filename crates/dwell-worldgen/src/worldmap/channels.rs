//! The rivers' channels, derived from the map's polylines on load (ADR 0049; WORLD_GENERATION.md
//! §3.3): each river smoothed by corner cutting (`rivers.smooth` times; its ends kept, so it still
//! leaves its lake and reaches the sea), a river joining another snapped onto the other's smoothed
//! course, the mouths widened (estuaries deepened, deltas split into distributaries), and the
//! segments indexed in 64 m buckets for the per-column query: the nearest channel by its edge.
//!
//! Levels only ever fall downstream: smoothing takes convex combinations of the vertices and the
//! map's surfaces never rise along a river, so neither do the segments'.

use dwell_world::hash::HashMap;

use super::{Mouth, RiverEnd, WorldMap};
use crate::hydrology::math::sin_cos;
use crate::terrain::SURFACE as S;

const BUCKET: f64 = 64.0;

/// A point of a channel's course: face-local m, `s` the water surface (m above sea level).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub z: f64,
    pub s: f64,
    pub w: f64,
    pub d: f64,
}

impl Point {
    fn lerp(a: Point, b: Point, t: f64) -> Point {
        let m = |p: f64, q: f64| p + (q - p) * t;
        Point {
            x: m(a.x, b.x),
            z: m(a.z, b.z),
            s: m(a.s, b.s),
            w: m(a.w, b.w),
            d: m(a.d, b.d),
        }
    }
}

/// One straight piece of a channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seg {
    pub a: Point,
    pub b: Point,
    /// How far from its centreline it shapes the ground (m).
    pub radius: f64,
    pub river: u32,
}

/// A column's channel: the nearest by its edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    /// Distance from the centreline (m).
    pub d: f64,
    /// Half the channel's width (m).
    pub half: f64,
    /// The water's surface (m above sea level).
    pub level: f64,
    /// The bed's depth at the centre (m).
    pub depth: f64,
    /// How far beyond the channel's edge its valley reaches (m).
    pub valley: f64,
    /// The flow's direction (a unit vector, downstream).
    pub dir: (f64, f64),
    pub river: u32,
}

#[derive(Debug, Default)]
pub struct Channels {
    pub segs: Vec<Seg>,
    /// The smoothed courses, rivers first (by index), then the deltas' distributaries.
    pub courses: Vec<Vec<Point>>,
    buckets: HashMap<(i32, i32), Vec<u32>>,
}

/// Corner cutting (Chaikin), the ends kept.
fn smooth(p: &[Point], times: u32) -> Vec<Point> {
    let mut p = p.to_vec();
    for _ in 0..times {
        if p.len() < 3 {
            break;
        }
        let mut out = Vec::with_capacity(2 * p.len());
        out.push(p[0]);
        for w in p.windows(2) {
            out.push(Point::lerp(w[0], w[1], 0.25));
            out.push(Point::lerp(w[0], w[1], 0.75));
        }
        out.push(p[p.len() - 1]);
        p = out;
    }
    p
}

/// The point of `course` nearest (x, z), with its interpolated values.
fn project(course: &[Point], x: f64, z: f64) -> Option<Point> {
    let mut best: Option<(f64, Point)> = None;
    for w in course.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (ex, ez) = (b.x - a.x, b.z - a.z);
        let l2 = ex * ex + ez * ez;
        let t = if l2 > 0.0 {
            (((x - a.x) * ex + (z - a.z) * ez) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let p = Point::lerp(a, b, t);
        let d2 = (p.x - x) * (p.x - x) + (p.z - z) * (p.z - z);
        if best.is_none_or(|(bd, _)| d2 < bd) {
            best = Some((d2, p));
        }
    }
    best.map(|(_, p)| p)
}

/// Arc length from each point to the course's end (m).
fn to_end(course: &[Point]) -> Vec<f64> {
    let mut out = vec![0.0; course.len()];
    for i in (0..course.len().saturating_sub(1)).rev() {
        let (a, b) = (course[i], course[i + 1]);
        out[i] = out[i + 1] + ((b.x - a.x) * (b.x - a.x) + (b.z - a.z) * (b.z - a.z)).sqrt();
    }
    out
}

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Channels {
    pub fn build(map: &WorldMap) -> Self {
        let rivers = &map.rivers;
        let mut courses: Vec<Vec<Point>> = Vec::with_capacity(rivers.len());
        for r in rivers {
            let mut pts: Vec<Point> = r
                .path
                .iter()
                .map(|v| Point {
                    x: f64::from(v.x),
                    z: f64::from(v.z),
                    s: f64::from(v.surface),
                    w: f64::from(v.width),
                    d: f64::from(v.depth),
                })
                .collect();
            // A lake's outlet: the channel starts in the lake, at its entry cell.
            if let Some(lake) = map.lakes.get(usize::from(r.from_lake).wrapping_sub(1))
                && (lake.entry as usize) < map.height.len()
                && let Some(&first) = pts.first()
            {
                let (x, z) = map.centre(lake.entry as usize);
                let s = f64::from(lake.level).max(first.s);
                pts.insert(0, Point { x, z, s, ..first });
            }
            let mut pts = smooth(&pts, S.river_smooth);
            if r.end == RiverEnd::Sea && r.mouth != Mouth::None && S.mouth_reach > 0.0 {
                let left = to_end(&pts);
                let (widen, deepen) = match r.mouth {
                    Mouth::Estuary => (S.estuary_widen, S.estuary_deepen),
                    _ => (S.delta_widen, 1.0),
                };
                for (p, &l) in pts.iter_mut().zip(&left) {
                    let t = smoothstep(1.0 - l / S.mouth_reach);
                    p.w *= 1.0 + (widen - 1.0) * t;
                    p.d *= 1.0 + (deepen - 1.0) * t;
                }
            }
            courses.push(pts);
        }
        // Never above the land it runs through: a smoothed course cuts corners across ground the
        // map's cells did not sample, so each point is held below the map's height there (outside
        // lakes) and the levels kept falling downstream.
        for c in &mut courses {
            lower_into_land(map, c);
        }
        // A river joining another ends on the other's smoothed course, never above it.
        for k in 0..rivers.len() {
            let RiverEnd::Joins(j) = rivers[k].end else {
                continue;
            };
            let j = j as usize;
            if j == k || j >= courses.len() || courses[k].len() < 2 {
                continue;
            }
            let last = courses[k][courses[k].len() - 1];
            if let Some(p) = project(&courses[j], last.x, last.z) {
                let n = courses[k].len();
                let prev = courses[k][n - 2].s;
                courses[k][n - 1] = Point {
                    x: p.x,
                    z: p.z,
                    s: p.s.min(prev),
                    ..last
                };
            }
        }
        let mut river_of: Vec<u32> = (0..courses.len() as u32).collect();
        // Deltas: distributaries from a point upstream of the mouth, fanning out to the sea.
        if S.delta_branches > 0 {
            for k in 0..rivers.len() {
                if rivers[k].mouth != Mouth::Delta || rivers[k].end != RiverEnd::Sea {
                    continue;
                }
                for b in delta_branches(map, &courses[k]) {
                    courses.push(b);
                    river_of.push(k as u32);
                }
            }
        }
        let mut segs = Vec::new();
        for (k, c) in courses.iter().enumerate() {
            for w in c.windows(2) {
                let (a, b) = (w[0], w[1]);
                if a.x == b.x && a.z == b.z {
                    continue;
                }
                let wmax = a.w.max(b.w);
                segs.push(Seg {
                    a,
                    b,
                    radius: 0.5 * wmax + S.river_reach + S.river_reach_per_width * wmax,
                    river: river_of[k],
                });
            }
        }
        let mut buckets: HashMap<(i32, i32), Vec<u32>> = HashMap::default();
        for (id, s) in segs.iter().enumerate() {
            // The query's point is warped (the meander) by up to twice its amplitude.
            let r = s.radius + 2.0 * S.river_meander + 2.0;
            let b = |v: f64| (v / BUCKET).floor() as i32;
            let (x0, x1) = (b(s.a.x.min(s.b.x) - r), b(s.a.x.max(s.b.x) + r));
            let (z0, z1) = (b(s.a.z.min(s.b.z) - r), b(s.a.z.max(s.b.z) + r));
            for bz in z0..=z1 {
                for bx in x0..=x1 {
                    buckets.entry((bx, bz)).or_default().push(id as u32);
                }
            }
        }
        Self {
            segs,
            courses,
            buckets,
        }
    }

    /// Whether any channel may reach within `extra` m of face-local (x, z) (a quick test before the
    /// meander's warp and the query).
    pub fn near(&self, x: f64, z: f64, extra: f64) -> bool {
        let span = (extra / BUCKET).ceil() as i32;
        let (bx, bz) = ((x / BUCKET).floor() as i32, (z / BUCKET).floor() as i32);
        (-span..=span).any(|dz| (-span..=span).any(|dx| self.buckets.contains_key(&(bx + dx, bz + dz))))
    }

    /// The nearest channel (by its edge) whose valley reaches face-local (x, z); its half-width at
    /// least `min_half` m (the LOD's widening, with `min_width`: only rivers at least that wide).
    pub fn query(&self, x: f64, z: f64, min_half: f64, min_width: f64) -> Option<Hit> {
        let span = (min_half / BUCKET).ceil() as i32;
        let (bx, bz) = ((x / BUCKET).floor() as i32, (z / BUCKET).floor() as i32);
        let mut best: Option<(f64, u32, Hit)> = None;
        for dz in -span..=span {
            for dx in -span..=span {
                let Some(list) = self.buckets.get(&(bx + dx, bz + dz)) else {
                    continue;
                };
                for &id in list {
                    if best.is_some_and(|(_, bid, _)| bid == id) {
                        continue;
                    }
                    let s = &self.segs[id as usize];
                    let (ex, ez) = (s.b.x - s.a.x, s.b.z - s.a.z);
                    let l2 = ex * ex + ez * ez;
                    let t = (((x - s.a.x) * ex + (z - s.a.z) * ez) / l2).clamp(0.0, 1.0);
                    let p = Point::lerp(s.a, s.b, t);
                    let d = ((x - p.x) * (x - p.x) + (z - p.z) * (z - p.z)).sqrt();
                    let mut half = 0.5 * p.w;
                    if min_half > half && p.w >= min_width {
                        half = min_half;
                    }
                    let valley = S.river_reach + S.river_reach_per_width * p.w;
                    if d >= half + valley {
                        continue;
                    }
                    let key = d - half;
                    if best.is_some_and(|(bk, bid, _)| bk < key || (bk == key && bid < id)) {
                        continue;
                    }
                    let l = l2.sqrt();
                    best = Some((
                        key,
                        id,
                        Hit {
                            d,
                            half,
                            level: p.s,
                            depth: p.d,
                            valley,
                            dir: (ex / l, ez / l),
                            river: s.river,
                        },
                    ));
                }
            }
        }
        best.map(|(_, _, h)| h)
    }
}

/// Holds a course's levels at least `RIVER_SINK` m below the map's height along it (outside lakes,
/// never below sea level) and falling downstream.
fn lower_into_land(map: &WorldMap, course: &mut [Point]) {
    let mut prev = f64::INFINITY;
    for p in course.iter_mut() {
        let c = map.cell_at(p.x, p.z);
        if map.lake[c] == super::NO_LAKE {
            let (h, _, _) = map.height_at(p.x, p.z);
            p.s = p.s.min((h - RIVER_SINK).max(0.0));
        }
        p.s = p.s.min(prev);
        prev = p.s;
    }
}

/// How far below the land a river's surface lies at least (m).
const RIVER_SINK: f64 = 0.5;

/// A delta's distributaries: from the point `delta_split` m upstream of the mouth, `delta_branches`
/// courses fanned `delta_spread` degrees either side of the river's last heading, each turning back
/// toward it until it reaches the sea, their surfaces falling evenly to sea level.
fn delta_branches(map: &WorldMap, course: &[Point]) -> Vec<Vec<Point>> {
    let left = to_end(course);
    let Some(i) = left.iter().rposition(|&l| l >= S.delta_split) else {
        return Vec::new();
    };
    let start = course[i];
    let end = course[course.len() - 1];
    let (mx, mz) = (end.x - start.x, end.z - start.z);
    let l0 = (mx * mx + mz * mz).sqrt();
    if l0 <= 0.0 {
        return Vec::new();
    }
    let (dx, dz) = (mx / l0, mz / l0);
    let n = S.delta_branches;
    let step = 40.0;
    let most = ((3.0 * left[i]) / step).ceil() as usize + 10;
    let mut out = Vec::new();
    for b in 0..n {
        let f = if n == 1 {
            1.0
        } else {
            2.0 * f64::from(b) / f64::from(n - 1) - 1.0
        };
        let (sin, cos) = sin_cos(f * S.delta_spread * std::f64::consts::PI / 180.0);
        let (mut hx, mut hz) = (dx * cos - dz * sin, dx * sin + dz * cos);
        let mut pts = vec![start];
        let (mut x, mut z) = (start.x, start.z);
        let mut reached = false;
        for _ in 0..most {
            x += hx * step;
            z += hz * step;
            pts.push(Point { x, z, ..start });
            if map.is_sea(map.cell_at(x, z)) {
                reached = true;
                break;
            }
            // Turn back toward the river's heading.
            let (nx, nz) = (hx + 0.08 * dx, hz + 0.08 * dz);
            let l = (nx * nx + nz * nz).sqrt();
            (hx, hz) = (nx / l, nz / l);
        }
        if !reached {
            continue;
        }
        let total = (pts.len() - 1) as f64;
        let k = pts.len();
        for (j, p) in pts.iter_mut().enumerate() {
            let t = j as f64 / total;
            p.s = start.s * (1.0 - t);
            p.w = start.w * S.delta_branch_width;
            p.d = start.d * S.delta_branch_width.max(0.5);
            if j + 1 == k {
                p.s = 0.0;
            }
        }
        out.push(smooth(&pts, 1));
    }
    out
}
