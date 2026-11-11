//! The leaves' look (docs/TREE_GROWTH.md §8, the Leaf Lab's option 5): which blocks are drawn as a
//! canopy, how far their lighting normals lean toward the crown's outward direction, and how far
//! their exposed edges are cut back, from `shared/look/leaves.json`. The geometry is the chunk
//! mesher's (`dwell-mesh` `mesher/leaves.rs`).

#[path = "leaves.gen.rs"]
mod generated;
pub use generated::LEAVES;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LeavesLook {
    /// The blocks drawn as a canopy, by name.
    pub blocks: &'static [&'static str],
    /// 0–1: how far a leaf face's normal leans toward the crown's outward direction.
    pub canopy_blend: f64,
    /// Cells: how far an exposed edge or corner is cut back.
    pub chamfer: f64,
}
