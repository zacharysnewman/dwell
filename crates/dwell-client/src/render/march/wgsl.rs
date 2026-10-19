//! The marcher's WGSL (docs/RAY_MARCHED_RENDERER.md §4.2): a transcription of `dwell_march::march`
//! over the words `dwell_march::Scene::gpu` uploads, generated here so the two can't drift — every
//! constant the shader knows (the layout's sizes, tags and masks, the step-histogram and debug
//! layout, the face tints, the atlas) is written from the Rust value, and a test parses the module
//! and checks each constant against its source.
//!
//! The arithmetic follows `march.rs` operation for operation (the same plane distances, the same
//! tie rules, `⌊log2⌋` from the exponent bits, a power-of-two reciprocal from the bits), so a GPU
//! ray lands on the same cell, face and `t` as the reference marcher given the same direction —
//! the per-pixel test (`march/test.rs`) checks exactly that.
//!
//! # Bindings
//!
//! - group 0: the shared frame uniforms, atlas and sampler (`shaders::lighting_source`).
//! - group 1: `u` (uniform, `MarchUniforms`: the levels and the camera), `slots`, `nodes`,
//!   `node_columns`, `columns`, `pool`, `looks` (storage, read), `stats` (storage, atomic: the
//!   step histogram, then the per-pixel debug records when asked for).
//! - group 2: the outputs, storage textures: `out_color` (`rgba16float`, lit linear colour, alpha
//!   1 where a ray hit, 0 for the sky), `out_depth` (`r32float`, view depth in metres, 0 for none),
//!   `out_id` (`r32uint`, `cell_id`: hashed level and cell, and the face, 0 for none).

use dwell_march::layout::{
    BRICK_CELLS, COLUMN_MIPS, COLUMN_WORDS, GRID_OFFSET, LEVEL_COUNT, MAX_LEVEL, MAX_NODES,
    MIP_OFFSET, NODE_WORDS, OCC_WORDS, RAW_WIDTH, SLOT_NONE, TAG_AIR, TAG_SHIFT, TAG_SOLID,
};
use dwell_march::march::COLUMN_EPS;
use dwell_march::scene::COLUMNS_NONE;

use super::looks::LOOK_WORDS;
use super::stats::{DEBUG_WORDS, HIST_BINS, HIST_CAPPED, HIST_HITS, HIST_MISSES, HIST_WORDS};

/// Workgroup side (8 × 8 invocations).
pub const WORKGROUP: u32 = 8;
/// A large finite stand-in for +∞ (fast-math backends may not keep infinities).
pub const BIG: f32 = 3.0e38;

/// `MarchUniforms.flags` bits.
pub const FLAG_COLUMNS: u32 = 1;
pub const FLAG_DISTANCES: u32 = 2;
pub const FLAG_HISTOGRAM: u32 = 4;
pub const FLAG_DEBUG: u32 = 8;
pub const FLAG_DATA: u32 = 16;

/// The hit record's bits (`hit_record`): set on every hit, and for a cell on face B.
pub const HIT_BIT: u32 = 1 << 31;
pub const ON_B_BIT: u32 = 1 << 24;

/// The face code of a hit that started inside an occupied cell (no face): `cell_id`'s low bits.
pub const FACE_NONE: u32 = 6;

/// A WGSL constant's value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    U32(u32),
    I32(i32),
    F32(f32),
}

/// Every constant the shader knows, from its Rust source.
pub fn constants() -> Vec<(&'static str, Value)> {
    use Value::*;
    let tint = |axis: usize, sign: i32| dwell_look::face_tint(axis, sign).map(|v| v as f32);
    let [tx, ty, tz] = tint(1, 1);
    let [sxx, sxy, sxz] = tint(0, 1);
    let [szx, szy, szz] = tint(2, 1);
    let [bx, by, bz] = tint(1, -1);
    let midplane_grid = dwell_protocol::world::MIDPLANE_Y + GRID_OFFSET;
    vec![
        ("SLOT_NONE", U32(SLOT_NONE)),
        ("TAG_SHIFT", U32(TAG_SHIFT)),
        ("TAG_AIR", U32(TAG_AIR)),
        ("TAG_SOLID", U32(TAG_SOLID)),
        ("NODE_MASK", U32(MAX_NODES - 1)),
        ("NODE_WORDS", U32(NODE_WORDS as u32)),
        ("BRICK_CELLS", I32(BRICK_CELLS)),
        ("OCC_WORDS", U32(OCC_WORDS as u32)),
        ("RAW_WIDTH", U32(RAW_WIDTH)),
        ("COLUMN_WORDS", U32(COLUMN_WORDS as u32)),
        ("COLUMN_MIPS", U32(COLUMN_MIPS as u32)),
        ("COLUMNS_NONE", U32(COLUMNS_NONE)),
        ("COLUMN_EPS", F32(COLUMN_EPS)),
        ("MAX_LEVEL", U32(MAX_LEVEL)),
        ("LEVEL_COUNT", U32(LEVEL_COUNT as u32)),
        ("LOOK_WORDS", U32(LOOK_WORDS as u32)),
        ("HIST_BINS", U32(HIST_BINS as u32)),
        ("HIST_CAPPED", U32(HIST_CAPPED as u32)),
        ("HIST_HITS", U32(HIST_HITS as u32)),
        ("HIST_MISSES", U32(HIST_MISSES as u32)),
        ("DEBUG_BASE", U32(HIST_WORDS as u32)),
        ("DEBUG_WORDS", U32(DEBUG_WORDS as u32)),
        ("FLAG_COLUMNS", U32(FLAG_COLUMNS)),
        ("FLAG_DISTANCES", U32(FLAG_DISTANCES)),
        ("FLAG_HISTOGRAM", U32(FLAG_HISTOGRAM)),
        ("FLAG_DEBUG", U32(FLAG_DEBUG)),
        ("FLAG_DATA", U32(FLAG_DATA)),
        ("FACE_NONE", U32(FACE_NONE)),
        ("HIT_BIT", U32(HIT_BIT)),
        ("ON_B_BIT", U32(ON_B_BIT)),
        ("BIG", F32(BIG)),
        ("MIDPLANE_GRID", I32(midplane_grid as i32)),
        ("ATLAS_SIZE", F32(dwell_mesh::ATLAS_SIZE as f32)),
        ("ATLAS_CELL", F32(dwell_mesh::CELL as f32)),
        ("ATLAS_GUTTER", F32(dwell_mesh::GUTTER as f32)),
        ("TILE_TEXELS", F32(dwell_mesh::TILE as f32)),
        ("TINT_TOP_R", F32(tx)),
        ("TINT_TOP_G", F32(ty)),
        ("TINT_TOP_B", F32(tz)),
        ("TINT_X_R", F32(sxx)),
        ("TINT_X_G", F32(sxy)),
        ("TINT_X_B", F32(sxz)),
        ("TINT_Z_R", F32(szx)),
        ("TINT_Z_G", F32(szy)),
        ("TINT_Z_B", F32(szz)),
        ("TINT_BOTTOM_R", F32(bx)),
        ("TINT_BOTTOM_G", F32(by)),
        ("TINT_BOTTOM_B", F32(bz)),
    ]
}

fn literal(v: Value) -> String {
    match v {
        Value::U32(x) => format!("{x}u"),
        Value::I32(x) => format!("{x}i"),
        Value::F32(x) => {
            let s = format!("{x:?}");
            if s.contains('.') || s.contains('e') {
                format!("{s}f")
            } else {
                format!("{s}.0f")
            }
        }
    }
}

fn constant_block() -> String {
    let mut out = String::new();
    for (name, v) in constants() {
        let ty = match v {
            Value::U32(_) => "u32",
            Value::I32(_) => "i32",
            Value::F32(_) => "f32",
        };
        out += &format!("const {name}: {ty} = {};\n", literal(v));
    }
    // The column grid's mip offsets (layout::MIP_OFFSET), as a function: a runtime index into a
    // const array is not portable.
    out += "fn mip_offset(m: u32) -> u32 {\n    switch m {\n";
    for (m, off) in MIP_OFFSET.iter().enumerate() {
        out += &format!("        case {m}u: {{ return {off}u; }}\n");
    }
    out += "        default: { return 0u; }\n    }\n}\n";
    out
}

/// The uniforms (group 1, binding 0), `MARCH_UNIFORM_WORDS` words; `MarchUniforms::to_words` writes them.
const UNIFORMS: &str = r"
struct LevelInfo {
    origin: vec3<i32>,
    occ_hi: i32,
    dims: vec3<u32>,
    slot_base: u32,
    cell_log2: u32,
    occ_lo: i32,
    band_end: f32,
    _pad: u32,
};
struct MarchUniforms {
    levels: array<LevelInfo, LEVEL_COUNT>,
    eye_cell: vec3<i32>,
    top: u32,
    eye_frac: vec3<f32>,
    max_t: f32,
    wlo: vec3<i32>,
    step_cap: u32,
    whi: vec3<i32>,
    flags: u32,
    forward: vec3<f32>,
    pixel_angle: f32,
    right: vec3<f32>,
    lod_pixels: f32,
    up: vec3<f32>,
    tan_x: f32,
    size: vec2<u32>,
    tan_y: f32,
    dbg: u32,
};
@group(1) @binding(0) var<uniform> u: MarchUniforms;
@group(1) @binding(6) var<storage, read> looks: array<u32>;

// The ray through pixel `at` (MarchFrame::ray), unit length.
fn pixel_ray(at: vec2<u32>) -> vec3<f32> {
    let size = vec2<f32>(u.size);
    let ndc = vec2<f32>(
        (f32(at.x) + 0.5) / size.x * 2.0 - 1.0,
        1.0 - (f32(at.y) + 0.5) / size.y * 2.0,
    );
    return normalize(u.forward + ndc.x * u.tan_x * u.right + ndc.y * u.tan_y * u.up);
}
";

/// The march's own bindings: the brick structure, the stats, and its outputs.
const MARCH_BINDINGS: &str = r"
@group(1) @binding(1) var<storage, read> slots: array<u32>;
@group(1) @binding(2) var<storage, read> nodes: array<u32>;
@group(1) @binding(3) var<storage, read> node_columns: array<u32>;
@group(1) @binding(4) var<storage, read> columns: array<u32>;
@group(1) @binding(5) var<storage, read> pool: array<u32>;
@group(1) @binding(7) var<storage, read_write> stats: array<atomic<u32>>;

@group(2) @binding(0) var out_depth: texture_storage_2d<r32float, write>;
@group(2) @binding(1) var out_id: texture_storage_2d<r32uint, write>;
@group(2) @binding(2) var out_hit: texture_storage_2d<r32uint, write>;
";

/// The traversal: `march.rs` line for line.
const MARCH: &str = r"
// The ray (RayMath): cell, fraction, unit direction, its reciprocal (0 on a zero component), step
// signs, and which components are zero.
var<private> rc: vec3<i32>;
var<private> rf: vec3<f32>;
var<private> rd: vec3<f32>;
var<private> rinv: vec3<f32>;
var<private> rstep: vec3<i32>;
var<private> rzero: vec3<bool>;
// The traversal's state: distance, the entry plane (axis, or -1 at the start, and its grid
// coordinate) and the box the ray is in.
var<private> t: f32;
var<private> ea: i32;
var<private> ep: i32;
var<private> blo: vec3<i32>;
var<private> bhi: vec3<i32>;

const MISS: u32 = 0u;
const HIT: u32 = 1u;
const CAP: u32 = 2u;

const AXES: vec3<i32> = vec3<i32>(0, 1, 2);

struct Result {
    kind: u32,
    steps: u32,
    level: u32,
    cell: vec3<i32>,
    face: u32,
    t: f32,
    state: u32,
};

// Distances to the planes `grid = b` on each axis (BIG on an axis the ray doesn't move along).
// The same operations per component as `RayMath::tp`.
fn tpv(b: vec3<i32>) -> vec3<f32> {
    return select((vec3<f32>(b - rc) - rf) * rinv, vec3<f32>(BIG), rzero);
}

// The axis of the nearest plane, ties to the lowest axis (RayMath::exit's rule).
fn nearest_axis(v: vec3<f32>) -> i32 {
    if (v.x <= v.y && v.x <= v.z) {
        return 0;
    }
    return select(2, 1, v.y <= v.z);
}

struct Exit {
    t: f32,
    a: i32,
    plane: i32,
};

// Where the ray leaves box [lo, hi) (ties: the lowest axis), and the plane it leaves by.
fn exit_box(lo: vec3<i32>, hi: vec3<i32>) -> Exit {
    let planes = select(lo, hi, rstep > vec3<i32>(0));
    let v = tpv(planes);
    let a = nearest_axis(v);
    let m = AXES == vec3<i32>(a);
    let pl = select(select(planes.z, planes.y, m.y), planes.x, m.x);
    return Exit(min(v.x, min(v.y, v.z)), a, pl);
}

// Leaves box [lo, hi) and continues beyond its exit plane (the skip! macro).
fn skip(lo: vec3<i32>, hi: vec3<i32>) {
    let e = exit_box(lo, hi);
    let m = AXES == vec3<i32>(e.a);
    let up = rstep > vec3<i32>(0);
    // One cell thick on the exit axis, the old box's extent on the others.
    blo = select(lo, select(vec3<i32>(e.plane - 1), vec3<i32>(e.plane), up), m);
    bhi = select(hi, select(vec3<i32>(e.plane), vec3<i32>(e.plane + 1), up), m);
    t = e.t;
    ea = e.a;
    ep = e.plane;
}

// The grid cell the ray is in at t inside [blo, bhi) (RayMath::cell_at).
fn cell_at() -> vec3<i32> {
    let up = rstep > vec3<i32>(0);
    let strict = AXES > vec3<i32>(ea);
    var v = rc + vec3<i32>(floor(rf + t * rd));
    let next = select(vec3<i32>(0), vec3<i32>(1), up);
    let tn = tpv(v + next);
    let te = tpv(v + vec3<i32>(1) - next);
    let crossed_n = (tn < vec3<f32>(t)) | ((tn == vec3<f32>(t)) & !strict);
    let crossed_e = (te < vec3<f32>(t)) | ((te == vec3<f32>(t)) & !strict);
    v = select(select(v, v - rstep, !crossed_e & (te > vec3<f32>(0.0))), v + rstep, crossed_n);
    v = select(v, rc, rzero);
    let entry = select(vec3<i32>(ep - 1), vec3<i32>(ep), up);
    v = select(v, entry, AXES == vec3<i32>(ea));
    return clamp(v, blo, bhi - vec3<i32>(1));
}

fn rel_y(base: i32, at: f32) -> f32 {
    return f32(rc.y - base) + rf.y + at * rd.y;
}

// Cone LOD (level_for): floor(log2(footprint)) from the exponent bits.
fn level_for(at: f32) -> u32 {
    let footprint = at * u.pixel_angle * u.lod_pixels;
    if (footprint < 2.0) {
        return 0u;
    }
    let e = bitcast<u32>(footprint) >> 23u;
    return min(select(0u, e - 127u, e > 127u), MAX_LEVEL);
}

fn class_of(s: u32) -> u32 {
    if (s * LOOK_WORDS >= arrayLength(&looks)) {
        return 0u;
    }
    return looks[s * LOOK_WORDS] & 3u;
}

// The shape hook (march::shape_hit): cubes only today.
fn shape_hit(s: u32) -> bool {
    return class_of(s) >= 2u;
}

fn brick_state(off: u32, v: u32) -> u32 {
    let width = pool[off] & 0x1Fu;
    let base = off + 1u + OCC_WORDS;
    if (width == RAW_WIDTH) {
        return (pool[base + (v >> 1u)] >> ((v & 1u) * 16u)) & 0xFFFFu;
    }
    let palette_words = ((1u << width) + 1u) / 2u;
    let bitpos = v * width;
    let i = (pool[base + palette_words + (bitpos >> 5u)] >> (bitpos & 31u)) & ((1u << width) - 1u);
    return (pool[base + (i >> 1u)] >> ((i & 1u) * 16u)) & 0xFFFFu;
}

fn column_span(cols: u32, m: u32, rx: u32, rz: u32) -> vec2<u32> {
    let i = mip_offset(m) + rx + (32u >> m) * rz;
    let e = columns[cols + (i >> 1u)] >> ((i & 1u) * 16u);
    return vec2<u32>(e & 0xFFu, (e >> 8u) & 0xFFu);
}

fn face_of(a: i32) -> u32 {
    return 2u * u32(a) + select(0u, 1u, rstep[a] > 0);
}

fn done(kind: u32, steps: u32) -> Result {
    return Result(kind, steps, 0u, vec3<i32>(0), FACE_NONE, 0.0, 0u);
}

fn hit(level: u32, cell: vec3<i32>, steps: u32, s: u32) -> Result {
    let face = select(face_of(ea), FACE_NONE, ea < 0);
    return Result(HIT, steps, level, cell, face, t, s);
}

// A level with data at grid cell g (Scene::find): the level, the cell there, its node and slot.
struct Found {
    ok: bool,
    l: u32,
    cell: vec3<i32>,
    n: vec3<i32>,
    slot: u32,
};

fn find(g: vec3<i32>, first: u32, top: u32) -> Found {
    for (var lv = first; lv <= top; lv++) {
        let info = u.levels[lv];
        let c = g >> vec3<u32>(lv);
        let nn = c >> vec3<u32>(5u);
        let d = nn - info.origin;
        if (any(d < vec3<i32>(0)) || any(vec3<u32>(d) >= info.dims)) {
            continue;
        }
        let i = info.slot_base + u32(d.x) + info.dims.x * (u32(d.y) + info.dims.y * u32(d.z));
        let s = slots[i];
        if (s != SLOT_NONE) {
            return Found(true, lv, c, nn, s);
        }
    }
    return Found(false, 0u, vec3<i32>(0), vec3<i32>(0), SLOT_NONE);
}

fn brick_of(cell: vec3<i32>, n: vec3<i32>) -> u32 {
    let bc = (cell - (n << vec3<u32>(5u))) >> vec3<u32>(3u);
    return u32(bc.x + 4 * (bc.y + 4 * bc.z));
}

fn mask_bit(base: u32, b: u32) -> bool {
    return ((nodes[base + (b >> 5u)] >> (b & 31u)) & 1u) != 0u;
}

// Whether a ray entering the found cell would hit it (Scene::occupied).
fn occupied(f: Found) -> bool {
    let tag = f.slot >> TAG_SHIFT;
    if (tag == TAG_AIR) {
        return false;
    }
    if (tag == TAG_SOLID) {
        return true;
    }
    let base = (f.slot & NODE_MASK) * NODE_WORDS;
    let b = brick_of(f.cell, f.n);
    let word = nodes[base + 4u + b];
    if (mask_bit(base, b)) {
        let lc = f.cell & vec3<i32>(7);
        let v = u32(lc.x + BRICK_CELLS * (lc.y + BRICK_CELLS * lc.z));
        return ((pool[word + 1u + (v >> 5u)] >> (v & 31u)) & 1u) != 0u && shape_hit(brick_state(word, v));
    }
    return mask_bit(base + 2u, b) && shape_hit(word);
}

// The column region test at mip m (Scene::march's column search): the region's exit `t` if the
// ray clears its occupied rows across it, else -1. Clearing is monotone in m (a finer region's
// span and exit lie inside a coarser one's), so the coarsest clearing mip is found by walking
// from a guess.
struct Region {
    lo: vec3<i32>,
    hi: vec3<i32>,
    te: f32,
};

fn region(cols: u32, m: u32, local: vec3<i32>, nlo: vec3<i32>, nhi: vec3<i32>, l: u32, scale: f32) -> Region {
    let rx = u32(local.x) >> m;
    let rz = u32(local.z) >> m;
    let span = column_span(cols, m, rx, rz);
    let w = (1i << m) << l;
    let lo = vec3<i32>(nlo.x + i32(rx) * w, nlo.y, nlo.z + i32(rz) * w);
    let hi = vec3<i32>(lo.x + w, nhi.y, lo.z + w);
    let te = exit_box(lo, hi).t;
    var clear = true;
    if (span.x < span.y) {
        let y0 = rel_y(nlo.y, t) * scale;
        let y1 = rel_y(nlo.y, te) * scale;
        let ymin = min(y0, y1);
        let ymax = max(y0, y1);
        clear = ymin >= f32(span.y) + COLUMN_EPS || ymax <= f32(span.x) - COLUMN_EPS;
    }
    return Region(lo, hi, select(-1.0, te, clear));
}

fn march() -> Result {
    if ((u.flags & FLAG_DATA) == 0u) {
        return done(MISS, 0u);
    }
    let wlo = u.wlo;
    let whi = u.whi;
    let top = u.top;
    var steps = 0u;
    t = 0.0;
    ea = -1;
    ep = 0;
    blo = rc;
    bhi = rc + vec3<i32>(1);
    // Into the data's box.
    if (any(rc < wlo) || any(rc >= whi)) {
        if (any(rzero & ((rc < wlo) | (rc >= whi)))) {
            return done(MISS, 0u);
        }
        let up = rstep > vec3<i32>(0);
        let tn = tpv(select(whi, wlo, up));
        let tf = tpv(select(wlo, whi, up));
        // The latest entry (ties: the lowest axis), over the moving axes.
        let tin = select(tn, vec3<f32>(-BIG), rzero);
        var a = 0;
        if (tin.y > tin.x) {
            a = 1;
        }
        if (tin.z > tin[a]) {
            a = 2;
        }
        let t_in = tin[a];
        let t_out = min(tf.x, min(tf.y, tf.z));
        if (t_in >= t_out || t_out <= 0.0) {
            return done(MISS, 0u);
        }
        t = max(t_in, 0.0);
        ea = a;
        ep = select(whi[a], wlo[a], rstep[a] > 0);
        blo = wlo;
        bhi = whi;
    }
    let t_end = min(u.max_t, exit_box(wlo, whi).t);
    // The level marched at in the last iteration (none yet), and the column mip guess.
    var cur = -1;
    var mip_guess = 0u;
    loop {
        if (t > t_end) {
            return done(MISS, steps);
        }
        steps += 1u;
        if (steps > u.step_cap) {
            return done(CAP, steps);
        }
        let g = cell_at();
        let want = min(level_for(t), top);
        var f = find(g, want, top);
        // A LOD switch waits while the coarser cell here is occupied and the level the ray
        // marched at has data.
        if (f.ok && cur >= 0 && f.l > u32(cur) && occupied(f)) {
            let fine = find(g, u32(cur), top);
            if (fine.ok && fine.l < f.l) {
                f = fine;
            }
        }
        if (!f.ok) {
            cur = -1;
            let sh = vec3<u32>(top + 5u);
            let b = g >> sh;
            skip(b << sh, (b + vec3<i32>(1)) << sh);
            continue;
        }
        cur = i32(f.l);
        let l = f.l;
        let n = f.n;
        let cell = f.cell;
        // Above or below everything the level holds: leave its box in one skip, within the box
        // around the eye that holds the ray until the cone LOD moves past the level.
        if ((u.flags & FLAG_COLUMNS) != 0u) {
            let info = u.levels[l];
            let ls = vec3<u32>(l + 5u);
            let reach = vec3<i32>(floor(info.band_end * abs(rd)));
            let llo = max(info.origin << ls, rc - reach);
            let lhi = min((info.origin + vec3<i32>(info.dims)) << ls, rc + reach + vec3<i32>(1));
            let inside = all(g >= llo) && all(g < lhi);
            if (inside && g.y >= info.occ_hi) {
                skip(vec3<i32>(llo.x, max(info.occ_hi, llo.y), llo.z), lhi);
                continue;
            }
            if (inside && g.y < info.occ_lo) {
                skip(llo, vec3<i32>(lhi.x, min(info.occ_lo, lhi.y), lhi.z));
                continue;
            }
        }
        let node_shift = vec3<u32>(l + 5u);
        let nlo = n << node_shift;
        let nhi = (n + vec3<i32>(1)) << node_shift;
        let tag = f.slot >> TAG_SHIFT;
        if (tag == TAG_AIR) {
            skip(nlo, nhi);
            continue;
        }
        if (tag == TAG_SOLID) {
            return hit(l, cell, steps, f.slot & 0xFFFFu);
        }
        let ni = f.slot & NODE_MASK;
        let base = ni * NODE_WORDS;
        let local = cell - (n << vec3<u32>(5u));
        let bc = local >> vec3<u32>(3u);
        let b = u32(bc.x + 4 * (bc.y + 4 * bc.z));
        let brick_shift = vec3<u32>(l + 3u);
        let word = nodes[base + 4u + b];
        let mixed = mask_bit(base, b);
        if (!mixed && mask_bit(base + 2u, b)) {
            // Uniform.
            if (shape_hit(word)) {
                return hit(l, cell, steps, word);
            }
            let bb = cell >> vec3<u32>(3u);
            skip(bb << brick_shift, (bb + vec3<i32>(1)) << brick_shift);
            continue;
        }
        // The largest column region the ray clears, and where it leaves it.
        let cols = node_columns[ni];
        var column = false;
        var col: Region;
        if ((u.flags & FLAG_COLUMNS) != 0u && cols != COLUMNS_NONE) {
            let scale = bitcast<f32>((127u - l) << 23u);
            var m = min(mip_guess, COLUMN_MIPS - 1u);
            let r = region(cols, m, local, nlo, nhi, l, scale);
            if (r.te >= 0.0) {
                column = true;
                col = r;
                loop {
                    if (m + 1u >= COLUMN_MIPS) {
                        break;
                    }
                    let up = region(cols, m + 1u, local, nlo, nhi, l, scale);
                    if (up.te < 0.0) {
                        break;
                    }
                    m += 1u;
                    col = up;
                }
            } else {
                loop {
                    if (m == 0u) {
                        break;
                    }
                    m -= 1u;
                    let down = region(cols, m, local, nlo, nhi, l, scale);
                    if (down.te >= 0.0) {
                        column = true;
                        col = down;
                        break;
                    }
                }
            }
            mip_guess = m;
        }
        if (mixed) {
            if (column) {
                skip(col.lo, col.hi);
                continue;
            }
            let off = word;
            let b0 = (n << vec3<u32>(5u)) + (bc << vec3<u32>(3u));
            let b1 = b0 + vec3<i32>(BRICK_CELLS);
            let up = rstep > vec3<i32>(0);
            var c = cell;
            var occ_i = 0xFFFFFFFFu;
            var occ = 0u;
            loop {
                let lc = c - b0;
                let v = u32(lc.x + BRICK_CELLS * (lc.y + BRICK_CELLS * lc.z));
                let wi = v >> 5u;
                if (wi != occ_i) {
                    occ_i = wi;
                    occ = pool[off + 1u + wi];
                }
                if (((occ >> (v & 31u)) & 1u) != 0u) {
                    let s = brick_state(off, v);
                    if (shape_hit(s)) {
                        return hit(l, c, steps, s);
                    }
                }
                let planes = select(c, c + vec3<i32>(1), up) << vec3<u32>(l);
                let tv = tpv(planes);
                let a = nearest_axis(tv);
                let m = AXES == vec3<i32>(a);
                t = min(tv.x, min(tv.y, tv.z));
                ea = a;
                ep = select(select(planes.z, planes.y, m.y), planes.x, m.x);
                c += select(vec3<i32>(0), rstep, m);
                if (t > t_end) {
                    return done(MISS, steps);
                }
                if (any(c < b0) || any(c >= b1)) {
                    blo = c << vec3<u32>(l);
                    bhi = (c + vec3<i32>(1)) << vec3<u32>(l);
                    break;
                }
                steps += 1u;
                if (steps > u.step_cap) {
                    return done(CAP, steps);
                }
            }
            continue;
        }
        // Empty: its distance's cube of bricks, or the column region if that reaches further.
        let r = select(0, i32(max(word, 1u)) - 1, (u.flags & FLAG_DISTANCES) != 0u);
        let n4 = n << vec3<u32>(2u);
        let lo = (n4 + max(bc - vec3<i32>(r), vec3<i32>(0))) << brick_shift;
        let hi = (n4 + min(bc + vec3<i32>(r + 1), vec3<i32>(4))) << brick_shift;
        if (column && col.te > exit_box(lo, hi).t) {
            skip(col.lo, col.hi);
        } else {
            skip(lo, hi);
        }
    }
    return done(MISS, steps);
}

// The hit point relative to the eye (Hit::point): the face's plane exactly on its axis.
fn hit_point(r: Result) -> vec3<f32> {
    var p = r.t * rd;
    if (r.face < FACE_NONE) {
        let a = r.face / 2u;
        let c = select(r.cell[a] + 1, r.cell[a], (r.face & 1u) == 1u);
        p[a] = f32((c << r.level) - rc[a]) - rf[a];
    }
    return p;
}

// What the id target holds (march::cell_id): the level and cell hashed, the face (+1) below.
fn cell_id(r: Result) -> u32 {
    var h = (r.level + 1u) * 0x9E3779B9u;
    h = (h ^ bitcast<u32>(r.cell.x)) * 0x85EBCA6Bu;
    h = (h ^ bitcast<u32>(r.cell.y)) * 0xC2B2AE35u;
    h = (h ^ bitcast<u32>(r.cell.z)) * 0x27D4EB2Fu;
    h ^= h >> 15u;
    return (h << 3u) | (r.face + 1u);
}
";

/// The march's entry point: one ray per pixel, its view depth, id and hit record.
const MARCH_MAIN: &str = r"
// The hit record the shade pass reads (0: none): HIT_BIT, whether the cell is on face B, the
// level, the face and the state.
fn hit_record(r: Result) -> u32 {
    let on_b = (r.cell.y << r.level) < MIDPLANE_GRID;
    return HIT_BIT | select(0u, ON_B_BIT, on_b) | (r.level << 19u) | (r.face << 16u) | r.state;
}

@compute @workgroup_size(8, 8)
fn march_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= u.size.x || gid.y >= u.size.y) {
        return;
    }
    rd = pixel_ray(gid.xy);
    rc = u.eye_cell;
    rf = u.eye_frac;
    rzero = rd == vec3<f32>(0.0);
    rinv = select(vec3<f32>(1.0) / rd, vec3<f32>(0.0), rzero);
    rstep = select(vec3<i32>(1), vec3<i32>(-1), rd < vec3<f32>(0.0));
    let r = march();
    let at = vec2<i32>(gid.xy);
    if (r.kind == HIT) {
        let p = hit_point(r);
        textureStore(out_depth, at, vec4<f32>(dot(p, u.forward), 0.0, 0.0, 0.0));
        textureStore(out_id, at, vec4<u32>(cell_id(r), 0u, 0u, 0u));
        textureStore(out_hit, at, vec4<u32>(hit_record(r), 0u, 0u, 0u));
    } else {
        textureStore(out_depth, at, vec4<f32>(0.0));
        textureStore(out_id, at, vec4<u32>(0u));
        textureStore(out_hit, at, vec4<u32>(0u));
    }
    if ((u.flags & FLAG_HISTOGRAM) != 0u) {
        let bin = select(min(r.steps, HIST_BINS - 1u), HIST_CAPPED, r.kind == CAP);
        atomicAdd(&stats[bin], 1u);
        atomicAdd(&stats[select(HIST_MISSES, HIST_HITS, r.kind == HIT)], 1u);
    }
    if ((u.flags & FLAG_DEBUG) != 0u) {
        let o = DEBUG_BASE + (gid.x + gid.y * u.size.x) * DEBUG_WORDS;
        atomicStore(&stats[o], r.kind);
        atomicStore(&stats[o + 1u], r.steps);
        atomicStore(&stats[o + 2u], r.level);
        atomicStore(&stats[o + 3u], bitcast<u32>(r.cell.x));
        atomicStore(&stats[o + 4u], bitcast<u32>(r.cell.y));
        atomicStore(&stats[o + 5u], bitcast<u32>(r.cell.z));
        atomicStore(&stats[o + 6u], r.face);
        atomicStore(&stats[o + 7u], bitcast<u32>(r.t));
        atomicStore(&stats[o + 8u], r.state);
        atomicStore(&stats[o + 9u], bitcast<u32>(rd.x));
        atomicStore(&stats[o + 10u], bitcast<u32>(rd.y));
        atomicStore(&stats[o + 11u], bitcast<u32>(rd.z));
    }
}
";

/// Shading the hits, in a pass of its own so that the march keeps few registers. It uses the
/// shared lighting and the meshers' colours: the chunk mesher's raw colour × face tint × atlas
/// texel at level 0, and the LOD mesher's linear flat colour × face tint above. Reads the march's
/// hit records and view depth; writes the lit linear colour (alpha 1 on a hit, 0 for the sky).
const SHADE: &str = r"
@group(2) @binding(0) var out_color: texture_storage_2d<rgba16float, write>;
@group(2) @binding(1) var hit_in: texture_2d<u32>;
@group(2) @binding(2) var depth_in: texture_2d<f32>;

fn tint_of(lf: u32) -> vec3<f32> {
    switch lf {
        case 2u: { return vec3<f32>(TINT_TOP_R, TINT_TOP_G, TINT_TOP_B); }
        case 3u: { return vec3<f32>(TINT_BOTTOM_R, TINT_BOTTOM_G, TINT_BOTTOM_B); }
        case 0u, 1u: { return vec3<f32>(TINT_X_R, TINT_X_G, TINT_X_B); }
        default: { return vec3<f32>(TINT_Z_R, TINT_Z_G, TINT_Z_B); }
    }
}

fn raw_rgb(c: u32) -> vec3<f32> {
    return vec3<f32>(f32((c >> 16u) & 0xFFu), f32((c >> 8u) & 0xFFu), f32(c & 0xFFu)) / 255.0;
}

fn linear_rgb(c: u32) -> vec3<f32> {
    let v = raw_rgb(c);
    return select(pow((v + 0.055) / 1.055, vec3<f32>(2.4)), v / 12.92, v <= vec3<f32>(0.04045));
}

// One hit's colour: `h` its record, `rd` its ray, `t` its distance.
fn shade(h: u32, rd: vec3<f32>, t: f32) -> vec4<f32> {
    let state = h & 0xFFFFu;
    let face = (h >> 16u) & 7u;
    let level = (h >> 19u) & 31u;
    let on_b = (h & ON_B_BIT) != 0u;
    let p = t * rd;
    var n = -rd;
    if (face < FACE_NONE) {
        n = vec3<f32>(0.0);
        n[face / 2u] = select(-1.0, 1.0, (face & 1u) == 0u);
    }
    // The face the player's up sees (march::face_up): world -Y is the top on face B.
    var lf = select(face, 2u, face == FACE_NONE);
    if (on_b && (lf == 2u || lf == 3u)) {
        lf = 5u - lf;
    }
    let group = select(1u, select(2u, 0u, lf == 2u), lf == 2u || lf == 3u);
    let look = state * LOOK_WORDS;
    var albedo: vec3<f32>;
    if (level == 0u) {
        let tile = (looks[look + 2u] >> (8u * group)) & 0xFFu;
        // Position in the cell, 0..1 per axis; the mesher's uv in blocks (x/z on tops, z/y on x
        // sides, x/y on z sides), mirrored up the sides on face B.
        let q = u.eye_frac + p;
        let a = select(face / 2u, 1u, face == FACE_NONE);
        var st: vec2<f32>;
        if (a == 1u) {
            st = vec2<f32>(q.x, q.z);
        } else if (a == 0u) {
            st = vec2<f32>(q.z, q.y);
        } else {
            st = vec2<f32>(q.x, q.y);
        }
        if (on_b && a != 1u) {
            st.y = 1.0 - st.y;
        }
        let cell_xy = vec2<f32>(f32(tile % ATLAS_CELLS), f32(tile / ATLAS_CELLS));
        let origin = (cell_xy * ATLAS_CELL + ATLAS_GUTTER) / ATLAS_SIZE;
        let uv = origin + fract(st) * (TILE_TEXELS / ATLAS_SIZE);
        // The ray cone's footprint in texels, widened by the slant.
        let slant = max(abs(dot(n, rd)), 0.1);
        let texels = t * u.pixel_angle * TILE_TEXELS / slant;
        let mip = max(log2(max(texels, 1e-6)), 0.0);
        let texel = textureSampleLevel(atlas, atlas_sampler, uv, mip);
        albedo = raw_rgb(looks[look + 1u]) * tint_of(lf) * texel.rgb;
    } else {
        albedo = linear_rgb(looks[look + 3u + group]) * tint_of(lf);
    }
    return vec4<f32>(lambert(albedo, n, frame.eye.x + p.y), 1.0);
}

@compute @workgroup_size(8, 8)
fn shade_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= u.size.x || gid.y >= u.size.y) {
        return;
    }
    let at = vec2<i32>(gid.xy);
    let h = textureLoad(hit_in, at, 0).r;
    if (h == 0u) {
        textureStore(out_color, at, vec4<f32>(0.0));
        return;
    }
    let rd = pixel_ray(gid.xy);
    let t = textureLoad(depth_in, at, 0).r / dot(rd, u.forward);
    textureStore(out_color, at, shade(h, rd, t));
}
";

/// The march's compute module (`march_main`).
pub fn source() -> String {
    format!(
        "{}\n{UNIFORMS}\n{MARCH_BINDINGS}\n{MARCH}\n{MARCH_MAIN}",
        constant_block()
    )
}

/// The shade pass's compute module (`shade_main`).
pub fn shade_source() -> String {
    format!(
        "{}\n{}\n{UNIFORMS}\n{SHADE}",
        constant_block(),
        super::super::shaders::lighting_source()
    )
}

/// `cell_id` in the shader, on the CPU: the id target's value for a hit (0 for none).
pub fn cell_id(level: u32, cell: [i32; 3], face: Option<usize>) -> u32 {
    let face = face.map_or(FACE_NONE, |f| f as u32);
    let mut h = (level + 1).wrapping_mul(0x9E37_79B9);
    h = (h ^ cell[0] as u32).wrapping_mul(0x85EB_CA6B);
    h = (h ^ cell[1] as u32).wrapping_mul(0xC2B2_AE35);
    h = (h ^ cell[2] as u32).wrapping_mul(0x27D4_EB2F);
    h ^= h >> 15;
    (h << 3) | (face + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module() -> naga::Module {
        parse(&source())
    }

    fn parse(src: &str) -> naga::Module {
        let src = src.to_string();
        let module = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| {
            panic!("WGSL parses: {}", e.emit_to_string(&src));
        });
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
        module
    }

    #[test]
    fn the_marcher_module_parses_and_validates() {
        let m = module();
        assert!(m.entry_points.iter().any(|e| e.name == "march_main"));
        let shade = parse(&shade_source());
        assert!(shade.entry_points.iter().any(|e| e.name == "shade_main"));
    }

    #[test]
    fn every_wgsl_constant_equals_its_rust_source() {
        let m = module();
        let value = |name: &str| -> Value {
            let (_, c) = m
                .constants
                .iter()
                .find(|(_, c)| c.name.as_deref() == Some(name))
                .unwrap_or_else(|| panic!("{name} in the module"));
            match m.global_expressions[c.init] {
                naga::Expression::Literal(naga::Literal::U32(v)) => Value::U32(v),
                naga::Expression::Literal(naga::Literal::I32(v)) => Value::I32(v),
                naga::Expression::Literal(naga::Literal::F32(v)) => Value::F32(v),
                ref e => panic!("{name}: {e:?}"),
            }
        };
        let all = constants();
        assert!(all.len() > 30);
        for (name, v) in all {
            assert_eq!(value(name), v, "{name}");
        }
        // The column mips' offsets, from the generated switch.
        let src = source();
        for (m, off) in MIP_OFFSET.iter().enumerate() {
            assert!(src.contains(&format!("case {m}u: {{ return {off}u; }}")));
        }
    }

    #[test]
    fn the_uniform_struct_is_the_size_its_writer_fills() {
        let m = module();
        let (_, ty) = m
            .types
            .iter()
            .find(|(_, t)| t.name.as_deref() == Some("MarchUniforms"))
            .expect("MarchUniforms");
        let mut layouter = naga::proc::Layouter::default();
        layouter.update(m.to_ctx()).expect("layout");
        let size = ty.inner.size(m.to_ctx());
        assert_eq!(size as usize, 4 * super::super::MARCH_UNIFORM_WORDS);
        let (h, _) = m
            .types
            .iter()
            .find(|(_, t)| t.name.as_deref() == Some("LevelInfo"))
            .expect("LevelInfo");
        assert_eq!(
            layouter[h].size as usize,
            4 * dwell_march::layout::LEVEL_WORDS
        );
    }

    #[test]
    fn cell_ids_tell_neighbours_and_faces_apart_and_are_never_zero() {
        let a = cell_id(0, [100, 200, 300], Some(2));
        assert_ne!(a, 0);
        assert_ne!(a, cell_id(0, [101, 200, 300], Some(2)));
        assert_ne!(a, cell_id(1, [100, 200, 300], Some(2)));
        assert_ne!(a, cell_id(0, [100, 200, 300], Some(3)));
        assert_eq!(a & 7, 3);
        assert_eq!(cell_id(0, [0, 0, 0], None) & 7, FACE_NONE + 1);
    }
}
