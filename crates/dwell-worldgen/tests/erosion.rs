//! Rule-based erosion (systemic terrain T4, `shared/worldgen/rocks.json` `erosion`): neutral by
//! default; a biome that opts in is cut back from the world map's drainage lines into isolated
//! remnants (mesas, buttes) with sheer walls at their edges on a flat floor, not terraces on every
//! contour; the badlands' rills add relief at a short wavelength.

use dwell_world::*;
use dwell_worldgen::bifacial::BifacialTerrain;
use dwell_worldgen::geology::{
    Cut, Erosion, GEOLOGY, Geology, Jointing, Layer, Rock, Stack, eroded,
};
use dwell_worldgen::terrain::{BIOMES, Biome, TerrainGenerator};

/// A fixture mesa stack by data alone: a hard cap over a soft one, 20 m and 30 m.
const CAP: Rock = Rock {
    name: "test:caprock",
    hardness: 0.9,
    jointing: Jointing::Bedded,
    bed: 20.0,
    solubility: 0.0,
    face: "dwell:stone",
    cap: "dwell:stone",
    cap_by: &[],
    scree: "dwell:sand",
    color: 0xaa3322,
};
const SOFT: Rock = Rock {
    name: "test:soft",
    hardness: 0.1,
    jointing: Jointing::Bedded,
    bed: 30.0,
    solubility: 0.0,
    face: "dwell:dirt",
    cap: "dwell:dirt",
    cap_by: &[],
    scree: "dwell:sand",
    color: 0x886655,
};
const ROCKS: &[Rock] = &[CAP, SOFT];
const STACKS: &[Stack] = &[Stack {
    name: "test:mesa",
    weight: 1.0,
    layers: &[
        Layer {
            rock: "test:caprock",
            thickness: 20.0,
        },
        Layer {
            rock: "test:soft",
            thickness: 30.0,
        },
    ],
}];

/// Strong erosion over the fixture stack (the T2 shaping off: the cut-back alone).
fn geology(e: Erosion) -> Geology {
    Geology {
        rocks: ROCKS,
        stacks: STACKS,
        strength: 0.0,
        erosion: e,
        // The erosion engine's own shapes, flat tops and floors: the mesas' relief over them
        // (rocks.json `mesaRelief`, 0.64) is `tests/realistic_mesas.rs`'s.
        mesa_relief: dwell_worldgen::geology::MesaRelief::NONE,
        ..GEOLOGY
    }
}

fn strong() -> Erosion {
    Erosion {
        strength: 1.0,
        ..GEOLOGY.erosion
    }
}

fn biome(erosion: f64, gullies: f64) -> Biome {
    Biome {
        rock_stack: Some("test:mesa"),
        erosion,
        gullies,
        ..BIOMES[0]
    }
}

fn generator(f: FaceFrame, e: Erosion, b: Biome) -> TerrainGenerator {
    let t = BifacialTerrain::height_field(7);
    t.local(f.face)
        .clone()
        .with_geology(geology(e))
        .with_biomes(&[b])
}

const STEP: i64 = 4;
const SIDE: usize = 400;
/// The patch's corner: inland hills with drainage (seed 7).
const X0: i64 = -1600;
const Z0: i64 = 200;

/// The patch's columns every `STEP` m: (height over the sea, floor, margin) or None (water).
fn patch(g: &TerrainGenerator) -> Vec<Option<(f64, f64, f64)>> {
    let mut out = Vec::with_capacity(SIDE * SIDE);
    for j in 0..SIDE {
        for i in 0..SIDE {
            let (x, z) = (X0 + i as i64 * STEP, Z0 + j as i64 * STEP);
            let c = g.column_at(x as i32, z as i32);
            let dry = c.water == dwell_worldgen::terrain::Water::None;
            out.push(match g.erosion_at(x, z) {
                Some((_, m, floor)) if dry => Some((f64::from(c.height) - SEA_LEVEL_M, floor, m)),
                _ if dry => Some((f64::from(c.height) - SEA_LEVEL_M, f64::NAN, f64::NAN)),
                _ => None,
            });
        }
    }
    out
}

/// The connected (4-neighbour) groups of `set`'s cells of at least `least` cells.
fn components(set: &[bool], least: usize) -> usize {
    let mut seen = vec![false; set.len()];
    let mut count = 0;
    for s in 0..set.len() {
        if !set[s] || seen[s] {
            continue;
        }
        let mut stack = vec![s];
        seen[s] = true;
        let mut size = 0;
        while let Some(c) = stack.pop() {
            size += 1;
            let (i, j) = (c % SIDE, c / SIDE);
            let mut push = |n: usize| {
                if set[n] && !seen[n] {
                    seen[n] = true;
                    stack.push(n);
                }
            };
            if i > 0 {
                push(c - 1);
            }
            if i + 1 < SIDE {
                push(c + 1);
            }
            if j > 0 {
                push(c - SIDE);
            }
            if j + 1 < SIDE {
                push(c + SIDE);
            }
        }
        if size >= least {
            count += 1;
        }
    }
    count
}

dwell_world::both_faces!(
    /// The build's erosion is neutral: no build biome opts in, so the default world's columns are
    /// the plain generator's (the goldens pin the chunks), and a biome at erosion 0 is unchanged.
    fn the_default_erosion_changes_nothing(f: FaceFrame) {
        assert!(BIOMES.iter().all(|b| b.erosion == 0.0 && b.gullies == 0.0));
        let plain = generator(f, strong(), biome(0.0, 0.0));
        assert!(plain.erosion_at(0, 0).is_none(), "nothing erodes");
        let t = BifacialTerrain::height_field(7);
        let g = t.local(f.face).clone();
        let off = g.clone().with_biomes(&[Biome {
            erosion: 0.0,
            ..BIOMES[0]
        }]);
        for x in (-4000..4000).step_by(97) {
            assert_eq!(g.column_at(x, 900).height, off.column_at(x, 900).height);
        }
    }
);

#[test]
fn the_profile_is_a_floor_a_talus_skirt_a_wall_and_the_top() {
    // face-a-only: a pure rule over face-local heights; the generator's tests cover both faces.
    let e = strong();
    let (h, floor) = (140.0, 60.0);
    assert_eq!(eroded(&e, h, floor, -1000.0), (floor, Cut::Floor));
    assert_eq!(eroded(&e, h, floor, 1000.0), (h, Cut::Standing));
    let (foot, cut) = eroded(&e, h, floor, 0.0);
    assert_eq!(cut, Cut::Wall);
    assert!((foot - (floor + e.talus_share * 80.0)).abs() < 1e-9);
    // The wall rises the hill's height less the skirt over `wall` m.
    let (top, _) = eroded(&e, h, floor, e.wall - 1e-6);
    assert!(
        top - foot > 4.0 * e.wall,
        "sheer: {} m over {} m",
        top - foot,
        e.wall
    );
    assert_eq!(eroded(&e, h, floor, -0.5 * e.talus_width).1, Cut::Talus);
}

dwell_world::both_faces!(
    /// With the fixture mesa stack and strong erosion the land splits into separate remnants on a
    /// mostly flat floor, with walls at the remnants' edges, and few columns between the levels:
    /// isolated mesas and buttes, not a terrace on every contour.
    fn strong_erosion_leaves_isolated_remnants_on_a_flat_floor(f: FaceFrame) {
        let g = generator(f, strong(), biome(1.0, 0.0));
        let plain = generator(f, strong(), biome(0.0, 0.0));
        let cols = patch(&g);
        let before = patch(&plain);
        let mut land = 0usize;
        let mut on_floor = 0usize;
        let mut between = 0usize;
        let mut standing = vec![false; cols.len()];
        for (k, (c, p)) in cols.iter().zip(&before).enumerate() {
            let (Some((h, floor, m)), Some((h0, _, _))) = (c, p) else {
                continue;
            };
            if !floor.is_finite() || *h0 < 10.0 {
                continue;
            }
            land += 1;
            if (h - floor).abs() < 3.0 {
                on_floor += 1;
            } else if (h - h0).abs() > 3.0 {
                between += 1;
            }
            standing[k] = *m >= GEOLOGY.erosion.wall && (h - floor) > 8.0;
        }
        let remnants = components(&standing, 8);
        let walls = cols
            .windows(2)
            .enumerate()
            .filter(|(k, w)| {
                (k + 1) % SIDE != 0
                    && matches!((w[0], w[1]), (Some(a), Some(b)) if (a.0 - b.0).abs() >= 8.0)
            })
            .count();
        let share = |n: usize| n as f64 / land.max(1) as f64;
        eprintln!(
            "land {land}, floor {:.3}, between {:.3}, remnants {remnants}, walls {walls}",
            share(on_floor),
            share(between)
        );
        assert!(land > 20_000, "enough land: {land}");
        assert!(
            share(on_floor) > 0.5,
            "mostly floor: {:.3}",
            share(on_floor)
        );
        assert!(share(between) < 0.15, "few between: {:.3}", share(between));
        assert!(remnants >= 5, "separate remnants: {remnants}");
        assert!(walls >= 50, "walls (≥ 8 m over 4 m): {walls}");
    }
);

dwell_world::both_faces!(
    /// The badlands' rills: soft beds with `gullies` have more short-wavelength relief than the
    /// same erosion without.
    fn gullies_add_short_relief(f: FaceFrame) {
        let smooth = patch(&generator(f, strong(), biome(1.0, 0.0)));
        let gullied = patch(&generator(f, strong(), biome(1.0, 1.0)));
        // The mean |second difference| along x, over the land.
        let rough = |cols: &[Option<(f64, f64, f64)>]| {
            let (mut s, mut n) = (0.0, 0usize);
            for w in cols.windows(3) {
                if let (Some(a), Some(b), Some(c)) = (w[0], w[1], w[2]) {
                    s += (a.0 - 2.0 * b.0 + c.0).abs();
                    n += 1;
                }
            }
            s / n.max(1) as f64
        };
        let (a, b) = (rough(&smooth), rough(&gullied));
        eprintln!("roughness {a:.3} → {b:.3}");
        assert!(b > 1.5 * a, "gullies roughen the ground: {a:.3} → {b:.3}");
    }
);

dwell_world::both_faces!(
    /// The cut-back follows the data: more reach cuts more of the land down to the floor.
    fn more_reach_cuts_more(f: FaceFrame) {
        let cut = |reach: f64| {
            let g = generator(f, Erosion { reach, ..strong() }, biome(1.0, 0.0));
            let mut n = 0;
            for x in (-1600..0).step_by(16) {
                for z in (200..1800).step_by(16) {
                    if g.erosion_at(x, z).is_some_and(|(_, m, _)| m < 0.0) {
                        n += 1;
                    }
                }
            }
            n
        };
        let (a, b) = (cut(200.0), cut(1200.0));
        assert!(b > a, "{a} → {b}");
    }
);
