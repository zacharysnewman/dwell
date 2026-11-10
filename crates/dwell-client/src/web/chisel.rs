//! Chisel's preview worker (ADR 0074 B3; `client/src/chisel/worker.ts`): one world's face-A
//! height field built once, the page's edit put on it, and preview tiles rasterised from it
//! (`crate::chisel`).

use wasm_bindgen::prelude::*;

use crate::chisel::{Overlay, edit_from_json, preview};
use dwell_worldgen::bifacial::BifacialTerrain;
use dwell_worldgen::terrain::TerrainGenerator;

/// A preview generator for one world seed.
#[wasm_bindgen]
pub struct ChiselPreview {
    base: TerrainGenerator,
    current: TerrainGenerator,
}

#[wasm_bindgen]
impl ChiselPreview {
    /// The world `seed`'s face A, with the build's biomes until [`ChiselPreview::edit`].
    #[wasm_bindgen(constructor)]
    pub fn new(seed: u64) -> ChiselPreview {
        let base = BifacialTerrain::height_field(seed)
            .local(dwell_world::Face::A)
            .clone();
        Self {
            current: base.clone(),
            base,
        }
    }

    /// Puts an edit (`{ biomes: [...], biomeMap: {...} }`, the default biome first) on the world;
    /// throws the parse error.
    pub fn edit(&mut self, json: &str) -> Result<(), JsError> {
        let (biomes, map) = edit_from_json(json).map_err(|e| JsError::new(&e))?;
        self.current = self.base.clone().with_biomes_on(&biomes, map);
        Ok(())
    }

    /// An `n` × `n` RGBA tile of `texel` m from face-local (x0, z0): `overlay` 0 blocks, 1 biome
    /// weights, 2 temperature, 3 wetness.
    pub fn tile(&self, x0: i32, z0: i32, n: u32, texel: u32, overlay: u32) -> Vec<u8> {
        preview(
            &self.current,
            i64::from(x0),
            i64::from(z0),
            i64::from(n),
            i64::from(texel.max(1)),
            Overlay::from_index(overlay),
        )
    }
}
