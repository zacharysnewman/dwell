//! The pick ray against a ramp at a cliff edge (BUGS.md #124): the ray meets the wedge's sloped
//! face, its real shape, from every angle that face is seen, not passing over it into the drop.
//! Each ray is checked against an oracle that marches it through the cells' solids in small steps.

use dwell_blocks::shape::{point_in_solid, shape_of};
use dwell_blocks::{StateId, require};
use dwell_protocol::{BlockEditAction, BlockEditRequest};
use dwell_world::edit::{DEFAULT_REACH, EditCheck, check_block_edit, raycast_block};
use dwell_world::*;

fn s(name: &str) -> StateId {
    require(name)
}

/// The row of the land's top cell (face-local).
const TOP: i32 = 20;

/// The facings and the way each descends (x, z), face-locally.
const FACINGS: [(&str, [i32; 2]); 4] = [
    ("north", [0, -1]),
    ("east", [1, 0]),
    ("south", [0, 1]),
    ("west", [-1, 0]),
];

/// A face-local cell `a` columns along `out` (the edge column at 0, the land behind it below 0,
/// the drop beyond), `b` across it, row `y`.
fn at(out: [i32; 2], a: i32, b: i32, y: i32) -> [i32; 3] {
    [out[0] * a - out[1] * b, y, out[1] * a + out[0] * b]
}

/// How the ramp sits on the edge.
#[derive(Clone, Copy, Debug)]
enum Edge {
    /// The edge column's top rows are the ramp, descending over the drop (the generator's).
    Shaped,
    /// Placed on the edge column's top, uphill ahead of the player (the hotbar's slope, its
    /// `facing` toward the player): its high side over the drop.
    PlacedUphill,
    /// Placed on the edge column's top, descending over the drop.
    PlacedDownhill,
}

/// Face-local stone land behind and on the edge column up to row `TOP`, the ramp `shapes` (lowest
/// first) of `block` on the edge as `edge` says, the drop (air) beyond.
fn cliff(f: FaceFrame, block: &str, out: [i32; 2], edge: Edge, shapes: &[&str]) -> VoxelWorld {
    let mut w = VoxelWorld::new(empty_generator(), None);
    let stone = s("dwell:stone");
    let set = |w: &mut VoxelWorld, c: [i32; 3], m: StateId| {
        let [x, y, z] = f.cell(c);
        w.set_voxel(x, y, z, f.state(m));
    };
    let n = shapes.len() as i32;
    let (edge_top, first, facing_out) = match edge {
        Edge::Shaped => (TOP - n, TOP - n + 1, true),
        Edge::PlacedUphill => (TOP, TOP + 1, false),
        Edge::PlacedDownhill => (TOP, TOP + 1, true),
    };
    let d = if facing_out { out } else { [-out[0], -out[1]] };
    let facing = FACINGS.iter().find(|(_, v)| *v == d).expect("a facing").0;
    for b in -4..=4 {
        for a in -6..=0 {
            let top = if a == 0 { edge_top } else { TOP };
            for y in 0..=top {
                set(&mut w, at(out, a, b, y), stone);
            }
        }
        for (i, shape) in shapes.iter().enumerate() {
            let m = s(&format!(
                "{block}[facing={facing},flooded=false,half=bottom,shape={shape}]"
            ));
            set(&mut w, at(out, 0, b, first + i as i32), m);
        }
    }
    w
}

/// The first solid point along the ray within `max` m: its distance and cell, by marching it in
/// 0.25 mm steps through the cells' solids (the cells' own shapes, as the world holds them).
fn oracle(w: &mut VoxelWorld, o: [f64; 3], d: [f64; 3], max: f64) -> Option<(f64, [i32; 3])> {
    let start = cell_of_pos(o);
    let step = 0.000_25;
    let mut t = 0.0;
    while t <= max {
        let p = [o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t];
        let c = cell_of_pos(p);
        if c != start {
            let m = w.get_voxel(c[0], c[1], c[2]);
            if m != 0 {
                let q = cell_space(p);
                let fx = (q[0] - f64::from(c[0])) as f32;
                let fy = (q[1] - f64::from(c[1])) as f32;
                let fz = (q[2] - f64::from(c[2])) as f32;
                if point_in_solid(shape_of(m), fx, fy, fz, 0.0) {
                    return Some((t, c));
                }
            }
        }
        t += step;
    }
    None
}

/// From 1, 2 and 3 m back from the edge (the eye 1.62 m over the land), looking out over the edge
/// and up to 30° off it, at pitches −15° to −70°: every ray the oracle finds meeting the ramp is
/// targeted on the ramp, where the oracle meets it.
fn sweep(f: FaceFrame, block: &str, edge: Edge, shapes: &[&str]) {
    for (_, out) in FACINGS {
        let mut w = cliff(f, block, out, edge, shapes);
        let ramp_rows = match edge {
            Edge::Shaped => TOP - shapes.len() as i32 + 1..=TOP,
            _ => TOP + 1..=TOP + shapes.len() as i32,
        };
        let is_ramp = |c: [i32; 3]| {
            let l = f.cell(c);
            let a = l[0] * out[0] + l[2] * out[1];
            a == 0 && ramp_rows.contains(&l[1])
        };
        let (ox, oz) = (f64::from(out[0]), f64::from(out[1]));
        let land_m = m_of_row(TOP + 1);
        let mut met = 0;
        for back in [1.0f64, 2.0, 3.0] {
            for yaw in [-30.0f64, -12.0, 0.0, 12.0, 30.0] {
                for tenth in (-700..=-150).step_by(5) {
                    let pitch = f64::from(tenth) / 10.0;
                    let (ps, pc) = pitch.to_radians().sin_cos();
                    let (ys, yc) = yaw.to_radians().sin_cos();
                    // The eye `back` m behind the edge column's land-side face, over its middle.
                    let land_face = if ox + oz > 0.0 { 0.0 } else { 1.0 };
                    let e_along = land_face - back * (ox + oz);
                    let eye_local = if ox != 0.0 {
                        [e_along, land_m + 1.62, 0.5]
                    } else {
                        [0.5, land_m + 1.62, e_along]
                    };
                    // Along `out` by pc·yc, across it by pc·ys.
                    let dir_local = [ox * pc * yc - oz * pc * ys, ps, oz * pc * yc + ox * pc * ys];
                    let eye = f.point(eye_local);
                    let dir = f.vector(dir_local);
                    let Some((t, c)) = oracle(&mut w, eye, dir, 5.0) else {
                        continue;
                    };
                    if !is_ramp(c) {
                        continue;
                    }
                    met += 1;
                    let what = format!(
                        "{block} {edge:?} {shapes:?} out {out:?}: {back} m back, yaw {yaw}°, \
                         pitch {pitch}°"
                    );
                    let hit = raycast_block(&mut w, eye, dir.map(|v| v as f32), 5.0)
                        .unwrap_or_else(|| panic!("{what}: targets nothing, the ramp at {t:.3} m"));
                    assert_eq!(hit.cell, c, "{what}: the ramp's cell at {t:.3} m");
                    assert!(
                        (f64::from(hit.distance) - t).abs() < 2e-3,
                        "{what}: met at {} m, the ramp is at {t:.3} m",
                        hit.distance
                    );
                    // And the server takes a break of what is aimed at (its sight lines reach it).
                    let req = BlockEditRequest {
                        action: BlockEditAction::Break,
                        cell: hit.cell,
                        face: hit.face,
                        material: 0,
                        rows: 1,
                        down: false,
                        point: None,
                    };
                    let o = check_block_edit(&mut w, &req, eye, &[], DEFAULT_REACH);
                    assert_eq!(
                        o.check,
                        EditCheck::Ok,
                        "{what}: the server's check (face {})",
                        hit.face
                    );
                }
            }
        }
        assert!(
            met > 50,
            "{block} {edge:?} {shapes:?} out {out:?}: only {met} rays meet it"
        );
    }
}

dwell_world::both_faces!(
    /// A steep (45°) ramp at a cliff edge, red sandstone as in the report (pitches −35° to −45°
    /// from 2 m back met nothing): shaped into the edge, and placed on it either way round.
    fn a_steep_ramp_at_a_cliff_edge_is_targeted_on_its_slope(f: FaceFrame) {
        for edge in [Edge::Shaped, Edge::PlacedUphill, Edge::PlacedDownhill] {
            sweep(
                f,
                "dwell:red_sandstone_ramp",
                edge,
                &["steep_lower", "steep_upper"],
            );
        }
    }
);

dwell_world::both_faces!(
    /// The ½ m ramp at a cliff edge too.
    fn a_half_ramp_at_a_cliff_edge_is_targeted_on_its_slope(f: FaceFrame) {
        for edge in [Edge::Shaped, Edge::PlacedUphill, Edge::PlacedDownhill] {
            sweep(f, "dwell:red_sandstone_ramp", edge, &["ramp"]);
        }
    }
);
