//! The starter world (`test:starter`, `shared/worldgen/worlds/starter.json`): a candidate default
//! world assembled by data alone from the systemic terrain layers (ADR 0075: the rain shadow, rocks,
//! vegetation, erosion, tectonics) and the preset biomes (ADR 0074), played at
//! `?worldgen=test:starter`. The fixture parses to its edit and generates on both faces, every biome
//! it lists holds a share of the land, and its main cone stands over 4400 m. The default world is
//! untouched (`tests/golden.rs`). `STARTER_PNG=<dir>` also writes each face's overview (the biome
//! that wins each 100 m cell, hillshaded) for the tour.

use std::sync::Arc;

use dwell_world::*;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::terrain::{FIXTURE_WORLDS, TerrainGenerator};
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:starter";

fn generator(f: FaceFrame, seed: u64) -> (TerrainGenerator, Vec<&'static str>) {
    let e = world_config(NAME).unwrap().expect("the starter's edit");
    let h = e.height.expect("the starter's height field");
    let map = WorldMap::for_height(face_seed(seed, f.face), h)
        .with_worldgen(NAME)
        .unwrap();
    let names = e.biomes.iter().map(|b| b.name).collect();
    let g = TerrainGenerator::with_map(face_seed(seed, f.face), Arc::new(map))
        .with_biomes_and_geology(&e.biomes, e.map, e.geology)
        .with_vegetation(e.vegetation);
    (g, names)
}

#[test]
fn the_starter_fixture_is_an_edit_with_its_own_biomes() {
    // face-a-only: parses the data, no terrain.
    let e = world_config(NAME).unwrap().unwrap();
    assert_eq!(e.biomes[0].name, "dwell:default");
    assert!(e.biomes.len() >= 11, "{} biomes", e.biomes.len());
    assert!(e.map.rain_share > 0.0, "the rain shadow is on");
    let (_, h) = FIXTURE_WORLDS.iter().find(|(n, _)| *n == NAME).unwrap();
    assert_eq!(e.height, Some(*h), "the edit's height is the fixture's");
}

/// The biomes the starter world must show (the owner's climates), each on at least `MIN` of land.
const LISTED: &[&str] = &[
    "starter:broadleaf_forest",
    "preset:mixed_forest",
    "preset:boreal",
    "preset:temperate_meadow",
    "preset:steppe",
    "preset:tropical_beach",
    "preset:sand_desert",
    "preset:red_rock_mesa",
    "preset:badlands",
    "preset:alpine_tundra",
    "preset:wetland",
];
const MIN: f64 = 0.02;

fn colour(i: usize) -> [f64; 3] {
    const C: &[[f64; 3]] = &[
        [150.0, 150.0, 150.0], // default
        [60.0, 140.0, 50.0],   // broadleaf
        [90.0, 130.0, 70.0],   // mixed
        [30.0, 90.0, 60.0],    // boreal
        [150.0, 200.0, 90.0],  // meadow
        [190.0, 180.0, 110.0], // steppe
        [240.0, 225.0, 160.0], // beach
        [235.0, 200.0, 120.0], // desert
        [190.0, 80.0, 50.0],   // mesa
        [160.0, 110.0, 150.0], // badlands
        [235.0, 240.0, 245.0], // tundra
        [70.0, 160.0, 150.0],  // wetland
    ];
    C[i % C.len()]
}

dwell_world::both_faces!(
    /// Every listed biome wins at least `MIN` of the land (sampled every 200 m), and the starter's
    /// chunks generate at a few spots.
    fn every_listed_biome_holds_a_share_of_the_land(f: FaceFrame) {
        let (g, names) = generator(f, 0);
        let r = i64::from(WORLD_RADIUS) * 3 / 4;
        let step = 200;
        let mut won = vec![0u32; names.len()];
        let mut land = 0u32;
        let png = std::env::var("STARTER_PNG").ok();
        let n = (2 * r / step + 1) as usize;
        let mut img = vec![0u8; n * n * 3];
        let map = g.map();
        for (j, z) in (-r..=r).step_by(step as usize).enumerate() {
            for (i, x) in (-r..=r).step_by(step as usize).enumerate() {
                let geo = g.biome_geography(x, z).expect("a biome map");
                let h = map.height_at(x as f64, z as f64).0;
                let px = &mut img[(j * n + i) * 3..(j * n + i) * 3 + 3];
                if geo.coast <= 0.0 {
                    px.copy_from_slice(&[40, 70, 120]);
                    continue;
                }
                land += 1;
                let c = g.biome_mix(x, z).chosen as usize;
                won[c] += 1;
                if png.is_some() {
                    let e = map.height_at(x as f64 + 100.0, z as f64 - 100.0).0;
                    let shade = (1.0 + (h - e) / 60.0).clamp(0.5, 1.4);
                    let k = colour(c);
                    for q in 0..3 {
                        px[q] = (k[q] * shade).clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
        let mut report = String::new();
        for (i, name) in names.iter().enumerate() {
            report += &format!(
                "{name} {:.1}%, ",
                100.0 * f64::from(won[i]) / f64::from(land)
            );
        }
        println!("face {:?}: {land} land samples: {report}", f.face);
        if let Some(dir) = png {
            write_png(
                &format!("{dir}/overview_{:?}.png", f.face),
                n as u32,
                n as u32,
                &img,
            );
        }
        for l in LISTED {
            let i = names.iter().position(|n| n == l).expect(l);
            let share = f64::from(won[i]) / f64::from(land);
            assert!(
                share >= MIN,
                "{l} holds {:.2}% of the land: {report}",
                100.0 * share
            );
        }
        for (x, z) in [(0, 0), (4000, -3000), (-8000, 6000)] {
            let col = g.column_at(x, z);
            assert!(col.height > 0.0, "a column at {x}, {z}");
        }
    }
);

dwell_world::both_faces!(
    /// The arc's first cone, Rainier-shaped, stands at least 4400 m above the sea on the map.
    fn the_main_cone_stands_over_4400_m(f: FaceFrame) {
        let (g, _) = generator(f, 0);
        let h = world_config(NAME).unwrap().unwrap().height.unwrap();
        let v = h.tectonics.volcanoes()[0];
        let m = g.map();
        let mut top = f64::NEG_INFINITY;
        for i in -30..=30 {
            for j in -30..=30 {
                let t = m
                    .height_at(v.x + f64::from(i) * 50.0, v.z + f64::from(j) * 50.0)
                    .0;
                top = top.max(t);
            }
        }
        println!(
            "face {:?}: the main cone at ({:.0}, {:.0}) stands {top:.0} m",
            f.face, v.x, v.z
        );
        assert!(top >= 4400.0, "the main cone stands {top:.0} m");
    }
);

fn write_png(path: &str, w: u32, h: u32, rgb: &[u8]) {
    // A minimal PNG writer: stored (uncompressed) deflate blocks (as examples/terrain_view.rs).
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xedb8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc(&body).to_be_bytes());
    }
    let mut raw = Vec::new();
    for y in 0..h as usize {
        raw.push(0);
        raw.extend_from_slice(&rgb[y * w as usize * 3..(y + 1) * w as usize * 3]);
    }
    let mut z = vec![0x78, 0x01];
    for (i, block) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        z.push(u8::from(last));
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    std::fs::write(path, out).expect("write png");
}

/// The tour's spots (`cargo test … -- --ignored --nocapture tour`): for each listed biome the 200 m
/// sample of it (over 0.6 of the mix) nearest the centre of its cells (else its strongest sample), as an `at=` world point on face A (feet 2 m over
/// the ground), the volcano 15 km off its summit and the shore facing the hotspot chain.
#[test]
#[ignore = "prints the tour's links"]
fn tour() {
    // face-a-only: the links are face A's.
    let f = FaceFrame::A;
    let (g, names) = generator(f, 0);
    let r = i64::from(WORLD_RADIUS) * 3 / 4;
    let mut cells: Vec<Vec<(i64, i64)>> = vec![Vec::new(); names.len()];
    // A biome that never holds 0.6 of a mix (the mesa beside the badlands): its strongest chosen
    // sample instead.
    let mut best: Vec<(f32, i64, i64)> = vec![(0.0, 0, 0); names.len()];
    for z in (-r..=r).step_by(200) {
        for x in (-r..=r).step_by(200) {
            if g.biome_geography(x, z).is_none_or(|g| g.coast < 300.0) {
                continue;
            }
            let m = g.biome_mix(x, z);
            if let Some(&(_, w)) = m.weights[..m.len as usize]
                .iter()
                .find(|(i, _)| *i == m.chosen)
                && w > best[m.chosen as usize].0
            {
                best[m.chosen as usize] = (w, x, z);
            }
            if m.weights[..m.len as usize]
                .iter()
                .any(|(i, w)| *i == m.chosen && *w > 0.6)
            {
                cells[m.chosen as usize].push((x, z));
            }
        }
    }
    let at = |x: i64, z: i64| {
        let y = f64::from(g.column_at(x as i32, z as i32).height) + 2.0;
        format!("{x}.5,{y:.0},{z}.5")
    };
    for (i, c) in cells.iter().enumerate() {
        if c.is_empty() {
            let (w, x, z) = best[i];
            if w > 0.0 {
                println!("{} (at most {w:.2} of a mix) at={}", names[i], at(x, z));
            }
            continue;
        }
        let n = c.len() as f64;
        let (mx, mz) = c.iter().fold((0.0, 0.0), |a, p| {
            (a.0 + p.0 as f64 / n, a.1 + p.1 as f64 / n)
        });
        let p = c
            .iter()
            .min_by(|a, b| {
                let d = |p: &&(i64, i64)| (p.0 as f64 - mx).powi(2) + (p.1 as f64 - mz).powi(2);
                d(a).total_cmp(&d(b))
            })
            .unwrap();
        println!("{} at={}", names[i], at(p.0, p.1));
    }
    let h = world_config(NAME).unwrap().unwrap().height.unwrap();
    let vs = h.tectonics.volcanoes();
    let (v, w) = (vs[0], vs[1]);
    for (label, (x, z)) in [
        ("volcano 15 km east", (v.x + 15000.0, v.z)),
        ("volcano 15 km south", (v.x, v.z + 15000.0)),
        ("between the cones", ((v.x + w.x) / 2.0, (v.z + w.z) / 2.0)),
    ] {
        println!(
            "{label} (cone at {:.0},{:.0}) at={}",
            v.x,
            v.z,
            at(x as i64, z as i64)
        );
    }
    let hs = vs[2];
    let mut best = (f64::INFINITY, 0, 0);
    for z in (-r..=r).step_by(200) {
        for x in (-r..=r).step_by(200) {
            if g.biome_geography(x, z)
                .is_some_and(|g| g.coast > 50.0 && g.coast < 400.0)
            {
                let d = (x as f64 - hs.x).hypot(z as f64 - hs.z);
                if d < best.0 && (x * x + z * z) < 24000 * 24000 {
                    best = (d, x, z);
                }
            }
        }
    }
    println!(
        "shore facing the islands (first at {:.0},{:.0}) at={}",
        hs.x,
        hs.z,
        at(best.1, best.2)
    );
}

/// Times generating the chunks round the tour's tree spots (oaks, the mixed forest, palms): run
/// before and after a change to the trees (`cargo test --release -p dwell-worldgen --test
/// starter_world time_tree_spots -- --ignored --nocapture`).
#[test]
#[ignore = "a timing, printed"]
fn time_tree_spots() {
    // face-a-only: a timing of face A's chunks.
    let (g, _) = generator(FaceFrame::A, 0);
    for (label, x, z) in [
        ("broadleaf", 13900, -7100),
        ("mixed", -17700, -7300),
        ("coast", 20500, 11900),
    ] {
        let t = std::time::Instant::now();
        let (mut chunks, mut solid) = (0, 0usize);
        for cz in -4..4 {
            for cx in -4..4 {
                let (wx, wz) = (x / CHUNK_SIZE + cx, z / CHUNK_SIZE + cz);
                let h = f64::from(g.column_at(wx * CHUNK_SIZE, wz * CHUNK_SIZE).height);
                let row = rows_for_m(h).div_euclid(CHUNK_SIZE);
                for cy in row - 1..=row + 2 {
                    let mut c = Chunk::default();
                    g.generate(ChunkCoord::new(wx, cy, wz), &mut c);
                    solid += c.voxels().iter().filter(|&&m| m != 0).count();
                    chunks += 1;
                }
            }
        }
        println!(
            "{label}: {chunks} chunks in {:.0} ms ({solid} solid voxels)",
            t.elapsed().as_secs_f64() * 1000.0
        );
    }
}

/// The blocks round a spot: the log voxels standing in the 8 × 8 chunks round it and the surface
/// blocks of their top voxels, by name.
fn spot_blocks(
    g: &TerrainGenerator,
    x: i32,
    z: i32,
) -> (usize, std::collections::BTreeMap<&'static str, usize>) {
    let log = dwell_blocks::block_of(dwell_blocks::require("dwell:log")).name;
    let mut logs = 0;
    let mut tops = std::collections::BTreeMap::new();
    for cz in -4..4 {
        for cx in -4..4 {
            let (wx, wz) = (x / CHUNK_SIZE + cx, z / CHUNK_SIZE + cz);
            let h = f64::from(g.column_at(wx * CHUNK_SIZE, wz * CHUNK_SIZE).height);
            let row = rows_for_m(h).div_euclid(CHUNK_SIZE);
            let mut cs = Vec::new();
            for cy in row - 2..=row + 3 {
                let mut c = Chunk::default();
                g.generate(ChunkCoord::new(wx, cy, wz), &mut c);
                cs.push(c);
            }
            for c in &cs {
                logs += c
                    .voxels()
                    .iter()
                    .filter(|&&m| m != 0 && dwell_blocks::block_of(m).name == log)
                    .count();
            }
            // The top voxel of the chunk column's corner, by its block's name.
            let col = g.column_at(wx * CHUNK_SIZE, wz * CHUNK_SIZE);
            let r = rows_for_m(f64::from(col.height)) - 1;
            let ci = (r.div_euclid(CHUNK_SIZE) - (row - 2)) as usize;
            if let Some(c) = cs.get(ci) {
                let m = c.get(0, r.rem_euclid(CHUNK_SIZE), 0);
                if m != 0 {
                    *tops.entry(dwell_blocks::block_of(m).name).or_insert(0) += 1;
                }
            }
        }
    }
    (logs, tops)
}

/// The 0.52.55 tour's boreal spot (terraced uplands in the range's dry lee) stood no pines: the pine
/// species' climate floor (6.5 °C) put it above the tree line and its moisture was 0. The conifer
/// species is hardier (vegetation.json) and the starter's boreal keeps off the driest lee.
#[test]
fn the_boreal_spot_stands_pines() {
    // face-a-only: the tour's spot is face A's (the placement is face-local, both faces alike).
    let (g, _) = generator(FaceFrame::A, 0);
    let (logs, tops) = spot_blocks(&g, -9300, 2900);
    println!("boreal: {logs} log voxels, tops {tops:?}");
    assert!(logs > 200, "the boreal spot's logs: {logs}, tops {tops:?}");
}

dwell_world::both_faces!(
    /// The pick's `sharpen` (biomeMap, 0.52.63): with the starter's 2 the strongest biome takes more
    /// of the mixed columns than with 1 (the weights as they are), so two biomes sharing a climate
    /// (the mesa and the badlands) speckle less; the weights themselves are the same.
    fn sharpen_gives_the_strongest_biome_more_columns(f: FaceFrame) {
        let e = world_config(NAME).unwrap().unwrap();
        assert!(e.map.sharpen > 1.0, "the starter sharpens");
        let h = e.height.unwrap();
        let map = Arc::new(
            WorldMap::for_height(face_seed(0, f.face), h)
                .with_worldgen(NAME)
                .unwrap(),
        );
        let gen_with = |sharpen: f64| {
            let mut m = e.map;
            m.sharpen = sharpen;
            TerrainGenerator::with_map(face_seed(0, f.face), map.clone())
                .with_biomes_and_geology(&e.biomes, m, e.geology)
        };
        let (plain, sharp) = (gen_with(1.0), gen_with(e.map.sharpen));
        let (mut top_plain, mut top_sharp, mut mixed) = (0u32, 0u32, 0u32);
        for z in (-12000..=12000).step_by(97) {
            for x in (-12000..=12000).step_by(97) {
                let (a, b) = (plain.biome_mix(x, z), sharp.biome_mix(x, z));
                assert_eq!(a.weights, b.weights, "the weights are the same at {x}, {z}");
                if a.len < 2 {
                    continue;
                }
                mixed += 1;
                let top = a.weights[..a.len as usize]
                    .iter()
                    .max_by(|p, q| p.1.total_cmp(&q.1))
                    .unwrap()
                    .0;
                top_plain += u32::from(a.chosen == top);
                top_sharp += u32::from(b.chosen == top);
            }
        }
        println!(
            "{mixed} mixed columns: the strongest picked {top_plain} (1), {top_sharp} (sharpened)"
        );
        assert!(mixed > 1000, "{mixed}");
        assert!(
            f64::from(top_sharp) > 1.05 * f64::from(top_plain),
            "{top_sharp} vs {top_plain}"
        );
    }
);

dwell_world::both_faces!(
    /// A biome's `decorations` (0.52.63): tall grass grows only where the column's biome lists it.
    /// The starter with its broadleaf forest listing none: round the tour's broadleaf spot no tall
    /// grass stands in a column of that biome, and some stands in its neighbours' grass.
    fn decorations_follow_the_biome(f: FaceFrame) {
        let mut e = world_config(NAME).unwrap().unwrap();
        let broadleaf = e
            .biomes
            .iter()
            .position(|b| b.name == "starter:broadleaf_forest")
            .unwrap();
        assert!(!e.biomes[broadleaf].decorations.is_empty());
        e.biomes[broadleaf].decorations = &[];
        let h = e.height.unwrap();
        let map = WorldMap::for_height(face_seed(0, f.face), h)
            .with_worldgen(NAME)
            .unwrap();
        let g = TerrainGenerator::with_map(face_seed(0, f.face), Arc::new(map))
            .with_biomes_and_geology(&e.biomes, e.map, e.geology)
            .with_vegetation(e.vegetation);
        let grass = dwell_blocks::block_of(dwell_blocks::require("dwell:tall_grass")).name;
        let (mut unlisted, mut listed) = (0, 0);
        let (x, z) = (22300, 100);
        for cz in -3..3 {
            for cx in -3..3 {
                let (wx, wz) = (x / CHUNK_SIZE + cx, z / CHUNK_SIZE + cz);
                let h = f64::from(g.column_at(wx * CHUNK_SIZE, wz * CHUNK_SIZE).height);
                let row = rows_for_m(h).div_euclid(CHUNK_SIZE);
                for cy in row - 2..=row + 2 {
                    let mut c = Chunk::default();
                    g.generate(ChunkCoord::new(wx, cy, wz), &mut c);
                    for lz in 0..CHUNK_SIZE {
                        for lx in 0..CHUNK_SIZE {
                            for ly in 0..CHUNK_SIZE {
                                let m = c.get(lx, ly, lz);
                                if m == 0 || dwell_blocks::block_of(m).name != grass {
                                    continue;
                                }
                                let (px, pz) = (wx * CHUNK_SIZE + lx, wz * CHUNK_SIZE + lz);
                                let b = g.biome_mix(i64::from(px), i64::from(pz)).chosen;
                                if e.biomes[b as usize].decorations.is_empty() {
                                    unlisted += 1;
                                } else {
                                    listed += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        println!("tall grass: {listed} where listed, {unlisted} where not");
        assert_eq!(
            unlisted, 0,
            "tall grass in biomes that list none ({listed} listed)"
        );
        assert!(listed > 0, "tall grass where listed");
    }
);

dwell_world::both_faces!(
    /// A world's `cliffs` (0.52.64): the starter's 0 leaves its chunks as without the terrain
    /// volume (no cliffs), where the build's volume at full strength reshapes them.
    fn the_starters_cliffs_are_off(f: FaceFrame) {
        let e = world_config(NAME).unwrap().unwrap();
        assert_eq!(e.cliffs, Some(0.0));
        let def = dwell_worldgen::volume::terrain_volume().expect("the build's cliffs");
        let (plain, _) = generator(f, 0);
        let with = plain.clone().with_volume(def);
        // The build's world (no worldgen config) at the same seed: the volume reshapes it.
        let build = TerrainGenerator::new(face_seed(0, f.face));
        let build_cliffs = build.clone().with_volume(def);
        let (mut same, mut differ) = (true, false);
        for (x, z) in [(22300, 100), (-4100, 8300), (-9300, 2900)] {
            let (cx, cz) = (x / CHUNK_SIZE, z / CHUNK_SIZE);
            let row = rows_for_m(f64::from(plain.column_at(x, z).height)).div_euclid(CHUNK_SIZE);
            let row_b = rows_for_m(f64::from(build.column_at(x, z).height)).div_euclid(CHUNK_SIZE);
            for dy in -1..=1 {
                let (mut a, mut b) = (Chunk::default(), Chunk::default());
                plain.generate(ChunkCoord::new(cx, row + dy, cz), &mut a);
                with.generate(ChunkCoord::new(cx, row + dy, cz), &mut b);
                same &= a.voxels() == b.voxels();
                let (mut c, mut d) = (Chunk::default(), Chunk::default());
                build.generate(ChunkCoord::new(cx, row_b + dy, cz), &mut c);
                build_cliffs.generate(ChunkCoord::new(cx, row_b + dy, cz), &mut d);
                differ |= c.voxels() != d.voxels();
            }
        }
        assert!(same, "the starter's chunks are as without the volume");
        assert!(
            differ,
            "the build's volume reshapes the build's chunks there"
        );
    }
);
