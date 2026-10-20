//! Greedy mesher for LOD sections (`client/src/mesh/lodMesher.ts`; ARCHITECTURE.md §6.6): a
//! section's 34³ cells (its 32³ and a one-cell apron, `dwell_world::lod::cell_index` order — the
//! chunk mesher's padded layout) → flat-coloured geometry in cell units. Every material but air and
//! liquids is a full cube at LOD; each face has its material's flat colour (the average of its
//! texture) with the chunk mesher's face shading. Faces on the section's border that the apron
//! hides go to a per-side *skirt* instead of being dropped: the renderer shows a side's skirt when
//! the neighbour there is not drawn at the same level, which closes the cracks between levels.
//!
//! As in the TS, arithmetic is in `f64` and the per-column tables that are `Float32Array`s there
//! are `f32` here, so heights round the same way.

use std::sync::OnceLock;

use dwell_blocks::{STATES, StateId};
use dwell_world::lod::{PAD as LOD_PAD, SECTION_CELLS, VOLUME as LOD_VOLUME, cell_index};

use crate::LengthError;
use crate::js_round;
use crate::style::{Look, fluid_value, material_style};
use crate::textures::{average_tile_color, srgb_to_linear};
use dwell_look::face_tint;

const N: i32 = SECTION_CELLS;
const NU: usize = SECTION_CELLS as usize;
const PADU: usize = LOD_PAD as usize;

/// A flat-coloured mesh (see the crate docs for the layout).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlatMesh {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    /// Shading × flat material colour, linear light.
    pub colors: Vec<f32>,
    /// A section's `water` mesh: the fluid id per vertex (`crate::NO_FLUID` for a see-through
    /// face that is no fluid's); empty in every other mesh.
    pub fluids: Vec<f32>,
    pub indices: Vec<u32>,
}

impl FlatMesh {
    /// Bytes of all streams.
    pub fn byte_len(&self) -> usize {
        4 * (self.positions.len()
            + self.normals.len()
            + self.colors.len()
            + self.fluids.len()
            + self.indices.len())
    }
}

/// A section's meshes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SectionMeshes {
    pub opaque: FlatMesh,
    /// Liquids, drawn see-through after the opaque pass.
    pub water: FlatMesh,
    /// Border faces hidden by the apron, per face index (0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z).
    pub skirts: [FlatMesh; 6],
}

/// GPU bytes of a result (for the LOD cache budget).
pub fn section_bytes(m: &SectionMeshes) -> usize {
    m.opaque.byte_len()
        + m.water.byte_len()
        + m.skirts.iter().map(FlatMesh::byte_len).sum::<usize>()
}

const FACES: [(usize, i32); 6] = [(0, 1), (0, -1), (1, 1), (1, -1), (2, 1), (2, -1)];

fn cell(cells: &[u16], x: i32, y: i32, z: i32) -> StateId {
    cells[cell_index(x, y, z)]
}

fn is_liquid(m: StateId) -> bool {
    static LIQUID: OnceLock<Vec<bool>> = OnceLock::new();
    let table = LIQUID.get_or_init(|| {
        (0..STATES.len())
            .map(|i| material_style(i as StateId).look == Look::Fluid)
            .collect()
    });
    match table.get(usize::from(m)) {
        Some(&liquid) => liquid,
        // A state registered at run time (a synced one, ADR 0038).
        None => material_style(m).look == Look::Fluid,
    }
}

fn compute_lod_color(m: StateId, group: usize) -> u32 {
    let style = material_style(m);
    match style.textures {
        Some(t) => average_tile_color(match group {
            0 => t.top,
            2 => t.bottom,
            _ => t.side,
        }),
        None => style.color,
    }
}

/// A material's flat colour (sRGB `0xRRGGBB`) on a face group (0 top, 1 side, 2 bottom).
pub fn lod_color(m: StateId, group: usize) -> u32 {
    // One table per texture set: the colours are its tiles' averages.
    static COLORS: [OnceLock<Vec<[u32; 3]>>; 2] = [OnceLock::new(), OnceLock::new()];
    let set = crate::textures::texture_set();
    let table = COLORS[set.index()].get_or_init(|| {
        (0..STATES.len())
            .map(|i| [0, 1, 2].map(|g| compute_lod_color(i as StateId, g)))
            .collect()
    });
    match table.get(usize::from(m)) {
        Some(c) => c[group.min(2)],
        None => compute_lod_color(m, group),
    }
}

/// Step sides on slopes: full detail shows the side faces of one-block steps on a hillside, about
/// a third of the gradient as a share of its surface, which a distant cell's smooth top does not
/// have. Tops are tinted toward the material's side colour by that share, in TINT_STEPS steps (so
/// equal tops still merge).
pub const SIDE_SHARE_PER_GRADIENT: f64 = 1.0 / 3.0;
pub const SIDE_SHARE_MAX: f64 = 0.5;
const TINT_STEPS: i32 = 8;

/// A material's top colour with `tint` (0..TINT_STEPS) eighths of its side colour mixed in.
fn lod_top(m: StateId, tint: i32) -> u32 {
    if tint == 0 {
        return lod_color(m, 0);
    }
    let a = lod_color(m, 0);
    let b = lod_color(m, 1);
    let f = f64::from(tint) / f64::from(TINT_STEPS);
    let mut c = 0;
    for shift in [16, 8, 0] {
        let v = js_round(
            f64::from((a >> shift) & 0xff) * (1.0 - f) + f64::from((b >> shift) & 0xff) * f,
        );
        c |= (v as u32) << shift;
    }
    c
}

fn linear_rgb(color: u32, tint: [f64; 3]) -> [f32; 3] {
    [
        (srgb_to_linear(((color >> 16) & 0xff) as u8) * tint[0]) as f32,
        (srgb_to_linear(((color >> 8) & 0xff) as u8) * tint[1]) as f32,
        (srgb_to_linear((color & 0xff) as u8) * tint[2]) as f32,
    ]
}

#[derive(Default)]
struct Builder {
    m: FlatMesh,
}

impl Builder {
    /// Tags the vertices added since the last tag with material `m`'s fluid (the water mesh).
    fn tag_fluid(&mut self, m: StateId) {
        let n = self.m.positions.len() / 3;
        self.m
            .fluids
            .resize(n, fluid_value(material_style(m).fluid));
    }

    fn push_indices(&mut self, base: u32, sign: i32) {
        if sign > 0 {
            self.m
                .indices
                .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        } else {
            self.m
                .indices
                .extend([base, base + 2, base + 1, base, base + 3, base + 2]);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn quad(
        &mut self,
        axis: usize,
        sign: i32,
        plane: f64,
        (u0, u1): (f64, f64),
        (v0, v1): (f64, f64),
        color: u32,
    ) {
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        let base = (self.m.positions.len() / 3) as u32;
        // Linear, like the chunks' texels (their sRGB texture is decoded before lighting).
        let rgb = linear_rgb(color, face_tint(axis, sign));
        let s = f64::from(sign);
        for (cu, cv) in [(u0, v0), (u1, v0), (u1, v1), (u0, v1)] {
            let mut p = [0.0; 3];
            p[axis] = plane;
            p[u] = cu;
            p[v] = cv;
            self.m.positions.extend(p.map(|c| c as f32));
            self.m.normals.extend([
                if axis == 0 { s } else { 0.0 } as f32,
                if axis == 1 { s } else { 0.0 } as f32,
                if axis == 2 { s } else { 0.0 } as f32,
            ]);
            self.m.colors.extend(rgb);
        }
        self.push_indices(base, sign);
    }

    /// A convex polygon (counter-clockwise from outside) of cell-local points `pts` at `at`, shaded
    /// by its normal as the chunks shade sloped faces.
    fn polygon(&mut self, pts: &[[f32; 3]], at: [f64; 3], color: u32) {
        let p: Vec<[f64; 3]> = pts
            .iter()
            .map(|q| {
                [
                    at[0] + f64::from(q[0]),
                    at[1] + f64::from(q[1]),
                    at[2] + f64::from(q[2]),
                ]
            })
            .collect();
        let (a, b, c) = (p[0], p[1], p[2]);
        let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
        let n = n.map(|v| v / len);
        let rgb = linear_rgb(color, dwell_look::normal_tint(n[0], n[1], n[2]));
        let base = (self.m.positions.len() / 3) as u32;
        for q in &p {
            self.m.positions.extend(q.map(|v| v as f32));
            self.m.normals.extend(n.map(|v| v as f32));
            self.m.colors.extend(rgb);
        }
        for k in 1..p.len() as u32 - 1 {
            self.m.indices.extend([base, base + k, base + k + 1]);
        }
    }

    /// A wall in the plane `axis = plane` (axis 0: u = y, v = z; axis 2: u = x, v = y) over the run
    /// [b0, b1] along the wall, from heights lo to hi that may differ at the two ends.
    #[allow(clippy::too_many_arguments)]
    fn wall(
        &mut self,
        axis: usize,
        sign: i32,
        plane: f64,
        b0: f64,
        b1: f64,
        lo: [f64; 2],
        hi: [f64; 2],
        color: u32,
    ) {
        let rgb = linear_rgb(color, face_tint(axis, sign));
        let base = (self.m.positions.len() / 3) as u32;
        // The same corner order as `quad`: (u0, v0), (u1, v0), (u1, v1), (u0, v1).
        let corners = if axis == 0 {
            [
                [plane, lo[0], b0],
                [plane, hi[0], b0],
                [plane, hi[1], b1],
                [plane, lo[1], b1],
            ]
        } else {
            [
                [b0, lo[0], plane],
                [b1, lo[1], plane],
                [b1, hi[1], plane],
                [b0, hi[0], plane],
            ]
        };
        let s = f64::from(sign);
        for c in corners {
            self.m.positions.extend(c.map(|v| v as f32));
            self.m.normals.extend([
                if axis == 0 { s } else { 0.0 } as f32,
                0.0,
                if axis == 2 { s } else { 0.0 } as f32,
            ]);
            self.m.colors.extend(rgb);
        }
        self.push_indices(base, sign);
    }
}

/// Floats per column of a generated section's surface (C++ core::LodSurface): height, material,
/// flags (1 valid, 2 wet), and where wet the water's level.
pub const SURFACE_STRIDE: usize = 4;
/// Surface heights are drawn in steps of 1/SURFACE_STEPS of a cell.
pub const SURFACE_STEPS: i32 = 2;
const SURFACE_VALID: i32 = 1;
const SURFACE_WET: i32 = 2;

/// Options of [`mesh_section`].
#[derive(Clone, Copy, Debug, Default)]
pub struct MeshSectionOptions<'a> {
    /// Column surfaces (generated sections): each column's exact surface, 34² × SURFACE_STRIDE
    /// floats in (z + 1) · 34 + (x + 1) order — height in cells from the section's bottom,
    /// material, flags (1 valid, 2 wet), and where wet the water's level in cells. A cell is
    /// filled from its bottom voxel, so its top lifts the ground by up to a cell; where a column's
    /// topmost solid cell holds its surface, that cell's top is drawn at the surface instead, with
    /// walls down to lower neighbours. Likewise a column's water is drawn at its level. Ignored
    /// when shorter than 34² × SURFACE_STRIDE.
    pub surface: Option<&'a [f32]>,
    /// How far (in cells) a liquid's top face sits below its cell's top — the chunks draw water's
    /// surface at 7/8 of a block, so at sea level it is 1/8 m below the LOD cells' grid.
    pub water_drop: f64,
    /// A section of face B (BIFACIAL_WORLD.md §5): meshed as its mirror image — cell rows reversed
    /// (r ↔ 31 − r, the apron's too), with `surface` in that mirrored frame — and the geometry
    /// turned back: y → 32 − y, triangles re-wound, the ±Y skirts swapped. Face tints stay.
    pub mirror: bool,
}

/// The rows of a section's cells reversed: row r ↔ row 31 − r, apron rows −1 and 32 included.
/// Entries past the section's volume are ignored; missing ones read as air.
pub fn mirror_rows(cells: &[u16]) -> Vec<u16> {
    let mut out = vec![0u16; LOD_VOLUME];
    let layer = PADU * PADU;
    for y in -1..=N {
        let from = (y + 1) as usize * layer;
        let to = (N - 1 - y + 1) as usize * layer;
        for i in 0..layer {
            out[to + i] = cells.get(from + i).copied().unwrap_or(0);
        }
    }
    out
}

/// A mesh in a section's cell units mirrored in y (about the section's middle): re-wound.
fn flip_mesh_y(mut m: FlatMesh) -> FlatMesh {
    for p in m.positions.iter_mut().skip(1).step_by(3) {
        *p = (f64::from(N) - f64::from(*p)) as f32;
    }
    for n in m.normals.iter_mut().skip(1).step_by(3) {
        *n = -*n;
    }
    for t in m.indices.chunks_exact_mut(3) {
        t.swap(1, 2);
    }
    m
}

/// Meshes a section's cells (34³, `cell_index` order); positions in cells, 0..32. Liquids are
/// see-through surfaces (the `water` mesh) over the floor, as the chunks draw them.
pub fn mesh_section(
    cells: &[u16],
    options: &MeshSectionOptions<'_>,
) -> Result<SectionMeshes, LengthError> {
    if options.mirror {
        let m = mesh_section_above(&mirror_rows(cells), options)?;
        let [s0, s1, s2, s3, s4, s5] = m.skirts;
        let skirts = [s0, s1, s3, s2, s4, s5].map(flip_mesh_y);
        return Ok(SectionMeshes {
            opaque: flip_mesh_y(m.opaque),
            water: flip_mesh_y(m.water),
            skirts,
        });
    }
    mesh_section_above(cells, options)
}

/// Column index in the 34 × 34 padded layout, x and z each −1..32.
fn col(x: i32, z: i32) -> usize {
    (x + 1 + LOD_PAD * (z + 1)) as usize
}

/// mesh_section for a section whose terrain faces up (face A, or a mirrored face B).
fn mesh_section_above(
    cells: &[u16],
    options: &MeshSectionOptions<'_>,
) -> Result<SectionMeshes, LengthError> {
    if cells.len() != LOD_VOLUME {
        return Err(LengthError {
            expected: LOD_VOLUME,
            got: cells.len(),
        });
    }
    let water_drop = options.water_drop;
    let mut opaque = Builder::default();
    let mut water_mesh = Builder::default();
    let mut skirts: [Builder; 6] = Default::default();
    let special = find_surfaces(cells, options.surface);
    // Merge key per slice cell: 0 none, else ((material + 1) · 9 + tint) · 2 + (1 for a skirt).
    let mut mask = vec![0i32; NU * NU];

    for (face, &(axis, sign)) in FACES.iter().enumerate() {
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        let group = if axis == 1 {
            if sign > 0 { 0 } else { 2 }
        } else {
            1
        };
        for d in 0..N {
            mask.fill(0);
            let mut any = false;
            let border = if sign > 0 { d == N - 1 } else { d == 0 };
            for j in 0..N {
                for i in 0..N {
                    let mut c = [0i32; 3];
                    c[axis] = d;
                    c[u] = i;
                    c[v] = j;
                    let m = cell(cells, c[0], c[1], c[2]);
                    if m == 0 {
                        continue;
                    }
                    let wc = col(c[0], c[2]);
                    // A column's surface cell: its top and sides are drawn at the surface (below).
                    let sy = i32::from(special.y[wc]);
                    if face != 3 && sy == c[1] {
                        continue;
                    }
                    // Under a sea floor drawn in the water cell above, the top is covered — and
                    // where the floor lies on the cell's bottom, in the same plane.
                    if face == 2 && sy == c[1] + 1 {
                        continue;
                    }
                    // The top liquid cell of a column with a water level: its top is drawn at the
                    // level (emit_surfaces), its sides up to it (below).
                    let at_level = i32::from(special.wy[wc]) == c[1];
                    if at_level && face == 2 {
                        continue;
                    }
                    let y = c[1];
                    c[axis] = d + sign;
                    let n = cell(cells, c[0], c[1], c[2]);
                    let liquid = is_liquid(m);
                    let n_solid = n != 0 && !is_liquid(n);
                    let hidden = n_solid || (liquid && n != 0);
                    if hidden && (!border || liquid) {
                        continue;
                    }
                    if at_level && axis != 1 {
                        let lid = f64::from(special.w[wc]) - water_drop;
                        let plane = f64::from(if sign > 0 { d + 1 } else { d });
                        let color = lod_color(m, group);
                        let (fi, fj, fy) = (f64::from(i), f64::from(j), f64::from(y));
                        if axis == 0 {
                            water_mesh.quad(0, sign, plane, (fy, lid), (fj, fj + 1.0), color);
                        } else {
                            water_mesh.quad(2, sign, plane, (fi, fi + 1.0), (fy, lid), color);
                        }
                        water_mesh.tag_fluid(m);
                        continue;
                    }
                    // An exposed top takes its column's tint (tint_slopes).
                    let tint = if face == 2 && n == 0 && !liquid {
                        i32::from(special.t[wc])
                    } else {
                        0
                    };
                    mask[i as usize + NU * j as usize] =
                        ((i32::from(m) + 1) * (TINT_STEPS + 1) + tint) * 2 + i32::from(hidden);
                    any = true;
                }
            }
            if !any {
                continue;
            }
            for j in 0..NU {
                let mut i = 0;
                while i < NU {
                    let key = mask[i + NU * j];
                    if key == 0 {
                        i += 1;
                        continue;
                    }
                    let mut w = 1;
                    while i + w < NU && mask[i + w + NU * j] == key {
                        w += 1;
                    }
                    let mut h = 1;
                    'grow: while j + h < NU {
                        for k in 0..w {
                            if mask[i + k + NU * (j + h)] != key {
                                break 'grow;
                            }
                        }
                        h += 1;
                    }
                    for dv in 0..h {
                        mask[i + NU * (j + dv)..i + w + NU * (j + dv)].fill(0);
                    }
                    let tint = (key >> 1) % (TINT_STEPS + 1);
                    let m = ((key >> 1) / (TINT_STEPS + 1) - 1) as StateId;
                    let skirt = key & 1 == 1;
                    let water = !skirt && is_liquid(m);
                    let target = if skirt {
                        &mut skirts[face]
                    } else if water {
                        &mut water_mesh
                    } else {
                        &mut opaque
                    };
                    // Water's surface where the chunks draw it (water_drop below the cell's top).
                    let drop = if axis == 1 && sign > 0 && is_liquid(m) {
                        water_drop
                    } else {
                        0.0
                    };
                    let color = if group == 0 && tint > 0 {
                        lod_top(m, tint)
                    } else {
                        lod_color(m, group)
                    };
                    target.quad(
                        axis,
                        sign,
                        f64::from(if sign > 0 { d + 1 } else { d }) - drop,
                        (i as f64, (i + w) as f64),
                        (j as f64, (j + h) as f64),
                        color,
                    );
                    if water {
                        target.tag_fluid(m);
                    }
                    i += w;
                }
            }
        }
    }
    emit_surfaces(
        cells,
        &special,
        &mut opaque,
        &mut water_mesh,
        &mut skirts,
        water_drop,
    );
    Ok(SectionMeshes {
        opaque: opaque.m,
        water: water_mesh.m,
        skirts: skirts.map(|s| s.m),
    })
}

struct Surfaces {
    /// The surface cell's y per column (NO_SURFACE: none): its top is drawn at `h`.
    y: Vec<i16>,
    /// Surface height in cells from the section's bottom.
    h: Vec<f32>,
    /// The surface cell's material (for a sea's floor drawn in a water cell, the floor's).
    m: Vec<u16>,
    /// The column's topmost liquid cell (with air above it), whose top is drawn at `w`.
    wy: Vec<i16>,
    /// Its water level in cells from the section's bottom (within that cell).
    w: Vec<f32>,
    /// 1 where the column or a neighbour is wet: its ground is drawn at its exact height.
    shore: Vec<u8>,
    /// The column's top tint toward its side colour, in TINT_STEPS steps.
    t: Vec<u8>,
    /// Where the surface cell is an adaptive top block with a sloped top (ADAPTIVE_BLOCKS.md §4.4):
    /// its shape (index into SHAPES; 0 none). A flat top (the rules' slab) is drawn at `h`.
    shape: Vec<u16>,
    /// Apron columns whose surface cell is an adaptive top block: their shape needs cells beyond
    /// the apron, so walls toward them are drawn down to the row's floor (any part they hide lies
    /// inside them).
    apron: Vec<bool>,
}

const NO_SURFACE: i16 = -1000;

/// JS `ToInt32` of a float (`flags & 2`).
fn to_int32(v: f64) -> i32 {
    if !v.is_finite() {
        return 0;
    }
    (v.trunc() as i64).rem_euclid(1 << 32) as u32 as i32
}

/// JS `ToUint16` of a float (a store into a `Uint16Array`).
fn to_uint16(v: f64) -> u16 {
    if !v.is_finite() {
        return 0;
    }
    (v.trunc() as i64).rem_euclid(1 << 16) as u16
}

/// The topmost filled cell of column (x, z) at or below `top`, or −2 for none.
fn topmost_filled(cells: &[u16], x: i32, z: i32) -> i32 {
    let mut y = N;
    while y >= -1 && cell(cells, x, y, z) == 0 {
        y -= 1;
    }
    y
}

/// Which cell of each column holds its surface, where the surface data says so.
fn find_surfaces(cells: &[u16], surface: Option<&[f32]>) -> Surfaces {
    let n = PADU * PADU;
    let mut out = Surfaces {
        y: vec![NO_SURFACE; n],
        h: vec![0.0; n],
        m: vec![0; n],
        wy: vec![NO_SURFACE; n],
        w: vec![0.0; n],
        shore: vec![0; n],
        t: vec![0; n],
        shape: vec![0; n],
        apron: vec![false; n],
    };
    let data = surface.filter(|s| s.len() >= n * SURFACE_STRIDE);
    let top = N;
    if let Some(data) = data {
        let at = |c: usize, k: usize| f64::from(data[c * SURFACE_STRIDE + k]);
        // Water levels: a wet column's topmost cell, if liquid, has its top at the water's level.
        for z in -1..=top {
            for x in -1..=top {
                let c = col(x, z);
                let flags = to_int32(at(c, 2));
                if flags & SURFACE_VALID == 0 || flags & SURFACE_WET == 0 {
                    continue;
                }
                let y = topmost_filled(cells, x, z);
                if y < -1 || !is_liquid(cell(cells, x, y, z)) {
                    continue;
                }
                let level = at(c, 3);
                let fy = f64::from(y);
                if level <= fy + 1e-6 || level > fy + 1.0 + 1e-4 {
                    continue; // not this cell's: its top
                }
                out.wy[c] = y as i16;
                out.w[c] = level.min(fy + 1.0) as f32;
            }
        }
        // Shores: dry columns beside wet ones, drawn at their exact height. A column is wet with
        // the wet flag, or when its topmost filled cell is liquid.
        let mut wet = vec![0u8; n];
        for z in -1..=top {
            for x in -1..=top {
                let c = col(x, z);
                let y = topmost_filled(cells, x, z);
                if to_int32(at(c, 2)) & SURFACE_WET != 0
                    || (y >= -1 && is_liquid(cell(cells, x, y, z)))
                {
                    wet[c] = 1;
                }
            }
        }
        for z in -1..=top {
            for x in -1..=top {
                let c = col(x, z);
                if wet[c] != 0 {
                    continue;
                }
                let mut beside = false;
                'search: for dz in -1..=1 {
                    for dx in -1..=1 {
                        let (nx, nz) = (x + dx, z + dz);
                        if nx >= -1 && nx <= top && nz >= -1 && nz <= top && wet[col(nx, nz)] == 1 {
                            beside = true;
                            break 'search;
                        }
                    }
                }
                if beside {
                    out.shore[c] = 1;
                }
            }
        }
        for z in -1..=top {
            for x in -1..=top {
                let c = col(x, z);
                let flags = to_int32(at(c, 2));
                if flags & SURFACE_VALID == 0 {
                    continue;
                }
                let h = at(c, 0);
                // The topmost solid cell (above it only air or liquid), and it must hold the surface.
                let mut y = top;
                while y >= -1 {
                    let m = cell(cells, x, y, z);
                    if m != 0 && !is_liquid(m) {
                        break;
                    }
                    // A sea's floor within one of its water cells.
                    let fy = f64::from(y);
                    if m != 0 && flags & SURFACE_WET != 0 && h >= fy && h <= fy + 1.0 {
                        break;
                    }
                    y -= 1;
                }
                let fy = f64::from(y);
                if y < -1 || y == top || h < fy - 1e-3 || h > fy + 1.0 + 1e-3 {
                    continue;
                }
                let m = cell(cells, x, y, z);
                let level = if i32::from(out.wy[c]) == y {
                    f64::from(out.w[c])
                } else {
                    fy + 1.0
                };
                if out.shore[c] != 0 && !is_liquid(m) {
                    // A bank beside water: at its height (a full cell is drawn as usual).
                    if h >= fy + 1.0 - 1e-3 {
                        continue;
                    }
                    out.y[c] = y as i16;
                    out.h[c] = h.max(fy) as f32;
                    out.m[c] = m;
                    continue;
                }
                let steps_f = f64::from(SURFACE_STEPS);
                let mut steps = js_round((h - fy).clamp(0.0, 1.0) * steps_f);
                if steps == steps_f && !is_liquid(m) {
                    continue;
                }
                // A sea floor is never drawn at or over its water's surface when that surface is
                // this cell's: the highest step strictly below the level.
                if is_liquid(m) && cell(cells, x, y + 1, z) == 0 {
                    steps = steps.min(0f64.max(((level - fy) * steps_f).ceil() - 1.0));
                }
                out.y[c] = y as i16;
                out.h[c] = (fy + steps / steps_f) as f32;
                out.m[c] = if is_liquid(m) { to_uint16(at(c, 1)) } else { m };
            }
        }
    }
    adaptive_tops(cells, &mut out);
    tint_slopes(cells, &mut out);
    out
}

/// The adaptive rule on LOD cells (ADAPTIVE_BLOCKS.md §4.4): a dry column's topmost cell, if an
/// adaptive block with nothing above it, takes the shape the rule table gives its neighbourhood
/// of cells — corner heights in halves of a cell — replacing the surface data's heights there.
/// Wet columns and shores keep their exact heights.
fn adaptive_tops(cells: &[u16], out: &mut Surfaces) {
    let top = N;
    let inside = |v: i32| (-1..=top).contains(&v);
    for z in -1..=top {
        for x in -1..=top {
            let c = col(x, z);
            if out.shore[c] != 0 {
                continue;
            }
            let y = topmost_filled(cells, x, z);
            if y < 0 || y >= top {
                continue; // none, or its top is another section's
            }
            let m = cell(cells, x, y, z);
            if is_liquid(m) || !STATES.get(usize::from(m)).is_some_and(|s| s.adaptive) {
                continue;
            }
            if out.y[c] != NO_SURFACE && i32::from(out.y[c]) != y {
                continue; // a sea floor drawn in a water cell
            }
            let fy = y as f32;
            if !(0..top).contains(&x) || !(0..top).contains(&z) {
                out.apron[c] = true;
                out.y[c] = y as i16;
                out.h[c] = fy + 0.5;
                out.m[c] = m;
                continue;
            }
            let get = |dx: i32, dy: i32, dz: i32| {
                let (a, b, d) = (x + dx, y + dy, z + dz);
                if inside(a) && inside(b) && inside(d) {
                    cell(cells, a, b, d)
                } else {
                    0
                }
            };
            let Some(i) = dwell_blocks::adaptive::derive(get, 1) else {
                continue;
            };
            let shape = &dwell_blocks::SHAPES[usize::from(i)];
            if !shape.fan && shape.corners == [2, 2, 2, 2] {
                out.y[c] = NO_SURFACE; // the rules say full: a plain cell
                continue;
            }
            out.y[c] = y as i16;
            out.m[c] = m;
            out.h[c] = fy + f32::from(shape.points[1][1]) / 4.0;
            let flat = shape
                .points
                .iter()
                .flatten()
                .all(|&q| q == shape.points[0][0]);
            out.shape[c] = if flat { 0 } else { i };
        }
    }
}

/// Each dry column's top tint from its gradient: its height against its four neighbours'.
fn tint_slopes(cells: &[u16], out: &mut Surfaces) {
    let top = N;
    let mut height = vec![f32::NAN; PADU * PADU];
    for z in -1..=top {
        for x in -1..=top {
            let c = col(x, z);
            if out.y[c] != NO_SURFACE {
                height[c] = out.h[c];
                continue;
            }
            let y = topmost_filled(cells, x, z);
            if y < -1 || is_liquid(cell(cells, x, y, z)) {
                continue; // none, or under water
            }
            height[c] = (y + 1) as f32;
        }
    }
    for z in 0..top {
        for x in 0..top {
            let c = col(x, z);
            let h = f64::from(height[c]);
            if h.is_nan() {
                continue;
            }
            let mut gradient: f64 = 0.0;
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let n = f64::from(height[col(x + dx, z + dz)]);
                if !n.is_nan() {
                    gradient = gradient.max((n - h).abs());
                }
            }
            let share = SIDE_SHARE_MAX.min(gradient * SIDE_SHARE_PER_GRADIENT);
            out.t[c] = js_round(share * f64::from(TINT_STEPS)) as u8;
        }
    }
}

/// Heights (in cells) of a column's top along its side (axis, sign) at the side's ends and middle,
/// in the order the wall runs (z for ±X walls, x for ±Z). Flat tops are level; adaptive tops follow
/// their shape.
fn profile(s: &Surfaces, c: usize, axis: usize, sign: i32) -> [f64; 3] {
    let h = f64::from(s.h[c]);
    if s.shape[c] == 0 {
        return [h, h, h];
    }
    let p = dwell_blocks::SHAPES[usize::from(s.shape[c])].points;
    let y = f64::from(s.y[c]);
    let q: [u8; 3] = match (axis, sign > 0) {
        (0, true) => [p[0][2], p[1][2], p[2][2]],
        (0, false) => [p[0][0], p[1][0], p[2][0]],
        (_, true) => p[2],
        (_, false) => p[0],
    };
    q.map(|v| y + f64::from(v) / 4.0)
}

/// [axis, sign, face index] for ±X and ±Z.
const SIDES: [(usize, i32, usize); 4] = [(0, 1, 0), (0, -1, 1), (2, 1, 4), (2, -1, 5)];

/// A wall between two columns, before merging runs of equal ones.
#[derive(Clone, Copy, Debug)]
struct Wall {
    /// 0 opaque, else skirt face + 1.
    target: usize,
    sign: i32,
    /// Heights at the wall's two ends and middle.
    lo: [f64; 3],
    hi: [f64; 3],
    material: StateId,
}

impl Wall {
    fn flat(&self) -> bool {
        self.lo.iter().all(|&v| v == self.lo[0]) && self.hi.iter().all(|&v| v == self.hi[0])
    }
}

/// Tops at the surface height and the walls between columns (see mesh_section), merged like the
/// rest: tops of equal height and material into rectangles, walls along a row into strips. A sea
/// floor drawn in a water cell gets that cell's water surface above it.
fn emit_surfaces(
    cells: &[u16],
    s: &Surfaces,
    opaque: &mut Builder,
    water_mesh: &mut Builder,
    skirts: &mut [Builder; 6],
    water_drop: f64,
) {
    // Tops: greedy rectangles over (z, x) of equal height and material.
    let mut top_key = vec![0f64; NU * NU];
    let mut top_h = vec![0f32; NU * NU];
    for z in 0..N {
        for x in 0..N {
            let c = col(x, z);
            if s.y[c] < 0 {
                continue;
            }
            if s.shape[c] != 0 {
                // A sloped adaptive top: its shape's surface, polygon by polygon.
                let shape = &dwell_blocks::SHAPES[usize::from(s.shape[c])];
                let color = lod_top(s.m[c], i32::from(s.t[c]));
                let at = [f64::from(x), f64::from(s.y[c]), f64::from(z)];
                for f in dwell_blocks::shape::faces_of(shape) {
                    if f.tag == dwell_blocks::shape::SURFACE_TAG {
                        opaque.polygon(&f.v[..usize::from(f.count)], at, color);
                    }
                }
                continue;
            }
            let at = z as usize + NU * x as usize;
            top_key[at] = f64::from((i32::from(s.m[c]) + 1) * (TINT_STEPS + 1) + i32::from(s.t[c]));
            top_h[at] = s.h[c];
        }
    }
    let same_top = |k: &[f64], a: usize, b: usize| k[a] == k[b] && top_h[a] == top_h[b];
    for x in 0..NU {
        let mut z = 0;
        while z < NU {
            let at = z + NU * x;
            let key = top_key[at];
            if key == 0.0 {
                z += 1;
                continue;
            }
            let mut w = 1;
            while z + w < NU && same_top(&top_key, z + w + NU * x, at) {
                w += 1;
            }
            let mut d = 1;
            'grow: while x + d < NU {
                for k in 0..w {
                    if !same_top(&top_key, z + k + NU * (x + d), at) {
                        break 'grow;
                    }
                }
                d += 1;
            }
            let h = f64::from(top_h[at]);
            for dx in 0..d {
                top_key[z + NU * (x + dx)..z + w + NU * (x + dx)].fill(0.0);
            }
            // The top: axis y, u = z, v = x.
            let key = key as i32;
            let material = (key / (TINT_STEPS + 1) - 1) as StateId;
            opaque.quad(
                1,
                1,
                h,
                (z as f64, (z + w) as f64),
                (x as f64, (x + d) as f64),
                lod_top(material, key % (TINT_STEPS + 1)),
            );
            z += w;
        }
    }

    // Water surfaces at their level: over floors drawn inside water cells, and over the top liquid
    // cell of any column with a water level (rivers and lakes above sea level).
    {
        let mut level = vec![0f32; NU * NU];
        for z in 0..N {
            for x in 0..N {
                let c = col(x, z);
                let at = z as usize + NU * x as usize;
                top_key[at] = 0.0;
                let mut y = s.wy[c];
                let mut w = s.w[c];
                if y == NO_SURFACE {
                    // A floor drawn in a water cell with no level of its own (a sea): the cell's top.
                    y = s.y[c];
                    if y < 0 {
                        continue;
                    }
                    let yi = i32::from(y);
                    let m = cell(cells, x, yi, z);
                    let above = cell(cells, x, yi + 1, z);
                    if !is_liquid(m) || above != 0 {
                        continue;
                    }
                    w = (yi + 1) as f32;
                }
                if y < 0 || i32::from(y) >= N {
                    continue;
                }
                top_key[at] = f64::from(cell(cells, x, i32::from(y), z)) + 1.0;
                level[at] = w;
            }
        }
        let same = |k: &[f64], a: usize, b: usize| k[a] == k[b] && level[a] == level[b];
        for x in 0..NU {
            let mut z = 0;
            while z < NU {
                let at = z + NU * x;
                let key = top_key[at];
                if key == 0.0 {
                    z += 1;
                    continue;
                }
                let mut w = 1;
                while z + w < NU && same(&top_key, z + w + NU * x, at) {
                    w += 1;
                }
                let mut d = 1;
                'grow: while x + d < NU {
                    for k in 0..w {
                        if !same(&top_key, z + k + NU * (x + d), at) {
                            break 'grow;
                        }
                    }
                    d += 1;
                }
                let top = f64::from(level[at]);
                for dx in 0..d {
                    top_key[z + NU * (x + dx)..z + w + NU * (x + dx)].fill(0.0);
                }
                water_mesh.quad(
                    1,
                    1,
                    top - water_drop,
                    (z as f64, (z + w) as f64),
                    (x as f64, (x + d) as f64),
                    lod_color((key - 1.0) as StateId, 0),
                );
                water_mesh.tag_fluid((key - 1.0) as StateId);
                z += w;
            }
        }
    }

    // Walls, per side and slot (see wall_of): computed per column, then runs of equal flat walls
    // along the row merged; sloped walls stand alone.
    for (axis, sign, face_index) in SIDES {
        for slot in 0..2 {
            for a in 0..N {
                // `a` is the coordinate along the wall's axis (x for ±X walls, z for ±Z), `b`
                // along the row.
                let mut run: Option<Wall> = None;
                let mut run_start = 0;
                for b in 0..=N {
                    let wall = if b < N {
                        let (x, z) = if axis == 0 { (a, b) } else { (b, a) };
                        wall_of(cells, s, x, z, axis, sign, face_index, slot)
                    } else {
                        None
                    };
                    let same = match (&wall, &run) {
                        (Some(w), Some(r)) => {
                            w.flat()
                                && r.flat()
                                && w.target == r.target
                                && w.sign == r.sign
                                && w.lo[0] == r.lo[0]
                                && w.hi[0] == r.hi[0]
                                && w.material == r.material
                        }
                        _ => false,
                    };
                    if same {
                        continue;
                    }
                    // Flush the run: a flat run whole, a sloped wall (one column) in two halves.
                    if let Some(r) = run {
                        let plane = f64::from(if sign > 0 { a + 1 } else { a });
                        let builder = if r.target == 0 {
                            &mut *opaque
                        } else {
                            &mut skirts[r.target - 1]
                        };
                        let (b0, b1) = (f64::from(run_start), f64::from(b));
                        let halves = if r.flat() {
                            vec![(b0, b1, 0, 0)]
                        } else {
                            let m = (b0 + b1) / 2.0;
                            vec![(b0, m, 0, 1), (m, b1, 1, 2)]
                        };
                        for (s0, s1, i, j) in halves {
                            if r.hi[i] - r.lo[i] < 1e-4 && r.hi[j] - r.lo[j] < 1e-4 {
                                continue;
                            }
                            builder.wall(
                                axis,
                                r.sign,
                                plane,
                                s0,
                                s1,
                                [r.lo[i], r.lo[j]],
                                [r.hi[i], r.hi[j]],
                                lod_color(r.material, 1),
                            );
                        }
                    }
                    run = wall;
                    run_start = b;
                }
            }
        }
    }
}

/// The wall on one side of column (x, z) with a surface, if any (see mesh_section). Slot 0 is what
/// shows; slot 1 the skirt below it on the section's border, which the apron hides.
///
/// Every wall is drawn by exactly one section, including those on its border: a step between our
/// surface and a neighbour's across the border is ours to draw (the neighbour section's cells see
/// our apron cell as solid and draw only a skirt there, shown only when we are not).
#[allow(clippy::too_many_arguments)]
fn wall_of(
    cells: &[u16],
    s: &Surfaces,
    x: i32,
    z: i32,
    axis: usize,
    sign: i32,
    face: usize,
    slot: usize,
) -> Option<Wall> {
    let c = col(x, z);
    let y = s.y[c];
    if y < 0 {
        return None;
    }
    let yf = f64::from(y);
    let ours = profile(s, c, axis, sign);
    let nx = if axis == 0 { x + sign } else { x };
    let nz = if axis == 2 { z + sign } else { z };
    let nc = col(nx, nz);
    let border = !(0..N).contains(&nx) || !(0..N).contains(&nz);
    let nm = cell(cells, nx, i32::from(y), nz);
    let n_surface_here = s.y[nc] == y;
    let n_filled = n_surface_here || (nm != 0 && !is_liquid(nm));
    let material = s.m[c];
    let skirt = face + 1;
    let floor = [yf; 3];
    if !n_filled {
        // Open.
        return (slot == 0).then_some(Wall {
            target: 0,
            sign,
            lo: floor,
            hi: ours,
            material,
        });
    }
    if n_surface_here {
        // Both surfaces in this row: the higher one's side shows down to the lower (and below
        // that, on the border, a skirt). The neighbour's top along the same edge is its opposite
        // side.
        // (An apron column's adaptive top: its shape is unknown here, so it hides nothing.)
        let theirs = if s.apron[nc] {
            floor
        } else {
            profile(s, nc, axis, -sign)
        };
        let lo = [0, 1, 2].map(|i| theirs[i].min(ours[i]));
        if slot == 0 {
            return (0..3).any(|i| lo[i] < ours[i]).then_some(Wall {
                target: 0,
                sign,
                lo,
                hi: ours,
                material,
            });
        }
        return (border && lo.iter().any(|&v| v > yf)).then_some(Wall {
            target: skirt,
            sign,
            lo: floor,
            hi: lo,
            material,
        });
    }
    // A full neighbour cell stands taller: its face towards us, above our surface (and on the
    // border, our side below as a skirt).
    if slot == 0 {
        return Some(Wall {
            target: 0,
            sign: -sign,
            lo: ours,
            hi: [yf + 1.0; 3],
            material: nm,
        });
    }
    border.then_some(Wall {
        target: skirt,
        sign,
        lo: floor,
        hi: ours,
        material,
    })
}

#[cfg(test)]
#[path = "lod_mesher_tests.rs"]
mod tests;
