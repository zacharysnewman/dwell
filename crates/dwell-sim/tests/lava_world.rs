//! A world's own lava vents (docs/proposals/VOLCANOES.md §6.2): the starter world
//! (`test:starter`, `shared/worldgen/worlds/starter.json`) lists a lake vent on its youngest
//! hotspot island's summit (`tectonics.lavaVents`), placed by the server on each face when the
//! world is created, and lava stands round the vent after a while with a player beside it. V2
//! (§6.4): a river vent on the same island's flank on face B, whose way downhill reaches the sea
//! 1.3 km off, and (a long run, `--ignored`) how far its lava gets: the physics' outcome, no target. Its own test binary: the starter's maps are installed process-wide.

mod common;

use std::sync::OnceLock;

use common::*;
use dwell_blocks::require;
use dwell_lava::field::row_at;
use dwell_protocol::*;
use dwell_sim::server::ServerConfig;
use dwell_world::{Face, FaceFrame, GENERATOR_TERRAIN};
use dwell_worldgen::bifacial::{BifacialTerrain, face_seed};

const SEED: u64 = 0x1a7a_5eed;
const NAME: &str = "test:starter";

/// The starter's maps on both faces, installed once.
fn install() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        for face in [Face::A, Face::B] {
            let bytes =
                dwell_worldgen::config::worldgen_map_bytes(face_seed(SEED, face), NAME).unwrap();
            dwell_worldgen::worldmap::install(&bytes).unwrap();
        }
    });
}

/// The starter's vent site (face-local column) and the ground's height there on face `f`.
fn vent(f: FaceFrame) -> ([i32; 2], f64) {
    site(f, 0)
}

/// The starter's vent `n` (face-local column) and the ground's height there on face `f`.
fn site(f: FaceFrame, n: usize) -> ([i32; 2], f64) {
    let sites = dwell_worldgen::config::lava_vent_sites(NAME).unwrap();
    assert_eq!(sites.len(), 2, "the starter lists a lake and a river");
    let [x, z] = sites[n].1;
    let c = [x.floor() as i32, z.floor() as i32];
    let t = BifacialTerrain::for_generator(GENERATOR_TERRAIN, SEED).unwrap();
    let y = t
        .local(f.face)
        .surface_y_at(c[0], c[1])
        .expect("on the disc");
    (c, y)
}

#[test]
fn the_starter_world_lists_a_lake_and_a_river_vent_on_its_youngest_hotspot_and_the_default_none() {
    // face-a-only: the data, no terrain.
    use dwell_worldgen::tectonics::{Cone, LavaVentKind};
    let sites = dwell_worldgen::config::lava_vent_sites(NAME).unwrap();
    assert_eq!(sites.len(), 2);
    let (v, [x, z]) = &sites[0];
    assert_eq!((v.cone, v.kind), (Cone::Hotspot(0), LavaVentKind::Lake));
    assert_eq!(v.fluid, "dwell:lava");
    // The youngest hotspot island's centre (starter.json hotspotX, hotspotZ).
    assert_eq!([*x, *z], [30000.0, -8000.0]);
    // The river: 400 m out on its flank at 75°, on face B only.
    let (r, [rx, rz]) = &sites[1];
    assert_eq!((r.cone, r.kind), (Cone::Hotspot(0), LavaVentKind::River));
    let (dx, dz) = (rx - 30000.0, rz + 8000.0);
    assert!(((dx * dx + dz * dz).sqrt() - 400.0).abs() < 1e-6);
    assert!((dz.atan2(dx).to_degrees() - 75.0).abs() < 1e-6);
    assert_eq!((v.faces, r.faces), ([true, true], [false, true]));
    assert!(
        dwell_worldgen::config::lava_vent_sites("")
            .unwrap()
            .is_empty()
    );
    assert!(
        dwell_worldgen::config::lava_vent_sites("test:tall_peak")
            .unwrap()
            .is_empty()
    );
}

dwell_world::both_faces!(
    fn the_starter_world_gets_its_vent_and_lava_stands_on_the_summit(f: FaceFrame) {
        install();
        let (c, y) = vent(f);
        assert!(y > 50.0, "the island stands out of the sea: {y} m");
        let mut fx = Fixture::new(ServerConfig {
            generator_version: GENERATOR_TERRAIN,
            world_seed: SEED,
            // Beside the vent, a few metres off, on the summit.
            spawn: Some(f.point([f64::from(c[0]) + 6.5, y + 1.0, f64::from(c[1]) + 0.5])),
            mode: "creative".into(),
            ..flat()
        });
        assert_eq!(fx.server.worldgen_text(), NAME);
        let fields = &fx.server.lava().fields;
        assert_eq!(fields.len(), 3, "a lake on each face, the river on face B");
        let i = fields
            .iter()
            .position(|p| p.face == f && p.field.origin == c)
            .expect("this face's lake");
        assert_eq!(
            fields[i].field.origin, c,
            "the vent at its field's column (0, 0)"
        );
        fx.join(1, &Client::new(1));
        for _ in 0..20 * SIM_HZ {
            fx.server.step();
        }
        let p = &fx.server.lava().fields[i];
        let at = p.field.at(0, 0);
        assert!(p.field.depth[at] > 0.0, "lava over the vent");
        let bed = row_at(p.field.ground[at]);
        let [wx, wy, wz] = f.cell([c[0], bed, c[1]]);
        assert_eq!(
            f.state(fx.server.world().get_voxel(wx, wy, wz)),
            require("dwell:lava"),
            "and in the chunks"
        );
    }
);

/// The way lava poured at column `c` on face `f`'s terrain takes to the sea, on a 4 m grid: down
/// to the lowest of the 8 neighbours while one is lower, and out of each hollow it meets by filling
/// it until it spills lower (a priority flood from its bottom). Returns the straight distance to the
/// first column under the sea and the deepest hollow filled on the way (m); `None` if a hollow never
/// spills within ~6 km² (an enclosed basin) or no sea within 3,000 moves.
fn pour(f: FaceFrame, c: [i32; 2]) -> Option<(f64, f64)> {
    use std::cmp::Reverse;
    use std::collections::{BinaryHeap, HashSet};
    let t = BifacialTerrain::for_generator(GENERATOR_TERRAIN, SEED).unwrap();
    let y = |[x, z]: [i32; 2]| t.local(f.face).surface_y_at(x, z).unwrap_or(f64::MAX);
    let ring = |p: [i32; 2]| {
        [
            (4, 0),
            (-4, 0),
            (0, 4),
            (0, -4),
            (4, 4),
            (4, -4),
            (-4, 4),
            (-4, -4),
        ]
        .map(|(a, b)| [p[0] + a, p[1] + b])
    };
    let key = |h: f64| Reverse((h * 1000.0) as i64);
    let (mut at, mut deepest) = (c, 0.0f64);
    for _ in 0..3000 {
        if y(at) < dwell_world::SEA_LEVEL_M {
            let d = f64::from((at[0] - c[0]).pow(2) + (at[1] - c[1]).pow(2)).sqrt();
            return Some((d, deepest));
        }
        let low = ring(at)
            .into_iter()
            .min_by(|&p, &q| y(p).total_cmp(&y(q)))?;
        if y(low) < y(at) {
            at = low;
            continue;
        }
        let bottom = y(at);
        let mut seen = HashSet::from([at]);
        let mut heap: BinaryHeap<_> = ring(at)
            .into_iter()
            .inspect(|&p| {
                seen.insert(p);
            })
            .map(|p| (key(y(p)), p))
            .collect();
        let (mut level, mut out) = (bottom, None);
        while let Some((Reverse(h), p)) = heap.pop() {
            let h = h as f64 / 1000.0;
            if h < level - 1e-3 {
                out = Some(p);
                break;
            }
            level = level.max(h);
            if seen.len() > 400_000 {
                return None;
            }
            for n in ring(p) {
                if seen.insert(n) {
                    heap.push((key(y(n)), n));
                }
            }
        }
        deepest = deepest.max(level - bottom);
        at = out?;
    }
    None
}

#[test]
fn the_starter_river_vent_stands_on_face_bs_flank_over_a_kilometre_from_the_sea() {
    // face-b-only: the vent is face B's (`faces`); face A's island drains its flanks into an enclosed
    // basin above the sea (VOLCANOES.md §6.4).
    install();
    let f = FaceFrame::B;
    let (c, y) = site(f, 1);
    let (_, summit) = site(f, 0);
    let sea = dwell_world::SEA_LEVEL_M;
    assert!(
        y > sea + 50.0 && y < summit,
        "on the flank: {y} m (summit {summit} m)"
    );
    let (run, hollow) = pour(f, c).expect("a way to the sea");
    assert!((1000.0..2000.0).contains(&run), "{run} m to the sea");
    assert!(
        hollow <= 2.0,
        "no hollow on the way deeper than 2 m: {hollow} m"
    );
}

// The long run (`cargo test -p dwell-sim --test lava_world -- --ignored`, about four minutes;
// `DWELL_LAVA_TRACE=1` prints its progress): the starter's river vent, its field growing over the
// terrain (the bed the terrain generates, the sea over the ground below sea level), fast-forwarded
// far from any player for twenty world days (800 s each at lava speed 1), or until it builds land in
// the sea. Whether it gets there is the physics' (owner, 2026-11-11: "sometimes it may, other times it
// may not"; VOLCANOES.md §6.4): no target, it prints the outcome and checks only that the river ran
// well down the flank. With the build's lava it reaches ~600 m of its 1.3 km and crusts there, no
// land. `DWELL_LAVA_TUNE=rate=4,breakout=3,…` (the keys below) runs it with other data, for tuning.
// Too long for CI (~3 min).
#[test]
#[ignore = "a long fast-forward (minutes); run by hand"]
fn the_starter_river_runs_its_course() {
    // face-b-only: the river is face B's (`faces`); the macro can't carry `#[ignore]` anyway.
    install();
    {
        let f = FaceFrame::B;
        let sites = dwell_worldgen::config::lava_vent_sites(NAME).unwrap();
        let v = &sites[1].0;
        let (c, _) = site(f, 1);
        let t = BifacialTerrain::for_generator(GENERATOR_TERRAIN, SEED).unwrap();
        let sea = dwell_world::SEA_LEVEL_M as f32;
        let mut ground = |x: i32, z: i32, _: f32| {
            let y = t.local(f.face).surface_y_at(c[0] + x, c[1] + z)? as f32;
            Some(dwell_lava::Column {
                bed: y,
                water: (y < sea).then_some(sea),
            })
        };
        let mut fl = *dwell_lava::DATA.fluid(&v.fluid).unwrap();
        let mut rules = dwell_lava::DATA;
        let mut rate = v.rate_m3s;
        if let Ok(t) = std::env::var("DWELL_LAVA_TUNE") {
            for kv in t.split(',') {
                let (k, val) = kv.split_once('=').unwrap();
                let val: f32 = val.parse().unwrap();
                match k {
                    "visc" => fl.viscosity_m2s = val,
                    "yield" => fl.yield_depth_m = val,
                    "rest" => fl.rest_seconds = val,
                    "still" => fl.still_ms = val,
                    "keep" => fl.source_keep_m = val,
                    "breakout" => fl.breakout_depth_m = val,
                    "rate" => rate = val,
                    "step" => rules.step_depth_m = val,
                    "far" => rules.far_every_seconds = val,
                    _ => panic!("{k}"),
                }
            }
        }
        rules.fluids = std::slice::from_ref(Box::leak(Box::new(fl)));
        let mut field = dwell_lava::Field::growing(&rules, &v.fluid).unwrap();
        field.origin = c;
        let r = v.radius as i32;
        field.cover([-r, -r], [r, r], &mut ground, 0.0);
        field.vents.push(dwell_lava::Vent {
            kind: dwell_lava::VentKind::River,
            x: 0,
            z: 0,
            rate_m3s: rate,
            level_m: 0.0,
        });
        let day = 800.0;
        let mut t_s = 0.0;
        let land = |field: &dwell_lava::Field| {
            field
                .columns()
                .filter(|&i| {
                    let [x, z] = field.col(i);
                    !field.water[i]
                        && field.water_top[i] < dwell_lava::field::DRY
                        && field.ground[i] >= sea
                        && ground_was_sea(&t, f, [c[0] + x, c[1] + z], sea)
                })
                .count()
        };
        while t_s < 20.0 * day && land(&field) == 0 {
            for _ in 0..10 {
                field.advance_on(field.rules.far_every_seconds, false, Some(&mut ground));
                field.take_events();
            }
            t_s += 10.0 * field.rules.far_every_seconds;
            if std::env::var("DWELL_LAVA_TRACE").is_ok() && (t_s as i32) % 1600 == 0 {
                let reach = field
                    .columns()
                    .filter(|&i| field.depth[i] > 0.0)
                    .map(|i| field.col(i))
                    .map(|[x, z]| f64::from(x * x + z * z).sqrt())
                    .fold(0.0, f64::max);
                let deep = field.columns().map(|i| field.depth[i]).fold(0.0, f32::max);
                eprintln!(
                    "{f:?} day {:.0}: reach {reach:.0} m, {} tiles ({} molten), stored {:.0} m³, rock {:.0} m³, deepest {deep:.1} m, dt {:.3}",
                    t_s / day,
                    field.tiles().len(),
                    field.molten_tiles(),
                    field.stored(),
                    field.rocked,
                    field.max_step()
                );
            }
        }
        let n = land(&field);
        let reach = field
            .columns()
            .filter(|&i| field.depth[i] > 0.0 || field.crust[i] > 0.0)
            .map(|i| field.col(i))
            .map(|[x, z]| f64::from(x * x + z * z).sqrt())
            .fold(0.0, f64::max);
        eprintln!(
            "the starter river: reach {reach:.0} m, land {n} columns after {:.1} world days, {} tiles",
            t_s / day,
            field.tiles().len()
        );
        assert!(reach > 300.0, "it ran down the flank: {reach:.0} m");
    }
}

fn ground_was_sea(t: &BifacialTerrain, f: FaceFrame, [x, z]: [i32; 2], sea: f32) -> bool {
    t.local(f.face)
        .surface_y_at(x, z)
        .is_some_and(|y| (y as f32) < sea)
}

/// The dev report (`__dwell.lava()`) lists the starter's field on each face round its vent, and the
/// dev rate scale (`?lavaRate=`) fills it faster. Prints the site and the ground there for the
/// bench's spots.
#[test]
fn the_dev_report_lists_the_starter_fields_and_the_rate_scale_fills_faster() {
    // both-faces: the report lists the fields of both faces at once.
    install();
    let (c, y) = vent(FaceFrame::A);
    eprintln!("starter vent (seed {SEED:#x}): column {c:?}, face A ground {y} m");
    let run = |k: f32| {
        let mut fx = Fixture::new(ServerConfig {
            generator_version: GENERATOR_TERRAIN,
            world_seed: SEED,
            spawn: Some([f64::from(c[0]) + 6.5, y + 1.0, f64::from(c[1]) + 0.5]),
            mode: "creative".into(),
            ..flat()
        });
        assert_eq!(
            fx.server.scale_lava_rate(k),
            3,
            "a lake on each face, the river on face B"
        );
        fx.join(1, &Client::new(1));
        for _ in 0..10 * SIM_HZ {
            fx.server.step();
        }
        fx.server.lava_report()
    };
    let slow = run(1.0);
    eprintln!("{slow}");
    assert!(
        slow.contains(&format!(r#""centre":[{},{}]"#, c[0], c[1])),
        "{slow}"
    );
    assert!(slow.contains(r#""face":"a""#) && slow.contains(r#""face":"b""#));
    assert!(slow.contains(r#""worldgen":"test:starter""#), "{slow}");
    let molten = |r: &str| -> usize {
        let i = r.find(r#""face":"a""#).unwrap();
        let m = &r[i..];
        let j = m.find(r#""molten":"#).unwrap() + 9;
        m[j..].split(',').next().unwrap().parse().unwrap()
    };
    let fast = run(20.0);
    assert!(
        molten(&fast) > molten(&slow),
        "slow {}, fast {}",
        molten(&slow),
        molten(&fast)
    );
}
