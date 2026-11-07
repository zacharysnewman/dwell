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
        };
        let size = p.size;
        quad(
            &mut out,
            |x, y| {
                std::array::from_fn(|k| {
                    c[k] + (basis.right[k] * (2.0 * x - 1.0) + basis.up[k] * (2.0 * y - 1.0)) * size
                })
            },
            |x, y| [(2.0 * x - 1.0) as f32, (2.0 * y - 1.0) as f32],
            [0.0, p.alpha as f32, 0.0],
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
    let smokes = list(&|e| lin(e.effects.smoke.color));
    let dusts = list(&|e| lin(e.effects.dust.color));
    let fires = list(&|e| lin(e.effects.flash.color));
    let n = EXPLOSIVES.len().max(1);
    format!(
        "const BLAST_SMOKES = array<vec3f, {n}>({smokes});
const BLAST_DUSTS = array<vec3f, {n}>({dusts});
const BLAST_FIRES = array<vec3f, {n}>({fires});
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
fn blast_colour(kind: u32, look: u32, uv: vec2f, rest: vec3f, rel: vec3f, seed: u32) -> vec4f {{
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
    // One compare, not the scene's soft grid: the puffs are soft and overlap many deep, and per
    // pixel the grid was most of their cost (docs/DEV_TOOLS.md, Worst-case blasts).
    l.direct = shadow_light_taps(rel, up, l.direct, 1);
    let lit = lambert_lit(c * (0.85 + 0.3 * blast_noise(uv * 3.0 + s)), up, y, l);
    return vec4f(lit * a, a);
}}
",
        last = n - 1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_puffs_take_one_shadow_compare_a_pixel() {
        // Eight charges by a river: the puffs' soft shadow grid per pixel, many deep, held the Air
        // under 60 fps (docs/DEV_TOOLS.md, Worst-case blasts).
        let src = wgsl();
        assert!(src.contains("shadow_light_taps(rel, up, l.direct, 1)"), "{src}");
        assert!(!src.contains("shadow_light(rel"), "no soft grid");
    }

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
}
