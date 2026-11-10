//! Soil and vegetation from climate (systemic terrain T3, `shared/worldgen/vegetation.json`): a
//! biome that opts in (`vegetation` > 0) places its trees by each kind's climate envelope, so the
//! tree line emerges from temperature and a dry column grows none; soil is thin on steep hard rock
//! and deep on valley floors. The default biome doesn't opt in: the default world is unchanged.

use dwell_world::*;
use dwell_worldgen::bifacial::BifacialTerrain;
use dwell_worldgen::terrain::{BIOMES, Biome, PRESET_BIOMES, TerrainGenerator};
use dwell_worldgen::vegetation::{Kind, VEGETATION, Vegetation};

fn preset(name: &str) -> Biome {
    *PRESET_BIOMES.iter().find(|b| b.name == name).unwrap()
}

/// The default biome and the boreal preset, both with the climate's share `s`.
fn world(f: FaceFrame, s: f64) -> TerrainGenerator {
    let t = BifacialTerrain::height_field(7);
    let a = Biome {
        vegetation: s,
        ..BIOMES[0]
    };
    let b = Biome {
        vegetation: s,
        ..preset("preset:boreal")
    };
    t.local(f.face).clone().with_biomes(&[a, b])
}

/// The build's vegetation with its one kind's envelope replaced (a fixture by data alone).
fn with_kind(k: Kind) -> Vegetation {
    Vegetation {
        kinds: Box::leak(Box::new([k])),
        ..VEGETATION
    }
}

fn pines(temperature: [f64; 2], moisture: [f64; 2]) -> Kind {
    Kind {
        temperature,
        moisture,
        fade: [0.0, 0.0, 0.0],
        min_soil: 0.0,
        max_slope_deg: 90.0,
        ..VEGETATION.kinds[0]
    }
}

/// Square land patches of 96 m over the island (centres every `step` m), their trees.
fn trees_over(g: &TerrainGenerator, step: usize) -> Vec<[i32; 3]> {
    let r = WORLD_RADIUS * 7 / 10;
    let mut out = Vec::new();
    for x in (-r..=r).step_by(step) {
        for z in (-r..=r).step_by(step) {
            if i64::from(x).pow(2) + i64::from(z).pow(2) > i64::from(r).pow(2) {
                continue;
            }
            for t in g.trees_in((x, z), (x + 95, z + 95)) {
                let a = t.anchor;
                if (x..=x + 95).contains(&a[0]) && (z..=z + 95).contains(&a[2]) {
                    out.push(a);
                }
            }
        }
    }
    out
}

dwell_world::both_faces!(
    /// With the climate's share 0 a fixture envelope that admits nothing changes no tree.
    fn strength_zero_changes_no_tree(f: FaceFrame) {
        let none = with_kind(pines([70.0, 80.0], [0.0, 1.0]));
        let a = world(f, 0.0);
        let b = world(f, 0.0).with_vegetation(none);
        let ta = trees_over(&a, 2400);
        assert!(!ta.is_empty());
        assert_eq!(ta, trees_over(&b, 2400));
    }
);

dwell_world::both_faces!(
    /// A fixture tree line at 11 °C: no tree stands where it is colder (higher), trees stand below it,
    /// and the mean anchor is lower than without the climate.
    fn trees_stop_at_the_tree_line(f: FaceFrame) {
        let line = 11.0;
        let g = world(f, 1.0).with_vegetation(with_kind(pines([line, 80.0], [0.0, 1.0])));
        let free = trees_over(&world(f, 0.0), 1200);
        let kept = trees_over(&g, 1200);
        assert!(kept.len() > 20, "{} trees below the line", kept.len());
        assert!(kept.len() < free.len(), "{} of {}", kept.len(), free.len());
        for a in &kept {
            let c = g.climate_at(a[0].into(), a[2].into()).unwrap();
            assert!(
                c.temperature_c >= line,
                "a tree at {:.1} °C",
                c.temperature_c
            );
        }
        // Above the line: every column that free placement grew a tree in, but colder, has none.
        let cold = free
            .iter()
            .filter(|a| {
                g.climate_at(a[0].into(), a[2].into())
                    .unwrap()
                    .temperature_c
                    < line
            })
            .count();
        assert!(cold > 5, "{cold} trees above the line without the climate");
        let mean = |v: &[[i32; 3]]| v.iter().map(|a| f64::from(a[1])).sum::<f64>() / v.len() as f64;
        assert!(
            mean(&kept) < mean(&free),
            "the tree line lowers the mean anchor"
        );
    }
);

dwell_world::both_faces!(
    /// A fixture moisture threshold: no tree stands in a column drier than it.
    fn dry_columns_grow_no_trees(f: FaceFrame) {
        let wet = 0.3;
        let g = world(f, 1.0).with_vegetation(with_kind(pines([-80.0, 80.0], [wet, 1.0])));
        let kept = trees_over(&g, 1200);
        let free = trees_over(&world(f, 0.0), 1200);
        assert!(
            !kept.is_empty() && kept.len() < free.len(),
            "{} of {}",
            kept.len(),
            free.len()
        );
        for a in &kept {
            let c = g.climate_at(a[0].into(), a[2].into()).unwrap();
            assert!(c.moisture >= wet, "a tree at wetness {:.2}", c.moisture);
        }
    }
);

dwell_world::both_faces!(
    /// Soil is thinner on steep hard rock than on flat, wet valley floors.
    fn soil_is_thin_on_steep_hard_rock(f: FaceFrame) {
        let v = VEGETATION;
        let (steep, _) = v.soil("dwell:granite", 30.0, 0.0);
        let (floor, _) = v.soil("dwell:shale", 2.0, 1.0);
        assert!(
            steep * 5.0 < floor,
            "granite at 30°: {steep} m; a shale floor: {floor} m"
        );
        // On seed 7's land: the mean soil of steep columns under that of flat wet ones.
        let g = world(f, 1.0);
        let (mut s, mut fl) = ((0.0, 0u32), (0.0, 0u32));
        let r = WORLD_RADIUS * 8 / 10;
        for x in (-r..=r).step_by(97) {
            for z in (-r..=r).step_by(97) {
                let Some(c) = g.climate_at(x.into(), z.into()) else {
                    continue;
                };
                if g.column_at(x, z).outside {
                    continue;
                }
                if c.slope_deg > 20.0 {
                    s = (s.0 + c.soil_depth, s.1 + 1);
                } else if c.slope_deg < 4.0 && c.moisture > 0.5 {
                    fl = (fl.0 + c.soil_depth, fl.1 + 1);
                }
            }
        }
        assert!(
            s.1 > 10 && fl.1 > 10,
            "{} steep, {} floor samples",
            s.1,
            fl.1
        );
        let (ms, mf) = (s.0 / f64::from(s.1), fl.0 / f64::from(fl.1));
        assert!(ms < mf * 0.5, "steep {ms:.2} m, floors {mf:.2} m");
    }
);
