//! Finding places for the dev tools (docs/DEV_TOOLS.md "Finding places"): the nearest loaded cell
//! that matches a query (a block by name, a fluid, any fluid), searched outward from a point, with
//! filters (an open surface over it, a depth, an area, a current, a face), and a heading to look
//! along there (down or up the current, across it, toward a point). Scenarios and console sessions
//! find their scene this way instead of by coordinates that go stale when the terrain changes.
//!
//! Queries name blocks through the registry (`dwell:sand`, `dwell:water`), never by state id. All
//! of it is face-local through [`FaceFrame`]: "above" is toward the cell's face's up, a surface's
//! height is its top face-locally, so on face B the water's surface is mirrored as it is drawn.
//!
//! [`nearest_column`] is the same search over the world map's columns (the generator's sea, rivers
//! and lakes, unloaded too), for a local world to move the player to before searching its cells.

use dwell_blocks::{FLUIDS, FluidId, StateId, info};
use dwell_world::fluid::fluid_of;
use dwell_world::{
    CHUNK_SIZE, ChunkCoord, FaceFrame, VoxelWorld, centre_of_cell, chunk_origin_m, m_of_row,
    m_of_rows_f,
};
use std::collections::HashSet;

/// What a query matches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Matcher {
    /// Any fluid (flooded shaped cells included).
    AnyFluid,
    /// One fluid: a fluid block's name (`dwell:water`) matches its cells at any fill and flooded
    /// cells of it.
    Fluid(FluidId),
    /// Every state of one block (`dwell:sand`; the table's block index).
    Block(usize),
    /// One state (`dwell:ladder[facing=north]`).
    State(StateId),
}

impl Matcher {
    /// `fluid` (any fluid), a block id (`dwell:sand`; a name without a namespace is `dwell:`'s,
    /// so `water` is `dwell:water`) or a state (`dwell:ladder[facing=north]`). An unknown name is
    /// an error that names it.
    pub fn parse(query: &str) -> Result<Self, String> {
        let q = query.trim();
        if q.is_empty() {
            return Err("an empty query".into());
        }
        if q == "fluid" {
            return Ok(Self::AnyFluid);
        }
        let name = if q.contains(':') {
            q.to_owned()
        } else {
            format!("dwell:{q}")
        };
        if name.contains('[') {
            return dwell_blocks::parse_state(&name).map(Self::State);
        }
        let block = dwell_blocks::find_block(&name)
            .ok_or_else(|| format!("no block named \"{name}\" (or \"fluid\" for any fluid)"))?;
        let first = block.first_state;
        Ok(match info(first).fluid {
            Some(f) => Self::Fluid(f),
            None => Self::Block(info(first).block),
        })
    }

    pub fn matches(self, s: StateId) -> bool {
        match self {
            Self::AnyFluid => fluid_of(s).is_some(),
            Self::Fluid(f) => fluid_of(s) == Some(f),
            Self::Block(b) => dwell_blocks::try_info(s).is_some_and(|i| i.block == b),
            Self::State(id) => s == id,
        }
    }

    fn is_fluid(self) -> bool {
        matches!(self, Self::AnyFluid | Self::Fluid(_))
    }
}

/// A search.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Query {
    pub what: Matcher,
    /// How far from the start (m) a cell may be.
    pub radius_m: f64,
    /// The cell must be a surface: the cell above it (toward its face's up) neither matches nor
    /// is solid (a fluid's top under air or a plant, a block's top in the open).
    pub surface: bool,
    /// Matching cells under it, itself included, down to at least this depth (m below its
    /// surface): water deep enough to swim in or float a camera over.
    pub min_depth_m: f64,
    /// Matching surface cells joined to it at its row (4-connected), itself included: a pool's
    /// size, so a 1-cell puddle is passed over.
    pub min_area: u32,
    /// The current over it at least this fast (m/s): running water, a river not a pond.
    pub min_current: f32,
    /// Only cells on this face (None: either).
    pub face: Option<FaceFrame>,
}

impl Query {
    pub fn new(what: Matcher) -> Self {
        Self {
            what,
            radius_m: DEFAULT_RADIUS_M,
            surface: true,
            min_depth_m: 0.0,
            min_area: 1,
            min_current: 0.0,
            face: None,
        }
    }
}

/// A search's default radius (m): about the full-detail chunks a client holds.
pub const DEFAULT_RADIUS_M: f64 = 256.0;
/// The largest area counted (cells): a found cell's `area` is at most this (or `min_area`).
pub const AREA_CAP: u32 = 4096;
/// The deepest depth counted (rows).
const DEPTH_CAP_ROWS: i32 = 512;
/// The columns around a found cell (each way) whose currents make its `flow`.
const FLOW_REACH: i32 = 3;

/// What a search found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Found {
    pub cell: [i32; 3],
    pub state: StateId,
    pub face: FaceFrame,
    /// The world height (m) of its surface: a fluid's drawn surface (its fill, less the fluid's
    /// `surfaceDrop` under air), a block's top; on face B toward −y.
    pub surface_y: f64,
    /// How deep the matching cells under the surface go (m, at most 512 rows).
    pub depth_m: f64,
    /// The matching surface cells joined to it (at most [`AREA_CAP`] or the query's `min_area`).
    pub area: u32,
    /// From the start to the cell's centre (m).
    pub distance_m: f64,
    /// The current in its column (m/s along x and z).
    pub current: [f32; 2],
    /// The mean of the moving currents within 3 columns (m/s along x and z): the way the water
    /// runs there, steadier than one column's.
    pub flow: [f32; 2],
}

impl Found {
    /// The point on its surface over the cell's centre (world m).
    pub fn point(&self) -> [f64; 3] {
        [
            f64::from(self.cell[0]) + 0.5,
            self.surface_y,
            f64::from(self.cell[2]) + 0.5,
        ]
    }
    /// The point `height` m over its surface along its face's up (world m).
    pub fn over(&self, height: f64) -> [f64; 3] {
        let f = self.face;
        let [x, y, z] = self.point();
        [x, f.y(f.y(y) + height), z]
    }
}

/// The nearest cell matching `q` in the chunks `world` holds, from world point `from` (m), or
/// None within the radius. Nearest by straight-line distance to the cell's centre.
pub fn find(world: &VoxelWorld, from: [f64; 3], q: &Query) -> Option<Found> {
    let mut chunks: Vec<(f64, ChunkCoord)> = world
        .chunks()
        .map(|(c, _)| (chunk_distance(from, *c), *c))
        .filter(|&(d, c)| d <= q.radius_m && q.face.is_none_or(|f| FaceFrame::at_chunk_y(c.y) == f))
        .collect();
    chunks.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut best: Option<Found> = None;
    let mut rejected: HashSet<[i32; 3]> = HashSet::new();
    for (lower, coord) in chunks {
        if best.is_some_and(|b| lower >= b.distance_m) {
            break;
        }
        let Some(chunk) = world.find(coord) else {
            continue;
        };
        if !chunk.voxels().iter().any(|&s| q.what.matches(s)) {
            continue;
        }
        let limit = best.map_or(q.radius_m, |b| b.distance_m);
        let base = [
            coord.x * CHUNK_SIZE,
            coord.y * CHUNK_SIZE,
            coord.z * CHUNK_SIZE,
        ];
        let mut candidates = Vec::new();
        for ly in 0..CHUNK_SIZE {
            for lz in 0..CHUNK_SIZE {
                for lx in 0..CHUNK_SIZE {
                    if !q.what.matches(chunk.get(lx, ly, lz)) {
                        continue;
                    }
                    let cell = [base[0] + lx, base[1] + ly, base[2] + lz];
                    let d = distance(from, centre_of_cell(cell));
                    if d > limit || (q.surface && !open_above(world, q.what, cell)) {
                        continue;
                    }
                    candidates.push((d, cell));
                }
            }
        }
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (d, cell) in candidates {
            if rejected.contains(&cell) {
                continue;
            }
            if let Some(found) = accept(world, q, cell, d, &mut rejected) {
                best = Some(found);
                break;
            }
        }
    }
    best.map(|mut b| {
        if q.surface {
            b.area = area(world, q.what, b.cell, AREA_CAP.max(q.min_area)).0;
        }
        b
    })
}

/// `cell` as a find, if it passes the query's filters; a pool too small has its cells rejected.
fn accept(
    world: &VoxelWorld,
    q: &Query,
    cell: [i32; 3],
    distance_m: f64,
    rejected: &mut HashSet<[i32; 3]>,
) -> Option<Found> {
    let f = FaceFrame::at_row(cell[1]);
    let state = world.get_voxel_if_loaded(cell);
    let top = surface_local(world, q.what, cell, state);
    let rows = depth_rows(world, q.what, cell);
    let bottom = m_of_row(f.row(cell[1]) - rows + 1);
    let depth_m = top - bottom;
    if depth_m + 1e-9 < q.min_depth_m {
        return None;
    }
    let p = centre_of_cell(cell);
    let current = world.currents.at(p);
    if q.min_current > 0.0 && current[0].hypot(current[1]) < q.min_current {
        return None;
    }
    let mut n = 1;
    if q.surface && q.min_area > 1 {
        let (count, seen) = area(world, q.what, cell, q.min_area);
        if count < q.min_area {
            rejected.extend(seen);
            return None;
        }
        n = count;
    }
    Some(Found {
        cell,
        state,
        face: f,
        surface_y: f.y(top),
        depth_m,
        area: n,
        distance_m,
        current,
        flow: flow(world, cell),
    })
}

/// The cell above `cell` toward its face's up.
fn above(cell: [i32; 3]) -> [i32; 3] {
    let f = FaceFrame::at_row(cell[1]);
    [cell[0], f.row(f.row(cell[1]) + 1), cell[2]]
}

fn open_above(world: &VoxelWorld, what: Matcher, cell: [i32; 3]) -> bool {
    let s = world.get_voxel_if_loaded(above(cell));
    !what.matches(s) && !dwell_blocks::try_info(s).is_some_and(|i| i.solid)
}

/// The face-local height (m) of `cell`'s surface: a fluid's drawn surface, a block's top.
fn surface_local(world: &VoxelWorld, what: Matcher, cell: [i32; 3], state: StateId) -> f64 {
    let f = FaceFrame::at_row(cell[1]);
    let row = f.row(cell[1]);
    let i = info(state);
    if !what.is_fluid() {
        return m_of_row(row + 1);
    }
    let fill = if i.liquid { f64::from(i.fill) } else { 1.0 };
    let covered = what.matches(world.get_voxel_if_loaded(above(cell)));
    let drop = match fluid_of(state) {
        Some(fl) if !covered => m_of_rows_f(FLUIDS[usize::from(fl)].surface_drop),
        _ => 0.0,
    };
    m_of_row(row) + m_of_rows_f(fill) - drop
}

/// The matching cells from `cell` down (itself included), toward its face's ground.
fn depth_rows(world: &VoxelWorld, what: Matcher, cell: [i32; 3]) -> i32 {
    let f = FaceFrame::at_row(cell[1]);
    let row = f.row(cell[1]);
    let mut n = 1;
    while n < DEPTH_CAP_ROWS
        && what.matches(world.get_voxel_if_loaded([cell[0], f.row(row - n), cell[2]]))
    {
        n += 1;
    }
    n
}

/// The matching surface cells joined to `cell` at its row (4-connected), counted up to `cap`, and
/// the cells seen.
fn area(world: &VoxelWorld, what: Matcher, cell: [i32; 3], cap: u32) -> (u32, Vec<[i32; 3]>) {
    let mut seen = vec![cell];
    let mut set: HashSet<[i32; 3]> = HashSet::from([cell]);
    let mut i = 0;
    while i < seen.len() && (seen.len() as u32) < cap {
        let [x, y, z] = seen[i];
        i += 1;
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let n = [x + dx, y, z + dz];
            if set.contains(&n)
                || !what.matches(world.get_voxel_if_loaded(n))
                || !open_above(world, what, n)
            {
                continue;
            }
            set.insert(n);
            seen.push(n);
            if seen.len() as u32 >= cap {
                break;
            }
        }
    }
    (seen.len() as u32, seen)
}

/// The mean of the moving currents in the columns within [`FLOW_REACH`] of `cell`, on its face.
fn flow(world: &VoxelWorld, cell: [i32; 3]) -> [f32; 2] {
    let p = centre_of_cell(cell);
    let (mut sum, mut n) = ([0.0f32; 2], 0);
    for dx in -FLOW_REACH..=FLOW_REACH {
        for dz in -FLOW_REACH..=FLOW_REACH {
            let c = world
                .currents
                .at([p[0] + f64::from(dx), p[1], p[2] + f64::from(dz)]);
            if c[0] != 0.0 || c[1] != 0.0 {
                sum = [sum[0] + c[0], sum[1] + c[1]];
                n += 1;
            }
        }
    }
    if n == 0 {
        return [0.0; 2];
    }
    [sum[0] / n as f32, sum[1] / n as f32]
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The distance (m) from `p` to chunk `c`'s box: no cell in it is nearer.
fn chunk_distance(p: [f64; 3], c: ChunkCoord) -> f64 {
    let lo = chunk_origin_m(c);
    let hi = chunk_origin_m(ChunkCoord::new(c.x + 1, c.y + 1, c.z + 1));
    let d: [f64; 3] = std::array::from_fn(|k| (lo[k] - p[k]).max(p[k] - hi[k]).max(0.0));
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// How to look from a place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Aim {
    /// This heading (degrees; yaw 0 = +Z, 90 = +X, as the player's look and `placeCamera`).
    Yaw(f64),
    /// Along the water's flow there.
    Downstream,
    /// Against it.
    Upstream,
    /// A quarter turn right of downstream (the far bank on the right of the flow).
    Across,
    /// Toward a world point (m).
    Toward([f64; 3]),
}

impl Aim {
    /// `downstream`, `upstream` or `across` (else None).
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "downstream" => Some(Self::Downstream),
            "upstream" => Some(Self::Upstream),
            "across" => Some(Self::Across),
            _ => None,
        }
    }
}

/// Below this the flow has no way to look along (m/s).
pub const STILL_FLOW: f32 = 0.01;

/// The heading (degrees, 0..360; yaw 0 = +Z, 90 = +X) of a horizontal direction.
pub fn heading(dx: f64, dz: f64) -> f64 {
    dx.atan2(dz).to_degrees().rem_euclid(360.0)
}

/// The yaw (degrees) and, toward a point, the pitch (degrees toward the face's up) to look with
/// from `eye` at `found`: an error says why there is none (still water has no downstream). The
/// faces mirror only up and down, so a heading is the same in the world and face-locally.
pub fn aim(found: &Found, eye: [f64; 3], how: Aim) -> Result<(f64, Option<f64>), String> {
    let along = |turn: f64| {
        let [u, v] = if found.flow == [0.0; 2] {
            found.current
        } else {
            found.flow
        };
        if u.hypot(v) < STILL_FLOW {
            return Err(format!(
                "no current at {:?} to look along (the water is still, or its currents haven't arrived)",
                found.cell
            ));
        }
        Ok((
            (heading(f64::from(u), f64::from(v)) + turn).rem_euclid(360.0),
            None,
        ))
    };
    match how {
        Aim::Yaw(y) => Ok((y, None)),
        Aim::Downstream => along(0.0),
        Aim::Upstream => along(180.0),
        Aim::Across => along(90.0),
        Aim::Toward(t) => {
            let f = FaceFrame::at_height(eye[1]);
            let (dx, dz) = (t[0] - eye[0], t[2] - eye[2]);
            let rise = f.y(t[1]) - f.y(eye[1]);
            let pitch = rise.atan2(dx.hypot(dz)).to_degrees();
            Ok((heading(dx, dz), Some(pitch)))
        }
    }
}

/// The nearest point to `from` (x, z; m) where `at` returns something, sampled in rings outward to
/// `max_radius_m`: rings `step` apart and samples about `step` apart along each, `step` growing
/// with the distance (8 m near, about 1/48 of the distance far), so a far search costs some
/// hundred thousand samples; the first ring with any sample wins, its nearest. A coarse search
/// (a narrow river far off can fall between samples): for moving the player near, then [`find`].
pub fn nearest_column<T>(
    from: [f64; 2],
    max_radius_m: f64,
    mut at: impl FnMut(i32, i32) -> Option<T>,
) -> Option<([i32; 2], T)> {
    let sample = |x: f64, z: f64| [x.floor() as i32, z.floor() as i32];
    let [x0, z0] = sample(from[0], from[1]);
    if let Some(t) = at(x0, z0) {
        return Some(([x0, z0], t));
    }
    let step_at = |r: f64| (r / 48.0).max(8.0);
    let mut r = step_at(0.0);
    while r <= max_radius_m {
        let step = step_at(r);
        let n = ((std::f64::consts::TAU * r / step).ceil() as usize).max(8);
        for i in 0..n {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            let [x, z] = sample(from[0] + r * a.sin(), from[1] + r * a.cos());
            if let Some(t) = at(x, z) {
                return Some(([x, z], t));
            }
        }
        r += step;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_blocks::require;
    use dwell_world::{FaceFrame, row_of_m};

    /// A streamed world with `cells` set, each a face-local cell placed through `f`.
    fn world(f: FaceFrame, cells: &[([i32; 3], &str)]) -> VoxelWorld {
        let mut w = VoxelWorld::streamed();
        for &(c, s) in cells {
            let [x, y, z] = f.cell(c);
            w.set_voxel(x, y, z, f.state(require(s)));
        }
        w
    }

    /// A face-local slab of `name` over `x0..x1` × `z0..z1` at rows `y0..y1`.
    fn slab(
        out: &mut Vec<([i32; 3], &'static str)>,
        name: &'static str,
        [x0, x1]: [i32; 2],
        [y0, y1]: [i32; 2],
        [z0, z1]: [i32; 2],
    ) {
        for x in x0..x1 {
            for y in y0..y1 {
                for z in z0..z1 {
                    out.push(([x, y, z], name));
                }
            }
        }
    }

    fn water() -> Query {
        Query::new(Matcher::parse("water").unwrap())
    }

    /// The face-local start point `p` (m) in the world.
    fn from(f: FaceFrame, p: [f64; 3]) -> [f64; 3] {
        f.point(p)
    }

    const GROUND: i32 = 40;

    #[test]
    fn queries_name_blocks_and_fluids_by_name() {
        assert_eq!(Matcher::parse("fluid"), Ok(Matcher::AnyFluid));
        let water = dwell_blocks::fluid("dwell:water").unwrap();
        assert_eq!(Matcher::parse("dwell:water"), Ok(Matcher::Fluid(water)));
        assert_eq!(Matcher::parse("water"), Ok(Matcher::Fluid(water)));
        let sand = Matcher::parse("dwell:sand").unwrap();
        assert!(sand.matches(require("dwell:sand")));
        assert!(!sand.matches(require("dwell:stone")));
        assert!(
            Matcher::parse("dwell:no-such-block")
                .unwrap_err()
                .contains("dwell:no-such-block")
        );
        // Flooded shaped cells hold water.
        let ladder = require("dwell:ladder[facing=north,flooded=true]");
        assert!(Matcher::Fluid(water).matches(ladder));
        assert!(Matcher::AnyFluid.matches(ladder));
        assert!(
            Matcher::parse("dwell:ladder[facing=north,flooded=true]")
                .unwrap()
                .matches(ladder)
        );
    }

    dwell_world::both_faces!(
        fn the_nearest_water_surface_is_found_with_its_height_and_depth(f: FaceFrame) {
            // A stone floor; a 1-deep puddle near the start, a 3-deep pool farther off.
            let mut cells = Vec::new();
            slab(
                &mut cells,
                "dwell:stone",
                [-20, 40],
                [GROUND - 4, GROUND],
                [-20, 20],
            );
            slab(
                &mut cells,
                "dwell:water",
                [4, 5],
                [GROUND, GROUND + 1],
                [0, 1],
            );
            slab(
                &mut cells,
                "dwell:water",
                [20, 30],
                [GROUND - 2, GROUND + 1],
                [-5, 5],
            );
            let w = world(f, &cells);
            let start = from(f, [0.5, m_of_row(GROUND + 4), 0.5]);
            let near = find(&w, start, &water()).expect("the puddle");
            assert_eq!(near.cell, f.cell([4, GROUND, 0]));
            assert_eq!(near.face, f);
            // The drawn surface: the cell's top less the water's drop, face-locally.
            let drop = m_of_rows_f(dwell_blocks::require_fluid("dwell:water").surface_drop);
            assert!((f.y(near.surface_y) - (m_of_row(GROUND + 1) - drop)).abs() < 1e-9);
            assert!((near.depth_m - (m_of_row(1) - drop)).abs() < 1e-9);
            assert_eq!(near.area, 1);
            // Over it, along the face's up.
            let eye = near.over(2.0);
            assert!((f.y(eye[1]) - (f.y(near.surface_y) + 2.0)).abs() < 1e-9);

            // Deeper than the puddle: the pool, at its nearest top cell.
            let deep = find(
                &w,
                start,
                &Query {
                    min_depth_m: 1.0,
                    ..water()
                },
            )
            .expect("the pool");
            assert_eq!(deep.cell, f.cell([20, GROUND, 0]));
            assert!((deep.depth_m - (m_of_row(3) - drop)).abs() < 1e-9);
            // As big as the pool: the puddle passed over too.
            let wide = find(
                &w,
                start,
                &Query {
                    min_area: 50,
                    ..water()
                },
            )
            .expect("the pool");
            assert_eq!(wide.cell, f.cell([20, GROUND, 0]));
            assert_eq!(wide.area, 100);
            assert!(
                find(
                    &w,
                    start,
                    &Query {
                        min_area: 101,
                        ..water()
                    }
                )
                .is_none()
            );
            // Out of reach.
            assert!(
                find(
                    &w,
                    start,
                    &Query {
                        radius_m: 3.0,
                        ..water()
                    }
                )
                .is_none()
            );
        }
    );

    dwell_world::both_faces!(
        fn a_surface_is_open_above_and_a_block_query_finds_its_top(f: FaceFrame) {
            // Water under a stone roof is no surface; sand under air is, at its top cell.
            let mut cells = Vec::new();
            slab(
                &mut cells,
                "dwell:water",
                [2, 3],
                [GROUND, GROUND + 1],
                [0, 1],
            );
            slab(
                &mut cells,
                "dwell:stone",
                [2, 3],
                [GROUND + 1, GROUND + 2],
                [0, 1],
            );
            slab(
                &mut cells,
                "dwell:water",
                [8, 9],
                [GROUND, GROUND + 1],
                [0, 1],
            );
            slab(
                &mut cells,
                "dwell:sand",
                [-6, -5],
                [GROUND - 3, GROUND + 1],
                [0, 1],
            );
            let w = world(f, &cells);
            let start = from(f, [0.5, m_of_row(GROUND), 0.5]);
            assert_eq!(
                find(&w, start, &water()).unwrap().cell,
                f.cell([8, GROUND, 0])
            );
            let buried = find(
                &w,
                start,
                &Query {
                    surface: false,
                    ..water()
                },
            )
            .unwrap();
            assert_eq!(buried.cell, f.cell([2, GROUND, 0]));
            let sand = find(
                &w,
                start,
                &Query::new(Matcher::parse("dwell:sand").unwrap()),
            )
            .unwrap();
            assert_eq!(sand.cell, f.cell([-6, GROUND, 0]));
            assert!((f.y(sand.surface_y) - m_of_row(GROUND + 1)).abs() < 1e-9);
            assert!((sand.depth_m - m_of_row(4)).abs() < 1e-9);
            // On the other face only: nothing.
            assert!(
                find(
                    &w,
                    start,
                    &Query {
                        face: Some(f.other()),
                        ..water()
                    }
                )
                .is_none()
            );
            assert_eq!(row_of_m(f.y(sand.surface_y)), GROUND + 1);
        }
    );

    dwell_world::both_faces!(
        fn running_water_is_found_and_looked_down_along_its_current(f: FaceFrame) {
            // Two pools; only the farther one runs (+X at 0.5 m/s).
            let mut cells = Vec::new();
            slab(
                &mut cells,
                "dwell:water",
                [3, 6],
                [GROUND - 1, GROUND + 1],
                [-1, 2],
            );
            slab(
                &mut cells,
                "dwell:water",
                [30, 40],
                [GROUND - 1, GROUND + 1],
                [-1, 2],
            );
            let mut w = world(f, &cells);
            for x in 30..40 {
                for z in -1..2 {
                    let (key, i) = dwell_world::currents::current_key(f, x, z);
                    let mut c = w
                        .currents
                        .get(key)
                        .cloned()
                        .unwrap_or_else(|| Box::new([[0.0; 2]; dwell_world::currents::CA]));
                    c[i] = [0.5, 0.0];
                    w.currents.set(key, Some(c));
                }
            }
            let start = from(f, [0.5, m_of_row(GROUND + 2), 0.5]);
            let still = find(&w, start, &water()).unwrap();
            assert_eq!(still.cell[0], 3);
            let err = aim(&still, still.over(1.6), Aim::Downstream).unwrap_err();
            assert!(err.contains("no current"), "{err}");
            let river = find(
                &w,
                start,
                &Query {
                    min_current: 0.1,
                    ..water()
                },
            )
            .unwrap();
            assert_eq!(river.cell, f.cell([30, GROUND, 0]));
            assert_eq!(river.current, [0.5, 0.0]);
            let eye = river.over(1.6);
            let yaw = |a| aim(&river, eye, a).unwrap().0;
            assert!((yaw(Aim::Downstream) - 90.0).abs() < 1e-6);
            assert!((yaw(Aim::Upstream) - 270.0).abs() < 1e-6);
            assert!((yaw(Aim::Across) - 180.0).abs() < 1e-6);
            // Toward a point below and ahead (+Z), face-locally: pitched down on either face.
            let target = f.point([eye[0], f.y(eye[1]) - 10.0, eye[2] + 10.0]);
            let (yaw, pitch) = aim(&river, eye, Aim::Toward(target)).unwrap();
            assert!(yaw.abs() < 1e-6);
            assert!((pitch.unwrap() + 45.0).abs() < 1e-6);
        }
    );

    #[test]
    fn headings_follow_the_look_convention() {
        // yaw 0 = +Z, 90 = +X (FaceFrame::look).
        assert!((heading(0.0, 1.0) - 0.0).abs() < 1e-9);
        assert!((heading(1.0, 0.0) - 90.0).abs() < 1e-9);
        assert!((heading(0.0, -1.0) - 180.0).abs() < 1e-9);
        assert!((heading(-1.0, 0.0) - 270.0).abs() < 1e-9);
        let d = FaceFrame::A.look(heading(3.0, 4.0), 0.0);
        assert!((d[0] - 0.6).abs() < 1e-9 && (d[2] - 0.8).abs() < 1e-9);
    }

    #[test]
    fn the_map_search_finds_the_nearest_ring_first() {
        // Water east of x = 1000 and a pond at (−300, 0): the pond is nearer from the origin.
        let at =
            |x: i32, z: i32| (x >= 1000 || ((x + 300).pow(2) + z.pow(2) < 30 * 30)).then_some(());
        let ([x, z], ()) = nearest_column([0.0, 0.0], 5000.0, at).unwrap();
        assert!((-330..=-270).contains(&x) && z.abs() <= 30, "({x}, {z})");
        let ([x, _], ()) = nearest_column([900.0, 0.0], 5000.0, at).unwrap();
        assert!((1000..1030).contains(&x), "{x}");
        assert!(nearest_column([0.0, 0.0], 200.0, at).is_none());
        // Where it starts, at once.
        assert_eq!(
            nearest_column([1200.5, 3.5], 10.0, at).unwrap().0,
            [1200, 3]
        );
    }
}
