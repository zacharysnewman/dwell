//! The volumetric clouds' tiling 3D noise (docs/CLOUDS.md §9.3), baked once on the CPU when a
//! volume tier first draws: the **base** texture (RGBA: a Perlin–Worley in R and three octaves of
//! Worley fBm in G, B, A, Schneider's "low-frequency" noise) and the **detail** texture (R a Worley
//! fBm three octaves finer, G and B the curl of a Perlin field across, the wisps' distortion; A the
//! Perlin field itself). Every lattice is periodic over the texture, so both tile seamlessly in all
//! three axes; the shader samples them with a repeating, trilinear sampler, and [`Noise3::sample`]
//! is that sampler's twin for the CPU reference (`crate::clouds`' shape model).
//!
//! The sizes are data (`clouds.json` `form.baseSize`, `form.detailSize`); the bake's cost and the
//! textures' memory are in docs/CLOUDS.md §9.4.

/// A baked RGBA8 3D texture, `size`³ texels, x fastest then y then z (`queue.write_texture`'s
/// order).
#[derive(Clone, Debug, PartialEq)]
pub struct Noise3 {
    pub size: u32,
    pub texels: Vec<[u8; 4]>,
}

impl Noise3 {
    /// The texel at whole coordinates, wrapped.
    pub fn texel(&self, x: i64, y: i64, z: i64) -> [u8; 4] {
        let n = i64::from(self.size);
        let (x, y, z) = (x.rem_euclid(n), y.rem_euclid(n), z.rem_euclid(n));
        self.texels[((z * n + y) * n + x) as usize]
    }

    /// The trilinear, repeating sample at `uvw` (one texture's width per unit): the GPU sampler's
    /// twin (texel centres at (i + ½) / size), each channel 0–1.
    pub fn sample(&self, uvw: [f64; 3]) -> [f64; 4] {
        let n = f64::from(self.size);
        let p = uvw.map(|c| c * n - 0.5);
        let i = p.map(f64::floor);
        let f = [p[0] - i[0], p[1] - i[1], p[2] - i[2]];
        let (x, y, z) = (i[0] as i64, i[1] as i64, i[2] as i64);
        let mut out = [0.0; 4];
        for corner in 0..8 {
            let (dx, dy, dz) = (corner & 1, (corner >> 1) & 1, (corner >> 2) & 1);
            let w = [(dx, f[0]), (dy, f[1]), (dz, f[2])]
                .iter()
                .map(|&(d, f)| if d == 1 { f } else { 1.0 - f })
                .product::<f64>();
            let t = self.texel(x + dx, y + dy, z + dz);
            for c in 0..4 {
                out[c] += w * f64::from(t[c]) / 255.0;
            }
        }
        out
    }

    /// The texture's bytes, for the upload.
    pub fn bytes(&self) -> Vec<u8> {
        self.texels.iter().flatten().copied().collect()
    }
}

fn hash(x: i64, y: i64, z: i64, period: i64, seed: u32) -> u32 {
    let (x, y, z) = (
        x.rem_euclid(period) as u32,
        y.rem_euclid(period) as u32,
        z.rem_euclid(period) as u32,
    );
    let mut h = x.wrapping_mul(0x8da6_b343)
        ^ y.wrapping_mul(0xd816_3841)
        ^ z.wrapping_mul(0xcb1a_b31f)
        ^ seed.wrapping_mul(0x1656_67b1);
    h = (h ^ (h >> 16)).wrapping_mul(0x7feb_352d);
    h = (h ^ (h >> 15)).wrapping_mul(0x846c_a68b);
    h ^ (h >> 16)
}

fn unit(h: u32) -> f64 {
    f64::from(h) / 4_294_967_295.0
}

/// Gradient (Perlin) noise at `p` (cells) on a lattice that repeats every `period` cells, about
/// −1 to 1.
pub fn perlin(p: [f64; 3], period: i64, seed: u32) -> f64 {
    // The twelve edge directions of a cube.
    const G: [[f64; 3]; 12] = [
        [1.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0],
        [1.0, -1.0, 0.0],
        [-1.0, -1.0, 0.0],
        [1.0, 0.0, 1.0],
        [-1.0, 0.0, 1.0],
        [1.0, 0.0, -1.0],
        [-1.0, 0.0, -1.0],
        [0.0, 1.0, 1.0],
        [0.0, -1.0, 1.0],
        [0.0, 1.0, -1.0],
        [0.0, -1.0, -1.0],
    ];
    let i = p.map(f64::floor);
    let f = [p[0] - i[0], p[1] - i[1], p[2] - i[2]];
    let u = f.map(|f| f * f * f * (f * (f * 6.0 - 15.0) + 10.0));
    let (x, y, z) = (i[0] as i64, i[1] as i64, i[2] as i64);
    let corner = |dx: i64, dy: i64, dz: i64| {
        let g = G[(hash(x + dx, y + dy, z + dz, period, seed) % 12) as usize];
        g[0] * (f[0] - dx as f64) + g[1] * (f[1] - dy as f64) + g[2] * (f[2] - dz as f64)
    };
    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
    let x00 = lerp(corner(0, 0, 0), corner(1, 0, 0), u[0]);
    let x10 = lerp(corner(0, 1, 0), corner(1, 1, 0), u[0]);
    let x01 = lerp(corner(0, 0, 1), corner(1, 0, 1), u[0]);
    let x11 = lerp(corner(0, 1, 1), corner(1, 1, 1), u[0]);
    lerp(lerp(x00, x10, u[1]), lerp(x01, x11, u[1]), u[2])
}

/// One periodic Worley lattice: a feature point per cell, `period` cells across, kept padded by a
/// cell on every side (the wrapped neighbours) so the search needs no wrapping.
struct Worley {
    period: i64,
    /// (period + 2)³ feature points in cells, x fastest, the cell at -1 first.
    points: Vec<[f64; 3]>,
}

impl Worley {
    fn new(period: i64, seed: u32) -> Self {
        let m = period + 2;
        let mut points = Vec::with_capacity((m * m * m) as usize);
        for z in -1..=period {
            for y in -1..=period {
                for x in -1..=period {
                    let h = hash(x, y, z, period, seed);
                    let h2 = hash(x, y, z, period, seed ^ 0x9e37_79b9);
                    let h3 = hash(x, y, z, period, seed ^ 0x85eb_ca6b);
                    points.push([x as f64 + unit(h), y as f64 + unit(h2), z as f64 + unit(h3)]);
                }
            }
        }
        Self { period, points }
    }

    /// The inverted distance to the nearest feature point (1 at a point, 0 a cell or more away):
    /// round cells, the billows. `p` in cells, wrapped into one period first.
    fn cells(&self, p: [f64; 3]) -> f64 {
        let n = self.period;
        let p = p.map(|c| c.rem_euclid(n as f64));
        let m = (n + 2) as usize;
        let i = p.map(|c| (c.floor() as i64).clamp(0, n - 1) as usize);
        let mut best = f64::INFINITY;
        for z in i[2]..i[2] + 3 {
            for y in i[1]..i[1] + 3 {
                let row = (z * m + y) * m;
                for f in &self.points[row + i[0]..row + i[0] + 3] {
                    let d = [f[0] - p[0], f[1] - p[1], f[2] - p[2]];
                    best = best.min(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
                }
            }
        }
        1.0 - best.sqrt().min(1.0)
    }
}

/// Three octaves (f, 2f, 4f) of inverted Worley, weighted ⅝, ¼, ⅛ (Schneider's fBm).
fn worley_fbm(w: &[Worley], at: usize, u: [f64; 3]) -> f64 {
    let o = |k: usize| w[at + k].cells(u.map(|c| c * w[at + k].period as f64));
    0.625 * o(0) + 0.25 * o(1) + 0.125 * o(2)
}

/// Three octaves of Perlin noise mapped to 0–1, the lowest `freq` cells across.
fn perlin_fbm(u: [f64; 3], freq: i64, seed: u32) -> f64 {
    let mut sum = 0.0;
    let (mut amp, mut f) = (1.0, freq);
    for o in 0..3 {
        sum += amp * perlin(u.map(|c| c * f as f64), f, seed + o);
        amp *= 0.5;
        f *= 2;
    }
    (0.5 + 0.5 * sum / 1.75 * 1.6).clamp(0.0, 1.0)
}

fn byte(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The base noise, `size`³: R the Perlin–Worley (a three-octave Perlin fBm 4 cells across, dilated
/// by the Worley fBm 4 cells across: `remap(perlin, 0, 1, worley, 1)`, billowy blobs), G, B, A the
/// Worley fBm 4, 8 and 16 cells across. `size` a power of two of at least 16.
pub fn bake_base(size: u32) -> Noise3 {
    Noise3 {
        size,
        texels: bake_base_slab(size, 0, size),
    }
}

/// [`bake_base`]'s texels for the slices `z0..z1` only (a worker job's share; the slabs
/// concatenated in order are the whole texture).
pub fn bake_base_slab(size: u32, z0: u32, z1: u32) -> Vec<[u8; 4]> {
    let w: Vec<Worley> = [4, 8, 16, 32, 64]
        .iter()
        .enumerate()
        .map(|(k, &p)| Worley::new(p, 11 + k as u32))
        .collect();
    bake(size, z0, z1, |_, u| {
        let g = worley_fbm(&w, 0, u);
        let b = worley_fbm(&w, 1, u);
        let a = worley_fbm(&w, 2, u);
        let perlin = perlin_fbm(u, 4, 3);
        let r = g + perlin * (1.0 - g);
        [byte(r), byte(g), byte(b), byte(a)]
    })
}

/// The detail noise, `size`³: R the Worley fBm 4 cells across (8 and 16 its finer octaves), G and
/// B the curl across (x, z) of a Perlin field 2 cells across, mapped from −1–1 to 0–1, A a blue-noise
/// tile ([`blue_noise`], `size`², the same in every slice: the march's per-pixel jitter, read by
/// pixel). `size` a power of two of at least 8.
pub fn bake_detail(size: u32) -> Noise3 {
    Noise3 {
        size,
        texels: bake_detail_slab(size, 0, size),
    }
}

/// [`bake_detail`]'s texels for the slices `z0..z1`.
pub fn bake_detail_slab(size: u32, z0: u32, z1: u32) -> Vec<[u8; 4]> {
    let blue = blue_noise(size);
    let w: Vec<Worley> = [4, 8, 16]
        .iter()
        .enumerate()
        .map(|(k, &p)| Worley::new(p, 31 + k as u32))
        .collect();
    let psi = |u: [f64; 3]| perlin(u.map(|c| c * 2.0), 2, 41);
    let e = 0.5 / f64::from(size);
    bake(size, z0, z1, |[x, y, _], u| {
        let r = worley_fbm(&w, 0, u);
        // ψ's gradient across by central differences (in cells of the field): curl = (∂ψ/∂z, −∂ψ/∂x).
        let dx = (psi([u[0] + e, u[1], u[2]]) - psi([u[0] - e, u[1], u[2]])) / (2.0 * e * 2.0);
        let dz = (psi([u[0], u[1], u[2] + e]) - psi([u[0], u[1], u[2] - e])) / (2.0 * e * 2.0);
        let c = |v: f64| 0.5 + 0.5 * (v / CURL_RANGE).clamp(-1.0, 1.0);
        [
            byte(r),
            byte(c(dz)),
            byte(c(-dx)),
            blue[y as usize * size as usize + x as usize],
        ]
    })
}

/// The curl's range stored in the detail texture's G and B (cells of the field per cell): ±this
/// maps to 0–1. The shader's `CLOUD_CURL_RANGE`.
pub const CURL_RANGE: f64 = 2.0;

fn bake(size: u32, z0: u32, z1: u32, at: impl Fn([u32; 3], [f64; 3]) -> [u8; 4]) -> Vec<[u8; 4]> {
    let n = size;
    let mut texels = Vec::with_capacity((n * n * z1.saturating_sub(z0)) as usize);
    let s = f64::from(size);
    for z in z0..z1.min(n) {
        for y in 0..n {
            for x in 0..n {
                texels.push(at(
                    [x, y, z],
                    [
                        (f64::from(x) + 0.5) / s,
                        (f64::from(y) + 0.5) / s,
                        (f64::from(z) + 0.5) / s,
                    ],
                ));
            }
        }
    }
    texels
}

/// A `size` × `size` tile of blue noise (each value 0–255 once per 256 pixels' share, ranked by
/// Ulichney's void-and-cluster with a toroidal Gaussian of σ 1.5, so it tiles): neighbouring pixels
/// get far-apart values and the error is high-frequency, which the clouds' accumulation and the eye
/// average away (BUGS.md #90; the white-ish gradient noise before left low-frequency clumps).
pub fn blue_noise(size: u32) -> Vec<u8> {
    let n = size as usize;
    let count = n * n;
    // The Gaussian over toroidal offsets.
    let sigma2 = 2.0 * 1.5 * 1.5;
    let wrap = |d: usize| d.min(n - d) as f64;
    let kernel: Vec<f64> = (0..count)
        .map(|i| {
            let (dx, dy) = (wrap(i % n), wrap(i / n));
            (-(dx * dx + dy * dy) / sigma2).exp()
        })
        .collect();
    let mut energy = vec![0.0; count];
    let mut on = vec![false; count];
    let splat = |energy: &mut [f64], at: usize, sign: f64| {
        let (ax, ay) = (at % n, at / n);
        for (i, e) in energy.iter_mut().enumerate() {
            let (x, y) = (i % n, i / n);
            let k = ((y + n - ay) % n) * n + (x + n - ax) % n;
            *e += sign * kernel[k];
        }
    };
    // The initial pattern: a tenth of the pixels, hashed, then relaxed (the tightest cluster moved
    // to the largest void) until stable.
    let ones = (count / 10).max(1);
    let mut placed = 0;
    let mut h = 0u32;
    while placed < ones {
        let i = (hash(i64::from(h), 7, 0, 1 << 32, 5) as usize) % count;
        h += 1;
        if !on[i] {
            on[i] = true;
            splat(&mut energy, i, 1.0);
            placed += 1;
        }
    }
    let pick = |energy: &[f64], on: &[bool], want: bool, max: bool| {
        let mut best = None::<(usize, f64)>;
        for i in 0..count {
            if on[i] != want {
                continue;
            }
            let e = energy[i];
            if best.is_none_or(|(_, b)| if max { e > b } else { e < b }) {
                best = Some((i, e));
            }
        }
        best.map(|(i, _)| i)
    };
    for _ in 0..count {
        let tight = pick(&energy, &on, true, true).unwrap();
        on[tight] = false;
        splat(&mut energy, tight, -1.0);
        let void = pick(&energy, &on, false, false).unwrap();
        on[void] = true;
        splat(&mut energy, void, 1.0);
        if void == tight {
            break;
        }
    }
    let mut rank = vec![0usize; count];
    // Phase 1: the pattern's own pixels, tightest cluster first, take the ranks below `ones`.
    let (mut e1, mut o1) = (energy.clone(), on.clone());
    for r in (0..ones).rev() {
        let tight = pick(&e1, &o1, true, true).unwrap();
        o1[tight] = false;
        splat(&mut e1, tight, -1.0);
        rank[tight] = r;
    }
    // Phase 2: the largest voids take the rest in order.
    for r in ones..count {
        let void = pick(&energy, &on, false, false).unwrap();
        on[void] = true;
        splat(&mut energy, void, 1.0);
        rank[void] = r;
    }
    rank.iter()
        .map(|&r| ((r * 256) / count).min(255) as u8)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_noise_tiles_seamlessly_in_every_axis() {
        let base = bake_base(16);
        let detail = bake_detail(8);
        for tex in [&base, &detail] {
            // The sample a hair inside one edge matches the sample a hair inside the opposite one, as
            // across any texel boundary: no seam where the texture repeats.
            for k in 0..3 {
                for i in 0..20 {
                    let mut a = [0.13 + 0.037 * i as f64, 0.71 - 0.029 * i as f64, 0.4];
                    let mut b = a;
                    a[k] = 1.0 - 1e-6;
                    b[k] = 1e-6;
                    let (sa, sb) = (tex.sample(a), tex.sample(b));
                    for c in 0..4 {
                        assert!((sa[c] - sb[c]).abs() < 1e-4, "axis {k} channel {c}");
                    }
                }
            }
            // And the lattice itself repeats: a whole period over is the same sample.
            let p = [0.31, 0.62, 0.17];
            let (a, b) = (
                tex.sample(p),
                tex.sample([p[0] + 1.0, p[1] - 2.0, p[2] + 3.0]),
            );
            assert!((0..4).all(|c| (a[c] - b[c]).abs() < 1e-9));
        }
        // The continuous fields wrap too (what the bake reads at the edges).
        let w = Worley::new(4, 1);
        assert!((w.cells([0.0, 1.3, 2.2]) - w.cells([4.0, 1.3, 2.2])).abs() < 1e-12);
        assert!((perlin([0.0, 1.3, 2.2], 4, 2) - perlin([4.0, 1.3, 2.2], 4, 2)).abs() < 1e-12);
    }

    #[test]
    fn the_channels_have_the_ranges_the_shape_model_expects() {
        let base = bake_base(16);
        let mean = |c: usize| {
            base.texels.iter().map(|t| f64::from(t[c])).sum::<f64>()
                / (255.0 * base.texels.len() as f64)
        };
        // Perlin–Worley sits above its Worley (the dilation); every channel spans most of 0–1.
        assert!(mean(0) > mean(1));
        for c in 0..4 {
            let lo = base.texels.iter().map(|t| t[c]).min().unwrap();
            let hi = base.texels.iter().map(|t| t[c]).max().unwrap();
            assert!(hi - lo > 100, "channel {c}: {lo}–{hi}");
        }
        // The curl is centred on 0 (0.5 stored).
        let detail = bake_detail(16);
        let curl: f64 = detail.texels.iter().map(|t| f64::from(t[1])).sum::<f64>()
            / (255.0 * detail.texels.len() as f64);
        assert!((curl - 0.5).abs() < 0.05, "{curl}");
    }

    #[test]
    fn the_blue_noise_spreads_values_and_has_little_low_frequency_energy() {
        let n = 16;
        let b = blue_noise(n as u32);
        // Every value band is used evenly.
        let mut bins = [0; 4];
        for &v in &b {
            bins[v as usize / 64] += 1;
        }
        assert!(bins.iter().all(|&c| c == n * n / 4), "{bins:?}");
        // Neighbours differ more than white noise's would (≈ 85 of 255 apart on average): the
        // low-frequency energy is small.
        let at = |x: usize, y: usize| f64::from(b[(y % n) * n + x % n]);
        let mut diff = 0.0;
        for y in 0..n {
            for x in 0..n {
                diff += (at(x, y) - at(x + 1, y)).abs() + (at(x, y) - at(x, y + 1)).abs();
            }
        }
        let mean = diff / (2.0 * (n * n) as f64);
        assert!(mean > 95.0, "neighbours {mean:.1} apart");
        // The slabs put together are the whole texture.
        let whole = bake_detail(8);
        let parts: Vec<_> = [bake_detail_slab(8, 0, 3), bake_detail_slab(8, 3, 8)].concat();
        assert_eq!(whole.texels, parts);
        assert_eq!(
            bake_base(16).texels,
            [bake_base_slab(16, 0, 9), bake_base_slab(16, 9, 16)].concat()
        );
    }

    #[test]
    #[ignore = "timing: cargo test -p dwell-look --release -- --ignored bake_cost --nocapture"]
    fn bake_cost() {
        for (name, size, f) in [
            ("base", 64, bake_base as fn(u32) -> Noise3),
            ("base", 128, bake_base),
            ("detail", 32, bake_detail),
        ] {
            let t = std::time::Instant::now();
            let n = f(size);
            println!(
                "{name} {size}³: {:.0} ms, {} KiB",
                t.elapsed().as_secs_f64() * 1e3,
                n.texels.len() * 4 / 1024
            );
        }
    }
}
