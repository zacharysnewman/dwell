//! Fragmentation of reliable messages on WebRTC data channels (ARCHITECTURE.md §8.2, ADR 0008).
//!
//! A data channel carries one protocol message per SCTP message, but a peer accepts SCTP messages
//! only up to its max-message-size, while a protocol message may be up to
//! `MAX_RELIABLE_MESSAGE_BYTES`. The dedicated server's str0m endpoint has no SDP exchange, so it
//! assumes the browser's default max-message-size (64 KiB, RFC 8841) and sends SCTP messages of at
//! most [`MAX_WEBRTC_MESSAGE_BYTES`]; fragments are never larger than that on any link. So a sender splits a message larger than its link's limit into
//! fragments, sent back to back on the same reliable, ordered channel:
//!
//! ```text
//! WEBRTC_FRAGMENT_TAG (0xFF) ‖ u32 little-endian total length ‖ the next bytes of the message
//! ```
//!
//! A message that fits is sent unchanged. Its first byte is its message type, never
//! [`WEBRTC_FRAGMENT_TAG`] (`shared/protocol/constants.json` checks it), so the receiver tells the two apart. Every message that fit before
//! fragmentation existed is byte-for-byte what it was; fragments appear only where the sender used
//! to fail (str0m refused the write, a browser's `send` threw).

use crate::{MAX_WEBRTC_MESSAGE_BYTES, WEBRTC_FRAGMENT_TAG as FRAGMENT_TAG};

/// Tag and total length.
pub const FRAGMENT_HEADER_BYTES: usize = 5;

/// The SCTP messages that carry `message` on a link whose peer accepts messages of at most
/// `max_sctp_bytes`: the message itself when it fits, else its fragments.
pub fn fragment(message: &[u8], max_sctp_bytes: usize) -> Vec<Vec<u8>> {
    if message.len() <= max_sctp_bytes {
        return vec![message.to_vec()];
    }
    let total = u32::try_from(message.len()).expect("a message under 4 GiB");
    let body = max_sctp_bytes
        .min(MAX_WEBRTC_MESSAGE_BYTES)
        .saturating_sub(FRAGMENT_HEADER_BYTES)
        .max(1);
    message
        .chunks(body)
        .map(|part| {
            let mut out = Vec::with_capacity(FRAGMENT_HEADER_BYTES + part.len());
            out.push(FRAGMENT_TAG);
            out.extend_from_slice(&total.to_le_bytes());
            out.extend_from_slice(part);
            out
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FragmentError {
    /// A message (or a fragment's total) over the receiver's limit.
    TooLarge,
    /// A fragment shorter than its header, carrying no bytes, or past its total.
    Malformed,
    /// A whole message or a fragment of another total while a message was being reassembled.
    Interleaved,
}

/// Reassembles one reliable channel's SCTP messages into protocol messages.
#[derive(Debug)]
pub struct Reassembler {
    max_bytes: usize,
    total: usize,
    buf: Vec<u8>,
}

impl Reassembler {
    /// Accepts messages of at most `max_bytes` (whole or reassembled).
    pub fn new(max_bytes: usize) -> Self {
        Self {
            max_bytes,
            total: 0,
            buf: Vec::new(),
        }
    }

    /// Takes one SCTP message: `Ok(Some(message))` when it completes one, `Ok(None)` while a
    /// fragmented message is still arriving. An error leaves the channel unusable: the caller
    /// closes the connection.
    pub fn push(&mut self, sctp: &[u8]) -> Result<Option<Vec<u8>>, FragmentError> {
        if sctp.first() != Some(&FRAGMENT_TAG) {
            if self.total != 0 {
                return Err(FragmentError::Interleaved);
            }
            if sctp.len() > self.max_bytes {
                return Err(FragmentError::TooLarge);
            }
            return Ok(Some(sctp.to_vec()));
        }
        if sctp.len() <= FRAGMENT_HEADER_BYTES {
            return Err(FragmentError::Malformed);
        }
        let total = u32::from_le_bytes(sctp[1..5].try_into().expect("4 bytes")) as usize;
        let part = &sctp[FRAGMENT_HEADER_BYTES..];
        if self.total == 0 {
            if total > self.max_bytes {
                return Err(FragmentError::TooLarge);
            }
            self.total = total;
            self.buf = Vec::with_capacity(total);
        } else if total != self.total {
            return Err(FragmentError::Interleaved);
        }
        if self.buf.len() + part.len() > self.total {
            return Err(FragmentError::Malformed);
        }
        self.buf.extend_from_slice(part);
        if self.buf.len() < self.total {
            return Ok(None);
        }
        self.total = 0;
        Ok(Some(std::mem::take(&mut self.buf)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MAX_RELIABLE_MESSAGE_BYTES, MessageType};

    fn message(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 7 + i / 251) as u8).collect()
    }

    #[test]
    fn no_message_type_is_the_fragment_tag() {
        assert_eq!(MessageType::from_u8(FRAGMENT_TAG), None);
    }

    #[test]
    fn a_message_that_fits_is_sent_unchanged() {
        let m = message(MAX_WEBRTC_MESSAGE_BYTES);
        assert_eq!(fragment(&m, MAX_WEBRTC_MESSAGE_BYTES), vec![m.clone()]);
        let mut r = Reassembler::new(MAX_RELIABLE_MESSAGE_BYTES);
        assert_eq!(r.push(&m), Ok(Some(m)));
    }

    #[test]
    fn a_large_message_is_split_under_the_limit_and_reassembled() {
        for len in [
            MAX_WEBRTC_MESSAGE_BYTES + 1,
            200_000,
            MAX_RELIABLE_MESSAGE_BYTES,
        ] {
            let m = message(len);
            let parts = fragment(&m, MAX_WEBRTC_MESSAGE_BYTES);
            assert!(parts.len() > 1);
            assert!(parts.iter().all(|p| p.len() <= MAX_WEBRTC_MESSAGE_BYTES));
            let mut r = Reassembler::new(MAX_RELIABLE_MESSAGE_BYTES);
            let (last, rest) = parts.split_last().unwrap();
            for p in rest {
                assert_eq!(r.push(p), Ok(None));
            }
            assert_eq!(r.push(last), Ok(Some(m)));
            // The channel carries the next message as before.
            assert_eq!(r.push(&[17, 1, 2]), Ok(Some(vec![17, 1, 2])));
        }
    }

    #[test]
    fn a_larger_link_limit_makes_larger_fragments_capped_at_the_webrtc_limit() {
        let m = message(600_000);
        assert_eq!(fragment(&m, 262_144).len(), 10);
        assert!(fragment(&m, 16_384).iter().all(|p| p.len() <= 16_384));
    }

    #[test]
    fn bad_fragments_are_refused() {
        let max = 100_000;
        let mut r = Reassembler::new(max);
        assert_eq!(r.push(&message(max + 1)), Err(FragmentError::TooLarge));
        let parts = fragment(&message(max + 1), 65_536);
        assert_eq!(r.push(&parts[0]), Err(FragmentError::TooLarge));

        let parts = fragment(&message(90_000), 65_536);
        let mut r = Reassembler::new(max);
        assert_eq!(r.push(&parts[0]), Ok(None));
        assert_eq!(r.push(&[17]), Err(FragmentError::Interleaved));
        let mut r = Reassembler::new(max);
        r.push(&parts[0]).unwrap();
        let other = fragment(&message(80_000), 65_536);
        assert_eq!(r.push(&other[1]), Err(FragmentError::Interleaved));
        let mut r = Reassembler::new(max);
        r.push(&parts[0]).unwrap();
        let mut long = parts[1].clone();
        long.extend_from_slice(&[0; 10]);
        assert_eq!(r.push(&long), Err(FragmentError::Malformed));
        assert_eq!(
            Reassembler::new(max).push(&[FRAGMENT_TAG, 1, 0, 0, 0]),
            Err(FragmentError::Malformed)
        );
        assert_eq!(
            Reassembler::new(max).push(&[FRAGMENT_TAG, 1]),
            Err(FragmentError::Malformed)
        );
    }
}
