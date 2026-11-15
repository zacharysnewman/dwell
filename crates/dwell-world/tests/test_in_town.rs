//! "Test in town" (owner, 2026-11-14: "a quick way to test a saved building in a village";
//! docs/BLUEPRINTS.md §6c): `TownRules::test_spot` puts a structure's blueprint beside the nearest
//! town site within reach, clear of the town's layout and of the blueprints already placed, on
//! known ground; none with no site near.

use dwell_world::FaceFrame;
use dwell_world::blueprint::{SHIPPED, TownSite};

fn site(x: i32, z: i32) -> TownSite {
    TownSite {
        face: FaceFrame::A,
        cell: [0, 0],
        x,
        z,
        plan: 0,
    }
}

fn clear(a: [i32; 4], b: [i32; 4]) -> bool {
    a[2] < b[0] || b[2] < a[0] || a[3] < b[1] || b[3] < a[1]
}

// face-a-only: test_spot is face-local columns only (the client passes face-local sites and boxes).
#[test]
fn a_test_blueprint_goes_beside_the_nearest_town_clear_of_its_layout_and_others() {
    let t = &SHIPPED.towns;
    let (near, far) = (site(100, 100), site(900, 900));
    let ground = |_: i32, _: i32| Some(40);
    let half = 2; // a 5 × 5 hut about its centre
    let at = t
        .test_spot(&[far, near], [90.0, 60.0], 400.0, half, &[], &ground)
        .expect("a spot by the near town");
    assert_eq!(at[1], 40, "on the ground");
    let b = [at[0] - half, at[2] - half, at[0] + half, at[2] + half];
    let layout = t.clear_box(&near);
    assert!(clear(b, layout), "{b:?} clear of the layout {layout:?}");
    // Beside it: within a few metres of the layout's edge, on the player's side.
    assert!(
        at[2] < layout[1],
        "the north side, toward the player: {at:?}"
    );
    assert!(layout[1] - at[2] <= half + 4, "close by: {at:?} {layout:?}");
    // A blueprint already there: the next spot keeps clear of it too.
    let next = t
        .test_spot(&[near], [90.0, 60.0], 400.0, half, &[b], &ground)
        .expect("another spot");
    let nb = [
        next[0] - half,
        next[2] - half,
        next[0] + half,
        next[2] + half,
    ];
    assert!(clear(nb, b) && clear(nb, layout), "{nb:?}");
    // Ground not known there: none on that column.
    let unknown = |x: i32, _: i32| (x > 200).then_some(40);
    assert_eq!(
        t.test_spot(&[near], [90.0, 60.0], 400.0, half, &[], &unknown),
        None
    );
    // No town within reach: none (the client places it at the crosshair).
    assert_eq!(
        t.test_spot(&[far], [90.0, 60.0], 400.0, half, &[], &ground),
        None
    );
}
