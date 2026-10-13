//! Player authentication (ADR 0004): RFC 8032 Ed25519 signatures, the format WebCrypto produces,
//! verified with `ed25519-dalek` (ADR 0028 §8).

use ed25519_dalek::{Signature, Verifier, VerifyingKey};

/// Verifies `signature` over `message` by `public_key`.
pub fn verify_ed25519(signature: &[u8; 64], public_key: &[u8; 32], message: &[u8]) -> bool {
    VerifyingKey::from_bytes(public_key).is_ok_and(|key| {
        key.verify(message, &Signature::from_bytes(signature))
            .is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
            .collect()
    }

    /// The values of every `"key": "hex"` field, in order.
    fn fields<'a>(json: &'a str, key: &str) -> Vec<&'a str> {
        let pat = format!("\"{key}\": \"");
        json.match_indices(&pat)
            .map(|(i, _)| {
                let rest = &json[i + pat.len()..];
                &rest[..rest.find('"').expect("closing quote")]
            })
            .collect()
    }

    #[test]
    fn webcrypto_signatures_verify_and_tampering_fails() {
        // Signed by WebCrypto in the master server's tests (shared/master/vectors.json).
        let json = include_str!("../../../shared/master/vectors.json");
        let key: [u8; 32] = hex(fields(json, "publicKey")[0])
            .try_into()
            .expect("32 bytes");
        let messages = fields(json, "message");
        let signatures = fields(json, "signature");
        assert!(messages.len() >= 3 && messages.len() == signatures.len());
        for (m, s) in messages.iter().zip(&signatures) {
            let (m, s): (Vec<u8>, [u8; 64]) = (hex(m), hex(s).try_into().expect("64 bytes"));
            assert!(verify_ed25519(&s, &key, &m));
            let mut tampered = m.clone();
            tampered[0] ^= 1;
            assert!(!verify_ed25519(&s, &key, &tampered));
            let mut bad_sig = s;
            bad_sig[10] ^= 1;
            assert!(!verify_ed25519(&bad_sig, &key, &m));
        }
        assert!(!verify_ed25519(&[0; 64], &[0xFF; 32], b"x"));
    }
}
