//! What the marcher knows about each block state, derived from the registry's data
//! (`shared/blocks/*.json` → `dwell_blocks::STATES`), never from state ids in code.
//!
//! A state's **march class** decides what a primary ray does in its cell:
//!
//! | class         | rule (registry fields)                                | primary ray     |
//! |---------------|-------------------------------------------------------|-----------------|
//! | `Empty`       | not liquid, `shape: Empty`, `opacity` 0 (air)         | passes          |
//! | `Transparent` | `liquid` (water)                                      | passes (raster) |
//! | `Solid`       | `shape: Full`                                         | hits the cube   |
//! | `Shaped`      | `shape: Shaped`, or `shape: Empty` with `opacity` > 0 | the shape hook  |
//!
//! Only `Solid` and `Shaped` set a brick's occupancy bits. **Water** is its own `Transparent`
//! class: it stays a raster pass over both renderers at first (RAY_MARCHED_RENDERER.md §6), so it
//! is not occupied for primary rays; its ids are still kept in a mixed brick's states for the water
//! rays of WATER_RENDERING.md W6, and a brick holding only air and water is empty. `Shaped` covers
//! the registry's shape table and thin looks the mesher draws without a shape (a ladder's plate):
//! the shape hook ([`crate::march::shape_hit`]) is where analytic shape planes go (step A5); today
//! it hits the cell's cube.

use dwell_blocks::{StateId, StateInfo, VoxelShape};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarchClass {
    Empty = 0,
    Transparent = 1,
    Solid = 2,
    Shaped = 3,
}

impl MarchClass {
    pub fn of(s: &StateInfo) -> Self {
        if s.liquid {
            MarchClass::Transparent
        } else {
            match s.shape {
                VoxelShape::Full => MarchClass::Solid,
                VoxelShape::Shaped => MarchClass::Shaped,
                VoxelShape::Empty if s.opacity > 0.0 => MarchClass::Shaped,
                VoxelShape::Empty => MarchClass::Empty,
            }
        }
    }
    /// Sets occupancy: a primary ray stops here (or asks the shape hook).
    pub fn occupied(self) -> bool {
        matches!(self, MarchClass::Solid | MarchClass::Shaped)
    }
}

/// One state as the shader reads it (8 bytes): `flags` bits 0–1 the class, bit 2 flooded, bits
/// 8–23 the registry's shape index; `color` the sRGB `0xRRGGBB` for untextured shading. The
/// client's atlas tiles are added by the GPU marcher (A3) from the same rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateGpu {
    pub flags: u32,
    pub color: u32,
}

pub const FLAG_FLOODED: u32 = 1 << 2;

/// The marcher's state table: one entry per registry state, in id order.
#[derive(Clone, Debug)]
pub struct StateTable {
    classes: Vec<MarchClass>,
    shapes: Vec<u16>,
    gpu: Vec<StateGpu>,
}

impl StateTable {
    /// From states in id order: the registry's ([`StateTable::registry`]) or a fixture.
    pub fn from_states(states: &[StateInfo]) -> Self {
        assert!(states.len() <= usize::from(u16::MAX), "state ids are u16");
        let classes: Vec<MarchClass> = states.iter().map(MarchClass::of).collect();
        let gpu = states
            .iter()
            .zip(&classes)
            .map(|(s, &c)| StateGpu {
                flags: c as u32
                    | if s.flooded { FLAG_FLOODED } else { 0 }
                    | (u32::from(s.shape_index) << 8),
                color: s.color,
            })
            .collect();
        Self {
            classes,
            shapes: states.iter().map(|s| s.shape_index).collect(),
            gpu,
        }
    }
    /// This build's registry.
    pub fn registry() -> Self {
        Self::from_states(&dwell_blocks::STATES)
    }
    /// Unknown ids are empty.
    pub fn class(&self, id: StateId) -> MarchClass {
        self.classes
            .get(usize::from(id))
            .copied()
            .unwrap_or(MarchClass::Empty)
    }
    pub fn occupied(&self, id: StateId) -> bool {
        self.class(id).occupied()
    }
    /// The registry shape index (what a shape plane holds for a shaped cell).
    pub fn shape_index(&self, id: StateId) -> u16 {
        self.shapes.get(usize::from(id)).copied().unwrap_or(0)
    }
    pub fn len(&self) -> usize {
        self.classes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.classes.is_empty()
    }
    pub fn to_words(&self) -> Vec<u32> {
        self.gpu.iter().flat_map(|s| [s.flags, s.color]).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_blocks::require;

    #[test]
    fn registry_classes_come_from_the_data() {
        let t = StateTable::registry();
        assert_eq!(t.class(require("dwell:air")), MarchClass::Empty);
        assert_eq!(t.class(require("dwell:stone")), MarchClass::Solid);
        assert_eq!(t.class(require("dwell:water")), MarchClass::Transparent);
        assert_eq!(t.class(require("dwell:ladder")), MarchClass::Shaped);
        assert!(!t.occupied(require("dwell:water")));
        assert!(!t.occupied(u16::MAX));
        assert_eq!(t.len(), dwell_blocks::STATES.len());
        assert_eq!(t.to_words().len(), 2 * t.len());
    }
}
