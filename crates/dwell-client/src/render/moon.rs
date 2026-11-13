//! The moon's far-body maps on the GPU (docs/proposals/MOON_WORLD.md §5, M-2): the moon's face A
//! (tidally locked: home always sees it) baked from its real terrain by `dwell-worldgen`'s
//! `bake_far`, packed into one storage buffer the sky's `dwell_moon` reads (`dwell_moon_ground`),
//! so the disc in the sky is shaded by the baked normals and albedo, lit from the sun's side.
//!
//! A storage buffer, not a texture: the composite pass already binds the 16 sampled textures a
//! stage may have (WebGPU's default limit), and the disc is a few hundred pixels, so the shader's
//! own bilinear fetch costs nothing. The buffer is `[size, levels, normals, 0]`, then each mip level
//! from `size` down (each `side`² texels row-major, two words a texel: the albedo then the normal,
//! `pack4x8unorm`). Until a world's bake arrives it holds the header only (`size` 0), and the
//! shader draws the moon as it did before M-2 (its procedural maria, a flat disc).
//!
//! The sun world's face A (docs/proposals/SUN_WORLD.md S-3) shares the buffer: its own header and levels from
//! word [`SUN_BASE`], its albedo only (`dwell_sun_ground`, the sky's sun disc shaded by its lava and
//! crusts); `size` 0 there draws today's plain sun.

use dwell_worldgen::grids::FarBake;

/// The frame group's binding.
pub const BINDING: u32 = 20;

/// The smallest mip level's side (texels): below it the disc is a dot of a pixel or two.
pub const MIN_SIDE: usize = 4;

/// The grid the sky's moon is (`shared/worldgen/grids.json`).
pub const MOON_GRID: &str = "dwell:moon";

/// The grid the sky's sun is (docs/proposals/SUN_WORLD.md).
pub const SUN_GRID: &str = "dwell:sun";

pub fn layout_entry() -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding: BINDING,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: wgpu::BufferSize::new(16),
        },
        count: None,
    }
}

/// The words of a bake's levels, sides halving from `bake.size` down to [`MIN_SIDE`]. The albedo
/// is the ground's luminance over the bake's brightest, so the brightest highland is the sky's
/// `MOON_COLOR` as before (the moon as bright as today's); outside the disc the mean, so filtering
/// at its rim doesn't darken it. `normals` false (the CPU adapter's cheap tier) leaves them out:
/// the header says so and the shader lights a flat disc by the albedo alone. `relief` scales the
/// normals' slopes (`daynight.json`'s moon `relief`): the craters are a few degrees steep, too
/// shallow to read from home at 1:1.
pub fn pack(bake: &FarBake, normals: bool, relief: f64) -> Vec<u32> {
    let size = bake.size;
    let n = size * size;
    let luma = |p: &[u8]| {
        (0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])) / 255.0
    };
    let inside = |i: usize| bake.albedo[i * 4 + 3] > 0;
    let max = (0..n)
        .filter(|&i| inside(i))
        .map(|i| luma(&bake.albedo[i * 4..]))
        .fold(0.0f64, f64::max)
        .max(1e-6);
    let count = (0..n).filter(|&i| inside(i)).count().max(1);
    let mean = (0..n)
        .filter(|&i| inside(i))
        .map(|i| luma(&bake.albedo[i * 4..]) / max)
        .sum::<f64>()
        / count as f64;
    // Level 0 as floats: albedo, normal x, z, up.
    let mut level: Vec<[f64; 4]> = (0..n)
        .map(|i| {
            let p = &bake.normal_height[i * 4..];
            let d = |b: u8| f64::from(b) / 255.0 * 2.0 - 1.0;
            let a = if inside(i) {
                luma(&bake.albedo[i * 4..]) / max
            } else {
                mean
            };
            let up = d(p[2]).max(1e-3);
            let (sx, sz) = (d(p[0]) / up * relief, d(p[1]) / up * relief);
            let len = (sx * sx + sz * sz + 1.0).sqrt();
            [a, sx / len, sz / len, 1.0 / len]
        })
        .collect();
    let mut side = size;
    let mut levels = Vec::new();
    loop {
        levels.push((side, level.clone()));
        if side / 2 < MIN_SIDE {
            break;
        }
        let half = side / 2;
        level = (0..half * half)
            .map(|k| {
                let (i, j) = (k % half, k / half);
                let mut s = [0.0; 4];
                for (di, dj) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let t = level[(2 * j + dj) * side + 2 * i + di];
                    for c in 0..4 {
                        s[c] += t[c] * 0.25;
                    }
                }
                s
            })
            .collect();
        side = half;
    }
    let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    let mut out = vec![size as u32, levels.len() as u32, u32::from(normals), 0];
    for (_, texels) in &levels {
        for t in texels {
            out.push(byte(t[0]) * 0x0101_0101);
            let (x, z, up) = if normals {
                let len = (t[1] * t[1] + t[2] * t[2] + t[3] * t[3]).sqrt().max(1e-9);
                (t[1] / len, t[2] / len, t[3] / len)
            } else {
                (0.0, 0.0, 1.0)
            };
            let e = |v: f64| byte(v * 0.5 + 0.5);
            out.push(e(x) | e(z) << 8 | e(up) << 16 | 255 << 24);
        }
    }
    out
}

/// The moon's bake for a world: its face A (the one home sees) at the grid's data size, or none
/// for a build without the moon grid.
pub fn bake_for(world_seed: u64) -> Option<FarBake> {
    let def = dwell_worldgen::grids::grid(MOON_GRID)?;
    let terrain = dwell_worldgen::grids::CrateredTerrain::new(def, world_seed)?;
    Some(terrain.bake_far_default(dwell_world::Face::A))
}

/// The sun world's bake for a world: its face A (the one home sees) at the grid's data size, or
/// none for a build without the sun grid.
pub fn sun_bake_for(world_seed: u64) -> Option<FarBake> {
    let def = dwell_worldgen::grids::grid(SUN_GRID)?;
    let terrain = dwell_worldgen::volcanic::VolcanicTerrain::new(def, world_seed)?;
    Some(terrain.bake_far_default(dwell_world::Face::A))
}

/// The buffer: room for a full 256² bake's levels (written in place, so the frame group never
/// changes), the header saying "none yet" until one is written.
pub struct MoonGpu {
    pub buffer: wgpu::Buffer,
}

/// Words a bake of side `size` packs to.
pub const fn words(size: usize) -> usize {
    let mut w = 4;
    let mut side = size;
    loop {
        w += 2 * side * side;
        if side / 2 < MIN_SIDE {
            return w;
        }
        side /= 2;
    }
}

/// The side the buffer is sized for (`grids.json`'s moon `bake.size`; 256).
pub const CAPACITY_SIDE: usize = 256;

/// The word the sun's header and levels start at: after room for the moon's.
pub const SUN_BASE: usize = words(CAPACITY_SIDE);

impl MoonGpu {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("moon maps"),
            size: (SUN_BASE + words(CAPACITY_SIDE)) as u64 * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, &[0u8; 16]);
        queue.write_buffer(&buffer, SUN_BASE as u64 * 4, &[0u8; 16]);
        Self { buffer }
    }

    pub fn group_entry(&self) -> wgpu::BindGroupEntry<'_> {
        wgpu::BindGroupEntry {
            binding: BINDING,
            resource: self.buffer.as_entire_binding(),
        }
    }

    /// Writes a packed bake ([`pack`]); one bigger than its room is refused (the old moon kept).
    pub fn write(&self, queue: &wgpu::Queue, words: &[u32]) -> bool {
        if words.len() > SUN_BASE {
            return false;
        }
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        queue.write_buffer(&self.buffer, 0, &bytes);
        true
    }

    /// Writes the sun world's packed bake ([`pack`], its albedo read) at [`SUN_BASE`]; one bigger
    /// than its room is refused (today's plain sun kept).
    pub fn write_sun(&self, queue: &wgpu::Queue, packed: &[u32]) -> bool {
        if packed.len() > words(CAPACITY_SIDE) {
            return false;
        }
        let bytes: Vec<u8> = packed.iter().flat_map(|w| w.to_le_bytes()).collect();
        queue.write_buffer(&self.buffer, SUN_BASE as u64 * 4, &bytes);
        true
    }
}

/// The shader's half: `dwell_moon_ground` (the sky's `dwell_moon` calls it) over the buffer, the
/// procedural moon of before while it is empty.
pub fn wgsl() -> String {
    format!(
        "@group(0) @binding({BINDING}) var<storage, read> moon_maps: array<u32>;
const MOON_MIN_SIDE: u32 = {MIN_SIDE}u;
const SUN_BASE: u32 = {SUN_BASE}u;
{MOON_WGSL}"
    )
}

const MOON_WGSL: &str = r"
// Texel (i, j) of the level at word `base`, `side` square (clamped): albedo and normal, unpacked.
fn moon_texel(base: u32, side: u32, t: vec2i) -> array<vec4f, 2> {
    let c = clamp(t, vec2i(0), vec2i(i32(side) - 1));
    let w = base + 2u * (u32(c.y) * side + u32(c.x));
    return array<vec4f, 2>(unpack4x8unorm(moon_maps[w]), unpack4x8unorm(moon_maps[w + 1u]));
}

// The moon's ground at `p` on its unit disc (x right, y up as the viewer sees it) for a pixel
// `px` disc radii wide: the baked albedo and normal (moon-local: z toward the viewer) of face A,
// bilinear in the mip level whose texel is about a pixel.
fn dwell_moon_ground(p: vec2f, px: f32) -> MoonGround {
    let size = moon_maps[0];
    if (size == 0u) {
        return dwell_moon_ground_procedural(p);
    }
    let levels = moon_maps[1];
    let per_px = px * f32(size) * 0.5;
    let lod = min(u32(max(floor(log2(max(per_px, 1.0))), 0.0)), levels - 1u);
    var base = 4u;
    var side = size;
    for (var k = 0u; k < lod; k++) {
        base += 2u * side * side;
        side = max(side / 2u, MOON_MIN_SIDE);
    }
    // Face-local x along +x, face-local z toward the viewer's down (a right-handed turn of face A's
    // up toward the viewer).
    let uv = vec2f(p.x, -p.y) * 0.5 + 0.5;
    let t = uv * f32(side) - 0.5;
    let i = vec2i(floor(t));
    let f = fract(t);
    let a = moon_texel(base, side, i);
    let b = moon_texel(base, side, i + vec2i(1, 0));
    let c = moon_texel(base, side, i + vec2i(0, 1));
    let d = moon_texel(base, side, i + vec2i(1, 1));
    let al = mix(mix(a[0], b[0], f.x), mix(c[0], d[0], f.x), f.y).r;
    if (moon_maps[2] == 0u) {
        return MoonGround(al, vec3f(0.0, 0.0, 1.0));
    }
    let n = mix(mix(a[1], b[1], f.x), mix(c[1], d[1], f.x), f.y).xyz * 2.0 - 1.0;
    return MoonGround(al, normalize(vec3f(n.x, -n.y, n.z)));
}

// The sun world's ground at `p` on its unit disc (as `dwell_moon_ground`'s) for a pixel `px` disc
// radii wide: its baked albedo over its brightest (lava 1, the crusts darker), 1 with no bake.
fn dwell_sun_ground(p: vec2f, px: f32) -> f32 {
    let size = moon_maps[SUN_BASE];
    if (size == 0u) {
        return 1.0;
    }
    let levels = moon_maps[SUN_BASE + 1u];
    let per_px = px * f32(size) * 0.5;
    let lod = min(u32(max(floor(log2(max(per_px, 1.0))), 0.0)), levels - 1u);
    var base = SUN_BASE + 4u;
    var side = size;
    for (var k = 0u; k < lod; k++) {
        base += 2u * side * side;
        side = max(side / 2u, MOON_MIN_SIDE);
    }
    let uv = vec2f(p.x, -p.y) * 0.5 + 0.5;
    let t = uv * f32(side) - 0.5;
    let i = vec2i(floor(t));
    let f = fract(t);
    let a = moon_texel(base, side, i)[0].r;
    let b = moon_texel(base, side, i + vec2i(1, 0))[0].r;
    let c = moon_texel(base, side, i + vec2i(0, 1))[0].r;
    let d = moon_texel(base, side, i + vec2i(1, 1))[0].r;
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}
";

#[cfg(test)]
mod tests {
    use super::*;

    fn bake() -> FarBake {
        let def = dwell_worldgen::grids::grid(MOON_GRID).expect("the moon grid");
        let t = dwell_worldgen::grids::CrateredTerrain::new(def, 7).expect("cratered");
        t.bake_far(dwell_world::Face::A, 64)
    }

    #[test]
    fn a_bake_packs_its_header_and_every_mip_level() {
        let b = bake();
        let w = pack(&b, true, 1.0);
        assert_eq!(&w[..4], &[64, 5, 1, 0], "64, 32, 16, 8, 4");
        assert_eq!(w.len(), words(64));
        assert_eq!(
            words(256),
            4 + 2 * (65536 + 16384 + 4096 + 1024 + 256 + 64 + 16)
        );
        // The brightest highland is full albedo (the moon as bright as before).
        let top = (0..64 * 64).map(|i| w[4 + 2 * i] & 0xff).max().unwrap();
        assert_eq!(top, 255);
    }

    #[test]
    fn the_normals_survive_packing_and_the_cpu_tier_has_none() {
        let b = bake();
        let w = pack(&b, true, 1.0);
        // Some texel leans (a crater wall), and every packed normal points out of face A.
        let mut leaning = 0;
        for i in 0..64 * 64 {
            let n = w[4 + 2 * i + 1];
            let (x, z, up) = (n & 0xff, n >> 8 & 0xff, n >> 16 & 0xff);
            assert!(up >= 128, "out of the ground");
            let src = &b.normal_height[i * 4..];
            assert!(u32::from(src[0]).abs_diff(x) <= 2 && u32::from(src[1]).abs_diff(z) <= 2);
            leaning += usize::from(x.abs_diff(128) > 1 || z.abs_diff(128) > 1);
        }
        assert!(leaning > 20, "craters lean the normals ({leaning})");
        // Relief steepens them, still out of the ground.
        let steep = pack(&b, true, 8.0);
        let lean = |w: &[u32]| {
            (0..64 * 64)
                .map(|i| (w[4 + 2 * i + 1] & 0xff).abs_diff(128))
                .max()
                .unwrap()
        };
        assert!(
            lean(&steep) > lean(&w) * 2,
            "{} vs {}",
            lean(&steep),
            lean(&w)
        );
        let flat = pack(&b, false, 1.0);
        assert_eq!(flat[2], 0);
        assert!(
            (0..64 * 64).all(|i| flat[4 + 2 * i + 1] == 128 | 128 << 8 | 255 << 16 | 255 << 24)
        );
        // The albedo is the same either way.
        assert!((0..64 * 64).all(|i| flat[4 + 2 * i] == w[4 + 2 * i]));
    }

    #[test]
    fn a_full_bake_fits_the_buffer() {
        let def = dwell_worldgen::grids::grid(MOON_GRID).unwrap();
        let t = dwell_worldgen::grids::CrateredTerrain::new(def, 1).unwrap();
        let b = t.bake_far_default(dwell_world::Face::A);
        assert!(
            pack(&b, true, 1.0).len() <= words(CAPACITY_SIDE),
            "bake.size {}",
            b.size
        );
    }
}

#[cfg(test)]
mod sun_tests {
    use super::*;

    #[test]
    fn the_sun_world_s_bake_packs_lava_brightest_and_fits_after_the_moon() {
        let b = sun_bake_for(1).expect("the sun grid");
        let w = pack(&b, false, 1.0);
        assert!(w.len() <= words(CAPACITY_SIDE), "fits its room");
        assert_eq!(SUN_BASE, words(CAPACITY_SIDE), "after the moon's room");
        let n = b.size * b.size;
        let lava = dwell_blocks::info(dwell_blocks::require("dwell:lava")).color;
        let mut crust = 0;
        for i in 0..n {
            let a = &b.albedo[i * 4..i * 4 + 4];
            if a[3] == 0 {
                continue;
            }
            let rgb = (u32::from(a[0]) << 16) | (u32::from(a[1]) << 8) | u32::from(a[2]);
            let packed = w[4 + 2 * i] & 0xff;
            if rgb != lava {
                assert!(packed < 160, "a crust is darker than the lava ({packed})");
                crust += 1;
            }
        }
        assert!(crust > 0);
        assert_eq!(
            (0..n).map(|i| w[4 + 2 * i] & 0xff).max(),
            Some(255),
            "lava full"
        );
    }

    #[test]
    fn the_sun_keeps_its_size_glow_and_light() {
        // Owner, 2026-11-12: the sun world is drawn as today's disc, "same size in the sky, its glow
        // and sunlight unchanged"; "The sun's doesn't really light the earth world or moon world,
        // just conceptually."
        let s = dwell_look::daynight::DAY_NIGHT.sun;
        assert_eq!(s.radius_deg, 1.5);
        assert_eq!(
            (s.bloom_deg, s.bloom, s.glow_deg, s.glow),
            (2.5, 0.2, 28.0, 0.3)
        );
        let wgsl = dwell_look::sky_wgsl_with(&wgsl());
        assert!(wgsl.contains("core = core * mix(1.0, ground, SUN_SURFACE);"));
        assert!(wgsl.contains("fn dwell_sun_ground(p: vec2f, px: f32) -> f32"));
        // Without a bake the plain sun: the procedural ground is 1.
        assert!(
            dwell_look::sky_wgsl()
                .contains("fn dwell_sun_ground(p: vec2f, px: f32) -> f32 {\n    return 1.0;")
        );
    }
}

#[cfg(test)]
mod look_tests {
    use dwell_look::daynight::DAY_NIGHT;

    #[test]
    fn the_moon_keeps_its_size_glow_and_brightness() {
        // Owner, 2026-11-12: the moon "should be the same size in the sky as it is now" and "glow
        // like it is now"; "the moon should look bright". M-2 changes its disc's shading only.
        let m = DAY_NIGHT.moon;
        assert_eq!(m.radius_deg, 1.2);
        assert_eq!(
            (m.bloom_deg, m.bloom, m.glow_deg, m.glow),
            (2.5, 0.45, 12.0, 0.14)
        );
        assert_eq!(m.earthshine, 0.06);
        let wgsl = dwell_look::sky_wgsl_with(&super::wgsl());
        assert!(
            wgsl.contains(&format!(
                "const MOON_RADIUS: f32 = {:?};",
                1.2f64.to_radians() as f32
            )) || wgsl.contains("const MOON_RADIUS: f32 = 0.0209")
        );
        // Lit by the baked ground, as bright as MOON_COLOR at full on the brightest highland.
        assert!(wgsl.contains("MOON_COLOR * g.albedo * (lit + MOON_EARTHSHINE)"));
        assert!(wgsl.contains("var<storage, read> moon_maps: array<u32>"));
    }

    #[test]
    fn the_sky_with_the_baked_moon_validates() {
        let src = format!(
            "{}\n{}\n{}\n@group(0) @binding(0) var<uniform> day: DaySky;\n\
             @fragment fn fs(@builtin(position) p: vec4f) -> @location(0) vec4f {{\n    \
             return dwell_moon(normalize(p.xyz), day, 0.001);\n}}\n",
            dwell_world::FaceFrame::wgsl(),
            dwell_look::daynight::DAY_SKY_WGSL,
            dwell_look::sky_wgsl_with(&super::wgsl()),
        );
        let module = naga::front::wgsl::parse_str(&src)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("valid");
    }
}
