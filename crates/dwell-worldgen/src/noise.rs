//! Deterministic noise (ARCHITECTURE.md §6.3, ADR 0010). Gradients come from integer hashing and
//! evaluation is strict IEEE float — only +, −, × and comparisons (Rust never contracts to FMA) and
//! the correctly rounded square root — so native and WASM builds are bit-identical, which the
//! golden tests check.

/// Avalanche mix of a 32-bit value (murmur3 finalizer).
pub const fn mix32(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    h
}

pub const fn hash2(seed: u32, x: i32, z: i32) -> u32 {
    mix32(seed ^ (x as u32).wrapping_mul(0x27D4_EB2F) ^ (z as u32).wrapping_mul(0x1656_67B1))
}

pub const fn hash3(seed: u32, x: i32, y: i32, z: i32) -> u32 {
    mix32(
        seed ^ (x as u32).wrapping_mul(0x27D4_EB2F)
            ^ (y as u32).wrapping_mul(0xD3A2_646D)
            ^ (z as u32).wrapping_mul(0x1656_67B1),
    )
}

/// A 32-bit seed for one noise stream of a world.
pub const fn seed_word(world_seed: u64, stream: u32) -> u32 {
    let lo = world_seed as u32;
    let hi = (world_seed >> 32) as u32;
    mix32(mix32(lo ^ mix32(stream.wrapping_mul(0x9E37_79B9))) ^ mix32(hi.wrapping_add(stream)))
}

/// Uniform [0, 1) from a hash (24 bits, exact in float).
pub fn unit(h: u32) -> f32 {
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

pub fn floor_to_int(v: f32) -> i32 {
    let i = v as i32;
    if v < i as f32 { i - 1 } else { i }
}

pub const fn floor_div(a: i32, b: i32) -> i32 {
    a / b
        - if a % b != 0 && ((a < 0) != (b < 0)) {
            1
        } else {
            0
        }
}
pub const fn floor_mod(a: i32, b: i32) -> i32 {
    a - floor_div(a, b) * b
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
pub fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}
/// Cubic smoothstep of v over [e0, e1] (e0 > e1 allowed: a falling edge).
pub fn smooth_step(e0: f32, e1: f32, v: f32) -> f32 {
    let t = clamp01((v - e0) / (e1 - e0));
    t * t * (3.0 - 2.0 * t)
}

/// A coordinate on a noise lattice: its integer cell and the float offset within it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LatticeCoord {
    pub cell: i32,
    /// [0, 1)
    pub frac: f32,
}

/// World coordinate `p` (m) on a lattice of spacing `wavelength / 2^octave`. The split is exact
/// integer arithmetic; only the offset within the cell is a float (one correctly rounded division),
/// so noise is equally precise anywhere in the world (ADR 0011).
pub fn lattice(p: i64, wavelength: i32, octave: u32) -> LatticeCoord {
    let v = p * (1i64 << octave);
    let w = i64::from(wavelength);
    let mut cell = v / w;
    let mut rem = v % w;
    if rem < 0 {
        cell -= 1;
        rem += w;
    }
    LatticeCoord {
        cell: cell as i32,
        frac: rem as f32 / wavelength as f32,
    }
}

/// Quintic fade 6t⁵ − 15t⁴ + 10t³.
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Eight unnormalised directions: axes and diagonals.
fn grad2(h: u32, x: f32, z: f32) -> f32 {
    match h & 7 {
        0 => x + z,
        1 => -x + z,
        2 => x - z,
        3 => -x - z,
        4 => x,
        5 => -x,
        6 => z,
        _ => -z,
    }
}

/// Per-octave seeds: independent streams.
fn octave_seed(seed: u32, octave: u32) -> u32 {
    mix32(seed.wrapping_add(octave.wrapping_mul(0x9E37_79B9)))
}

/// Gradient noise, roughly in [−1, 1], zero at lattice points.
pub fn perlin2(seed: u32, x: LatticeCoord, z: LatticeCoord) -> f32 {
    let (x0, z0, fx, fz) = (x.cell, z.cell, x.frac, z.frac);
    let u = fade(fx);
    let v = fade(fz);
    let n00 = grad2(hash2(seed, x0, z0), fx, fz);
    let n10 = grad2(hash2(seed, x0.wrapping_add(1), z0), fx - 1.0, fz);
    let n01 = grad2(hash2(seed, x0, z0.wrapping_add(1)), fx, fz - 1.0);
    let n11 = grad2(
        hash2(seed, x0.wrapping_add(1), z0.wrapping_add(1)),
        fx - 1.0,
        fz - 1.0,
    );
    lerp(lerp(n00, n10, u), lerp(n01, n11, u), v)
}

fn split(v: f32) -> LatticeCoord {
    let cell = floor_to_int(v);
    LatticeCoord {
        cell,
        frac: v - cell as f32,
    }
}

/// Float-coordinate form, for small (local) coordinates only: tests and tools.
pub fn perlin2f(seed: u32, x: f32, z: f32) -> f32 {
    perlin2(seed, split(x), split(z))
}

/// Fractal sum normalised by the total amplitude. Each octave doubles the frequency, halves the
/// amplitude and has its own seed. `kept` (level of detail) evaluates only the first `kept`
/// octaves, still normalised by the total, so dropped octaves count as their mean (zero).
pub fn fbm2(seed: u32, x: i64, z: i64, wavelength: i32, octaves: u32, kept: Option<u32>) -> f32 {
    let mut sum = 0.0f32;
    let mut norm = 0.0f32;
    let mut amplitude = 1.0f32;
    for o in 0..octaves {
        if kept.is_none_or(|k| o < k) {
            sum += perlin2(
                octave_seed(seed, o),
                lattice(x, wavelength, o),
                lattice(z, wavelength, o),
            ) * amplitude;
        }
        norm += amplitude;
        amplitude *= 0.5;
    }
    sum / norm
}

/// A hashed offset (m) to shift a field's lattice by: 17..4,016, never a multiple of 16, so round
/// world coordinates do not all sit on lattice points, where gradient noise is zero.
pub const fn noise_offset(seed: u32) -> i64 {
    let o = 17 + (mix32(seed) % 4000) as i64;
    if o % 16 == 0 { o + 1 } else { o }
}

/// Octaves of a fractal sum whose lattice spacing is at least `cell` m: the ones a level-of-detail
/// cell of that size resolves (§6.6).
pub const fn octaves_resolved(wavelength: i32, octaves: u32, cell: i64) -> u32 {
    let mut n = 0;
    while n < octaves && ((wavelength as i64) >> n) >= cell {
        n += 1;
    }
    n
}

/// Twelve unnormalised directions: the cube's edge midpoints (3D gradient noise).
fn grad3(h: u32, x: f32, y: f32, z: f32) -> f32 {
    match h % 12 {
        0 => x + y,
        1 => -x + y,
        2 => x - y,
        3 => -x - y,
        4 => x + z,
        5 => -x + z,
        6 => x - z,
        7 => -x - z,
        8 => y + z,
        9 => -y + z,
        10 => y - z,
        _ => -y - z,
    }
}

/// 3D gradient noise, roughly in [−1, 1], zero at lattice points.
pub fn perlin3(seed: u32, x: LatticeCoord, y: LatticeCoord, z: LatticeCoord) -> f32 {
    let (fx, fy, fz) = (x.frac, y.frac, z.frac);
    let (u, v, w) = (fade(fx), fade(fy), fade(fz));
    let corner = |dx: i32, dy: i32, dz: i32| {
        let h = hash3(
            seed,
            x.cell.wrapping_add(dx),
            y.cell.wrapping_add(dy),
            z.cell.wrapping_add(dz),
        );
        grad3(h, fx - dx as f32, fy - dy as f32, fz - dz as f32)
    };
    let y0 = lerp(
        lerp(corner(0, 0, 0), corner(1, 0, 0), u),
        lerp(corner(0, 0, 1), corner(1, 0, 1), u),
        w,
    );
    let y1 = lerp(
        lerp(corner(0, 1, 0), corner(1, 1, 0), u),
        lerp(corner(0, 1, 1), corner(1, 1, 1), u),
        w,
    );
    lerp(y0, y1, v)
}

/// [`fbm2`]'s fractal sum in 3D: x and z (m) on lattices of `wavelength`, y (rows) on one of
/// `wavelength_y` rows, each octave twice as fine.
pub fn fbm3(seed: u32, (x, y, z): (i64, i64, i64), wavelength: i32, wavelength_y: i32, octaves: u32) -> f32 {
    let mut sum = 0.0f32;
    let mut norm = 0.0f32;
    let mut amplitude = 1.0f32;
    for o in 0..octaves {
        sum += perlin3(
            octave_seed(seed, o),
            lattice(x, wavelength, o),
            lattice(y, wavelength_y, o),
            lattice(z, wavelength, o),
        ) * amplitude;
        norm += amplitude;
        amplitude *= 0.5;
    }
    sum / norm
}
