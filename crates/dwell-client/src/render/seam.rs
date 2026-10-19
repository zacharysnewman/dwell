//! The renderer seam (docs/RAY_MARCHED_RENDERER.md §3): terrain is drawn by one of two terrain
//! renderers — the mesher's (`raster`) or the ray marcher's (`march`) — or by both side by side in
//! compare mode, while the camera, players, outline, lines, stats and the shared frame
//! (`frame.rs`) are common code. Platform free (no GPU types), so the routing is unit-tested.
//!
//! - A terrain renderer takes its own **payload** per chunk and per LOD section
//!   ([`TerrainRenderer`]): raster `ChunkMeshes`/`SectionMeshes`, march `ChunkBricks`/
//!   `SectionBricks` (`march.rs`).
//! - The workers make the payloads the [`Selection`] needs ([`Payloads`]): one meshing job per
//!   chunk or section makes the mesh, the bricks, or both ([`Terrain`]).
//! - [`Routed`] hands each part to its renderer. Switching the selection clears the renderers no
//!   longer drawn (their GPU memory is freed) and the game rebuilds every chunk and section from
//!   its CPU data (`ChunkStreamer::remesh_all`, `LodSystem::rebuild_all`), without a reload.

use dwell_lod::SectionPayload;
use dwell_mesh::{ChunkMeshes, SectionMeshes, section_bytes};
use dwell_world::ChunkCoord;

use super::StandIn;
use super::march::{ChunkBricks, SectionBricks};

/// A terrain renderer by name (`?renderer=`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TerrainKind {
    /// The greedy mesher's triangles, rasterised (today's renderer).
    Raster,
    /// GPU ray marching over a brickmap (A3; a placeholder until then).
    March,
}

impl TerrainKind {
    pub fn name(self) -> &'static str {
        match self {
            TerrainKind::Raster => "raster",
            TerrainKind::March => "march",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "raster" => Some(TerrainKind::Raster),
            "march" => Some(TerrainKind::March),
            _ => None,
        }
    }
    fn payload(self) -> Payloads {
        match self {
            TerrainKind::Raster => Payloads::MESH,
            TerrainKind::March => Payloads::BRICKS,
        }
    }
}

/// What draws the terrain: one renderer, or two side by side (compare mode: `left` on the left
/// half, `right` on the right).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub left: TerrainKind,
    /// Compare mode's second renderer.
    pub right: Option<TerrainKind>,
}

impl Default for Selection {
    fn default() -> Self {
        Self::single(TerrainKind::Raster)
    }
}

impl Selection {
    pub fn single(kind: TerrainKind) -> Self {
        Self {
            left: kind,
            right: None,
        }
    }

    /// From `?renderer=raster|march|compare` and, in compare mode, `?compare=` (the right side's
    /// renderer; default `march`: `?compare=raster` puts raster on both sides, the zero-difference
    /// check). None for an unknown name.
    pub fn parse(renderer: &str, compare: Option<&str>) -> Option<Self> {
        if renderer == "compare" {
            let right = match compare {
                None | Some("") => TerrainKind::March,
                Some(c) => TerrainKind::parse(c)?,
            };
            return Some(Self {
                left: TerrainKind::Raster,
                right: Some(right),
            });
        }
        TerrainKind::parse(renderer).map(Self::single)
    }

    pub fn compare(self) -> bool {
        self.right.is_some()
    }

    /// The payloads the workers make for it.
    pub fn payloads(self) -> Payloads {
        self.right
            .map_or(Payloads::NONE, TerrainKind::payload)
            .with(self.left.payload())
    }

    /// Whether `kind` draws.
    pub fn draws(self, kind: TerrainKind) -> bool {
        self.left == kind || self.right == Some(kind)
    }

    /// What the F3 overlay says draws: `renderer raster`, or with a placeholder marcher
    /// `renderer compare raster | march (march: not built)`.
    pub fn status(self) -> String {
        let placeholder = self.draws(TerrainKind::March) && !super::march::BUILT;
        format!(
            "renderer {}{}",
            self.label(),
            if placeholder {
                " (march: not built)"
            } else {
                ""
            }
        )
    }

    /// `raster`, `march`, or `compare raster | march`.
    pub fn label(self) -> String {
        match self.right {
            None => self.left.name().to_string(),
            Some(r) => format!("compare {} | {}", self.left.name(), r.name()),
        }
    }
}

/// Which payloads a meshing job makes (a bit each).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Payloads(pub u8);

impl Payloads {
    pub const NONE: Self = Self(0);
    pub const MESH: Self = Self(1);
    pub const BRICKS: Self = Self(2);
    pub const BOTH: Self = Self(3);

    pub fn mesh(self) -> bool {
        self.0 & 1 != 0
    }
    pub fn bricks(self) -> bool {
        self.0 & 2 != 0
    }
    pub fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl Default for Payloads {
    fn default() -> Self {
        Self::MESH
    }
}

/// One chunk's or section's payloads: the parts its job was asked for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Terrain<R, M> {
    pub raster: Option<R>,
    pub march: Option<M>,
}

pub type ChunkTerrain = Terrain<ChunkMeshes, ChunkBricks>;
pub type SectionTerrain = Terrain<SectionMeshes, SectionBricks>;

impl ChunkTerrain {
    pub fn meshes(m: ChunkMeshes) -> Self {
        Self {
            raster: Some(m),
            march: None,
        }
    }
}

impl SectionTerrain {
    pub fn meshes(m: SectionMeshes) -> Self {
        Self {
            raster: Some(m),
            march: None,
        }
    }
}

impl SectionPayload for SectionTerrain {
    fn payload_bytes(&self) -> usize {
        self.raster.as_ref().map_or(0, section_bytes) + self.march.as_ref().map_or(0, |b| b.bytes())
    }
}

/// A terrain renderer's data side: what the marcher implements to plug into the seam (its GPU
/// drawing is `MarchRenderer::draw`, into the shared frame's targets). Removing is `None`.
pub trait TerrainRenderer {
    /// A full-detail chunk's payload.
    type Chunk;
    /// An LOD section's payload.
    type Section;
    /// Adds, replaces or (`None`) removes a chunk; payloads are chunk-local.
    fn set_chunk(&mut self, coord: ChunkCoord, payload: Option<&Self::Chunk>);
    /// Adds, replaces or (`None`) removes an LOD section, in cells of `cell_size` from `origin`.
    fn set_section(
        &mut self,
        id: i64,
        origin: [f64; 3],
        cell_size: f64,
        payload: Option<&Self::Section>,
    );
    /// The sections to draw this frame, with the sides (bit per face) whose skirt shows.
    fn show_sections(&mut self, visible: &[(i64, u8)]);
    /// Ancestor sections drawn clipped to boxes this frame.
    fn show_stand_ins(&mut self, stand_ins: &[StandIn]);
    /// Drops everything it holds (no longer drawn: a renderer switch).
    fn clear(&mut self);
    /// GPU memory its terrain holds (bytes).
    fn gpu_bytes(&self) -> u64;
}

/// Both terrain renderers behind the seam: each payload part goes to its renderer while the
/// selection draws it. A payload without the part a drawn renderer takes (a job submitted before a
/// switch) leaves that renderer's copy as it is; only `None` removes.
pub struct Routed<A, B> {
    pub raster: A,
    pub march: B,
    selection: Selection,
}

impl<A, B> Routed<A, B>
where
    A: TerrainRenderer<Chunk = ChunkMeshes, Section = SectionMeshes>,
    B: TerrainRenderer<Chunk = ChunkBricks, Section = SectionBricks>,
{
    pub fn new(raster: A, march: B, selection: Selection) -> Self {
        Self {
            raster,
            march,
            selection,
        }
    }

    pub fn selection(&self) -> Selection {
        self.selection
    }

    /// Switches renderer: those no longer drawn are cleared. True when the payloads change (the
    /// caller then rebuilds every chunk and section).
    pub fn select(&mut self, selection: Selection) -> bool {
        let before = self.selection.payloads();
        if !selection.draws(TerrainKind::Raster) {
            self.raster.clear();
        }
        if !selection.draws(TerrainKind::March) {
            self.march.clear();
        }
        self.selection = selection;
        before != selection.payloads()
    }
}

impl<A, B> TerrainRenderer for Routed<A, B>
where
    A: TerrainRenderer<Chunk = ChunkMeshes, Section = SectionMeshes>,
    B: TerrainRenderer<Chunk = ChunkBricks, Section = SectionBricks>,
{
    type Chunk = ChunkTerrain;
    type Section = SectionTerrain;

    fn set_chunk(&mut self, coord: ChunkCoord, payload: Option<&ChunkTerrain>) {
        let s = self.selection;
        match payload {
            None => {
                self.raster.set_chunk(coord, None);
                self.march.set_chunk(coord, None);
            }
            Some(t) => {
                if let (true, Some(m)) = (s.draws(TerrainKind::Raster), &t.raster) {
                    self.raster.set_chunk(coord, Some(m));
                }
                if let (true, Some(b)) = (s.draws(TerrainKind::March), &t.march) {
                    self.march.set_chunk(coord, Some(b));
                }
            }
        }
    }

    fn set_section(
        &mut self,
        id: i64,
        origin: [f64; 3],
        cell_size: f64,
        payload: Option<&SectionTerrain>,
    ) {
        let s = self.selection;
        match payload {
            None => {
                self.raster.set_section(id, origin, cell_size, None);
                self.march.set_section(id, origin, cell_size, None);
            }
            Some(t) => {
                if let (true, Some(m)) = (s.draws(TerrainKind::Raster), &t.raster) {
                    self.raster.set_section(id, origin, cell_size, Some(m));
                }
                if let (true, Some(b)) = (s.draws(TerrainKind::March), &t.march) {
                    self.march.set_section(id, origin, cell_size, Some(b));
                }
            }
        }
    }

    fn show_sections(&mut self, visible: &[(i64, u8)]) {
        self.raster.show_sections(visible);
        self.march.show_sections(visible);
    }

    fn show_stand_ins(&mut self, stand_ins: &[StandIn]) {
        self.raster.show_stand_ins(stand_ins);
        self.march.show_stand_ins(stand_ins);
    }

    fn clear(&mut self) {
        self.raster.clear();
        self.march.clear();
    }

    fn gpu_bytes(&self) -> u64 {
        self.raster.gpu_bytes() + self.march.gpu_bytes()
    }
}

/// A terrain renderer that records what it is given (tests; also a template for a new renderer).
#[derive(Debug)]
pub struct Recording<C, S> {
    pub chunks: std::collections::BTreeMap<ChunkCoord, C>,
    pub sections: std::collections::BTreeMap<i64, S>,
    pub shown: Vec<(i64, u8)>,
    pub clears: u32,
}

impl<C, S> Default for Recording<C, S> {
    fn default() -> Self {
        Self {
            chunks: Default::default(),
            sections: Default::default(),
            shown: Vec::new(),
            clears: 0,
        }
    }
}

impl<C: Clone, S: Clone> TerrainRenderer for Recording<C, S> {
    type Chunk = C;
    type Section = S;
    fn set_chunk(&mut self, coord: ChunkCoord, payload: Option<&C>) {
        match payload {
            Some(p) => self.chunks.insert(coord, p.clone()),
            None => self.chunks.remove(&coord),
        };
    }
    fn set_section(&mut self, id: i64, _origin: [f64; 3], _cell: f64, payload: Option<&S>) {
        match payload {
            Some(p) => self.sections.insert(id, p.clone()),
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
        self.clears += 1;
    }
    fn gpu_bytes(&self) -> u64 {
        (self.chunks.len() + self.sections.len()) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Both =
        Routed<Recording<ChunkMeshes, SectionMeshes>, Recording<ChunkBricks, SectionBricks>>;

    fn both(selection: Selection) -> Both {
        Routed::new(Recording::default(), Recording::default(), selection)
    }

    fn full() -> ChunkTerrain {
        Terrain {
            raster: Some(ChunkMeshes::default()),
            march: Some(ChunkBricks(dwell_march::NodeBuild::Solid(7))),
        }
    }

    const C: ChunkCoord = ChunkCoord { x: 1, y: 2, z: 3 };

    #[test]
    fn the_selection_parses_from_the_address_and_names_its_payloads() {
        let raster = Selection::parse("raster", None).unwrap();
        assert_eq!(raster, Selection::default());
        assert_eq!(raster.payloads(), Payloads::MESH);
        let march = Selection::parse("march", Some("raster")).unwrap();
        assert_eq!(
            march,
            Selection::single(TerrainKind::March),
            "?compare= only in compare"
        );
        assert_eq!(march.payloads(), Payloads::BRICKS);
        let compare = Selection::parse("compare", None).unwrap();
        assert_eq!(compare.right, Some(TerrainKind::March));
        assert_eq!(compare.payloads(), Payloads::BOTH);
        assert_eq!(compare.label(), "compare raster | march");
        assert_eq!(raster.status(), "renderer raster");
        assert_eq!(
            march.status(),
            if super::super::march::BUILT {
                "renderer march"
            } else {
                "renderer march (march: not built)"
            }
        );
        let twice = Selection::parse("compare", Some("raster")).unwrap();
        assert_eq!(
            twice.payloads(),
            Payloads::MESH,
            "raster on both sides: meshes only"
        );
        assert!(twice.compare());
        assert_eq!(Selection::parse("voxels", None), None);
        assert_eq!(Selection::parse("compare", Some("voxels")), None);
    }

    #[test]
    fn each_mode_routes_only_the_payloads_its_renderers_take() {
        for (sel, raster, march) in [
            (Selection::single(TerrainKind::Raster), true, false),
            (Selection::single(TerrainKind::March), false, true),
            (Selection::parse("compare", None).unwrap(), true, true),
            (
                Selection::parse("compare", Some("raster")).unwrap(),
                true,
                false,
            ),
        ] {
            let mut r = both(sel);
            r.set_chunk(C, Some(&full()));
            r.set_section(
                9,
                [0.0; 3],
                2.0,
                Some(&Terrain {
                    raster: Some(SectionMeshes::default()),
                    march: Some(SectionBricks(dwell_march::NodeBuild::Solid(1))),
                }),
            );
            assert_eq!(r.raster.chunks.contains_key(&C), raster, "{sel:?}");
            assert_eq!(r.march.chunks.contains_key(&C), march, "{sel:?}");
            assert_eq!(r.raster.sections.contains_key(&9), raster, "{sel:?}");
            assert_eq!(r.march.sections.contains_key(&9), march, "{sel:?}");
            r.set_chunk(C, None);
            r.set_section(9, [0.0; 3], 2.0, None);
            assert!(r.raster.chunks.is_empty() && r.march.chunks.is_empty());
            assert!(r.raster.sections.is_empty() && r.march.sections.is_empty());
        }
    }

    #[test]
    fn a_payload_missing_a_drawn_part_keeps_the_renderers_copy() {
        // A job submitted before a switch to compare mode brings meshes only: the marcher's chunk
        // is not removed by it (only None removes), and the rebuild brings its bricks.
        let mut r = both(Selection::parse("compare", None).unwrap());
        r.set_chunk(C, Some(&full()));
        r.set_chunk(C, Some(&ChunkTerrain::meshes(ChunkMeshes::default())));
        assert_eq!(
            r.march.chunks[&C],
            ChunkBricks(dwell_march::NodeBuild::Solid(7))
        );
    }

    #[test]
    fn switching_clears_the_renderers_no_longer_drawn_and_says_when_to_rebuild() {
        let mut r = both(Selection::parse("compare", None).unwrap());
        r.set_chunk(C, Some(&full()));
        assert!(
            r.select(Selection::single(TerrainKind::Raster)),
            "bricks no longer made"
        );
        assert!(r.march.chunks.is_empty() && r.march.clears == 1);
        assert!(r.raster.chunks.contains_key(&C), "raster still draws: kept");
        assert!(
            !r.select(Selection::parse("compare", Some("raster")).unwrap()),
            "same payloads"
        );
        assert!(r.select(Selection::single(TerrainKind::March)));
        assert!(r.raster.chunks.is_empty());
        assert_eq!(r.selection(), Selection::single(TerrainKind::March));
    }

    #[test]
    fn section_payloads_count_both_parts_for_the_lod_cache() {
        let mut m = SectionMeshes::default();
        m.opaque.positions = vec![0.0; 10];
        let t = SectionTerrain {
            raster: Some(m.clone()),
            march: Some(SectionBricks(dwell_march::NodeBuild::Solid(3))),
        };
        assert_eq!(
            t.payload_bytes(),
            section_bytes(&m) + SectionBricks(dwell_march::NodeBuild::Solid(3)).bytes()
        );
    }
}
