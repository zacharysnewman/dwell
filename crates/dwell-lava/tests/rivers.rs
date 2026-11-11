//! Lava V2 (docs/proposals/VOLCANOES.md §6.4): fields that grow with their flow, rivers that reach
//! the sea and build land. On a fixture flank (a 0.3 slope falling 1100 m to a shore, then a sea
//! floor under 2 m of water), a river vent's lava runs over a kilometre downslope, quenches in the
//! sea and builds new land above it; the field adds the tiles its lava reaches and drops the ones
//! all rock; the area cap holds; a save mid-flow loads and runs on identically (and a V1 record
//! still loads); a long river's tick costs what its molten tiles cost, under a budget. On both
//! faces: the field is face-local, its cells and the players' points cross through the face frame.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use dwell_lava::field::DRY;
use dwell_lava::{
    Column, Field, LavaData, LavaEvent, TILE, Vent, field_of, fixture::FIXTURE, record_of,
};
use dwell_world::{FaceFrame, both_faces};

/// The shore (field-local x, m from the vent) and the sea's surface (face-local m).
const SHORE: i32 = 1100;
const SEA: f32 = 2.0;
const SLOPE: f32 = 0.3;

/// The fixture flank's column (x, z): a slope falling `SLOPE` a metre toward +x to the shore, with a
/// shallow valley along z = 0 and a ripple across it; past the shore, a sea floor 1.5 m under the
/// sea at the shore, falling 0.05 a metre.
fn flank(x: i32, z: i32) -> Column {
    let (xf, zf) = (x as f32, z as f32);
    let wobble = 0.3 * (zf * 0.37).sin() + 0.02 * zf.abs();
    if x < SHORE {
        Column {
            bed: SEA + 0.5 + SLOPE * (SHORE as f32 - xf) + wobble,
            water: None,
        }
    } else {
        Column {
            bed: SEA - 1.5 - 0.05 * (xf - SHORE as f32) + wobble,
            water: Some(SEA),
        }
    }
}

/// A growing field of the fixture's runny lava under `rules`, its vent at (0, 0) covering 8
/// columns round it to start.
fn river(rules: &LavaData) -> Field {
    let mut f = Field::growing(rules, "test:runny").unwrap();
    f.origin = [-40, 7000];
    f.cover([-8, -8], [8, 8], &mut |x, z, _| Some(flank(x, z)), 0.0);
    f.vents
        .push(Vent::from(FIXTURE.vent("test:flank_river").unwrap()));
    f
}

/// Runs `secs` lava seconds far from any player (runs of `farEverySeconds`), growing on the flank.
fn far(f: &mut Field, secs: f32) -> Vec<LavaEvent> {
    let mut out = Vec::new();
    let n = (secs / f.rules.far_every_seconds).round() as usize;
    for _ in 0..n {
        f.advance_on(
            f.rules.far_every_seconds,
            false,
            Some(&mut |x, z, _| Some(flank(x, z))),
        );
        out.extend(f.take_events());
    }
    out
}

/// The furthest field-local x with lava or new rock on it.
fn reach(f: &Field) -> i32 {
    f.columns()
        .filter(|&i| {
            let [x, z] = f.col(i);
            f.depth[i] > 0.0 || f.ground[i] > flank(x, z).bed + 1e-3
        })
        .map(|i| f.col(i)[0])
        .max()
        .unwrap_or(0)
}

/// The long run, once a process (it is face-local; each face's test checks its own crossing).
struct Run {
    field: Field,
    /// Lava seconds until the front passed 1 km, and until the sea had new land.
    at_km: f32,
    at_land: f32,
    events: Vec<LavaEvent>,
    most_tiles: usize,
}

fn long_run() -> &'static Run {
    static RUN: OnceLock<Run> = OnceLock::new();
    RUN.get_or_init(|| {
        let mut field = river(&FIXTURE);
        let start = field.tiles().len();
        let (mut t, mut at_km, mut at_land) = (0.0f32, f32::NAN, f32::NAN);
        let mut events = Vec::new();
        let mut most = start;
        // At most six lava hours.
        while t < 6.0 * 3600.0 {
            events.extend(far(&mut field, 300.0));
            t += 300.0;
            most = most.max(field.tiles().len());
            if at_km.is_nan() && reach(&field) >= 1000 {
                at_km = t;
            }
            if land(&field) > 0 {
                at_land = t;
                break;
            }
        }
        Run {
            field,
            at_km,
            at_land,
            events,
            most_tiles: most,
        }
    })
}

/// Columns past the shore whose rock now stands above the sea (dry land).
fn land(f: &Field) -> usize {
    f.columns()
        .filter(|&i| f.col(i)[0] >= SHORE && !f.water[i] && f.ground[i] >= SEA)
        .count()
}

both_faces!(
    fn a_river_vents_lava_runs_over_a_kilometre_to_the_sea_and_builds_land(f: FaceFrame) {
        let run = long_run();
        assert!(
            run.at_km.is_finite(),
            "the front passed 1 km (reached {} m)",
            reach(&run.field)
        );
        assert!(run.at_land.is_finite(), "it reached the sea");
        // Within two lava hours (nine world days of 800 s at speed 1).
        assert!(
            run.at_land <= 2.0 * 3600.0,
            "at the sea after {} s",
            run.at_land
        );
        let field = &run.field;
        let drop = flank(0, 0).bed - SEA;
        assert!(drop > 300.0, "{drop} m downslope");
        // Basalt behind the front, obsidian at the sea; each event's cells are its own rows on
        // this face.
        let rocks = |want: &str, from: i32| {
            run.events.iter().any(
                |e| matches!(e, LavaEvent::Rock { x, rock, .. } if *rock == want && *x >= from),
            )
        };
        assert!(rocks("dwell:basalt", 900), "basalt down the flank");
        assert!(
            rocks("dwell:obsidian", SHORE),
            "obsidian quenched in the sea"
        );
        for e in run.events.iter().step_by(97) {
            if let LavaEvent::Rock { x, z, rows, .. } = e {
                let back: Vec<[i32; 3]> = e
                    .cells(f, field.origin)
                    .iter()
                    .map(|&c| f.cell(c))
                    .collect();
                let want: Vec<[i32; 3]> = rows
                    .clone()
                    .map(|r| [field.origin[0] + x, r, field.origin[1] + z])
                    .collect();
                assert_eq!(back, want);
            }
        }
        // A player at the sea's edge is near the field; one 2 km past it is not.
        let near = |x: f64| {
            f.point([
                f64::from(field.origin[0]) + x,
                f64::from(SEA) + 1.0,
                f64::from(field.origin[1]),
            ])
        };
        assert!(field.is_near(f, &[near(f64::from(SHORE))]));
        assert!(!field.is_near(f, &[near(f64::from(SHORE) + 2000.0)]));
        new_land_appears_at_the_shore(run);
        the_tiles_followed_the_river(run);
        a_long_rivers_tick_stays_under_a_budget(f, run);
    }
);

/// New land at the shore: built up out of the sea.
fn new_land_appears_at_the_shore(run: &Run) {
    {
        let field = &run.field;
        let n = land(field);
        assert!(n > 0, "dry land past the shore");
        // Built on the sea floor: up through the water, every new land column was a sea column.
        for i in field.columns() {
            let [x, z] = field.col(i);
            if x >= SHORE && !field.water[i] {
                assert!(field.ground[i] >= SEA, "({x}, {z}) is land above the sea");
                assert!(flank(x, z).bed < SEA, "and was under it");
            }
        }
        // The steam came off the sea.
        assert!(
            run.events
                .iter()
                .any(|e| matches!(e, LavaEvent::Steam { x, .. } if *x >= SHORE))
        );
    }
}

/// The tiles followed the river from its 17 × 17 start to the shore.
fn the_tiles_followed_the_river(run: &Run) {
    assert!(run.most_tiles > 100, "{} tiles", run.most_tiles);
    let span = (run.field.tiles().iter().map(|t| t[0]).max().unwrap() + 1) * TILE as i32;
    assert!(span >= SHORE, "tiles reach the shore: {span}");
}

both_faces!(
    fn a_field_adds_the_tiles_its_lava_reaches_and_drops_the_ones_all_rock(f: FaceFrame) {
        // A short river: then its vent stops, it all cools to rock, and its tiles go.
        let mut field = river(&FIXTURE);
        far(&mut field, 600.0);
        let grown = field.tiles().len();
        assert!(grown > 9, "grew: {grown} tiles");
        let rocked = field.rocked;
        field.vents.clear();
        far(&mut field, 3600.0);
        assert_eq!(field.molten_tiles(), 0, "all rock");
        assert_eq!(field.tiles().len(), 0, "and every tile dropped");
        assert!(field.rocked > rocked, "its lava turned to rock first");
        assert_eq!(field.check_links(), Ok(()));
        let _ = f;
    }
);

both_faces!(
    fn the_area_cap_holds(f: FaceFrame) {
        let rules = LavaData {
            max_tiles: 16,
            ..FIXTURE
        };
        let mut field = river(&rules);
        let events = far(&mut field, 1800.0);
        assert!(field.tiles().len() <= 16, "{} tiles", field.tiles().len());
        assert!(field.refused > 0, "the cap refused tiles");
        // Its lava stays within the tiles it has: walls at their edges.
        let edge = field
            .tiles()
            .iter()
            .map(|t| (t[0] + 1) * TILE as i32)
            .max()
            .unwrap();
        assert!(reach(&field) < edge);
        assert!(events.iter().all(|e| match e {
            LavaEvent::Rock { x, z, .. } => field.slot_any(*x, *z).is_some() || *x < edge,
            LavaEvent::Steam { .. } => true,
        }));
        assert_eq!(field.check_links(), Ok(()));
        let _ = f;
    }
);

both_faces!(
    fn a_save_mid_flow_loads_and_runs_on_identically(f: FaceFrame) {
        let mut field = river(&FIXTURE);
        far(&mut field, 900.0);
        field.advance(7.0, false); // some time owed
        let rec = record_of(&field);
        let mut back = field_of(&rec, &FIXTURE).expect("reads its own record");
        assert_eq!(back.tiles(), field.tiles());
        assert_eq!(back.owed_s, field.owed_s);
        assert_eq!(back.vents, field.vents);
        let a = far(&mut field, 600.0);
        let b = far(&mut back, 600.0);
        assert_eq!(a, b, "the same events");
        assert_eq!(back.tiles(), field.tiles(), "the same tiles");
        assert_eq!(back.depth, field.depth, "the same lava");
        assert_eq!(back.ground, field.ground, "the same rock");
        assert_eq!(back.heat, field.heat);
        let feet = f.point([
            f64::from(field.origin[0]) + 0.5,
            f64::from(flank(0, 0).bed) + 0.1,
            f64::from(field.origin[1]) + 0.5,
        ]);
        assert_eq!(back.submerged(f, feet), field.submerged(f, feet));
        assert!(field.submerged(f, feet), "standing in the vent's lava");
    }
);

both_faces!(
    // The record grows additively: V1's (a fixed rectangle) still loads, as a fixed field.
    fn a_version_1_record_still_loads(f: FaceFrame) {
        let name = b"dwell:lava";
        let (w, h) = (3u16, 2u16);
        let mut rec = vec![1, name.len() as u8];
        rec.extend_from_slice(name);
        rec.extend_from_slice(&w.to_le_bytes());
        rec.extend_from_slice(&h.to_le_bytes());
        rec.extend_from_slice(&5i32.to_le_bytes());
        rec.extend_from_slice(&(-9i32).to_le_bytes());
        rec.extend_from_slice(&1.5f32.to_le_bytes());
        for i in 0..6 {
            rec.extend_from_slice(&(10.0 - i as f32).to_le_bytes());
            rec.extend_from_slice(&(if i == 0 { 0.8f32 } else { 0.0 }).to_le_bytes());
            rec.extend_from_slice(&1.0f32.to_le_bytes());
            rec.push(u8::from(i == 5));
        }
        rec.extend_from_slice(&1u16.to_le_bytes());
        rec.push(1);
        rec.extend_from_slice(&0u16.to_le_bytes());
        rec.extend_from_slice(&0u16.to_le_bytes());
        rec.extend_from_slice(&1.0f32.to_le_bytes());
        rec.extend_from_slice(&0.0f32.to_le_bytes());
        let mut field = field_of(&rec, &dwell_lava::DATA).expect("a V1 record loads");
        assert!(!field.grows, "as a fixed field");
        assert_eq!((field.origin, field.owed_s), ([5, -9], 1.5));
        let (v, last) = (field.at(0, 0), field.at(2, 1));
        assert_eq!((field.ground[v], field.depth[v]), (10.0, 0.8));
        assert_eq!(
            (field.ground[last], field.water[last], field.water_top[last]),
            (5.0, true, DRY)
        );
        assert_eq!(field.columns().count(), 6, "its rectangle only");
        assert!(field.slot(3, 0).is_none(), "a wall past its edge");
        field.run(10.0);
        assert!(
            field.tiles().len() == 1 && field.depth[field.at(1, 0)] > 0.0,
            "and runs"
        );
        let feet = f.point([5.5, 10.1, -8.5]);
        assert!(field.submerged(f, feet));
        // Saved again, it is a V2 record of the same field.
        let again = field_of(&record_of(&field), &dwell_lava::DATA).unwrap();
        assert_eq!((again.depth, again.grows), (field.depth.clone(), false));
    }
);

/// A tick's cost follows the molten tiles, not the field's size: a river a kilometre long costs a
/// tick near a player of at most its molten tiles' columns and the columns its flow reaches out of
/// them, each substep, and well under a frame.
fn a_long_rivers_tick_stays_under_a_budget(f: FaceFrame, run: &Run) {
    {
        let mut field = run.field.clone();
        let player = f.point([
            f64::from(field.origin[0] + SHORE),
            f64::from(SEA) + 1.0,
            f64::from(field.origin[1]),
        ]);
        assert!(field.is_near(f, &[player]));
        let molten = field.molten_tiles();
        assert!(molten > 50, "a long molten river: {molten} tiles");
        let total = field.tiles().len();
        let lava_tick = field.rules.tick_every as f32 / 60.0;
        let mut worst = Duration::ZERO;
        for _ in 0..10 {
            let (work, steps) = (field.work, field.substeps);
            let molten = field.molten_tiles();
            let t = Instant::now();
            let near = field.is_near(f, &[player]);
            field.advance_on(lava_tick, near, Some(&mut |x, z, _| Some(flank(x, z))));
            worst = worst.max(t.elapsed());
            let per = (field.work - work) / (field.substeps - steps).max(1);
            // Its molten tiles' columns, and at most each one's four edges out of them.
            let bound = molten * TILE * TILE + molten * 4 * TILE;
            assert!(
                per as usize <= bound,
                "{per} columns a substep, bound {bound} ({molten} molten of {total} tiles)"
            );
            field.take_events();
        }
        assert!(worst < Duration::from_millis(20), "a tick took {worst:?}");
    }
}
