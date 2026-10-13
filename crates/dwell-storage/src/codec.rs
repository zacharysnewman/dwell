//! Blob encodings: LZ4 (with the raw size prepended, checked before decompressing) over the
//! protocol's palette + RLE, and the player state record.

use dwell_protocol::bytes::{ByteReader, ByteWriter};
use dwell_protocol::{CHUNK_VOLUME, LOD_CELL_COUNT, decode_chunk_voxels, encode_chunk_voxels};

/// Palette + RLE of one chunk is at most palette (2 + 2·32768) + runs (≤ 5 bytes per voxel).
pub const MAX_CHUNK_RAW: usize = 2 + 2 * CHUNK_VOLUME + 5 * CHUNK_VOLUME;
/// … and of one LOD section, palette (2 + 2·65535) + runs.
pub const MAX_LOD_RAW: usize = 2 + 2 * 65535 + 5 * LOD_CELL_COUNT;

pub fn compress_bytes(raw: &[u8]) -> Vec<u8> {
    lz4_flex::compress_prepend_size(raw)
}

/// `None` if corrupt or larger than `max_size` (checked before allocating).
pub fn decompress_bytes(blob: &[u8], max_size: usize) -> Option<Vec<u8>> {
    let size = u32::from_le_bytes(blob.get(..4)?.try_into().ok()?) as usize;
    if size > max_size {
        return None;
    }
    lz4_flex::decompress_size_prepended(blob)
        .ok()
        .filter(|raw| raw.len() == size)
}

pub fn compress_chunk(voxels: &[u16]) -> Vec<u8> {
    compress_bytes(&encode_chunk_voxels(voxels))
}

pub fn decompress_chunk(blob: &[u8]) -> Option<Vec<u16>> {
    decode_chunk_voxels(&decompress_bytes(blob, MAX_CHUNK_RAW)?)
}

/// A player's saved state, keyed by device public key (§10.4).
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerRecord {
    pub key: [u8; 32],
    pub display_name: String,
    pub feet: [f64; 3],
    pub health: i32,
    /// Unix seconds; 0 on save: keep the stored value (or now).
    pub first_seen: i64,
    pub last_seen: i64,
}

/// Player state blob, version 1: u8 1, f64×3 feet, u8 health.
pub fn encode_player_state(p: &PlayerRecord) -> Vec<u8> {
    let mut out = Vec::new();
    let mut w = ByteWriter::new(&mut out);
    w.u8(1);
    for v in p.feet {
        w.f64(v);
    }
    w.u8(p.health.clamp(0, 255) as u8);
    out
}

/// The feet and health of a state blob; `None` unless well formed and finite.
pub fn decode_player_state(bytes: &[u8]) -> Option<([f64; 3], i32)> {
    let mut r = ByteReader::new(bytes);
    let version = r.u8();
    r.check(version == 1);
    let feet = [r.f64(), r.f64(), r.f64()];
    let health = i32::from(r.u8());
    r.check(feet.iter().all(|v| v.is_finite()));
    r.at_end().then_some((feet, health))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blobs_round_trip_and_reject_corruption() {
        let voxels: Vec<u16> = (0..CHUNK_VOLUME).map(|i| (i / 1000) as u16).collect();
        let blob = compress_chunk(&voxels);
        assert!(blob.len() < 1000, "{} bytes", blob.len());
        assert_eq!(decompress_chunk(&blob), Some(voxels));
        let mut bad = blob.clone();
        bad[0..4].copy_from_slice(&(MAX_CHUNK_RAW as u32 + 1).to_le_bytes());
        assert_eq!(decompress_chunk(&bad), None);
        assert_eq!(decompress_chunk(&blob[..blob.len() / 2]), None);
        assert_eq!(decompress_bytes(&[1, 2], 10), None);
    }

    #[test]
    fn player_state_round_trips() {
        let p = PlayerRecord {
            key: [7; 32],
            display_name: "Zack".into(),
            feet: [1.5, -2.0, 8e6],
            health: 300,
            first_seen: 0,
            last_seen: 0,
        };
        assert_eq!(
            decode_player_state(&encode_player_state(&p)),
            Some((p.feet, 255))
        );
        let mut nan = encode_player_state(&p);
        nan[1..9].copy_from_slice(&f64::NAN.to_le_bytes());
        assert_eq!(decode_player_state(&nan), None);
    }
}
