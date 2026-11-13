//! The map markers' reads of the world (docs/MAP_MARKERS.md): the ground a marker placed on the
//! big map stands on (the loaded column's top; the shell falls back to the map's height), and the
//! portals the player has seen (the `portal` auto kind, BIFACIAL_WORLD.md §12), each opening on the
//! player's face within the render radius, standing on its rim.

use dwell_world::{CHUNK_SIZE, FaceFrame, VoxelWorld};

/// How far across the ground (m) a portal is seen: the render radius.
pub fn seen_m() -> f64 {
    dwell_protocol::world::RENDER_RADIUS_CHUNKS as f64 * f64::from(CHUNK_SIZE)
}

/// How far over and under the eye (m) a column is read for its ground.
const SCAN_M: f64 = 256.0;

/// The face-local height (m) of the loaded ground at face-local (`x`, `z`) on face `f`, read round
/// the face-local eye height `eye_y` (`dwell_game::minimap::ground_at`). None: nothing loaded there.
pub fn ground(world: &VoxelWorld, f: FaceFrame, x: f64, z: f64, eye_y: f64) -> Option<f64> {
    dwell_game::minimap::ground_at(
        world,
        f,
        x.floor() as i32,
        z.floor() as i32,
        eye_y + SCAN_M,
        (eye_y - SCAN_M).max(0.0),
    )
}

/// The portals seen from world point `pos`: those whose axis (world x, z, the same on both faces)
/// lies within [`seen_m`] across the ground, on the player's face, as [face (0 A, 1 B), x, y, z]
/// face-local, y the top of the rim beside the opening (NaN while its rim is not loaded: the shell
/// takes the map's height there).
pub fn seen_portals(world: &VoxelWorld, axes: &[[f64; 2]], pos: [f64; 3]) -> Vec<[f64; 4]> {
    let f = FaceFrame::at_height(pos[1]);
    let eye_y = f.point(pos)[1];
    let rules = dwell_worldgen::portals::MIDPLANE.portals;
    // The rim's middle, east of the axis (the ladder's wall side is the rim's too; any side is).
    let rim = f64::from(rules.radius + rules.rim_width / 2.0);
    let far = seen_m();
    let mut out = Vec::new();
    for &[x, z] in axes {
        if (x - pos[0]).hypot(z - pos[2]) > far {
            continue;
        }
        let y = ground(world, f, x + rim, z, eye_y).unwrap_or(f64::NAN);
        out.push([f64::from(u8::from(f.is_b())), x, y, z]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_world::{both_faces, empty_generator, m_of_row};

    const GROUND: i32 = 80;

    fn stone() -> dwell_blocks::StateId {
        dwell_blocks::parse_state("dwell:stone").expect("stone")
    }

    /// A stone floor at face-local row GROUND − 1 on face `f` over x in `x0..x1`, z in −4..4.
    fn floor(w: &mut VoxelWorld, f: FaceFrame, x0: i32, x1: i32) {
        for x in x0..x1 {
            for z in -4..4 {
                let [x, y, z] = f.cell([x, GROUND - 1, z]);
                w.set_voxel(x, y, z, stone());
            }
        }
    }

    both_faces!(
        fn a_portal_is_seen_on_the_players_face_within_the_render_radius_on_its_rim(f: FaceFrame) {
            let mut w = VoxelWorld::new(empty_generator(), None);
            floor(&mut w, f, 0, 30);
            // The rim: a cell over the floor east of the axis at (10.5, 0.5).
            let [x, y, z] = f.cell([13, GROUND, 0]);
            w.set_voxel(x, y, z, stone());
            let near = [10.5, 0.5];
            let far = [seen_m() + 50.0, 0.5];
            // A portal in range whose ground isn't loaded.
            let unloaded = [-200.5, 0.5];
            let pos = f.point([2.0, m_of_row(GROUND) + 1.6, 0.0]);
            let seen = seen_portals(&w, &[near, far, unloaded], pos);
            let face = f64::from(u8::from(f.is_b()));
            assert_eq!(seen.len(), 2, "the far one is not seen: {seen:?}");
            assert_eq!(
                seen[0],
                [face, 10.5, m_of_row(GROUND + 1), 0.5],
                "on its rim"
            );
            let [uf, ux, uy, uz] = seen[1];
            assert_eq!([uf, ux, uz], [face, -200.5, 0.5]);
            assert!(uy.is_nan(), "an unloaded rim's height is unknown: {uy}");
        }
    );

    both_faces!(
        fn the_ground_is_the_loaded_columns_top_and_none_where_nothing_is_loaded(f: FaceFrame) {
            let mut w = VoxelWorld::new(empty_generator(), None);
            floor(&mut w, f, 0, 8);
            let eye = m_of_row(GROUND) + 1.6;
            assert_eq!(ground(&w, f, 3.4, -1.2, eye), Some(m_of_row(GROUND)));
            assert_eq!(ground(&w, f, 40.0, 0.0, eye), None);
        }
    );
}
