//! Greedy mesher for LOD sections (`client/src/mesh/lodMesher.ts`; ARCHITECTURE.md §6.6): a
//! section's 34³ cells (its 32³ and a one-cell apron, `dwell_world::lod::cell_index` order — the
//! chunk mesher's padded layout) → flat-coloured geometry in cell units. Every material but air and
//! liquids is a full cube at LOD; a top or bottom has its material's flat colour (the average of
//! its texture), a riser the area-weighted colour of the fine rise it stands for (`riser_color`,
//! ARCHITECTURE.md §6.6 "Colour"), with the chunk mesher's face shading. Faces on the section's border that the apron
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
use crate::textures::{
    TILE, average_tile_color, linear_to_srgb_byte, srgb_to_linear, tile_rows_average,
};
use dwell_look::face_tint;
use dwell_look::lod_colour::LOD_COLOUR;

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
    // The colours are the tiles' averages.
    static COLORS: OnceLock<Vec<[u32; 3]>> = OnceLock::new();
    let table = COLORS.get_or_init(|| {
        (0..STATES.len())
            .map(|i| [0, 1, 2].map(|g| compute_lod_color(i as StateId, g)))
            .collect()
    });
    match table.get(usize::from(m)) {
        Some(c) => c[group.min(2)],
        None => compute_lod_color(m, group),
    }
}

/// Risers' gradients are quantised to `LOD_COLOUR.gradient_step` up to this many steps.
const MAX_GRADIENT_STEPS: i32 = 255;

/// The rows of a side tile a one-cell riser shows (the top `CELL_H` of the 1 m tile) start here.
const FRINGE_ROW: usize = TILE - (TILE as f64 * dwell_world::CELL_H) as usize;

fn compute_side_bands(m: StateId) -> [u32; 2] {
    let style = material_style(m);
    match style.textures {
        Some(t) => [
            tile_rows_average(t.side, FRINGE_ROW, TILE),
            tile_rows_average(t.side, 0, FRINGE_ROW),
        ],
        None => [style.color; 2],
    }
}

/// A material's side colours (sRGB): [0] the top cell's band a one-cell riser shows, [1] the rest.
fn side_bands(m: StateId) -> [u32; 2] {
    static BANDS: OnceLock<Vec<[u32; 2]>> = OnceLock::new();
    let table = BANDS.get_or_init(|| {
        (0..STATES.len())
            .map(|i| compute_side_bands(i as StateId))
            .collect()
    });
    match table.get(usize::from(m)) {
        Some(&b) => b,
        None => compute_side_bands(m),
    }
}

/// A column's gradient (m per m) as a step of `LOD_COLOUR.gradient_step`.
fn gradient_step(gradient: f64) -> u8 {
    if gradient.is_nan() {
        return 0;
    }
    js_round(gradient / LOD_COLOUR.gradient_step).clamp(0.0, f64::from(MAX_GRADIENT_STEPS)) as u8
}

/// The flat colour (sRGB) of a coarse riser of material `own` in a column topped with `top` whose
/// ground has gradient step `step`: the area-weighted colour of the fine rise it stands for
/// (`dwell_look::lod_colour`): the top's top tile, the top's side band a one-cell step shows, and
/// `own`'s side below that band, by the makeup at that gradient, mixed in linear light as the
/// screen averages them. The top tile's share is the fine slope's ramps, shaded as the sloped faces
/// they are (a ½ m ramp, `normal_tint`), not as the vertical wall (`face_tint`) drawing it, so it
/// is scaled by the ratio of the two for the wall's side.
fn riser_color(own: StateId, top: StateId, step: u8, axis: usize, sign: i32) -> u32 {
    let [wt, wf, wr] = LOD_COLOUR.makeup(f64::from(step) * LOD_COLOUR.gradient_step);
    let colours = [lod_color(top, 0), side_bands(top)[0], side_bands(own)[1]];
    let ramp = ramp_tint_ratio(axis, sign);
    let mut c = 0;
    for (ch, shift) in [16, 8, 0].into_iter().enumerate() {
        let [a, b, r] = colours.map(|k| srgb_to_linear(((k >> shift) & 0xff) as u8));
        c |= u32::from(linear_to_srgb_byte(a * ramp[ch] * wt + b * wf + r * wr)) << shift;
    }
    c
}

/// A ½ m ramp's shading (rising CELL_H over 1 m, facing the wall's side) over the wall's.
fn ramp_tint_ratio(axis: usize, sign: i32) -> [f64; 3] {
    let len = (1.0 + dwell_world::CELL_H * dwell_world::CELL_H).sqrt();
    let h = f64::from(sign) * dwell_world::CELL_H / len;
    let (nx, nz) = if axis == 0 { (h, 0.0) } else { (0.0, h) };
    let ramp = dwell_look::normal_tint(nx, 1.0 / len, nz);
    let wall = face_tint(axis, sign);
    std::array::from_fn(|i| ramp[i] / wall[i])
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

/// How far (in cells) a section's top skirt is sunk below its top plane (ARCHITECTURE.md §6.6,
/// "Known bug class"). The top skirt covers the section's top row where the section above is not
/// drawn at the same level; the ground there lies above the plane — by as little as nothing, where
/// it crosses the plane — so a skirt on the plane was in the ground's plane and fought it as the
/// camera moved (dirt showing through the grass). Sunk, it is at least this far under any ground
/// above the plane, and still closes the hole it is for.
pub const SKIRT_SINK: f64 = 0.125;

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

/// A section's mesh in its cells scaled so that the section's uniform level scale (2^L) makes
/// metres: y × the cell height, normals from cell space to metres (HALF_HEIGHT_GRID.md §2).
fn mesh_to_metres(mut m: FlatMesh) -> FlatMesh {
    if dwell_world::CELL_H != 1.0 {
        for p in m.positions.iter_mut().skip(1).step_by(3) {
            *p = (f64::from(*p) * dwell_world::CELL_H) as f32;
        }
        for n in m.normals.chunks_exact_mut(3) {
            let v = dwell_world::normal_to_m([n[0], n[1], n[2]].map(f64::from));
            n.copy_from_slice(&v.map(|c| c as f32));
        }
    }
    m
}

/// Meshes a section's cells (34³, `cell_index` order); positions in cells, 0..32, with y scaled
/// by the cell height (so x 0..32, y 0..32 × the cell height). Liquids are see-through surfaces
/// (the `water` mesh) over the floor, as the chunks draw them.
pub fn mesh_section(
    cells: &[u16],
    options: &MeshSectionOptions<'_>,
) -> Result<SectionMeshes, LengthError> {
    let m = if options.mirror {
        let m = mesh_section_above(&mirror_rows(cells), options)?;
        let [s0, s1, s2, s3, s4, s5] = m.skirts;
        let skirts = [s0, s1, s3, s2, s4, s5].map(flip_mesh_y);
        SectionMeshes {
            opaque: flip_mesh_y(m.opaque),
            water: flip_mesh_y(m.water),
            skirts,
        }
    } else {
        mesh_section_above(cells, options)?
    };
    Ok(SectionMeshes {
        opaque: mesh_to_metres(m.opaque),
        water: mesh_to_metres(m.water),
        skirts: m.skirts.map(mesh_to_metres),
    })
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
    // Merge key per slice cell: 0 none, else face_key (material, a riser's top material and share,
    // skirt).
    let mut mask = vec![0u64; NU * NU];

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
                    // A side takes the colour of the fine risers it stands for (riser_color).
                    let (top, share) = if axis != 1 && !liquid {
                        (special.top[wc], special.slope[wc])
                    } else {
                        (m, 0)
                    };
                    // A top skirt stands in for the ground above the section (the column's
                    // surface, in the section above): it takes the ground's colour, not the top
                    // row's (dirt a cell or more down), should it ever show.
                    let m = if hidden && face == 2 && !liquid && special.ground_m[wc] != 0 {
                        special.ground_m[wc]
                    } else {
                        m
                    };
                    mask[i as usize + NU * j as usize] = face_key(m, top, share, hidden);
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
                    let (m, top, share, skirt) = parse_key(key);
                    let water = !skirt && is_liquid(m);
                    let target = if skirt {
                        &mut skirts[face]
                    } else if water {
                        &mut water_mesh
                    } else {
                        &mut opaque
                    };
                    // Water's surface where the chunks draw it (water_drop below the cell's top);
                    // a top skirt sunk under the ground above the plane (SKIRT_SINK).
                    let drop = if axis == 1 && sign > 0 && is_liquid(m) {
                        water_drop
                    } else if skirt && face == 2 {
                        SKIRT_SINK
                    } else {
                        0.0
                    };
                    let color = if group == 1 && !is_liquid(m) {
                        riser_color(m, top, share, axis, sign)
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
    /// The column's ground height in cells from the section's bottom (its surface, or its topmost
    /// filled cell's top; a wet column's water surface), NaN for none in the section.
    g: Vec<f32>,
    /// The material on top of the column's ground (0: none): what its risers' top band shows.
    top: Vec<u16>,
    /// The column's surface height exactly (the generator's, in cells from the section's bottom),
    /// NaN where the surface data has none.
    exact: Vec<f32>,
    /// The ground's gradient at the column as a `gradient_step`: what its risers stand for.
    slope: Vec<u8>,
    /// The column's surface material from the surface data (0: none, or under water): what a top
    /// skirt under ground above the section shows.
    ground_m: Vec<u16>,
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
        g: vec![f32::NAN; n],
        top: vec![0; n],
        exact: vec![f32::NAN; n],
        slope: vec![0; n],
        ground_m: vec![0; n],
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
                out.exact[c] = h as f32;
                let material = to_uint16(at(c, 1));
                if flags & SURFACE_WET == 0 && !is_liquid(material) {
                    out.ground_m[c] = material;
                }
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
    column_grounds(cells, &mut out);
    out
}

/// Each column's ground height and top material (`Surfaces::g`, `top`), which colour its risers.
fn column_grounds(cells: &[u16], out: &mut Surfaces) {
    let top = N;
    for z in -1..=top {
        for x in -1..=top {
            let c = col(x, z);
            let y = topmost_filled(cells, x, z);
            if y < -1 {
                continue; // none
            }
            let m = cell(cells, x, y, z);
            (out.g[c], out.top[c]) = if is_liquid(m) {
                // Under water: the water's surface, which a bank's riser stands above.
                let level = if i32::from(out.wy[c]) == y {
                    out.w[c]
                } else {
                    (y + 1) as f32
                };
                (level, m)
            } else if !out.exact[c].is_nan() {
                (
                    out.exact[c],
                    if out.y[c] != NO_SURFACE { out.m[c] } else { m },
                )
            } else if out.y[c] != NO_SURFACE {
                (out.h[c], out.m[c])
            } else {
                ((y + 1) as f32, m)
            };
        }
    }
    // Gradients: central differences, one-sided at the apron or beside a column with no ground.
    // Heights are in cells, 2^L × CELL_H m tall and 2^L m apart: cells per cell × CELL_H is m per m.
    let height = |x: i32, z: i32| {
        if (-1..=top).contains(&x) && (-1..=top).contains(&z) {
            f64::from(out.g[col(x, z)])
        } else {
            f64::NAN
        }
    };
    for z in -1..=top {
        for x in -1..=top {
            let h = height(x, z);
            if h.is_nan() {
                continue;
            }
            let along = |(dx, dz): (i32, i32)| {
                let (a, b) = (height(x + dx, z + dz), height(x - dx, z - dz));
                match (a.is_nan(), b.is_nan()) {
                    (false, false) => (a - b) / 2.0,
                    (false, true) => a - h,
                    (true, false) => h - b,
                    (true, true) => 0.0,
                }
            };
            let gradient = along((1, 0)).hypot(along((0, 1))) * dwell_world::CELL_H;
            out.slope[col(x, z)] = gradient_step(gradient);
        }
    }
}

/// A greedy-pass merge key: material, a riser's top material and share, and the skirt bit.
fn face_key(m: StateId, top: StateId, step: u8, skirt: bool) -> u64 {
    (u64::from(m) + 1) | u64::from(top) << 17 | u64::from(step) << 34 | u64::from(skirt) << 42
}

fn parse_key(key: u64) -> (StateId, StateId, u8, bool) {
    (
        ((key & 0x1_ffff) - 1) as StateId,
        ((key >> 17) & 0x1_ffff) as StateId,
        ((key >> 34) & 0xff) as u8,
        (key >> 42) & 1 == 1,
    )
}

/// Heights (in cells) of a column's top along a side at the side's ends and middle: LOD tops are
/// level (no block is shaped by its neighbours, ADR 0042).
fn profile(s: &Surfaces, c: usize) -> [f64; 3] {
    [f64::from(s.h[c]); 3]
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
    /// The top material of the column whose rise this is, and its share (riser_color).
    top: StateId,
    share: u8,
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
            let at = z as usize + NU * x as usize;
            top_key[at] = f64::from(s.m[c]) + 1.0;
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
            let material = (key - 1.0) as StateId;
            opaque.quad(
                1,
                1,
                h,
                (z as f64, (z + w) as f64),
                (x as f64, (x + d) as f64),
                lod_color(material, 0),
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
                                && w.top == r.top
                                && w.share == r.share
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
                                riser_color(r.material, r.top, r.share, axis, r.sign),
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
    let ours = profile(s, c);
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
    let (top, share) = (s.top[c], s.slope[c]);
    if !n_filled {
        // Open.
        return (slot == 0).then_some(Wall {
            target: 0,
            sign,
            lo: floor,
            hi: ours,
            material,
            top,
            share,
        });
    }
    if n_surface_here {
        // Both surfaces in this row: the higher one's side shows down to the lower (and below
        // that, on the border, a skirt). The neighbour's top along the same edge is its opposite
        // side.
        let theirs = profile(s, nc);
        let lo = [0, 1, 2].map(|i| theirs[i].min(ours[i]));
        if slot == 0 {
            return (0..3).any(|i| lo[i] < ours[i]).then_some(Wall {
                target: 0,
                sign,
                lo,
                hi: ours,
                material,
                top,
                share,
            });
        }
        return (border && lo.iter().any(|&v| v > yf)).then_some(Wall {
            target: skirt,
            sign,
            lo: floor,
            hi: lo,
            material,
            top,
            share,
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
            top: s.top[nc],
            share: s.slope[nc],
        });
    }
    border.then_some(Wall {
        target: skirt,
        sign,
        lo: floor,
        hi: ours,
        material,
        top,
        share,
    })
}

#[cfg(test)]
#[path = "lod_mesher_tests.rs"]
mod tests;
