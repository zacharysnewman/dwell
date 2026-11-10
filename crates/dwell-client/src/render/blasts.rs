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
            // z the puff's half size (m): the vertex shader finds its centre from it
            // (`puff_centre`) and lights the puff there once.
            [leaf, p.alpha as f32, size as f32],
            (p.explosive as u32 & 0xff) | kind << 8,
            p.seed,
        );
    }
    (out, far.len())
}

/// The puffs' shading in WGSL: `blast_colour(kind, look, uv, rest, rel, seed, bits, light)`,
/// premultiplied, `light` the puff's (`puff_light` at its centre, from the vertex shader); the
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
// A puff's centre (eye-relative) from one of its corners: the corner less its offset along the
// camera's axes (`blast_quads`: `right`·u + `up`·v times the half size in rest z).
fn puff_centre(rel: vec3f, uv: vec2f, size: f32) -> vec3f {{
    return rel - (frame.right.xyz * uv.x + frame.up.xyz * uv.y) * size;
}}
// The light on a smoke or dust puff, once per puff at its centre (the vertex shader; docs/
// EXPLOSIVES.md §4): the sky light, the sun's and moon's by the shadow maps there, facing the
// face's up. `lambert_lit` is linear in the albedo, so a pixel's colour is its albedo times this.
fn puff_light(rel: vec3f) -> vec3f {{
    let y = frame.eye.x + rel.y;
    let up = face_up(face_at(y));
    var l = terrain_light(1.0, vec2f(1.0, 0.0));
    l.direct = shadow_light(rel, up, l.direct);
    return lambert_lit(vec3f(1.0), up, y, l);
}}
fn blast_colour(kind: u32, look: u32, uv: vec2f, rest: vec3f, rel: vec3f, seed: u32, bits: u32, light: vec3f) -> vec4f {{
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
    // Lit once per puff (`puff_light`, in the vertex shader): the soft shape and its mottling per
    // pixel, the light flat over it.
    let lit = c * (0.85 + 0.3 * blast_noise(uv * 3.0 + s)) * light;
    return vec4f(lit * a, a);
}}
// An axe's chip (kind 11: a flat splinter, turned by its seed, tumbling; rest x 1: a leaf, rounder
// and green). Fresh wood keeps its tan hue in shade (`wood_lit`: the light's brightness on the
// wood's own colour, not the sky's blue on tan, which read grey-olive). The cuts themselves are
// carved into the terrain mesh (dwell-mesh carve.rs).
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

    dwell_world::both_faces!(
        /// Lit once per puff (docs/EXPLOSIVES.md §4): every vertex of a smoke or dust puff finds
        /// the same point, the puff's centre, by the shader's `puff_centre` (mirrored here on the
        /// CPU: the corner less right·u + up·v times rest z), so the light it gets there is the
        /// light a pixel lit at the centre got before; from any view on either face.
        fn every_vertex_of_a_puff_finds_its_centre(f: FaceFrame) {
            use crate::render::camera::aim;
            let eye = f.point([3.0, 70.0, -4.0]);
            for (yaw, pitch) in [(0.0, 0.0), (37.0, -50.0), (210.0, 25.0), (300.0, 80.0)] {
                let basis = aim(yaw, pitch, f, 0.0);
                let puffs = [
                    Puff {
                        at: f.point([5.0, 72.5, 1.0]),
                        size: 2.25,
                        alpha: 0.6,
                        kind: PuffKind::Smoke,
                        explosive: 0,
                        seed: 7,
                    },
                    Puff {
                        at: f.point([-8.0, 69.0, 3.0]),
                        size: 0.8,
                        alpha: 0.4,
                        kind: PuffKind::Dust,
                        explosive: 0,
                        seed: 9,
                    },
                ];
                let (v, n) = blast_quads(&puffs, eye, &basis);
                assert_eq!(n, 2);
                for vert in v.chunks_exact(VERTEX_FLOATS) {
                    let at = |k: usize| f64::from(vert[k]);
                    let (u, w, size) = (at(3), at(4), at(7));
                    let centre: [f64; 3] = std::array::from_fn(|k| {
                        at(k) - (basis.right[k] * u + basis.up[k] * w) * size + eye[k]
                    });
                    let puff = puffs
                        .iter()
                        .find(|p| (p.size - size).abs() < 1e-6)
                        .expect("its puff");
                    for k in 0..3 {
                        assert!(
                            (centre[k] - puff.at[k]).abs() < 1e-4,
                            "{centre:?} vs {:?} at yaw {yaw} pitch {pitch}",
                            puff.at
                        );
                    }
                }
            }
        }
    );

    #[test]
    fn smoke_and_dust_are_lit_in_the_vertex_shader_not_per_pixel() {
        // face-a-only: reads the shader's text, which has no face.
        let src = wgsl();
        let colour = src
            .split("fn blast_colour")
            .nth(1)
            .unwrap()
            .split("fn wood_lit")
            .next()
            .unwrap();
        // The fragment's path takes the flat light; no shadow lookup or lighting per pixel.
        assert!(!colour.contains("shadow_light"));
        assert!(!colour.contains("lambert_lit"));
        assert!(colour.contains("* light;"));
        // `puff_light` is the light the fragment shader computed before, with a white albedo
        // (`lambert_lit` is linear in it).
        let light = src.split("fn puff_light").nth(1).unwrap();
        let light = light.split("fn blast_colour").next().unwrap();
        for line in [
            "var l = terrain_light(1.0, vec2f(1.0, 0.0));",
            "l.direct = shadow_light(rel, up, l.direct);",
            "return lambert_lit(vec3f(1.0), up, y, l);",
        ] {
            assert!(light.contains(line), "{line}");
        }
        // The vertex shader lights smoke and dust at the centre, flat to the fragments.
        let flames = include_str!("flames.rs");
        assert!(flames.contains("o.lit = puff_light(puff_centre(v.rel, v.uv, v.rest.z));"));
        assert!(flames.contains("@location(5) @interpolate(flat) lit: vec3f,"));
        // Its shadow lookups are visible to the vertex stage.
        for src in [include_str!("shadows.rs"), include_str!("lod_shadows.rs")] {
            let entries = src.split("pub fn layout_entries()").nth(1).unwrap();
            let entries = entries.split("\n}\n").next().unwrap();
            assert_eq!(entries.matches("ShaderStages::VERTEX_FRAGMENT").count(), 3);
        }
    }

}
