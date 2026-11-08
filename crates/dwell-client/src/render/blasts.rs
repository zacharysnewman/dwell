//! A blast's billboards (docs/EXPLOSIVES.md §4; crate::blasts): smoke and dust puffs, lit over what
//! is behind them, and the flash's fireball, self-lit and added; through the flames' billboard
//! pass. Their colours are the explosives' data.

use super::camera::Basis;
use super::flames::{VERTEX_FLOATS, quad};
use crate::blasts::{Puff, PuffKind};
use dwell_world::explosives::EXPLOSIVES;

/// A smoke puff (lit, over what is behind).
pub const KIND_SMOKE: u32 = 8;
/// A dust puff (lit, over what is behind).
pub const KIND_DUST: u32 = 9;
/// The fireball (self-lit, added).
pub const KIND_FIRE: u32 = 10;
/// A wood chip off an axe's swing (crate::chips; lit, its wood's colour).
pub const KIND_CHIP: u32 = 11;
/// A gouge on a cut wood cell (crate::chips; lit, its wood's colour): a notch's wedge or a kerf.
pub const KIND_GOUGE: u32 = 12;

/// The puffs' quads, farthest first (blended over one another), seen with `basis`; and how many.
pub fn blast_quads(puffs: &[Puff], eye: [f64; 3], basis: &Basis) -> (Vec<f32>, usize) {
    let mut far: Vec<(f64, &Puff)> = puffs
        .iter()
        .map(|p| {
            let d = (0..3).map(|k| (p.at[k] - eye[k]).powi(2)).sum::<f64>();
            (d, p)
        })
        .collect();
    far.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut out = Vec::with_capacity(far.len() * 6 * VERTEX_FLOATS);
    for (_, p) in &far {
        let c: [f64; 3] = std::array::from_fn(|k| p.at[k] - eye[k]);
        let kind = match p.kind {
            PuffKind::Smoke => KIND_SMOKE,
            PuffKind::Dust => KIND_DUST,
            PuffKind::Fire => KIND_FIRE,
            PuffKind::Chip | PuffKind::Leaf => KIND_CHIP,
            PuffKind::Gouge { u, v, kerf, bits } => {
                // On the cell's side, spanning it: uv −1 to 1 (y along the grain); rest x the
                // depth, z 1 for a kerf's slit.
                quad(
                    &mut out,
                    |x, y| {
                        std::array::from_fn(|k| {
                            c[k] + u[k] * (2.0 * x - 1.0) + v[k] * (2.0 * y - 1.0)
                        })
                    },
                    |x, y| [(2.0 * x - 1.0) as f32, (2.0 * y - 1.0) as f32],
                    [p.size as f32, p.alpha as f32, f32::from(u8::from(kerf))],
                    (p.explosive as u32 & 0xff) | KIND_GOUGE << 8 | (bits & 0xffff) << 16,
                    p.seed,
                );
                continue;
            }
        };
        // A chip's rest x: 1 for a leaf (its wood's `leaves` colour, a rounder flutter).
        let leaf = f32::from(u8::from(p.kind == PuffKind::Leaf));
        let size = p.size;
        quad(
            &mut out,
            |x, y| {
                std::array::from_fn(|k| {
                    c[k] + (basis.right[k] * (2.0 * x - 1.0) + basis.up[k] * (2.0 * y - 1.0)) * size
                })
            },
            |x, y| [(2.0 * x - 1.0) as f32, (2.0 * y - 1.0) as f32],
            [leaf, p.alpha as f32, 0.0],
            (p.explosive as u32 & 0xff) | kind << 8,
            p.seed,
        );
    }
    (out, far.len())
}

/// The puffs' shading in WGSL: `blast_colour(kind, look, uv, rest, rel, seed)`, premultiplied; the
/// fireball's alpha 0 (added).
pub fn wgsl() -> String {
    let lin = |c: [f32; 3]| -> String {
        let [r, g, b] = c.map(|v| dwell_look::srgb_to_linear(f64::from(v)) as f32);
        format!("vec3f({r:?}, {g:?}, {b:?})")
    };
    let list = |f: &dyn Fn(&dwell_world::explosives::ExplosiveData) -> String| {
        let v: Vec<String> = EXPLOSIVES.iter().map(f).collect();
        if v.is_empty() {
            "vec3f(1.0)".to_string()
        } else {
            v.join(", ")
        }
    };
    let woods: Vec<String> = dwell_protocol::chopping::WOODS
        .iter()
        .map(|w| lin(w.chips.color.map(|c| f32::from(c) / 255.0)))
        .collect();
    let nw = woods.len().max(1);
    let leaves: Vec<String> = dwell_protocol::chopping::WOODS
        .iter()
        .map(|w| lin(w.leaves.color.map(|c| f32::from(c) / 255.0)))
        .collect();
    let leaves = if leaves.is_empty() {
        "vec3f(1.0)".to_string()
    } else {
        leaves.join(", ")
    };
    let woods = if woods.is_empty() {
        "vec3f(1.0)".to_string()
    } else {
        woods.join(", ")
    };
    let smokes = list(&|e| lin(e.effects.smoke.color));
    let dusts = list(&|e| lin(e.effects.dust.color));
    let fires = list(&|e| lin(e.effects.flash.color));
    let n = EXPLOSIVES.len().max(1);
    format!(
        "const BLAST_SMOKES = array<vec3f, {n}>({smokes});
const BLAST_DUSTS = array<vec3f, {n}>({dusts});
const BLAST_FIRES = array<vec3f, {n}>({fires});
const CHIP_WOODS = array<vec3f, {nw}>({woods});
const CHIP_LEAVES = array<vec3f, {nw}>({leaves});
fn blast_hash(p: vec2f) -> f32 {{
    return fract(sin(dot(p, vec2f(269.5, 183.3))) * 43758.5453);
}}
fn blast_noise(p: vec2f) -> f32 {{
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(blast_hash(i), blast_hash(i + vec2f(1.0, 0.0)), u.x),
        mix(blast_hash(i + vec2f(0.0, 1.0)), blast_hash(i + vec2f(1.0, 1.0)), u.x), u.y);
}}
// A blast's puff (render/blasts.rs): kind 8 smoke, 9 dust (lit, over what is behind), 10 the
// fireball (self-lit, added: alpha 0). uv −1 to 1; rest y its opacity; a ragged edge from its seed.
fn blast_colour(kind: u32, look: u32, uv: vec2f, rest: vec3f, rel: vec3f, seed: u32, bits: u32) -> vec4f {{
    if (kind >= 11u) {{
        return chip_colour(kind, min(look, {last_wood}u), uv, rest, rel, seed, bits);
    }}
    let k = min(look, {last}u);
    let s = vec2f(f32(seed & 1023u), f32((seed >> 10u) & 1023u)) * 0.37;
    let edge = 0.65 + 0.35 * blast_noise(uv * 2.2 + s);
    let a = (1.0 - smoothstep(0.35 * edge, edge, length(uv))) * rest.y;
    if (a < 0.01) {{
        return vec4f(0.0);
    }}
    if (kind == 10u) {{
        let core = 1.0 - smoothstep(0.0, 0.7, length(uv));
        let c = mix(BLAST_FIRES[k], vec3f(1.0, 0.97, 0.85), core) * (1.6 * a);
        return vec4f(flame_light(c), 0.0);
    }}
    var c = BLAST_SMOKES[k];
    if (kind == 9u) {{
        c = BLAST_DUSTS[k];
    }}
    let y = frame.eye.x + rel.y;
    let up = face_up(face_at(y));
    var l = terrain_light(1.0, vec2f(1.0, 0.0));
    l.direct = shadow_light(rel, up, l.direct);
    let lit = lambert_lit(c * (0.85 + 0.3 * blast_noise(uv * 3.0 + s)), up, y, l);
    return vec4f(lit * a, a);
}}
// An axe's chip (kind 11: a flat splinter, turned by its seed, tumbling; rest x 1: a leaf, rounder
// and green) or a gouge on a cut wood cell (kind 12: rest x its depth 0-1). A notch is a V cut into
// the side: its two cut faces lit by their own normals (`bits`: the side's normal and the grain's
// way, chips.rs `Gouge::bits`), so the face toward the sky is bright and the one under it in
// shade, with fresh end grain, a dark crease at its apex the deeper it goes and a bark lip at its
// rim; a kerf is a dark slit. Fresh wood keeps its tan hue in shade (`wood_lit`: the light's
// brightness on the wood's own colour, not the sky's blue on tan, which read grey-olive).
fn gouge_axis(a: u32) -> vec3f {{
    return select(select(vec3f(0.0, 0.0, 1.0), vec3f(0.0, 1.0, 0.0), a == 1u), vec3f(1.0, 0.0, 0.0), a == 0u);
}}
fn wood_lit(c: vec3f, n: vec3f, rel: vec3f, y: f32) -> vec3f {{
    let up = face_up(face_at(y));
    var l = terrain_light(1.0, vec2f(1.0, 0.0));
    l.direct = shadow_light(rel, n, l.direct);
    let lit = lambert_lit(c, n, y, l);
    let w = vec3f(0.2126, 0.7152, 0.0722);
    let own = c * (dot(lit, w) / max(dot(c, w), 1e-4));
    return mix(lit, own, 0.75);
}}
fn chip_colour(kind: u32, k: u32, uv: vec2f, rest: vec3f, rel: vec3f, seed: u32, bits: u32) -> vec4f {{
    let fresh = CHIP_WOODS[k];
    let y = frame.eye.x + rel.y;
    let up = face_up(face_at(y));
    var c = fresh;
    var a = rest.y;
    var n = up;
    if (kind == 11u) {{
        let ang = f32(seed & 255u) * 0.0246;
        let r = vec2f(cos(ang) * uv.x - sin(ang) * uv.y, sin(ang) * uv.x + cos(ang) * uv.y);
        if (rest.x > 0.5) {{
            // A leaf: an oval with a pointed tip and a midrib.
            let e = r.x * r.x / 0.85 + r.y * r.y / (0.3 * (1.0 - 0.5 * r.x));
            if (e > 1.0) {{
                return vec4f(0.0);
            }}
            c = CHIP_LEAVES[k] * (0.8 + 0.35 * blast_hash(vec2f(f32(seed & 1023u), 5.0))) * select(1.0, 0.8, abs(r.y) < 0.05);
        }} else {{
            if (abs(r.x) > 0.95 || abs(r.y) > 0.45 + 0.15 * blast_hash(vec2f(f32(seed & 63u), r.x))) {{
                return vec4f(0.0);
            }}
            // A splinter: fresh wood, a fibre or two along it.
            let fibre = 0.9 + 0.12 * sin(r.y * 31.0 + f32(seed & 15u));
            c = fresh * fibre * (0.92 + 0.16 * blast_hash(vec2f(f32(seed & 1023u), 3.0)));
        }}
    }} else {{
        let fi = bits & 7u;
        let side = gouge_axis(fi / 2u) * select(-1.0, 1.0, (fi & 1u) == 1u);
        let along = gouge_axis((bits >> 3u) & 3u) * select(1.0, -1.0, ((bits >> 5u) & 1u) == 1u);
        n = side;
        if (rest.z > 0.5) {{
            // The slit: dark at its middle, fading to the face at its lips.
            let lip = 1.0 - abs(uv.y);
            c = fresh * mix(0.5, 0.1, rest.x);
            a = a * smoothstep(0.0, 0.5, lip) * (1.0 - smoothstep(0.85, 1.0, abs(uv.x)));
        }} else {{
            // The V: each half is a cut face sloping in to the apex along the middle (the notch
            // opens 1.25× its depth each way), lit by its own normal.
            let toward = select(-1.0, 1.0, uv.y > 0.0);
            n = normalize(side * 1.25 - along * toward);
            let into = 1.0 - abs(uv.y);
            // End grain: the growth rings' arcs across the cut, and fine fibre.
            let s = f32(seed & 255u);
            let rings = 0.93 + 0.07 * sin(length(vec2f(uv.x * 4.0 + s * 0.1, into * 1.5 + 3.0)) * 9.0);
            let fibre = 0.95 + 0.08 * blast_noise(vec2f(uv.x * 14.0, uv.y * 3.0) + vec2f(s));
            // The crease at the apex, darker the deeper; the bark's lip at the rim.
            let crease = 1.0 - 0.5 * smoothstep(0.82, 1.0, into) * clamp(rest.x * 4.0, 0.35, 1.0);
            let rim = smoothstep(0.86, 1.0, max(abs(uv.y), abs(uv.x)));
            c = mix(fresh * rings * fibre * crease, fresh * 0.42, rim * 0.6);
            a = a * (1.0 - smoothstep(0.94, 1.0, abs(uv.x))) * (1.0 - smoothstep(0.95, 1.0, abs(uv.y)));
        }}
    }}
    if (a < 0.01) {{
        return vec4f(0.0);
    }}
    return vec4f(wood_lit(c, n, rel, y) * a, a);
}}
",
        last = n - 1,
        last_wood = nw - 1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn puffs_are_quads_farthest_first_with_their_kind() {
        let basis = Basis {
            right: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            back: [0.0, 0.0, 1.0],
        };
        let p = |x: f64, kind| Puff {
            at: [x, 0.0, 0.0],
            size: 1.0,
            alpha: 0.5,
            kind,
            explosive: 0,
            seed: 1,
        };
        let (v, n) = blast_quads(
            &[p(1.0, PuffKind::Smoke), p(5.0, PuffKind::Fire)],
            [0.0; 3],
            &basis,
        );
        assert_eq!(n, 2);
        assert_eq!(v.len(), 2 * 6 * VERTEX_FLOATS);
        // The first quad is the farther (the fireball).
        assert_eq!(v[8].to_bits() >> 8, KIND_FIRE);
        assert_eq!(v[6 * VERTEX_FLOATS + 8].to_bits() >> 8, KIND_SMOKE);
    }

    #[test]
    fn a_gouge_is_a_quad_flat_on_its_cells_side_not_a_billboard() {
        // face-a-only: the quad's corners are the gouge's own (crate::chips tests both faces).
        // A notch's wedge on a −x face at x = 3: spanning z ±0.44 and y ±0.12 about its centre.
        let basis = Basis {
            right: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            back: [0.0, 0.0, 1.0],
        };
        let g = Puff {
            at: [2.996, 40.25, 0.5],
            size: 0.1,
            alpha: 1.0,
            kind: PuffKind::Gouge {
                u: [0.0, 0.0, 0.44],
                v: [0.0, 0.12, 0.0],
                kerf: false,
                bits: 0b01_000,
            },
            explosive: 0,
            seed: 3,
        };
        let (v, n) = blast_quads(&[g], [0.0, 40.0, 0.0], &basis);
        assert_eq!(n, 1);
        assert_eq!((v[8].to_bits() >> 8) & 0xff, KIND_GOUGE);
        assert_eq!(
            v[8].to_bits() >> 16,
            0b01_000,
            "the side and grain bits ride along"
        );
        for k in 0..6 {
            let p = &v[k * VERTEX_FLOATS..k * VERTEX_FLOATS + 3];
            // On the face's plane whatever the camera (x), within its extents.
            assert!((f64::from(p[0]) - 2.996).abs() < 1e-5, "{p:?}");
            assert!((f64::from(p[1]) - 0.25).abs() <= 0.12 + 1e-5);
            assert!((f64::from(p[2]) - 0.5).abs() <= 0.44 + 1e-5);
            // Its depth rides in rest.x.
            assert!((v[k * VERTEX_FLOATS + 5] - 0.1).abs() < 1e-6);
        }
    }
}
