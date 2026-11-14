//! Chisel's preview worker (ADR 0074 B3; `client/src/chisel/worker.ts`): one world's face-A
//! height field built once, the page's edit put on it, and preview tiles rasterised from it
//! (`crate::chisel`).

use wasm_bindgen::prelude::*;

use crate::chisel::{Overlay, edit_from_json, overview_frame, preview_shaded};
use crate::chisel_section::{Mesh, Tints, section_mesh, section_tinted};
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
    /// The Textures tab's tints on the 3D section's colours (ADR 0074 B6).
    tints: Tints,
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
            tints: Tints::new(),
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
        self.current = ground
            .with_biomes_and_geology(&e.biomes, e.map, e.geology)
            .with_vegetation(e.vegetation);
        Ok(())
    }

    /// An `n` × `n` RGBA tile of `texel` m from face-local (x0, z0): `overlay` 0 blocks, 1 biome
    /// weights, 2 temperature, 3 wetness, 4 rainfall, 5 rock; hill-shaded when `relief`.
    pub fn tile(
        &self,
        x0: i32,
        z0: i32,
        n: u32,
        texel: u32,
        overlay: u32,
        relief: bool,
    ) -> Vec<u8> {
        preview_shaded(
            &self.current,
            i64::from(x0),
            i64::from(z0),
            i64::from(n),
            i64::from(texel.max(1)),
            Overlay::from_index(overlay),
            relief,
        )
    }

    /// The Textures tab's edit on the 3D section (ADR 0074 B6): `{ "dwell:stone": [r, g, b] }`,
    /// a factor per channel (linear light) on each named block's colour; throws the parse error.
    pub fn set_tints(&mut self, json: &str) -> Result<(), JsError> {
        self.tints = serde_json::from_str(json).map_err(|e| JsError::new(&e.to_string()))?;
        Ok(())
    }

    /// The 3D view's section (`crate::chisel_section`): `n` × `n` columns of `texel` m from
    /// face-local (x0, z0), meshed.
    pub fn section(&self, x0: i32, z0: i32, n: u32, texel: u32) -> ChiselSection {
        let s = section_tinted(
            &self.current,
            i64::from(x0),
            i64::from(z0),
            n as usize,
            i64::from(texel.max(1)),
            &self.tints,
        );
        let (ground, water) = section_mesh(&s);
        ChiselSection {
            x0: s.x0 as i32,
            z0: s.z0 as i32,
            ground,
            water,
        }
    }

    /// The world overview's frame for `n` texels a side: `[x0, texel]` (x0 also the frame's z0),
    /// the whole disc inside (`chisel::overview_frame`).
    pub fn overview_frame(n: u32) -> Vec<i32> {
        let (x0, texel) = overview_frame(i64::from(n));
        vec![x0 as i32, texel as i32]
    }
}

/// A meshed section for the page: vertices of four u32 (x, y, z f32 bits; `0xNNRRGGBB`) and
/// indices, ground and water (`crate::chisel_section::Mesh`).
#[wasm_bindgen]
pub struct ChiselSection {
    x0: i32,
    z0: i32,
    ground: Mesh,
    water: Mesh,
}

#[wasm_bindgen]
impl ChiselSection {
    /// The section's corner (face-local, on its texel grid): `[x0, z0]`.
    pub fn origin(&self) -> Vec<i32> {
        vec![self.x0, self.z0]
    }
    pub fn ground_verts(&self) -> Vec<u32> {
        self.ground.verts.clone()
    }
    pub fn ground_index(&self) -> Vec<u32> {
        self.ground.index.clone()
    }
    pub fn water_verts(&self) -> Vec<u32> {
        self.water.verts.clone()
    }
    pub fn water_index(&self) -> Vec<u32> {
        self.water.index.clone()
    }
}
