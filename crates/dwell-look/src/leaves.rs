//! The leaves' look (docs/TREE_GROWTH.md §10, the Leaf Lab's option 5): which blocks are drawn as a
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
    pub cards: LeafCards,
}

/// The leaf cards: alpha-cut leaf clumps on exposed leaf faces, near trees only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LeafCards {
    /// The atlas tile they are painted with (shared/textures/materials.json `tiles`).
    pub tile: &'static str,
    /// Cards per exposed face (before the per-face share).
    pub density: f64,
    /// A card's width (m), ±25 %.
    pub size_m: f64,
    /// How far the wind moves a card's corners (m).
    pub sway_m: f64,
    /// They dissolve with distance from `fade_start_m` to `fade_end_m` (m).
    pub fade_start_m: f64,
    pub fade_end_m: f64,
    /// A face's cards are dropped when a leaf cell lies within this many cells straight out of it
    /// (the face looks into a cavity inside the crown, not out of it); 0 keeps every face's.
    pub interior_cells: u32,
    /// The vertex shader drops a card whose plane the eye is more than this far behind (m): a card
    /// on the crown's far side, facing away; 0 never.
    pub back_cull_m: f64,
    /// Which renderer tiers mesh them (`dwell-client` `render::cards_for`).
    pub tiers: CardTiers,
}

/// Whether each renderer tier meshes the cards (shared/look/leaves.json `cards.tiers`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CardTiers {
    /// A GPU on a desktop page.
    pub desktop: bool,
    /// A touch device's page (the phone tier).
    pub phone: bool,
    /// A CPU adapter (the cheapest tier of everything).
    pub cpu: bool,
}
