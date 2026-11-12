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

use dwell_worldgen::grids::FarBake;

/// The frame group's binding.
pub const BINDING: u32 = 20;

/// The smallest mip level's side (texels): below it the disc is a dot of a pixel or two.
pub const MIN_SIDE: usize = 4;

/// The grid the sky's moon is (`shared/worldgen/grids.json`).
pub const MOON_GRID: &str = "dwell:moon";

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

/// The buffer: room for a full 256² bake's levels (written in place, so the frame group never
/// changes), the header saying "none yet" until one is written.
pub struct MoonGpu {
    pub buffer: wgpu::Buffer,
}

/// Words a bake of side `size` packs to.
pub fn words(size: usize) -> usize {
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

impl MoonGpu {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("moon maps"),
            size: words(CAPACITY_SIDE) as u64 * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, &[0u8; 16]);
        Self { buffer }
    }

    pub fn group_entry(&self) -> wgpu::BindGroupEntry<'_> {
        wgpu::BindGroupEntry {
            binding: BINDING,
            resource: self.buffer.as_entire_binding(),
        }
    }

    /// Writes a packed bake ([`pack`]); one bigger than the buffer is refused (the old moon kept).
    pub fn write(&self, queue: &wgpu::Queue, words: &[u32]) -> bool {
        if words.len() as u64 * 4 > self.buffer.size() {
            return false;
        }
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        queue.write_buffer(&self.buffer, 0, &bytes);
        true
    }
}

/// The shader's half: `dwell_moon_ground` (the sky's `dwell_moon` calls it) over the buffer, the
/// procedural moon of before while it is empty.
pub fn wgsl() -> String {
    format!(
        "@group(0) @binding({BINDING}) var<storage, read> moon_maps: array<u32>;
const MOON_MIN_SIDE: u32 = {MIN_SIDE}u;
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
