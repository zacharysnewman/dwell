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
        near: Vec::new(),
    }
}

/// Every group's size within its species' `dieBelow` (under it, it is gone) and cap.
fn assert_groups_in_bounds(eco: &Ecosystem, data: &Data) {
    for g in &eco.groups {
        let r = data.species[g.species].group;
        assert!(
            g.size <= r.cap + 1e-9 && g.size >= r.die_below - 1e-9,
            "day {}: group {} of {} has {} head, outside [{}, {}]",
            eco.day,
            g.id,
            data.species[g.species].name,
            g.size,
            r.die_below,
            r.cap
        );
        assert!((0.0..=1.0).contains(&g.hunger) && g.energy.is_finite());
    }
}

#[test]
fn a_thousand_days_keep_both_species_in_most_seeds_and_never_run_away() {
    // The owner's goals: "systemically resistant to being wiped out, not immune from it"
    // (2026-11-11), and no runaway: each species' island total stays under its carrying capacity
    // from the data. With no floor a species may die out, but not in most seeds.
    let isl = island(&DATA);
    let (k_deer, k_wolf) = (
        isl.carrying_capacity(&DATA, deer()),
        isl.carrying_capacity(&DATA, wolf()),
    );
    assert!(
        k_deer > 100.0 && k_wolf > 4.0,
        "the test island is too poor: {k_deer}, {k_wolf}"
    );
    let mut kept = 0;
    let mut report = Vec::new();
    for seed in [1, 7, 42, 1234] {
        let mut eco = Ecosystem::new(&isl, &DATA, seed);
        let (mut most_deer, mut most_wolves) = (0.0f64, 0.0f64);
        for _ in 0..100 {
            step(&isl, &DATA, &mut eco, 10.0);
            let (d, w) = (eco.population(deer()), eco.population(wolf()));
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
        // Quick to replicate: the herds grew well past their start (8 × 6).
        assert!(
            most_deer > 100.0,
            "seed {seed}: the deer never grew ({most_deer})"
        );
        let (d, w) = (eco.population(deer()), eco.population(wolf()));
        report.push(format!("seed {seed}: {d:.0} deer, {w:.1} wolves"));
        if eco.group_count(deer()) > 0 && eco.group_count(wolf()) > 0 {
            kept += 1;
        }
    }
    assert!(
        kept >= 3,
        "both species kept in {kept} of 4 seeds: {report:?}"
    );
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
        refuge: 0.0,
    }],
    ..DATA
};

#[test]
fn a_herd_with_no_grazing_starves_slowly_and_dies_out_in_the_end() {
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
    step(&isl, &BARE, &mut eco, 70.0);
    assert!(eco.group_count(deer()) == 1, "slow: alive after 100 days");
    step(&isl, &BARE, &mut eco, 900.0);
    assert_eq!(eco.group_count(deer()), 0, "no floor: it died out");
    assert_eq!(eco.population(deer()), 0.0);
}

#[test]
fn an_island_whose_grazing_collapsed_loses_its_deer_and_then_its_wolves() {
    // A thriving island, then its forage gone (every cell bare from then on).
    let isl = island(&DATA);
    let mut eco = Ecosystem::new(&isl, &DATA, 7);
    step(&isl, &DATA, &mut eco, 200.0);
    assert!(eco.population(deer()) > 50.0);
    let bare = island(&BARE);
    eco.grazing.iter_mut().for_each(|g| *g = 0.0);
    step(&bare, &BARE, &mut eco, 1500.0);
    assert_eq!(eco.group_count(deer()), 0, "the deer starved out");
    step(&bare, &BARE, &mut eco, 500.0);
    assert_eq!(eco.group_count(wolf()), 0, "then the wolves, with no prey");
}

#[test]
fn wolves_with_no_deer_die_out() {
    let isl = island(&DATA);
    let mut eco = by_hand(&isl, vec![group(0, wolf(), 0.0, 0.0, 6.0, 0.3)]);
    step(&isl, &DATA, &mut eco, 30.0);
    assert_eq!(eco.group_count(wolf()), 1, "not at once: {:?}", eco.groups);
    step(&isl, &DATA, &mut eco, 970.0);
    assert_eq!(eco.group_count(wolf()), 0, "{:?}", eco.groups);
}

#[test]
fn predation_eases_at_low_prey_density_holling_type_iii() {
    let diet = DATA.species[wolf()].diet;
    let dwell_ecology::Diet::Prey {
        half_saturation_head: h,
        ..
    } = diet
    else {
        panic!("the wolf hunts")
    };
    let share = |n: f64| Data::catch_share(diet, n);
    assert_eq!(share(0.0), 0.0);
    assert!(
        (share(h) - 0.5).abs() < 1e-12,
        "half at the half-saturation"
    );
    assert!(share(100.0 * h) > 0.99, "saturates when prey is plentiful");
    // Sigmoid: at low density it rises with the square (doubling the prey quadruples the share),
    // so each prey head's risk (share / N) falls as the prey gets scarce.
    let (a, b) = (share(0.05 * h), share(0.1 * h));
    assert!((b / a - 4.0).abs() < 0.05, "{a} → {b}");
    for n in [1.0, 2.0, 4.0, 8.0] {
        assert!(
            share(n) / n < share(2.0 * n) / (2.0 * n),
            "risk a head at {n}"
        );
    }
    // In the step: the same pack takes a smaller share of a scarce herd than of a big one (the
    // pack first in id order, so it strikes before the herd runs; bare ground and a half-hungry
    // herd, so it neither breeds nor starves over the half day and only the kills count).
    let isl = island(&BARE);
    let taken = |size: f64| {
        let mut eco = by_hand(
            &isl,
            vec![
                group(0, wolf(), 50.0, 0.0, 4.0, 0.5),
                group(1, deer(), 0.0, 0.0, size, 0.5),
            ],
        );
        step(&isl, &BARE, &mut eco, 0.5);
        1.0 - eco.population(deer()) / size
    };
    let (scarce, plenty) = (taken(1.5), taken(12.0));
    assert!(scarce > 0.0 && plenty > 0.0);
    assert!(
        scarce < plenty / 2.0,
        "a scarce herd's share taken {scarce} vs a big one's {plenty}"
    );
}

#[test]
fn a_herd_in_a_refuge_loses_fewer_to_a_pack() {
    // West of x = 0 open ground, east crag (steep: a refuge), both bare (only the kills count).
    const ROCKS: Data = Data {
        grounds: &[
            GroundDef {
                name: "open",
                max_height_m: 100000.0,
                max_slope_deg: 30.0,
                max: 0.0,
                refuge: 0.0,
            },
            GroundDef {
                name: "crag",
                max_height_m: 100000.0,
                max_slope_deg: 90.0,
                max: 0.0,
                refuge: 0.8,
            },
        ],
        ..DATA
    };
    let isl = Island::from_ground(&ROCKS, 24, -2300.0, |x, _| Ground::Land {
        height_m: 50.0,
        slope_deg: if x < 0.0 { 5.0 } else { 60.0 },
    });
    assert!(isl.refuge_at(1000.0, 0.0) > 0.5 && isl.refuge_at(-1000.0, 0.0) == 0.0);
    let lost = |x: f64| {
        let mut eco = by_hand(
            &isl,
            vec![
                group(0, wolf(), x + 50.0, 0.0, 6.0, 0.5),
                group(1, deer(), x, 0.0, 10.0, 0.5),
            ],
        );
        step(&isl, &ROCKS, &mut eco, 0.5);
        10.0 - eco.population(deer())
    };
    let (open, refuge) = (lost(-1000.0), lost(1000.0));
    assert!(open > 0.0);
    assert!(refuge < open * 0.5, "refuge {refuge} vs open {open}");
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
    // Forage is f32 (docs/ECOLOGY.md §6): within its rounding of the closed form.
    for (c, &g) in eco.grazing.iter().enumerate() {
        let max = f64::from(isl.grazing_max[c]);
        assert!(
            (f64::from(g) - max * want).abs() < 1e-6 * max.max(1.0),
            "cell {c}: {g}"
        );
    }
    assert!(isl.grazing_max.iter().any(|&m| m > 0.0));
}

#[test]
fn grazing_is_depleted_where_a_herd_eats() {
    let isl = island(&DATA);
    let mut eco = by_hand(&isl, vec![group(0, deer(), -1500.0, 0.0, 12.0, 0.6)]);
    let before: f64 = eco.grazing.iter().map(|&g| f64::from(g)).sum();
    step(&isl, &DATA, &mut eco, 1.0);
    let after: f64 = eco.grazing.iter().map(|&g| f64::from(g)).sum();
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
        let sum = |e: &Ecosystem| e.grazing.iter().map(|&g| f64::from(g)).sum::<f64>();
        let (gb, gm) = (sum(&big), sum(&small));
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
fn the_far_step_sees_groups_near_players() {
    // A hungry pack 1 km from a herd that is near players (individuals: only its summary here).
    let isl = island(&DATA);
    let mut eco = by_hand(&isl, vec![group(1, wolf(), 1000.0, 0.0, 4.0, 0.5)]);
    eco.near.push(group(0, deer(), 0.0, 0.0, 6.0, 0.2));
    step(&isl, &DATA, &mut eco, 0.1);
    assert!(
        eco.groups[0].x < 1000.0 - 100.0,
        "the pack closes on the near herd: {}",
        eco.groups[0].x
    );
    // Within reach it kills from the near herd's summary (the near side reads the loss back).
    let mut eco = by_hand(&isl, vec![group(1, wolf(), 50.0, 0.0, 7.0, 0.9)]);
    eco.near.push(group(0, deer(), 0.0, 0.0, 3.0, 0.9));
    step(&isl, &DATA, &mut eco, 1.0);
    assert!(eco.near[0].size < 3.0, "the pack ate: {}", eco.near[0].size);
    assert_eq!(eco.near[0].x, 0.0, "the near summary isn't stepped");
    // A far herd flees a pack that is near players.
    let mut eco = by_hand(&isl, vec![group(0, deer(), 0.0, 0.0, 6.0, 0.2)]);
    eco.near.push(group(1, wolf(), 300.0, 0.0, 4.0, 0.5));
    step(&isl, &DATA, &mut eco, 1.0);
    assert_eq!(eco.groups[0].activity, Activity::Fleeing);
    // The near head counts in the totals: an island full of near deer leaves no room to breed.
    let room = isl.carrying_capacity(&DATA, deer());
    let run = |near: f64| {
        let mut eco = by_hand(&isl, vec![group(0, deer(), 0.0, 0.0, 4.0, 0.0)]);
        if near > 0.0 {
            eco.near.push(group(9, deer(), -1500.0, 0.0, near, 0.0));
        }
        step(&isl, &DATA, &mut eco, 5.0);
        eco.groups[0].size
    };
    assert!(run(0.0) > 4.0, "an empty island breeds");
    assert!(
        run(room) <= 4.0 + 1e-9,
        "a full one doesn't: {} (capacity {room})",
        run(room)
    );
}

#[test]
fn predation_takes_deer() {
    let isl = island(&DATA);
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
    assert_groups_in_bounds(&eco, &DATA);
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
        assert!(
            isl.cells() > 0 && isl.cells() < isl.n * isl.n,
            "land and water"
        );
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

#[test]
fn an_ecosystem_round_trips_through_its_bytes() {
    let isl = island(&DATA);
    let mut eco = Ecosystem::new(&isl, &DATA, 5);
    step(&isl, &DATA, &mut eco, 120.0);
    let bytes = eco.to_bytes(&isl, &DATA);
    let back = Ecosystem::from_bytes(&bytes, &isl, &DATA).expect("reads its own bytes");
    assert_eq!(back, eco);
    assert_eq!(back.digest(), eco.digest());
    // Cut short, or another version: not read.
    assert_eq!(
        Ecosystem::from_bytes(&bytes[..bytes.len() - 1], &isl, &DATA),
        None
    );
    let mut other = bytes.clone();
    other[0] = 99;
    assert_eq!(Ecosystem::from_bytes(&other, &isl, &DATA), None);
}

#[test]
fn saved_species_are_mapped_by_name_and_unknown_ones_dropped() {
    // Saved under the build's data (deer, wolf); read by data that knows only the deer, at another
    // index: the deer kept, the wolves dropped. A grid of another size starts full.
    let isl = island(&DATA);
    let mut eco = Ecosystem::new(&isl, &DATA, 5);
    step(&isl, &DATA, &mut eco, 20.0);
    let bytes = eco.to_bytes(&isl, &DATA);
    const ONLY_DEER: Data = Data {
        species: &[
            dwell_ecology::SpeciesDef {
                name: "test:elk",
                ..DATA.species[0]
            },
            DATA.species[0],
        ],
        ..DATA
    };
    let small = Island::from_ground(&DATA, 4, 0.0, |_, _| Ground::Land {
        height_m: 10.0,
        slope_deg: 1.0,
    });
    let back = Ecosystem::from_bytes(&bytes, &small, &ONLY_DEER).expect("reads");
    assert_eq!(back.grazing, small.grazing_max, "another grid: full");
    assert_eq!(
        back.group_count(1),
        eco.group_count(deer()),
        "deer at their new index"
    );
    assert_eq!(back.group_count(0), 0, "no elk were saved");
    assert_eq!(
        back.groups.len(),
        eco.group_count(deer()),
        "the wolves dropped"
    );
}

/// The far forage and the near tall grass agree (docs/ECOLOGY.md §10): scaled by the grass's
/// climate share, a cell's most forage over its ground's most is the near cap over `maxDensity`,
/// and its forage regrows toward that, so a dry cell's stays sparse.
#[test]
fn the_far_forage_regrows_toward_the_same_climate_cap_as_the_near_grass() {
    use dwell_worldgen::grass::{Rule, far_share};
    let unscaled = island(&DATA);
    let mut isl = unscaled.clone();
    // A climate from dry in the west (0) to lush in the east (1).
    let env = |x: i32, _z: i32| (0.5f32, ((f64::from(x) + 1800.0) / 3600.0).clamp(0.0, 1.0));
    isl.scale_grazing(|x, z| far_share(&env, x, z));
    let rule = Rule::build(1);
    for c in 0..isl.cells() {
        if unscaled.grazing_max[c] == 0.0 {
            continue;
        }
        let (x, z) = isl.centre(c);
        let share = env(x.floor() as i32, z.floor() as i32).1;
        let far = f64::from(isl.grazing_max[c]) / f64::from(unscaled.grazing_max[c]);
        let near = rule.cap(share) / rule.data.max_density;
        // The most forage is f32 (docs/ECOLOGY.md §6): within its rounding.
        assert!(
            (far - near).abs() < 1e-6,
            "cell {c}: far {far}, near {near}"
        );
    }
    let mut eco = by_hand(&isl, vec![]);
    eco.grazing.iter_mut().for_each(|g| *g = 0.0);
    step(&isl, &DATA, &mut eco, 200.0);
    for (c, &g) in eco.grazing.iter().enumerate() {
        assert!(
            g <= isl.grazing_max[c] + 1e-9,
            "cell {c} regrew past its climate's most"
        );
        let (x, _) = isl.centre(c);
        if x < -1500.0 {
            assert!(
                g < 0.1 * unscaled.grazing_max[c].max(1e-9),
                "a dry cell stays sparse: {g}"
            );
        }
    }
}

/// Version 1's bytes (before the land-only island): the whole `n` × `n` grid's forage as f64,
/// water cells 0, then the species and groups as version 2 writes them.
fn version_1_bytes(eco: &Ecosystem, isl: &Island, data: &Data) -> Vec<u8> {
    let mut grid = vec![0.0f64; isl.n * isl.n];
    for (c, k) in isl.grid_indices().enumerate() {
        grid[k] = f64::from(eco.grazing[c]);
    }
    let mut b = vec![1u8];
    b.extend(eco.seed.to_le_bytes());
    b.extend(eco.day.to_bits().to_le_bytes());
    b.extend(eco.next_id.to_le_bytes());
    b.extend((grid.len() as u32).to_le_bytes());
    for g in grid {
        b.extend(g.to_bits().to_le_bytes());
    }
    b.extend((data.species.len() as u16).to_le_bytes());
    for s in data.species {
        b.extend((s.name.len() as u16).to_le_bytes());
        b.extend(s.name.as_bytes());
    }
    b.extend((eco.groups.len() as u32).to_le_bytes());
    for g in &eco.groups {
        b.extend(g.id.to_le_bytes());
        b.extend((g.species as u16).to_le_bytes());
        for v in [g.x, g.z, g.size, g.hunger, g.energy] {
            b.extend(v.to_bits().to_le_bytes());
        }
        b.push(match g.activity {
            Activity::Resting => 0,
            Activity::Grazing => 1,
            Activity::Moving => 2,
            Activity::Fleeing => 3,
            Activity::Hunting => 4,
            Activity::Roaming => 5,
            Activity::Dispersing => 6,
        });
    }
    b
}

#[test]
fn an_older_version_1_record_still_loads_its_land_cells_forage() {
    let isl = island(&DATA);
    let mut eco = Ecosystem::new(&isl, &DATA, 5);
    step(&isl, &DATA, &mut eco, 120.0);
    assert!(
        eco.grazing
            .iter()
            .zip(&isl.grazing_max)
            .any(|(g, m)| g != m),
        "some forage was grazed"
    );
    let v1 = version_1_bytes(&eco, &isl, &DATA);
    let back = Ecosystem::from_bytes(&v1, &isl, &DATA).expect("reads a version 1 record");
    assert_eq!(back, eco, "the land's forage and the groups as they were");
    // And written back as version 2, smaller.
    let v2 = back.to_bytes(&isl, &DATA);
    assert_eq!(v2[0], 2);
    assert!(v2.len() < v1.len(), "{} vs {}", v2.len(), v1.len());
}

#[test]
fn the_land_only_island_matches_its_whole_grid() {
    // Every grid cell: land exactly where the sampler said, each land cell's index found back from
    // its centre, in row-major order.
    let isl = island(&DATA);
    let land = |x: f64, z: f64| x.hypot(z) <= 1800.0;
    let mut next = 0;
    for j in 0..isl.n {
        for i in 0..isl.n {
            let (x, z) = isl.grid_centre(i, j);
            let c = isl.index(i, j);
            assert_eq!(c.is_some(), land(x, z), "cell ({i}, {j})");
            assert_eq!(isl.cell_at(x + 40.0, z - 40.0), c);
            if let Some(c) = c {
                assert_eq!(c, next, "row-major");
                assert_eq!(isl.centre(c), (x, z));
                assert_eq!(isl.grid_of(c), (i, j));
                next += 1;
            }
        }
    }
    assert_eq!(next, isl.cells());
    assert!(isl.cells() < isl.n * isl.n, "the sea is not stored");
    assert_eq!(isl.cell_at(-1.0e6, 0.0), None);
    assert_eq!(isl.index(isl.n, 0), None);
}

#[test]
fn a_records_forage_runs_round_trip_whatever_their_pattern() {
    let isl = island(&DATA);
    let mut eco = by_hand(&isl, vec![group(0, deer(), 0.0, 0.0, 5.0, 0.2)]);
    // Every third cell grazed, a long bare stretch, the rest full.
    for c in 0..eco.grazing.len() {
        if c % 3 == 0 || (40..90).contains(&c) {
            eco.grazing[c] *= 0.25;
        }
    }
    let bytes = eco.to_bytes(&isl, &DATA);
    assert_eq!(
        Ecosystem::from_bytes(&bytes, &isl, &DATA),
        Some(eco.clone())
    );
    // A cut-short forage section is not read.
    let fresh = by_hand(&isl, vec![]).to_bytes(&isl, &DATA);
    for cut in [25, 26, 27, fresh.len() - 1] {
        assert_eq!(Ecosystem::from_bytes(&fresh[..cut], &isl, &DATA), None);
    }
}

dwell_world::both_faces!(
    fn a_seed_1_worlds_ecology_is_small_in_memory_and_saved(f: dwell_world::FaceFrame) {
        // Measured on a seed-1 world (docs/ECOLOGY.md §6, "Cost"): each face's island and
        // ecosystem; both faces together under 3 MB (12.5 MB before the land-only island).
        let map =
            dwell_worldgen::worldmap::for_seed(dwell_worldgen::bifacial::face_seed(1, f.face));
        let isl = Island::from_world_map(&map, &DATA);
        let mut eco = Ecosystem::new(&isl, &DATA, 1);
        let fresh = eco.to_bytes(&isl, &DATA).len();
        step(&isl, &DATA, &mut eco, 200.0);
        let bytes = isl.heap_bytes() + eco.heap_bytes();
        println!(
            "face {:?}: {} of {} cells land, {bytes} bytes; a fresh record {fresh} bytes",
            f.face,
            isl.cells(),
            isl.n * isl.n
        );
        assert!(
            2 * bytes <= 3 << 20,
            "both faces' ecology under 3 MB: {bytes} bytes a face"
        );
        assert!(fresh <= 100 << 10, "a fresh face's record: {fresh} bytes");
        // Grazed bare, its herds gone, and regrown: every cell back at its most exactly, so the
        // record is as small as a fresh one's.
        eco.groups.clear();
        eco.grazing.iter_mut().for_each(|g| *g = 0.0);
        step(&isl, &DATA, &mut eco, 2000.0);
        assert_eq!(eco.grazing, isl.grazing_max, "regrown to the most exactly");
        let regrown = eco.to_bytes(&isl, &DATA).len();
        assert!(
            regrown <= 100 << 10,
            "a regrown face's record: {regrown} bytes"
        );
        let back = Ecosystem::from_bytes(&eco.to_bytes(&isl, &DATA), &isl, &DATA);
        assert_eq!(back, Some(eco));
    }
);
