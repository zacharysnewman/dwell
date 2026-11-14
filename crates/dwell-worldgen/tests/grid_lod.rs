//! A grid's LOD sections (`dwell_worldgen::grid_lod`, BUGS.md #156): on the moon and the sun,
//! both faces, a level-0 section is the grid's chunk (its cells' blocks, ramps aside), coarser
//! levels keep the ground within a cell of the chunks' and its blocks, and sections past the
//! rim are empty.

use dwell_world::lod::{self, LodCells, LodCoord, LodKind, LodSurfaces, lod_of_chunk, parent};
use dwell_world::{CELL_H, Chunk, FaceFrame, chunk_of_pos, local_index};
use dwell_worldgen::grid_lod::{GridColumns, generate_grid_lod, grid_lod_bounds, grid_world};
use dwell_worldgen::grids::CrateredTerrain;
use dwell_worldgen::volcanic::VolcanicTerrain;

fn colour(s: dwell_blocks::StateId) -> u32 {
    dwell_blocks::info(s).color
}

/// The chunk under face-local (x, z) of face `f` (the one holding its ground's top) and the
/// level-0 section that is it.
fn chunk_and_section(
    src: &dyn GridColumns,
    generate: &dyn Fn(dwell_world::ChunkCoord, &mut Chunk),
    f: FaceFrame,
    x: f64,
    z: f64,
) -> (Chunk, LodCells, LodKind) {
    let col = src.lod_column(f.face, x, z, 1.0).expect("on the disc");
    let top = f.point([x, col.ground_m.max(col.fluid_m) - 0.25, z]);
    let coord = chunk_of_pos(top);
    let mut chunk = Chunk::default();
    generate(coord, &mut chunk);
    let mut cells = LodCells::new();
    let (mut a, mut b) = (LodSurfaces::new(), LodSurfaces::new());
    let kind = generate_grid_lod(
        src,
        &grid_world(src),
        lod_of_chunk(coord),
        &mut cells,
        Some(&mut a),
        Some(&mut b),
    );
    (chunk, cells, kind)
}

/// Level 0 is the chunk: every cell's solidity and (ramps aside, whose pieces are the ground's own
/// colour) its colour, within `tolerance` of the cells (the LOD samples craters at least ½ m wide).
fn level_0_is_the_chunk(
    name: &str,
    src: &dyn GridColumns,
    generate: &dyn Fn(dwell_world::ChunkCoord, &mut Chunk),
    f: FaceFrame,
    x: f64,
    z: f64,
) {
    let (chunk, cells, kind) = chunk_and_section(src, generate, f, x, z);
    assert_eq!(kind, LodKind::Content, "{name}: the surface's section");
    let v = chunk.voxels();
    let (mut solid_off, mut colour_off, mut solid) = (0, 0, 0);
    for y in 0..32 {
        for z in 0..32 {
            for x in 0..32 {
                let c = v[local_index(x, y, z)];
                let l = cells[lod::cell_index(x, y, z)];
                let (cs, ls) = (c != dwell_world::M.air, l != dwell_world::M.air);
                solid += usize::from(cs);
                if cs != ls {
                    solid_off += 1;
                } else if cs && colour(c) != colour(l) {
                    colour_off += 1;
                }
            }
        }
    }
    assert!(solid > 0, "{name}: the chunk holds ground");
    assert!(
        solid_off * 100 <= 32 * 32 * 2,
        "{name} face {:?}: {solid_off} cells differ in solidity from the chunk",
        f.face
    );
    assert!(
        colour_off * 100 <= solid,
        "{name} face {:?}: {colour_off} of {solid} cells in another colour than the chunk's",
        f.face
    );
}

/// At levels 1–5 the surface of every column within a cell (rows) of the chunks' top there, and
/// its block the chunks' top block's colour, for the section column over (x, z).
fn coarser_levels_keep_the_ground(name: &str, src: &dyn GridColumns, f: FaceFrame, x: f64, z: f64) {
    let world = grid_world(src);
    let col = src.lod_column(f.face, x, z, 1.0).unwrap();
    let base = lod_of_chunk(chunk_of_pos(f.point([x, col.ground_m - 0.25, z])));
    for level in 1..=5 {
        let mut c = base;
        while c.level < level {
            c = parent(c);
        }
        let mut cells = LodCells::new();
        let (mut a, mut b) = (LodSurfaces::new(), LodSurfaces::new());
        let kind = generate_grid_lod(src, &world, c, &mut cells, Some(&mut a), Some(&mut b));
        if kind != LodKind::Content {
            continue; // the surface is in the section above or below
        }
        let s = if f.is_b() { &b } else { &a };
        let o = lod::section_origin(c);
        let cell = lod::cell_size(level);
        let mut seen = 0;
        for zi in 0..32 {
            for xi in 0..32 {
                let surf = s[lod::column_index(xi, zi)];
                if !surf.valid {
                    continue;
                }
                seen += 1;
                let wx = (o.x + i64::from(xi) * cell) as f64 + cell as f64 / 2.0;
                let wz = (o.z + i64::from(zi) * cell) as f64 + cell as f64 / 2.0;
                let fine = src.lod_column(f.face, wx, wz, 1.0).unwrap();
                let rows = fine.ground_m / CELL_H;
                assert!(
                    (f64::from(surf.height) - rows).abs() <= cell as f64 + 1.0,
                    "{name} level {level}: a column's top {} rows, the chunks' {rows:.1}",
                    surf.height
                );
                assert_eq!(
                    colour(surf.material),
                    colour(fine.ground),
                    "{name} level {level}"
                );
            }
        }
        assert!(
            seen > 0,
            "{name} level {level}: some column's top in its section"
        );
    }
}

dwell_world::both_faces!(
    /// BUGS.md #156: the moon's LOD sections are its chunks at level 0 and its ground coarser.
    fn the_moon_s_sections_are_its_ground(f: FaceFrame) {
        let t = CrateredTerrain::for_grid("dwell:moon", 7).unwrap();
        let make = |c, ch: &mut Chunk| t.generate(c, ch);
        let [sx, _, sz] = t.spawn_point(f.face);
        for (x, z) in [(sx, sz), (2400.5, -3100.5), (-7000.5, 5200.5)] {
            level_0_is_the_chunk("moon", &t, &make, f, x, z);
            coarser_levels_keep_the_ground("moon", &t, f, x, z);
        }
    }
);

dwell_world::both_faces!(
    /// The same on the sun, its lava over the crust included.
    fn the_sun_s_sections_are_its_ground(f: FaceFrame) {
        let t = VolcanicTerrain::for_grid("dwell:sun", 7).unwrap();
        let make = |c, ch: &mut Chunk| t.generate(c, ch);
        let [sx, _, sz] = t.spawn_point(f.face);
        for (x, z) in [(sx, sz), (-9000.5, 4000.5), (12000.5, 9000.5)] {
            level_0_is_the_chunk("sun", &t, &make, f, x, z);
            coarser_levels_keep_the_ground("sun", &t, f, x, z);
        }
    }
);

dwell_world::both_faces!(
    /// Sections past the rim are empty, the bounds of a column off the disc hold nothing, and a
    /// column of sections deep under the ground is buried (both faces' bounds).
    fn past_the_rim_nothing_and_under_the_ground_buried(f: FaceFrame) {
        let t = CrateredTerrain::for_grid("dwell:moon", 7).unwrap();
        let world = grid_world(&t);
        let mut cells = LodCells::new();
        // A level-6 section 20 km out: past the 12.5 km rim.
        let far = LodCoord::new(6, 0, 0, 0);
        let o = lod::section_origin(far);
        assert!(o.x.abs() > 20_000);
        assert_eq!(
            generate_grid_lod(&t, &world, far, &mut cells, None, None),
            LodKind::Empty
        );
        let b = grid_lod_bounds(&t, 6, 0, 0);
        assert!(!b.any_inside);
        // The section under the spawn's ground, two levels of sections down.
        let [x, _, z] = t.spawn_point(f.face);
        let col = t.lod_column(f.face, x, z, 1.0).unwrap();
        let mut c = lod_of_chunk(chunk_of_pos(f.point([x, col.ground_m - 40.0, z])));
        c = parent(c);
        assert_eq!(
            generate_grid_lod(&t, &world, c, &mut cells, None, None),
            LodKind::Buried,
            "face {:?}",
            f.face
        );
    }
);
