//! Greedy chunk mesher (`client/src/mesh/mesher.ts`; ARCHITECTURE.md §5, §6.1): a chunk's voxels,
//! with a one-voxel apron from its neighbours, into render geometry. Visible faces of full cubes
//! and water merge into rectangles of one material per slice; shaped blocks (drawn with their true
//! normals) and ladders are emitted polygon by polygon. Textures repeat once per block across a
//! merged quad: `uvs` carry positions in blocks and `tiles` the atlas rectangle.
//!
//! Arithmetic is done in `f64` and stored as `f32`, as the TS computes in doubles and stores into
//! `Float32Array`s, so the buffers match it value for value.

use std::sync::OnceLock;

use dwell_blocks::{FLUIDS, FluidInfo, STATES, ShapeInfo, StateId, StateInfo, shape::faces_of};
use dwell_protocol::CHUNK_SIZE;

use crate::LengthError;
use crate::style::{
    Look, MaterialStyle, cube, fluid_value, material_style, mirror_material, style_of,
};
use crate::textures::{TileRect, tile_rect};
use dwell_look::{face_tint, normal_tint};

const N: i32 = CHUNK_SIZE;

/// Edge of the padded voxel block: the chunk plus one voxel on each side.
pub const PAD: usize = CHUNK_SIZE as usize + 2;
pub const PADDED_VOLUME: usize = PAD * PAD * PAD;

/// Index into padded voxels of chunk-local (x, y, z), each in −1..CHUNK_SIZE.
pub const fn padded_index(x: i32, y: i32, z: i32) -> usize {
    let pad = PAD as i32;
    (x + 1 + pad * (z + 1 + pad * (y + 1))) as usize
}

/// One pass's vertex streams (see the crate docs for the layout).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshArrays {
    /// x, y, z per vertex.
    pub positions: Vec<f32>,
    /// Unit normal per vertex.
    pub normals: Vec<f32>,
    /// Face tint × (untextured) material colour per vertex; multiplies the texture.
    pub colors: Vec<f32>,
    /// Texture coordinates in blocks per vertex: the shader repeats the tile once per unit.
    pub uvs: Vec<f32>,
    /// Atlas rectangle per vertex: u0, v0, width, height.
    pub tiles: Vec<f32>,
    /// The transparent mesh's fluid id per vertex (`crate::NO_FLUID` for a see-through face that
    /// is no fluid's); empty in the opaque mesh.
    pub fluids: Vec<f32>,
    /// Triangles, counter-clockwise seen from outside.
    pub indices: Vec<u32>,
}

impl MeshArrays {
    /// Bytes of all streams (what a transfer or a GPU upload moves).
    pub fn byte_len(&self) -> usize {
        4 * (self.positions.len()
            + self.normals.len()
            + self.colors.len()
            + self.uvs.len()
            + self.tiles.len()
            + self.fluids.len()
            + self.indices.len())
    }
}

/// A chunk's meshes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChunkMeshes {
    pub opaque: MeshArrays,
    /// Water and other see-through materials, drawn after the opaque pass.
    pub transparent: MeshArrays,
}

/// Face index → (axis, sign): 0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z.
const FACES: [(usize, i32); 6] = [(0, 1), (0, -1), (1, 1), (1, -1), (2, 1), (2, -1)];

/// The plate sits this far in front of the cell's back face.
const LADDER_INSET: f64 = 0.05;
/// Water in a flooded cell sits this far inside its faces.
const FLOOD_INSET: f64 = 0.002;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Empty,
    Full,
    /// Its polygons come from the shape table.
    Shaped,
}

#[derive(Clone, Copy, Debug)]
struct Kind {
    shape: Shape,
    /// The solid's geometry (Full: the cube; Shaped: its shape; otherwise none).
    def: Option<&'static ShapeInfo>,
    liquid: bool,
    /// Shaped blocks with water in their open part.
    flooded: bool,
    /// Ladders: the one face drawn.
    ladder_face: Option<usize>,
    style: MaterialStyle,
}

fn make_kind(id: StateId, style: MaterialStyle) -> Kind {
    let empty = id == 0 || style.look == Look::Fluid || style.look == Look::Ladder;
    let shaped = !empty && style.look == Look::Shaped;
    Kind {
        shape: if empty {
            Shape::Empty
        } else if shaped {
            Shape::Shaped
        } else {
            Shape::Full
        },
        def: if empty {
            None
        } else if shaped {
            dwell_blocks::SHAPES.get(style.shape)
        } else {
            Some(cube())
        },
        liquid: style.look == Look::Fluid,
        flooded: shaped && style.flooded,
        ladder_face: if style.look == Look::Ladder {
            Some(style.ladder_face.unwrap_or(4))
        } else {
            None
        },
        style,
    }
}

/// The mesher's view of a registry: every state's kind, and the state whose look fills flooded
/// cells. The build's ([`Kinds::build`]), or a test registry's (`dwell_blocks::fixture`).
pub(crate) struct Kinds {
    table: Vec<Kind>,
    flood: StateId,
}

impl Kinds {
    pub(crate) fn new(states: &[StateInfo], fluids: &[FluidInfo], flood: Option<StateId>) -> Self {
        let table = states
            .iter()
            .enumerate()
            .map(|(i, s)| make_kind(i as StateId, style_of(s, fluids)))
            .collect();
        Self {
            table,
            flood: flood.unwrap_or(StateId::MAX),
        }
    }

    /// The build's registry; flooded cells are drawn as `dwell:water`.
    fn build() -> &'static Self {
        static KINDS: OnceLock<Kinds> = OnceLock::new();
        KINDS.get_or_init(|| Self::new(&STATES, &FLUIDS, dwell_blocks::state("dwell:water")))
    }

    fn of(&self, id: StateId) -> Kind {
        match self.table.get(usize::from(id)) {
            Some(k) => *k,
            None => make_kind(id, material_style(id)),
        }
    }

    /// How flooded cells' water is drawn.
    fn flood(&self) -> Kind {
        self.of(self.flood)
    }
}

/// The drawn top of a fluid cell (chunk-local, cells): `surface_drop` below the cell's top.
fn fluid_top(y: i32, style: &MaterialStyle) -> f64 {
    f64::from(y) + (1.0 - style.surface_drop)
}

/// Whether the part of `shape` on its cell face `face` is completely covered by the opposite face
/// of `neighbour` (mirrors FaceCovered in block_shape.cpp). Sloped surfaces are never covered.
pub fn face_covered(shape: &ShapeInfo, face: usize, neighbour: Option<&ShapeInfo>) -> bool {
    let Some(neighbour) = neighbour else {
        return false;
    };
    if face == 2 {
        return !shape.full_top || neighbour.full_bottom;
    }
    if face == 3 {
        return !shape.full_bottom || neighbour.full_top;
    }
    // Side profiles are indexed +X −X +Z −Z; the neighbour's face is the opposite one.
    let side = |f: usize| if f < 2 { f } else { f - 2 };
    let mine = shape.sides[side(face)];
    let theirs = neighbour.sides[side(face ^ 1)];
    if mine[0] == 0 && mine[1] == 0 {
        return true;
    }
    if shape.inverted == neighbour.inverted {
        return theirs[0] >= mine[0] && theirs[1] >= mine[1];
    }
    theirs[0] == 2 && theirs[1] == 2
}

/// Is the full-cube face `face` of a cube or water voxel `m` hidden by neighbour `n`?
fn hidden(kinds: &Kinds, m: StateId, n: StateId, face: usize) -> bool {
    let km = kinds.of(m);
    let kn = kinds.of(n);
    if km.liquid && (n == m || kn.flooded) {
        return true;
    }
    face_covered(cube(), face, kn.def)
}

fn channel(color: u32, shift: u32) -> f64 {
    f64::from((color >> shift) & 0xff) / 255.0
}

#[derive(Default)]
struct Builder {
    m: MeshArrays,
}

impl Builder {
    fn vertex_count(&self) -> u32 {
        (self.m.positions.len() / 3) as u32
    }

    /// Tags the vertices added since the last tag with `fluid` (the transparent mesh's `fluids`).
    fn tag_fluid(&mut self, fluid: Option<dwell_blocks::FluidId>) {
        let n = self.vertex_count() as usize;
        self.m.fluids.resize(n, fluid_value(fluid));
    }

    fn push_tile(&mut self, tile: &TileRect) {
        self.m.tiles.extend([
            tile.u0 as f32,
            tile.v0 as f32,
            (tile.u1 - tile.u0) as f32,
            (tile.v1 - tile.v0) as f32,
        ]);
    }

    /// Quad on `axis = plane`, spanning [u0,u1] × [v0,v1] (u = axis+1, v = axis+2), facing `sign`.
    #[allow(clippy::too_many_arguments)]
    fn quad(
        &mut self,
        axis: usize,
        sign: i32,
        plane: f64,
        (u0, u1): (f64, f64),
        (v0, v1): (f64, f64),
        color: u32,
        tile: &TileRect,
    ) {
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        let base = self.vertex_count();
        let tint = face_tint(axis, sign);
        let r = channel(color, 16) * tint[0];
        let g = channel(color, 8) * tint[1];
        let b = channel(color, 0) * tint[2];
        let s = f64::from(sign);
        for (cu, cv) in [(u0, v0), (u1, v0), (u1, v1), (u0, v1)] {
            let mut p = [0.0; 3];
            p[axis] = plane;
            p[u] = cu;
            p[v] = cv;
            let [px, py, pz] = p;
            self.m.positions.extend([px as f32, py as f32, pz as f32]);
            self.m.normals.extend([
                if axis == 0 { s } else { 0.0 } as f32,
                if axis == 1 { s } else { 0.0 } as f32,
                if axis == 2 { s } else { 0.0 } as f32,
            ]);
            self.m.colors.extend([r as f32, g as f32, b as f32]);
            // In blocks: s across, t up the face (world y on sides), tops and bottoms in x/z.
            let (su, sv) = match axis {
                1 => (px, pz),
                0 => (pz, py),
                _ => (px, py),
            };
            self.m.uvs.extend([su as f32, sv as f32]);
            self.push_tile(tile);
        }
        // e_u × e_v = e_axis: (0, 1, 2) is counter-clockwise seen from +axis.
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

    /// A convex polygon (counter-clockwise seen from outside) of `pts`, offset by `at`. `normal` is
    /// its unit normal; uvs project the face along its dominant axis, in blocks. With `side_top` (a
    /// vertical side face), v runs down from the face's top edge rather than the cell's top.
    #[allow(clippy::too_many_arguments)]
    fn polygon(
        &mut self,
        pts: &[[f64; 3]],
        [x, y, z]: [f64; 3],
        normal: [f64; 3],
        tint: [f64; 3],
        color: u32,
        tile: &TileRect,
        side_top: Option<SideTop>,
    ) {
        let base = self.vertex_count();
        let r = channel(color, 16) * tint[0];
        let g = channel(color, 8) * tint[1];
        let b = channel(color, 0) * tint[2];
        let [nx, ny, nz] = normal;
        for pt in pts {
            let [qx, qy, qz] = *pt;
            let px = qx + x;
            let py = qy + y;
            let pz = qz + z;
            // Texture height: from the face's top edge down (a side), else the world height.
            let tv = match side_top {
                Some(st) => y + qy - st.at(pt) + 1.0,
                None => py,
            };
            self.m.positions.extend([px as f32, py as f32, pz as f32]);
            self.m.normals.extend([nx as f32, ny as f32, nz as f32]);
            self.m.colors.extend([r as f32, g as f32, b as f32]);
            let (su, sv) = if ny.abs() >= nx.abs() && ny.abs() >= nz.abs() {
                (px, pz)
            } else if nx.abs() >= nz.abs() {
                (pz, tv)
            } else {
                (px, tv)
            };
            self.m.uvs.extend([su as f32, sv as f32]);
            self.push_tile(tile);
        }
        for i in 1..pts.len().saturating_sub(1) as u32 {
            self.m.indices.extend([base, base + i, base + i + 1]);
        }
    }
}

/// Vertex colour and texture tile of a face: textured faces take their colour from the texture.
fn surface(style: &MaterialStyle, axis: usize, sign: i32) -> (u32, TileRect) {
    match style.textures {
        None => (style.color, tile_rect("plain")),
        Some(t) => {
            let name = if axis != 1 {
                t.side
            } else if sign > 0 {
                t.top
            } else {
                t.bottom
            };
            (0xffffff, tile_rect(name))
        }
    }
}

/// A ladder's plate: one quad against the back of its cell.
fn ladder_face(b: &mut Builder, style: &MaterialStyle, face: usize, x: i32, y: i32, z: i32) {
    let (axis, sign) = FACES[face];
    let lo = [f64::from(x), f64::from(y), f64::from(z)];
    let hi = [lo[0] + 1.0, lo[1] + 1.0, lo[2] + 1.0];
    let u = (axis + 1) % 3;
    let v = (axis + 2) % 3;
    // A thin plate against the cell's back face, facing out of the facing side.
    let plane = if sign > 0 {
        lo[axis] + LADDER_INSET
    } else {
        hi[axis] - LADDER_INSET
    };
    let (color, tile) = surface(style, axis, sign);
    b.quad(
        axis,
        sign,
        plane,
        (lo[u], hi[u]),
        (lo[v], hi[v]),
        color,
        &tile,
    );
}

/// The unit normal of a polygon (first three points, counter-clockwise).
fn polygon_normal(pts: &[[f64; 3]]) -> [f64; 3] {
    let o = [0.0; 3];
    let a = pts.first().copied().unwrap_or(o);
    let b = pts.get(1).copied().unwrap_or(o);
    let c = pts.get(2).copied().unwrap_or(o);
    let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let nx = e1[1] * e2[2] - e1[2] * e2[1];
    let ny = e1[2] * e2[0] - e1[0] * e2[2];
    let nz = e1[0] * e2[1] - e1[1] * e2[0];
    let len = (nx * nx + ny * ny + nz * nz).sqrt();
    let len = if len == 0.0 || len.is_nan() { 1.0 } else { len };
    [nx / len, ny / len, nz / len]
}

/// Side profile index (+X −X +Z −Z) of each cell face tag (+X −X +Y −Y +Z −Z); −1: not a side.
const SIDE_OF_TAG: [i32; 6] = [0, 1, -1, -1, 2, 3];

/// The height of an upright piece's top edge along one of its side faces: the side profile runs
/// linearly between its heights (halves) at the low and high running coordinate (z on ±X faces, x
/// on ±Z).
#[derive(Clone, Copy, Debug)]
struct SideTop {
    h0: f64,
    h1: f64,
    run: usize,
}

impl SideTop {
    fn at(self, pt: &[f64; 3]) -> f64 {
        (self.h0 + (self.h1 - self.h0) * pt[self.run]) / 2.0
    }
}

/// None for faces that are not sides and for hanging pieces, whose sides meet the cell's top.
fn side_top_of(def: &ShapeInfo, tag: usize) -> Option<SideTop> {
    let side = SIDE_OF_TAG.get(tag).copied().unwrap_or(-1);
    if side < 0 || def.inverted {
        return None;
    }
    let [h0, h1] = def.sides[side as usize];
    Some(SideTop {
        h0: f64::from(h0),
        h1: f64::from(h1),
        run: if side < 2 { 2 } else { 0 },
    })
}

fn neighbour_of(x: i32, y: i32, z: i32, axis: usize, sign: i32) -> [i32; 3] {
    let mut n = [x, y, z];
    n[axis] += sign;
    n
}

/// A shaped voxel: each polygon of its shape, but the parts of cell faces a neighbour covers.
#[allow(clippy::too_many_arguments)]
fn shaped_voxel(b: &mut Builder, kinds: &Kinds, k: &Kind, padded: &[u16], x: i32, y: i32, z: i32) {
    let Some(def) = k.def else {
        return;
    };
    for f in faces_of(def) {
        let tag = usize::from(f.tag);
        if tag < 6 {
            let (axis, sign) = FACES[tag];
            let [nx, ny, nz] = neighbour_of(x, y, z, axis, sign);
            if face_covered(def, tag, kinds.of(padded[padded_index(nx, ny, nz)]).def) {
                continue;
            }
        }
        let pts: Vec<[f64; 3]> = f.v[..usize::from(f.count)]
            .iter()
            .map(|p| p.map(f64::from))
            .collect();
        let normal = polygon_normal(&pts);
        let face_sign = if tag % 2 == 0 { 1 } else { -1 };
        let tint = if tag < 6 {
            face_tint(tag / 2, face_sign)
        } else {
            normal_tint(normal[0], normal[1], normal[2])
        };
        // Sides take the side tile, a sloped surface the top (or, hanging, the bottom) tile.
        let axis = if tag < 6 { tag / 2 } else { 1 };
        let sign = if tag < 6 {
            face_sign
        } else if normal[1] >= 0.0 {
            1
        } else {
            -1
        };
        let (color, tile) = surface(&k.style, axis, sign);
        let at = [f64::from(x), f64::from(y), f64::from(z)];
        b.polygon(&pts, at, normal, tint, color, &tile, side_top_of(def, tag));
    }
}

/// Water in the open part of a flooded shaped cell: its faces, inset so the solid shows through.
fn flooded_water(b: &mut Builder, kinds: &Kinds, padded: &[u16], x: i32, y: i32, z: i32) {
    let water = kinds.flood();
    for (face, &(axis, sign)) in FACES.iter().enumerate() {
        let [nx, ny, nz] = neighbour_of(x, y, z, axis, sign);
        let kn = kinds.of(padded[padded_index(nx, ny, nz)]);
        if kn.liquid || kn.flooded || face_covered(cube(), face, kn.def) {
            continue;
        }
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        let lo = [f64::from(x), f64::from(y), f64::from(z)];
        let mut plane = lo[axis]
            + if sign > 0 {
                1.0 - FLOOD_INSET
            } else {
                FLOOD_INSET
            };
        if face == 2 {
            plane = fluid_top(y, &water.style);
        }
        let (color, tile) = surface(&water.style, axis, sign);
        b.quad(
            axis,
            sign,
            plane,
            (lo[u], lo[u] + 1.0),
            (lo[v], lo[v] + 1.0),
            color,
            &tile,
        );
        b.tag_fluid(water.style.fluid);
    }
}

/// The padded voxels of a chunk turned upside down: rows reversed, shaped blocks turned over.
/// Entries past `PADDED_VOLUME` are ignored; missing ones read as air.
pub fn mirror_padded(padded: &[u16]) -> Vec<u16> {
    let mut out = vec![0u16; PADDED_VOLUME];
    let layer = PAD * PAD;
    for y in 0..PAD {
        let from = y * layer;
        let to = (PAD - 1 - y) * layer;
        for i in 0..layer {
            out[to + i] = mirror_material(padded.get(from + i).copied().unwrap_or(0));
        }
    }
    out
}

/// A mesh in chunk-local units mirrored in y (about the chunk's middle): re-wound.
fn flip_arrays_y(mut m: MeshArrays) -> MeshArrays {
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

/// Meshes a chunk from its padded voxels (`PADDED_VOLUME` state ids, [`padded_index`] order).
/// Positions are chunk-local. A chunk of face B (`mirror`; BIFACIAL_WORLD.md §6) is meshed as its
/// mirror image and the geometry turned back: its faces toward the player's up take the top
/// texture and shading, and its water surface lies 1/8 above the bottom of its cell.
pub fn mesh_chunk(padded: &[u16], mirror: bool) -> Result<ChunkMeshes, LengthError> {
    mesh_chunk_with(Kinds::build(), padded, mirror)
}

/// [`mesh_chunk`] over the registry `kinds` describes.
pub(crate) fn mesh_chunk_with(
    kinds: &Kinds,
    padded: &[u16],
    mirror: bool,
) -> Result<ChunkMeshes, LengthError> {
    if padded.len() != PADDED_VOLUME {
        return Err(LengthError {
            expected: PADDED_VOLUME,
            got: padded.len(),
        });
    }
    if mirror {
        let m = mesh_chunk_with(kinds, &mirror_padded(padded), false)?;
        return Ok(ChunkMeshes {
            opaque: flip_arrays_y(m.opaque),
            transparent: flip_arrays_y(m.transparent),
        });
    }
    let mut opaque = Builder::default();
    let mut transparent = Builder::default();
    let n = N as usize;
    // Merge mask for one slice: material + 1 of a visible mergeable face, 0 for none.
    let mut mask = vec![0i32; n * n];

    for (face, &(axis, sign)) in FACES.iter().enumerate() {
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        for d in 0..N {
            mask.fill(0);
            let mut any = false;
            for j in 0..N {
                for i in 0..N {
                    let mut cell = [0i32; 3];
                    cell[axis] = d;
                    cell[u] = i;
                    cell[v] = j;
                    let [cx, cy, cz] = cell;
                    let m = padded[padded_index(cx, cy, cz)];
                    if m == 0 {
                        continue;
                    }
                    let k = kinds.of(m);
                    if let Some(lf) = k.ladder_face {
                        if lf == face {
                            ladder_face(&mut opaque, &k.style, face, cx, cy, cz);
                        }
                        continue;
                    }
                    if k.shape == Shape::Shaped {
                        continue; // drawn whole, below
                    }
                    let [nx, ny, nz] = neighbour_of(cx, cy, cz, axis, sign);
                    if hidden(kinds, m, padded[padded_index(nx, ny, nz)], face) {
                        continue;
                    }
                    mask[i as usize + n * j as usize] = i32::from(m) + 1;
                    any = true;
                }
            }
            if !any {
                continue;
            }
            // Greedy rectangles: widest run along u, then as many rows along v as match it.
            for j in 0..n {
                let mut i = 0;
                while i < n {
                    let key = mask[i + n * j];
                    if key == 0 {
                        i += 1;
                        continue;
                    }
                    let mut w = 1;
                    while i + w < n && mask[i + w + n * j] == key {
                        w += 1;
                    }
                    let mut h = 1;
                    'grow: while j + h < n {
                        for k in 0..w {
                            if mask[i + k + n * (j + h)] != key {
                                break 'grow;
                            }
                        }
                        h += 1;
                    }
                    for dv in 0..h {
                        mask[i + n * (j + dv)..i + w + n * (j + dv)].fill(0);
                    }
                    let m = (key - 1) as StateId;
                    let style = kinds.of(m).style;
                    let mut plane = f64::from(if sign > 0 { d + 1 } else { d });
                    if style.look == Look::Fluid && face == 2 {
                        plane = fluid_top(d, &style);
                    }
                    let (color, tile) = surface(&style, axis, sign);
                    let see_through = style.opacity < 1.0;
                    let target = if see_through {
                        &mut transparent
                    } else {
                        &mut opaque
                    };
                    target.quad(
                        axis,
                        sign,
                        plane,
                        (i as f64, (i + w) as f64),
                        (j as f64, (j + h) as f64),
                        color,
                        &tile,
                    );
                    if see_through {
                        target.tag_fluid(style.fluid);
                    }
                    i += w;
                }
            }
        }
    }
    // Shaped blocks: whole voxels, not slices.
    for cz in 0..N {
        for cy in 0..N {
            for cx in 0..N {
                let m = padded[padded_index(cx, cy, cz)];
                if m == 0 {
                    continue;
                }
                let k = kinds.of(m);
                if k.shape != Shape::Shaped {
                    continue;
                }
                shaped_voxel(&mut opaque, kinds, &k, padded, cx, cy, cz);
                if k.flooded {
                    flooded_water(&mut transparent, kinds, padded, cx, cy, cz);
                }
            }
        }
    }
    Ok(ChunkMeshes {
        opaque: opaque.m,
        transparent: transparent.m,
    })
}

#[cfg(test)]
#[path = "mesher_tests.rs"]
mod tests;
