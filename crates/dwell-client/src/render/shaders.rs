//! The renderer's WGSL, generated so the sky function, the midplane and sea level come from their
//! one source (dwell-look, the protocol constants). It reproduces the Three.js look (Phase 7):
//! three's Lambert shading with a sun or moon and a hemisphere light per face (each fragment takes
//! only its own face's lights), ACES filmic tone mapping with exposure, sRGB encoding, then the
//! height fog mixed toward the sky's colour in output space, as three's fog was.
//!
//! The frame is shared (docs/RAY_MARCHED_RENDERER.md step A1, docs/WATER_RENDERING.md W2): the scene passes write lit, linear
//! colour (premultiplied by coverage: alpha 0 where nothing is drawn) into an `rgba16float` target
//! and each opaque fragment's view depth into an `r32float` one; the composite (`composite_fs`)
//! tone-maps, encodes, fogs by the stored depth and puts the sky behind, into the canvas. Lines are
//! drawn over the composite in output colour, depth-tested against the stored depth.
//!
//! Fluids have their own shader (`fluid_vs`, `water_fs`; docs/WATER_RENDERING.md W1b): each vertex
//! names its fluid, whose look is a row of a storage buffer (`render/water.rs`); the surface stays
//! still and ripples are scrolling normal detail, with Schlick's Fresnel, a GGX glint and the sky
//! reflected. While the eye is under a fluid's surface the composite fogs by the fluid instead.

use dwell_protocol::world;

/// The per-frame uniforms (group 0, binding 0). Keep in step with `FrameUniforms`.
const FRAME: &str = r"
struct Frame {
    view_proj: mat4x4f,
    // x eye height (world y), y face (±1), z exposure, w fog density
    eye: vec4f,
    // x fog sigma, y fog scale height, z seconds (the water's scroll), w the water's
    // cost-isolation switches (render/water.rs `DEBUG_SWITCHES`; 0 in play)
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
    // The sky's clouds (dwell-look's cloud_uniforms): the noise point under the eye and the warp
    // field's offset (cells), and coverage, octaves (0: off), self-shadow, the deck's height above
    // the eye.
    clouds: vec4f,
    cloud_cfg: vec4f,
    // The eye under a fluid's drawn surface: x 1 (else 0), y fluid id, z the surface's world y;
    // w the water quality (0 Low, 1 Medium, 2 High).
    water: vec4f,
    // The wind (docs/MOTION_EFFECTS.md; `wind_uniforms` in mod.rs). point: the vanishing point
    // (NDC xy), intensity 0–1, time (s). view: aspect, the flight layer 0–1, drawing height (px),
    // +1 moving into the view or −1 away. color: streak colour (sRGB), peak opacity. streaks:
    // count, flight count, lit fraction of a cycle, width (px). shape: the radii the streaks fade
    // in between (half screen heights from the centre), vignette, flow speed. extra: x edge blur.
    wind_point: vec4f,
    wind_view: vec4f,
    wind_color: vec4f,
    wind_streaks: vec4f,
    wind_shape: vec4f,
    wind_extra: vec4f,
    // xy the eye's world x and z wrapped to [0, WRAP_M) (render/water.rs `wrapped_origin`).
    wrap: vec4f,
    // The water's reflections for its quality (dwell-look `reflection_uniforms`): x march steps,
    // y refine steps, z reach (m), w where hits start fading along it (m); then the cloud
    // octaves (0: the sky alone), the deck's self-shadow (0 or 1), the edge fade (share of the
    // screen height) and the hit thickness (m).
    reflect: vec4f,
    reflect_sky: vec4f,
};
@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;
// Per atlas tile (index cx + cy × ATLAS_CELLS): its grid lines (x every, y strength; 0 for none),
// from the registry's `grid` look option of the blocks that use the tile (dwell-mesh tile_grids).
@group(0) @binding(3) var<uniform> tile_grids: array<vec4f, ATLAS_TILES>;

// A fluid's look (render/water.rs `pack_fluids`; the registry's `fluids`), indexed by fluid id.
// Colours are linear.
struct Fluid {
    // rgb Beer–Lambert σ per metre, a the opacity
    absorption: vec4f,
    // rgb the scatter colour, a the roughness
    scatter: vec4f,
    // x IOR, y normal scale, z wave scale (m), w flow speed (m/s)
    wave: vec4f,
    // rgb the foam colour, a the foam depth (m)
    foam: vec4f,
    // x Fresnel F0 (from the IOR), y caustics' strength, z the surface drop (cells), w scattering
    // (σ per metre)
    extra: vec4f,
    // x refraction (m per unit slope), y caustics' cell size (m), z caustics' fade depth (m),
    // w sparkle
    optics: vec4f,
};
@group(0) @binding(4) var<storage, read> fluids: array<Fluid>;

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
    let a = v * (v + FIT_A) - FIT_B;
    let b = v * (FIT_C * v + FIT_D) + FIT_E;
    return a / b;
}

// three's ACESFilmicToneMapping (the matrices and the fit are render/water.rs's, with its tested
// inverse).
fn aces(color_in: vec3f) -> vec3f {
    var color = color_in * (frame.eye.z / 0.6);
    color = ACES_OUTPUT * rrt_odt_fit(ACES_INPUT * color);
    return clamp(color, vec3f(0.0), vec3f(1.0));
}

fn srgb_encode(c: vec3f) -> vec3f {
    let lo = c * 12.92;
    let hi = pow(c, vec3f(0.41666)) * 1.055 - 0.055;
    return select(hi, lo, c <= vec3f(0.0031308));
}

fn srgb_decode(c: vec3f) -> vec3f {
    let lo = c / 12.92;
    let hi = pow((max(c, vec3f(0.0)) + 0.055) / 1.055, vec3f(2.4));
    return select(hi, lo, c <= vec3f(0.04045));
}

// The scene colour that `display` shows as `c` (render/water.rs `inverse_aces`): the sky is drawn
// in output colour, and its reflection needs it in the scene's linear light.
fn inverse_display(c: vec3f) -> vec3f {
    let y = clamp(ACES_OUTPUT_INV * srgb_decode(c), vec3f(0.0), vec3f(INVERSE_FIT_MAX));
    let qa = 1.0 - FIT_C * y;
    let qb = FIT_A - FIT_D * y;
    let qc = -(FIT_B + FIT_E * y);
    let v = (-qb + sqrt(qb * qb - 4.0 * qa * qc)) / (2.0 * qa);
    return max(ACES_INPUT_INV * v * (0.6 / frame.eye.z), vec3f(0.0));
}

// Depth along the view axis of a point relative to the eye (what the scene's depth target holds).
fn view_depth(rel: vec3f) -> f32 {
    return dot(rel, frame.forward.xyz);
}

// The view ray through a point of the screen (ndc), scaled to view depth 1: `ray × depth` is the
// point at that depth relative to the eye.
fn view_ray(ndc: vec2f) -> vec3f {
    return frame.forward.xyz + ndc.x * frame.tan.x * frame.right.xyz + ndc.y * frame.tan.y * frame.up.xyz;
}

fn sky(dir: vec3f) -> vec3f {
    return dwell_sky(dir, frame.eye.y, frame.day_pole.xyz, frame.sun.xyz, frame.moon.xyz);
}

// The sky's light along `dir`, in the scene's linear colour.
fn sky_radiance(dir: vec3f) -> vec3f {
    return inverse_display(sky(dir));
}

// A water cost-isolation switch (`?waterdbg=`, render/water.rs `DEBUG_SWITCHES`): true turns the
// piece off. Uniform per frame, so its branches don't diverge.
fn water_off(bit: u32) -> bool {
    return (u32(frame.fog.w) & bit) != 0u;
}

// Schlick's Fresnel (render/water.rs `schlick`).
fn schlick(cos: f32, f0: f32) -> f32 {
    let m = pow(1.0 - clamp(cos, 0.0, 1.0), 5.0);
    return f0 + (1.0 - f0) * m;
}

// The underwater fog, the W4 stub (WATER_RENDERING.md W4 builds the full underwater view): over
// `dist` metres of the fluid, Beer–Lambert absorption of what is behind and the fluid's in-scatter,
// lit as the face's up is.
fn underwater(lin: vec3f, dist: f32, f: Fluid) -> vec3f {
    let t = exp(-(f.absorption.rgb + f.extra.w) * dist);
    let inscatter = lambert(f.scatter.rgb, vec3f(0.0, frame.eye.y, 0.0), frame.eye.x);
    return lin * t + inscatter * (1.0 - t);
}

// The height fog (dwell-look's haze_amount, per pixel): how much of the sky behind to mix in, in
// output colour. `rel` is the point relative to the eye.
fn fog_amount(rel: vec3f) -> f32 {
    let density = frame.eye.w;
    if (density <= 0.0) {
        return 0.0;
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
    return density * (1.0 - exp(-depth));
}

fn clipped(draw: Draw, rel: vec3f) -> bool {
    return draw.clip_lo.w > 0.0 && (any(rel < draw.clip_lo.xyz) || any(rel > draw.clip_hi.xyz));
}

// Tone-mapped and encoded: the scene's linear colour as the canvas shows it.
fn display(color: vec3f) -> vec3f {
    return srgb_encode(aces(color));
}

// What the scene passes write: lit linear colour (premultiplied: alpha is coverage, 1 for opaque
// draws) and the view depth (opaque draws only: the see-through's pipelines don't write it).
struct SceneOut {
    @location(0) color: vec4f,
    @location(1) depth: f32,
};

fn scene_out(albedo: vec3f, n: vec3f, rel: vec3f, alpha: f32) -> SceneOut {
    var o: SceneOut;
    o.color = vec4f(lambert(albedo, n, frame.eye.x + rel.y), alpha);
    o.depth = view_depth(rel);
    return o;
}
";

const TERRAIN: &str = r"
struct TerrainIn {
    @location(0) position: vec3f,
    @location(1) normal: vec3f,
    @location(2) color: vec3f,
    @location(3) uv: vec2f,
    @location(4) tile: vec4f,
};
struct TerrainOut {
    @builtin(position) clip: vec4f,
    @location(0) rel: vec3f,
    @location(1) normal: vec3f,
    @location(2) color: vec3f,
    @location(3) uv: vec2f,
    @location(4) tile: vec4f,
    @location(5) @interpolate(flat) draw: u32,
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
    return o;
}

// The grid of the atlas tile whose rectangle starts at `tile_origin` (its cell: the tile's origin
// lies inside its cell, past the gutter).
fn tile_grid(tile_origin: vec2f) -> vec2f {
    let cell = vec2u(clamp(floor(tile_origin * f32(ATLAS_CELLS)), vec2f(0.0), vec2f(f32(ATLAS_CELLS - 1u))));
    return tile_grids[cell.x + cell.y * ATLAS_CELLS].xy;
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
// gradients of the unwrapped uv so mip selection is continuous across block edges. Opaque draws
// (tint alpha 1) cover their pixels fully whatever the texel's alpha.
@fragment
fn terrain_fs(i: TerrainOut, @builtin(front_facing) front: bool) -> SceneOut {
    let draw = draws[i.draw];
    let atlas_uv = i.tile.xy + fract(i.uv) * i.tile.zw;
    let texel = textureSampleGrad(atlas, atlas_sampler, atlas_uv, dpdx(i.uv) * i.tile.zw, dpdy(i.uv) * i.tile.zw);
    let n = select(-i.normal, i.normal, front);
    let shade = grid_shade(i.uv, tile_grid(i.tile.xy));
    let alpha = select(draw.tint.a * texel.a, 1.0, draw.tint.a >= 1.0);
    return scene_out(i.color * texel.rgb * shade * draw.tint.rgb, normalize(n), i.rel, alpha);
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
fn flat_fs(i: FlatOut, @builtin(front_facing) front: bool) -> SceneOut {
    let draw = draws[i.draw];
    if (clipped(draw, i.rel)) {
        discard;
    }
    let n = select(-i.normal, i.normal, front);
    return scene_out(i.color * draw.tint.rgb, normalize(n), i.rel, draw.tint.a);
}

struct LineIn {
    @location(0) position: vec3f,
    @location(1) color: vec3f,
};
struct LineOut {
    @builtin(position) clip: vec4f,
    @location(0) color: vec3f,
    @location(1) @interpolate(flat) draw: u32,
    @location(2) depth: f32,
};

// Lines (the block outline, debug lines): unlit sRGB colours, drawn over the composite (so neither
// tone-mapped nor fogged) with the near pass's projection.
@vertex
fn line_vs(v: LineIn, @builtin(instance_index) k: u32) -> LineOut {
    let draw = draws[k];
    var o: LineOut;
    o.draw = k;
    let rel = draw.offset.xyz + v.position * draw.offset.w;
    o.clip = frame.view_proj * vec4f(rel, 1.0);
    o.color = v.color;
    o.depth = view_depth(rel);
    return o;
}

// Depth-tested against the scene's stored depth sample by sample: the share of the pixel's samples
// the line is in front of (or that show the sky) is its coverage, as the depth buffer's per-sample
// test gave it.
@fragment
fn line_fs(i: LineOut) -> @location(0) vec4f {
    let draw = draws[i.draw];
    let p = vec2i(i.clip.xy);
    var shown = 0.0;
    for (var s = 0; s < SCENE_SAMPLES; s++) {
        let d = textureLoad(scene_depth, p, s).r;
        shown += select(0.0, 1.0, d <= 0.0 || i.depth <= d * (1.0 + 1e-5));
    }
    return vec4f(i.color * draw.tint.rgb, draw.tint.a * shown / f32(SCENE_SAMPLES));
}

// Lines drawn over everything (the debug lines).
@fragment
fn line_top_fs(i: LineOut) -> @location(0) vec4f {
    let draw = draws[i.draw];
    return vec4f(i.color * draw.tint.rgb, draw.tint.a);
}

struct SkyOut {
    @builtin(position) clip: vec4f,
    @location(0) ndc: vec2f,
};

// A full-screen triangle.
@vertex
fn sky_vs(@builtin(vertex_index) i: u32) -> SkyOut {
    let p = array(vec2f(-1.0, -1.0), vec2f(3.0, -1.0), vec2f(-1.0, 3.0));
    var o: SkyOut;
    o.clip = vec4f(p[i], 0.0, 1.0);
    o.ndc = p[i];
    return o;
}

// The opaque scene's view depth at a pixel: the mean over the samples something covers (x; 0
// where none is) and the share of samples covered (y).
fn depth_of(d: array<f32, SAMPLE_SLOTS>) -> vec2f {
    var sum = 0.0;
    var n = 0.0;
    for (var s = 0; s < SCENE_SAMPLES; s++) {
        if (d[s] > 0.0) {
            sum += d[s];
            n += 1.0;
        }
    }
    return vec2f(select(0.0, sum / n, n > 0.0), n / f32(SCENE_SAMPLES));
}

// The clouds' light, from the frame: the sky frame and the lights, as the terrain takes them.
fn cloud_lighting() -> CloudLighting {
    return CloudLighting(frame.day_pole.xyz, frame.sun.xyz, frame.moon.xyz, frame.sun_color.rgb, frame.moon_color.rgb, frame.day_sky.rgb, frame.night_sky.rgb);
}

fn scene_depth_at(p: vec2i) -> vec2f {
    var d: array<f32, SAMPLE_SLOTS>;
    for (var s = 0; s < SCENE_SAMPLES; s++) {
        d[s] = textureLoad(scene_depth, p, s).r;
    }
    return depth_of(d);
}

// The composite: the scene's colour tone-mapped and encoded, fogged by its depth toward the sky
// of the pixel's view direction (already sRGB; no tone mapping), and over that sky by its coverage
// — so the haze terrain fades into is the colour behind it.
// The clouds (dwell-look's clouds.rs) are drawn over the sky only where the sky shows: the haze the
// terrain fades into is the clear sky behind it, so distant ground never takes a cloud's colour.
// The composite of one pixel: the scene tone-mapped, encoded, fogged and over the sky.
fn composited(p: vec2i, ndc: vec2f) -> vec3f {
    let ray = view_ray(ndc);
    let dir = normalize(ray);
    let face = frame.eye.y;
    // In uniform control flow, for the pixel's footprint on the deck.
    let deck = dwell_cloud_deck(dir, face, frame.clouds, frame.cloud_cfg.w);
    let fw = max(fwidth(deck.x), fwidth(deck.y));
    let clear = sky(dir);
    let c = textureLoad(scene_color, p, 0);
    var back = clear;
    if (c.a < 1.0) {
        back = dwell_clouds(clear, dir, face, deck, fw, frame.clouds, frame.cloud_cfg, cloud_lighting());
    }
    return composited_of(c, scene_depth_at(p).x, ray, clear, back);
}

// The scene at pixel `p` fogged along `ray` (view depth 1) and over the sky colour `back`.
fn composited_over(p: vec2i, ray: vec3f, back: vec3f) -> vec3f {
    return composited_of(textureLoad(scene_color, p, 0), scene_depth_at(p).x, ray, back, back);
}

// A scene colour `c` (premultiplied by coverage) at view depth `depth` along `ray` (view depth 1),
// tone-mapped, encoded, fogged toward the clear sky `clear` and over `back` (the sky with its
// clouds) by its coverage. Compare mode's two sides are composited by it too, as the frame is.
fn composited_of(c: vec4f, depth: f32, ray: vec3f, clear: vec3f, back: vec3f) -> vec3f {
    if (c.a <= 0.0) {
        return back;
    }
    let shown = mix(display(c.rgb / c.a), clear, fog_amount(ray * depth));
    return mix(back, shown, min(c.a, 1.0));
}

// Then the wind over it while there is any (MOTION: streaks, vignette, edge blur).
@fragment
fn composite_fs(i: SkyOut) -> @location(0) vec4f {
    let p = vec2i(i.clip.xy);
    if (frame.water.x > 0.5) {
        return vec4f(composite_underwater(p, view_ray(i.ndc), textureLoad(scene_color, p, 0)), 1.0);
    }
    var color = composited(p, i.ndc);
    if (frame.wind_point.z > 1e-3) {
        color = motion_composite(color, p, i.ndc);
    }
    return vec4f(color, 1.0);
}

// Compare mode's second side (group 3): its scene targets and the difference counts.
fn scene_b_depth_at(p: vec2i) -> vec2f {
    var d: array<f32, SAMPLE_SLOTS>;
    for (var s = 0; s < SCENE_SAMPLES; s++) {
        d[s] = textureLoad(scene_b_depth, p, s).r;
    }
    return depth_of(d);
}

// sRGB-encoded colour to CIE L*a*b* (D65).
fn srgb_to_lab(c: vec3f) -> vec3f {
    let lin = select(pow((c + 0.055) / 1.055, vec3f(2.4)), c / 12.92, c <= vec3f(0.04045));
    let xyz = mat3x3f(
        vec3f(0.4124, 0.2126, 0.0193),
        vec3f(0.3576, 0.7152, 0.1192),
        vec3f(0.1805, 0.0722, 0.9505),
    ) * lin / vec3f(0.95047, 1.0, 1.08883);
    let f = select(xyz * 7.787 + 16.0 / 116.0, pow(xyz, vec3f(1.0 / 3.0)), xyz > vec3f(0.008856));
    return vec3f(116.0 * f.y - 16.0, 500.0 * (f.x - f.y), 200.0 * (f.y - f.z));
}

// Compare mode's difference view (render/compare.rs): each pixel in its worst class — coverage
// differs (red), view depth differs (orange), the composited colour's ΔE (yellow) — counted into
// diff_counts (pixels, coverage, depth, colour); elsewhere the left side, dimmed to grey.
@fragment
fn difference_fs(i: SkyOut) -> @location(0) vec4f {
    let p = vec2i(i.clip.xy);
    let da = scene_depth_at(p);
    let db = scene_b_depth_at(p);
    let ray = view_ray(i.ndc);
    let back = sky(normalize(ray));
    let a = composited_of(textureLoad(scene_color, p, 0), da.x, ray, back, back);
    let b = composited_of(textureLoad(scene_b_color, p, 0), db.x, ray, back, back);
    atomicAdd(&diff_counts[0], 1u);
    let hit_a = da.y > 0.5;
    let hit_b = db.y > 0.5;
    if (hit_a != hit_b) {
        atomicAdd(&diff_counts[1], 1u);
        return vec4f(1.0, 0.15, 0.1, 1.0);
    }
    if (hit_a && abs(da.x - db.x) > DIFF_DEPTH * max(da.x, db.x)) {
        atomicAdd(&diff_counts[2], 1u);
        return vec4f(1.0, 0.55, 0.0, 1.0);
    }
    if (length(srgb_to_lab(a) - srgb_to_lab(b)) > DIFF_DELTA_E) {
        atomicAdd(&diff_counts[3], 1u);
        return vec4f(1.0, 0.95, 0.1, 1.0);
    }
    return vec4f(vec3f(dot(a, vec3f(0.2126, 0.7152, 0.0722)) * 0.35), 1.0);
}

// How many metres the pixel at `p` covers on a surface at view depth `depth` along `ray`, facing
// it obliquely as the floor does (the slant from how far the ray dips).
fn pixel_m(p: vec2i, ray: vec3f, depth: f32) -> f32 {
    let h = f32(textureDimensions(scene_color).y);
    let across = depth * length(ray) * 2.0 * frame.tan.y / h;
    return across / max(abs(normalize(ray).y), 0.15);
}

// The composite with the eye under a fluid's surface (the W4 stub): the fluid's fog over the
// distance in it — to what the pixel shows, or to the surface where the ray reaches it first —
// replaces the height fog; past the surface, the sky.
fn composite_underwater(p: vec2i, ray: vec3f, c: vec4f) -> vec3f {
    let f = fluids[min(u32(frame.water.y), arrayLength(&fluids) - 1u)];
    let dir = normalize(ray);
    let rise = dir.y * frame.eye.y;
    let below = max((frame.water.z - frame.eye.x) * frame.eye.y, 0.0);
    let to_surface = select(1e6, below / rise, rise > 1e-4);
    let back = display(underwater(sky_radiance(dir), to_surface, f));
    if (c.a <= 0.0) {
        return back;
    }
    let depth = scene_depth_at(p).x;
    let dist = select(1e6, depth * length(ray), depth > 0.0);
    var lit = c.rgb / c.a;
    // Caustics on what the pixel shows under the surface (W3, Medium and High): the seabed, walls,
    // anything opaque, before the fog takes its share over the distance to it.
    if (frame.water.w >= 0.5 && depth > 0.0 && !water_off(DBG_UNDER)) {
        let rel = ray * depth;
        let under = (frame.water.z - (frame.eye.x + rel.y)) * frame.eye.y;
        // Lit by light that came down through the fluid, focused into caustics.
        lit *= exp(-f.absorption.rgb * max(under, 0.0))
            * caustic_light(frame.wrap.xy + rel.xz, under, frame.eye.x + rel.y, pixel_m(p, ray, depth), UNDERWATER_CAUSTICS, f);
    }
    let shown = display(underwater(lit, min(dist, to_surface), f));
    return mix(back, shown, min(c.a, 1.0));
}
";

/// The wind (docs/MOTION_EFFECTS.md, Phase 29), applied by the composite over the finished picture:
/// thin soft streaks radiating from the vanishing point of the camera's velocity, in angular cells
/// around it with hashed jitter (procedural, no textures), fading in toward the screen's edges so
/// the crosshair stays clear; a denser layer for fast flight; a subtle edge vignette; and a radial
/// blur of the scene toward the vanishing point at the edges. Its uniforms are `frame.wind_*`.
const MOTION: &str = r"
const TAU: f32 = 6.283185307179586;

// PCG-style integer hash to 0–1.
fn motion_hash(n: u32) -> f32 {
    var x = n * 747796405u + 2891336453u;
    x = ((x >> ((x >> 28u) + 4u)) ^ x) * 277803737u;
    x = (x >> 22u) ^ x;
    return f32(x) / 4294967295.0;
}

// One layer of `count` streaks around the vanishing point `vp` (both in half screen heights).
fn motion_streaks(p: vec2f, vp: vec2f, count: f32, seed: u32, width_px: f32, speed: f32) -> f32 {
    let d = p - vp;
    let r = max(length(d), 1e-4);
    let x = (atan2(d.y, d.x) / TAU + 0.5) * count;
    let n = max(u32(count), 1u);
    let id = (u32(floor(x)) % n) + seed * 4099u;
    let h0 = motion_hash(id);
    let h1 = motion_hash(id ^ 0x9e3779b9u);
    let h2 = motion_hash(id * 3u + 17u);
    // Across: the pixel distance from this cell's streak (jittered within the cell).
    let centre = 0.25 + 0.5 * h0;
    let across = abs(fract(x) - centre) / count * TAU * r * frame.wind_view.z * 0.5;
    let w = width_px * (0.6 + 0.8 * h1);
    let line = 1.0 - smoothstep(0.2 * w, w, across);
    // Along: dashes in log radius flowing outward (longer farther out), each cycle on or off.
    let phase = log(r) * 1.3 - frame.wind_point.w * speed * (0.6 + 0.8 * h2) * frame.wind_view.w + h1 * 13.0;
    let cycle = bitcast<u32>(i32(floor(phase)));
    let on = step(motion_hash(id ^ (cycle * 2654435761u)), 0.55);
    let f = fract(phase);
    let len = frame.wind_streaks.z;
    let dash = smoothstep(0.0, 0.3 * len, f) * (1.0 - smoothstep(0.45 * len, len, f));
    return line * dash * on;
}

// A pixel at `ndc` in half screen heights from the centre (x scaled by the aspect).
fn motion_screen(ndc: vec2f) -> vec2f {
    return vec2f(ndc.x * frame.wind_view.x, ndc.y);
}

// How strongly the wind shows at a screen point: nothing round the centre, full at the edges.
fn motion_edge(ndc: vec2f) -> f32 {
    return smoothstep(frame.wind_shape.x, frame.wind_shape.y, length(motion_screen(ndc)));
}

// The streaks and vignette over a pixel at `ndc`, premultiplied: streaks add their colour, the
// vignette darkens.
fn motion_wind(ndc: vec2f) -> vec4f {
    let p = motion_screen(ndc);
    let vp = motion_screen(frame.wind_point.xy);
    let intensity = frame.wind_point.z;
    let clear = smoothstep(0.15, 0.6, length(p - vp));
    var s = motion_streaks(p, vp, frame.wind_streaks.x, 1u, frame.wind_streaks.w, frame.wind_shape.w);
    let flight = frame.wind_view.y;
    if (flight > 0.0 && frame.wind_streaks.y >= 1.0) {
        s += flight * 0.8 * motion_streaks(p, vp, frame.wind_streaks.y, 2u, frame.wind_streaks.w * 0.7, frame.wind_shape.w * 2.2);
    }
    let a = clamp(s * motion_edge(ndc) * clear * intensity * frame.wind_color.a, 0.0, 1.0);
    let v = frame.wind_shape.z * intensity * smoothstep(0.75, 1.9, length(p));
    return vec4f(frame.wind_color.rgb * a, a + v * (1.0 - a));
}

// The composited picture at pixel `px` blurred toward the vanishing point (the scene streaming
// past), then the streaks and vignette over it. `color` is the pixel's own composite.
// The taps share this pixel's view ray and sky (the sky is the composite's costly part; it barely
// changes over a few pixels) and are jittered per pixel, so the blur is smooth rather than copies.
fn motion_composite(color: vec3f, px: vec2i, ndc: vec2f) -> vec3f {
    var out = color;
    let blur = frame.wind_extra.x * frame.wind_point.z * frame.wind_point.z * motion_edge(ndc);
    if (blur > 1e-4) {
        let dims = vec2f(textureDimensions(scene_color));
        let here = vec2f(px) + 0.5;
        let vp = vec2f(frame.wind_point.x * 0.5 + 0.5, 0.5 - frame.wind_point.y * 0.5) * dims;
        let step = (vp - here) * blur / 4.0;
        let ray = view_ray(ndc);
        let back = sky(normalize(ray));
        let jitter = motion_hash(u32(px.x) * 1973u + u32(px.y) * 9277u);
        var sum = color;
        for (var k = 0; k < 4; k++) {
            let q = clamp(here + step * (f32(k) + jitter), vec2f(0.5), dims - 0.5);
            sum += composited_over(vec2i(q), ray, back);
        }
        out = sum / 5.0;
    }
    let w = motion_wind(ndc);
    return w.rgb + out * (1.0 - w.a);
}

";

const WATER: &str = r"
struct FluidIn {
    @location(0) position: vec3f,
    @location(1) normal: vec3f,
    @location(2) color: vec3f,
    @location(3) fluid: f32,
};
struct FluidOut {
    @builtin(position) clip: vec4f,
    @location(0) rel: vec3f,
    @location(1) normal: vec3f,
    @location(2) color: vec3f,
    // World x and z wrapped to [0, WRAP_M) (the draw's wrapped origin plus the vertex): where the
    // normal detail is evaluated, exact at any distance from the origin.
    @location(3) wrapped: vec2f,
    @location(4) @interpolate(flat) fluid: u32,
    @location(5) @interpolate(flat) draw: u32,
};

// Fluid meshes: chunk water and LOD water alike (the draw's offset.w scales cells to metres;
// rotation.zw holds the draw's origin wrapped, render/water.rs `wrapped_origin`).
@vertex
fn fluid_vs(v: FluidIn, @builtin(instance_index) k: u32) -> FluidOut {
    let draw = draws[k];
    var o: FluidOut;
    o.draw = k;
    o.rel = draw.offset.xyz + v.position * draw.offset.w;
    o.clip = frame.view_proj * vec4f(o.rel, 1.0);
    o.normal = v.normal;
    o.color = v.color;
    o.wrapped = draw.rotation.zw + v.position.xz * draw.offset.w;
    o.fluid = u32(v.fluid + 0.5);
    return o;
}

// A unit gradient per lattice point, wrapping every `period` points.
fn hash_grad(c: vec2i, period: i32) -> vec2f {
    let w = vec2u(((c % period) + period) % period);
    var h = (w.x * 0x8da6b343u) ^ (w.y * 0xd8163841u) ^ 0x9e3779b9u;
    h = (h ^ (h >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    h = h ^ (h >> 16u);
    let a = f32(h >> 8u) * (6.283185307 / 16777216.0);
    return vec2f(cos(a), sin(a));
}

// Two values in [0, 1) per lattice point, wrapping every `period` points (no trig: the caustics'
// Worley points).
fn hash_unit2(c: vec2i, period: i32) -> vec2f {
    let w = vec2u(((c % period) + period) % period);
    var h = (w.x * 0x8da6b343u) ^ (w.y * 0xd8163841u) ^ 0x9e3779b9u;
    h = (h ^ (h >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return vec2f(f32(h & 0xffffu), f32(h >> 16u)) * (1.0 / 65536.0);
}

// Gradient noise and its derivatives (value, d/dx, d/dy), quintic, wrapping every `period` cells.
fn noise_d(p: vec2f, period: i32) -> vec3f {
    let cell = floor(p);
    let i = vec2i(cell);
    let f = p - cell;
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let du = 30.0 * f * f * (f * (f - 2.0) + 1.0);
    let ga = hash_grad(i, period);
    let gb = hash_grad(i + vec2i(1, 0), period);
    let gc = hash_grad(i + vec2i(0, 1), period);
    let gd = hash_grad(i + vec2i(1, 1), period);
    let va = dot(ga, f);
    let vb = dot(gb, f - vec2f(1.0, 0.0));
    let vc = dot(gc, f - vec2f(0.0, 1.0));
    let vd = dot(gd, f - vec2f(1.0, 1.0));
    let k = va - vb - vc + vd;
    let value = va + u.x * (vb - va) + u.y * (vc - va) + u.x * u.y * k;
    let d = ga + u.x * (gb - ga) + u.y * (gc - ga) + u.x * u.y * (ga - gb - gc + gd)
        + du * (u.yx * k + vec2f(vb - va, vc - va));
    return vec3f(value, d);
}

// One layer of normal detail (render/water.rs `WaveLayer`): its height slope (xy) at wrapped
// world `xz`, faded out where a pixel covers `footprint` m (Nyquist), and the slope variance the
// fade lost (z). Wavelengths divide WRAP_M, so the detail wraps with the position.
fn wave_layer(xz: vec2f, f: Fluid, footprint: f32, ratio: f32, slope: f32, speed: f32, dir: vec2f, turn: mat2x2f) -> vec3f {
    let n = max(1.0, round(WRAP_M / max(f.wave.z * ratio, 1e-3)));
    let wavelength = WRAP_M / n;
    let s = f.wave.y * slope;
    let fade = 1.0 - smoothstep(wavelength * FADE_LO, wavelength * FADE_HI, footprint);
    let lost = (1.0 - fade * fade) * s * s;
    if (fade <= 0.0) {
        return vec3f(0.0, 0.0, lost);
    }
    let shift = dir * (frame.fog.z * f.wave.w * speed / wavelength);
    let q = turn * (xz / wavelength) + (shift - n * floor(shift / n));
    let d = noise_d(q, i32(n));
    let scale = sqrt(abs(determinant(turn)));
    return vec3f(transpose(turn) * d.yz * (s * fade / scale), lost);
}

// GGX specular of a light (render/water.rs `glint_alpha` gives α), height-correlated Smith.
fn ggx_glint(n: vec3f, v: vec3f, l: vec3f, a: f32, f0: f32) -> f32 {
    let nl = dot(n, l);
    if (nl <= 0.0) {
        return 0.0;
    }
    let h = normalize(v + l);
    let nv = max(dot(n, v), 1e-3);
    let nh = max(dot(n, h), 0.0);
    let a2 = a * a;
    let d = nh * nh * (a2 - 1.0) + 1.0;
    let dist = a2 / (PI * d * d);
    let vis = 0.5 / (nl * sqrt(nv * nv * (1.0 - a2) + a2) + nv * sqrt(nl * nl * (1.0 - a2) + a2));
    return min(dist * vis * schlick(dot(v, h), f0) * nl, GLINT_MAX);
}

const GLINT_MAX: f32 = 16.0;

// The glint's width (render/water.rs `glint_alpha`): the fluid's roughness, widened by the slope
// variance the faded layers no longer draw.
fn glint_alpha(roughness: f32, lost: f32) -> f32 {
    let a = max(roughness * roughness, MIN_ALPHA);
    return sqrt(a * a + 2.0 * lost);
}

// A fluid's surface as seen from one fragment: the rippled normal toward the side seen (w: the
// summed slopes and the variance lost to fading), the view direction, and the body's in-scattered
// light (the fluid lit from above, the face's up, whichever side it is seen from).
struct Surface {
    n: vec3f,
    w: vec3f,
    v: vec3f,
    ndv: f32,
    // The face's up (±1) and the fragment's world y.
    up: f32,
    frag_y: f32,
    on_a: bool,
    body: vec3f,
};

fn water_surface(i: FluidOut, front: bool, footprint: f32, f: Fluid, draw: Draw) -> Surface {
    var s: Surface;
    s.v = normalize(-i.rel);
    let face_n = normalize(i.normal);
    // The face's normal toward the eye's side (outward from outside, inward from inside).
    let seen = select(-face_n, face_n, front);
    s.frag_y = frame.eye.x + i.rel.y;
    s.on_a = s.frag_y >= MIDPLANE_Y;
    s.up = select(-1.0, 1.0, s.on_a);
    // Ripples on the surface (tops and bottoms; sides stay flat), toward the side seen: on face B
    // the mirrored surface's normal points down, and so does the rippled one.
    s.w = vec3f(0.0);
    s.n = seen;
    if (abs(face_n.y) > 0.5) {
        s.w = water_slope(i.wrapped, f, footprint, select(WAVE_LAYERS, 1, frame.water.w < 0.5));
        s.n = sign(seen.y) * normalize(vec3f(-s.w.x, 1.0, -s.w.y));
        // A ripple can't face away from the eye: bend it back.
        s.n = normalize(s.n + max(0.02 - dot(s.n, s.v), 0.0) * s.v);
    }
    s.ndv = clamp(dot(s.n, s.v), 1e-3, 1.0);
    s.body = lambert(f.scatter.rgb * draw.tint.rgb, vec3f(0.0, s.up, 0.0), s.frag_y);
    return s;
}

// The view reflected off the ripple. Reflections below the surface's plane fold back up (the
// surface would hide them).
fn reflected_dir(s: Surface) -> vec3f {
    let seen_up = vec3f(0.0, sign(s.n.y), 0.0);
    let r = reflect(-s.v, s.n);
    return normalize(r - 2.0 * min(dot(r, seen_up), 0.0) * seen_up);
}

// What the surface sends back from above besides the body: what it reflects (`refl`: its light,
// and in a the share of it that is the scene, which hides the sun and moon behind it), Fresnel
// weighted; the sun's and moon's GGX glint; and the sun's sparkle off the facets of fine ripples.
fn surface_light(i: FluidOut, s: Surface, f: Fluid, fres: f32, footprint: f32, refl: vec4f) -> vec3f {
    let a = glint_alpha(f.scatter.a, s.w.z);
    let f0 = f.extra.x;
    var glint = vec3f(0.0);
    if (!water_off(DBG_GLINT)) {
        glint = ggx_glint(s.n, s.v, frame.sun.xyz, a, f0) * frame.sun_color.rgb * own(frame.sun.y, s.on_a)
            + ggx_glint(s.n, s.v, frame.moon.xyz, a, f0) * frame.moon_color.rgb * own(frame.moon.y, s.on_a);
    }
    var reflected = refl.rgb;
    if (water_off(DBG_SKY)) {
        reflected = frame.day_sky.rgb;
    }
    var spark = vec3f(0.0);
    if (!water_off(DBG_SPARKLE)) {
        spark = sparkle(i, s, f, footprint);
    }
    return fres * reflected + (1.0 - refl.a) * (glint + spark);
}

// The sky along a reflected ray `r` from the surface point `rel`, with the cloud deck over it on
// the tiers that reflect clouds (WATER_RENDERING.md §8; the composite's own sky and clouds, so
// the day, night and dusk follow), in the scene's linear light. `fw` is the deck's footprint
// (`reflected_deck_fw`).
fn reflected_env(r: vec3f, rel: vec3f, fw: f32) -> vec3f {
    let clear = sky(r);
    var c = clear;
    let octaves = frame.reflect_sky.x;
    if (octaves > 0.5 && !water_off(DBG_CLOUDS)) {
        let face = frame.eye.y;
        // The deck seen from the surface point, not the eye: its height above it, and the noise
        // point under it.
        let height = frame.cloud_cfg.w - rel.y * face;
        let offsets = vec4f(frame.clouds.xy + rel.xz / CLOUD_SCALE, frame.clouds.zw);
        let deck = dwell_cloud_deck(r, face, offsets, height);
        let cfg = vec4f(frame.cloud_cfg.x, octaves, frame.reflect_sky.y, height);
        c = dwell_clouds(clear, r, face, deck, fw, offsets, cfg, cloud_lighting());
    }
    return inverse_display(c);
}

// The deck's footprint in a reflection, taken off the calm surface's mirror ray (derivatives need
// uniform control flow: call it before any branch on the fragment). 0 while no clouds reflect.
fn reflected_deck_fw(rel: vec3f) -> f32 {
    if (frame.reflect_sky.x < 0.5 || water_off(DBG_CLOUDS)) {
        return 0.0;
    }
    let up = select(-1.0, 1.0, frame.eye.x + rel.y >= MIDPLANE_Y);
    let d = normalize(rel);
    let r = vec3f(d.x, abs(d.y) * up, d.z);
    let height = frame.cloud_cfg.w - rel.y * frame.eye.y;
    let deck = dwell_cloud_deck(r, frame.eye.y, vec4f(frame.clouds.xy + rel.xz / CLOUD_SCALE, frame.clouds.zw), height);
    return max(fwidth(deck.x), fwidth(deck.y));
}

// The screen point (pixels, y down) of a point relative to the eye, and its view depth
// (render/reflection.rs `View::to_screen`).
fn to_screen(rel: vec3f, size: vec2f) -> vec3f {
    let d = dot(rel, frame.forward.xyz);
    let n = vec2f(dot(rel, frame.right.xyz) / (d * frame.tan.x), dot(rel, frame.up.xyz) / (d * frame.tan.y));
    return vec3f((n.x * 0.5 + 0.5) * size.x, (0.5 - n.y * 0.5) * size.y, d);
}

fn ssr_depth(px: vec2f) -> f32 {
    return textureLoad(scene_depth, vec2i(px), 0).r;
}

// A march step at view depth `d` (from `prev`) meets the scene at depth `scene`: it is behind it,
// and the surface lies within the step (else the ray passed behind something in front).
fn ssr_hits(scene: f32, d: f32, prev: f32) -> bool {
    return scene > 0.0 && d >= scene && scene >= min(prev, d) - (frame.reflect_sky.w + SSR_THICKNESS_REL * scene);
}

// How much of a hit at `px` survives the fade at the screen's edges (render/reflection.rs
// `edge_weight`).
fn ssr_edge(px: vec2f, size: vec2f) -> f32 {
    let e = min(min(px.x, size.x - px.x), min(px.y, size.y - px.y));
    return smoothstep(0.0, 1.0, e / (frame.reflect_sky.z * size.y));
}

// Screen-space reflection (render/reflection.rs `trace`, the tested twin): the reflected ray `r`
// from the surface point `p` marched on screen through the scene's view depth, in even steps to
// the screen's edge or the reach, a hit pinned by binary steps. Where it meets the scene (xy, in
// pixels) and how much of the reflection that gives (z; 0: missed).
fn ssr_trace(p: vec3f, r: vec3f, size: vec2f) -> vec3f {
    let steps = i32(frame.reflect.x);
    let d0 = dot(p, frame.forward.xyz);
    let toward = dot(r, frame.forward.xyz);
    var len = frame.reflect.z;
    // A ray back toward the eye ends in front of it.
    if (toward < 0.0) {
        len = min(len, 0.9 * d0 / -toward);
    }
    if (steps <= 0 || len <= 0.0 || water_off(DBG_SSR)) {
        return vec3f(0.0);
    }
    let a = to_screen(p, size);
    let b = to_screen(p + r * len, size);
    let delta = b.xy - a.xy;
    // Clipped to the screen: no steps spent past its edge.
    let t_end = screen_clip(a.xy, delta, size);
    // View depth runs hyperbolically along the screen: 1/depth is linear.
    let k0 = 1.0 / a.z;
    let k1 = 1.0 / b.z;
    var prev_t = 0.0;
    var prev_d = a.z;
    for (var i = 1; i <= steps; i++) {
        let t = t_end * f32(i) / f32(steps);
        let d = 1.0 / mix(k0, k1, t);
        if (ssr_hits(ssr_depth(a.xy + delta * t), d, prev_d)) {
            var t_lo = prev_t;
            var t_hi = t;
            for (var j = 0; j < i32(frame.reflect.y); j++) {
                let mid = 0.5 * (t_lo + t_hi);
                let dm = 1.0 / mix(k0, k1, mid);
                let sm = ssr_depth(a.xy + delta * mid);
                if (sm > 0.0 && dm >= sm) {
                    t_hi = mid;
                } else {
                    t_lo = mid;
                }
            }
            let px = a.xy + delta * t_hi;
            let dh = 1.0 / mix(k0, k1, t_hi);
            // How far along the ray in 3D: view depth is linear along it.
            let u = select(t_hi, (dh - a.z) / (b.z - a.z), abs(b.z - a.z) > 1e-3 * a.z);
            let w = ssr_edge(px, size) * (1.0 - smoothstep(frame.reflect.w, frame.reflect.z, u * len));
            return vec3f(px, w);
        }
        prev_t = t;
        prev_d = d;
    }
    return vec3f(0.0);
}

// The share of a screen segment from `a` along `delta` that stays on screen.
fn screen_clip(a: vec2f, delta: vec2f, size: vec2f) -> f32 {
    var t_end = 1.0;
    let lo = vec2f(0.5) - a;
    let hi = size - 0.5 - a;
    for (var k = 0; k < 2; k++) {
        if (delta[k] > 1e-3) {
            t_end = min(t_end, hi[k] / delta[k]);
        } else if (delta[k] < -1e-3) {
            t_end = min(t_end, lo[k] / delta[k]);
        }
    }
    return max(t_end, 0.0);
}

// What the surface reflects (render/reflection.rs `reflected`): the scene at a screen-space hit,
// from the copy and fogged by its distance as the composite would, over `env` (the sky and
// clouds) by the hit's weight and coverage. a: the share that is the scene.
fn ssr_reflected(env: vec3f, hit: vec3f, size: vec2f) -> vec4f {
    if (hit.z <= 0.0) {
        return vec4f(env, 0.0);
    }
    let c = textureSampleLevel(scene_color, scene_sampler, hit.xy / size, 0.0);
    if (c.a <= 0.0) {
        return vec4f(env, 0.0);
    }
    let w = hit.z * min(c.a, 1.0);
    let ndc = vec2f(hit.x / size.x * 2.0 - 1.0, 1.0 - hit.y / size.y * 2.0);
    let fog = fog_amount(view_ray(ndc) * ssr_depth(hit.xy));
    let seen = mix(c.rgb / c.a, env, fog);
    return vec4f(mix(env, seen, w), w);
}

// The sun's sparkle: a tight highlight off the facets of a fine, fast ripple layer, so the glint
// breaks into glitter where the ripples are drawn (faded where a pixel covers them, Nyquist).
fn sparkle(i: FluidOut, s: Surface, f: Fluid, footprint: f32) -> vec3f {
    let k = f.optics.w;
    let wavelength = SPARKLE_WAVELENGTH * f.wave.z;
    let fade = 1.0 - smoothstep(wavelength * FADE_LO, wavelength * FADE_HI, footprint);
    if (k <= 0.0 || fade <= 0.0) {
        return vec3f(0.0);
    }
    let n_cells = max(1.0, round(WRAP_M / wavelength));
    let shift = vec2f(0.6, 0.8) * (frame.fog.z * f.wave.w * 3.1 / wavelength);
    let q = mat2x2f(2.0, -1.0, 1.0, 2.0) * (i.wrapped / wavelength) + (shift - n_cells * floor(shift / n_cells));
    let d = noise_d(q, i32(n_cells));
    let n = normalize(s.n + vec3f(-d.y, 0.0, -d.z) * 0.35 * sign(s.n.y));
    let r = reflect(-s.v, n);
    let lobe = pow(max(dot(r, frame.sun.xyz), 0.0), SPARKLE_POWER);
    return frame.sun_color.rgb * own(frame.sun.y, s.on_a) * (k * fade * lobe * schlick(s.ndv, f.extra.x));
}

const SPARKLE_WAVELENGTH: f32 = 0.06;
const SPARKLE_POWER: f32 = 2400.0;

// A fluid's surface seen from inside (the W4 stub): the body's colour, out through Snell's window,
// else totally reflected.
fn from_below(s: Surface, f: Fluid) -> vec4f {
    let out_dir = refract(-s.v, s.n, f.wave.x);
    let tir = dot(out_dir, out_dir) < 0.5;
    let fres = select(schlick(abs(dot(out_dir, s.n)), f.extra.x), 1.0, tir);
    return vec4f(s.body, 1.0 - (1.0 - fres) * (1.0 - f.absorption.a));
}

// A fluid's surface (WATER_RENDERING.md W1b; the Low water tier, which reads no copy of the
// scene): still geometry, ripples in the shading. From outside, Schlick's Fresnel mixes the sky's
// reflection off the rippled normal with the fluid's lit body at its opacity, plus the sun's (and
// moon's) GGX glint and sparkle; the alpha leaves what is behind seen through the share the
// surface neither reflects nor hides. From inside: `from_below`. A see-through face that is no
// fluid's draws its vertex colour at half opacity.
@fragment
fn water_fs(i: FluidOut, @builtin(front_facing) front: bool) -> SceneOut {
    let draw = draws[i.draw];
    // Derivatives first, in uniform control flow: how many metres a pixel covers, and the cloud
    // deck's footprint in the reflection.
    let footprint = max(length(dpdx(i.wrapped)), length(dpdy(i.wrapped)));
    let deck_fw = reflected_deck_fw(i.rel);
    let count = arrayLength(&fluids);
    var o: SceneOut;
    o.depth = view_depth(i.rel);
    if (i.fluid >= count) {
        let n = normalize(i.normal);
        o.color = vec4f(lambert(i.color * draw.tint.rgb, select(-n, n, front), frame.eye.x + i.rel.y), 0.5);
        return o;
    }
    let f = fluids[i.fluid];
    let s = water_surface(i, front, footprint, f, draw);
    if (!front) {
        o.color = from_below(s, f);
        return o;
    }
    let fres = schlick(s.ndv, f.extra.x);
    let t = 1.0 - f.absorption.a;
    let alpha = 1.0 - (1.0 - fres) * t;
    let refl = vec4f(reflected_env(reflected_dir(s), i.rel, deck_fw), 0.0);
    o.color = vec4f((surface_light(i, s, f, fres, footprint, refl) + (1.0 - fres) * (1.0 - t) * s.body) / alpha, alpha);
    return o;
}

// The length of the view ray in the fluid (render/water.rs `ray_thickness`).
fn ray_thickness(d_behind: f32, d_water: f32, rel_len: f32) -> f32 {
    if (d_behind <= 0.0) {
        return DEEP_M;
    }
    return min(max(d_behind - d_water, 0.0) * rel_len / max(d_water, 1e-4), DEEP_M);
}

// The water over the scene behind it (WATER_RENDERING.md W3; the Medium and High tiers, drawn in
// each scene pass's water half): the scene, from the copy taken after the pass's opaque draws,
// shifted by the ripples (refraction, undistorted where the shifted sample is in front of the
// water); Beer–Lambert absorption over the ray's length in the fluid and the light's way down to
// what it shows, with the fluid's lit body in-scattered over the rest; caustics on that floor;
// shoreline foam where the water is thin; then the sky's reflection, glint and sparkle. The whole
// view through the surface, so alpha 1 (from inside: `from_below`, the W4 stub, as on Low).
@fragment
fn water_scene_fs(i: FluidOut, @builtin(front_facing) front: bool) -> @location(0) vec4f {
    let draw = draws[i.draw];
    let footprint = max(length(dpdx(i.wrapped)), length(dpdy(i.wrapped)));
    let deck_fw = reflected_deck_fw(i.rel);
    let count = arrayLength(&fluids);
    if (i.fluid >= count) {
        let n = normalize(i.normal);
        return vec4f(lambert(i.color * draw.tint.rgb, select(-n, n, front), frame.eye.x + i.rel.y), 0.5);
    }
    if (water_off(DBG_FLAT)) {
        return vec4f(0.02, 0.1, 0.2, 1.0);
    }
    let f = fluids[i.fluid];
    let s = water_surface(i, front, footprint, f, draw);
    if (!front) {
        return from_below(s, f);
    }
    let fres = schlick(s.ndv, f.extra.x);
    let size = vec2f(textureDimensions(scene_color));
    let px = i.clip.xy;
    let d_w = max(view_depth(i.rel), 1e-3);
    let rel_len = length(i.rel);
    // Sample 0 of the view depth: one load (the mean over samples costs four, for edges only).
    let behind0 = textureLoad(scene_depth, vec2i(px), 0).r;
    // Refraction: the ripples' slope shifts what is seen, less where the water is thin (none at
    // the waterline, where a shifted sample would land on the bank).
    let thin = min(ray_thickness(behind0, d_w, rel_len), 1.0);
    let lean = vec3f(-s.w.x, 0.0, -s.w.y) * (f.optics.x * thin);
    let to_px = size.y / (2.0 * frame.tan.y * d_w);
    let shift = vec2f(dot(lean, frame.right.xyz), -dot(lean, frame.up.xyz)) * to_px;
    let q = clamp(px + shift, vec2f(0.5), size - 0.5);
    var behind_q = behind0;
    var use_q = false;
    if (!water_off(DBG_REFRACTION)) {
        behind_q = textureLoad(scene_depth, vec2i(q), 0).r;
        use_q = behind_q <= 0.0 || behind_q > d_w;
    }
    let uv = select(px, q, use_q) / size;
    let d_b = select(behind0, behind_q, use_q);
    let scene = textureSampleLevel(scene_color, scene_sampler, uv, 0.0);
    // What is behind, and how far under the surface it is.
    let length_in = ray_thickness(d_b, d_w, rel_len);
    let floor_rel = i.rel * (d_b / d_w);
    let under = select(DEEP_M, max((i.rel.y - floor_rel.y) * s.up, 0.0), d_b > 0.0);
    var floor = s.body;
    if (scene.a > 0.0 && d_b > 0.0) {
        floor = scene.rgb / scene.a;
    }
    if (scene.a > 0.0 && d_b > 0.0 && !water_off(DBG_CAUSTICS)) {
        // The floor's pixel footprint: the surface's, scaled out to the floor's distance.
        let floor_fp = footprint * d_b / d_w;
        floor *= caustic_light(i.wrapped + floor_rel.xz - i.rel.xz, under, frame.eye.x + floor_rel.y, floor_fp, 1.0, f);
    }
    var through = mix(floor, s.body, 0.5);
    if (!water_off(DBG_ABSORPTION)) {
        let t = exp(-(f.absorption.rgb + f.extra.w) * length_in);
        let down = exp(-f.absorption.rgb * min(under, DEEP_M));
        through = floor * t * down + s.body * (1.0 - t);
    }
    // The reflection: land by screen-space reflection, else the sky and its clouds.
    let r = reflected_dir(s);
    let refl = ssr_reflected(reflected_env(r, i.rel, deck_fw), ssr_trace(i.rel, r, size), size);
    var color = surface_light(i, s, f, fres, footprint, refl) + (1.0 - fres) * through;
    // Shoreline foam where the water is thin.
    var foam = 0.0;
    if (!water_off(DBG_FOAM)) {
        foam = foam_amount(i.wrapped, under, f);
    }
    if (foam > 0.0) {
        let lit = lambert(f.foam.rgb, vec3f(0.0, s.up, 0.0), s.frag_y);
        color = mix(color, lit, foam);
    }
    return vec4f(color, 1.0);
}

const DEEP_M: f32 = 1e4;

// Shoreline foam: where the water over what is behind is under the
// fluid's foam depth, thin drifting lines lapping toward the shore, broken up by noise, densest at
// the waterline.
fn foam_amount(xz: vec2f, under: f32, f: Fluid) -> f32 {
    let depth = f.foam.a;
    if (depth <= 0.0 || under >= depth) {
        return 0.0;
    }
    let edge = 1.0 - under / depth;
    let t = frame.fog.z;
    let n_cells = round(WRAP_M / FOAM_CELL);
    let drift = vec2f(0.13, 0.09) * t / FOAM_CELL;
    let q = xz / FOAM_CELL + (drift - n_cells * floor(drift / n_cells));
    let n = noise_d(q, i32(n_cells)).x + 0.5 * noise_d(q * 2.0 + vec2f(0.5, 0.25), i32(2.0 * n_cells)).x;
    // Lines that move toward the shore (edge rises toward it) as time passes.
    let lap = fract(edge * FOAM_LINES - t * FOAM_LAP_SPEED + n * 0.8);
    let line = smoothstep(0.0, 0.18, lap) * (1.0 - smoothstep(0.3, 0.55, lap));
    let breakup = smoothstep(-0.15, 0.35, n);
    // At the waterline (where the water meets the bank, a thin line on screen): a soft rim.
    let rim = smoothstep(0.55, 1.0, edge) * smoothstep(-0.35, 0.25, n);
    return clamp(line * breakup * 0.45 * edge + rim * 0.8, 0.0, 0.8);
}

const FOAM_CELL: f32 = 0.9;
const FOAM_LINES: f32 = 1.6;
const FOAM_LAP_SPEED: f32 = 0.12;

// Caustics (WATER_RENDERING.md W3, brought forward from W4): the light the ripples focus on what is
// `under` metres below the surface, at wrapped world `xz`, as a multiplier of its light (1: none).
// The pattern is shifted back along the refracted sun (or moon, much dimmer), animated with the
// ripples' flow, and fades with depth over the fluid's caustic depth (render/water.rs
// `caustic_fade`). Focused, not added: dimmer between the lines, brighter on them.
fn caustic_light(xz: vec2f, under: f32, frag_y: f32, footprint: f32, boost: f32, f: Fluid) -> f32 {
    // Faded out where a pixel covers a good part of a cell (they would alias into noise).
    let sharp = 1.0 - smoothstep(0.08, 0.3, footprint / max(f.optics.y, 1e-3));
    if (under <= 0.0 || f.extra.y <= 0.0 || sharp <= 0.0) {
        return 1.0;
    }
    let up = select(-1.0, 1.0, frag_y >= MIDPLANE_Y);
    let sun_k = smoothstep(0.0, 0.25, frame.sun.y * up);
    let moon_k = smoothstep(0.0, 0.25, frame.moon.y * up) * MOON_CAUSTICS;
    let k = max(sun_k, moon_k);
    if (k <= 0.0) {
        return 1.0;
    }
    let l = select(frame.moon.xyz, frame.sun.xyz, sun_k >= moon_k);
    // Where the light reaching here crossed the surface: back along the refracted ray.
    let h = length(l.xz);
    let sin_t = h / f.wave.x;
    let tan_t = sin_t / sqrt(max(1.0 - sin_t * sin_t, 1e-3));
    let back = select(vec2f(0.0), l.xz / max(h, 1e-4) * (under * tan_t), h > 1e-4);
    let fade = exp(-under / max(f.optics.z, 1e-3)) * smoothstep(0.0, 0.35, under);
    let strength = f.extra.y * k * fade * sharp * boost;
    // Too deep (or too far) to see: skip the pattern.
    if (strength < 0.02) {
        return 1.0;
    }
    let c = caustic_pattern(xz + back, f, footprint);
    return max(1.0 + strength * (c * CAUSTIC_GAIN - CAUSTIC_MEAN), 0.0);
}

const MOON_CAUSTICS: f32 = 0.2;
// Seen from under the surface the caustics are the show: brighter than through it from above,
// where the surface's own reflection and glint compete with them.
const UNDERWATER_CAUSTICS: f32 = 1.7;
const CAUSTIC_GAIN: f32 = 2.2;
const CAUSTIC_MEAN: f32 = 0.2;

// Two layers of moving cell edges (Worley F2 − F1), warped so the lines wander, wrapping with the
// position (cells divide WRAP_M, render/water.rs `caustic_period`): bright thin lines, near 0
// between them.
fn caustic_pattern(xz: vec2f, f: Fluid, footprint: f32) -> f32 {
    let n = max(1.0, round(WRAP_M / max(f.optics.y, 1e-3)));
    let p = xz / (WRAP_M / n);
    let t = frame.fog.z * (0.5 + 12.0 * f.wave.w);
    let warp = 0.11 * vec2f(sin(6.2831853 * p.y + t), sin(6.2831853 * p.x + 1.37 * t));
    let a = cell_edge(p + warp, i32(n), t);
    var c = exp(-a * 11.0);
    // The finer layer only near, where a pixel is small against its cells.
    let fine = 1.0 - smoothstep(0.04, 0.12, footprint / max(f.optics.y, 1e-3));
    if (fine > 0.0) {
        let b = cell_edge(p * 2.0 + vec2f(0.37, 0.71) - 2.0 * warp, i32(2.0 * n), 1.31 * t + 2.0);
        c += 0.6 * fine * exp(-b * 9.0);
    }
    return c;
}

// The distance from the nearest cell edge of moving Worley points (F2 − F1), wrapping every
// `period` cells.
fn cell_edge(p: vec2f, period: i32, t: f32) -> f32 {
    let cell = floor(p);
    let fp = p - cell;
    let ci = vec2i(cell);
    var d1 = 9.0;
    var d2 = 9.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let o = vec2i(x, y);
            let g = hash_unit2(ci + o, period);
            let pt = vec2f(o) + 0.5 + 0.38 * vec2f(sin(t + 6.2831853 * g.x), cos(0.83 * t + 6.2831853 * g.y));
            let d = length(pt - fp);
            if (d < d1) {
                d2 = d1;
                d1 = d;
            } else if (d < d2) {
                d2 = d;
            }
        }
    }
    return d2 - d1;
}
";

/// `water_slope` in WGSL: the surface's summed height slopes (xy) and the slope variance lost to
/// fading (z), one `wave_layer` per layer of render/water.rs `LAYERS` (unrolled: the layer table
/// is constants); past `layers` (the quality's), a layer is all lost (its slope widens the glint).
fn water_slope() -> String {
    let mut s = String::from(
        "fn water_slope(xz: vec2f, f: Fluid, footprint: f32, layers: i32) -> vec3f {\n    var sum = vec3f(0.0);\n",
    );
    for (k, l) in super::water::LAYERS.iter().enumerate() {
        s.push_str(&format!(
            "    if (layers > {k} && !water_off({bit}u)) {{\n        sum += wave_layer(xz, f, footprint, {:?}, {:?}, {:?}, vec2f({:?}, {:?}), mat2x2f({:.1}, {:.1}, {:.1}, {:.1}));\n    }} else {{\n        sum.z += f.wave.y * f.wave.y * {:?};\n    }}\n",
            l.wavelength,
            l.slope,
            l.speed,
            l.dir[0],
            l.dir[1],
            f64::from(l.turn[0][0]),
            f64::from(l.turn[0][1]),
            f64::from(l.turn[1][0]),
            f64::from(l.turn[1][1]),
            l.slope * l.slope,
            bit = 1u32 << k
        ));
    }
    s.push_str("    return sum;\n}\n");
    s
}

/// The scene targets the composite and the lines read (group 2), for `samples` per pixel: the
/// resolved colour, and the view depth, multisampled when the scene is (r32float can't be
/// resolved).
fn scene_bindings(samples: u32) -> String {
    let depth = if samples > 1 {
        "texture_multisampled_2d<f32>"
    } else {
        "texture_2d<f32>"
    };
    format!(
        "const SCENE_SAMPLES: i32 = {samples};\n\
         const SAMPLE_SLOTS: u32 = {samples}u;\n\
         const DIFF_DEPTH: f32 = {:e};\n\
         const DIFF_DELTA_E: f32 = {:?};\n\
         @group(2) @binding(0) var scene_color: texture_2d<f32>;\n\
         @group(2) @binding(1) var scene_depth: {depth};\n\
         @group(3) @binding(0) var scene_b_color: texture_2d<f32>;\n\
         @group(3) @binding(1) var scene_b_depth: {depth};\n\
         @group(3) @binding(2) var<storage, read_write> diff_counts: array<atomic<u32>, 4>;\n\
         @group(2) @binding(2) var scene_sampler: sampler;\n",
        super::compare::DEPTH_TOLERANCE,
        super::compare::DELTA_E,
    )
}

/// The whole shader module for a scene of `samples` per pixel: constants, uniforms, the sky, the
/// shading and every entry point.
pub fn source(samples: u32) -> String {
    format!(
        "const MIDPLANE_Y: f32 = {:.1};\nconst SEA_LEVEL: f32 = {:.1};\nconst ATLAS_CELLS: u32 = {}u;\nconst ATLAS_TILES: u32 = {}u;\n{}\n{FRAME}\n{}\n{}\n{}\n{COMMON}\n{TERRAIN}\n{MOTION}\n{WATER}\n{}",
        world::MIDPLANE_Y as f64,
        world::SEA_LEVEL as f64,
        dwell_mesh::ATLAS_CELLS,
        dwell_mesh::ATLAS_TILES,
        format!("{}\n{}", super::water::wgsl(), super::reflection::wgsl()),
        scene_bindings(samples),
        dwell_look::sky_wgsl(),
        dwell_look::clouds::cloud_wgsl(&dwell_look::clouds::CLOUDS),
        water_slope()
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_shader_module_parses_and_validates() {
        // With and without multisampling (the scene depth's texture type differs).
        for samples in [1, 4] {
            let module =
                naga::front::wgsl::parse_str(&super::source(samples)).expect("WGSL parses");
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
                "line_top_fs",
                "sky_vs",
                "composite_fs",
                "fluid_vs",
                "water_fs",
                "water_scene_fs",
                "difference_fs",
            ] {
                assert!(
                    module.entry_points.iter().any(|e| e.name == entry),
                    "{entry}"
                );
            }
        }
    }

    #[test]
    fn tone_mapping_and_fog_are_only_in_the_composite() {
        // The scene passes write linear colour: their fragment shaders never tone-map, encode or
        // fog; the composite does all three (RAY_MARCHED_RENDERER.md A1).
        let src = super::source(4);
        let body = |name: &str| -> String {
            let at = src.find(&format!("fn {name}(")).expect(name);
            let rest = &src[at..];
            rest[..rest.find("\n}\n").expect("end")].to_string()
        };
        for entry in ["terrain_fs", "flat_fs", "scene_out"] {
            let b = body(entry);
            for f in ["aces(", "srgb_encode(", "display(", "fog_amount(", "sky("] {
                assert!(!b.contains(f), "{entry} calls {f}");
            }
        }
        let c = body("composited_of");
        assert!(c.contains("display(") && c.contains("fog_amount("));
        assert!(body("composited_over").contains("composited_of("));
        let c = body("composited");
        assert!(
            c.contains("sky(") && c.contains("dwell_clouds(") && c.contains("dwell_cloud_deck(")
        );
        // Both compare-mode sides are composited as the frame is, before they are compared.
        let d = body("difference_fs");
        assert_eq!(d.matches("composited_of(").count(), 2);
        assert!(body("composite_fs").contains("composited("));
    }

    #[test]
    fn terrain_fogs_toward_the_clear_sky_and_clouds_only_where_the_sky_shows() {
        // Distant ground fades into the sky without the clouds (they are above the haze): the fog
        // mixes toward `clear`; the clouds go into `back`, under the scene by its coverage.
        let src = super::source(1);
        let body = |name: &str| {
            let at = src.find(&format!("fn {name}(")).expect(name);
            src[at..at + src[at..].find("\n}\n").expect("end")].to_string()
        };
        let of = body("composited_of");
        assert_eq!(of.matches("fog_amount(").count(), 1);
        assert!(of.contains(", clear, fog_amount("));
        assert!(of.contains("mix(back, shown,"));
        // The deck's footprint is taken before any branch (derivatives need uniform control flow).
        let c = body("composited");
        let deck = c.find("dwell_cloud_deck(").expect("deck");
        assert!(
            deck < c.find("if (").expect("branch")
                && c.find("fwidth(").expect("fw") < c.find("if (").unwrap()
        );
    }

    #[test]
    fn the_wind_is_drawn_by_the_composite_only_while_there_is_any() {
        let src = super::source(4);
        let at = src.find("fn composite_fs(").expect("composite");
        let c = &src[at..at + src[at..].find("\n}\n").expect("end")];
        assert!(c.contains("if (frame.wind_point.z > 1e-3)") && c.contains("motion_composite("));
        // Procedural: no textures beyond the scene's own colour, which the edge blur samples.
        let m = &src[src.find("fn motion_hash(").expect("motion")..];
        assert!(!m.contains("textureSample("));
        assert!(
            m.contains("composited_over("),
            "the blur samples the composited scene"
        );
    }
}
