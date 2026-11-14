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

impl LodMapTunables {
    /// Water's brightness (a factor on its linear colour) over `depth` m of water: 1 at the
    /// surface, toward `water_deep_shade` over `water_depth_m`. The generated map's (`dwell-client`
    /// `lod_map.rs`) and the explored map's and minimap's (`dwell_game::minimap`), so exploring
    /// the sea reads as the generated sea does.
    pub fn water_shade(&self, depth: f64) -> f64 {
        let k = 1.0 - (-depth.max(0.0) / self.water_depth_m).exp();
        1.0 + (self.water_deep_shade - 1.0) * k
    }

    /// How far (m) a scan follows water down for its colour: three `water_depth_m`, where
    /// [`Self::water_shade`] is within 5% of its deepest.
    pub fn water_colour_depth_m(&self) -> f64 {
        self.water_depth_m * 3.0
    }
}

/// `0xRRGGBB` (sRGB) scaled by `k` in linear light, as [`LodMapTunables::water_shade`] applies.
pub fn shade_srgb(c: u32, k: f64) -> u32 {
    let ch = |s: u32| {
        let v = crate::srgb_to_linear(f64::from((c >> s) & 0xff) / 255.0) * k;
        let b = (crate::daynight::linear_to_srgb(v.max(0.0)).clamp(0.0, 1.0) * 255.0 + 0.5).floor();
        (b as u32) << s
    };
    ch(16) | ch(8) | ch(0)
}
