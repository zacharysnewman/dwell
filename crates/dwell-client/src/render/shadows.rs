//! Sun and moon shadows by cascaded shadow maps (docs/DAY_NIGHT.md §10; the lighting's second
//! layer). Each frame the light that casts (dwell-look `shadows::caster`: the sun over the
//! viewer's face, else the moon) gets up to four orthographic depth maps around the camera, one per
//! slice of the view's depth. Each cascade is fitted to its slice's bounding sphere (whose radius
//! doesn't change as the camera turns) and its origin is snapped to whole texels in light space (so
//! moving doesn't make the edges shimmer). The scene's shaders (`SHADOW_WGSL`) pick a fragment's
//! cascade, look it up with a soft PCF, normal-offset and biased, and the result multiplies only the
//! direct sun and moon term (`lambert_lit`'s `shadow`); past the last cascade it fades to the first
//! layer's stand-in.
//!
//! Everything is camera-relative, as the rest of the renderer: a cascade's matrix maps a point
//! relative to the eye to its map, and only the snapping reads world coordinates (in f64).

use dwell_look::shadows::{Caster, MAX_CASCADES, ShadowQuality, ShadowTier, ShadowTunables};

use super::camera::{Mat4, V3, cross, dot, normalize};

/// The Shadows setting an adapter draws: a CPU adapter draws Low at most (its default; every map
/// is the CPU's time there), a GPU the setting.
pub fn shadows_for(device_type: wgpu::DeviceType, quality: ShadowQuality) -> ShadowQuality {
    if device_type == wgpu::DeviceType::Cpu && quality == ShadowQuality::High {
        ShadowQuality::Low
    } else {
        quality
    }
}

/// The shadow map's format.
pub const SHADOW_DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The light's axes (world): `z` toward the light, `x` and `y` across its maps. They follow the
/// light alone (never the camera), so a cascade's texel grid stays put in the world as the camera
/// moves and turns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightBasis {
    pub x: V3,
    pub y: V3,
    pub z: V3,
}

/// The light's axes for a light toward `dir`, its maps' y as near `reference` as it can be (the
/// sky's turning axis: the sun's path turns about it, so the maps turn smoothly with the sun).
pub fn light_basis(dir: V3, reference: V3) -> LightBasis {
    let z = normalize(dir);
    let mut r = reference;
    if dot(normalize(r), z).abs() > 0.99 {
        r = [1.0, 0.0, 0.0];
        if dot(r, z).abs() > 0.99 {
            r = [0.0, 0.0, 1.0];
        }
    }
    let x = normalize(cross(r, z));
    let y = cross(z, x);
    LightBasis { x, y, z }
}

/// The view depths the cascades split at (`count + 1` of them, from `near` to `far`): the
/// practical split, `lambda` of the way from even to logarithmic.
pub fn splits(count: usize, near: f64, far: f64, lambda: f64) -> Vec<f64> {
    (0..=count)
        .map(|i| {
            let t = i as f64 / count as f64;
            let log = near * (far / near).powf(t);
            let even = near + (far - near) * t;
            lambda * log + (1.0 - lambda) * even
        })
        .collect()
}

/// The smallest sphere around the slice of the view between depths `d0` and `d1` (half-angle
/// tangents `tan_x`, `tan_y`): its centre's view depth and its radius. It depends on the slice's
/// shape alone, so turning the camera leaves it the same (the stable fit).
pub fn slice_sphere(d0: f64, d1: f64, tan_x: f64, tan_y: f64) -> (f64, f64) {
    let k2 = tan_x * tan_x + tan_y * tan_y;
    // The centre on the axis equidistant from the near and far corners, unless that is past the
    // far face (a wide slice): then the far face's centre, which holds the near corners too.
    let z = ((d1 * d1 - d0 * d0) * (1.0 + k2)) / (2.0 * (d1 - d0));
    if z >= d1 {
        (d1, d1 * k2.sqrt())
    } else {
        let z = z.max(d0);
        (z, ((d1 - z).powi(2) + d1 * d1 * k2).sqrt())
    }
}

/// One cascade this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cascade {
    /// The slice's view depths.
    pub near: f64,
    pub far: f64,
    /// Its box's half-size across the light (m): the slice's sphere's radius, rounded up.
    pub radius: f64,
    /// Its box's centre (world), snapped to whole texels across the light.
    pub origin: V3,
    /// The box's centre relative to the eye.
    pub origin_rel: V3,
    /// A texel's size (m).
    pub texel_m: f64,
    /// The box's depth along the light (m): its sphere and the casters' reach above it.
    pub depth_m: f64,
    pub basis: LightBasis,
    /// Eye-relative points to the map: xy in [−1, 1] (clip space), z in [0, 1] from the light.
    pub view_proj: Mat4,
    /// What its slice of the view covers in light space (relative to `origin`, m): x and y's
    /// ranges, and the least height toward the light. Only casters over it can shade the slice, so
    /// only those are drawn (the sphere's square, which keeps the map stable, is much larger).
    pub footprint: [f64; 5],
}

/// The eye-relative point → map transform of a box centred `origin_rel` from the eye: x and y
/// over ±`radius` across the light, depth 0 at its face toward the light (`radius + reach` up)
/// and 1 at its far face (column-major).
fn light_matrix(basis: LightBasis, origin_rel: V3, radius: f64, reach: f64) -> Mat4 {
    let depth_m = 2.0 * radius + reach;
    let rows: [[f64; 4]; 3] = [
        {
            let a = basis.x.map(|c| c / radius);
            [a[0], a[1], a[2], -dot(origin_rel, basis.x) / radius]
        },
        {
            let a = basis.y.map(|c| c / radius);
            [a[0], a[1], a[2], -dot(origin_rel, basis.y) / radius]
        },
        {
            let a = basis.z.map(|c| -c / depth_m);
            [
                a[0],
                a[1],
                a[2],
                (radius + reach + dot(origin_rel, basis.z)) / depth_m,
            ]
        },
    ];
    let mut m = [[0.0f32; 4]; 4];
    for (c, col) in m.iter_mut().enumerate() {
        for (r, row) in rows.iter().enumerate() {
            col[r] = row[c] as f32;
        }
        col[3] = if c == 3 { 1.0 } else { 0.0 };
    }
    m
}

/// What a frame's cascades are fitted from.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// The eye (world).
    pub eye: V3,
    /// The camera's view axis, right and up (unit, world).
    pub forward: V3,
    pub right: V3,
    pub up: V3,
    /// The view's half-angle tangents.
    pub tan_x: f64,
    pub tan_y: f64,
}

/// Fits a cascade to the slice `[near, far]` of `view`: around the slice's sphere (its radius
/// rounded up to `radius_step`), its centre snapped to the map's texels across the light, deep
/// enough to hold casters `reach` metres above it toward the light.
pub fn fit(
    view: &View,
    basis: LightBasis,
    near: f64,
    far: f64,
    resolution: u32,
    radius_step: f64,
    reach: f64,
) -> Cascade {
    let (centre_depth, r) = slice_sphere(near, far, view.tan_x, view.tan_y);
    let radius = (r / radius_step).ceil().max(1.0) * radius_step;
    let texel_m = 2.0 * radius / f64::from(resolution);
    let centre = std::array::from_fn::<f64, 3, _>(|k| view.eye[k] + view.forward[k] * centre_depth);
    let snap = |axis: V3| (dot(centre, axis) / texel_m).round() * texel_m;
    let (u, v, w) = (snap(basis.x), snap(basis.y), dot(centre, basis.z));
    let origin = std::array::from_fn(|k| basis.x[k] * u + basis.y[k] * v + basis.z[k] * w);
    let origin_rel: V3 = std::array::from_fn(|k| origin[k] - view.eye[k]);
    let depth_m = 2.0 * radius + reach;
    let view_proj = light_matrix(basis, origin_rel, radius, reach);
    // The slice's corners in light space, widened by a few texels for the filter and the
    // normal offset.
    let margin = 4.0 * texel_m;
    let mut footprint = [
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
    ];
    for d in [near, far] {
        for sx in [-1.0, 1.0] {
            for sy in [-1.0, 1.0] {
                let c: V3 = std::array::from_fn(|k| {
                    view.forward[k] * d
                        + view.right[k] * sx * view.tan_x * d
                        + view.up[k] * sy * view.tan_y * d
                        - origin_rel[k]
                });
                let (x, y, z) = (dot(c, basis.x), dot(c, basis.y), dot(c, basis.z));
                footprint[0] = footprint[0].min(x - margin);
                footprint[1] = footprint[1].max(x + margin);
                footprint[2] = footprint[2].min(y - margin);
                footprint[3] = footprint[3].max(y + margin);
                footprint[4] = footprint[4].min(z - margin);
            }
        }
    }
    Cascade {
        near,
        far,
        radius,
        origin,
        origin_rel,
        texel_m,
        depth_m,
        basis,
        view_proj,
        footprint,
    }
}

impl Cascade {
    /// The cascade as drawn earlier, seen from `eye` now: the same box in the world (and so the
    /// same map), its matrix made again for the eye-relative points of this frame.
    pub fn rebased(&self, eye: V3) -> Self {
        let origin_rel: V3 = std::array::from_fn(|k| self.origin[k] - eye[k]);
        let reach = self.depth_m - 2.0 * self.radius;
        Self {
            origin_rel,
            view_proj: light_matrix(self.basis, origin_rel, self.radius, reach),
            ..*self
        }
    }

    /// Where an eye-relative point falls in the map: clip x, y and the depth (f64; the shader's
    /// matrix in full precision).
    pub fn project(&self, rel: V3) -> V3 {
        let d: V3 = std::array::from_fn(|k| rel[k] - self.origin_rel[k]);
        let reach = self.depth_m - 2.0 * self.radius;
        [
            dot(d, self.basis.x) / self.radius,
            dot(d, self.basis.y) / self.radius,
            (self.radius + reach - dot(d, self.basis.z)) / self.depth_m,
        ]
    }

    /// Whether a box (its centre relative to the eye and half-size, axis-aligned) can cast into
    /// this cascade: it overlaps the cascade's box, which reaches up toward the light, over the
    /// slice's footprint.
    pub fn touches(&self, centre_rel: V3, half: V3) -> bool {
        let d: V3 = std::array::from_fn(|k| centre_rel[k] - self.origin_rel[k]);
        let extent = |a: V3| (0..3).map(|k| a[k].abs() * half[k]).sum::<f64>();
        let reach = self.depth_m - 2.0 * self.radius;
        let (x, y, z) = (
            dot(d, self.basis.x),
            dot(d, self.basis.y),
            dot(d, self.basis.z),
        );
        let (ex, ey, ez) = (
            extent(self.basis.x),
            extent(self.basis.y),
            extent(self.basis.z),
        );
        let f = &self.footprint;
        x.abs() <= self.radius + ex
            && y.abs() <= self.radius + ey
            && x + ex >= f[0]
            && x - ex <= f[1]
            && y + ey >= f[2]
            && y - ey <= f[3]
            && z + ez >= f[4].max(-self.radius)
            && z <= self.radius + reach + ez
    }
}

/// This frame's shadows: the light, the tier and its cascades (none: no shadows drawn).
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub caster: Caster,
    pub tier: ShadowTier,
    pub cascades: Vec<Cascade>,
    /// The shadow distance (m of view depth) and where its fade to the stand-in starts.
    pub distance: f64,
    pub fade_start: f64,
}

/// The frame's shadow plan: for `quality` (already `shadows_for` the adapter), the light `caster`
/// (none when neither light is up), `view`, the sky's axis (the light's map axes) and the
/// spyglass's `magnification` (the shadow distance grows with it, so the zoomed view has shadows
/// where it looks, up to `spyglassMaxM`).
pub fn plan(
    t: &ShadowTunables,
    quality: ShadowQuality,
    caster: Option<Caster>,
    view: &View,
    sky_axis: V3,
    magnification: f64,
) -> Option<Plan> {
    let tier = *t.tier(quality)?;
    let caster = caster.filter(|c| c.strength > 1e-3)?;
    let c = &t.cascades;
    let distance =
        (tier.distance_m * magnification.max(1.0)).min(c.spyglass_max_m.max(tier.distance_m));
    let count = (tier.cascades as usize).clamp(1, MAX_CASCADES);
    let basis = light_basis(caster.dir, sky_axis);
    let depths = splits(
        count,
        c.near_m.min(distance * 0.5),
        distance,
        tier.split_lambda,
    );
    // The first slice reaches back to the eye (the near plane): what is under your feet.
    let cascades = (0..count)
        .map(|i| {
            let near = if i == 0 { 0.0 } else { depths[i] };
            fit(
                view,
                basis,
                near,
                depths[i + 1],
                tier.resolution,
                c.radius_step_m,
                c.caster_reach_m,
            )
        })
        .collect();
    Some(Plan {
        caster,
        tier,
        cascades,
        distance,
        fade_start: distance * (1.0 - c.edge_fade),
    })
}

/// Which cascades a frame draws, and the cascades its receivers use. The first (nearest, the
/// sharpest and the one the player's own shadow is in) is drawn every frame; the others take turns,
/// one a frame (`refreshFrames`), and between their turns are read as last drawn (`rebased` to this frame's eye: the
/// map holds the same world box). That bounds a frame's casters to two cascades' worth; a far
/// cascade's moving casters (other players) lag by its turn's few frames. Everything is drawn when
/// there was no earlier frame with these maps, the light changed (sun ↔ moon, the tier) or `all`.
pub fn stagger(
    prev: Option<&Plan>,
    plan: &Plan,
    eye: V3,
    frame: u64,
    all: bool,
) -> (Vec<Cascade>, u8) {
    let n = plan.cascades.len();
    let every = ((1u16 << n) - 1) as u8;
    let Some(prev) = prev.filter(|p| {
        !all && p.cascades.len() == n && p.tier == plan.tier && p.caster.moon == plan.caster.moon
    }) else {
        return (plan.cascades.clone(), every);
    };
    // The far cascades' turns: one slot a frame over `refreshFrames` (at least n − 1) frames.
    let period = u64::from(plan.tier.refresh_frames).max(n as u64 - 1).max(1);
    let slot = frame % period;
    let mask = if n <= 1 {
        every
    } else if slot < n as u64 - 1 {
        1 | 1 << (1 + slot as u8)
    } else {
        1
    };
    let used = (0..n)
        .map(|i| {
            if mask & (1 << i) != 0 {
                plan.cascades[i]
            } else {
                prev.cascades[i].rebased(eye)
            }
        })
        .collect();
    (used, mask)
}

/// Floats in the shadow uniform block (`Shadows` in `SHADOW_WGSL`).
pub const UNIFORM_FLOATS: usize = MAX_CASCADES * 16 + MAX_CASCADES * 4 + 8;

/// The shadow uniform block: each cascade's matrix; per cascade its texel (m), the normal offset
/// (m), the compare's bias (map depth) and the filter's radius (texels); then the cascade count
/// (0: no shadows), the strength, the fade's start and end (view depth); the filter's taps a
/// side, the map's size (texels).
pub fn uniforms(t: &ShadowTunables, plan: Option<&Plan>) -> [f32; UNIFORM_FLOATS] {
    let mut u = [0.0f32; UNIFORM_FLOATS];
    let Some(p) = plan else {
        return u;
    };
    for (i, c) in p.cascades.iter().enumerate() {
        for (k, col) in c.view_proj.iter().enumerate() {
            u[i * 16 + k * 4..i * 16 + k * 4 + 4].copy_from_slice(col);
        }
        let at = MAX_CASCADES * 16 + i * 4;
        u[at..at + 4].copy_from_slice(&[
            c.texel_m as f32,
            (c.texel_m * t.bias.normal_offset_texels) as f32,
            (t.bias.depth_bias_m / c.depth_m) as f32,
            p.tier.filter_texels as f32,
        ]);
    }
    let at = MAX_CASCADES * 20;
    u[at..at + 8].copy_from_slice(&[
        p.cascades.len() as f32,
        p.caster.strength as f32,
        p.fade_start as f32,
        p.distance as f32,
        p.tier.filter_taps as f32,
        p.tier.resolution as f32,
        0.0,
        0.0,
    ]);
    u
}

/// The frame group's shadow entries (bindings 8–10: the uniform block, the maps, the compare
/// sampler), for the scene's fragment shaders.
pub fn layout_entries() -> [wgpu::BindGroupLayoutEntry; 3] {
    [
        wgpu::BindGroupLayoutEntry {
            binding: 8,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(UNIFORM_FLOATS as u64 * 4),
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 9,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 10,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
            count: None,
        },
    ]
}

/// The maps on the GPU: a depth array with a layer per cascade (1×1×1 while there are none, so
/// the frame group always binds one), its views, the compare sampler, the uniform block, and per
/// cascade a frame uniform (the scene's, with the cascade's matrix as `view_proj`) and its group
/// for the shadow passes.
pub struct ShadowMaps {
    /// The maps' size and layers as made.
    pub size: (u32, u32),
    texture: wgpu::Texture,
    pub array_view: wgpu::TextureView,
    pub layers: Vec<wgpu::TextureView>,
    pub sampler: wgpu::Sampler,
    pub uniform: wgpu::Buffer,
    pub frames: Vec<wgpu::Buffer>,
    pub frame_groups: Vec<wgpu::BindGroup>,
}

impl ShadowMaps {
    /// Maps of `resolution` texels a side, `layers` deep (≥ 1), with the shadow passes' frame
    /// groups (`pass_layout`: the frame uniform, the atlas and its sampler; `frame_bytes` each).
    pub fn new(
        device: &wgpu::Device,
        resolution: u32,
        layers: u32,
        pass_layout: &wgpu::BindGroupLayout,
        atlas: (&wgpu::TextureView, &wgpu::Sampler),
        frame_bytes: u64,
    ) -> Self {
        let layers = layers.max(1);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow maps"),
            size: wgpu::Extent3d {
                width: resolution,
                height: resolution,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SHADOW_DEPTH,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let array_view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("shadow maps"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let layer_views = (0..layers)
            .map(|i| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("shadow cascade"),
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: i,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        // Linear filtering: each compare is a 2×2 PCF in hardware.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow compare"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shadows"),
            size: UNIFORM_FLOATS as u64 * 4,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frames: Vec<wgpu::Buffer> = (0..layers)
            .map(|_| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("shadow frame"),
                    size: frame_bytes,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            })
            .collect();
        let frame_groups = frames
            .iter()
            .map(|b| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("shadow frame"),
                    layout: pass_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: b.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(atlas.0),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(atlas.1),
                        },
                    ],
                })
            })
            .collect();
        Self {
            size: (resolution, layers),
            texture,
            array_view,
            layers: layer_views,
            sampler,
            uniform,
            frames,
            frame_groups,
        }
    }

    /// The frame group's entries for the maps (bindings 8–10).
    pub fn group_entries(&self) -> [wgpu::BindGroupEntry<'_>; 3] {
        [
            wgpu::BindGroupEntry {
                binding: 8,
                resource: self.uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 9,
                resource: wgpu::BindingResource::TextureView(&self.array_view),
            },
            wgpu::BindGroupEntry {
                binding: 10,
                resource: wgpu::BindingResource::Sampler(&self.sampler),
            },
        ]
    }

    /// The maps' GPU bytes.
    pub fn bytes(&self) -> u64 {
        u64::from(self.size.0) * u64::from(self.size.0) * u64::from(self.size.1) * 4
    }
}

impl Drop for ShadowMaps {
    fn drop(&mut self) {
        // wgpu's WebGPU backend frees nothing on drop.
        self.texture.destroy();
        self.uniform.destroy();
        for b in &self.frames {
            b.destroy();
        }
    }
}

/// The maps' size and layers for a setting's tier (a 1×1 single layer for Off). They follow the
/// setting, not the time of day: kept through the night, when no light casts and no pass draws.
pub fn maps_size(tier: Option<&ShadowTier>) -> (u32, u32) {
    tier.map_or((1, 1), |t| {
        (t.resolution, t.cascades.clamp(1, MAX_CASCADES as u32))
    })
}

/// The casters' pipelines' depth state: the slope-scaled bias against acne on ramps.
pub fn caster_depth(t: &ShadowTunables) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: SHADOW_DEPTH,
        depth_write_enabled: Some(true),
        depth_compare: Some(wgpu::CompareFunction::LessEqual),
        stencil: Default::default(),
        bias: wgpu::DepthBiasState {
            constant: 0,
            slope_scale: t.bias.slope_scale as f32,
            clamp: 0.0,
        },
    }
}

/// The shadow lookup the scene's shaders share (group 0, bindings 8–10), after `COMMON`'s
/// helpers and the distant terrain's march (`lod_shadows::LOD_SHADOW_WGSL`).
/// `shadow_light(rel, n, stand_in)` is the factor on the direct sun and moon term: the map's
/// visibility near the eye, fading past the last cascade to `stand_in` (the first layer's: direct
/// light × sky light) shaded by the distant terrain, and as the light's shadows fade with the time
/// of day.
pub const SHADOW_WGSL: &str = r"
struct Shadows {
    cascades: array<mat4x4f, 4>,
    // Per cascade: x its texel (m), y the normal offset (m), z the compare's bias (map depth),
    // w the filter's radius (texels).
    texel: array<vec4f, 4>,
    // x cascades (0: none), y strength, z the fade to the stand-in's start, w its end (view depth).
    cfg: vec4f,
    // x the filter's taps a side, y the map's size (texels).
    pcf: vec4f,
};
@group(0) @binding(8) var<uniform> shadows: Shadows;
@group(0) @binding(9) var shadow_map: texture_depth_2d_array;
@group(0) @binding(10) var shadow_sampler: sampler_comparison;

// The map's visibility of an eye-relative point in cascade `c` (1 lit, 0 shadowed): a grid of
// `taps`² hardware 2×2 compares over the filter's radius, a soft PCF.
fn shadow_pcf(c: i32, p: vec3f) -> f32 {
    let size = shadows.pcf.y;
    let uv = vec2f(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    let z = p.z - shadows.texel[c].z;
    let taps = i32(shadows.pcf.x);
    let radius = shadows.texel[c].w / size;
    if (taps <= 1) {
        return textureSampleCompareLevel(shadow_map, shadow_sampler, uv, c, z);
    }
    let step = 2.0 * radius / f32(taps - 1);
    var sum = 0.0;
    for (var j = 0; j < taps; j++) {
        for (var i = 0; i < taps; i++) {
            let o = vec2f(f32(i), f32(j)) * step - radius;
            sum += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + o, c, z);
        }
    }
    return sum / f32(taps * taps);
}

// The first cascade whose map holds an eye-relative point (with the filter's margin), or −1.
fn shadow_cascade(rel: vec3f) -> i32 {
    let count = i32(shadows.cfg.x);
    for (var c = 0; c < count; c++) {
        let p = shadows.cascades[c] * vec4f(rel, 1.0);
        let margin = 1.0 - (shadows.texel[c].w + 2.0) * 2.0 / shadows.pcf.y;
        if (all(abs(p.xy) <= vec2f(margin)) && p.z >= 0.0 && p.z <= 1.0) {
            return c;
        }
    }
    return -1;
}

// Clouds' shadows on the terrain (CLOUDS.md §6, a future item, Phase 31's C7): the deck's
// transmittance toward the light over an eye-relative point. None yet.
fn cloud_shadow(rel: vec3f) -> f32 {
    return 1.0;
}

// Past the cascades: the stand-in shaded by the distant terrain (`lod_shadow`, the third layer),
// by the light's strength (dwell-look `lod_shadows::combine`'s `past`).
fn shadow_past(rel: vec3f, n: vec3f, stand_in: f32) -> f32 {
    return stand_in * (1.0 + shadows.cfg.y * (lod_shadow(rel, n) - 1.0));
}

// The factor on the direct sun and moon term at an eye-relative point with normal `n`: the map's
// visibility near the eye; past the last cascade `stand_in` (the first layer's) shaded by the
// distant terrain; between, over the cascades' last `edgeFade`, a blend of the two, so neither
// edge shows (dwell-look `lod_shadows::combine` is this in f64, tested). The time of day's fade
// weakens both alike.
fn shadow_light(rel: vec3f, n: vec3f, stand_in: f32) -> f32 {
    if (shadows.cfg.x < 0.5) {
        return stand_in * cloud_shadow(rel);
    }
    let depth = view_depth(rel);
    let near = 1.0 - smoothstep(shadows.cfg.z, shadows.cfg.w, depth);
    let c = shadow_cascade(rel);
    if (near <= 0.0 || c < 0) {
        return shadow_past(rel, n, stand_in) * cloud_shadow(rel);
    }
    // Normal offset: the lookup moves out along the surface's normal by the cascade's texels.
    let at = rel + n * shadows.texel[c].y;
    let p = (shadows.cascades[c] * vec4f(at, 1.0)).xyz;
    let inside = mix(stand_in, shadow_pcf(c, p), shadows.cfg.y);
    if (near >= 1.0) {
        return inside * cloud_shadow(rel);
    }
    return mix(shadow_past(rel, n, stand_in), inside, near) * cloud_shadow(rel);
}
";

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_look::daynight::DAY_NIGHT;
    use dwell_look::shadows::{SHADOWS, caster};

    fn view(eye: V3, forward: V3) -> View {
        let forward = normalize(forward);
        let right = normalize(cross(forward, [0.0, 1.0, 0.0]));
        View {
            eye,
            forward,
            right,
            up: cross(right, forward),
            tan_x: 1.3,
            tan_y: 0.75,
        }
    }

    fn corners(v: &View, d0: f64, d1: f64) -> Vec<V3> {
        let (f, right, up) = (v.forward, v.right, v.up);
        let mut out = Vec::new();
        for d in [d0, d1] {
            for sx in [-1.0, 1.0] {
                for sy in [-1.0, 1.0] {
                    out.push(std::array::from_fn(|k| {
                        f[k] * d + right[k] * sx * v.tan_x * d + up[k] * sy * v.tan_y * d
                    }));
                }
            }
        }
        out
    }

    #[test]
    fn the_splits_run_from_near_to_far_between_even_and_logarithmic() {
        let even = splits(4, 1.0, 161.0, 0.0);
        assert_eq!(even, vec![1.0, 41.0, 81.0, 121.0, 161.0]);
        let log = splits(2, 1.0, 100.0, 1.0);
        assert!((log[1] - 10.0).abs() < 1e-9);
        let mid = splits(4, 0.5, 160.0, 0.75);
        assert_eq!((mid[0], mid[4]), (0.5, 160.0));
        assert!(mid.windows(2).all(|w| w[1] > w[0]));
        // Between the two: nearer than even, farther than log.
        assert!(mid[1] < splits(4, 0.5, 160.0, 0.0)[1] && mid[1] > splits(4, 0.5, 160.0, 1.0)[1]);
    }

    #[test]
    fn a_slices_sphere_holds_its_corners_and_no_wider_than_it_must() {
        for (d0, d1) in [(0.0, 8.0), (8.0, 30.0), (30.0, 160.0), (100.0, 101.0)] {
            let v = view([0.0; 3], [0.0, 0.0, -1.0]);
            let (z, r) = slice_sphere(d0, d1, v.tan_x, v.tan_y);
            let centre = [0.0, 0.0, -z];
            let far: f64 = corners(&v, d0, d1)
                .iter()
                .map(|c| dot(sub(*c, centre), sub(*c, centre)).sqrt())
                .fold(0.0, f64::max);
            assert!(far <= r + 1e-9, "{d0}–{d1}: corner {far} out of {r}");
            // Tight: the farthest corner is on the sphere.
            assert!(far > r - 1e-6, "{d0}–{d1}: {far} well inside {r}");
        }
    }

    fn sub(a: V3, b: V3) -> V3 {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    #[test]
    fn each_cascade_holds_its_slice_and_the_casters_above_it() {
        let sun = normalize([0.3, 0.8, -0.4]);
        let basis = light_basis(sun, [0.0, 0.0, 1.0]);
        for forward in [[0.0, 0.0, -1.0], [1.0, -0.6, 0.2], [-0.3, 0.9, 0.1]] {
            let v = view([1234.5, 80.25, -987.75], forward);
            let d = splits(3, 0.5, 120.0, 0.7);
            for i in 0..3 {
                let c = fit(&v, basis, d[i], d[i + 1], 2048, 1.0, 300.0);
                for p in corners(&v, d[i], d[i + 1]) {
                    let q = c.project(p);
                    assert!(q[0].abs() <= 1.0 && q[1].abs() <= 1.0, "{q:?}");
                    assert!((0.0..=1.0).contains(&q[2]), "{q:?}");
                    // A caster 250 m up toward the light still lands in the map's depth.
                    let up: V3 = std::array::from_fn(|k| p[k] + sun[k] * 250.0);
                    assert!((0.0..=1.0).contains(&c.project(up)[2]));
                    assert!(c.touches(up, [0.5; 3]));
                }
                // The f32 matrix agrees with the f64 projection.
                let p = corners(&v, d[i], d[i + 1])[5];
                let m = c.view_proj;
                let g: Vec<f64> = (0..3)
                    .map(|r| {
                        (0..3).map(|k| f64::from(m[k][r]) * p[k]).sum::<f64>() + f64::from(m[3][r])
                    })
                    .collect();
                let q = c.project(p);
                for k in 0..3 {
                    assert!((g[k] - q[k]).abs() < 1e-4, "{g:?} vs {q:?}");
                }
            }
        }
    }

    #[test]
    fn the_far_cascades_take_turns_and_keep_their_world_box_between() {
        let noon = DAY_NIGHT.state(0.0, Some(0.5), Some(0.5));
        let sun = caster(&SHADOWS, &noon, 1.0);
        let at = |eye: V3| {
            plan(
                &SHADOWS,
                ShadowQuality::High,
                sun,
                &view(eye, [0.0, -0.2, -1.0]),
                noon.axis,
                1.0,
            )
            .unwrap()
        };
        let first = at([0.0, 60.0, 0.0]);
        let n = first.cascades.len();
        // No earlier frame: all of them.
        let (_, all) = stagger(None, &first, [0.0, 60.0, 0.0], 0, false);
        assert_eq!(all, (1 << n) - 1);
        // Then the nearest every frame and each other one in turn.
        let eye = [3.0, 60.5, -2.0];
        let next = at(eye);
        let mut seen = 0u8;
        let period = u64::from(SHADOWS.tiers[1].refresh_frames);
        for frame in 0..period {
            let (used, mask) = stagger(Some(&first), &next, eye, frame, false);
            assert_eq!(mask & 1, 1);
            assert!(mask.count_ones() <= 2);
            seen |= mask;
            for (i, kept) in used.iter().enumerate() {
                if mask & (1 << i) == 0 {
                    // Kept as drawn: the same world box, its matrix rebased to the new eye.
                    assert_eq!(kept.origin, first.cascades[i].origin);
                    let world: V3 = [10.0, -5.0, -30.0];
                    let old = first.cascades[i].project(sub(world, [0.0, 60.0, 0.0]));
                    let new = kept.project(sub(world, eye));
                    for k in 0..3 {
                        assert!((old[k] - new[k]).abs() < 1e-9);
                    }
                }
            }
        }
        assert_eq!(seen, (1 << n) - 1, "every cascade gets its turn");
        // The light changed: all again.
        let mut moon = next.clone();
        moon.caster.moon = true;
        assert_eq!(stagger(Some(&first), &moon, eye, 5, false).1, (1 << n) - 1);
    }

    #[test]
    fn only_casters_over_the_slice_are_drawn() {
        // The sun overhead, the camera looking along −z: a block behind the eye casts nowhere
        // in view, one ahead (or high above the slice) does.
        let basis = light_basis([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
        let v = view([0.0, 50.0, 0.0], [0.0, -0.1, -1.0]);
        let c = fit(&v, basis, 0.0, 40.0, 1024, 1.0, 300.0);
        assert!(c.touches([0.0, -2.0, -20.0], [0.5; 3]));
        assert!(c.touches([0.0, 150.0, -20.0], [0.5; 3]));
        assert!(!c.touches([0.0, -2.0, 25.0], [0.5; 3]), "behind the eye");
        assert!(
            !c.touches([200.0, -2.0, -20.0], [0.5; 3]),
            "far off to the side"
        );
        // Under the whole slice (farther from the light than any of it): no shadow on it.
        assert!(!c.touches([0.0, -200.0, -20.0], [0.5; 3]));
    }

    #[test]
    fn turning_the_camera_keeps_each_cascades_size() {
        let basis = light_basis(normalize([0.2, 0.9, 0.3]), [0.0, 0.0, 1.0]);
        let a = fit(
            &view([5.0, 40.0, 5.0], [0.0, 0.0, -1.0]),
            basis,
            10.0,
            40.0,
            1024,
            1.0,
            200.0,
        );
        for f in [[1.0, 0.0, 0.0], [0.3, -0.8, 0.5], [0.0, 1.0, 0.01]] {
            let b = fit(
                &view([5.0, 40.0, 5.0], f),
                basis,
                10.0,
                40.0,
                1024,
                1.0,
                200.0,
            );
            assert_eq!((a.radius, a.texel_m), (b.radius, b.texel_m));
        }
        // A radius rounds up to its step: a slightly wider view keeps it.
        let mut wider = view([5.0, 40.0, 5.0], [0.0, 0.0, -1.0]);
        wider.tan_x *= 1.001;
        let w = fit(&wider, basis, 10.0, 40.0, 1024, 1.0, 200.0);
        assert_eq!(w.radius, a.radius);
    }

    /// A world point's place in the map, in texels.
    fn texel_of(c: &Cascade, eye: V3, world: V3, resolution: f64) -> [f64; 2] {
        let q = c.project(sub(world, eye));
        [
            (q[0] * 0.5 + 0.5) * resolution,
            (0.5 - q[1] * 0.5) * resolution,
        ]
    }

    #[test]
    fn moving_a_sub_texel_step_keeps_the_light_space_origin_and_the_texel_grid() {
        let basis = light_basis(normalize([0.4, 0.7, -0.2]), [0.0, 0.0, 1.0]);
        let res = 2048;
        let start = [10_000.25, 72.5, -3_000.75];
        let c0 = fit(
            &view(start, [0.2, -0.3, -1.0]),
            basis,
            0.0,
            12.0,
            res,
            1.0,
            300.0,
        );
        let step = c0.texel_m * 0.1;
        let landmark = [10_003.0, 70.0, -3_005.0];
        let grid = texel_of(&c0, start, landmark, f64::from(res));
        let mut kept = 0;
        let mut last = c0.origin;
        for k in 1..=30 {
            // Sub-texel steps along the map's x and the light's own axis (depth only).
            let eye: V3 = std::array::from_fn(|i| {
                start[i] + basis.x[i] * step * f64::from(k) + basis.z[i] * 0.37 * f64::from(k)
            });
            let c = fit(
                &view(eye, [0.2, -0.3, -1.0]),
                basis,
                0.0,
                12.0,
                res,
                1.0,
                300.0,
            );
            let moved: V3 = sub(c.origin, last);
            let across = [dot(moved, basis.x), dot(moved, basis.y)];
            // The origin moves by whole texels across the light, or not at all.
            for a in across {
                let t = a / c.texel_m;
                assert!((t - t.round()).abs() < 1e-6, "{t} texels");
                assert!(t.abs() <= 1.0 + 1e-6);
            }
            if across.iter().all(|a| (a / c.texel_m).abs() < 1e-6) {
                kept += 1;
            }
            last = c.origin;
            // The texel grid stays put in the world: a fixed point's place in the map moves by
            // whole texels only.
            let g = texel_of(&c, eye, landmark, f64::from(res));
            for i in 0..2 {
                let d = g[i] - grid[i];
                assert!((d - d.round()).abs() < 1e-4, "{d}");
            }
        }
        assert!(
            kept >= 26,
            "a tenth-texel step kept the origin only {kept} times of 30"
        );
    }

    #[test]
    fn the_plan_follows_the_setting_the_light_and_the_spyglass() {
        let v = view([0.0, 60.0, 0.0], [0.0, -0.2, -1.0]);
        let noon = DAY_NIGHT.state(0.0, Some(0.5), Some(0.5));
        let sun = caster(&SHADOWS, &noon, 1.0);
        assert!(plan(&SHADOWS, ShadowQuality::Off, sun, &v, noon.axis, 1.0).is_none());
        let high = plan(&SHADOWS, ShadowQuality::High, sun, &v, noon.axis, 1.0).unwrap();
        let low = plan(&SHADOWS, ShadowQuality::Low, sun, &v, noon.axis, 1.0).unwrap();
        assert_eq!(high.cascades.len(), SHADOWS.tiers[1].cascades as usize);
        assert_eq!(low.cascades.len(), SHADOWS.tiers[0].cascades as usize);
        assert_eq!(high.distance, SHADOWS.tiers[1].distance_m);
        assert_eq!(high.cascades.last().unwrap().far, high.distance);
        assert_eq!(high.cascades[0].near, 0.0);
        // The slices tile the view's depth.
        for w in high.cascades.windows(2) {
            assert_eq!(w[0].far, w[1].near);
        }
        // No light up: no maps.
        assert!(plan(&SHADOWS, ShadowQuality::High, None, &v, noon.axis, 1.0).is_none());
        // The spyglass stretches the distance, up to its cap.
        let zoomed = plan(&SHADOWS, ShadowQuality::High, sun, &v, noon.axis, 4.0).unwrap();
        assert_eq!(
            zoomed.distance,
            (SHADOWS.tiers[1].distance_m * 4.0).min(SHADOWS.cascades.spyglass_max_m)
        );
        // A CPU adapter draws Low at most.
        assert_eq!(
            shadows_for(wgpu::DeviceType::Cpu, ShadowQuality::High),
            ShadowQuality::Low
        );
        assert_eq!(
            shadows_for(wgpu::DeviceType::Cpu, ShadowQuality::Off),
            ShadowQuality::Off
        );
        assert_eq!(
            shadows_for(wgpu::DeviceType::DiscreteGpu, ShadowQuality::High),
            ShadowQuality::High
        );
    }

    #[test]
    fn the_uniform_block_carries_the_cascades_and_none_when_off() {
        assert_eq!(uniforms(&SHADOWS, None), [0.0; UNIFORM_FLOATS]);
        let v = view([0.0, 60.0, 0.0], [0.0, -0.2, -1.0]);
        let noon = DAY_NIGHT.state(0.0, Some(0.5), Some(0.5));
        let p = plan(
            &SHADOWS,
            ShadowQuality::Low,
            caster(&SHADOWS, &noon, 1.0),
            &v,
            noon.axis,
            1.0,
        )
        .unwrap();
        let u = uniforms(&SHADOWS, Some(&p));
        let cfg = MAX_CASCADES * 20;
        assert_eq!(u[cfg], p.cascades.len() as f32);
        assert_eq!(u[cfg + 3], p.distance as f32);
        assert_eq!(u[cfg + 5], SHADOWS.tiers[0].resolution as f32);
        assert_eq!(u[0..4], p.cascades[0].view_proj[0]);
        assert_eq!(u[MAX_CASCADES * 16], p.cascades[0].texel_m as f32);
    }

    dwell_world::both_faces!(
        /// On either face the light over it casts down onto it: a point above the ground (toward
        /// the face's sky) is nearer the light in the map than the ground under it.
        fn the_light_over_either_face_casts_onto_it(f: FaceFrame) {
            // Face A's noon; face B's noon is face A's midnight.
            let time = if f.sign_f64() > 0.0 { 0.5 } else { 0.0 };
            let day = DAY_NIGHT.state(0.0, Some(time), Some(0.5));
            let c = caster(&SHADOWS, &day, f.sign_f64()).expect("the sun is up");
            assert!(!c.moon);
            let up = f.up();
            let eye = f.point([0.0, 60.0, 0.0]);
            let forward = f.vector([0.0, -0.3, -1.0]);
            let p = plan(
                &SHADOWS,
                ShadowQuality::High,
                Some(c),
                &view(eye, forward),
                day.axis,
                1.0,
            )
            .unwrap();
            let k = &p.cascades[0];
            let ground = [0.0, -1.7 * up[1], -3.0];
            let above: V3 = std::array::from_fn(|i| ground[i] + up[i] * 2.0);
            assert!(k.project(above)[2] < k.project(ground)[2]);
        }
    );
}
