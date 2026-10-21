//! The LOD's skirts against the ground they hide under (ARCHITECTURE.md §6.6, "Known bug class:
//! coplanar and near-coplanar surfaces"): generated seed 7 hillsides, meshed at levels 1–4 on both
//! faces as the client meshes them, and every skirt triangle checked against the generator's ground
//! (face-local, upright).
//!
//! A skirt is drawn only where its neighbour is not drawn at the same level, and it is hidden by
//! depth alone: by the neighbour's ground, a little in front of it. Where the two are nearly in one
//! plane they fight as the camera moves — the owner's dirt showing through grass from high up
//! (2026-10-21): the section below the one holding the ground showed its top skirt (dirt, the
//! section's top row) a few centimetres under the finer ground above wherever the ground crossed
//! the section's top plane, in a ring around the camera where the upper section had refined and
//! the lower one had not.

use dwell_mesh::{FlatMesh, MeshSectionOptions, SKIRT_SINK, SURFACE_STRIDE, mesh_section};
use dwell_world::lod::{self, LodCoord, LodKind, LodSurfaces};
use dwell_world::{CELL_H, Face, FaceFrame};
use dwell_worldgen::BifacialTerrain;

type V3 = [f64; 3];

fn surface_floats(s: &LodSurfaces) -> Vec<f32> {
    s.iter()
        .flat_map(|c| {
            [
                c.height,
                f32::from(c.material),
                f32::from(u8::from(c.valid) | (u8::from(c.wet) << 1)),
                c.water,
            ]
        })
        .collect()
}

/// A meshed section: its skirts' triangles (upright, face-local metres) per face-local side
/// (`FaceFrame::cell_face`: 2 is the top skirt on either face), and its cell size.
struct Section {
    skirts: [Vec<[V3; 3]>; 6],
    /// The opaque mesh's upward faces (face-local): the drawn tops.
    tops: Vec<[V3; 3]>,
    cell: f64,
    /// The section's top plane, face-local metres.
    plane: f64,
}

fn tris(m: &FlatMesh, origin: V3, scale: f64, f: FaceFrame) -> Vec<[V3; 3]> {
    m.indices
        .chunks_exact(3)
        .map(|t| {
            [0, 1, 2].map(|k| {
                let p = [0, 1, 2]
                    .map(|a| f64::from(m.positions[t[k] as usize * 3 + a]) * scale + origin[a]);
                // Upright: face B's terrain turned face-local.
                [p[0], f.y(p[1]), p[2]]
            })
        })
        .collect()
}

/// The first seed 7 hillside (a 0.15–0.6 slope inland) of face `f`, as `lod_colour.rs` picks
/// them: its x and z.
fn hillside(t: &BifacialTerrain, f: FaceFrame) -> (i32, i32) {
    let g = t.local(if f.is_b() { Face::B } else { Face::A });
    for gz in -12..=12 {
        for gx in -12..=12 {
            let (x, z) = (gx * 1500, gz * 1500);
            let c = g.column_at(x, z);
            if c.outside || c.coast < 3000.0 || c.height < dwell_world::SEA_LEVEL_M as f32 + 8.0 {
                continue;
            }
            let d = 16;
            let sx = (g.column_at(x + d, z).height - g.column_at(x - d, z).height) / (2 * d) as f32;
            let sz = (g.column_at(x, z + d).height - g.column_at(x, z - d).height) / (2 * d) as f32;
            if (0.15..0.6).contains(&sx.hypot(sz)) {
                return (x, z);
            }
        }
    }
    panic!("no hillside")
}

/// The sections of `level` with content within `reach` m of (x, z) on face `f`, meshed.
fn sections(t: &BifacialTerrain, f: FaceFrame, (x, z): (i32, i32), level: i32) -> Vec<Section> {
    let g = t.local(if f.is_b() { Face::B } else { Face::A });
    let size = lod::section_size(level);
    let cell = lod::cell_size(level) as f64;
    let reach = 2 * size;
    let sec = |v: i64, origin: i64| lod::floor_div64(v - origin, size) as i32;
    let mut lo = i32::MAX;
    let mut hi = i32::MIN;
    for dz in (-reach..=reach).step_by(8) {
        for dx in (-reach..=reach).step_by(8) {
            if let Some(y) = g.ground_y(x + dx as i32, z + dz as i32) {
                lo = lo.min(f.row(y));
                hi = hi.max(f.row(y));
            }
        }
    }
    let mut out = Vec::new();
    for k in sec(i64::from(z) - reach, lod::ORIGIN_Z)..=sec(i64::from(z) + reach, lod::ORIGIN_Z) {
        for i in sec(i64::from(x) - reach, lod::ORIGIN_X)..=sec(i64::from(x) + reach, lod::ORIGIN_X)
        {
            for j in sec(i64::from(lo), lod::ORIGIN_Y) - 1..=sec(i64::from(hi), lod::ORIGIN_Y) + 1 {
                let c = LodCoord::new(level, i, j, k);
                let mut cells = Vec::new();
                let (mut a, mut b) = (LodSurfaces::new(), LodSurfaces::new());
                if t.generate_lod(c, &mut cells, Some(&mut a), Some(&mut b)) != LodKind::Content {
                    continue;
                }
                let o = lod::section_origin(c);
                if FaceFrame::at_row(o.y as i32) != f {
                    continue;
                }
                // In cells from the section's (or its mirror image's) bottom, as the client hands
                // them to the mesher (dwell-lod `surface_in_cells`).
                let y0 = f.span(o.y, size) as f64;
                let mut surface = surface_floats(if f.is_b() { &b } else { &a });
                for col in surface.chunks_mut(SURFACE_STRIDE) {
                    col[0] = ((f64::from(col[0]) - y0) / cell) as f32;
                    col[3] = ((f64::from(col[3]) - y0) / cell) as f32;
                }
                let opts = MeshSectionOptions {
                    surface: Some(&surface),
                    water_drop: 0.125 / cell,
                    mirror: f.is_b(),
                };
                let m = mesh_section(&cells, &opts).unwrap();
                let origin = [o.x as f64, o.y as f64 * CELL_H, o.z as f64];
                let skirts =
                    std::array::from_fn(|side| tris(&m.skirts[f.cell_face(side)], origin, cell, f));
                let tops = tris(&m.opaque, origin, cell, f)
                    .into_iter()
                    .filter(|t| {
                        let (a, b) = (sub(t[1], t[0]), sub(t[2], t[0]));
                        // Upward (face-local), counter-clockwise seen from above.
                        (a[2] * b[0] - a[0] * b[2]).abs() > 1e-9
                            && (t[0][1] - t[1][1]).abs() < 1e-6
                            && (t[0][1] - t[2][1]).abs() < 1e-6
                    })
                    .collect();
                let bottom = f.y(o.y as f64 * CELL_H);
                let top = f.y((o.y + size) as f64 * CELL_H);
                out.push(Section {
                    skirts,
                    tops,
                    cell,
                    plane: bottom.max(top),
                });
            }
        }
    }
    out
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn inside_xz(t: &[V3; 3], x: f64, z: f64) -> bool {
    let side = |a: V3, b: V3| (b[0] - a[0]) * (z - a[2]) - (b[2] - a[2]) * (x - a[0]);
    let s = [side(t[0], t[1]), side(t[1], t[2]), side(t[2], t[0])];
    s.iter().all(|&v| v >= -1e-9) || s.iter().all(|&v| v <= 1e-9)
}

/// The ground (face-local metres) under the cells of size `cell` a horizontal triangle covers: at
/// each cell's centre, and the lowest of the centre and the cell's corners (just inside them).
fn ground_under(g: &dwell_worldgen::TerrainGenerator, t: &[V3; 3], cell: f64) -> Vec<(f64, f64)> {
    let (x0, x1) = (
        t.iter().map(|p| p[0]).fold(f64::MAX, f64::min),
        t.iter().map(|p| p[0]).fold(f64::MIN, f64::max),
    );
    let (z0, z1) = (
        t.iter().map(|p| p[2]).fold(f64::MAX, f64::min),
        t.iter().map(|p| p[2]).fold(f64::MIN, f64::max),
    );
    let inside = |x: f64, z: f64| inside_xz(t, x, z);
    let mut out = Vec::new();
    let mut z = z0 + cell / 2.0;
    while z < z1 {
        let mut x = x0 + cell / 2.0;
        while x < x1 {
            if inside(x, z) {
                let at = |x: f64, z: f64| {
                    f64::from(g.column_at(x.floor() as i32, z.floor() as i32).height)
                };
                let r = 0.45 * cell;
                let low = [(-r, -r), (r, -r), (-r, r), (r, r)]
                    .iter()
                    .map(|(dx, dz)| at(x + dx, z + dz))
                    .fold(at(x, z), f64::min);
                out.push((at(x, z), low));
            }
            x += cell;
        }
        z += cell;
    }
    out
}

dwell_world::both_faces!(
    /// The fix's invariant: a top skirt (a section's top row under a solid apron, which shows where
    /// the section above is not drawn at the same level) lies at least half of `SKIRT_SINK` (of a
    /// cell) under the ground at the centre of every cell it covers, so it is never in the
    /// ground's plane and the ground hides it at any depth precision; and it is sunk under the
    /// lowest ground over its cell, so few lie above the ground another level draws. Before the
    /// fix it lay on the section's top plane, millimetres under ground crossing that plane.
    fn a_top_skirt_lies_under_the_ground_by_a_margin(f: FaceFrame) {
        let t = BifacialTerrain::mirrored(7);
        let g = t.local(if f.is_b() { Face::B } else { Face::A });
        let at = hillside(&t, f);
        let (mut checked, mut poking) = (0, 0);
        for level in 1..=4 {
            for s in sections(&t, f, at, level) {
                let margin = 0.5 * SKIRT_SINK * s.cell * CELL_H;
                for tri in &s.skirts[2] {
                    let y = tri[0][1];
                    assert!(
                        tri.iter().all(|p| (p[1] - y).abs() < 1e-3),
                        "a top skirt is flat"
                    );
                    for (centre, ground) in ground_under(g, tri, s.cell) {
                        // Ground below the plane at the centre of a solid top row's cell is a
                        // tree's or an overhang's cells, not the ground this skirt stands under.
                        if centre < s.plane - 1e-3 {
                            continue;
                        }
                        checked += 1;
                        assert!(
                            centre - y >= margin,
                            "level {level}: a top skirt at {y:.3} m, {:.3} m under the ground at its cell's centre {centre:.3} m (margin {margin:.3} m), {tri:?}",
                            centre - y
                        );
                        if ground < y {
                            poking += 1;
                        }
                    }
                }
            }
        }
        assert!(
            checked > 100,
            "the hillside has top skirts to check ({checked})"
        );
        // Sunk under the lowest ground over its cell as this level sees it; full detail's ground
        // (octaves this level drops) still dips under a few, where its colour hides it (below).
        assert!(
            poking * 20 < checked,
            "{poking} of {checked} top skirts above full detail's ground"
        );
    }
);

dwell_world::both_faces!(
    /// The guard: no side skirt stands above what its section draws (ARCHITECTURE.md §6.6): every
    /// vertex lies at or under the higher drawn top of the two columns either side of its plane
    /// (the section's top plane where a column's ground is above the section), so a skirt never
    /// pokes out of the surface as a fin of dirt, whatever level its neighbour is drawn at.
    fn no_side_skirt_rises_above_the_drawn_surface(f: FaceFrame) {
        let t = BifacialTerrain::mirrored(7);
        let at = hillside(&t, f);
        let mut checked = 0;
        for level in 1..=4 {
            for s in sections(&t, f, at, level) {
                let top_at = |x: f64, z: f64| {
                    s.tops
                        .iter()
                        .filter(|t| inside_xz(t, x, z))
                        .map(|t| t[0][1])
                        .fold(f64::NAN, f64::max)
                };
                for side in [0, 1, 4, 5] {
                    for tri in &s.skirts[side] {
                        for p in tri {
                            let (dx, dz) = if side < 2 {
                                (s.cell / 2.0, 0.0)
                            } else {
                                (0.0, s.cell / 2.0)
                            };
                            // Along the plane, just inside the triangle's span.
                            let (cx, cz) = (
                                (tri[0][0] + tri[1][0] + tri[2][0]) / 3.0,
                                (tri[0][2] + tri[1][2] + tri[2][2]) / 3.0,
                            );
                            let (x, z) = if side < 2 { (p[0], cz) } else { (cx, p[2]) };
                            let hi = top_at(x - dx, z - dz).max(top_at(x + dx, z + dz));
                            let hi = if hi.is_nan() {
                                s.plane
                            } else {
                                hi.max(s.plane.min(p[1]))
                            };
                            checked += 1;
                            assert!(
                                p[1] <= hi + 1e-3,
                                "level {level}: skirt {side} vertex {p:?} above the drawn top {hi:.3} m"
                            );
                        }
                    }
                }
            }
        }
        assert!(
            checked > 100,
            "the hillside has side skirts to check ({checked})"
        );
    }
);
