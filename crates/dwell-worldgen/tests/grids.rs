//! The world's grids and the moon's `cratered` generator (docs/proposals/MOON_WORLD.md M-1): the
//! data parses and validates, the moon's seed comes from the world's, its crater field follows the
//! data (floors under rims, rims raised, density by `perKm2`), maria are flatter, and the far-body
//! bake is deterministic per seed, differs between seeds and faces, and its normals point out of
//! the crater walls, on both faces.

use dwell_world::{
    CELL_H, CHUNK_SIZE, Chunk, ChunkCoord, Face, FaceFrame, both_faces, local_index,
};
use dwell_worldgen::grids::{
    CrateredTerrain, GRIDS_JSON, GridFrameKind, GridGenerator, bake_far, grid, grids, parse_grids,
};

fn moon(seed: u64) -> CrateredTerrain {
    CrateredTerrain::for_grid("dwell:moon", seed).expect("the build's moon grid")
}

fn other(face: Face) -> Face {
    match face {
        Face::A => Face::B,
        Face::B => Face::A,
    }
}

#[test]
fn the_build_s_grids_are_home_at_rest_and_a_cratered_moon() {
    let g = grids();
    let home = grid("dwell:home").expect("home");
    assert_eq!(home.frame, GridFrameKind::Rest);
    assert_eq!(home.radius_m, None, "the home grid is the world's own size");
    assert_eq!(home.generator, GridGenerator::Terrain { bake_size: 256 });
    assert_eq!(home.seed(42), 42, "home keeps the world seed");
    let m = grid("dwell:moon").expect("moon");
    assert_eq!(m.frame, GridFrameKind::Orbit);
    assert_eq!(
        m.radius_m,
        Some(12_500.0),
        "about a quarter of the home disc"
    );
    assert!(m.gravity_scale < 0.2, "the moon's gravity is weaker");
    assert!(m.capture_m.unwrap() > 12_500.0);
    assert_ne!(m.seed(42), 42);
    assert_ne!(m.seed(42), m.seed(43), "every world its own moon");
    assert_eq!(g.len(), 3, "home, the moon and the sun");
}

/// A bad entry fails with a message naming it.
#[test]
fn bad_grids_fail_with_a_message() {
    let cases: [(&str, &str, &str); 15] = [
        (
            "\"ramps\": \"notched\"",
            "\"ramps\": \"wavy\"",
            "generator.ramps",
        ),
        ("\"frame\": \"orbit\"", "\"frame\": \"spin\"", "`frame`"),
        ("\"dwell:moon_dust\"", "\"dwell:no_such\"", "names no block"),
        (
            "\"dwell:moon_mare_dust\"",
            "\"dwell:no_mare\"",
            "maria: `block` names no block",
        ),
        ("\"perKm2\": 30", "\"perKm2\": 0", "perKm2"),
        ("\"reach\": 2.4", "\"reach\": 0.9", "reach"),
        ("\"captureM\": 40000", "\"captureM\": 9000", "captureM"),
        (
            "\"kind\": \"cratered\"",
            "\"kind\": \"lumpy\"",
            "generator.kind",
        ),
        ("\"seedSalt\": 1297040974,", "", "seedSalt"),
        ("\"captureM\": 40000,", "", "needs a `captureM`"),
        ("\"atmosphere\": false", "\"atmosphere\": 0", "atmosphere"),
        ("\"cells\": 96", "\"cells\": 2", "nearMesh.cells"),
        ("\"sinkM\": 8", "\"sinkM\": -1", "nearMesh.sinkM"),
        ("\"levels\": 9", "\"levels\": 3", "must reach its disc"),
        ("\"cells\": 48", "\"cells\": 50", "lod.cells"),
    ];
    for (from, to, want) in cases {
        assert!(
            GRIDS_JSON.contains(from),
            "the fixture edit `{from}` applies"
        );
        let bad = GRIDS_JSON.replacen(from, to, 1);
        let err = parse_grids(&bad).expect_err(&format!("`{to}` is rejected"));
        assert!(err.contains(want), "`{err}` mentions {want}");
        assert!(err.contains("dwell:moon"), "`{err}` names the grid");
    }
}

both_faces!(
    /// A grid added by data alone generates: a second moon with its own salt and radius.
    fn a_grid_added_by_data_alone_generates(f: FaceFrame) {
        let moon_entry_at = GRIDS_JSON
            .find("    {\n      \"name\": \"dwell:moon\"")
            .unwrap();
        let end = GRIDS_JSON.rfind("\n  ]").unwrap();
        // The moon's own entry: up to the next grid's (the sun's) or the list's end.
        let entry_end = GRIDS_JSON[moon_entry_at..end]
            .find(",\n    {")
            .map_or(end, |k| moon_entry_at + k);
        let entry = GRIDS_JSON[moon_entry_at..entry_end]
            .replace("dwell:moon\"", "test:little_moon\"")
            .replace("1297040974", "7")
            .replace("\"radiusM\": 12500", "\"radiusM\": 3000")
            .replace("\"captureM\": 40000", "\"captureM\": 9000");
        let text = format!("{},\n{}{}", &GRIDS_JSON[..end], entry, &GRIDS_JSON[end..]);
        let list = parse_grids(&text).expect("valid");
        let def = list.iter().find(|g| g.name == "test:little_moon").unwrap();
        let t = CrateredTerrain::new(def, 1).unwrap();
        assert_eq!(t.radius_m(), 3000.0);
        assert!(t.top_row(f.face, 0, 0).is_some());
        assert!(t.top_row(f.face, 3100, 0).is_none(), "outside its disc");
    }
);

both_faces!(
    /// The moon's heights stay in the build volume over its whole disc: never under the data's
    /// floor (`minM`, where stacked craters would dig through the midplane), and the bake's too.
    fn the_moon_s_heights_stay_in_the_build_volume(f: FaceFrame) {
        let t = moon(5);
        let r = t.radius_m();
        let bake = t.bake_far(f.face, 128);
        assert!(
            bake.height_range[0] >= t.rules().min_m,
            "{:?}",
            bake.height_range
        );
        for i in -20..=20 {
            for j in -20..=20 {
                let (x, z) = (f64::from(i) * r / 20.0, f64::from(j) * r / 20.0);
                if !t.inside(x, z) {
                    continue;
                }
                let h = t.height(f.face, x, z);
                assert!(
                    h >= t.rules().min_m && h < dwell_world::BUILD_TOP_M - 8.0,
                    "{h} at {x}, {z}"
                );
            }
        }
    }
);

both_faces!(
    /// The big craters' floors lie under their rims, and their rims stand raised over the ground
    /// their ejecta reaches out to.
    fn crater_floors_are_under_raised_rims(f: FaceFrame) {
        for seed in [1u64, 2, 3] {
            let t = moon(seed);
            let field = &t.rules().craters;
            let mut big = t.craters_near(f.face, 0.0, 0.0, t.radius_m() * 0.8, 150.0);
            big.sort_by(|a, b| b.radius.total_cmp(&a.radius));
            assert!(big.len() >= 5, "seed {seed}: some big craters on the face");
            for c in big.iter().take(8) {
                // Heights over the ground the craters sit on (the plains, ridges and maria
                // left out), so a ridge or a basin under the ring doesn't count.
                let ring = |k: f64| {
                    (0..8)
                        .map(|a| {
                            let th = f64::from(a) * std::f64::consts::TAU / 8.0;
                            let (x, z) =
                                (c.x + k * c.radius * th.cos(), c.z + k * c.radius * th.sin());
                            t.height(f.face, x, z) - t.base_height(f.face, x, z)
                        })
                        .sum::<f64>()
                        / 8.0
                };
                let floor = t.height(f.face, c.x, c.z) - t.base_height(f.face, c.x, c.z);
                let (rim, out) = (ring(1.0), ring(field.reach));
                assert!(
                    floor < rim - c.depth * 0.5,
                    "seed {seed} {c:?}: floor {floor} rim {rim}"
                );
                assert!(
                    rim > out,
                    "seed {seed} {c:?}: rim {rim} over the surround {out}"
                );
            }
        }
    }
);

both_faces!(
    /// The crater count follows `perKm2`: four times the density, four times the craters.
    fn crater_density_is_data(f: FaceFrame) {
        let class = "{ \"radiusM\": [18, 60], \"perKm2\": 5 }";
        assert!(GRIDS_JSON.contains(class));
        let count = |per: f64| {
            let text = GRIDS_JSON.replacen(
                class,
                &format!("{{ \"radiusM\": [18, 60], \"perKm2\": {per} }}"),
                1,
            );
            let list = parse_grids(&text).unwrap();
            let t = CrateredTerrain::new(list.iter().find(|g| g.name == "dwell:moon").unwrap(), 9)
                .unwrap();
            // Centres in a 6 km square far from the field's other classes' influence: count the
            // class's craters only.
            t.craters_near(f.face, 0.0, 0.0, 6000.0, 0.0)
                .iter()
                .filter(|c| c.class == 1 && c.x.abs() < 3000.0 && c.z.abs() < 3000.0)
                .count() as f64
        };
        let (one, four) = (count(5.0), count(20.0));
        assert!(
            one > 60.0,
            "{one} craters at 5 a km² over 36 km² (fewer in maria)"
        );
        let ratio = four / one;
        assert!((3.2..4.8).contains(&ratio), "{four} / {one} = {ratio}");
    }
);

both_faces!(
    /// A mare's floor is flatter than the highlands: smaller height changes between neighbours.
    fn maria_are_flatter(f: FaceFrame) {
        let t = moon(4);
        let r = t.radius_m();
        let rough = |x: f64, z: f64| {
            (t.height(f.face, x + 30.0, z) - t.height(f.face, x, z)).abs()
                + (t.height(f.face, x, z + 30.0) - t.height(f.face, x, z)).abs()
        };
        let (mut mare, mut high) = (Vec::new(), Vec::new());
        for i in -30..=30 {
            for j in -30..=30 {
                let (x, z) = (f64::from(i) * r / 32.0, f64::from(j) * r / 32.0);
                if !t.inside(x, z) {
                    continue;
                }
                match t.mare_weight(f.face, x, z) {
                    w if w > 0.99 => mare.push(rough(x, z)),
                    w if w < 0.01 => high.push(rough(x, z)),
                    _ => {}
                }
            }
        }
        assert!(
            mare.len() > 30 && high.len() > 30,
            "{} / {}",
            mare.len(),
            high.len()
        );
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        let (m, h) = (mean(&mare), mean(&high));
        assert!(m < h * 0.7, "maria {m} vs highlands {h}");
        // And lower: the basins sit under the datum.
        let floor = t.maria_near(f.face, 0.0, 0.0, r)[0];
        assert!(t.base_height(f.face, floor.x, floor.z) < t.rules().datum_m);
    }
);

both_faces!(
    /// The bake is a pure function of (world seed, face): the same seed bakes the same bytes, other
    /// seeds and the other face bake others; its texels are RGBA at the data's size.
    fn the_bake_is_deterministic_and_differs_by_seed_and_face(f: FaceFrame) {
        let size = 64;
        let a = moon(77).bake_far(f.face, size);
        assert_eq!(a.normal_height.len(), size * size * 4);
        assert_eq!(a.albedo.len(), size * size * 4);
        assert_eq!(a, moon(77).bake_far(f.face, size), "deterministic");
        assert_ne!(
            a.normal_height,
            moon(78).bake_far(f.face, size).normal_height
        );
        assert_ne!(
            a.normal_height,
            moon(77).bake_far(other(f.face), size).normal_height
        );
        assert_eq!(
            moon(1).bake_far_default(f.face).size,
            256,
            "the data's bake size"
        );
        // The corner is outside the disc (alpha 0); the centre inside, gray and opaque.
        assert_eq!(a.normal_height[3], 0);
        let c = ((size / 2) * size + size / 2) * 4;
        assert_eq!(a.albedo[c + 3], 255);
        let [r, g, b] = [a.albedo[c], a.albedo[c + 1], a.albedo[c + 2]];
        assert!(
            r.abs_diff(g) < 12 && g.abs_diff(b) < 12,
            "gray: {r} {g} {b}"
        );
    }
);

both_faces!(
    /// On a big crater's inner wall the baked normal leans toward the crater's centre: out of the
    /// wall. (Its outer flank is too gentle at the rim's cap to beat the ridges at a texel.)
    fn baked_normals_point_out_of_crater_walls(f: FaceFrame) {
        let t = moon(3);
        let bake = bake_far(&t, f.face, 256);
        let s = bake.texel_m();
        let mut big = t.craters_near(f.face, 0.0, 0.0, t.radius_m() * 0.7, 6.0 * s);
        big.sort_by(|a, b| b.radius.total_cmp(&a.radius));
        let c = big.first().copied().expect("a crater several texels wide");
        assert!(c.radius > 4.0 * s, "{c:?}");
        let texel = |v: f64| ((v + bake.radius_m) / s).floor() as usize;
        let (mut inner, mut n) = (0, 0);
        for a in 0..16 {
            let th = f64::from(a) * std::f64::consts::TAU / 16.0;
            let (dx, dz) = (th.cos(), th.sin());
            let lean = |k: f64| {
                let (x, z) = (c.x + k * c.radius * dx, c.z + k * c.radius * dz);
                let nrm = bake.normal(texel(x), texel(z));
                nrm[0] * dx + nrm[2] * dz
            };
            n += 1;
            inner += usize::from(lean(0.75) < 0.0);
        }
        assert!(
            inner >= n * 3 / 4,
            "inner wall leans in at {inner}/{n} headings"
        );
    }
);

both_faces!(
    /// A chunk through the surface holds the ground's block up to the column's height and air over
    /// it, placed on the face's side of the midplane through its FaceFrame.
    fn a_chunk_holds_the_ground_up_to_its_height(f: FaceFrame) {
        let t = moon(8);
        let top = t.top_row(f.face, 5, 7).unwrap();
        let local = ChunkCoord::new(0, (top as i32).div_euclid(CHUNK_SIZE), 0);
        let coord = f.chunk(local);
        let mut ch = Chunk::default();
        t.generate(coord, &mut ch);
        let dust = f.state(t.surface_block(f.face, 5, 7));
        let ly_of = |row: i64| {
            let y = row as i32 - local.y * CHUNK_SIZE;
            if f.is_b() { CHUNK_SIZE - 1 - y } else { y }
        };
        assert_eq!(ch.voxels()[local_index(5, ly_of(top), 7)], dust);
        if (top + 1) as i32 - local.y * CHUNK_SIZE < CHUNK_SIZE {
            assert_eq!(
                ch.voxels()[local_index(5, ly_of(top + 1), 7)],
                dwell_world::M.air
            );
        }
        let h = t.height(f.face, 5.5, 7.5);
        assert!(((top + 1) as f64 * CELL_H - h).abs() <= CELL_H / 2.0 + 1e-9);
    }
);

both_faces!(
    /// The moon's spawn on each face is on flat ground clear of crater walls, standing on the
    /// ground, the same for the same seed and apart on the two faces.
    fn the_moon_spawn_is_flat_and_clear_of_craters(f: FaceFrame) {
        for seed in [1u64, 2, 3] {
            let t = moon(seed);
            let [x, y, z] = t.spawn_point(f.face);
            assert!(t.inside(x, z));
            assert_eq!([x, y, z], moon(seed).spawn_point(f.face), "deterministic");
            assert_ne!([x, y, z], t.spawn_point(other(f.face)), "each face its own");
            let h = t.height(f.face, x, z);
            assert!((y - h).abs() <= CELL_H, "on the ground: {y} vs {h}");
            for (dx, dz) in [(3.0, 0.0), (-3.0, 0.0), (0.0, 3.0), (0.0, -3.0)] {
                assert!((t.height(f.face, x + dx, z + dz) - h).abs() <= 1.0, "flat");
            }
            for c in t.craters_near(f.face, x, z, 0.0, 10.0) {
                assert!((c.x - x).hypot(c.z - z) > c.radius, "not in {c:?}");
            }
        }
    }
);

/// The block in column (x, z)'s top cell on `f`'s face, from a generated chunk.
fn top_cell(t: &CrateredTerrain, f: FaceFrame, x: i64, z: i64) -> dwell_blocks::StateId {
    let top = t.top_row(f.face, x, z).expect("inside the disc");
    let s = i64::from(CHUNK_SIZE);
    let local = ChunkCoord::new(
        x.div_euclid(s) as i32,
        top.div_euclid(s) as i32,
        z.div_euclid(s) as i32,
    );
    let mut ch = Chunk::default();
    t.generate(f.chunk(local), &mut ch);
    let y = top.rem_euclid(s) as i32;
    let ly = if f.is_b() { CHUNK_SIZE - 1 - y } else { y };
    ch.voxels()[local_index(x.rem_euclid(s) as i32, ly, z.rem_euclid(s) as i32)]
}

/// A state's block name without its ramp family's suffix.
fn base_of(s: dwell_blocks::StateId) -> String {
    let n = dwell_blocks::block_of(s).name;
    n.strip_suffix("_ramp").unwrap_or(n).to_owned()
}

/// Points of the disc on a coarse grid, split by mare weight: (in a mare, in the highlands).
type Points = Vec<(f64, f64)>;

fn mare_and_highland_points(t: &CrateredTerrain, face: Face) -> (Points, Points) {
    let r = t.radius_m();
    let (mut mare, mut high) = (Vec::new(), Vec::new());
    for i in -24..=24 {
        for j in -24..=24 {
            let (x, z) = (f64::from(i) * r / 25.0, f64::from(j) * r / 25.0);
            if x.hypot(z) > 0.95 * r {
                continue;
            }
            match t.mare_weight(face, x, z) {
                w if w > 0.99 => mare.push((x, z)),
                w if w < 0.01 => high.push((x, z)),
                _ => {}
            }
        }
    }
    (mare, high)
}

both_faces!(
    /// The maria's ground is the data's darker block (owner, 2026-11-12: "keep darker seas (add a
    /// dark moon aria dirt block)"), the highlands the grid's block; a column at least half in a
    /// mare takes the mare's.
    fn maria_are_the_dark_block_and_highlands_moon_dust(f: FaceFrame) {
        let t = moon(4);
        assert_eq!(t.rules().maria.block, "dwell:moon_mare_dust");
        let dark = dwell_blocks::require("dwell:moon_mare_dust");
        let dust = dwell_blocks::require("dwell:moon_dust");
        let (mare, high) = mare_and_highland_points(&t, f.face);
        assert!(
            mare.len() > 20 && high.len() > 20,
            "{} / {}",
            mare.len(),
            high.len()
        );
        for &(x, z) in mare.iter().step_by(4) {
            let (cx, cz) = (x.floor() as i64, z.floor() as i64);
            assert_eq!(t.surface_block(f.face, cx, cz), dark);
            // The top may be one of the block's ramps (BUGS.md #144): its base block.
            assert_eq!(
                base_of(top_cell(&t, f, cx, cz)),
                base_of(dark),
                "mare at {x} {z}"
            );
        }
        for &(x, z) in high.iter().step_by(8) {
            let (cx, cz) = (x.floor() as i64, z.floor() as i64);
            assert_eq!(t.surface_block(f.face, cx, cz), dust);
            assert_eq!(
                base_of(top_cell(&t, f, cx, cz)),
                base_of(dust),
                "highland at {x} {z}"
            );
        }
    }
);

both_faces!(
    /// The far bake is darker over the maria, by the blocks' own colours (blocks.json `color`): a
    /// mare's texel is the mare block's colour, a highland's the moon dust's, with no separate
    /// albedo constant.
    fn the_bake_is_darker_over_the_maria_by_the_blocks_colours(f: FaceFrame) {
        let t = moon(4);
        let bake = t.bake_far(f.face, 128);
        let rgb = |name: &str| {
            let c = dwell_blocks::info(dwell_blocks::require(name)).color;
            [(c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff].map(|v| v as u8)
        };
        let (dark, dust) = (rgb("dwell:moon_mare_dust"), rgb("dwell:moon_dust"));
        let lum = |c: [u8; 3]| c.iter().map(|&v| f64::from(v)).sum::<f64>();
        assert!(lum(dark) < 0.8 * lum(dust), "the mare block is darker");
        let s = bake.texel_m();
        let texel = |v: f64| ((v + bake.radius_m) / s).floor() as usize;
        let at = |x: f64, z: f64| {
            let p = (texel(z) * bake.size + texel(x)) * 4;
            [bake.albedo[p], bake.albedo[p + 1], bake.albedo[p + 2]]
        };
        let (mare, high) = mare_and_highland_points(&t, f.face);
        let near = |a: [u8; 3], b: [u8; 3]| (0..3).all(|i| a[i].abs_diff(b[i]) <= 3);
        let opaque = |x: f64, z: f64| bake.albedo[(texel(z) * bake.size + texel(x)) * 4 + 3] == 255;
        let in_mare = |x: f64, z: f64| {
            t.mare_weight(
                f.face,
                bake.texel_centre(texel(x)),
                bake.texel_centre(texel(z)),
            )
        };
        let (mut m, mut h) = (0, 0);
        for &(x, z) in &mare {
            if opaque(x, z) && in_mare(x, z) > 0.99 {
                assert!(
                    near(at(x, z), dark),
                    "mare texel {:?} vs {dark:?}",
                    at(x, z)
                );
                m += 1;
            }
        }
        for &(x, z) in &high {
            if opaque(x, z) && in_mare(x, z) < 0.01 {
                assert!(
                    near(at(x, z), dust),
                    "highland texel {:?} vs {dust:?}",
                    at(x, z)
                );
                h += 1;
            }
        }
        assert!(m > 20 && h > 20, "{m} / {h}");
    }
);

both_faces!(
    /// The bake's seas are clearly dark: over the maria its luma (as the sky's moon reads it,
    /// dwell-client `render::moon::pack`) is 0.55 × the highlands', the old sky moon's contrast
    /// (its shader's maria at `1 − moon.maria`, 0.45; owner, 2026-11-12: "keep darker seas").
    fn the_bakes_maria_have_the_old_moons_contrast(f: FaceFrame) {
        let t = moon(4);
        let bake = t.bake_far(f.face, 128);
        let luma = |p: &[u8]| {
            0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])
        };
        let (mut mare, mut high) = (Vec::new(), Vec::new());
        for j in 0..bake.size {
            for i in 0..bake.size {
                let p = &bake.albedo[(j * bake.size + i) * 4..][..4];
                if p[3] != 255 {
                    continue;
                }
                match t.mare_weight(f.face, bake.texel_centre(i), bake.texel_centre(j)) {
                    w if w > 0.99 => mare.push(luma(p)),
                    w if w < 0.01 => high.push(luma(p)),
                    _ => {}
                }
            }
        }
        assert!(mare.len() > 100 && high.len() > 100);
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        let contrast = mean(&mare) / mean(&high);
        assert!(
            (contrast - 0.55).abs() < 0.03,
            "maria at {contrast:.3} × the highlands' luma, the old moon's 0.55"
        );
    }
);

/// Home has air, no sky disc and a coarse mesh to be seen by from the moon (M-6), and no grid LOD
/// (its far view is its own LOD tree); the moon has no air (its sky is space at its ground), a
/// coarse mesh of its surface (MOON_WORLD.md §5 M-5, §7.3) and a LOD reaching past its disc, as
/// the sun does (BUGS.md #143, #145).
// both-faces: the grids' data; no face enters.
#[test]
fn the_moon_is_airless_and_has_a_near_mesh() {
    let list = parse_grids(GRIDS_JSON).unwrap();
    let home = list.iter().find(|g| g.name == "dwell:home").unwrap();
    let moon = list.iter().find(|g| g.name == "dwell:moon").unwrap();
    assert!(home.atmosphere && !home.sky_disc && home.near_mesh.is_some() && home.lod.is_none());
    assert!(!moon.atmosphere && moon.sky_disc);
    let sun = list.iter().find(|g| g.name == "dwell:sun").unwrap();
    for g in [moon, sun] {
        let lod = g.lod.expect("a LOD on the grid");
        assert!(lod.reach_m() >= g.radius_m.unwrap(), "{}: {lod:?}", g.name);
    }
    let near = moon.near_mesh.expect("the moon's near mesh");
    assert!(near.cells >= 64 && near.sink_m > 0.0, "{near:?}");
    // A grid that declares no atmosphere has none (the sun's, by data alone).
    let bare = GRIDS_JSON.replacen("\"atmosphere\": false,", "", 1);
    let list = parse_grids(&bare).unwrap();
    assert!(
        !list
            .iter()
            .find(|g| g.name == "dwell:moon")
            .unwrap()
            .atmosphere
    );
}

both_faces!(
    /// The moon's slopes are ramped, not a field of bare ½ m stairs (owner, 0.61: "moon rock should
    /// have slopes, seems to only be generating half blocks"; BUGS.md #144): over generated chunks
    /// across the disc, a ½ m edge (a 4-neighbour exactly one row lower) under a flat top is bare;
    /// few are (the measure of `realistic_steps.rs`), the pieces are the ground's own ramp family,
    /// placed through the face's frame (hanging on face B). (Its ground is rarely 45° steep, so
    /// 1 m ramps are few; the rule's steep case is `ramps.rs`'s.)
    fn moon_slopes_are_ramps_not_bare_half_metre_stairs(f: FaceFrame) {
        let t = moon(11);
        let s = CHUNK_SIZE;
        let (mut edges, mut bare, mut ramps, mut steep) = (0usize, 0usize, 0usize, 0usize);
        // Patches of open ground, and one down the inner wall of the biggest crater near the
        // centre (its steepest ground: the 1 m ramps).
        let wall = t
            .craters_near(f.face, 0.0, 0.0, 3000.0, 60.0)
            .into_iter()
            .max_by(|a, b| a.radius.total_cmp(&b.radius))
            .expect("a crater");
        let w = (
            ((wall.x + 0.6 * wall.radius) / f64::from(s)).floor() as i32,
            (wall.z / f64::from(s)).floor() as i32,
        );
        for (cx, cz) in [(0, 0), (40, -30), (-120, 75), (230, 10), (-310, -260), w] {
            for (dx, dz) in [(0, 0), (1, 0), (0, 1)] {
                let (x0, z0) = (i64::from((cx + dx) * s), i64::from((cz + dz) * s));
                let top = |x: i64, z: i64| t.top_row(f.face, x, z);
                let mut chunks = std::collections::HashMap::new();
                for z in z0..z0 + i64::from(s) {
                    for x in x0..x0 + i64::from(s) {
                        let Some(tr) = top(x, z) else { continue };
                        let row_key = tr.div_euclid(i64::from(s));
                        let ch = chunks.entry(row_key).or_insert_with(|| {
                            let mut ch = Chunk::default();
                            t.generate(
                                f.chunk(ChunkCoord::new(cx + dx, row_key as i32, cz + dz)),
                                &mut ch,
                            );
                            ch
                        });
                        let y = tr.rem_euclid(i64::from(s)) as i32;
                        let ly = if f.is_b() { s - 1 - y } else { y };
                        let st = ch.voxels()[local_index((x - x0) as i32, ly, (z - z0) as i32)];
                        let base = dwell_blocks::block_of(st).name;
                        let ground = dwell_blocks::block_of(t.surface_block(f.face, x, z)).name;
                        let flat = base == ground;
                        if !flat {
                            assert_eq!(
                                base,
                                format!("{ground}_ramp"),
                                "the ground's own ramp family"
                            );
                            let half = dwell_blocks::state_property(st, "half");
                            assert_eq!(half == Some("top"), f.is_b(), "hung on face B: {half:?}");
                            ramps += 1;
                            if dwell_blocks::state_property(st, "shape") == Some("steep_upper") {
                                steep += 1;
                            }
                        }
                        let half = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                            .iter()
                            .any(|&(dx, dz)| top(x + dx, z + dz) == Some(tr - 1));
                        if half {
                            edges += 1;
                            if flat {
                                bare += 1;
                            }
                        }
                    }
                }
            }
        }
        let share = bare as f64 / edges.max(1) as f64;
        eprintln!(
            "face {:?}: ½ m edges {edges}, bare {bare} ({share:.3}), ramp tops {ramps}, steep {steep}",
            f.face
        );
        assert!(edges > 300, "enough slope sampled: {edges}");
        assert!(
            share < 0.1,
            "the moon's slopes ramp, not a field of ½ m stairs: {bare} of {edges} bare"
        );
    }
);
