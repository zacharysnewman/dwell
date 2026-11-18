//! The damage mask (BLOCK_DAMAGE.md §15): it grows by stage, it takes the bite rule's volume out
//! of any shape (the cube, the octagonal log, a ramp, a fixture shape by data alone), and it meshes
//! within budget.

use super::*;
use crate::mesher::{ChunkFlags, N, PADDED_VOLUME, mesh_chunk_damaged, padded_index};
use dwell_protocol::damage::{DamageLookKind, look_of};
use dwell_world::FaceFrame;

/// A face-local cell and its world twin on face `f` (face B's rows turned over).
fn world_cell(f: FaceFrame, local: [i32; 3]) -> [i32; 3] {
    if f == FaceFrame::B {
        [local[0], N - 1 - local[1], local[2]]
    } else {
        local
    }
}

/// A face-local face index as face `f` names it in the world (B: up and down swap).
fn world_face(f: FaceFrame, local: u8) -> u8 {
    match (f == FaceFrame::B, local) {
        (true, 2) => 3,
        (true, 3) => 2,
        (_, x) => x,
    }
}

/// A face-local point as the server records it on face `f` (B: its height turned over).
fn world_point(f: FaceFrame, p: [u8; 3]) -> [u8; 3] {
    if f == FaceFrame::B {
        [p[0], 255 - p[1], p[2]]
    } else {
        p
    }
}

/// The opaque mesh of one `state` cell (face-local `local`) on face `f`, alone in the chunk,
/// damaged `fraction` at `marks` (face-local faces and points): its vertices in face-local cells
/// (y in cells) and triangles.
fn mesh_one(
    f: FaceFrame,
    state: u16,
    local: [i32; 3],
    marks: &[(u8, [u8; 3])],
    fraction: u8,
) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    mesh_cells(f, &[(local, state)], local, marks, fraction)
}

/// [`mesh_one`] of several cells (face-local, each its state), `local` the one damaged.
fn mesh_cells(
    f: FaceFrame,
    states: &[([i32; 3], u16)],
    local: [i32; 3],
    marks: &[(u8, [u8; 3])],
    fraction: u8,
) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    let cell = world_cell(f, local);
    let mut padded = vec![0u16; PADDED_VOLUME];
    for &(l, state) in states {
        let c = world_cell(f, l);
        padded[padded_index(c[0], c[1], c[2])] = state;
    }
    let view = DamageView {
        fraction,
        marks: marks
            .iter()
            .map(|&(face, point)| HitMark::at(world_face(f, face), world_point(f, point)))
            .collect(),
    };
    let damage = if fraction > 0 {
        vec![(cell, view)]
    } else {
        Vec::new()
    };
    let m = mesh_chunk_damaged(
        &padded,
        &[],
        f == FaceFrame::B,
        [0, 0, 0],
        &[],
        None,
        ChunkFlags::default(),
        &damage,
    )
    .expect("padded volume")
    .opaque;
    let pts = m
        .positions
        .chunks_exact(3)
        .map(|p| {
            let y = f64::from(p[1]) / CELL_H;
            [
                f64::from(p[0]),
                if f == FaceFrame::B {
                    f64::from(N) - y
                } else {
                    y
                },
                f64::from(p[2]),
            ]
        })
        .collect();
    let mut tris: Vec<[u32; 3]> = m
        .indices
        .chunks_exact(3)
        .map(|t| [t[0], t[1], t[2]])
        .collect();
    // Face B's mesh is turned over: its winding too.
    if f == FaceFrame::B {
        for t in &mut tris {
            t.swap(1, 2);
        }
    }
    (pts, tris)
}

fn fixture_look(profile: CarveProfile) -> DamageLook {
    DamageLook {
        kind: DamageLookKind::Chips,
        size: 0.4,
        color: [200, 180, 150],
        crack: [80, 60, 40],
        depth: 0.15,
        rim: 0.0,
        parallax: 0,
        darken: 0.2,
        profile,
        facets: 5,
        flakes: 2,
        grid: 8,
        triangles: 256,
        cracks: None,
    }
}

fn view(fraction: u8, marks: &[(u8, [u8; 3])]) -> DamageView {
    DamageView {
        fraction,
        marks: marks.iter().map(|&(f, p)| HitMark::at(f, p)).collect(),
    }
}

/// The mask grows by stage: each stage's removed sub-cells hold the last stage's, more of them,
/// for every profile (the same marks, more damage).
#[test]
fn the_mask_grows_by_stage() {
    // both-faces: face-local data, no up (the meshes below cover face B).
    for profile in [
        CarveProfile::Bowl,
        CarveProfile::Facets,
        CarveProfile::Spall,
    ] {
        let look = fixture_look(profile);
        let marks = [(2u8, [120u8, 255, 140]), (0, [255, 128, 90])];
        let mut last: Option<DamageMask> = None;
        for stage in 1..=8u32 {
            let fraction = (stage * 255 / 8) as u8;
            let mask = DamageMask::of(&look, &view(fraction, &marks), &Hull::cube())
                .expect("a damaged cell's mask removes something");
            let [a, b, c] = mask.dims();
            if let Some(prev) = &last {
                assert!(
                    mask.removed_count() > prev.removed_count(),
                    "{profile:?}: stage {stage} removes more"
                );
                for k in 0..c {
                    for j in 0..b {
                        for i in 0..a {
                            assert!(
                                !prev.is_removed(i, j, k) || mask.is_removed(i, j, k),
                                "{profile:?}: stage {stage} keeps the last stage's removal"
                            );
                        }
                    }
                }
            }
            last = Some(mask);
        }
        // At the break: half the cell (the bite rule), within a sub-cell.
        let mask = last.unwrap();
        let [a, b, c] = mask.dims();
        let half = (a * b * c) as f64 * BROKEN_AT;
        assert!(
            (mask.removed_count() as f64 - half).abs() <= 1.0,
            "{profile:?}: half the cell at the break: {} of {}",
            mask.removed_count(),
            a * b * c
        );
    }
}

/// No damage, no marks: no mask (the cell meshes as before).
#[test]
fn no_damage_no_mask() {
    // both-faces: face-local data.
    let look = fixture_look(CarveProfile::Bowl);
    assert!(DamageMask::of(&look, &view(0, &[(2, [128, 255, 128])]), &Hull::cube()).is_none());
    assert!(DamageMask::of(&look, &view(200, &[]), &Hull::cube()).is_none());
}

/// The mask starts where the block was struck: the first stage's sub-cells all lie against the
/// struck face, round the struck point.
#[test]
fn the_first_chunks_come_off_the_struck_face_round_the_point() {
    // both-faces: face-local data.
    for profile in [
        CarveProfile::Bowl,
        CarveProfile::Facets,
        CarveProfile::Spall,
    ] {
        let look = fixture_look(profile);
        // Struck on +X (face 0) low and toward −Z.
        let mask = DamageMask::of(&look, &view(40, &[(0, [255, 60, 60])]), &Hull::cube()).unwrap();
        let [a, b, c] = mask.dims();
        for k in 0..c {
            for j in 0..b {
                for i in 0..a {
                    if mask.is_removed(i, j, k) {
                        assert!(i >= a / 2, "{profile:?}: in the struck half ({i})");
                        assert!(k < c * 3 / 4, "{profile:?}: round the point ({k})");
                    }
                }
            }
        }
        assert!(
            (0..b).any(|j| (0..c).any(|k| mask.is_removed(a - 1, j, k))),
            "{profile:?}: from the struck face"
        );
    }
}

/// The meshed cell's closed volume (m³).
fn volume(f: FaceFrame, state: u16, local: [i32; 3], marks: &[(u8, [u8; 3])], fraction: u8) -> f64 {
    let (pts, tris) = mesh_one(f, state, local, marks, fraction);
    mesh_volume_m3(&pts, &tris)
}

dwell_world::both_faces!(
    /// A damaged octagonal log loses volume by stage (owner, 2026-11-15: "I don't see any chunks
    /// disappearing from the log"): its closed mesh is the octagon's wood less the bite rule's
    /// share at each stage (half of it at the break), struck on its side; on both faces.
    fn a_damaged_octagon_log_loses_volume_by_stage(f: FaceFrame) {
        let log = dwell_blocks::require("dwell:log");
        let local = [5, 6, 5];
        let marks = [(0u8, [255u8, 128, 128])];
        let whole = volume(f, log, local, &[], 0);
        assert!(
            whole > 0.3 && whole < CELL_H,
            "the octagon is less than its cell: {whole}"
        );
        let mut last = whole;
        for stage in 1..=4u32 {
            let fraction = (stage * 255 / 4) as u8;
            let got = volume(f, log, local, &marks, fraction);
            let want = whole * (1.0 - BROKEN_AT * f64::from(fraction) / 255.0);
            assert!(
                got < last - 1e-3,
                "stage {stage}: chunks come off ({got} after {last})"
            );
            assert!(
                (got - want).abs() < 0.02 * whole + 0.004,
                "stage {stage}: the bite rule's share gone: {got} vs {want} of {whole}"
            );
            last = got;
        }
    }
);

dwell_world::both_faces!(
    /// A 2×2 trunk's quarter cut at its seam stays closed (BUGS.md #203; owner, 2026-11-17: "the
    /// big (2x2 trees show holes in mesh when cutting inside edge)"): the four quarters' mesh
    /// encloses the trunk less the bite rule's share of the struck quarter, no hole along the
    /// joined side the carve reaches.
    fn a_cut_quarter_of_a_two_by_two_trunk_leaves_no_hole_at_its_seam(f: FaceFrame) {
        let log = dwell_blocks::require("dwell:log");
        let quarters = [[5, 6, 5], [6, 6, 5], [5, 6, 6], [6, 6, 6]];
        let states: Vec<_> = quarters.iter().map(|&q| (q, log)).collect();
        let struck = quarters[0];
        let vol = |marks: &[(u8, [u8; 3])], fraction| {
            let (pts, tris) = mesh_cells(f, &states, struck, marks, fraction);
            mesh_volume_m3(&pts, &tris)
        };
        let whole = vol(&[], 0);
        assert!(
            whole > 3.5 * CELL_H,
            "four joined quarters fill most of their cells: {whole}"
        );
        // Struck on its outer -x side right beside the seam with the quarter at +z.
        for (face, point) in [(1u8, [0u8, 128, 250]), (5u8, [250u8, 128, 0])] {
            for fraction in [128u8, 255] {
                let got = vol(&[(face, point)], fraction);
                let want = whole * (1.0 - BROKEN_AT * f64::from(fraction) / 255.0 / 4.0);
                assert!(
                    (got - want).abs() < 0.01 * whole,
                    "face {face} at {fraction}: {got} enclosed, want {want} of {whole} (a hole?)"
                );
            }
        }
    }
);

dwell_world::both_faces!(
    /// A trunk's quarter left alone, frozen with its joined sides (#204), is closed, whole and cut
    /// (BUGS.md #205: its squared sides were drawn by neither cell): it encloses a quarter of the
    /// joined trunk, less the bite rule's share when struck.
    fn a_frozen_quarter_alone_is_closed_whole_and_cut(f: FaceFrame) {
        let log = dwell_blocks::require("dwell:log");
        let quarters = [[5, 6, 5], [6, 6, 5], [5, 6, 6], [6, 6, 6]];
        let states: Vec<_> = quarters.iter().map(|&q| (q, log)).collect();
        let (pts, tris) = mesh_cells(f, &states, quarters[0], &[], 0);
        let trunk = mesh_volume_m3(&pts, &tris);
        let frozen = dwell_blocks::octagon::with_joined(log, (1 << 0) | (1 << 4)).expect("frozen");
        let vol = |marks: &[(u8, [u8; 3])], fraction| {
            let (pts, tris) = mesh_one(f, f.state(frozen), quarters[0], marks, fraction);
            mesh_volume_m3(&pts, &tris)
        };
        let whole = vol(&[], 0);
        assert!(
            (whole - trunk / 4.0).abs() < 0.01 * trunk,
            "alone {whole} of the trunk's {trunk}"
        );
        let cut = vol(&[(1, [0, 128, 250])], 255);
        let want = whole * (1.0 - BROKEN_AT);
        assert!((cut - want).abs() < 0.02 * whole, "cut {cut}, want {want}");
    }
);

dwell_world::both_faces!(
    /// A cube's damage (stone and dirt) is the same shape class as before: a hollow into the struck
    /// face that takes the bite rule's volume (k bites after k hits, half at the break), no
    /// vertex outside the cell, the faces not struck untouched near their far edges; and a stored
    /// ramp (½ a cell) loses its own share, on both faces.
    fn a_cubes_and_a_ramps_damage_takes_the_bite_rules_volume(f: FaceFrame) {
        let local = [3, 5, 4];
        for block in ["dwell:stone", "dwell:dirt"] {
            let state = dwell_blocks::require(block);
            let whole = volume(f, state, local, &[], 0);
            assert!(
                (whole - CELL_H).abs() < 1e-6,
                "{block}: the full cell {whole}"
            );
            for k in [64u8, 128, 191, 255] {
                let marks = [(2u8, [128u8, 255, 128])];
                let got = volume(f, state, local, &marks, k);
                let want = whole - dwell_protocol::damage::carved_m3(block, f64::from(k) / 255.0);
                assert!(
                    (got - want).abs() < 0.02 * whole,
                    "{block} at {k}: {got} vs {want}"
                );
                let (pts, _) = mesh_one(f, state, local, &marks, k);
                for p in &pts {
                    for a in 0..3 {
                        let c = f64::from(local[a]);
                        assert!(p[a] >= c - 1e-5 && p[a] <= c + 1.0 + 1e-5, "inside: {p:?}");
                    }
                }
            }
        }
        // A stored ramp of dirt: half a cell, its damage half of that at the break.
        let ramp = dwell_blocks::parse_state("dwell:dirt_ramp").expect("the ramp is data");
        let whole = volume(f, ramp, local, &[], 0);
        assert!(
            whole > 0.2 * CELL_H && whole < 0.8 * CELL_H,
            "a ramp: {whole}"
        );
        let got = volume(f, ramp, local, &[(2, [128, 128, 128])], 255);
        assert!(
            (got - whole * (1.0 - BROKEN_AT)).abs() < 0.03 * whole,
            "the ramp loses half its own volume at the break: {got} of {whole}"
        );
    }
);

/// A fixture block with a new shape gets damage by data alone: a ½ m slab (a shape no block in
/// the game has as a damaged block) and a fixture look, the mask taking half the slab at the
/// break and none outside it.
#[test]
fn a_fixture_shape_gets_damage_by_data_alone() {
    // both-faces: face-local data.
    // A wedge: the slab's lower half cut by a diagonal plane (a new convex shape as polygons).
    let p = |x: f64, y: f64, z: f64| [x, y, z];
    let polys = vec![
        vec![
            p(0.0, 0.0, 0.0),
            p(1.0, 0.0, 0.0),
            p(1.0, 0.0, 1.0),
            p(0.0, 0.0, 1.0),
        ],
        vec![
            p(0.0, 0.0, 0.0),
            p(0.0, 0.0, 1.0),
            p(0.0, 1.0, 1.0),
            p(0.0, 1.0, 0.0),
        ],
        vec![
            p(0.0, 1.0, 0.0),
            p(0.0, 1.0, 1.0),
            p(1.0, 0.0, 1.0),
            p(1.0, 0.0, 0.0),
        ],
        vec![p(0.0, 0.0, 0.0), p(0.0, 1.0, 0.0), p(1.0, 0.0, 0.0)],
        vec![p(0.0, 0.0, 1.0), p(1.0, 0.0, 1.0), p(0.0, 1.0, 1.0)],
    ];
    let hull = Hull::of_polygons(&polys);
    let look = DamageLook {
        grid: 12,
        ..fixture_look(CarveProfile::Facets)
    };
    let mask = DamageMask::of(&look, &view(255, &[(2, [64, 128, 128])]), &hull).unwrap();
    let [a, b, c] = mask.dims();
    let mut inside = 0.0;
    let mut gone = 0.0;
    for k in 0..c {
        for j in 0..b {
            for i in 0..a {
                let lo = [
                    i as f64 / a as f64,
                    j as f64 / b as f64,
                    k as f64 / c as f64,
                ];
                let hi = [
                    (i + 1) as f64 / a as f64,
                    (j + 1) as f64 / b as f64,
                    (k + 1) as f64 / c as f64,
                ];
                let s = share_in(&hull, lo, hi);
                inside += s;
                if mask.is_removed(i, j, k) {
                    assert!(s > 0.0, "nothing removed outside the shape");
                    gone += s;
                }
            }
        }
    }
    assert!(
        (gone / inside - BROKEN_AT).abs() < 0.02,
        "half the wedge: {gone} of {inside}"
    );
    // Its walls lie inside the shape, the clipped faces too.
    for w in mask.inner_faces(&hull, 0) {
        for q in &w.pts {
            assert!(q[0] + q[1] <= 1.0 + 1e-6, "a wall inside the wedge: {q:?}");
        }
    }
    for poly in &polys {
        for piece in mask.clip(poly) {
            assert!(area(&piece) > 0.0);
        }
    }
}

/// The mask's cost: a chunk of stone and dirt with 50 damaged cells (one to four struck faces
/// each), against the same chunk undamaged, within the old carve's budget per cell and time.
#[test]
fn meshing_fifty_damaged_cells_stays_within_budget() {
    // face-a-only: a cost measurement; the mask is face-local (the tests above cover face B).
    let (stone, dirt) = (
        dwell_blocks::require("dwell:stone"),
        dwell_blocks::require("dwell:dirt"),
    );
    let mut padded = vec![0u16; PADDED_VOLUME];
    for x in 0..N {
        for z in 0..N {
            padded[padded_index(x, 0, z)] = stone;
        }
    }
    let mut damage = Vec::new();
    for k in 0..50 {
        let (x, z) = (1 + 3 * (k % 10), 2 + 3 * (k / 10));
        let block = if k % 2 == 0 { stone } else { dirt };
        padded[padded_index(x, 1, z)] = block;
        let faces: Vec<u8> = [2u8, 0, 4, 1]
            .into_iter()
            .take(1 + k as usize % 4)
            .collect();
        damage.push((
            [x, 1, z],
            DamageView {
                fraction: (60 + 4 * k) as u8,
                marks: faces
                    .iter()
                    .map(|&f| HitMark::at(f, [(70 + 3 * k) as u8, 140, (200 - 2 * k) as u8]))
                    .collect(),
            },
        ));
    }
    let run = |d: &[([i32; 3], DamageView)]| {
        let t = std::time::Instant::now();
        let mut tris = 0;
        for _ in 0..10 {
            let m = mesh_chunk_damaged(
                &padded,
                &[],
                false,
                [0, 0, 0],
                &[],
                None,
                ChunkFlags::default(),
                d,
            )
            .unwrap();
            tris = m.opaque.indices.len() / 3;
        }
        (tris, t.elapsed().as_secs_f64() * 1000.0 / 10.0)
    };
    let (plain, plain_ms) = run(&[]);
    let (masked, masked_ms) = run(&damage);
    println!(
        "50 damaged cells: {masked} opaque triangles ({plain} undamaged, {:.0} a cell), meshed in {masked_ms:.2} ms ({plain_ms:.2} ms undamaged)",
        (masked - plain) as f64 / 50.0
    );
    assert!(masked > plain);
    let budget: usize = 50
        * look_of("dwell:dirt")
            .unwrap()
            .triangles
            .max(look_of("dwell:stone").unwrap().triangles) as usize;
    assert!(masked - plain <= budget, "within the cells' budgets");
    // Generous for a debug build on a busy machine: the mesher's time stays in the chunk's order.
    assert!(
        masked_ms < plain_ms * 4.0 + 60.0,
        "meshing time: {masked_ms:.1} ms vs {plain_ms:.1}"
    );
}

/// The opaque triangles of a chunk holding stone at the face-local `cells`, `damaged` struck hard
/// on its −x face, on face `f`: how many face +x on the plane x = `plane` (the face of the stone
/// on the damaged block's −x side, toward it).
fn faces_toward_damage(
    f: FaceFrame,
    cells: &[[i32; 3]],
    damaged: Option<[i32; 3]>,
    plane: f32,
) -> usize {
    let stone = dwell_blocks::require("dwell:stone");
    let mut padded = vec![0u16; PADDED_VOLUME];
    for &c in cells {
        let w = world_cell(f, c);
        padded[padded_index(w[0], w[1], w[2])] = stone;
    }
    let damage: Vec<_> = damaged
        .map(|c| {
            let view = DamageView {
                fraction: 230,
                marks: [[128, 128, 128], [128, 40, 200], [128, 220, 60]]
                    .iter()
                    .map(|&p| HitMark::at(world_face(f, 1), world_point(f, [0, p[1], p[2]])))
                    .collect(),
            };
            (world_cell(f, c), view)
        })
        .into_iter()
        .collect();
    let m = mesh_chunk_damaged(
        &padded,
        &[],
        f == FaceFrame::B,
        [0, 0, 0],
        &[],
        None,
        ChunkFlags::default(),
        &damage,
    )
    .expect("padded volume")
    .opaque;
    m.indices
        .chunks_exact(3)
        .filter(|t| {
            let p =
                |i: u32| -> [f32; 3] { std::array::from_fn(|k| m.positions[i as usize * 3 + k]) };
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            let on = [a, b, c].iter().all(|v| (v[0] - plane).abs() < 1e-4);
            let u: [f32; 3] = std::array::from_fn(|k| b[k] - a[k]);
            let v: [f32; 3] = std::array::from_fn(|k| c[k] - a[k]);
            on && u[1] * v[2] - u[2] * v[1] > 0.0
        })
        .count()
}

dwell_world::both_faces!(
    /// BUGS.md #187 (owner, 2026-11-15: "chunks mined away in the 3D Block damage bit mask
    /// properly render the blocks under/behind it"): the stone beside a damaged stone draws its
    /// face toward it, which the damaged block's carved chunks show through; inside a chunk and
    /// across a chunk's border (the damaged block in the next chunk). Red before: culled, the
    /// hole showed the sky.
    fn a_blocks_face_toward_a_damaged_block_draws(f: FaceFrame) {
        // Inside the chunk: stone at x 7, the damaged stone at x 8 (its −x face struck).
        let (near, hit) = ([7, 8, 8], [8, 8, 8]);
        assert_eq!(
            faces_toward_damage(f, &[near, hit], None, 8.0),
            0,
            "undamaged: hidden"
        );
        assert!(
            faces_toward_damage(f, &[near, hit], Some(hit), 8.0) > 0,
            "damaged: drawn"
        );
        // Across the border: the chunk's stone at x 0, the damaged stone at x −1 (the
        // neighbour chunk's, in this one's padding)... the face of x 0 toward −x.
        let border = |damaged: Option<[i32; 3]>| {
            // Mirror the setup in x: the damaged block on the −x side, so count −x faces at x 0.
            let stone = dwell_blocks::require("dwell:stone");
            let mut padded = vec![0u16; PADDED_VOLUME];
            for c in [[0, 8, 8], [-1, 8, 8]] {
                let w = world_cell(f, c);
                padded[padded_index(w[0], w[1], w[2])] = stone;
            }
            let damage: Vec<_> = damaged
                .map(|c| (world_cell(f, c), view(230, &[(0, [255, 128, 128])])))
                .into_iter()
                .collect();
            let m = mesh_chunk_damaged(
                &padded,
                &[],
                f == FaceFrame::B,
                [0, 0, 0],
                &[],
                None,
                ChunkFlags::default(),
                &damage,
            )
            .expect("padded volume")
            .opaque;
            m.indices
                .chunks_exact(3)
                .filter(|t| {
                    let x = |i: u32| m.positions[i as usize * 3];
                    t.iter().all(|&i| x(i).abs() < 1e-4)
                })
                .count()
        };
        assert_eq!(border(None), 0, "undamaged across the border: hidden");
        assert!(
            border(Some([-1, 8, 8])) > 0,
            "damaged across the border: drawn"
        );
    }
);

/// BUGS.md #188 (owner, 2026-11-15: "circling the tree block and destroying all the way around
/// seems to repair the other side"): hit after hit round all four sides of a block, each hit's mask
/// holds every sub-cell the last one removed, as a client keeps the views (every mark it has seen,
/// `DamageView::seen_after`, though each server view carries only its newest 4). Red before: the
/// mask was drawn from the newest 4 marks, so the first sides' bites filled back in.
#[test]
fn chopping_round_a_block_never_fills_a_bite_back_in() {
    // both-faces: face-local data, no up.
    let look = fixture_look(CarveProfile::Facets);
    // Round the sides, twice: +x, +z, −x, −z (faces 0, 4, 1, 5), mid-height.
    let sides: [(u8, [u8; 3]); 4] = [
        (0, [255, 128, 128]),
        (4, [128, 128, 255]),
        (1, [0, 128, 128]),
        (5, [128, 128, 0]),
    ];
    let need = 12u32;
    let mut seen = DamageView::default();
    let mut last: Option<DamageMask> = None;
    for hit in 1..=10u32 {
        // The server's view: the fraction so far and its newest 4 marks.
        let all: Vec<HitMark> = (1..=hit)
            .map(|h| {
                let (f, p) = sides[(h as usize - 1) % 4];
                HitMark::at(f, p)
            })
            .collect();
        let wire = DamageView {
            fraction: (hit * 255 / need) as u8,
            marks: all[all.len().saturating_sub(4)..].to_vec(),
        };
        seen = seen.seen_after(&wire);
        assert_eq!(seen.marks, all, "hit {hit}: the client keeps every mark");
        let mask = DamageMask::of(&look, &seen, &Hull::cube()).expect("a mask");
        if let Some(prev) = &last {
            let [a, b, c] = mask.dims();
            for k in 0..c {
                for j in 0..b {
                    for i in 0..a {
                        assert!(
                            !prev.is_removed(i, j, k) || mask.is_removed(i, j, k),
                            "hit {hit}: ({i}, {j}, {k}) filled back in"
                        );
                    }
                }
            }
        }
        last = Some(mask);
    }
}

#[test]
fn a_lower_fraction_starts_the_seen_marks_over() {
    // both-faces: no geometry. A healed (or new) damage drops the old bites.
    let a = DamageView {
        fraction: 200,
        marks: vec![HitMark::at(0, [255, 10, 10]), HitMark::at(1, [0, 10, 10])],
    };
    let b = DamageView {
        fraction: 20,
        marks: vec![HitMark::at(2, [10, 255, 10])],
    };
    assert_eq!(a.seen_after(&b), b);
}

/// [`faces_toward_damage`] with the block at `hit` of state `name` (stone at `near`): the +x
/// faces on the plane x = `plane`, the block at `hit` damaged or not.
fn faces_toward_shape(f: FaceFrame, name: &str, damaged: bool) -> usize {
    faces_toward_shape_beside(f, "dwell:stone", name, damaged)
}

/// [`faces_toward_shape`] with `beside` (a state's name) in place of the stone.
fn faces_toward_shape_beside(f: FaceFrame, beside: &str, name: &str, damaged: bool) -> usize {
    let stone = dwell_blocks::parse_state(beside).expect("the block is data");
    let state = dwell_blocks::parse_state(name).expect("the block is data");
    let (near, hit) = ([7, 8, 8], [8, 8, 8]);
    let mut padded = vec![0u16; PADDED_VOLUME];
    let w = world_cell(f, near);
    padded[padded_index(w[0], w[1], w[2])] = stone;
    let w = world_cell(f, hit);
    padded[padded_index(w[0], w[1], w[2])] = f.state(state);
    let damage: Vec<_> = damaged
        .then(|| {
            let view = DamageView {
                fraction: 230,
                marks: [[128, 128, 128], [128, 40, 200], [128, 220, 60]]
                    .iter()
                    .map(|&p| HitMark::at(world_face(f, 1), world_point(f, [0, p[1], p[2]])))
                    .collect(),
            };
            (world_cell(f, hit), view)
        })
        .into_iter()
        .collect();
    let m = mesh_chunk_damaged(
        &padded,
        &[],
        f == FaceFrame::B,
        [0, 0, 0],
        &[],
        None,
        ChunkFlags::default(),
        &damage,
    )
    .expect("padded volume")
    .opaque;
    m.indices
        .chunks_exact(3)
        .filter(|t| {
            let p =
                |i: u32| -> [f32; 3] { std::array::from_fn(|k| m.positions[i as usize * 3 + k]) };
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            let on = [a, b, c].iter().all(|v| (v[0] - 8.0).abs() < 1e-4);
            let u: [f32; 3] = std::array::from_fn(|k| b[k] - a[k]);
            let v: [f32; 3] = std::array::from_fn(|k| c[k] - a[k]);
            on && u[1] * v[2] - u[2] * v[1] > 0.0
        })
        .count()
}

dwell_world::both_faces!(
    /// BUGS.md #190 (owner, 2026-11-16: "regular slabs properly show the under blocks when
    /// damaged, but slopes and irregular shapes like logs don't, showing the void"): the stone
    /// beside a damaged log or ramp draws its face toward it, as beside a damaged cube (#187).
    fn a_blocks_face_toward_a_damaged_log_or_ramp_draws(f: FaceFrame) {
        for name in [
            "dwell:log",
            "dwell:stone_ramp[facing=east,flooded=false,half=bottom,shape=ramp]",
            "dwell:stone_ramp[facing=west,flooded=false,half=bottom,shape=ramp]",
        ] {
            let hidden = faces_toward_shape(f, name, false);
            if hidden > 0 {
                continue; // this one's side toward the stone hides nothing
            }
            assert!(
                faces_toward_shape(f, name, true) > 0,
                "{name} damaged: drawn"
            );
        }
        // A trunk's logs end to end (a felled one along x): the end beside a damaged log draws.
        let log = "dwell:log[axis=x]";
        assert_eq!(
            faces_toward_shape_beside(f, log, log, false),
            0,
            "undamaged: hidden"
        );
        assert!(
            faces_toward_shape_beside(f, log, log, true) > 0,
            "damaged log: drawn"
        );
    }
);

dwell_world::both_faces!(
    fn a_cannonball_is_a_sphere_of_its_radius_on_its_cells_floor(f: FaceFrame) {
        // docs/CANNONS.md C2: the `ball` look, its radius from the cannons data by name.
        let m = dwell_blocks::require("dwell:iron_ball");
        let r = dwell_world::cannons::ball("dwell:iron_ball")
            .unwrap()
            .radius_m;
        let (pts, tris) = mesh_one(f, m, [4, 4, 4], &[], 0);
        assert!(tris.len() >= 40, "a sphere's triangles: {}", tris.len());
        let lo = |k: usize| pts.iter().map(|p| p[k]).fold(f64::MAX, f64::min);
        let hi = |k: usize| pts.iter().map(|p| p[k]).fold(f64::MIN, f64::max);
        let near = |a: f64, b: f64| (a - b).abs() < 0.01;
        assert!(near(lo(1), 4.0), "on the floor: {}", lo(1));
        assert!(near(hi(1), 4.0 + 2.0 * r / CELL_H), "its top: {}", hi(1));
        assert!(
            near(lo(0), 4.5 - r) && near(hi(0), 4.5 + r),
            "x {} {}",
            lo(0),
            hi(0)
        );
        // Along z the ten segments put no vertex at ±r: its half-width is r · sin 72°.
        let half_z = 0.5 * (hi(2) - lo(2));
        assert!(
            near(0.5 * (hi(2) + lo(2)), 4.5),
            "z centre {} {}",
            lo(2),
            hi(2)
        );
        assert!(
            (0.94 * r..=r + 1e-6).contains(&half_z),
            "z half-width {half_z}"
        );
    }
);

dwell_world::both_faces!(
    fn a_ships_wheel_stands_on_its_post_spoked(f: FaceFrame) {
        // docs/CANNONS.md (owner, 2026-11-18: "A spoked wheel model"): the `wheel` look, its sizes
        // the helm's data.
        let m = dwell_blocks::require("dwell:helm[facing=north]");
        let w = dwell_world::cannons::helm("dwell:helm").unwrap().wheel;
        let (pts, tris) = mesh_one(f, f.state(m), [4, 4, 4], &[], 0);
        // Its post, hub, rim segments and spokes: boxes of 12 triangles each.
        let boxes = 2 + 2 * w.spokes as usize + w.spokes as usize;
        assert_eq!(tris.len(), boxes * 12, "{} triangles", tris.len());
        let lo = pts.iter().map(|p| p[1]).fold(f64::MAX, f64::min);
        let hi = pts.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
        assert!((lo - 4.0).abs() < 0.01, "from the cell's floor: {lo}");
        let top = 4.0 + (w.hub_m + w.radius_m + w.handle_m) / CELL_H;
        assert!((hi - top).abs() < 0.1, "to a spoke's tip: {hi} want {top}");
        // Across the facing (north, −z): wide in x, thin in z.
        let span = |k: usize| {
            let a = pts.iter().map(|p| p[k]).fold(f64::MAX, f64::min);
            let b = pts.iter().map(|p| p[k]).fold(f64::MIN, f64::max);
            b - a
        };
        assert!(
            span(0) > 2.0 * w.radius_m && span(2) < 0.3,
            "x {} z {}",
            span(0),
            span(2)
        );
    }
);
