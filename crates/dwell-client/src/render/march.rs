//! The ray-marched terrain renderer's place behind the seam (docs/RAY_MARCHED_RENDERER.md §3, §4):
//! **a placeholder** until step A3 (`crates/dwell-march` and its WGSL). What it fixes is the
//! interface the real marcher implements:
//!
//! - **Payloads** [`ChunkBricks`] and [`SectionBricks`], made in the job workers by
//!   [`build_chunk_bricks`] and [`build_section_bricks`] from the same inputs the mesher takes (a
//!   chunk's padded voxels, an LOD section's cells), and carried across `postMessage` by
//!   `jobs/codec.rs` (the `bricks`/`section_bricks` encoders). Today they hold a solid-cell count.
//! - **Data**: [`TerrainRenderer`] (`set_chunk`, `set_section`, `None` removes, `show_sections`,
//!   `clear`, `gpu_bytes`) — the renderer's pool and allocator live behind it.
//! - **Drawing**: [`MarchRenderer::draw`] records its pass(es) into one side's [`Targets`] (the
//!   shared frame, `frame.rs`): linear colour premultiplied by coverage, and view depth (metres
//!   along the view axis, 0 where nothing is hit); the composite fogs and tone-maps after. The
//!   frame uniforms (group 0, `shaders.rs` `Frame`) are bound by the caller's bind group. Its
//!   pass is timed as `gpu.march` (`timing.rs`). Players, the outline and lines over marched
//!   terrain come with A3's `frag_depth` pass.
//!
//! The placeholder draws no terrain: it fills its side with [`PLACEHOLDER`] (magenta) so a march
//! side is unmistakable, and the overlay and F3 say "march: not built".

use dwell_world::ChunkCoord;

use super::StandIn;
use super::frame::Targets;
use super::seam::TerrainRenderer;

/// False until the marcher draws terrain: `__dwell.renderers()` lists `march` only when true, so
/// `bench --compare` skips march configurations against the placeholder.
pub const BUILT: bool = false;

/// The placeholder's fill, linear and fully covering (magenta).
pub const PLACEHOLDER: [f64; 4] = [0.8, 0.0, 0.8, 1.0];

/// A full-detail chunk's bricks (placeholder: its solid cells).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChunkBricks {
    pub solid: u32,
}

/// An LOD section's bricks (placeholder: its solid cells).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SectionBricks {
    pub solid: u32,
}

impl ChunkBricks {
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

impl SectionBricks {
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

/// A chunk's bricks from its padded voxels (34³, the mesher's input), mirrored for face B.
pub fn build_chunk_bricks(padded: &[u16], _mirror: bool) -> ChunkBricks {
    ChunkBricks {
        solid: interior_solid(padded),
    }
}

/// An LOD section's bricks from its cells (34³, `cell_index` order).
pub fn build_section_bricks(cells: &[u16], _mirror: bool) -> SectionBricks {
    SectionBricks {
        solid: interior_solid(cells),
    }
}

/// Non-air cells inside a 34³ padded block (the 32³ interior).
fn interior_solid(padded: &[u16]) -> u32 {
    const P: usize = 34;
    if padded.len() != P * P * P {
        return 0;
    }
    let mut n = 0;
    for z in 1..P - 1 {
        for y in 1..P - 1 {
            for x in 1..P - 1 {
                n += u32::from(padded[x + P * (y + P * z)] != 0);
            }
        }
    }
    n
}

/// The ray marcher (placeholder): holds the bricks it is given, draws the placeholder fill.
#[derive(Debug, Default)]
pub struct MarchRenderer {
    chunks: dwell_world::hash::HashMap<ChunkCoord, ChunkBricks>,
    sections: dwell_world::hash::HashMap<i64, SectionBricks>,
    shown: Vec<(i64, u8)>,
}

impl MarchRenderer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Chunks and sections held.
    pub fn held(&self) -> (usize, usize) {
        (self.chunks.len(), self.sections.len())
    }

    /// Records the march pass into `targets` (one side of the frame): clears its colour to the
    /// placeholder and its view depth to "nothing hit". `timestamp_writes` times it.
    pub fn draw(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        targets: &Targets,
        timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        let [r, g, b, a] = PLACEHOLDER;
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("march"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment {
                    view: targets.scene_colour(),
                    depth_slice: None,
                    resolve_target: targets.colour_ms.as_ref().map(|_| &targets.colour.1),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }),
                        store: if targets.colour_ms.is_some() {
                            wgpu::StoreOp::Discard
                        } else {
                            wgpu::StoreOp::Store
                        },
                    },
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &targets.view_depth.1,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                }),
            ],
            depth_stencil_attachment: None,
            timestamp_writes,
            ..Default::default()
        });
        drop(pass);
    }
}

impl TerrainRenderer for MarchRenderer {
    type Chunk = ChunkBricks;
    type Section = SectionBricks;
    fn set_chunk(&mut self, coord: ChunkCoord, payload: Option<&ChunkBricks>) {
        match payload {
            Some(b) => self.chunks.insert(coord, *b),
            None => self.chunks.remove(&coord),
        };
    }
    fn set_section(
        &mut self,
        id: i64,
        _origin: [f64; 3],
        _cell_size: f64,
        payload: Option<&SectionBricks>,
    ) {
        match payload {
            Some(b) => self.sections.insert(id, *b),
            None => self.sections.remove(&id),
        };
    }
    fn show_sections(&mut self, visible: &[(i64, u8)]) {
        self.shown = visible.to_vec();
    }
    fn show_stand_ins(&mut self, _stand_ins: &[StandIn]) {}
    fn clear(&mut self) {
        self.chunks.clear();
        self.sections.clear();
        self.shown.clear();
    }
    fn gpu_bytes(&self) -> u64 {
        // The placeholder uploads nothing.
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_bricks_count_the_interior_solid_cells() {
        let mut padded = vec![0u16; 34 * 34 * 34];
        padded[0] = 5; // the apron does not count
        padded[1 + 34 * (1 + 34)] = 5;
        padded[2 + 34 * (1 + 34)] = 5;
        assert_eq!(build_chunk_bricks(&padded, false).solid, 2);
        assert_eq!(build_section_bricks(&padded, true).solid, 2);
        assert_eq!(
            build_chunk_bricks(&[1, 2], false).solid,
            0,
            "malformed: empty"
        );
    }

    #[test]
    fn the_placeholder_holds_what_it_is_given_and_clears() {
        let mut m = MarchRenderer::new();
        let c = ChunkCoord { x: 0, y: 0, z: 0 };
        m.set_chunk(c, Some(&ChunkBricks { solid: 1 }));
        m.set_section(3, [0.0; 3], 1.0, Some(&SectionBricks { solid: 1 }));
        assert_eq!(m.held(), (1, 1));
        m.set_chunk(c, None);
        assert_eq!(m.held(), (0, 1));
        m.clear();
        assert_eq!(m.held(), (0, 0));
        assert!(!BUILT, "flip once the marcher draws terrain (A3)");
    }
}
