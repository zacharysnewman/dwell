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
    // One merged rectangle of water over the section.
    assert_eq!(quads(&m.water), 1);
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

mod step_sides_on_slopes {
    //! Regression (playtest on 0.6.1-dev.18: everything turned green grass farther away): tops
    //! are tinted toward the material's side colour by the column's gradient (SIDE_SHARE).
    use super::*;

    fn top_colours(gradient: f64) -> Vec<String> {
        // A ramp along x rising `gradient` cells per cell, grass on stone, surfaces given.
        let mut cells = new_cells();
        let mut surface = new_surface();
        for z in -1..=32 {
            for x in -1..=32 {
                let h = 4.0 + gradient * f64::from(x + 1);
                let top = h.floor() as i32;
                for y in -1..top {
                    put(&mut cells, x, y, z, stone());
                }
                put(&mut cells, x, top, z, grass());
                let c = surface_at(x, z);
                surface[c..c + 4].copy_from_slice(&[
                    h.min(f64::from(top + 1)) as f32,
                    f32::from(grass()),
                    1.0,
                    0.0,
                ]);
            }
        }
        let m = mesh_with(&cells, &surface, 0.0);
        let mut out: Vec<String> = Vec::new();
        let (n, c) = (&m.opaque.normals, &m.opaque.colors);
        for v in 0..n.len() / 3 {
            if n[v * 3 + 1] > 0.5 {
                let k = [0, 1, 2].map(|k| format!("{:.4}", c[v * 3 + k])).join(",");
                if !out.contains(&k) {
                    out.push(k);
                }
            }
        }
        out
    }

    #[test]
    fn draws_flat_ground_in_the_top_colour_alone() {
        assert_eq!(top_colours(0.0).len(), 1);
    }

    #[test]
    fn tints_sloped_ground_toward_the_side_colour_more_where_steeper() {
        let flat = top_colours(0.0)[0].clone();
        let gentle = top_colours(0.25);
        let steep = top_colours(1.0);
        // On a steep ramp every column steps: no top is drawn in the flat top colour. A gentle one
        // is terraces (the adaptive rule rests a level run at half height) whose steps are tinted.
        assert!(gentle.iter().any(|k| *k != flat));
        assert!(!steep.contains(&flat));
        // Grass's side (dirt) is redder and less green than its top: steeper, less green.
        let green = |k: &String| {
            k.split(',')
                .nth(1)
                .and_then(|g| g.parse::<f64>().ok())
                .unwrap_or(f64::NAN)
        };
        let steep_max = steep.iter().map(green).fold(f64::MIN, f64::max);
        let gentle_min = gentle.iter().map(green).fold(f64::MAX, f64::min);
        assert!(steep_max < gentle_min);
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

mod adaptive_tops {
    //! The adaptive rule on LOD cells (ADAPTIVE_BLOCKS.md §4.4): a natural column's top cell takes
    //! the shape the rule table gives its neighbourhood of cells, as the chunks' blocks do.
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
    fn a_lone_natural_cell_is_a_half_pyramid() {
        let mut cells = new_cells();
        put(&mut cells, 3, 4, 5, grass());
        let m = mesh(&cells);
        // Four 45° faces (normal y = cos 45°) and the floor; nothing reaches the cell's top.
        assert_eq!(normal_ys(&m.opaque), [-1000, 707]);
        assert_eq!(max(m.opaque.positions.iter().skip(1).step_by(3).copied()), 4.5);
    }

    #[test]
    fn a_natural_field_rests_at_half_a_cell_and_merges() {
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
        assert_eq!(tops, [0.5]);
        let flat_tops = (0..quads(&m.opaque))
            .filter(|q| m.opaque.normals[q * 12 + 1] == 1.0)
            .count();
        assert_eq!(flat_tops, 1);
    }

    #[test]
    fn a_one_cell_step_is_one_even_slope_and_face_b_hangs() {
        // Natural ground one cell deep for x < 16, two from x = 16: the step slopes.
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
        // The lower block rises to its top, the upper sinks to its floor: ½ cell per cell, 26.57°.
        assert!(normal_ys(&a.opaque).contains(&894), "{:?}", normal_ys(&a.opaque));
        let b = mesh_section(
            &mirror_rows(&cells),
            &MeshSectionOptions {
                mirror: true,
                ..MeshSectionOptions::default()
            },
        )
        .expect("section volume");
        assert!(normal_ys(&b.opaque).contains(&-894), "{:?}", normal_ys(&b.opaque));
    }
}
