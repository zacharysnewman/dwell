//! Water through the server (the water surface grid, docs/WATER_SURFACE_GRID.md): a player's edit
//! frees the water near it, the water ticks in game time, and its changes go out as modifications.

mod common;

use common::*;
use dwell_protocol::*;
use dwell_sim::server::SessionId;
use dwell_world::{Chunk, ChunkCoord, FLAT_TOP_Y, M, chunk_hash, generate_flat_chunk};

const G: i32 = FLAT_TOP_Y;

fn join_streaming(f: &mut Fixture, id: SessionId) {
    let w = f.join(id, &Client::new(id as u8));
    let mut v = Chunk::default();
    generate_flat_chunk(ChunkCoord::from_array(w.verification_chunk), &mut v);
    f.send(id, Message::WorldgenCheck(chunk_hash(&v)));
}

#[test]
fn breaking_a_lakes_wall_drains_its_top_row_and_clients_see_it() {
    // face-a-only: the server's flat world is face A's; the water's both faces are tested in
    // `water::tests`.
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.take_all();
    // A walled lake four rows deep in front of the player (x −2–2, z 3–8), its near wall at z 2.
    let stone = dwell_blocks::require("dwell:stone");
    let w = f.server.world();
    for x in -3..=3 {
        for z in 2..=9 {
            for y in G..G + 4 {
                let wall = x == -3 || x == 3 || z == 2 || z == 9;
                w.set_voxel(x, y, z, if wall { stone } else { M.water });
            }
        }
    }
    f.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Break,
            cell: [0, G + 3, 2],
            face: 5,
            material: 0,
            rows: 1,
            down: false,
        }),
    );
    let mut drained = Vec::new();
    for _ in 0..240 {
        f.server.step();
        for (_, out) in f.take_all() {
            for m in out {
                if let Message::VoxelModification(v) = m
                    && v.reason == VoxelModificationReason::Rebake
                {
                    drained.extend(v.chunks.iter().flat_map(|c| c.changes.clone()));
                }
            }
        }
    }
    let w = f.server.world();
    for x in -2..=2 {
        for z in 3..=8 {
            assert_eq!(
                w.get_voxel(x, G + 3, z),
                M.air,
                "the top row drained at {x}, {z}"
            );
            assert_eq!(
                w.get_voxel(x, G + 2, z),
                M.water,
                "the row below the cut stays at {x}, {z}"
            );
        }
    }
    assert!(
        drained.iter().filter(|c| c.material == M.air).count() >= 30,
        "the client was sent the drained cells ({} changes)",
        drained.len()
    );
    let sill = dwell_world::m_of_row(G + 3);
    let level = f
        .server
        .water()
        .level_at(dwell_world::FaceFrame::A, 0, 5)
        .expect("the lake");
    assert!(
        (level - sill).abs() < 0.06,
        "the lake drained to the gap's sill: {level:.3} against {sill:.3}"
    );
}

#[test]
fn the_waters_modifications_reach_a_client_in_revision_order() {
    // face-a-only: the server's flat world is face A's; the water's both faces are tested in
    // `water::tests`.
    // Owner, 2026-11-04: rising water "disappears and then reappears at a slightly higher level".
    // The water wrote its cells with `set_voxel`, a revision per cell, then sent them as one
    // modification: every water modification skipped revisions, so the client asked for the whole
    // chunk again and drew it dark until relit. Each chunk's modifications now follow one another.
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.take_all();
    let stone = dwell_blocks::require("dwell:stone");
    let w = f.server.world();
    for x in -3..=3 {
        for z in 2..=9 {
            for y in G..G + 4 {
                let wall = x == -3 || x == 3 || z == 2 || z == 9;
                w.set_voxel(x, y, z, if wall { stone } else { M.water });
            }
        }
    }
    let mut seen: std::collections::HashMap<[i32; 3], u32> = std::collections::HashMap::new();
    let w = f.server.world();
    for x in -1..=0 {
        for y in -1..=1 {
            for z in -1..=0 {
                if let Some(c) = w.find(ChunkCoord::new(x, y, z)) {
                    seen.insert([x, y, z], c.revision());
                }
            }
        }
    }
    f.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Break,
            cell: [0, G + 3, 2],
            face: 5,
            material: 0,
            rows: 1,
            down: false,
        }),
    );
    let mut water = 0;
    for _ in 0..240 {
        f.server.step();
        for (_, out) in f.take_all() {
            for m in out {
                let Message::VoxelModification(v) = m else {
                    continue;
                };
                water += usize::from(v.reason == VoxelModificationReason::Rebake);
                for c in &v.chunks {
                    if let Some(last) = seen.get(&c.coord) {
                        assert_eq!(
                            c.revision,
                            last + 1,
                            "chunk {:?}: revision {} after {last} ({:?})",
                            c.coord,
                            c.revision,
                            v.reason
                        );
                    }
                    seen.insert(c.coord, c.revision);
                }
            }
        }
    }
    assert!(water > 0, "the water sent its changes");
}

#[test]
fn a_lake_cut_open_drains_on_after_its_player_leaves() {
    // face-a-only: the server's flat world is face A's; the water's both faces are tested in
    // `water::tests`.
    // Owner, 2026-11-03: water an edit set going changes whether or not anyone is near (two
    // players far apart on one river). A lake cut open, its player leaving at once (no one near): the lake
    // drains on (its chunks kept loaded, not evicted) and settles. Red before: it waited as left.
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.take_all();
    let stone = dwell_blocks::require("dwell:stone");
    let w = f.server.world();
    for x in -3..=3 {
        for z in 2..=9 {
            for y in G..G + 4 {
                let wall = x == -3 || x == 3 || z == 2 || z == 9;
                w.set_voxel(x, y, z, if wall { stone } else { M.water });
            }
        }
    }
    f.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Break,
            cell: [0, G + 3, 2],
            face: 5,
            material: 0,
            rows: 1,
            down: false,
        }),
    );
    for _ in 0..7 {
        f.server.step();
    }
    let lake_chunk = dwell_world::chunk_of(0, G, 5);
    f.server.on_disconnected(1);
    for _ in 0..1200 {
        f.server.step();
        f.take_all();
    }
    assert!(
        f.server.world().find(lake_chunk).is_some(),
        "the lake's chunk kept loaded"
    );
    let w = f.server.world();
    for x in -2..=2 {
        for z in 3..=8 {
            assert_eq!(w.get_voxel(x, G + 3, z), M.air, "drained at {x}, {z}");
            assert_eq!(
                w.get_voxel(x, G + 2, z),
                M.water,
                "below the cut at {x}, {z}"
            );
        }
    }
    assert!(
        f.server
            .water()
            .level_at(dwell_world::FaceFrame::A, 0, 5)
            .is_some_and(|l| (l - dwell_world::m_of_row(G + 3)).abs() < 0.06),
        "drained to the sill"
    );
}

/// Owner, 2026-11-03: a block set at the bottom of any river flooded it without end, or drained
/// it; generated water must rest by the one law, as any water does. In generated worlds (seeds 0–2),
/// a stone set in the bed of rivers picked along the world map's rivers changes no other cell within
/// 48 m over six hours of water time, and the water sleeps again (the grid's rest layer,
/// WATER_SURFACE_GRID.md §2b: red without it, 1049 cells changed on seed 0); a ramp at the water's
/// edge may take its flooded state.
#[test]
fn a_block_set_in_a_generated_rivers_bed_changes_nothing_else() {
    // face-a-only: the generated world's face A; the water's both faces are tested in
    // `water::tests`.
    use dwell_world::{Face, GENERATOR_TERRAIN, VoxelWorld};
    const R: i32 = 48;
    let liquid = |m: u16| dwell_blocks::try_info(m).is_some_and(|i| i.liquid);
    let mut checked = 0;
    for seed in [0u64, 1, 2] {
        let rivers = std::sync::Arc::new(
            dwell_worldgen::BifacialTerrain::for_generator(GENERATOR_TERRAIN, seed)
                .expect("terrain"),
        );
        let map = rivers.local(Face::A).map().clone();
        // Vertices along the map's rivers, inland, of small and medium flows.
        let mut picks: Vec<(i32, i32)> = Vec::new();
        for r in &map.rivers {
            for (i, v) in r.path.iter().enumerate() {
                if i % 40 == 20 && v.surface > 3.0 && (0.3..30.0).contains(&v.discharge) {
                    picks.push((v.x as i32, v.z as i32));
                }
            }
        }
        let mut sites = 0;
        for (px, pz) in picks {
            if sites == 4 {
                break;
            }
            let mut w = VoxelWorld::new(
                dwell_worldgen::generator_for(GENERATOR_TERRAIN, seed),
                Some(dwell_worldgen::air_test_for(GENERATOR_TERRAIN, seed)),
            );
            // A column of the river two or more cells deep near the vertex: its bed cell.
            let mut bed = None;
            'find: for dx in -6..=6 {
                for dz in -6..=6 {
                    let (x, z) = (px + dx, pz + dz);
                    let Some(top) = (0..700).rev().find(|&y| {
                        let m = w.get_voxel(x, y, z);
                        m != dwell_world::M.air
                            && !dwell_blocks::try_info(m).is_some_and(|i| i.replaceable)
                    }) else {
                        continue;
                    };
                    if !liquid(w.get_voxel(x, top, z)) {
                        continue;
                    }
                    let mut b = top;
                    while liquid(w.get_voxel(x, b - 1, z)) {
                        b -= 1;
                    }
                    if top - b >= 1 {
                        bed = Some([x, b, z]);
                        break 'find;
                    }
                }
            }
            let Some(bed) = bed else {
                continue;
            };
            sites += 1;
            let (lo, hi) = (bed[1] - 24, bed[1] + 24);
            // Loaded well past what is checked: a river running on out of the loaded world
            // seeps away there, not a part of this check.
            const LOAD: i32 = R + 80;
            for x in (bed[0] - LOAD..=bed[0] + LOAD).step_by(8) {
                for z in (bed[2] - LOAD..=bed[2] + LOAD).step_by(8) {
                    for y in (lo - 16..=hi + 16).step_by(8) {
                        w.get_voxel(x, y, z);
                    }
                }
            }
            let snapshot = |w: &VoxelWorld| -> Vec<u16> {
                let mut v = Vec::new();
                for x in bed[0] - R..=bed[0] + R {
                    for z in bed[2] - R..=bed[2] + R {
                        for y in lo..=hi {
                            v.push(w.get_voxel_if_loaded([x, y, z]));
                        }
                    }
                }
                v
            };
            let stone = dwell_blocks::require("dwell:stone");
            let was = w.get_voxel(bed[0], bed[1], bed[2]);
            w.set_voxel(bed[0], bed[1], bed[2], stone);
            let before = snapshot(&w);
            // With the map's rivers too, as the server's (their currents and the flow they carry,
            // §2b): a river at rest sends nothing down them.
            let mut water = dwell_sim::water::Water::for_world(GENERATOR_TERRAIN, seed, true);
            water.touch([(bed, was)]);
            let player = [
                f64::from(bed[0]) + 0.5,
                dwell_world::m_of_row(bed[1]) + 2.0,
                f64::from(bed[2]) + 0.5,
            ];
            // Six hours of water time at a 20-minute day (7.2 s a water tick).
            for _ in 0..3000 {
                water.tick(&mut w, &[player], 7.2);
            }
            let after = snapshot(&w);
            assert_eq!(
                water.awake_chunks(),
                0,
                "seed {seed}, bed {bed:?}: the water settled again"
            );
            // A ramp at the water's edge may take its flooded state: nothing else.
            let flooded =
                |a: u16, b: u16| dwell_blocks::with_property(a, "flooded", "true") == Some(b);
            let changed = before
                .iter()
                .zip(&after)
                .filter(|&(&a, &b)| a != b && !flooded(a, b))
                .count();
            if changed > 0 {
                let mut kinds: std::collections::BTreeMap<(String, String), usize> =
                    Default::default();
                for (&a, &b) in before.iter().zip(&after) {
                    if a != b {
                        let n = |m: u16| {
                            dwell_blocks::try_info(m).map_or("?".to_string(), |i| {
                                format!(
                                    "{}{}",
                                    i.name,
                                    if i.liquid {
                                        format!(" {:.2}", i.fill)
                                    } else {
                                        String::new()
                                    }
                                )
                            })
                        };
                        *kinds.entry((n(a), n(b))).or_default() += 1;
                    }
                }
                eprintln!(
                    "DIAG {kinds:?} held {:?} free {:.1} awake {} {}",
                    water.held(),
                    water.free_volume(),
                    water.awake_chunks(),
                    water.report(bed, 6)
                );
            }
            assert_eq!(
                changed, 0,
                "seed {seed}: {changed} cells changed near {bed:?}"
            );
            checked += 1;
        }
    }
    assert!(checked >= 5, "river pools checked: {checked}");
}

#[test]
fn an_untouched_rivers_water_has_its_maps_current() {
    // face-a-only: the generated world's face A; the water's both faces are tested in
    // `water::tests`. Phase 53 G3b: held river water runs as the world map's river does, so a
    // swimmer in a river nobody changed drifts downstream.
    use dwell_world::{Face, FaceFrame, GENERATOR_TERRAIN, VoxelWorld};
    let liquid = |m: u16| dwell_blocks::try_info(m).is_some_and(|i| i.liquid);
    let terrain = std::sync::Arc::new(
        dwell_worldgen::BifacialTerrain::for_generator(GENERATOR_TERRAIN, 0).expect("terrain"),
    );
    let map = terrain.local(Face::A).map().clone();
    let mut checked = 0;
    for r in &map.rivers {
        let Some(v) = r.path.get(r.path.len() / 2) else {
            continue;
        };
        if v.surface <= 3.0 || checked == 3 {
            continue;
        }
        let mut w = VoxelWorld::new(
            dwell_worldgen::generator_for(GENERATOR_TERRAIN, 0),
            Some(dwell_worldgen::air_test_for(GENERATOR_TERRAIN, 0)),
        );
        // A river column (its water at least a cell deep) near the course's midpoint.
        let (px, pz) = (v.x as i32, v.z as i32);
        let mut at = None;
        'find: for dx in -6..=6 {
            for dz in -6..=6 {
                let (x, z) = (px + dx, pz + dz);
                let Some(top) = (0..700).rev().find(|&y| {
                    let m = w.get_voxel(x, y, z);
                    m != dwell_world::M.air
                        && !dwell_blocks::try_info(m).is_some_and(|i| i.replaceable)
                }) else {
                    continue;
                };
                if liquid(w.get_voxel(x, top, z))
                    && liquid(w.get_voxel(x, top - 1, z))
                    && let Some(want) = terrain.local(Face::A).river_current(x, z, 0.75)
                {
                    at = Some(([x, top, z], want));
                    break 'find;
                }
            }
        }
        let Some((cell, want)) = at else {
            continue;
        };
        let rivers = terrain.clone();
        let mut water = dwell_sim::water::Water::new().with_rivers(std::sync::Arc::new(
            move |f: FaceFrame, x, z, depth| rivers.local(f.face).river_current(x, z, depth),
        ));
        water.tick(&mut w, &[], 0.1);
        let p = [
            f64::from(cell[0]) + 0.5,
            dwell_world::m_of_row(cell[1]) + 0.1,
            f64::from(cell[2]) + 0.5,
        ];
        let [u, v] = w.currents.at(p);
        let along = f64::from(u) * want[0] + f64::from(v) * want[1];
        assert!(
            f64::from(u.hypot(v)) > 0.05 && along > 0.0,
            "the river at {cell:?} runs {u:.3}, {v:.3} m/s; its map says {want:?}"
        );
        checked += 1;
    }
    assert!(checked >= 2, "{checked} rivers checked");
}

/// Owner, 2026-11-06: "damming should raise the water and does" (BUGS.md #38). Seed 0's river at
/// the bench's dam site (`?at=2364.5,142,7443.5`), a stone wall 32 m across it at z 7452 (the
/// browser's measurement: 2 cm in ~1.7 h of water time). The river is drawn flat between its
/// steps, so the water as generated carried none of the river's flow and a dam had nothing to back
/// up; with the map's river driving it (WATER_SURFACE_GRID.md §2b), the water behind the wall
/// rises by the river's discharge.
#[test]
fn a_dam_across_a_generated_flat_reach_backs_the_river_up() {
    // face-a-only: the generated world's face A; the water's both faces are tested in
    // `water::tests` (`a_dam_across_a_flat_river_backs_it_up_by_the_maps_flow`).
    use dwell_world::{Face, FaceFrame, GENERATOR_TERRAIN, VoxelWorld};
    let terrain = std::sync::Arc::new(
        dwell_worldgen::BifacialTerrain::for_generator(GENERATOR_TERRAIN, 0).expect("terrain"),
    );
    let mut w = VoxelWorld::new(
        dwell_worldgen::generator_for(GENERATOR_TERRAIN, 0),
        Some(dwell_worldgen::air_test_for(GENERATOR_TERRAIN, 0)),
    );
    let (x0, x1, z0, z1) = (2349, 2380, 7452, 7453);
    // Loaded well around the site: the backwater runs up the river.
    for x in (x0 - 160..=x1 + 160).step_by(8) {
        for z in (z0 - 200..=z1 + 120).step_by(8) {
            for y in (240..=320).step_by(8) {
                w.get_voxel(x, y, z);
            }
        }
    }
    // As the server's: the map's rivers' currents and flow.
    let mut water = dwell_sim::water::Water::for_world(GENERATOR_TERRAIN, 0, true);
    water.tick(&mut w, &[], 0.1);
    let f = FaceFrame::A;
    let probe = (2364, 7448);
    let level0 = water
        .level_at(f, probe.0, probe.1)
        .expect("the river at the site");
    let want = terrain.local(Face::A).river_current(probe.0, probe.1, 1.0);
    let stone = dwell_blocks::require("dwell:stone");
    let mut old = Vec::new();
    for x in x0..=x1 {
        for y in 262..=293 {
            for z in z0..=z1 {
                old.push(([x, y, z], w.get_voxel(x, y, z)));
                w.set_voxel(x, y, z, stone);
            }
        }
    }
    water.touch(old);
    let player = [2364.5, 142.0, 7443.5];
    // Twenty minutes of water time in ticks of 1 s.
    for _ in 0..1200 {
        water.tick(&mut w, &[player], 1.0);
    }
    let level = water
        .level_at(f, probe.0, probe.1)
        .expect("the river behind the wall");
    eprintln!(
        "dam site: {level0:.3} → {level:.3} m (map current {want:?}); {}",
        water.report([probe.0, 280, probe.1], 3)
    );
    assert!(
        level > level0 + 0.25,
        "the river behind the wall rose: {level0:.3} → {level:.3}"
    );
}

/// Seed 0's river dammed as the owner did it (BUGS.md #40): a stone wall 32 m across it at `z0`
/// (two thick), its top at the river's level, run `low` water minutes (the water it displaced
/// stands on it and the river overtops it), then raised 2 m and run `raised` minutes. Calls
/// `each` after every water minute with the wet cells over the raised top (0 before the raise),
/// the cells the water wrote that minute and the terrain chunks they fell in, and the water.
fn dam_seed0_river(
    z0: i32,
    low: usize,
    raised: usize,
    mut each: impl FnMut(usize, usize, usize, usize, &dwell_sim::water::Water),
) {
    use dwell_world::{FaceFrame, GENERATOR_TERRAIN, VoxelWorld};
    let mut w = VoxelWorld::new(
        dwell_worldgen::generator_for(GENERATOR_TERRAIN, 0),
        Some(dwell_worldgen::air_test_for(GENERATOR_TERRAIN, 0)),
    );
    let (x0, x1, z1) = (2349, 2380, z0 + 1);
    for x in (x0 - 100..=x1 + 100).step_by(8) {
        for z in (z0 - 120..=z1 + 80).step_by(8) {
            for y in (240..=320).step_by(8) {
                w.get_voxel(x, y, z);
            }
        }
    }
    let mut water = dwell_sim::water::Water::for_world(GENERATOR_TERRAIN, 0, true);
    water.tick(&mut w, &[], 0.1);
    let level = (x0..=x1)
        .filter_map(|x| water.level_at(FaceFrame::A, x, z0))
        .fold(f64::NAN, f64::max);
    assert!(level.is_finite(), "the river at z {z0}");
    let top = dwell_world::row_of_m(level);
    let stone = dwell_blocks::require("dwell:stone");
    let build = |w: &mut VoxelWorld, water: &mut dwell_sim::water::Water, top: i32| {
        let mut old = Vec::new();
        for x in x0..=x1 {
            for y in top - 30..=top {
                for z in z0..=z1 {
                    let was = w.get_voxel(x, y, z);
                    if was != stone {
                        old.push(([x, y, z], was));
                        w.set_voxel(x, y, z, stone);
                    }
                }
            }
        }
        water.touch(old);
    };
    let player = [2364.5, 142.0, f64::from(z0) - 8.0];
    build(&mut w, &mut water, top);
    for m in 0..low + raised {
        if m == low {
            build(&mut w, &mut water, top + 4);
        }
        let (mut writes, mut chunks) = (0, std::collections::HashSet::new());
        for _ in 0..60 {
            for (c, _) in water.tick(&mut w, &[player], 1.0) {
                writes += 1;
                chunks.insert(dwell_world::chunk_of(c[0], c[1], c[2]));
            }
        }
        let mut wet = 0;
        if m >= low {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    for y in top + 5..=top + 8 {
                        wet += usize::from(dwell_blocks::info(w.get_voxel(x, y, z)).liquid);
                    }
                }
            }
        }
        each(m, wet, writes, chunks.len(), &water);
    }
}

/// Owner, 2026-11-06 (BUGS.md #40): "once water has entered the column (covered the top of the
/// dam with water) that water will never drain away even if you raise the height of the dam". At
/// seed 0's river by z 7380, below a drawn step, a wall overtopped and raised 2 m kept water
/// standing on its top and pouring off it (red: 20 cells of water over the raised top).
#[test]
fn water_on_a_raised_dam_across_seed0s_river_drains() {
    // face-a-only: the generated world's face A; both faces in `water::tests`
    // (`water_on_a_raised_dam_drains_and_the_river_settles`).
    let mut last = usize::MAX;
    dam_seed0_river(7380, 20, 15, |_, wet, _, _, _| last = wet);
    assert_eq!(last, 0, "{last} cells of water stand on the raised dam");
}

/// The water's churn after a dam across seed 0's river is raised (BUGS.md #40): the cells it
/// writes a minute, the terrain chunks they fall in (each one's LOD section computed again) and
/// the water chunks awake, every 10 water minutes. `cargo test -p dwell-sim --test water
/// measure_a_raised_dams_churn -- --ignored --nocapture`.
#[test]
#[ignore]
fn measure_a_raised_dams_churn() {
    // face-a-only: a measurement on the generated world's face A
    for z0 in [7452, 7380] {
        dam_seed0_river(z0, 20, 150, |m, wet, writes, chunks, water| {
            if m % 10 == 9 {
                eprintln!(
                    "z {z0} minute {:>3}{}: {wet} wet over the top, {writes} cells written, {chunks} chunks, {} awake, {:.0} m³ free",
                    m + 1,
                    if m >= 20 { " (raised)" } else { "" },
                    water.awake_chunks(),
                    water.free_volume()
                );
            }
        });
    }
}

dwell_world::both_faces!(
    /// BUGS.md #42 (owner, 2026-11-06: "in this spot the river stops flowing too"): a client is
    /// sent the currents of every water chunk as far out as it draws the flow (`waterflow.json`
    /// `fadeToM`), so a river doesn't stop running where the currents stopped being sent. Red
    /// before: 2 chunks, 33 m out from a chunk's edge, inside the 40 m the flow shows fully.
    fn a_player_is_sent_the_currents_as_far_as_the_flow_is_drawn(f: dwell_world::FaceFrame) {
        use dwell_world::currents::{CS, current_key};
        let mut fx = Fixture::new(flat());
        join_streaming(&mut fx, 1);
        let feet = f.point([1.5, dwell_world::FLAT_TOP_M, 1.5]);
        let reach = dwell_look::waterflow::WATER_FLOW.fade_to_m;
        // Every water chunk with a column within the flow's reach of the player runs.
        let ((fb, pcx, pcz), _) = current_key(f, 1, 1);
        let mut want = std::collections::BTreeSet::new();
        for cx in pcx - 8..=pcx + 8 {
            for cz in pcz - 8..=pcz + 8 {
                let near = |p: f64, c: i32| {
                    let (lo, hi) = (f64::from(c * CS), f64::from((c + 1) * CS));
                    (lo - p).max(p - hi).max(0.0)
                };
                if near(1.5, cx).hypot(near(1.5, cz)) <= reach {
                    want.insert((fb, cx, cz));
                }
            }
        }
        let w = fx.server.world();
        for &k in &want {
            w.currents
                .set(k, Some(Box::new([[0.5, 0.0]; dwell_world::currents::CA])));
        }
        let mut got = std::collections::BTreeSet::new();
        for _ in 0..900 {
            fx.move_to(1, feet[0], feet[1], feet[2]);
            fx.server.step();
            for o in fx.server.take_outbox() {
                if o.session == 1
                    && matches!(o.kind, dwell_sim::server::OutKind::Datagram)
                    && let Some(Message::WaterCurrents(c)) = Message::decode(&o.bytes)
                    && c.cells.iter().any(|q| *q != [0, 0])
                {
                    got.insert((c.face_b, c.cx, c.cz));
                }
            }
        }
        let missed: Vec<_> = want.difference(&got).collect();
        assert!(
            missed.is_empty(),
            "{} of {} chunks within {reach} m never sent: {missed:?}",
            missed.len(),
            want.len()
        );
    }
);
