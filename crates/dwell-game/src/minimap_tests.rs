use super::*;
use dwell_world::{FLAT_TOP_Y, empty_generator, m_of_row, row_of_m};

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
        // Off: the ground's own colour everywhere; on: its block colour tinted by height for the
        // hill's top and the floor's (the big map's look).
        let frame = MapFrame::new(f, 0.5, 0.5, 0.0);
        let s = frame.to_screen([15.5, 0.5]);
        assert_eq!(pixel(&plain.image, &t, s[0], s[1]), GREEN);
        assert_eq!(pixel(&plain.image, &t, -s[0], s[1]), GREEN);
        let hill = pixel(&shaded.image, &t, s[0], s[1]);
        let flat = pixel(&shaded.image, &t, -s[0], s[1]);
        assert_eq!(hill, tint_rgb(&t, GREEN, m_of_row(GROUND + 12), false));
        assert_eq!(flat, tint_rgb(&t, GREEN, m_of_row(GROUND), false));
        assert!(
            lum(hill) > lum(flat),
            "6 m higher is a little brighter: {hill:06x} vs {flat:06x}"
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
    let t = std::hint::black_box(MAP);
    // The zoom levels (owner, 2026-11-09): at most five, the closest the minimap's area at 1 m a
    // cell, the widest the whole world disc (its sea too), geometric between, each level coarser.
    let levels = t.big_map_levels;
    assert!((2..=5).contains(&levels.len()));
    assert_eq!(levels[0], (1.0, 2.0 * t.minimap_radius_m));
    assert_eq!(
        levels[levels.len() - 1].1,
        2.0 * dwell_protocol::world::WORLD_RADIUS as f64
    );
    for w in levels.windows(2) {
        assert!(w[1].0 > w[0].0 && w[1].1 > w[0].1);
    }
    assert!((t.big_map_start_level as usize) < levels.len());
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
    let grid = scan_tops(&w, f, f.point(pos), &t, &colour_of, None, false);
    let scan = start.elapsed().as_secs_f64() * 1e3;
    let start = std::time::Instant::now();
    let mut image = Vec::new();
    render_disc(
        &grid,
        &MapFrame::new(f, 0.5, 0.5, 30.0),
        &t,
        Minimap::pixels(&t),
        true,
        &|_, _| None,
        &mut image,
    );
    let draw = start.elapsed().as_secs_f64() * 1e3;
    let start = std::time::Instant::now();
    let p = f.point([1.5, m_of_row(GROUND) + 1.6, 0.5]);
    scan_tops(&w, f, p, &t, &colour_of, Some(&grid), false);
    let hinted = start.elapsed().as_secs_f64() * 1e3;
    let start = std::time::Instant::now();
    scan_tops(&w, f, p, &t, &colour_of, Some(&grid), true);
    let strip = start.elapsed().as_secs_f64() * 1e3;
    println!(
        "minimap: scan {scan:.2} ms, from the last tops {hinted:.2} ms, a move's strip {strip:.2} ms, draw {draw:.2} ms (native, this build's profile)"
    );
}

dwell_world::both_faces!(
    fn a_hill_over_the_scans_top_shows_its_surface_not_its_inside(f: FaceFrame) {
        // 0.50.37's browser check: hills higher than `scanUpM` over the eye drew as their stone.
        let t = MAP;
        let mut w = floor(f, 30);
        let up = dwell_world::row_of_m(t.minimap_scan_up_m) + 30;
        for x in 10..14 {
            for z in 0..4 {
                for r in GROUND..GROUND + up {
                    let [x, y, z] = f.cell([x, r, z]);
                    w.set_voxel(x, y, z, stone());
                }
                let [x, y, z] = f.cell([x, GROUND + up, z]);
                w.set_voxel(x, y, z, sand());
            }
        }
        let grid = scan_tops(
            &w,
            f,
            [0.5, m_of_row(GROUND) + 1.6, 0.5],
            &t,
            &colour_of,
            None,
            false,
        );
        let i = grid.index(11.5, 1.5).expect("in");
        assert_eq!(grid.colour[i], RED, "the hill's top");
        assert!((f64::from(grid.height[i]) - m_of_row(GROUND + up + 1)).abs() < 1e-6);
    }
);

dwell_world::both_faces!(
    fn a_read_from_the_last_reads_tops_matches_a_whole_read(f: FaceFrame) {
        // Hills, a pit and a tall pillar (past the hint, from below) between two reads 3 m apart.
        let t = MAP;
        let mut w = floor(f, 80);
        let y = m_of_row(GROUND) + 1.6;
        let first = scan_tops(&w, f, [0.5, y, 0.5], &t, &colour_of, None, false);
        for x in -30i32..30 {
            for z in -30..30 {
                let rows = ((x * 7 + z * 13).rem_euclid(9)) - 2;
                for r in GROUND - 1..GROUND - 1 + rows.max(0) {
                    let [x, y, z] = f.cell([x, r, z]);
                    w.set_voxel(x, y, z, if (x + z) % 3 == 0 { dirt() } else { stone() });
                }
                if rows < 0 {
                    let [x, y, z] = f.cell([x, GROUND - 1, z]);
                    w.set_voxel(x, y, z, M.air);
                }
            }
        }
        for r in GROUND..GROUND + 60 {
            let [x, y, z] = f.cell([5, r, 5]);
            w.set_voxel(x, y, z, sand());
        }
        let hinted = scan_tops(&w, f, [3.5, y, 0.5], &t, &colour_of, Some(&first), false);
        let whole = scan_tops(&w, f, [3.5, y, 0.5], &t, &colour_of, None, false);
        // A move's read keeps the columns it had and reads the strip it uncovers: on an
        // unchanged world, the same as a whole read.
        let here = scan_tops(&w, f, [0.5, y, 0.5], &t, &colour_of, None, false);
        let moved = scan_tops(&w, f, [3.5, y, -2.5], &t, &colour_of, Some(&here), true);
        let fresh = scan_tops(&w, f, [3.5, y, -2.5], &t, &colour_of, None, false);
        assert_eq!(moved.colour, fresh.colour);
        assert_eq!(hinted.colour, whole.colour);
        assert_eq!(
            hinted
                .height
                .iter()
                .map(|h| h.to_bits())
                .collect::<Vec<_>>(),
            whole.height.iter().map(|h| h.to_bits()).collect::<Vec<_>>()
        );
    }
);

/// The cells whose centres are within the minimap's radius of face-local (x, z): what a player
/// there has explored, a 2D distance alone (owner, 2026-11-09).
fn cells_within(t: &MapTunables, x: f64, z: f64) -> usize {
    let (cell, r) = (t.explored_cell_m, t.minimap_radius_m);
    let n = (r / cell).ceil() as i32 + 1;
    let (ci, ck) = ((x / cell).floor() as i32, (z / cell).floor() as i32);
    let mut count = 0;
    for k in ck - n..=ck + n {
        for i in ci - n..=ci + n {
            let (cx, cz) = ((f64::from(i) + 0.5) * cell, (f64::from(k) + 0.5) * cell);
            if (cx - x).powi(2) + (cz - z).powi(2) <= r * r {
                count += 1;
            }
        }
    }
    count
}

dwell_world::both_faces!(
    fn flying_high_over_a_valley_explores_it_all(f: FaceFrame) {
        // The owner's report (2026-11-09): flying over a valley left it unexplored. The ground is
        // past the scan's reach below the eye (`scanDownM`), so the tops find nothing there.
        let t = MAP;
        let w = floor(f, 100);
        let y = m_of_row(GROUND) + t.minimap_scan_down_m + 40.0;
        let mut map = Minimap::default();
        map.update(&w, f.point([0.5, y, 0.5]), 0.0, 0.0, false, &t, &colour_of);
        let fresh = map.explored.take();
        assert_eq!(
            fresh.len(),
            cells_within(&t, 0.5, 0.5),
            "every cell in the radius"
        );
        let face = u8::from(f.is_b());
        assert!(fresh.iter().all(|&((fc, ..), _)| fc == face));
        // Nothing seen yet: the pending colour, until the generator or the terrain gives one.
        assert!(fresh.iter().all(|&(_, c)| c == t.explored_pending_color));
    }
);

dwell_world::both_faces!(
    fn standing_on_a_hilltop_explores_the_valley_below(f: FaceFrame) {
        // The valley floor is further below the hilltop than the scan reaches.
        let t = MAP;
        let mut w = floor(f, 100);
        let rise = row_of_m(t.minimap_scan_down_m + 20.0);
        for x in -2..=2 {
            for z in -2..=2 {
                for up in 0..rise {
                    let [x, y, z] = f.cell([x, GROUND + up, z]);
                    w.set_voxel(x, y, z, sand());
                }
            }
        }
        let y = m_of_row(GROUND + rise) + 1.6;
        let mut map = Minimap::default();
        map.update(&w, f.point([0.5, y, 0.5]), 0.0, 0.0, false, &t, &colour_of);
        let fresh = map.explored.take();
        assert_eq!(fresh.len(), cells_within(&t, 0.5, 0.5), "the valley too");
        // The hilltop's own cells are its sand, seen; the valley's wait for a colour.
        let at = |i: i32, k: i32| {
            fresh
                .iter()
                .find(|&&((_, a, b), _)| (a, b) == (i, k))
                .map(|&(_, c)| c)
        };
        assert_eq!(at(0, 0), Some(RED));
        assert_eq!(at(5, 0), Some(t.explored_pending_color));
    }
);

/// A generated tile all of colour `c` (`revealed.tileCells`² RGBA texels).
fn tile_all(t: &MapTunables, c: u32) -> Vec<u8> {
    let n = (t.revealed_tile_cells as usize).pow(2);
    [(c >> 16) as u8, (c >> 8) as u8, c as u8, 255].repeat(n)
}

dwell_world::both_faces!(
    fn explored_cells_take_the_generators_colour_then_the_loaded_terrains(f: FaceFrame) {
        let t = MAP;
        let w = floor(f, 100);
        let high = m_of_row(GROUND) + t.minimap_scan_down_m + 40.0;
        let mut map = Minimap::default();
        map.update(
            &w,
            f.point([0.5, high, 0.5]),
            0.0,
            0.0,
            false,
            &t,
            &colour_of,
        );
        assert_eq!(map.explored.take().len(), cells_within(&t, 0.5, 0.5));
        // The pending cells' tiles are asked for, at most `jobsInFlight` at once, each once.
        let face = u8::from(f.is_b());
        let mut given = 0;
        loop {
            let asked = map.explored.ask(&t);
            if asked.is_empty() {
                break;
            }
            assert!(asked.len() <= t.revealed_jobs_in_flight as usize);
            assert!(
                map.explored.ask(&t).is_empty(),
                "none more while those are in flight"
            );
            for tk in asked {
                assert_eq!(tk.0, face);
                map.explored.fill(tk, Some(tile_all(&t, BLUE)), &t);
                given += 1;
            }
        }
        // The disc round the origin spans four tiles.
        assert_eq!(given, 4);
        let fresh = map.explored.take();
        assert_eq!(
            fresh.len(),
            cells_within(&t, 0.5, 0.5),
            "every cell coloured again"
        );
        assert!(fresh.iter().all(|&(_, c)| c == BLUE), "from the generator");
        // The disc shows the valley it cannot reach in the generator's 1 m colour (red here),
        // not a gap and not the explored cells' 8 m one.
        for key in map.generated.wants(face, 0.5, 0.5, &t) {
            map.generated.fill(key, tile_all(&t, RED));
        }
        map.update(
            &w,
            f.point([0.5, high, 0.5]),
            30.0,
            0.5,
            false,
            &t,
            &colour_of,
        );
        assert_eq!(pixel(&map.image, &t, 20.0, 20.0), RED);
        // Down in the valley the loaded terrain's tops replace it (at the next whole read):
        // reported again, green.
        let low = m_of_row(GROUND) + 1.6;
        map.update(
            &w,
            f.point([0.5, low, 0.5]),
            30.0,
            2.5,
            false,
            &t,
            &colour_of,
        );
        let fresh = map.explored.take();
        assert_eq!(fresh.len(), cells_within(&t, 0.5, 0.5));
        assert!(fresh.iter().all(|&(_, c)| c == GREEN));
        // Up again: the terrain's colour stays (the generator's never replaces it).
        map.update(
            &w,
            f.point([0.5, high, 0.5]),
            30.0,
            5.0,
            false,
            &t,
            &colour_of,
        );
        assert!(map.explored.take().is_empty());
        assert!(map.explored.ask(&t).is_empty());
    }
);

dwell_world::both_faces!(
    fn without_a_terrain_to_generate_from_cells_stay_pending_until_seen(f: FaceFrame) {
        let t = MAP;
        let w = floor(f, 100);
        let high = m_of_row(GROUND) + t.minimap_scan_down_m + 40.0;
        let mut map = Minimap::default();
        map.update(
            &w,
            f.point([0.5, high, 0.5]),
            0.0,
            0.0,
            false,
            &t,
            &colour_of,
        );
        map.explored.take();
        loop {
            let asked = map.explored.ask(&t);
            if asked.is_empty() {
                break;
            }
            for tk in asked {
                map.explored.fill(tk, None, &t);
            }
        }
        assert!(map.explored.take().is_empty(), "nothing new to report");
        // Moving on over the same tiles asks for none again.
        map.update(
            &w,
            f.point([8.5, high, 0.5]),
            0.0,
            0.1,
            false,
            &t,
            &colour_of,
        );
        assert!(!map.explored.take().is_empty(), "the new cells ahead");
        assert!(map.explored.ask(&t).is_empty());
        // The disc keeps its gap over ground nothing has coloured.
        assert_eq!(pixel(&map.image, &t, 20.0, 20.0), 0);
    }
);

// The maps' look (owner, 2026-11-09, after the height bands of 0.50.53 hid the sand: block
// colours "lightly tinted by height").

/// A colour's brightness (the channels' sum).
fn lum(c: u32) -> u32 {
    ((c >> 16) & 0xff) + ((c >> 8) & 0xff) + (c & 0xff)
}

const SAND: u32 = 0xd8_c27a;

#[test]
fn a_sand_cell_stays_sand_coloured_and_height_changes_its_brightness_gently() {
    // face-a-only: heights are face-local and the tint reads them alone (the disc's both-face
    // test goes through FaceFrame).
    let t = MAP;
    let sea = dwell_world::SEA_LEVEL_M;
    let ch = |c: u32, sh: u32| f64::from((c >> sh) & 0xff);
    let mut last = 0;
    for h in [-50.0, 0.0, 100.0, 300.0, 600.0, 2000.0] {
        let c = tint_rgb(&t, SAND, sea + h, false);
        // Its hue kept: each channel the same factor of sand's, within rounding.
        let k = ch(c, 16) / ch(SAND, 16);
        for sh in [8, 0] {
            assert!(
                (ch(c, sh) - ch(SAND, sh) * k).abs() <= 1.5,
                "{h} m: {c:06x} is sand {k}×"
            );
        }
        // Gently: never past the data's darkest and its high ground's brightest.
        assert!(k >= t.height_tint_darkest - 0.01 && k <= t.height_tint_peak_brightest + 0.01);
        // Brighter, never darker, going up.
        assert!(lum(c) >= last, "{h} m");
        last = lum(c);
    }
    // Held past the ends; sea-level sand the darkest, high sand the brightest.
    assert_eq!(
        tint_rgb(&t, SAND, sea - 50.0, false),
        scale_rgb(SAND, t.height_tint_darkest)
    );
    assert_eq!(
        tint_rgb(&t, SAND, sea + t.height_tint_high_m, false),
        scale_rgb(SAND, t.height_tint_brightest)
    );
    assert_eq!(
        tint_rgb(&t, SAND, sea + t.height_tint_peak_m + 500.0, false),
        scale_rgb(SAND, t.height_tint_peak_brightest)
    );
    // A few metres make little difference (no bands): 8 m changes no channel by more than 4.
    let (a, b) = (
        tint_rgb(&t, SAND, sea + 300.0, false),
        tint_rgb(&t, SAND, sea + 308.0, false),
    );
    for sh in [16, 8, 0] {
        assert!((ch(a, sh) - ch(b, sh)).abs() <= 4.0);
    }
}

#[test]
fn high_ground_keeps_a_gradient_past_400_m() {
    // face-a-only: the tint reads a colour and a height alone.
    // 0.52.27: a 4800 m peak's slopes were one flat brightness above 400 m; the ramp goes on to
    // peakM, and below highM nothing changed.
    let t = MAP;
    let sea = dwell_world::SEA_LEVEL_M;
    let grey = 0x60_6060;
    let at = |m: f64| tint_rgb(&t, grey, sea + m, false);
    assert!(at(1000.0) > at(400.0) && at(3000.0) > at(1000.0) && at(4800.0) > at(3000.0));
    // The default world's heights (under highM) as before: the low ramp alone.
    let k = t.height_tint_darkest + (t.height_tint_brightest - t.height_tint_darkest) * 0.5;
    assert_eq!(at(t.height_tint_high_m * 0.5), scale_rgb(grey, k));
}

#[test]
fn water_keeps_its_colour() {
    // face-a-only: the tint reads a colour and a height alone.
    let sea = dwell_world::SEA_LEVEL_M;
    for h in [sea - 20.0, sea, sea + 250.0] {
        assert_eq!(tint_rgb(&MAP, 0x3f_8bc2, h, true), 0x3f_8bc2);
    }
}

#[test]
fn the_tints_strength_is_data() {
    // face-a-only: data, no face.
    let t = MapTunables {
        height_tint_low_m: 0.0,
        height_tint_high_m: 100.0,
        height_tint_darkest: 0.5,
        height_tint_brightest: 1.0,
        ..MAP
    };
    let sea = dwell_world::SEA_LEVEL_M;
    assert_eq!(tint_rgb(&t, 0x80_8080, sea, false), 0x40_4040);
    assert_eq!(tint_rgb(&t, 0x80_8080, sea + 50.0, false), 0x60_6060);
    assert_eq!(tint_rgb(&t, 0x80_8080, sea + 100.0, false), 0x80_8080);
    // No strength: the block colours as they are.
    let none = MapTunables {
        height_tint_darkest: 1.0,
        height_tint_brightest: 1.0,
        ..MAP
    };
    assert_eq!(tint_rgb(&none, SAND, sea + 321.0, false), SAND);
}

#[test]
fn height_codes_round_trip_and_tiles_carry_them() {
    for (h, w) in [
        (0.0, false),
        (12.5, true),
        (-3.5, false),
        (-0.5, true),
        (400.0, false),
    ] {
        assert_eq!(decode_height(height_code(h, w)), Some((h, w)));
    }
    assert_eq!(decode_height(NO_HEIGHT), None);
    // A tile's cell: its RGBA, then its i16 code after all the colours.
    let mut tile = vec![1, 2, 3, 255, 0, 0, 0, 0];
    tile.extend_from_slice(&(height_code(20.0, true) as i16).to_le_bytes());
    tile.extend_from_slice(&i16::MIN.to_le_bytes());
    assert_eq!(
        tile_cell(&tile, 2, 0),
        Some((0x010203, height_code(20.0, true)))
    );
    assert_eq!(tile_cell(&tile, 2, 1), None, "no ground");
    // A tile without heights (an older payload) has none.
    assert_eq!(tile_cell(&tile[..8], 2, 0), Some((0x010203, NO_HEIGHT)));
}

#[test]
fn the_big_maps_grid_has_no_lighting_a_slope_and_a_flat_at_one_height_match() {
    // face-a-only: the grid is the shell's screen-order cells, face-free.
    // A 3 × 1 grid (with its border): once rising to the right, once flat, the middle cell at the
    // same height in both: the same colour (no hill-shade), its own colour tinted by its height.
    // The empty cell stays transparent; a cell with no height is its own colour.
    let t = MAP;
    let (w, h) = (3, 1);
    let grey = 0x80_8080;
    let mid = dwell_world::SEA_LEVEL_M + 300.0;
    let grid = |rise: f64, colour: u32| {
        let mut colours = vec![colour; (w + 2) * (h + 2)];
        let mut codes = vec![NO_HEIGHT; (w + 2) * (h + 2)];
        for r in 0..h + 2 {
            for c in 0..w + 2 {
                codes[r * (w + 2) + c] = height_code(mid + rise * (c as f64 - 2.0) * 4.0, false);
            }
        }
        colours[w + 2 + 3] = 0xff00_0000;
        codes[w + 2 + 1] = NO_HEIGHT;
        shade_grid(&t, &colours, &codes, w, h)
    };
    let slope = grid(1.0, grey);
    let flat = grid(0.0, grey);
    let sand = grid(0.0, SAND);
    let rgb = |p: &[u8]| (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]);
    assert_eq!(rgb(&slope[4..8]), tint_rgb(&t, grey, mid, false));
    assert_eq!(rgb(&flat[4..8]), rgb(&slope[4..8]));
    assert_eq!(
        rgb(&sand[4..8]),
        tint_rgb(&t, SAND, mid, false),
        "sand stays sand"
    );
    assert_eq!(slope[11], 0, "nothing there: transparent");
    assert_eq!(
        &slope[0..4],
        &[0x80, 0x80, 0x80, 255],
        "no height: its own colour"
    );
}

dwell_world::both_faces!(
    fn explored_cells_carry_the_terrains_heights(f: FaceFrame) {
        let t = MAP;
        let w = floor(f, 100);
        let y = m_of_row(GROUND) + 1.6;
        let mut map = Minimap::default();
        map.update(&w, f.point([0.5, y, 0.5]), 0.0, 0.0, false, &t, &colour_of);
        let fresh = map.explored.take();
        assert!(!fresh.is_empty());
        for (key, _) in fresh {
            let (hm, water) = decode_height(map.explored.height_of(key)).expect("a height");
            assert_eq!(hm, m_of_row(GROUND), "the floor's top, face-local");
            assert!(!water);
        }
    }
);

dwell_world::both_faces!(
    fn explored_cells_take_the_generated_tiles_heights(f: FaceFrame) {
        let t = MAP;
        let w = floor(f, 10);
        let high = m_of_row(GROUND) + t.minimap_scan_down_m + 40.0;
        let mut map = Minimap::default();
        map.update(
            &w,
            f.point([0.5, high, 0.5]),
            0.0,
            0.0,
            false,
            &t,
            &colour_of,
        );
        map.explored.take();
        let n = (t.revealed_tile_cells as usize).pow(2);
        let mut tile = tile_all(&t, BLUE);
        for _ in 0..n {
            tile.extend_from_slice(&(height_code(30.0, false) as i16).to_le_bytes());
        }
        loop {
            let asked = map.explored.ask(&t);
            if asked.is_empty() {
                break;
            }
            for tk in asked {
                map.explored.fill(tk, Some(tile.clone()), &t);
            }
        }
        let fresh = map.explored.take();
        assert!(!fresh.is_empty());
        for (key, c) in fresh {
            assert_eq!(c, BLUE);
            assert_eq!(map.explored.height_of(key), height_code(30.0, false));
        }
    }
);

/// A generated 1 m tile (`revealed.tileCells`² cells) whose every cell's colour names its own
/// face-local cell (i, k): red its i, green its k (each & 0xff), blue 0x80; no heights.
fn tile_named(t: &MapTunables, ti: i32, tk: i32) -> Vec<u8> {
    let n = t.revealed_tile_cells as i32;
    let mut out = Vec::new();
    for j in 0..n * n {
        let (i, k) = (ti * n + j % n, tk * n + j / n);
        out.extend([(i & 0xff) as u8, (k & 0xff) as u8, 0x80, 255]);
    }
    out
}

dwell_world::both_faces!(
    fn the_disc_shows_unloaded_ground_at_one_metre_never_coarse_or_black(f: FaceFrame) {
        // The owner's report (2026-11-10): sprinting left the minimap "fragmented" with "black
        // edges" and "a coarse view sometimes". Only 20 m round the player is loaded: the rest of
        // the disc is the generator's 1 m tops, each pixel its own metre's, none transparent and
        // none an 8 m explored cell's.
        let t = MAP;
        assert_eq!(Generated::cell_m(&t), 1.0, "the closest level is 1 m cells");
        let w = floor(f, 20);
        let (px, pz, yaw) = (3.5, -2.5, 37.0);
        let pos = f.point([px, m_of_row(GROUND) + 1.6, pz]);
        let mut map = Minimap::default();
        map.update(&w, pos, yaw, 0.0, false, &t, &colour_of);
        // The explored cells' generated colours are there too (blue), and must not show.
        let face = u8::from(f.is_b());
        loop {
            let asked = map.explored.ask(&t);
            if asked.is_empty() {
                break;
            }
            for tk in asked {
                map.explored.fill(tk, Some(tile_all(&t, BLUE)), &t);
            }
        }
        let wants = map.generated.wants(face, px, pz, &t);
        assert!(!wants.is_empty());
        // Nearest first, and every tile the disc touches.
        assert_eq!(wants[0], (face, 0, -1));
        for key in wants {
            map.generated.fill(key, tile_named(&t, key.1, key.2));
        }
        assert!(map.generated.wants(face, px, pz, &t).is_empty(), "all held");
        let up = map.update(&w, pos, yaw, 0.1, false, &t, &colour_of);
        assert!(up.drawn, "a tile's arrival draws the disc again");
        let n = Minimap::pixels(&t);
        let per = 2.0 * t.minimap_radius_m / n as f64;
        let half = n as f64 / 2.0;
        let frame = MapFrame::new(f, px, pz, yaw);
        let (mut loaded, mut generated) = (0, 0);
        for qy in 0..n {
            for qx in 0..n {
                let sx = (qx as f64 + 0.5 - half) * per;
                let sy = (half - (qy as f64 + 0.5)) * per;
                if sx * sx + sy * sy > (t.minimap_radius_m - 1.0).powi(2) {
                    continue;
                }
                let o = (qy * n + qx) * 4;
                let p = &map.image[o..o + 4];
                assert_eq!(p[3], 255, "pixel ({qx}, {qy}) inside the disc is drawn");
                let c = (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]);
                assert_ne!(c, BLUE, "no explored cell's colour");
                let [lx, lz] = frame.to_local([sx, sy]);
                let (i, k) = (lx.floor() as i32, lz.floor() as i32);
                if i.abs() <= 19 && k.abs() <= 19 {
                    assert_eq!(c, GREEN, "loaded ground is its blocks'");
                    loaded += 1;
                } else if i.abs() > 21 || k.abs() > 21 {
                    let want = (((i & 0xff) as u32) << 16) | (((k & 0xff) as u32) << 8) | 0x80;
                    assert_eq!(
                        c, want,
                        "pixel ({qx}, {qy}) is cell ({i}, {k})'s own 1 m top"
                    );
                    generated += 1;
                }
            }
        }
        assert!(loaded > 1000 && generated > 5000, "{loaded} {generated}");
        // Far tiles are let go.
        map.generated.wants(face, px + 1000.0, pz, &t);
        assert!(!map.generated.has((face, 0, -1)));
    }
);
