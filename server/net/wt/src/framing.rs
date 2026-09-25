//! Reliable-channel framing over WebTransport streams (ARCHITECTURE.md §8.2).
//!
//! A reliable channel is one QUIC stream. Its first byte is the channel id (0 = `control`,
//! client-opened bidirectional; 1 = `world`, server-opened unidirectional). After that the stream
//! carries messages as `u32 little-endian length ‖ payload`.

use wtransport::{RecvStream, SendStream};

#[derive(Debug)]
pub enum FrameError {
    Closed,
    TooLarge,
    Io,
}

/// Reads one length-prefixed message. `Ok(None)` means the peer finished the stream cleanly.
pub async fn read_frame(
    rx: &mut RecvStream,
    max_bytes: u32,
) -> Result<Option<Vec<u8>>, FrameError> {
    let mut len_buf = [0u8; 4];
    // Distinguish a clean end-of-stream (no bytes of the next header) from a truncated header.
    let mut got = 0;
    while got < 4 {
        match rx.read(&mut len_buf[got..]).await {
            Ok(Some(n)) => got += n,
            Ok(None) if got == 0 => return Ok(None),
            Ok(None) => return Err(FrameError::Closed),
            Err(_) => return Err(FrameError::Io),
        }
    }
    let len = u32::from_le_bytes(len_buf);
    if len > max_bytes {
        return Err(FrameError::TooLarge);
    }
    let mut payload = vec![0u8; len as usize];
    rx.read_exact(&mut payload)
        .await
        .map_err(|_| FrameError::Io)?;
    Ok(Some(payload))
}

pub async fn write_frame(tx: &mut SendStream, payload: &[u8]) -> Result<(), FrameError> {
    let len = u32::try_from(payload.len()).map_err(|_| FrameError::TooLarge)?;
    tx.write_all(&len.to_le_bytes())
        .await
        .map_err(|_| FrameError::Io)?;
    tx.write_all(payload).await.map_err(|_| FrameError::Io)
}
