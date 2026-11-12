//! Ramps show their hits (BUGS.md #111; owner, 2026-11-12: "Slopes aren't affected visually by
//! tools when hit, though they do break.").

use super::*;
use crate::mesher::{ChunkFlags, N, PADDED_VOLUME, mesh_chunk_damaged, padded_index};
use dwell_protocol::damage::{CarveProfile, DamageLookKind, look_of};
use dwell_world::FaceFrame;

const LOCAL: [i32; 3] = [3, 5, 4];

fn ramp(block: &str) -> String {
    shaped(block, "ramp")
}

fn shaped(block: &str, shape: &str) -> String {
    format!("dwell:{block}_ramp[facing=east,flooded=false,half=bottom,shape={shape}]")
}

fn world_cell(f: FaceFrame, local: [i32; 3]) -> [i32; 3] {
    if f == FaceFrame::B {
        [local[0], N - 1 - local[1], local[2]]
    } else {
        local
    }
}

/// The face-local ramp's polygons (tags, cell units), in `faces_of` order.
fn polygons_of(state: &str) -> Vec<(u8, Vec<V3>)> {
    let id = dwell_blocks::parse_state(state).expect("a ramp state");
    faces_of(dwell_blocks::shape::shape_of(id))
        .iter()
        .map(|f| {
            let pts: Vec<V3> = f.v[..usize::from(f.count)]
                .iter()
                .map(|p| p.map(f64::from))
                .collect();
            (f.tag, pts)
        })
        .collect()
}

/// The face-local ramp's planes (coplanar polygons merged).
fn planes_of(state: &str) -> Vec<Option<Plane>> {
    merged(&polygons_of(state)).0
}

/// The sloped polygon (not along a cube face) of the face-local ramp: its index and plane.
fn slope(state: &str) -> (usize, Plane) {
    planes_of(state)
        .into_iter()
        .enumerate()
        .find_map(|(i, p)| {
            p.filter(|p| p.n.iter().filter(|x| x.abs() > 1e-6).count() > 1)
                .map(|p| (i, p))
        })
        .expect("a ramp has a slope")
}

/// A face-local point of the cell (m) as the server records it on face `f` (bytes, world rows).
fn mark_at(f: FaceFrame, p: V3) -> HitMark {
    let b = |x: f64| (x * 255.0).round().clamp(0.0, 255.0) as u8;
    let y = b(p[1] / CELL_H);
    // The slope is struck from above: face-locally the top (+Y), face B's −Y in the world.
    HitMark::at(
        if f == FaceFrame::B { 3 } else { 2 },
        [
            b(p[0]),
            if f == FaceFrame::B { 255 - y } else { y },
            b(p[2]),
        ],
    )
}

/// The opaque mesh of one face-local ramp `state` on face `f` struck at the face-local points
/// `hits` (m in the cell) with `fraction`: face-local positions in cells, triangles.
fn mesh_ramp(
    f: FaceFrame,
    state: &str,
    hits: &[V3],
    fraction: u8,
) -> (Vec<V3>, Vec<[u32; 3]>, MeshArrays) {
    let cell = world_cell(f, LOCAL);
    let mut padded = vec![0u16; PADDED_VOLUME];
    let id = dwell_blocks::parse_state(state).unwrap();
    padded[padded_index(cell[0], cell[1], cell[2])] = f.state(id);
    let view = DamageView {
        fraction,
        marks: hits.iter().map(|&p| mark_at(f, p)).collect(),
    };
    let m = mesh_chunk_damaged(
        &padded,
        &[],
        f == FaceFrame::B,
        [0, 0, 0],
        &[],
        None,
        ChunkFlags::default(),
        &[(cell, view)],
    )
    .expect("padded volume")
    .opaque;
    let pts = m
        .positions
        .chunks_exact(3)
        .map(|p| {
            let y = f64::from(p[1]) / CELL_H;
            [
                f64::from(p[0]) - f64::from(LOCAL[0]),
                if f == FaceFrame::B {
                    f64::from(N) - y
                } else {
                    y
                } - f64::from(LOCAL[1]),
                f64::from(p[2]) - f64::from(LOCAL[2]),
            ]
        })
        .collect();
    let tris = m
        .indices
        .chunks_exact(3)
        .map(|t| {
            // Face B's mesh is turned over: its winding too.
            if f == FaceFrame::B {
                [t[0], t[2], t[1]]
            } else {
                [t[0], t[1], t[2]]
            }
        })
        .collect();
    (pts, tris, m)
}

/// The volume (m³) a triangle list closes, taken about `o` (cells): the same about any origin
/// only when it is closed.
fn volume_about(pts: &[V3], tris: &[[u32; 3]], o: V3) -> f64 {
    tris.iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| sub(pts[i as usize], o));
            dot(a, cross(b, c)) / 6.0
        })
        .sum::<f64>()
        * CELL_H
}

/// Points on the slope at grid parameters (s, t), m in the cell.
fn on_slope(state: &str, st: &[[f64; 2]]) -> Vec<V3> {
    let (_, pl) = slope(state);
    st.iter().map(|&[s, t]| pl.grid_point(s, t)).collect()
}

const SPOTS: [[f64; 2]; 4] = [[0.5, 0.5], [0.3, 0.6], [0.65, 0.35], [0.45, 0.7]];

dwell_world::both_faces!(
    /// The bite rule on a ramp (BLOCK_DAMAGE.md §10, a ramp's volume ½): after k of its n hits the
    /// meshed ramp has lost k × ½ × V ÷ n (V the ramp's, a quarter of a cubic metre), half at the
    /// break, within 8 %; its mesh closed (the same volume about two origins); dirt's bowls and
    /// stone's flakes, the 45° halves too (the upper's ¼ cell carved as far as it holds), on
    /// both faces.
    fn a_ramp_after_k_hits_has_carved_k_bites_of_half_its_volume(f: FaceFrame) {
        for (block, tool, shape) in [
            ("dirt", "dwell:wooden_shovel", "ramp"),
            ("stone", "dwell:stone_pickaxe", "ramp"),
            ("dirt", "dwell:wooden_shovel", "steep_lower"),
            ("sand", "dwell:wooden_shovel", "steep_upper"),
        ] {
            let state = shaped(block, shape);
            let name = format!("dwell:{block}_ramp");
            let n = dwell_protocol::damage::hits_to_break(&name, Some(tool)).expect("it breaks");
            assert!(n > 1, "{name}: more than one hit");
            let id = dwell_blocks::parse_state(&state).unwrap();
            let whole = f64::from(dwell_blocks::shape::shape_of(id).volume)
                * dwell_protocol::damage::CELL_M3;
            let (p0, t0, _) = mesh_ramp(f, &state, &[], 0);
            let v0 = volume_about(&p0, &t0, [0.0; 3]);
            assert!(
                (v0 - whole).abs() < 1e-4,
                "{name} {shape}: whole {v0} of {whole}"
            );
            let bite = dwell_protocol::damage::bite_m3_of(&name, Some(tool)).unwrap();
            let mut last = 0.0;
            for k in 1..=n {
                let hits: Vec<V3> = on_slope(&state, &SPOTS[..(k as usize).min(4)]);
                let frac = (255.0 * f64::from(k) / f64::from(n)).round() as u8;
                let (pts, tris, _) = mesh_ramp(f, &state, &hits, frac);
                let v = volume_about(&pts, &tris, [0.0; 3]);
                let v2 = volume_about(&pts, &tris, [7.0, -3.0, 11.0]);
                assert!(
                    (v - v2).abs() < 1e-5,
                    "{name}: closed after {k} hits: {v} vs {v2}"
                );
                let got = whole - v;
                let want = dwell_protocol::damage::carved_m3(&name, f64::from(frac) / 255.0);
                assert!(got > last, "{name} {shape}: deeper with each hit");
                last = got;
                if want > 0.6 * whole {
                    // A 45° upper half holds ¼ cell, under the rule's ½ (a block's volume, not a
                    // state's): carved as far as its solid allows, inside it.
                    assert!(got < whole, "{name} {shape}: inside its solid");
                    continue;
                }
                assert!(
                    (got - want).abs() < 0.08 * want,
                    "{name} {shape}: {k} of {n} hits carved {got:.4} m³, the rule {want:.4}"
                );
                assert!((want - f64::from(k) * bite).abs() < 0.01 * want, "k bites");
            }
        }
    }
);

dwell_world::both_faces!(
    /// The slope is carved where it was struck: vertices beneath the slope's plane near the hit,
    /// the slope's far corner untouched, every vertex inside the ramp's solid (no decal off it, no
    /// dent through the floor); on both faces.
    fn the_sloped_face_is_carved_where_struck_inside_the_ramp(f: FaceFrame) {
        for block in ["dirt", "stone"] {
            let state = ramp(block);
            let (_, pl) = slope(&state);
            let hit = pl.grid_point(0.35, 0.4);
            let (pts, tris, _) = mesh_ramp(f, &state, &[hit], 255);
            assert!(tris.len() > 12, "{block}: carved, {} triangles", tris.len());
            let planes: Vec<Plane> = planes_of(&state).into_iter().flatten().collect();
            let mut deepest_near = 0.0f64;
            for p in &pts {
                let m = metres(*p);
                for q in &planes {
                    assert!(
                        dot(sub(m, q.o), q.n) < 1e-4,
                        "{block}: inside the ramp: {p:?}"
                    );
                }
                let below = -dot(sub(m, pl.o), pl.n);
                let d = sub(m, hit);
                let along = (dot(d, d) - dot(d, pl.n).powi(2)).max(0.0).sqrt();
                if along < 0.1 {
                    deepest_near = deepest_near.max(below);
                }
            }
            assert!(
                deepest_near > 0.03,
                "{block}: dug in at the hit: {deepest_near}"
            );
            // The far corner of the slope from the hit stays on the slope's plane.
            let far = pl.grid_point(1.0, 1.0);
            assert!(
                pts.iter().any(|p| {
                    let d = sub(metres(*p), far);
                    dot(d, d) < 1e-8
                }),
                "{block}: the slope's corner kept"
            );
        }
    }
);

dwell_world::both_faces!(
    /// Cracks on a ramp are staged by its damage (BLOCK_DAMAGE.md §6.3): none below the first
    /// stage, and past it every exposed polygon (the slope, the high wall, the two sides, the
    /// floor) carries the stage's tile; on both faces.
    fn a_ramps_cracks_are_staged_by_damage(f: FaceFrame) {
        let state = ramp("dirt");
        let look = look_of("dwell:dirt_ramp").expect("its damage look is data");
        let c = look.cracks.expect("dirt cracks");
        let hit = on_slope(&state, &[[0.5, 0.5]]);
        let below = ((c.stages[0] - 0.05) * 255.0) as u8;
        let (_, _, m) = mesh_ramp(f, &state, &hit, below);
        assert!(
            m.cracks.iter().all(|&x| (x >> 16) & 0xff == 0),
            "no cracks below the first stage"
        );
        let (_, _, m) = mesh_ramp(f, &state, &hit, 200);
        let stage = c.stage(200.0 / 255.0).unwrap();
        let slot = crate::textures::TILE_ORDER
            .iter()
            .position(|t| *t == c.tiles[stage])
            .unwrap() as u32
            + 1;
        assert_eq!(
            m.cracks.len(),
            m.positions.len() / 3,
            "the stream covers every vertex"
        );
        let mut normals = std::collections::HashSet::new();
        for v in 0..m.cracks.len() {
            assert_eq!(
                (m.cracks[v] >> 16) & 0xff,
                slot,
                "every vertex at the stage's tile"
            );
            // The polygon it lies on by its main normal (face-local: B's turned back).
            let n: Vec<f32> = m.normals[3 * v..3 * v + 3].to_vec();
            let ny = if f == FaceFrame::B { -n[1] } else { n[1] };
            let key = (
                n[0].signum() as i32 * i32::from(n[0].abs() > 0.3),
                ny.signum() as i32 * i32::from(ny.abs() > 0.3),
                n[2].signum() as i32 * i32::from(n[2].abs() > 0.3),
            );
            normals.insert(key);
        }
        // The slope (+X and +Y), the high wall (−X), the sides (±Z), the floor (−Y).
        for want in [(1, 1, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, -1, 0)] {
            assert!(
                normals.contains(&want),
                "a cracked polygon facing {want:?}: {normals:?}"
            );
        }
    }
);

#[test]
fn a_fixture_ramp_material_by_data_alone_gets_its_facets_and_its_bite() {
    // both-faces: one face-local ramp's polygons, carved directly.
    let look = DamageLook {
        kind: DamageLookKind::Chips,
        size: 0.4,
        color: [200, 180, 150],
        crack: [80, 60, 40],
        depth: 0.12,
        rim: 0.0,
        parallax: 0,
        darken: 0.2,
        profile: CarveProfile::Facets,
        facets: 7,
        flakes: 2,
        grid: 12,
        triangles: 400,
        cracks: None,
    };
    let state = ramp("stone");
    let polys = polygons_of(&state);
    let (k, pl) = slope(&state);
    let view = DamageView {
        fraction: 128,
        marks: vec![mark_at(FaceFrame::A, pl.grid_point(0.5, 0.5))],
    };
    let target = 0.03;
    let visible = vec![true; polys.len()];
    let d = RampDamage::of_polygons(&polys, look, target, &view, [0, 0, 0], &visible);
    assert_eq!(d.on[k], vec![0], "the hit is the slope's");
    let carve = d.carve(k).expect("the slope is carved");
    let Carve::Facets(fs) = &carve else {
        panic!("facets by its data");
    };
    assert_eq!(fs.shards[0].corners.len(), 7, "seven facets by its data");
    let got = d.volume(k, carve);
    assert!(
        (got - target).abs() < 0.02 * target,
        "its bite: {got} of {target}"
    );
    let tile = crate::textures::tile_rect("plain");
    let face = RampFace {
        cell: [0, 0, 0],
        color: 0xffffff,
        tint: [1.0; 3],
        tile: &tile,
        side_top: None,
        light: &|_| 1.0,
        flood: [1.0, 0.0],
    };
    let mut m = MeshArrays::default();
    assert!(d.draw(&mut m, d.first[k], &face));
    let mut slants = std::collections::HashSet::new();
    for t in m.normals.chunks_exact(9) {
        assert!(
            t[0..3] == t[3..6] && t[3..6] == t[6..9],
            "a facet is lit flat"
        );
        slants.insert(
            t[0..3]
                .iter()
                .map(|x| (x * 100.0).round() as i32)
                .collect::<Vec<_>>(),
        );
    }
    assert!(slants.len() >= 5, "angular facets: {} slants", slants.len());
}
