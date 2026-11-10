//! Rock types and their shaping (systemic terrain T2, `shared/worldgen/rocks.json`): neutral by
//! default; a hard-over-soft stack steps the ground into caprock terraces with walls at the cap's
//! edge and talus below; a massive rock rises unstepped.

use dwell_world::*;
use dwell_worldgen::bifacial::BifacialTerrain;
use dwell_worldgen::geology::{GEOLOGY, Geology, Jointing, Layer, Part, Profile, Rock, Stack};
use dwell_worldgen::terrain::{BIOMES, Biome, TerrainGenerator};

/// A fixture rock and stack by data alone: a hard cap over a soft one, 20 m and 30 m.
const CAP: Rock = Rock {
    name: "test:caprock",
    hardness: 0.9,
    jointing: Jointing::Bedded,
    bed: 20.0,
    solubility: 0.0,
    face: "dwell:stone",
    cap: "dwell:stone",
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
    scree: "dwell:sand",
    color: 0x886655,
};
const DOME: Rock = Rock {
    name: "test:dome",
    hardness: 0.9,
    jointing: Jointing::Massive,
    bed: 50.0,
    solubility: 0.0,
    face: "dwell:stone",
    cap: "dwell:stone",
    scree: "dwell:stone",
    color: 0xbbbbbb,
};
const ROCKS: &[Rock] = &[CAP, SOFT, DOME];
const STACKS: &[Stack] = &[
    Stack {
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
    },
    Stack {
        name: "test:dome",
        weight: 1.0,
        layers: &[Layer {
            rock: "test:dome",
            thickness: 50.0,
        }],
    },
];

fn geology(strength: f64) -> Geology {
    Geology {
        rocks: ROCKS,
        stacks: STACKS,
        strength,
        ..GEOLOGY
    }
}

fn biome(stack: &'static str) -> Biome {
    Biome {
        rock_stack: Some(stack),
        ..BIOMES[0]
    }
}

/// The land's columns' heights (rows) over a patch, and their tops' states, every 2 m.
fn heights(g: &TerrainGenerator) -> Vec<(i32, i32, f32, dwell_blocks::StateId)> {
    let mut out = Vec::new();
    for x in (-3000..3000).step_by(2) {
        let z = 1500;
        let c = g.column_at(x, z);
        if c.water == dwell_worldgen::terrain::Water::None && c.height > 70.0 + SEA_LEVEL_M as f32 {
            out.push((x, z, c.height, g.top_state(&c)));
        }
    }
    out
}

dwell_world::both_faces!(
    /// The build's geology is neutral: the default world's columns are the plain generator's
    /// (the goldens pin the chunks).
    fn the_default_geology_changes_nothing(f: FaceFrame) {
        assert_eq!(GEOLOGY.strength, 0.0, "rocks.json ships neutral");
        let t = BifacialTerrain::height_field(7);
        let g = t.local(f.face).clone();
        let shaped = g.clone().with_biomes(&[biome("dwell:red_beds")]);
        for x in (-4000..4000).step_by(97) {
            let (a, b) = (g.column_at(x, 900), shaped.column_at(x, 900));
            assert_eq!(a.height, b.height);
            assert_eq!(g.top_state(&a), shaped.top_state(&b));
        }
    }
);

#[test]
fn a_hard_over_soft_profile_has_walls_treads_and_talus() {
    // face-a-only: the profile maps a face-local height; the generator's tests below cover both.
    let p = Profile::of(&geology(1.0), &STACKS[0]);
    assert_eq!(p.period(), 50.0);
    // Sample the period finely: the cap's wall takes 20 m of rise in little input; its tread is
    // flat; the soft rock rises over more input than its thickness's share would give a wall.
    let mut wall_in = 0.0;
    let mut tread_in = 0.0;
    let mut talus_in = 0.0;
    let n = 50_000;
    for i in 0..n {
        let h = 100.0 + 50.0 * i as f64 / n as f64;
        let (_, rock, part) = p.map(h);
        let du = 50.0 / n as f64;
        match part {
            Part::Wall => {
                assert_eq!(ROCKS[rock].name, "test:caprock");
                wall_in += du;
            }
            Part::Tread => tread_in += du,
            Part::Talus => {
                assert_eq!(ROCKS[rock].name, "test:soft");
                talus_in += du;
            }
            Part::Plain => panic!("no massive rock in the stack"),
        }
    }
    // The wall rises 20 m over under 1 m of input: near-vertical on a slope of 1 in 20.
    assert!(wall_in < 1.0, "wall {wall_in}");
    assert!(tread_in > 15.0, "tread {tread_in}");
    assert!(talus_in > 15.0, "talus {talus_in}");
    // Monotone and continuous over a period boundary.
    let mut last = p.map(99.0).0;
    for i in 0..1000 {
        let v = p.map(99.0 + 0.11 * i as f64).0;
        assert!(v >= last - 1e-9, "monotone");
        assert!(v - last < 20.0 + 1e-6, "continuous to a wall's height");
        last = v;
    }
}

dwell_world::both_faces!(
    /// With the fixture hard-over-soft stack at full strength the ground steps: its heights
    /// gather on the caps' levels (flat tops), the cap's edge is a near-vertical wall of its face
    /// block, and the soft rock below shows its scree.
    fn a_hard_over_soft_stack_steps_into_terraces(f: FaceFrame) {
        let t = BifacialTerrain::height_field(7);
        let plain = t
            .local(f.face)
            .clone()
            .with_geology(geology(1.0))
            .with_biomes(&[BIOMES[0]]);
        let g = plain.clone().with_biomes(&[biome("test:mesa")]);
        // The share of land within 1 m of a level `off` m over a multiple of 50 m (the caps'
        // levels at 0, mid-talus at 25).
        let share = |g: &TerrainGenerator, off: f64| {
            let hs = heights(g);
            let near = hs
                .iter()
                .filter(|(_, _, h, _)| {
                    let a = f64::from(*h) - SEA_LEVEL_M - off;
                    (a - 50.0 * (a / 50.0).round()).abs() < 1.0
                })
                .count();
            (near as f64 / hs.len().max(1) as f64, hs.len())
        };
        let (caps, n) = share(&g, 0.0);
        let (mid, _) = share(&g, 25.0);
        let (plain_mid, _) = share(&plain, 25.0);
        assert!(n > 200, "enough land sampled: {n}");
        assert!(
            caps > 0.2 && caps > 5.0 * mid,
            "the caps' levels gather the ground: {caps:.3} on the caps, {mid:.3} mid-slope"
        );
        assert!(
            mid < plain_mid,
            "the slopes between are steepened: {mid:.3} vs {plain_mid:.3}"
        );
        // Walls: a step of ≥ 4 m between columns 2 m apart, its top the cap's face block.
        let hs = heights(&g);
        let stone = dwell_blocks::require("dwell:stone");
        let sand = dwell_blocks::require("dwell:sand");
        let walls = hs
            .windows(2)
            .filter(|w| (w[0].2 - w[1].2).abs() >= 4.0 && w[0].0 + 2 == w[1].0)
            .count();
        assert!(
            walls >= 3,
            "near-vertical walls at the caps' edges: {walls}"
        );
        assert!(
            hs.iter().any(|c| c.3 == sand),
            "the soft rock's talus shows its scree"
        );
        assert!(hs.iter().any(|c| c.3 == stone), "walls and caps show stone");
    }
);

dwell_world::both_faces!(
    /// A massive rock rounds instead of stepping: its ground gathers on no levels.
    fn a_massive_rock_does_not_step(f: FaceFrame) {
        let t = BifacialTerrain::height_field(7);
        let g = t
            .local(f.face)
            .clone()
            .with_geology(geology(1.0))
            .with_biomes(&[biome("test:dome")]);
        let plain = t.local(f.face).clone();
        let a = heights(&g);
        let b = heights(&plain);
        assert_eq!(a.len(), b.len());
        for (p, q) in a.iter().zip(&b) {
            assert!((p.2 - q.2).abs() < 1e-3, "unstepped: {} vs {}", p.2, q.2);
        }
    }
);
