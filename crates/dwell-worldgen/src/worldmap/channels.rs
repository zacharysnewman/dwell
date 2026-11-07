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
    /// The river's discharge here (m³/s).
    pub q: f64,
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
            q: m(a.q, b.q),
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
    /// Its course's mean fall over `riverFallReachM` up and down stream of its middle (m per m):
    /// the fall an untouched river's current runs on where its own is less (`mean_fall`).
    pub mean_fall: f64,
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
    /// How fast the water's surface falls downstream (m per m).
    pub fall: f64,
    /// How fast it falls on average over `riverFallReachM` up and down its course (m per m): a
    /// reach held level where the land dips falls by the course's drops round it.
    pub mean_fall: f64,
    /// The river's discharge here (m³/s).
    pub q: f64,
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

/// Each river's flow along its course, in whole lip cells (`terrain::lip_cells`), as discharge:
/// its head's, the same down to each river joining it, which adds its own from the point it
/// joins at (rivers given by index; a join loop, never made by the map, is cut).
fn step_flows(rivers: &[super::MapRiver], courses: &mut [Vec<Point>]) {
    let per_cell =
        dwell_world::flow::sill_flow(dwell_world::flow::water_weir(), 1.0, dwell_world::CELL_H);
    if per_cell <= 0.0 {
        return;
    }
    let n = rivers.len().min(courses.len());
    let cells_at_head = |k: usize| -> u32 {
        courses[k]
            .first()
            .map_or(1, |p| ((p.q / per_cell).round() as u32).max(1))
    };
    let mut joins: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
    for k in 0..n {
        let RiverEnd::Joins(j) = rivers[k].end else {
            continue;
        };
        let j = j as usize;
        if j == k || j >= n || courses[k].is_empty() || courses[j].is_empty() {
            continue;
        }
        let last = courses[k][courses[k].len() - 1];
        let at = courses[j]
            .iter()
            .enumerate()
            .min_by(|a, b| {
                let d = |p: &Point| (p.x - last.x).powi(2) + (p.z - last.z).powi(2);
                d(a.1).total_cmp(&d(b.1))
            })
            .map_or(0, |(i, _)| i);
        joins[j].push((k, at));
    }
    // Each river's flow at its end (cells), tributaries first.
    let mut total: Vec<Option<u32>> = vec![None; n];
    let mut state = vec![0u8; n];
    fn end_cells(
        k: usize,
        joins: &[Vec<(usize, usize)>],
        head: &dyn Fn(usize) -> u32,
        total: &mut [Option<u32>],
        state: &mut [u8],
    ) -> u32 {
        if let Some(t) = total[k] {
            return t;
        }
        if state[k] == 1 {
            return 0; // a loop
        }
        state[k] = 1;
        let mut t = head(k);
        for &(trib, _) in &joins[k] {
            t += end_cells(trib, joins, head, total, state);
        }
        state[k] = 2;
        total[k] = Some(t);
        t
    }
    let mut along: Vec<Vec<f64>> = Vec::with_capacity(n);
    for k in 0..n {
        let head = cells_at_head(k);
        let mut add = vec![0u32; courses[k].len()];
        for &(trib, at) in &joins[k] {
            let c = end_cells(trib, &joins, &cells_at_head, &mut total, &mut state);
            if let Some(a) = add.get_mut(at) {
                *a += c;
            }
        }
        let mut run = head;
        along.push(
            add.iter()
                .map(|&a| {
                    run += a;
                    f64::from(run) * per_cell
                })
                .collect(),
        );
    }
    // A channel wide enough for its lips' cells on any heading (√2 as many metres across the
    // grid's diagonal) and two cells either side, so a lip has room to pour into all of them
    // (`terrain::LipPlan`).
    for (k, qs) in along.into_iter().enumerate() {
        for (p, q) in courses[k].iter_mut().zip(qs) {
            p.q = q;
            p.w =
                p.w.max(std::f64::consts::SQRT_2 * (q / per_cell).round() + 4.0);
        }
    }
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
                    q: f64::from(v.discharge),
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
        // A river joining another ends on the other's smoothed course, at its level: the other's
        // water backs up into it where it lies lower, and it falls to it gradually where higher.
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
                let end = Point {
                    x: p.x,
                    z: p.z,
                    s: p.s,
                    ..last
                };
                courses[k][n - 1] = end;
                let reach = 0.5 * (p.w + last.w) + S.river_join_margin;
                let bank = meet_at_bank(&mut courses[k], end.s, reach);
                ease_fall(&mut courses[k][..=bank], S.river_join_slope);
            }
        }
        // The flow its lips are sized for (ADR 0058): water comes into a stretch only where it is
        // seen to, so a river carries the same flow from its head to where another joins it and
        // their two flows together below: each river's lips the cells its head's discharge needs
        // (at least one), and a river joined the more by the joining river's own from there on.
        // The map's discharge growing along a river (runoff nobody sees come in) sizes no lip.
        step_flows(rivers, &mut courses);
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
        let reach = dwell_world::flow::FLOW.river_fall_reach_m;
        for (k, c) in courses.iter().enumerate() {
            let falls = mean_falls(c, reach);
            for (w, &mean_fall) in c.windows(2).zip(&falls) {
                let (a, b) = (w[0], w[1]);
                if a.x == b.x && a.z == b.z {
                    continue;
                }
                let wmax = a.w.max(b.w);
                segs.push(Seg {
                    a,
                    b,
                    radius: 0.5 * wmax + S.river_reach + S.river_reach_per_width * wmax,
                    mean_fall,
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
        (-span..=span)
            .any(|dz| (-span..=span).any(|dx| self.buckets.contains_key(&(bx + dx, bz + dz))))
    }

    /// The nearest channel (by its edge) whose valley reaches face-local (x, z); its half-width at
    /// least `min_half` m (the LOD's widening, with `min_width`: only rivers at least that wide).
    pub fn query(&self, x: f64, z: f64, min_half: f64, min_width: f64) -> Option<Hit> {
        let mut best: Option<(f64, u32, Hit)> = None;
        self.each_near(x, z, min_half, min_width, |id, hit| {
            let key = hit.d - hit.half;
            if !best.is_some_and(|(bk, bid, _)| bk < key || (bk == key && bid < id)) {
                best = Some((key, id, hit));
            }
        });
        best.map(|(_, _, h)| h)
    }

    /// Calls `f` with every channel segment whose valley reaches (x, z), each with its id and its
    /// hit as [`Channels::query`] measures it: where two rivers' valleys meet, the terrain carves
    /// by all of them (`terrain.rs`), so the ground never steps where the nearest one changes.
    pub fn each_near(
        &self,
        x: f64,
        z: f64,
        min_half: f64,
        min_width: f64,
        mut f: impl FnMut(u32, Hit),
    ) {
        let span = (min_half / BUCKET).ceil() as i32;
        let (bx, bz) = ((x / BUCKET).floor() as i32, (z / BUCKET).floor() as i32);
        let mut seen: Vec<u32> = Vec::new();
        for dz in -span..=span {
            for dx in -span..=span {
                let Some(list) = self.buckets.get(&(bx + dx, bz + dz)) else {
                    continue;
                };
                for &id in list {
                    if span > 0 {
                        // A segment is listed in every bucket it crosses: once each.
                        if seen.contains(&id) {
                            continue;
                        }
                        seen.push(id);
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
                    let l = l2.sqrt();
                    f(
                        id,
                        Hit {
                            d,
                            half,
                            level: p.s,
                            depth: p.d,
                            valley,
                            dir: (ex / l, ez / l),
                            fall: (s.a.s - s.b.s) / l,
                            mean_fall: s.mean_fall,
                            q: p.q,
                            river: s.river,
                        },
                    );
                }
            }
        }
    }
}

/// Each segment of `course`'s mean fall (m per m) over `reach` m up and down the course from its
/// middle (clipped to the course's ends): the fall its water runs on, round the reaches held level
/// where the land dips (`lower_into_land`; BUGS.md #42). Its own fall with `reach` 0.
pub fn mean_falls(course: &[Point], reach: f64) -> Vec<f64> {
    let mut along = Vec::with_capacity(course.len());
    let mut total = 0.0;
    for (i, p) in course.iter().enumerate() {
        if i > 0 {
            total += (p.x - course[i - 1].x).hypot(p.z - course[i - 1].z);
        }
        along.push(total);
    }
    // The surface `t` m along the course.
    let surface = |t: f64| {
        let i = along
            .partition_point(|&a| a <= t)
            .clamp(1, course.len().max(2) - 1);
        let (a, b) = (along[i - 1], along[i]);
        let u = if b > a {
            ((t - a) / (b - a)).clamp(0.0, 1.0)
        } else {
            1.0
        };
        course[i - 1].s + (course[i].s - course[i - 1].s) * u
    };
    course
        .windows(2)
        .enumerate()
        .map(|(i, w)| {
            let l = along[i + 1] - along[i];
            let own = if l > 0.0 { (w[0].s - w[1].s) / l } else { 0.0 };
            let mid = 0.5 * (along[i] + along[i + 1]);
            let (lo, hi) = ((mid - reach).max(0.0), (mid + reach).min(total));
            if hi - lo <= l {
                return own;
            }
            (surface(lo) - surface(hi)) / (hi - lo)
        })
        .collect()
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

/// Sets the end of a joining river's course to the joined river's level `s` over its last
/// `reach` m (both channels' half-widths and a margin: the joining channel's side reaches into
/// the joined one beside its centreline), a point added where that stretch
/// begins, so inside the joined channel both waters stand at one level; upstream of it, where the
/// joining river lies lower than `s`, the joined river's water backs up into it at `s` (levels
/// never rise downstream). Returns the index of the stretch's first point.
fn meet_at_bank(course: &mut Vec<Point>, s: f64, reach: f64) -> usize {
    let mut i = course.len() - 1;
    let mut along = 0.0;
    let bank = loop {
        if i == 0 {
            break 0;
        }
        let (a, b) = (course[i - 1], course[i]);
        let l = ((b.x - a.x).powi(2) + (b.z - a.z).powi(2)).sqrt();
        if along + l > reach {
            let t = 1.0 - (reach - along) / l;
            let mut at = Point::lerp(a, b, t);
            at.s = s;
            course.insert(i, at);
            break i;
        }
        along += l;
        course[i - 1].s = s;
        i -= 1;
    };
    for p in course[..bank].iter_mut().rev() {
        if p.s >= s {
            break;
        }
        p.s = s;
    }
    bank
}

/// Lowers a course from its end upstream until it falls no more than `slope` m per m: a river
/// hanging above the one it joins cuts down to it gradually instead of standing over it. Stops at
/// the first stretch already that gentle.
fn ease_fall(course: &mut [Point], slope: f64) {
    for i in (1..course.len()).rev() {
        let (a, b) = (course[i - 1], course[i]);
        let l = ((b.x - a.x).powi(2) + (b.z - a.z).powi(2)).sqrt();
        let most = b.s + slope * l;
        if a.s <= most {
            return;
        }
        course[i - 1].s = most;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// BUGS.md #42: a course falling 1 m per km, held level for 200 m and then dropping what it
    /// owes: the level reach's mean fall is the course's round it, the drop's gentler than its
    /// own, and a reach of 0 leaves each segment its own fall.
    #[test]
    fn a_level_reach_takes_its_courses_mean_fall() {
        // face-a-only: a course is face-local; the faces' maps are tested in `terrain.rs`
        // (`a_level_reach_runs_on_its_courses_fall`).
        let p = |x: f64, s: f64| Point {
            x,
            z: 0.0,
            s,
            w: 4.0,
            d: 1.0,
            q: 1.0,
        };
        let mut c = Vec::new();
        for i in 0..=40 {
            let x = f64::from(i) * 20.0;
            // level from 300 to 500 m, then a 20 m drop of the 0.2 m held back
            let s = if (300.0..=500.0).contains(&x) {
                10.0 - 0.3
            } else if x > 500.0 && x <= 520.0 {
                10.0 - 0.52
            } else {
                10.0 - x * 0.001
            };
            c.push(p(x, s));
        }
        let own = mean_falls(&c, 0.0);
        let mean = mean_falls(&c, 256.0);
        let level = 20; // the segment 400–420 m
        assert_eq!(own[level], 0.0);
        assert!(
            (mean[level] - 0.001).abs() < 2e-4,
            "the level reach falls {} m per m on average",
            mean[level]
        );
        assert!(
            mean[25] < own[25],
            "the drop's mean is gentler than its own"
        );
        assert!((own[0] - 0.001).abs() < 1e-9);
        assert!(mean.iter().all(|&f| f >= 0.0), "{mean:?}");
    }
}
