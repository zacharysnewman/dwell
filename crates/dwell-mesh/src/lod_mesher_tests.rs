//! `client/src/mesh/lodMesher.test.ts`.

use super::*;
use crate::textures::average_tile_color;
use dwell_blocks::require;

fn water() -> u16 {
    require("dwell:water")
}
fn grass() -> u16 {
    require("dwell:grass")
}
fn sand() -> u16 {
    require("dwell:sand")
}
fn stone() -> u16 {
    require("dwell:stone")
}
/// A full cube that is not adaptive: the cell mechanics below (cubes, skirts, surface heights)
/// apart from the adaptive rule, which shapes natural blocks' tops (`adaptive_tops` below).
fn cube_block() -> u16 {
    require("dwell:sandbox_floor")
}

fn quads(m: &FlatMesh) -> usize {
    m.indices.len() / 6
}

fn new_cells() -> Vec<u16> {
    vec![0u16; LOD_VOLUME]
}

/// Sets a cell, ignoring cells outside the section and its apron (as a typed-array store does).
fn put(cells: &mut [u16], x: i32, y: i32, z: i32, m: u16) {
    if let Some(c) = cells.get_mut(cell_index(x, y, z)) {
        *c = m;
    }
}

fn new_surface() -> Vec<f32> {
    vec![0f32; PADU * PADU * SURFACE_STRIDE]
}

fn surface_at(x: i32, z: i32) -> usize {
    col(x, z) * SURFACE_STRIDE
}

// The LOD mesher emits y scaled by the cell height (HALF_HEIGHT_GRID.md §2). These tests are about
// what is drawn where in a section's cells, so they read it back in cells: `mesh_section` below
// shadows the mesher's, dividing y by the cell height.
fn in_cells(mut m: SectionMeshes) -> SectionMeshes {
    let h = dwell_world::CELL_H;
    let fix = |a: &mut FlatMesh| {
        for p in a.positions.iter_mut().skip(1).step_by(3) {
            *p = (f64::from(*p) / h) as f32;
        }
    };
    fix(&mut m.opaque);
    fix(&mut m.water);
    m.skirts.iter_mut().for_each(fix);
    m
}
fn mesh_section(
    cells: &[u16],
    options: &MeshSectionOptions<'_>,
) -> Result<SectionMeshes, LengthError> {
    super::mesh_section(cells, options).map(in_cells)
}

fn mesh(cells: &[u16]) -> SectionMeshes {
    mesh_section(cells, &MeshSectionOptions::default()).expect("section volume")
}

fn mesh_with(cells: &[u16], surface: &[f32], water_drop: f64) -> SectionMeshes {
    mesh_section(
        cells,
        &MeshSectionOptions {
            surface: Some(surface),
            water_drop,
            mirror: false,
        },
    )
    .expect("section volume")
}

fn mirrored(cells: &[u16]) -> SectionMeshes {
    mesh_section(
        cells,
        &MeshSectionOptions {
            mirror: true,
            ..Default::default()
        },
    )
    .expect("section volume")
}

fn close(a: f64, b: f64, digits: i32) -> bool {
    (a - b).abs() < 10f64.powi(-digits) / 2.0
}

/// Distinct values, in first-seen order (a JS `Set` spread).
fn distinct(values: impl IntoIterator<Item = f32>) -> Vec<f32> {
    let mut out: Vec<f32> = Vec::new();
    for v in values {
        if !out.iter().any(|o| o.to_bits() == v.to_bits()) {
            out.push(v);
        }
    }
    out
}

fn max(values: impl IntoIterator<Item = f32>) -> f64 {
    values
        .into_iter()
        .map(f64::from)
        .fold(f64::NEG_INFINITY, f64::max)
}

mod face_b {
    use super::*;

    /// A solid slab of grass over rows [lo, hi] of a section whose apron is solid sideways.
    fn slab(lo: i32, hi: i32) -> Vec<u16> {
        let mut cells = new_cells();
        for y in lo..=hi {
            for z in -1..=32 {
                for x in -1..=32 {
                    put(&mut cells, x, y, z, cube_block());
                }
            }
        }
        cells
    }

    #[test]
    fn reverses_the_rows_of_a_section_apron_included_and_back() {
        let mut cells = new_cells();
        put(&mut cells, 3, -1, 5, 7);
        put(&mut cells, 1, 0, 2, 8);
        put(&mut cells, 4, 31, 6, 9);
        put(&mut cells, 5, 32, 7, 10);
        let m = mirror_rows(&cells);
        assert_eq!(m[cell_index(3, 32, 5)], 7);
        assert_eq!(m[cell_index(1, 31, 2)], 8);
        assert_eq!(m[cell_index(4, 0, 6)], 9);
        assert_eq!(m[cell_index(5, -1, 7)], 10);
        assert_eq!(mirror_rows(&m), cells);
    }

    fn ys(m: &FlatMesh, ny: f32) -> Vec<f32> {
        distinct(
            (0..m.normals.len() / 3)
                .filter(|v| m.normals[v * 3 + 1] == ny)
                .map(|v| m.positions[v * 3 + 1]),
        )
    }

    fn colour(m: &FlatMesh, ny: f32) -> [f32; 3] {
        let i = (0..m.normals.len())
            .find(|&n| n % 3 == 1 && m.normals[n] == ny)
            .expect("a face");
        [0, 1, 2].map(|c| m.colors[i - 1 + c])
    }

    #[test]
    fn meshes_a_ceiling_as_the_turned_over_image_of_a_floor_with_the_top_shading_on_its_underside()
    {
        // Face A: a floor of rows 0..9 (its top at y = 10). Face B: the same slab as the world has
        // it, rows 22..31 (its underside at y = 22 faces down — it is a face-B "floor").
        let a = mesh(&slab(0, 9));
        let b = mirrored(&slab(22, 31));
        assert_eq!(quads(&b.opaque), quads(&a.opaque));
        assert_eq!(ys(&a.opaque, 1.0), [10.0]);
        assert_eq!(ys(&b.opaque, -1.0), [22.0]);
        assert_eq!(ys(&a.opaque, -1.0), [0.0]);
        assert_eq!(ys(&b.opaque, 1.0), [32.0]);
        // The colour of A's top is B's underside; A's bottom, B's top.
        assert_eq!(colour(&b.opaque, -1.0), colour(&a.opaque, 1.0));
        assert_eq!(colour(&b.opaque, 1.0), colour(&a.opaque, -1.0));
    }

    #[test]
    fn winds_the_turned_over_faces_to_face_the_way_their_normals_point_and_swaps_the_y_skirts() {
        let b = mirrored(&slab(22, 31));
        let p = &b.opaque.positions;
        let vec = |arr: &[f32], i: u32| {
            let i = i as usize * 3;
            [
                f64::from(arr[i]),
                f64::from(arr[i + 1]),
                f64::from(arr[i + 2]),
            ]
        };
        let sub = |u: [f64; 3], v: [f64; 3]| [u[0] - v[0], u[1] - v[1], u[2] - v[2]];
        assert!(!b.opaque.indices.is_empty());
        for t in b.opaque.indices.chunks_exact(3) {
            let a = vec(p, t[0]);
            let e1 = sub(vec(p, t[1]), a);
            let e2 = sub(vec(p, t[2]), a);
            let cross = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let n = vec(&b.opaque.normals, t[0]);
            let dot = cross[0] * n[0] + cross[1] * n[1] + cross[2] * n[2];
            assert!(dot > 0.0); // counter-clockwise seen from the normal's side
        }
        // A floor in the top row with a cell above it: the skirt a mirrored section shows on its
        // bottom is the original's top.
        let mut rim = new_cells();
        for z in -1..=32 {
            for x in -1..=32 {
                put(&mut rim, x, 31, z, grass());
            }
        }
        put(&mut rim, 5, 32, 5, grass()); // solid above the top row: that border face is hidden
        let m = mirrored(&rim);
        let plain = mesh(&mirror_rows(&rim));
        assert_eq!(quads(&m.skirts[2]), quads(&plain.skirts[3]));
        assert_eq!(quads(&m.skirts[3]), quads(&plain.skirts[2]));
    }
}

#[test]
fn draws_a_lone_cell_as_six_faces_in_cell_units() {
    let mut cells = new_cells();
    put(&mut cells, 3, 4, 5, cube_block());
    let m = mesh(&cells);
    assert_eq!(quads(&m.opaque), 6);
    assert_eq!(max(m.opaque.positions.iter().copied()), 6.0);
    assert!(m.skirts.iter().all(|s| quads(s) == 0));
}

#[test]
fn merges_a_floor_and_puts_border_faces_the_apron_hides_into_per_side_skirts() {
    // A 32 × 32 floor at y = 0 with its neighbours' floors (the apron) around it.
    let mut cells = new_cells();
    for z in -1..=32 {
        for x in -1..=32 {
            put(&mut cells, x, 0, z, cube_block());
        }
    }
    let m = mesh(&cells);
    // Top and bottom each one merged quad; the sides are all hidden by the apron.
    assert_eq!(quads(&m.opaque), 2);
    for face in [0, 1, 4, 5] {
        assert_eq!(quads(&m.skirts[face]), 1);
    }
    assert_eq!(quads(&m.skirts[2]) + quads(&m.skirts[3]), 0);
    // Without the apron the sides are ordinary faces.
    let mut alone = new_cells();
    for z in 0..32 {
        for x in 0..32 {
            put(&mut alone, x, 0, z, cube_block());
        }
    }
    let a = mesh(&alone);
    assert_eq!(quads(&a.opaque), 6);
    assert!(a.skirts.iter().all(|s| quads(s) == 0));
}

#[test]
fn draws_water_apart_hides_it_against_water_and_colours_faces_flat() {
    let mut cells = new_cells();
    for z in -1..=32 {
        for x in -1..=32 {
            put(&mut cells, x, 0, z, sand());
            put(&mut cells, x, 1, z, water());
        }
    }
    let m = mesh(&cells);
    assert_eq!(quads(&m.water), 1); // only the surface: sides hidden by the apron's water
    // Its vertices carry water's fluid id (the water shader's parameters); other meshes none.
    let id = f32::from(dwell_blocks::fluid("dwell:water").expect("water"));
    assert_eq!(m.water.fluids, [id; 4]);
    assert!(m.opaque.fluids.is_empty());
    // The sand's sides are hidden by the apron's sand (skirts); water never gets skirts.
    for face in [0, 1, 4, 5] {
        assert_eq!(quads(&m.skirts[face]), 1);
    }
    assert_eq!(lod_color(grass(), 0), average_tile_color("grass"));
    assert_eq!(lod_color(stone(), 1), average_tile_color("stone"));
}

#[test]
fn writes_linear_vertex_colours_the_textured_chunks_are_srgb_decoded_before_lighting() {
    // Regression (phone playtest): sRGB bytes used as linear colours drew distant land paler.
    // The middle of a 7 × 7 field of grass: flat (the rule's slab) and level all round (untinted).
    let mut cells = new_cells();
    for z in 2..9 {
        for x in 0..7 {
            put(&mut cells, x, 4, z, grass());
        }
    }
    let m = mesh(&cells);
    let n = &m.opaque.normals;
    let srgb = average_tile_color("grass");
    let linear = |byte: u32| {
        let c = f64::from(byte) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let rgb = [(srgb >> 16) & 0xff, (srgb >> 8) & 0xff, srgb & 0xff];
    // Some flat top (the untinted middle) has exactly the decoded colour.
    let decoded = (0..n.len() / 3).filter(|v| n[v * 3 + 1] == 1.0).any(|top| {
        rgb.into_iter().enumerate().all(|(ch, byte)| {
            close(
                f64::from(m.opaque.colors[top * 3 + ch]),
                linear(byte) * face_tint(1, 1)[ch],
                4,
            )
        })
    });
    assert!(decoded);
}

#[test]
fn draws_a_column_top_at_its_surface_height_not_at_its_cell_top() {
    // Regression (playtest: distant land and seas looked too tall): cells fill from their bottom
    // voxel, so cell tops lift the ground by up to a cell — kilometres at the horizon.
    let mut cells = new_cells();
    let mut surface = new_surface();
    for z in -1..=32 {
        for x in -1..=32 {
            put(&mut cells, x, 0, z, cube_block()); // one cell thick
            let c = surface_at(x, z);
            surface[c] = if x < 16 { 0.5 } else { 1.0 }; // a terrace at half height, then the cell
            surface[c + 1] = f32::from(cube_block());
            surface[c + 2] = 1.0; // valid
        }
    }
    let m = mesh_with(&cells, &surface, 0.0);
    let mut tops: Vec<f64> = Vec::new();
    let mut wall_up: f64 = 0.0;
    let p = &m.opaque.positions;
    let n = &m.opaque.normals;
    for v in 0..p.len() / 3 {
        if n[v * 3 + 1] == 1.0 {
            let t = crate::js_round(f64::from(p[v * 3 + 1]) * 100.0) / 100.0;
            if !tops.contains(&t) {
                tops.push(t);
            }
        }
        // The wall between the terraces faces −X at x = 16, from 0.5 to 1.
        if n[v * 3] == -1.0 && p[v * 3] == 16.0 {
            wall_up = wall_up.max(f64::from(p[v * 3 + 1]));
        }
    }
    tops.sort_by(f64::total_cmp);
    assert_eq!(tops, [0.5, 1.0]);
    assert!(close(wall_up, 1.0, 5));
}

/// Distinct heights of the vertices whose normal's `axis` component is `sign`, sorted.
fn ys(f: &FlatMesh, axis: usize, sign: f32) -> Vec<f64> {
    let mut out: Vec<f64> = distinct(
        (0..f.positions.len() / 3)
            .filter(|v| f.normals[v * 3 + axis] == sign)
            .map(|v| f.positions[v * 3 + 1]),
    )
    .into_iter()
    .map(f64::from)
    .collect();
    out.sort_by(f64::total_cmp);
    out
}

#[test]
fn draws_a_sea_floor_inside_a_water_cell_under_a_water_surface_at_the_chunks_water_height() {
    // Regression (playtest: a seam and a height step where near water met LOD water): every level
    // draws the see-through surface 1/8 m below its cell top, over the true floor.
    let mut cells = new_cells();
    let mut surface = new_surface();
    for z in -1..=32 {
        for x in -1..=32 {
            put(&mut cells, x, -1, z, sand());
            put(&mut cells, x, 0, z, water()); // the cell holds the whole sea
            let c = surface_at(x, z);
            surface[c] = 0.5;
            surface[c + 1] = f32::from(sand()); // a sand floor
            surface[c + 2] = 3.0; // valid, wet
        }
    }
    let water_drop = 0.125 / 256.0; // level 8
    let m = mesh_with(&cells, &surface, water_drop);
    assert_eq!(ys(&m.water, 1, 1.0), [1.0 - water_drop]);
    assert_eq!(ys(&m.opaque, 1, 1.0), [0.5]);
    // One merged rectangle of water over the section, tagged with water's fluid id.
    assert_eq!(quads(&m.water), 1);
    let id = f32::from(dwell_blocks::fluid("dwell:water").expect("water"));
    assert_eq!(m.water.fluids, [id; 4]);
}

mod water_above_sea_level {
    //! Rivers and lakes: not on the cells' grid. Regression (playtest: distant rivers flooded
    //! their banks, and from the coarsest levels hung as sheets hundreds of metres up): the
    //! surface data carries each wet column's water level, and the water is drawn there.
    use super::*;

    const WATER_DROP: f64 = 0.125 / 16.0; // level 4

    struct Col {
        h: f32,
        m: u16,
        wet: bool,
        water: f32,
    }

    fn surface_of(at: impl Fn(i32, i32) -> Col) -> Vec<f32> {
        let mut surface = new_surface();
        for z in -1..=32 {
            for x in -1..=32 {
                let c = surface_at(x, z);
                let s = at(x, z);
                surface[c] = s.h;
                surface[c + 1] = f32::from(s.m);
                surface[c + 2] = if s.wet { 3.0 } else { 1.0 };
                surface[c + 3] = s.water;
            }
        }
        surface
    }

    #[test]
    fn draws_a_river_over_a_floor_in_its_cell_at_the_water_level_not_the_cell_top() {
        // Floor 118 m and water 121 m in the cell 112..128: 0.375 and 0.5625 of a cell.
        let mut cells = new_cells();
        for z in -1..=32 {
            for x in -1..=32 {
                put(&mut cells, x, -1, z, sand());
                put(&mut cells, x, 0, z, water());
            }
        }
        let surface = surface_of(|_, _| Col {
            h: 0.375,
            m: water(),
            wet: true,
            water: 0.5625,
        });
        let m = mesh_with(&cells, &surface, WATER_DROP);
        assert_eq!(ys(&m.water, 1, 1.0), [0.5625 - WATER_DROP]);
        assert!(
            ys(&m.opaque, 1, 1.0)
                .iter()
                .copied()
                .fold(f64::MIN, f64::max)
                < 0.5625 - WATER_DROP
        );
    }

    #[test]
    fn caps_a_column_of_water_cells_at_the_water_level() {
        // A lake 1.25 cells deep over a full sand cell: water cells at y = 1 and 2, the surface at
        // 2.25.
        let mut cells = new_cells();
        for z in -1..=32 {
            for x in -1..=32 {
                put(&mut cells, x, 0, z, sand());
                put(&mut cells, x, 1, z, water());
                put(&mut cells, x, 2, z, water());
            }
        }
        let surface = surface_of(|_, _| Col {
            h: 1.0,
            m: water(),
            wet: true,
            water: 2.25,
        });
        let m = mesh_with(&cells, &surface, WATER_DROP);
        assert_eq!(ys(&m.water, 1, 1.0), [2.25 - WATER_DROP]);
    }

    #[test]
    fn keeps_a_floor_that_rounds_up_to_half_a_cell_below_shallow_water() {
        // Floor 0.28, water 0.3 of the cell: the floor's half-cell step (0.5) would cover it.
        let mut cells = new_cells();
        for z in -1..=32 {
            for x in -1..=32 {
                put(&mut cells, x, -1, z, sand());
                put(&mut cells, x, 0, z, water());
            }
        }
        let surface = surface_of(|_, _| Col {
            h: 0.28,
            m: water(),
            wet: true,
            water: 0.3,
        });
        let m = mesh_with(&cells, &surface, WATER_DROP);
        // (0.3 as the f32 surface data holds it.)
        let tops = ys(&m.water, 1, 1.0);
        assert_eq!(tops.len(), 1);
        let top = tops[0];
        assert!(close(top, 0.3 - WATER_DROP, 6));
        assert!(
            ys(&m.opaque, 1, 1.0)
                .iter()
                .copied()
                .fold(f64::MIN, f64::max)
                < top
        );
    }

    #[test]
    fn ends_waters_side_faces_at_the_water_level() {
        // A pool (x < 16: a water cell over a sand cell, the surface half way up it) beside lower
        // dry ground (x ≥ 16: the sand cell only): the pool's +X face stands from its cell's
        // bottom to the water level, not to the cell's top.
        let mut cells = new_cells();
        for z in -1..=32 {
            for x in -1..=32 {
                put(&mut cells, x, 0, z, sand());
                if x < 16 {
                    put(&mut cells, x, 1, z, water());
                }
            }
        }
        let surface = surface_of(|x, _| {
            if x < 16 {
                Col {
                    h: 0.75,
                    m: water(),
                    wet: true,
                    water: 1.5,
                }
            } else {
                Col {
                    h: 1.0,
                    m: water(),
                    wet: false,
                    water: 0.0,
                }
            }
        });
        let m = mesh_with(&cells, &surface, WATER_DROP);
        assert_eq!(ys(&m.water, 1, 1.0), [1.5 - WATER_DROP]);
        let side = ys(&m.water, 0, 1.0)
            .iter()
            .copied()
            .fold(f64::MIN, f64::max);
        assert!(close(side, 1.5 - WATER_DROP, 6));
    }
}

mod riser_colours {
    //! Regression (owner, 2026-10-21: far hills turned olive and brown with each coarser level
    //! while full detail is green): a coarse riser takes the colour of the fine rise it stands for
    //! (`dwell_look::lod_colour`): on a gentle slope mostly the grass's fringe and ramps, on a cliff
    //! mostly the dirt or stone below; tops are their top colour alone, however steep. The levels'
    //! colours against full detail are measured in `tests/lod_colour.rs`.
    use super::*;

    fn dirt() -> u16 {
        require("dwell:dirt")
    }

    /// Ground of height `h(x)` cells along x: grass over `under`, the generator's exact surface
    /// given; face A's cells and surface.
    fn ground(h: impl Fn(i32) -> f64, under: u16) -> (Vec<u16>, Vec<f32>) {
        let mut cells = new_cells();
        let mut surface = new_surface();
        for z in -1..=32 {
            for x in -1..=32 {
                let h = h(x);
                let top = h.floor() as i32;
                for y in -1..top {
                    put(&mut cells, x, y, z, under);
                }
                put(&mut cells, x, top, z, grass());
                let c = surface_at(x, z);
                surface[c..c + 4].copy_from_slice(&[h as f32, f32::from(grass()), 1.0, 0.0]);
            }
        }
        (cells, surface)
    }

    /// A slope rising `rise` cells per cell along x.
    fn slope(rise: f64, under: u16) -> (Vec<u16>, Vec<f32>) {
        ground(|x| (1.0 + rise * f64::from(x + 1)).min(31.5), under)
    }

    /// A 16-cell cliff at x = 16 (a level-L cell is 2^L × ½ m tall and 2^L m wide: 8 m per m).
    fn cliff(under: u16) -> (Vec<u16>, Vec<f32>) {
        ground(|x| if x < 16 { 2.5 } else { 18.5 }, under)
    }

    /// Area-weighted mean linear colour of the drawn faces (not the skirts) with normal `n`.
    fn mean_colour(m: &SectionMeshes, n: [f32; 3]) -> Option<[f64; 3]> {
        let f = &m.opaque;
        let (mut sum, mut total) = ([0.0; 3], 0.0);
        let p = |i: u32| [0, 1, 2].map(|a| f64::from(f.positions[i as usize * 3 + a]));
        for t in f.indices.chunks_exact(3) {
            let v = t[0] as usize;
            if !(0..3).all(|a| (f.normals[v * 3 + a] - n[a]).abs() < 1e-3) {
                continue;
            }
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            let (u, w) = (
                [0, 1, 2].map(|k| b[k] - a[k]),
                [0, 1, 2].map(|k| c[k] - a[k]),
            );
            let x = [
                u[1] * w[2] - u[2] * w[1],
                u[2] * w[0] - u[0] * w[2],
                u[0] * w[1] - u[1] * w[0],
            ];
            let area = (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt() / 2.0;
            for (k, s) in sum.iter_mut().enumerate() {
                *s += area * f64::from(f.colors[v * 3 + k]);
            }
            total += area;
        }
        (total > 0.0).then(|| sum.map(|v| v / total))
    }

    fn tinted(color: u32, axis: usize, sign: i32) -> [f64; 3] {
        linear_rgb(color, face_tint(axis, sign)).map(f64::from)
    }

    fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
        (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
    }

    /// Both faces: face A's section, and face B's (its mirror image, meshed mirrored), with the
    /// same surface.
    fn both_faces((cells, surface): (Vec<u16>, Vec<f32>)) -> [SectionMeshes; 2] {
        let a = mesh_with(&cells, &surface, 0.0);
        let b = mesh_section(
            &mirror_rows(&cells),
            &MeshSectionOptions {
                surface: Some(&surface),
                water_drop: 0.0,
                mirror: true,
            },
        )
        .expect("section volume");
        [a, b]
    }

    /// The risers' mean colour (they face −x: the ground rises along +x), on both faces alike.
    fn risers(section: (Vec<u16>, Vec<f32>)) -> [f64; 3] {
        let [a, b] = both_faces(section);
        let wa = mean_colour(&a, [-1.0, 0.0, 0.0]).expect("risers");
        let wb = mean_colour(&b, [-1.0, 0.0, 0.0]).expect("face B's risers");
        assert!(distance(wa, wb) < 1e-6, "both faces alike: {wa:?} {wb:?}");
        wa
    }

    #[test]
    fn draws_the_tops_of_any_ground_in_the_top_colour_alone() {
        let top = tinted(lod_color(grass(), 0), 1, 1);
        for section in [
            slope(0.0, dirt()),
            slope(0.25, dirt()),
            slope(0.6, dirt()),
            cliff(dirt()),
        ] {
            let [a, b] = both_faces(section);
            let up = mean_colour(&a, [0.0, 1.0, 0.0]).expect("tops");
            assert!(distance(up, top) < 1e-6, "{up:?} vs {top:?}");
            // Face B's ground faces down: its tops are the faces of normal −y, shaded alike.
            let down = mean_colour(&b, [0.0, -1.0, 0.0]).expect("face B's tops");
            assert!(distance(down, top) < 1e-6, "face B: {down:?}");
        }
    }

    #[test]
    fn colours_a_gentle_slopes_risers_mostly_grass_and_a_cliffs_mostly_what_is_below_it() {
        let grass_top = tinted(lod_color(grass(), 0), 0, -1);
        let dirt_side = tinted(lod_color(dirt(), 1), 0, -1);
        let stone_side = tinted(lod_color(stone(), 1), 0, -1);
        // Gentle (a quarter cell per cell: 0.125 m per m at any level): green, near the grass.
        // Before: a riser under grass took grassSide's whole-tile average, half dirt.
        let gentle = risers(slope(0.25, dirt()));
        assert!(
            distance(gentle, grass_top) < distance(gentle, dirt_side) / 3.0,
            "{gentle:?}"
        );
        // A cliff: mostly the dirt or the stone below its fringe, at every level.
        let earth = risers(cliff(dirt()));
        assert!(
            distance(earth, dirt_side) < distance(earth, grass_top) / 3.0,
            "{earth:?}"
        );
        let rock = risers(cliff(stone()));
        assert!(
            distance(rock, stone_side) < distance(rock, grass_top) / 3.0,
            "{rock:?}"
        );
    }

    #[test]
    fn mixes_a_riser_by_the_makeup_at_its_gradient() {
        // A riser of material m topped by grass at gradient g is the makeup's mix of grass's top
        // (shaded as a ramp), grass's fringe band and m's lower band.
        let g = 0.4;
        let [wt, wf, wr] = LOD_COLOUR.makeup(g);
        let got = riser_color(dirt(), grass(), gradient_step(g), 2, 1);
        let ramp = ramp_tint_ratio(2, 1);
        let parts = [
            lod_color(grass(), 0),
            side_bands(grass())[0],
            side_bands(dirt())[1],
        ];
        for (ch, shift) in [16, 8, 0].into_iter().enumerate() {
            let [a, b, r] = parts.map(|k| srgb_to_linear(((k >> shift) & 0xff) as u8));
            let want = linear_to_srgb_byte(a * ramp[ch] * wt + b * wf + r * wr);
            assert_eq!((got >> shift) & 0xff, u32::from(want));
        }
        // The fringe band is the side tile's top half: grass's is green, its lower band dirt.
        let [fringe, lower] = side_bands(grass());
        assert!((fringe >> 8) & 0xff > (lower >> 8) & 0xff);
    }
}

mod shores {
    //! The ground beside water at its true height. Regression (playtest on 0.6.1-dev.18: river and
    //! lake shores were swallowed by the water beyond full detail, and sand stuck up out of it):
    //! beside water the ground is drawn at its height, not in half-cell steps.
    use super::*;

    const WATER_DROP: f64 = 0.125 / 16.0;
    const LEVEL: f32 = 0.5625; // 121 m in the cell 112..128

    /// The highest upward face over the centre of the column beside the river (16.5, 10.5).
    fn top_at_column(m: &SectionMeshes) -> f64 {
        let mut top = f64::NEG_INFINITY;
        let FlatMesh {
            positions: p,
            normals: n,
            indices,
            ..
        } = &m.opaque;
        for t in indices.chunks_exact(3) {
            let vs = [t[0] as usize, t[1] as usize, t[2] as usize];
            if n[vs[0] * 3 + 1] <= 0.5 {
                continue;
            }
            let xs = vs.map(|v| f64::from(p[v * 3]));
            let zs = vs.map(|v| f64::from(p[v * 3 + 2]));
            let min = |a: [f64; 3]| a.iter().copied().fold(f64::INFINITY, f64::min);
            let max = |a: [f64; 3]| a.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            if min(xs) > 16.5 || max(xs) < 16.5 || min(zs) > 10.5 || max(zs) < 10.5 {
                continue;
            }
            for v in vs {
                top = top.max(f64::from(p[v * 3 + 1]));
            }
        }
        top
    }

    fn at(h: f32) -> f64 {
        let mut cells = new_cells();
        let mut surface = new_surface();
        for z in -1..=32 {
            for x in -1..=32 {
                let c = surface_at(x, z);
                put(&mut cells, x, -1, z, sand());
                if x < 16 {
                    put(&mut cells, x, 0, z, water()); // the river: floor 118 m, water 121 m
                    surface[c..c + 4].copy_from_slice(&[0.375, f32::from(sand()), 3.0, LEVEL]);
                } else {
                    put(&mut cells, x, 0, z, sand()); // the bank: sand at h
                    surface[c..c + 4].copy_from_slice(&[h, f32::from(sand()), 1.0, 0.0]);
                }
            }
        }
        top_at_column(&mesh_with(&cells, &surface, WATER_DROP))
    }

    #[test]
    fn does_not_round_a_bank_just_above_the_water_down_under_it() {
        // 121.6 m: half-cell steps put it at 120 m.
        assert!(close(at(0.6), f64::from(0.6f32), 5));
    }

    #[test]
    fn does_not_round_a_bank_just_above_the_water_up_to_its_cells_top() {
        // 124.8 m: half-cell steps put it at 128 m.
        assert!(close(at(0.8), f64::from(0.8f32), 5));
    }

    #[test]
    fn keeps_a_beach_above_the_sea_in_the_section_above_it() {
        // A section whose bottom is sea level: the sea is the apron row below, so this section's
        // surface data has no wet column. A beach 0.8 m up in a 16 m cell (0.05).
        let mut cells = new_cells();
        let mut surface = new_surface();
        for z in -1..=32 {
            for x in -1..=32 {
                if x < 16 {
                    put(&mut cells, x, -1, z, water()); // the sea's top cell, below the section
                } else {
                    put(&mut cells, x, -1, z, sand());
                    put(&mut cells, x, 0, z, sand()); // the beach
                    let c = surface_at(x, z);
                    surface[c..c + 4].copy_from_slice(&[0.05, f32::from(sand()), 1.0, 0.0]);
                }
            }
        }
        let top = top_at_column(&mesh_with(&cells, &surface, WATER_DROP));
        assert!(close(top, 0.05, 5));
    }
}

#[test]
fn never_draws_a_sea_floor_in_the_water_surface_or_over_the_cell_below() {
    // Regression (playtest: z-fighting on distant water): a floor inside a water cell, rounded to
    // half cells, landed on the cell's bottom or level with the water surface.
    /// Upward quads by height, in first-seen order.
    fn upward(f: &FlatMesh) -> Vec<(f32, usize)> {
        let mut at: Vec<(f32, usize)> = Vec::new();
        for q in 0..f.indices.len() / 6 {
            let v = f.indices[q * 6] as usize;
            if f.normals[v * 3 + 1] != 1.0 {
                continue;
            }
            let y = f.positions[v * 3 + 1];
            match at.iter_mut().find(|(k, _)| *k == y) {
                Some((_, n)) => *n += 1,
                None => at.push((y, 1)),
            }
        }
        at
    }
    for floor in [0.1f32, 0.9] {
        let mut cells = new_cells();
        let mut surface = new_surface();
        for z in -1..=32 {
            for x in -1..=32 {
                put(&mut cells, x, 0, z, sand());
                put(&mut cells, x, 1, z, water());
                let c = surface_at(x, z);
                surface[c] = 1.0 + floor;
                surface[c + 1] = f32::from(grass()); // a floor of another material than below
                surface[c + 2] = 3.0; // valid, wet
            }
        }
        let water_drop = 0.125 / 256.0; // level 8
        let m = mesh_with(&cells, &surface, water_drop);
        let tops = upward(&m.opaque);
        // One floor, drawn once, clearly under the water surface.
        assert_eq!(
            tops.iter().map(|t| t.1).collect::<Vec<_>>(),
            [1],
            "floor {floor}"
        );
        assert!(tops[0].0 <= 1.5, "floor {floor}");
        let water_tops: Vec<f64> = upward(&m.water).iter().map(|t| f64::from(t.0)).collect();
        assert_eq!(water_tops, [2.0 - water_drop]);
    }
}

#[test]
fn closes_steps_between_surfaces_across_the_section_border_without_skirts() {
    // Regression (playtest: sky-blue cracks along straight lines): where a column's surface was
    // lower than its neighbour across the border, the step between them went only into a skirt.
    let mut surface = new_surface();
    let mut cells = new_cells();
    let set = |cells: &mut Vec<u16>, surface: &mut Vec<f32>, x: i32, z: i32, top: i32, h: f32| {
        for y in -1..=top {
            put(cells, x, y, z, cube_block());
        }
        let c = surface_at(x, z);
        surface[c] = h;
        surface[c + 1] = f32::from(cube_block());
        surface[c + 2] = 1.0;
    };
    for z in -1..=32 {
        set(&mut cells, &mut surface, -1, z, 1, 1.5); // across the −X border: a full cell 1
        set(&mut cells, &mut surface, 0, z, 0, 0.5); // ours: half way up cell 0
        for x in 1..=32 {
            set(&mut cells, &mut surface, x, z, 0, 0.5);
        }
    }
    // On the −X border plane, the neighbour's cell 0 shows above our surface: from 0.5 to 1,
    // facing us (+X), in the opaque mesh.
    let covered = |m: &SectionMeshes, facing: f32| {
        let (p, n) = (&m.opaque.positions, &m.opaque.normals);
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for v in 0..p.len() / 3 {
            if p[v * 3] != 0.0 || n[v * 3] != facing {
                continue;
            }
            lo = lo.min(f64::from(p[v * 3 + 1]));
            hi = hi.max(f64::from(p[v * 3 + 1]));
        }
        [lo, hi]
    };
    assert_eq!(covered(&mesh_with(&cells, &surface, 0.0), 1.0), [0.5, 1.0]);
    // Both surfaces in cell 0, ours higher: our side shows from the neighbour's (0) to ours (0.5),
    // facing it (−X).
    cells.fill(0);
    surface.fill(0.0);
    for z in -1..=32 {
        set(&mut cells, &mut surface, -1, z, 0, 0.0);
        for x in 0..=32 {
            set(&mut cells, &mut surface, x, z, 0, 0.5);
        }
    }
    assert_eq!(covered(&mesh_with(&cells, &surface, 0.0), -1.0), [0.0, 0.5]);
}

#[test]
fn rejects_a_wrong_cell_count_and_accounts_bytes() {
    // The TS throws a RangeError; `sectionBuffers` (transferables) is covered by `section_bytes`,
    // the port of `sectionBytes`.
    assert_eq!(
        mesh_section(&[0u16; 3], &MeshSectionOptions::default()),
        Err(LengthError {
            expected: LOD_VOLUME,
            got: 3
        })
    );
    let mut cells = new_cells();
    put(&mut cells, 3, 4, 5, cube_block());
    let m = mesh(&cells);
    // 24 vertices × 9 floats + 36 indices, 4 bytes each.
    assert_eq!(section_bytes(&m), 4 * (24 * 9 + 36));
}

mod natural_tops {
    //! Natural LOD cells on the half-height grid (Phase 41): no block is adaptive, so a natural
    //! column's top cell is a block, as the chunks' are (Phase 18 shaped it by the rule table).
    use super::*;

    fn normal_ys(m: &FlatMesh) -> Vec<i32> {
        let mut out: Vec<i32> = (0..m.normals.len() / 3)
            .map(|v| (m.normals[v * 3 + 1] * 1000.0).round() as i32)
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    #[test]
    fn a_lone_natural_cell_is_a_block() {
        let mut cells = new_cells();
        put(&mut cells, 3, 4, 5, grass());
        let m = mesh(&cells);
        assert_eq!(normal_ys(&m.opaque), [-1000, 0, 1000]);
        assert_eq!(
            max(m.opaque.positions.iter().skip(1).step_by(3).copied()),
            5.0
        );
    }

    #[test]
    fn a_natural_field_is_flat_at_its_top_and_merges() {
        let mut cells = new_cells();
        for z in -1..=32 {
            for x in -1..=32 {
                put(&mut cells, x, 0, z, grass());
            }
        }
        let m = mesh(&cells);
        let tops = distinct(
            (0..m.opaque.normals.len() / 3)
                .filter(|v| m.opaque.normals[v * 3 + 1] == 1.0)
                .map(|v| m.opaque.positions[v * 3 + 1]),
        );
        assert_eq!(tops, [1.0]);
        let flat_tops = (0..quads(&m.opaque))
            .filter(|q| m.opaque.normals[q * 12 + 1] == 1.0)
            .count();
        assert_eq!(flat_tops, 1);
    }

    // both-faces: meshes the step on face A and its face-B mirror and compares them
    #[test]
    fn a_one_cell_step_is_a_step_and_face_b_mirrors_it() {
        let mut cells = new_cells();
        for z in -1..=32 {
            for x in -1..=32 {
                put(&mut cells, x, 0, z, grass());
                if x >= 16 {
                    put(&mut cells, x, 1, z, grass());
                }
            }
        }
        let a = mesh(&cells);
        assert_eq!(normal_ys(&a.opaque), [-1000, 0, 1000]);
        let b = mesh_section(
            &mirror_rows(&cells),
            &MeshSectionOptions {
                mirror: true,
                ..MeshSectionOptions::default()
            },
        )
        .expect("section volume");
        assert_eq!(normal_ys(&b.opaque), [-1000, 0, 1000]);
    }
}

/// The owner's dirt through the grass from high up (2026-10-21; ARCHITECTURE.md §6.6, "Known bug
/// class"): a section under the one holding the ground — dirt to its top, the apron above solid,
/// the grass a little over its top plane — has its top row as a top skirt, shown where the section
/// above is drawn at another level. It lay on the plane in dirt's colour, where the other level's
/// ground (a few millimetres up, or rounded under the plane) let it through. It is sunk
/// `SKIRT_SINK` under the plane and coloured as the ground it stands in for, and the column's side
/// skirts mix the ground's colour as a riser does. Both faces: face B meshes the mirror image.
mod top_skirts {
    use super::*;

    fn dirt() -> u16 {
        require("dwell:dirt")
    }

    /// Dirt filling the section and the apron above it; the generator's ground `h` cells over the
    /// section's bottom (above its top), grass.
    fn buried(h: f64) -> (Vec<u16>, Vec<f32>) {
        let mut cells = new_cells();
        let mut surface = new_surface();
        for z in -1..=32 {
            for x in -1..=32 {
                for y in -1..=32 {
                    put(&mut cells, x, y, z, dirt());
                }
                let c = surface_at(x, z);
                surface[c..c + 4].copy_from_slice(&[h as f32, f32::from(grass()), 1.0, 0.0]);
            }
        }
        (cells, surface)
    }

    fn meshed(f: dwell_world::FaceFrame, (cells, surface): (Vec<u16>, Vec<f32>)) -> SectionMeshes {
        let cells = if f.is_b() { mirror_rows(&cells) } else { cells };
        mesh_section(
            &cells,
            &MeshSectionOptions {
                surface: Some(&surface),
                water_drop: 0.0,
                mirror: f.is_b(),
            },
        )
        .expect("section volume")
    }

    fn colours(m: &FlatMesh) -> Vec<[f32; 3]> {
        m.colors
            .chunks_exact(3)
            .map(|c| [c[0], c[1], c[2]])
            .collect()
    }

    dwell_world::both_faces!(
        fn lie_under_the_ground_they_stand_for_in_the_grounds_colour(f: dwell_world::FaceFrame) {
            // The grass 2 cm (in cells) over the plane: on the plane, the skirt was in its plane.
            let m = meshed(f, buried(32.02));
            let top = &m.skirts[f.cell_face(2)];
            assert!(!top.positions.is_empty(), "a top skirt");
            for p in top.positions.chunks_exact(3) {
                // Face-local: face B's mesh is turned back, its y read from the section's top.
                let y = f64::from(p[1]);
                let y = if f.is_b() { f64::from(N) - y } else { y };
                assert!(close(y, 32.0 - SKIRT_SINK, 6), "sunk SKIRT_SINK: {y}");
            }
            let grass_top = linear_rgb(lod_color(grass(), 0), face_tint(1, 1));
            for c in colours(top) {
                for a in 0..3 {
                    assert!(
                        (c[a] - grass_top[a]).abs() < 1e-6,
                        "grass's top colour, not dirt's: {c:?} vs {grass_top:?}"
                    );
                }
            }
            // The side skirts mix the grass in, as the riser of a grass slope does, not dirt alone.
            let dirt_side = linear_rgb(side_bands(dirt())[1], face_tint(0, 1));
            for c in colours(&m.skirts[0]) {
                assert!(
                    c[1] > dirt_side[1] * 1.2,
                    "a side skirt under grass is not dirt's side: {c:?} vs {dirt_side:?}"
                );
            }
        }
    );
}
