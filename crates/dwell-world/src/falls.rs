//! Waterfalls (docs/WATER_SURFACE_GRID.md §2e, Phase 53 G6): where water falls, found from what the
//! server and every client already have — the world's water cells and the current table
//! ([`VoxelWorld::currents`]) — so the server's physics and a client's prediction, drawing and
//! sound agree with no message of their own.
//!
//! A **fall face** is a face between two columns where the current at the high column crosses the
//! face (at least `min_speed_ms`, carrying at least `min_flow_m2s` per metre), the cell beside the
//! lip is open and the far column's surface (its water, or its floor if dry) lies more than
//! `min_drop_m` below the near water's level. Faces side by side along one lip make one [`Fall`].
//! Its sheet's volume is beyond the face (`reach_m` out), from the landing up to the lip; what is in
//! it is pushed down and out ([`push_at`], [`push_velocity`]).
//!
//! Face-local throughout: a column is (face, x, z) with heights in its face's frame
//! (`FaceFrame::row`), so face B is face A's mirror; currents are the same vector in both frames.
//! Data: `shared/fluids/falls.json` → `falls.gen.rs` (`node shared/fluids/falls.mjs`).

use dwell_blocks::{StateId, try_info};

use crate::currents::current_key;
use crate::{CELL_H, FaceFrame, M, VoxelWorld, chunk_of, m_of_row, row_of_m};

/// The waterfalls' data (`shared/fluids/falls.json`; what each field means is in its `$comment`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FallsData {
    pub min_speed_ms: f64,
    pub min_flow_m2s: f64,
    pub min_drop_m: f64,
    pub max_drop_m: f64,
    pub merge_m: f64,
    pub reach_m: f64,
    pub full_flow_m2s: f64,
    pub push_out_ms: f64,
    pub push_down_ms: f64,
    pub push_rate: f64,
}

// Included (not a `#[path]` module) so rustfmt leaves the generated file as written.
mod falls_generated {
    include!("falls.gen.rs");
}
pub use falls_generated::FALLS;

/// The four ways water leaves a column: +x, −x, +z, −z.
pub const DIRS: [[i32; 2]; 4] = [[1, 0], [-1, 0], [0, 1], [0, -1]];

/// What a cell is to falling water.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Solid,
    Open,
    Liquid,
}

fn kind(m: StateId) -> Kind {
    if m == M.air {
        return Kind::Open;
    }
    match try_info(m) {
        Some(s) if s.liquid => Kind::Liquid,
        Some(s) if s.replaceable && !s.solid => Kind::Open,
        _ => Kind::Solid,
    }
}

/// Face `f`'s local cell (x, row r, z): its kind and state, None where the world isn't loaded.
fn at(world: &VoxelWorld, f: FaceFrame, x: i32, r: i32, z: i32) -> Option<(Kind, StateId)> {
    let c = f.cell([x, r, z]);
    world.find(chunk_of(c[0], c[1], c[2]))?;
    let m = world.get_voxel_if_loaded(c);
    Some((kind(m), m))
}

/// A liquid cell's share of its cell (a full one, or its eighths).
fn fill_of(m: StateId) -> f64 {
    let fill = try_info(m).map_or(1.0, |s| f64::from(s.fill));
    if fill <= 0.0 { 1.0 } else { fill.min(1.0) }
}

/// The water standing in a column: its top cell's row, its level and floor (face-local m) and its
/// liquid's fluid (`dwell_blocks::FLUIDS` index).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    pub top: i32,
    pub level: f64,
    pub floor: f64,
    pub fluid: u8,
}

/// The water standing in face `f`'s column (x, z) nearest local row `near` (looked for within
/// `rows` either way): water on a floor (water over open air is a fall's, not a column's).
pub fn column(world: &VoxelWorld, f: FaceFrame, x: i32, z: i32, near: i32, rows: i32) -> Option<Column> {
    let liquid = |r: i32| at(world, f, x, r, z).filter(|(k, _)| *k == Kind::Liquid);
    let mut found = None;
    for k in 0..=rows {
        for r in [near - k, near + k] {
            if liquid(r).is_some() {
                found = Some(r);
                break;
            }
        }
        if found.is_some() {
            break;
        }
    }
    let mut top = found?;
    while liquid(top + 1).is_some() {
        top += 1;
    }
    let mut bottom = top;
    while liquid(bottom - 1).is_some() {
        bottom -= 1;
    }
    if at(world, f, x, bottom - 1, z)?.0 != Kind::Solid {
        return None;
    }
    let (_, m) = liquid(top)?;
    Some(Column {
        top,
        level: m_of_row(top) + fill_of(m) * CELL_H,
        floor: m_of_row(bottom),
        fluid: try_info(m).and_then(|s| s.fluid).unwrap_or(0),
    })
}

/// One face of a fall: its high column (x, z), the way it falls (`DIRS` index), the lip (the near
/// water's level) and the landing (the far surface) in face-local m, the current across the face
/// (m/s), the flow per metre of width (m²/s) and the liquid's fluid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FallFace {
    pub x: i32,
    pub z: i32,
    pub dir: u8,
    pub lip: f64,
    pub land: f64,
    pub speed: f64,
    pub flow: f64,
    pub fluid: u8,
}

/// The fall face (if any) from face `f`'s column whose water is `col` at (x, z), toward `dir`.
pub fn fall_face(world: &VoxelWorld, f: FaceFrame, x: i32, z: i32, col: &Column, dir: u8) -> Option<FallFace> {
    let d = &FALLS;
    let (key, i) = current_key(f, x, z);
    let c = world.currents.get(key)?[i];
    let [dx, dz] = DIRS[usize::from(dir)];
    let speed = f64::from(c[0]) * f64::from(dx) + f64::from(c[1]) * f64::from(dz);
    let flow = speed * (col.level - col.floor);
    if speed < d.min_speed_ms || flow < d.min_flow_m2s {
        return None;
    }
    let (nx, nz) = (x + dx, z + dz);
    if at(world, f, nx, col.top, nz)?.0 != Kind::Open {
        return None;
    }
    let lowest = row_of_m(col.level - d.max_drop_m);
    let mut r = col.top - 1;
    let land = loop {
        if r < lowest {
            return None;
        }
        match at(world, f, nx, r, nz)? {
            (Kind::Open, _) => r -= 1,
            (Kind::Liquid, m) => break m_of_row(r) + fill_of(m) * CELL_H,
            (Kind::Solid, _) => break m_of_row(r + 1),
        }
    };
    (col.level - land > d.min_drop_m).then_some(FallFace {
        x,
        z,
        dir,
        lip: col.level,
        land,
        speed,
        flow,
        fluid: col.fluid,
    })
}

/// A fall: one or more fall faces side by side along a lip. Face-local: `x`, `z` the first high
/// column (the one least along the lip's axis), `width` columns along it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fall {
    pub face: FaceFrame,
    pub x: i32,
    pub z: i32,
    pub dir: u8,
    pub width: u32,
    /// Its lip and landing (face-local m, the faces' mean).
    pub lip: f64,
    pub land: f64,
    /// The current over the lip (m/s) and the flow per metre (m²/s), the faces' mean.
    pub speed: f64,
    pub flow: f64,
    pub fluid: u8,
}

impl Fall {
    /// The way it falls (a unit vector along x and z).
    pub fn out(&self) -> [f64; 2] {
        let [dx, dz] = DIRS[usize::from(self.dir)];
        [f64::from(dx), f64::from(dz)]
    }
    /// The lip's middle (face-local m): on the face it falls over, at the lip's height.
    pub fn lip_mid(&self) -> [f64; 3] {
        let [dx, dz] = DIRS[usize::from(self.dir)];
        let w = f64::from(self.width);
        let (x, z) = (f64::from(self.x), f64::from(self.z));
        let px = if dx > 0 { x + 1.0 } else if dx < 0 { x } else { x + 0.5 * w };
        let pz = if dz > 0 { z + 1.0 } else if dz < 0 { z } else { z + 0.5 * w };
        [px, self.lip, pz]
    }
    /// The water it carries over (m³/s).
    pub fn discharge(&self) -> f64 {
        self.flow * f64::from(self.width)
    }
    /// Its strength (0–1): its flow per metre over `full_flow_m2s`.
    pub fn strength(&self) -> f64 {
        (self.flow / FALLS.full_flow_m2s).clamp(0.0, 1.0)
    }
    /// The lip's middle in the world.
    pub fn lip_world(&self) -> [f64; 3] {
        self.face.point(self.lip_mid())
    }
}

/// The falls on the world near world point `eye` (within `radius` m along x and z, their lips within
/// `radius` m of its height): every column of the current table there that may carry a fall. Each
/// face's faces side by side along a lip merged into one fall.
pub fn falls_near(world: &VoxelWorld, eye: [f64; 3], radius: f64) -> Vec<Fall> {
    let mut faces: Vec<(FaceFrame, FallFace)> = Vec::new();
    let cs = crate::currents::CS;
    let rows = (radius / CELL_H).ceil() as i32;
    for key in world.currents.keys().collect::<Vec<_>>() {
        let (b, cx, cz) = key;
        let f = if b { FaceFrame::B } else { FaceFrame::A };
        let near = row_of_m(f.y(eye[1]));
        let (x0, z0) = (cx * cs, cz * cs);
        if f64::from(x0 + cs) < eye[0] - radius
            || f64::from(x0) > eye[0] + radius
            || f64::from(z0 + cs) < eye[2] - radius
            || f64::from(z0) > eye[2] + radius
        {
            continue;
        }
        let Some(cells) = world.currents.get(key) else {
            continue;
        };
        for (i, c) in cells.iter().enumerate() {
            if f64::from(c[0].abs().max(c[1].abs())) < FALLS.min_speed_ms {
                continue;
            }
            let (x, z) = (x0 + i as i32 % cs, z0 + i as i32 / cs);
            if (f64::from(x) + 0.5 - eye[0]).hypot(f64::from(z) + 0.5 - eye[2]) > radius {
                continue;
            }
            let Some(col) = column(world, f, x, z, near, rows) else {
                continue;
            };
            for dir in 0..4u8 {
                if let Some(ff) = fall_face(world, f, x, z, &col, dir) {
                    faces.push((f, ff));
                }
            }
        }
    }
    merge(faces)
}

/// Faces side by side along one lip (the same face and way, the next column across, lips and
/// landings within `merge_m`) as one fall each.
pub fn merge(mut faces: Vec<(FaceFrame, FallFace)>) -> Vec<Fall> {
    // across: the coordinate across the way it falls (along the lip); line: the one along it
    let across = |dir: u8, x: i32, z: i32| if DIRS[usize::from(dir)][0] == 0 { x } else { z };
    let line = |dir: u8, x: i32, z: i32| if DIRS[usize::from(dir)][0] == 0 { z } else { x };
    faces.sort_by_key(|(fr, f)| (fr.is_b(), f.dir, line(f.dir, f.x, f.z), across(f.dir, f.x, f.z)));
    let mut out: Vec<Fall> = Vec::new();
    for (fr, f) in faces {
        if let Some(last) = out.last_mut()
            && last.face == fr
            && last.dir == f.dir
            && last.fluid == f.fluid
            && line(last.dir, last.x, last.z) == line(f.dir, f.x, f.z)
            && across(last.dir, last.x, last.z) + last.width as i32 == across(f.dir, f.x, f.z)
            && (last.lip - f.lip).abs() <= FALLS.merge_m
            && (last.land - f.land).abs() <= FALLS.merge_m
        {
            let n = f64::from(last.width);
            let mean = |a: f64, b: f64| (a * n + b) / (n + 1.0);
            last.lip = mean(last.lip, f.lip);
            last.land = mean(last.land, f.land);
            last.speed = mean(last.speed, f.speed);
            last.flow = mean(last.flow, f.flow);
            last.width += 1;
            continue;
        }
        out.push(Fall {
            face: fr,
            x: f.x,
            z: f.z,
            dir: f.dir,
            width: 1,
            lip: f.lip,
            land: f.land,
            speed: f.speed,
            flow: f.flow,
            fluid: f.fluid,
        });
    }
    out
}

/// A fall's push on what is in its sheet: the way it falls (`DIRS` index) and its strength (0–1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FallPush {
    pub dir: u8,
    pub strength: f64,
}

/// The push of the strongest fall whose sheet a body overlaps: centred at world point `p`, reaching
/// `half_height` m up and down (face-local) and `radius` m along x and z. None out of every sheet.
pub fn push_at(world: &VoxelWorld, p: [f64; 3], half_height: f64, radius: f64) -> Option<FallPush> {
    let f = FaceFrame::at_height(p[1]);
    let l = f.point(p);
    let (lo, hi) = (l[1] - half_height, l[1] + half_height);
    let reach = FALLS.reach_m;
    let span = (reach + radius).ceil() as i32 + 1;
    let (bx, bz) = (l[0].floor() as i32, l[2].floor() as i32);
    // the lip is at or above the body's bottom, at most `max_drop_m` over it
    let near = row_of_m(hi);
    let rows = (FALLS.max_drop_m / CELL_H).ceil() as i32;
    let mut best: Option<FallPush> = None;
    for x in bx - span..=bx + span {
        for z in bz - span..=bz + span {
            let (key, i) = current_key(f, x, z);
            let Some(cells) = world.currents.get(key) else {
                continue;
            };
            let c = cells[i];
            if f64::from(c[0].abs().max(c[1].abs())) < FALLS.min_speed_ms {
                continue;
            }
            let Some(col) = column(world, f, x, z, near, rows) else {
                continue;
            };
            if col.level < lo {
                continue;
            }
            for dir in 0..4u8 {
                let Some(ff) = fall_face(world, f, x, z, &col, dir) else {
                    continue;
                };
                if ff.land > hi || ff.lip < lo {
                    continue;
                }
                // the sheet's box: beyond the face, `reach` out, across the column
                let [dx, dz] = DIRS[usize::from(dir)];
                let (x0, x1, z0, z1) = match (dx, dz) {
                    (1, _) => (f64::from(x + 1), f64::from(x + 1) + reach, f64::from(z), f64::from(z + 1)),
                    (-1, _) => (f64::from(x) - reach, f64::from(x), f64::from(z), f64::from(z + 1)),
                    (_, 1) => (f64::from(x), f64::from(x + 1), f64::from(z + 1), f64::from(z + 1) + reach),
                    _ => (f64::from(x), f64::from(x + 1), f64::from(z) - reach, f64::from(z)),
                };
                let inside = l[0] + radius > x0 && l[0] - radius < x1 && l[2] + radius > z0 && l[2] - radius < z1;
                if !inside {
                    continue;
                }
                let s = (ff.flow / FALLS.full_flow_m2s).clamp(0.0, 1.0);
                if best.is_none_or(|b| s > b.strength) {
                    best = Some(FallPush { dir, strength: s });
                }
            }
        }
    }
    best
}

/// A face-local velocity `v` (m/s) after `dt` s in a fall's sheet: drawn toward the falling water's
/// (`push_out_ms` out over the lip, `push_down_ms` down, by its strength) at `push_rate`, only ever
/// further out and down; it can't rise.
pub fn push_velocity(v: [f64; 3], push: FallPush, dt: f64) -> [f64; 3] {
    let s = push.strength;
    if s <= 0.0 {
        return v;
    }
    let k = 1.0 - (-FALLS.push_rate * s * dt).exp();
    let [dx, dz] = DIRS[usize::from(push.dir)];
    let (dx, dz) = (f64::from(dx), f64::from(dz));
    let out = v[0] * dx + v[2] * dz;
    let want_out = FALLS.push_out_ms * s;
    let add = if out < want_out { (want_out - out) * k } else { 0.0 };
    let want_down = -FALLS.push_down_ms * s;
    let mut y = v[1].min(0.0);
    if y > want_down {
        y += (want_down - y) * k;
    }
    [v[0] + dx * add, y, v[2] + dz * add]
}

#[cfg(test)]
mod tests;
