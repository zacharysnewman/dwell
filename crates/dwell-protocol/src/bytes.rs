//! Little-endian byte reading and writing (ARCHITECTURE.md §8.2: all fields little-endian), and
//! IEEE 754 binary16 conversions.

/// Round-to-nearest-even float → half (after F. Giesen's `float_to_half_fast3_rtne`); overflow
/// goes to ±inf, NaN stays NaN.
pub fn float_to_half(f: f32) -> u16 {
    let mut x = f.to_bits();
    let sign = x & 0x8000_0000;
    x ^= sign;
    let out = if x >= 0x4780_0000 {
        // ≥ 65536, inf or NaN
        if x > 0x7F80_0000 { 0x7E00 } else { 0x7C00 }
    } else if x < 0x3880_0000 {
        // Subnormal half or zero: let float addition round.
        let magic = f32::from_bits(0x3F00_0000); // 0.5
        (f32::from_bits(x) + magic)
            .to_bits()
            .wrapping_sub(0x3F00_0000)
    } else {
        let mantissa_odd = (x >> 13) & 1;
        x = x.wrapping_add(0xC800_0FFF); // rebias the exponent (15 − 127) and round
        x = x.wrapping_add(mantissa_odd);
        x >> 13
    };
    (out | (sign >> 16)) as u16
}

pub fn half_to_float(h: u16) -> f32 {
    let sign = u32::from(h & 0x8000) << 16;
    let exponent = u32::from((h >> 10) & 0x1F);
    let mantissa = u32::from(h & 0x3FF);
    if exponent == 0 {
        let value = mantissa as f32 * (1.0 / 16_777_216.0); // 2^-24
        return f32::from_bits(value.to_bits() | sign);
    }
    if exponent == 31 {
        return f32::from_bits(sign | 0x7F80_0000 | (mantissa << 13));
    }
    f32::from_bits(sign | ((exponent + 112) << 23) | (mantissa << 13))
}

/// Appends little-endian fields to a buffer.
pub struct ByteWriter<'a> {
    out: &'a mut Vec<u8>,
}

impl<'a> ByteWriter<'a> {
    pub fn new(out: &'a mut Vec<u8>) -> Self {
        Self { out }
    }
    pub fn u8(&mut self, v: u8) {
        self.out.push(v);
    }
    pub fn u16(&mut self, v: u16) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u64(&mut self, v: u64) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i8(&mut self, v: i8) {
        self.u8(v as u8);
    }
    pub fn i16(&mut self, v: i16) {
        self.u16(v as u16);
    }
    pub fn i32(&mut self, v: i32) {
        self.u32(v as u32);
    }
    pub fn f32(&mut self, v: f32) {
        self.u32(v.to_bits());
    }
    pub fn f64(&mut self, v: f64) {
        self.u64(v.to_bits());
    }
    pub fn f16(&mut self, v: f32) {
        self.u16(float_to_half(v));
    }
    pub fn bytes(&mut self, b: &[u8]) {
        self.out.extend_from_slice(b);
    }
    /// UTF-8 string: u16 byte length, then the bytes. Callers clamp to per-field limits first.
    pub fn str(&mut self, s: &str) {
        self.u16(s.len() as u16);
        self.out.extend_from_slice(s.as_bytes());
    }
}

/// Bounds-checked little-endian reader. Any failure latches `ok() == false`; reads after a failure
/// return zeros, so decoders read every field and check once at the end.
pub struct ByteReader<'a> {
    input: &'a [u8],
    pos: usize,
    ok: bool,
}

macro_rules! read_le {
    ($name:ident, $t:ty) => {
        pub fn $name(&mut self) -> $t {
            const N: usize = std::mem::size_of::<$t>();
            match self.take(N) {
                Some(b) => <$t>::from_le_bytes(b.try_into().expect("N bytes")),
                None => 0 as $t,
            }
        }
    };
}

impl<'a> ByteReader<'a> {
    pub fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            pos: 0,
            ok: true,
        }
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if !self.ok || self.input.len() - self.pos < n {
            self.ok = false;
            return None;
        }
        let b = &self.input[self.pos..self.pos + n];
        self.pos += n;
        Some(b)
    }
    read_le!(u8, u8);
    read_le!(u16, u16);
    read_le!(u32, u32);
    read_le!(u64, u64);
    read_le!(i8, i8);
    read_le!(i16, i16);
    read_le!(i32, i32);
    pub fn f32(&mut self) -> f32 {
        f32::from_bits(self.u32())
    }
    pub fn f64(&mut self) -> f64 {
        f64::from_bits(self.u64())
    }
    pub fn f16(&mut self) -> f32 {
        half_to_float(self.u16())
    }
    /// Fails the read (like truncation) when `condition` is false: range checks in decoders.
    pub fn check(&mut self, condition: bool) {
        if !condition {
            self.ok = false;
        }
    }
    /// `n` raw bytes (empty after a failure).
    pub fn bytes(&mut self, n: usize) -> Vec<u8> {
        self.take(n).map(<[u8]>::to_vec).unwrap_or_default()
    }
    pub fn fixed<const N: usize>(&mut self) -> [u8; N] {
        self.take(N)
            .map(|b| b.try_into().expect("N bytes"))
            .unwrap_or([0; N])
    }
    /// A u16-length-prefixed UTF-8 string of at most `max_bytes`; invalid UTF-8 fails.
    pub fn str(&mut self, max_bytes: usize) -> String {
        let len = usize::from(self.u16());
        if !self.ok || len > max_bytes {
            self.ok = false;
            return String::new();
        }
        match self.take(len).map(std::str::from_utf8) {
            Some(Ok(s)) => s.to_owned(),
            _ => {
                self.ok = false;
                String::new()
            }
        }
    }
    pub fn ok(&self) -> bool {
        self.ok
    }
    pub fn at_end(&self) -> bool {
        self.ok && self.pos == self.input.len()
    }
}

/// Truncates to at most `max_bytes` without splitting a UTF-8 sequence.
pub fn clamp_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_round_trips_and_rounds_to_even() {
        for v in [0.0f32, 1.0, -2.5, 65504.0, 0.000_061_035_156] {
            assert_eq!(half_to_float(float_to_half(v)), v);
        }
        assert_eq!(float_to_half(1e6), 0x7C00);
        assert!(half_to_float(float_to_half(f32::NAN)).is_nan());
        // 1 + 2^-11 lies halfway between two halves: rounds to the even one (1.0).
        assert_eq!(float_to_half(1.0 + 1.0 / 2048.0), 0x3C00);
    }

    #[test]
    fn reader_latches_failure() {
        let mut r = ByteReader::new(&[1, 2]);
        assert_eq!(r.u16(), 0x0201);
        assert_eq!(r.u32(), 0);
        assert!(!r.ok());
    }
}
