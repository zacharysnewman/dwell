//! Structure files (ARCHITECTURE.md §6.3): hand-built voxel geometry as data, stamped over a base
//! generator (the movement playground the controller tests walk is one). Text, one command per
//! line, `#` starts a comment:
//!
//! ```text
//! fill x0 y0 z0 x1 y1 z1 <block state>   # the inclusive box, e.g. dwell:ladder[facing=north]
//! ```
//!
//! Blocks are named by state string, never by id; later commands overwrite earlier ones.

use std::sync::Arc;

use dwell_blocks::{StateId, parse_state};

use crate::{AirChunkTest, CHUNK_SIZE, ChunkCoord, ChunkGenerator, floor_div, local_index};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fill {
    /// Inclusive, `min ≤ max`.
    pub min: [i32; 3],
    pub max: [i32; 3],
    pub material: StateId,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Structure {
    pub fills: Vec<Fill>,
}

impl Structure {
    /// The material the structure puts at a voxel (the last fill containing it).
    pub fn at(&self, p: [i32; 3]) -> Option<StateId> {
        self.fills
            .iter()
            .rev()
            .find(|f| (0..3).all(|i| p[i] >= f.min[i] && p[i] <= f.max[i]))
            .map(|f| f.material)
    }

    /// True when no fill touches the chunk.
    pub fn misses(&self, c: ChunkCoord) -> bool {
        let c = c.to_array();
        !self.fills.iter().any(|f| {
            (0..3).all(|i| {
                floor_div(f.min[i], CHUNK_SIZE) <= c[i] && floor_div(f.max[i], CHUNK_SIZE) >= c[i]
            })
        })
    }

    /// Parses a structure; an error names the line and the reason.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut s = Self::default();
        for (number, line) in text.lines().enumerate() {
            let fail = |why: String| Err(format!("line {}: {why}", number + 1));
            let line = line.split('#').next().unwrap_or("");
            let w: Vec<&str> = line.split_whitespace().collect();
            if w.is_empty() {
                continue;
            }
            if w[0] != "fill" {
                return fail(format!("unknown command '{}'", w[0]));
            }
            if w.len() != 8 {
                return fail("fill takes x0 y0 z0 x1 y1 z1 and a block state".into());
            }
            let mut v = [0i32; 6];
            for i in 0..6 {
                match w[i + 1].parse() {
                    Ok(n) => v[i] = n,
                    Err(_) => return fail(format!("bad coordinate '{}'", w[i + 1])),
                }
            }
            let material = match parse_state(w[7]) {
                Ok(m) => m,
                Err(why) => return fail(format!("block state '{}': {why}", w[7])),
            };
            s.fills.push(Fill {
                min: [v[0].min(v[3]), v[1].min(v[4]), v[2].min(v[5])],
                max: [v[0].max(v[3]), v[1].max(v[4]), v[2].max(v[5])],
                material,
            });
        }
        Ok(s)
    }

    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// `base` with the structure stamped over it.
pub fn with_structure(base: ChunkGenerator, structure: Arc<Structure>) -> ChunkGenerator {
    Arc::new(move |coord, chunk| {
        base(coord, chunk);
        if structure.misses(coord) {
            return;
        }
        let voxels = chunk.voxels_mut();
        for lz in 0..CHUNK_SIZE {
            for ly in 0..CHUNK_SIZE {
                for lx in 0..CHUNK_SIZE {
                    let p = [
                        coord.x * CHUNK_SIZE + lx,
                        coord.y * CHUNK_SIZE + ly,
                        coord.z * CHUNK_SIZE + lz,
                    ];
                    if let Some(m) = structure.at(p) {
                        voxels[local_index(lx, ly, lz)] = m;
                    }
                }
            }
        }
    })
}

/// `base_air` except for chunks the structure touches.
pub fn with_structure_air(base_air: AirChunkTest, structure: Arc<Structure>) -> AirChunkTest {
    Arc::new(move |c| structure.misses(c) && base_air(c))
}
