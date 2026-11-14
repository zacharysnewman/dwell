//! The bifacial world's terrain (docs/BIFACIAL_WORLD.md §5, ADR 0023): two face-local generators —
//! face A from the world seed, face B from its own seed streams — and chunks of face B are
//! face-local chunks flipped about the midplane. Still a pure function of (seed, chunk).

use dwell_blocks::mirror;
use dwell_world::lod::{self, LodBounds, LodCells, LodCoord, LodKind, LodSurfaces};
use dwell_world::{
    CHUNK_SIZE, Chunk, ChunkCoord, Face, M, face_local_chunk, face_of_chunk_y, face_of_y,
    local_index, mirror_y,
};

use std::sync::Arc;

use dwell_protocol::world_size::WorldSize;

use crate::noise::seed_word;
use crate::portals::{MIDPLANE, Portal, PortalRules, Portals};
use crate::terrain::TerrainGenerator;

/// The seed of a face's generator: face A keeps the world seed, face B's is derived from it.
pub fn face_seed(world_seed: u64, face: Face) -> u64 {
    match face {
        Face::A => world_seed,
        Face::B => {
            let lo = u64::from(seed_word(world_seed, 0xFACE_B001));
            let hi = u64::from(seed_word(world_seed, 0xFACE_B002));
            (hi << 32) | lo
        }
    }
}

#[derive(Clone, Debug)]
pub struct BifacialTerrain {
    a: TerrainGenerator,
    b: TerrainGenerator,
}

impl BifacialTerrain {
    /// The world's terrain: both faces' height fields with the terrain's volume over them
    /// (`volume.json` `terrain`: the cliffs; WORLD_GENERATION.md §5).
    pub fn new(world_seed: u64) -> Self {
        Self::sized(world_seed, WorldSize::DEFAULT)
    }

    /// [`BifacialTerrain::new`] in a world of `size` (BIG_MAP.md M2): both faces' maps, discs,
    /// build volumes and base fields the size's. At today's size, [`BifacialTerrain::new`].
    pub fn sized(world_seed: u64, size: WorldSize) -> Self {
        let t = Self::with_portal_rules_sized(world_seed, MIDPLANE.portals, size);
        let t = match crate::volume::terrain_volume() {
            Some(def) => Self {
                a: t.a.with_volume(def),
                b: t.b.with_volume(def),
            },
            None => t,
        };
        // The server founds towns on this world (`town_ground_for_sized`): its generators keep
        // the planned towns clear of trees, bushes and boulders (BUGS.md #169).
        Self {
            a: t.a
                .with_towns(crate::towns::Towns::new(world_seed, Face::A)),
            b: t.b
                .with_towns(crate::towns::Towns::new(world_seed, Face::B)),
        }
    }

    /// The height fields alone, without the terrain's volume (the layers under it, for tests and
    /// tools that look at the hills and the rivers by themselves).
    pub fn height_field(world_seed: u64) -> Self {
        Self::with_portal_rules(world_seed, MIDPLANE.portals)
    }

    /// The terrain of a terrain generator version (`dwell_world::has_terrain`); none for the
    /// other versions.
    pub fn for_generator(version: u32, world_seed: u64) -> Option<Self> {
        Self::for_generator_sized(version, world_seed, WorldSize::DEFAULT)
    }

    /// [`BifacialTerrain::for_generator`] in a world of `size`.
    pub fn for_generator_sized(version: u32, world_seed: u64, size: WorldSize) -> Option<Self> {
        (version == dwell_world::GENERATOR_TERRAIN).then(|| Self::sized(world_seed, size))
    }

    /// The world with a terrain volume over both faces' height fields (`volume.rs`), each face
    /// its own noise. The portals are sited on the height field, as on the plain terrain.
    pub fn with_volume(world_seed: u64, def: &'static crate::volume::VolumeDef) -> Self {
        let t = Self::height_field(world_seed);
        Self {
            a: t.a.with_volume(def),
            b: t.b.with_volume(def),
        }
    }

    /// The world with these portal rules in place of the build's (tests add a fixture by data
    /// alone).
    pub fn with_portal_rules(world_seed: u64, rules: PortalRules) -> Self {
        Self::with_portal_rules_sized(world_seed, rules, WorldSize::DEFAULT)
    }

    /// [`BifacialTerrain::with_portal_rules`] in a world of `size`.
    pub fn with_portal_rules_sized(world_seed: u64, rules: PortalRules, size: WorldSize) -> Self {
        // Both faces' world maps at once (on two threads natively) before either face needs one.
        crate::worldmap::cache::prefetch_sized(
            &[
                face_seed(world_seed, Face::A),
                face_seed(world_seed, Face::B),
            ],
            size,
        );
        let a = TerrainGenerator::sized(face_seed(world_seed, Face::A), size);
        let b = TerrainGenerator::sized(face_seed(world_seed, Face::B), size);
        Self::joined(world_seed, rules, a, b, false)
    }

    /// The faces joined by their portals (`portals.rs`): both faces' generators cut the same
    /// shafts, sited on both faces' ground.
    fn joined(
        world_seed: u64,
        rules: PortalRules,
        a: TerrainGenerator,
        b: TerrainGenerator,
        mirrored: bool,
    ) -> Self {
        let portals = Arc::new(Portals::resolve(world_seed, rules, &a, &b, mirrored));
        // Each face's resurrection shrines, sited on its own ground clear of the portals.
        let shrines = |g: &TerrainGenerator| {
            Arc::new(crate::shrines::Shrines::new(
                g.seed(),
                crate::shrines::rules_for(world_setting_name(g).as_deref()),
                portals.sites.clone(),
            ))
        };
        let (sa, sb) = (shrines(&a), shrines(&b));
        Self {
            a: a.with_portals(portals.clone()).with_shrines(sa),
            b: b.with_portals(portals).with_shrines(sb),
        }
    }

    /// The mirror world (a test tool for face parity, BIFACIAL_WORLD.md §11): face B generated
    /// from face A's seed, so it is face A's exact mirror image and anything drawn, collided or
    /// edited on one face can be compared with the other. Never a real world's generator.
    pub fn mirrored(world_seed: u64) -> Self {
        let a = TerrainGenerator::new(face_seed(world_seed, Face::A));
        Self::joined(world_seed, MIDPLANE.portals, a.clone(), a, true)
    }

    /// [`BifacialTerrain::mirrored`] with these biomes on both faces in place of the build's
    /// (tests add a fixture biome by data alone and compare the faces).
    pub fn mirrored_with_biomes(world_seed: u64, biomes: &[crate::terrain::Biome]) -> Self {
        let a = TerrainGenerator::new(face_seed(world_seed, Face::A)).with_biomes(biomes);
        Self::joined(world_seed, MIDPLANE.portals, a.clone(), a, true)
    }

    /// The shrine nearest (x, z) (world metres) on `face`, and where a player stands up at it
    /// (world feet), if any lies within the search (docs/SHRINES.md).
    pub fn nearest_shrine(
        &self,
        face: Face,
        x: f64,
        z: f64,
    ) -> Option<(crate::shrines::Shrine, [f64; 3])> {
        let g = self.local(face);
        let sh = g.shrines()?;
        let s = sh.nearest(g, x, z)?;
        let feet = dwell_world::FaceFrame { face }.point(g.shrine_stand(sh, s));
        Some((s, feet))
    }

    /// [`Self::nearest_shrine`]'s feet without running the spawn search (the client's guess of
    /// where it will stand up, for the respawn preload).
    pub fn nearest_known_shrine(&self, face: Face, x: f64, z: f64) -> Option<[f64; 3]> {
        let g = self.local(face);
        let sh = g.shrines()?;
        let s = sh.nearest_known(g, x, z)?;
        Some(dwell_world::FaceFrame { face }.point(g.shrine_stand(sh, s)))
    }

    /// The shrines on `face` in the grid cells within `cells` of column (x, z)'s (the map's).
    pub fn shrines_round(
        &self,
        face: Face,
        x: i32,
        z: i32,
        cells: i32,
    ) -> Vec<crate::shrines::Shrine> {
        let g = self.local(face);
        g.shrines()
            .map(|sh| sh.in_cells_round(g, x, z, cells))
            .unwrap_or_default()
    }

    /// The ground a town site needs at a face-local column (docs/BLUEPRINTS.md §6; the
    /// `TownGround` the server sites with): its height (face-local m) where it is dry land (no sea,
    /// lake, river or stream, inside the world) within `towns.heightM` above sea level, clear of the
    /// shrines and the portals, else None.
    pub fn town_ground(&self, face: Face, x: i32, z: i32) -> Option<f64> {
        crate::towns::town_ground(self.local(face), x, z)
    }

    /// The town sites on `face` in the town grid cells within `cells` of face-local column
    /// (x, z)'s (the map's; the server's own siting, `TownRules::site_in`, on this ground).
    pub fn towns_round(
        &self,
        world_seed: u64,
        face: Face,
        x: i32,
        z: i32,
        cells: i32,
    ) -> Vec<dwell_world::blueprint::TownSite> {
        let [ci, ck] = dwell_world::blueprint::SHIPPED.towns.cell_of(x, z);
        let mut out = Vec::new();
        for k in ck - cells..=ck + cells {
            for i in ci - cells..=ci + cells {
                out.extend(self.town_in(world_seed, face, [i, k]));
            }
        }
        out
    }

    /// The town site in town grid cell `cell` on `face`, if any (`TownRules::site_in` on
    /// [`Self::town_ground`]).
    pub fn town_in(
        &self,
        world_seed: u64,
        face: Face,
        cell: [i32; 2],
    ) -> Option<dwell_world::blueprint::TownSite> {
        let ground = |x: i32, z: i32| self.town_ground(face, x, z);
        dwell_world::blueprint::SHIPPED.towns.site_in(
            world_seed,
            dwell_world::FaceFrame { face },
            cell,
            &ground,
        )
    }

    /// The town site on `face` nearest face-local (x, z), within `radius` m.
    pub fn nearest_town(
        &self,
        world_seed: u64,
        face: Face,
        x: f64,
        z: f64,
        radius: f64,
    ) -> Option<dwell_world::blueprint::TownSite> {
        let f = dwell_world::FaceFrame { face };
        let ground = |x: i32, z: i32| self.town_ground(face, x, z);
        let d = |t: &dwell_world::blueprint::TownSite| {
            (f64::from(t.x) + 0.5 - x).hypot(f64::from(t.z) + 0.5 - z)
        };
        dwell_world::blueprint::SHIPPED
            .towns
            .sites_near(world_seed, f, x, z, radius, &ground)
            .into_iter()
            .min_by(|a, b| d(a).total_cmp(&d(b)))
    }

    /// The portals between the faces: their sites and how they are built.
    pub fn portals(&self) -> &Portals {
        self.a.portals().expect("both faces have the portals")
    }

    /// The portals' sites (x, z of each shaft's axis column).
    pub fn portal_sites(&self) -> &[Portal] {
        &self.portals().sites
    }

    /// A face's generator, in face-local coordinates.
    /// Turns the LOD colour map's river valleys on or off on both faces
    /// (`TerrainGenerator::set_lod_valleys`).
    pub fn set_lod_valleys(&mut self, margin: Option<f64>) {
        self.a.set_lod_valleys(margin);
        self.b.set_lod_valleys(margin);
    }

    pub fn local(&self, face: Face) -> &TerrainGenerator {
        match face {
            Face::A => &self.a,
            Face::B => &self.b,
        }
    }

    /// Chunk `coord`'s ground densities from its face's biomes (ADR 0074 B5; the chunk column's
    /// corners, face-local x and z being the world's); empty where they do not vary.
    pub fn ground_density(&self, coord: ChunkCoord) -> dwell_world::ground::GroundDensity {
        self.local(face_of_chunk_y(coord.y))
            .ground_density_chunk(coord.x, coord.z)
    }

    /// The species trees reaching chunk `coord`'s columns with their leaf tints (ADR 0079), in its
    /// face's face-local frame — the frame `dwell_mesh::mesh_chunk_trees` meshes a face-B chunk in.
    pub fn tree_tints(&self, coord: ChunkCoord) -> Vec<dwell_world::tree_tint::TreeTint> {
        const S: i32 = CHUNK_SIZE;
        let (x0, z0) = (coord.x * S, coord.z * S);
        self.local(face_of_chunk_y(coord.y))
            .tree_tints((x0, z0), (x0 + S - 1, z0 + S - 1))
    }

    /// The leaf tints of LOD section `c`'s tree cells as [`BifacialTerrain::generate_lod`] put them
    /// in `cells` (ADR 0079; `dwell_mesh::mesh_section_tinted`): each face's rows from that face's
    /// trees, the cells indexed in the section's world rows.
    pub fn lod_tints(
        &self,
        c: LodCoord,
        cells: &[u16],
    ) -> Vec<(u32, dwell_world::tree_tint::Tint)> {
        let mut out = Vec::new();
        if !lod::in_world_of(c, &self.world()) {
            return out;
        }
        let y0 = lod::section_origin(c).y;
        let first_a = lod::first_face_a_row(c);
        if first_a <= lod::SECTION_CELLS {
            self.a
                .lod_tints_at(c, y0, first_a..=lod::SECTION_CELLS, &|r| r, cells, &mut out);
        }
        if first_a >= 0 {
            let origin_l = lod::mirror_row_bottom(y0, lod::section_size(c.level));
            self.b.lod_tints_at(
                c,
                origin_l,
                -1..=(first_a - 1).min(lod::SECTION_CELLS),
                &|r| lod::SECTION_CELLS - 1 - r,
                cells,
                &mut out,
            );
        }
        out
    }

    /// Whether either face's biomes vary the ground items (ADR 0074 B5).
    pub fn ground_varies(&self) -> bool {
        self.a.ground_varies() || self.b.ground_varies()
    }

    /// A chunk of either face: face A's directly, face B's as the vertical flip of the face-local
    /// chunk with shaped blocks turned over.
    pub fn generate(&self, coord: ChunkCoord, chunk: &mut Chunk) {
        if face_of_chunk_y(coord.y) == Face::A {
            self.a.generate(coord, chunk);
            return;
        }
        let mut scratch = Chunk::default();
        self.b.generate(face_local_chunk(coord), &mut scratch);
        let src = scratch.voxels();
        let dst = chunk.voxels_mut();
        const S: i32 = CHUNK_SIZE;
        for z in 0..S {
            for y in 0..S {
                for x in 0..S {
                    dst[local_index(x, S - 1 - y, z)] = mirror(src[local_index(x, y, z)]);
                }
            }
        }
    }

    pub fn is_air_chunk(&self, coord: ChunkCoord) -> bool {
        self.local(face_of_chunk_y(coord.y))
            .is_air_chunk(face_local_chunk(coord))
    }

    /// The sky test's height for a chunk column of a face (face-local).
    pub fn sky_floor_at(&self, cx: i32, cz: i32, face: Face) -> f32 {
        self.local(face).sky_floor_at(cx, cz)
    }

    /// `TerrainGenerator::is_air_chunk_with` for a chunk in the world.
    pub fn is_air_chunk_with(
        world: &dwell_world::WorldBounds,
        coord: ChunkCoord,
        sky_floor: f32,
    ) -> bool {
        TerrainGenerator::is_air_chunk_with(world, face_local_chunk(coord), sky_floor)
    }

    /// The world's disc and build volume (its size's, BIG_MAP.md M2).
    pub fn world(&self) -> dwell_world::WorldBounds {
        self.a.world()
    }

    /// The column bounds of both faces.
    pub fn lod_bounds_at(&self, level: i32, i: i32, k: i32) -> LodBounds {
        let a = self.a.lod_bounds_at(level, i, k);
        let b = self.b.lod_bounds_at(level, i, k);
        LodBounds {
            lo: a.lo,
            hi: a.hi,
            lo_b: b.lo,
            hi_b: b.hi,
            any_inside: a.any_inside || b.any_inside,
            bifacial: true,
        }
    }

    /// A section of the world, its rows in world order. Rows belong to the face of their centre:
    /// face A's come from its generator, face B's from the mirror image of the section in face B's.
    /// `surface` receives face A's column surfaces, `surface_b` face B's (face-local).
    pub fn generate_lod(
        &self,
        c: LodCoord,
        cells: &mut LodCells,
        surface: Option<&mut LodSurfaces>,
        surface_b: Option<&mut LodSurfaces>,
    ) -> LodKind {
        let columns = (lod::PAD * lod::PAD) as usize;
        cells.clear();
        cells.resize(lod::VOLUME, M.air);
        let mut surface = surface;
        let mut surface_b = surface_b;
        for s in [surface.as_deref_mut(), surface_b.as_deref_mut()]
            .into_iter()
            .flatten()
        {
            s.clear();
            s.resize(columns, Default::default());
        }
        if !lod::in_world_of(c, &self.world()) {
            return LodKind::Empty;
        }
        let y0 = lod::section_origin(c).y;
        let first_a = lod::first_face_a_row(c);
        let mut bounds = LodBounds::default();
        let mut local = LodCells::new();
        let copy_rows = |cells: &mut LodCells,
                         local: &LodCells,
                         rows: std::ops::RangeInclusive<i32>,
                         row_of: &dyn Fn(i32) -> i32| {
            for r in rows {
                let from = row_of(r);
                for z in -1..=lod::SECTION_CELLS {
                    for x in -1..=lod::SECTION_CELLS {
                        cells[lod::cell_index(x, r, z)] = local[lod::cell_index(x, from, z)];
                    }
                }
            }
        };
        if first_a <= lod::SECTION_CELLS {
            let (_, b) = self.a.generate_lod_at(c, y0, &mut local, surface);
            bounds.lo = b.lo;
            bounds.hi = b.hi;
            bounds.any_inside = b.any_inside;
            copy_rows(cells, &local, first_a..=lod::SECTION_CELLS, &|r| r);
        }
        if first_a >= 0 {
            // Face-local row r' is world row 31 − r': the section's mirror image starts at the
            // mirror of the section's top.
            let origin_l = lod::mirror_row_bottom(y0, lod::section_size(c.level));
            let (_, b) = self.b.generate_lod_at(c, origin_l, &mut local, surface_b);
            bounds.lo_b = b.lo;
            bounds.hi_b = b.hi;
            bounds.any_inside = bounds.any_inside || b.any_inside;
            copy_rows(
                cells,
                &local,
                -1..=(first_a - 1).min(lod::SECTION_CELLS),
                &|r| lod::SECTION_CELLS - 1 - r,
            );
        }
        // Classified in this world's own disc and volume (BUGS.md #108).
        let kind = lod::kind_from_bounds_of(c, &bounds, &self.world());
        // A section the bounds decide is canonical whatever the passes generated.
        match kind {
            LodKind::Empty => cells.fill(M.air),
            LodKind::Buried => cells.fill(M.stone),
            LodKind::Content => {}
        }
        kind
    }

    /// Feet position players spawn at: always on face A.
    pub fn spawn_point(&self) -> [f64; 3] {
        self.a.spawn_point()
    }

    /// Face B's spawn point (a dev tool's: `?spawn=b` local worlds), its generator's face-local
    /// feet mirrored into the world: the player stands on face B's ground, upside down.
    pub fn spawn_point_b(&self) -> [f64; 3] {
        let [x, y, z] = self.b.spawn_point();
        [x, dwell_world::FaceFrame::B.y(y), z]
    }

    /// Face B's voxel (x, y, z) is the face-local voxel (x, mirror_y(y), z) of face B's generator.
    pub fn solid_at(&self, x: i32, y: i32, z: i32) -> bool {
        match face_of_y(i64::from(y)) {
            Face::A => self.a.solid_at(x, y, z),
            Face::B => self.b.solid_at(x, mirror_y(y), z),
        }
    }
}

/// The world setting's name a face's generator was built from (`shared/worldgen/worlds`, its
/// `name`), if it was built from one: the shrines' `worlds` overrides are keyed by it.
fn world_setting_name(g: &TerrainGenerator) -> Option<String> {
    let w = g.map().worldgen.as_ref()?;
    let v: serde_json::Value = serde_json::from_str(&w.text).ok()?;
    v.get("name")?.as_str().map(str::to_owned)
}
