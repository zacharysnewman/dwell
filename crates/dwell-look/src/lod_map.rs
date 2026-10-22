//! The LOD colour map's tunables (ARCHITECTURE.md §6.6 "Colour map", a preview behind `?lodmap=on`):
//! nested rings of a minimap-style colour texture around the eye that LOD tops take their colour
//! from, from `shared/look/lodmap.json`. The rings' placing and rasterising are the client's
//! (`dwell-client` `lod_map.rs`).

#[path = "lodmap.gen.rs"]
mod generated;
pub use generated::LOD_MAP;

/// One ring: its texel (m) and size (texels square).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LodMapRing {
    pub texel_m: i64,
    pub size: i64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodMapTunables {
    /// Finest first; every ring the same size.
    pub rings: &'static [LodMapRing],
    /// A ring recentres in steps of this many texels.
    pub snap_texels: i64,
    /// The most texels one raster job holds.
    pub tile_texels: i64,
    /// Raster jobs in flight at once.
    pub jobs_in_flight: usize,
    /// A river is drawn at least this many texels wide.
    pub river_min_texels: f64,
    /// The LOD tops take the map's colour from `blend_from_m` to `blend_to_m` from the eye.
    pub blend_from_m: f64,
    pub blend_to_m: f64,
    /// A ring hands over to the next coarser over this many texels at its edge.
    pub ring_fade_texels: f64,
    /// The block whose LOD top colour paints water, darkened toward `water_deep_shade` over
    /// `water_depth_m` of depth.
    pub water_block: &'static str,
    pub water_depth_m: f64,
    pub water_deep_shade: f64,
    /// Where a river crosses an LOD cell too coarse to hold it, its ground lies this far under the
    /// river's water level.
    pub valley_margin_m: f64,
}
