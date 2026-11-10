//! Chisel's preview worker (ADR 0074 B3; `client/src/chisel/worker.ts`): one world's face-A
//! height field built once, the page's edit put on it, and preview tiles rasterised from it
//! (`crate::chisel`).

use wasm_bindgen::prelude::*;

use crate::chisel::{Overlay, edit_from_json, overview_frame, preview};
use dwell_worldgen::bifacial::BifacialTerrain;
use dwell_worldgen::terrain::TerrainGenerator;

/// A preview generator for one world seed.
#[wasm_bindgen]
pub struct ChiselPreview {
    seed: u64,
    base: TerrainGenerator,
    current: TerrainGenerator,
    /// The world knobs' generator (the archipelago's, ADR 0074 B4), kept while the knobs stay.
    shaped: Option<(dwell_worldgen::terrain::HeightField, TerrainGenerator)>,
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
            seed: dwell_worldgen::bifacial::face_seed(seed, dwell_world::Face::A),
            current: base.clone(),
            base,
            shaped: None,
        }
    }

    /// Puts an edit (`{ biomes: [...], biomeMap: {...} }`, the default biome first) on the world;
    /// throws the parse error.
    pub fn edit(&mut self, json: &str) -> Result<(), JsError> {
        let e = edit_from_json(json).map_err(|e| JsError::new(&e))?;
        // World knobs changed: the map rebuilt over the edited height field (seconds), once.
        let ground = match e.height {
            None => self.base.clone(),
            Some(h) => {
                if self.shaped.as_ref().is_none_or(|(k, _)| *k != h) {
                    let map = dwell_worldgen::worldmap::WorldMap::for_height(self.seed, h);
                    let g = TerrainGenerator::with_map(self.seed, std::sync::Arc::new(map));
                    self.shaped = Some((h, g));
                }
                self.shaped
                    .as_ref()
                    .map(|(_, g)| g.clone())
                    .unwrap_or_else(|| self.base.clone())
            }
        };
        self.current = ground.with_biomes_and_geology(&e.biomes, e.map, e.geology);
        Ok(())
    }

    /// An `n` × `n` RGBA tile of `texel` m from face-local (x0, z0): `overlay` 0 blocks, 1 biome
    /// weights, 2 temperature, 3 wetness, 4 rainfall, 5 rock.
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

    /// The world overview's frame for `n` texels a side: `[x0, texel]` (x0 also the frame's z0),
    /// the whole disc inside (`chisel::overview_frame`).
    pub fn overview_frame(n: u32) -> Vec<i32> {
        let (x0, texel) = overview_frame(i64::from(n));
        vec![x0 as i32, texel as i32]
    }
}
