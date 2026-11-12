//! The sky's clouds (docs/CLOUDS.md, ADR 0034): a thin cloud deck at a fixed altitude, drawn as part
//! of the sky (not 3D). Each sky pixel's view ray meets the deck — a sphere of radius `curvature_m`
//! around a point that far below the eye, `altitude_m` above sea level — and the hit point's
//! horizontal world position drives a domain-warped fBm of value noise. So the clouds have true
//! perspective: large overhead, compressed toward the horizon, which the deck's curve meets at a
//! finite distance; they move past slowly as the eye flies under them.
//!
//! The tunables are data (`shared/look/clouds.json` → `clouds.gen.rs`); the shader is generated
//! here from them ([`cloud_wgsl`]), as the sky's is. The noise repeats every `wrap_m` metres (a
//! whole number of its largest cells, with an octave transform that keeps the lattice), so the
//! renderer wraps the eye's and the wind's offsets into one period in f64 and the shader's f32
//! coordinates stay small anywhere in the world. Client-side visuals only: no network, no
//! simulation.

use crate::Rgb;
use crate::daynight::DAY_NIGHT;
use dwell_world::FaceFrame;

/// How far (in the sine of the sun's height) the clouds' twilight reaches either side of the
/// horizon.
const TWILIGHT: f64 = 0.1;

/// Where the deck is and how it fades (`clouds.json`'s `deck`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeckTunables {
    pub altitude_m: f64,
    pub curvature_m: f64,
    pub cross_fade_m: f64,
    pub inside_fog: f64,
    pub horizon_fade: f64,
    pub fade_start_m: f64,
    pub fade_end_m: f64,
}

/// The noise and its remapping to cloud (`shape`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeTunables {
    pub scale_m: f64,
    pub wrap_m: f64,
    pub octaves: u32,
    pub low_octaves: u32,
    pub gain: f64,
    pub warp: f64,
    pub coverage: f64,
    pub edge_softness: f64,
    pub opacity: f64,
}

/// The drift (`wind`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindTunables {
    pub speed_mps: f64,
    pub direction_deg: f64,
    pub evolve_mps: f64,
    pub evolve_direction_deg: f64,
}

/// The lighting (`light`): sRGB colours and weights.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloudLight {
    pub sunlit: Rgb,
    pub shade: Rgb,
    pub sunset: Rgb,
    pub night_lit: Rgb,
    pub night_shade: Rgb,
    pub absorption: f64,
    pub shadow_step_m: f64,
    pub silver_lining: f64,
    pub silver_power: f64,
    pub sun_warmth: f64,
    pub top_light: f64,
}

/// The volumetric layer (`volume`, docs/CLOUDS.md §8).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeTunables {
    pub base_m: f64,
    pub top_m: f64,
    pub extinction: f64,
    /// The first light sample's length toward the sun or moon (m); each next is
    /// `light_step_growth` times the last.
    pub light_step_m: f64,
    pub light_step_growth: f64,
    /// The floor of light that reaches deep inside (multiple scattering: a wider, weaker Beer).
    pub multi_scatter: f64,
    /// The powder term: thin edges facing away from the light a little darker.
    pub powder: f64,
    /// The dual-lobe Henyey–Greenstein phase: the forward and back lobes' g, the back lobe's
    /// share, and how much it shapes the light (0: even all round).
    pub phase_forward: f64,
    pub phase_back: f64,
    pub phase_back_weight: f64,
    pub phase_strength: f64,
    /// The sun's (or moon's) light's weight.
    pub sun_strength: f64,
    /// The sky's light at a cloud's top (× the shade colour), and the ground's bounce at its base
    /// (× the shade colour × `bounce_tint`); each dims by e every `ambient_depth_m` of cloud
    /// above (the sky's) or below (the bounce's) the sample.
    pub sky_ambient: f64,
    pub ground_bounce: f64,
    pub bounce_tint: Rgb,
    pub ambient_depth_m: f64,
    pub max_distance_m: f64,
    /// The composite's band against the scene (BUGS.md #88): a texel's cloud is a pixel's where
    /// the texel's march ended within the band of the pixel's scene: the texel's depth range, at
    /// least `depth_band` and at most `depth_band_range` of the distance.
    pub depth_band: f64,
    pub depth_band_range: f64,
    /// How much rising 3D noise breaks a plume's density up (0: smooth).
    pub plume_billow: f64,
    /// Within this of the layer (or in it) a tier marches with its near steps and divisor
    /// (BUGS.md #90).
    pub near_m: f64,
}

/// The clouds' shape (`form`, docs/CLOUDS.md §9): the baked noise's sizes and tiles, and how the
/// detail erodes the base shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FormTunables {
    /// The base and detail textures' sides (texels; powers of two).
    pub base_size: u32,
    pub detail_size: u32,
    /// How far each texture (and the curl's field) repeats, m; each divides `shape.wrap_m`.
    pub base_tile_m: f64,
    pub detail_tile_m: f64,
    pub curl_tile_m: f64,
    /// How far the curl moves the detail's point at the layer's floor (m; none at its top).
    pub curl_m: f64,
    /// How much of the base shape's density the detail may take away (only where it is below 1).
    pub detail_erosion: f64,
    /// Below this share of the layer the detail is wispy (its fBm), above it billowy (inverted).
    pub wisp_height: f64,
    /// How far the shape leans downwind per metre up (m/m).
    pub shear: f64,
    /// The weather map's cover rises from 0 to 1 over this much of the deck's noise past the cut.
    pub coverage_softness: f64,
}

/// The weather map's data (`weather`, §9.2): the cloud type along the type axis and the rain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeatherTunables {
    pub r#type: f64,
    pub type_variation: f64,
    pub type_from_coverage: f64,
    pub precipitation: f64,
}

/// A cloud type (`cloudTypes`, §9.2): its place along the weather map's type axis and its height
/// profile in the layer (`base`: the fade-in from the floor, `top`: the fade-out; shares of the
/// layer's depth), its anvil (how much its top spreads), density and how much rain darkens it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloudType {
    pub name: &'static str,
    pub at: f64,
    pub base: [f64; 2],
    pub top: [f64; 2],
    pub anvil: f64,
    pub density: f64,
    pub darkening: f64,
}

/// One Clouds setting value's march (`tiers`): steps per pixel (0: the 2D deck), light samples
/// per step, the march's resolution divisor and the share of each new frame in the accumulated
/// picture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeTier {
    pub steps: u32,
    pub light_steps: u32,
    pub divisor: u32,
    pub history_blend: f64,
    /// The samples through each plume a ray crosses (0: no plumes; §8.7).
    pub plume_steps: u32,
    /// The steps and divisor while the eye is near or in the layer (BUGS.md #90).
    pub near_steps: u32,
    pub near_divisor: u32,
}

impl VolumeTier {
    /// This tier as drawn with the eye `near` the layer ([`near_layer`]): its near steps and
    /// divisor.
    pub fn at(self, near: bool) -> Self {
        if near {
            Self {
                steps: self.near_steps,
                divisor: self.near_divisor,
                ..self
            }
        } else {
            self
        }
    }
}

/// Whether an eye `eye_h` above sea level (face-local) is near the layer: within `near_m` of it or
/// in it, with a band of half `near_m` more to leave it (`was`: last frame's), so hovering at the
/// edge doesn't switch the buffers every frame.
pub fn near_layer(v: &VolumeTunables, eye_h: f64, was: bool) -> bool {
    let off = (v.base_m - eye_h).max(eye_h - v.top_m).max(0.0);
    off < if was { 1.5 * v.near_m } else { v.near_m }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeTiers {
    pub high: VolumeTier,
    pub medium: VolumeTier,
    pub low: VolumeTier,
}

/// A plume the world starts with (`plumes`, the run-time list's defaults, [`crate::plumes`]): a
/// vertical column of cloud, e.g. a volcano's ash, at world x, z, its heights above sea level on
/// face A. Its radius grows by `spread` times from base to top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plume {
    pub name: &'static str,
    pub x_m: f64,
    pub z_m: f64,
    pub radius_m: f64,
    pub spread: f64,
    pub base_m: f64,
    pub top_m: f64,
    pub density: f64,
    /// Its colour (sRGB), tinting the lit cloud where it is the plume (ash grey, smoke).
    pub color: Rgb,
    /// How fast its billows rise (m/s).
    pub rise_mps: f64,
}

/// A run-time source's plume look (`sources`): a plume's, `height_m` tall over the source.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlumeSource {
    pub radius_m: f64,
    pub spread: f64,
    pub density: f64,
    pub color: Rgb,
    pub rise_mps: f64,
    pub height_m: f64,
}

/// The run-time sources the game knows (`sources`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlumeSources {
    /// The steam over a lava vent's active lake (VOLCANOES.md).
    pub lava_vent: PlumeSource,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloudTunables {
    pub deck: DeckTunables,
    pub shape: ShapeTunables,
    pub wind: WindTunables,
    pub light: CloudLight,
    pub volume: VolumeTunables,
    pub form: FormTunables,
    pub weather: WeatherTunables,
    pub tiers: VolumeTiers,
    /// The cloud types along the weather map's type axis, in order (§9.2).
    pub cloud_types: &'static [CloudType],
    /// How many plumes the run-time list holds (the plume uniform's slots).
    pub plume_slots: u32,
    pub sources: PlumeSources,
    pub plumes: &'static [Plume],
}

pub use crate::clouds_gen::CLOUDS;

/// The most octaves the shader loops over (`clouds.mjs`'s MAX_OCTAVES).
pub const MAX_OCTAVES: u32 = 8;
/// How far the octave transform scales: `[[2, 1], [−1, 2]]` maps the integer lattice onto itself
/// (so every octave repeats with the first) and turns each octave 26.6° off the last, which hides
/// the lattice's axes.
pub const LACUNARITY: f64 = 2.236_067_977_499_79;
/// The fBm's spread about its mean is narrow: it is stretched by this before the coverage cut, so
/// `coverage` is close to the share of the sky covered.
const CONTRAST: f64 = 1.9;

/// The Clouds setting: Off, Low (the 2D deck: fewer octaves, no self-shadow), Medium or High (the
/// volume at their `tiers`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloudQuality {
    Off,
    Low,
    Medium,
    High,
}

impl CloudQuality {
    /// From the settings store's name (`off`, `low`, `high`); anything else is None.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "off" => Some(Self::Off),
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }
    pub fn octaves(self, t: &CloudTunables) -> u32 {
        match self {
            Self::Off => 0,
            Self::Low => t.shape.low_octaves,
            Self::Medium | Self::High => t.shape.octaves,
        }
    }
    pub fn self_shadow(self) -> bool {
        self == Self::High
    }
    /// The volumetric march this setting draws (`tiers`; Off: none). Zero steps: the 2D deck.
    pub fn volume(self, t: &CloudTunables) -> VolumeTier {
        match self {
            Self::Off => VolumeTier {
                steps: 0,
                light_steps: 0,
                divisor: 1,
                history_blend: 1.0,
                plume_steps: 0,
                near_steps: 0,
                near_divisor: 1,
            },
            Self::Low => t.tiers.low,
            Self::Medium => t.tiers.medium,
            Self::High => t.tiers.high,
        }
    }
}

/// The most march steps and light samples per step (`clouds.mjs`'s MAX_STEPS, MAX_LIGHT_STEPS).
pub const MAX_STEPS: u32 = 64;
pub const MAX_LIGHT_STEPS: u32 = 6;
// The uniforms pack a tier's light samples as eighths of its steps (`cloud_uniforms`).
const _: () = assert!(MAX_LIGHT_STEPS < 8);
/// The most samples through one plume and plume slots (`clouds.mjs`'s MAX_PLUME_STEPS,
/// MAX_PLUME_SLOTS).
pub const MAX_PLUME_STEPS: u32 = 16;
pub const MAX_PLUME_SLOTS: u32 = 32;

/// The distance along a unit view ray to the deck. The deck is the sphere of radius `curvature +
/// altitude` about a centre `curvature` below sea level, under the eye; the eye is `height` below it
/// (negative: above it). `above` is the ray's dot with the viewer's up. From below (inside the
/// sphere) every ray meets it; from above only rays down past the deck's own horizon do, at the near
/// root. Each root is in the form that doesn't cancel (straight up or down it is |h|). None at the
/// deck itself, or for a ray that misses.
pub fn deck_distance(above: f64, height: f64, altitude: f64, curvature: f64) -> Option<f64> {
    // The eye's distance from the centre, and |eye|² − |deck|² (< 0 inside).
    let e = curvature + altitude - height;
    let k = -height * (2.0 * (curvature + altitude) - height);
    let ey = e * above;
    if k < 0.0 {
        Some(-k / (ey + (ey * ey - k).sqrt()))
    } else if k == 0.0 || above >= 0.0 {
        None
    } else {
        let disc = ey * ey - k;
        (disc >= 0.0).then(|| k / (-ey + disc.sqrt()))
    }
}

/// The distance fade's start and end along the ray. From below they are the data's; from above
/// they reach out with the deck's own horizon (`√(|eye|² − |deck|²)`: from 40% of it to all of it),
/// so from high above the deck reads as a cloud sea out to its horizon. `dwell_cloud_fade` is this.
pub fn deck_fade(t: &CloudTunables, height: f64) -> (f64, f64) {
    let (a, r) = (t.deck.altitude_m, t.deck.curvature_m);
    let k = -height * (2.0 * (r + a) - height);
    let far = k.max(0.0).sqrt();
    (
        t.deck.fade_start_m.max(0.4 * far),
        t.deck.fade_end_m.max(far),
    )
}

/// How far from level (the ray's dot with up) the edge-on band reaches.
pub const BAND: f64 = 0.12;

/// The deck seen edge-on from within the layer, `cross_fade_m` either side of it: for a ray nearly
/// level with it (`above`, its dot with up), cloud where the ray leaves the layer, no farther than
/// the fade's start; strongest with the eye at the deck (`height` 0) and the ray level. Returns the
/// distance and the weight; the shader's `dwell_cloud_band` is this.
pub fn deck_band(t: &CloudTunables, height: f64, above: f64) -> (f64, f64) {
    let level = above.abs();
    let inside = 1.0 - smoothstep(0.0, t.deck.cross_fade_m, height.abs());
    let d = (t.deck.cross_fade_m / level.max(1e-4)).min(t.deck.fade_start_m);
    (d, inside * (1.0 - smoothstep(0.0, BAND, level)))
}

/// Whether the deck, met `t` metres along a unit ray, is in front of what the scene drew there: the
/// view ray scaled to view depth 1 is `ray_len` long, so the hit is at view depth `t / ray_len`;
/// `scene_depth` is the opaque scene's view depth (0: nothing, the sky). `rise` is how far that
/// ray (at view depth 1) climbs, face-local, and `height` the deck's height above the eye
/// (negative: above it). Seen from above, the deck is also in front of any scene point below its
/// altitude: the ray crossed the deck to reach it. The world is flat and the deck a sphere, which
/// sinks below sea level √(2·r·altitude) out, so its hit alone would put the far sea in front of
/// it (owner, 2026-10-21). The composite's `dwell_cloud_in_front` is this.
pub fn cloud_in_front(t: f64, ray_len: f64, scene_depth: f64, rise: f64, height: f64) -> bool {
    scene_depth <= 0.0 || t / ray_len < scene_depth || (height < 0.0 && rise * scene_depth < height)
}

/// The deck's height above the eye: the eye's height above sea level on its own face.
pub fn deck_height(t: &CloudTunables, eye_y: f64, face: FaceFrame) -> f64 {
    t.deck.altitude_m - (face.y(eye_y) - dwell_protocol::world::SEA_LEVEL_M)
}

fn heading(deg: f64) -> [f64; 2] {
    let r = deg.to_radians();
    [r.cos(), r.sin()]
}

/// The clouds' per-frame uniforms: `offsets` (the noise point under the eye, and the warp field's
/// offset from it, in cells, each wrapped into one period), `cfg` (coverage, octaves, 1 with the
/// self-shadow, the deck's height above the eye: negative above it) and `inside` (x: how much the
/// eye is in cloud — the cloud's density under the eye, faded in over `cross_fade_m` either side of
/// the deck, times `inside_fog`, 0 when the volume is drawn: its march is the fog; y, z the eye's
/// world x and z modulo `wrap_m` (the plumes' place); w the volume's steps plus its light samples
/// over 8, 0 for the 2D deck). `seconds` is the
/// clouds' clock (world time where there is one). Face B's clouds are another part of the noise
/// (half a period away).
pub fn cloud_uniforms(
    t: &CloudTunables,
    eye: [f64; 3],
    face: FaceFrame,
    seconds: f64,
    quality: CloudQuality,
    coverage: f64,
) -> [[f32; 4]; 3] {
    let wrap = t.shape.wrap_m;
    let [wx, wz] = heading(t.wind.direction_deg).map(|c| c * t.wind.speed_mps * seconds);
    let [ex, ez] = heading(t.wind.evolve_direction_deg).map(|c| c * t.wind.evolve_mps * seconds);
    let side = if face.is_b() { wrap / 2.0 } else { 0.0 };
    let cells = |m: f64| m.rem_euclid(wrap) / t.shape.scale_m;
    let under = [cells(eye[0] - wx + side), cells(eye[2] - wz + side)];
    let warp = [cells(wx - ex), cells(wz - ez)];
    let height = deck_height(t, eye[1], face);
    let coverage = coverage.clamp(0.0, 1.0);
    let octaves = quality.octaves(t);
    let near = 1.0 - smoothstep(0.0, t.deck.cross_fade_m, height.abs());
    let inside = if near > 0.0 {
        near * t.deck.inside_fog * t.shape.opacity * density(t, under, warp, octaves, coverage)
    } else {
        0.0
    };
    let vol = quality.volume(t);
    let cells = |m: f64| cells(m) as f32;
    [
        [
            cells(eye[0] - wx + side),
            cells(eye[2] - wz + side),
            cells(wx - ex),
            cells(wz - ez),
        ],
        [
            coverage.clamp(0.0, 1.0) as f32,
            quality.octaves(t) as f32,
            if quality.self_shadow() { 1.0 } else { 0.0 },
            height as f32,
        ],
        [
            if vol.steps > 0 { 0.0 } else { inside as f32 },
            eye[0].rem_euclid(wrap) as f32,
            eye[2].rem_euclid(wrap) as f32,
            vol.steps as f32 + vol.light_steps as f32 / 8.0,
        ],
    ]
}

// ----- The noise, on the CPU (the shader's twin, for tests and tools) -----

/// The lattice's period in cells.
pub fn period(t: &CloudTunables) -> i64 {
    (t.shape.wrap_m / t.shape.scale_m).round() as i64
}

fn hash(x: i64, y: i64, period: i64) -> f64 {
    let (x, y) = (x.rem_euclid(period) as u32, y.rem_euclid(period) as u32);
    let mut h = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841);
    h = (h ^ (h >> 16)).wrapping_mul(0x7feb_352d);
    h = (h ^ (h >> 15)).wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    f64::from(h) / 4_294_967_295.0
}

fn value(p: [f64; 2], period: i64) -> f64 {
    let (i, j) = (p[0].floor(), p[1].floor());
    let f = [p[0] - i, p[1] - j];
    let u = f.map(|f| f * f * f * (f * (f * 6.0 - 15.0) + 10.0));
    let (i, j) = (i as i64, j as i64);
    let a = hash(i, j, period);
    let b = hash(i + 1, j, period);
    let c = hash(i, j + 1, period);
    let d = hash(i + 1, j + 1, period);
    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
    lerp(lerp(a, b, u[0]), lerp(c, d, u[0]), u[1])
}

fn next_octave(p: [f64; 2]) -> [f64; 2] {
    [2.0 * p[0] + p[1] + 13.0, -p[0] + 2.0 * p[1] + 7.0]
}

fn fbm(t: &CloudTunables, mut p: [f64; 2], octaves: u32) -> f64 {
    let n = period(t);
    let (mut amp, mut sum, mut norm) = (0.5, 0.0, 0.0);
    for _ in 0..octaves {
        sum += amp * value(p, n);
        norm += amp;
        amp *= t.shape.gain;
        p = next_octave(p);
    }
    sum / norm
}

/// The warped point: `p` moved by two two-octave fields sampled `warp_off` cells from it.
fn warped(t: &CloudTunables, p: [f64; 2], warp_off: [f64; 2]) -> [f64; 2] {
    let w = [p[0] + warp_off[0], p[1] + warp_off[1]];
    let qx = fbm(t, w, 2) - 0.5;
    let qy = fbm(t, [w[0] + 5.2, w[1] + 1.3], 2) - 0.5;
    [
        p[0] + 2.0 * t.shape.warp * qx,
        p[1] + 2.0 * t.shape.warp * qy,
    ]
}

fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn shape(t: &CloudTunables, n: f64, coverage: f64) -> f64 {
    let n = ((n - 0.5) * CONTRAST + 0.5).clamp(0.0, 1.0);
    let cut = 1.0 - coverage;
    if coverage <= 0.0 {
        return 0.0;
    }
    smoothstep(cut, cut + t.shape.edge_softness, n)
}

/// The cloud's density (0–1) at the noise point `p` (cells; `cloud_uniforms`' offsets plus the
/// hit's horizontal offset over `scale_m`).
pub fn density(
    t: &CloudTunables,
    p: [f64; 2],
    warp_off: [f64; 2],
    octaves: u32,
    coverage: f64,
) -> f64 {
    if octaves == 0 {
        return 0.0;
    }
    shape(t, fbm(t, warped(t, p, warp_off), octaves), coverage)
}

// ----- The volume's lighting, on the CPU (the shader's twin, docs/CLOUDS.md §8.8) -----

/// The deck's fBm stretched about its mean (the volume's weather noise, `cloud_vol_n`).
pub fn volume_stretch(n: f64) -> f64 {
    ((n - 0.5) * CONTRAST + 0.5).clamp(0.0, 1.0)
}

// ----- The shape model (docs/CLOUDS.md §9), on the CPU: the shader's twin -----

/// `x` mapped from `lo..hi` onto `a..b` (Schneider's remap; not clamped).
pub fn remap(x: f64, lo: f64, hi: f64, a: f64, b: f64) -> f64 {
    a + (x - lo) * (b - a) / (hi - lo)
}

/// A column's cloud-type profile: the fade-in and fade-out spans (shares of the layer's depth),
/// the anvil, density and rain darkening, mixed between the two types either side of `k` along
/// the type axis (`cloud_type_profile`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Profile {
    pub base: [f64; 2],
    pub top: [f64; 2],
    pub anvil: f64,
    pub density: f64,
    pub darkening: f64,
}

fn profile_of(c: &CloudType) -> Profile {
    Profile {
        base: c.base,
        top: c.top,
        anvil: c.anvil,
        density: c.density,
        darkening: c.darkening,
    }
}

/// The profile at `k` along the type axis: the types' own at their `at`, mixed linearly between
/// (so stratus thickens into stratocumulus and on), clamped to the first and last.
pub fn type_profile(types: &[CloudType], k: f64) -> Profile {
    let first = &types[0];
    if k <= first.at {
        return profile_of(first);
    }
    for w in types.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if k <= b.at {
            let f = (k - a.at) / (b.at - a.at);
            let m = |x: f64, y: f64| x + (y - x) * f;
            return Profile {
                base: [m(a.base[0], b.base[0]), m(a.base[1], b.base[1])],
                top: [m(a.top[0], b.top[0]), m(a.top[1], b.top[1])],
                anvil: m(a.anvil, b.anvil),
                density: m(a.density, b.density),
                darkening: m(a.darkening, b.darkening),
            };
        }
    }
    profile_of(types.last().unwrap())
}

/// The profile's height gradient `y` up the layer (0 floor, 1 top): faded in over its base span
/// (a short one: a flat base) and out over its top span (a long one: the base noise's weaker parts
/// end lower, so the top is rounded). `cloud_height_gradient`.
pub fn height_gradient(p: &Profile, y: f64) -> f64 {
    if y <= 0.0 || y >= 1.0 {
        return 0.0;
    }
    smoothstep(p.base[0], p.base[1], y) * (1.0 - smoothstep(p.top[0], p.top[1], y))
}

/// The weather map at a column (§9.2): `cover` (0–1), `kind` (along the type axis) and the rain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weather {
    pub cover: f64,
    pub kind: f64,
    pub precipitation: f64,
}

/// The weather at a column of stretched deck noise `n` ([`volume_stretch`]) and second noise `v`
/// (0–1) under the global `coverage`: cover the deck's own cut, softened; the type rises with how
/// far the column is past the cut (a cloud's core towers over its edges) and varies with `v`.
/// `cloud_weather`.
pub fn weather(t: &CloudTunables, n: f64, v: f64, coverage: f64) -> Weather {
    let w = &t.weather;
    let cut = 1.0 - coverage;
    let cover = if coverage <= 0.0 {
        0.0
    } else {
        smoothstep(cut, cut + t.form.coverage_softness, n)
    };
    Weather {
        cover,
        kind: (w.r#type + w.type_from_coverage * (n - cut) + w.type_variation * (2.0 * v - 1.0))
            .clamp(0.0, 1.0),
        precipitation: w.precipitation,
    }
}

/// The cover at `y` up the layer with the profile's anvil: from its top fade's start upward the
/// cover is raised (`cover^(1 − ½ anvil)`), so a cumulonimbus's top spreads wider than its trunk.
pub fn anvil_cover(p: &Profile, y: f64, cover: f64) -> f64 {
    if p.anvil <= 0.0 || cover <= 0.0 {
        return cover;
    }
    let k = smoothstep(p.top[0] - 0.15, p.top[0], y);
    cover.powf(1.0 - 0.5 * p.anvil * k)
}

/// The base shape's density (§9.3) from the base texture's sample `b` (RGBA: Perlin–Worley, Worley
/// fBm ×3) `y` up the layer: the Perlin–Worley dilated by the Worley fBm
/// (`remap(R, −(1 − fbm), 1, 0, 1)`), times the profile's gradient, cut by the column's cover
/// (`remap(·, 1 − cover, 1, 0, 1) × cover`), so no cover is no cloud. `cloud_base_shape`.
pub fn base_shape(b: [f64; 4], y: f64, w: &Weather, p: &Profile) -> f64 {
    let g = height_gradient(p, y);
    let cover = anvil_cover(p, y, w.cover);
    if g <= 0.0 || cover <= 0.0 {
        return 0.0;
    }
    let fbm = 0.625 * b[1] + 0.25 * b[2] + 0.125 * b[3];
    let base = remap(b[0], fbm - 1.0, 1.0, 0.0, 1.0).clamp(0.0, 1.0);
    (remap(base * g, 1.0 - cover, 1.0, 0.0, 1.0).max(0.0) * cover).min(1.0)
}

/// The detail's erosion of base density `d` (§9.3) by the detail fBm `hf` (0–1, its inverted
/// Worley: round billows) `y` up the layer: below `wisp_height` the fBm itself takes away (wispy
/// bases), above it its inverse (the gaps between billows: billowy tops); by `detail_erosion` at
/// most and only where `d` is under 1, so the core stays. `cloud_erode`.
pub fn erode(t: &CloudTunables, d: f64, hf: f64, y: f64) -> f64 {
    if d <= 0.0 {
        return 0.0;
    }
    let m = hf + (1.0 - 2.0 * hf) * smoothstep(0.0, t.form.wisp_height, y);
    remap(d, m * t.form.detail_erosion, 1.0, 0.0, 1.0).clamp(0.0, 1.0)
}

/// The density's scale for the profile and the rain at `y` (thicker, darker rain bases).
pub fn density_scale(p: &Profile, w: &Weather, y: f64) -> f64 {
    p.density * (1.0 + w.precipitation * p.darkening * (1.0 - y))
}

/// The shape point (m, face-local: x, height, z) of a point `x, z` metres across (in the noise's
/// wrapped frame) at height `h`: leaned downwind by `form.shear` per metre over the layer's floor.
/// `cloud_shape_point`.
pub fn shape_point(t: &CloudTunables, x: f64, h: f64, z: f64) -> [f64; 3] {
    let [dx, dz] = heading(t.wind.direction_deg);
    let lean = t.form.shear * (h - t.volume.base_m);
    [x + dx * lean, h, z + dz * lean]
}

/// The baked noise (`crate::cloud_noise`) and the sampling the shader does with it.
pub struct ShapeNoise {
    pub base: crate::cloud_noise::Noise3,
    pub detail: crate::cloud_noise::Noise3,
}

impl ShapeNoise {
    /// The textures at the data's sizes (or `size_override`'s smaller ones, for quick tests).
    pub fn bake(t: &CloudTunables, size_override: Option<(u32, u32)>) -> Self {
        let (b, d) = size_override.unwrap_or((t.form.base_size, t.form.detail_size));
        Self {
            base: crate::cloud_noise::bake_base(b),
            detail: crate::cloud_noise::bake_detail(d),
        }
    }

    /// The base texture's sample at shape point `pm`.
    pub fn base_at(&self, t: &CloudTunables, pm: [f64; 3]) -> [f64; 4] {
        self.base.sample(pm.map(|c| c / t.form.base_tile_m))
    }

    /// The detail fBm at shape point `pm`, `y` up the layer, with the detail's drift `drift` (m
    /// across): its point moved first by the curl (most at the floor). `cloud_detail_fbm`.
    pub fn detail_at(&self, t: &CloudTunables, pm: [f64; 3], y: f64, drift: [f64; 2]) -> f64 {
        let c = self.detail.sample(pm.map(|c| c / t.form.curl_tile_m));
        let k = t.form.curl_m * (1.0 - y.clamp(0.0, 1.0));
        let p = [
            pm[0] + (2.0 * c[1] - 1.0) * k + drift[0],
            pm[1],
            pm[2] + (2.0 * c[2] - 1.0) * k + drift[1],
        ];
        self.detail.sample(p.map(|c| c / t.form.detail_tile_m))[0]
    }

    /// The whole density at shape point `pm` (`y` up the layer) under weather `w`: the base shape
    /// eroded by the detail, scaled by the profile and the rain. `with_detail` false: the light
    /// samples' cheap base shape alone.
    pub fn density(
        &self,
        t: &CloudTunables,
        pm: [f64; 3],
        y: f64,
        w: &Weather,
        drift: [f64; 2],
        with_detail: bool,
    ) -> f64 {
        let p = type_profile(t.cloud_types, w.kind);
        let d = base_shape(self.base_at(t, pm), y, w, &p);
        if d <= 0.0 {
            return 0.0;
        }
        let d = if with_detail {
            erode(t, d, self.detail_at(t, pm, y, drift), y)
        } else {
            d
        };
        d * density_scale(&p, w, y)
    }
}

/// The cloud above and below a sample `y` up a column of cover `cover` whose profile ends at
/// `p.top[1]`, in metres of density 1 (half its cover on average): what dims the sky's light and
/// the ground's bounce (read from the profile at no cost: no lookups).
pub fn column_cover_m(t: &CloudTunables, p: &Profile, y: f64, cover: f64) -> (f64, f64) {
    let h = t.volume.top_m - t.volume.base_m;
    (
        0.5 * cover * (p.top[1] - y).max(0.0) * h,
        0.5 * cover * y.clamp(0.0, p.top[1]) * h,
    )
}

/// The light samples toward the sun or moon: each one's midpoint along the way and its length
/// (m), the first `light_step_m` long and each next `light_step_growth` times the last.
pub fn light_samples(t: &CloudTunables, steps: u32) -> Vec<(f64, f64)> {
    let (mut at, mut len) = (0.0, t.volume.light_step_m);
    (0..steps.min(MAX_LIGHT_STEPS))
        .map(|_| {
            let s = (at + 0.5 * len, len);
            at += len;
            len *= t.volume.light_step_growth;
            s
        })
        .collect()
}

/// The optical depth toward the light (m of density 1) from a sample `y` up the layer of density
/// `d`, with the light's face-local up part `light_rise`: half a light step of its own, then the
/// tier's `steps` light samples, each the base shape's density `toward(s)` metres toward the light
/// (no detail: the cheap shape), stopping where the path leaves the layer. With no samples, the
/// cloud above (`above_m`, [`column_cover_m`]) along the light's slant stands for the path.
/// `cloud_layer_march`'s light march is the twin.
pub fn light_depth(
    t: &CloudTunables,
    steps: u32,
    y: f64,
    d: f64,
    above_m: f64,
    light_rise: f64,
    toward: impl Fn(f64) -> f64,
) -> f64 {
    let h = t.volume.top_m - t.volume.base_m;
    let mut od = 0.5 * t.volume.light_step_m * d;
    if steps == 0 {
        return od
            + if light_rise > 0.0 {
                above_m / light_rise.max(0.3)
            } else {
                h
            };
    }
    for (mid, len) in light_samples(t, steps) {
        let yl = y + light_rise * mid / h;
        if yl <= 0.0 || yl >= 1.0 {
            break;
        }
        od += len * toward(mid);
    }
    od
}

/// Henyey–Greenstein's phase for the cosine `c` between the view ray and the light's direction,
/// scaled so that even scattering is 1.
pub fn henyey_greenstein(c: f64, g: f64) -> f64 {
    let k = 1.0 + g * g - 2.0 * g * c;
    (1.0 - g * g) / (k * k.sqrt())
}

/// The clouds' phase (`cloud_phase`): the forward and back lobes mixed, then toward even by
/// `phase_strength`. Greatest looking toward the light (the silver lining).
pub fn cloud_phase(v: &VolumeTunables, c: f64) -> f64 {
    let two = henyey_greenstein(c, v.phase_forward) * (1.0 - v.phase_back_weight)
        + henyey_greenstein(c, v.phase_back) * v.phase_back_weight;
    1.0 + (two - 1.0) * v.phase_strength
}

/// A sample's light: the sun's (or moon's) weight through `od` (m of density 1) toward it, seen at
/// `c` (the view ray's cosine with the light's direction), and the sky's and the ground's ambient
/// weights with `above_m`, `below_m` of cloud over and under it ([`volume_cover_m`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SampleLight {
    pub sun: f64,
    pub sky: f64,
    pub bounce: f64,
}

/// `cloud_vol_light`'s weights: Beer toward the light with the multiple-scattering floor, the
/// powder term (strongest looking away from the light), the phase; the ambient from above dimmed
/// by the cloud above, the bounce from below by the cloud below.
pub fn sample_light(t: &CloudTunables, od: f64, c: f64, above_m: f64, below_m: f64) -> SampleLight {
    let v = &t.volume;
    let tau = t.light.absorption * v.extinction * od;
    let beer = (-tau).exp().max(v.multi_scatter * (-0.25 * tau).exp());
    let powder = 1.0 - v.powder * (0.5 - 0.5 * c) * (-2.0 * tau).exp();
    SampleLight {
        sun: v.sun_strength * beer * powder * cloud_phase(v, c),
        sky: v.sky_ambient * (-above_m / v.ambient_depth_m).exp(),
        bounce: v.ground_bounce * (-below_m / v.ambient_depth_m).exp(),
    }
}

/// A sample's colour from its light and the frame's lit and shade colours (`cloud_shading`'s):
/// the shade colour from the sky above and, tinted by `bounce_tint`, from the ground below, plus
/// the lit colour by the light's weight.
pub fn sample_colour(t: &CloudTunables, l: SampleLight, lit: Rgb, shade: Rgb) -> Rgb {
    let tint = t.volume.bounce_tint;
    [0, 1, 2].map(|i| shade[i] * (l.sky + l.bounce * tint[i]) + lit[i] * l.sun)
}

/// The march's colour with its brightest parts (a silver lining's) rolled off toward a little over
/// white rather than clipped: `dwell_cloud_march`'s last step.
pub fn soft_clip(c: Rgb) -> Rgb {
    let k = (c[0].max(c[1]).max(c[2]) - 1.0).max(0.0);
    let s = (1.0 + 0.35 * k / (1.0 + k)) / (1.0 + k);
    c.map(|x| x * s)
}

// ----- The march against the scene (BUGS.md #88, docs/CLOUDS.md §8.9) -----

/// The clouds pass's per-pixel step offset (`cloud_jitter` in the client's shader): the blue-noise
/// tile's value at pixel `p` ([`crate::cloud_noise::blue_noise`], the detail texture's alpha: `tile`
/// of `size`²), turned each frame by a golden-ratio step over 16 frames (BUGS.md #90: before, an
/// interleaved gradient noise, whose clumps showed as dither where the history was dropped).
pub fn cloud_jitter(tile: &[u8], size: u32, p: [i32; 2], frame: u32) -> f64 {
    let fract = |x: f64| x - x.floor();
    let m = size as i32 - 1;
    let blue = f64::from(tile[((p[1] & m) * size as i32 + (p[0] & m)) as usize]) / 255.0;
    fract(blue + f64::from(frame % 16) * 0.618034)
}

/// The span (t0, t1) of a ray in the layer, out to `max_t` and the march's reach (m along the ray;
/// `rise` the ray's face-local up part, `eye_h` the eye's height above sea level): the layer
/// march's (`cloud_layer_march`); `None` where it never crosses the layer inside the reach.
pub fn layer_span(v: &VolumeTunables, rise: f64, eye_h: f64, max_t: f64) -> Option<(f64, f64)> {
    let mut t0: f64 = 0.0;
    let mut t1 = max_t.min(v.max_distance_m);
    if rise.abs() > 1e-5 {
        let a = (v.base_m - eye_h) / rise;
        let b = (v.top_m - eye_h) / rise;
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
    } else if eye_h < v.base_m || eye_h > v.top_m {
        return None;
    }
    (t1 > t0).then_some((t0, t1))
}

/// The distances along the ray of a march's `steps` samples over `span` offset by `jitter`
/// (`cloud_layer_march`'s `t`).
pub fn march_samples(span: (f64, f64), jitter: f64, steps: u32) -> Vec<f64> {
    let dt = (span.1 - span.0) / f64::from(steps);
    (0..steps)
        .map(|i| span.0 + (f64::from(i) + jitter) * dt)
        .collect()
}

/// The scene's distance (m along the ray) over one clouds-pass texel's footprint (its
/// full-resolution pixels): the nearest and the farthest, each at most `reach` (the march's; the
/// sky is `reach`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneRange {
    pub near: f64,
    pub far: f64,
}

/// [`SceneRange`] of a texel's pixels' scene distances (0: the sky): `clouds_fs`'s reduced depth.
pub fn scene_range(depths: &[f64], reach: f64) -> SceneRange {
    let at = |d: f64| if d > 0.0 { d.min(reach) } else { reach };
    let mut r = SceneRange {
        near: reach,
        far: 0.0,
    };
    for &d in depths {
        r.near = r.near.min(at(d));
        r.far = r.far.max(at(d));
    }
    r.far = r.far.max(r.near);
    r
}

/// How far the texel marches (m along its ray): to its farthest scene pixel, so none of its cloud
/// is behind all of its scene, or the whole layer where any of it is sky or past the reach.
pub fn march_end(r: SceneRange, reach: f64) -> f64 {
    if r.far >= reach { f64::INFINITY } else { r.far }
}

/// What the clouds pass keeps beside the colour for the composite (its second target): the texel's
/// scene depth range, which doesn't move with the jitter.
pub fn texel_depth(r: SceneRange) -> [f64; 2] {
    [r.near, r.far]
}

/// The weight of a texel (its `texel_depth`) in the composite's upsample at a pixel whose scene is
/// `e` m along its ray (`reach` for the sky): 1 where the texel's cloud is the pixel's (its march ended
/// at the pixel's scene, within a band of the texel's depth range, at least `depth_band` and at most
/// `depth_band_range` of the distance), falling to 0 at twice the band.
pub fn upsample_weight(v: &VolumeTunables, e: f64, rec: [f64; 2]) -> f64 {
    let band = upsample_band(v, e, rec);
    1.0 - smoothstep(band, 2.0 * band, (rec[1] - e).abs())
}

/// [`upsample_weight`]'s band (m) at a pixel `e` m out for a texel's `texel_depth`.
pub fn upsample_band(v: &VolumeTunables, e: f64, rec: [f64; 2]) -> f64 {
    (rec[1] - rec[0])
        .min(v.depth_band_range * e)
        .max(v.depth_band * e)
        .max(1.0)
}

/// The composite's cloud cover at a pixel whose scene is `e` m along its ray (`reach`: the sky)
/// from the four nearest texels, each (its cover, its `texel_depth`, its bilinear weight):
/// `upsampled_clouds`'s twin.
/// Where no texel's march ended at the pixel's scene (a sub-texel spire), the nearest match leads
/// by `UPSAMPLE_FALLBACK`'s share: still a function of depths alone, so it doesn't flicker.
pub fn upsample_cover(v: &VolumeTunables, e: f64, texels: &[(f64, [f64; 2], f64)]) -> f64 {
    let (mut sum, mut weight) = (0.0, 0.0);
    for &(cover, rec, bilinear) in texels {
        let band = upsample_band(v, e, rec);
        let w = bilinear
            * (upsample_weight(v, e, rec) + UPSAMPLE_FALLBACK / (1.0 + (rec[1] - e).abs() / band));
        sum += w * cover;
        weight += w;
    }
    if weight > 0.0 { sum / weight } else { 0.0 }
}

/// The clouds' resolve (BUGS.md #90, `clouds_resolve_fs`): the box a texel's history is clamped to,
/// from this frame's march at it and its 3 × 3 neighbours whose scene matched its own (`samples`,
/// premultiplied colour and cover): each channel's mean ± `gamma` standard deviations, widened to
/// take the texel's own sample.
pub fn neighbourhood_box(own: [f64; 4], samples: &[[f64; 4]], gamma: f64) -> ([f64; 4], [f64; 4]) {
    let n = samples.len().max(1) as f64;
    let mut lo = [0.0; 4];
    let mut hi = [0.0; 4];
    for c in 0..4 {
        let m1 = samples.iter().map(|s| s[c]).sum::<f64>() / n;
        let m2 = samples.iter().map(|s| s[c] * s[c]).sum::<f64>() / n;
        let sd = (m2 - m1 * m1).max(0.0).sqrt();
        lo[c] = (m1 - gamma * sd).min(own[c]);
        hi[c] = (m1 + gamma * sd).max(own[c]);
    }
    (lo, hi)
}

/// The resolved texel: the history (`matched`: how much of it is this texel's, 0–1) clamped into
/// this frame's neighbourhood `bx` ([`neighbourhood_box`]) and blended with the new march `cur` by
/// the tier's `blend`. Clamped, not dropped, as the eye moves through the layer: before, a moving
/// eye near the clouds handed each texel to its raw jittered march (the dither of BUGS.md #90).
pub fn resolve(
    cur: [f64; 4],
    bx: ([f64; 4], [f64; 4]),
    hist: [f64; 4],
    matched: f64,
    blend: f64,
) -> [f64; 4] {
    let w = 1.0 + (blend - 1.0) * matched;
    std::array::from_fn(|c| {
        let h = hist[c].clamp(bx.0[c], bx.1[c]);
        h + (cur[c] - h) * w
    })
}

/// The most a tier's `divisor` may be (`clouds.mjs`'s bound): the clouds pass reads at most its
/// square of scene pixels per texel (`cloud_scene_span`'s loop bound).
pub const MAX_DIVISOR: u32 = 4;

/// The composite's floor weight for a texel by how near its march's end is to the pixel's scene
/// (`upsample_cover`), small enough to vanish beside any texel within the band.
pub const UPSAMPLE_FALLBACK: f64 = 1e-4;

// ----- The shader -----

fn f(n: f64) -> String {
    format!("{n:.6}")
}
fn vec3(c: Rgb) -> String {
    format!("vec3f({}, {}, {})", f(c[0]), f(c[1]), f(c[2]))
}

/// The clouds for shaders, generated from `t`:
///
/// - `fn dwell_cloud_deck(dir: vec3f, face: f32, offsets: vec4f, height: f32) -> vec4f`: where
///   the view ray `dir` meets the deck: the noise point (xy, cells), the distance (z) and how much
///   of the cloud there shows (w: 0 below the horizon, past the fade, or with the eye at the deck;
///   negative for the edge-on band within the layer, which takes the coverage as its density).
///   Call it in uniform control flow and take `fwidth` of its xy (the pixel's footprint in cells).
/// - `fn dwell_clouds(under: vec3f, clear: vec3f, dir: vec3f, face: f32, deck: vec4f, fw: f32, offsets: vec4f,
///   cfg: vec4f, lights: CloudLighting) -> vec3f`, with `CloudLighting` the sky frame and the
///   lights' colours from the renderer's frame (the data's colours are tints of those): `under`
///   (the sky, or the scene where the deck is in front of it) with the clouds over it, far cloud
///   hazing toward the clear sky `clear` (sRGB, from `dwell_sky`).
/// - `fn dwell_cloud_layer(clear, dir, face, deck, fw, offsets, cfg, lights) -> vec4f` and
///   `fn dwell_cloud_over(under: vec3f, layer: vec4f) -> vec3f`: the same clouds found once as a
///   layer (colour, cover) and put over any colour behind them (`dwell_clouds` is the two).
///
/// `offsets` and `cfg` are [`cloud_uniforms`]'s.
pub fn cloud_wgsl(t: &CloudTunables) -> String {
    let d = &t.deck;
    let s = &t.shape;
    let l = &t.light;
    let consts = [
        format!("const CLOUD_PERIOD: i32 = {};", period(t)),
        format!("const CLOUD_MAX_OCTAVES: i32 = {MAX_OCTAVES};"),
        format!("const CLOUD_SCALE: f32 = {};", f(s.scale_m)),
        format!("const CLOUD_CURVATURE: f32 = {};", f(d.curvature_m)),
        format!("const CLOUD_ALTITUDE: f32 = {};", f(d.altitude_m)),
        format!("const CLOUD_CROSS_FADE: f32 = {};", f(d.cross_fade_m)),
        format!("const CLOUD_BAND: f32 = {};", f(BAND)),
        format!("const CLOUD_TOP_LIGHT: f32 = {};", f(l.top_light)),
        format!("const CLOUD_HORIZON_FADE: f32 = {};", f(d.horizon_fade)),
        format!("const CLOUD_FADE_START: f32 = {};", f(d.fade_start_m)),
        format!("const CLOUD_FADE_END: f32 = {};", f(d.fade_end_m)),
        format!("const CLOUD_GAIN: f32 = {};", f(s.gain)),
        format!("const CLOUD_WARP: f32 = {};", f(2.0 * s.warp)),
        format!("const CLOUD_LACUNARITY: f32 = {};", f(LACUNARITY)),
        format!("const CLOUD_CONTRAST: f32 = {};", f(CONTRAST)),
        format!("const CLOUD_SOFTNESS: f32 = {};", f(s.edge_softness)),
        format!("const CLOUD_OPACITY: f32 = {};", f(s.opacity)),
        format!("const CLOUD_SUNLIT: vec3f = {};", vec3(l.sunlit)),
        format!("const CLOUD_SHADE: vec3f = {};", vec3(l.shade)),
        format!("const CLOUD_SUNSET: vec3f = {};", vec3(l.sunset)),
        format!("const CLOUD_NIGHT_LIT: vec3f = {};", vec3(l.night_lit)),
        format!("const CLOUD_NIGHT_SHADE: vec3f = {};", vec3(l.night_shade)),
        format!(
            "const CLOUD_SUN_GLOW: vec3f = {};",
            vec3(DAY_NIGHT.brightest().glow)
        ),
        format!(
            "const CLOUD_MOON_GLOW: vec3f = {};",
            vec3(DAY_NIGHT.moon.glow_color)
        ),
        format!("const CLOUD_ABSORPTION: f32 = {};", f(l.absorption)),
        format!(
            "const CLOUD_SHADOW_STEP: f32 = {};",
            f(l.shadow_step_m / s.scale_m)
        ),
        format!("const CLOUD_SILVER: f32 = {};", f(l.silver_lining)),
        format!("const CLOUD_SILVER_POWER: f32 = {};", f(l.silver_power)),
        format!("const CLOUD_SUN_WARMTH: f32 = {};", f(l.sun_warmth)),
        format!("const CLOUD_TWILIGHT: f32 = {};", f(TWILIGHT)),
    ];
    let v = &t.volume;
    let fm = &t.form;
    let w = &t.weather;
    let [sx, sz] = heading(t.wind.direction_deg).map(|c| c * fm.shear);
    let volume = [
        format!("const CLOUD_MAX_STEPS: i32 = {MAX_STEPS};"),
        format!("const CLOUD_MAX_LIGHT_STEPS: i32 = {MAX_LIGHT_STEPS};"),
        format!("const CLOUD_WRAP: f32 = {};", f(s.wrap_m)),
        format!("const CLOUD_VOL_BASE: f32 = {};", f(v.base_m)),
        format!("const CLOUD_VOL_TOP: f32 = {};", f(v.top_m)),
        format!("const CLOUD_VOL_PERIOD: i32 = {PLUME_NOISE_PERIOD};"),
        format!("const CLOUD_BASE_TILE: f32 = {};", f(fm.base_tile_m)),
        format!("const CLOUD_DETAIL_TILE: f32 = {};", f(fm.detail_tile_m)),
        format!("const CLOUD_CURL_TILE: f32 = {};", f(fm.curl_tile_m)),
        format!("const CLOUD_CURL_M: f32 = {};", f(fm.curl_m)),
        format!("const CLOUD_EROSION: f32 = {};", f(fm.detail_erosion)),
        format!("const CLOUD_WISP: f32 = {};", f(fm.wisp_height)),
        format!("const CLOUD_SHEAR: vec2f = vec2f({}, {});", f(sx), f(sz)),
        format!("const CLOUD_COVER_SOFT: f32 = {};", f(fm.coverage_softness)),
        format!("const CLOUD_TYPE_BASE: f32 = {};", f(w.r#type)),
        format!("const CLOUD_TYPE_VAR: f32 = {};", f(w.type_variation)),
        format!("const CLOUD_TYPE_COVER: f32 = {};", f(w.type_from_coverage)),
        format!("const CLOUD_PRECIP: f32 = {};", f(w.precipitation)),
        format!("const CLOUD_DETAIL_SIZE: i32 = {};", fm.detail_size),
        format!("const CLOUD_VOL_EXTINCTION: f32 = {};", f(v.extinction)),
        format!("const CLOUD_VOL_H: f32 = {};", f(v.top_m - v.base_m)),
        format!("const CLOUD_VOL_LIGHT_STEP: f32 = {};", f(v.light_step_m)),
        format!(
            "const CLOUD_VOL_LIGHT_GROWTH: f32 = {};",
            f(v.light_step_growth)
        ),
        format!("const CLOUD_VOL_MULTI: f32 = {};", f(v.multi_scatter)),
        format!("const CLOUD_VOL_POWDER: f32 = {};", f(v.powder)),
        format!("const CLOUD_VOL_PHASE_F: f32 = {};", f(v.phase_forward)),
        format!("const CLOUD_VOL_PHASE_B: f32 = {};", f(v.phase_back)),
        format!(
            "const CLOUD_VOL_PHASE_BW: f32 = {};",
            f(v.phase_back_weight)
        ),
        format!("const CLOUD_VOL_PHASE_S: f32 = {};", f(v.phase_strength)),
        format!("const CLOUD_VOL_SUN: f32 = {};", f(v.sun_strength)),
        format!("const CLOUD_VOL_SKY: f32 = {};", f(v.sky_ambient)),
        format!("const CLOUD_VOL_BOUNCE: f32 = {};", f(v.ground_bounce)),
        format!(
            "const CLOUD_VOL_BOUNCE_TINT: vec3f = {};",
            vec3(v.bounce_tint)
        ),
        format!(
            "const CLOUD_VOL_AMBIENT_DEPTH: f32 = {};",
            f(v.ambient_depth_m)
        ),
        format!(
            "const CLOUD_VOL_MAX_DISTANCE: f32 = {};",
            f(v.max_distance_m)
        ),
        format!("const CLOUD_VOL_DEPTH_BAND: f32 = {};", f(v.depth_band)),
        format!(
            "const CLOUD_VOL_DEPTH_BAND_RANGE: f32 = {};",
            f(v.depth_band_range)
        ),
        format!(
            "const CLOUD_UPSAMPLE_FALLBACK: f32 = {};",
            f(UPSAMPLE_FALLBACK)
        ),
        format!("const CLOUD_PLUME_BILLOW: f32 = {};", f(v.plume_billow)),
        format!("const CLOUD_MAX_PLUME_STEPS: i32 = {MAX_PLUME_STEPS};"),
        format!("const CLOUD_PLUME_SLOTS: u32 = {}u;", t.plume_slots),
    ];
    format!(
        "{}\n{CLOUD_FNS}\n{}\n{VOLUME_FNS}\n{}",
        consts.join("\n"),
        volume.join("\n"),
        type_profile_wgsl(t.cloud_types),
    )
}

/// The plume billows' value-noise period (cells; plumes' noise needn't tile with anything).
const PLUME_NOISE_PERIOD: i32 = 4096;

/// The most cloud types (`clouds.mjs`'s MAX_CLOUD_TYPES; the generated chain's length).
pub const MAX_CLOUD_TYPES: usize = 8;

fn profile_wgsl(c: &CloudType) -> String {
    format!(
        "CloudProfile(vec4f({}, {}, {}, {}), {}, {}, {})",
        f(c.base[0]),
        f(c.base[1]),
        f(c.top[0]),
        f(c.top[1]),
        f(c.anvil),
        f(c.density),
        f(c.darkening)
    )
}

/// `cloud_type_profile(k)` generated from the data's types ([`type_profile`]'s twin): each type's
/// profile at its `at`, mixed between neighbours, clamped to the first and last.
fn type_profile_wgsl(types: &[CloudType]) -> String {
    let mut s = String::from(
        "// The cloud type's profile at `k` along the type axis (generated from clouds.json's cloudTypes:\n\
         // type_profile's twin).\nfn cloud_type_profile(k: f32) -> CloudProfile {\n",
    );
    let first = &types[0];
    s += &format!(
        "\tif (k <= {}) {{\n\t\treturn {};\n\t}}\n",
        f(first.at),
        profile_wgsl(first)
    );
    for (i, w) in types.windows(2).enumerate() {
        let (a, b) = (&w[0], &w[1]);
        s += &format!(
            "\tif (k <= {}) {{\n\t\treturn cloud_profile_mix({}, {}, (k - {}) / {}); // {} → {} ({i})\n\t}}\n",
            f(b.at),
            profile_wgsl(a),
            profile_wgsl(b),
            f(a.at),
            f(b.at - a.at),
            a.name,
            b.name
        );
    }
    s += &format!("\treturn {};\n}}\n", profile_wgsl(types.last().unwrap()));
    s
}

/// The volume's march (`dwell_cloud_march`, the layer then the run-time plumes) with the plume
/// list's uniform (`CloudPlumes`, [`crate::plumes::PlumeList::pack`]) declared at `group`,
/// `binding`: for the module that runs the clouds pass (the client's, group 3), with
/// [`cloud_wgsl`]'s functions and constants beside it. Apart from [`cloud_wgsl`] so the modules
/// that only draw the deck need no plume binding.
///
/// The baked shape noise (`crate::cloud_noise`, §9.3) is declared at `noise`, `noise + 1` (the base
/// and detail `texture_3d<f32>`) and `noise + 2` (their repeating, filtering sampler).
pub fn cloud_march_wgsl(group: u32, binding: u32, noise: u32) -> String {
    format!(
        "{MARCH_FNS}\n@group({group}) @binding({binding}) var<uniform> cloud_plume_list: CloudPlumes;\n\
         @group({group}) @binding({noise}) var cloud_base_noise: texture_3d<f32>;\n\
         @group({group}) @binding({}) var cloud_detail_noise: texture_3d<f32>;\n\
         @group({group}) @binding({}) var cloud_noise_sampler: sampler;\n",
        noise + 1,
        noise + 2
    )
}

/// A plume's density (0 to its `density`) at a point `dx`, `dz` metres across from its axis and at
/// `u` of the way from its base to its top, before the billows: full within half its radius there,
/// fading to none at the radius, faded in above its base and out below its top. The shader's
/// `cloud_plume_density` is the twin.
pub fn plume_density(radius_m: f64, spread: f64, density: f64, dx: f64, dz: f64, u: f64) -> f64 {
    if !(0.0..=1.0).contains(&u) {
        return 0.0;
    }
    let r = radius_m * (1.0 + spread * u);
    let dist = (dx * dx + dz * dz).sqrt();
    density
        * (1.0 - smoothstep(0.5 * r, r, dist))
        * smoothstep(0.0, 0.08, u)
        * (1.0 - smoothstep(0.85, 1.0, u))
}

/// The span `[t0, t1]` along a ray from the eye (`dir` a unit vector, `rise` its face-local up part,
/// `eye_h` the eye's height above sea level) inside a plume's bounding column: `at` its axis's offset
/// from the eye (m, across), `r` its widest radius, `base`/`top` its heights, out to `reach`; None if the ray misses
/// it. The shader's `cloud_plume_span` is the twin.
pub fn plume_span(
    dir_xz: [f64; 2],
    rise: f64,
    eye_h: f64,
    at: [f64; 2],
    r: f64,
    [base, top]: [f64; 2],
    reach: f64,
) -> Option<(f64, f64)> {
    let (mut t0, mut t1) = (0.0_f64, reach);
    if rise.abs() > 1e-5 {
        let (a, b) = ((base - eye_h) / rise, (top - eye_h) / rise);
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
    } else if eye_h < base || eye_h > top {
        return None;
    }
    // Across: |dir_xz t - at|² = r².
    let aa = dir_xz[0] * dir_xz[0] + dir_xz[1] * dir_xz[1];
    let bb = dir_xz[0] * at[0] + dir_xz[1] * at[1];
    let cc = at[0] * at[0] + at[1] * at[1] - r * r;
    if aa < 1e-10 {
        if cc > 0.0 {
            return None;
        }
    } else {
        let disc = bb * bb - aa * cc;
        if disc <= 0.0 {
            return None;
        }
        let q = disc.sqrt();
        t0 = t0.max((bb - q) / aa);
        t1 = t1.min((bb + q) / aa);
    }
    (t1 > t0).then_some((t0, t1))
}

const VOLUME_FNS: &str = r"
fn cloud_hash3(c: vec3i) -> f32 {
	let x = ((c.x % CLOUD_VOL_PERIOD) + CLOUD_VOL_PERIOD) % CLOUD_VOL_PERIOD;
	let z = ((c.z % CLOUD_VOL_PERIOD) + CLOUD_VOL_PERIOD) % CLOUD_VOL_PERIOD;
	var h = (u32(x) * 0x8da6b343u) ^ (bitcast<u32>(c.y) * 0xd8163841u) ^ (u32(z) * 0xcb1ab31fu);
	h = (h ^ (h >> 16u)) * 0x7feb352du;
	h = (h ^ (h >> 15u)) * 0x846ca68bu;
	h ^= h >> 16u;
	return f32(h) / 4294967295.0;
}

// Trilinear value noise with a smooth (cubic) fade, periodic across.
fn cloud_value3(p: vec3f) -> f32 {
	let i = floor(p);
	let f = p - i;
	let u = f * f * (3.0 - 2.0 * f);
	let c = vec3i(i);
	let a = mix(cloud_hash3(c), cloud_hash3(c + vec3i(1, 0, 0)), u.x);
	let b = mix(cloud_hash3(c + vec3i(0, 1, 0)), cloud_hash3(c + vec3i(1, 1, 0)), u.x);
	let d = mix(cloud_hash3(c + vec3i(0, 0, 1)), cloud_hash3(c + vec3i(1, 0, 1)), u.x);
	let e = mix(cloud_hash3(c + vec3i(0, 1, 1)), cloud_hash3(c + vec3i(1, 1, 1)), u.x);
	return mix(mix(a, b, u.y), mix(d, e, u.y), u.z);
}

// The volume's weather noise at the warped point `wq` (cells): the deck's noise, two octaves (the
// march's budget), stretched about its mean (volume_stretch).
fn cloud_vol_n(wq: vec2f) -> f32 {
	return clamp((cloud_fbm(wq, 2, 0.0) - 0.5) * CLOUD_CONTRAST + 0.5, 0.0, 1.0);
}

// The one-octave warp's offset at `q` (cells): found once per view sample, its light samples reuse it.
fn cloud_vol_warp(q: vec2f, warp_off: vec2f) -> vec2f {
	let w0 = q + warp_off;
	return CLOUD_WARP * (vec2f(cloud_value(w0), cloud_value(w0 + vec2f(5.2, 1.3))) - 0.5);
}

// A column's cloud-type profile (docs/CLOUDS.md §9.2; Profile): fade = the base's fade-in span and
// the top's fade-out span (shares of the layer), then the anvil, density and rain darkening.
struct CloudProfile {
	fade: vec4f,
	anvil: f32,
	density: f32,
	darkening: f32,
};

fn cloud_profile_mix(a: CloudProfile, b: CloudProfile, f: f32) -> CloudProfile {
	return CloudProfile(mix(a.fade, b.fade, f), mix(a.anvil, b.anvil, f), mix(a.density, b.density, f), mix(a.darkening, b.darkening, f));
}

// The weather map's cover alone (the light samples'), from the stretched deck noise `n`.
fn cloud_cover(n: f32, coverage: f32) -> f32 {
	let cut = 1.0 - coverage;
	return select(smoothstep(cut, cut + CLOUD_COVER_SOFT, n), 0.0, coverage <= 0.0);
}

// The weather map at a column (weather): x its cover, y its type along the type axis, from the
// column's stretched deck noise `n`, the warped point `wq` (the type's second noise) and the coverage.
fn cloud_weather(n: f32, wq: vec2f, coverage: f32) -> vec2f {
	let cut = 1.0 - coverage;
	let cover = cloud_cover(n, coverage);
	let v = cloud_value(wq + vec2f(17.3, 41.9));
	let kind = clamp(CLOUD_TYPE_BASE + CLOUD_TYPE_COVER * (n - cut) + CLOUD_TYPE_VAR * (2.0 * v - 1.0), 0.0, 1.0);
	return vec2f(cover, kind);
}

// The profile's height gradient `y` up the layer (height_gradient): a short fade-in (flat bases), a
// long fade-out (rounded tops).
fn cloud_height_gradient(p: CloudProfile, y: f32) -> f32 {
	if (y <= 0.0 || y >= 1.0) {
		return 0.0;
	}
	return smoothstep(p.fade.x, p.fade.y, y) * (1.0 - smoothstep(p.fade.z, p.fade.w, y));
}

// The base shape (base_shape) from the base texture's sample `b`: the Perlin-Worley dilated by the
// Worley fBm, times the gradient `g`, cut by the cover (raised toward the top by the anvil).
fn cloud_base_shape(b: vec4f, y: f32, cover_in: f32, p: CloudProfile, g: f32) -> f32 {
	var cover = cover_in;
	if (p.anvil > 0.0 && cover > 0.0) {
		cover = pow(cover, 1.0 - 0.5 * p.anvil * smoothstep(p.fade.z - 0.15, p.fade.z, y));
	}
	if (g <= 0.0 || cover <= 0.0) {
		return 0.0;
	}
	let fbm = dot(b.yzw, vec3f(0.625, 0.25, 0.125));
	let base = clamp((b.x - (fbm - 1.0)) / (2.0 - fbm), 0.0, 1.0);
	return min(max((base * g - (1.0 - cover)) / max(cover, 1e-4), 0.0) * cover, 1.0);
}

// The detail's erosion (erode): the fBm below CLOUD_WISP (wispy bases), its inverse above (billowy
// tops); only where the base is under 1, so the core stays.
fn cloud_erode(d: f32, hf: f32, y: f32) -> f32 {
	let m = mix(hf, 1.0 - hf, smoothstep(0.0, CLOUD_WISP, y));
	let e = m * CLOUD_EROSION;
	return clamp((d - e) / (1.0 - e), 0.0, 1.0);
}

// The profile's and the rain's density scale `y` up the layer (density_scale).
fn cloud_density_scale(p: CloudProfile, y: f32) -> f32 {
	return p.density * (1.0 + CLOUD_PRECIP * p.darkening * (1.0 - y));
}

// The shape point (m: across, height, across) of noise point `q` (cells) at height `h`, leaned
// downwind (shape_point).
fn cloud_shape_point(q: vec2f, h: f32) -> vec3f {
	let a = q * CLOUD_SCALE + CLOUD_SHEAR * (h - CLOUD_VOL_BASE);
	return vec3f(a.x, h, a.y);
}

// Henyey-Greenstein's phase, scaled so even scattering is 1 (henyey_greenstein).
fn cloud_hg(c: f32, g: f32) -> f32 {
	let k = 1.0 + g * g - 2.0 * g * c;
	return (1.0 - g * g) / (k * sqrt(k));
}

// The dual-lobe phase (cloud_phase): brightest toward the light, the silver lining.
fn cloud_phase(c: f32) -> f32 {
	let two = mix(cloud_hg(c, CLOUD_VOL_PHASE_F), cloud_hg(c, CLOUD_VOL_PHASE_B), CLOUD_VOL_PHASE_BW);
	return mix(1.0, two, CLOUD_VOL_PHASE_S);
}
";

const MARCH_FNS: &str = r"
// The base texture at shape point `pm` (m): Perlin-Worley and three Worley fBm octaves.
fn cloud_base_at(pm: vec3f) -> vec4f {
	return textureSampleLevel(cloud_base_noise, cloud_noise_sampler, pm / CLOUD_BASE_TILE, 0.0);
}

// The detail fBm at shape point `pm`, `y` up the layer, drifted by `drift` (m): its point moved
// first by the curl, most at the floor (ShapeNoise::detail_at).
fn cloud_detail_fbm(pm: vec3f, y: f32, drift: vec2f) -> f32 {
	let c = textureSampleLevel(cloud_detail_noise, cloud_noise_sampler, pm / CLOUD_CURL_TILE, 0.0).yz * 2.0 - 1.0;
	let o = c * CLOUD_CURL_M * (1.0 - clamp(y, 0.0, 1.0)) + drift;
	return textureSampleLevel(cloud_detail_noise, cloud_noise_sampler, (pm + vec3f(o.x, 0.0, o.y)) / CLOUD_DETAIL_TILE, 0.0).x;
}

// The blue-noise tile (the detail texture's alpha, the same in every slice) at pixel `p`: the
// march's per-pixel step offset before the frame's turn (cloud_jitter; BUGS.md #90).
fn dwell_cloud_blue(p: vec2i) -> f32 {
	return textureLoad(cloud_detail_noise, vec3i(p & vec2i(CLOUD_DETAIL_SIZE - 1), 0), 0).w;
}

// The march's layer (colour not yet hazed) and the distance (m) along the ray to the first cloud
// it met (-1: none).
struct CloudMarch {
	layer: vec4f,
	first: f32,
};

// The light a march's samples take: the lit and shade colours, the light's direction (the sun or
// the moon, whichever is over the face) and how much it is day.
struct CloudShading {
	lit: vec3f,
	shade: vec3f,
	light: vec3f,
	// The light's face-local up part.
	light_rise: f32,
	dayness: f32,
};

fn cloud_shading(face: f32, lights: CloudLighting) -> CloudShading {
	let up = face_up(face);
	let sun_up = dot(lights.sun, up);
	let light = select(lights.moon, lights.sun, sun_up >= dot(lights.moon, up));
	let dayness = smoothstep(-CLOUD_TWILIGHT, CLOUD_TWILIGHT, dot(up, lights.day_pole));
	let low = (1.0 - smoothstep(0.02, 0.4, sun_up)) * CLOUD_SUN_WARMTH;
	let day_lit = mix(CLOUD_SUNLIT * cloud_hue(lights.sun_color), CLOUD_SUNSET, low);
	let day_shade = CLOUD_SHADE * mix(vec3f(1.0), cloud_hue(lights.day_ambient), 0.5);
	let night_lit = CLOUD_NIGHT_LIT * cloud_hue(lights.moon_color);
	let night_shade = CLOUD_NIGHT_SHADE * mix(vec3f(1.0), cloud_hue(lights.night_ambient), 0.5);
	return CloudShading(mix(night_lit, day_lit, dayness), mix(night_shade, day_shade, dayness), light, dot(light, up), dayness);
}

// Per ray: the phase for its angle to the light, and the powder term's strength (strongest looking
// away from the light).
fn cloud_ray_light(dir: vec3f, sh: CloudShading) -> vec2f {
	let c = dot(dir, sh.light);
	return vec2f(cloud_phase(c), CLOUD_VOL_POWDER * (0.5 - 0.5 * c));
}

// A sample's colour (sample_light, sample_colour): the light through `od` (m of density 1) toward
// it by Beer with a multiple-scattering floor, the powder term and the ray's phase (`ray`:
// cloud_ray_light), plus the sky's light from above dimmed by the cloud `above` (m) and the
// ground's darker bounce from below dimmed by the cloud `below`: bases darker than tops.
fn cloud_vol_light(sh: CloudShading, od: f32, above: f32, below: f32, ray: vec2f) -> vec3f {
	let tau = CLOUD_ABSORPTION * CLOUD_VOL_EXTINCTION * od;
	let beer = max(exp(-tau), CLOUD_VOL_MULTI * exp(-0.25 * tau));
	let powder = 1.0 - ray.y * exp(-2.0 * tau);
	let sun = CLOUD_VOL_SUN * beer * powder * ray.x;
	let amb = CLOUD_VOL_SKY * exp(-above / CLOUD_VOL_AMBIENT_DEPTH) + CLOUD_VOL_BOUNCE * exp(-below / CLOUD_VOL_AMBIENT_DEPTH) * CLOUD_VOL_BOUNCE_TINT;
	return sh.shade * amb + sh.lit * sun;
}

// The layer's march (the deck's noise as a volume) along `dir` (`rise` its face-local up part) from
// an eye `eye_h` above sea level, in front-to-back colour (rgb, not divided by cover) and
// transmittance (a), and the first cloud's distance. Early-outs: no coverage, or a ray that never
// crosses the layer inside the reach, takes no samples.
fn cloud_layer_march(dir: vec3f, rise: f32, eye_h: f32, max_t: f32, jitter: f32, offsets: vec4f, cfg: vec4f, steps: i32, light_steps: i32, sh: CloudShading) -> CloudMarch {
	let none = CloudMarch(vec4f(0.0, 0.0, 0.0, 1.0), -1.0);
	if (cfg.x <= 0.0) {
		return none;
	}
	var t0 = 0.0;
	var t1 = min(max_t, CLOUD_VOL_MAX_DISTANCE);
	if (abs(rise) > 1e-5) {
		let a = (CLOUD_VOL_BASE - eye_h) / rise;
		let b = (CLOUD_VOL_TOP - eye_h) / rise;
		t0 = max(t0, min(a, b));
		t1 = min(t1, max(a, b));
	} else if (eye_h < CLOUD_VOL_BASE || eye_h > CLOUD_VOL_TOP) {
		return none;
	}
	if (t1 <= t0) {
		return none;
	}
	let dt = (t1 - t0) / f32(steps);
	let ray = cloud_ray_light(dir, sh);
	var trans = 1.0;
	var energy = vec3f(0.0);
	var first = -1.0;
	for (var i = 0; i < CLOUD_MAX_STEPS; i++) {
		if (i >= steps || trans < 0.02) {
			break;
		}
		let t = t0 + (f32(i) + jitter) * dt;
		let rel = dir.xz * t;
		let h = eye_h + rise * t;
		let y = (h - CLOUD_VOL_BASE) / CLOUD_VOL_H;
		let q = offsets.xy + rel / CLOUD_SCALE;
		// The warp and the weather noise once per sample; the light samples reuse the warp and the
		// column's profile.
		let wq = q + cloud_vol_warp(q, offsets.zw);
		let n = cloud_vol_n(wq);
		let wm = cloud_weather(n, wq, cfg.x);
		if (wm.x <= 0.001) {
			continue;
		}
		let prof = cloud_type_profile(wm.y);
		let g = cloud_height_gradient(prof, y);
		if (g <= 0.001) {
			continue;
		}
		let pm = cloud_shape_point(q, h);
		var d = cloud_base_shape(cloud_base_at(pm), y, wm.x, prof, g);
		if (d <= 0.001) {
			continue;
		}
		// The detail erodes the edges (two fetches: the curl, then the fBm); the core stays.
		d = cloud_erode(d, cloud_detail_fbm(pm, y, offsets.zw * CLOUD_SCALE), y) * cloud_density_scale(prof, y);
		if (d <= 0.001) {
			continue;
		}
		if (first < 0.0) {
			first = t;
		}
		// The cloud above and below in this column, from its profile (column_cover_m; no lookups).
		let above = 0.5 * wm.x * max(prof.fade.w - y, 0.0) * CLOUD_VOL_H;
		let below = 0.5 * wm.x * clamp(y, 0.0, prof.fade.w) * CLOUD_VOL_H;
		// The light march (light_depth): growing steps toward the light through the cheap base shape
		// (no detail), stopping where the path leaves the layer; with none, the cloud above along the
		// light's slant.
		var od = 0.5 * CLOUD_VOL_LIGHT_STEP * d;
		if (light_steps <= 0) {
			od += select(CLOUD_VOL_H, above / max(sh.light_rise, 0.3), sh.light_rise > 0.0);
		}
		var at = 0.0;
		var len = CLOUD_VOL_LIGHT_STEP;
		for (var j = 0; j < CLOUD_MAX_LIGHT_STEPS; j++) {
			if (j >= light_steps) {
				break;
			}
			let mid = at + 0.5 * len;
			let yl = y + sh.light_rise * mid / CLOUD_VOL_H;
			if (yl <= 0.0 || yl >= 1.0) {
				break;
			}
			let cl = cloud_cover(cloud_vol_n(wq + sh.light.xz * (mid / CLOUD_SCALE)), cfg.x);
			let pl = pm + vec3f(sh.light.x, sh.light_rise, sh.light.z) * mid;
			od += len * cloud_base_shape(cloud_base_at(pl), yl, cl, prof, cloud_height_gradient(prof, yl)) * cloud_density_scale(prof, yl);
			at += len;
			len *= CLOUD_VOL_LIGHT_GROWTH;
		}
		let col = cloud_vol_light(sh, od, above, below, ray);
		let a = 1.0 - exp(-CLOUD_VOL_EXTINCTION * d * dt);
		energy += trans * a * col;
		trans *= 1.0 - a;
	}
	return CloudMarch(vec4f(energy, trans), first);
}

// One plume of the run-time list (dwell-look plumes.rs `PlumeList::pack`): `at` its axis's offset
// from the eye (m, x and z), its base radius and spread; `span` its base and top (m above sea level
// on the viewer's face), its density and how far its billows have risen (m); `color` its colour.
struct CloudPlume {
	at: vec4f,
	span: vec4f,
	color: vec4f,
};

// The plume list: `head.x` the plumes in use, `head.y` the samples through each (the tier's
// plumeSteps; 0: none).
struct CloudPlumes {
	head: vec4f,
	items: array<CloudPlume, CLOUD_PLUME_SLOTS>,
};

// A plume's density at `rel` metres across from the eye and height `h` (plumes.rs plume_density's
// twin, then broken up by rising billows).
fn cloud_plume_density(p: CloudPlume, rel: vec2f, h: f32) -> f32 {
	let u = (p.span.y - p.span.x);
	let v = (h - p.span.x) / u;
	if (v <= 0.0 || v >= 1.0) {
		return 0.0;
	}
	let r = p.at.z * (1.0 + p.at.w * v);
	let o = rel - p.at.xy;
	var k = p.span.z * (1.0 - smoothstep(0.5 * r, r, length(o))) * smoothstep(0.0, 0.08, v) * (1.0 - smoothstep(0.85, 1.0, v));
	if (k > 0.0 && CLOUD_PLUME_BILLOW > 0.0) {
		let n = cloud_value3(vec3f(o.x / r, (h - p.span.x - p.span.w) / r, o.y / r) + vec3f(7.0, 0.0, 3.0));
		k *= 1.0 - CLOUD_PLUME_BILLOW * (1.0 - n);
	}
	return k;
}

// The span (t0, t1) of the ray in a plume's bounding column (plumes.rs plume_span's twin);
// t1 <= t0: it misses.
fn cloud_plume_span(p: CloudPlume, dir: vec3f, rise: f32, eye_h: f32, reach: f32) -> vec2f {
	var t0 = 0.0;
	var t1 = reach;
	if (abs(rise) > 1e-5) {
		let a = (p.span.x - eye_h) / rise;
		let b = (p.span.y - eye_h) / rise;
		t0 = max(t0, min(a, b));
		t1 = min(t1, max(a, b));
	} else if (eye_h < p.span.x || eye_h > p.span.y) {
		return vec2f(1.0, 0.0);
	}
	let r = p.at.z * (1.0 + p.at.w);
	let aa = dot(dir.xz, dir.xz);
	let bb = dot(dir.xz, p.at.xy);
	let cc = dot(p.at.xy, p.at.xy) - r * r;
	if (aa < 1e-10) {
		if (cc > 0.0) {
			return vec2f(1.0, 0.0);
		}
	} else {
		let disc = bb * bb - aa * cc;
		if (disc <= 0.0) {
			return vec2f(1.0, 0.0);
		}
		let q = sqrt(disc);
		t0 = max(t0, (bb - q) / aa);
		t1 = min(t1, (bb + q) / aa);
	}
	return vec2f(t0, t1);
}

// The run-time plumes (§8.7) along `dir`, each the ray crosses marched by `head.y` samples over its
// own span (a ray that misses every plume's bounding column takes none), put together with the
// layer's march `layer` front to back by each one's first distance. Lit by its own thickness toward
// the light (Beer over its radius), tinted by its colour.
fn dwell_cloud_plumes(layer: CloudMarch, dir: vec3f, rise: f32, eye_h: f32, max_t: f32, jitter: f32, sh: CloudShading) -> CloudMarch {
	var out = layer;
	let count = u32(cloud_plume_list.head.x);
	let steps = i32(cloud_plume_list.head.y);
	if (count == 0u || steps <= 0) {
		return out;
	}
	let reach = min(max_t, CLOUD_VOL_MAX_DISTANCE);
	let ray = cloud_ray_light(dir, sh);
	for (var k = 0u; k < CLOUD_PLUME_SLOTS; k++) {
		if (k >= count) {
			break;
		}
		let p = cloud_plume_list.items[k];
		let span = cloud_plume_span(p, dir, rise, eye_h, reach);
		if (span.y <= span.x) {
			continue;
		}
		let dt = (span.y - span.x) / f32(steps);
		var trans = 1.0;
		var energy = vec3f(0.0);
		var first = -1.0;
		for (var i = 0; i < CLOUD_MAX_PLUME_STEPS; i++) {
			if (i >= steps || trans < 0.02) {
				break;
			}
			let t = span.x + (f32(i) + jitter) * dt;
			let h = eye_h + rise * t;
			let d = cloud_plume_density(p, dir.xz * t, h);
			if (d <= 0.001) {
				continue;
			}
			if (first < 0.0) {
				first = t;
			}
			// Its own thickness toward the light (its radius), the column over and under the sample.
			let col = p.color.rgb * cloud_vol_light(sh, d * p.at.z, 0.5 * d * (p.span.y - h), 0.5 * d * (h - p.span.x), ray);
			let a = 1.0 - exp(-CLOUD_VOL_EXTINCTION * d * dt);
			energy += trans * a * col;
			trans *= 1.0 - a;
		}
		if (first < 0.0) {
			continue;
		}
		// Front to back by first distance: the nearer over the farther.
		if (out.first < 0.0 || first < out.first) {
			out = CloudMarch(vec4f(energy + trans * out.layer.rgb, trans * out.layer.a), first);
		} else {
			out = CloudMarch(vec4f(out.layer.rgb + out.layer.a * energy, out.layer.a * trans), out.first);
		}
	}
	return out;
}

// The march's end for a texel whose scene spans `rec` (m along the ray: its nearest and farthest
// pixel, the sky at CLOUD_VOL_MAX_DISTANCE): its farthest pixel, or the whole layer where any of it
// is sky or past the reach (march_end's twin, BUGS.md #88).
fn dwell_cloud_march_end(rec: vec2f) -> f32 {
	return select(rec.y, 1e9, rec.y >= CLOUD_VOL_MAX_DISTANCE);
}

// A texel's weight in the composite's upsample at a pixel whose scene is `e` m along its ray
// (CLOUD_VOL_MAX_DISTANCE: the sky), for the texel's scene span `rec`: 1 where its march ended at
// the pixel's scene within the band, falling to the fallback's share past twice the band
// (upsample_weight + upsample_cover's floor, the twins). A function of depths alone: no jitter.
fn dwell_cloud_upsample_weight(e: f32, rec: vec2f) -> f32 {
	let band = max(max(min(rec.y - rec.x, CLOUD_VOL_DEPTH_BAND_RANGE * e), CLOUD_VOL_DEPTH_BAND * e), 1.0);
	let miss = abs(rec.y - e);
	return 1.0 - smoothstep(band, 2.0 * band, miss) + CLOUD_UPSAMPLE_FALLBACK / (1.0 + miss / band);
}

// The volumetric clouds along the unit ray `dir` out to `max_t` (m): the layer, then the run-time
// plumes, with the distance to the first cloud (the reduced-resolution pass keeps it, so the
// composite can put the scene in front of clouds behind it). `jitter` (0-1, per pixel) offsets the
// steps; `inside` is cloud_uniforms' third vector (its w the steps and light samples; 0: none, the
// 2D deck draws). Lit by the sun or the moon over the face (§8.8): Beer's law along a few light
// samples, a powder term, a dual-lobe phase (the silver lining toward the light), the sky's light
// from above and the ground's darker bounce from below, dimmed by the cloud over and under. The
// layer is colour (rgb, not hazed: dwell_cloud_hazed) and cover (a).
fn dwell_cloud_march(dir: vec3f, face: f32, max_t: f32, jitter: f32, offsets: vec4f, cfg: vec4f, inside: vec4f, lights: CloudLighting) -> CloudMarch {
	let none = CloudMarch(vec4f(0.0), -1.0);
	let steps = i32(floor(inside.w));
	let light_steps = i32(round(fract(inside.w) * 8.0));
	if (steps <= 0 || cfg.y <= 0.0) {
		return none;
	}
	let eye_h = CLOUD_ALTITUDE - cfg.w;
	let rise = face_rise(face, dir);
	let sh = cloud_shading(face, lights);
	let layer = cloud_layer_march(dir, rise, eye_h, max_t, jitter, offsets, cfg, steps, light_steps, sh);
	let m = dwell_cloud_plumes(layer, dir, rise, eye_h, max_t, jitter, sh);
	let trans = m.layer.a;
	let cover = 1.0 - trans;
	if (cover <= 0.0 || m.first < 0.0) {
		return none;
	}
	var col = m.layer.rgb / cover;
	// The silver lining is the phase's (thin cloud toward the light); its brightest rolls off a
	// little over white (soft_clip).
	let k = max(max(col.r, max(col.g, col.b)) - 1.0, 0.0);
	col *= (1.0 + 0.35 * k / (1.0 + k)) / (1.0 + k);
	return CloudMarch(vec4f(col, cover), m.first);
}

// A march's layer hazed toward the clear sky `clear` behind it with its first cloud's distance, and
// faded out with the deck's distance fade: called only where there is cloud, so a pixel with none
// never works out its sky.
fn dwell_cloud_hazed(m: CloudMarch, clear: vec3f, cfg: vec4f) -> vec4f {
	if (m.first < 0.0) {
		return vec4f(0.0);
	}
	let fade = dwell_cloud_fade(cfg.w);
	let col = mix(m.layer.rgb, clear, 0.85 * smoothstep(0.0, fade.y, m.first));
	return vec4f(col, m.layer.a * CLOUD_OPACITY * (1.0 - smoothstep(fade.x, fade.y, m.first)));
}
";

const CLOUD_FNS: &str = r"
fn cloud_hash(c: vec2i) -> f32 {
	let w = vec2u(((c % CLOUD_PERIOD) + CLOUD_PERIOD) % CLOUD_PERIOD);
	var h = (w.x * 0x8da6b343u) ^ (w.y * 0xd8163841u);
	h = (h ^ (h >> 16u)) * 0x7feb352du;
	h = (h ^ (h >> 15u)) * 0x846ca68bu;
	h ^= h >> 16u;
	return f32(h) / 4294967295.0;
}

fn cloud_value(p: vec2f) -> f32 {
	let i = floor(p);
	let f = p - i;
	let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
	let c = vec2i(i);
	let a = cloud_hash(c);
	let b = cloud_hash(c + vec2i(1, 0));
	let d = cloud_hash(c + vec2i(0, 1));
	let e = cloud_hash(c + vec2i(1, 1));
	return mix(mix(a, b, u.x), mix(d, e, u.x), u.y);
}

// The fBm, each octave faded to its mean as it shrinks below the pixel (`fw`: the pixel's
// footprint in cells), so the far deck doesn't shimmer.
fn cloud_fbm(p_in: vec2f, octaves: i32, fw: f32) -> f32 {
	var p = p_in;
	var amp = 0.5;
	var sum = 0.0;
	var norm = 0.0;
	var foot = fw;
	for (var i = 0; i < CLOUD_MAX_OCTAVES; i++) {
		if (i >= octaves) {
			break;
		}
		let keep = 1.0 - smoothstep(0.25, 0.6, foot);
		sum += amp * mix(0.5, cloud_value(p), keep);
		norm += amp;
		amp *= CLOUD_GAIN;
		p = vec2f(2.0 * p.x + p.y + 13.0, -p.x + 2.0 * p.y + 7.0);
		foot *= CLOUD_LACUNARITY;
	}
	return sum / norm;
}

fn cloud_warped(p: vec2f, warp_off: vec2f, fw: f32) -> vec2f {
	let w = p + warp_off;
	let q = vec2f(cloud_fbm(w, 2, fw), cloud_fbm(w + vec2f(5.2, 1.3), 2, fw)) - 0.5;
	return p + CLOUD_WARP * q;
}

fn cloud_shape(n_in: f32, coverage: f32) -> f32 {
	let n = clamp((n_in - 0.5) * CLOUD_CONTRAST + 0.5, 0.0, 1.0);
	let cut = 1.0 - coverage;
	return select(0.0, smoothstep(cut, cut + CLOUD_SOFTNESS, n), coverage > 0.0);
}

// The distance fade's start and end (deck_fade): the data's from below; from above, out to the
// deck's own horizon.
fn dwell_cloud_fade(height: f32) -> vec2f {
	let k = -height * (2.0 * (CLOUD_CURVATURE + CLOUD_ALTITUDE) - height);
	let far = sqrt(max(k, 0.0));
	return vec2f(max(CLOUD_FADE_START, 0.4 * far), max(CLOUD_FADE_END, far));
}

// `height` is the deck's height above the eye (negative: the eye is above it). From below, clouds
// show above the horizon; from above, rays down past the deck's horizon meet its top, faded where
// they graze it.
fn dwell_cloud_deck(dir: vec3f, face: f32, offsets: vec4f, height: f32) -> vec4f {
	let above = face_rise(face, dir);
	let e = CLOUD_CURVATURE + CLOUD_ALTITUDE - height;
	let k = -height * (2.0 * (CLOUD_CURVATURE + CLOUD_ALTITUDE) - height);
	let ey = e * above;
	let disc = ey * ey - k;
	let s = sqrt(max(disc, 0.0));
	let from_below = k < 0.0;
	let t = select(k / max(-ey + s, 1e-6), -k / max(ey + s, 1e-6), from_below);
	let hit = from_below || (above < 0.0 && disc >= 0.0 && k > 0.0);
	// The sine of the angle the ray meets the deck at, from above: 0 at the deck's horizon.
	let graze = s / (CLOUD_CURVATURE + CLOUD_ALTITUDE);
	let facing = select(smoothstep(0.0, CLOUD_HORIZON_FADE, graze), smoothstep(0.0, CLOUD_HORIZON_FADE, above), from_below);
	let shown = select(0.0, 1.0, hit) * facing
		* smoothstep(0.0, CLOUD_CROSS_FADE, abs(height))
		* (1.0 - smoothstep(dwell_cloud_fade(height).x, dwell_cloud_fade(height).y, t));
	// Within the layer the deck is seen edge-on: a band at the horizon (dwell_cloud_band).
	let band = dwell_cloud_band(height, above);
	let use_band = band.y > shown;
	let d = select(t, band.x, use_band);
	let q = offsets.xy + dir.xz * (d / CLOUD_SCALE);
	// The band is flagged by a negative weight: it is the layer seen through its length, so it
	// takes the coverage's mean rather than the noise (whose columns would show as stripes).
	return vec4f(q, d, select(shown, -band.y, use_band));
}

// The deck seen edge-on from within the layer (`cross_fade_m` either side of it): along a ray
// nearly level with it, cloud where the ray leaves the layer, out to the fade's start; strongest
// with the eye at the deck and the ray level. (distance, weight); deck_band is its twin.
fn dwell_cloud_band(height: f32, above: f32) -> vec2f {
	let level = abs(above);
	let inside = 1.0 - smoothstep(0.0, CLOUD_CROSS_FADE, abs(height));
	let d = min(CLOUD_CROSS_FADE / max(level, 1e-4), CLOUD_FADE_START);
	return vec2f(d, inside * (1.0 - smoothstep(0.0, CLOUD_BAND, level)));
}

// Whether the deck point `t` along the unit ray is in front of the opaque scene at view depth
// `depth` (0: the sky); `ray_len` is the length of the view ray scaled to view depth 1, `rise` how
// far it climbs (face-local), `height` the deck's height above the eye. From above, the deck is
// in front of anything below its altitude, however far the sphere has sunk (cloud_in_front).
fn dwell_cloud_in_front(t: f32, ray_len: f32, depth: f32, rise: f32, height: f32) -> bool {
	return depth <= 0.0 || t / ray_len < depth || (height < 0.0 && rise * depth < height);
}

// The light the clouds take, from the renderer's frame (so they follow the sky frame and the
// lights, whatever sets them): the sky frame's directions, and the sun's, moon's and each sky's
// ambient light colours (linear, any intensity: only their hue is used).
struct CloudLighting {
	day_pole: vec3f,
	sun: vec3f,
	moon: vec3f,
	sun_color: vec3f,
	moon_color: vec3f,
	day_ambient: vec3f,
	night_ambient: vec3f,
};

// A light's hue for display: its linear colour scaled to a brightest channel of 1, encoded.
fn cloud_hue(c: vec3f) -> vec3f {
	return pow(c / max(max(c.r, max(c.g, c.b)), 1e-4), vec3f(1.0 / 2.2));
}

fn dwell_clouds(under: vec3f, clear: vec3f, dir: vec3f, face: f32, deck: vec4f, fw: f32, offsets: vec4f, cfg: vec4f, lights: CloudLighting) -> vec3f {
	return dwell_cloud_over(under, dwell_cloud_layer(clear, dir, face, deck, fw, offsets, cfg, lights));
}

// A cloud layer (dwell_cloud_layer) over `under`.
fn dwell_cloud_over(under: vec3f, layer: vec4f) -> vec3f {
	return mix(under, layer.rgb, layer.a);
}

// The clouds along `dir` as a layer: their colour (rgb, far cloud hazed toward `clear`) and how
// much of what is behind they hide (a, 0 for none). Found once, it goes over any number of colours
// behind it (the edge blur's taps: MOTION_EFFECTS.md).
fn dwell_cloud_layer(clear: vec3f, dir: vec3f, face: f32, deck: vec4f, fw: f32, offsets: vec4f, cfg: vec4f, lights: CloudLighting) -> vec4f {
	let day_pole = lights.day_pole;
	let sun = lights.sun;
	let moon = lights.moon;
	let octaves = i32(cfg.y);
	let band = deck.w < 0.0;
	let weight = abs(deck.w);
	if (octaves <= 0 || weight <= 0.0) {
		return vec4f(0.0);
	}
	var p = deck.xy;
	var d = cfg.x;
	if (!band) {
		p = cloud_warped(deck.xy, offsets.zw, fw);
		d = cloud_shape(cloud_fbm(p, octaves, fw), cfg.x);
	}
	if (d <= 0.0) {
		return vec4f(0.0);
	}
	let up = face_up(face);
	// The light: the sun or the moon, whichever is above this face.
	let sun_up = dot(sun, up);
	let light = select(moon, sun, sun_up >= dot(moon, up));
	// Self-shadow (High): the density a step toward the light, plus the cloud's own thickness. Not
	// from above: the sunlit top barely shows it (topLight), so the second sample is skipped there.
	let from_above = cfg.w < 0.0;
	var occl = 0.6 * d;
	if (cfg.z > 0.0 && !from_above && !band) {
		let toward = light.xz * CLOUD_SHADOW_STEP;
		occl = 0.35 * d + cloud_shape(cloud_fbm(p + toward, octaves, fw), cfg.x);
	}
	// Seen from above, the deck's sunlit top: the light reaches it through less cloud.
	let lit = exp(-CLOUD_ABSORPTION * occl * select(1.0, 1.0 - CLOUD_TOP_LIGHT, from_above && dot(light, up) > 0.0));
	// Day or night as the viewer's sky is (the day/night cycle: the same time of day all round the
	// sky), by the day pole's height: the sun's.
	let dayness = smoothstep(-CLOUD_TWILIGHT, CLOUD_TWILIGHT, dot(up, day_pole));
	var day = mix(CLOUD_SHADE * mix(vec3f(1.0), cloud_hue(lights.day_ambient), 0.5), CLOUD_SUNLIT * cloud_hue(lights.sun_color), lit);
	let low = (1.0 - smoothstep(0.02, 0.4, sun_up)) * CLOUD_SUN_WARMTH;
	day = mix(day, CLOUD_SUNSET * mix(0.75, 1.0, lit), low * lit);
	let night = mix(CLOUD_NIGHT_SHADE * mix(vec3f(1.0), cloud_hue(lights.night_ambient), 0.5), CLOUD_NIGHT_LIT * cloud_hue(lights.moon_color), lit);
	var col = mix(night, day, dayness);
	// From above the tops are white in full light: the light's tint shows half as much (a low or
	// warm sun read as sand on the deck's top, where the eye sees only lit cloud).
	if (from_above) {
		col = mix(col, vec3f(dot(col, vec3f(0.2126, 0.7152, 0.0722))), 0.5 * dayness);
	}
	// The silver lining: thin cloud near the light glows.
	let toward_light = max(dot(dir, light), 0.0);
	let rim = CLOUD_SILVER * pow(toward_light, CLOUD_SILVER_POWER) * (1.0 - d) * (1.0 - d);
	col += rim * mix(CLOUD_MOON_GLOW * 0.5, CLOUD_SUN_GLOW, dayness);
	// Far cloud takes the sky's haze behind it.
	col = mix(col, clear, 0.85 * smoothstep(0.0, dwell_cloud_fade(cfg.w).y, deck.z));
	// From above the deck is seen through its whole depth: denser, and solid sooner at its edges.
	let cover = select(d, sqrt(d), from_above);
	return vec4f(col, cover * CLOUD_OPACITY * weight);
}

// Inside the cloud: the whole view mixed toward the cloud's lit colour by `amount` (cloud_uniforms'
// inside.x), so passing through the deck is a fog, not a pop.
fn dwell_cloud_fog(color: vec3f, amount: f32, face: f32, lights: CloudLighting) -> vec3f {
	if (amount <= 0.0) {
		return color;
	}
	let up = face_up(face);
	let dayness = smoothstep(-CLOUD_TWILIGHT, CLOUD_TWILIGHT, dot(up, lights.day_pole));
	let day = mix(CLOUD_SHADE * mix(vec3f(1.0), cloud_hue(lights.day_ambient), 0.5), CLOUD_SUNLIT * cloud_hue(lights.sun_color), 0.6);
	let night = mix(CLOUD_NIGHT_SHADE * mix(vec3f(1.0), cloud_hue(lights.night_ambient), 0.5), CLOUD_NIGHT_LIT * cloud_hue(lights.moon_color), 0.6);
	return mix(color, mix(night, day, dayness), amount);
}
";

#[cfg(test)]
mod tests {
    use super::*;

    /// The deck alone, as the modules that draw no march take it (no plume binding).
    fn validate(src: &str) {
        let src = format!("{}{src}", dwell_world::FaceFrame::wgsl());
        let module = naga::front::wgsl::parse_str(&src).expect("WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
    }

    #[test]
    fn the_shader_is_valid_wgsl() {
        validate(&cloud_wgsl(&CLOUDS));
    }

    /// The hit `t` along the ray with dot `y` with up, from an eye `h` below the deck, lies on the
    /// deck's sphere (radius r + a about a centre r below sea level).
    fn on_sphere(y: f64, h: f64, a: f64, r: f64, t: f64) {
        let e = r + a - h;
        let (x, z) = ((1.0 - y * y).sqrt() * t, y * t + e);
        assert!(
            ((x * x + z * z).sqrt() - (r + a)).abs() < 1e-6,
            "y {y} h {h} t {t}"
        );
    }

    #[test]
    fn from_below_the_deck_is_its_height_straight_up_and_finite_at_the_horizon() {
        // The eye at sea level, the deck 1.5 km up.
        let (a, r) = (1500.0, 120_000.0);
        let h = a;
        assert!((deck_distance(1.0, h, a, r).unwrap() - h).abs() < 1e-9);
        // At the horizon: the tangent's length to the sphere r + h, √(2rh + h²).
        let horizon = deck_distance(0.0, h, a, r).unwrap();
        assert!((horizon - (2.0 * r * h + h * h).sqrt()).abs() < 1e-6);
        // Farther as the ray drops toward the horizon, and the hit is on the sphere.
        let mut last = 0.0;
        for i in (0..=100).rev() {
            let y = f64::from(i) / 100.0;
            let t = deck_distance(y, h, a, r).unwrap();
            assert!(t > last);
            last = t;
            on_sphere(y, h, a, r, t);
        }
        // A flat deck's 1/sin(e) for a huge radius.
        let flat = deck_distance(0.25, h, a, 1e12).unwrap();
        assert!((flat - h / 0.25).abs() < 1e-3);
        // From below every ray meets it, even one aimed down (the composite's depth test hides it).
        assert!(deck_distance(-0.5, h, a, r).is_some());
        let mut t = CLOUDS;
        (t.deck.altitude_m, t.deck.curvature_m) = (a, r);
        assert_eq!(deck_fade(&t, h), (t.deck.fade_start_m, t.deck.fade_end_m));
    }

    #[test]
    fn from_above_only_rays_down_past_its_horizon_meet_the_deck_top() {
        let (a, r) = (1800.0, 120_000.0);
        let h = -1200.0; // the eye 3 km up, 1.2 km over the deck
        assert!((deck_distance(-1.0, h, a, r).unwrap() - 1200.0).abs() < 1e-9);
        assert_eq!(deck_distance(1.0, h, a, r), None);
        assert_eq!(deck_distance(0.0, h, a, r), None);
        // The deck's horizon, seen from above: the ray grazing the sphere, √(|eye|² − |deck|²) away.
        let e = r + a - h;
        let grazing = -((e * e - (r + a) * (r + a)) / (e * e)).sqrt();
        let at_horizon = deck_distance(grazing - 1e-9, h, a, r).unwrap();
        let tangent = (e * e - (r + a) * (r + a)).sqrt();
        assert!(
            (at_horizon / tangent - 1.0).abs() < 1e-3,
            "{at_horizon} vs {tangent}"
        );
        assert_eq!(deck_distance(grazing + 1e-3, h, a, r), None);
        // Nearer as the ray steepens, on the sphere, and the near side (not through the far one).
        let mut last = f64::INFINITY;
        for i in 1..=100 {
            let y = grazing - (1.0 + grazing) * f64::from(i) / 100.0;
            let t = deck_distance(y, h, a, r).unwrap();
            assert!(t < last + 1e-9 && t >= 1200.0 - 1e-9);
            last = t;
            on_sphere(y, h, a, r, t);
        }
        // High above, the deck reads as a sea out to its own horizon: the fade reaches it.
        let mut t = CLOUDS;
        (t.deck.altitude_m, t.deck.curvature_m) = (a, r);
        let high = -13_000.0;
        let e = r + a - high;
        let horizon = (e * e - (r + a) * (r + a)).sqrt();
        let (start, end) = deck_fade(&t, high);
        assert!((end - horizon).abs() < 1e-6 && start < end && start > t.deck.fade_start_m);
        // Just over the deck, the data's fade.
        assert_eq!(
            deck_fade(&t, -10.0),
            (t.deck.fade_start_m, t.deck.fade_end_m)
        );
    }

    #[test]
    fn inside_the_deck_it_is_near_and_the_crossing_has_no_jump() {
        let (a, r) = (1800.0, 120_000.0);
        assert_eq!(deck_distance(1.0, 0.0, a, r), None);
        // Just under and just over: straight up or down it is the small gap, and level rays meet it
        // nearby from below and miss from above.
        assert!((deck_distance(1.0, 0.5, a, r).unwrap() - 0.5).abs() < 1e-9);
        assert!((deck_distance(-1.0, -0.5, a, r).unwrap() - 0.5).abs() < 1e-9);
        assert!(deck_distance(0.0, 0.5, a, r).unwrap() < 400.0);
        assert_eq!(deck_distance(0.0, -0.5, a, r), None);
        // The view fogs as the eye nears the deck inside a cloud, from either side alike, and the
        // deck itself fades out over the same band (dwell_cloud_deck's cross fade): no pop.
        let mut t = CLOUDS;
        t.shape.coverage = 1.0;
        let deck = t.deck.altitude_m + dwell_protocol::world::SEA_LEVEL_M;
        // An eye position over a cloud (not every point of even full coverage is cloud).
        let x = (0..200)
            .map(|i| f64::from(i) * 97.0)
            .find(|&x| {
                cloud_uniforms(
                    &t,
                    [x, deck, 0.0],
                    FaceFrame::A,
                    0.0,
                    CloudQuality::Low,
                    1.0,
                )[2][0]
                    > 0.3
            })
            .expect("a cloud");
        let at = |y: f64| {
            cloud_uniforms(&t, [x, y, 0.0], FaceFrame::A, 0.0, CloudQuality::Low, 1.0)[2][0]
        };
        let full = (t.deck.inside_fog * t.shape.opacity) as f32;
        assert!(at(deck) > 0.0 && at(deck) <= full + 1e-6, "{}", at(deck));
        assert!((at(deck - 100.0) - at(deck + 100.0)).abs() < 0.05);
        assert!(at(deck - 50.0) < at(deck) && at(deck - 300.0) < at(deck - 50.0));
        assert_eq!(at(deck - t.deck.cross_fade_m - 1.0), 0.0);
        assert_eq!(at(deck + t.deck.cross_fade_m + 1.0), 0.0);
        assert_eq!(
            cloud_uniforms(
                &t,
                [0.0, deck, 0.0],
                FaceFrame::A,
                0.0,
                CloudQuality::Off,
                1.0
            )[2][0],
            0.0
        );
    }

    #[test]
    fn at_the_deck_it_is_a_band_at_the_horizon_that_hands_over_to_the_deck() {
        let t = CLOUDS;
        // At the deck, looking level: the band at full weight, as far as the fade's start.
        assert_eq!(deck_band(&t, 0.0, 0.0), (t.deck.fade_start_m, 1.0));
        // Steeper rays leave the layer sooner and the band goes; out of the layer it is gone.
        let (near, w) = deck_band(&t, 0.0, 0.08);
        assert!(near < t.deck.fade_start_m && w > 0.0 && w < 1.0);
        assert_eq!(deck_band(&t, 0.0, 0.2).1, 0.0);
        assert_eq!(deck_band(&t, t.deck.cross_fade_m, 0.0).1, 0.0);
        assert_eq!(deck_band(&t, -t.deck.cross_fade_m, 0.0).1, 0.0);
        // Symmetric either side of the deck, and the shader carries the same expression.
        assert_eq!(deck_band(&t, 120.0, 0.03), deck_band(&t, -120.0, -0.03));
        let src = cloud_wgsl(&t);
        assert!(
            src.contains("let d = min(CLOUD_CROSS_FADE / max(level, 1e-4), CLOUD_FADE_START);")
        );
    }

    #[test]
    fn the_deck_draws_over_the_scene_only_in_front_of_it() {
        // A view ray 20° off the view axis is 1/cos 20° long at view depth 1.
        let len = 1.0 / 20f64.to_radians().cos();
        let depth_of = |t: f64| t / len;
        // The sky (no depth): always.
        assert!(cloud_in_front(5000.0, len, 0.0, 0.0, 1800.0));
        // A chunk 50 m away in front of a deck 1.2 km below: the chunk shows.
        assert!(!cloud_in_front(1200.0, len, depth_of(50.0), 0.0, -1200.0));
        // A full-detail chunk at the edge of its range (352 m), and a LOD section 20 km away,
        // with the deck at 5 km: the chunk shows, the far section is under the cloud.
        assert!(!cloud_in_front(5000.0, len, depth_of(352.0), 0.0, 5000.0));
        assert!(cloud_in_front(5000.0, len, depth_of(20_000.0), 0.0, 5000.0));
        // Exactly at the surface the scene wins (strictly nearer to draw over it).
        assert!(!cloud_in_front(1000.0, len, depth_of(1000.0), 0.0, 1000.0));
        // The shader's twin is the same expression.
        let src = cloud_wgsl(&CLOUDS);
        assert!(src.contains(
            "return depth <= 0.0 || t / ray_len < depth || (height < 0.0 && rise * depth < height);"
        ));
    }

    #[test]
    fn from_high_above_the_deck_covers_the_flat_world_out_to_its_edge() {
        // Owner, 2026-10-21 (iPhone, flying 2 km/s): from tens of km up the clouds showed in a
        // circle round the player and over the void past the world's edge, but not over the sea
        // between. The deck is a sphere under the eye; the world is flat. The sphere sinks below
        // sea level √(2·r·altitude) (38 km) out, so its hit along a ray to the far sea is behind
        // the sea, and the depth test hid it. The sea is below the deck's altitude: seen from
        // above, the ray crossed the deck to reach it, so the deck is in front.
        let (a, r) = (CLOUDS.deck.altitude_m, CLOUDS.deck.curvature_m);
        let eye = 40_000.0;
        let height = a - eye;
        for out in [5_000.0, 30_000.0, 45_000.0, 49_000.0] {
            let len = (out * out + eye * eye).sqrt();
            let above = -eye / len;
            // The ray along the view axis: view depth is the distance.
            let t = deck_distance(above, height, a, r).unwrap();
            assert!(
                cloud_in_front(t, 1.0, len, above, height),
                "the sea {out} m out from {eye} m up: deck hit {t} m, sea {len} m"
            );
        }
        // A peak above the deck, seen from above, is not under it; nor is the sea seen from below.
        assert!(!cloud_in_front(9_000.0, 1.0, 5_000.0, -0.1, -1_000.0));
        assert!(!cloud_in_front(9_000.0, 1.0, 5_000.0, -0.1, 1_000.0));
    }

    dwell_world::both_faces!(
        /// The deck is over each face's own sea level: an eye 300 m over face `f`'s sea (face-local)
        /// is 300 m nearer its deck.
        fn the_deck_is_over_each_faces_own_sea_level(f: FaceFrame) {
            let alt = CLOUDS.deck.altitude_m;
            let sea = dwell_protocol::world::SEA_LEVEL_M;
            assert_eq!(deck_height(&CLOUDS, f.y(sea), f), alt);
            assert_eq!(deck_height(&CLOUDS, f.y(sea + 300.0), f), alt - 300.0);
        }
    );

    #[test]
    fn the_noise_repeats_seamlessly_every_wrap() {
        let n = period(&CLOUDS) as f64;
        assert_eq!(n * CLOUDS.shape.scale_m, CLOUDS.shape.wrap_m);
        for i in 0..50 {
            let p = [f64::from(i) * 0.731 - 3.0, f64::from(i) * 1.37 + 0.2];
            let w = [0.4, 1.9];
            let at = density(&CLOUDS, p, w, 5, 0.6);
            for shift in [[n, 0.0], [0.0, n], [-n, 2.0 * n]] {
                let q = [p[0] + shift[0], p[1] + shift[1]];
                assert!((density(&CLOUDS, q, w, 5, 0.6) - at).abs() < 1e-9);
            }
        }
    }

    /// The share of a grid of points that is cloud (density over a half).
    fn cover(t: &CloudTunables, coverage: f64, octaves: u32) -> f64 {
        let n = 60;
        let mut c = 0;
        for i in 0..n {
            for j in 0..n {
                let p = [f64::from(i) * 0.53, f64::from(j) * 0.53];
                c += usize::from(density(t, p, [0.0, 0.0], octaves, coverage) > 0.5);
            }
        }
        c as f64 / f64::from(n * n)
    }

    #[test]
    fn coverage_is_roughly_the_share_of_sky_covered() {
        assert_eq!(cover(&CLOUDS, 0.0, 5), 0.0);
        let mut last = 0.0;
        for c in [0.2, 0.4, 0.6, 0.8, 1.0] {
            let k = cover(&CLOUDS, c, 5);
            assert!(k >= last, "{c}: {k} < {last}");
            last = k;
        }
        let half = cover(&CLOUDS, 0.5, 5);
        assert!((0.2..0.6).contains(&half), "{half}");
        assert!(cover(&CLOUDS, 1.0, 5) > 0.9);
        assert_eq!(density(&CLOUDS, [1.0, 2.0], [0.0, 0.0], 0, 1.0), 0.0);
    }

    #[test]
    fn a_tunable_changed_by_data_changes_the_clouds() {
        // What a JSON edit does once generated: the shader carries the new value, and the clouds
        // the CPU twin computes change with it.
        let mut t = CLOUDS;
        t.shape.edge_softness = 0.5;
        t.light.shade = [0.1, 0.2, 0.3];
        let src = cloud_wgsl(&t);
        validate(&src);
        assert_ne!(src, cloud_wgsl(&CLOUDS));
        assert!(src.contains("const CLOUD_SOFTNESS: f32 = 0.500000;"));
        assert!(src.contains("const CLOUD_SHADE: vec3f = vec3f(0.100000, 0.200000, 0.300000);"));
        let mut changed = 0;
        for i in 0..40 {
            let p = [f64::from(i) * 0.37, f64::from(i) * 0.91];
            let a = density(&CLOUDS, p, [0.0, 0.0], 5, 0.5);
            let b = density(&t, p, [0.0, 0.0], 5, 0.5);
            changed += usize::from((a - b).abs() > 1e-6);
        }
        assert!(changed > 10);
        // The uniforms follow the data's coverage and octaves.
        let mut t = CLOUDS;
        t.shape.octaves = 7;
        let u = cloud_uniforms(
            &t,
            [0.0; 3],
            FaceFrame::A,
            0.0,
            CloudQuality::High,
            t.shape.coverage,
        );
        assert_eq!(u[1][1], 7.0);
    }

    #[test]
    fn the_quality_setting_sets_octaves_and_the_self_shadow() {
        let u = |q| cloud_uniforms(&CLOUDS, [0.0; 3], FaceFrame::A, 0.0, q, 0.4)[1];
        assert_eq!(u(CloudQuality::Off)[1], 0.0);
        assert_eq!(u(CloudQuality::Low)[1], CLOUDS.shape.low_octaves as f32);
        assert_eq!(u(CloudQuality::High)[1], CLOUDS.shape.octaves as f32);
        assert_eq!(u(CloudQuality::Low)[2], 0.0);
        assert_eq!(u(CloudQuality::High)[2], 1.0);
        assert_eq!(u(CloudQuality::High)[0], 0.4);
        const { assert!(CLOUDS.shape.low_octaves < CLOUDS.shape.octaves) };
        for (name, q) in [
            ("off", CloudQuality::Off),
            ("low", CloudQuality::Low),
            ("high", CloudQuality::High),
        ] {
            assert_eq!(CloudQuality::from_name(name), Some(q));
        }
        assert_eq!(CloudQuality::from_name("ultra"), None);
    }

    #[test]
    fn offsets_stay_in_one_period_far_from_the_origin_and_drift_with_the_wind() {
        let n = period(&CLOUDS) as f32;
        let far = [7.9e6, 1200.0, -6.3e6];
        for seconds in [0.0, 1e3, 7e7] {
            let u = cloud_uniforms(&CLOUDS, far, FaceFrame::A, seconds, CloudQuality::High, 0.5)[0];
            assert!(u.iter().all(|&c| (0.0..=n).contains(&c)), "{u:?}");
        }
        // Moving the eye by a metre moves the noise point under it by a metre (in cells).
        let a = cloud_uniforms(
            &CLOUDS,
            [100.0, 0.0, 100.0],
            FaceFrame::A,
            0.0,
            CloudQuality::High,
            0.5,
        )[0];
        let b = cloud_uniforms(
            &CLOUDS,
            [101.0, 0.0, 100.0],
            FaceFrame::A,
            0.0,
            CloudQuality::High,
            0.5,
        )[0];
        assert!(((b[0] - a[0]) * CLOUDS.shape.scale_m as f32 - 1.0).abs() < 1e-3);
        // After a second the pattern has moved speed_mps downwind: the point under the eye is that
        // far upwind.
        let c = cloud_uniforms(
            &CLOUDS,
            [100.0, 0.0, 100.0],
            FaceFrame::A,
            1.0,
            CloudQuality::High,
            0.5,
        )[0];
        let moved = ((a[0] - c[0]).hypot(a[1] - c[1])) * CLOUDS.shape.scale_m as f32;
        assert!((f64::from(moved) - CLOUDS.wind.speed_mps).abs() < 1e-2);
        // Face B looks at another part of the noise.
        let d = cloud_uniforms(
            &CLOUDS,
            [100.0, -5000.0, 100.0],
            FaceFrame::B,
            0.0,
            CloudQuality::High,
            0.5,
        )[0];
        assert!((d[0] - a[0]).abs() > 1.0);
    }
}

#[cfg(test)]
mod volume_tests {
    use super::*;

    /// A test fixture plume (a volcano's ash column, by data alone).
    const ASH: Plume = Plume {
        name: "test:ash",
        x_m: 500.0,
        z_m: -300.0,
        radius_m: 200.0,
        spread: 1.5,
        base_m: 300.0,
        top_m: 3500.0,
        density: 1.0,
        color: [0.43, 0.42, 0.4],
        rise_mps: 4.0,
    };
    const WITH_ASH: CloudTunables = CloudTunables {
        plumes: &[ASH],
        ..CLOUDS
    };

    /// The cloud shader with the plume list's uniform declared, as the clouds pass declares it.
    fn validate(src: &str) {
        let src = format!(
            "{}{src}{}",
            dwell_world::FaceFrame::wgsl(),
            cloud_march_wgsl(0, 0, 1)
        );
        let module = naga::front::wgsl::parse_str(&src).expect("WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
    }

    #[test]
    fn the_settings_pick_their_tier_from_the_data() {
        assert_eq!(CloudQuality::High.volume(&CLOUDS), CLOUDS.tiers.high);
        assert_eq!(CloudQuality::Low.volume(&CLOUDS), CLOUDS.tiers.low);
        assert_eq!(CloudQuality::Off.volume(&CLOUDS).steps, 0);
        // The shipped tiers: High marches, Low (phones' default) keeps the 2D deck.
        let high = CloudQuality::High.volume(&CLOUDS).steps;
        assert!(high > 0 && high <= MAX_STEPS);
        assert_eq!(CLOUDS.tiers.low.steps, 0);
    }

    #[test]
    fn the_uniforms_carry_the_tier_and_the_volume_replaces_the_deck_fog() {
        let at = |q| cloud_uniforms(&CLOUDS, [10.0, 1800.0, 20.0], FaceFrame::A, 0.0, q, 1.0)[2];
        let high = at(CloudQuality::High);
        let tier = CLOUDS.tiers.high;
        assert_eq!(high[3].floor() as u32, tier.steps);
        assert_eq!((high[3].fract() * 8.0).round() as u32, tier.light_steps);
        assert_eq!(high[0], 0.0, "the march is the fog inside the volume");
        assert_eq!([high[1], high[2]], [10.0, 20.0]);
        let low = at(CloudQuality::Low);
        assert_eq!(low[3], 0.0);
    }

    #[test]
    fn a_plume_by_data_starts_the_run_time_list_and_is_dense_on_its_axis() {
        let d = |dx: f64, h: f64| {
            let u = (h - ASH.base_m) / (ASH.top_m - ASH.base_m);
            plume_density(ASH.radius_m, ASH.spread, ASH.density, dx, 0.0, u)
        };
        assert!(d(0.0, 1500.0) > 0.99);
        assert_eq!(d(0.0, 200.0), 0.0, "below its base");
        assert_eq!(d(0.0, 3600.0), 0.0, "above its top");
        assert_eq!(d(600.0, 400.0), 0.0, "past its radius near the base");
        assert!(d(300.0, 3000.0) > 0.5, "it widens upward");
        // It is in the list the game starts with; the shader takes it from the uniform, so the
        // shader is the same with or without it (no pipeline per plume).
        let list = crate::plumes::PlumeList::new(&WITH_ASH);
        assert_eq!(list.get("test:ash").unwrap().x_m, 500.0);
        assert_eq!(cloud_wgsl(&WITH_ASH), cloud_wgsl(&CLOUDS));
        assert!(cloud_wgsl(&CLOUDS).contains(&format!(
            "const CLOUD_PLUME_SLOTS: u32 = {}u;",
            CLOUDS.plume_slots
        )));
    }

    #[test]
    fn the_volume_shader_is_valid_wgsl_and_marches_plumes_apart_from_the_layer() {
        let src = cloud_wgsl(&CLOUDS) + &cloud_march_wgsl(0, 0, 1);
        validate(&cloud_wgsl(&CLOUDS));
        validate(&cloud_wgsl(&CloudTunables {
            plume_slots: MAX_PLUME_SLOTS,
            ..CLOUDS
        }));
        // The deck-only modules' part has no plume binding; the march's has.
        assert!(!cloud_wgsl(&CLOUDS).contains("cloud_plume_list"));
        // The layer's density no longer samples the plumes every step (§8.7's cut).
        let body = |name: &str| {
            let a = src.find(&format!("fn {name}(")).unwrap();
            let b = src[a..].find("\n}\n").unwrap();
            src[a..a + b].to_owned()
        };
        assert!(!body("cloud_layer_march").contains("plume"));
        assert!(body("dwell_cloud_march").contains("dwell_cloud_plumes("));
        // Early-outs: no coverage and no plume crossed take no samples.
        assert!(body("cloud_layer_march").contains("if (cfg.x <= 0.0)"));
        assert!(body("dwell_cloud_plumes").contains("if (span.y <= span.x)"));
    }
    // ----- The volume's lighting (§8.8) -----

    fn lum(c: Rgb) -> f64 {
        0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
    }

    /// A broad cumulus (full cover, the same base noise all round): its profile and weather.
    fn broad() -> (Profile, Weather) {
        let w = Weather {
            cover: 1.0,
            kind: CLOUDS.cloud_types[2].at,
            precipitation: 0.0,
        };
        (type_profile(CLOUDS.cloud_types, w.kind), w)
    }
    const B: [f64; 4] = [0.8, 0.5, 0.5, 0.5];

    /// The broad cloud's base-shape density `y` up the layer (the light samples' shape).
    fn broad_density(y: f64) -> f64 {
        let (p, w) = broad();
        base_shape(B, y, &w, &p) * density_scale(&p, &w, y)
    }

    /// A sample `y` up the broad cloud in the day's colours, lit from a light whose face-local up
    /// part is `rise`, seen at `c` from it, with `steps` light samples.
    fn lit_sample(t: &CloudTunables, y: f64, rise: f64, c: f64, steps: u32) -> Rgb {
        let (p, w) = broad();
        let h = t.volume.top_m - t.volume.base_m;
        let d = broad_density(y);
        let (above, below) = column_cover_m(t, &p, y, w.cover);
        let od = light_depth(t, steps, y, d, above, rise, |s| {
            broad_density(y + rise * s / h)
        });
        let l = sample_light(t, od, c, above, below);
        soft_clip(sample_colour(t, l, t.light.sunlit, t.light.shade))
    }

    /// The broad cloud's top: where its density last passes a tenth.
    fn broad_top() -> f64 {
        (0..1000)
            .map(|i| f64::from(i) / 1000.0)
            .filter(|&y| broad_density(y) > 0.1)
            .fold(0.0, f64::max)
    }

    #[test]
    fn a_clouds_sun_facing_top_is_brighter_than_its_shaded_base() {
        let top = broad_top();
        assert!(top > 0.15, "a cumulus reaches up: {top}");
        for steps in [
            0,
            CLOUDS.tiers.medium.light_steps,
            CLOUDS.tiers.high.light_steps,
        ] {
            let at = |y| lum(lit_sample(&CLOUDS, y, 1.0, 0.0, steps));
            let (hi, lo) = (at(top - 0.01), at(0.04));
            assert!(
                hi > 1.4 * lo,
                "{steps} light samples: top {hi:.3}, base {lo:.3}"
            );
            // The base reads as shade, not as a hole: the sky's and the ground's light reach it.
            assert!(
                lo > 0.25 && hi <= 1.36,
                "{steps}: top {hi:.3}, base {lo:.3}"
            );
        }
    }

    #[test]
    fn the_light_march_darkens_samples_deeper_from_the_sun() {
        let top = broad_top();
        let (p, w) = broad();
        let h = CLOUDS.volume.top_m - CLOUDS.volume.base_m;
        let steps = CLOUDS.tiers.medium.light_steps;
        let mut last = (0.0, f64::INFINITY);
        for i in 0..6 {
            // From just under the top down toward the base, the sun overhead.
            let y = top - 0.01 - f64::from(i) * (top - 0.06) / 5.0;
            let d = broad_density(y);
            let (above, _) = column_cover_m(&CLOUDS, &p, y, w.cover);
            let od = light_depth(&CLOUDS, steps, y, d, above, 1.0, |s| {
                broad_density(y + s / h)
            });
            let b = lum(lit_sample(&CLOUDS, y, 1.0, 0.0, steps));
            assert!(od > last.0, "deeper, more cloud toward the sun at y {y:.2}");
            assert!(b < last.1, "deeper, darker at y {y:.2}: {b:.3}");
            last = (od, b);
        }
        // A light step's sample counts only cloud: the same sample under clear sky toward the sun
        // (a lone puff) is lit by its own half step alone.
        let d = broad_density(0.1);
        let lone = light_depth(&CLOUDS, steps, 0.1, d, 0.0, 1.0, |_| 0.0);
        assert!((lone - 0.5 * CLOUDS.volume.light_step_m * d).abs() < 1e-9);
    }

    #[test]
    fn the_phase_brightens_toward_the_sun() {
        let v = &CLOUDS.volume;
        let (toward, side, away) = (
            cloud_phase(v, 1.0),
            cloud_phase(v, 0.0),
            cloud_phase(v, -1.0),
        );
        assert!(
            toward > 2.0 * side,
            "the silver lining: {toward:.2} vs {side:.2}"
        );
        assert!(away > side, "the back lobe: {away:.2} vs {side:.2}");
        // Thin cloud (little in the way) toward the sun is brighter than seen from the side.
        let thin = |c| sample_light(&CLOUDS, 20.0, c, 0.0, 0.0).sun;
        assert!(thin(0.98) > 1.5 * thin(0.0));
        // By data: no phase strength, even all round; HG is 1 for g 0.
        let even = VolumeTunables {
            phase_strength: 0.0,
            ..*v
        };
        assert_eq!(cloud_phase(&even, 1.0), 1.0);
        assert!((henyey_greenstein(0.3, 0.0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn the_tiers_light_samples_come_from_the_data_and_cap_the_march() {
        let t = &CLOUDS.tiers;
        assert!(
            (4..=MAX_LIGHT_STEPS).contains(&t.medium.light_steps),
            "Medium shadows itself"
        );
        assert!(t.high.light_steps >= t.medium.light_steps);
        assert_eq!(
            (t.low.steps, t.low.light_steps),
            (0, 0),
            "Low keeps the 2D deck"
        );
        for tier in [t.high, t.medium] {
            let s = light_samples(&CLOUDS, tier.light_steps);
            assert_eq!(s.len() as u32, tier.light_steps);
            // Growing steps: Medium's few reach through a cumulus's depth (a tower's top is lit
            // past them as clear).
            let reach: f64 = s.iter().map(|(_, len)| len).sum();
            let cumulus = CLOUDS
                .cloud_types
                .iter()
                .find(|c| c.name == "cumulus")
                .unwrap();
            assert!(
                reach >= cumulus.top[1] * (CLOUDS.volume.top_m - CLOUDS.volume.base_m),
                "{reach}"
            );
        }
        assert_eq!(light_samples(&CLOUDS, 99).len() as u32, MAX_LIGHT_STEPS);
        // A fixture tier by data reaches the shader through the uniforms, and the shader's loop
        // bound is the cap the generator checks.
        let fixture = CloudTunables {
            tiers: VolumeTiers {
                medium: VolumeTier {
                    light_steps: 2,
                    ..t.medium
                },
                ..*t
            },
            ..CLOUDS
        };
        let u = cloud_uniforms(
            &fixture,
            [0.0, 10.0, 0.0],
            FaceFrame::A,
            0.0,
            CloudQuality::Medium,
            0.5,
        );
        assert_eq!((u[2][3].fract() * 8.0).round() as u32, 2);
        assert!(cloud_wgsl(&CLOUDS).contains(&format!(
            "const CLOUD_MAX_LIGHT_STEPS: i32 = {MAX_LIGHT_STEPS};"
        )));
    }

    #[test]
    fn the_march_reuses_its_noise_and_the_shader_lights_by_the_data() {
        let src = cloud_wgsl(&CLOUDS) + &cloud_march_wgsl(0, 0, 1);
        let body = |name: &str| {
            let a = src.find(&format!("fn {name}(")).unwrap();
            let b = src[a..].find("\n}\n").unwrap();
            src[a..a + b].to_owned()
        };
        let march = body("cloud_layer_march");
        // One warp per view sample; the light samples take only the two-octave weather noise.
        assert_eq!(march.matches("cloud_vol_warp(").count(), 1);
        assert_eq!(march.matches("cloud_vol_n(").count(), 2);
        assert!(!march.contains("dwell_cloud_density("));
        // The light leaves the march where it leaves the layer.
        assert!(march.contains("if (yl <= 0.0 || yl >= 1.0)"));
        assert!(body("cloud_vol_light").contains("CLOUD_VOL_BOUNCE_TINT"));
        assert!(src.contains(&format!(
            "const CLOUD_VOL_PHASE_F: f32 = {};",
            f(CLOUDS.volume.phase_forward)
        )));
        // The light's up part is the viewer's face's.
        assert!(body("cloud_shading").contains("let up = face_up(face);"));
    }

    dwell_world::both_faces!(
        fn the_light_is_face_local_so_both_faces_shade_alike(f: FaceFrame) {
            // The sun up and a little across the face, in the world through the face's frame; the
            // march takes its face-local up part (face_rise).
            let local = [0.3, 0.9, 0.2];
            let len = local.iter().map(|c: &f64| c * c).sum::<f64>().sqrt();
            let world = f.vector(local.map(|c| c / len));
            let rise = f.vector(world)[1];
            let steps = CLOUDS.tiers.medium.light_steps;
            let top = broad_top();
            let a = lit_sample(&CLOUDS, top - 0.01, 0.9 / len, 0.0, steps);
            let here = lit_sample(&CLOUDS, top - 0.01, rise, 0.0, steps);
            assert!((lum(a) - lum(here)).abs() < 1e-9);
            assert!(lum(here) > 1.4 * lum(lit_sample(&CLOUDS, 0.04, rise, 0.0, steps)));
        }
    );
}

#[cfg(test)]
mod depth_tests {
    //! BUGS.md #88: the clouds pass against a mountain inside the layer, as the shader does it.
    use super::*;

    const V: VolumeTunables = CLOUDS.volume;
    /// A ray from a valley 100 m above sea level rising 0.4 (face-local up) into the layer.
    const EYE_H: f64 = 100.0;
    const RISE: f64 = 0.4;

    /// The cover a march over `span`'s samples finds in a fixture cloud of density 1 between `cloud`
    /// (m along the ray), and its first cloud sample (-1: none).
    fn fixture_march(span: (f64, f64), jitter: f64, steps: u32, cloud: (f64, f64)) -> (f64, f64) {
        let dt = (span.1 - span.0) / f64::from(steps);
        let mut trans = 1.0;
        let mut first = -1.0;
        for t in march_samples(span, jitter, steps) {
            if t < cloud.0 || t > cloud.1 {
                continue;
            }
            if first < 0.0 {
                first = t;
            }
            trans *= (-V.extinction * dt).exp();
        }
        (1.0 - trans, first)
    }

    /// One texel over `scene` (its pixels' distances along the ray, 0: sky) on frame `frame`:
    /// its cover, its `texel_depth`, and the samples it took.
    fn texel(
        scene: &[f64],
        rise: f64,
        p: [i32; 2],
        frame: u32,
        cloud: (f64, f64),
    ) -> (f64, [f64; 2], Vec<f64>) {
        let reach = V.max_distance_m;
        let r = scene_range(scene, reach);
        let steps = CLOUDS.tiers.medium.steps;
        let jitter = cloud_jitter(&BLUE, 16, p, frame);
        let Some(span) = layer_span(&V, rise, EYE_H, march_end(r, reach)) else {
            return (0.0, texel_depth(r), vec![]);
        };
        let (cover, _) = fixture_march(span, jitter, steps, cloud);
        (cover, texel_depth(r), march_samples(span, jitter, steps))
    }

    static BLUE: std::sync::LazyLock<Vec<u8>> =
        std::sync::LazyLock::new(|| crate::cloud_noise::blue_noise(16));

    /// The rock 4 km out, inside the layer (1.7 km up), its slope across the texel 3.99–4.01 km.
    const ROCK: [f64; 4] = [3990.0, 4000.0, 4005.0, 4010.0];

    #[test]
    fn the_march_stops_at_the_scene_so_no_cloud_behind_the_rock_draws() {
        // A cloud wholly behind the rock: nothing of it on any frame of the jitter's sequence.
        for frame in 0..16 {
            let (cover, _, samples) = texel(&ROCK, RISE, [7, 3], frame, (4100.0, 6000.0));
            assert!(
                samples.iter().all(|&t| t <= 4010.0),
                "frame {frame}: {samples:?}"
            );
            let shown = upsample_cover(
                &V,
                4000.0,
                &[(
                    cover,
                    texel(&ROCK, RISE, [7, 3], frame, (4100.0, 6000.0)).1,
                    1.0,
                )],
            );
            assert_eq!(
                shown, 0.0,
                "frame {frame}: cloud behind the rock drew {shown}"
            );
        }
    }

    #[test]
    fn a_cloud_round_the_rock_draws_in_front_of_it_on_every_frame_alike() {
        // A cloud from 100 m in front of the rock to far behind it: the part in front draws, the cut
        // doesn't flip with the jitter (the old cut by the jittered first cloud did).
        let cloud = (3900.0, 6000.0);
        let mut seen = vec![];
        for frame in 0..16 {
            let (cover, rec, _) = texel(&ROCK, RISE, [7, 3], frame, cloud);
            for e in ROCK {
                let w = upsample_weight(&V, e, rec);
                seen.push(w);
                assert!(cover > 0.0, "frame {frame}: the cloud in front is gone");
            }
        }
        assert!(
            seen.iter().all(|&w| w == seen[0]),
            "the cut flips frame to frame: {seen:?}"
        );
        assert_eq!(seen[0], 1.0);
        // The cover is the cloud in front only: ~100 m of it, not the 2 km behind.
        let (cover, _, _) = texel(&ROCK, RISE, [7, 3], 0, cloud);
        assert!(cover < 1.0 - (-V.extinction * 300.0).exp(), "{cover}");
    }

    #[test]
    fn at_a_silhouette_rock_and_sky_pixels_take_their_own_texels() {
        let reach = V.max_distance_m;
        let cloud = (3500.0, 9000.0);
        let rock = texel(&ROCK, RISE, [7, 3], 0, cloud);
        let edge = texel(&[4000.0, 0.0, 4002.0, 0.0], RISE, [8, 3], 0, cloud);
        let sky = texel(&[0.0; 4], RISE, [9, 3], 0, cloud);
        assert!(
            sky.0 > rock.0,
            "the sky's texel holds the cloud behind the rock too"
        );
        // A rock pixel between the rock's texel and the edge's: the rock's cloud only.
        let at_rock = upsample_cover(&V, 4001.0, &[(rock.0, rock.1, 0.5), (edge.0, edge.1, 0.5)]);
        assert!((at_rock - rock.0).abs() < 1e-3, "{at_rock} vs {}", rock.0);
        // A sky pixel between the edge's texel and the rock's: the sky's cloud, not the rock's.
        let at_sky = upsample_cover(&V, reach, &[(edge.0, edge.1, 0.5), (rock.0, rock.1, 0.5)]);
        assert!((at_sky - edge.0).abs() < 1e-3, "{at_sky} vs {}", edge.0);
        // Far from any rock the sky is the bilinear filter's.
        let open = upsample_cover(&V, reach, &[(sky.0, sky.1, 0.25), (edge.0, edge.1, 0.75)]);
        assert!((open - (0.25 * sky.0 + 0.75 * edge.0)).abs() < 1e-9);
    }

    #[test]
    fn the_shader_twins_are_valid_and_every_tier_fits_the_depth_taps() {
        for t in [CLOUDS.tiers.high, CLOUDS.tiers.medium, CLOUDS.tiers.low] {
            assert!(t.divisor <= MAX_DIVISOR, "{t:?}");
        }
        let src = format!(
            "{}{}{}",
            dwell_world::FaceFrame::wgsl(),
            cloud_wgsl(&CLOUDS),
            cloud_march_wgsl(0, 0, 1)
        );
        let module = naga::front::wgsl::parse_str(&src).expect("WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
        assert!(src.contains("fn dwell_cloud_upsample_weight(e: f32, rec: vec2f) -> f32"));
        assert!(src.contains(&format!(
            "const CLOUD_VOL_DEPTH_BAND: f32 = {};",
            f(V.depth_band)
        )));
    }

    dwell_world::both_faces!(
        fn the_march_stops_at_the_scene_on_both_faces(f: FaceFrame) {
            // The ray in the world through the face's frame; the march takes its face-local rise.
            let local = [0.0, RISE, (1.0 - RISE * RISE).sqrt()];
            let rise = f.vector(f.vector(local))[1];
            for frame in 0..16 {
                let (_, _, samples) = texel(&ROCK, rise, [2, 5], frame, (0.0, 1e9));
                assert!(!samples.is_empty() && samples.iter().all(|&t| t <= 4010.0));
            }
        }
    );
}

#[cfg(test)]
mod shape_tests {
    //! docs/CLOUDS.md §9: the shape model on the CPU over the baked noise (small sizes, quick).
    use super::*;
    use std::sync::LazyLock;

    static NOISE: LazyLock<ShapeNoise> =
        LazyLock::new(|| ShapeNoise::bake(&CLOUDS, Some((32, 16))));
    const H: f64 = CLOUDS.volume.top_m - CLOUDS.volume.base_m;

    fn kind(name: &str) -> f64 {
        CLOUDS
            .cloud_types
            .iter()
            .find(|c| c.name == name)
            .unwrap()
            .at
    }

    /// The density `y` up the layer at x, z (m) in a round cloud of radius 1.5 km about the origin
    /// (its cover falling from 1 at the centre to 0 at the rim) of type `k`.
    fn round_cloud(t: &CloudTunables, k: f64, x: f64, z: f64, y: f64, detail: bool) -> f64 {
        let r = (x * x + z * z).sqrt();
        let w = Weather {
            cover: (1.0 - r / 1500.0).clamp(0.0, 1.0),
            kind: k,
            precipitation: 0.0,
        };
        let h = t.volume.base_m + y * H;
        NOISE.density(t, shape_point(t, x, h, z), y, &w, [0.0, 0.0], detail)
    }

    /// The share of the cloud's footprint that has cloud `y` up (a horizontal slice's width).
    fn width(t: &CloudTunables, k: f64, y: f64) -> f64 {
        let mut n = 0;
        let mut hit = 0;
        for i in -40..=40 {
            for j in -40..=40 {
                let (x, z) = (f64::from(i) * 37.0, f64::from(j) * 37.0);
                if x * x + z * z > 1500.0 * 1500.0 {
                    continue;
                }
                n += 1;
                if round_cloud(t, k, x, z, y, true) > 0.05 {
                    hit += 1;
                }
            }
        }
        f64::from(hit) / f64::from(n)
    }

    #[test]
    fn a_cumulus_is_flat_based_and_rounded_at_its_top() {
        let k = kind("cumulus");
        let ys: Vec<f64> = (1..60).map(|i| f64::from(i) * 0.008).collect();
        let w: Vec<f64> = ys.iter().map(|&y| width(&CLOUDS, k, y)).collect();
        let (imax, max) = w
            .iter()
            .copied()
            .enumerate()
            .fold((0, 0.0), |a, (i, v)| if v > a.1 { (i, v) } else { a });
        assert!(max > 0.2, "the cloud has a body: {max:.2}");
        let p = type_profile(CLOUDS.cloud_types, k);
        // Flat base: the cloud is near its widest just over the base's short fade-in, and its
        // widest is low in the cloud.
        let just_over = ys.iter().position(|&y| y >= p.base[1] + 0.01).unwrap();
        assert!(
            w[just_over] > 0.75 * max,
            "base {:.2} of {max:.2}",
            w[just_over]
        );
        assert!(ys[imax] < 0.5 * p.top[1], "widest at y {:.2}", ys[imax]);
        // Rounded top: above the widest it narrows (to under half near the top), never widening
        // by more than the noise's few percent.
        let top = ys
            .iter()
            .zip(&w)
            .filter(|(_, v)| **v > 0.0)
            .map(|(y, _)| *y)
            .fold(0.0, f64::max);
        assert!(top > p.top[0] && top <= p.top[1], "top {top:.2}");
        for i in imax + 1..w.len() {
            assert!(
                w[i] <= w[i - 1] + 0.05 * max,
                "y {:.2}: {:.2} after {:.2}",
                ys[i],
                w[i],
                w[i - 1]
            );
        }
        let near_top = ys.iter().position(|&y| y >= 0.85 * top).unwrap();
        assert!(
            w[near_top] < 0.5 * max,
            "near the top {:.2} of {max:.2}",
            w[near_top]
        );
    }

    #[test]
    fn a_cumulonimbus_spreads_into_an_anvil() {
        let k = kind("cumulonimbus");
        let p = type_profile(CLOUDS.cloud_types, k);
        let (trunk, anvil) = (
            width(&CLOUDS, k, 0.5 * p.top[0]),
            width(&CLOUDS, k, p.top[0] + 0.02),
        );
        assert!(anvil > trunk, "anvil {anvil:.2} over trunk {trunk:.2}");
        // And it towers where a cumulus has long ended.
        assert!(width(&CLOUDS, k, 0.7) > 0.05);
        assert_eq!(width(&CLOUDS, kind("cumulus"), 0.7), 0.0);
    }

    #[test]
    fn no_coverage_is_no_cloud() {
        for i in 0..200 {
            let n = f64::from(i) / 199.0;
            let w = weather(&CLOUDS, n, 0.5, 0.0);
            assert_eq!(w.cover, 0.0);
            for y in [0.02, 0.1, 0.3, 0.8] {
                let pm = shape_point(
                    &CLOUDS,
                    f64::from(i) * 91.0,
                    CLOUDS.volume.base_m + y * H,
                    17.0,
                );
                assert_eq!(NOISE.density(&CLOUDS, pm, y, &w, [0.0, 0.0], true), 0.0);
            }
        }
        // Under the cut at the shipped coverage, none either; past it, cover rises to 1.
        let cut = 1.0 - CLOUDS.shape.coverage;
        assert_eq!(
            weather(&CLOUDS, cut - 0.01, 0.5, CLOUDS.shape.coverage).cover,
            0.0
        );
        assert_eq!(
            weather(
                &CLOUDS,
                cut + CLOUDS.form.coverage_softness,
                0.5,
                CLOUDS.shape.coverage
            )
            .cover,
            1.0
        );
    }

    #[test]
    fn the_detail_only_erodes_the_edges_and_the_core_stays() {
        let mut eroded = 0;
        for i in 0..400 {
            let x = f64::from(i % 20) * 61.0 - 600.0;
            let z = f64::from(i / 20) * 61.0 - 600.0;
            let y = 0.02 + f64::from(i % 7) * 0.03;
            let base = round_cloud(&CLOUDS, kind("cumulus"), x, z, y, false);
            let full = round_cloud(&CLOUDS, kind("cumulus"), x, z, y, true);
            assert!(full <= base + 1e-12, "the detail only takes away");
            if base > 0.0 && full < base {
                eroded += 1;
            }
        }
        assert!(eroded > 20, "edges eroded: {eroded}");
        // A core (base shape 1) keeps its density whatever the detail says, wispy or billowy.
        for hf in [0.0, 0.3, 0.7, 1.0] {
            for y in [0.01, 0.5] {
                assert_eq!(erode(&CLOUDS, 1.0, hf, y), 1.0);
                assert!(erode(&CLOUDS, 0.3, hf, y) <= 0.3);
            }
        }
        // Wispy bases, billowy tops: at the base the fBm's peaks erode most, at the top its gaps.
        assert!(erode(&CLOUDS, 0.5, 1.0, 0.0) < erode(&CLOUDS, 0.5, 0.0, 0.0));
        assert!(erode(&CLOUDS, 0.5, 0.0, 0.9) < erode(&CLOUDS, 0.5, 1.0, 0.9));
    }

    #[test]
    fn the_shape_tiles_with_its_textures() {
        // A whole base tile over (every tile divides it) is the same cloud: no seam where the
        // shader's coordinates wrap.
        let tile = CLOUDS.form.base_tile_m;
        for t in [CLOUDS.form.detail_tile_m, CLOUDS.form.curl_tile_m] {
            assert_eq!((tile / t).fract(), 0.0);
        }
        assert_eq!((CLOUDS.shape.wrap_m / tile).fract(), 0.0);
        let w = Weather {
            cover: 0.8,
            kind: kind("cumulus"),
            precipitation: 0.0,
        };
        for i in 0..50 {
            let pm = [
                f64::from(i) * 83.0,
                CLOUDS.volume.base_m + 300.0,
                f64::from(i) * 41.0,
            ];
            let a = NOISE.density(&CLOUDS, pm, 0.1, &w, [5.0, 9.0], true);
            let b = NOISE.density(
                &CLOUDS,
                [pm[0] + tile, pm[1], pm[2] - 2.0 * tile],
                0.1,
                &w,
                [5.0, 9.0],
                true,
            );
            assert!((a - b).abs() < 1e-9, "{a} {b}");
        }
    }

    /// A fixture cloud type (a tower), added by data alone.
    const TOWER: CloudType = CloudType {
        name: "test:tower",
        at: 1.0,
        base: [0.0, 0.05],
        top: [0.9, 1.0],
        anvil: 0.0,
        density: 2.0,
        darkening: 0.5,
    };
    const WITH_TOWER: CloudTunables = CloudTunables {
        cloud_types: &[
            CloudType { at: 0.0, ..TOWER },
            CloudType {
                name: "test:puff",
                at: 0.5,
                top: [0.05, 0.1],
                ..TOWER
            },
            TOWER,
        ],
        ..CLOUDS
    };

    #[test]
    fn a_fixture_cloud_type_by_data_alone_shapes_the_clouds_and_the_shader() {
        let b = [0.8, 0.5, 0.5, 0.5];
        let w = Weather {
            cover: 1.0,
            kind: 1.0,
            precipitation: 0.0,
        };
        let p = type_profile(WITH_TOWER.cloud_types, 1.0);
        assert_eq!(p.top, TOWER.top);
        assert!(
            base_shape(b, 0.85, &w, &p) > 0.5,
            "the tower reaches 85 % up"
        );
        let shipped = type_profile(CLOUDS.cloud_types, kind("cumulus"));
        assert_eq!(base_shape(b, 0.85, &w, &shipped), 0.0);
        // Between types the profile is mixed: halfway from the puff to the tower.
        let mid = type_profile(WITH_TOWER.cloud_types, 0.75);
        assert!((mid.top[1] - 0.55).abs() < 1e-9);
        // The shader's chain is generated from the data and validates.
        let src = cloud_wgsl(&WITH_TOWER);
        assert!(src.contains("test:puff → test:tower"));
        assert!(src.contains(&format!("return {};", profile_wgsl(&TOWER))));
        let full = format!(
            "{}{src}{}",
            dwell_world::FaceFrame::wgsl(),
            cloud_march_wgsl(0, 0, 1)
        );
        let module = naga::front::wgsl::parse_str(&full).expect("WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
        // One type alone works too.
        let one = cloud_wgsl(&CloudTunables {
            cloud_types: &[TOWER],
            ..CLOUDS
        });
        assert!(one.contains("fn cloud_type_profile("));
    }

    #[test]
    fn the_march_samples_the_textures_and_the_light_takes_the_cheap_base_shape() {
        let src = cloud_wgsl(&CLOUDS) + &cloud_march_wgsl(0, 0, 1);
        let a = src.find("fn cloud_layer_march(").unwrap();
        let body = &src[a..a + src[a..].find("\n}\n").unwrap()];
        // Per view sample: one base fetch and the detail (curl + fBm); per light sample, one base
        // fetch and no detail.
        assert_eq!(body.matches("cloud_base_at(").count(), 2);
        assert_eq!(body.matches("cloud_detail_fbm(").count(), 1);
        let light = &body[body.find("for (var j = 0;").unwrap()..];
        assert!(!light.contains("cloud_detail_fbm(") && !light.contains("cloud_erode("));
        // The deck-only part declares no texture: the modules that only draw the deck bind none.
        assert!(!cloud_wgsl(&CLOUDS).contains("texture_3d"));
    }

    dwell_world::both_faces!(
        fn the_shape_is_face_local_so_both_faces_have_the_same_clouds(f: FaceFrame) {
            // An eye 300 m up on the face: its height over the face's sea level is the same on
            // both, so a sample the same face-local way along the ray is the same cloud's.
            let local = [120.0, dwell_protocol::world::SEA_LEVEL_M + 300.0, -40.0];
            let world = f.point(local);
            let u = cloud_uniforms(&CLOUDS, world, f, 0.0, CloudQuality::Medium, 0.5);
            let a = cloud_uniforms(&CLOUDS, local, FaceFrame::A, 0.0, CloudQuality::Medium, 0.5);
            assert_eq!(
                u[1][3], a[1][3],
                "the deck's height over the eye is face-local"
            );
            let eye_h = CLOUDS.deck.altitude_m - f64::from(u[1][3]);
            let h = eye_h + 1500.0;
            let y = (h - CLOUDS.volume.base_m) / H;
            let w = Weather {
                cover: 0.9,
                kind: kind("cumulus"),
                precipitation: 0.0,
            };
            let d = NOISE.density(
                &CLOUDS,
                shape_point(&CLOUDS, 300.0, h, 200.0),
                y,
                &w,
                [0.0, 0.0],
                true,
            );
            let ea = CLOUDS.deck.altitude_m - f64::from(a[1][3]) + 1500.0;
            let da = NOISE.density(
                &CLOUDS,
                shape_point(&CLOUDS, 300.0, ea, 200.0),
                y,
                &w,
                [0.0, 0.0],
                true,
            );
            assert_eq!(d, da);
        }
    );

    // ----- BUGS.md #90: dither and pixelation at cloud level -----

    #[test]
    fn near_the_layer_a_tier_takes_its_near_steps_and_resolution() {
        let v = &CLOUDS.volume;
        let m = CLOUDS.tiers.medium;
        assert!(near_layer(v, v.base_m + 10.0, false), "inside");
        assert!(
            near_layer(v, v.base_m - 0.9 * v.near_m, false),
            "just under"
        );
        assert!(!near_layer(v, v.base_m - 1.2 * v.near_m, false));
        assert!(
            near_layer(v, v.base_m - 1.2 * v.near_m, true),
            "the band to leave it"
        );
        assert!(!near_layer(v, v.top_m + 2.0 * v.near_m, true));
        let near = m.at(true);
        assert_eq!((near.steps, near.divisor), (m.near_steps, m.near_divisor));
        assert!(near.steps > m.steps && near.divisor < m.divisor);
        assert_eq!(m.at(false), m);
    }

    #[test]
    fn the_jitter_is_blue_noise_and_the_resolve_is_stable_under_it_as_the_eye_moves() {
        let blue = crate::cloud_noise::blue_noise(16);
        // A fixture texel: a cloud edge it marches with few steps, so its raw cover depends on
        // where the jittered steps land; the eye moving through the layer each frame (where the
        // old pass dropped the history: the raw march showed).
        let march = |jitter: f64| {
            let steps = 4;
            let mut trans: f64 = 1.0;
            for i in 0..steps {
                let t = (f64::from(i) + jitter) / f64::from(steps);
                if (0.3..0.68).contains(&t) {
                    trans *= (-1.4_f64).exp();
                }
            }
            1.0 - trans
        };
        // A row of 8 texels and their neighbours; frames 16..80 after a warm-up.
        let (mut raw_var, mut res_var) = (0.0, 0.0);
        for x in 0..8 {
            let mut hist = [0.0; 4];
            let (mut raw, mut res) = (vec![], vec![]);
            for frame in 0..80u32 {
                let at = |dx: i32, dy: i32| {
                    let c = march(cloud_jitter(&blue, 16, [x + dx, 3 + dy], frame));
                    [c, c, c, c]
                };
                let own = at(0, 0);
                let nb: Vec<[f64; 4]> = (-1..=1)
                    .flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)))
                    .map(|(dx, dy)| at(dx, dy))
                    .collect();
                let bx = neighbourhood_box(own, &nb, 1.5);
                hist = resolve(own, bx, hist, 1.0, CLOUDS.tiers.medium.history_blend);
                if frame >= 16 {
                    raw.push(own[3]);
                    res.push(hist[3]);
                }
            }
            let var = |v: &[f64]| {
                let m = v.iter().sum::<f64>() / v.len() as f64;
                v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len() as f64
            };
            raw_var += var(&raw);
            res_var += var(&res);
        }
        assert!(
            raw_var > 0.01,
            "the fixture's raw march flickers: {raw_var:.4}"
        );
        assert!(
            res_var < 0.25 * raw_var,
            "resolved {res_var:.4} vs raw {raw_var:.4}"
        );
        // Neighbouring texels' jitter is far apart (blue): over the 16-frame cycle every texel
        // takes 16 distinct offsets spread over 0–1.
        let mut seen: Vec<f64> = (0..16)
            .map(|f| cloud_jitter(&blue, 16, [2, 5], f))
            .collect();
        seen.sort_by(f64::total_cmp);
        assert!(seen.windows(2).all(|w| w[1] - w[0] > 0.01));
    }

    #[test]
    fn the_resolve_clamps_stale_history_and_the_upsample_keeps_a_silhouette_sharp() {
        // History from before the eye moved (a cloud now gone) is clamped into this frame's
        // neighbourhood instead of ghosting, and kept when it agrees.
        let clear = [[0.0; 4]; 9];
        let bx = neighbourhood_box([0.0; 4], &clear, 1.5);
        assert_eq!(resolve([0.0; 4], bx, [0.8; 4], 1.0, 0.2), [0.0; 4]);
        let cloudy = [[0.5; 4]; 9];
        let bx = neighbourhood_box([0.5; 4], &cloudy, 1.5);
        assert_eq!(resolve([0.5; 4], bx, [0.5; 4], 1.0, 0.2), [0.5; 4]);
        // The composite's depth-aware upsample at a silhouette (a peak 4 km out against the sky):
        // the rock pixel takes only the rock texels' cloud and the sky pixel only the sky's, each
        // within 1 % of its side's, whatever the bilinear shares say.
        let v = &CLOUDS.volume;
        let reach = v.max_distance_m;
        let rock = [4000.0, 4010.0];
        let sky = [reach, reach];
        for share in [0.1, 0.5, 0.9] {
            let texels = [(0.2, rock, share), (0.9, sky, 1.0 - share)];
            assert!((upsample_cover(v, 4005.0, &texels) - 0.2).abs() < 0.01);
            assert!((upsample_cover(v, reach, &texels) - 0.9).abs() < 0.01);
        }
    }
}
