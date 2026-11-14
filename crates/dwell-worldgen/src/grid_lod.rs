//! LOD sections of a grid other than home (docs/proposals/MOON_WORLD.md §7.3, BUGS.md #156): the
//! moon's and the sun's distant terrain drawn by home's LOD system (`dwell-lod` `LodSystem`, its
//! graded levels and fades), with sections from the grid's own generator. A grid's generator gives
//! its columns ([`GridColumns`]: the ground's height at a cell's scale, its block, and a fluid over
//! it); one builder here turns them into a section's cells and column surfaces, both faces, the
//! way `BifacialTerrain::generate_lod` does for home, so the client meshes them with the same LOD
//! mesher and colours them by the same block looks as home's far terrain and the grid's chunks.
//! Any grid whose generator implements [`GridColumns`] gets one: no per-grid code in the client.

use std::sync::Arc;

use dwell_blocks::StateId;
use dwell_world::lod::{
    self, LodBounds, LodCells, LodCoord, LodKind, LodOrigin, LodSurface, LodSurfaces,
};
use dwell_world::{CELL_H, Face, M, WorldBounds};

use crate::grids::{GridDef, GridGenerator};

/// What a grid's column holds, face-local: its ground's top and block, and the fluid standing
/// over it (lava on the sun) up to `fluid_m` where that is over the ground.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridColumn {
    /// The ground's top (face-local m).
    pub ground_m: f64,
    /// The ground's block: its top and everything under it.
    pub ground: StateId,
    /// The fluid's surface (face-local m); at or under `ground_m` for none.
    pub fluid_m: f64,
    pub fluid: StateId,
}

impl GridColumn {
    /// The face-local row of the ground's top cell, as the chunks round it.
    pub fn top_row(&self) -> i64 {
        (self.ground_m / CELL_H).round() as i64 - 1
    }
    /// The face-local row of the fluid's top cell (`None` for no fluid over the ground).
    pub fn fluid_row(&self) -> Option<i64> {
        let f = ((self.fluid_m / CELL_H).round() as i64).saturating_sub(1);
        (f > self.top_row()).then_some(f)
    }
}

/// A grid's generator read column by column for its LOD sections.
pub trait GridColumns: Send + Sync {
    /// The disc's radius (m).
    fn radius_m(&self) -> f64;
    /// The column at face-local (x, z) of `face` with the detail at least `cell_m` wide (`None`
    /// off the disc).
    fn lod_column(&self, face: Face, x: f64, z: f64, cell_m: f64) -> Option<GridColumn>;
}

/// Grid `def`'s columns in the world of `world_seed`: the `cratered` and `volcanic` kinds; `None`
/// for home's `terrain` (whose sections come from `BifacialTerrain`).
pub fn grid_columns(def: &GridDef, world_seed: u64) -> Option<Arc<dyn GridColumns>> {
    match &def.generator {
        GridGenerator::Cratered(_) => Some(Arc::new(crate::grids::CrateredTerrain::new(
            def, world_seed,
        )?)),
        GridGenerator::Volcanic(_) => Some(Arc::new(crate::volcanic::VolcanicTerrain::new(
            def, world_seed,
        )?)),
        GridGenerator::Terrain { .. } => None,
    }
}

/// The disc and volume a grid's LOD selects its sections in.
pub fn grid_world(src: &(impl GridColumns + ?Sized)) -> WorldBounds {
    WorldBounds::grid(src.radius_m().ceil() as i64)
}

/// The padded columns of a section from origin `o` at `level` on `face`, sampled at each cell's
/// centre at the cell's scale; a cell whose centre is off the disc but which reaches onto it at
/// its point nearest the disc's centre (a coarse cell over the rim: a small grid's coarsest
/// sections are wider than its disc).
fn columns(
    src: &(impl GridColumns + ?Sized),
    face: Face,
    o: LodOrigin,
    level: i32,
) -> Vec<Option<GridColumn>> {
    let cell = lod::cell_size(level);
    let half = cell as f64 / 2.0;
    let r = src.radius_m();
    let mut out = Vec::with_capacity((lod::PAD * lod::PAD) as usize);
    for z in -1..=lod::SECTION_CELLS {
        for x in -1..=lod::SECTION_CELLS {
            let x0 = (o.x + i64::from(x) * cell) as f64;
            let z0 = (o.z + i64::from(z) * cell) as f64;
            let (wx, wz) = (x0 + half, z0 + half);
            let near = |lo: f64| 0.0f64.clamp(lo, lo + cell as f64);
            let (nx, nz) = (near(x0), near(z0));
            let (px, pz) = if wx.hypot(wz) <= r || nx.hypot(nz) > r {
                (wx, wz)
            } else {
                // Just inside the rim along the way to the centre.
                let d = wx.hypot(wz);
                let k = (r - 0.5).max(0.0) / d;
                let (rx, rz) = (wx * k, wz * k);
                let (cx, cz) = (
                    rx.clamp(x0, x0 + cell as f64),
                    rz.clamp(z0, z0 + cell as f64),
                );
                if cx.hypot(cz) <= r {
                    (cx, cz)
                } else {
                    (nx, nz)
                }
            };
            out.push(src.lod_column(face, px, pz, cell as f64));
        }
    }
    out
}

/// One face's bounds (rows) from its columns: cells sampling at or above `hi` are open, below
/// `lo` solid; a column off the disc leaves nothing buried.
fn bounds_of(cols: &[Option<GridColumn>]) -> LodBounds {
    let mut b = LodBounds {
        lo: f64::INFINITY,
        hi: f64::NEG_INFINITY,
        any_inside: false,
        ..LodBounds::default()
    };
    let mut all_inside = true;
    for c in cols {
        let Some(c) = c else {
            all_inside = false;
            continue;
        };
        b.any_inside = true;
        let top = c.top_row();
        let over = c.fluid_row().unwrap_or(top);
        b.hi = b.hi.max((over + 2) as f64);
        b.lo = b.lo.min((top - 1) as f64);
    }
    if !all_inside {
        b.lo = f64::NEG_INFINITY;
    }
    b
}

/// The column bounds of both faces of section column (level, i, ·, k).
pub fn grid_lod_bounds(src: &(impl GridColumns + ?Sized), level: i32, i: i32, k: i32) -> LodBounds {
    let o = lod::section_origin(LodCoord::new(level, i, 0, k));
    let a = bounds_of(&columns(src, Face::A, o, level));
    let b = bounds_of(&columns(src, Face::B, o, level));
    LodBounds {
        lo: a.lo,
        hi: a.hi,
        lo_b: b.lo,
        hi_b: b.hi,
        any_inside: a.any_inside || b.any_inside,
        bifacial: true,
    }
}

/// One face's section whose face-local rows start at `origin_y`: each cell solid with the
/// column's ground where its bottom row is at or under the ground's top, fluid over it up to the
/// fluid's top, the column's top cell giving its surface (`LodSurface`, rows) as home's do.
fn generate_face(
    src: &(impl GridColumns + ?Sized),
    face: Face,
    c: LodCoord,
    origin_y: i64,
    cells: &mut LodCells,
    mut surface: Option<&mut LodSurfaces>,
) -> (LodKind, LodBounds) {
    cells.clear();
    cells.resize(lod::VOLUME, M.air);
    if let Some(s) = surface.as_deref_mut() {
        s.clear();
        s.resize((lod::PAD * lod::PAD) as usize, LodSurface::default());
    }
    let mut o = lod::section_origin(c);
    o.y = origin_y;
    let cell = lod::cell_size(c.level);
    let cols = columns(src, face, o, c.level);
    let bounds = bounds_of(&cols);
    let kind = if bounds.any_inside {
        lod::kind_of_face(c.level, origin_y, bounds.lo, bounds.hi)
    } else {
        LodKind::Empty
    };
    if kind != LodKind::Content {
        if kind == LodKind::Buried {
            cells.fill(M.stone);
        }
        return (kind, bounds);
    }
    for z in -1..=lod::SECTION_CELLS {
        for x in -1..=lod::SECTION_CELLS {
            let Some(col) = cols[lod::column_index(x, z)] else {
                continue; // beyond the rim: the void
            };
            let top = col.top_row();
            let fluid = col.fluid_row();
            let wet = fluid.is_some();
            // The fluid's level (rows): the first row over its top cell.
            let level = fluid.map_or(0, |f| f + 1);
            for y in -1..=lod::SECTION_CELLS {
                let a = o.y + i64::from(y) * cell;
                let m = &mut cells[lod::cell_index(x, y, z)];
                // A cell reaching under the face samples at its lowest row (the midplane).
                let at = a.max(0);
                if at > top {
                    if wet && at < level {
                        *m = col.fluid;
                    }
                    continue;
                }
                if a + cell - 1 < top {
                    *m = col.ground;
                    continue;
                }
                // The column's top: its surface seen from above; under a fluid no cell sampled,
                // the fluid shows.
                let fluid_above = a + cell < level;
                *m = if wet && !fluid_above {
                    col.fluid
                } else {
                    col.ground
                };
                if let Some(s) = surface.as_deref_mut() {
                    s[lod::column_index(x, z)] = LodSurface {
                        valid: true,
                        wet,
                        height: ((top + 1) as f32).min((a + cell) as f32),
                        material: col.ground,
                        water: if wet { level as f32 } else { 0.0 },
                    };
                }
            }
        }
    }
    (kind, bounds)
}

/// A section of grid `src` in its world `world` (its disc), rows in world order: face A's rows
/// from its columns, face B's from the mirror image of the section in face B's, as
/// `BifacialTerrain::generate_lod`. `surface` receives face A's column surfaces, `surface_b`
/// face B's (face-local).
pub fn generate_grid_lod(
    src: &(impl GridColumns + ?Sized),
    world: &WorldBounds,
    c: LodCoord,
    cells: &mut LodCells,
    mut surface: Option<&mut LodSurfaces>,
    mut surface_b: Option<&mut LodSurfaces>,
) -> LodKind {
    let columns = (lod::PAD * lod::PAD) as usize;
    cells.clear();
    cells.resize(lod::VOLUME, M.air);
    for s in [surface.as_deref_mut(), surface_b.as_deref_mut()]
        .into_iter()
        .flatten()
    {
        s.clear();
        s.resize(columns, LodSurface::default());
    }
    if !lod::in_world_of(c, world) {
        return LodKind::Empty;
    }
    let y0 = lod::section_origin(c).y;
    let first_a = lod::first_face_a_row(c);
    let mut bounds = LodBounds::default();
    let mut local = LodCells::new();
    let copy_rows = |cells: &mut LodCells,
                     local: &LodCells,
                     rows: std::ops::RangeInclusive<i32>,
                     row_of: &dyn Fn(i32) -> i32| {
        for r in rows {
            let from = row_of(r);
            for z in -1..=lod::SECTION_CELLS {
                for x in -1..=lod::SECTION_CELLS {
                    cells[lod::cell_index(x, r, z)] = local[lod::cell_index(x, from, z)];
                }
            }
        }
    };
    if first_a <= lod::SECTION_CELLS {
        let (_, b) = generate_face(src, Face::A, c, y0, &mut local, surface);
        bounds.lo = b.lo;
        bounds.hi = b.hi;
        bounds.any_inside = b.any_inside;
        copy_rows(cells, &local, first_a..=lod::SECTION_CELLS, &|r| r);
    }
    if first_a >= 0 {
        let origin_l = lod::mirror_row_bottom(y0, lod::section_size(c.level));
        let (_, b) = generate_face(src, Face::B, c, origin_l, &mut local, surface_b);
        bounds.lo_b = b.lo;
        bounds.hi_b = b.hi;
        bounds.any_inside = bounds.any_inside || b.any_inside;
        copy_rows(
            cells,
            &local,
            -1..=(first_a - 1).min(lod::SECTION_CELLS),
            &|r| lod::SECTION_CELLS - 1 - r,
        );
    }
    let kind = lod::kind_from_bounds_of(c, &bounds, world);
    match kind {
        LodKind::Empty => cells.fill(M.air),
        LodKind::Buried => cells.fill(M.stone),
        LodKind::Content => {}
    }
    kind
}
