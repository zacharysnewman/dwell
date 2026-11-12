//! Per-frame streaming budgets by device tier, from `shared/render/streaming.json`
//! (docs/ARCHITECTURE.md §7.4, docs/BUGS.md #72): how many bytes of finished chunk meshes one
//! frame uploads (`dwell_game::upload_budget`).

use std::sync::OnceLock;

/// The streaming budgets of each tier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamingBudgets {
    pub upload_desktop: u64,
    pub upload_phone: u64,
}

impl StreamingBudgets {
    /// Parses and checks the data (a bad entry fails with a message).
    pub fn parse(json: &str) -> Result<Self, String> {
        let v: serde_json::Value =
            serde_json::from_str(json).map_err(|e| format!("streaming.json: {e}"))?;
        let up = v
            .get("uploadBytesPerFrame")
            .ok_or("streaming.json: no uploadBytesPerFrame")?;
        let tier = |name: &str| -> Result<u64, String> {
            match up.get(name).and_then(serde_json::Value::as_u64) {
                Some(b) if b >= 65_536 => Ok(b),
                Some(b) => Err(format!(
                    "streaming.json: uploadBytesPerFrame.{name} is {b} B, under 64 KiB (a chunk's mesh is often larger)"
                )),
                None => Err(format!(
                    "streaming.json: uploadBytesPerFrame.{name} is not a whole number of bytes"
                )),
            }
        };
        Ok(Self {
            upload_desktop: tier("desktop")?,
            upload_phone: tier("phone")?,
        })
    }

    /// The build's budgets.
    pub fn get() -> &'static Self {
        static B: OnceLock<StreamingBudgets> = OnceLock::new();
        B.get_or_init(|| {
            Self::parse(include_str!("../../../../shared/render/streaming.json"))
                .unwrap_or_else(|e| panic!("{e}"))
        })
    }

    /// A frame's upload budget in bytes on a phone page or another.
    pub fn upload_bytes(&self, phone: bool) -> u64 {
        if phone {
            self.upload_phone
        } else {
            self.upload_desktop
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_phone_uploads_less_a_frame_than_a_desktop() {
        let b = StreamingBudgets::get();
        assert!(b.upload_phone < b.upload_desktop);
        assert_eq!(b.upload_bytes(true), b.upload_phone);
        assert_eq!(b.upload_bytes(false), b.upload_desktop);
    }

    #[test]
    fn a_bad_entry_fails_with_a_message() {
        let e = StreamingBudgets::parse(r#"{"uploadBytesPerFrame": {"desktop": 4194304}}"#)
            .unwrap_err();
        assert!(e.contains("uploadBytesPerFrame.phone"), "{e}");
        let e = StreamingBudgets::parse(
            r#"{"uploadBytesPerFrame": {"desktop": 4194304, "phone": 100}}"#,
        )
        .unwrap_err();
        assert!(e.contains("under 64 KiB"), "{e}");
        // A fixture tier by data alone.
        let b = StreamingBudgets::parse(
            r#"{"uploadBytesPerFrame": {"desktop": 1048576, "phone": 262144}}"#,
        )
        .unwrap();
        assert_eq!(b.upload_bytes(true), 262_144);
    }
}
