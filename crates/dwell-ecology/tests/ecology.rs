//! The far ecology (docs/ECOLOGY.md): herds and packs over a seeded island.

use dwell_ecology::{
    Activity, DATA, Data, Ecosystem, Ground, GroundDef, Group, Island, fixture, step,
};

/// A round island 3.6 km across: meadow round the shore, upland toward a 300 m top.
fn island(data: &Data) -> Island {
    let n = 24;
    let x0 = -(n as f64 - 1.0) * data.island.cell_m / 2.0;
    Island::from_ground(data, n, x0, |x, z| {
        if x.hypot(z) > 1800.0 {
            Ground::Water
        } else {
            Ground::Land {
                height_m: 300.0 * (1.0 - x.hypot(z) / 1800.0),
                slope_deg: (300.0f64 / 1800.0).atan().to_degrees(),
            }
        }
    })
}

fn deer() -> usize {
    DATA.find("dwell:deer").expect("deer is in the data")
}

fn wolf() -> usize {
    DATA.find("dwell:wolf").expect("wolf is in the data")
}

/// A group of `species` placed by hand.
fn group(id: u32, species: usize, x: f64, z: f64, size: f64, hunger: f64) -> Group {
    Group {
        id,
        species,
        x,
        z,
        size,
        hunger,
        energy: 1.0,
        activity: Activity::Resting,
    }
}

/// An ecology of hand-placed groups on `island`, every cell's forage full.
fn by_hand(island: &Island, groups: Vec<Group>) -> Ecosystem {
    Ecosystem {
        seed: 3,
        day: 0.0,
        grazing: island.grazing_max.clone(),
        next_id: groups.len() as u32,
        groups,
    }
}

/// Every group's size within its species' floor and cap.
fn assert_groups_in_bounds(eco: &Ecosystem, data: &Data) {
    for g in &eco.groups {
        let r = data.species[g.species].group;
        assert!(
            g.size <= r.cap + 1e-9 && g.size >= r.floor - 1e-9,
            "day {}: group {} of {} has {} head, outside [{}, {}]",
            eco.day,
            g.id,
            data.species[g.species].name,
            g.size,
            r.floor,
            r.cap
        );
        assert!((0.0..=1.0).contains(&g.hunger) && g.energy.is_finite());
    }
}

#[test]
fn a_thousand_days_never_lose_a_species_nor_run_away() {
    // The owner's goals: resistant to complete destruction, and (2026-11-11) no runaway either:
    // each species' island total stays under its carrying capacity from the data.
    let isl = island(&DATA);
    let (k_deer, k_wolf) = (
        isl.carrying_capacity(&DATA, deer()),
        isl.carrying_capacity(&DATA, wolf()),
    );
    assert!(
        k_deer > 100.0 && k_wolf > 4.0,
        "the test island is too poor: {k_deer}, {k_wolf}"
    );
    for seed in [1, 7, 42, 1234] {
        let mut eco = Ecosystem::new(&isl, &DATA, seed);
        let (mut most_deer, mut most_wolves) = (0.0f64, 0.0f64);
        for _ in 0..100 {
            step(&isl, &DATA, &mut eco, 10.0);
            let (d, w) = (eco.population(deer()), eco.population(wolf()));
            assert!(
                d > 0.0 && eco.group_count(deer()) > 0,
                "seed {seed} day {}: no deer",
                eco.day
            );
            assert!(
                w > 0.0 && eco.group_count(wolf()) > 0,
                "seed {seed} day {}: no wolves",
                eco.day
            );
            assert!(
                d <= k_deer,
                "seed {seed} day {}: {d} deer over the capacity {k_deer}",
                eco.day
            );
            assert!(
                w <= k_wolf,
                "seed {seed} day {}: {w} wolves over the capacity {k_wolf}",
                eco.day
            );
            assert_groups_in_bounds(&eco, &DATA);
            most_deer = most_deer.max(d);
            most_wolves = most_wolves.max(w);
        }
        // Quick to replicate: the herds grew well past their start (8 × 6) and split to do it.
        assert!(
            most_deer > 100.0,
            "seed {seed}: the deer never grew ({most_deer})"
        );
        assert!(eco.group_count(deer()) > DATA.species[deer()].start_groups);
        assert!(
            most_wolves > 8.0,
            "seed {seed}: the wolves never grew ({most_wolves})"
        );
    }
}

#[test]
fn a_herd_over_its_cap_splits_in_two() {
    let isl = island(&DATA);
    let cap = DATA.species[deer()].group.cap;
    let mut eco = by_hand(&isl, vec![group(0, deer(), -1500.0, 0.0, cap - 0.05, 0.0)]);
    step(&isl, &DATA, &mut eco, 5.0);
    assert_eq!(eco.group_count(deer()), 2, "{:?}", eco.groups);
    assert_groups_in_bounds(&eco, &DATA);
    assert!(eco.population(deer()) > cap, "the split keeps every head");
}

#[test]
fn at_the_islands_most_groups_a_herd_stops_at_its_cap() {
    let isl = island(&DATA);
    let rules = DATA.species[deer()].group;
    let groups = (0..rules.max_groups as u32)
        .map(|k| {
            group(
                k,
                deer(),
                -1500.0 + 60.0 * f64::from(k),
                0.0,
                rules.cap - 0.05,
                0.0,
            )
        })
        .collect();
    let mut eco = by_hand(&isl, groups);
    step(&isl, &DATA, &mut eco, 5.0);
    assert!(eco.group_count(deer()) <= rules.max_groups);
    assert_groups_in_bounds(&eco, &DATA);
}

#[test]
fn small_herds_near_each_other_merge() {
    let isl = island(&DATA);
    let mut eco = by_hand(
        &isl,
        vec![
            group(0, deer(), -1500.0, 0.0, 2.0, 0.5),
            group(1, deer(), -1450.0, 0.0, 3.0, 0.5),
        ],
    );
    step(&isl, &DATA, &mut eco, 1.0);
    assert_eq!(eco.group_count(deer()), 1, "{:?}", eco.groups);
    assert!(eco.population(deer()) >= 5.0 - 1e-6);
}

/// The build's data on bare ground: no cell holds forage.
const BARE: Data = Data {
    grounds: &[GroundDef {
        name: "bare",
        max_height_m: 100000.0,
        max_slope_deg: 90.0,
        max: 0.0,
    }],
    ..DATA
};

#[test]
fn a_herd_with_no_grazing_starves_slowly_and_keeps_its_floor() {
    let isl = island(&BARE);
    let mut eco = by_hand(&isl, vec![group(0, deer(), 0.0, 0.0, 10.0, 0.0)]);
    step(&isl, &BARE, &mut eco, 1.0);
    assert_eq!(
        eco.population(deer()),
        10.0,
        "a day without food kills none"
    );
    step(&isl, &BARE, &mut eco, 6.0);
    assert!(
        eco.population(deer()) > 9.9,
        "a week: {}",
        eco.population(deer())
    );
    step(&isl, &BARE, &mut eco, 23.0);
    let month = eco.population(deer());
    assert!(
        month > 7.0 && month < 10.0,
        "a month starving loses some, slowly: {month}"
    );
    step(&isl, &BARE, &mut eco, 970.0);
    let floor = DATA.species[deer()].group.floor;
    assert_eq!(
        eco.population(deer()),
        floor,
        "starving to the floor, never below"
    );
}

#[test]
fn grazing_regrows_toward_each_cells_most() {
    let isl = island(&DATA);
    let mut eco = by_hand(&isl, vec![]);
    eco.grazing.iter_mut().for_each(|g| *g = 0.0);
    let mut last = eco.grazing.clone();
    for _ in 0..4 {
        step(&isl, &DATA, &mut eco, 5.0);
        for (c, (&g, &was)) in eco.grazing.iter().zip(&last).enumerate() {
            assert!(g >= was && g <= isl.grazing_max[c] + 1e-9);
        }
        last = eco.grazing.clone();
    }
    // 20 days at 0.05 a day of the gap: 1 − e^−1 of the most.
    let want = 1.0 - (-1.0f64).exp();
    for (c, &g) in eco.grazing.iter().enumerate() {
        assert!(
            (g - isl.grazing_max[c] * want).abs() < 1e-9,
            "cell {c}: {g}"
        );
    }
    assert!(isl.grazing_max.iter().any(|&m| m > 0.0));
}

#[test]
fn grazing_is_depleted_where_a_herd_eats() {
    let isl = island(&DATA);
    let mut eco = by_hand(&isl, vec![group(0, deer(), -1500.0, 0.0, 12.0, 0.6)]);
    let before: f64 = eco.grazing.iter().sum();
    step(&isl, &DATA, &mut eco, 1.0);
    let after: f64 = eco.grazing.iter().sum();
    assert!(after < before - 5.0, "{before} → {after}");
}

#[test]
fn a_hundred_days_at_once_roughly_match_a_hundred_single_days() {
    let isl = island(&DATA);
    for seed in [1, 7] {
        let mut big = Ecosystem::new(&isl, &DATA, seed);
        let mut small = big.clone();
        step(&isl, &DATA, &mut big, 100.0);
        for _ in 0..100 {
            step(&isl, &DATA, &mut small, 1.0);
        }
        assert!((big.day - small.day).abs() < 1e-9);
        for s in [deer(), wolf()] {
            let (b, m) = (big.population(s), small.population(s));
            assert!(
                (b - m).abs() <= 0.15 * m.max(b),
                "seed {seed} {}: {b} at once, {m} by days",
                DATA.species[s].name
            );
        }
        let (gb, gm): (f64, f64) = (big.grazing.iter().sum(), small.grazing.iter().sum());
        assert!((gb - gm).abs() <= 0.15 * gm.max(gb), "grazing {gb} vs {gm}");
    }
}

#[test]
fn the_same_seed_gives_the_same_island_and_another_seed_another() {
    let isl = island(&DATA);
    let run = |seed| {
        let mut e = Ecosystem::new(&isl, &DATA, seed);
        step(&isl, &DATA, &mut e, 300.0);
        e
    };
    let (a, b) = (run(5), run(5));
    assert_eq!(a, b);
    assert_eq!(a.digest(), b.digest());
    assert_ne!(a.digest(), run(6).digest());
}

#[test]
fn deer_flee_a_pack_and_a_hungry_pack_closes_on_a_herd() {
    let isl = island(&DATA);
    let mut eco = by_hand(
        &isl,
        vec![
            group(0, deer(), 0.0, 0.0, 6.0, 0.2),
            group(1, wolf(), 300.0, 0.0, 4.0, 0.5),
        ],
    );
    step(&isl, &DATA, &mut eco, 1.0);
    let (d, w) = (&eco.groups[0], &eco.groups[1]);
    assert_eq!(d.activity, Activity::Fleeing);
    let from_pack = (d.x - 300.0).hypot(d.z);
    assert!(
        from_pack > 400.0,
        "the herd ran from the pack: {from_pack} m off"
    );

    let mut eco = by_hand(
        &isl,
        vec![
            group(0, deer(), -1500.0, 0.0, 6.0, 0.2),
            group(1, wolf(), 1000.0, 0.0, 4.0, 0.5),
        ],
    );
    step(&isl, &DATA, &mut eco, 0.1);
    assert!(
        eco.groups[1].x < 1000.0 - 100.0,
        "the pack moved toward the herd: {}",
        eco.groups[1].x
    );
    assert_eq!(w.species, wolf());
}

#[test]
fn predation_takes_deer_but_never_below_the_herds_floor() {
    let isl = island(&DATA);
    let floor = DATA.species[deer()].group.floor;
    let mut eco = by_hand(
        &isl,
        vec![
            group(0, deer(), 0.0, 0.0, 3.0, 0.9),
            group(1, wolf(), 50.0, 0.0, 7.0, 0.9),
        ],
    );
    step(&isl, &DATA, &mut eco, 1.0);
    let herd = eco.population(deer());
    assert!(herd < 3.0, "the pack ate: {herd}");
    step(&isl, &DATA, &mut eco, 30.0);
    assert!(eco.population(deer()) >= floor - 1e-9);
    assert!(eco.population(wolf()) >= DATA.species[wolf()].group.floor - 1e-9);
}

#[test]
fn a_starving_pack_disperses_toward_the_islands_biggest_herd() {
    let isl = island(&DATA);
    let mut eco = by_hand(
        &isl,
        vec![
            group(0, deer(), -1500.0, 0.0, 10.0, 0.2),
            group(1, wolf(), 1500.0, 0.0, 5.0, 0.95),
        ],
    );
    step(&isl, &DATA, &mut eco, 0.1);
    let pack = &eco.groups[1];
    assert_eq!(pack.activity, Activity::Dispersing);
    assert!(pack.size < 5.0, "members leave");
}

#[test]
fn species_are_added_by_data_alone() {
    let data = fixture::DATA;
    let hare = data.find("test:hare").expect("the fixture's hare");
    let fox = data.find("test:fox").expect("the fixture's fox");
    let isl = Island::from_ground(&data, 16, -750.0, |x, z| {
        if x.hypot(z) > 700.0 {
            Ground::Water
        } else {
            Ground::Land {
                height_m: 10.0,
                slope_deg: 2.0,
            }
        }
    });
    let mut eco = Ecosystem::new(&isl, &data, 9);
    let start = eco.population(hare);
    step(&isl, &data, &mut eco, 300.0);
    assert!(
        eco.population(hare) > start,
        "hares bred: {start} → {}",
        eco.population(hare)
    );
    assert!(eco.population(fox) > 0.0);
    assert!(eco.population(hare) <= isl.carrying_capacity(&data, hare));
    assert_groups_in_bounds(&eco, &data);
}

dwell_world::both_faces!(
    fn a_faces_world_map_island_keeps_its_deer_and_wolves(f: dwell_world::FaceFrame) {
        // The face's own map (each face has its own ecology, face-local).
        let map =
            dwell_worldgen::worldmap::for_seed(dwell_worldgen::bifacial::face_seed(11, f.face));
        let isl = Island::from_world_map(&map, &DATA);
        assert!(isl.walkable.iter().any(|&w| w) && isl.walkable.iter().any(|&w| !w));
        assert!(isl.grazing_max.iter().any(|&g| g > 0.0));
        let mut eco = Ecosystem::new(&isl, &DATA, 11);
        for _ in 0..20 {
            step(&isl, &DATA, &mut eco, 50.0);
            assert!(eco.population(deer()) > 0.0 && eco.population(wolf()) > 0.0);
            assert_groups_in_bounds(&eco, &DATA);
        }
        for g in &eco.groups {
            assert!(isl.is_walkable(g.x, g.z), "group {} is off the land", g.id);
        }
    }
);
