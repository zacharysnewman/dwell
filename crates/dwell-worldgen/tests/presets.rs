//! The preset biomes (`shared/worldgen/biomes/presets/*.json`, Phase 57): starting points Chisel
//! offers, never on the default world's map. Each builds a generator beside the default; a world
//! of several places them by its geography (the desert warm and dry, the tundra high and cold).

use dwell_world::*;
use dwell_worldgen::bifacial::BifacialTerrain;
use dwell_worldgen::terrain::{BIOMES, Biome, PRESET_BIOMES};

fn preset(name: &str) -> Biome {
    *PRESET_BIOMES
        .iter()
        .find(|b| b.name == name)
        .unwrap_or_else(|| panic!("the preset {name}"))
}

dwell_world::both_faces!(
    /// Every preset, beside the default, builds a generator whose columns generate.
    fn every_preset_builds_a_generator(f: FaceFrame) {
        assert!(PRESET_BIOMES.len() >= 12, "{} presets", PRESET_BIOMES.len());
        assert_eq!(BIOMES.len(), 1, "the presets are not the default world's");
        let t = BifacialTerrain::height_field(7);
        for p in PRESET_BIOMES {
            let g = t.local(f.face).clone().with_biomes(&[BIOMES[0], *p]);
            for (x, z) in [(0, 0), (2500, -4000), (-6000, 3000)] {
                let m = g.biome_mix(x, z);
                let sum: f64 = m.iter().map(|(_, w)| w).sum();
                assert!((sum - 1.0).abs() < 1e-5, "{}: weights sum to 1", p.name);
                let _ = g.column_at(x as i32, z as i32);
            }
        }
    }
);

dwell_world::both_faces!(
    /// A world of several presets: each appears, and where each wins, the geography is its own:
    /// the desert warmer and drier than the world's land on average, the tundra higher and colder.
    fn several_presets_are_placed_by_geography(f: FaceFrame) {
        let names = [
            "preset:sand_desert",
            "preset:alpine_tundra",
            "preset:mixed_forest",
            "preset:steppe",
            "preset:tropical_beach",
        ];
        let mut list = vec![BIOMES[0]];
        list.extend(names.iter().map(|n| preset(n)));
        let t = BifacialTerrain::height_field(7);
        let g = t.local(f.face).clone().with_biomes(&list);
        let r = i64::from(WORLD_RADIUS) * 9 / 10;
        let mut all = (0.0, 0.0, 0.0, 0u32);
        let mut won = vec![(0.0, 0.0, 0.0, 0u32); list.len()];
        for x in (-r..=r).step_by(256) {
            for z in (-r..=r).step_by(256) {
                if x * x + z * z > r * r {
                    continue;
                }
                let geo = g.biome_geography(x, z).expect("a biome map");
                if geo.coast <= 0.0 {
                    continue;
                }
                let add = |s: &mut (f64, f64, f64, u32)| {
                    *s = (s.0 + geo.temperature, s.1 + geo.wetness, s.2 + geo.elevation, s.3 + 1);
                };
                add(&mut all);
                if let Some((i, w)) = g
                    .biome_mix(x, z)
                    .iter()
                    .max_by(|a, b| a.1.total_cmp(&b.1))
                    && w > 0.6
                {
                    add(&mut won[i]);
                }
            }
        }
        let mean = |s: (f64, f64, f64, u32)| {
            let n = f64::from(s.3.max(1));
            (s.0 / n, s.1 / n, s.2 / n)
        };
        let land = mean(all);
        for (i, s) in won.iter().enumerate().skip(1) {
            assert!(s.3 > 10, "{} wins somewhere: {} columns ({:?})", list[i].name, s.3, f.face);
        }
        let desert = mean(won[1]);
        assert!(
            desert.0 > land.0 && desert.1 < land.1,
            "the desert warm and dry: {desert:?} vs the land's {land:?}"
        );
        let tundra = mean(won[2]);
        assert!(
            tundra.2 > land.2 + 300.0 && tundra.0 < land.0,
            "the tundra high and cold: {tundra:?} vs the land's {land:?}"
        );
        let beach = mean(won[5]);
        assert!(beach.2 < 60.0, "the beach low: {beach:?}");
    }
);
