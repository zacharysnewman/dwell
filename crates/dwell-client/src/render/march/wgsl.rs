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

/// The uniforms (group 1, binding 0), 272 words; `MarchUniforms::to_words` writes them.
const UNIFORMS: &str = r"
struct LevelInfo {
    origin: vec3<i32>,
    dims: vec3<u32>,
    slot_base: u32,
    cell_log2: u32,
};
struct MarchUniforms {
    levels: array<LevelInfo, 20>,
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
    _pad: u32,
};
@group(1) @binding(0) var<uniform> u: MarchUniforms;
@group(1) @binding(1) var<storage, read> slots: array<u32>;
@group(1) @binding(2) var<storage, read> nodes: array<u32>;
@group(1) @binding(3) var<storage, read> node_columns: array<u32>;
@group(1) @binding(4) var<storage, read> columns: array<u32>;
@group(1) @binding(5) var<storage, read> pool: array<u32>;
@group(1) @binding(6) var<storage, read> looks: array<u32>;
@group(1) @binding(7) var<storage, read_write> stats: array<atomic<u32>>;

@group(2) @binding(0) var out_color: texture_storage_2d<rgba16float, write>;
@group(2) @binding(1) var out_depth: texture_storage_2d<r32float, write>;
@group(2) @binding(2) var out_id: texture_storage_2d<r32uint, write>;
";

/// The traversal: `march.rs` line for line.
const MARCH: &str = r"
// The ray (RayMath): cell, fraction, unit direction, its reciprocal and step signs.
var<private> rc: vec3<i32>;
var<private> rf: vec3<f32>;
var<private> rd: vec3<f32>;
var<private> rinv: vec3<f32>;
var<private> rstep: vec3<i32>;
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

struct Result {
    kind: u32,
    steps: u32,
    level: u32,
    cell: vec3<i32>,
    face: u32,
    t: f32,
    state: u32,
};

// Distance to the plane `grid = b` on axis k.
fn tp(k: u32, b: i32) -> f32 {
    if (rd[k] == 0.0) {
        return BIG;
    }
    return (f32(b - rc[k]) - rf[k]) * rinv[k];
}

struct Exit {
    t: f32,
    a: u32,
};

// Where the ray leaves box [lo, hi) (ties: the lowest axis).
fn exit_box(lo: vec3<i32>, hi: vec3<i32>) -> Exit {
    var best = Exit(BIG, 0u);
    for (var k = 0u; k < 3u; k++) {
        let x = tp(k, select(lo[k], hi[k], rstep[k] > 0));
        if (x < best.t) {
            best = Exit(x, k);
        }
    }
    return best;
}

// Leaves box [lo, hi) and continues beyond its exit plane (the skip! macro).
fn skip(lo: vec3<i32>, hi: vec3<i32>) {
    let e = exit_box(lo, hi);
    let a = e.a;
    var nlo = lo;
    var nhi = hi;
    var plane: i32;
    if (rstep[a] > 0) {
        plane = hi[a];
        nlo[a] = plane;
        nhi[a] = plane + 1;
    } else {
        plane = lo[a];
        nlo[a] = plane - 1;
        nhi[a] = plane;
    }
    t = e.t;
    ea = i32(a);
    ep = plane;
    blo = nlo;
    bhi = nhi;
}

fn crossed(x: f32, strict: bool) -> bool {
    return x < t || (x == t && !strict);
}

// The grid cell the ray is in at t inside [blo, bhi) (RayMath::cell_at).
fn cell_at() -> vec3<i32> {
    var g: vec3<i32>;
    for (var k = 0u; k < 3u; k++) {
        var v: i32;
        if (ea == i32(k)) {
            v = select(ep - 1, ep, rstep[k] > 0);
        } else if (rd[k] == 0.0) {
            v = rc[k];
        } else {
            let strict = ea < 0 || i32(k) > ea;
            v = rc[k] + i32(floor(rf[k] + t * rd[k]));
            let next = select(0, 1, rstep[k] > 0);
            let enter = select(1, 0, rstep[k] > 0);
            if (crossed(tp(k, v + next), strict)) {
                v += rstep[k];
            } else if (!crossed(tp(k, v + enter), strict) && tp(k, v + enter) > 0.0) {
                v -= rstep[k];
            }
        }
        g[k] = clamp(v, blo[k], bhi[k] - 1);
    }
    return g;
}

fn rel(k: u32, base: i32, at: f32) -> f32 {
    return f32(rc[k] - base) + rf[k] + at * rd[k];
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

fn mask_bit(base: u32, b: u32) -> bool {
    return ((nodes[base + (b >> 5u)] >> (b & 31u)) & 1u) != 0u;
}

fn brick_occupied(off: u32, v: u32) -> bool {
    return ((pool[off + 1u + (v >> 5u)] >> (v & 31u)) & 1u) != 0u;
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
        var t_in = -BIG;
        var t_in_k = 0u;
        var t_out = BIG;
        for (var k = 0u; k < 3u; k++) {
            if (rd[k] == 0.0) {
                if (rc[k] < wlo[k] || rc[k] >= whi[k]) {
                    return done(MISS, 0u);
                }
                continue;
            }
            let near = select(whi[k], wlo[k], rstep[k] > 0);
            let far = select(wlo[k], whi[k], rstep[k] > 0);
            let tn = tp(k, near);
            if (tn > t_in) {
                t_in = tn;
                t_in_k = k;
            }
            t_out = min(t_out, tp(k, far));
        }
        if (t_in >= t_out || t_out <= 0.0) {
            return done(MISS, 0u);
        }
        let a = t_in_k;
        t = max(t_in, 0.0);
        ea = i32(a);
        ep = select(whi[a], wlo[a], rstep[a] > 0);
        blo = wlo;
        bhi = whi;
    }
    let t_end = min(u.max_t, exit_box(wlo, whi).t);
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
        var found = false;
        var l = 0u;
        var cell = vec3<i32>(0);
        var n = vec3<i32>(0);
        var slot = SLOT_NONE;
        for (var lv = want; lv <= top; lv++) {
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
                found = true;
                l = lv;
                cell = c;
                n = nn;
                slot = s;
                break;
            }
        }
        if (!found) {
            let sh = vec3<u32>(top + 5u);
            let b = g >> sh;
            skip(b << sh, (b + vec3<i32>(1)) << sh);
            continue;
        }
        let node_shift = vec3<u32>(l + 5u);
        let nlo = n << node_shift;
        let nhi = (n + vec3<i32>(1)) << node_shift;
        let tag = slot >> TAG_SHIFT;
        if (tag == TAG_AIR) {
            skip(nlo, nhi);
            continue;
        }
        if (tag == TAG_SOLID) {
            return hit(l, cell, steps, slot & 0xFFFFu);
        }
        let ni = slot & NODE_MASK;
        let base = ni * NODE_WORDS;
        let local = cell - (n << vec3<u32>(5u));
        let cols = node_columns[ni];
        // The largest column region the ray clears, and where it leaves it.
        var column = false;
        var col_lo = vec3<i32>(0);
        var col_hi = vec3<i32>(0);
        var col_te = 0.0;
        if ((u.flags & FLAG_COLUMNS) != 0u && cols != COLUMNS_NONE) {
            let scale = bitcast<f32>((127u - l) << 23u);
            for (var mm = 0u; mm < COLUMN_MIPS; mm++) {
                let m = COLUMN_MIPS - 1u - mm;
                let rx = u32(local.x) >> m;
                let rz = u32(local.z) >> m;
                let span = column_span(cols, m, rx, rz);
                let w = (1i << m) << l;
                let lo = vec3<i32>(nlo.x + i32(rx) * w, nlo.y, nlo.z + i32(rz) * w);
                let hi = vec3<i32>(lo.x + w, nhi.y, lo.z + w);
                let te = exit_box(lo, hi).t;
                if (span.x < span.y) {
                    let y0 = rel(1u, nlo.y, t) * scale;
                    let y1 = rel(1u, nlo.y, te) * scale;
                    let ymin = min(y0, y1);
                    let ymax = max(y0, y1);
                    let clear = ymin >= f32(span.y) + COLUMN_EPS || ymax <= f32(span.x) - COLUMN_EPS;
                    if (!clear) {
                        continue;
                    }
                }
                column = true;
                col_lo = lo;
                col_hi = hi;
                col_te = te;
                break;
            }
        }
        let bc = local >> vec3<u32>(3u);
        let b = u32(bc.x + 4 * (bc.y + 4 * bc.z));
        let brick_shift = vec3<u32>(l + 3u);
        let word = nodes[base + 4u + b];
        if (mask_bit(base, b)) {
            // Mixed.
            if (column) {
                skip(col_lo, col_hi);
                continue;
            }
            let off = word;
            let b0 = (n << vec3<u32>(5u)) + (bc << vec3<u32>(3u));
            var c = cell;
            loop {
                let lc = c - b0;
                let v = u32(lc.x + BRICK_CELLS * (lc.y + BRICK_CELLS * lc.z));
                if (brick_occupied(off, v)) {
                    let s = brick_state(off, v);
                    if (shape_hit(s)) {
                        return hit(l, c, steps, s);
                    }
                }
                let clo = c << vec3<u32>(l);
                let chi = (c + vec3<i32>(1)) << vec3<u32>(l);
                let e = exit_box(clo, chi);
                let a = e.a;
                t = e.t;
                ea = i32(a);
                ep = select(clo[a], chi[a], rstep[a] > 0);
                c[a] += rstep[a];
                if (t > t_end) {
                    return done(MISS, steps);
                }
                if (c[a] < b0[a] || c[a] >= b0[a] + BRICK_CELLS) {
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
        if (mask_bit(base + 2u, b)) {
            // Uniform.
            if (shape_hit(word)) {
                return hit(l, cell, steps, word);
            }
            let bb = cell >> vec3<u32>(3u);
            skip(bb << brick_shift, (bb + vec3<i32>(1)) << brick_shift);
            continue;
        }
        // Empty: its distance's cube of bricks, or the column region if that reaches further.
        let r = select(0, i32(max(word, 1u)) - 1, (u.flags & FLAG_DISTANCES) != 0u);
        let n4 = n << vec3<u32>(2u);
        let lo = (n4 + max(bc - vec3<i32>(r), vec3<i32>(0))) << brick_shift;
        let hi = (n4 + min(bc + vec3<i32>(r + 1), vec3<i32>(4))) << brick_shift;
        if (column && col_te > exit_box(lo, hi).t) {
            skip(col_lo, col_hi);
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

/// Shading the hit with the shared lighting (the mesher's colours: the chunk mesher's raw colour ×
/// face tint × atlas texel at level 0, the LOD mesher's linear flat colour × face tint above).
const SHADE: &str = r"
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

fn shade(r: Result) -> vec4<f32> {
    let p = hit_point(r);
    var n = -rd;
    if (r.face < FACE_NONE) {
        n = vec3<f32>(0.0);
        n[r.face / 2u] = select(-1.0, 1.0, (r.face & 1u) == 0u);
    }
    // The face the player's up sees (march::face_up): world -Y is the top on face B.
    let on_b = (r.cell.y << r.level) < MIDPLANE_GRID;
    var lf = select(r.face, 2u, r.face == FACE_NONE);
    if (on_b && (lf == 2u || lf == 3u)) {
        lf = 5u - lf;
    }
    let group = select(1u, select(2u, 0u, lf == 2u), lf == 2u || lf == 3u);
    let look = r.state * LOOK_WORDS;
    var albedo: vec3<f32>;
    if (r.level == 0u) {
        let tile = (looks[look + 2u] >> (8u * group)) & 0xFFu;
        // Position in the cell, 0..1 per axis; the mesher's uv in blocks (x/z on tops, z/y on x
        // sides, x/y on z sides), mirrored up the sides on face B.
        let q = vec3<f32>(rc - r.cell) + rf + p;
        let a = select(r.face / 2u, 1u, r.face == FACE_NONE);
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
        let texels = r.t * u.pixel_angle * TILE_TEXELS / slant;
        let mip = max(log2(max(texels, 1e-6)), 0.0);
        let texel = textureSampleLevel(atlas, atlas_sampler, uv, mip);
        albedo = raw_rgb(looks[look + 1u]) * tint_of(lf) * texel.rgb;
    } else {
        albedo = linear_rgb(looks[look + 3u + group]) * tint_of(lf);
    }
    return vec4<f32>(lambert(albedo, n, frame.eye.x + p.y), 1.0);
}

@compute @workgroup_size(8, 8)
fn march_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= u.size.x || gid.y >= u.size.y) {
        return;
    }
    let size = vec2<f32>(u.size);
    let ndc = vec2<f32>(
        (f32(gid.x) + 0.5) / size.x * 2.0 - 1.0,
        1.0 - (f32(gid.y) + 0.5) / size.y * 2.0,
    );
    let ray = u.forward + ndc.x * u.tan_x * u.right + ndc.y * u.tan_y * u.up;
    rd = normalize(ray);
    rc = u.eye_cell;
    rf = u.eye_frac;
    rinv = select(vec3<f32>(1.0) / rd, vec3<f32>(0.0), rd == vec3<f32>(0.0));
    rstep = select(vec3<i32>(1), vec3<i32>(-1), rd < vec3<f32>(0.0));
    let r = march();
    let at = vec2<i32>(gid.xy);
    if (r.kind == HIT) {
        let p = hit_point(r);
        textureStore(out_color, at, shade(r));
        textureStore(out_depth, at, vec4<f32>(dot(p, u.forward), 0.0, 0.0, 0.0));
        textureStore(out_id, at, vec4<u32>(cell_id(r), 0u, 0u, 0u));
    } else {
        textureStore(out_color, at, vec4<f32>(0.0));
        textureStore(out_depth, at, vec4<f32>(0.0));
        textureStore(out_id, at, vec4<u32>(0u));
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

/// The whole compute module.
pub fn source() -> String {
    format!(
        "{}\n{}\n{UNIFORMS}\n{MARCH}\n{SHADE}",
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
        let src = source();
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
