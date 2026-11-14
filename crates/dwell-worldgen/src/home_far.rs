//! The home world's far-body bake (docs/proposals/MOON_WORLD.md §5, slice M-6): the same
//! [`FarSource`] and [`bake_far`] as the moon's, over the home world's own data, so one far-body
//! renderer draws every grid (owner, 2026-11-12: "The earth should use the same normal map kinda
//! technique when far enough away, when on the moon.").
//!
//! Per face, from what the world already holds at join (the face's world map and generator; no
//! chunk is generated):
//!
//! - **Height** (the normal and A of `normal_height`): the world map's eroded cell heights,
//!   box-filtered over the texel at the map's cell spacing (at most [`MAX_SAMPLES`]² cells), with
//!   water standing at its level (the sea's, a lake's), so the sea and lakes bake flat.
//! - **Albedo:** the block the column at the texel's centre shows from afar
//!   (`TerrainGenerator::far_surface`: its biome's palette for its band) by the block's colour
//!   (blocks.json `color`). No new colour table: the biomes' palettes are the data.
//! - **Water:** the share of the texel's samples in the map's sea or a lake (rivers are narrower
//!   than a texel and left out), a separate channel for the sea's colour, glint and reflection.
//!
//! Clouds are out of scope. Face-local throughout (`FarSource`'s frame); face B's map is its own.

use dwell_world::{Face, SEA_LEVEL_M};

use crate::bifacial::BifacialTerrain;
use crate::grids::{FarBake, FarSource, GridGenerator, bake_far, grid};
use crate::terrain::TerrainGenerator;
use crate::worldmap::{NO_LAKE, WorldMap};

/// The most samples a side of a texel takes (16 a texel): the map's cells are 100 m and a 256²
/// texel of the default disc 390 m, so 4 a side reaches every cell.
pub const MAX_SAMPLES: usize = 4;

/// The home grid's [`FarSource`]: both faces' generators (and through them their world maps).
#[derive(Clone, Copy)]
pub struct HomeFar<'a> {
    faces: [&'a TerrainGenerator; 2],
}

impl<'a> HomeFar<'a> {
    /// The source over a world's terrain, both faces.
    pub fn new(terrain: &'a BifacialTerrain) -> Self {
        Self::of(terrain.local(Face::A), terrain.local(Face::B))
    }

    /// The source over two face generators (tests: fixture maps).
    pub fn of(a: &'a TerrainGenerator, b: &'a TerrainGenerator) -> Self {
        Self { faces: [a, b] }
    }

    fn face(&self, f: Face) -> &'a TerrainGenerator {
        match f {
            Face::A => self.faces[0],
            Face::B => self.faces[1],
        }
    }

    fn map(&self, f: Face) -> &'a WorldMap {
        self.face(f).map()
    }

    /// The bake at `size` texels a side.
    pub fn bake(&self, face: Face, size: usize) -> FarBake {
        bake_far(self, face, size)
    }

    /// The bake at the home grid's data size (`grids.json` `dwell:home` `bake.size`).
    pub fn bake_default(&self, face: Face) -> FarBake {
        self.bake(face, bake_size())
    }

    /// The `n` × `n` sample points (face-local) of the texel `texel_m` wide centred on (x, z): at
    /// the map's cell spacing or finer, at most [`MAX_SAMPLES`] a side.
    fn samples(
        &self,
        face: Face,
        x: f64,
        z: f64,
        texel_m: f64,
    ) -> impl Iterator<Item = (f64, f64)> {
        let cell = self.map(face).cell;
        let n = ((texel_m / cell).ceil() as usize).clamp(1, MAX_SAMPLES);
        let step = texel_m / n as f64;
        let o = -0.5 * texel_m + 0.5 * step;
        (0..n * n).map(move |k| (x + o + (k % n) as f64 * step, z + o + (k / n) as f64 * step))
    }

    /// The water's level (m over sea) at (x, z), if its map cell is sea or a lake.
    fn water_level(&self, face: Face, x: f64, z: f64) -> Option<f64> {
        let m = self.map(face);
        let c = m.cell_at(x, z);
        if m.is_sea(c) {
            return Some(0.0);
        }
        match m.lake[c] {
            NO_LAKE => None,
            l => Some(f64::from(m.lakes[usize::from(l) - 1].level)),
        }
    }
}

/// The home grid's bake size from the data.
pub fn bake_size() -> usize {
    match grid("dwell:home").map(|g| &g.generator) {
        Some(GridGenerator::Terrain { bake_size }) => *bake_size,
        _ => 256,
    }
}

impl FarSource for HomeFar<'_> {
    fn far_radius_m(&self) -> f64 {
        self.map(Face::A).world_radius() as f64
    }

    fn far_height(&self, face: Face, x: f64, z: f64, texel_m: f64) -> f64 {
        let m = self.map(face);
        let (mut sum, mut n) = (0.0, 0.0);
        for (sx, sz) in self.samples(face, x, z, texel_m) {
            let ground = f64::from(m.height[m.cell_at(sx, sz)]);
            let top = match self.water_level(face, sx, sz) {
                Some(level) => ground.max(level),
                None => ground,
            };
            sum += top;
            n += 1.0;
        }
        SEA_LEVEL_M + sum / n
    }

    fn far_albedo(&self, face: Face, x: f64, z: f64) -> [f64; 3] {
        // One coarse column at the texel's centre (a column costs ~2 µs: a 2 × 2 sub-grid doubled
        // the bake's time for a look the biomes' wide blends barely change).
        let cell = self.map(face).cell;
        // The LOD's cells are powers of two: the largest within a map cell.
        let lod_cell = 1i64 << (cell.max(1.0).log2().floor() as u32);
        let s = self
            .face(face)
            .far_surface(x.round() as i64, z.round() as i64, lod_cell);
        if s == dwell_world::M.air {
            return [0.0; 3];
        }
        let c = dwell_blocks::info(s).color;
        [(c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff].map(|v| f64::from(v) / 255.0)
    }

    fn far_water(&self, face: Face, x: f64, z: f64, texel_m: f64) -> f64 {
        let (mut wet, mut n) = (0.0, 0.0);
        for (sx, sz) in self.samples(face, x, z, texel_m) {
            wet += f64::from(u8::from(self.water_level(face, sx, sz).is_some()));
            n += 1.0;
        }
        wet / n
    }
}
