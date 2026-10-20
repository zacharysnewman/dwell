//! Greedy chunk mesher (`client/src/mesh/mesher.ts`; ARCHITECTURE.md §5, §6.1): a chunk's voxels,
//! with a one-voxel apron from its neighbours, into render geometry. Visible faces of full cubes
//! and water merge into rectangles of one material per slice; shaped blocks (drawn with their true
//! normals) and ladders are emitted polygon by polygon. Textures repeat once per block across a
//! merged quad: `uvs` carry positions in blocks and `tiles` the atlas rectangle.
//!
//! Arithmetic is done in `f64` and stored as `f32`, as the TS computes in doubles and stores into
//! `Float32Array`s, so the buffers match it value for value.

use std::sync::OnceLock;

use dwell_blocks::{
    FLUIDS, FluidInfo, RenderMode, STATES, ShapeInfo, StateId, StateInfo, shape::faces_of,
};
use dwell_protocol::CHUNK_SIZE;

use crate::LengthError;
use crate::style::{
    Look, MaterialStyle, cube, fluid_value, material_style, mirror_material, style_of,
};
use crate::textures::{TileRect, tile_rect};
use dwell_blocks::shape::plant_sink;
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
    /// The cutout mesh only (empty in the others): per vertex, how far wind sways it (m; 0 at a
    /// plant's base and on ladders), the wind's speed, and the distance fade's start and end (m;
    /// end 0: no fade).
    pub plants: Vec<f32>,
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

/// The cells' kinds where adaptive blocks change them (docs/ADAPTIVE_BLOCKS.md): an adaptive top
/// block in the chunk takes its derived shape — a cube when the rules say full — and holds water
/// beside or under a liquid. Apron cells' shapes need voxels beyond the apron, so an adaptive top
/// block there is taken to cover none of its sides (its neighbour's faces toward it are drawn; any
/// it does cover lie inside it). The padded block faces up (a face-B chunk is meshed mirrored).
struct Cells<'a> {
    kinds: &'a Kinds,
    padded: &'a [u16],
    over: Vec<Option<(u16, bool)>>,
}

/// The shape an apron's adaptive top block is drawn against: no side covered, the floor full.
fn apron_shape() -> u16 {
    // A lone block (every neighbour lower): the half pyramid, whose sides are all zero.
    dwell_blocks::ADAPTIVE_UP[dwell_blocks::adaptive::index([dwell_blocks::adaptive::LOWER; 8])]
}

impl<'a> Cells<'a> {
    fn new(kinds: &'a Kinds, padded: &'a [u16]) -> Self {
        let mut over = vec![None; padded.len()];
        let p = PAD as i32;
        let inside = |v: i32| (-1..p - 1).contains(&v);
        let at = |x: i32, y: i32, z: i32| {
            if inside(x) && inside(y) && inside(z) {
                padded[padded_index(x, y, z)]
            } else {
                0
            }
        };
        for y in -1..N + 1 {
            for z in -1..N + 1 {
                for x in -1..N + 1 {
                    let m = padded[padded_index(x, y, z)];
                    if m == 0 || !STATES.get(usize::from(m)).is_some_and(|s| s.adaptive) {
                        continue;
                    }
                    let interior =
                        (0..N).contains(&x) && (0..N).contains(&y) && (0..N).contains(&z);
                    let get = |dx: i32, dy: i32, dz: i32| at(x + dx, y + dy, z + dz);
                    let derived = if interior {
                        dwell_blocks::adaptive::derive(get, 1)
                    } else if y < N
                        && !STATES
                            .get(usize::from(get(0, 1, 0)))
                            .is_some_and(|s| s.solid)
                    {
                        Some(apron_shape())
                    } else {
                        None
                    };
                    if let Some(i) = derived {
                        over[padded_index(x, y, z)] =
                            Some((i, dwell_blocks::adaptive::flooded_at(get, 1)));
                    }
                }
            }
        }
        Self {
            kinds,
            padded,
            over,
        }
    }

    fn state(&self, x: i32, y: i32, z: i32) -> StateId {
        self.padded[padded_index(x, y, z)]
    }

    /// The solid a plant in the chunk's cell (x, y, z) stands on: the cell below's shape — an
    /// adaptive top block's derived one, worked out exactly even in the apron's bottom row (its
    /// neighbours there are all in the padded block) — or none when it has no solid.
    fn support(&self, x: i32, y: i32, z: i32) -> Option<&'static ShapeInfo> {
        let sy = y - 1;
        let k = if sy >= 0 {
            self.kind(x, y - 1, z)
        } else {
            let mut k = self.kinds.of(self.state(x, sy, z));
            let get = |dx: i32, dy: i32, dz: i32| self.state(x + dx, sy + dy, z + dz);
            if let Some(i) = dwell_blocks::adaptive::derive(get, 1) {
                k.shape = Shape::Shaped;
                k.def = Some(&dwell_blocks::SHAPES[usize::from(i)]);
            }
            k
        };
        if k.shape == Shape::Empty { None } else { k.def }
    }

    fn kind(&self, x: i32, y: i32, z: i32) -> Kind {
        let i = padded_index(x, y, z);
        let mut k = self.kinds.of(self.padded[i]);
        if let Some((shape, flooded)) = self.over[i] {
            let def = &dwell_blocks::SHAPES[usize::from(shape)];
            let full = !def.fan && def.corners == [2, 2, 2, 2];
            k.shape = if full { Shape::Full } else { Shape::Shaped };
            k.def = Some(def);
            k.flooded = flooded && !full;
        }
        k
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
    // Ladders before it carry no sway: line the plants stream up with the vertices.
    b.m.plants.resize(b.vertex_count() as usize * 4, 0.0);
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

/// An upright shape whose top is flat at half height (the adaptive rules' slab).
fn is_half_slab(s: &ShapeInfo) -> bool {
    !s.inverted && !s.fan && s.corners == [1, 1, 1, 1]
}

/// Greedy rectangles of the flat half-height tops in layer `y` (`mask`: material + 1 per
/// x + N·z), drawn as the shaped mesher draws a top: the top tile, the top's shading.
fn flat_tops(b: &mut Builder, kinds: &Kinds, mask: &mut [i32], y: i32) {
    let n = N as usize;
    for z in 0..n {
        let mut x = 0;
        while x < n {
            let key = mask[x + n * z];
            if key == 0 {
                x += 1;
                continue;
            }
            let mut w = 1;
            while x + w < n && mask[x + w + n * z] == key {
                w += 1;
            }
            let mut h = 1;
            'grow: while z + h < n {
                for k in 0..w {
                    if mask[x + k + n * (z + h)] != key {
                        break 'grow;
                    }
                }
                h += 1;
            }
            for dz in 0..h {
                mask[x + n * (z + dz)..x + w + n * (z + dz)].fill(0);
            }
            let style = kinds.of((key - 1) as StateId).style;
            let (color, tile) = surface(&style, 1, 1);
            // Axis y: u = z, v = x.
            b.quad(
                1,
                1,
                f64::from(y) + 0.5,
                (z as f64, (z + h) as f64),
                (x as f64, (x + w) as f64),
                color,
                &tile,
            );
            x += w;
        }
    }
}

/// A shaped voxel: each polygon of its shape, but the parts of cell faces a neighbour covers.
/// `merged_top`: its flat top is drawn merged with its neighbours' (`flat_tops`), not here.
#[allow(clippy::too_many_arguments)]
fn shaped_voxel(
    b: &mut Builder,
    cells: &Cells<'_>,
    k: &Kind,
    x: i32,
    y: i32,
    z: i32,
    merged_top: bool,
) {
    let Some(def) = k.def else {
        return;
    };
    for f in faces_of(def) {
        let tag = usize::from(f.tag);
        if merged_top && f.tag == dwell_blocks::shape::SURFACE_TAG {
            continue;
        }
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
        let (color, tile) = if tag < 6 && axis != 1 {
            side_surface(&k.style, tag, side_rule(cells, k, x, y, z, tag))
        } else {
            surface(&k.style, axis, sign)
        };
        let at = [f64::from(x), f64::from(y), f64::from(z)];
        b.polygon(&pts, at, normal, tint, color, &tile, side_top_of(def, tag));
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
/// side between the cell's floor and `top` (cells) that lies above the solid of the neighbour
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
    top: f64,
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
        let rect = [(ra, 0.0), (rb, 0.0), (rb, top), (ra, top)];
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
        b.polygon(&pts, at, normal, tint, color, &tile, None);
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
            fluid_side(b, &water.style, &kn, [x, y, z], face, plane, top);
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
    mesh_chunk_with(Kinds::build(), padded, mirror, origin)
}

/// [`mesh_chunk_at`] over the registry `kinds` describes.
pub(crate) fn mesh_chunk_with(
    kinds: &Kinds,
    padded: &[u16],
    mirror: bool,
    origin: [i32; 3],
) -> Result<ChunkMeshes, LengthError> {
    if padded.len() != PADDED_VOLUME {
        return Err(LengthError {
            expected: PADDED_VOLUME,
            got: padded.len(),
        });
    }
    if mirror {
        let m = mesh_chunk_with(kinds, &mirror_padded(padded), false, origin)?;
        return Ok(ChunkMeshes {
            opaque: flip_arrays_y(m.opaque),
            transparent: flip_arrays_y(m.transparent),
            cutout: flip_arrays_y(m.cutout),
        });
    }
    let mut opaque = Builder::default();
    let mut transparent = Builder::default();
    let cells = Cells::new(kinds, padded);
    let mut cutout = Builder::default();
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
                    let k = cells.kind(cx, cy, cz);
                    if let Some(lf) = k.ladder_face {
                        if lf == face {
                            let target = match k.style.render {
                                RenderMode::Opaque => &mut opaque,
                                RenderMode::Cutout => &mut cutout,
                                RenderMode::Translucent => &mut transparent,
                            };
                            ladder_face(target, &k.style, face, cx, cy, cz);
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
                    if hidden(&k, &kn, m, cells.state(nx, ny, nz), face) {
                        continue;
                    }
                    let mut surface_row = 0;
                    if k.liquid && axis != 1 {
                        let top = fluid_side_top(&cells, &k, m, cx, cy, cz);
                        if fluid_side_alone(&kn) {
                            let plane = f64::from(if sign > 0 { d + 1 } else { d });
                            fluid_side(&mut transparent, &k.style, &kn, cell, face, plane, top);
                            continue;
                        }
                        if top < 1.0 {
                            surface_row = SURFACE_BIT;
                        }
                    }
                    let rule = side_rule(&cells, &k, cx, cy, cz, face).map_or(0, |r| r as i32 + 1);
                    mask[i as usize + n * j as usize] =
                        (i32::from(m) + 1) | (rule << RULE_SHIFT) | surface_row;
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
                    let m = ((key & STATE_MASK) - 1) as StateId;
                    let rule = ((key & !SURFACE_BIT) >> RULE_SHIFT)
                        .checked_sub(1)
                        .map(|r| r as usize);
                    let style = kinds.of(m).style;
                    let mut plane = f64::from(if sign > 0 { d + 1 } else { d });
                    if style.look == Look::Fluid && face == 2 {
                        plane = fluid_top(d, &style);
                    }
                    let (color, tile) = side_surface(&style, face, rule);
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
                    target.quad(axis, sign, plane, us, vs, color, &tile);
                    if see_through {
                        target.tag_fluid(style.fluid);
                    }
                    i += w;
                }
            }
        }
    }
    // Shaped blocks: whole voxels, not slices — but the flat tops of a level run of adaptive
    // blocks (the rules' slab, half a block up) merge into rectangles like cube faces.
    for cy in 0..N {
        mask.fill(0);
        let mut any = false;
        for cz in 0..N {
            for cx in 0..N {
                let m = padded[padded_index(cx, cy, cz)];
                if m == 0 {
                    continue;
                }
                let k = cells.kind(cx, cy, cz);
                if k.shape == Shape::Shaped {
                    let flat = k.def.is_some_and(is_half_slab);
                    if flat {
                        mask[cx as usize + n * cz as usize] = i32::from(m) + 1;
                        any = true;
                    }
                    shaped_voxel(&mut opaque, &cells, &k, cx, cy, cz, flat);
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
                    cross_voxel(target, &k.style, world, [cx, cy, cz], support);
                    if k.style.render == RenderMode::Translucent {
                        target.tag_fluid(k.style.fluid);
                    }
                }
                if k.flooded {
                    flooded_water(&mut transparent, &cells, cx, cy, cz);
                }
            }
        }
        if any {
            flat_tops(&mut opaque, kinds, &mut mask, cy);
        }
    }
    // Ladders carry no sway or fade.
    let vertices = cutout.m.positions.len() / 3;
    cutout.m.plants.resize(vertices * 4, 0.0);
    Ok(ChunkMeshes {
        opaque: opaque.m,
        transparent: transparent.m,
        cutout: cutout.m,
    })
}

#[cfg(test)]
#[path = "mesher_tests.rs"]
mod tests;
