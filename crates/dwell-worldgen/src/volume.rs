//! Terrain volumes (docs/proposals/TERRAIN_HYDROLOGY_AND_VOLUMES.md §3 layer 2, a prototype): a 3D
//! density over the height field, solid where it is above zero. A volume's density is a tree of
//! nodes from a small fixed vocabulary (`Node`), as data (`shared/worldgen/volume.json`, generated
//! into `volume.gen.rs` by `shared/worldgen/gen.mjs`), resolved by name once per generator.
//!
//! Evaluation is per column then per cell: the nodes that don't depend on the cell's height (the
//! slope, constants, 2D noise and what is made only of them) are computed once per column, the rest
//! per cell, and a product stops at its first zero factor, so a column the density leaves alone (a
//! gentle slope under a cliff mask) costs a few operations per cell. Only cells within the volume's
//! `reach` of the height field's surface are evaluated; beyond it the height field stands. Strict
//! IEEE float as `noise.rs`, so native and WASM agree.

use dwell_blocks::StateId;
use dwell_world::{CELL_H, SEA_LEVEL_M, m_of_row, rows_for_m};

use crate::noise::{fbm3, noise_offset, seed_word, smooth_step};

/// One node of a density tree. Children are indices of earlier nodes (the generator emits the tree
/// children first, its root last).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Node {
    Const(f32),
    /// Metres from the cell's centre down from the height field's surface: > 0 below it.
    Depth,
    /// The cell centre's height above sea level (m).
    Height,
    /// The surface's gradient (rise per run) over ±`radius` m.
    Slope {
        radius: u8,
    },
    /// 3D gradient noise in [−1, 1]: `wavelength` m across, its vertical frequency `stretch_y`
    /// times the horizontal one (0: a 2D field, the same at every height), seeded by `stream`.
    Noise {
        wavelength: i32,
        octaves: u32,
        stretch_y: f32,
        stream: u32,
    },
    Add(&'static [u16]),
    Mul(&'static [u16]),
    Min(&'static [u16]),
    Max(&'static [u16]),
    Abs(u16),
    Clamp {
        of: u16,
        min: f32,
        max: f32,
    },
    Smoothstep {
        of: u16,
        from: f32,
        to: f32,
    },
}

/// A volume as data: its density tree, how far from the surface it reaches (m), and what its added
/// cells are made of (a block by name).
#[derive(Clone, Copy, Debug)]
pub struct VolumeDef {
    pub name: &'static str,
    pub reach: f32,
    pub material: &'static str,
    pub nodes: &'static [Node],
}

pub mod data {
    include!("volume.gen.rs");
}
pub use data::VOLUMES;

/// The build's volume named `name`.
pub fn find(name: &str) -> Option<&'static VolumeDef> {
    VOLUMES.iter().find(|v| v.name == name)
}

/// A noise node's lattice, resolved for a world.
#[derive(Clone, Copy, Debug)]
struct NoiseAt {
    seed: u32,
    offset: i64,
    /// The vertical lattice's spacing (rows); 0 for a 2D field.
    wavelength_y: i32,
}

/// A volume resolved for one face's seed.
#[derive(Clone, Debug)]
pub struct Volume {
    nodes: &'static [Node],
    /// Per node: whether it depends on the cell's height (else it is computed once per column).
    varying: Vec<bool>,
    noise: Vec<Option<NoiseAt>>,
    /// The slope radii the nodes ask for, distinct.
    radii: Vec<u8>,
    pub reach_rows: i32,
    pub material: StateId,
}

/// One column's inputs: the height field's surface (m, face-local) and its slopes.
pub struct ColumnInput<'a> {
    pub x: i32,
    pub z: i32,
    pub surface: f32,
    /// The slope over each of [`Volume::radii`], in that order.
    pub slopes: &'a [f32],
}

/// A column's evaluated column-only nodes.
pub struct ColumnEval {
    x: i32,
    z: i32,
    surface: f32,
    values: Vec<f32>,
}

impl Volume {
    /// `def` for face seed `world_seed`. Panics on a tree the generator would not emit (a child
    /// after its parent): `volume.gen.rs` is validated data.
    pub fn resolve(def: &'static VolumeDef, world_seed: u64) -> Self {
        let mut varying = Vec::with_capacity(def.nodes.len());
        let mut noise = Vec::with_capacity(def.nodes.len());
        let mut radii = Vec::new();
        for (i, n) in def.nodes.iter().enumerate() {
            let kids = children(n);
            assert!(
                kids.iter().all(|&k| usize::from(k) < i),
                "volume {}: node {i} names a later child",
                def.name
            );
            let own = match *n {
                Node::Depth | Node::Height => true,
                Node::Noise { stretch_y, .. } => stretch_y > 0.0,
                _ => false,
            };
            varying.push(own || kids.iter().any(|&k| varying[usize::from(k)]));
            noise.push(match *n {
                Node::Noise {
                    wavelength,
                    stretch_y,
                    stream,
                    ..
                } => {
                    let seed = seed_word(world_seed, 0x5A1D_0000 ^ stream);
                    let wavelength_y = if stretch_y > 0.0 {
                        ((wavelength as f32 / CELL_H as f32 / stretch_y) as i32).max(1)
                    } else {
                        0
                    };
                    Some(NoiseAt {
                        seed,
                        offset: noise_offset(seed),
                        wavelength_y,
                    })
                }
                _ => None,
            });
            if let Node::Slope { radius } = *n
                && !radii.contains(&radius)
            {
                radii.push(radius);
            }
        }
        Self {
            nodes: def.nodes,
            varying,
            noise,
            radii,
            reach_rows: rows_for_m(f64::from(def.reach)),
            material: dwell_blocks::require(def.material),
        }
    }

    /// The slope radii the column inputs must carry, in order.
    pub fn radii(&self) -> &[u8] {
        &self.radii
    }

    /// The widest slope radius (m): how far beyond a chunk its columns are read.
    pub fn margin(&self) -> i32 {
        self.radii.iter().copied().max().map_or(0, i32::from)
    }

    /// The column-only nodes of one column.
    pub fn column(&self, input: &ColumnInput<'_>) -> ColumnEval {
        let mut values = vec![0.0f32; self.nodes.len()];
        for i in 0..self.nodes.len() {
            if !self.varying[i] {
                values[i] = self.eval_fixed(i, input, &values);
            }
        }
        ColumnEval {
            x: input.x,
            z: input.z,
            surface: input.surface,
            values,
        }
    }

    /// Column-only node `i`, its children already in `values`.
    fn eval_fixed(&self, i: usize, input: &ColumnInput<'_>, values: &[f32]) -> f32 {
        match self.nodes[i] {
            Node::Slope { radius } => {
                let k = self
                    .radii
                    .iter()
                    .position(|&r| r == radius)
                    .expect("a radius");
                input.slopes[k]
            }
            Node::Noise {
                wavelength,
                octaves,
                ..
            } => {
                let n = self.noise[i].expect("noise resolved");
                let (x, z) = (i64::from(input.x) + n.offset, i64::from(input.z) + n.offset);
                // A 2D field: the noise's y lattice at a fixed row.
                fbm3(n.seed, (x, 0, z), wavelength, 1, octaves)
            }
            _ => self.combine(i, |k| values[usize::from(k)]),
        }
    }

    /// The density of the cell at `row` (face-local) of column `col`: solid when above zero.
    pub fn density(&self, col: &ColumnEval, row: i32) -> f32 {
        self.eval(self.nodes.len() - 1, col, row)
    }

    fn eval(&self, i: usize, col: &ColumnEval, row: i32) -> f32 {
        if !self.varying[i] {
            return col.values[i];
        }
        match self.nodes[i] {
            Node::Depth => col.surface - (m_of_row(row) + CELL_H / 2.0) as f32,
            Node::Height => (m_of_row(row) + CELL_H / 2.0 - SEA_LEVEL_M) as f32,
            Node::Noise {
                wavelength,
                octaves,
                ..
            } => {
                let n = self.noise[i].expect("noise resolved");
                let p = (
                    i64::from(col.x) + n.offset,
                    i64::from(row) + n.offset,
                    i64::from(col.z) + n.offset,
                );
                fbm3(n.seed, p, wavelength, n.wavelength_y, octaves)
            }
            Node::Mul(kids) => {
                let mut v = 1.0f32;
                for &k in kids {
                    v *= self.eval(usize::from(k), col, row);
                    if v == 0.0 {
                        break;
                    }
                }
                v
            }
            _ => self.combine(i, |k| self.eval(usize::from(k), col, row)),
        }
    }

    /// Node `i` of the combining kinds (and a constant) from its children's values.
    fn combine(&self, i: usize, mut kid: impl FnMut(u16) -> f32) -> f32 {
        match self.nodes[i] {
            Node::Const(v) => v,
            Node::Add(kids) => kids.iter().map(|&k| kid(k)).sum(),
            Node::Mul(kids) => kids.iter().map(|&k| kid(k)).product(),
            Node::Min(kids) => kids.iter().map(|&k| kid(k)).fold(f32::INFINITY, f32::min),
            Node::Max(kids) => kids
                .iter()
                .map(|&k| kid(k))
                .fold(f32::NEG_INFINITY, f32::max),
            Node::Abs(k) => kid(k).abs(),
            Node::Clamp { of, min, max } => kid(of).clamp(min, max),
            Node::Smoothstep { of, from, to } => smooth_step(from, to, kid(of)),
            Node::Depth | Node::Height | Node::Slope { .. } | Node::Noise { .. } => {
                unreachable!("inputs are evaluated by their callers")
            }
        }
    }
}

fn children(n: &Node) -> Vec<u16> {
    match *n {
        Node::Add(k) | Node::Mul(k) | Node::Min(k) | Node::Max(k) => k.to_vec(),
        Node::Abs(k) | Node::Clamp { of: k, .. } | Node::Smoothstep { of: k, .. } => vec![k],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(surface: f32, slopes: &[f32]) -> ColumnInput<'_> {
        ColumnInput {
            x: 10,
            z: -20,
            surface,
            slopes,
        }
    }

    #[test]
    fn depth_alone_is_the_height_field() {
        static DEF: VolumeDef = VolumeDef {
            name: "plain",
            reach: 4.0,
            material: "dwell:stone",
            nodes: &[Node::Depth],
        };
        let v = Volume::resolve(&DEF, 1);
        let c = v.column(&input(40.0, &[]));
        // Row 79 is 39.5..40 m: its centre is under the surface; row 80 is above it.
        assert!(v.density(&c, 79) > 0.0);
        assert!(v.density(&c, 80) < 0.0);
    }

    #[test]
    fn a_product_with_a_zero_column_factor_leaves_the_surface() {
        // The cliff shape: a slope mask times 3D noise; on flat ground the mask is 0.
        static DEF: VolumeDef = VolumeDef {
            name: "masked",
            reach: 8.0,
            material: "dwell:stone",
            nodes: &[
                Node::Depth,
                Node::Slope { radius: 2 },
                Node::Smoothstep {
                    of: 1,
                    from: 0.5,
                    to: 1.0,
                },
                Node::Const(8.0),
                Node::Noise {
                    wavelength: 16,
                    octaves: 2,
                    stretch_y: 1.0,
                    stream: 3,
                },
                Node::Mul(&[2, 3, 4]),
                Node::Add(&[0, 5]),
            ],
        };
        let v = Volume::resolve(&DEF, 9);
        assert_eq!(v.radii(), &[2]);
        let flat = v.column(&input(40.0, &[0.1]));
        let steep = v.column(&input(40.0, &[1.5]));
        let mut differs = 0;
        for row in 64..96 {
            let plain = 40.0 - (m_of_row(row) + CELL_H / 2.0) as f32;
            assert_eq!(v.density(&flat, row), plain, "row {row}");
            if (v.density(&steep, row) > 0.0) != (plain > 0.0) {
                differs += 1;
            }
        }
        assert!(differs > 0, "the steep column is reshaped");
    }

    #[test]
    fn the_noise_is_the_same_wherever_it_is_asked_for() {
        let def = find("cliffs").expect("the cliffs volume");
        let a = Volume::resolve(def, 7);
        let b = Volume::resolve(def, 7);
        let slopes = vec![1.2; a.radii().len()];
        let (ca, cb) = (
            a.column(&input(60.0, &slopes)),
            b.column(&input(60.0, &slopes)),
        );
        for row in 100..140 {
            assert_eq!(a.density(&ca, row).to_bits(), b.density(&cb, row).to_bits());
        }
    }
}
