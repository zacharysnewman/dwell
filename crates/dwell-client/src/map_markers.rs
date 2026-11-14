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
/// face-local, y the top of the rim beside the opening: the opening's height the generator gives
/// (`world.portals`, the shaft's `top`, known from the seed wherever the player is: BUGS.md #153,
/// a player flying hundreds of metres up read no ground and the marker took their own height),
/// else the loaded rim's top, else NaN (the shell takes the map's height there, or waits).
pub fn seen_portals(world: &VoxelWorld, axes: &[[f64; 2]], pos: [f64; 3]) -> Vec<[f64; 4]> {
    let f = FaceFrame::at_height(pos[1]);
    let eye_y = f.point(pos)[1];
    let face = if f.is_b() {
        dwell_world::Face::B
    } else {
        dwell_world::Face::A
    };
    let rules = dwell_worldgen::portals::MIDPLANE.portals;
    // The rim's middle, east of the axis (the ladder's wall side is the rim's too; any side is).
    let rim = f64::from(rules.radius + rules.rim_width / 2.0);
    let far = seen_m();
    let mut out = Vec::new();
    for &[x, z] in axes {
        if (x - pos[0]).hypot(z - pos[2]) > far {
            continue;
        }
        let y = match world.portals.at(x, z) {
            Some(shaft) => shaft.top(face),
            None => ground(world, f, x + rim, z, eye_y).unwrap_or(f64::NAN),
        };
        out.push([f64::from(u8::from(f.is_b())), x, y, z]);
    }
    out
}

/// The map's town sites with their state (docs/BLUEPRINTS.md §6), flat [x, z, state]*: each
/// site's centre (face-local m, the column's middle) and 1 while a blueprint of the world's (`boxes`,
/// each [min x, min z, max x, max z] world cells on the site's face; x and z are the same on both
/// faces) lies within `reach` m of it across the ground (it was founded and is being built), else 0
/// (not founded yet, or built: a finished blueprint disappears).
pub fn town_states(
    sites: &[dwell_world::blueprint::TownSite],
    boxes: &[[i32; 4]],
    reach: i32,
) -> Vec<f64> {
    let mut out = Vec::with_capacity(sites.len() * 3);
    for t in sites {
        let building = boxes.iter().any(|&[x0, z0, x1, z1]| {
            t.x >= x0 - reach && t.x <= x1 + reach && t.z >= z0 - reach && t.z <= z1 + reach
        });
        out.extend([
            f64::from(t.x) + 0.5,
            f64::from(t.z) + 0.5,
            f64::from(u8::from(building)),
        ]);
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
        fn a_portal_seen_from_far_overhead_stands_at_its_openings_height(f: FaceFrame) {
            // BUGS.md #153: 400 m over a portal whose ground isn't loaded, the marker took the
            // player's height. The opening's height comes from the generator's shafts.
            let mut w = VoxelWorld::new(empty_generator(), None);
            let top = m_of_row(GROUND + 1);
            let mut tops = [0.0; 2];
            tops[usize::from(f.is_b())] = top;
            w.portals = dwell_world::portal_shafts::PortalShafts::new(
                vec![dwell_world::portal_shafts::Shaft {
                    axis: [10.5, 0.5],
                    radius: 2.0,
                    top: tops,
                }],
                Default::default(),
            );
            let pos = f.point([10.5, m_of_row(GROUND) + 400.0, 0.5]);
            let seen = seen_portals(&w, &[[10.5, 0.5]], pos);
            let face = f64::from(u8::from(f.is_b()));
            assert_eq!(
                seen,
                vec![[face, 10.5, top, 0.5]],
                "at the opening, not the eye"
            );
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

    both_faces!(
        fn a_town_site_reads_building_while_a_blueprint_lies_in_its_footprint(f: FaceFrame) {
            let site = |x, z| dwell_world::blueprint::TownSite {
                face: f,
                cell: [0, 0],
                x,
                z,
                plan: 0,
            };
            let sites = [site(100, 200), site(-500, 40)];
            // A hut's box 9 m west of the first site's centre; none near the second.
            let boxes = [[88, 196, 94, 202]];
            assert_eq!(
                town_states(&sites, &boxes, 32),
                vec![100.5, 200.5, 1.0, -499.5, 40.5, 0.0]
            );
            assert_eq!(
                town_states(&sites, &[], 32),
                vec![100.5, 200.5, 0.0, -499.5, 40.5, 0.0]
            );
        }
    );
}
