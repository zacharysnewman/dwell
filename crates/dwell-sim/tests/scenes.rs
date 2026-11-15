//! Scenes through the server (docs/THEATER_MODE.md §6.3, `dwell_sim::scene`): the dam-break scene
//! built on the flat world makes its valley, dam and lake; the lake is held still at its level
//! from the start; its event deletes the dam and the lake runs down the valley.

mod common;

use common::*;
use dwell_sim::scene::Scene;
use dwell_world::{FaceFrame, row_of_m};

fn dam_break() -> Scene {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../shared/theater/scenes/dam_break.json"
    );
    Scene::parse(&std::fs::read_to_string(path).expect("the scene file")).expect("valid")
}

fn water_level(f: &Fixture, x: i32, z: i32) -> Option<f64> {
    f.server.water().level_at(FaceFrame::A, x, z)
}

/// The topmost water cell's top (m) in column (x, z) of the world, if any.
fn water_top(f: &mut Fixture, x: i32, z: i32) -> Option<f64> {
    let w = f.server.world();
    (0..128)
        .rev()
        .find(|&r| dwell_blocks::info(w.get_voxel(x, r, z)).liquid)
        .map(|r| dwell_world::m_of_row(r + 1))
}

#[test]
fn the_dam_break_scene_builds_its_valley_dam_and_lake_and_the_flood_runs_down_it() {
    // face-a-only: the flat world the scene builds on is face A's; the scene's cells on both
    // faces are `scene::tests::the_scene_is_built_face_local`.
    let scene = dam_break();
    let mut f = Fixture::new(flat());
    let t0 = std::time::Instant::now();
    let report = f.server.build_scene(&scene);
    let built_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let r: serde_json::Value = serde_json::from_str(&report).expect("JSON");
    eprintln!("built in {built_ms:.0} ms: {report}");
    assert!(r["cells"].as_u64().unwrap() > 100_000, "{report}");
    assert!(r["waterCells"].as_u64().unwrap() > 40_000, "{report}");

    // The cells: the floor 12 m under the land (20 m); the dam 10 m over it 100 m down (rows
    // 40–59: 20–30 m), 4 m thick; the lake to 29 m behind it.
    let stone = dwell_blocks::require("dwell:stone");
    let sand = dwell_blocks::require("dwell:sand");
    let w = f.server.world();
    assert_eq!(
        w.get_voxel(0, row_of_m(19.5), 20),
        sand,
        "the bed at the head"
    );
    for z in 100..104 {
        assert_eq!(
            w.get_voxel(0, row_of_m(20.0), z),
            stone,
            "the dam's foot at z {z}"
        );
        assert_eq!(
            w.get_voxel(0, row_of_m(29.75), z),
            stone,
            "the dam's top row at z {z}"
        );
        assert_eq!(
            w.get_voxel(0, row_of_m(30.25), z),
            dwell_blocks::AIR,
            "over the dam at z {z}"
        );
    }
    assert_eq!(
        w.get_voxel(0, row_of_m(20.25), 104),
        dwell_blocks::AIR,
        "below the dam"
    );
    assert_eq!(water_top(&mut f, 0, 50), Some(29.0), "the lake's level");
    assert_eq!(water_top(&mut f, 0, 150), None, "dry below the dam");

    // Held still: after a few seconds the lake is at its level, nothing awake.
    for _ in 0..180 {
        f.server.step();
        f.take_all();
    }
    assert!(
        f.server.water().held_at(FaceFrame::A, 0, 50),
        "the lake is held"
    );
    let lake = water_level(&f, 0, 50).expect("the lake joined the grid");
    assert!((lake - 29.0).abs() < 0.01, "level {lake}");
    assert_eq!(f.server.water().awake_chunks(), 0, "still");
    assert_eq!(water_top(&mut f, 0, 150), None);

    // The event: the dam deleted at once; the lake falls and the flood runs down the valley.
    let e: serde_json::Value =
        serde_json::from_str(&f.server.scene_event(&scene, 0).expect("the event")).unwrap();
    let dam_cells = r["structures"][0]["cells"].as_u64().unwrap();
    assert_eq!(e["removed"].as_u64(), Some(dam_cells), "{e}");
    let mut reached = None;
    for tick in 0..60 * 20 {
        f.server.step();
        f.take_all();
        if reached.is_none() && water_top(&mut f, 0, 220).is_some() {
            reached = Some(f64::from(tick) / 60.0);
        }
    }
    let lake = water_level(&f, 0, 50).unwrap_or(0.0);
    eprintln!(
        "after 20 s: the lake at {lake:.2} m, the flood 116 m below the dam after {reached:?} s, {} chunks awake",
        f.server.water().awake_chunks()
    );
    assert!(lake < 28.0, "the lake fell: {lake}");
    assert!(
        reached.is_some(),
        "the flood reached 116 m below the dam within 20 s"
    );
    let world = f.server.world();
    for r in row_of_m(20.0)..row_of_m(30.0) {
        assert_ne!(
            world.get_voxel(0, r, 101),
            stone,
            "the dam is gone (row {r})"
        );
    }
}
