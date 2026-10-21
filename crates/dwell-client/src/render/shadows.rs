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

/// One cascade: a box around the eye as it was when the map was drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cascade {
    /// The distances from the eye it covers (m): `near` is where the cascade before it ends.
    pub near: f64,
    pub far: f64,
    /// Its box's half-size across the light (m): `far` and the slack, rounded up.
    pub radius: f64,
    /// Its box's centre (world): the eye when drawn, snapped to whole texels across the light.
    pub origin: V3,
    /// The box's centre relative to the eye.
    pub origin_rel: V3,
    /// A texel's size (m).
    pub texel_m: f64,
    /// The box's depth along the light (m): the box and the casters' reach above it.
    pub depth_m: f64,
    pub basis: LightBasis,
    /// Eye-relative points to the map: xy in [−1, 1] (clip space), z in [0, 1] from the light.
    pub view_proj: Mat4,
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

/// A matrix that holds nothing (every point's depth −1): a cascade not drawn yet.
pub const NO_CASCADE: Mat4 = [
    [0.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, -1.0, 1.0],
];

/// Fits a cascade covering everything within `far` of `eye` (world), its box `slack` wider each
/// way (the eye may move that far before the map must be drawn again) and its half-size rounded up
/// to `radius_step`; its centre the eye snapped to the map's texels across the light, deep enough
/// to hold casters `reach` metres above it toward the light. It depends on the eye and the light
/// alone, never on where the camera looks (Minecraft shaderpacks' player-centred shadow map), so
/// turning leaves a far cascade's map as it is.
#[allow(clippy::too_many_arguments)]
pub fn fit(
    eye: V3,
    basis: LightBasis,
    near: f64,
    far: f64,
    slack: f64,
    resolution: u32,
    radius_step: f64,
    reach: f64,
) -> Cascade {
    let radius = ((far + slack) / radius_step).ceil().max(1.0) * radius_step;
    let texel_m = 2.0 * radius / f64::from(resolution);
    let snap = |axis: V3| (dot(eye, axis) / texel_m).round() * texel_m;
    let (u, v, w) = (snap(basis.x), snap(basis.y), dot(eye, basis.z));
    let origin = std::array::from_fn(|k| basis.x[k] * u + basis.y[k] * v + basis.z[k] * w);
    let origin_rel: V3 = std::array::from_fn(|k| origin[k] - eye[k]);
    Cascade {
        near,
        far,
        radius,
        origin,
        origin_rel,
        texel_m,
        depth_m: 2.0 * radius + reach,
        basis,
        view_proj: light_matrix(basis, origin_rel, radius, reach),
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

    /// Whether everything within `far` of `eye` (world) is still inside the box (with the
    /// filter's few texels): the eye hasn't left the slack.
    pub fn holds(&self, eye: V3) -> bool {
        let d: V3 = std::array::from_fn(|k| eye[k] - self.origin[k]);
        dot(d, d).sqrt() + self.far + 4.0 * self.texel_m <= self.radius
    }

    /// How far the light has turned since the map was drawn (degrees), toward `dir`.
    pub fn turned_deg(&self, dir: V3) -> f64 {
        dot(self.basis.z, normalize(dir))
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees()
    }

    /// Whether a box (its centre relative to the eye and half-size, axis-aligned) can cast into
    /// this cascade: it overlaps the cascade's box, which reaches up toward the light.
    pub fn touches(&self, centre_rel: V3, half: V3) -> bool {
        let d: V3 = std::array::from_fn(|k| centre_rel[k] - self.origin_rel[k]);
        let extent = |a: V3| (0..3).map(|k| a[k].abs() * half[k]).sum::<f64>();
        let reach = self.depth_m - 2.0 * self.radius;
        let (x, y, z) = (
            dot(d, self.basis.x),
            dot(d, self.basis.y),
            dot(d, self.basis.z),
        );
        x.abs() <= self.radius + extent(self.basis.x)
            && y.abs() <= self.radius + extent(self.basis.y)
            && z + extent(self.basis.z) >= -self.radius
            && z - extent(self.basis.z) <= self.radius + reach
    }
}

/// This frame's shadows: the light, the tier and its cascades fitted to this frame's eye (none:
/// no shadows drawn). Which of them are drawn and which are kept from earlier frames is the
/// schedule's (`Cache::schedule`).
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub caster: Caster,
    pub tier: ShadowTier,
    pub cascades: Vec<Cascade>,
    /// The shadow distance (m from the eye) and where its fade to the stand-in starts.
    pub distance: f64,
    pub fade_start: f64,
}

/// The frame's shadow plan: for `quality` (already `shadows_for` the adapter), the light `caster`
/// (none when neither light is up), the `eye` (world), the sky's axis (the light's map axes) and
/// the spyglass's `magnification` (the shadow distance grows with it, so the zoomed view has
/// shadows where it looks, up to `spyglassMaxM`).
pub fn plan(
    t: &ShadowTunables,
    quality: ShadowQuality,
    caster: Option<Caster>,
    eye: V3,
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
    let d = splits(
        count,
        c.near_m.min(distance * 0.5),
        distance,
        tier.split_lambda,
    );
    // The first reaches back to the eye: what is under your feet. It is drawn every frame, so it
    // needs no slack.
    let cascades = (0..count)
        .map(|i| {
            let near = if i == 0 { 0.0 } else { d[i] };
            let slack = if i == 0 { 0.0 } else { d[i + 1] * c.slack };
            fit(
                eye,
                basis,
                near,
                d[i + 1],
                slack,
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

/// Why a far cascade wants drawing again, least urgent first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Want {
    /// Its map is right as kept.
    Nothing,
    /// Moving casters (other players) are in it and `refreshFrames` have passed.
    Moving,
    /// The casters in it changed (a chunk or section drawn, replaced or removed; a fade stepped).
    Changed,
    /// The light has turned `lightTurnDeg` since it was drawn.
    Turned,
    /// The eye has left its slack: part of what it must cover is outside its box.
    Moved,
    /// Never drawn for this light and tier: it holds nothing yet.
    Missing,
}

/// A far cascade's map as last drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Kept {
    pub cascade: Cascade,
    /// The frame it was drawn in.
    pub frame: u64,
    /// The casters it was drawn with (`signature`), to find when they change.
    pub casters: u64,
}

/// What a frame draws and reads: per cascade, the box it reads (none: not drawn yet, reads
/// nothing) and, as a bit per cascade, which it draws this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub cascades: Vec<Option<Cascade>>,
    pub draw: u8,
}

/// The maps kept between frames, and the light and tier they were drawn for.
#[derive(Clone, Debug, Default)]
pub struct Cache {
    pub kept: Vec<Option<Kept>>,
    light: Option<(bool, ShadowTier)>,
}

impl Cache {
    /// Forgets every map (the maps were made again, or the setting changed).
    pub fn clear(&mut self) {
        self.kept.clear();
        self.light = None;
    }

    /// Why each far cascade wants drawing again this frame (the nearest is always drawn; its
    /// entry is `Missing`). `casters(i, kept)` is the signature of the casters touching kept
    /// cascade `i`'s box now; `moving` whether moving casters (other players) are about.
    pub fn wants(
        &self,
        t: &ShadowTunables,
        plan: &Plan,
        eye: V3,
        frame: u64,
        moving: bool,
        casters: &mut dyn FnMut(usize, &Cascade) -> u64,
    ) -> Vec<Want> {
        let same = self.light == Some((plan.caster.moon, plan.tier));
        (0..plan.cascades.len())
            .map(|i| {
                let Some(k) = self
                    .kept
                    .get(i)
                    .copied()
                    .flatten()
                    .filter(|_| same && i > 0)
                else {
                    return Want::Missing;
                };
                let fresh = &plan.cascades[i];
                if k.cascade.far != fresh.far
                    || k.cascade.radius != fresh.radius
                    || !k.cascade.holds(eye)
                {
                    Want::Moved
                } else if k.cascade.turned_deg(plan.caster.dir) > t.cascades.light_turn_deg {
                    Want::Turned
                } else if casters(i, &k.cascade.rebased(eye)) != k.casters {
                    Want::Changed
                } else if moving && frame >= k.frame + u64::from(plan.tier.refresh_frames) {
                    Want::Moving
                } else {
                    Want::Nothing
                }
            })
            .collect()
    }

    /// This frame's schedule from `wants`: the nearest cascade drawn every frame, and at most
    /// `farPerFrame` far ones that want it, the most urgent first (then the longest kept), so no
    /// frame draws more than that however much has changed (no spikes). The rest are read as
    /// kept, rebased to this `eye`, even while they want drawing; one never drawn reads nothing.
    pub fn schedule(&self, plan: &Plan, wants: &[Want], eye: V3) -> Frame {
        let same = self.light == Some((plan.caster.moon, plan.tier));
        let kept = |i: usize| self.kept.get(i).copied().flatten().filter(|_| same);
        let mut order: Vec<usize> = (1..plan.cascades.len())
            .filter(|&i| wants[i] != Want::Nothing)
            .collect();
        order.sort_by_key(|&i| {
            (
                std::cmp::Reverse(wants[i]),
                kept(i).map_or(0, |k| k.frame),
                i,
            )
        });
        let budget = plan.tier.far_per_frame.max(1) as usize;
        let mut draw = 1u8;
        for &i in order.iter().take(budget) {
            draw |= 1 << i;
        }
        let cascades = (0..plan.cascades.len())
            .map(|i| {
                if draw & (1 << i) != 0 {
                    Some(plan.cascades[i])
                } else {
                    kept(i).map(|k| k.cascade.rebased(eye))
                }
            })
            .collect();
        Frame { cascades, draw }
    }

    /// Records what `frame` (number `n`) drew: `casters(i)` the signature of the casters each drawn
    /// cascade was drawn with.
    pub fn drew(&mut self, plan: &Plan, f: &Frame, n: u64, casters: &dyn Fn(usize) -> u64) {
        if self.light != Some((plan.caster.moon, plan.tier)) {
            self.kept.clear();
            self.light = Some((plan.caster.moon, plan.tier));
        }
        self.kept.resize(plan.cascades.len(), None);
        for (i, c) in f.cascades.iter().enumerate() {
            if f.draw & (1 << i) != 0 {
                self.kept[i] = c.map(|cascade| Kept {
                    cascade,
                    frame: n,
                    casters: casters(i),
                });
            }
        }
    }
}

/// An order-free signature of a set of casters: each caster's identity hashed, the hashes summed
/// (so the casters can be visited in any order, and one added, removed or changed changes it).
pub fn signature(ids: impl IntoIterator<Item = u64>) -> u64 {
    ids.into_iter().fold(0u64, |s, id| s.wrapping_add(mix(id)))
}

/// A 64-bit finaliser (splitmix64's).
pub fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Floats in the shadow uniform block (`Shadows` in `SHADOW_WGSL`).
pub const UNIFORM_FLOATS: usize = MAX_CASCADES * 16 + MAX_CASCADES * 4 + 8;

/// The shadow uniform block: each cascade's matrix (`NO_CASCADE` for one not drawn yet); per
/// cascade its texel (m), the normal offset (m), the compare's bias (map depth) and the filter's
/// radius (texels); then the cascade count (0: no shadows), the strength, the fade's start and
/// end (distance from the eye); the filter's taps a side, the map's size (texels).
pub fn uniforms(t: &ShadowTunables, plan: Option<(&Plan, &Frame)>) -> [f32; UNIFORM_FLOATS] {
    let mut u = [0.0f32; UNIFORM_FLOATS];
    let Some((p, f)) = plan else {
        return u;
    };
    for (i, c) in f.cascades.iter().enumerate() {
        // A cascade not drawn yet holds nothing: its fragments take the next one, or the far
        // shadows past them.
        let m = c.map_or(NO_CASCADE, |c| c.view_proj);
        for (k, col) in m.iter().enumerate() {
            u[i * 16 + k * 4..i * 16 + k * 4 + 4].copy_from_slice(col);
        }
        let Some(c) = c else { continue };
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
        f.cascades.len() as f32,
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
    // x cascades (0: none), y strength, z the fade to the stand-in's start, w its end (distance).
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
    return stand_in * (1.0 + shadows.cfg.y * (lod_shadow(rel) - 1.0));
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
    // The cascades cover a sphere around the eye (whichever way it looks): fade by distance.
    let near = 1.0 - smoothstep(shadows.cfg.z, shadows.cfg.w, length(rel));
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

    fn sub(a: V3, b: V3) -> V3 {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    fn noon_plan(q: ShadowQuality, eye: V3) -> Plan {
        let noon = DAY_NIGHT.state(0.0, Some(0.5), Some(0.5));
        plan(
            &SHADOWS,
            q,
            caster(&SHADOWS, &noon, 1.0),
            eye,
            noon.axis,
            1.0,
        )
        .unwrap()
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

    /// Points within `far` of the eye in every direction.
    fn around(far: f64) -> Vec<V3> {
        let mut out = Vec::new();
        for i in 0..64 {
            let a = i as f64 * 0.7;
            let y = (i as f64 / 63.0) * 2.0 - 1.0;
            let r = (1.0 - y * y).sqrt();
            out.push([r * a.cos() * far, y * far, r * a.sin() * far]);
        }
        out
    }

    #[test]
    fn each_cascade_holds_everything_within_its_distance_and_the_casters_above_it() {
        let sun = normalize([0.3, 0.8, -0.4]);
        let basis = light_basis(sun, [0.0, 0.0, 1.0]);
        let eye = [1234.5, 80.25, -987.75];
        let d = splits(3, 0.5, 120.0, 0.7);
        for i in 0..3 {
            let c = fit(eye, basis, d[i], d[i + 1], d[i + 1] * 0.1, 2048, 1.0, 300.0);
            assert!(c.holds(eye));
            for p in around(d[i + 1]) {
                let q = c.project(p);
                assert!(q[0].abs() <= 1.0 && q[1].abs() <= 1.0, "{q:?}");
                assert!((0.0..=1.0).contains(&q[2]), "{q:?}");
                // A caster 250 m up toward the light still lands in the map's depth.
                let up: V3 = std::array::from_fn(|k| p[k] + sun[k] * 250.0);
                assert!((0.0..=1.0).contains(&c.project(up)[2]));
                assert!(c.touches(up, [0.5; 3]));
            }
            // The f32 matrix agrees with the f64 projection.
            let p = around(d[i + 1])[5];
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

    #[test]
    fn a_kept_cascade_holds_its_distance_until_the_eye_leaves_its_slack() {
        let basis = light_basis(normalize([0.2, 0.9, 0.3]), [0.0, 0.0, 1.0]);
        let eye = [100.0, 40.0, -50.0];
        let c = fit(eye, basis, 20.0, 60.0, 6.0, 2048, 1.0, 120.0);
        for step in [0.0, 2.0, 5.0] {
            let moved = [eye[0] + step, eye[1], eye[2]];
            let k = c.rebased(moved);
            assert!(k.holds(moved), "moved {step} m");
            // Everything within its distance of the moved eye is still in the box.
            for p in around(60.0) {
                let q = k.project(p);
                assert!(q[0].abs() <= 1.0 && q[1].abs() <= 1.0 && (0.0..=1.0).contains(&q[2]));
            }
        }
        assert!(!c.holds([eye[0] + 6.5, eye[1], eye[2]]), "out of the slack");
        assert!(
            !c.holds([eye[0], eye[1] - 7.0, eye[2]]),
            "out of the slack along the light"
        );
        // The light: the angle since it was drawn.
        assert!(c.turned_deg(basis.z) < 1e-6);
        let tilted = normalize([basis.z[0] + 0.01, basis.z[1], basis.z[2]]);
        assert!(c.turned_deg(tilted) > 0.1);
    }

    #[test]
    fn casters_all_around_the_eye_cast_but_not_off_the_box_or_under_it() {
        // The sun overhead: a block behind the eye casts too (the box doesn't follow the view),
        // one far off to the side or far under the ground does not.
        let basis = light_basis([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
        let c = fit([0.0, 50.0, 0.0], basis, 0.0, 40.0, 0.0, 1024, 1.0, 300.0);
        assert!(c.touches([0.0, -2.0, -20.0], [0.5; 3]));
        assert!(c.touches([0.0, -2.0, 25.0], [0.5; 3]), "behind the eye");
        assert!(c.touches([0.0, 150.0, -20.0], [0.5; 3]), "high above");
        assert!(
            !c.touches([200.0, -2.0, -20.0], [0.5; 3]),
            "far off to the side"
        );
        assert!(!c.touches([0.0, -200.0, -20.0], [0.5; 3]), "under the box");
        assert!(!c.touches([0.0, 400.0, -20.0], [0.5; 3]), "past the reach");
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
        let c0 = fit(start, basis, 0.0, 12.0, 0.0, res, 1.0, 300.0);
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
            let c = fit(eye, basis, 0.0, 12.0, 0.0, res, 1.0, 300.0);
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
        let eye = [0.0, 60.0, 0.0];
        let noon = DAY_NIGHT.state(0.0, Some(0.5), Some(0.5));
        let sun = caster(&SHADOWS, &noon, 1.0);
        assert!(plan(&SHADOWS, ShadowQuality::Off, sun, eye, noon.axis, 1.0).is_none());
        let high = noon_plan(ShadowQuality::High, eye);
        let low = noon_plan(ShadowQuality::Low, eye);
        assert_eq!(high.cascades.len(), SHADOWS.tiers[1].cascades as usize);
        assert_eq!(low.cascades.len(), SHADOWS.tiers[0].cascades as usize);
        assert_eq!(high.distance, SHADOWS.tiers[1].distance_m);
        assert_eq!(high.cascades.last().unwrap().far, high.distance);
        assert_eq!(high.cascades[0].near, 0.0);
        // The ranges tile the distance, and the far ones have their slack.
        for w in high.cascades.windows(2) {
            assert_eq!(w[0].far, w[1].near);
            assert!(w[1].radius >= w[1].far * (1.0 + SHADOWS.cascades.slack));
        }
        // No light up: no maps.
        assert!(plan(&SHADOWS, ShadowQuality::High, None, eye, noon.axis, 1.0).is_none());
        // The spyglass stretches the distance, up to its cap.
        let zoomed = plan(&SHADOWS, ShadowQuality::High, sun, eye, noon.axis, 4.0).unwrap();
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

    /// One frame of the schedule, as the renderer runs it: the wants, the schedule, the record.
    fn step(
        cache: &mut Cache,
        p: &Plan,
        eye: V3,
        n: u64,
        moving: bool,
        sig: &dyn Fn(usize) -> u64,
    ) -> (Vec<Want>, Frame) {
        let wants = cache.wants(&SHADOWS, p, eye, n, moving, &mut |i, _| sig(i));
        let f = cache.schedule(p, &wants, eye);
        cache.drew(p, &f, n, sig);
        (wants, f)
    }

    #[test]
    fn the_far_cascades_are_drawn_one_a_frame_then_kept_until_something_changes() {
        let eye = [0.0, 60.0, 0.0];
        let p = noon_plan(ShadowQuality::High, eye);
        let n = p.cascades.len();
        assert!(n >= 3, "the test wants two far cascades");
        let budget = p.tier.far_per_frame as usize;
        let mut cache = Cache::default();
        let none = |_: usize| 7u64;
        // The first frames: the nearest and `farPerFrame` far ones a frame, the others reading
        // nothing until drawn (no frame draws them all at once).
        let (_, f) = step(&mut cache, &p, eye, 0, false, &none);
        assert_eq!(f.draw.count_ones() as usize, 1 + budget);
        assert!(f.cascades.iter().filter(|c| c.is_none()).count() == n - 1 - budget);
        assert_eq!(
            uniforms(&SHADOWS, Some((&p, &f)))[(n - 1) * 16..(n - 1) * 16 + 16],
            NO_CASCADE.concat()[..],
            "a cascade not drawn yet holds nothing"
        );
        let mut frame = 1;
        while cache.kept.iter().skip(1).any(Option::is_none) {
            let (_, f) = step(&mut cache, &p, eye, frame, false, &none);
            assert!(f.draw.count_ones() as usize <= 1 + budget);
            frame += 1;
        }
        // Then only the nearest, frame after frame, while the eye, the light and the casters stay.
        for k in 0..10 {
            let (wants, f) = step(&mut cache, &p, eye, frame + k, false, &none);
            assert_eq!(f.draw, 1, "{wants:?}");
            assert!(f.cascades.iter().all(Option::is_some));
        }
        frame += 10;
        // The casters in the farthest change: it alone is drawn again.
        let changed = |i: usize| if i == n - 1 { 8 } else { 7 };
        let (wants, f) = step(&mut cache, &p, eye, frame, false, &changed);
        assert_eq!(wants[n - 1], Want::Changed);
        assert_eq!(f.draw, 1 | 1 << (n - 1));
        frame += 1;
        let (_, f) = step(&mut cache, &p, eye, frame, false, &changed);
        assert_eq!(f.draw, 1);
        frame += 1;
        // Moving casters: each far cascade again once every `refreshFrames`, one a frame.
        let mut drawn = vec![0u32; n];
        let period = u64::from(p.tier.refresh_frames);
        for k in 0..period * 4 {
            let (_, f) = step(&mut cache, &p, eye, frame + k, true, &changed);
            assert!(f.draw.count_ones() as usize <= 1 + budget);
            for (i, d) in drawn.iter_mut().enumerate() {
                *d += u32::from(f.draw & (1 << i) != 0);
            }
        }
        for (i, d) in drawn.iter().enumerate().skip(1) {
            assert!(
                (3..=5).contains(d),
                "cascade {i} drawn {d} times in {} frames",
                period * 4
            );
        }
    }

    #[test]
    fn moving_out_of_the_slack_or_turning_the_light_draws_again_the_most_urgent_first() {
        let eye = [0.0, 60.0, 0.0];
        let p = noon_plan(ShadowQuality::High, eye);
        let n = p.cascades.len();
        let mut cache = Cache::default();
        let sig = |_: usize| 1u64;
        for k in 0..n as u64 {
            step(&mut cache, &p, eye, k, false, &sig);
        }
        // A step that leaves the nearer far cascade's slack but not the farther one's.
        let slack1 = p.cascades[1].radius - p.cascades[1].far;
        let moved = [eye[0] + slack1 * 1.1, eye[1], eye[2]];
        let q = noon_plan(ShadowQuality::High, moved);
        let wants = cache.wants(&SHADOWS, &q, moved, 10, false, &mut |_, _| 1);
        assert_eq!(wants[1], Want::Moved);
        assert_eq!(wants[n - 1], Want::Nothing);
        // The light turned past the threshold: every far one wants it, the moved one first.
        let mut turned = q.clone();
        let tilt = (SHADOWS.cascades.light_turn_deg * 2.0).to_radians();
        let d = p.caster.dir;
        turned.caster.dir = normalize([d[0] + tilt, d[1], d[2]]);
        let wants = cache.wants(&SHADOWS, &turned, moved, 10, false, &mut |_, _| 1);
        assert_eq!(wants[1], Want::Moved);
        assert!(wants[2..].iter().all(|w| *w == Want::Turned), "{wants:?}");
        let f = cache.schedule(&turned, &wants, moved);
        assert_eq!(f.draw, 1 | 1 << 1);
        // The kept ones are read where they were drawn (rebased to this eye).
        let kept = f.cascades[n - 1].unwrap();
        assert_eq!(kept.origin, p.cascades[n - 1].origin);
        // The light changed (the moon) or the tier: nothing kept counts.
        let mut moon = q.clone();
        moon.caster.moon = true;
        let wants = cache.wants(&SHADOWS, &moon, moved, 10, false, &mut |_, _| 1);
        assert!(wants.iter().all(|w| *w == Want::Missing));
    }

    #[test]
    fn signatures_ignore_order_and_see_one_caster_change() {
        let a = signature([1, 2, 3]);
        assert_eq!(a, signature([3, 1, 2]));
        assert_ne!(a, signature([1, 2]));
        assert_ne!(a, signature([1, 2, 4]));
    }

    #[test]
    fn the_uniform_block_carries_the_cascades_and_none_when_off() {
        assert_eq!(uniforms(&SHADOWS, None), [0.0; UNIFORM_FLOATS]);
        let p = noon_plan(ShadowQuality::Low, [0.0, 60.0, 0.0]);
        let f = Frame {
            cascades: p.cascades.iter().copied().map(Some).collect(),
            draw: 3,
        };
        let u = uniforms(&SHADOWS, Some((&p, &f)));
        let cfg = MAX_CASCADES * 20;
        assert_eq!(u[cfg], p.cascades.len() as f32);
        assert_eq!(u[cfg + 3], p.distance as f32);
        assert_eq!(u[cfg + 5], SHADOWS.tiers[0].resolution as f32);
        assert_eq!(u[0..4], p.cascades[0].view_proj[0]);
        assert_eq!(u[MAX_CASCADES * 16], p.cascades[0].texel_m as f32);
    }

    dwell_world::both_faces!(
        /// On either face the light over it casts down onto it: a point above the ground (toward
        /// the face's sky) is nearer the light in the map than the ground under it, a caster high
        /// over the face casts into the cascades and one deep under its ground does not.
        fn the_light_over_either_face_casts_onto_it(f: FaceFrame) {
            // Face A's noon; face B's noon is face A's midnight.
            let time = if f.sign_f64() > 0.0 { 0.5 } else { 0.0 };
            let day = DAY_NIGHT.state(0.0, Some(time), Some(0.5));
            let c = caster(&SHADOWS, &day, f.sign_f64()).expect("the sun is up");
            assert!(!c.moon);
            let up = f.up();
            let eye = f.point([0.0, 60.0, 0.0]);
            let p = plan(&SHADOWS, ShadowQuality::High, Some(c), eye, day.axis, 1.0).unwrap();
            let k = &p.cascades[0];
            let ground = f.vector([0.0, -1.7, -3.0]);
            let above: V3 = std::array::from_fn(|i| ground[i] + up[i] * 2.0);
            assert!(k.project(above)[2] < k.project(ground)[2]);
            let far = p.cascades.last().unwrap();
            assert!(
                far.touches(f.vector([10.0, 80.0, -30.0]), [8.0; 3]),
                "over the face"
            );
            assert!(
                !far.touches(f.vector([10.0, -400.0, -30.0]), [8.0; 3]),
                "under its ground"
            );
        }
    );
}
