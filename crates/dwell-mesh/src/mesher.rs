//! Greedy chunk mesher (`client/src/mesh/mesher.ts`; ARCHITECTURE.md §5, §6.1): a chunk's voxels,
//! with a one-voxel apron from its neighbours, into render geometry. Visible faces of full cubes
//! and water merge into rectangles of one material per slice; shaped blocks (drawn with their true
//! normals) and ladders are emitted polygon by polygon. Textures repeat once per metre across a
//! merged quad: `uvs` carry world positions (in blocks across, metres up a side, so a face's uv
//! never depends on the quad it merged into) and `tiles` the atlas rectangle.
//!
//! Arithmetic is done in `f64` and stored as `f32`, as the TS computes in doubles and stores into
//! `Float32Array`s, so the buffers match it value for value.

use std::sync::OnceLock;

use dwell_blocks::{
    FLUIDS, FluidInfo, RenderMode, STATES, ShapeInfo, StateId, StateInfo, shape::faces_of,
};
use dwell_protocol::CHUNK_SIZE;
use dwell_world::FaceFrame;

use crate::LengthError;
use crate::flood::Flood;
use crate::sky::{AO_OPEN, LatticeRays, Occupancy, corner_key, key_flood, key_light, vertex_light};
use crate::style::{
    Look, MaterialStyle, cube, fluid_value, material_style, mirror_material, style_of,
};
use crate::textures::{TileRect, tile_rect};
use dwell_blocks::shape::plant_sink;
use dwell_look::{face_tint, normal_tint};
use dwell_world::{CELL_H, SKY_APRON, SKY_EXTENT, SKY_VOLUME, normal_to_m};

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
    /// The cutout mesh only (empty in the others): per vertex, how far wind sways it (m; 0 at a
    /// plant's base and on ladders), the wind's speed, and the distance fade's start and end (m;
    /// end 0: no fade).
    pub plants: Vec<f32>,
    /// The cutout mesh only (empty in the others): per vertex, 1 where the ink outline doesn't
    /// draw (block data `outline: false`, the cross plants' default; docs/ART_DIRECTION.md §7.4),
    /// else 0 (ladders and every other cutout).
    pub bare: Vec<f32>,
    /// The opaque and cutout meshes (empty in the transparent one): per vertex, the baked ambient
    /// light, corner occlusion × sky visibility (0–1; 1 open to the sky; `sky.rs`). The shader
    /// multiplies the hemisphere light by it, above the floor (shared/look/ambient.json).
    pub light: Vec<f32>,
    /// Every mesh (the transparent one for the water shader): per vertex, the flood light
    /// (docs/DAY_NIGHT.md §11), sky then block level as a share of 15, averaged over the cells
    /// the vertex touches (`flood.rs`). The shader turns them into brightness by
    /// shared/look/light.json's curves.
    pub flood: Vec<f32>,
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
            + self.plants.len()
            + self.bare.len()
            + self.light.len()
            + self.flood.len()
            + self.fluids.len()
            + self.indices.len())
    }
}

/// A chunk's meshes, one per render mode (the block data's `render`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChunkMeshes {
    pub opaque: MeshArrays,
    /// Translucent materials (water), blended after the opaque and cutout draws, far to near.
    pub transparent: MeshArrays,
    /// Cutout materials (ladders, plants): alpha-tested, depth-writing, drawn from both sides,
    /// with the `plants` stream.
    pub cutout: MeshArrays,
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
    /// Hides the faces of neighbours it covers (opaque solids; see-through ones never do).
    occludes: bool,
    /// Blocks the ambient light (corner occlusion, sky rays): full cubes that are not see-through
    /// (leaves are; glass and fluids are not). Shaped blocks, plants and ladders let it through.
    blocks_light: bool,
    style: MaterialStyle,
}

fn make_kind(id: StateId, style: MaterialStyle) -> Kind {
    let empty = id == 0
        || style.look == Look::Fluid
        || style.look == Look::Ladder
        || style.look == Look::Cross;
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
        // Shaped blocks, ladders and plants hold water in a flooded cell (a water plant, block
        // data `medium: water`, is drawn inside it).
        flooded: (shaped || style.look == Look::Ladder || style.look == Look::Cross)
            && style.flooded,
        occludes: !empty && style.render == RenderMode::Opaque,
        blocks_light: !empty && !shaped && style.render != RenderMode::Translucent,
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
    if mine == [0; 3] {
        return true;
    }
    if shape.inverted == neighbour.inverted {
        return (0..3).all(|i| theirs[i] >= mine[i]);
    }
    theirs == [4; 3]
}

/// The geometry a neighbour covers faces with: none if it is see-through (cutout, translucent).
fn cover(k: &Kind) -> Option<&'static ShapeInfo> {
    if k.occludes { k.def } else { None }
}

/// The light keys of the corners of the face cell (i, j) (u, v) on plane `plane` of the face
/// looking along `axis` toward `sign`, whose front cell is `front`: each corner's occlusion level
/// from the three cells around it in the front layer, and its sky rays (shared by the faces
/// through the vertex, `rays`).
fn corner_keys(
    cells: &Cells<'_>,
    rays: &mut [u8],
    (face, plane): (usize, i32),
    [i, j]: [i32; 2],
    front: [i32; 3],
) -> CornerKeys {
    let axis = FACES[face].0;
    let u = (axis + 1) % 3;
    let v = (axis + 2) % 3;
    let stride = N as usize + 1;
    [(0, 0), (1, 0), (1, 1), (0, 1)].map(|(cu, cv)| {
        let level = cells.occ.corner_ao(front, u, 2 * cu - 1, v, 2 * cv - 1);
        let (vu, vv) = (i + cu, j + cv);
        let slot = vu as usize + stride * vv as usize;
        if rays[slot] == u8::MAX {
            let mut p = [0i32; 3];
            p[axis] = plane;
            p[u] = vu;
            p[v] = vv;
            rays[slot] = cells.lattice_rays(p, face);
        }
        let (sky, block) = cells
            .flood
            .corner(cells.occ, front, u, 2 * cu - 1, v, 2 * cv - 1);
        corner_key(level, rays[slot], sky, block)
    })
}

/// The light keys of a see-through face's corners (fluids, glass): the flood light only, as
/// terrain's corners average it (`Flood::corner`); no corner occlusion or sky rays (the water
/// shader takes no baked light). Faces merge only where these match, so a merged quad's corners
/// carry their cells' flood light.
fn flood_keys(cells: &Cells<'_>, face: usize, front: [i32; 3]) -> CornerKeys {
    let axis = FACES[face].0;
    let u = (axis + 1) % 3;
    let v = (axis + 2) % 3;
    let open = dwell_look::ambient::AMBIENT.rays.min(63) as u8;
    [(0, 0), (1, 0), (1, 1), (0, 1)].map(|(cu, cv)| {
        let (sky, block) = cells
            .flood
            .corner(cells.occ, front, u, 2 * cu - 1, v, 2 * cv - 1);
        corner_key(AO_OPEN, open, sky, block)
    })
}

/// Is the full-cube face `face` of a cube or water voxel `m` (kind `km`) hidden by neighbour `n`
/// (kind `kn`)?
fn hidden(km: &Kind, kn: &Kind, m: StateId, n: StateId, face: usize) -> bool {
    if km.liquid && (n == m || kn.flooded) {
        return true;
    }
    // Between two cells of one see-through block (panes of glass, leaves) no face is drawn.
    if n == m && !km.occludes {
        return true;
    }
    face_covered(cube(), face, cover(kn))
}

/// The padded block's cells and their kinds. The padded block faces up (a face-B chunk is meshed
/// mirrored). Every cell takes its state's own shape: no block is shaped by its neighbours (ADR 0042).
struct Cells<'a> {
    kinds: &'a Kinds,
    padded: &'a [u16],
    /// Which cells block the ambient light, over the chunk and its (sky) apron, face-local.
    occ: &'a Occupancy,
    /// The lattice vertices' rays for `occ`'s layout.
    paths: &'static LatticeRays,
    /// The rays cast from shaped faces' vertices, by point (eighths of a cell) and normal.
    shaped_rays: std::cell::RefCell<std::collections::HashMap<ShapedKey, u8>>,
    /// The chunk's minimum corner, face-local world cells (the sky rays' turn).
    origin: [i32; 3],
    /// The flood light around the chunk, face-local.
    flood: &'a Flood,
}

impl<'a> Cells<'a> {
    fn new(
        kinds: &'a Kinds,
        padded: &'a [u16],
        occ: &'a Occupancy,
        flood: &'a Flood,
        origin: [i32; 3],
    ) -> Self {
        Self {
            kinds,
            padded,
            occ,
            flood,
            paths: occ.lattice_paths(),
            shaped_rays: Default::default(),
            origin,
        }
    }

    /// [`Self::sky_rays`] for the lattice vertex `at` of the axis-aligned face `face`.
    fn lattice_rays(&self, at: [i32; 3], face: usize) -> u8 {
        let w: [i64; 3] =
            std::array::from_fn(|a| (i64::from(self.origin[a]) + i64::from(at[a])) * 8);
        self.occ
            .lattice_rays(self.paths, at, face, [w[0], w[1], w[2], face as i64])
    }

    /// How many sky rays escape from chunk-local point `cell + frac` (cells) on a surface with
    /// unit normal `n` (metres); `salt` tells apart surfaces through one point.
    /// Shaped faces share their vertices with their neighbours': each point and normal is cast
    /// once (`shaped_rays`).
    fn sky_rays(&self, cell: [i32; 3], frac: [f64; 3], n: [f64; 3], salt: i64) -> u8 {
        let world: [i64; 3] = std::array::from_fn(|a| {
            (i64::from(self.origin[a]) + i64::from(cell[a])) * 8 + (frac[a] * 8.0).round() as i64
        });
        let key = (world, n.map(|c| (c * 1e6).round() as i64));
        if let Some(&r) = self.shaped_rays.borrow().get(&key) {
            return r;
        }
        let r = self
            .occ
            .visible_rays(cell, frac, n, [world[0], world[1], world[2], salt]);
        self.shaped_rays.borrow_mut().insert(key, r);
        r
    }

    fn state(&self, x: i32, y: i32, z: i32) -> StateId {
        self.padded[padded_index(x, y, z)]
    }

    /// The solid a plant in the chunk's cell (x, y, z) stands on: the cell below's shape, or none
    /// when it has no solid.
    fn support(&self, x: i32, y: i32, z: i32) -> Option<&'static ShapeInfo> {
        let k = self.kind(x, y - 1, z);
        if k.shape == Shape::Empty { None } else { k.def }
    }

    fn kind(&self, x: i32, y: i32, z: i32) -> Kind {
        self.kinds.of(self.padded[padded_index(x, y, z)])
    }
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

    /// Quad on `axis = plane`, spanning [u0,u1] × [v0,v1] (u = axis+1, v = axis+2), facing `sign`,
    /// with the baked ambient `light` at its corners (u0v0, u1v0, u1v1, u0v1). It is split along
    /// the diagonal whose corners are the brighter pair, so a lone dark (or bright) corner shades
    /// the same whichever way the quad faces (the classic anisotropy fix).
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
        light: [f32; 4],
        flood: [[f32; 2]; 4],
    ) {
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        let base = self.vertex_count();
        let tint = face_tint(axis, sign);
        let r = channel(color, 16) * tint[0];
        let g = channel(color, 8) * tint[1];
        let b = channel(color, 0) * tint[2];
        let s = f64::from(sign);
        // A side's texture runs in world metres (a 1 m tile spans 1 m, HALF_HEIGHT_GRID.md §6), so
        // it stays put however the faces merge. A hanging tile (a turf skirt) runs down from the
        // quad's top edge instead, on a tile boundary: its quads are one cell tall, so the edge is
        // the block's own top and the fringe hangs from every block.
        let top = if u == 1 { u1.max(u0) } else { v1.max(v0) };
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
            let ty = if tile.hang {
                (py - top) * CELL_H + top
            } else {
                py * CELL_H
            };
            let (su, sv) = match axis {
                1 => (px, pz),
                0 => (pz, ty),
                _ => (px, ty),
            };
            let (su, sv) = if tile.turn { (sv, su) } else { (su, sv) };
            self.m.uvs.extend([su as f32, sv as f32]);
            self.push_tile(tile);
        }
        self.m.light.extend(light);
        self.m.flood.extend(flood.iter().flatten());
        // e_u × e_v = e_axis: (0, 1, 2) is counter-clockwise seen from +axis. The diagonal joins
        // corners 0 and 2, or 1 and 3 when they are the brighter pair.
        let lit: [f32; 4] = std::array::from_fn(|i| light[i] * flood[i][0] + flood[i][1]);
        let k = u32::from(flip_diagonal(lit));
        let c = |i: u32| base + (i + k) % 4;
        if sign > 0 {
            self.m.indices.extend([c(0), c(1), c(2), c(0), c(2), c(3)]);
        } else {
            self.m.indices.extend([c(0), c(2), c(1), c(0), c(3), c(2)]);
        }
    }

    /// A convex polygon (counter-clockwise seen from outside) of `pts`, offset by `at`. `normal` is
    /// its unit normal in cell space, stored in metres (HALF_HEIGHT_GRID.md §2); uvs project the
    /// face along its dominant axis, in blocks. With `side_top` (a vertical side face), v runs down
    /// from the face's top edge rather than the cell's top.
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
        light: Option<(&[f32], &[[f32; 2]])>,
    ) {
        let base = self.vertex_count();
        let r = channel(color, 16) * tint[0];
        let g = channel(color, 8) * tint[1];
        let b = channel(color, 0) * tint[2];
        let [nx, ny, nz] = normal_to_m(normal);
        for pt in pts {
            let [qx, qy, qz] = *pt;
            let px = qx + x;
            let py = qy + y;
            let pz = qz + z;
            // Texture height in metres (a 1 m tile spans 1 m whatever the cell height,
            // HALF_HEIGHT_GRID.md §6): the world height, as on a cube's sides; a hanging tile on
            // a side, from the face's top edge down, the edge on a tile boundary so the fringe
            // hangs from it.
            let tv = match side_top {
                Some(st) if tile.hang => (qy - st.at(pt)) * CELL_H + y + 1.0,
                _ => py * CELL_H,
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
        match light {
            Some((l, f)) => {
                self.m.light.extend(l.iter().copied().take(pts.len()));
                self.m.flood.extend(f.iter().take(pts.len()).flatten());
            }
            None => {
                self.m.light.extend(std::iter::repeat_n(1.0, pts.len()));
                self.m
                    .flood
                    .extend(std::iter::repeat_n([1.0, 0.0], pts.len()).flatten());
            }
        }
        for i in 1..pts.len().saturating_sub(1) as u32 {
            self.m.indices.extend([base, base + i, base + i + 1]);
        }
    }
}

/// Whether a quad with corner lights `l` is split along its 1–3 diagonal: when those corners are
/// the brighter pair.
pub(crate) fn flip_diagonal(l: [f32; 4]) -> bool {
    l[1] + l[3] > l[0] + l[2]
}

/// Vertex colour and texture tile of a face: textured faces take their colour from the texture.
/// The faces along the block's axis (`style.axis`: up for most, a felled log's length) take its top
/// and bottom tiles, the rest its side tile, turned where the face's v doesn't run along the axis
/// (a side's v runs up, a top's along z), so a log's grain follows it.
fn surface(style: &MaterialStyle, axis: usize, sign: i32) -> (u32, TileRect) {
    match style.textures {
        None => (style.color, tile_rect("plain")),
        Some(t) => {
            if axis == style.axis {
                return (0xffffff, tile_rect(if sign > 0 { t.top } else { t.bottom }));
            }
            let v = if axis == 1 { 2 } else { 1 };
            let mut tile = tile_rect(t.side);
            tile.turn = v != style.axis;
            (0xffffff, tile)
        }
    }
}

/// Merge-mask keys: the state + 1 in the low bits, the side rule + 1 (0: none) above them.
const RULE_SHIFT: u32 = 17;
const STATE_MASK: i32 = (1 << RULE_SHIFT) - 1;
/// Merge-mask flag: a fluid's side face in its surface's row, which stops at the drawn surface.
const SURFACE_BIT: i32 = 1 << 30;

/// The side-tile rule (an index into `k.style.side_under`, block data `sideUnder`) that side face
/// `face` of the cell at (x, y, z) takes: the first naming the block in the cell above, unless that
/// block shows a full side on the same face itself (a full grass cube already hangs its own
/// skirt). The padded block faces up, so on face B "above" is toward its sky too.
fn side_rule(cells: &Cells<'_>, k: &Kind, x: i32, y: i32, z: i32, face: usize) -> Option<usize> {
    let side = SIDE_OF_TAG.get(face).copied().unwrap_or(-1);
    if k.style.side_under.is_empty() || side < 0 || y + 1 > N {
        return None;
    }
    let above = cells.kind(x, y + 1, z);
    let rule = k
        .style
        .side_under
        .iter()
        .position(|r| r.above == above.style.block)?;
    let full_side = above
        .def
        .is_some_and(|d| !d.inverted && d.sides[side as usize] == [4; 3]);
    (!full_side).then_some(rule)
}

/// The vertex colour and tile of side face `face` under `rule` (`side_rule`), or as `surface`.
fn side_surface(style: &MaterialStyle, face: usize, rule: Option<usize>) -> (u32, TileRect) {
    let (axis, sign) = FACES[face];
    match rule.and_then(|i| style.side_under.get(i)) {
        Some(r) => (0xffffff, tile_rect(r.side)),
        None => surface(style, axis, sign),
    }
}

/// A ladder's plate: one quad against the back of its cell.
fn ladder_face(
    b: &mut Builder,
    style: &MaterialStyle,
    flood: [f32; 2],
    face: usize,
    [x, y, z]: [i32; 3],
) {
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
        [1.0; 4],
        [flood; 4],
    );
}

/// A cell's jitter value in [0, 1) for `salt`, from its world position: every client and every
/// re-mesh draws the same plant.
fn cell_noise(cell: [i64; 3], salt: u64) -> f64 {
    let mut h = (cell[0] as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (cell[1] as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f)
        ^ (cell[2] as u64).wrapping_mul(0x1656_67b1_9e37_79f9)
        ^ salt.wrapping_mul(0x27d4_eb2f_1656_67c5);
    h ^= h >> 31;
    h = h.wrapping_mul(0x7fb5_d329_728e_a185);
    h ^= h >> 27;
    h = h.wrapping_mul(0x81da_def4_bc2d_d44d);
    h ^= h >> 33;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// Half the width of a cross's quad: corner to corner of the cell at 45° (√2 / 2).
const CROSS_HALF_WIDTH: f64 = std::f64::consts::FRAC_1_SQRT_2;
/// How much a cross's normals lean toward up (1: straight up, 0: the quad's own normal), so a
/// plant is lit like the ground it stands on from every side instead of dark edge-on.
const CROSS_UP: f64 = 0.8;

/// A cross (plants): `quads` vertical quads through the cell's centre, evenly turned, jittered by
/// the cell's world position (`world`), each vertex carrying its wind sway and the distance fade
/// in `plants`. Drawn from both sides by the cutout pipeline: one face per quad.
///
/// A plant that sits on its support (`Cross::sit_on_support`) stands on `support`'s drawn
/// surface: each foot is lowered by the support's [`plant_sink`] under it (a level field's top at
/// half height, a slope's height at that foot), its top by the same amount, so the blades keep
/// their height and the wind still leans only the tops.
fn cross_voxel(
    b: &mut Builder,
    style: &MaterialStyle,
    world: [i64; 3],
    [x, y, z]: [i32; 3],
    support: Option<&ShapeInfo>,
    (light, flood): (f32, [f32; 2]),
) {
    let Some(c) = style.cross else { return };
    let sink = |px: f64, pz: f64| match support {
        Some(s) if c.sit_on_support => f64::from(plant_sink(
            s,
            (px - f64::from(x)) as f32,
            (pz - f64::from(z)) as f32,
            1,
        )),
        _ => 0.0,
    };
    let jitter = |salt| 2.0 * cell_noise(world, salt) - 1.0;
    let cx = f64::from(x) + 0.5 + jitter(1) * f64::from(c.offset);
    let cz = f64::from(z) + 0.5 + jitter(2) * f64::from(c.offset);
    let turn = jitter(3) * f64::from(c.rotation_deg).to_radians();
    let scale =
        f64::from(c.height_min) + f64::from(c.height_max - c.height_min) * cell_noise(world, 4);
    let height = f64::from(c.height) * scale;
    let (y0, y1) = (f64::from(y), f64::from(y) + height);
    let sway = style.wind.map_or(0.0, |w| f64::from(w.strength) * scale);
    let speed = style.wind.map_or(0.0, |w| f64::from(w.speed));
    let (fade0, fade1) = style
        .fade
        .map_or((0.0, 0.0), |f| (f64::from(f.start), f64::from(f.end)));
    let tile = surface(style, 0, 1).1;
    // Ladders before it carry no sway and take the ink: line the plants streams up with the
    // vertices.
    b.m.plants.resize(b.vertex_count() as usize * 4, 0.0);
    b.m.bare.resize(b.vertex_count() as usize, 0.0);
    let bare = if style.outline { 0.0 } else { 1.0 };
    // Texture: across the quad and up it, just inside the tile (fract(1) would wrap to its foot).
    const EDGE: f64 = 1e-3;
    for q in 0..u32::from(c.quads) {
        let angle = std::f64::consts::FRAC_PI_4
            + turn
            + f64::from(q) * std::f64::consts::PI / f64::from(c.quads);
        let (dx, dz) = (angle.cos(), angle.sin());
        // The quad's normal (e_along × e_up), leaned up.
        let (qx, qz) = (-dz, dx);
        let n = {
            let v = [qx * (1.0 - CROSS_UP), CROSS_UP, qz * (1.0 - CROSS_UP)];
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            [v[0] / l, v[1] / l, v[2] / l]
        };
        let tint = normal_tint(n[0], n[1], n[2]);
        let (color, _) = surface(style, 1, 1);
        let base = b.vertex_count();
        let corners = [
            (-CROSS_HALF_WIDTH, y0, EDGE, EDGE, 0.0),
            (CROSS_HALF_WIDTH, y0, 1.0 - EDGE, EDGE, 0.0),
            (CROSS_HALF_WIDTH, y1, 1.0 - EDGE, 1.0 - EDGE, sway),
            (-CROSS_HALF_WIDTH, y1, EDGE, 1.0 - EDGE, sway),
        ];
        for (along, py, u, v, s) in corners {
            let (px, pz) = (cx + dx * along, cz + dz * along);
            let py = py - sink(px, pz);
            b.m.positions.extend([px as f32, py as f32, pz as f32]);
            b.m.normals.extend(n.map(|v| v as f32));
            b.m.colors.extend([
                (channel(color, 16) * tint[0]) as f32,
                (channel(color, 8) * tint[1]) as f32,
                (channel(color, 0) * tint[2]) as f32,
            ]);
            b.m.uvs.extend([u as f32, v as f32]);
            b.push_tile(&tile);
            b.m.plants
                .extend([s as f32, speed as f32, fade0 as f32, fade1 as f32]);
            b.m.bare.push(bare);
            b.m.light.push(light);
            b.m.flood.extend(flood);
        }
        b.m.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
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
/// linearly between its heights (quarters of a cell) at the low end, the middle and the high end
/// of the running coordinate (z on ±X faces, x on ±Z).
#[derive(Clone, Copy, Debug)]
struct SideTop {
    h: [f64; 3],
    run: usize,
}

impl SideTop {
    fn at(self, pt: &[f64; 3]) -> f64 {
        let s = pt[self.run];
        let q = if s <= 0.5 {
            self.h[0] + (self.h[1] - self.h[0]) * s * 2.0
        } else {
            self.h[1] + (self.h[2] - self.h[1]) * (s - 0.5) * 2.0
        };
        q / 4.0
    }
}

/// None for faces that are not sides and for hanging pieces, whose sides meet the cell's top.
fn side_top_of(def: &ShapeInfo, tag: usize) -> Option<SideTop> {
    let side = SIDE_OF_TAG.get(tag).copied().unwrap_or(-1);
    if side < 0 || def.inverted {
        return None;
    }
    Some(SideTop {
        h: def.sides[side as usize].map(f64::from),
        run: if side < 2 { 2 } else { 0 },
    })
}

fn neighbour_of(x: i32, y: i32, z: i32, axis: usize, sign: i32) -> [i32; 3] {
    let mut n = [x, y, z];
    n[axis] += sign;
    n
}

/// A shaped voxel: each polygon of its shape, but the parts of cell faces a neighbour covers.
fn shaped_voxel(b: &mut Builder, cells: &Cells<'_>, k: &Kind, x: i32, y: i32, z: i32) {
    let Some(def) = k.def else {
        return;
    };
    for f in faces_of(def) {
        let tag = usize::from(f.tag);
        if tag < 6 {
            let (axis, sign) = FACES[tag];
            let [nx, ny, nz] = neighbour_of(x, y, z, axis, sign);
            if face_covered(def, tag, cover(&cells.kind(nx, ny, nz))) {
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
            // Shaded as it stands in metres, not as the cell-space slope.
            let m = normal_to_m(normal);
            normal_tint(m[0], m[1], m[2])
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
        let (color, tile) = if tag < 6 && axis != 1 {
            side_surface(&k.style, tag, side_rule(cells, k, x, y, z, tag))
        } else {
            surface(&k.style, axis, sign)
        };
        let at = [f64::from(x), f64::from(y), f64::from(z)];
        // Sky visibility only: a ramp's corners take no corner occlusion.
        let n_m = normal_to_m(normal);
        let salt = 6 + i64::from(f.tag);
        let light: Vec<f32> = pts
            .iter()
            .map(|p| vertex_light(AO_OPEN, cells.sky_rays([x, y, z], *p, n_m, salt)))
            .collect();
        let flood = vec![cells.flood.shaped([x, y, z], normal); pts.len()];
        b.polygon(
            &pts,
            at,
            normal,
            tint,
            color,
            &tile,
            side_top_of(def, tag),
            Some((&light, &flood)),
        );
    }
}

/// The height (cells above its floor) a fluid cell's side faces reach: its drawn surface when its
/// top face shows (nothing of the same fluid, no flooded cell and no solid over it), else the
/// cell's top.
fn fluid_side_top(cells: &Cells<'_>, k: &Kind, m: StateId, x: i32, y: i32, z: i32) -> f64 {
    let ka = cells.kind(x, y + 1, z);
    let a = cells.state(x, y + 1, z);
    if hidden(k, &ka, m, a, 2) || (a == m && ka.liquid) {
        1.0
    } else {
        1.0 - k.style.surface_drop
    }
}

/// Whether a fluid's side face must be drawn cell by cell ([`fluid_side`]) rather than merged:
/// against a shaped neighbour, where only its open part shows the fluid.
fn fluid_side_alone(kn: &Kind) -> bool {
    kn.shape == Shape::Shaped && cover(kn).is_some()
}

/// A fluid's side face `face` (not ±Y) of cell (x, y, z), on its plane `plane`: the part of the
/// side between `bottom` and `top` (cells; the floor is 0) that lies above the solid of the neighbour
/// `kn` across it — so no fluid face stands behind a neighbour's slope or ramp, or above the
/// fluid's drawn surface (WATER_RENDERING.md §9). A hanging neighbour is taken as open.
#[allow(clippy::too_many_arguments)]
fn fluid_side(
    b: &mut Builder,
    style: &MaterialStyle,
    kn: &Kind,
    [x, y, z]: [i32; 3],
    face: usize,
    plane: f64,
    bottom: f64,
    top: f64,
    flood: [f32; 2],
) {
    let (axis, sign) = FACES[face];
    // The neighbour's profile on the face it turns toward this cell (quarters of a cell, along
    // the running coordinate: z on ±X faces, x on ±Z).
    let profile = match cover(kn) {
        Some(def) if !def.inverted => {
            let side = SIDE_OF_TAG[face ^ 1] as usize;
            def.sides[side].map(|q| f64::from(q) / 4.0)
        }
        _ => [0.0; 3],
    };
    let run = if axis == 0 { 2 } else { 0 };
    let (color, tile) = surface(style, axis, sign);
    let mut normal = [0.0; 3];
    normal[axis] = f64::from(sign);
    let tint = face_tint(axis, sign);
    // A level profile is one piece across the side; else each half has its own line.
    let halves: &[(f64, f64, f64, f64)] = if profile[0] == profile[1] && profile[1] == profile[2] {
        &[(0.0, 1.0, profile[0], profile[0])]
    } else {
        &[
            (0.0, 0.5, profile[0], profile[1]),
            (0.5, 1.0, profile[1], profile[2]),
        ]
    };
    for &(ra, rb, ha, hb) in halves {
        // The rectangle [ra, rb] × [0, top] clipped to above the profile's line (Sutherland–
        // Hodgman against one half-plane): a convex polygon in (run, y).
        let above = |r: f64, h: f64| h - (ha + (hb - ha) * (r - ra) / (rb - ra));
        let rect = [(ra, bottom), (rb, bottom), (rb, top), (ra, top)];
        let mut poly: Vec<(f64, f64)> = Vec::with_capacity(5);
        for i in 0..4 {
            let (p, q) = (rect[i], rect[(i + 1) % 4]);
            let (fp, fq) = (above(p.0, p.1), above(q.0, q.1));
            if fp >= 0.0 {
                poly.push(p);
            }
            if (fp >= 0.0) != (fq >= 0.0) {
                let t = fp / (fp - fq);
                poly.push((p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t));
            }
        }
        poly.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9);
        if poly.len() < 3 {
            continue;
        }
        let mut pts: Vec<[f64; 3]> = poly
            .iter()
            .map(|&(r, h)| {
                let mut p = [0.0; 3];
                p[axis] = plane - f64::from([x, y, z][axis]);
                p[run] = r;
                p[1] = h;
                p
            })
            .collect();
        let n = polygon_normal(&pts);
        if n[axis] * f64::from(sign) < 0.0 {
            pts.reverse();
        }
        // Too thin to see (a sliver where the profile meets the surface).
        if polygon_normal(&pts)[axis].abs() < 0.5 {
            continue;
        }
        let at = [f64::from(x), f64::from(y), f64::from(z)];
        let light = vec![1.0; pts.len()];
        let lit = vec![flood; pts.len()];
        b.polygon(
            &pts,
            at,
            normal,
            tint,
            color,
            &tile,
            None,
            Some((&light, &lit)),
        );
    }
    b.tag_fluid(style.fluid);
}

/// Water in the open part of a flooded shaped cell: its faces, inset so the solid shows through.
fn flooded_water(b: &mut Builder, cells: &Cells<'_>, x: i32, y: i32, z: i32) {
    let water = cells.kinds.flood();
    // Its sides reach its drawn surface, unless water or a solid is over it.
    let ka = cells.kind(x, y + 1, z);
    let top = if ka.liquid || ka.flooded || face_covered(cube(), 2, cover(&ka)) {
        1.0
    } else {
        1.0 - water.style.surface_drop
    };
    for (face, &(axis, sign)) in FACES.iter().enumerate() {
        let [nx, ny, nz] = neighbour_of(x, y, z, axis, sign);
        let kn = cells.kind(nx, ny, nz);
        if kn.liquid || kn.flooded || face_covered(cube(), face, cover(&kn)) {
            continue;
        }
        if axis != 1 {
            let plane = f64::from([x, y, z][axis])
                + if sign > 0 {
                    1.0 - FLOOD_INSET
                } else {
                    FLOOD_INSET
                };
            let lit = cells.flood.cell([nx, ny, nz]);
            fluid_side(b, &water.style, &kn, [x, y, z], face, plane, 0.0, top, lit);
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
            [1.0; 4],
            [cells.flood.cell([nx, ny, nz]); 4],
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

/// A mesh in chunk-local cells scaled to chunk-local metres: y × the cell height
/// (HALF_HEIGHT_GRID.md §2). Normals are already in metres (`Builder::polygon`); uvs stay in cells.
fn to_metres(mut m: MeshArrays) -> MeshArrays {
    if CELL_H != 1.0 {
        for p in m.positions.iter_mut().skip(1).step_by(3) {
            *p = (f64::from(*p) * CELL_H) as f32;
        }
    }
    m
}

/// A mesh in chunk-local cells mirrored in y (about the chunk's middle): re-wound.
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
/// Positions are chunk-local metres. A chunk of face B (`mirror`; BIFACIAL_WORLD.md §6) is meshed as its
/// mirror image and the geometry turned back: its faces toward the player's up take the top
/// texture and shading, and its water surface lies 1/8 above the bottom of its cell. Plants'
/// jitter is seeded as if the chunk's minimum corner were the world's origin ([`mesh_chunk_at`]).
pub fn mesh_chunk(padded: &[u16], mirror: bool) -> Result<ChunkMeshes, LengthError> {
    mesh_chunk_at(padded, mirror, [0; 3])
}

/// [`mesh_chunk`] for the chunk whose minimum corner is world cell `origin`: plants (the cross
/// look) are jittered by their world position, so the same plant looks the same in every chunk
/// mesh and on every client.
pub fn mesh_chunk_at(
    padded: &[u16],
    mirror: bool,
    origin: [i32; 3],
) -> Result<ChunkMeshes, LengthError> {
    mesh_chunk_lit(padded, &[], mirror, origin)
}

/// [`mesh_chunk_at`] with the chunk's flood light (docs/DAY_NIGHT.md §11): `light` is its
/// packed sky and block light with a one-cell apron in [`padded_index`] order, world-oriented
/// (`dwell_world::light::LightMap::padded`); empty (or short) lights it as open sky.
pub fn mesh_chunk_lit(
    padded: &[u16],
    light: &[u8],
    mirror: bool,
    origin: [i32; 3],
) -> Result<ChunkMeshes, LengthError> {
    mesh_chunk_with(Kinds::build(), padded, light, mirror, origin)
}

/// [`mesh_chunk_at`] over the registry `kinds` describes.
pub(crate) fn mesh_chunk_with(
    kinds: &Kinds,
    padded: &[u16],
    light: &[u8],
    mirror: bool,
    origin: [i32; 3],
) -> Result<ChunkMeshes, LengthError> {
    // The one-voxel apron, or the sky volume's deeper one (the client's jobs): the faces are
    // meshed from the one-voxel block inside it, the ambient light from all of it.
    let (inner, ext, apron) = match padded.len() {
        PADDED_VOLUME => (std::borrow::Cow::Borrowed(padded), [PAD as i32; 3], [1; 3]),
        SKY_VOLUME => (
            std::borrow::Cow::Owned(inner_block(padded)),
            SKY_EXTENT,
            SKY_APRON,
        ),
        got => {
            return Err(LengthError {
                expected: PADDED_VOLUME,
                got,
            });
        }
    };
    let occ = Occupancy::new(padded, ext, apron, mirror, |id| kinds.of(id).blocks_light);
    let flood = Flood::new(light, mirror);
    let m = if mirror {
        // Meshed in the face-local frame (BIFACIAL_WORLD.md §11): the mirror image's minimum
        // corner is the face-local chunk's, so a plant is jittered as its face-A twin is.
        let local = [origin[0], FaceFrame::B.row(origin[1] + N - 1), origin[2]];
        let m = mesh_cells(kinds, &mirror_padded(&inner), &occ, &flood, local);
        ChunkMeshes {
            opaque: flip_arrays_y(m.opaque),
            transparent: flip_arrays_y(m.transparent),
            cutout: flip_arrays_y(m.cutout),
        }
    } else {
        mesh_cells(kinds, &inner, &occ, &flood, origin)
    };
    Ok(ChunkMeshes {
        opaque: to_metres(m.opaque),
        transparent: to_metres(m.transparent),
        cutout: to_metres(m.cutout),
    })
}

/// The one-voxel padded block ([`PADDED_VOLUME`]) inside a sky volume ([`SKY_VOLUME`]).
fn inner_block(sky: &[u16]) -> Vec<u16> {
    let [ex, _, ez] = SKY_EXTENT;
    let [ax, ay, az] = SKY_APRON;
    let mut out = vec![0u16; PADDED_VOLUME];
    for y in -1..=N {
        for z in -1..=N {
            let from = (ax - 1 + ex * ((z + az) + ez * (y + ay))) as usize;
            let to = padded_index(-1, y, z);
            out[to..to + PAD].copy_from_slice(&sky[from..from + PAD]);
        }
    }
    out
}

/// A shaped vertex's place in the ray cache: its face-local world point in eighths of a cell, and
/// its normal in millionths.
type ShapedKey = ([i64; 3], [i64; 3]);

/// The light key of each corner of a face cell (u0v0, u1v0, u1v1, u0v1; `sky::corner_key`).
type CornerKeys = [u32; 4];

/// The meshes of a chunk of face A (or a mirrored one), in chunk-local cells.
fn mesh_cells(
    kinds: &Kinds,
    padded: &[u16],
    occ: &Occupancy,
    flood: &Flood,
    origin: [i32; 3],
) -> ChunkMeshes {
    let mut opaque = Builder::default();
    let mut transparent = Builder::default();
    let cells = Cells::new(kinds, padded, occ, flood, origin);
    let mut cutout = Builder::default();
    let n = N as usize;
    // Merge mask for one slice: material + 1 of a visible mergeable face, 0 for none.
    let mut mask = vec![0i32; n * n];
    // Each masked face's corner light keys: faces merge only where these match too, so a merged
    // quad's corners carry its cells' light exactly (equal keys on neighbours mean it is level
    // along the merge).
    let mut light_mask: Vec<CornerKeys> = vec![[0; 4]; n * n];
    // The slice's sky rays per vertex ((n + 1)², u fastest), computed once a face needs them.
    let mut vertex_rays: Vec<u8> = vec![u8::MAX; (n + 1) * (n + 1)];

    for (face, &(axis, sign)) in FACES.iter().enumerate() {
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        for d in 0..N {
            mask.fill(0);
            vertex_rays.fill(u8::MAX);
            let plane = if sign > 0 { d + 1 } else { d };
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
                    let k = cells.kind(cx, cy, cz);
                    if let Some(lf) = k.ladder_face {
                        if lf == face {
                            let target = match k.style.render {
                                RenderMode::Opaque => &mut opaque,
                                RenderMode::Cutout => &mut cutout,
                                RenderMode::Translucent => &mut transparent,
                            };
                            ladder_face(
                                target,
                                &k.style,
                                cells.flood.cell([cx, cy, cz]),
                                face,
                                cell,
                            );
                            if k.style.render == RenderMode::Translucent {
                                target.tag_fluid(k.style.fluid);
                            }
                        }
                        continue;
                    }
                    if k.shape == Shape::Shaped || k.style.look == Look::Cross {
                        continue; // drawn whole, below
                    }
                    let [nx, ny, nz] = neighbour_of(cx, cy, cz, axis, sign);
                    let kn = cells.kind(nx, ny, nz);
                    // Beside the same fluid whose surface is drawn lower (a river's step down: this
                    // column goes on up, the next ends here), the strip between the two surfaces
                    // is this cell's to draw, or the step shows a slit into the water
                    // (docs/BUGS.md 4).
                    if k.liquid && axis != 1 && kn.liquid && cells.state(nx, ny, nz) == m {
                        let own = fluid_side_top(&cells, &k, m, cx, cy, cz);
                        let theirs = fluid_side_top(&cells, &kn, m, nx, ny, nz);
                        if own > theirs + 1e-9 {
                            let plane = f64::from(if sign > 0 { d + 1 } else { d });
                            let lit = cells.flood.cell([cx, cy, cz]);
                            fluid_side(
                                &mut transparent,
                                &k.style,
                                &kn,
                                cell,
                                face,
                                plane,
                                theirs,
                                own,
                                lit,
                            );
                        }
                        continue;
                    }
                    if hidden(&k, &kn, m, cells.state(nx, ny, nz), face) {
                        continue;
                    }
                    let mut surface_row = 0;
                    if k.liquid && axis != 1 {
                        let top = fluid_side_top(&cells, &k, m, cx, cy, cz);
                        if fluid_side_alone(&kn) {
                            let plane = f64::from(if sign > 0 { d + 1 } else { d });
                            let lit = cells.flood.cell([nx, ny, nz]);
                            fluid_side(
                                &mut transparent,
                                &k.style,
                                &kn,
                                cell,
                                face,
                                plane,
                                0.0,
                                top,
                                lit,
                            );
                            continue;
                        }
                        if top < 1.0 {
                            surface_row = SURFACE_BIT;
                        }
                    }
                    let rule = side_rule(&cells, &k, cx, cy, cz, face).map_or(0, |r| r as i32 + 1);
                    mask[i as usize + n * j as usize] =
                        (i32::from(m) + 1) | (rule << RULE_SHIFT) | surface_row;
                    light_mask[i as usize + n * j as usize] =
                        if k.style.render == RenderMode::Translucent {
                            flood_keys(&cells, face, [nx, ny, nz])
                        } else {
                            corner_keys(
                                &cells,
                                &mut vertex_rays,
                                (face, plane),
                                [i, j],
                                [nx, ny, nz],
                            )
                        };
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
                    let lkey = light_mask[i + n * j];
                    let m = ((key & STATE_MASK) - 1) as StateId;
                    let rule = ((key & !SURFACE_BIT) >> RULE_SHIFT)
                        .checked_sub(1)
                        .map(|r| r as usize);
                    let style = kinds.of(m).style;
                    let (color, tile) = side_surface(&style, face, rule);
                    // A hanging tile runs from each block's top: its quads stay one cell tall.
                    let hang = tile.hang && axis != 1;
                    let same = |at: usize| mask[at] == key && light_mask[at] == lkey;
                    let mut w = 1;
                    while i + w < n && !(hang && u == 1) && same(i + w + n * j) {
                        w += 1;
                    }
                    let mut h = 1;
                    'grow: while j + h < n && !(hang && v == 1) {
                        for k in 0..w {
                            if !same(i + k + n * (j + h)) {
                                break 'grow;
                            }
                        }
                        h += 1;
                    }
                    for dv in 0..h {
                        mask[i + n * (j + dv)..i + w + n * (j + dv)].fill(0);
                    }
                    let mut plane = f64::from(if sign > 0 { d + 1 } else { d });
                    if style.look == Look::Fluid && face == 2 {
                        plane = fluid_top(d, &style);
                    }
                    let see_through = style.render == RenderMode::Translucent;
                    let target = match style.render {
                        RenderMode::Opaque => &mut opaque,
                        RenderMode::Cutout => &mut cutout,
                        RenderMode::Translucent => &mut transparent,
                    };
                    // A fluid's side in its surface's row (one row of cells tall) stops at the
                    // drawn surface.
                    let mut us = (i as f64, (i + w) as f64);
                    let mut vs = (j as f64, (j + h) as f64);
                    if key & SURFACE_BIT != 0 {
                        let top = 1.0 - style.surface_drop;
                        if u == 1 {
                            us.1 = us.0 + top;
                        } else if v == 1 {
                            vs.1 = vs.0 + top;
                        }
                    }
                    target.quad(
                        axis,
                        sign,
                        plane,
                        us,
                        vs,
                        color,
                        &tile,
                        lkey.map(key_light),
                        lkey.map(key_flood),
                    );
                    if see_through {
                        target.tag_fluid(style.fluid);
                    }
                    i += w;
                }
            }
        }
    }
    // Shaped blocks (the generated ramps): whole voxels, not slices.
    for cy in 0..N {
        for cz in 0..N {
            for cx in 0..N {
                let m = padded[padded_index(cx, cy, cz)];
                if m == 0 {
                    continue;
                }
                let k = cells.kind(cx, cy, cz);
                if k.shape == Shape::Shaped {
                    shaped_voxel(&mut opaque, &cells, &k, cx, cy, cz);
                }
                if k.style.look == Look::Cross {
                    let target = match k.style.render {
                        RenderMode::Translucent => &mut transparent,
                        _ => &mut cutout,
                    };
                    let world = [
                        i64::from(origin[0]) + i64::from(cx),
                        i64::from(origin[1]) + i64::from(cy),
                        i64::from(origin[2]) + i64::from(cz),
                    ];
                    let support = cells.support(cx, cy, cz);
                    // A plant is lit by the sky its ground sees: the ground's top at the cell's
                    // corner (no corner occlusion: it is not a cell's face, and it blocks no
                    // light itself).
                    let light = vertex_light(AO_OPEN, cells.lattice_rays([cx, cy, cz], 2));
                    let flood = cells.flood.cell([cx, cy, cz]);
                    cross_voxel(
                        target,
                        &k.style,
                        world,
                        [cx, cy, cz],
                        support,
                        (light, flood),
                    );
                    if k.style.render == RenderMode::Translucent {
                        target.tag_fluid(k.style.fluid);
                    }
                }
                if k.flooded {
                    flooded_water(&mut transparent, &cells, cx, cy, cz);
                }
            }
        }
    }
    // Ladders carry no sway or fade, and take the ink.
    let vertices = cutout.m.positions.len() / 3;
    cutout.m.plants.resize(vertices * 4, 0.0);
    cutout.m.bare.resize(vertices, 0.0);
    // Fluids and see-through faces are lit by their own shader: no baked light, but the flood
    // light (docs/DAY_NIGHT.md §11), so water in a cave or under a roof is dark.
    transparent.m.light.clear();
    ChunkMeshes {
        opaque: opaque.m,
        transparent: transparent.m,
        cutout: cutout.m,
    }
}

#[cfg(test)]
#[path = "mesher_tests.rs"]
mod tests;
