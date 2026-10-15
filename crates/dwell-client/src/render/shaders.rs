//! The renderer's WGSL, generated so the sky function, the midplane and sea level come from their
//! one source (dwell-look, the protocol constants). It reproduces the Three.js look (Phase 7):
//! three's Lambert shading with a sun or moon and a hemisphere light per face (each fragment takes
//! only its own face's lights), ACES filmic tone mapping with exposure, sRGB encoding, then the
//! height fog mixed toward the sky's colour in output space, as three's fog was.

use dwell_protocol::world;

/// The per-frame uniforms (group 0, binding 0). Keep in step with `FrameUniforms`.
const FRAME: &str = r"
struct Frame {
    view_proj: mat4x4f,
    // x eye height (world y), y face (±1), z exposure, w fog density
    eye: vec4f,
    // x fog sigma, y fog scale height, z/w unused
    fog: vec4f,
    day_pole: vec4f,
    sun: vec4f,
    moon: vec4f,
    // Linear colours × intensity.
    sun_color: vec4f,
    moon_color: vec4f,
    day_sky: vec4f,
    day_ground: vec4f,
    night_sky: vec4f,
    night_ground: vec4f,
    // The sky's view rays: the camera's axes and the half-angle tangents (x, y).
    right: vec4f,
    up: vec4f,
    forward: vec4f,
    tan: vec4f,
};
@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

// Per draw (group 1, a storage array indexed by the draw's instance: draw k is drawn as instance k,
// so a draw needs no bind call of its own): the mesh's origin relative to the eye (xyz) and its scale (w),
// an optional clip box (relative to the eye; clip_lo.w > 0 enables it), and a tint (rgb) and
// opacity (a).
struct Draw {
    offset: vec4f,
    clip_lo: vec4f,
    clip_hi: vec4f,
    tint: vec4f,
    // x, y: cos and sin of a turn about +y (players' yaw); identity (1, 0) otherwise.
    rotation: vec4f,
};
@group(1) @binding(0) var<storage, read> draws: array<Draw>;
";

const COMMON: &str = r"
const PI: f32 = 3.141592653589793;

fn own(light_up: f32, frag_on_a: bool) -> f32 {
    let face_up = select(-1.0, 1.0, frag_on_a);
    return smoothstep(-0.1, 0.1, light_up * face_up);
}

// three's Lambert: (direct + hemisphere irradiance) × albedo / π, each light weighted to the
// fragment's own face (BIFACIAL_WORLD.md §6).
fn lambert(albedo: vec3f, n: vec3f, frag_y: f32) -> vec3f {
    let on_a = frag_y >= MIDPLANE_Y;
    var irradiance = vec3f(0.0);
    irradiance += max(dot(n, frame.sun.xyz), 0.0) * frame.sun_color.rgb * own(frame.sun.y, on_a);
    irradiance += max(dot(n, frame.moon.xyz), 0.0) * frame.moon_color.rgb * own(frame.moon.y, on_a);
    let day = frame.day_pole.xyz;
    let hemi_day = mix(frame.day_ground.rgb, frame.day_sky.rgb, dot(n, day) * 0.5 + 0.5);
    irradiance += hemi_day * own(day.y, on_a);
    let hemi_night = mix(frame.night_ground.rgb, frame.night_sky.rgb, dot(n, -day) * 0.5 + 0.5);
    irradiance += hemi_night * own(-day.y, on_a);
    return irradiance * albedo / PI;
}

fn rrt_odt_fit(v: vec3f) -> vec3f {
    let a = v * (v + 0.0245786) - 0.000090537;
    let b = v * (0.983729 * v + 0.4329510) + 0.238081;
    return a / b;
}

// three's ACESFilmicToneMapping.
fn aces(color_in: vec3f) -> vec3f {
    let input = mat3x3f(vec3f(0.59719, 0.07600, 0.02840), vec3f(0.35458, 0.90834, 0.13383), vec3f(0.04823, 0.01566, 0.83777));
    let output = mat3x3f(vec3f(1.60475, -0.10208, -0.00327), vec3f(-0.53108, 1.10813, -0.07276), vec3f(-0.07367, -0.00605, 1.07602));
    var color = color_in * (frame.eye.z / 0.6);
    color = output * rrt_odt_fit(input * color);
    return clamp(color, vec3f(0.0), vec3f(1.0));
}

fn srgb_encode(c: vec3f) -> vec3f {
    let lo = c * 12.92;
    let hi = pow(c, vec3f(0.41666)) * 1.055 - 0.055;
    return select(hi, lo, c <= vec3f(0.0031308));
}

fn sky(dir: vec3f) -> vec3f {
    return dwell_sky(dir, frame.eye.y, frame.day_pole.xyz, frame.sun.xyz, frame.moon.xyz);
}

// The height fog (dwell-look's haze_amount, per fragment), mixed toward the sky behind, in output
// colour. `rel` is the fragment relative to the eye.
fn fogged(color: vec3f, rel: vec3f) -> vec3f {
    let density = frame.eye.w;
    if (density <= 0.0) {
        return color;
    }
    let face = frame.eye.y;
    let dist = length(rel);
    let rise = face * rel.y;
    let cam = select(2.0 * MIDPLANE_Y - frame.eye.x, frame.eye.x, face > 0.0);
    let low = min(cam, cam + rise) - SEA_LEVEL;
    let h = frame.fog.y;
    let k = abs(rise) / h;
    let base = exp(min(80.0, -low / h));
    let along = select((1.0 - exp(-k)) / k, 1.0 - k * 0.5, k < 1e-4);
    let depth = frame.fog.x * dist * base * along;
    return mix(color, sky(normalize(rel)), density * (1.0 - exp(-depth)));
}

fn clipped(draw: Draw, rel: vec3f) -> bool {
    return draw.clip_lo.w > 0.0 && (any(rel < draw.clip_lo.xyz) || any(rel > draw.clip_hi.xyz));
}

// Lit, tone-mapped, encoded and fogged: what three's materials output.
fn finish(albedo: vec3f, n: vec3f, rel: vec3f) -> vec3f {
    return fogged(srgb_encode(aces(lambert(albedo, n, frame.eye.x + rel.y))), rel);
}
";

const TERRAIN: &str = r"
struct TerrainIn {
    @location(0) position: vec3f,
    @location(1) normal: vec3f,
    @location(2) color: vec3f,
    @location(3) uv: vec2f,
    @location(4) tile: vec4f,
    @location(5) grid: vec2f,
};
struct TerrainOut {
    @builtin(position) clip: vec4f,
    @location(0) rel: vec3f,
    @location(1) normal: vec3f,
    @location(2) color: vec3f,
    @location(3) uv: vec2f,
    @location(4) tile: vec4f,
    @location(5) @interpolate(flat) draw: u32,
    @location(6) @interpolate(flat) grid: vec2f,
};

@vertex
fn terrain_vs(v: TerrainIn, @builtin(instance_index) k: u32) -> TerrainOut {
    let draw = draws[k];
    var o: TerrainOut;
    o.draw = k;
    o.rel = draw.offset.xyz + v.position * draw.offset.w;
    o.clip = frame.view_proj * vec4f(o.rel, 1.0);
    o.normal = v.normal;
    o.color = v.color;
    o.uv = v.uv;
    o.tile = v.tile;
    o.grid = v.grid;
    return o;
}

// World-space grid lines (the registry's `grid` look option): a line GRID_WIDTH blocks wide every
// `grid.x` blocks, darkening by `grid.y`. The uvs are chunk-local blocks and `grid.x` divides the
// chunk size, so the lines line up across chunks. Antialiased by the pixel's footprint, and where
// the footprint outgrows the line the line's coverage spreads over it (it averages out, as a
// mipmap would: no shimmer at a distance); beyond half a period the lines fade to that average.
const GRID_WIDTH: f32 = 0.12;
fn grid_shade(uv: vec2f, grid: vec2f) -> f32 {
    // Derivatives first, in uniform control flow; a face without a grid shades 1.
    let foot = max(fwidth(uv), vec2f(1e-4));
    let every = max(grid.x, 1.0);
    let d = abs(fract(uv / every + 0.5) - 0.5) * every;
    let w = max(vec2f(GRID_WIDTH), foot);
    let line = (1.0 - smoothstep(w * 0.5 - foot * 0.5, w * 0.5 + foot * 0.5, d)) * (GRID_WIDTH / w);
    let average = GRID_WIDTH / every;
    let far = smoothstep(vec2f(0.25 * every), vec2f(0.5 * every), foot);
    let cover = mix(line, vec2f(average), far);
    return select(1.0, 1.0 - grid.y * max(cover.x, cover.y), grid.x > 0.0);
}

// The texture repeats once per block across merged quads: tile.xy + fract(uv) × tile.zw, with the
// gradients of the unwrapped uv so mip selection is continuous across block edges.
@fragment
fn terrain_fs(i: TerrainOut, @builtin(front_facing) front: bool) -> @location(0) vec4f {
    let draw = draws[i.draw];
    let atlas_uv = i.tile.xy + fract(i.uv) * i.tile.zw;
    let texel = textureSampleGrad(atlas, atlas_sampler, atlas_uv, dpdx(i.uv) * i.tile.zw, dpdy(i.uv) * i.tile.zw);
    let n = select(-i.normal, i.normal, front);
    let shade = grid_shade(i.uv, i.grid);
    return vec4f(finish(i.color * texel.rgb * shade * draw.tint.rgb, normalize(n), i.rel), draw.tint.a * texel.a);
}

struct FlatIn {
    @location(0) position: vec3f,
    @location(1) normal: vec3f,
    @location(2) color: vec3f,
};
struct FlatOut {
    @builtin(position) clip: vec4f,
    @location(0) rel: vec3f,
    @location(1) normal: vec3f,
    @location(2) color: vec3f,
    @location(3) @interpolate(flat) draw: u32,
};

// The draw's turn about +y (Three.js's rotation.y): x' = x cos + z sin, z' = −x sin + z cos.
fn turn(draw: Draw, p: vec3f) -> vec3f {
    let c = draw.rotation.x;
    let s = draw.rotation.y;
    return vec3f(p.x * c + p.z * s, p.y, -p.x * s + p.z * c);
}

@vertex
fn flat_vs(v: FlatIn, @builtin(instance_index) k: u32) -> FlatOut {
    let draw = draws[k];
    var o: FlatOut;
    o.draw = k;
    o.rel = draw.offset.xyz + turn(draw, v.position * draw.offset.w);
    o.clip = frame.view_proj * vec4f(o.rel, 1.0);
    o.normal = turn(draw, v.normal);
    o.color = v.color;
    return o;
}

// LOD sections and players: vertex colours (linear) × the draw's tint, no texture.
@fragment
fn flat_fs(i: FlatOut, @builtin(front_facing) front: bool) -> @location(0) vec4f {
    let draw = draws[i.draw];
    if (clipped(draw, i.rel)) {
        discard;
    }
    let n = select(-i.normal, i.normal, front);
    return vec4f(finish(i.color * draw.tint.rgb, normalize(n), i.rel), draw.tint.a);
}

struct LineIn {
    @location(0) position: vec3f,
    @location(1) color: vec3f,
};
struct LineOut {
    @builtin(position) clip: vec4f,
    @location(0) color: vec3f,
    @location(1) @interpolate(flat) draw: u32,
};

// Lines (the block outline, debug lines): unlit sRGB colours.
@vertex
fn line_vs(v: LineIn, @builtin(instance_index) k: u32) -> LineOut {
    let draw = draws[k];
    var o: LineOut;
    o.draw = k;
    o.clip = frame.view_proj * vec4f(draw.offset.xyz + v.position * draw.offset.w, 1.0);
    o.color = v.color;
    return o;
}

@fragment
fn line_fs(i: LineOut) -> @location(0) vec4f {
    let draw = draws[i.draw];
    return vec4f(i.color * draw.tint.rgb, draw.tint.a);
}

struct SkyOut {
    @builtin(position) clip: vec4f,
    @location(0) ndc: vec2f,
};

// The sky behind the world: a full-screen triangle, each pixel coloured by the sky of its view
// direction (already sRGB; no tone mapping), so the haze terrain fades into is the colour behind it.
@vertex
fn sky_vs(@builtin(vertex_index) i: u32) -> SkyOut {
    let p = array(vec2f(-1.0, -1.0), vec2f(3.0, -1.0), vec2f(-1.0, 3.0));
    var o: SkyOut;
    o.clip = vec4f(p[i], 0.0, 1.0);
    o.ndc = p[i];
    return o;
}

@fragment
fn sky_fs(i: SkyOut) -> @location(0) vec4f {
    let dir = normalize(frame.forward.xyz + i.ndc.x * frame.tan.x * frame.right.xyz + i.ndc.y * frame.tan.y * frame.up.xyz);
    return vec4f(sky(dir), 1.0);
}
";

/// The whole shader module: constants, uniforms, the sky, the shading and every entry point.
pub fn source() -> String {
    format!(
        "const MIDPLANE_Y: f32 = {:.1};\nconst SEA_LEVEL: f32 = {:.1};\n{FRAME}\n{}\n{COMMON}\n{TERRAIN}",
        world::MIDPLANE_Y as f64,
        world::SEA_LEVEL as f64,
        dwell_look::sky_wgsl()
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_shader_module_parses_and_validates() {
        let module = naga::front::wgsl::parse_str(&super::source()).expect("WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
        for entry in [
            "terrain_vs",
            "terrain_fs",
            "flat_vs",
            "flat_fs",
            "line_vs",
            "line_fs",
            "sky_vs",
            "sky_fs",
        ] {
            assert!(
                module.entry_points.iter().any(|e| e.name == entry),
                "{entry}"
            );
        }
    }
}
