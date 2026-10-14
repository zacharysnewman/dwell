//! Unpredictable bytes (handshake nonces), provided by the host so the core makes no OS calls
//! itself (ARCHITECTURE.md §4.3).

pub trait Entropy {
    fn fill(&mut self, out: &mut [u8]);
}

/// The operating system's generator (native hosts; the browser host supplies
/// `crypto.getRandomValues`).
#[cfg(not(target_arch = "wasm32"))]
pub struct SystemEntropy;

#[cfg(not(target_arch = "wasm32"))]
impl Entropy for SystemEntropy {
    fn fill(&mut self, out: &mut [u8]) {
        getrandom::fill(out).expect("the system's random generator");
    }
}

/// A deterministic stream (tests and tools): splitmix64 from a seed.
pub struct SeededEntropy(pub u64);

impl Entropy for SeededEntropy {
    fn fill(&mut self, out: &mut [u8]) {
        for b in out.chunks_mut(8) {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            b.copy_from_slice(&z.to_le_bytes()[..b.len()]);
        }
    }
}
