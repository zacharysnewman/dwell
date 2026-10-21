//! Deterministic maths for the hydrology (ADR 0010): powers built from +, −, ×, ÷ and bit
//! manipulation only, so native and WASM builds agree bit for bit. `f64::powf`, `ln` and `exp` call
//! the platform's maths library, whose last bits differ between targets.

const LN2: f64 = std::f64::consts::LN_2;

/// log₂ x for x > 0 (finite, normal): the exponent from the bits, the mantissa in [√½, √2) by the
/// atanh series of (m − 1) / (m + 1). Accurate to a few ulps.
pub fn log2(x: f64) -> f64 {
    debug_assert!(x > 0.0 && x.is_finite());
    let bits = x.to_bits();
    let mut e = ((bits >> 52) & 0x7ff) as i64 - 1023;
    let mut m = f64::from_bits((bits & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000);
    if m > std::f64::consts::SQRT_2 {
        m *= 0.5;
        e += 1;
    }
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    // atanh(s) = s + s³/3 + s⁵/5 + …; |s| ≤ 0.1716, so 12 terms reach below 1e-18.
    let mut term = s;
    let mut sum = 0.0;
    let mut k = 1.0;
    for _ in 0..12 {
        sum += term / k;
        term *= s2;
        k += 2.0;
    }
    e as f64 + 2.0 * sum / LN2
}

/// 2^t: the whole part into the exponent bits, 2^f for f in [−½, ½] by its Taylor series.
pub fn exp2(t: f64) -> f64 {
    if t > 1023.0 {
        return f64::INFINITY;
    }
    if t < -1022.0 {
        return 0.0;
    }
    let k = (t + 0.5).floor();
    let f = (t - k) * LN2;
    let mut term = 1.0;
    let mut sum = 1.0;
    let mut i = 1.0;
    for _ in 0..18 {
        term *= f / i;
        sum += term;
        i += 1.0;
    }
    sum * f64::from_bits(((k as i64 + 1023) as u64) << 52)
}

/// x^y for x ≥ 0, with exact fast paths for the exponents the profiles use (1, ½, 2).
pub fn pow(x: f64, y: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if y == 1.0 {
        x
    } else if y == 0.5 {
        x.sqrt()
    } else if y == 2.0 {
        x * x
    } else if y == 0.0 {
        1.0
    } else {
        exp2(y * log2(x))
    }
}

/// (sin x, cos x) by Taylor series after reducing x to [−π, π] (whole turns removed with `floor`):
/// deterministic, accurate to ~1e-15, for directions in data (degrees → a unit vector).
pub fn sin_cos(x: f64) -> (f64, f64) {
    let tau = 2.0 * std::f64::consts::PI;
    let x = x - tau * (x / tau + 0.5).floor();
    let x2 = x * x;
    let (mut s, mut c) = (0.0, 0.0);
    let (mut ts, mut tc) = (x, 1.0);
    let mut k = 1.0;
    for _ in 0..16 {
        s += ts;
        c += tc;
        ts *= -x2 / ((k + 1.0) * (k + 2.0));
        tc *= -x2 / (k * (k + 1.0));
        k += 2.0;
    }
    (s, c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powers_match_the_platform_closely() {
        for &x in &[1e-6, 0.3, 1.0, 2.0, 7.5, 1234.5, 3.3e7, 9.9e12] {
            for &y in &[0.35, 0.4, 0.5, 0.6, 1.0, 1.3, 2.0] {
                let ours = pow(x, y);
                let theirs = f64::powf(x, y);
                assert!(
                    ((ours - theirs) / theirs).abs() < 1e-13,
                    "{x}^{y}: {ours} vs {theirs}"
                );
            }
        }
        assert_eq!(pow(0.0, 0.5), 0.0);
        assert_eq!(log2(8.0), 3.0);
        assert_eq!(exp2(-3.0), 0.125);
    }

    #[test]
    fn sines_and_cosines_match_the_platform_closely() {
        for i in -40..=40 {
            let x = f64::from(i) * 0.37;
            let (s, c) = sin_cos(x);
            assert!(
                (s - x.sin()).abs() < 1e-13 && (c - x.cos()).abs() < 1e-13,
                "{x}"
            );
        }
    }
}
