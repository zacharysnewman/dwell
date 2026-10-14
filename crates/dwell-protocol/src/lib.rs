//! Dwell's wire protocol (ARCHITECTURE.md §8). The constants are generated from
//! `shared/protocol/constants.json` by `shared/protocol/gen.mjs`; the codec is pinned by the golden
//! vectors in `shared/protocol/vectors.txt`.

pub mod auth;
pub mod bytes;
mod messages;

#[rustfmt::skip]
mod constants {
    include!("constants.gen.rs");
}

pub use constants::*;
pub use messages::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_round_trip_through_u8() {
        for v in 0..=MessageType::MAX {
            if let Some(t) = MessageType::from_u8(v) {
                assert_eq!(t as u8, v);
            }
        }
        assert_eq!(MessageType::from_u8(MessageType::MAX + 1), None);
    }

    #[test]
    fn values_match_the_data() {
        let json = include_str!("../../../shared/protocol/constants.json");
        assert!(json.contains(&format!("\"protocolVersion\": {PROTOCOL_VERSION}")));
        assert!(json.contains(&format!("\"chunkSize\": {CHUNK_SIZE}")));
    }
}
