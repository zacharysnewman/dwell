//! Lava V1 (docs/proposals/VOLCANOES.md §6): flow, a crater lake, a river building land, the quench,
//! burning, saves, a fluid by data alone, validation and the far fast-forward, all on both faces:
//! fields are face-local, and every world point or cell crosses through the face frame.

use dwell_lava::{
    Burn, DATA, Field, LavaData, LavaEvent, VentDef, VentKind, field_of, fixture::FIXTURE,
    record_of,
};
use dwell_world::{FaceFrame, both_faces};

/// A river field: `w × h` columns on a slope falling `fall` a metre toward +x from `top`.
fn slope(w: usize, h: usize, top: f32, fall: f32) -> Field {
    let mut f = Field::new(&DATA, "dwell:lava", w, h).expect("dwell:lava");
    for z in 0..h {
        for x in 0..w {
            let i = f.at(x, z);
            f.ground[i] = top - fall * x as f32;
        }
    }
    f
}

/// The furthest column toward +x with lava or new rock on it.
fn front(f: &Field, bed: &[f32]) -> usize {
    (0..f.w * f.h)
        .filter(|&i| f.depth[i] > 0.0 || f.ground[i] > bed[i] + 1e-3)
        .map(|i| i % f.w)
        .max()
        .unwrap_or(0)
}

/// Runs `secs` as lava ticks of 0.1 s near a player.
fn near(f: &mut Field, secs: f32) -> Vec<LavaEvent> {
    let mut out = Vec::new();
    for _ in 0..(secs * 10.0).round() as usize {
        f.advance(0.1, true);
        out.extend(f.take_events());
    }
    out
}

/// A rock event's world cells on face `f`, read back face-local: they must be its own rows.
fn check_cells(f: FaceFrame, field: &Field, events: &[LavaEvent]) {
    for e in events {
        if let LavaEvent::Rock { x, z, rows, .. } = e {
            let world = e.cells(f, field.origin);
            let back: Vec<[i32; 3]> = world.iter().map(|&c| f.cell(c)).collect();
            let want: Vec<[i32; 3]> = rows
                .clone()
                .map(|r| [field.origin[0] + *x as i32, r, field.origin[1] + *z as i32])
                .collect();
            assert_eq!(back, want);
            assert_eq!(
                world,
                e.cells(FaceFrame::A, field.origin)
                    .iter()
                    .map(|&c| f.cell(c))
                    .collect::<Vec<_>>()
            );
        }
    }
}

const RIVER: VentDef = VentDef {
    name: "test:here",
    fluid: "dwell:lava",
    kind: VentKind::River,
    x: 2,
    z: 4,
    rate_m3s: 1.0,
    level_m: 0.0,
};

both_faces!(
    fn lava_flows_downhill_far_slower_than_water(f: FaceFrame) {
        let mut field = slope(64, 9, 30.0, 0.1);
        field.add_vent(&RIVER).unwrap();
        let bed = field.ground.clone();
        let events = near(&mut field, 60.0);
        check_cells(f, &field, &events);
        let reached = front(&field, &bed) as f32 - RIVER.x as f32;
        assert!(reached >= 3.0, "lava moves downhill: {reached} m");
        // Water on the same 10 % slope at half a metre deep, by its own law (Manning's, the water
        // grid's roughness): metres a second. Lava's front makes a small part of that.
        let n = dwell_world::flow::FLOW.liquids[0].manning as f32;
        let water = 0.5f32.powf(2.0 / 3.0) * 0.1f32.sqrt() / n;
        let lava = reached / 60.0;
        assert!(lava < water / 20.0, "lava {lava} m/s, water {water} m/s");
        // Mass: what the vent brought is in the field or its rock.
        let total = field.stored() + field.rocked;
        assert!(
            (total - field.brought).abs() < 1e-2 * field.brought,
            "{total} vs {}",
            field.brought
        );
    }
);

both_faces!(
    fn a_lake_vent_fills_its_crater_to_the_rim(f: FaceFrame) {
        let mut field = Field::new(&DATA, "dwell:lava", 33, 33).unwrap();
        let r = |x: usize, z: usize| ((x as f32 - 16.0).powi(2) + (z as f32 - 16.0).powi(2)).sqrt();
        for z in 0..33 {
            for x in 0..33 {
                let d = r(x, z);
                let i = field.at(x, z);
                field.ground[i] = if d < 6.0 {
                    8.0 + 2.0 * d / 6.0
                } else if d < 8.0 {
                    10.0
                } else {
                    10.0 - (d - 8.0)
                };
            }
        }
        let vent = FIXTURE.vent("test:crater_lake").expect("fixture vent");
        field.add_vent(vent).unwrap();
        let bed = field.ground.clone();
        let events = near(&mut field, 300.0);
        check_cells(f, &field, &events);
        for z in 0..33 {
            for x in 0..33 {
                let i = field.at(x, z);
                if r(x, z) < 5.5 {
                    // Up to the rim, short of it only at the thin edge, where the depth that never
                    // flows and the law's h³ hold the last of it back.
                    let short = 10.0 - 2.0 * field.fluid.yield_depth_m - 0.01;
                    assert!(
                        field.surface(i) >= short,
                        "({x}, {z}) at {}",
                        field.surface(i)
                    );
                }
                if r(x, z) > 8.5 {
                    assert_eq!(field.surface(i), bed[i], "({x}, {z}) is past the rim");
                }
                assert!(field.surface(i) <= 10.0 + 1e-3);
            }
        }
        let v = field.at(16, 16);
        assert!(field.depth[v] > 0.0, "the vent's lava stays molten");
    }
);

both_faces!(
    fn a_river_vent_reaches_lower_ground_and_cools_behind_its_front(f: FaceFrame) {
        let mut field = slope(48, 16, 20.0, 0.3);
        let vent = FIXTURE.vent("test:river").expect("fixture vent");
        field.add_vent(vent).unwrap();
        let bed = field.ground.clone();
        let events = near(&mut field, 240.0);
        check_cells(f, &field, &events);
        let reach = front(&field, &bed);
        assert!(
            bed[field.at(reach, 8)] <= bed[field.at(3, 8)] - 3.0,
            "front at x {reach}"
        );
        let rock: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                LavaEvent::Rock { x, rock, .. } => Some((*x, *rock)),
                _ => None,
            })
            .collect();
        assert!(rock.iter().all(|&(_, r)| r == "dwell:basalt"));
        assert!(
            rock.iter().any(|&(x, _)| x < reach),
            "basalt behind the front"
        );
        let land = (0..field.w * field.h)
            .map(|i| field.ground[i] - bed[i])
            .fold(0.0, f32::max);
        assert!(land >= 1.0, "new land: {land} m");
    }
);

both_faces!(
    fn lava_meeting_water_turns_to_obsidian_with_steam(f: FaceFrame) {
        let mut field = slope(24, 9, 10.0, 0.2);
        for z in 0..9 {
            let i = field.at(12, z);
            field.water[i] = true;
        }
        field.add_vent(&RIVER).unwrap();
        let events = near(&mut field, 120.0);
        check_cells(f, &field, &events);
        let steam: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                LavaEvent::Steam { x, water_m, .. } => Some((*x, *water_m)),
                _ => None,
            })
            .collect();
        assert!(!steam.is_empty(), "steam where lava met water");
        assert!(steam.iter().all(|&(x, w)| x == 12 && w == 0.5));
        assert!(
            events.iter().any(|e| matches!(
                e,
                LavaEvent::Rock {
                    x: 11 | 12,
                    rock: "dwell:obsidian",
                    ..
                }
            )),
            "obsidian at the quench edge"
        );
        assert!(
            (13..24).all(|x| (0..9).all(|z| field.depth[field.at(x, z)] == 0.0)),
            "none past the water"
        );
    }
);

both_faces!(
    fn a_player_in_lava_loses_health_a_second_and_burns_after(f: FaceFrame) {
        let mut field = Field::new(&DATA, "dwell:lava", 8, 8).unwrap();
        field.origin = [100, -40];
        for i in 0..64 {
            field.ground[i] = 5.0;
            field.depth[i] = 1.0;
            field.heat[i] = 1.0;
        }
        let lava = field.fluid;
        let feet = f.point([103.5, 5.2, -36.5]);
        assert!(field.submerged(f, feet));
        assert!(
            !field.submerged(f, f.point([103.5, 7.0, -36.5])),
            "above the surface"
        );
        assert!(
            !field.submerged(f, f.point([90.5, 5.2, -36.5])),
            "off the field"
        );
        assert!(
            !field.submerged(f.other(), feet),
            "the other face's frame reads it elsewhere"
        );
        let mut burn = Burn::default();
        let dt = 1.0 / 60.0;
        let second = |burn: &mut Burn, in_lava: bool| {
            (0..60).map(|_| burn.step(lava, in_lava, dt)).sum::<i32>()
        };
        let a = second(&mut burn, field.submerged(f, feet));
        assert!(
            (a - lava.damage_per_second as i32).abs() <= 1,
            "{a} a second in it"
        );
        let mut after = 0;
        let mut seconds = 0;
        while burn.burning() {
            after += second(&mut burn, false);
            seconds += 1;
        }
        assert_eq!(
            seconds,
            lava.burn_seconds.ceil() as i32,
            "burns for its burnSeconds"
        );
        let want = (lava.burn_seconds * lava.burn_damage_per_second) as i32;
        assert!(
            (after - want).abs() <= 1,
            "{after} after leaving, want {want}"
        );
        assert_eq!(second(&mut burn, false), 0, "and then no more");
    }
);

both_faces!(
    fn a_field_saves_and_loads(f: FaceFrame) {
        let mut field = slope(20, 6, 10.0, 0.2);
        field.origin = [-7, 300];
        field.add_vent(&RIVER).unwrap();
        let sea = field.at(19, 0);
        field.water[sea] = true;
        near(&mut field, 20.0);
        field.advance(3.0, false);
        let rec = record_of(&field);
        let mut back = field_of(&rec, &DATA).expect("reads its own record");
        assert_eq!(
            (
                &back.ground,
                &back.depth,
                &back.heat,
                &back.water,
                &back.vents,
                back.origin,
                back.owed_s
            ),
            (
                &field.ground,
                &field.depth,
                &field.heat,
                &field.water,
                &field.vents,
                field.origin,
                field.owed_s
            )
        );
        near(&mut field, 5.0);
        near(&mut back, 5.0);
        assert_eq!(back.depth, field.depth, "and runs on alike");
        let feet = f.point([-7.0 + 2.5, 10.0 - 0.4 + 0.1, 304.5]);
        assert_eq!(back.submerged(f, feet), field.submerged(f, feet));
        let mut v2 = rec.clone();
        v2[0] = 2;
        assert!(field_of(&v2, &DATA).is_none(), "a newer version");
        assert!(field_of(&rec[..rec.len() - 1], &DATA).is_none(), "damaged");
        assert!(
            field_of(&[rec.clone(), vec![0]].concat(), &DATA).is_none(),
            "trailing bytes"
        );
        assert!(
            field_of(&rec, &FIXTURE).is_none(),
            "a fluid the data doesn't have"
        );
    }
);

both_faces!(
    fn a_fluid_added_by_data_alone_runs(f: FaceFrame) {
        let magma = FIXTURE.fluid("test:magma").expect("fixture fluid");
        let vent = VentDef {
            fluid: "test:magma",
            ..RIVER
        };
        let mut thin = Field::new(&FIXTURE, "test:magma", 64, 9).unwrap();
        let mut thick = slope(64, 9, 30.0, 0.1);
        thin.ground.clone_from(&thick.ground);
        thin.add_vent(&vent).unwrap();
        thick.add_vent(&RIVER).unwrap();
        let bed = thick.ground.clone();
        let events = near(&mut thin, 30.0);
        check_cells(f, &thin, &events);
        near(&mut thick, 30.0);
        assert!(
            front(&thin, &bed) > front(&thick, &bed),
            "less viscous runs further"
        );
        assert_eq!(magma.light, 15);
        assert!(
            events
                .iter()
                .all(|e| !matches!(e, LavaEvent::Rock { rock, .. } if *rock != "dwell:obsidian"))
        );
    }
);

both_faces!(
    fn bad_data_and_vents_fail_with_messages(f: FaceFrame) {
        assert_eq!(DATA.validate(), Ok(()));
        assert_eq!(FIXTURE.validate(), Ok(()));
        let mut lava = DATA.fluids[0];
        lava.rock = "dwell:nope";
        let bad = LavaData {
            fluids: std::slice::from_ref(Box::leak(Box::new(lava))),
            ..DATA
        };
        assert_eq!(
            bad.validate(),
            Err("lava fluid dwell:lava: rock \"dwell:nope\" is not a block of the registry".into())
        );
        assert_eq!(
            Field::new(&DATA, "dwell:tar", 2, 2).err(),
            Some("no hot fluid named \"dwell:tar\"".into())
        );
        let mut field = slope(4, 4, 1.0, 0.0);
        assert_eq!(
            field.add_vent(&VentDef {
                fluid: "test:magma",
                ..RIVER
            }),
            Err("vent test:here: feeds test:magma, the field holds dwell:lava".into())
        );
        assert_eq!(
            field.add_vent(&VentDef { x: 9, ..RIVER }),
            Err("vent test:here: column (9, 4) is off the 4×4 field".into())
        );
        assert!(
            DATA.fluid("dwell:lava").unwrap().rock_state()
                != DATA.fluid("dwell:lava").unwrap().quench_state()
        );
        let _ = f;
    }
);

both_faces!(
    fn far_fields_fast_forward_close_to_near_ones(f: FaceFrame) {
        let mut a = slope(48, 9, 20.0, 0.2);
        a.add_vent(&RIVER).unwrap();
        let mut b = a.clone();
        let bed = a.ground.clone();
        let player = f.point([4.0, 20.0, 4.0]);
        let away = f.point([4000.0, 20.0, 4.0]);
        assert!(a.is_near(f, &[player]) && !a.is_near(f, &[away]));
        let mut ran = 0.0;
        for _ in 0..900 {
            a.advance(0.1, a.is_near(f, &[player]));
            ran += b.advance(0.1, b.is_near(f, &[away]));
        }
        assert!(
            (ran - 90.0f32).abs() < 1e-2,
            "the far field ran its 90 s in {ran}"
        );
        assert!(b.owed_s.abs() < 1e-3);
        let (sa, sb) = (a.stored() + a.rocked, b.stored() + b.rocked);
        assert!((sa - sb).abs() < 0.02 * sa, "volume {sa} vs {sb}");
        let (fa, fb) = (front(&a, &bed) as i32, front(&b, &bed) as i32);
        assert!((fa - fb).abs() <= 2, "front {fa} vs {fb}");
        let rock = |x: &Field| {
            x.ground
                .iter()
                .zip(&bed)
                .map(|(g, b)| f64::from(g - b))
                .sum::<f64>()
        };
        assert!(
            (rock(&a) - rock(&b)).abs() < 0.1 * rock(&a).max(1.0),
            "rock {} vs {}",
            rock(&a),
            rock(&b)
        );
    }
);

both_faces!(
    // A rock event's rows are cell rows (½ m tall, HALF_HEIGHT_GRID.md §2), not metres: 1 m of lava
    // cooled on a bed at 2 m is rows 4..6.
    fn a_rock_events_rows_are_half_metre_cell_rows(f: FaceFrame) {
        // One column: nowhere to flow.
        let mut field = Field::new(&DATA, "dwell:lava", 1, 1).expect("dwell:lava");
        field.ground[0] = 2.0;
        let i = 0;
        field.depth[i] = 1.0;
        field.heat[i] = 0.001;
        field.step(1.0);
        let rows: Vec<_> = field
            .take_events()
            .into_iter()
            .filter_map(|e| match e {
                LavaEvent::Rock {
                    x: 0, z: 0, rows, ..
                } => Some(rows),
                _ => None,
            })
            .collect();
        assert_eq!(rows, vec![4..6], "one rock event over the two ½ m rows");
        assert_eq!(field.lava_rows(i), 6..6, "no lava left above the rock");
        let cells = LavaEvent::Rock {
            x: 1,
            z: 1,
            rows: 4..6,
            rock: "dwell:basalt",
        }
        .cells(f, [0, 0]);
        assert_eq!(
            cells.iter().map(|&c| f.cell(c)[1]).collect::<Vec<_>>(),
            vec![4, 5]
        );
    }
);
