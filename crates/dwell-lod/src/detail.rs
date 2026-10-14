//! Detail settings (ARCHITECTURE.md §6.6; `client/src/lod/detail.ts`), in the settings menu and kept
//! in the browser:
//! - the full-detail distance: how far the terrain is drawn as chunks (every block) before the
//!   LOD's level 1 takes over. At least the streamed view (always loaded, for collision); at most
//!   the radius the server streams requested chunks in;
//! - the LOD's pixel error (CSS pixels): the finest the distant view refines to;
//! - the LOD's memory (LOD_CACHE_MB): the view coarsens while its sections need more than this.

use dwell_protocol::{CHUNK_SIZE, lod, world};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DetailSettings {
    /// Chunks are drawn out to this distance from the camera (m).
    pub distance_m: f64,
    /// Distant sections refine while their cells project larger than this (CSS pixels).
    pub pixel_error: f64,
    /// The LOD's cache budget (MB): the pixel error is raised while the view needs more.
    pub memory_mb: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub min: f64,
    pub max: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DetailLimits {
    pub distance_m: Range,
    pub pixel_error: Range,
    pub memory_mb: Range,
}

pub const DETAIL_LIMITS: DetailLimits = DetailLimits {
    distance_m: Range {
        min: (world::VIEW_RADIUS_CHUNKS * CHUNK_SIZE as i64) as f64,
        max: ((world::RENDER_RADIUS_CHUNKS - 1) * CHUNK_SIZE as i64) as f64,
    },
    pixel_error: Range {
        min: 1.0,
        max: 16.0,
    },
    memory_mb: Range {
        min: 32.0,
        max: 1024.0,
    },
};

/// On phones the distant view's memory stops here, as mobile Safari closes a tab that uses too
/// much (more was a crash, not more detail).
pub const MOBILE_MEMORY_MAX_MB: f64 = 192.0;

/// The ranges on this device.
pub fn detail_limits(mobile: bool) -> DetailLimits {
    if mobile {
        DetailLimits {
            memory_mb: Range {
                min: DETAIL_LIMITS.memory_mb.min,
                max: MOBILE_MEMORY_MAX_MB,
            },
            ..DETAIL_LIMITS
        }
    } else {
        DETAIL_LIMITS
    }
}

/// Defaults: phones hold fewer chunks and sections (memory), desktops more.
pub fn default_detail(mobile: bool) -> DetailSettings {
    if mobile {
        DetailSettings {
            distance_m: 128.0,
            pixel_error: lod::PIXEL_ERROR_MOBILE as f64,
            memory_mb: lod::CACHE_MB_MOBILE as f64,
        }
    } else {
        DetailSettings {
            distance_m: 256.0,
            pixel_error: lod::PIXEL_ERROR_DESKTOP as f64,
            memory_mb: lod::CACHE_MB_DESKTOP as f64,
        }
    }
}

/// Stored settings as read back (any field may be missing or not a number).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StoredDetail {
    pub distance_m: Option<f64>,
    pub pixel_error: Option<f64>,
    pub memory_mb: Option<f64>,
}

/// Clamps stored settings into range; anything missing or not finite takes the default.
pub fn sanitize_detail(
    stored: Option<&StoredDetail>,
    fallback: DetailSettings,
    limits: &DetailLimits,
) -> DetailSettings {
    let field = |x: Option<f64>, r: Range, d: f64| match x {
        Some(v) if v.is_finite() => v.max(r.min).min(r.max),
        _ => d,
    };
    let s = stored.copied().unwrap_or_default();
    DetailSettings {
        distance_m: field(s.distance_m, limits.distance_m, fallback.distance_m),
        pixel_error: field(s.pixel_error, limits.pixel_error, fallback.pixel_error),
        memory_mb: field(s.memory_mb, limits.memory_mb, fallback.memory_mb),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_and_defaults_stored_settings() {
        let d = default_detail(false);
        assert_eq!(sanitize_detail(None, d, &DETAIL_LIMITS), d);
        let s = StoredDetail {
            distance_m: Some(1e9),
            pixel_error: Some(f64::NAN),
            memory_mb: Some(1.0),
        };
        let out = sanitize_detail(Some(&s), d, &detail_limits(true));
        assert_eq!(out.distance_m, DETAIL_LIMITS.distance_m.max);
        assert_eq!(out.pixel_error, d.pixel_error);
        assert_eq!(out.memory_mb, 32.0);
        let s = StoredDetail {
            memory_mb: Some(500.0),
            ..s
        };
        assert_eq!(
            sanitize_detail(Some(&s), d, &detail_limits(true)).memory_mb,
            MOBILE_MEMORY_MAX_MB
        );
        assert_eq!(
            DETAIL_LIMITS.distance_m,
            Range {
                min: 96.0,
                max: 352.0
            }
        );
    }
}
