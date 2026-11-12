//! The renderer's WGSL, generated so the sky function, the midplane and sea level come from their
//! one source (dwell-look, the protocol constants). It reproduces the Three.js look (Phase 7):
//! three's Lambert shading with a sun or moon and a hemisphere light per face (each fragment takes
//! only its own face's lights), ACES filmic tone mapping with exposure, sRGB encoding, then the
//! height fog mixed toward the sky's colour in output space, as three's fog was.
//!
//! The frame is shared (docs/WATER_RENDERING.md W2): the scene passes write lit, linear
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
    // The ink outline (dwell-look `outline_uniforms`; docs/ART_DIRECTION.md §7): x the edge kinds
    // (bits: 1 silhouettes, 2 creases, 4 block edges; 0 off), y the taps' distance (px), z and w
    // the distance fade's start and end (m); the ink (sRGB) and its strength; x the silhouette and
    // y the crease threshold; z 1 to ink only the near side of an edge (the pixel-art preview's
    // 1-pixel edge).
    outline: vec4f,
    outline_ink: vec4f,
    outline_cfg: vec4f,
    // The pixel-art preview (dwell-look `pixel_art::light_uniforms`; 0s when off): x the direct
    // light's toon levels, y the ambient's (0: smooth), z the shadow hue's strength; the shadow
    // tint (linear, luminance 1).
    pixel_art: vec4f,
    pixel_tint: vec4f,
    // The lines (`line_vs`): x, y the drawing size (px), z a line's width (px: CSS px × the pixel
    // ratio, render/mod.rs `line_width_px`), so outlines read the same near and far.
    lines: vec4f,
    // The light shafts (ADR 0064; dwell-look `shaft_uniforms`): shafts xy the light's NDC point in
    // the air (under a fluid y the cells' drift, m/s), z the weight (0: none drawn), w the mode (0
    // none, 1 air, 2 under a fluid); cfg x the samples, y the reach (a share of the way; under: m),
    // z the decay (under: the fade depth, m), w the disc's radius (half screen heights; under: the
    // cells' size, m); color rgb the light's (sRGB), w Snell's window's softened rim; window rgb
    // its glow (sRGB), w how much of the window it is.
    shafts: vec4f,
    shafts_cfg: vec4f,
    shafts_color: vec4f,
    shafts_window: vec4f,
    // The placed and held lights' shafts (dwell-look `light_shaft_uniforms`): [0] x the count (0:
    // none), y the samples toward each, z the reach, w the decay; per light its NDC point, depth
    // along the view (m) and halo radius (half screen heights), then its glow (sRGB × weight).
    torch_shafts: array<vec4f, 9>,
    // x eye height (world y), y face (±1), z exposure (× the eye's adaptation's), w the eye's face's
    // fog density (× its time of day's; the other face's is day.other_stars.y)
    eye: vec4f,
    // x fog sigma, y fog scale height, z seconds (the water's scroll, the plants' wind), w the
    // plants' wind scale (0: still)
    fog: vec4f,
    // The sky's view rays: the camera's axes and the half-angle tangents (x, y); z, w the eye's
    // world x and z modulo WIND_PERIOD (the gust field's place).
    right: vec4f,
    up: vec4f,
    forward: vec4f,
    tan: vec4f,
    // The sky's clouds (dwell-look's cloud_uniforms): the noise point under the eye and the warp
    // field's offset (cells); coverage, octaves (0: off), self-shadow, the deck's height above the
    // eye (negative above it); how much the eye is in cloud.
    clouds: vec4f,
    cloud_cfg: vec4f,
    cloud_inside: vec4f,
    // The other face's deck (as `clouds` and `cloud_cfg`, in its frame): what a ray that crosses
    // the midplane meets (docs/DAY_NIGHT.md §13).
    clouds_o: vec4f,
    cloud_cfg_o: vec4f,
    // The eye under a fluid's drawn surface: x 1 (else 0), y fluid id, z the surface's world y;
    // w the water tier's switches (dwell-look `tier_bits`, read by `water_tier_*`).
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
    // xy the eye's world x and z wrapped to [0, WRAP_M) (render/water.rs `wrapped_origin`); z the
    // water's cost-isolation switches (render/water.rs `DEBUG_SWITCHES`; 0 in play).
    wrap: vec4f,
    // The water's reflections for its quality (dwell-look `reflection_uniforms`): x march steps,
    // y refine steps, z reach (m), w where hits start fading along it (m); then the cloud
    // octaves (0: the sky alone), the deck's self-shadow (0 or 1), the edge fade (share of the
    // screen height) and the hit thickness (m).
    reflect: vec4f,
    reflect_sky: vec4f,
    // The tree being chopped leans toward its notch (crate::chop `TreeLean`, mod.rs
    // `lean_uniforms`): the hinge (relative to the eye) and the radius; the tree's up and its
    // height; the turn's axis and angle (radians; 0: nothing leans).
    lean_hinge: vec4f,
    lean_up: vec4f,
    lean_axis: vec4f,
    // The far trees' hand-over (render/impostors.rs, dwell-worldgen `impostor::Far`): x where it
    // starts, y its fade (m), z where far trees stop, w 1 when impostors draw (else the near
    // trees' leaves draw to the full-detail distance as before).
    far_trees: vec4f,
    // The day/night cycle (dwell-look `DaySky`): the sun, moon and stars, the viewer's sky and each
    // face's lights.
    day: DaySky,
};
@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;
// An atlas uv moved to its level-0 texel's centre: with anisotropic filtering on, the sampler's
// magnification is linear (WebGPU wants every filter linear), and sampling at texel centres keeps
// texels crisp up close as the nearest filter did; far away the footprint spans many texels and
// the half-texel move is lost in it (render/mod.rs `atlas_sampler_descriptor`).
fn atlas_crisp(uv: vec2f) -> vec2f {
    let size = vec2f(textureDimensions(atlas));
    return (floor(uv * size) + 0.5) / size;
}
// Atlas gradients with the long axis capped so an anisotropic footprint stays inside the tile's
// wrapped gutter: the sampler picks the level from the short axis (2^L texels), where the gutter is
// GUTTER / 2^L texels, and spreads its taps along the long axis; keeping that axis within
// 2 × GUTTER level-0 texels (or the short axis, past it) keeps the taps off the neighbouring tiles
// and the atlas's empty space, the grey grid far ground showed at a grazing angle.
fn atlas_grads(gx: vec2f, gy: vec2f) -> array<vec2f, 2> {
    let size = vec2f(textureDimensions(atlas));
    let lx = length(gx * size);
    let ly = length(gy * size);
    let cap = max(2.0 * f32(ATLAS_GUTTER), min(lx, ly));
    return array<vec2f, 2>(gx * min(1.0, cap / max(lx, 1e-12)), gy * min(1.0, cap / max(ly, 1e-12)));
}
// Per atlas tile (index cx + cy × ATLAS_CELLS): its grid lines (x every, y strength; 0 for none),
// from the registry's `grid` look option of the blocks that use the tile, z its glow, w how close
// to the eye (m) its faces dissolve (block data `nearFade`: leaves; dwell-mesh tile_grids).
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
    // Its flow look (shared/look/waterflow.json; zeros: it shows none): x how far ripples are
    // carried per metre the water moves, y the carry's cycle (s), z and w the current where foam
    // streaks begin and are full (m/s)
    flow: vec4f,
    // The streaks' colour (linear), a their strength at full
    flow_foam: vec4f,
    // x a streak's length along the flow, y its width across (m), z the shore foam's bands, w the
    // seconds one takes to roll out
    flow_shape: vec4f,
    // The shore foam (WATER_RENDERING.md §14): x the contact line's width (m), y a band's width
    // as it is born (m), z the foam's most, w how far noise varies the bands (0..1)
    shore: vec4f,
    // Its underwater look (shared/look/underwater.json, WATER_RENDERING.md §12): x the lens (m
    // ahead of the eye: the plane where under or above is decided), y the fog's clear distance
    // (m), z the fog's density (× its rates), w the band's depth (m on the lens plane)
    under: vec4f,
    // x the line's wave height (m), y its wavelength (m), z its cycles a second, w the underside's
    // texture strength
    line: vec4f,
    // rgb the band's tint (a multiplier, output colour), a its streaks' strength
    band: vec4f,
    // x the dark rim's width (px), y its shade (×), z the bright line's width (px), w its glow (×)
    rim: vec4f,
    // x the band's glints, y how broken its ripples are, z how far (px) they shift and blur its
    // mirror of the scene above, w the most of it that mirror is
    band_extra: vec4f,
    // The underside seen from below: x how broken its cells are (0: the caustic web; 1: bright
    // cells, dark gaps), y the sun patch's strength, z its tightness (a power), w the cells'
    // scale against the floor's caustics (a whole number, so the pattern still wraps)
    underside: vec4f,
    // Snell's window seen from below (WATER_RENDERING.md §16): x the share of the underside's
    // cells over the sky in it, y the sun's and moon's discs' strength in it; zw spare
    window: vec4f,
};
@group(0) @binding(4) var<storage, read> fluids: array<Fluid>;
// The water's tiles (render/water_tiles.rs): gradient noise and its derivatives over NOISE_TILE
// lattice cells (value, d/dx, d/dy), and per fluid this frame's caustic cell-edge distances over
// CAUSTIC_TILE cells (the coarse layer, the fine one); a repeating linear sampler.
@group(0) @binding(5) var noise_tile: texture_2d<f32>;
@group(0) @binding(6) var caustic_tile: texture_2d_array<f32>;
@group(0) @binding(7) var tile_sampler: sampler;

// Per draw (group 1, a storage array indexed by the draw's instance: draw k is drawn as instance k,
// so a draw needs no bind call of its own): the mesh's origin relative to the eye (xyz) and its scale (w),
// an optional clip box (relative to the eye; clip_lo.w > 0 enables it), the LOD cross-fade in
// clip_hi.w (`fade_discard`), and a tint (rgb) and opacity (a).
struct Draw {
    offset: vec4f,
    clip_lo: vec4f,
    clip_hi: vec4f,
    tint: vec4f,
    // x, y: cos and sin of a turn about +y (players' yaw); identity (1, 0) otherwise.
    rotation: vec4f,
    // The draw's own rotation about its offset, a unit quaternion (x, y, z, w): a falling piece's
    // (docs/FELLING.md); (0, 0, 0, 1) for the rest.
    quat: vec4f,
};

// `v` turned by the unit quaternion `q`.
fn qrot(q: vec4f, v: vec3f) -> vec3f {
    let t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}
";

/// The view depth as the frame stores it (docs/ART_DIRECTION.md §7.4): its lowest bit is the
/// ink flag, set where the surface takes no ink outline (block data `outline: false`: the cross
/// plants), and the next the self-lit flag (`self_lit_depth`, docs/LIGHTS.md §2), set where the
/// surface gives off its own colour (a torch's flame), which the time of day's desaturation leaves
/// alone. Two units in the last place (about 2e-7 of the depth) are far below anything a reader
/// resolves, so the readers that don't ask are unchanged. **Every writer of the view depth goes
/// through `frame_depth`** (a test checks it), or the flags would be its depth's own noise.
const FRAME_DEPTH: &str = r"
fn frame_depth(d: f32, inked: bool) -> f32 {
    let b = bitcast<u32>(d) & ~3u;
    return bitcast<f32>(select(b | 1u, b, inked));
}

// A stored view depth flagged as a self-lit surface's (or not).
fn self_lit_depth(d: f32, self_lit: bool) -> f32 {
    let b = bitcast<u32>(d) & ~2u;
    return bitcast<f32>(select(b, b | 2u, self_lit));
}

// Whether a stored view depth is a self-lit surface's.
fn frame_self_lit(d: f32) -> bool {
    return (bitcast<u32>(d) & 2u) != 0u;
}

// Whether a stored view depth takes the ink (nothing drawn, 0, does).
fn frame_inked(d: f32) -> bool {
    return (bitcast<u32>(d) & 1u) == 0u;
}
";

/// The per-draw array (group 1): the raster passes'.
const DRAWS: &str = r"
@group(1) @binding(0) var<storage, read> draws: array<Draw>;
";

/// The LOD cross-fade (ARCHITECTURE.md §6.6 "Swaps", dwell-lod fade.rs): a draw's `clip_hi.w` is
/// 0 when it draws in full, 1 + t while fading in (it keeps the pixels whose `fade_threshold` is
/// under t) and −(1 + t) while fading out (it keeps the rest), so the two levels of a swap share
/// the pixels exactly (dwell-lod `Fade::keeps`, `Fade::shader_value`). A pixel of a level fading
/// out takes no ink (`frame_depth`'s flag): the outline's edges come from the incoming level, and
/// the dither's seams between the levels' depths never draw as lines.
const FADE: &str = r"
fn fade_discard(draw: Draw, p: vec2f) -> bool {
    let w = draw.clip_hi.w;
    if (w == 0.0) {
        return false;
    }
    let t = abs(w) - 1.0;
    let n = fade_threshold(p);
    return select((n < t), (n >= t), (w > 0.0));
}

fn fade_inked(draw: Draw) -> bool {
    return draw.clip_hi.w >= 0.0;
}
";

const COMMON: &str = r"
const PI: f32 = 3.141592653589793;

// A light's weight on `face` (its frame's sign, `face_at`): its height over that face's horizon,
// smoothly (dwell_look::light_weight).
fn own(light: vec3f, face: f32) -> f32 {
    return smoothstep(-0.1, 0.1, face_rise(face, light));
}

// three's Lambert: (direct + hemisphere irradiance) × albedo / π, with the lights of the fragment's
// own face (BIFACIAL_WORLD.md §6): its time of day's sun, moon and sky, each direct light weighted
// by its height over that face's horizon.
fn lambert(albedo: vec3f, n: vec3f, frag_y: f32) -> vec3f {
    return lambert_lit(albedo, n, frag_y, open_light());
}

// How a surface is lit (docs/DAY_NIGHT.md §9–10; dwell_look::ambient::shading mirrors it): the
// share of the hemisphere light, of the direct sun and moon, and the block light's glow; and the
// dynamic lights' irradiance (`point_lights`, docs/LIGHTS.md §3), set where the fragment's place
// and normal are known (`scene_out`, the water, the characters).
struct TerrainLight {
    ambient: f32,
    direct: f32,
    glow: f32,
    points: vec3f,
};

// A surface open to the sky (LOD sections, players, water's own pieces).
fn open_light() -> TerrainLight {
    return TerrainLight(1.0, 1.0, 0.0, vec3f(0.0));
}

// How close to the eye (m) faces with the tile at `tile_origin` dissolve (0: never): leaves, so a
// canopy round the eye shows without faces against the lens (docs/FELLING.md).
fn tile_near_fade(tile_origin: vec2f) -> f32 {
    let cell = vec2u(clamp(floor(tile_origin * f32(ATLAS_CELLS)), vec2f(0.0), vec2f(f32(ATLAS_CELLS - 1u))));
    return tile_grids[cell.x + cell.y * ATLAS_CELLS].w;
}

// How much of its colour the tile at `tile_origin` gives off unlit (its material's `emissive`,
// docs/LIGHTS.md): a torch's flame.
fn tile_glow(tile_origin: vec2f) -> f32 {
    let cell = vec2u(clamp(floor(tile_origin * f32(ATLAS_CELLS)), vec2f(0.0), vec2f(f32(ATLAS_CELLS - 1u))));
    return abs(tile_grids[cell.x + cell.y * ATLAS_CELLS].z);
}

// Whether the tile at `tile_origin` is molten (its material's `molten`: lava; dwell-mesh
// `tile_glow_entry` stores its glow negated) and the tier moves its heat (not the CPU tier).
fn tile_molten(tile_origin: vec2f) -> bool {
    let cell = vec2u(clamp(floor(tile_origin * f32(ATLAS_CELLS)), vec2f(0.0), vec2f(f32(ATLAS_CELLS - 1u))));
    return tile_grids[cell.x + cell.y * ATLAS_CELLS].z < 0.0 && water_tier_molten();
}

// Lava's slow heat (WATER_RENDERING.md §17): the crust drifts at MOLTEN_DRIFT blocks a second
// and two slow waves over the world brighten and dim its glowing cracks, 0.7–1.4 × the glow.
const MOLTEN_DRIFT: vec2f = vec2f(0.012, 0.007);
const MOLTEN_SPAN: f32 = 3.0;
fn molten_heat(world_xz: vec2f, t: f32) -> f32 {
    let a = sin(dot(world_xz, vec2f(0.41, 0.23)) - 0.45 * t);
    let b = sin(dot(world_xz, vec2f(-0.17, 0.37)) + 0.31 * t + 1.7);
    return 1.05 + 0.35 * (0.6 * a + 0.4 * b);
}

// Lava's crust plates (WATER_RENDERING.md §17.1): a cellular pattern in world metres, continuous
// across blocks: plates MOLTEN_CELL m across whose seams (the distance to the second-nearest
// plate's centre less the nearest's) glow. The plates' centres wander slowly (flowing crust) and
// the plane is warped a little so the seams bend. Returns x the seam (1 on it, 0 inside a plate),
// y a per-plate value (0–1, to vary the crust's darkness).
const MOLTEN_CELL: f32 = 1.7;
const MOLTEN_SEAM: f32 = 0.11;
const MOLTEN_OPEN: f32 = 0.35;
fn molten_hash2(c: vec2f) -> vec2f {
    let p = vec2f(dot(c, vec2f(127.1, 311.7)), dot(c, vec2f(269.5, 183.3)));
    return fract(sin(p) * 43758.5453);
}
fn molten_plates(world: vec2f, t: f32, crust_cover: f32) -> vec2f {
    let warp = vec2f(sin(world.y * 0.9 + t * 0.21), sin(world.x * 0.8 - t * 0.17)) * 0.22;
    let p = (world + warp) / MOLTEN_CELL;
    let base = floor(p);
    var d1 = 9.0;
    var d2 = 9.0;
    var id = vec2f(0.0);
    for (var j = -1; j <= 1; j++) {
        for (var i = -1; i <= 1; i++) {
            let c = base + vec2f(f32(i), f32(j));
            let h = molten_hash2(c);
            let centre = c + 0.5 + 0.38 * sin(6.2831853 * h + t * 0.08);
            let d = length(p - centre);
            if (d < d1) {
                d2 = d1;
                d1 = d;
                id = h;
            } else if (d < d2) {
                d2 = d;
            }
        }
    }
    let gap = (d2 - d1) * MOLTEN_CELL;
    // The seams widen as the crust thins (its stop's `crust`): hot lava is mostly molten channels
    // between small plates, crusting lava all plate with hairline seams.
    let w = (MOLTEN_SEAM + MOLTEN_OPEN * (1.0 - crust_cover)) * (0.7 + 0.6 * id.y);
    return vec2f(1.0 - smoothstep(w * 0.4, w, gap), id.x);
}

// A level's brightness on `curve` (shared/look/light.json): x / (curve − (curve − 1)·x).
fn light_curve(x: f32, curve: f32) -> f32 {
    let c = clamp(x, 0.0, 1.0);
    return c / (curve - (curve - 1.0) * c);
}

// A terrain vertex's light: its baked `light` (corner occlusion × sky visibility, dwell-mesh
// sky.rs) and its flood light (sky and block level as shares of 15, dwell-mesh flood.rs). The
// hemisphere light takes the sky light's brightness, the baked light shading it as much as the
// vertex is open to the sky (dwell_look::ambient::open_share), above the floor; the sun
// and moon the sky light's direct ramp, the shadow maps' stand-in; the block light
// glows by its own curve. In the scene passes the direct share is the shadow maps' near the eye,
// fading to this stand-in past their last cascade (`scene_out`, `shadow_light`).
fn terrain_light(light: f32, flood: vec2f) -> TerrainLight {
    let sky = light_curve(flood.x, SKY_CURVE);
    let l = clamp(light, 0.0, 1.0);
    let level = flood.x * 15.0;
    let direct = smoothstep(DIRECT_FROM, DIRECT_FULL, level);
    return TerrainLight(
        AMBIENT_FLOOR + (1.0 - AMBIENT_FLOOR) * sky * (l + (1.0 - l) * (1.0 - sky)),
        direct,
        light_curve(flood.y, BLOCK_CURVE) * BLOCK_STRENGTH,
        vec3f(0.0),
    );
}

// `lambert` lit by `l` (`terrain_light`; `open_light()` for surfaces with no baked light): the
// hemisphere light by `l.ambient`, the sun and moon by `l.direct`, the block light's warm glow and
// the dynamic lights (`l.points`).
fn lambert_lit(albedo: vec3f, n: vec3f, frag_y: f32, l: TerrainLight) -> vec3f {
    let face = face_at(frag_y);
    let on_a = face > 0.0;
    let d = frame.day;
    let sun = select(d.sun_b.rgb, d.sun_a.rgb, on_a);
    let moon = select(d.moon_b.rgb, d.moon_a.rgb, on_a);
    let hemi_sky = select(d.sky_b.rgb, d.sky_a.rgb, on_a);
    let hemi_ground = select(d.ground_b.rgb, d.ground_a.rgb, on_a);
    // The sun's and moon's share (N·L, over the face's horizon, by the shadow), on the pixel-art
    // preview's toon levels when it is on (`band`; smooth otherwise).
    let ks = band(max(dot(n, d.sun.xyz), 0.0) * own(d.sun.xyz, face) * l.direct, frame.pixel_art.x);
    let km = band(max(dot(n, d.moon.xyz), 0.0) * own(d.moon.xyz, face) * l.direct, frame.pixel_art.x);
    let direct = ks * sun + km * moon;
    let ambient = band(l.ambient, frame.pixel_art.y);
    // The preview's hue-shifted shadows: the sky light where the sun doesn't reach takes the
    // tint's hue (the block light and torches keep theirs).
    let shade = frame.pixel_art.z * (1.0 - clamp(ks, 0.0, 1.0));
    let hemi = mix(hemi_ground, hemi_sky, face_rise(face, n) * 0.5 + 0.5) * ambient
        * mix(vec3f(1.0), frame.pixel_tint.rgb, shade);
    let irradiance = direct + hemi + BLOCK_LIGHT * (PI * l.glow) + l.points;
    return irradiance * albedo / PI;
}

// A light share `x` on `n` toon levels from dark to lit (0 or 1: smooth); dwell-look
// `pixel_art::band`.
fn band(x: f32, n: f32) -> f32 {
    if (n < 1.5) {
        return x;
    }
    let k = n - 1.0;
    return round(clamp(x, 0.0, 1.0) * k) / k;
}

// Whether `lambert_lit` gives a surface with normal `n` (at world y `frag_y`) any direct sun or
// moon light: the shadow factor multiplies nothing where it doesn't.
fn lit_by_direct(n: vec3f, frag_y: f32) -> bool {
    let face = face_at(frag_y);
    let d = frame.day;
    return dot(n, d.sun.xyz) * own(d.sun.xyz, face) > 0.0
        || dot(n, d.moon.xyz) * own(d.moon.xyz, face) > 0.0;
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

// The sky's colour toward `dir` (output colour): what far terrain fogs to and water reflects.
fn sky(dir: vec3f) -> vec3f {
    return dwell_sky(dir, frame.eye.y, frame.day);
}

// Half-a-step dither for the 8-bit canvas (interleaved gradient noise, triangular): smooth night
// and twilight gradients without bands.
fn dither(p: vec2i) -> f32 {
    let q = vec2f(p);
    let a = fract(52.9829189 * fract(dot(q, vec2f(0.06711056, 0.00583715))));
    let b = fract(52.9829189 * fract(dot(q + vec2f(17.0, 59.0), vec2f(0.06711056, 0.00583715))));
    return (a + b - 1.0) / 255.0;
}

// The sky's light along `dir`, in the scene's linear colour.
fn sky_radiance(dir: vec3f) -> vec3f {
    return inverse_display(sky(dir));
}

// A water cost-isolation switch (`?waterdbg=`, render/water.rs `DEBUG_SWITCHES`): true turns the
// piece off. Uniform per frame, so its branches don't diverge.
fn water_off(bit: u32) -> bool {
    return (u32(frame.wrap.z) & bit) != 0u;
}

// Schlick's Fresnel (render/water.rs `schlick`).
fn schlick(cos: f32, f0: f32) -> f32 {
    let m = pow(1.0 - clamp(cos, 0.0, 1.0), 5.0);
    return f0 + (1.0 - f0) * m;
}

// The underwater fog, the W4 stub (WATER_RENDERING.md W4 builds the full underwater view): over
// `dist` metres of the fluid, Beer–Lambert absorption of what is behind and the fluid's in-scatter,
// lit as the face's up is.
// The underwater look's fog: none over its clear distance, then its rates × its density.
fn underwater(lin: vec3f, dist: f32, f: Fluid) -> vec3f {
    let d = max(dist - f.under.y, 0.0) * f.under.z;
    let t = exp(-(f.absorption.rgb + f.extra.w) * d);
    let inscatter = lambert(f.scatter.rgb, face_up(frame.eye.y), frame.eye.x);
    return lin * t + inscatter * (1.0 - t);
}

// The water tier's switches (shared/look/watertiers.json; dwell-look `tier_bits`): bit 0 the scene
// copy is read, bit 1 flow streaks, bit 2 the bank band, bits 3-4 the ripple layers.
fn water_tier_bits() -> u32 {
    return u32(frame.water.w + 0.5);
}
fn water_reads_scene() -> bool {
    return (water_tier_bits() & 1u) != 0u;
}
fn water_tier_streaks() -> bool {
    return (water_tier_bits() & 2u) != 0u;
}
fn water_tier_band() -> bool {
    return (water_tier_bits() & 4u) != 0u;
}
fn water_tier_layers() -> i32 {
    return clamp(i32((water_tier_bits() >> 3u) & 3u), 1, WAVE_LAYERS);
}
// Bit 5: the caustic tile's lines on the water in the blended pass; bits 6-9: the share of the
// fluid's opacity drawn there, in fifteenths.
fn water_tier_caustics() -> bool {
    return (water_tier_bits() & 32u) != 0u;
}
fn water_tier_opacity() -> f32 {
    return f32((water_tier_bits() >> 6u) & 15u) / 15.0;
}
// Bit 10: molten surfaces (lava) move their heat (every tier but the CPU's: flat, still glow).
fn water_tier_molten() -> bool {
    return (water_tier_bits() & 1024u) != 0u;
}

// The fluid the eye is under or just above (`frame.water`).
fn eye_fluid() -> Fluid {
    return fluids[min(u32(frame.water.y), arrayLength(&fluids) - 1u)];
}

// The eye itself under a fluid's surface (`frame.water.x` 1 deep, 2 near the surface; 3 just
// above it; render/mod.rs `waterline_state`).
fn eye_under() -> bool {
    return frame.water.x > 0.5 && frame.water.x < 2.5;
}

// The waterline (WATER_RENDERING.md §12; render/water.rs `waterline_m` is its CPU twin): how far
// (m, along the face's up) the pixel's point on the lens plane (the fluid's lens ahead of the eye
// along the camera's forward axis: `ray`, the view ray at view depth 1, times the lens) is above
// the eye's fluid's surface, less the line's waves there. Negative: the pixel looks out from
// under the surface. The lens points lie on a plane, so the line is straight at any eye height
// and roll, and moves smoothly with the eye.
fn waterline_m(ray: vec3f, f: Fluid) -> f32 {
    let face = frame.eye.y;
    let lens = ray * f.under.x;
    let xz = frame.wrap.xy + lens.xz;
    let k = 6.2831853 / f.line.y;
    let t = 6.2831853 * f.line.z * frame.fog.z;
    let wave = f.line.x * (0.6 * sin(xz.x * k + t) + 0.4 * sin((0.6 * xz.x + 0.8 * xz.y) * 1.7 * k - 1.3 * t));
    return face_y(face, frame.eye.x + lens.y) - face_y(face, frame.water.z) - wave;
}

// The height fog (dwell-look's haze_amount, per pixel) in the eye's face's air: how much of the
// sky behind to mix in, in output colour. `rel` is the point relative to the eye.
fn fog_amount(rel: vec3f) -> f32 {
    let face = frame.eye.y;
    let cam = face_y(face, frame.eye.x);
    return dwell_haze(cam, cam + face_rise(face, rel), length(rel), frame.eye.w);
}

// The air colour a point at `rel` from the eye fades toward (aerial perspective, dwell-look
// fog.rs `air_direction` and `air_tint`): its clear sky `clear`, or with the fog's aerial look the
// sky at least HAZE_LIFT (a sine) above the horizon in its azimuth, warmer toward the sun and cooler
// away by HAZE_SUN_TINT. HAZE_AERIAL is a constant: off, this is `clear`, the fade as before.
fn haze_air(rel: vec3f, clear: vec3f) -> vec3f {
    if (!HAZE_AERIAL) {
        return clear;
    }
    let up = face_up(frame.eye.y);
    let dir = normalize(rel);
    let e = dot(dir, up);
    let level = dir - e * up;
    let len = length(level);
    var air = clear;
    if (e < HAZE_LIFT && len > 1e-6) {
        air = sky(level / len * sqrt(1.0 - HAZE_LIFT * HAZE_LIFT) + HAZE_LIFT * up);
    }
    let sun = frame.day.sun.xyz;
    let sh = sun - dot(sun, up) * up;
    let lens = length(sh) * len;
    let toward = select(0.0, dot(level, sh) / max(lens, 1e-6), lens > 1e-6);
    let tint = mix(HAZE_COOL, HAZE_WARM, clamp(0.5 + 0.5 * toward, 0.0, 1.0));
    return air * (1.0 + HAZE_SUN_TINT * (tint - 1.0));
}

// `c` (output colour) `dist` m off veiled before the haze (dwell-look fog.rs `veiled`): toward its
// grey by HAZE_VEIL_SATURATION and its darks toward the air colour `air` by HAZE_VEIL_CONTRAST, half
// of each by HAZE_VEIL_HALF metres. Off (HAZE_AERIAL false), `c`.
fn haze_veiled(c: vec3f, air: vec3f, dist: f32) -> vec3f {
    if (!HAZE_AERIAL) {
        return c;
    }
    let k = 1.0 - exp2(-dist / HAZE_VEIL_HALF);
    let lum = vec3f(0.2126, 0.7152, 0.0722);
    let d = mix(c, vec3f(dot(c, lum)), HAZE_VEIL_SATURATION * k);
    let dark = clamp(1.0 - dot(d, lum) / max(dot(air, lum), 1e-3), 0.0, 1.0);
    return mix(d, air, HAZE_VEIL_CONTRAST * k * dark);
}

// `c` (output colour) at `rel` from the eye, fogged as the world is, not the viewer
// (docs/DAY_NIGHT.md §13): toward the eye's face's clear sky `clear` by its fog over the part of
// the ray in its air, and where the ray crosses the midplane (a point seen down a shaft or past the
// rim) first toward the other face's sky by the other face's fog over the part in its air.
fn fogged(c: vec3f, rel: vec3f, clear: vec3f) -> vec3f {
    let face = frame.eye.y;
    let cam = face_y(face, frame.eye.x);
    let mid = face_y(face, FACE_MIDPLANE_Y);
    let rise = face_rise(face, rel);
    if (cam + rise >= mid || rise >= 0.0) {
        let air = haze_air(rel, clear);
        return mix(haze_veiled(c, air, length(rel)), air, fog_amount(rel));
    }
    let dist = length(rel);
    let near = clamp((cam - mid) / -rise, 0.0, 1.0);
    let own = dwell_haze(cam, mid, dist * near, frame.eye.w);
    let o = -face;
    let far = dwell_haze(face_y(o, FACE_MIDPLANE_Y), face_y(o, frame.eye.x + rel.y), dist * (1.0 - near), frame.day.other_stars.y);
    let other_clear = dwell_sky(normalize(rel), o, dwell_other_day(frame.day));
    return mix(mix(c, other_clear, far), clear, own);
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

fn scene_out(albedo: vec3f, n: vec3f, rel: vec3f, alpha: f32, light: TerrainLight) -> SceneOut {
    var o: SceneOut;
    // The shadow maps near the eye, the flood light's direct share beyond their last cascade.
    var l = light;
    // A face turned away from both lights takes no direct light, so no lookup.
    if (lit_by_direct(n, frame.eye.x + rel.y)) {
        l.direct = shadow_light(rel, n, l.direct);
    }
    // The dynamic lights (held torches), unshadowed.
    l.points = point_lights(rel, n);
    o.color = vec4f(lambert_lit(albedo, n, frame.eye.x + rel.y, l), alpha);
    o.depth = frame_depth(view_depth(rel), true);
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
    // The baked ambient light (dwell-mesh sky.rs): corner occlusion × sky visibility.
    @location(5) light: f32,
    // The flood light (dwell-mesh flood.rs): sky and block level as shares of 15.
    @location(6) flood: vec2f,
    // The crack overlay (docs/BLOCK_DAMAGE.md §6.3; dwell-mesh `CrackLayer::pack`, unorm8x4): the
    // crack tile's s and t over [−½, 1½], its atlas slot + 1 (/255; 0: none) and strength.
    @location(7) crack: vec4f,
};
struct TerrainOut {
    @builtin(position) clip: vec4f,
    @location(0) rel: vec3f,
    @location(1) normal: vec3f,
    @location(2) color: vec3f,
    @location(3) uv: vec2f,
    @location(4) tile: vec4f,
    @location(5) @interpolate(flat) draw: u32,
    @location(6) light: f32,
    @location(7) flood: vec2f,
    // The crack tile's s and t (0–1 on the tile), its atlas slot + 1 and strength (constant over
    // a face).
    @location(8) crack: vec4f,
};

@vertex
fn terrain_vs(v: TerrainIn, @builtin(instance_index) k: u32) -> TerrainOut {
    let draw = draws[k];
    var o: TerrainOut;
    o.draw = k;
    o.rel = tree_lean(draw.offset.xyz + qrot(draw.quat, v.position * draw.offset.w));
    o.clip = frame.view_proj * vec4f(o.rel, 1.0);
    o.normal = qrot(draw.quat, v.normal);
    o.color = v.color;
    o.uv = v.uv;
    o.tile = v.tile;
    o.light = v.light;
    o.flood = v.flood;
    o.crack = vec4f(v.crack.xy * CRACK_UV_RANGE + CRACK_UV_MIN, v.crack.z * 255.0, v.crack.w);
    return o;
}

// The crack overlay (docs/BLOCK_DAMAGE.md §6.3): a damaged block's faces multiplied by its crack
// stage's tile (white, with dark cracks), centred on the hit. One texture lookup, on the faces of
// cracked cells only (a branch constant over each face); `crack` as TerrainOut carries it. The
// tile's level from its own uv's footprint; past the tile (a far corner from the hit) no crack.
const CRACK_UV_MIN: f32 = -0.5;
const CRACK_UV_RANGE: f32 = 2.0;
fn crack_shade(crack: vec4f, gx: vec2f, gy: vec2f) -> vec3f {
    let slot = u32(round(crack.z));
    let st = crack.xy;
    if (slot == 0u || any(st < vec2f(0.0)) || any(st > vec2f(1.0))) {
        return vec3f(1.0);
    }
    let cell = slot - 1u;
    let size = vec2f(textureDimensions(atlas));
    let pitch = size / f32(ATLAS_CELLS);
    let tile = (pitch - 2.0 * f32(ATLAS_GUTTER)) / size;
    let origin = (vec2f(f32(cell % ATLAS_CELLS), f32(cell / ATLAS_CELLS)) * pitch + f32(ATLAS_GUTTER)) / size;
    let grads = atlas_grads(gx * tile, gy * tile);
    let texel = textureSampleGrad(atlas, atlas_sampler, atlas_crisp(origin + st * tile), grads[0], grads[1]);
    return mix(vec3f(1.0), texel.rgb, crack.w);
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
    // A molten tile (lava) drifts its crust slowly and breathes its cracks' heat.
    let molten = tile_molten(i.tile.xy);
    // Its tile spans MOLTEN_SPAN blocks (the tile wraps), so the crust plates are metres across
    // and don't repeat block by block (at one block a tile they read as a checkerboard).
    let span = select(1.0, 1.0 / MOLTEN_SPAN, molten);
    let drift = select(vec2f(0.0), MOLTEN_DRIFT * frame.fog.z, molten);
    let face_n = select(-i.normal, i.normal, front);
    let n = face_n;
    let atlas_uv = i.tile.xy + fract(i.uv * span + drift) * i.tile.zw;
    let grads = atlas_grads(dpdx(i.uv) * span * i.tile.zw, dpdy(i.uv) * span * i.tile.zw);
    let texel = textureSampleGrad(atlas, atlas_sampler, atlas_crisp(atlas_uv), grads[0], grads[1]);
    let shade = grid_shade(i.uv, tile_grid(i.tile.xy));
    if (fade_discard(draw, i.clip.xy)) {
        discard;
    }
    let alpha = select(draw.tint.a * texel.a, 1.0, draw.tint.a >= 1.0);
    // A molten block's faces carry its heat stop in their colour (k, −1, −1; dwell-mesh
    // `vertex_rgb`, VOLCANOES.md §6.5) and are white otherwise.
    let coded = i.color.g < 0.0;
    let vcol = select(i.color, vec3f(1.0), coded);
    let stop = select(0u, min(u32(max(i.color.r, 0.0)), LAVA_STOPS - 1u), coded);
    let look = lava_stop_look[stop];
    let core = lava_stop_core[stop];
    // The crack overlay's footprint, taken in uniform control flow.
    let crack_gx = dpdx(i.crack.xy);
    let crack_gy = dpdy(i.crack.xy);
    var albedo = vcol * texel.rgb * shade * draw.tint.rgb * crack_shade(i.crack, crack_gx, crack_gy);
    // Lava (§17.1): world-space crust plates over the painted tile, thin glowing seams between
    // them, continuous across blocks; a top face takes the world's x z, a side its along and y.
    var crack = smoothstep(0.25, 0.75, max(texel.r, texel.g));
    if (molten) {
        let world = vec3f(i.rel.x + frame.tan.z, i.rel.y + frame.eye.x, i.rel.z + frame.tan.w);
        let side = abs(n.y) < 0.5;
        let plane = select(world.xz, vec2f(world.x + world.z, world.y * 1.3), side);
        let plate = molten_plates(plane, frame.fog.z, core.w);
        // The tile's hue (orange) for the seams; the crust its darkest, one shade per plate with
        // the painted strokes faint in it.
        // The seams glow in the heat stop's colour (shared/fluids/lava.json `heat`); the crust is
        // darker the more of the surface it covers.
        let hue = texel.rgb / max(max(texel.r, texel.g), 0.05);
        // The stop's core colour only at the middle of the seams.
        let seam = mix(look.rgb, core.rgb, smoothstep(0.75, 1.0, plate.x) * (1.0 - core.w));
        let grain = dot(texel.rgb, vec3f(0.3, 0.5, 0.2));
        let crust = look.rgb * (0.04 + 0.03 * plate.y + 0.12 * (1.0 - core.w)) * (0.7 + 0.6 * grain);
        albedo = mix(crust, seam, plate.x) * vcol * draw.tint.rgb;
        crack = plate.x;
    }
    let lit = terrain_light(i.light, i.flood);
    var o = scene_out(albedo, normalize(n), i.rel, alpha, lit);
    // The heat brightens the glowing seams (or a still tile's bright cracks), not the crust.
    let heat = select(1.0, mix(0.15, 1.1 * molten_heat(i.rel.xz + frame.tan.zw, frame.fog.z), crack), molten);
    let shine = tile_glow(i.tile.xy) * heat * select(1.0, mix(1.0, look.w, crack), molten);
    // Lava keeps its hue as it glows (the tone curve would wash a hot orange to mustard).
    let glowing = select(emitted(o.color.rgb, albedo * alpha, shine), emitted_hue(o.color.rgb, albedo * alpha, shine), molten);
    o.color = vec4f(glowing, o.color.a);
    o.depth = self_lit_depth(frame_depth(o.depth, fade_inked(draw)), tile_glow(i.tile.xy) >= 1.0);
    return o;
}

// Cutout geometry (docs/TRANSPARENT_BLOCKS.md): the terrain vertex and a `plant` vec4 — how far
// wind sways the vertex (m; 0 at a plant's base and on ladders), the wind's speed, and the distance
// fade's start and end (m; end 0: no fade).
struct CutoutIn {
    @location(0) position: vec3f,
    @location(1) normal: vec3f,
    @location(2) color: vec3f,
    @location(3) uv: vec2f,
    @location(4) tile: vec4f,
    @location(5) plant: vec4f,
    @location(6) bare: f32,
    @location(7) light: f32,
    @location(8) flood: vec2f,
};
struct CutoutOut {
    @builtin(position) clip: vec4f,
    @location(0) rel: vec3f,
    @location(1) normal: vec3f,
    @location(2) color: vec3f,
    @location(3) uv: vec2f,
    @location(4) tile: vec4f,
    @location(5) @interpolate(flat) draw: u32,
    @location(6) fade: vec2f,
    @location(7) @interpolate(flat) bare: f32,
    @location(8) light: f32,
    @location(9) flood: vec2f,
};

// The gust field: travelling waves whose wavelengths divide WIND_PERIOD, sampled at the world
// position modulo that period (the eye's, frame.tan.zw, plus the offset), so it is seamless and
// stays put as the eye moves. In [0, 1].
const WIND_PERIOD: f32 = 1024.0;
fn gust(p: vec2f, t: f32) -> f32 {
    let k = 2.0 * PI / WIND_PERIOD;
    let a = sin(dot(p, vec2f(27.0, 11.0) * k) - 0.9 * t);
    let b = sin(dot(p, vec2f(-13.0, 31.0) * k) - 0.6 * t + 1.7);
    let c = sin(dot(p, vec2f(61.0, 47.0) * k) - 1.7 * t + 4.1);
    return clamp(0.5 + 0.3 * a + 0.15 * b + 0.1 * c, 0.0, 1.0);
}

// The tree being chopped leans (frame.lean_*): a vertex above the hinge, within the radius across
// and the height up, turned about the axis through it (crate::chop `TreeLean::apply`).
fn tree_lean(rel: vec3f) -> vec3f {
    let a = frame.lean_axis.w;
    if (a == 0.0) {
        return rel;
    }
    let d = rel - frame.lean_hinge.xyz;
    let h = dot(d, frame.lean_up.xyz);
    let across = d - frame.lean_up.xyz * h;
    if (h <= 0.0 || h > frame.lean_up.w || length(across) > frame.lean_hinge.w) {
        return rel;
    }
    let k = frame.lean_axis.xyz;
    let c = cos(a);
    let s = sin(a);
    return frame.lean_hinge.xyz + d * c + cross(k, d) * s + k * dot(k, d) * (1.0 - c);
}

// A leaf card's `plant.y` is -(1 + its plane normal octahedral-packed on a 2048-step grid per
// axis, u * 2049 + v) (dwell-mesh leaves.rs `card_speed`, `CARD_GRID`): a plant's is its wind
// speed (>= 0), a card's 1.
fn plant_speed(plant: vec4f) -> f32 {
    return select(plant.y, 1.0, plant.y < 0.0);
}
fn card_normal(packed: f32) -> vec3f {
    let q = -packed - 1.0;
    let uq = floor(q / 2049.0);
    let vq = q - uq * 2049.0;
    let u = uq / 2048.0 * 2.0 - 1.0;
    let v = vq / 2048.0 * 2.0 - 1.0;
    var n = vec3f(u, 1.0 - abs(u) - abs(v), v);
    if (n.y < 0.0) {
        // Octahedral decoding's quadrant sign (not a face sign): +1 where the coordinate is >= 0.
        n = vec3f((1.0 - abs(v)) * (step(0.0, u) * 2.0 - 1.0), n.y, (1.0 - abs(u)) * (step(0.0, v) * 2.0 - 1.0));
    }
    return normalize(n);
}
// BUGS.md #78 (the M1 at High): a leaf card the eye can't see costs no pixels. Dropped when the
// eye is more than CARD_BACK_CULL_M behind its plane (the crown's far side, facing away; the
// plane's distance is the same at its four corners, so a card goes whole). Any plant (a card, a
// blade of tall grass) is dropped when its corner is past its fade's end by more than its size
// (2 m: every corner is then past the end, so it goes whole): the forest's edge drew every blade
// out to the full-detail distance, all discarded in the fragment stage (WORK_QUEUE item 32, the
// edge's 14 ms of cutout). `rel` the corner before the wind's sway, `n` the card's normal in the
// world.
fn card_dropped(rel: vec3f, n: vec3f, plant: vec4f) -> bool {
    if (plant.w > 0.0 && length(rel) > plant.w + 2.0) {
        return true;
    }
    // A tree's leaf face (`TREE_LEAF_FADE`) past the far trees' hand-over: the impostors' alone.
    if (plant.w < 0.0 && frame.far_trees.w > 0.5
        && length(rel) > frame.far_trees.x + frame.far_trees.y + 2.0) {
        return true;
    }
    if (plant.y >= 0.0) {
        return false;
    }
    return CARD_BACK_CULL_M > 0.0 && dot(n, rel) > CARD_BACK_CULL_M;
}

// The wind sways a vertex by its `plant.x` (m): leaning with the gusts, a faster flutter on top.
// frame.fog.z is the time (s), frame.fog.w the wind's scale (0 when effects are off).
fn sway(rel: vec3f, plant: vec4f) -> vec3f {
    let amount = plant.x * frame.fog.w;
    if (amount == 0.0) {
        return rel;
    }
    let t = frame.fog.z * plant_speed(plant);
    let p = frame.tan.zw + rel.xz;
    let g = gust(p, t);
    let flutter = sin(2.7 * t + dot(p, vec2f(0.73, 0.41))) * 0.25;
    let dir = normalize(vec2f(1.0, 0.35));
    let lean = amount * (0.25 + g + flutter * (0.4 + g));
    // Swaying shortens the plant a little, as a bent blade does (toward its own ground).
    return rel + vec3f(dir.x * lean, 0.0, dir.y * lean) + sway_drop(frame.eye.x + rel.y, lean, plant.x);
}

@vertex
fn cutout_vs(v: CutoutIn, @builtin(instance_index) k: u32) -> CutoutOut {
    let draw = draws[k];
    var o: CutoutOut;
    o.draw = k;
    let still = tree_lean(draw.offset.xyz + qrot(draw.quat, v.position * draw.offset.w));
    o.rel = sway(still, v.plant);
    o.clip = frame.view_proj * vec4f(o.rel, 1.0);
    if (card_dropped(still, qrot(draw.quat, card_normal(v.plant.y)), v.plant)) {
        // Outside the clip volume, the same point for every corner: a card dropped whole draws
        // nothing (one past the fade's margin only where all of it is faded out anyway).
        o.clip = vec4f(0.0, 0.0, -1.0, 1.0);
    }
    o.normal = qrot(draw.quat, v.normal);
    o.color = v.color;
    o.uv = v.uv;
    o.tile = v.tile;
    o.fade = v.plant.zw;
    o.bare = v.bare;
    o.light = v.light;
    o.flood = v.flood;
    return o;
}

// What a cutout fragment writes: the scene's outputs and the samples it covers.
struct CutoutSceneOut {
    @location(0) color: vec4f,
    @location(1) depth: f32,
    @builtin(sample_mask) mask: u32,
};

// The near trees' share of a pixel `dist` m off (`Far::near_share`): 1 nearer than the far
// trees' hand-over, falling to 0 across its fade (1 when no impostors draw).
fn near_tree_share(dist: f32) -> f32 {
    if (frame.far_trees.w < 0.5) {
        return 1.0;
    }
    return 1.0 - clamp((dist - frame.far_trees.x) / max(frame.far_trees.y, 1e-3), 0.0, 1.0);
}

// A per-pixel value in [0, 1) that varies from pixel to pixel (interleaved gradient noise).
fn pixel_noise(p: vec2f) -> f32 {
    return fract(52.9829189 * fract(dot(floor(p), vec2f(0.06711056, 0.00583715))));
}

// Alpha-tested texels (the atlas's alpha), with alpha-to-coverage done here: the texel's alpha is
// sharpened to its edge over one pixel (so a blade's edge is antialiased whatever the mip), times
// the distance fade, and covers that share of the pixel's samples, the samples picked per pixel so
// overlapping blades add up. Covered samples write alpha 1 (the frame is premultiplied by
// coverage). Without multisampling it is a plain test, dithered while fading. Drawn from both
// sides: the back of a face takes the normal mirrored in the horizontal only, so a plant's
// up-leaning normals stay lit from either side and a ladder's plate is lit as its own back.
@fragment
fn cutout_fs(i: CutoutOut, @builtin(front_facing) front: bool) -> CutoutSceneOut {
    let draw = draws[i.draw];
    let atlas_uv = i.tile.xy + fract(i.uv) * i.tile.zw;
    // The mip from the gradients with their anisotropy capped at 2: a quad seen edge-on would
    // otherwise sample a level so coarse its gaps are covered (coverage-preserving mips fill a
    // tile's last levels), drawing slivers of solid quad across the view.
    var gx = dpdx(i.uv) * i.tile.zw;
    var gy = dpdy(i.uv) * i.tile.zw;
    let cap = 2.0 * min(length(gx), length(gy));
    gx *= min(1.0, cap / max(length(gx), 1e-12));
    gy *= min(1.0, cap / max(length(gy), 1e-12));
    let grads = atlas_grads(gx, gy);
    let texel = textureSampleGrad(atlas, atlas_sampler, atlas_crisp(atlas_uv), grads[0], grads[1]);
    // Up close the atlas is sampled nearest (crisp texels), which would cut blade edges into
    // stairs: there the alpha is filtered bilinearly by hand from the four texels around the
    // sample (level 0), so edges are smooth curves; from afar (a texel under a pixel) the mips'.
    let size = vec2f(textureDimensions(atlas));
    let st = atlas_uv * size - 0.5;
    let f = fract(st);
    let g = textureGather(3, atlas, atlas_sampler, (floor(st) + 1.0) / size);
    let smooth_a = mix(mix(g.w, g.z, f.x), mix(g.x, g.y, f.x), f.y);
    let texels = max(length(dpdx(st)), length(dpdy(st)));
    let alpha = mix(texel.a, smooth_a, clamp(2.0 - 2.0 * texels, 0.0, 1.0));
    let edge = max(fwidth(alpha), 1e-3);
    var cover = clamp((alpha - 0.5) / edge + 0.5, 0.0, 1.0);
    var fade = 1.0;
    if (i.fade.y > 0.0) {
        fade = 1.0 - smoothstep(i.fade.x, i.fade.y, length(i.rel));
        cover *= fade;
    }
    // A tree's leaf face hands over to the far trees' impostors (`imp_fs` keeps the other pixels).
    if (i.fade.y < 0.0) {
        fade = near_tree_share(length(i.rel));
        cover *= fade;
    }
    // Near the eye a leaf's faces dissolve (dithered, as a fading plant), from a third of its
    // tile's near fade to all of it.
    let near = tile_near_fade(i.tile.xy);
    if (near > 0.0) {
        let k = smoothstep(near * 0.33, near, length(i.rel));
        cover *= k;
        fade = min(fade, k);
    }
    let noise = pixel_noise(i.clip.xy);
    var mask = 0xffffffffu;
    if (SCENE_SAMPLES > 1) {
        // Edges round to the nearest sample count; only a fading plant is dithered (rounding
        // edges by noise would scatter single samples of a blade's faint margin across the view).
        let round = select(0.5, noise, fade < 1.0);
        let n = u32(floor(cover * f32(SCENE_SAMPLES) + round));
        let full = (1u << u32(SCENE_SAMPLES)) - 1u;
        let first = u32(noise * f32(SCENE_SAMPLES)) % u32(SCENE_SAMPLES);
        let run = (1u << n) - 1u;
        mask = ((run << first) | (run >> (u32(SCENE_SAMPLES) - first))) & full;
        if (n == 0u) {
            discard;
        }
    } else if (cover <= noise) {
        discard;
    }
    if (fade_discard(draw, i.clip.xy)) {
        discard;
    }
    let n = select(vec3f(-i.normal.x, i.normal.y, -i.normal.z), i.normal, front);
    let albedo = i.color * texel.rgb * draw.tint.rgb;
    let lit = scene_out(albedo, normalize(n), i.rel, 1.0, terrain_light(i.light, i.flood));
    var o: CutoutSceneOut;
    // A glowing tile (a torch's flame) is self-lit: its colour whatever the light.
    let shine = tile_glow(i.tile.xy);
    o.color = vec4f(emitted(lit.color.rgb, albedo, shine), lit.color.a);
    // Plants whose look says `outline: false` are flagged in the depth: no ink is drawn on them
    // and the edge rule sees through them (`ink_outline`).
    o.depth = self_lit_depth(frame_depth(lit.depth, i.bare < 0.5 && fade_inked(draw)), shine >= 1.0);
    o.mask = mask;
    return o;
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
    if (clipped(draw, i.rel) || fade_discard(draw, i.clip.xy)) {
        discard;
    }
    let n = select(-i.normal, i.normal, front);
    // An LOD top takes the colour map's albedo where it is on (render/lod_map.rs).
    var albedo = i.color;
    if (draw.rotation.z > 0.5) {
        let face = face_at(frame.eye.x + i.rel.y);
        if (face_rise(face, normalize(n)) > 0.7) {
            albedo = lod_map_albedo(i.rel, face, i.color);
        }
    }
    // No baked light: LOD sections are lit as open to the sky (ARCHITECTURE.md §6.6), players too.
    var o = scene_out(albedo * draw.tint.rgb, normalize(n), i.rel, draw.tint.a, open_light());
    o.depth = frame_depth(o.depth, fade_inked(draw));
    return o;
}

// The shadow passes (render/shadows.rs; docs/DAY_NIGHT.md §10): depth only, from the light, with
// the vertex stages above (the cascade's frame uniform holds its matrix as `view_proj`). Draws
// that draw in full have no fragment stage; these are for the rest. A draw in an LOD swap keeps
// the map texels its dither keeps, so the two levels share the map as they share the screen (it
// holds all of one or the other); a stand-in keeps its clip box; plants keep their texels' alpha
// and fade by their distance from the eye as they do on screen, so their shadows fade with them.
// Casters drawn in full need only their depth: a vertex stage that reads the position alone
// (the vertex buffer's first attribute, its stride unchanged) and writes nothing else.
struct ShadowIn {
    @location(0) position: vec3f,
};

@vertex
fn shadow_terrain_vs(v: ShadowIn, @builtin(instance_index) k: u32) -> @builtin(position) vec4f {
    let draw = draws[k];
    return frame.view_proj * vec4f(draw.offset.xyz + qrot(draw.quat, v.position * draw.offset.w), 1.0);
}

@vertex
fn shadow_flat_vs(v: ShadowIn, @builtin(instance_index) k: u32) -> @builtin(position) vec4f {
    let draw = draws[k];
    return frame.view_proj * vec4f(draw.offset.xyz + turn(draw, v.position * draw.offset.w), 1.0);
}

@fragment
fn shadow_terrain_fs(i: TerrainOut) {
    if (fade_discard(draws[i.draw], i.clip.xy)) {
        discard;
    }
}

@fragment
fn shadow_flat_fs(i: FlatOut) {
    let draw = draws[i.draw];
    if (clipped(draw, i.rel) || fade_discard(draw, i.clip.xy)) {
        discard;
    }
}

@fragment
fn shadow_cutout_fs(i: CutoutOut) {
    let draw = draws[i.draw];
    let atlas_uv = i.tile.xy + fract(i.uv) * i.tile.zw;
    let alpha = textureSampleLevel(atlas, atlas_sampler, atlas_crisp(atlas_uv), 0.0).a;
    var fade = 1.0;
    if (i.fade.y > 0.0) {
        fade = 1.0 - smoothstep(i.fade.x, i.fade.y, length(i.rel));
    }
    if (alpha < 0.5 || fade <= pixel_noise(i.clip.xy) || fade_discard(draw, i.clip.xy)) {
        discard;
    }
}

struct LineIn {
    @location(0) position: vec3f,
    @location(1) color: vec3f,
    // The segment's other end (the mesh's space), and which side of it this corner is (±1).
    @location(2) other: vec3f,
    @location(3) side: f32,
};
struct LineOut {
    @builtin(position) clip: vec4f,
    @location(0) color: vec3f,
    @location(1) @interpolate(flat) draw: u32,
    @location(2) depth: f32,
};

// Lines (the block outline, the region tools' boxes, the brush preview, debug lines): unlit sRGB
// colours, drawn over the composite (so neither tone-mapped nor fogged) with the near pass's
// projection. Each segment is a quad a constant `frame.lines.z` pixels wide on screen, whatever its
// distance (owner, 2026-11-06: far outlines stay readable), with square caps half a width out so a
// box's corners close; its ends are cut at the near plane first. The math is `line_corner` in
// render/mod.rs, unit-tested there. Turned by the draw's own rotation about its offset, as meshes
// are: the outline of a moving body's cell turns with the body.
@vertex
fn line_vs(v: LineIn, @builtin(instance_index) k: u32) -> LineOut {
    let draw = draws[k];
    var o: LineOut;
    o.draw = k;
    var rel = draw.offset.xyz + qrot(draw.quat, v.position * draw.offset.w);
    let rel_o = draw.offset.xyz + qrot(draw.quat, v.other * draw.offset.w);
    var a = frame.view_proj * vec4f(rel, 1.0);
    let b = frame.view_proj * vec4f(rel_o, 1.0);
    // Behind the near plane (w ≤ 0 for this projection): slide the end toward the other.
    let near = 1e-4;
    if (a.w < near && b.w >= near) {
        let t = (near - a.w) / (b.w - a.w);
        a = mix(a, b, t);
        rel = mix(rel, rel_o, t);
    }
    let size = max(frame.lines.xy, vec2f(1.0));
    let pa = a.xy / max(a.w, near) * size;
    let pb = b.xy / max(b.w, near) * size;
    var d = pb - pa;
    let len = length(d);
    d = select(vec2f(1.0, 0.0), d / len, len > 1e-6);
    let half = 0.5 * frame.lines.z;
    let px = (vec2f(-d.y, d.x) * v.side - d) * half;
    // Pixels to NDC (2 / size), back to clip by w.
    a = vec4f(a.xy + px * 2.0 / size * a.w, a.zw);
    if (a.w < near) {
        a = vec4f(0.0, 0.0, -1.0, 1.0); // both ends behind: dropped
    }
    o.clip = a;
    o.color = v.color;
    o.depth = view_depth(rel);
    return o;
}

// A brush preview's ghost: its faces in the world, no widening.
@vertex
fn ghost_vs(v: GhostIn, @builtin(instance_index) k: u32) -> LineOut {
    let draw = draws[k];
    var o: LineOut;
    o.draw = k;
    let rel = draw.offset.xyz + qrot(draw.quat, v.position * draw.offset.w);
    o.clip = frame.view_proj * vec4f(rel, 1.0);
    o.color = v.color;
    o.depth = view_depth(rel);
    return o;
}
struct GhostIn {
    @location(0) position: vec3f,
    @location(1) color: vec3f,
};

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

// A brush preview's ghost (BRUSHES.md §3): see-through faces, at a third of their opacity where
// the scene is in front of them, so cells inside the ground still show.
@fragment
fn ghost_fs(i: LineOut) -> @location(0) vec4f {
    let draw = draws[i.draw];
    let p = vec2i(i.clip.xy);
    var shown = 0.0;
    for (var s = 0; s < SCENE_SAMPLES; s++) {
        let d = textureLoad(scene_depth, p, s).r;
        shown += select(0.0, 1.0, d <= 0.0 || i.depth <= d * (1.0 + 1e-5));
    }
    let k = shown / f32(SCENE_SAMPLES);
    return vec4f(i.color * draw.tint.rgb, draw.tint.a * (0.33 + 0.67 * k));
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
//
// The day/night cycle's light for the viewer's face: its sun, moon and sky light; the day pole is
// the sun (its height over the horizon sets day or night).
fn cloud_lighting() -> CloudLighting {
    return cloud_lighting_for(frame.eye.y);
}

// The clouds' light over face `face` (±1): its own sun, moon and sky light, whichever face the
// viewer is on (each deck is lit by its own face's: docs/DAY_NIGHT.md §13).
fn cloud_lighting_for(face: f32) -> CloudLighting {
    let d = frame.day;
    let pole = d.sun.xyz;
    let on_a = face > 0.0;
    let sky = select(d.sky_b.rgb, d.sky_a.rgb, on_a);
    return CloudLighting(pole, d.sun.xyz, d.moon.xyz, select(d.sun_b.rgb, d.sun_a.rgb, on_a), select(d.moon_b.rgb, d.moon_a.rgb, on_a), sky, sky);
}

// The scene's colour as the canvas shows it, at the time of day's saturation, but for the share
// `self_lit` of its samples that show a self-lit surface (a flame keeps its colour by moonlight).
fn scene_display(c: vec4f, self_lit: f32) -> vec3f {
    let lit = display(c.rgb / c.a);
    let saturation = mix(frame.day.horizon.a, 1.0, self_lit);
    return mix(vec3f(dot(lit, vec3f(0.2126, 0.7152, 0.0722))), lit, saturation);
}

// The share of the pixel's drawn samples that show a self-lit surface (`frame_self_lit`).
fn scene_self_lit_at(p: vec2i) -> f32 {
    var lit = 0.0;
    var n = 0.0;
    for (var s = 0; s < SCENE_SAMPLES; s++) {
        let d = textureLoad(scene_depth, p, s).r;
        if (d > 0.0) {
            n += 1.0;
            lit += select(0.0, 1.0, frame_self_lit(d));
        }
    }
    return select(0.0, lit / n, n > 0.0);
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
// — so the haze terrain fades into is the colour behind it (the clear sky: distant ground never
// takes a cloud's colour).
// The clouds (dwell-look's clouds.rs) go over the sky, and over the scene where the deck is in front
// of it (its hit nearer than the stored view depth: clouds below an eye above the deck, or before a
// peak seen through the deck). Last, inside the cloud, the whole view fogs toward the cloud.
// The sky behind is the clear sky with the sun, moon and stars (`dwell_sky_bodies`) under the
// clouds; the time of day sets the scene's saturation (lower by moonlight); dithered last.
// The composite of one pixel (`color`: the scene tone-mapped, encoded, fogged and over the sky)
// with the pixel's view ray (view depth 1), clear sky, sky with its clouds, and the clouds as a
// layer over the scene (`front`, with the deck's distance along the ray `deck_t`: no cover unless
// the deck is in front of the pixel's scene), which the ink reads.
struct Composited {
    color: vec3f,
    ray: vec3f,
    clear: vec3f,
    back: vec3f,
    front: vec4f,
    deck_t: f32,
};

// Whether the deck, met `t` along the pixel's unit ray, is in front of the opaque scene at view
// depth `depth` along `ray` (view depth 1; 0: the sky): its hit nearer, or, seen from above, the
// scene below the deck's altitude (dwell_cloud_in_front). Every composite test of the deck against
// the scene goes through here: the deck is a sphere over a flat world, so its hit alone loses
// the far sea from high up (owner, 2026-10-21: clouds in a circle and past the world's edge).
fn cloud_in_front_of(t: f32, ray: vec3f, depth: f32) -> bool {
    return dwell_cloud_in_front(t, length(ray), depth, face_rise(frame.eye.y, ray), frame.cloud_cfg.w);
}

// Each face's cloud deck hit along a pixel's ray with its footprint (`fw`, `fw_o`): the
// composite's derivatives, taken in uniform control flow before any per-pixel branch.
struct Decks {
    deck: vec4f,
    fw: f32,
    deck_o: vec4f,
    fw_o: f32,
};

fn composited_decks(ndc: vec2f) -> Decks {
    let dir = normalize(view_ray(ndc));
    let face = frame.eye.y;
    let deck = dwell_cloud_deck(dir, face, frame.clouds, frame.cloud_cfg.w);
    let deck_o = dwell_cloud_deck(dir, -face, frame.clouds_o, frame.cloud_cfg_o.w);
    return Decks(deck, max(fwidth(deck.x), fwidth(deck.y)), deck_o, max(fwidth(deck_o.x), fwidth(deck_o.y)));
}

fn composited(p: vec2i, ndc: vec2f, decks: Decks) -> Composited {
    let ray = view_ray(ndc);
    let dir = normalize(ray);
    let face = frame.eye.y;
    // The volumetric clouds (docs/CLOUDS.md §8) replace the eye's 2D deck here when the tier marches
    // (cloud_inside.w): the deck's weight goes to 0 and its hit to the eye, so the layer below is
    // the march's, in front of the scene out to the scene's distance. The sky pass's other users
    // (Snell's window, reflections) keep the cheap deck.
    let volume = frame.cloud_inside.w >= 1.0;
    var deck = decks.deck;
    if (volume) {
        deck = vec4f(deck.xy, 0.0, 0.0);
    }
    let fw = decks.fw;
    let deck_o = decks.deck_o;
    let fw_o = decks.fw_o;
    let clear = sky(dir);
    let lights = cloud_lighting();
    let c = textureLoad(scene_color, p, 0);
    var depth = 0.0;
    var self_lit = 0.0;
    if (c.a > 0.0) {
        depth = scene_depth_at(p).x;
        self_lit = scene_self_lit_at(p);
    }
    var vol = vec4f(0.0);
    if (volume) {
        vol = upsampled_clouds(ndc, ray, select(0.0, depth, c.a > 0.0));
    }
    var back = clear;
    if (c.a < 1.0) {
        back = dwell_cloud_over(sky_behind_with_clouds(dir, clear, deck, fw, deck_o, fw_o), vol);
    }
    var front = vol;
    if (!volume && c.a > 0.0 && cloud_in_front_of(deck.z, ray, depth)) {
        front = dwell_cloud_layer(clear, dir, face, deck, fw, frame.clouds, frame.cloud_cfg, lights);
    }
    var s = Composited(vec3f(0.0), ray, clear, back, front, deck.z);
    s.color = composited_scene(c, depth, self_lit, s);
    return s;
}

// The volume march's per-pixel step offset: the baked blue-noise tile (dwell_cloud_blue; BUGS.md
// #90: was interleaved gradient noise), turned each frame (a golden-ratio step over 16 frames) so
// the accumulation averages the steps' dither away (dwell-look cloud_jitter).
fn cloud_jitter(p: vec2i, frame_index: u32) -> f32 {
    return fract(dwell_cloud_blue(p) + f32(frame_index % 16u) * 0.618034);
}

// The volumetric clouds' accumulation (docs/CLOUDS.md §8.6): the last frame's camera (right, up,
// forward, the half-angle tangents) and params: x the share of the new frame (1: no history),
// y the frame index, z how far the eye moved since the last frame (m), w the pass's resolution
// divisor.
struct CloudPrev {
    right: vec4f,
    up: vec4f,
    forward: vec4f,
    tan: vec4f,
    params: vec4f,
};

struct CloudOut {
    @location(0) color: vec4f,
    @location(1) dist: vec4f,
};

// The scene under the clouds pass's texel `p` (docs/CLOUDS.md §8.9, BUGS.md #88): its
// full-resolution pixels' nearest and farthest scene (sample 0 of each), as distances along the
// texel's ray (`ray_len` m per unit of view depth), the sky and past the reach at the reach. The
// footprint is the divisor's square: at most CLOUD_DEPTH_TAPS² loads (Medium's ¼: 16).
fn cloud_scene_span(p: vec2i, ray_len: f32) -> vec2f {
    let full = vec2i(textureDimensions(scene_depth));
    let k = clamp(vec2i(i32(cloud_prev.params.w)), vec2i(1), vec2i(CLOUD_DEPTH_TAPS));
    var span = vec2f(CLOUD_VOL_MAX_DISTANCE, 0.0);
    for (var y = 0; y < CLOUD_DEPTH_TAPS; y++) {
        if (y >= k.y) {
            break;
        }
        for (var x = 0; x < CLOUD_DEPTH_TAPS; x++) {
            if (x >= k.x) {
                break;
            }
            let q = min(p * k + vec2i(x, y), full - vec2i(1));
            let d = textureLoad(scene_depth, q, 0).r;
            let m = select(CLOUD_VOL_MAX_DISTANCE, min(d * ray_len, CLOUD_VOL_MAX_DISTANCE), d > 0.0);
            span = vec2f(min(span.x, m), max(span.y, m));
        }
    }
    return span;
}

// Four texels of a reduced buffer round `uv` (`size` texels): their coordinates and bilinear
// shares, for the depth-aware filters (the history's and the composite's).
struct CloudTaps {
    at: array<vec2i, 4>,
    share: vec4f,
};

fn cloud_taps(uv: vec2f, size: vec2i) -> CloudTaps {
    let pos = uv * vec2f(size) - 0.5;
    let base = vec2i(floor(pos));
    let f = pos - floor(pos);
    var t: CloudTaps;
    for (var k = 0; k < 4; k++) {
        let o = vec2i(k & 1, k >> 1u);
        t.at[k] = clamp(base + o, vec2i(0), size - vec2i(1));
        t.share[k] = mix(1.0 - f.x, f.x, f32(o.x)) * mix(1.0 - f.y, f.y, f32(o.y));
    }
    return t;
}

// The last frame's clouds at `uv` for a texel whose march now ends at `rec.y`: the four history
// texels round it, each by its bilinear share and by whether its march ended where this one's does
// (dwell_cloud_upsample_weight), so the cloud of the sky beside a peak doesn't smear onto the peak
// as the view turns. `matched` (0-1): how much of the history is this texel's (0: none, a new frame).
struct CloudHistory {
    color: vec4f,
    matched: f32,
};

fn cloud_history_at(uv: vec2f, rec: vec2f) -> CloudHistory {
    let taps = cloud_taps(uv, vec2i(textureDimensions(cloud_history)));
    var sum = vec4f(0.0);
    var weight = 0.0;
    for (var k = 0; k < 4; k++) {
        let w = taps.share[k] * dwell_cloud_upsample_weight(rec.y, textureLoad(cloud_dist_history, taps.at[k], 0).xy);
        sum += w * textureLoad(cloud_history, taps.at[k], 0);
        weight += w;
    }
    return CloudHistory(sum / max(weight, 1e-8), min(weight, 1.0));
}

// The clouds pass (docs/CLOUDS.md §8.6, §8.9, §9): the volume marched at the tier's reduced
// resolution, to the farthest scene pixel under the texel (none of its cloud behind its rock; the
// whole layer where any of it is sky): this frame's raw march, premultiplied by its cover, and the
// scene's span under the texel (nearest, farthest; m along the ray) with the first cloud's distance
// (z; 65000: none). The resolve (clouds_resolve_fs) puts it together with the history.
@fragment
fn clouds_fs(i: SkyOut) -> CloudOut {
    let ray = view_ray(i.ndc);
    let dir = normalize(ray);
    let p = vec2i(i.clip.xy);
    let rec = cloud_scene_span(p, length(ray));
    let m = dwell_cloud_march(dir, frame.eye.y, dwell_cloud_march_end(rec), cloud_jitter(p, u32(cloud_prev.params.y)), frame.clouds, frame.cloud_cfg, frame.cloud_inside, cloud_lighting(), cloud_prev.tan.z);
    // The sky behind is worked out only where there is cloud to haze toward it (§8.7's cut).
    var layer = vec4f(0.0);
    if (m.first >= 0.0) {
        layer = dwell_cloud_hazed(m, sky(dir), frame.cloud_cfg);
    }
    let first = select(65000.0, m.first, m.first >= 0.0);
    return CloudOut(vec4f(layer.rgb * layer.a, layer.a), vec4f(rec, first, 0.0));
}

// The clouds' resolve (BUGS.md #90; dwell-look neighbourhood_box, resolve): this frame's raw march
// at the texel with the last frame's result reprojected by direction (the clouds are far) from the
// history texels whose march ended where this one's does, clamped into the box of this frame's 3 × 3
// neighbours whose scene matched (mean ± CLOUD_HISTORY_CLAMP deviations): a moving eye in or near
// the layer keeps its history, clamped, instead of showing the raw jittered march.
@fragment
fn clouds_resolve_fs(i: SkyOut) -> @location(0) vec4f {
    let ray = view_ray(i.ndc);
    let dir = normalize(ray);
    let p = vec2i(i.clip.xy);
    let size = vec2i(textureDimensions(cloud_raw));
    let cur = textureLoad(cloud_raw, p, 0);
    let rec = textureLoad(cloud_dist_now, p, 0).xy;
    var m1 = vec4f(0.0);
    var m2 = vec4f(0.0);
    var n = 0.0;
    for (var k = 0; k < 9; k++) {
        let q = clamp(p + vec2i(k % 3 - 1, k / 3 - 1), vec2i(0), size - vec2i(1));
        if (dwell_cloud_upsample_weight(rec.y, textureLoad(cloud_dist_now, q, 0).xy) < 0.5) {
            continue;
        }
        let c = textureLoad(cloud_raw, q, 0);
        m1 += c;
        m2 += c * c;
        n += 1.0;
    }
    m1 /= max(n, 1.0);
    let sd = sqrt(max(m2 / max(n, 1.0) - m1 * m1, vec4f(0.0)));
    let lo = min(m1 - CLOUD_HISTORY_CLAMP * sd, cur);
    let hi = max(m1 + CLOUD_HISTORY_CLAMP * sd, cur);
    var out = cur;
    let f = dot(dir, cloud_prev.forward.xyz);
    if (cloud_prev.params.x < 1.0 && f > 1e-3) {
        let pn = vec2f(dot(dir, cloud_prev.right.xyz) / (f * cloud_prev.tan.x), dot(dir, cloud_prev.up.xyz) / (f * cloud_prev.tan.y));
        if (abs(pn.x) < 1.0 && abs(pn.y) < 1.0) {
            let uv = vec2f(pn.x * 0.5 + 0.5, 0.5 - pn.y * 0.5);
            let hist = cloud_history_at(uv, rec);
            out = mix(clamp(hist.color, lo, hi), cur, mix(1.0, cloud_prev.params.x, hist.matched));
        }
    }
    return out;
}

// The clouds pass's result at a full-resolution pixel (`ndc`, its `ray`, the scene's view depth
// `depth`, 0: the sky) as a layer for dwell_cloud_over (§8.9): the four texels round it, each by its
// bilinear share and by whether its march ended at this pixel's scene (dwell_cloud_upsample_weight,
// a band in metres of the texel's depth range), so a rock pixel takes the cloud in front of the rock
// and a sky pixel beside it the whole layer's. Only depths decide: no flicker frame to frame.
fn upsampled_clouds(ndc: vec2f, ray: vec3f, depth: f32) -> vec4f {
    let uv = vec2f(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    let taps = cloud_taps(uv, vec2i(textureDimensions(clouds_tex)));
    let e = select(CLOUD_VOL_MAX_DISTANCE, min(depth * length(ray), CLOUD_VOL_MAX_DISTANCE), depth > 0.0);
    var sum = vec4f(0.0);
    var weight = 0.0;
    for (var k = 0; k < 4; k++) {
        let w = taps.share[k] * dwell_cloud_upsample_weight(e, textureLoad(cloud_dist_tex, taps.at[k], 0).xy);
        sum += w * textureLoad(clouds_tex, taps.at[k], 0);
        weight += w;
    }
    let c = sum / max(weight, 1e-8);
    if (c.a <= 0.001) {
        return vec4f(0.0);
    }
    return vec4f(c.rgb / c.a, c.a);
}

// The sky behind the world toward `dir` with both faces' clouds, the world's and not the viewer's
// (docs/DAY_NIGHT.md §13): a ray above the eye's horizon shows its face's sky and deck; one below it
// leaves by the other face's air, so shows the other face's sky under the other face's deck (lit by
// its own sun and moon), hazed by the eye's face's air over the path to the midplane; the eye's own
// deck, seen from above, goes over either. `deck`/`fw` and `deck_o`/`fw_o` are the two decks' hits
// and footprints along the ray.
fn sky_behind_with_clouds(dir: vec3f, clear: vec3f, deck: vec4f, fw: f32, deck_o: vec4f, fw_o: f32) -> vec3f {
    let face = frame.eye.y;
    let e = face_rise(face, dir);
    if (e >= 0.0) {
        return sky_above_with_clouds(dir, clear, deck, fw, 1.0);
    }
    let o = dwell_other_day(frame.day);
    let other_clear = dwell_sky(dir, -face, o);
    var other = dwell_sky_bodies(dir, -face, o, other_clear);
    other = dwell_clouds(other, other_clear, dir, -face, deck_o, fw_o, frame.clouds_o, frame.cloud_cfg_o, cloud_lighting_for(-face));
    let height = face_y(face, frame.eye.x) - face_y(face, FACE_MIDPLANE_Y);
    let behind = mix(other, clear, dwell_behind_haze(e, height, frame.eye.w));
    return dwell_clouds(behind, behind, dir, face, deck, fw, frame.clouds, frame.cloud_cfg, cloud_lighting());
}

// The sky toward `dir` above the eye's horizon (its `clear` sky, the eye's deck's hit `deck` with its
// footprint `fw`): the stars, the moon and the sun (`dwell_sky_bodies`, at the strength `bodies`;
// 1 for the sky pass) under the clouds. The sky pass and Snell's window both show this one sky.
fn sky_above_with_clouds(dir: vec3f, clear: vec3f, deck: vec4f, fw: f32, bodies: f32) -> vec3f {
    let face = frame.eye.y;
    var lit = dwell_sky_bodies(dir, face, frame.day, clear);
    if (bodies != 1.0) {
        lit = mix(clear, lit, bodies);
    }
    return dwell_clouds(lit, lit, dir, face, deck, fw, frame.clouds, frame.cloud_cfg, cloud_lighting());
}

// A scene colour `c` (premultiplied by coverage) at view depth `depth`, composited along the
// pixel's ray `s.ray`: tone-mapped, encoded and fogged toward its clear sky, under its clouds where
// the deck is in front of it (its hit nearer than the stored view depth), and over its sky with
// its clouds by its coverage; last, inside the cloud, the whole view fogs toward the cloud.
fn composited_scene(c: vec4f, depth: f32, self_lit: f32, s: Composited) -> vec3f {
    var color = s.back;
    if (c.a > 0.0) {
        var shown = fogged(scene_display(c, self_lit), s.ray * depth, s.clear);
        if (cloud_in_front_of(s.deck_t, s.ray, depth)) {
            shown = dwell_cloud_over(shown, s.front);
        }
        color = mix(s.back, shown, min(c.a, 1.0));
    }
    return dwell_cloud_fog(color, frame.cloud_inside.x, frame.eye.y, cloud_lighting());
}

// A scene colour `c` (premultiplied by coverage) at view depth `depth` along `ray` (view depth 1),
// tone-mapped, encoded, fogged toward the clear sky `clear` and over `back` (the sky with its
// clouds) by its coverage.
fn composited_of(c: vec4f, depth: f32, ray: vec3f, clear: vec3f, back: vec3f) -> vec3f {
    if (c.a <= 0.0) {
        return back;
    }
    let shown = mix(scene_display(c, 0.0), clear, fog_amount(ray * depth));
    return mix(back, shown, min(c.a, 1.0));
}

// The finished picture at pixel `p` (docs/MOTION_EFFECTS.md: post effects that sample
// neighbours read the finished image): **every per-pixel effect of the composite lives here**,
// and nothing is applied after it but the dither. The effects that sample other pixels (the
// wind's edge blur) run as their own pass over this image (`motion_fs`), so whatever is added
// here is in their taps too. Three times a new effect vanished outside the blur's inner circle
// while the taps re-composited the scene without it: the clouds from below (0.11.19) and above
// (0.18.2), then the ink (0.20.1).

// The light shafts over the finished picture (ADR 0064): the shafts pass's glow (`shafts_fs`, at
// the tier's resolution, filtered up) × the light's colour and weight, screened over the colour so
// it brightens softly and never clips. None when the frame draws none (uniform).
fn with_shafts(composited: vec3f, p: vec2i) -> vec3f {
    let color = with_bloom(composited, p);
    let air = frame.shafts.w > 0.5 && frame.shafts.z > 0.0;
    let lit = frame.torch_shafts[0].x > 0.5;
    if (!air && !lit) {
        return color;
    }
    let uv = (vec2f(p) + 0.5) / vec2f(textureDimensions(scene_color));
    let s = textureSampleLevel(shafts_tex, scene_sampler, uv, 0.0);
    let g = select(0.0, s.a * frame.shafts.z, air);
    let glow = clamp(frame.shafts_color.rgb * g + select(vec3f(0.0), s.rgb, lit), vec3f(0.0), vec3f(1.0));
    return color + glow * (1.0 - color);
}

// Bloom over the composited colour (WORK_QUEUE item 19, render/bloom.rs): the chain's blurred glow
// of the self-lit pixels × BLOOM_INTENSITY added to the pixel's linear light before the tone curve
// (the composite's colour taken back through `inverse_display`, and only the difference the glow
// makes added, so a pixel without glow is exactly as it was). None with bloom off (its chain 1 × 1).
fn with_bloom(color: vec3f, p: vec2i) -> vec3f {
    if (textureDimensions(bloom_tex).x <= 1u) {
        return color;
    }
    let uv = (vec2f(p) + 0.5) / vec2f(textureDimensions(scene_color));
    let b = textureSampleLevel(bloom_tex, scene_sampler, uv, 0.0).rgb * BLOOM_INTENSITY;
    if (max(max(b.r, b.g), b.b) < 1e-5) {
        return color;
    }
    let lin = inverse_display(min(color, vec3f(0.995)));
    return color + max(display(lin + b) - display(lin), vec3f(0.0));
}

fn composite_pixel(p: vec2i, ndc: vec2f) -> vec3f {
    // Near or under a fluid's surface each pixel is under it or not by its own ray (the waterline,
    // WATER_RENDERING.md §10), with the meniscus along the line between.
    // Deep under it (x 1) the whole view is, as one uniform branch.
    if (frame.water.x > 0.5 && frame.water.x < 1.5) {
        return with_shafts(composite_underwater(p, view_ray(ndc), textureLoad(scene_color, p, 0)), p);
    }
    // Near the surface (x 2 or 3) a pixel under the line takes only the underwater composite and
    // the band (the ordinary composite would be thrown away: computing it anyway cost the whole
    // view's composite again with the eye just under). The composite's derivatives come first,
    // in uniform control flow.
    let decks = composited_decks(ndc);
    var rim_above = 1.0;
    if (frame.water.x > 1.5) {
        let ray = view_ray(ndc);
        let f = eye_fluid();
        let d = waterline_m(ray, f);
        if (d < 0.0) {
            return with_shafts(waterline_band(p, composite_underwater(p, ray, textureLoad(scene_color, p, 0)), d, f), p);
        }
        // The pixel just above the line takes the rim's shade by the share of it the line covers
        // (render/water.rs `line_shade`), so the rim's top edge is anti-aliased, not a whole
        // pixel's step along the wave (WATER_RENDERING.md §12.2).
        if (f.under.w > 0.0) {
            rim_above = 1.0 + (band_edge(0.0, f) - 1.0) * clamp(1.0 - d / waterline_across(f).x, 0.0, 1.0);
        }
    }
    var s = composited(p, ndc, decks);
    s.color *= rim_above;
    if (frame.outline.x > 0.5) {
        s.color = outlined(s, p);
    }
    return with_shafts(s.color, p);
}

// The finished picture, dithered, into the canvas (no wind this frame).
@fragment
fn composite_fs(i: SkyOut) -> @location(0) vec4f {
    let p = vec2i(i.clip.xy);
    return vec4f(composite_pixel(p, i.ndc) + dither(p), 1.0);
}

// The finished picture into the frame's finished target (`finished`), undithered, for the motion
// pass that reads it (the wind is on this frame).
@fragment
fn finished_fs(i: SkyOut) -> @location(0) vec4f {
    return vec4f(composite_pixel(vec2i(i.clip.xy), i.ndc), 1.0);
}

// How many metres the pixel at `p` covers on a surface at view depth `depth` along `ray`, facing
// it obliquely as the floor does (the slant from how far the ray dips).
fn pixel_m(p: vec2i, ray: vec3f, depth: f32) -> f32 {
    let h = f32(textureDimensions(scene_color).y);
    let across = depth * length(ray) * 2.0 * frame.tan.y / h;
    return across / max(abs(normalize(ray).y), 0.15);
}

// The band (WATER_RENDERING.md §12; render/water.rs `band_t`, `band_edge`, `band_uv` are its CPU
// twins): the surface's slab seen edge-on just under the line, over `c` (the pixel's underwater
// view), `d` its waterline_m (negative). Its colour is its surroundings': the scene just above
// the line, mirrored across it (the slab's underside reflects what is above edge-on), over the
// underwater view toward its foot, times the look's tint; textured by the underside's cells on a
// plane receding to the line (compressed toward it), with glints; along the top a dark rim over a
// bright line, both following the wavy edge; faded into the underwater view at its foot. Only the
// band's rows pay for it.
fn waterline_band(p: vec2i, c: vec3f, d: f32, f: Fluid) -> vec3f {
    let depth = f.under.w;
    if (depth <= 0.0 || d <= -depth) {
        return c;
    }
    let t = -d / depth;
    let size = vec2f(textureDimensions(scene_color));
    let across = waterline_across(f);
    let px = -d / across.x;
    let toward = across.yz;
    let side = vec2f(-toward.y, toward.x);
    // The underside's texture on a plane receding to the line (band_uv): along the line by the
    // screen, down it by the inverse distance from the line, so cells shrink toward it.
    let along = dot(vec2f(p), side) / size.y;
    let uv = vec2f(along * 4.0, 1.0) / (t + 0.08) + vec2f(0.0, 0.2 * frame.fog.z);
    // Above the line, mirrored: the scene there, or the sky where nothing is drawn; shifted and
    // blurred by the ripples (two taps, `mirrorWarpPx` apart at most) so the land reads as a
    // rippled reflection, not a second picture.
    let warp = f.band_extra.z;
    let wob = noise_t(vec2f(along * 9.0, t * 2.0 + 0.3 * frame.fog.z)).x;
    let q0 = vec2f(p) + 0.5 + toward * (2.0 * px + 2.0);
    let q1 = clamp(q0 + (side + 0.5 * toward) * (warp * wob), vec2f(0.5), size - 0.5);
    let q2 = clamp(q0 - side * (0.6 * warp) + toward * (0.4 * warp * wob), vec2f(0.5), size - 0.5);
    let above_c = 0.5 * (textureLoad(scene_color, vec2i(q1), 0) + textureLoad(scene_color, vec2i(q2), 0));
    let ndc = vec2f(q1.x / size.x * 2.0 - 1.0, 1.0 - q1.y / size.y * 2.0);
    var above = sky(normalize(view_ray(ndc)));
    if (above_c.a > 0.0) {
        above = mix(above, scene_display(above_c, 0.0), min(above_c.a, 1.0));
    }
    // At most `mirror` the mirrored scene, so a near bank reads as a tint.
    let m = f.band_extra.w;
    let base = mix(above, c, 1.0 - m + m * smoothstep(0.0, 1.0, t)) * f.band.rgb;
    // Ripples, broken by `breakup`: warped by the noise's slope and coming and going under a
    // second, finer noise, instead of smooth parallel stripes.
    // Blurred along the band's rows (three taps BAND_BLUR_PX apart; owner, 2026-11-07: the band
    // 'looks pixelated. Blurring is fine.'), where the receding plane packs the texture tightest.
    // Each layer kept only under the pixel's Nyquist rate (band_footprint, band_keep; 0.50.25):
    // toward the line the plane packs many noise cells into a pixel, which aliased into speckle.
    let b = f.band_extra.y;
    let s = t + 0.08;
    let fp = vec2f(4.0 / size.y / s, across.x / depth / (s * s));
    let duv = vec2f(4.0 * BAND_BLUR_PX / size.y / s, 0.0);
    let r = (band_ripples(uv - duv, b, f, fp) + band_ripples(uv, b, f, fp) + band_ripples(uv + duv, b, f, fp)) / 3.0;
    var col = base * r.x + vec3f(r.y);
    // The dark rim along the top edge, then the bright line under it (band_edge).
    col *= band_edge(px, f);
    return mix(c, col, 1.0 - smoothstep(0.55, 1.0, t));
}

// The band's blur along its rows (px of the drawing buffer between taps).
const BAND_BLUR_PX: f32 = 1.5;

// The band's ripples at texture point `uv` (waterline_band): x the streaks' multiplier, y the
// glint; broken by `b`, warped by the noise's slope and coming and going under a second, finer
// noise, instead of smooth parallel stripes; each layer faded to its mean where a pixel spans `fp`
// texture units too many for it (band_keep).
fn band_ripples(uv: vec2f, b: f32, f: Fluid, fp: vec2f) -> vec2f {
    let w = noise_t(uv * vec2f(0.45, 0.3) + 11.0) * band_keep(vec2f(0.45, 0.3), fp);
    let n0 = noise_t(uv * vec2f(1.0 + 1.2 * b, 0.35) + w.yz * (0.35 * b)).x * band_keep(vec2f(1.0 + 1.2 * b, 0.35), fp);
    let n1 = noise_t(uv * vec2f(2.6, 0.9) + 5.0).x * band_keep(vec2f(2.6, 0.9), fp);
    let n = mix(n0, n0 * 1.8 * smoothstep(-0.25, 0.45, n1) + 0.25 * w.x, b);
    let streak = clamp(1.0 + f.band.a * 2.5 * n, 0.3, 1.8);
    let glint = smoothstep(0.3, 0.45, noise_t(uv * vec2f(2.1, 0.8) + 7.0).x) * f.band_extra.x * band_keep(vec2f(4.2, 1.6), fp);
    return vec2f(streak, glint);
}

// How much of a band noise layer of frequency `k` (cycles a texture unit) is kept where a pixel
// spans `fp` texture units (render/water.rs `band_keep`): whole under a quarter cycle a pixel,
// none past Nyquist (half), where it would only alias into speckle.
fn band_keep(k: vec2f, fp: vec2f) -> f32 {
    return 1.0 - smoothstep(0.25, 0.5, max(k.x * fp.x, k.y * fp.y));
}

// How far `waterline_m` changes a pixel across the line (x, m; render/water.rs `meniscus_px_m`:
// it is linear on the screen, its gradient a pixel the lens × 2 tan_y / height × the camera's
// right and up axes' rise) and the screen direction from under the line toward it (yz).
fn waterline_across(f: Fluid) -> vec3f {
    let face = frame.eye.y;
    let size = vec2f(textureDimensions(scene_color));
    let g = vec2f(face_rise(face, frame.right.xyz), -face_rise(face, frame.up.xyz));
    let px_m = f.under.x * 2.0 * frame.tan.y / size.y * max(length(g), 1e-4);
    return vec3f(px_m, g / max(length(g), 1e-4));
}

// The band's edge `px` pixels under the line (render/water.rs `band_edge`): the dark rim's shade,
// then the bright line's glow under it, else 1.
fn band_edge(px: f32, f: Fluid) -> f32 {
    let rim = 1.0 - smoothstep(f.rim.x - 0.75, f.rim.x + 0.25, px);
    let bright = (1.0 - rim) * (1.0 - smoothstep(f.rim.x + f.rim.z - 0.75, f.rim.x + f.rim.z + 0.25, px));
    return mix(1.0, f.rim.y, rim) * mix(1.0, f.rim.w, bright);
}

// The underside seen from below (look `underside`, WATER_RENDERING.md §12.2) at the surface
// point `xz` (wrapped world m) along the view `dir` (unit): x the cells' multiplier, y the sun
// patch's share (the cells' light × (1 + y)). The cells (`line.w`) are the caustic cells on
// Medium and High, the noise tile on Low, projected on the plane (so perspective-correct, compressed
// toward the horizon) and faded where a pixel covers a cell; `breakup` turns the caustic web's
// bright lines into broken bright cells with dark gaps between, some lit more than others (a slow
// noise over a few cells). The fog fades both over the distance to the surface (the caller's).
fn underside_look(xz: vec2f, dir: vec3f, footprint: f32, f: Fluid, fluid: u32) -> vec2f {
    let k = f.line.w;
    var cells = 1.0;
    if (k > 0.0) {
        let scale = max(round(f.underside.w), 1.0);
        let size = max(f.optics.y, 0.1) / scale;
        let sharp = 1.0 - smoothstep(0.08, 0.3, footprint / size);
        let b = f.underside.x;
        let lit = smoothstep(-0.35, 0.45, noise_t(xz * (0.31 / size) + 5.3).x);
        var c = noise_t(xz / size).x * 1.5;
        c = mix(c, (c + 0.4) * (0.4 + 1.2 * lit) - 0.4, b);
        if (water_reads_scene() && f.extra.y > 0.0) {
            let web = caustic_pattern(xz * scale, f, footprint * scale, fluid);
            let gaps = min(web * CAUSTIC_GAIN * 0.8, 1.0);
            c = mix(web * CAUSTIC_GAIN - CAUSTIC_MEAN, (1.0 - 0.8 * gaps) * (0.2 + 1.3 * lit) - 0.35, b);
        }
        cells = max(1.0 + k * sharp * c, 0.0);
    }
    // The sun patch: brightest toward the sun's image through Snell's window (sun_through), so it
    // follows the sun across the sky and sinks to the window's rim at sunset; gone at night.
    // Hook: volumetric light shafts (a separate feature, being designed) would start from this
    // patch (the same direction and strength), not from a second sun model.
    let sun = frame.day.sun.xyz;
    let glow = f.underside.y * pow(max(dot(dir, sun_through(sun, f.wave.x)), 0.0), max(f.underside.z, 1.0))
        * own(sun, frame.eye.y);
    return vec2f(cells, glow);
}

// The sun seen from under the surface (render/water.rs `sun_through`): its direction refracted
// into the fluid (its slant from the face's up shrunk by 1 / ior; a sun on the horizon shows at
// the critical angle).
fn sun_through(sun: vec3f, ior: f32) -> vec3f {
    let face = frame.eye.y;
    let up = face_up(face);
    let h = (sun - up * face_rise(face, sun)) / max(ior, 1.0);
    return h + up * sqrt(max(1.0 - dot(h, h), 0.0));
}

// A ray `dir` (unit) from under a fluid of refractive index `ior` bent out through its surface,
// whose normal is `n` (unit, toward the air: the face's up, tilted by the ripples), into the air
// (render/water.rs `water_exit`; Snell's law): its slant from `n` grown by `ior`, so the window's
// ~97° cone opens onto the whole sky. Past the critical angle it is clamped to the horizon of `n`
// (the caller keeps those for the mirror).
fn water_exit(dir: vec3f, n: vec3f, ior: f32) -> vec3f {
    var h = (dir - n * dot(dir, n)) * ior;
    let l = dot(h, h);
    if (l > 1.0) {
        h = h / sqrt(l);
    }
    return h + n * sqrt(max(1.0 - dot(h, h), 0.0));
}

// The sky seen through Snell's window along the ray `out` bent out of the fluid (`water_exit`;
// `out_px` the next pixel's): the sky pass's own sky (`sky_behind_with_clouds` for a ray above the
// horizon: the gradient, the stars, the moon in its phase, the sun's disc, the clouds), its
// stars, moon and sun at the look's `window.bodies` strength (1: as above the surface).
// WATER_RENDERING.md §16 (owner, 2026-11-11: looking up from just under the water should match
// looking up from just above it).
fn window_sky(out: vec3f, out_px: vec3f, bodies: f32) -> vec3f {
    let face = frame.eye.y;
    let deck = dwell_cloud_deck(out, face, frame.clouds, frame.cloud_cfg.w);
    let next = dwell_cloud_deck(out_px, face, frame.clouds, frame.cloud_cfg.w);
    let fw = max(abs(next.x - deck.x), abs(next.y - deck.y));
    return sky_above_with_clouds(out, sky(out), deck, fw, bodies);
}

// The composite with the eye under a fluid's surface (the W4 stub): the fluid's fog over the
// distance in it — to what the pixel shows, or to the surface where the ray reaches it first —
// replaces the height fog; past the surface, the sky.
fn composite_underwater(p: vec2i, ray: vec3f, c: vec4f) -> vec3f {
    let fluid = min(u32(frame.water.y), arrayLength(&fluids) - 1u);
    let f = eye_fluid();
    let dir = normalize(ray);
    let rise = face_rise(frame.eye.y, dir);
    let below = max(face_y(frame.eye.y, frame.water.z) - face_y(frame.eye.y, frame.eye.x), 0.0);
    let to_surface = select(1e6, below / rise, rise > 1e-4);
    // Snell's window (render/water.rs `sees_out`): a ray reaching the surface leaves it only
    // within the critical angle of the face's up, bent by the ripples there; flatter, the surface
    // mirrors the water below (its in-scatter, rippled), so the line is the colour edge and the
    // view up from under is a bright window in a mirrored ceiling.
    // Only what the pixel needs is paid for: most pixels show a drawn surface or floor (c.a 1).
    let ior = f.wave.x;
    let reaches = to_surface < 1e5;
    var hit = vec2f(0.0);
    var leaves = false;
    if (reaches) {
        hit = frame.wrap.xy + dir.xz * min(to_surface, 64.0);
        let slope = noise_t(hit * 0.9).yz * 0.18;
        let bent = rise + dot(slope, dir.xz);
        leaves = bent > 0.0 && 1.0 - bent * bent < 1.0 / (ior * ior);
    }
    var depth = 0.0;
    var dist = 1e6;
    if (c.a > 0.0) {
        depth = scene_depth_at(p).x;
        dist = select(1e6, depth * length(ray), depth > 0.0);
    }
    var back = vec3f(0.0);
    if (reaches && (c.a < 1.0 || (!leaves && dist > to_surface))) {
        // The underside where the ray meets the surface: its cells and sun patch, under the fog
        // over the distance to it (`underwater` to to_surface), so toward level they fade into
        // the fog with no hard horizon (render/water.rs `underside_contrast`).
        let fp = to_surface * 2.0 * frame.tan.y / f32(textureDimensions(scene_color).y) / max(rise, 0.02);
        let look = underside_look(hit, dir, fp, f, fluid);
        let light = look.x * (1.0 + look.y);
        // The mirror: the water's in-scatter, textured.
        let mirror = moonlit(display(underwater(underwater(vec3f(0.0), 1e6, f) * light, to_surface, f)));
        if (leaves) {
            // Out through the window: the sky, under the window's glow (shafts.json `window`: a
            // bright white-cyan, strongest at its middle), with the patch's light round the sun;
            // its rim softened into the mirror over the look's `edge` (ADR 0064).
            // The sky is seen along the ray bent out of the fluid by Snell's law at the rippled
            // surface (`water_exit`: the whole sky squeezed into the window's ~97° cone, so the
            // sun and moon sit where they truly are), with their discs on a tier with `bodyPath`
            // (reflections.json; not the CPU's). The window's glow is scaled by the sky's own
            // light straight up, so it is bright at noon, warm at dusk and dim at night
            // (WATER_RENDERING.md §16; owner, 2026-11-11).
            let slope = noise_t(hit * 0.9).yz * 0.18;
            let bent = rise + dot(slope, dir.xz);
            let rim = sqrt(max(1.0 - 1.0 / (ior * ior), 0.0));
            let inside = smoothstep(rim, rim + max(frame.shafts_color.w, 1e-3) * (1.0 - rim), bent);
            let up = face_up(frame.eye.y);
            let out = water_exit(dir, normalize(up + vec3f(slope.x, 0.0, slope.y)), ior);
            // A pixel's step across the screen, bent out the same way: the clouds' footprint.
            let step = 2.0 * frame.tan.y / f32(textureDimensions(scene_color).y);
            let out_px = water_exit(normalize(dir + frame.right.xyz * step), normalize(up + vec3f(slope.x, 0.0, slope.y)), ior);
            let seen = window_sky(out, out_px, f.window.y);
            let zenith = sky_radiance(up);
            let lift = inverse_display(frame.shafts_window.rgb) * max(zenith.r, max(zenith.g, zenith.b));
            let sky_in = mix(inverse_display(seen), lift, frame.shafts_window.w * (0.55 + 0.45 * inside));
            // The sky through the window, waved by the underside's cells at the look's share. The
            // sun patch is the mirror's (the sun's own disc is in the window's sky), so it does not
            // wash the sky with the water's colour.
            let window = display(underwater(sky_in * mix(1.0, look.x, f.window.x), to_surface, f));
            back = mix(mirror, window, inside);
        } else {
            back = mirror;
        }
    } else if (c.a < 1.0) {
        back = display(underwater(sky_radiance(dir), to_surface, f));
    }
    // Past a mirroring surface nothing above it shows.
    if (reaches && !leaves && (c.a <= 0.0 || dist > to_surface)) {
        return back;
    }
    if (c.a <= 0.0) {
        return back;
    }
    var lit = c.rgb / c.a;
    // Caustics on what the pixel shows under the surface (W3, Medium and High): the seabed, walls,
    // anything opaque, before the fog takes its share over the distance to it.
    if (water_reads_scene() && depth > 0.0 && !water_off(DBG_UNDER)) {
        let rel = ray * depth;
        let under = face_y(frame.eye.y, frame.water.z) - face_y(frame.eye.y, frame.eye.x + rel.y);
        // Lit by light that came down through the fluid, focused into caustics.
        lit *= exp(-f.absorption.rgb * max(under, 0.0))
            * caustic_light(frame.wrap.xy + rel.xz, under, frame.eye.x + rel.y, pixel_m(p, ray, depth), UNDERWATER_CAUSTICS, f, fluid);
    }
    let shown = moonlit(display(underwater(lit, min(dist, to_surface), f)));
    return mix(back, shown, min(c.a, 1.0));
}

// The time of day's saturation (`frame.day.horizon.a`, lower by moonlight) on a colour the water
// shows from inside (the mirror, what is under it), as `scene_display` gives the scene above: at
// night the water below is as grey as the sea seen from above, not a saturated glowing blue
// (WATER_RENDERING.md §16). The sky through the window keeps its own colour, as the sky pass does.
fn moonlit(c: vec3f) -> vec3f {
    return mix(vec3f(dot(c, vec3f(0.2126, 0.7152, 0.0722))), c, frame.day.horizon.a);
}
";

/// The light shafts pass (ADR 0064): the glow a pixel of the shafts' buffer (the tier's share of
/// the drawing buffer) gathers, read by `with_shafts` in the composite.
const SHAFTS: &str = r"
// The most lights the shafts come from (dwell-look `MAX_SHAFT_LIGHTS`; `torch_shafts` holds 1 + 2 × it).
const MAX_SHAFT_LIGHTS: u32 = 4u;
// How many air samples share one look-up of the clouds' density.
const SHAFTS_CLOUD_EVERY: i32 = 6;

// A per-pixel offset (0–1) for the samples, so the steps band into noise, not rings.
fn shafts_jitter(p: vec2f) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2f(0.06711056, 0.00583715))));
}

// The full-resolution pixel at screen point `ndc` (clamped to the screen).
fn shafts_pixel(ndc: vec2f, size: vec2f) -> vec2i {
    return vec2i(clamp((vec2f(ndc.x, -ndc.y) * 0.5 + 0.5) * size, vec2f(0.0), size - 1.0));
}

// How much of the sky toward screen point `ndc` the cloud deck hides (0–1): the deck's density as
// the composite finds it (dwell_cloud_layer), at most three octaves and with no self-shadow, so
// the gaps between clouds let the shafts through. 0 with the clouds off.
fn shafts_cloud(ndc: vec2f) -> f32 {
    let octaves = min(i32(frame.cloud_cfg.y), 3);
    if (octaves <= 0) {
        return 0.0;
    }
    let deck = dwell_cloud_deck(normalize(view_ray(ndc)), frame.eye.y, frame.clouds, frame.cloud_cfg.w);
    if (deck.w <= 0.0) {
        return max(-deck.w, 0.0) * frame.cloud_cfg.x;
    }
    let p = cloud_warped(deck.xy, frame.clouds.zw, 0.0);
    return deck.w * cloud_shape(cloud_fbm(p, octaves, 0.0), frame.cloud_cfg.x);
}

// In the air: a radial blur toward the light's screen point over the open sky (the scene's
// coverage: geometry dark, sky bright, the clouds' density between), each sample weighed by the disc round the light and
// decayed along the way, so gaps in trees, clouds' edges, cave mouths and hills cast soft rays.
fn shafts_air(ndc: vec2f, j: f32, size: vec2f) -> f32 {
    let light = frame.shafts.xy;
    let steps = max(i32(frame.shafts_cfg.x), 1);
    let delta = (light - ndc) * frame.shafts_cfg.y / f32(steps);
    let aspect = size.x / size.y;
    var q = ndc + delta * j;
    var w = 1.0;
    var sum = 0.0;
    var total = 0.0;
    // The clouds' density is found every SHAFTS_CLOUD_EVERY samples and held between (the
    // deck's noise is the pass's dearest part; the per-pixel jitter blends the steps).
    var cloud = 0.0;
    for (var k = 0; k < steps; k++) {
        if (k % SHAFTS_CLOUD_EVERY == 0) {
            cloud = shafts_cloud(q + delta * (0.5 * f32(SHAFTS_CLOUD_EVERY - 1)));
        }
        let open = (1.0 - min(textureLoad(scene_color, shafts_pixel(q, size), 0).a, 1.0)) * (1.0 - cloud);
        let r = length((q - light) * vec2f(aspect, 1.0)) / max(frame.shafts_cfg.w, 1e-3);
        let disc = max(1.0 - r, 0.0);
        sum += open * disc * disc * w;
        total += w;
        w *= frame.shafts_cfg.z;
        q += delta;
    }
    return sum / max(total, 1e-4);
}

// Under a fluid: march the view ray (to what it shows, the surface or the reach), and at each
// point look up the surface along the sun's refracted direction: lit where the underside's cells
// are bright there (two drifting noise layers, so the shafts flicker as the reference's do), faded
// by the depth under the surface and the distance along the ray.
fn shafts_under(ndc: vec2f, j: f32, size: vec2f) -> f32 {
    let f = eye_fluid();
    let face = frame.eye.y;
    let ray = view_ray(ndc);
    let dir = normalize(ray);
    let px = shafts_pixel(ndc, size);
    var dist = frame.shafts_cfg.y;
    if (textureLoad(scene_color, px, 0).a > 0.0) {
        let d = scene_depth_at(px).x;
        if (d > 0.0) {
            dist = min(dist, d * length(ray));
        }
    }
    let rise = face_rise(face, dir);
    let below = max(face_y(face, frame.water.z) - face_y(face, frame.eye.x), 0.0);
    if (rise > 1e-4) {
        dist = min(dist, below / rise);
    }
    let sun = sun_through(frame.day.sun.xyz, f.wave.x);
    let slant = sun.xz / max(face_rise(face, sun), 0.2);
    let steps = max(i32(frame.shafts_cfg.x), 1);
    let dt = dist / f32(steps);
    let cell = max(frame.shafts_cfg.w, 0.05);
    let drift = frame.shafts.y * frame.fog.z;
    var sum = 0.0;
    for (var k = 0; k < steps; k++) {
        let t = (f32(k) + j) * dt;
        let h = max(below - rise * t, 0.0);
        let at = (frame.wrap.xy + dir.xz * t + slant * h) / cell;
        let a = noise_t(at + vec2f(drift, 0.6 * drift) / cell).x;
        let b = noise_t(at * 0.47 - vec2f(0.4 * drift, drift) / cell + 3.1).x;
        let lit = smoothstep(0.2, 0.6, a) * smoothstep(-0.2, 0.4, b);
        sum += lit * exp(-h / max(frame.shafts_cfg.z, 0.1) - t / max(frame.shafts_cfg.y, 0.1));
    }
    return sum * dt / max(frame.shafts_cfg.y, 0.1) * 3.0;
}

// From the placed and held lights (torches, lamps; ADR 0064): per light a radial blur toward its
// screen point over what doesn't hide it (the scene behind the flame or the sky, by the view
// depth's first sample), each sample weighed by the halo round the flame and decayed, so a torch
// glows through the air and casts soft rays past what stands in front of it. A pixel whose
// samples can't reach the halo skips the light.
fn shafts_lights(ndc: vec2f, j: f32, size: vec2f) -> vec3f {
    let head = frame.torch_shafts[0];
    let count = min(u32(head.x), MAX_SHAFT_LIGHTS);
    let steps = max(i32(head.y), 1);
    let aspect = vec2f(size.x / size.y, 1.0);
    var out = vec3f(0.0);
    for (var i = 0u; i < count; i++) {
        let a = frame.torch_shafts[1u + 2u * i];
        let b = frame.torch_shafts[2u + 2u * i];
        let rs = max(a.w, 1e-3);
        if (length((ndc - a.xy) * aspect) * (1.0 - head.z) > rs) {
            continue;
        }
        let delta = (a.xy - ndc) * head.z / f32(steps);
        var q = ndc + delta * j;
        var w = 1.0;
        var sum = 0.0;
        var total = 0.0;
        for (var k = 0; k < steps; k++) {
            let d = textureLoad(scene_depth, shafts_pixel(q, size), 0).r;
            let open = select(smoothstep(a.z - 0.75, a.z - 0.25, d), 1.0, d <= 0.0);
            let r = length((q - a.xy) * aspect) / rs;
            let disc = max(1.0 - r, 0.0);
            sum += open * disc * disc * w;
            total += w;
            w *= head.w;
            q += delta;
        }
        out += b.rgb * (sum / max(total, 1e-4));
    }
    return out;
}

@fragment
fn shafts_fs(i: SkyOut) -> @location(0) vec4f {
    let size = vec2f(textureDimensions(scene_color));
    let j = shafts_jitter(i.clip.xy);
    var g = 0.0;
    if (frame.shafts.w > 1.5) {
        g = shafts_under(i.ndc, j, size);
    } else if (frame.shafts.w > 0.5) {
        g = shafts_air(i.ndc, j, size);
    }
    var lit = vec3f(0.0);
    if (frame.torch_shafts[0].x > 0.5) {
        lit = shafts_lights(i.ndc, j, size);
    }
    return vec4f(lit, g);
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

// The finished picture at pixel `px` blurred toward the vanishing point (the scene streaming
// past), then the streaks and vignette over it. The taps read the finished image (`finished`,
// composite_pixel's output), never the scene: each is exactly what that pixel shows, clouds and
// ink included. They are jittered per pixel, so the blur is smooth rather than copies.
fn motion_composite(px: vec2i, ndc: vec2f) -> vec3f {
    let own = textureLoad(finished, px, 0).rgb;
    var out = own;
    let blur = frame.wind_extra.x * frame.wind_point.z * frame.wind_point.z * motion_edge(ndc);
    if (blur > 1e-4) {
        let dims = vec2f(textureDimensions(finished));
        let here = vec2f(px) + 0.5;
        let vp = vec2f(frame.wind_point.x * 0.5 + 0.5, 0.5 - frame.wind_point.y * 0.5) * dims;
        let step = (vp - here) * blur / 4.0;
        let jitter = motion_hash(u32(px.x) * 1973u + u32(px.y) * 9277u);
        var sum = own;
        for (var k = 0; k < 4; k++) {
            let q = clamp(here + step * (f32(k) + jitter), vec2f(0.5), dims - 0.5);
            sum += textureLoad(finished, vec2i(q), 0).rgb;
        }
        out = sum / 5.0;
    }
    let w = motion_wind(ndc);
    return w.rgb + out * (1.0 - w.a);
}

// The motion pass (the wind is on this frame): the finished image with the wind over it, dithered,
// into the canvas. Under water the wind isn't drawn (the finished image as it is).
@fragment
fn motion_fs(i: SkyOut) -> @location(0) vec4f {
    let p = vec2i(i.clip.xy);
    var color = textureLoad(finished, p, 0).rgb;
    if (!eye_under()) {
        color = motion_composite(p, i.ndc);
    }
    return vec4f(color + dither(p), 1.0);
}

";

/// The ink outline (docs/ART_DIRECTION.md §7), drawn by the composite over the scene's composited
/// colour while a mode is on (`frame.outline.x`, the edge kinds; 0 off): edges found in the view
/// depth alone (the frame has no normal or id targets). dwell-look's `outline.rs` is the CPU twin of its depth and crease rule and its fade.
const OUTLINE: &str = r"
const EDGE_DEPTH: u32 = 1u;
const EDGE_NORMAL: u32 = 2u;
const EDGE_BLOCK: u32 = 4u;

fn ink_inv(d: f32) -> f32 {
    return select(0.0, 1.0 / d, d > 0.0);
}

// Back from inverse depth (0 stays 0).
fn ink_dinv(w: f32) -> f32 {
    return select(0.0, 1.0 / w, w > 0.0);
}

// How much two slopes of inverse depth differ: 0 equal, 1 where one is flat or they have
// opposite signs (dwell-look `slope_change`).
fn ink_change(a: f32, b: f32, top: f32) -> f32 {
    return abs(a - b) / (abs(a) + abs(b) + 1e-4 * top);
}

// The edge strength from a 3 × 3 of inverse view depths at the tap distance (row by row from the
// top left; 0: nothing drawn), the pixel's in the middle (dwell-look `edge_strength`). Inverse
// depth is linear across the screen on a plane, so a flat face at any angle has no edge: a
// silhouette is a large second difference across the pixel (relative to the nearest tap), a crease
// a change of slope, along a row or column or between the outer ones (the surface's normal turns).
fn ink_edges(w: array<f32, 9>, edges: u32) -> f32 {
    var top = 0.0;
    var low = w[0];
    for (var k = 0; k < 9; k++) {
        top = max(top, w[k]);
        low = min(low, w[k]);
    }
    if (top <= 0.0) {
        return 0.0;
    }
    let dt = frame.outline_cfg.x;
    let ct = frame.outline_cfg.y;
    var s = 0.0;
    if ((edges & EDGE_DEPTH) != 0u) {
        let e = max(abs(w[3] + w[5] - 2.0 * w[4]), abs(w[1] + w[7] - 2.0 * w[4])) / top;
        s = max(s, smoothstep(dt, 2.0 * dt, e));
    }
    if ((edges & EDGE_NORMAL) != 0u && low > 0.0) {
        let c = max(
            max(ink_change(w[4] - w[3], w[5] - w[4], top), ink_change(w[4] - w[1], w[7] - w[4], top)),
            max(ink_change(w[6] - w[0], w[8] - w[2], top), ink_change(w[2] - w[0], w[8] - w[6], top)),
        );
        s = max(s, smoothstep(ct, 2.0 * ct, c));
    }
    return s;
}

// The taps with those on a surface that takes no ink (-1) seen through (dwell-look
// `see_through`): such a tap takes what the plane through the pixel and the opposite tap gives
// (no edge across the pair), or the pixel's own where both are flagged; never nearer than the sky.
// So neither the plant's own outline nor its edges against the ground or the sky draw.
fn ink_see_through(raw: array<f32, 9>) -> array<f32, 9> {
    var w = raw;
    for (var k = 0; k < 9; k++) {
        if (raw[k] < 0.0) {
            let o = raw[8 - k];
            w[k] = select(raw[4], max(2.0 * raw[4] - o, 0.0), o >= 0.0);
        }
    }
    return w;
}

fn ink_ndc(q: vec2i, dims: vec2f) -> vec2f {
    let uv = (vec2f(q) + 0.5) / dims;
    return vec2f(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
}

// The voxel cell behind a surface point (relative to the eye), a quarter cell inside it along the
// normal `n` (which faces the eye).
fn ink_cell(rel: vec3f, n: vec3f) -> vec3i {
    let world = vec3f(frame.wrap.x, frame.eye.x, frame.wrap.y) + rel - n * 0.25;
    return vec3i(floor(world));
}

// The view depth the edges are found in: one sample of the pixel (the first). One load per tap
// (the cost; a sample's depth is never a blend), where the composite's mean over the covered
// samples would let a few samples of a blade's tip or a fading plant's dither pull a pixel
// forward, which dots lines over the grass.
fn ink_depth(p: vec2i) -> f32 {
    return textureLoad(scene_depth, p, 0).r;
}

// The outline at pixel `p` (view ray `ray`): its opacity and the edge's view depth (its nearest
// tap), from the view depths of the 3 × 3 around it at the tap distance (dwell-look `edge_at`),
// faded out over the distance fade; with block edges, where the cell behind the surface changes
// from the pixel to a neighbour on it (positions from the depths, the normal from the nearer
// neighbours).
fn ink_outline(p: vec2i, ray: vec3f) -> vec2f {
    let edges = u32(frame.outline.x + 0.5);
    let r = i32(frame.outline.y);
    let size = textureDimensions(scene_color);
    let dims = vec2i(size);
    // Ground past the fade has no line of its own: one load and out (most of a distant view).
    // Nor has a surface that takes no ink (`frame_depth`'s flag: the plants).
    let centre = ink_depth(p);
    if (centre * length(ray) >= frame.outline.w || !frame_inked(centre)) {
        return vec2f(0.0);
    }
    var raw: array<f32, 9>;
    for (var k = 0; k < 9; k++) {
        let o = vec2i(k % 3 - 1, k / 3 - 1) * r;
        let d = ink_depth(clamp(p + o, vec2i(0), dims - 1));
        raw[k] = select(-1.0, ink_inv(d), frame_inked(d));
    }
    let w = ink_see_through(raw);
    var near = 0.0;
    for (var k = 0; k < 9; k++) {
        near = max(near, w[k]);
    }
    if (near <= 0.0) {
        return vec2f(0.0);
    }
    // The pixel-art preview's edge: only on the near side (the pixel nearest of its taps).
    if (frame.outline_cfg.z > 0.5 && w[4] < near * 0.97) {
        return vec2f(0.0);
    }
    var s = ink_edges(w, edges);
    // Left, right, up and down: their offsets and view depths.
    let q = array(p + vec2i(-r, 0), p + vec2i(r, 0), p + vec2i(0, -r), p + vec2i(0, r));
    let d = array(ink_dinv(w[3]), ink_dinv(w[5]), ink_dinv(w[1]), ink_dinv(w[7]));
    let d0 = ink_dinv(w[4]);
    if ((edges & EDGE_BLOCK) != 0u && d0 > 0.0 && s < 1.0) {
        let fd = vec2f(size);
        let p0 = ray * d0;
        var rel: array<vec3f, 4>;
        for (var k = 0; k < 4; k++) {
            rel[k] = view_ray(ink_ndc(q[k], fd)) * d[k];
        }
        // Each axis's step to the neighbour nearer in depth (the one on the same face).
        let use_r = d[1] > 0.0 && (d[0] <= 0.0 || abs(d[1] - d0) < abs(d0 - d[0]));
        let use_d = d[3] > 0.0 && (d[2] <= 0.0 || abs(d[3] - d0) < abs(d0 - d[2]));
        let ok = (d[0] > 0.0 || d[1] > 0.0) && (d[2] > 0.0 || d[3] > 0.0);
        if (ok) {
            let dx = select(p0 - rel[0], rel[1] - p0, use_r);
            let dy = select(p0 - rel[2], rel[3] - p0, use_d);
            var n = normalize(cross(dx, dy));
            n = select(n, -n, dot(n, p0) > 0.0);
            let c0 = ink_cell(p0, n);
            for (var k = 0; k < 4; k++) {
                if (d[k] > 0.0 && any(ink_cell(rel[k], n) != c0)) {
                    s = 1.0;
                }
            }
        }
    }
    let depth = 1.0 / near;
    let fade = 1.0 - smoothstep(frame.outline.z, frame.outline.w, depth * length(ray));
    return vec2f(s * fade * frame.outline_ink.a, depth);
}

// How much of an edge at `rel` (relative to the eye) shows: none under the sea's surface seen from
// above it. The water doesn't write the view depth, so the frame sees the seabed through it; the
// sea is the one water whose surface the composite knows (SEA_LEVEL on the viewer's face).
fn ink_above_sea(rel: vec3f) -> f32 {
    let face = frame.eye.y;
    let cam = face_y(face, frame.eye.x);
    let h = cam + face_rise(face, rel) - SEA_LEVEL;
    return select(1.0, smoothstep(-0.3, -0.05, h), cam > SEA_LEVEL);
}

// The composited pixel `s` at `p` with the ink over it: fogged toward the clear sky as the ground
// at the edge's depth is, under the cloud deck where it is in front, and gone inside a cloud.
fn outlined(s: Composited, p: vec2i) -> vec3f {
    let e = ink_outline(p, s.ray);
    if (e.x <= 0.0) {
        return s.color;
    }
    let rel = s.ray * e.y;
    var a = e.x * ink_above_sea(rel) * (1.0 - frame.cloud_inside.x);
    if (cloud_in_front_of(s.deck_t, s.ray, e.y)) {
        a *= 1.0 - s.front.a;
    }
    let ink = mix(frame.outline_ink.rgb, s.clear, fog_amount(rel));
    return mix(s.color, ink, a);
}
";

const WATER: &str = r"
struct FluidIn {
    @location(0) position: vec3f,
    @location(1) normal: vec3f,
    @location(2) color: vec3f,
    @location(3) fluid: f32,
    // The flood light (sky, block as shares of 15; dwell-mesh: the cells the face looks into).
    @location(4) flood: vec2f,
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
    @location(6) flood: vec2f,
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
    o.flood = v.flood;
    return o;
}

// Lattice coordinates wrapped to [0, period) (render/water.rs `wrap_cell`): the integer modulo
// once per lookup; neighbours step from it with `wrap_next`.
fn wrap_cell(c: vec2i, period: i32) -> vec2i {
    return ((c % period) + period) % period;
}

// A wrapped lattice coordinate stepped by -1, 0 or +1 (render/water.rs `wrap_step`).
fn wrap_step(w: vec2i, o: vec2i, period: i32) -> vec2i {
    let v = w + o;
    return select(select(v, v - period, v >= vec2i(period)), v + period, v < vec2i(0));
}

// The lattice hash of a wrapped point.
fn lattice_hash(w: vec2i) -> u32 {
    let u = vec2u(w);
    var h = (u.x * 0x8da6b343u) ^ (u.y * 0xd8163841u) ^ 0x9e3779b9u;
    h = (h ^ (h >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    return h ^ (h >> 16u);
}

// A unit vector from two bytes of a hash (render/water.rs `hash_dir`): no trig, one rsqrt.
fn hash_dir(h: u32) -> vec2f {
    let v = vec2f(f32(h & 0xffu), f32((h >> 8u) & 0xffu)) - 127.5;
    return v * inverseSqrt(max(dot(v, v), 1.0));
}

// Gradient noise and its derivatives (value, d/dx, d/dy), quintic, wrapping every `period` cells.
fn noise_d(p: vec2f, period: i32) -> vec3f {
    let cell = floor(p);
    let w = wrap_cell(vec2i(cell), period);
    let f = p - cell;
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let du = 30.0 * f * f * (f * (f - 2.0) + 1.0);
    let ga = hash_dir(lattice_hash(w));
    let gb = hash_dir(lattice_hash(wrap_step(w, vec2i(1, 0), period)));
    let gc = hash_dir(lattice_hash(wrap_step(w, vec2i(0, 1), period)));
    let gd = hash_dir(lattice_hash(wrap_step(w, vec2i(1, 1), period)));
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

// Cells of about `want` metres in WRAP_M, a whole number of `tile`s (render/water.rs `tile_cells`).
fn tile_cells(want: f32, tile: i32) -> f32 {
    return f32(tile) * max(1.0, round(WRAP_M / (max(want, 1e-3) * f32(tile))));
}

// Gradient noise and its derivatives (value, d/dx, d/dy) at lattice point `q`, from the noise
// tile: `noise_d` with a period that is a whole number of tiles.
fn noise_t(q: vec2f) -> vec3f {
    return textureSampleLevel(noise_tile, tile_sampler, q / f32(NOISE_TILE), 0.0).xyz;
}

// One layer of normal detail (render/water.rs `WaveLayer`): its height slope (xy) at wrapped
// world `xz`, faded out where a pixel covers `footprint` m (Nyquist), and the slope variance the
// fade lost (z). Wavelengths divide WRAP_M, so the detail wraps with the position.
fn wave_layer(xz: vec2f, f: Fluid, footprint: f32, ratio: f32, slope: f32, speed: f32, dir: vec2f, turn: mat2x2f) -> vec3f {
    let n = tile_cells(f.wave.z * ratio, NOISE_TILE);
    let wavelength = WRAP_M / n;
    let s = f.wave.y * slope;
    let fade = 1.0 - smoothstep(wavelength * FADE_LO, wavelength * FADE_HI, footprint);
    let lost = (1.0 - fade * fade) * s * s;
    if (fade <= 0.0) {
        return vec3f(0.0, 0.0, lost);
    }
    let shift = dir * (frame.fog.z * f.wave.w * speed / wavelength);
    let q = turn * (xz / wavelength) + (shift - n * floor(shift / n));
    let d = noise_t(q);
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
    // The fragment's face (its frame's sign, `face_at`) and world y.
    up: f32,
    frag_y: f32,
    on_a: bool,
    body: vec3f,
    // How the surface is lit by the flood light (`terrain_light`, no baked light): water in a
    // cave or under a roof is as dark as the ground beside it, whatever the Shadows setting.
    light: TerrainLight,
    // The direct light's shadow on the surface (`shadow_light`, the flood light's direct share as
    // its stand-in): it shades the glint, the sparkle and the body's in-scatter.
    shadow: f32,
    // The sky light's brightness at the surface (`light_curve` of its level): the share of the
    // sky's reflection it sees.
    sky: f32,
};

// A see-through face that is no fluid's (glass): lit by its flood light as terrain is.
fn see_through_light(i: FluidOut, n: vec3f) -> TerrainLight {
    var l = terrain_light(1.0, i.flood);
    l.direct = shadow_light(i.rel, n, l.direct);
    l.points = point_lights(i.rel, n);
    return l;
}

fn water_surface(i: FluidOut, front: bool, footprint: f32, f: Fluid, draw: Draw) -> Surface {
    var s: Surface;
    s.v = normalize(-i.rel);
    let face_n = normalize(i.normal);
    // The face's normal toward the eye's side (outward from outside, inward from inside).
    let seen = select(-face_n, face_n, front);
    s.frag_y = frame.eye.x + i.rel.y;
    s.up = face_at(s.frag_y);
    s.on_a = s.up > 0.0;
    // Ripples on the surface (tops and bottoms; sides stay flat), toward the side seen: on face B
    // the mirrored surface's normal points down, and so does the rippled one.
    s.w = vec3f(0.0);
    s.n = seen;
    if (abs(face_n.y) > 0.5) {
        let layers = water_tier_layers();
        // Carried by the current (docs/WATER_SURFACE_GRID.md §2c): two copies of the ripples half
        // a cycle apart, each carried downstream over its cycle and faded in and out, so neither
        // is stretched far (a flow map).
        let cur = select(vec2f(0.0), flow_at(i.wrapped, i.rel), flow_on(FLOW_RIPPLES) && f.flow.x > 0.0);
        if (dot(cur, cur) > 1e-4) {
            let t = frame.fog.z / f.flow.y;
            let p0 = fract(t);
            let p1 = fract(t + 0.5);
            let carry = cur * f.flow.x * f.flow.y;
            let w0 = 1.0 - abs(2.0 * p0 - 1.0);
            s.w = water_slope(i.wrapped - carry * p0, f, footprint, layers) * w0
                + water_slope(i.wrapped - carry * p1, f, footprint, layers) * (1.0 - w0);
        } else {
            s.w = water_slope(i.wrapped, f, footprint, layers);
        }
        s.n = ripple_normal(s.up, seen, s.w.xy);
        // A ripple can't face away from the eye: bend it back.
        s.n = normalize(s.n + max(0.02 - dot(s.n, s.v), 0.0) * s.v);
    }
    s.ndv = clamp(dot(s.n, s.v), 1e-3, 1.0);
    s.light = terrain_light(1.0, i.flood);
    s.light.points = point_lights(i.rel, face_up(s.up));
    s.sky = light_curve(i.flood.x, SKY_CURVE);
    s.shadow = shadow_light(i.rel, face_up(s.up), s.light.direct);
    // The body is the light scattered in the water under the surface, so a shadow on the surface
    // (the player's own, standing in the water) doesn't darken it: at full strength it read as a
    // dark blotch beside the eye (WATER_RENDERING.md §12), at 35 % (0.50.20) still a faint trace
    // (§12.2). The glint and sparkle take the shadow.
    s.body = lambert_lit(f.scatter.rgb * draw.tint.rgb, face_up(s.up), s.frag_y, s.light);
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
    // The glint of the face's own sun and moon at the time of day (the day/night cycle).
    let d = frame.day;
    var glint = vec3f(0.0);
    if (!water_off(DBG_GLINT)) {
        glint = ggx_glint(s.n, s.v, d.sun.xyz, a, f0) * select(d.sun_b.rgb, d.sun_a.rgb, s.on_a) * own(d.sun.xyz, s.up)
            + ggx_glint(s.n, s.v, d.moon.xyz, a, f0) * select(d.moon_b.rgb, d.moon_a.rgb, s.on_a) * own(d.moon.xyz, s.up);
        if (reflect_path()) {
            glint += body_paths(s, f0);
        }
    }
    var reflected = refl.rgb;
    if (water_off(DBG_SKY)) {
        reflected = select(d.sky_b.rgb, d.sky_a.rgb, s.on_a);
    }
    var spark = vec3f(0.0);
    if (!water_off(DBG_SPARKLE)) {
        spark = sparkle(i, s, f, footprint);
    }
    return fres * reflected + (1.0 - refl.a) * (glint + spark) * s.shadow;
}

// The reflection tier's switches (`frame.reflect_sky.y`, dwell-look `reflection_uniforms`): bit 0
// the deck's self-shadow, bit 1 the sun's and moon's discs and glitter paths (`bodyPath`).
fn reflect_bits() -> u32 {
    return u32(frame.reflect_sky.y + 0.5);
}
fn reflect_path() -> bool {
    return (reflect_bits() & 2u) != 0u;
}

// How much of a body's glitter path shows with it `rise` (the sine of its elevation) over the
// face's horizon (dwell-look `BodyPath::share`): whole under p.z, gone over p.w (radians).
fn path_share(p: vec4f, rise: f32) -> f32 {
    let e = asin(clamp(rise, -1.0, 1.0));
    return 1.0 - smoothstep(p.z, max(p.w, p.z + 1e-4), e);
}

// The sun's and moon's glitter paths (WATER_RENDERING.md §15; render/glitter.rs): a GGX lobe of
// each disc's own light off the rippled normal, a streak toward the viewer under the body broken
// up by the ripples; the ripples' lost slope variance widens and dims it (calm water: bright and
// tight). The moon's is its disc colour × its lit share (its phase); the sun's only while it is
// low. The cloud cover takes PATH_CLOUD_FADE of it while clouds are drawn.
fn body_paths(s: Surface, f0: f32) -> vec3f {
    let d = frame.day;
    let lost = 2.0 * s.w.z;
    let am = sqrt(MOON_PATH.y * MOON_PATH.y + lost);
    let asun = sqrt(SUN_PATH.y * SUN_PATH.y + lost);
    let moon = ggx_glint(s.n, s.v, d.moon.xyz, am, f0) * MOON_PATH.x * d.stars.w
        * path_share(MOON_PATH, face_rise(s.up, d.moon.xyz)) * own(d.moon.xyz, s.up);
    let sun = ggx_glint(s.n, s.v, d.sun.xyz, asun, f0) * SUN_PATH.x
        * path_share(SUN_PATH, face_rise(s.up, d.sun.xyz)) * own(d.sun.xyz, s.up);
    let cover = select(0.0, frame.cloud_cfg.x * PATH_CLOUD_FADE, frame.cloud_cfg.y > 0.5);
    return (moon * inverse_display(MOON_COLOR) + sun * inverse_display(d.disc.rgb)) * (1.0 - cover);
}

// The sun's and moon's discs (the moon in its phase) in the reflected sky `base` toward `r`, on a
// tier with `bodyPath` (`dwell_sky_bodies`' bodies without the stars; a ripple's mirror is a
// little blurred, so the edge is two pixels wide).
fn reflected_bodies(r: vec3f, base: vec3f) -> vec3f {
    let up = face_up(frame.eye.y);
    let px = 2.0 * max(frame.day.stars.z, 1e-5);
    let moon = dwell_moon(r, frame.day, px);
    let c = mix(base, max(base, base * 0.4 + moon.rgb), moon.w);
    let sun = dwell_sun_disc(r, up, frame.day, px);
    return min(mix(c, max(c, sun.rgb), sun.w), vec3f(1.0));
}

// The sky along a reflected ray `r` from the surface point `rel`, with the cloud deck over it on
// the tiers that reflect clouds (WATER_RENDERING.md §8; the composite's own sky and clouds, so
// the day, night and dusk follow), in the scene's linear light. `fw` is the deck's footprint
// (`reflected_deck_fw`).
// `clouds`: how much of the deck shows (`reflect_share`: none where the surface reflects too little to
// see it, and the deck isn't drawn, which cost a 33 ms frame ~3.7 ms looking down at the water).
fn reflected_env(r: vec3f, rel: vec3f, fw: f32, clouds: f32) -> vec3f {
    let clear = sky(r);
    var c = clear;
    if (reflect_path() && !water_off(DBG_GLINT)) {
        c = reflected_bodies(r, clear);
    }
    let under = c;
    let octaves = frame.reflect_sky.x;
    if (octaves > 0.5 && clouds > 0.0 && !water_off(DBG_CLOUDS)) {
        let face = frame.eye.y;
        // The deck seen from the surface point, not the eye: its height above it, and the noise
        // point under it.
        let height = frame.cloud_cfg.w - face_rise(face, rel);
        let offsets = vec4f(frame.clouds.xy + rel.xz / CLOUD_SCALE, frame.clouds.zw);
        let deck = dwell_cloud_deck(r, face, offsets, height);
        let cfg = vec4f(frame.cloud_cfg.x, octaves, select(0.0, frame.cloud_cfg.z, (reflect_bits() & 1u) != 0u), height);
        c = mix(under, dwell_clouds(under, clear, r, face, deck, fw, offsets, cfg, cloud_lighting()), clouds);
    }
    return inverse_display(c);
}

// The deck's footprint in a reflection, taken off the calm surface's mirror ray (derivatives need
// uniform control flow: call it before any branch on the fragment). 0 while no clouds reflect.
fn reflected_deck_fw(rel: vec3f) -> f32 {
    if (frame.reflect_sky.x < 0.5 || water_off(DBG_CLOUDS)) {
        return 0.0;
    }
    let up = face_at(frame.eye.x + rel.y);
    let d = normalize(rel);
    let r = vec3f(d.x, abs(d.y) * up, d.z);
    let height = frame.cloud_cfg.w - face_rise(frame.eye.y, rel);
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
    // Top and bottom only: fading at the sides drew a white haze there (docs/BUGS.md 6).
    let e = min(px.y, size.y - px.y);
    return smoothstep(0.0, 1.0, e / (frame.reflect_sky.z * size.y));
}

// Screen-space reflection (render/reflection.rs `trace`, the tested twin): the reflected ray `r`
// from the surface point `p` marched on screen through the scene's view depth, in even steps to
// the screen's edge or the reach, a hit pinned by binary steps. Where it meets the scene (xy, in
// pixels) and how much of the reflection that gives (z; 0: missed).
// render/reflection.rs `SIDE_LAND_M` (a test pins them equal).
const SIDE_LAND_M: f32 = 0.25;

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
            // At the scene's depth where it was hit, not the step's (render/reflection.rs `trace`).
            let dh = clamp(ssr_depth(px), min(a.z, b.z), max(a.z, b.z));
            // How far along the ray in 3D: view depth is linear along it.
            let u = select(t_hi, (dh - a.z) / (b.z - a.z), abs(b.z - a.z) > 1e-3 * a.z);
            let w = ssr_edge(px, size) * (1.0 - smoothstep(frame.reflect.w, frame.reflect.z, u * len));
            return vec3f(px, w);
        }
        prev_t = t;
        prev_d = d;
    }
    // Out through a side with no hit: the scene at the exit pixel stands in where it stands over
    // the surface (land), not the floor seen through the water (render/reflection.rs `trace`).
    // On past the side, the pixel held in the edge column, meeting land only (render/reflection.rs
    // `trace`).
    let exit = a.xy + delta * t_end;
    if (t_end < 1.0 && (exit.x <= 0.5 + 1e-3 || exit.x >= size.x - 0.5 - 1e-3)) {
        var prev_e = 1.0 / mix(k0, k1, t_end);
        for (var i = 1; i <= steps; i++) {
            let t = t_end + (1.0 - t_end) * f32(i) / f32(steps);
            let d = 1.0 / mix(k0, k1, t);
            let px = a.xy + delta * t;
            let at = vec2f(exit.x, clamp(px.y, 0.5, size.y - 0.5));
            let scene = ssr_depth(at);
            let ndc = vec2f(at.x / size.x * 2.0 - 1.0, 1.0 - at.y / size.y * 2.0);
            let land = scene > 0.0
                && face_rise(frame.eye.y, view_ray(ndc) * scene) > face_rise(frame.eye.y, p) + SIDE_LAND_M;
            if (ssr_hits(scene, d, prev_e) && land) {
                let u = select(t, (clamp(scene, min(a.z, b.z), max(a.z, b.z)) - a.z) / (b.z - a.z), abs(b.z - a.z) > 1e-3 * a.z);
                let w = ssr_edge(at, size) * (1.0 - smoothstep(frame.reflect.w, frame.reflect.z, u * len));
                return vec3f(at, w);
            }
            prev_e = d;
        }
    }
    return vec3f(0.0);
}

// How much of the land's reflection a surface reflecting `fresnel` shows (render/reflection.rs
// `land_share`): none at or under SSR_MIN_REFLECTANCE (no march), all from twice it.
fn reflect_share(fresnel: f32) -> f32 {
    if (SSR_MIN_REFLECTANCE <= 0.0) {
        return 1.0;
    }
    // Kept apart: Chrome's compiler rejects a smoothstep whose constant edges are equal (a min
    // of 0), even in a branch that never runs.
    let m = max(SSR_MIN_REFLECTANCE, 1e-4);
    return smoothstep(m, 2.0 * m, fresnel);
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

// The bank band (shared/look/watertiers.json `band`; owner, 2026-11-07, for phones): the land
// reflected without a march. The reflected ray's point BANK_REACH_M out from the surface point `p`
// is looked up on screen once; where the scene there stands in front of it (within BANK_SLACK of
// its view depth: the bank above the water's edge, not the sky or far land past it), a soft strip
// of the bank's colour from the scene copy (four taps across and up the screen), darkened, over
// `env` at BANK_STRENGTH × `land` (the share the surface reflects). xyz the colour, w its weight.
fn bank_band(env: vec3f, p: vec3f, r: vec3f, size: vec2f, land: f32) -> vec4f {
    let q = p + r * BANK_REACH_M;
    let d = dot(q, frame.forward.xyz);
    if (d <= 1e-3) {
        return vec4f(env, 0.0);
    }
    let ndc = vec2f(dot(q, frame.right.xyz) / (d * frame.tan.x), dot(q, frame.up.xyz) / (d * frame.tan.y));
    let px = vec2f((ndc.x * 0.5 + 0.5) * size.x, (0.5 - ndc.y * 0.5) * size.y);
    if (any(px < vec2f(0.5)) || any(px > size - 0.5)) {
        return vec4f(env, 0.0);
    }
    let behind = textureLoad(scene_depth, vec2i(px), 0).r;
    if (behind <= 0.0 || behind > d * (1.0 + BANK_SLACK)) {
        return vec4f(env, 0.0);
    }
    var c = vec4f(0.0);
    let taps = array<vec2f, 4>(
        vec2f(-BANK_SPREAD_PX, 0.0),
        vec2f(BANK_SPREAD_PX, 0.0),
        vec2f(-0.5 * BANK_SPREAD_PX, -BANK_BLUR_PX),
        vec2f(0.5 * BANK_SPREAD_PX, BANK_BLUR_PX),
    );
    for (var k = 0; k < 4; k++) {
        let at = clamp(px + taps[k], vec2f(0.5), size - 0.5);
        c += textureSampleLevel(scene_color, scene_sampler, at / size, 0.0);
    }
    if (c.a <= 0.0) {
        return vec4f(env, 0.0);
    }
    // Fades out toward the slack's end, so the strip has no hard edge where the bank stops.
    let edge = 1.0 - smoothstep(0.5 * BANK_SLACK, BANK_SLACK, behind / d - 1.0);
    let w = BANK_STRENGTH * land * edge * min(c.a * 0.25, 1.0);
    let fog = fog_amount(view_ray(ndc) * behind);
    let bank = mix(c.rgb / c.a * (1.0 - BANK_DARKEN), env, fog);
    return vec4f(mix(env, bank, w), w);
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
    let n_cells = tile_cells(wavelength, NOISE_TILE);
    let cell = WRAP_M / n_cells;
    let shift = vec2f(0.6, 0.8) * (frame.fog.z * f.wave.w * 3.1 / cell);
    let q = mat2x2f(2.0, -1.0, 1.0, 2.0) * (i.wrapped / cell) + (shift - n_cells * floor(shift / n_cells));
    let d = noise_t(q);
    let n = sparkle_normal(s.up, s.n, d.yz);
    let r = reflect(-s.v, n);
    let day = frame.day;
    let lobe = pow(max(dot(r, day.sun.xyz), 0.0), SPARKLE_POWER);
    return select(day.sun_b.rgb, day.sun_a.rgb, s.on_a) * own(day.sun.xyz, s.up) * (k * fade * lobe * schlick(s.ndv, f.extra.x));
}

const SPARKLE_WAVELENGTH: f32 = 0.06;
const SPARKLE_POWER: f32 = 2400.0;

// Whether a fluid face is seen from inside its fluid: its back, when it is a surface (a top or a
// bottom) or the eye is under a fluid. A side face's back seen from the air (across a pool's
// open edge, or past a bank) is the fluid seen from outside, so it takes the outside look, not
// the inside's flat body colour (WATER_RENDERING.md §9).
fn seen_from_inside(i: FluidOut, front: bool) -> bool {
    return !front && (abs(i.normal.y) > 0.5 || eye_under());
}

// A surface's underside seen from outside the water (owner, 2026-11-05: flat blue sheets on
// rivers): through a step's riser, from below the upper pool's level, the eye sees the underside
// of that pool's surface. Drawn as seen from inside it was a flat sheet of the body's colour; from
// outside the water it isn't drawn, and the riser shows the bed behind it through the water.
fn underside_from_outside(i: FluidOut, front: bool) -> bool {
    return !front && abs(i.normal.y) > 0.5 && !eye_under();
}

// The eye's own surface hidden where the waterline puts the pixel on its other side
// (WATER_RENDERING.md §10): its top seen from above where the pixel looks out from under it, its
// underside seen from below where the pixel looks out above it. Each half of a split view then
// shows its own side, and the line between them is the waterline's.
// Only near the surface (`frame.water.x` 2 or 3: a uniform branch, nothing to test elsewhere);
// else branch-free, so the fragment's derivatives after it stay in uniform control flow.
fn waterline_hides(i: FluidOut, front: bool) -> bool {
    if (frame.water.x < 1.5) {
        return false;
    }
    let face = frame.eye.y;
    let own = abs(face_y(face, frame.eye.x + i.rel.y) - face_y(face, frame.water.z)) <= 0.05;
    let other_side = (waterline_m(i.rel / max(view_depth(i.rel), 1e-4), eye_fluid()) < 0.0) == front;
    return (abs(i.normal.y) > 0.5) & own & other_side;
}

// A fluid's surface seen from inside: out through Snell's window, else totally reflected.
// Lit unshadowed: the underside shows the water below it, not the shadow on the surface (the
// player's own shadow read as a dark blotch overhead, WATER_RENDERING.md §12). The mirror carries
// the underside's texture and sun patch (`underside_look` at the surface point `xz`, perspective-correct on
// the mesh); inside the window what is above shows through by the Fresnel (the composite's
// `back`: the sky, or the scene behind).
fn from_below(s: Surface, f: Fluid, xz: vec2f, footprint: f32, fluid: u32) -> vec4f {
    let out_dir = refract(-s.v, s.n, f.wave.x);
    let tir = dot(out_dir, out_dir) < 0.5;
    let fres = select(schlick(abs(dot(out_dir, s.n)), f.extra.x), 1.0, tir);
    let body = lambert_lit(f.scatter.rgb, face_up(s.up), s.frag_y, TerrainLight(s.light.ambient, s.light.direct, s.light.glow, s.light.points));
    let look = underside_look(xz, -s.v, footprint, f, fluid);
    return vec4f(body * (look.x * (1.0 + look.y)), fres);
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
    // A swap's water fades with its level's surface (the same pixels: neither blends twice).
    let hidden = waterline_hides(i, front);
    if (fade_discard(draw, i.clip.xy) || hidden) {
        discard;
    }
    let count = arrayLength(&fluids);
    var o: SceneOut;
    o.depth = frame_depth(view_depth(i.rel), true);
    if (i.fluid >= count) {
        let n = normalize(i.normal);
        o.color = vec4f(lambert_lit(i.color * draw.tint.rgb, select(-n, n, front), frame.eye.x + i.rel.y, see_through_light(i, n)), 0.5);
        return o;
    }
    if (underside_from_outside(i, front)) {
        discard;
    }
    let f = fluids[i.fluid];
    let s = water_surface(i, front, footprint, f, draw);
    if (seen_from_inside(i, front)) {
        o.color = from_below(s, f, i.wrapped, footprint, i.fluid);
        return o;
    }
    let fres = schlick(s.ndv, f.extra.x);
    // The tier's share of the fluid's opacity: less lets the floor behind show through (Low: see-
    // through with no scene copy, so no pass split; owner, 2026-11-07).
    let t = 1.0 - f.absorption.a * water_tier_opacity();
    let alpha = max(1.0 - (1.0 - fres) * t, 1e-3);
    let refl = vec4f(reflected_env(reflected_dir(s), i.rel, deck_fw, reflect_share(fres)) * s.sky, 0.0);
    var lit = surface_light(i, s, f, fres, footprint, refl) + (1.0 - fres) * (1.0 - t) * s.body;
    // The caustics in the blended pass (watertiers.json `caustics`): the tile's lines as if on a
    // floor SURFACE_CAUSTICS_M down, added as light seen through the surface (no depth is read).
    if (water_tier_caustics() && !water_off(DBG_CAUSTICS)) {
        let m = caustic_light(i.wrapped, SURFACE_CAUSTICS_M, s.frag_y, footprint, 1.0, f, i.fluid);
        lit += max(m - 1.0, 0.0) * SURFACE_CAUSTICS_GAIN * (1.0 - fres) * s.shadow * mix(s.body, vec3f(1.0), 0.3);
    }
    o.color = vec4f(lit / alpha, alpha);
    // Foam streaks along the current (docs/WATER_SURFACE_GRID.md §2c), opaque where they are.
    let streaks = flow_streaks(i, f, footprint);
    if (streaks > 0.0) {
        o.color = vec4f(mix(o.color.rgb, streak_lit(s, f), streaks), mix(o.color.a, 1.0, streaks));
    }
    return o;
}

// Foam streaks along the current (docs/WATER_SURFACE_GRID.md §2c; shared/look/waterflow.json):
// noise stretched along the flow (`flow_shape`: a streak's length and width), carried with the
// water as the ripples are (two copies half a cycle apart, faded in and out), from the current
// where they begin to where they are full (`flow.zw`) at the look's strength; faded out where a
// pixel covers more than a streak's width (no shimmer far off). 0 with the Foam streaks setting
// off, for a liquid without a flow look, or in still water.
fn flow_streaks(i: FluidOut, f: Fluid, footprint: f32) -> f32 {
    if (!flow_on(FLOW_FOAM) || !water_tier_streaks() || f.flow_foam.a <= 0.0 || f.flow.y <= 0.0) {
        return 0.0;
    }
    let cur = flow_at(i.wrapped, i.rel);
    let speed = length(cur);
    let fine = 1.0 - smoothstep(0.5 * f.flow_shape.y, 2.0 * f.flow_shape.y, footprint);
    let amount = smoothstep(f.flow.z, f.flow.w, speed) * f.flow_foam.a * fine;
    if (amount <= 0.0) {
        return 0.0;
    }
    let dir = cur / speed;
    let p0 = fract(frame.fog.z / f.flow.y);
    let p1 = fract(frame.fog.z / f.flow.y + 0.5);
    let carry = cur * f.flow.y;
    let w0 = 1.0 - abs(2.0 * p0 - 1.0);
    return amount * (flow_streak(i.wrapped - carry * p0, dir, f) * w0
        + flow_streak(i.wrapped - carry * p1, dir, f) * (1.0 - w0));
}

// One copy of the streaks: fine noise (a streak's width) averaged over six taps back along the flow
// over a streak's length (taps about a width apart, so a streak is unbroken), drawn out along the
// current; only the noise's highest ridges show, so streaks are few (no turned frame: turning world
// coordinates by the local direction swings the pattern where the direction bends).
fn flow_streak(xz: vec2f, dir: vec2f, f: Fluid) -> f32 {
    let step = dir * (f.flow_shape.x / 6.0);
    var n = 0.0;
    for (var k = 0; k < 6; k++) {
        n += noise_t((xz - step * f32(k)) / f.flow_shape.y).x;
    }
    return smoothstep(0.16, 0.3, n / 6.0 * 2.0);
}

// The streaks' colour, lit as the shoreline foam is.
fn streak_lit(s: Surface, f: Fluid) -> vec3f {
    return lambert_lit(f.flow_foam.rgb, vec3f(0.0, s.up, 0.0), s.frag_y, TerrainLight(s.light.ambient, s.shadow, s.light.glow, s.light.points));
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
    let hidden = waterline_hides(i, front);
    if (fade_discard(draw, i.clip.xy) || hidden) {
        discard;
    }
    let count = arrayLength(&fluids);
    if (i.fluid >= count) {
        let n = normalize(i.normal);
        return vec4f(lambert_lit(i.color * draw.tint.rgb, select(-n, n, front), frame.eye.x + i.rel.y, see_through_light(i, n)), 0.5);
    }
    if (water_off(DBG_FLAT)) {
        return vec4f(0.02, 0.1, 0.2, 1.0);
    }
    if (underside_from_outside(i, front)) {
        discard;
    }
    let f = fluids[i.fluid];
    let s = water_surface(i, front, footprint, f, draw);
    if (seen_from_inside(i, front)) {
        return from_below(s, f, i.wrapped, footprint, i.fluid);
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
        floor *= caustic_light(i.wrapped + floor_rel.xz - i.rel.xz, under, frame.eye.x + floor_rel.y, floor_fp, 1.0, f, i.fluid);
    }
    var through = mix(floor, s.body, 0.5);
    if (!water_off(DBG_ABSORPTION)) {
        let t = exp(-(f.absorption.rgb + f.extra.w) * length_in);
        let down = exp(-f.absorption.rgb * min(under, DEEP_M));
        through = floor * t * down + s.body * (1.0 - t);
    }
    // The reflection: land by screen-space reflection, else the sky and its clouds.
    // The land's ray is steered by a calmer surface (SSR_RIPPLE of the ripples' slope), so the
    // reflection stays coherent where the ripples change from pixel to pixel.
    var calm = s;
    calm.n = normalize(mix(vec3f(0.0, sign(s.n.y), 0.0), s.n, SSR_RIPPLE));
    // Only where the surface reflects enough to see the land (`reflect_share`): looking down at water
    // the march is skipped, which was its largest cost close to the water (and the reflected
    // clouds, below, are too).
    let land = reflect_share(fres);
    var hit = vec3f(0.0);
    if (land > 0.0) {
        hit = ssr_trace(i.rel, reflected_dir(calm), size);
        hit.z *= land;
    }
    var refl = ssr_reflected(reflected_env(reflected_dir(s), i.rel, deck_fw, land) * s.sky, hit, size);
    // The tiers that march no land (Low) reflect the bank by the band instead.
    if (water_tier_band() && land > 0.0 && hit.z <= 0.0) {
        refl = bank_band(refl.rgb, i.rel, reflected_dir(calm), size, land);
    }
    var color = surface_light(i, s, f, fres, footprint, refl) + (1.0 - fres) * through;
    // Shoreline foam where the water is thin.
    var foam = 0.0;
    if (!water_off(DBG_FOAM)) {
        foam = foam_amount(i.wrapped, min(under, length_in), f);
    }
    if (foam > 0.0) {
        let lit = lambert_lit(f.foam.rgb, vec3f(0.0, s.up, 0.0), s.frag_y, TerrainLight(s.light.ambient, s.shadow, s.light.glow, s.light.points));
        color = mix(color, lit, foam);
    }
    // Foam streaks along the current (docs/WATER_SURFACE_GRID.md §2c).
    let streaks = flow_streaks(i, f, footprint);
    if (streaks > 0.0) {
        color = mix(color, streak_lit(s, f), streaks);
    }
    return vec4f(color, 1.0);
}

const DEEP_M: f32 = 1e4;

// Shore foam (WATER_RENDERING.md §14; owner, 2026-11-10: 'more like pulsing bands'): by `shore`,
// the distance to the shore (the depth over a slope, the ray's path through the water to a wall),
// a solid contact line where the water meets land or a block, and soft bands parallel to the
// shore, each born at the edge and rolling out to the fluid's foam depth while it thins and fades,
// a new one forming as the last goes. Slow noise shifts each band's timing and width along the
// shore; it never multiplies the foam, so it cuts no holes.
fn foam_amount(xz: vec2f, shore: f32, f: Fluid) -> f32 {
    let reach = f.foam.a;
    let most = f.shore.z;
    if (reach <= 0.0 || most <= 0.0 || shore >= reach + f.shore.y) {
        return 0.0;
    }
    let t = frame.fog.z;
    // slow noise along the shore, in -1..1 or so
    let n_cells = tile_cells(FOAM_CELL, NOISE_TILE);
    let cell = WRAP_M / n_cells;
    let drift = vec2f(0.03, 0.02) * t / cell;
    let q = xz / cell + (drift - n_cells * floor(drift / n_cells));
    let n = noise_t(q).x + 0.5 * noise_t(q * 2.0 + vec2f(0.5, 0.25)).x;
    let wob = f.shore.w;
    // the contact line: solid, a hair wider or narrower along the shore
    let line_w = f.shore.x * (1.0 + 0.4 * wob * n);
    var foam = 1.0 - smoothstep(line_w * 0.5, max(line_w, 1e-3), shore);
    let bands = u32(f.flow_shape.z);
    let period = max(f.flow_shape.w, 0.1);
    for (var k = 0u; k < min(bands, 4u); k++) {
        // its age (0 born at the edge, 1 gone at the reach), shifted along the shore by the noise
        let age = fract(t / period + f32(k) / f32(bands) + 0.25 * wob * n);
        let at = f.shore.x + age * reach;
        let w = f.shore.y * (1.0 - 0.7 * age) * (1.0 + 0.5 * wob * n);
        let shape = 1.0 - smoothstep(w * 0.25, w * 0.5, abs(shore - at));
        let fade = smoothstep(0.0, 0.12, age) * (1.0 - age) * (1.0 - age);
        foam = max(foam, shape * fade * 0.8);
    }
    return clamp(foam * most, 0.0, 1.0);
}

const FOAM_CELL: f32 = 0.9;

// Caustics (WATER_RENDERING.md W3, brought forward from W4): the light the ripples focus on what is
// `under` metres below the surface, at wrapped world `xz`, as a multiplier of its light (1: none).
// The pattern is shifted back along the refracted sun (or moon, much dimmer), animated with the
// ripples' flow, and fades with depth over the fluid's caustic depth (render/water.rs
// `caustic_fade`). Focused, not added: dimmer between the lines, brighter on them.
fn caustic_light(xz: vec2f, under: f32, frag_y: f32, footprint: f32, boost: f32, f: Fluid, fluid: u32) -> f32 {
    // Faded out where a pixel covers a good part of a cell (they would alias into noise).
    let sharp = 1.0 - smoothstep(0.08, 0.3, footprint / max(f.optics.y, 1e-3));
    if (under <= 0.0 || f.extra.y <= 0.0 || sharp <= 0.0) {
        return 1.0;
    }
    let up = face_at(frag_y);
    let sun_k = smoothstep(0.0, 0.25, face_rise(up, frame.day.sun.xyz));
    let moon_k = smoothstep(0.0, 0.25, face_rise(up, frame.day.moon.xyz)) * MOON_CAUSTICS;
    let k = max(sun_k, moon_k);
    if (k <= 0.0) {
        return 1.0;
    }
    let l = select(frame.day.moon.xyz, frame.day.sun.xyz, sun_k >= moon_k);
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
    let c = caustic_pattern(xz + back, f, footprint, fluid);
    return max(1.0 + strength * (c * CAUSTIC_GAIN - CAUSTIC_MEAN), 0.0);
}

const MOON_CAUSTICS: f32 = 0.2;
// Seen from under the surface the caustics are the show: brighter than through it from above,
// where the surface's own reflection and glint compete with them.
const UNDERWATER_CAUSTICS: f32 = 1.7;
const CAUSTIC_GAIN: f32 = 2.2;
const CAUSTIC_MEAN: f32 = 0.2;

// The caustic layers' time: each fluid's own (its flow speed), the coarse layer's and the fine's.
fn caustic_times(f: Fluid) -> vec2f {
    // The caustics' drift (their pace from before the ripples were sped up, 0.29.1).
    let t = frame.fog.z * (0.5 + 1.2 * f.wave.w);
    return vec2f(t, 1.31 * t + 2.0);
}

// One caustic layer's cell-edge distance at cell coordinate `p` (the pattern repeats every
// CAUSTIC_TILE cells, which divide the wrap): from the tile (channel `ch` of the fluid's layer),
// or, where a pixel covers fewer than CAUSTIC_EXACT_TEXELS texels (`texels`: texels per pixel),
// evaluated (`cell_edge`), blended between. CAUSTIC_EXACT_TEXELS is 0: always the tile
// (render/water.rs).
fn caustic_edge(p: vec2f, t: f32, texels: f32, fluid: u32, ch: i32) -> f32 {
    var k = 1.0;
    if (CAUSTIC_EXACT_TEXELS > 0.0) {
        // Kept apart: Chrome's compiler rejects a smoothstep whose constant edges are equal
        // (CAUSTIC_EXACT_TEXELS 0), even in a branch that never runs.
        let e = max(CAUSTIC_EXACT_TEXELS, 1e-3);
        k = smoothstep(0.6 * e, e, texels);
    }
    var a = 0.0;
    if (k > 0.0) {
        let c = textureSampleLevel(caustic_tile, tile_sampler, p / f32(CAUSTIC_TILE), fluid, 0.0);
        a = select(c.y, c.x, ch == 0);
    }
    if (k < 1.0) {
        a = mix(cell_edge(p, CAUSTIC_TILE, t), a, k);
    }
    return a;
}

// Two layers of moving cell edges (Worley F2 − F1), warped so the lines wander, wrapping with the
// position (cells divide WRAP_M in whole caustic tiles, render/water.rs `caustic_period`): bright
// thin lines, near 0 between them.
fn caustic_pattern(xz: vec2f, f: Fluid, footprint: f32, fluid: u32) -> f32 {
    let n = tile_cells(f.optics.y, CAUSTIC_TILE);
    let cell = WRAP_M / n;
    let p = xz / cell;
    let t = caustic_times(f);
    let warp = 0.11 * vec2f(sin(6.2831853 * p.y + t.x), sin(6.2831853 * p.x + 1.37 * t.x));
    // Texels of the tile per pixel, for the coarse layer (the fine one's cells are half the size).
    let texels = footprint / cell * CAUSTIC_TEXELS;
    let a = caustic_edge(p + warp, t.x, texels, fluid, 0);
    var c = exp(-a * 11.0);
    // The finer layer only near, where a pixel is small against its cells.
    let fine = 1.0 - smoothstep(0.04, 0.12, footprint / cell);
    if (fine > 0.0) {
        let b = caustic_edge(p * 2.0 + vec2f(0.37, 0.71) - 2.0 * warp, t.y, 2.0 * texels, fluid, 1);
        c += 0.6 * fine * exp(-b * 9.0);
    }
    return c;
}

// The water's tiles (render/water_tiles.rs), drawn by a full-screen triangle per tile (or caustic
// layer: the instance is the fluid).
struct TileOut {
    @builtin(position) clip: vec4f,
    @location(0) @interpolate(flat) fluid: u32,
};

@vertex
fn tile_vs(@builtin(vertex_index) i: u32, @builtin(instance_index) k: u32) -> TileOut {
    let p = array(vec2f(-1.0, -1.0), vec2f(3.0, -1.0), vec2f(-1.0, 3.0));
    var o: TileOut;
    o.clip = vec4f(p[i], 0.0, 1.0);
    o.fluid = k;
    return o;
}

// The noise tile: `noise_d` over NOISE_TILE cells at each texel's centre.
@fragment
fn noise_tile_fs(i: TileOut) -> @location(0) vec4f {
    return vec4f(noise_d(i.clip.xy / NOISE_TEXELS, NOISE_TILE), 0.0);
}

// A fluid's caustic layer: both caustic layers' cell-edge distances over CAUSTIC_TILE cells, at this
// frame's times.
@fragment
fn caustic_tile_fs(i: TileOut) -> @location(0) vec4f {
    let t = caustic_times(fluids[i.fluid]);
    let p = i.clip.xy / CAUSTIC_TEXELS;
    return vec4f(cell_edge(p, CAUSTIC_TILE, t.x), cell_edge(p, CAUSTIC_TILE, t.y), 0.0, 0.0);
}

// The distance from the nearest cell edge of moving Worley points (F2 − F1), wrapping every
// `period` cells. Each point wanders as 0.38 (sin(t + φ), cos(0.83 t + ψ)) about its cell's
// centre, its phases from the hash as unit vectors, so the angle sums need the time's sines only
// (once per call), not two per point.
fn cell_edge(p: vec2f, period: i32, t: f32) -> f32 {
    let cell = floor(p);
    let fp = p - cell;
    let w = wrap_cell(vec2i(cell), period);
    let ts = vec2f(sin(t), cos(t));
    let tc = vec2f(cos(0.83 * t), -sin(0.83 * t));
    var d1 = 9.0;
    var d2 = 9.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let o = vec2i(x, y);
            let h = lattice_hash(wrap_step(w, o, period));
            let wander = vec2f(dot(ts, hash_dir(h)), dot(tc, hash_dir(h >> 16u)));
            let d = length(vec2f(o) + 0.5 + 0.38 * wander - fp);
            d2 = min(d2, max(d1, d));
            d1 = min(d1, d);
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
    let bloom = dwell_look::bloom::BLOOM.intensity as f32;
    let taps = dwell_look::clouds::MAX_DIVISOR;
    let clamp = dwell_look::clouds::HISTORY_CLAMP as f32;
    format!(
        "const SCENE_SAMPLES: i32 = {samples};\n\
         const BLOOM_INTENSITY: f32 = {bloom:?};\n\
         const SAMPLE_SLOTS: u32 = {samples}u;\n\
         @group(2) @binding(0) var scene_color: texture_2d<f32>;\n\
         @group(2) @binding(1) var scene_depth: {depth};\n\
         @group(3) @binding(0) var finished: texture_2d<f32>;\n\
         @group(2) @binding(2) var scene_sampler: sampler;\n\
         @group(2) @binding(3) var shafts_tex: texture_2d<f32>;\n\
         @group(2) @binding(4) var clouds_tex: texture_2d<f32>;\n\
         @group(2) @binding(5) var cloud_dist_tex: texture_2d<f32>;\n\
         @group(2) @binding(6) var bloom_tex: texture_2d<f32>;\n\
         @group(3) @binding(1) var cloud_history: texture_2d<f32>;\n\
         @group(3) @binding(2) var cloud_history_sampler: sampler;\n\
         @group(3) @binding(3) var<uniform> cloud_prev: CloudPrev;\n\
         @group(3) @binding(5) var cloud_dist_history: texture_2d<f32>;\n\
         @group(3) @binding(9) var cloud_raw: texture_2d<f32>;\n\
         @group(3) @binding(10) var cloud_dist_now: texture_2d<f32>;\n\
         const CLOUD_DEPTH_TAPS: i32 = {taps};\n\
         const CLOUD_HISTORY_CLAMP: f32 = {clamp:?};\n{}",
        dwell_look::clouds::cloud_march_wgsl(3, 4, 6),
    )
}

/// The whole shader module for a scene of `samples` per pixel: constants, uniforms, the sky, the
/// shading and every entry point.
pub fn source(samples: u32) -> String {
    format!(
        "{}{DRAWS}\n{FADE}\n{}\n{}\n{}\n{}\n{TERRAIN}\n{MOTION}\n{OUTLINE}\n{WATER}\n{SHAFTS}\n{}",
        lighting_source(),
        dwell_look::lod_blend::LOD_BLEND.pattern.wgsl(),
        super::lod_map::wgsl(&dwell_look::lod_map::LOD_MAP),
        super::flow::wgsl(),
        scene_bindings(samples),
        water_slope()
    ) + &super::heightfield::wgsl()
        + &super::impostors::wgsl(dwell_worldgen::impostor::Config::get())
}

/// The flood light's shading constants (shared/look/light.json; docs/DAY_NIGHT.md §11).
fn light_wgsl() -> String {
    let l = dwell_world::light::LIGHT;
    let c = l.block_color.map(|v| dwell_look::srgb_to_linear(v) as f32);
    format!(
        "const SKY_CURVE: f32 = {:?};\nconst DIRECT_FROM: f32 = {:?};\nconst DIRECT_FULL: f32 = {:?};\nconst BLOCK_CURVE: f32 = {:?};\nconst BLOCK_STRENGTH: f32 = {:?};\nconst BLOCK_LIGHT: vec3f = vec3f({:?}, {:?}, {:?});\n",
        l.sky_curve as f32,
        f32::from(l.direct_from),
        f32::from(l.direct_full),
        l.block_curve as f32,
        l.block_strength as f32,
        c[0],
        c[1],
        c[2],
    )
}

/// Lava's look by heat (shared/fluids/lava.json `heat`, VOLCANOES.md §6.5), one row per stop, the
/// hottest first: `lava_stop_look` the seams' colour (linear) and emissive, `lava_stop_core` the
/// seams' core colour and the crust's cover. A molten cell's faces name their stop (dwell-mesh
/// `vertex_rgb`).
pub fn lava_wgsl() -> String {
    let stops = dwell_lava::DATA.heat;
    let row = |c: [f32; 3], w: f32| format!("vec4f({:?}, {:?}, {:?}, {w:?})", c[0], c[1], c[2]);
    let look: Vec<String> = stops.iter().map(|s| row(s.linear(), s.intensity)).collect();
    let core: Vec<String> = stops
        .iter()
        .map(|s| row(s.core_linear(), s.crust))
        .collect();
    let n = stops.len();
    format!(
        "const LAVA_STOPS: u32 = {n}u;\nvar<private> lava_stop_look: array<vec4f, {n}> = array<vec4f, {n}>({});\nvar<private> lava_stop_core: array<vec4f, {n}> = array<vec4f, {n}>({});\n",
        look.join(", "),
        core.join(", ")
    )
}

/// The shared lighting: the constants, the fluids, the frame uniforms (group 0: `frame`, `atlas`,
/// `atlas_sampler`), the sky, the clouds and the shading functions, without any pass's bindings or
/// entry points.
pub fn lighting_source() -> String {
    format!(
        "{}{}const SEA_LEVEL: f32 = {:.1};\nconst AMBIENT_FLOOR: f32 = {:?};\n{}{}const ATLAS_CELLS: u32 = {}u;\nconst ATLAS_TILES: u32 = {}u;\nconst ATLAS_GUTTER: u32 = {}u;\n{}\n{}\n{}\n{FRAME}\n{}\n{}\n{FRAME_DEPTH}\n{}\n{}\n{COMMON}\n{}\n{}\n",
        dwell_world::FaceFrame::wgsl(),
        super::face_shading::WGSL,
        world::SEA_LEVEL_M,
        dwell_look::ambient::AMBIENT.floor as f32,
        light_wgsl(),
        lava_wgsl(),
        dwell_mesh::ATLAS_CELLS,
        dwell_mesh::ATLAS_TILES,
        dwell_mesh::GUTTER,
        dwell_look::daynight::DAY_SKY_WGSL,
        super::water::wgsl(),
        super::reflection::wgsl(),
        dwell_look::sky_wgsl_with(&super::moon::wgsl()),
        dwell_look::clouds::cloud_wgsl(&dwell_look::clouds::CLOUDS),
        super::lights::wgsl(),
        super::point_shadows::wgsl(),
        super::lod_shadows::LOD_SHADOW_WGSL,
        super::shadows::SHADOW_WGSL,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn molten_lava_draws_its_stop_hottest_bright_orange_red_past_white() {
        // Owner, 2026-11-11: "Lava looks dark red instead of bright red/orange." The seams glowed
        // in a fixed mix of the tile's hue; now in each cell's heat stop's colour and emissive.
        let src = super::lighting_source();
        let stops = dwell_lava::DATA.heat;
        assert!(src.contains(&format!("const LAVA_STOPS: u32 = {}u;", stops.len())));
        let hot = stops[0];
        assert!(hot.intensity > 1.0);
        let [r, g, b] = hot.linear();
        assert!(
            r > 0.9 && g > 0.05 && g < 0.4 && b < 0.05,
            "orange-red, not mustard: {r} {g} {b}"
        );
        assert!(
            src.contains("emitted_hue("),
            "lava keeps its hue as it glows"
        );
    }

    #[test]
    fn the_block_outline_turns_with_its_draw_as_meshes_do() {
        // Owner, 2026-11-02: a moving body's hover outline was "tilted at uneven angles": the line
        // stage left out the draw's rotation, so the box about the body's origin didn't turn with
        // it. Every stage that places a vertex by a draw's offset turns it by the draw's quat.
        let src = super::source(1);
        let body = |name: &str| {
            let at = src.find(&format!("fn {name}(")).expect(name);
            src[at..at + src[at..].find("\n}\n").expect("end")].to_string()
        };
        for stage in ["terrain_vs", "cutout_vs", "line_vs"] {
            assert!(
                body(stage).contains("qrot(draw.quat, v.position * draw.offset.w)"),
                "{stage} turns by the draw's rotation"
            );
        }
    }

    #[test]
    fn the_sky_behind_the_world_takes_the_other_faces_below_the_horizon() {
        // docs/BUGS.md 10 and 13: the composite's sky behind the world is the other face's below
        // the viewer's horizon, under the other face's deck and the eye's face's haze over the path
        // to the midplane (`sky_behind_with_clouds`); the eye's face's fog fades to `sky()`.
        let src = super::source(1);
        let body = |name: &str| {
            let at = src.find(&format!("fn {name}(")).expect(name);
            src[at..at + src[at..].find("\n}\n").expect("end")].to_string()
        };
        let b = body("sky_behind_with_clouds");
        assert!(b.contains("dwell_sky_bodies(dir, -face, o, other_clear)"));
        assert!(b.contains("frame.clouds_o, frame.cloud_cfg_o, cloud_lighting_for(-face)"));
        assert!(b.contains("dwell_behind_haze(e, height, frame.eye.w)"));
        assert!(
            body("composited")
                .contains("sky_behind_with_clouds(dir, clear, deck, fw, deck_o, fw_o)")
        );
        assert!(src.contains("let clear = sky(dir);"));
    }

    #[test]
    fn a_point_across_the_midplane_fogs_by_each_faces_air() {
        // docs/BUGS.md 13: a point seen down a shaft took the eye's face's fog colour over its whole
        // path. The part of the ray in the other face's air fogs toward that face's sky first.
        let src = super::source(1);
        let at = src.find("fn fogged(").expect("fogged");
        let b = &src[at..at + src[at..].find("\n}\n").expect("end")];
        assert!(b.contains("frame.day.other_stars.y"));
        assert!(b.contains("return mix(mix(c, other_clear, far), clear, own);"));
        let at = src.find("fn composited_scene(").expect("composited_scene");
        assert!(src[at..].contains("fogged(scene_display(c, self_lit), s.ray * depth, s.clear)"));
    }

    #[test]
    fn a_water_surfaces_underside_is_drawn_only_from_inside_the_water() {
        // Owner, 2026-11-05: flat blue sheets on rivers. From below a stepped river's upper pool,
        // the eye saw that pool's surface from underneath through the step's riser, and it was drawn
        // as seen from inside (the body's flat colour). Both fluid shaders discard an underside seen
        // from outside the water before shading it.
        let src = super::source(1);
        let at = src.find("fn underside_from_outside(").expect("the rule");
        let rule = &src[at..at + src[at..].find("\n}\n").expect("end")];
        assert!(rule.contains("!front && abs(i.normal.y) > 0.5 && !eye_under()"));
        for entry in ["fn water_fs(", "fn water_scene_fs("] {
            let at = src.find(entry).expect(entry);
            let b = &src[at..at + src[at..].find("\n}\n").expect("end")];
            let skip = b
                .find("if (underside_from_outside(i, front)) {\n        discard;")
                .expect(entry);
            let shade = b.find("water_surface(i, front").expect(entry);
            assert!(
                skip < shade,
                "{entry} discards the underside before shading"
            );
        }
    }

    #[test]
    fn the_lights_shafts_block_holds_the_datas_lights_and_the_composite_adds_their_colour() {
        use super::{FRAME, SHAFTS, TERRAIN};
        use dwell_look::shafts::{LIGHT_SHAFT_VEC4S, MAX_SHAFT_LIGHTS};
        assert!(FRAME.contains(&format!("torch_shafts: array<vec4f, {LIGHT_SHAFT_VEC4S}>,")));
        assert!(SHAFTS.contains(&format!(
            "const MAX_SHAFT_LIGHTS: u32 = {MAX_SHAFT_LIGHTS}u;"
        )));
        // The pass writes the lights' glow in rgb and the sun's, moon's or fluid's in a.
        assert!(SHAFTS.contains("return vec4f(lit, g);"));
        let comp = &TERRAIN[TERRAIN.find("fn with_shafts").unwrap()..];
        let comp = &comp[..comp.find("fn composite_pixel").unwrap()];
        assert!(comp.contains("s.a * frame.shafts.z") && comp.contains("s.rgb, lit"));
    }

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
                "cutout_vs",
                "cutout_fs",
                "flat_vs",
                "flat_fs",
                "line_vs",
                "ghost_vs",
                "line_fs",
                "line_top_fs",
                "sky_vs",
                "composite_fs",
                "finished_fs",
                "motion_fs",
                "fluid_vs",
                "water_fs",
                "water_scene_fs",
                "tile_vs",
                "noise_tile_fs",
                "caustic_tile_fs",
                "shadow_terrain_vs",
                "shadow_flat_vs",
                "shadow_terrain_fs",
                "shadow_flat_fs",
                "shadow_cutout_fs",
            ] {
                assert!(
                    module.entry_points.iter().any(|e| e.name == entry),
                    "{entry}"
                );
            }
        }
    }

    #[test]
    fn water_and_see_through_faces_take_the_flood_light() {
        // Owner (2026-10-21): darkness from the flood light alone, Shadows Off included
        // (docs/DAY_NIGHT.md §11). Water under a 31 m roof drew as the open sea with Shadows Off
        // (33 % of white against 33 %), sun glint and sky reflection included: the water shader
        // lit every fluid surface as open sky.
        let src = super::source(4);
        let body = |name: &str| {
            let at = src.find(&format!("fn {name}(")).expect(name);
            src[at..at + src[at..].find("\n}\n").expect("end")].to_string()
        };
        let fluid_in = &src[src.find("struct FluidIn").expect("FluidIn")..];
        assert!(
            fluid_in[..fluid_in.find("};").expect("end")].contains("@location(4) flood: vec2f")
        );
        let surface = body("water_surface");
        // Its ambient, glow and direct share are the flood light's; the shadow maps shade the
        // direct share, they don't replace a full one.
        assert!(surface.contains("s.light = terrain_light(1.0, i.flood);"));
        assert!(surface.contains("s.shadow = shadow_light(i.rel, face_up(s.up), s.light.direct);"));
        // The body takes none of the shadow (WATER_RENDERING.md §12.2: at 35 % it still read as a
        // trace of the player's shadow from above).
        assert!(!surface.contains("BODY_SHADOW"));
        assert!(surface.contains(
            "s.body = lambert_lit(f.scatter.rgb * draw.tint.rgb, face_up(s.up), s.frag_y, s.light);"
        ));
        assert!(
            !src.contains("TerrainLight(1.0, s.shadow"),
            "a fluid lit as open sky"
        );
        // The sky's reflection is as bright as the sky light there.
        assert!(surface.contains("s.sky = light_curve(i.flood.x, SKY_CURVE);"));
        for entry in ["water_fs", "water_scene_fs"] {
            let fs = body(entry);
            assert!(
                fs.contains("deck_fw, ") && fs.contains(") * s.sky"),
                "{entry}: the sky reflection"
            );
            assert!(fs.contains("see_through_light(i, n)"), "{entry}: glass");
            assert!(!fs.contains("lambert("), "{entry}: lit as open sky");
        }
        assert!(body("see_through_light").contains("shadow_light(i.rel, n, l.direct)"));
    }

    #[test]
    fn the_shadow_multiplies_the_direct_sun_and_moon_term_only() {
        // docs/DAY_NIGHT.md §10: the shadow factor shades the direct term; the hemisphere light
        // (the ambient, AO and flood-light driven) never sees it.
        let src = super::source(4);
        let body = |name: &str| {
            let at = src.find(&format!("fn {name}(")).expect(name);
            src[at..at + src[at..].find("\n}\n").expect("end")].to_string()
        };
        let lit = body("lambert_lit");
        // The shadow (the maps, or the flood light's stand-in past them) is \`l.direct\`.
        // (Its share is on the pixel-art preview's toon levels when that is on: `band`.)
        assert!(lit.contains(
            "let ks = band(max(dot(n, d.sun.xyz), 0.0) * own(d.sun.xyz, face) * l.direct"
        ));
        assert!(lit.contains(
            "let km = band(max(dot(n, d.moon.xyz), 0.0) * own(d.moon.xyz, face) * l.direct"
        ));
        assert!(
            lit.contains(
                "let irradiance = direct + hemi + BLOCK_LIGHT * (PI * l.glow) + l.points;"
            )
        );
        let direct: Vec<&str> = lit.lines().filter(|l| l.contains("direct")).collect();
        assert!(
            direct
                .iter()
                .all(|l| !l.contains("hemi_") && !l.contains("ambient"))
        );
        let hemi = lit
            .lines()
            .find(|l| l.contains("let hemi ="))
            .expect("hemi");
        assert!(!hemi.contains("direct") && hemi.contains("* ambient"));
        assert!(lit.contains("let ambient = band(l.ambient, frame.pixel_art.y);"));
        // Every scene surface takes its shadow from the maps, or past them the flood light's
        // direct share (docs/DAY_NIGHT.md §11); characters too.
        assert!(body("scene_out").contains("shadow_light(rel, n, l.direct)"));
        assert!(
            super::super::characters::WGSL.contains("l.direct = shadow_light(rel, n, l.direct);")
        );
        assert!(body("lambert").contains("lambert_lit(albedo, n, frag_y, open_light())"));
        // The water's glint and sparkle are direct light; its reflection of the sky is not.
        let surface = body("surface_light");
        assert!(surface.contains("(glint + spark) * s.shadow"));
        assert!(surface.contains("fres * reflected +"));
        assert!(!surface.contains("reflected * s.shadow"));
        // The lookup's result is direct light's: past the maps (or with the light faded) it is
        // the stand-in, which the first layer's sky light replaces.
        let look = body("shadow_light");
        assert!(look.contains(
            "let inside = mix(stand_in, shadow_pcf(c, p) * lod_shadow(rel), shadows.cfg.y);"
        ));
        assert!(look.contains("return stand_in * cloud_shadow(rel);"));
        // Past the cascades, the stand-in shaded by the distant terrain (the third layer), and a
        // blend between (dwell-look `lod_shadows::combine`).
        assert!(look.contains(
            "return mix(shadow_past(rel, n, stand_in), inside, near) * cloud_shadow(rel);"
        ));
        assert!(
            body("shadow_past")
                .contains("return stand_in * (1.0 + shadows.cfg.y * (lod_shadow(rel) - 1.0));")
        );
    }

    #[test]
    fn tone_mapping_and_fog_are_only_in_the_composite() {
        // The scene passes write linear colour: their fragment shaders never tone-map, encode or
        // fog; the composite does all three.
        let src = super::source(4);
        let body = |name: &str| -> String {
            let at = src.find(&format!("fn {name}(")).expect(name);
            let rest = &src[at..];
            rest[..rest.find("\n}\n").expect("end")].to_string()
        };
        for entry in ["terrain_fs", "cutout_fs", "flat_fs", "scene_out"] {
            let b = body(entry);
            for f in ["aces(", "srgb_encode(", "display(", "fog_amount(", "sky("] {
                assert!(!b.contains(f), "{entry} calls {f}");
            }
        }
        let c = body("composited_of");
        assert!(c.contains("display(") && c.contains("fog_amount("));
        let c = body("composited_scene");
        assert!(c.contains("display(") && c.contains("fogged("));
        let c = body("composited");
        assert!(
            c.contains("sky(")
                && c.contains("sky_behind_with_clouds(")
                && body("composited_decks").contains("dwell_cloud_deck(")
        );
        assert!(body("composite_pixel").contains("composited("));
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
        let frame = body("composited_scene");
        assert_eq!(frame.matches("fogged(").count(), 1);
        assert!(frame.contains(", s.ray * depth, s.clear)"));
        // Over the scene only where the deck is in front of it, by the stored view depth.
        let front = frame
            .find("cloud_in_front_of(s.deck_t, s.ray, depth)")
            .expect("test");
        assert!(frame[front..].contains("shown = dwell_cloud_over(shown, s.front)"));
        assert!(frame.contains("color = mix(s.back, shown, min(c.a, 1.0))"));
        assert!(frame.contains("dwell_cloud_fog(color, frame.cloud_inside.x"));
        // The layer is the clouds hazed toward the clear sky, found where the deck is in front.
        let c = body("composited");
        assert!(c.contains("dwell_cloud_layer(clear, dir,"));
        assert!(c.contains("cloud_in_front_of(deck.z, ray, depth)"));
        // The deck's footprint is taken before any branch (derivatives need uniform control
        // flow): in composited_decks, which composite_pixel calls before its waterline branch.
        let c = body("composited_decks");
        assert!(c.contains("dwell_cloud_deck(") && c.contains("fwidth(") && !c.contains("if ("));
        let px = body("composite_pixel");
        let decks = px
            .find("let decks = composited_decks(ndc);")
            .expect("decks");
        assert!(
            decks
                < px.find("if (frame.water.x > 1.5)")
                    .expect("the waterline branch")
        );
    }

    #[test]
    fn the_water_reflects_land_from_the_scene_and_the_sky_with_its_clouds() {
        // WATER_RENDERING.md §8: the water that reads the scene reflects it by screen-space
        // reflection over the sky and clouds; Low (no copy) reflects the sky and clouds alone. The
        // deck's footprint is taken before any branch (derivatives need uniform control flow).
        let src = super::source(4);
        let body = |name: &str| {
            let at = src.find(&format!("fn {name}(")).expect(name);
            src[at..at + src[at..].find("\n}\n").expect("end")].to_string()
        };
        let scene = body("water_scene_fs");
        assert!(scene.contains("ssr_reflected(reflected_env(") && scene.contains("ssr_trace("));
        let low = body("water_fs");
        assert!(low.contains("reflected_env(") && !low.contains("ssr_"));
        for (name, b) in [("water_scene_fs", &scene), ("water_fs", &low)] {
            let fw = b.find("reflected_deck_fw(").expect(name);
            assert!(fw < b.find("if (").expect(name), "{name}");
        }
        let env = body("reflected_env");
        assert!(
            env.contains("sky(r)")
                && env.contains("dwell_clouds(")
                && env.contains("cloud_lighting()")
        );
        // What the scene hides of the sky hides the sun's glint and sparkle too.
        assert!(body("surface_light").contains("(1.0 - refl.a) * (glint + spark)"));
    }

    #[test]
    fn the_sun_and_moon_are_reflected_with_paths_only_on_a_body_path_tier() {
        // WATER_RENDERING.md §15: the discs and glitter paths are gated on the reflection tier's
        // bit (the CPU tier's is off: a simple highlight, the glint, only); the discs go under the
        // reflected clouds; the clouds' cover dims the path.
        let src = super::source(4);
        let body = |name: &str| {
            let at = src.find(&format!("fn {name}(")).expect(name);
            src[at..at + src[at..].find("\n}\n").expect("end")].to_string()
        };
        let light = body("surface_light");
        assert!(light.contains("if (reflect_path()) {\n            glint += body_paths(s, f0);"));
        let env = body("reflected_env");
        let bodies = env
            .find("reflected_bodies(r, clear)")
            .expect("discs reflected");
        assert!(env[..bodies].contains("reflect_path()"));
        assert!(
            bodies
                < env
                    .find("dwell_clouds(under")
                    .expect("clouds over the discs")
        );
        let paths = body("body_paths");
        assert!(
            paths.contains("d.stars.w") && paths.contains("frame.cloud_cfg.x * PATH_CLOUD_FADE")
        );
        assert!(
            paths.contains("face_rise(s.up, d.moon.xyz)") && paths.contains("own(d.sun.xyz, s.up)")
        );
        let cpu = dwell_look::water_tiers::cpu_tier();
        let u = dwell_look::reflections::reflection_uniforms(
            &dwell_look::reflections::REFLECTIONS,
            cpu,
            8,
            true,
            false,
        );
        assert_eq!(u[1][1] as u32 & 2, 0, "the CPU tier draws no path");
    }

    #[test]
    fn the_wind_is_drawn_by_the_motion_pass_only_while_there_is_any() {
        // The composite never draws the wind: with it on, the renderer draws the finished image
        // (`finished_fs`) and the motion pass over it (`motion_fs`, render/mod.rs `motion_pass`).
        let src = super::source(4);
        let body = |name: &str| {
            let at = src.find(&format!("fn {name}(")).expect(name);
            src[at..at + src[at..].find("\n}\n").expect("end")].to_string()
        };
        for entry in ["composite_fs", "finished_fs", "composite_pixel"] {
            assert!(!body(entry).contains("motion_"), "{entry} draws no wind");
        }
        let m = body("motion_fs");
        assert!(m.contains("motion_composite(p, i.ndc)") && m.contains("dither(p)"));
        // Under water the wind isn't drawn (as before the split).
        assert!(m.contains("if (!eye_under())"));
        // Procedural: no textures beyond the finished image, which the edge blur loads.
        let motion = &src[src.find("fn motion_hash(").expect("motion")
            ..src.find("const EDGE_DEPTH").expect("outline")];
        assert!(!motion.contains("textureSample("));
    }

    #[test]
    fn post_effects_that_sample_neighbours_read_the_finished_image() {
        // The pattern (docs/MOTION_EFFECTS.md): every per-pixel effect of the composite lives in
        // `composite_pixel`; nothing but the dither comes after it; and an effect that samples
        // other pixels (the edge blur) reads the finished image `composite_pixel` wrote, never
        // the scene. Three times the blur's taps re-composited the scene without the newest
        // per-pixel effect, which then vanished outside its inner circle: the clouds from below
        // (0.11.19), from above (0.18.2) and the ink (0.20.1). With this structure an effect
        // added to `composite_pixel` is in the taps without anyone remembering it.
        let src = super::source(4);
        let body = |name: &str| {
            let at = src.find(&format!("fn {name}(")).expect(name);
            src[at..at + src[at..].find("\n}\n").expect("end")].to_string()
        };
        // The two outputs of the composite are composite_pixel alone (and the dither on screen).
        let canvas = body("composite_fs");
        assert!(
            canvas.contains("return vec4f(composite_pixel(p, i.ndc) + dither(p), 1.0);"),
            "{canvas}"
        );
        assert_eq!(
            canvas.matches("let ").count(),
            1,
            "nothing else in composite_fs"
        );
        let finished = body("finished_fs");
        assert!(finished.contains("return vec4f(composite_pixel(vec2i(i.clip.xy), i.ndc), 1.0);"));
        assert_eq!(
            src.matches("composite_pixel(").count(),
            3,
            "defined once, called by the two outputs"
        );
        // The neighbour-sampling pass reads the finished image and nothing of the scene.
        let motion = &src[src.find("const TAU").expect("motion")
            ..src.find("const EDGE_DEPTH").expect("outline")];
        for scene in [
            "scene_color",
            "scene_depth",
            "composited",
            "outlined(",
            "ink_",
            "dwell_cloud",
            "sky(",
        ] {
            assert!(!motion.contains(scene), "the motion pass reads {scene}");
        }
        let blur = body("motion_composite");
        assert!(blur.contains("let own = textureLoad(finished, px, 0).rgb;"));
        assert!(blur.contains("sum += textureLoad(finished, vec2i(q), 0).rgb;"));
        assert_eq!(
            blur.matches("sum +=").count(),
            1,
            "every tap is a texel of the finished image"
        );
    }

    #[test]
    fn the_ink_outline_survives_the_edge_blur() {
        // Owner's report (2026-10-20, `?outline=creases`, sprinting): the outlines were masked
        // outside a circle. The blur's taps re-composited the scene (`composited_over`) without
        // the ink, so past the blur's inner radius four of the five samples had no line. The ink
        // is part of the finished image, and the taps read that image.
        let src = super::source(4);
        let body = |name: &str| {
            let at = src.find(&format!("fn {name}(")).expect(name);
            src[at..at + src[at..].find("\n}\n").expect("end")].to_string()
        };
        let pixel = body("composite_pixel");
        let ink = pixel
            .find("s.color = outlined(s, p);")
            .expect("the ink is in the pixel");
        assert!(
            pixel[ink..].contains("return with_shafts(s.color, p);"),
            "nothing undoes it after"
        );
        assert!(body("finished_fs").contains("composite_pixel("));
        assert!(body("motion_composite").contains("textureLoad(finished, vec2i(q), 0)"));
    }

    #[test]
    fn the_ink_outline_is_drawn_only_while_on_and_fades_far_pixels_out() {
        let src = super::source(4);
        let body = |name: &str| {
            let at = src.find(&format!("fn {name}(")).expect(name);
            src[at..at + src[at..].find("\n}\n").expect("end")].to_string()
        };
        // Off by default (zeros in the frame block): the composite skips it. It is part of the
        // finished picture, so the wind (a later pass) goes over it.
        let c = body("composite_pixel");
        let on = c.find("if (frame.outline.x > 0.5) {").expect("switch");
        assert!(
            c[on..].starts_with("if (frame.outline.x > 0.5) {\n        s.color = outlined(s, p);")
        );
        // The edge rule of dwell-look's outline.rs: inverse depth's second difference and slope
        // change, the nearest tap's depth, and the distance fade over the ray's length so far
        // pixels get none (dwell-look's `far_pixels_get_no_outline` pins the same rule).
        let rule = body("ink_edges");
        assert!(rule.contains(
            "let e = max(abs(w[3] + w[5] - 2.0 * w[4]), abs(w[1] + w[7] - 2.0 * w[4])) / top;"
        ));
        assert!(rule.contains("ink_change(w[6] - w[0], w[8] - w[2], top)"));
        assert!(body("ink_change").contains("abs(a - b) / (abs(a) + abs(b) + 1e-4 * top)"));
        let edge = body("ink_outline");
        assert!(edge.contains("let depth = 1.0 / near;"));
        assert!(edge.contains(
            "let fade = 1.0 - smoothstep(frame.outline.z, frame.outline.w, depth * length(ray));"
        ));
        assert!(edge.contains("return vec2f(s * fade * frame.outline_ink.a, depth);"));
        // And the height fog takes the ink toward the sky as it takes the ground.
        let o = body("outlined");
        assert!(o.contains("mix(frame.outline_ink.rgb, s.clear, fog_amount(rel))"));
    }

    #[test]
    fn every_plant_past_its_fade_is_dropped_in_the_vertex_stage() {
        // WORK_QUEUE item 32: the forest edge drew every blade of tall grass out to the
        // full-detail distance and discarded it in the fragment stage (14 of the edge's 25 ms
        // on the M1 at High). Any plant past its fade's end (plant.w) is dropped whole, not only
        // leaf cards (plant.y < 0): the fade check comes before the cards-only return.
        let src = super::source(4);
        let at = src.find("fn card_dropped(").expect("card_dropped");
        let body = &src[at..at + src[at..].find("\n}\n").expect("end")];
        let fade = body
            .find("plant.w > 0.0 && length(rel) > plant.w + 2.0")
            .expect("the fade drop");
        let cards_only = body.find("plant.y >= 0.0").expect("the cards-only return");
        assert!(
            fade < cards_only,
            "grass past its fade is not dropped:\n{body}"
        );
        assert!(
            src.contains("if (card_dropped(still,"),
            "cutout_vs drops by it"
        );
    }

    #[test]
    fn every_terrain_draw_cross_fades_and_a_level_fading_out_takes_no_ink() {
        // The LOD cross-fade (dwell-lod fade.rs): chunks, cutouts, sections, stand-ins and water
        // all discard by the draw's fade, and the opaque ones flag their view depth while fading
        // out, so the ink's edges come from the incoming level (dwell-look lod_blend.rs tests the
        // rule: unflagged, the dither's seams draw as lines).
        let src = super::source(4);
        let body = |name: &str| {
            let at = src.find(&format!("fn {name}(")).expect(name);
            src[at..at + src[at..].find("\n}\n").expect("end")].to_string()
        };
        for entry in [
            "terrain_fs",
            "cutout_fs",
            "flat_fs",
            "water_fs",
            "water_scene_fs",
        ] {
            assert!(
                body(entry).contains("fade_discard(draw, i.clip.xy)"),
                "{entry} does not fade"
            );
        }
        for entry in ["terrain_fs", "flat_fs"] {
            assert!(
                body(entry).contains("frame_depth(o.depth, fade_inked(draw))"),
                "{entry}"
            );
        }
        assert!(body("cutout_fs").contains("i.bare < 0.5 && fade_inked(draw)"));
        // The threshold rule is dwell-lod's `Fade::keeps` (in: under t; out: t and over) on its
        // encoding (`Fade::shader_value`), with the pattern from shared/look/lodblend.json.
        let rule = body("fade_discard");
        assert!(rule.contains("let t = abs(w) - 1.0;"));
        assert!(rule.contains("return select((n < t), (n >= t), (w > 0.0));"));
        assert!(body("fade_inked").contains("return draw.clip_hi.w >= 0.0;"));
        assert!(src.contains(&dwell_look::lod_blend::LOD_BLEND.pattern.wgsl()));
        use dwell_lod::fade::Fade;
        for (fade, n) in [
            (Fade::In(0.25), 0.2f32),
            (Fade::In(0.25), 0.3),
            (Fade::Out(0.25), 0.2),
        ] {
            let w = fade.shader_value();
            let t = w.abs() - 1.0;
            let discard = if w > 0.0 { n >= t } else { n < t };
            assert_eq!(!discard, fade.keeps(n), "{fade:?} {n}");
        }
    }

    /// The LOD z-fighting class (ARCHITECTURE.md §6.6, "Known bug class"): during a swap the two
    /// levels are drawn over each other, their ground often in one plane, so depth must never be what
    /// picks between them. The dither does: through the encoding the shaders decode (`fade_discard`:
    /// t = |w| − 1 in f32, which is not t exactly), at every pixel of every pattern and every
    /// progress — the pattern's own steps included, where `<` against `>=` decides — the incoming
    /// level and the outgoing one keep exactly one of the pixel between them, so coincident depths
    /// with different colours never reach the depth test.
    #[test]
    fn the_two_levels_of_a_swap_never_both_draw_a_pixel_through_the_shaders_encoding() {
        use dwell_lod::fade::Fade;
        use dwell_look::lod_blend::FadePattern;
        // The shaders' fade_discard, on the f32 value the draw carries.
        let shader_keeps = |fade: Fade, n: f32| {
            let w = fade.shader_value();
            if w == 0.0 {
                return true;
            }
            let t = w.abs() - 1.0;
            if w > 0.0 { n < t } else { n >= t }
        };
        for pattern in [FadePattern::Bayer4, FadePattern::Bayer8, FadePattern::Noise] {
            let ns: Vec<f32> = (0..64)
                .flat_map(|y| (0..64).map(move |x| pattern.threshold(x, y)))
                .collect();
            let mut ts: Vec<f32> = (0..=2048).map(|k| k as f32 / 2048.0).collect();
            // Progress landing on a threshold, and one ulp either side of it.
            for &n in &ns {
                ts.extend([
                    n,
                    f32::from_bits(n.to_bits() + 1),
                    f32::from_bits(n.to_bits().max(1) - 1),
                ]);
            }
            for &t in &ts {
                for &n in &ns {
                    let kept = [Fade::In(t), Fade::Out(t)]
                        .iter()
                        .filter(|f| shader_keeps(**f, n))
                        .count();
                    assert_eq!(
                        kept, 1,
                        "{pattern:?} t {t} n {n}: {kept} levels draw the pixel"
                    );
                }
            }
        }
    }

    #[test]
    fn plants_without_ink_are_flagged_in_the_depth_and_seen_through() {
        // Owner's report (2026-10-20, `?outline=creases`): tall grass was mostly black, every
        // blade inked. A cutout whose block says `outline: false` (the cross plants' default)
        // flags its view depth; the ink skips a flagged pixel, and the edge rule sees through
        // flagged taps (dwell-look `see_through`), so a plant's edges against the ground or the
        // sky draw nothing either.
        let src = super::source(4);
        let body = |name: &str| {
            let at = src.find(&format!("fn {name}(")).expect(name);
            src[at..at + src[at..].find("\n}\n").expect("end")].to_string()
        };
        assert!(body("cutout_vs").contains("o.bare = v.bare;"));
        assert!(
            body("cutout_fs").contains("frame_depth(lit.depth, i.bare < 0.5 && fade_inked(draw))")
        );
        let edge = body("ink_outline");
        assert!(
            edge.contains("|| !frame_inked(centre)"),
            "no ink on the plant itself"
        );
        assert!(edge.contains("raw[k] = select(-1.0, ink_inv(d), frame_inked(d));"));
        assert!(edge.contains("let w = ink_see_through(raw);"));
        let through = body("ink_see_through");
        assert!(through.contains("w[k] = select(raw[4], max(2.0 * raw[4] - o, 0.0), o >= 0.0);"));
        // The flag is the depth's lowest bit (the next the self-lit flag, cleared with it);
        // nothing drawn (0) takes the ink.
        let flag = super::FRAME_DEPTH;
        assert!(flag.contains("let b = bitcast<u32>(d) & ~3u;"));
        assert!(flag.contains("return (bitcast<u32>(d) & 2u) != 0u;"));
        assert!(flag.contains("return bitcast<f32>(select(b | 1u, b, inked));"));
        assert!(flag.contains("return (bitcast<u32>(d) & 1u) == 0u;"));
    }

    #[test]
    fn a_self_lit_surface_is_flagged_and_keeps_its_colour_by_moonlight() {
        // docs/LIGHTS.md §2: a fully glowing tile or material (a flame) flags its pixels in the
        // view depth, and the composite leaves them out of the time of day's desaturation.
        let src = super::source(4);
        let body = |name: &str| {
            let at = src.find(&format!("fn {name}(")).expect(name);
            src[at..at + src[at..].find("\n}\n").expect("end")].to_string()
        };
        for entry in ["terrain_fs", "cutout_fs"] {
            assert!(
                body(entry).contains("self_lit_depth(frame_depth("),
                "{entry}"
            );
            assert!(body(entry).contains("emitted("), "{entry}");
        }
        assert!(body("scene_display").contains("mix(frame.day.horizon.a, 1.0, self_lit)"));
        assert!(body("composited").contains("self_lit = scene_self_lit_at(p);"));
        let chars = super::super::characters::source();
        assert!(chars.contains("o.depth = self_lit_depth(o.depth, m.extra.z >= 1.0);"));
    }

    #[test]
    fn every_view_depth_writer_goes_through_frame_depth() {
        // The ink flag is the view depth's lowest bit, so every writer of the frame's view depth
        // must set it (`frame_depth`), or a surface's flag would be its depth's own noise. The
        // scene's fragment outputs (`SceneOut`, `CutoutSceneOut`).
        let src = super::source(4);
        let mut writers = 0;
        for s in [src.as_str()] {
            for (at, _) in s.match_indices("@fragment\nfn ") {
                let f = &s[at..at + s[at..].find("\n}\n").expect("end")];
                let header = &f[..f.find('{').expect("body")];
                if !["-> SceneOut", "-> CutoutSceneOut"]
                    .iter()
                    .any(|t| header.contains(t))
                {
                    continue;
                }
                for line in f.lines().filter(|l| l.contains(".depth = ")) {
                    writers += 1;
                    assert!(line.contains("frame_depth("), "{header}: {line}");
                }
            }
        }
        // And scene_out, which terrain_fs and flat_fs return.
        let scene_out = &src[src.find("fn scene_out(").expect("scene_out")..];
        assert!(
            scene_out[..scene_out.find("\n}\n").unwrap()]
                .contains("frame_depth(view_depth(rel), true)")
        );
        assert!(writers >= 2, "the cutout and the water: {writers}");
    }

    #[test]
    fn the_edge_blur_taps_see_the_clouds_the_pixel_sees() {
        // Owner's report (2026-10-16): the clouds tunnelled into a circle at speed. The blur's
        // taps composited over the clear sky (`sky(ray)`, no clouds), so past the blur's inner
        // radius every sky pixel was averaged with four cloudless taps: the clouds dropped to a
        // fifth at a hard circular edge. Each tap is now a pixel of the finished image, which has
        // its own sky with its clouds (`composited`: over `back`, `dwell_clouds`).
        let src = super::source(4);
        let body = |name: &str| {
            let at = src.find(&format!("fn {name}(")).expect(name);
            src[at..at + src[at..].find("\n}\n").expect("end")].to_string()
        };
        let m = body("motion_composite");
        assert!(!m.contains("sky(") && m.contains("textureLoad(finished, vec2i(q), 0)"));
        let c = body("composited");
        assert!(
            c.contains("sky_behind_with_clouds(")
                && body("sky_behind_with_clouds").contains("dwell_clouds(")
                && c.contains("composited_scene(c, depth, self_lit, s)")
        );
        assert!(body("composited_scene").contains("var color = s.back;"));
        assert!(body("composite_pixel").contains("composited(p, ndc, decks)"));
    }

    #[test]
    fn the_deck_is_over_the_far_flat_world_from_high_above() {
        // Owner's report (2026-10-21): from tens of km up, the clouds showed in a circle round
        // the player and over the void past the world's edge, not over the sea between. The
        // sphere's hit sank behind the flat sea. Every test of the deck against the scene (the
        // pixel, the scene under it, the ink) goes through `cloud_in_front_of`, which also puts
        // the deck in front of anything below its altitude seen from above, on the eye's face.
        for samples in [1, 4] {
            let src = super::source(samples);
            let at = src.find("fn cloud_in_front_of(").expect("helper");
            let helper = &src[at..at + src[at..].find("\n}\n").unwrap()];
            assert!(helper.contains(
                "dwell_cloud_in_front(t, length(ray), depth, face_rise(frame.eye.y, ray), frame.cloud_cfg.w)"
            ));
            // No other caller: the deck's own fn is the helper's alone.
            assert_eq!(
                src.matches("dwell_cloud_in_front(").count(),
                2,
                "{samples} samples"
            );
            assert_eq!(
                src.matches("cloud_in_front_of(").count(),
                4,
                "{samples} samples"
            );
        }
    }

    #[test]
    fn the_edge_blur_taps_keep_the_clouds_in_front_of_the_scene() {
        // Owner's report (2026-10-20): above the deck, looking down, moving fast, the clouds
        // showed only inside a circle. The blur's taps composited the scene without the deck in
        // front of it. Each tap is now a pixel of the finished image: under its clouds where the
        // deck is in front of its own depth, and inside the in-cloud fog, as every pixel is.
        let src = super::source(4);
        let body = |name: &str| {
            let at = src.find(&format!("fn {name}(")).expect(name);
            src[at..at + src[at..].find("\n}\n").expect("end")].to_string()
        };
        let scene = body("composited_scene");
        let front = scene
            .find("cloud_in_front_of(s.deck_t, s.ray, depth)")
            .expect("in front");
        assert!(scene[front..].contains("shown = dwell_cloud_over(shown, s.front)"));
        assert!(scene.contains("dwell_cloud_fog(color, frame.cloud_inside.x"));
        let c = body("composited");
        let layer = c.find("dwell_cloud_layer(").expect("layer");
        assert!(
            c[..layer]
                .contains("if (!volume && c.a > 0.0 && cloud_in_front_of(deck.z, ray, depth))")
        );
        assert!(body("motion_composite").contains("textureLoad(finished, vec2i(q), 0)"));
        // The layer is the clouds dwell_clouds draws, found once.
        let look = dwell_look::clouds::cloud_wgsl(&dwell_look::clouds::CLOUDS);
        assert!(look.contains("return dwell_cloud_over(under, dwell_cloud_layer(clear,"));
    }

    #[test]
    fn only_the_composite_marches_the_volume_and_the_cheap_sky_keeps_the_deck() {
        // docs/CLOUDS.md §8: the composite marches the volumetric clouds (once a pixel, out to the
        // scene) in place of the eye's deck; Snell's window and the reflections sample
        // sky_above_with_clouds, which stays the 2D deck (WATER_RENDERING.md §16).
        let src = super::source(4);
        let body = |name: &str| {
            let at = src.find(&format!("fn {name}(")).expect(name);
            src[at..at + src[at..].find("\n}\n").expect("end")].to_string()
        };
        let c = body("composited");
        assert!(c.contains("vol = upsampled_clouds(ndc, ray,"));
        assert!(c.contains("deck = vec4f(deck.xy, 0.0, 0.0);"));
        // The march runs in the clouds pass only (at the tier's resolution), accumulated.
        assert_eq!(src.matches("= dwell_cloud_march(").count(), 1);
        assert!(body("clouds_fs").contains("= dwell_cloud_march("));
        // BUGS.md #90: the history goes in the resolve, clamped to the raw march's neighbourhood
        // (not dropped as the eye moves), and the jitter is the baked blue noise.
        let resolve = body("clouds_resolve_fs");
        assert!(resolve.contains("cloud_history_at(uv, rec)"));
        assert!(resolve.contains("mix(clamp(hist.color, lo, hi), cur,"));
        assert!(!resolve.contains("parallax"));
        assert!(!body("clouds_fs").contains("cloud_history"));
        assert!(body("cloud_jitter").contains("dwell_cloud_blue(p)"));
        let cheap = body("sky_above_with_clouds");
        assert!(!cheap.contains("dwell_cloud_march") && !cheap.contains("upsampled_clouds"));
        // BUGS.md #88: the march ends at the texel's farthest scene pixel, the texel keeps its
        // scene span (not the jittered first cloud), and the upsample and the history weigh texels
        // by it (dwell-look's upsample_weight twin), not by a ±5 % cut at the first cloud.
        let fs = body("clouds_fs");
        assert!(fs.contains("let rec = cloud_scene_span(p, length(ray));"));
        assert!(fs.contains("dwell_cloud_march_end(rec)"));
        assert!(fs.contains(
            "return CloudOut(vec4f(layer.rgb * layer.a, layer.a), vec4f(rec, first, 0.0));"
        ));
        let up = body("upsampled_clouds");
        assert!(up.contains("dwell_cloud_upsample_weight(e, textureLoad(cloud_dist_tex"));
        assert!(!up.contains("smoothstep(0.95, 1.05"));
        assert!(body("cloud_history_at").contains("dwell_cloud_upsample_weight(rec.y,"));
        assert!(src.contains(&format!(
            "const CLOUD_DEPTH_TAPS: i32 = {};",
            dwell_look::clouds::MAX_DIVISOR
        )));
    }
}
