//! Dwell's wire protocol. The constants are generated from `shared/protocol/constants.json` by
//! `shared/protocol/gen.mjs`; the codec is ported in Phase 24d.

#[rustfmt::skip]
mod constants {
    include!("constants.gen.rs");
}

pub use constants::*;

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
