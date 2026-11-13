//! Per-frame streaming budgets by device tier, from `shared/render/streaming.json`
//! (docs/ARCHITECTURE.md §7.4, docs/BUGS.md #72): how many bytes of finished chunk meshes one
//! frame uploads (`dwell_game::upload_budget`).

use std::sync::OnceLock;

/// The streaming budgets of each tier.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StreamingBudgets {
    pub upload_desktop: u64,
    pub upload_phone: u64,
    pub collision_desktop: usize,
    pub collision_phone: usize,
    /// The look-ahead's collision builds (`collisionAhead`): desktop, phone.
    pub ahead_desktop: dwell_physics::terrain::CollisionAhead,
    pub ahead_phone: dwell_physics::terrain::CollisionAhead,
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
        let cells = v
            .get("collisionCellsPerTick")
            .ok_or("streaming.json: no collisionCellsPerTick")?;
        let cells_of = |name: &str| -> Result<usize, String> {
            match cells.get(name).and_then(serde_json::Value::as_u64) {
                Some(n) if n >= 64 => Ok(n as usize),
                _ => Err(format!(
                    "streaming.json: collisionCellsPerTick.{name} must be a whole number, at least 64"
                )),
            }
        };
        let ahead = v
            .get("collisionAhead")
            .ok_or("streaming.json: no collisionAhead")?;
        let num = |name: &str, max: f64| -> Result<f32, String> {
            match ahead.get(name).and_then(serde_json::Value::as_f64) {
                Some(n) if (0.0..=max).contains(&n) => Ok(n as f32),
                _ => Err(format!(
                    "streaming.json: collisionAhead.{name} must be a number in [0, {max}]"
                )),
            }
        };
        // Within the chunks the collider keeps round a player (two: 64 m across, 32 m up).
        let (seconds, max_metres, margin_metres, min_speed) = (
            num("seconds", 10.0)?,
            num("maxMetres", 64.0)?,
            num("marginMetres", 16.0)?,
            num("minSpeed", 100.0)?,
        );
        let per = ahead.get("chunksPerTick");
        let ahead_of = |name: &str| -> Result<dwell_physics::terrain::CollisionAhead, String> {
            match per
                .and_then(|p| p.get(name))
                .and_then(serde_json::Value::as_u64)
            {
                Some(n) if n <= 64 => Ok(dwell_physics::terrain::CollisionAhead {
                    seconds,
                    max_metres,
                    margin_metres,
                    min_speed,
                    chunks_per_tick: n as usize,
                }),
                _ => Err(format!(
                    "streaming.json: collisionAhead.chunksPerTick.{name} must be a whole number in [0, 64]"
                )),
            }
        };
        Ok(Self {
            ahead_desktop: ahead_of("desktop")?,
            ahead_phone: ahead_of("phone")?,
            upload_desktop: tier("desktop")?,
            upload_phone: tier("phone")?,
            collision_desktop: cells_of("desktop")?,
            collision_phone: cells_of("phone")?,
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

    /// The terrain collider cells a prediction tick puts in outside the player's reach.
    pub fn collision_cells(&self, phone: bool) -> usize {
        if phone {
            self.collision_phone
        } else {
            self.collision_desktop
        }
    }

    /// The look-ahead's collision builds on a phone page or another.
    pub fn collision_ahead(&self, phone: bool) -> dwell_physics::terrain::CollisionAhead {
        if phone {
            self.ahead_phone
        } else {
            self.ahead_desktop
        }
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
    fn a_desktop_predicts_with_the_servers_collision_budget_and_a_phone_with_less() {
        let b = StreamingBudgets::get();
        assert_eq!(
            b.collision_cells(false),
            dwell_physics::terrain::COLLISION_CELLS_PER_TICK
        );
        assert!(b.collision_cells(true) < b.collision_cells(false));
    }

    #[test]
    fn a_bad_entry_fails_with_a_message() {
        let e = StreamingBudgets::parse(r#"{"collisionCellsPerTick": {"desktop": 8192, "phone": 1024}, "uploadBytesPerFrame": {"desktop": 4194304}, "collisionAhead": {"seconds": 1, "maxMetres": 48, "marginMetres": 1, "minSpeed": 1, "chunksPerTick": {"desktop": 2, "phone": 1}}}"#)
            .unwrap_err();
        assert!(e.contains("uploadBytesPerFrame.phone"), "{e}");
        let e = StreamingBudgets::parse(
            r#"{"collisionCellsPerTick": {"desktop": 8192, "phone": 1024}, "uploadBytesPerFrame": {"desktop": 4194304, "phone": 100}, "collisionAhead": {"seconds": 1, "maxMetres": 48, "marginMetres": 1, "minSpeed": 1, "chunksPerTick": {"desktop": 2, "phone": 1}}}"#,
        )
        .unwrap_err();
        assert!(e.contains("under 64 KiB"), "{e}");
        // A fixture tier by data alone.
        let b = StreamingBudgets::parse(
            r#"{"collisionCellsPerTick": {"desktop": 8192, "phone": 512}, "uploadBytesPerFrame": {"desktop": 1048576, "phone": 262144}, "collisionAhead": {"seconds": 1, "maxMetres": 48, "marginMetres": 1, "minSpeed": 1, "chunksPerTick": {"desktop": 2, "phone": 3}}}"#,
        )
        .unwrap();
        assert_eq!(b.upload_bytes(true), 262_144);
        assert_eq!(b.collision_ahead(true).chunks_per_tick, 3);
        let e = StreamingBudgets::parse(
            r#"{"collisionCellsPerTick": {"desktop": 8192, "phone": 1024}, "uploadBytesPerFrame": {"desktop": 4194304, "phone": 524288}, "collisionAhead": {"seconds": 1, "maxMetres": 480, "marginMetres": 1, "minSpeed": 1, "chunksPerTick": {"desktop": 2, "phone": 1}}}"#,
        )
        .unwrap_err();
        assert!(e.contains("collisionAhead.maxMetres"), "{e}");
    }

    #[test]
    fn a_desktop_builds_collision_ahead_like_the_server_and_a_phone_fewer_a_tick() {
        let b = StreamingBudgets::get();
        assert_eq!(
            b.collision_ahead(false),
            dwell_physics::terrain::CollisionAhead::default()
        );
        let phone = b.collision_ahead(true);
        assert!(phone.chunks_per_tick >= 1, "the phone builds ahead too");
        assert!(phone.chunks_per_tick <= b.collision_ahead(false).chunks_per_tick);
    }
}
