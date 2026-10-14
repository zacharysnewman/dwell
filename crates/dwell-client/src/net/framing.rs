//! Reliable-channel framing (ARCHITECTURE.md §8.2): a stream starts with a one-byte channel id,
//! then carries messages as u32 little-endian length ‖ payload.

#[derive(Debug, PartialEq, Eq)]
pub struct FrameError(pub usize);

pub fn frame(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// Reassembles length-prefixed frames from arbitrary stream chunks.
pub struct FrameReader {
    buf: Vec<u8>,
    max: usize,
}

impl FrameReader {
    pub fn new(max_frame_bytes: usize) -> Self {
        Self {
            buf: Vec::new(),
            max: max_frame_bytes,
        }
    }

    /// Adds a chunk and returns every frame it completed; an oversize frame is an error.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<Vec<u8>>, FrameError> {
        self.buf.extend_from_slice(chunk);
        let mut frames = Vec::new();
        let mut pos = 0;
        while self.buf.len() - pos >= 4 {
            let len =
                u32::from_le_bytes(self.buf[pos..pos + 4].try_into().expect("4 bytes")) as usize;
            if len > self.max {
                return Err(FrameError(len));
            }
            if self.buf.len() - pos - 4 < len {
                break;
            }
            frames.push(self.buf[pos + 4..pos + 4 + len].to_vec());
            pos += 4 + len;
        }
        self.buf.drain(..pos);
        Ok(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reassembles_frames_split_across_chunks() {
        let bytes = [frame(&[1, 2, 3]), frame(&[])].concat();
        let mut r = FrameReader::new(1024);
        let mut out = r.push(&bytes[..2]).unwrap();
        out.extend(r.push(&bytes[2..6]).unwrap());
        out.extend(r.push(&bytes[6..]).unwrap());
        assert_eq!(out, vec![vec![1, 2, 3], vec![]]);
    }

    #[test]
    fn rejects_oversize_frames() {
        assert_eq!(
            FrameReader::new(2).push(&frame(&[0; 3])),
            Err(FrameError(3))
        );
    }
}
