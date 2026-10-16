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
    // x fog sigma, y fog scale height, z seconds (the water's scroll), w unused
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
    // x Fresnel F0 (from the IOR), y caustics, z the surface drop (cells), w unused
    extra: vec4f,
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
";

/// The per-draw array (group 1): the raster passes'. The marcher's module leaves it out and binds
/// its own data there.
const DRAWS: &str = r"
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

// Schlick's Fresnel (render/water.rs `schlick`).
fn schlick(cos: f32, f0: f32) -> f32 {
    let m = pow(1.0 - clamp(cos, 0.0, 1.0), 5.0);
    return f0 + (1.0 - f0) * m;
}

// The underwater fog, the W4 stub (WATER_RENDERING.md W4 builds the full underwater view): over
// `dist` metres of the fluid, Beer–Lambert absorption of what is behind and the fluid's in-scatter,
// lit as the face's up is.
fn underwater(lin: vec3f, dist: f32, f: Fluid) -> vec3f {
    let t = exp(-f.absorption.rgb * dist);
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
    let shown = display(underwater(c.rgb / c.a, min(dist, to_surface), f));
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

// A fluid's surface (WATER_RENDERING.md W1b): still geometry, ripples in the shading. From
// outside, Schlick's Fresnel mixes the sky's reflection off the rippled normal with the fluid's
// lit body at its opacity, plus the sun's (and moon's) GGX glint; the alpha leaves what is behind
// seen through the share the surface neither reflects nor hides. From inside (the W4 stub): the
// body's colour, and total internal reflection outside Snell's window. A see-through face that is
// no fluid's draws its vertex colour at half opacity.
@fragment
fn water_fs(i: FluidOut, @builtin(front_facing) front: bool) -> SceneOut {
    let draw = draws[i.draw];
    // Derivatives first, in uniform control flow: how many metres a pixel covers.
    let footprint = max(length(dpdx(i.wrapped)), length(dpdy(i.wrapped)));
    let count = arrayLength(&fluids);
    let f = fluids[min(i.fluid, count - 1u)];
    let v = normalize(-i.rel);
    let face_n = normalize(i.normal);
    // The face's normal toward the eye's side (outward from outside, inward from inside).
    let seen = select(-face_n, face_n, front);
    let frag_y = frame.eye.x + i.rel.y;
    let on_a = frag_y >= MIDPLANE_Y;
    var o: SceneOut;
    o.depth = view_depth(i.rel);
    if (i.fluid >= count) {
        o.color = vec4f(lambert(i.color * draw.tint.rgb, seen, frag_y), 0.5);
        return o;
    }
    // Ripples on the surface (tops and bottoms; sides stay flat), toward the side seen: on face B
    // the mirrored surface's normal points down, and so does the rippled one.
    var w = vec3f(0.0);
    var n = seen;
    if (abs(face_n.y) > 0.5) {
        w = water_slope(i.wrapped, f, footprint, select(WAVE_LAYERS, 1, frame.water.w < 0.5));
        n = sign(seen.y) * normalize(vec3f(-w.x, 1.0, -w.y));
        // A ripple can't face away from the eye: bend it back.
        n = normalize(n + max(0.02 - dot(n, v), 0.0) * v);
    }
    let ndv = clamp(dot(n, v), 1e-3, 1.0);
    let f0 = f.extra.x;
    let t = 1.0 - f.absorption.a;
    // The body's in-scattered light: the water is lit from above (the face's up), whichever side
    // it is seen from.
    let face_up = vec3f(0.0, select(-1.0, 1.0, on_a), 0.0);
    let body = lambert(f.scatter.rgb * draw.tint.rgb, face_up, frag_y);
    if (front) {
        let fres = schlick(ndv, f0);
        var r = reflect(-v, n);
        // Reflected below the surface's plane: folded back up (the surface would hide it).
        r = normalize(r - 2.0 * min(dot(r, seen), 0.0) * seen);
        let a = glint_alpha(f.scatter.a, w.z);
        let glint = ggx_glint(n, v, frame.sun.xyz, a, f0) * frame.sun_color.rgb * own(frame.sun.y, on_a)
            + ggx_glint(n, v, frame.moon.xyz, a, f0) * frame.moon_color.rgb * own(frame.moon.y, on_a);
        let alpha = 1.0 - (1.0 - fres) * t;
        o.color = vec4f((fres * sky_radiance(r) + (1.0 - fres) * (1.0 - t) * body + glint) / alpha, alpha);
        return o;
    }
    // From inside: out through Snell's window, else totally reflected (the body's colour).
    let out_dir = refract(-v, n, f.wave.x);
    let tir = dot(out_dir, out_dir) < 0.5;
    let fres = select(schlick(abs(dot(out_dir, n)), f0), 1.0, tir);
    o.color = vec4f(body, 1.0 - (1.0 - fres) * t);
    return o;
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
            "    if (layers > {k}) {{\n        sum += wave_layer(xz, f, footprint, {:?}, {:?}, {:?}, vec2f({:?}, {:?}), mat2x2f({:.1}, {:.1}, {:.1}, {:.1}));\n    }} else {{\n        sum.z += f.wave.y * f.wave.y * {:?};\n    }}\n",
            l.wavelength,
            l.slope,
            l.speed,
            l.dir[0],
            l.dir[1],
            f64::from(l.turn[0][0]),
            f64::from(l.turn[0][1]),
            f64::from(l.turn[1][0]),
            f64::from(l.turn[1][1]),
            l.slope * l.slope
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
         @group(3) @binding(2) var<storage, read_write> diff_counts: array<atomic<u32>, 4>;\n",
        super::compare::DEPTH_TOLERANCE,
        super::compare::DELTA_E,
    )
}

/// The whole shader module for a scene of `samples` per pixel: constants, uniforms, the sky, the
/// shading and every entry point.
pub fn source(samples: u32) -> String {
    format!(
        "{}{DRAWS}\n{}\n{TERRAIN}\n{MOTION}\n{WATER}\n{}",
        lighting_source(),
        scene_bindings(samples),
        water_slope()
    )
}

/// The shared lighting (RAY_MARCHED_RENDERER.md §2: one `lambert`, `aces`, `fog_amount`, `sky` for
/// both renderers): the constants, the fluids, the frame uniforms (group 0: `frame`, `atlas`,
/// `atlas_sampler`), the sky, the clouds and the shading functions, without any pass's bindings or
/// entry points. The marcher's shade module is this plus its own code.
pub fn lighting_source() -> String {
    format!(
        "const MIDPLANE_Y: f32 = {:.1};\nconst SEA_LEVEL: f32 = {:.1};\nconst ATLAS_CELLS: u32 = {}u;\nconst ATLAS_TILES: u32 = {}u;\n{}\n{FRAME}\n{}\n{}\n{COMMON}\n",
        world::MIDPLANE_Y as f64,
        world::SEA_LEVEL as f64,
        dwell_mesh::ATLAS_CELLS,
        dwell_mesh::ATLAS_TILES,
        super::water::wgsl(),
        dwell_look::sky_wgsl(),
        dwell_look::clouds::cloud_wgsl(&dwell_look::clouds::CLOUDS),
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
