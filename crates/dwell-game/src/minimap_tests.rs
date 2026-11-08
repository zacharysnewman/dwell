use super::*;
use dwell_world::{FLAT_TOP_Y, empty_generator, m_of_row};

const GROUND: i32 = FLAT_TOP_Y + 20;
const GREEN: u32 = 0x40a040;
const RED: u32 = 0xd02020;
const BLUE: u32 = 0x2040d0;

fn stone() -> StateId {
    dwell_blocks::parse_state("dwell:stone").expect("stone")
}
fn sand() -> StateId {
    dwell_blocks::parse_state("dwell:sand").expect("sand")
}
fn dirt() -> StateId {
    dwell_blocks::parse_state("dwell:dirt").expect("dirt")
}

/// Stone green, sand red, dirt blue; air nothing.
fn colour_of(m: StateId) -> Option<u32> {
    if m == stone() {
        Some(GREEN)
    } else if m == sand() {
        Some(RED)
    } else if m == dirt() {
        Some(BLUE)
    } else {
        None
    }
}

/// A flat stone floor at face-local row GROUND − 1 on face `f`, within `r` m of the origin.
fn floor(f: FaceFrame, r: i32) -> VoxelWorld {
    let mut w = VoxelWorld::new(empty_generator(), None);
    for x in -r..=r {
        for z in -r..=r {
            let [x, y, z] = f.cell([x, GROUND - 1, z]);
            w.set_voxel(x, y, z, stone());
        }
    }
    w
}

fn put(w: &mut VoxelWorld, f: FaceFrame, x: f64, z: f64, m: StateId) {
    let [x, y, z] = f.cell([x.floor() as i32, GROUND - 1, z.floor() as i32]);
    w.set_voxel(x, y, z, m);
}

/// [`put`] over the 3 × 3 cells round (x, z): a mark a pixel's rounding can't miss.
fn patch(w: &mut VoxelWorld, f: FaceFrame, x: f64, z: f64, m: StateId) {
    for dx in -1..=1 {
        for dz in -1..=1 {
            put(w, f, x + f64::from(dx), z + f64::from(dz), m);
        }
    }
}

/// The disc's pixel at metres (right, up) of the player.
fn pixel(image: &[u8], t: &MapTunables, sx: f64, sy: f64) -> u32 {
    let px = Minimap::pixels(t);
    let per = 2.0 * t.minimap_radius_m / px as f64;
    let qx = (px as f64 / 2.0 + sx / per).floor() as usize;
    let qy = (px as f64 / 2.0 - sy / per).floor() as usize;
    let o = (qy * px + qx) * 4;
    (u32::from(image[o]) << 16) | (u32::from(image[o + 1]) << 8) | u32::from(image[o + 2])
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

dwell_world::both_faces!(
    fn the_disc_turns_with_the_player_their_heading_up_and_their_right_right(f: FaceFrame) {
        let t = MAP;
        for yaw in [0.0, 37.0, 90.0, 200.0, 311.0] {
            let mut w = floor(f, 40);
            let (px, pz) = (3.5, -2.5);
            // Ahead: along the look's heading, in the world (FaceFrame::look). Right: the camera's
            // right, the look × the face's up in the world, an independent derivation of the
            // player's right on a mirrored face.
            let ahead = f.look(yaw, 0.0);
            let right = cross(ahead, f.up());
            let at = |d: [f64; 3], m: f64| {
                let d = f.vector(d);
                (px + d[0] * m, pz + d[2] * m)
            };
            let (ax, az) = at(ahead, 20.0);
            patch(&mut w, f, ax, az, sand());
            let (rx, rz) = at(right, 20.0);
            patch(&mut w, f, rx, rz, dirt());
            let pos = f.point([px, m_of_row(GROUND) + 1.6, pz]);
            let mut map = Minimap::default();
            map.update(&w, pos, yaw, 0.0, false, &t, &colour_of);
            assert_eq!(
                pixel(&map.image, &t, 0.0, 20.0),
                RED,
                "yaw {yaw}: ahead is up"
            );
            assert_eq!(
                pixel(&map.image, &t, 20.0, 0.0),
                BLUE,
                "yaw {yaw}: right is right"
            );
            assert_eq!(pixel(&map.image, &t, -20.0, 0.0), GREEN, "yaw {yaw}");
            assert_eq!(pixel(&map.image, &t, 0.0, -20.0), GREEN, "yaw {yaw}");
        }
    }
);

dwell_world::both_faces!(
    fn the_disc_shows_the_players_own_face(f: FaceFrame) {
        // face B's terrain on face B: the other face's floor under the same x and z is not seen.
        let t = MAP;
        let mut w = floor(f, 10);
        let other = f.other();
        for x in 20..30 {
            let [x, y, z] = other.cell([x, GROUND - 1, 0]);
            w.set_voxel(x, y, z, sand());
        }
        let mut map = Minimap::default();
        map.update(
            &w,
            f.point([0.5, m_of_row(GROUND) + 1.6, 0.5]),
            0.0,
            0.0,
            false,
            &t,
            &colour_of,
        );
        let g = &map.grid;
        assert_eq!(g.colour[g.index(0.5, 0.5).expect("in")], GREEN);
        assert_eq!(g.colour[g.index(25.5, 0.5).expect("in")], NO_TOP);
        assert!(
            (f64::from(g.height[g.index(0.5, 0.5).expect("in")]) - m_of_row(GROUND)).abs() < 1e-6
        );
    }
);

dwell_world::both_faces!(
    fn height_shading_is_a_toggle(f: FaceFrame) {
        let t = MAP;
        let mut w = floor(f, 40);
        // A hill of stone 6 m up to the player's +x.
        for x in 10..20 {
            for z in -5..5 {
                for r in GROUND..GROUND + 12 {
                    let [x, y, z] = f.cell([x, r, z]);
                    w.set_voxel(x, y, z, stone());
                }
            }
        }
        let pos = f.point([0.5, m_of_row(GROUND) + 1.6, 0.5]);
        let mut plain = Minimap::default();
        plain.update(&w, pos, 0.0, 0.0, false, &t, &colour_of);
        let mut shaded = Minimap::default();
        shaded.update(&w, pos, 0.0, 0.0, true, &t, &colour_of);
        // Off: the ground's own colour everywhere; on: the hill brighter than the flat.
        let frame = MapFrame::new(f, 0.5, 0.5, 0.0);
        let s = frame.to_screen([15.5, 0.5]);
        assert_eq!(pixel(&plain.image, &t, s[0], s[1]), GREEN);
        assert_eq!(pixel(&plain.image, &t, -s[0], s[1]), GREEN);
        let hill = pixel(&shaded.image, &t, s[0], s[1]);
        let flat = pixel(&shaded.image, &t, -s[0], s[1]);
        assert!(
            (hill >> 8) & 0xff > (flat >> 8) & 0xff,
            "{hill:06x} vs {flat:06x}"
        );
        // Toggling redraws even standing still.
        let up = plain.update(&w, pos, 0.0, 0.1, true, &t, &colour_of);
        assert!(up.drawn && !up.scanned);
        assert_eq!(plain.image, shaded.image);
    }
);

dwell_world::both_faces!(
    fn the_disc_is_drawn_again_only_on_a_move_or_a_turn(f: FaceFrame) {
        let t = MAP;
        let w = floor(f, 70);
        let y = m_of_row(GROUND) + 1.6;
        let mut map = Minimap::default();
        let up = map.update(&w, f.point([0.5, y, 0.5]), 10.0, 0.0, false, &t, &colour_of);
        assert!(up.scanned && up.drawn);
        let small = t.minimap_turn_degrees * 0.5;
        let up = map.update(
            &w,
            f.point([0.7, y, 0.5]),
            10.0 + small,
            0.1,
            false,
            &t,
            &colour_of,
        );
        assert_eq!(up, Updated::default(), "a little move and turn: nothing");
        let up = map.update(
            &w,
            f.point([0.7, y, 0.5]),
            10.0 + t.minimap_turn_degrees * 1.5,
            0.2,
            false,
            &t,
            &colour_of,
        );
        assert!(up.drawn && !up.scanned, "a turn redraws");
        let up = map.update(&w, f.point([2.5, y, 0.5]), 13.0, 0.3, false, &t, &colour_of);
        assert!(up.drawn && up.scanned, "a move reads again");
        let up = map.update(
            &w,
            f.point([2.5, y, 0.5]),
            13.0,
            0.31 + t.minimap_refresh_seconds,
            false,
            &t,
            &colour_of,
        );
        assert!(up.scanned, "and the refresh");
    }
);

dwell_world::both_faces!(
    fn explored_cells_are_those_the_disc_has_covered_with_their_colours(f: FaceFrame) {
        let t = MAP;
        let mut w = floor(f, 200);
        let y = m_of_row(GROUND) + 1.6;
        let mut map = Minimap::default();
        map.update(&w, f.point([0.5, y, 0.5]), 0.0, 0.0, false, &t, &colour_of);
        let fresh = map.explored.take();
        let face = u8::from(f.is_b());
        let cell = t.explored_cell_m;
        let r = t.minimap_radius_m;
        assert!(!fresh.is_empty());
        for &((fc, i, k), c) in &fresh {
            assert_eq!(fc, face);
            assert_eq!(c, GREEN);
            let (cx, cz) = ((f64::from(i) + 0.5) * cell, (f64::from(k) + 0.5) * cell);
            assert!((cx - 0.5).powi(2) + (cz - 0.5).powi(2) <= r * r);
        }
        // About the disc's area in cells.
        let expect = std::f64::consts::PI * r * r / (cell * cell);
        assert!(
            (fresh.len() as f64 - expect).abs() < expect * 0.15,
            "{} cells",
            fresh.len()
        );
        assert!(map.explored.take().is_empty(), "reported once");
        // Standing still: nothing new. A cell turned red (an edit) is reported again.
        for x in 0..8 {
            for z in 0..8 {
                put(&mut w, f, f64::from(x), f64::from(z), sand());
            }
        }
        map.update(&w, f.point([0.5, y, 0.5]), 0.0, 5.0, false, &t, &colour_of);
        assert_eq!(map.explored.take(), vec![((face, 0, 0), RED)]);
        // Walking on explores the cells ahead, not those behind again.
        map.update(&w, f.point([40.5, y, 0.5]), 0.0, 6.0, false, &t, &colour_of);
        let ahead = map.explored.take();
        assert!(!ahead.is_empty());
        assert!(
            ahead
                .iter()
                .all(|&((_, i, _), _)| f64::from(i) * cell + cell > r - 1.0 - 40.0)
        );
    }
);

#[test]
fn the_map_data_is_consistent() {
    // face-a-only: tunables, no up.
    assert_eq!(MAP.minimap_radius_m % MAP.explored_cell_m, 0.0);
    assert!(MAP.big_map_min_px_per_cell <= MAP.big_map_start_px_per_cell);
    assert!(MAP.big_map_start_px_per_cell <= MAP.big_map_max_px_per_cell);
    assert_eq!(Minimap::pixels(&MAP), 128);
}

#[test]
fn angles_wrap() {
    // face-a-only: headings, no up.
    assert!((angle_between(359.0, 1.0) - 2.0).abs() < 1e-9);
    assert!((angle_between(10.0, 190.0) - 180.0).abs() < 1e-9);
}

#[test]
fn a_grid_of_a_hundred_and_twenty_eight_columns_reads_and_draws_quickly() {
    // face-a-only: a timing on face A's floor (the scan is the same code on both).
    let f = FaceFrame::A;
    let w = floor(f, 70);
    let t = MAP;
    let pos = f.point([0.5, m_of_row(GROUND) + 1.6, 0.5]);
    let start = std::time::Instant::now();
    let grid = scan_tops(&w, f, f.point(pos), &t, &colour_of);
    let scan = start.elapsed().as_secs_f64() * 1e3;
    let start = std::time::Instant::now();
    let mut image = Vec::new();
    render_disc(
        &grid,
        &MapFrame::new(f, 0.5, 0.5, 30.0),
        &t,
        Minimap::pixels(&t),
        true,
        &mut image,
    );
    let draw = start.elapsed().as_secs_f64() * 1e3;
    println!("minimap: scan {scan:.2} ms, draw {draw:.2} ms (native, this build's profile)");
}
