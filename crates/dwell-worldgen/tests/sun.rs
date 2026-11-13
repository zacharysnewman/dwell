//! The sun world's `volcanic` generator (docs/proposals/SUN_WORLD.md S-1, S-2; WORK_QUEUE item 42): the sun
//! grid's data parses and validates, and its terrain is deterministic per seed and face, holds lava
//! oceans, lava rivers in its rifts and crater lakes in its cones, basalt crusts and obsidian
//! shores, all by data (a rule changed in the data changes the terrain), on both faces.

use dwell_world::{CELL_H, CHUNK_SIZE, Chunk, ChunkCoord, Face, both_faces, local_index};
use dwell_worldgen::grids::{GRIDS_JSON, GridDef, GridFrameKind, GridGenerator, grid, parse_grids};
use dwell_worldgen::volcanic::{Column, VolcanicTerrain};

fn sun(seed: u64) -> VolcanicTerrain {
    VolcanicTerrain::for_grid("dwell:sun", seed).expect("the build's sun grid")
}

fn other(face: Face) -> Face {
    match face {
        Face::A => Face::B,
        Face::B => Face::A,
    }
}

/// The sun's entry in the build's grids file, with `from` → `to` applied once, as a grid list.
fn edited(edits: &[(&str, &str)]) -> Result<Vec<GridDef>, String> {
    let at = GRIDS_JSON.find("\"name\": \"dwell:sun\"").unwrap();
    let (head, mut tail) = (GRIDS_JSON[..at].to_owned(), GRIDS_JSON[at..].to_owned());
    for (from, to) in edits {
        assert!(tail.contains(from), "the fixture edit `{from}` applies");
        tail = tail.replacen(from, to, 1);
    }
    parse_grids(&format!("{head}{tail}"))
}

fn sun_of(list: &[GridDef], seed: u64) -> VolcanicTerrain {
    VolcanicTerrain::new(list.iter().find(|g| g.name == "dwell:sun").unwrap(), seed).unwrap()
}

/// Columns over the disc on a grid `step` m apart.
fn columns(t: &VolcanicTerrain, face: Face, step: f64) -> Vec<(f64, f64, Column)> {
    let r = t.radius_m();
    let n = (r / step) as i32;
    let mut out = Vec::new();
    for j in -n..=n {
        for i in -n..=n {
            let (x, z) = (f64::from(i) * step, f64::from(j) * step);
            if t.inside(x, z) {
                out.push((x, z, t.column(face, x, z)));
            }
        }
    }
    out
}

#[test]
fn the_build_s_sun_is_a_volcanic_grid_on_the_sun_s_path() {
    let s = grid("dwell:sun").expect("the sun grid");
    assert_eq!(s.frame, GridFrameKind::SunPath);
    assert_eq!(s.radius_m, Some(25_000.0));
    // Owner, 2026-11-12: "Its gravity will be the same as earth world for now."
    assert_eq!(s.gravity_scale, 1.0);
    assert!(s.capture_m.unwrap() > 25_000.0);
    assert!(matches!(s.generator, GridGenerator::Volcanic(_)));
    assert_ne!(s.seed(42), 42);
    assert_ne!(s.seed(42), s.seed(43), "every world its own sun");
    assert_ne!(
        s.seed(42),
        grid("dwell:moon").unwrap().seed(42),
        "not the moon's"
    );
}

/// A bad entry fails with a message naming it.
#[test]
fn bad_sun_rules_fail_with_a_message() {
    let cases: [(&str, &str, &str); 8] = [
        (
            "\"frame\": \"sunPath\"",
            "\"frame\": \"sunpath\"",
            "`frame`",
        ),
        (
            "\"crust\": \"dwell:basalt\"",
            "\"crust\": \"dwell:no_rock\"",
            "`crust` names no block",
        ),
        (
            "\"fluid\": \"dwell:lava\"",
            "\"fluid\": \"dwell:magma\"",
            "lava: `fluid` names no block",
        ),
        (
            "\"seaM\": 92",
            "\"seaM\": 10",
            "`lava.seaM` must be over `minM`",
        ),
        (
            "\"cellM\": 3200",
            "\"cellM\": 2000",
            "`cones.radiusM` must stay under half",
        ),
        ("\"craterRatio\": 0.18", "\"craterRatio\": 1", "craterRatio"),
        (
            "\"width\": 0.06",
            "\"width\": 0",
            "`rifts.width` must be > 0",
        ),
        (
            "\"kind\": \"volcanic\"",
            "\"kind\": \"lava\"",
            "generator.kind",
        ),
    ];
    for (from, to, want) in cases {
        let err = edited(&[(from, to)]).expect_err(&format!("`{to}` is rejected"));
        assert!(err.contains(want), "`{err}` mentions {want}");
        assert!(err.contains("dwell:sun"), "`{err}` names the grid");
    }
}

both_faces!(
    /// The same seed gives the same sun; another seed or the other face another.
    fn the_sun_is_deterministic_per_seed_and_face(f: FaceFrame) {
        let (a, b) = (sun(7), sun(7));
        let pts = [(0.0, 0.0), (5_000.0, -3_000.0), (-12_000.0, 8_000.0)];
        let col = |t: &VolcanicTerrain, face: Face| {
            pts.iter()
                .map(|&(x, z)| t.column(face, x, z))
                .collect::<Vec<_>>()
        };
        assert_eq!(col(&a, f.face), col(&b, f.face));
        let grid = |t: &VolcanicTerrain, face: Face| {
            columns(t, face, 1_000.0)
                .iter()
                .map(|c| c.2.ground)
                .collect::<Vec<_>>()
        };
        assert_ne!(grid(&a, f.face), grid(&sun(8), f.face), "another seed");
        assert_ne!(grid(&a, f.face), grid(&a, other(f.face)), "the other face");
    }
);

both_faces!(
    /// The sun holds every feature its data asks for: lava oceans at the sea's level, lava rivers
    /// in the rifts (over the sea), crater lakes in cones (their lava over the flanks round them),
    /// basalt crust and obsidian shores; its ground stays in the build volume.
    fn the_sun_has_lava_oceans_rivers_lakes_and_crusts(f: FaceFrame) {
        let t = sun(3);
        let r = t.rules().clone();
        let cols = columns(&t, f.face, 200.0);
        let n = cols.len() as f64;
        let molten = cols.iter().filter(|c| c.2.molten()).count() as f64;
        let ocean = cols
            .iter()
            .filter(|c| c.2.molten() && c.2.lava == r.lava.sea_m)
            .count() as f64;
        let rivers = cols
            .iter()
            .filter(|c| {
                c.2.molten() && c.2.lava > r.lava.sea_m && t.rift_weight(f.face, c.0, c.1) > 0.5
            })
            .count();
        assert!(
            ocean / n > 0.05 && molten / n < 0.8,
            "oceans {:.2}, molten {:.2}",
            ocean / n,
            molten / n
        );
        assert!(
            rivers > 10,
            "lava rivers over the sea in the rifts: {rivers}"
        );
        // Every cone's crater holds a lake standing over its foot.
        let cones = t.cones_near(f.face, 0.0, 0.0, t.radius_m() * 0.8);
        assert!(cones.len() > 10, "{} cones", cones.len());
        let mut lakes = 0;
        for c in &cones {
            let summit = t.column(f.face, c.x, c.z);
            let foot = t.base_height(f.face, c.x, c.z);
            if summit.molten() && summit.lava > foot + 0.3 * c.height {
                lakes += 1;
            }
            let flank = t.column(f.face, c.x + 0.5 * c.radius, c.z);
            assert!(flank.ground > foot - 1e-9 || flank.molten(), "{c:?}");
        }
        assert!(
            lakes * 10 >= cones.len() * 8,
            "{lakes} of {} cones hold a lake",
            cones.len()
        );
        // Basalt and obsidian, both.
        let crust = dwell_blocks::require("dwell:basalt");
        let shore = dwell_blocks::require("dwell:obsidian");
        let dry: Vec<_> = cols.iter().filter(|c| !c.2.molten()).collect();
        let glass = dry.iter().filter(|c| t.ground_block(&c.2) == shore).count();
        let rock = dry.iter().filter(|c| t.ground_block(&c.2) == crust).count();
        assert!(glass > 0 && rock > glass, "obsidian {glass}, basalt {rock}");
        for (x, z, c) in &cols {
            assert!(
                c.ground >= r.min_m && c.surface() < dwell_world::BUILD_TOP_M - 8.0,
                "{c:?} at {x}, {z}"
            );
        }
    }
);

both_faces!(
    /// The data drives it: a higher sea floods more, no rift depth leaves no rivers, no cones leaves
    /// no crater lakes (a fixture by data alone).
    fn the_sun_s_rules_are_its_data(f: FaceFrame) {
        let molten = |t: &VolcanicTerrain| {
            columns(t, f.face, 400.0)
                .iter()
                .filter(|c| c.2.molten())
                .count()
        };
        let base = sun(5);
        let high = sun_of(&edited(&[("\"seaM\": 92", "\"seaM\": 110")]).unwrap(), 5);
        assert!(molten(&high) > molten(&base), "a higher sea floods more");
        let dry = sun_of(
            &edited(&[
                ("\"depthM\": 16", "\"depthM\": 0"),
                ("\"chance\": 0.55", "\"chance\": 0"),
                ("\"seaM\": 92", "\"seaM\": 17"),
            ])
            .unwrap(),
            5,
        );
        assert_eq!(molten(&dry), 0, "no sea, rivers or lakes: no lava");
        assert!(dry.cones_near(f.face, 0.0, 0.0, 20_000.0).is_empty());
    }
);

both_faces!(
    /// A chunk holds the crust up to the ground and lava over it up to its level, face-local
    /// through `FaceFrame`: the same column on both faces reads the same from its own face.
    fn the_sun_s_chunks_hold_the_crust_and_the_lava(f: FaceFrame) {
        let t = sun(3);
        let lava = f.state(t.lava_block());
        // A molten column.
        let (x, z) = (-24_000..24_000)
            .step_by(97)
            .flat_map(|x| (-24_000..24_000).step_by(89).map(move |z| (x, z)))
            .find(|&(x, z)| {
                t.rows(f.face, x, z)
                    .is_some_and(|(top, l)| l.is_some_and(|l| l > top + 2))
            })
            .expect("a molten column");
        let (top, l) = t.rows(f.face, x, z).unwrap();
        let l = l.unwrap();
        let s = i64::from(CHUNK_SIZE);
        let cell = |row: i64| {
            let local = ChunkCoord::new(
                x.div_euclid(s) as i32,
                row.div_euclid(s) as i32,
                z.div_euclid(s) as i32,
            );
            let mut ch = Chunk::default();
            t.generate(f.chunk(local), &mut ch);
            let y = row.rem_euclid(s) as i32;
            let ly = if f.is_b() { CHUNK_SIZE - 1 - y } else { y };
            ch.voxels()[local_index(x.rem_euclid(s) as i32, ly, z.rem_euclid(s) as i32)]
        };
        let col = t.column(f.face, x as f64 + 0.5, z as f64 + 0.5);
        assert_eq!(cell(top), f.state(t.ground_block(&col)), "the crust's top");
        assert_eq!(cell(top + 1), lava, "lava on it");
        assert_eq!(cell(l), lava, "up to its level");
        assert_eq!(cell(l + 1), dwell_world::M.air, "air over it");
        assert!((col.lava - (l + 1) as f64 * CELL_H).abs() <= CELL_H);
    }
);

both_faces!(
    /// The far bake (the sky's sun disc, S-3): lava bright and flat, the crusts dark, deterministic.
    fn the_sun_s_bake_is_its_lava_and_crusts(f: FaceFrame) {
        let t = sun(11);
        let b = t.bake_far(f.face, 64);
        assert_eq!(b, t.bake_far(f.face, 64), "deterministic");
        assert_ne!(b.albedo, t.bake_far(other(f.face), 64).albedo);
        let lava = dwell_blocks::info(t.lava_block()).color;
        let rgb = |p: &[u8]| (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]);
        let (mut molten, mut dry) = (0, 0);
        for k in 0..64 * 64 {
            let p = &b.albedo[k * 4..k * 4 + 4];
            if p[3] == 0 {
                continue;
            }
            if rgb(p) == lava {
                molten += 1;
            } else {
                dry += 1;
                let luma = u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]);
                assert!(luma < 0xe8 + 0x56 + 0x1c, "the crusts are darker than lava");
            }
        }
        assert!(molten > 100 && dry > 100, "lava {molten}, crust {dry}");
    }
);
