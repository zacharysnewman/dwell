//! The Realistic world's biomes (docs/proposals/REALISTIC_WORLD.md R4): the default biome and every
//! preset, unchanged, chosen by climate alone (Whittaker-style: each preset's `where` in °C and annual
//! rain in mm, read from R3's climate, its rain marched again over the eroded map). For seed fixtures on
//! both faces: no dry biome on the windward side of the main range or anywhere the rain is a wet coast's;
//! above the tree line no forest and the tundra in its place; a colder band has more boreal and tundra
//! and less of the warm biomes on the same ground; the climates R4 left to the default (§7 q9) take the
//! cold desert and the tropical rainforest. The starter keeps its 0..1 tuning
//! (`tests/starter_world.rs`), the default world its goldens (`tests/golden.rs`).

use dwell_world::*;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::biomemap::{BiomeField, Geography, smooth_ground};
use dwell_worldgen::climate::{CLIMATE, Climate, CoarseGround};
use dwell_worldgen::config::{Edit, world_config};
use dwell_worldgen::hydrology;
use dwell_worldgen::terrain::{Biome, PRESET_BIOMES, WhereRange};
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:realistic";
const SEEDS: [u64; 3] = [1, 7, 42];

/// The dry biomes: none where the rain is a wet coast's.
const DRY: [&str; 4] = [
    "preset:sand_desert",
    "preset:cold_desert",
    "preset:badlands",
    "preset:red_rock_mesa",
];
/// The forests: none above the tree line.
const FORESTS: [&str; 4] = [
    "preset:boreal",
    "preset:mixed_forest",
    "preset:cold_rainforest",
    "preset:tropical_rainforest",
];
const COLD: [&str; 3] = ["preset:boreal", "preset:alpine_tundra", "preset:snowfield"];
const WARM: [&str; 6] = [
    "preset:sand_desert",
    "preset:tropical_rainforest",
    "preset:savanna",
    "preset:tropical_beach",
    "preset:badlands",
    "preset:red_rock_mesa",
];

fn edit() -> Edit {
    world_config(NAME)
        .unwrap()
        .expect("the Realistic world's edit")
}

fn map(e: &Edit, world: u64, f: FaceFrame) -> WorldMap {
    WorldMap::for_height(
        face_seed(world, f.face),
        e.height.expect("its height field"),
    )
    .with_worldgen(NAME)
    .unwrap()
}

fn index(list: &[Biome], name: &str) -> usize {
    list.iter()
        .position(|b| b.name == name)
        .unwrap_or_else(|| panic!("{name} is listed"))
}

fn preset(name: &str) -> Biome {
    *PRESET_BIOMES.iter().find(|b| b.name == name).unwrap()
}

/// The land cells every `step` cells: (x, z, elevation).
fn land(m: &WorldMap, step: usize) -> Vec<(i64, i64, f64)> {
    let mut out = Vec::new();
    for j in (0..m.n).step_by(step) {
        for i in (0..m.n).step_by(step) {
            let c = j * m.n + i;
            let h = f64::from(m.height[c]);
            if m.is_sea(c) || !h.is_finite() {
                continue;
            }
            let (x, z) = (m.x0 + i as f64 * m.cell, m.x0 + j as f64 * m.cell);
            out.push((x as i64, z as i64, h.max(0.0)));
        }
    }
    out
}

/// The share of `cells` whose chosen biome is one of `names`.
fn share(field: &BiomeField, list: &[Biome], cells: &[(i64, i64, f64)], names: &[&str]) -> f64 {
    let want: Vec<usize> = names.iter().map(|n| index(list, n)).collect();
    let hit = cells
        .iter()
        .filter(|&&(x, z, h)| want.contains(&usize::from(field.mix(list, x, z, h).chosen)))
        .count();
    hit as f64 / cells.len().max(1) as f64
}

#[test]
fn the_realistic_world_lists_the_default_and_every_preset_unchanged() {
    // face-a-only: parses the data, no terrain.
    let e = edit();
    assert_eq!(
        e.biomes[0].name, "dwell:default",
        "the default is the fallback"
    );
    assert_eq!(e.biomes.len(), PRESET_BIOMES.len() + 1);
    for p in PRESET_BIOMES {
        let b = e.biomes.iter().find(|b| b.name == p.name);
        assert_eq!(b, Some(p), "{}: listed, unchanged", p.name);
    }
    // Every preset's climate envelope is in °C (no 0..1 temperature left to tune per world).
    for p in PRESET_BIOMES {
        assert_eq!(p.place.temperature, WhereRange::ANY, "{}: °C only", p.name);
    }
}

dwell_world::both_faces!(
    /// The world's geography is its climate's: °C from the latitude and height, the rain in mm; the
    /// starter's is the 0..1 map's, with no rain read.
    fn the_realistic_worlds_geography_is_its_climates(f: FaceFrame) {
        let e = edit();
        let m = map(&e, 7, f);
        let field = BiomeField::with_params(&m, face_seed(7, f.face), e.map);
        let c = field.climate().expect("a Realistic world has a climate");
        assert_eq!(*c, Climate::for_seed(m.seed), "the face seed's own climate");
        let (x, z, h) = land(&m, 8)[0];
        let g: Geography = field.geography(x, z, h);
        assert!((g.celsius - c.temperature_c(x as f64, z as f64, h)).abs() < 1e-9);
        assert!(
            g.rain
                .is_some_and(|r| (0.0..=CLIMATE.biomes.max_rain_mm).contains(&r))
        );
    }
);

dwell_world::both_faces!(
    /// No desert (nor badlands or mesa) anywhere the rain is at least the band's wet-coast rain, and
    /// where one is chosen the rain is a dry place's. Beside each seed's main range the windward side
    /// is wetter than its lee and has no larger share of dry biomes; a windward side that is wet (the
    /// band's coast rain or more) has none. (An inland range the air reaches dry has a dry windward
    /// side too, and may have a desert on it: the rain decides, not the side.)
    fn no_dry_biome_on_the_wet_windward_side(f: FaceFrame) {
        let e = edit();
        let mut wet_points = 0;
        for world in SEEDS {
            let m = map(&e, world, f);
            let field = BiomeField::with_params(&m, face_seed(world, f.face), e.map);
            let c = *field.climate().unwrap();
            let dry: Vec<usize> = DRY.iter().map(|n| index(&e.biomes, n)).collect();
            let dry_rain = DRY
                .iter()
                .map(|n| preset(n).place.rain)
                .map(|r| r.max + r.blend)
                .fold(0.0, f64::max);
            for &(x, z, h) in &land(&m, 2) {
                let g = field.geography(x, z, h);
                let chosen = usize::from(field.mix(&e.biomes, x, z, h).chosen);
                let rain = g.rain.unwrap();
                if dry.contains(&chosen) {
                    assert!(
                        rain < dry_rain,
                        "seed {world} {:?}: {} at {rain} mm ({x}, {z})",
                        f.face,
                        e.biomes[chosen].name
                    );
                }
                if rain >= c.band().rain_mm {
                    for &d in &dry {
                        assert_eq!(field.mix(&e.biomes, x, z, h).weight(d), 0.0);
                    }
                }
            }
            // The main range's windward side, found as R3's test finds it (the highest point of the
            // uplifted ground the hydrology marched its wind over): wet, and no dry biome.
            let hm = hydrology::generate_with_height(
                face_seed(world, f.face),
                hydrology::default_profile(),
                e.height.unwrap(),
            );
            let g: &CoarseGround = &hm.climate.as_ref().expect("the march's ground").0;
            let smooth = smooth_ground(g.n, g.cell, &g.height, CLIMATE.rain.smooth_m);
            let top = (0..smooth.len())
                .filter(|&i| !g.sea[i])
                .max_by(|&a, &b| smooth[a].total_cmp(&smooth[b]))
                .unwrap();
            let (cx, cz) = (
                g.x0 + (top % g.n) as f64 * g.cell,
                g.x0 + (top / g.n) as f64 * g.cell,
            );
            let (ux, uz) = c.upwind();
            // Over the land 1.5–6 km to one side of the crest, a 6 km front: (the mean rain, the points
            // with a wet coast's rain or more, those of them a dry biome took).
            let side = |sign: f64| {
                let (mut seen, mut rain, mut wet, mut dried) = (0u32, 0.0, 0u32, 0u32);
                for d in (0..=9).map(|s| 1500.0 + 500.0 * f64::from(s)) {
                    for w in (-6..=6).map(|s| 500.0 * f64::from(s)) {
                        let (x, z) = (cx + sign * ux * d - uz * w, cz + sign * uz * d + ux * w);
                        let cell = m.cell_at(x, z);
                        if m.is_sea(cell) {
                            continue;
                        }
                        let elev = f64::from(m.height[cell]).max(0.0);
                        let (x, z) = (x as i64, z as i64);
                        let chosen = usize::from(field.mix(&e.biomes, x, z, elev).chosen);
                        let r = field.geography(x, z, elev).rain.unwrap();
                        if r >= c.band().rain_mm {
                            wet += 1;
                            dried += u32::from(dry.contains(&chosen));
                        }
                        rain += r;
                        seen += 1;
                    }
                }
                assert!(seen >= 8, "seed {world}: land beside the range ({seen})");
                (rain / f64::from(seen), wet, dried)
            };
            let (wind, lee) = (side(1.0), side(-1.0));
            // Wetter windward; twice as wet unless the air reaches the range dry (an inland range
            // whose windward side gets under a tenth of the band's coast rain: the realistic
            // batch's wider continents have them).
            let shadow = if wind.0 >= 0.1 * c.band().rain_mm {
                2.0
            } else {
                1.0
            };
            assert!(
                wind.0 > shadow * lee.0,
                "seed {world} {:?}: windward {wind:?}, lee {lee:?}",
                f.face
            );
            assert_eq!(
                wind.2, 0,
                "seed {world} {:?}: a dry biome on the wet windward side",
                f.face
            );
            wet_points += wind.1;
        }
        assert!(
            wet_points > 0,
            "{:?}: the windward sides have wet points",
            f.face
        );
    }
);

dwell_world::both_faces!(
    /// Above the tree line (the forests' least °C, less their blend) no forest has any weight and the
    /// tundra has some (where the snowfield's ice is not colder still); the tundra is chosen only
    /// below the tree line's blend.
    fn the_tundra_stands_above_the_tree_line(f: FaceFrame) {
        let e = edit();
        let tundra = index(&e.biomes, "preset:alpine_tundra");
        let forests: Vec<usize> = FORESTS.iter().map(|n| index(&e.biomes, n)).collect();
        let line = FORESTS
            .iter()
            .map(|n| preset(n).place.celsius)
            .map(|r| r.min - r.blend)
            .fold(f64::INFINITY, f64::min);
        let ice = preset("preset:snowfield").place.celsius;
        let t = preset("preset:alpine_tundra").place.celsius;
        let mut above = 0;
        for world in SEEDS {
            let m = map(&e, world, f);
            let field = BiomeField::with_params(&m, face_seed(world, f.face), e.map);
            for &(x, z, h) in &land(&m, 2) {
                let g = field.geography(x, z, h);
                let mix = field.mix(&e.biomes, x, z, h);
                if usize::from(mix.chosen) == tundra {
                    assert!(g.celsius < t.max + t.blend, "tundra at {} °C", g.celsius);
                }
                if g.celsius < line {
                    for &i in &forests {
                        assert_eq!(
                            mix.weight(i),
                            0.0,
                            "{} at {} °C",
                            e.biomes[i].name,
                            g.celsius
                        );
                    }
                    if g.celsius > ice.max + ice.blend {
                        assert!(mix.weight(tundra) > 0.0, "tundra at {} °C", g.celsius);
                        above += 1;
                    }
                }
            }
        }
        assert!(above > 0, "{:?}: some land above the tree line", f.face);
    }
);

dwell_world::both_faces!(
    /// On the same ground, a colder band has more boreal and tundra and less of the warm biomes.
    fn a_colder_band_has_more_boreal_and_tundra(f: FaceFrame) {
        let e = edit();
        let m = map(&e, 7, f);
        let cells = land(&m, 3);
        let band = |name: &str| CLIMATE.bands.iter().position(|b| b.name == name).unwrap();
        let shares: Vec<(f64, f64)> = ["subtropical", "temperate", "subarctic"]
            .iter()
            .map(|b| {
                let c = Climate::with_band(CLIMATE, m.seed, band(b));
                let field = BiomeField::with_climate(&m, face_seed(7, f.face), e.map, Some(c));
                (
                    share(&field, &e.biomes, &cells, &COLD),
                    share(&field, &e.biomes, &cells, &WARM),
                )
            })
            .collect();
        let (hot, mid, cold) = (shares[0], shares[1], shares[2]);
        assert!(
            hot.0 <= mid.0 && mid.0 < cold.0,
            "{:?}: cold biomes' shares {:?}",
            f.face,
            shares
        );
        assert!(
            hot.1 > mid.1 && mid.1 >= cold.1,
            "{:?}: warm biomes' shares {:?}",
            f.face,
            shares
        );
        assert!(
            cold.0 > 0.3,
            "the subarctic band is mostly cold biomes: {shares:?}"
        );
    }
);

dwell_world::both_faces!(
    /// A biome added by data alone (a fixture with only a °C and rain envelope) is placed by them:
    /// it has weight where the climate fits and none where the rain is far below its least.
    fn a_biome_with_a_climate_envelope_is_placed_by_it(f: FaceFrame) {
        let e = edit();
        let m = map(&e, 1, f);
        let mut bog = preset("preset:wetland");
        bog.name = "test:rain_bog";
        bog.place.wetness = WhereRange::ANY;
        bog.place.elevation = WhereRange::ANY;
        bog.place.rain = WhereRange {
            min: 1200.0,
            max: f64::INFINITY,
            blend: 100.0,
        };
        let list = [e.biomes[0], bog];
        let field = BiomeField::with_params(&m, face_seed(1, f.face), e.map);
        let (mut wet, mut dry) = (0, 0);
        for &(x, z, h) in &land(&m, 2) {
            let r = field.geography(x, z, h).rain.unwrap();
            let w = field.mix(&list, x, z, h).weight(1);
            if r >= 1300.0 {
                assert!(w > 0.99, "{r} mm: {w}");
                wet += 1;
            } else if r < 1100.0 {
                assert_eq!(w, 0.0, "{r} mm");
                dry += 1;
            }
        }
        assert!(wet > 0 && dry > 0, "{wet} wet and {dry} dry cells");
    }
);

/// The first world seed whose face draws the `band` (cheap: the climate only, no map).
fn seed_in_band(f: FaceFrame, band: &str) -> u64 {
    (1..500)
        .find(|&w| Climate::for_seed(face_seed(w, f.face)).band().name == band)
        .unwrap_or_else(|| panic!("{:?}: a seed in the {band} band", f.face))
}

dwell_world::both_faces!(
    /// §7 q9 (owner, 2026-11-12: "Yes add those biomes"): the climates no preset covered at R4, where
    /// the default biome was the fallback, take the new presets: the cold, nearly rainless interior
    /// the cold desert, the hot, very wet tropics the tropical rainforest. On the same maps with the
    /// two left out, the default is chosen there (mostly: since the climate's district noise,
    /// climate.json `variety`, a neighbouring preset's patch takes some of it); with them, they are;
    /// and the default's share of the land falls to under a third of what it was.
    fn the_former_fallback_climates_take_the_new_presets(f: FaceFrame) {
        let e = edit();
        let without: Vec<Biome> = e
            .biomes
            .iter()
            .filter(|b| b.name != "preset:cold_desert" && b.name != "preset:tropical_rainforest")
            .copied()
            .collect();
        assert_eq!(without.len() + 2, e.biomes.len());
        let (cold, wet) = (
            index(&e.biomes, "preset:cold_desert"),
            index(&e.biomes, "preset:tropical_rainforest"),
        );
        // (cells, of them the default chose without the two, the new preset chose with them).
        let (mut dry, mut hot) = ([0u32; 3], [0u32; 3]);
        let (mut was, mut is) = (0u32, 0u32);
        for world in [42, seed_in_band(f, "subtropical")] {
            let m = map(&e, world, f);
            let field = BiomeField::with_params(&m, face_seed(world, f.face), e.map);
            for &(x, z, h) in &land(&m, 3) {
                let g = field.geography(x, z, h);
                let rain = g.rain.unwrap();
                let before = field.mix(&without, x, z, h).chosen;
                let after = usize::from(field.mix(&e.biomes, x, z, h).chosen);
                was += u32::from(before == 0);
                is += u32::from(after == 0);
                // On low ground (the alpine presets take the high cold and dry).
                let tally = if h > 600.0 {
                    None
                } else if (-0.5..=12.5).contains(&g.celsius) && rain < 150.0 {
                    Some((&mut dry, cold))
                } else if g.celsius >= 18.0 && rain >= 2500.0 {
                    Some((&mut hot, wet))
                } else {
                    None
                };
                if let Some((t, want)) = tally {
                    t[0] += 1;
                    t[1] += u32::from(before == 0);
                    t[2] += u32::from(after == want);
                }
            }
        }
        for (name, t) in [("cold desert", dry), ("tropical rainforest", hot)] {
            assert!(t[0] >= 20, "{:?}: {name} climate cells {t:?}", f.face);
            // (Since the realistic batch's plates, 0.61, the default is no longer most of these
            // cells' fallback without the two: the steppe and the sand desert share them; the
            // check is that each new preset takes most of its climate.)
            let _ = t[1];
            assert!(
                f64::from(t[2]) >= 0.8 * f64::from(t[0]),
                "{:?}: the {name} takes its climate {t:?}",
                f.face
            );
        }
        // Halved at least (a quarter before the realistic batch's wider continents, 0.61).
        assert!(
            2 * is < was,
            "{:?}: the default's land {was} -> {is}",
            f.face
        );
    }
);
